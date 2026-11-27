//! Flow narrowing from the shape of a value: `Array.isArray`, the `in` operator and
//! discriminant-property comparisons, in `if` conditions and `switch` arms.

use crate::lowering::ModuleBuilder;
use crate::lowering::expr::object_literals::literal_values_equal;
use crate::lowering::js_text::{js_str_literal, js_str_text};
use oxc::ast::ast::{Argument, Expression};
use oxc::syntax::operator::BinaryOperator;
use smelt_hir::{Body, ErasureReason, Literal, Type};

impl ModuleBuilder<'_> {
    /// Recognize `Array.isArray(value)` guard expressions.
    pub(in crate::lowering) fn array_is_array_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if !self.names_host(object.name.as_str(), "Array") || member.property.name != "isArray" {
            return None;
        }
        let [Argument::Identifier(identifier)] = call.arguments.as_slice() else {
            return None;
        };
        let name = identifier.name.as_str();
        let local_ty = self
            .scope
            .lookup(name)
            .and_then(|local| Self::local_ty_checked(body, local))
            .and_then(|ty| self.narrowed_type(name).or(Some(ty)));
        // A local already narrowed to one array or tuple type stays that type:
        // the guard proves nothing new about it, and widening it to an erased
        // list would throw away its element types (`typeof p === 'object'`
        // leaves only the `[keyof T, unknown]` arm, then `Array.isArray(p)`).
        if let Some(ty) = local_ty
            && matches!(
                self.ctx.krate.types.get(ty),
                Some(Type::List(_) | Type::Tuple(_))
            )
        {
            return Some((name.to_owned(), ty));
        }
        if let Some(ty) = local_ty
            && let Some(members) = self.filtered_union_members(ty, |union_member| {
                matches!(union_member, Type::List(_) | Type::Tuple(_))
            })
        {
            let narrowed = self.intern_filtered_union(members)?;
            return Some((name.to_owned(), narrowed));
        }
        let unknown = self.erase_with(ErasureReason::RuntimeNarrowing, name);
        Some((
            name.to_owned(),
            self.ctx.krate.types.intern(Type::List(unknown)),
        ))
    }

    /// Recognize `'field' in value` and retain only union arms exposing that field.
    pub(in crate::lowering) fn in_operator_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if binary.operator != BinaryOperator::In {
            return None;
        }
        let Expression::StringLiteral(field) = &binary.left else {
            return None;
        };
        let Expression::Identifier(identifier) = &binary.right else {
            return None;
        };
        let name = identifier.name.as_str();
        let local = self.scope.lookup(name)?;
        let ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let field_name = js_str_text(field.value);
        let retained = self
            .filtered_union_members(ty, |member| self.type_has_known_field(member, &field_name))?;
        let narrowed = self.intern_filtered_union(retained)?;
        Some((name.to_owned(), narrowed))
    }

    /// The fact a FAILED `'field' in value` proves: `value` is none of the
    /// union arms that always carry `field`.
    ///
    /// Only an interface arm whose field is required (its decode facts say it
    /// cannot be absent) is ruled out; an optional field may be missing, and a
    /// class arm or a dictionary may or may not hold the key, so they stay.
    pub(in crate::lowering) fn in_operator_inverse_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if binary.operator != BinaryOperator::In {
            return None;
        }
        let Expression::StringLiteral(field) = &binary.left else {
            return None;
        };
        let Expression::Identifier(identifier) = &binary.right else {
            return None;
        };
        let name = identifier.name.as_str();
        let local = self.scope.lookup(name)?;
        let ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let field_name = js_str_text(field.value);
        let Type::Union(items) = self.ctx.krate.types.get(ty)?.clone() else {
            return None;
        };
        let retained = items
            .iter()
            .copied()
            .filter(|item| !self.arm_always_has_field(*item, &field_name))
            .collect::<Vec<_>>();
        if retained.len() == items.len() {
            return None;
        }
        let narrowed = self.intern_filtered_union(retained)?;
        Some((name.to_owned(), narrowed))
    }

    /// Whether every value of interface arm `member` carries own property
    /// `field`: the interface declares it and its decode facts say it can
    /// never be absent.
    fn arm_always_has_field(&self, member: smelt_hir::TypeId, field: &str) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(member) else {
            return false;
        };
        let Some(interface) = self.find_interface(*name) else {
            return false;
        };
        interface.fields.iter().any(|candidate| {
            self.ctx.krate.symbols.source(candidate.name) == Some(field)
                && !candidate.optional
                && !matches!(
                    self.ctx.krate.types.get(candidate.ty),
                    Some(Type::Optional(..) | Type::Unknown)
                )
        }) && interface.decode_facts.iter().any(|facts| {
            self.ctx.krate.symbols.source(facts.name) == Some(field) && !facts.may_be_absent
        })
    }

    /// Recognize `value.field === literal` discriminant guards.
    ///
    /// TypeScript discriminated unions are narrowed by comparing a shared
    /// discriminant property against a literal. HIR stores a literal-typed
    /// field as its primitive, but each interface arm records the literals the
    /// field admits in its decode facts (`kind: "circle"` admits `"circle"`).
    /// The true branch keeps the arms that carry `field` and whose recorded
    /// literals admit the compared value; an arm whose field records no
    /// literal (a plain `string`) cannot be ruled out and stays. A `!==`/`!=`
    /// comparison proves what the failed `===` proves (see
    /// [`Self::discriminant_inverse_guard`]).
    ///
    /// Requirements: the narrowed type is exactly the set of arms the source
    /// value can still be at runtime under TypeScript's own narrowing (never
    /// fewer, so a read through the narrowed type never projects an absent
    /// arm), and a narrowed read is typed on the arm instead of erasing.
    ///
    /// The narrowing is intentionally conservative: it only fires when it
    /// excludes at least one union arm. When every arm survives the comparison
    /// proves nothing about the union shape and no fact is recorded.
    pub(in crate::lowering) fn property_equality_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        match binary.operator {
            BinaryOperator::StrictEquality | BinaryOperator::Equality => {
                let (name, field_name, literal) = Self::member_literal_comparison(binary)?;
                self.discriminant_narrowing(&name, &field_name, literal.as_slice(), true, body)
            }
            BinaryOperator::StrictInequality | BinaryOperator::Inequality => {
                let (name, field_name, literal) = Self::member_literal_comparison(binary)?;
                // `!==` against `null` is a presence test, not a discriminant.
                literal.as_ref()?;
                self.discriminant_narrowing(&name, &field_name, literal.as_slice(), false, body)
            }
            _ => None,
        }
    }

    /// The fact a FAILED `value.field === literal` proves (the `else` branch,
    /// the code after `if (s.kind === "circle") return ...`), and what a
    /// passing `value.field !== literal` proves.
    ///
    /// An arm is ruled out only when every literal its field admits equals the
    /// compared value: `kind: "circle"` cannot be at `s` once `s.kind !==
    /// "circle"`. An arm without the field, or whose field records no literal,
    /// stays.
    pub(in crate::lowering) fn discriminant_inverse_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        let matched = match binary.operator {
            BinaryOperator::StrictEquality | BinaryOperator::Equality => false,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality => true,
            _ => return None,
        };
        let (name, field_name, literal) = Self::member_literal_comparison(binary)?;
        literal.as_ref()?;
        self.discriminant_narrowing(&name, &field_name, literal.as_slice(), matched, body)
    }

    /// Narrow local `name` by a test of its `field` against `literals`.
    ///
    /// `matched` true: the field equals one of `literals` (an empty slice is a
    /// comparison whose value is not a discriminant literal, `=== null`, which
    /// only proves the field is present). `matched` false: the field equals
    /// none of them. `None` when `name` is no union local or no arm is ruled out.
    pub(in crate::lowering) fn discriminant_narrowing(
        &mut self,
        name: &str,
        field_name: &str,
        literals: &[Literal],
        matched: bool,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let local = self.scope.lookup(name)?;
        let ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        // `filtered_union_members` returns `None` for non-unions, so single
        // concrete types are untouched.
        let Type::Union(items) = self.ctx.krate.types.get(ty)?.clone() else {
            return None;
        };
        let keeps = |this: &Self, member: smelt_hir::TypeId| {
            let Some(member_ty) = this.ctx.krate.types.get(member) else {
                return true;
            };
            let admitted = this.arm_field_literals(member_ty, field_name);
            if matched {
                this.type_has_known_field(member_ty, field_name)
                    && (literals.is_empty()
                        || admitted.is_none_or(|admitted| {
                            admitted.iter().any(|value| {
                                literals
                                    .iter()
                                    .any(|literal| literal_values_equal(value, literal))
                            })
                        }))
            } else {
                !admitted.is_some_and(|admitted| {
                    admitted.iter().all(|value| {
                        literals
                            .iter()
                            .any(|literal| literal_values_equal(value, literal))
                    })
                })
            }
        };
        if items.iter().all(|item| keeps(self, *item)) {
            return None;
        }
        let retained = items
            .iter()
            .copied()
            .filter(|item| keeps(self, *item))
            .collect::<Vec<_>>();
        let narrowed = self.intern_filtered_union(retained)?;
        Some((name.to_owned(), narrowed))
    }

    /// The literals union arm `member` records for its `field` (`kind:
    /// "circle"` gives `["circle"]`), or `None` when nothing pins the field's
    /// value: no interface arm, no such field, or a field of a non-literal type.
    fn arm_field_literals(&self, member: &Type, field: &str) -> Option<&[Literal]> {
        let Type::Class { name, .. } = member else {
            return None;
        };
        let interface = self.find_interface(*name)?;
        interface
            .decode_facts
            .iter()
            .find(|facts| {
                !facts.literals.is_empty()
                    && self.ctx.krate.symbols.source(facts.name) == Some(field)
            })
            .map(|facts| facts.literals.as_slice())
    }

    /// Return the local, the property and the compared literal of a
    /// `value.field <op> literal` test.
    ///
    /// Recognizes both operand orders (`value.field === "x"` and
    /// `"x" === value.field`). Only static member access on a plain identifier
    /// compared against a literal counts; anything else yields `None`. The
    /// literal is `None` for `null`, which is no discriminant value.
    fn member_literal_comparison(
        binary: &oxc::ast::ast::BinaryExpression<'_>,
    ) -> Option<(String, String, Option<Literal>)> {
        if let Some((name, field)) = Self::static_member_of_local(&binary.left)
            && Self::is_comparison_literal(&binary.right)
        {
            return Some((name, field, Self::comparison_literal(&binary.right)));
        }
        let (name, field) = Self::static_member_of_local(&binary.right)?;
        Self::is_comparison_literal(&binary.left)
            .then(|| (name, field, Self::comparison_literal(&binary.left)))
    }

    /// The HIR literal of a discriminant comparison operand; `None` for `null`.
    fn comparison_literal(expression: &Expression<'_>) -> Option<Literal> {
        match expression {
            Expression::StringLiteral(literal) => Some(js_str_literal(literal.value)),
            Expression::NumericLiteral(literal) => Some(Literal::Float(literal.value)),
            Expression::BooleanLiteral(literal) => Some(Literal::Bool(literal.value)),
            _ => None,
        }
    }

    /// Return `(local, property)` when an expression is `local.property`.
    fn static_member_of_local(expression: &Expression<'_>) -> Option<(String, String)> {
        let Expression::StaticMemberExpression(member) = expression else {
            return None;
        };
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        Some((object.name.to_string(), member.property.name.to_string()))
    }

    /// Return whether an expression is a literal usable as a discriminant value.
    fn is_comparison_literal(expression: &Expression<'_>) -> bool {
        matches!(
            expression,
            Expression::StringLiteral(_)
                | Expression::NumericLiteral(_)
                | Expression::BooleanLiteral(_)
                | Expression::NullLiteral(_)
        )
    }

    /// Compute the fact one arm of a `switch (value.field)` proves.
    ///
    /// `labels` are the arm's case labels (its own and those of the empty
    /// cases grouped onto it): inside the arm `value.field` equals one of
    /// them, so the union keeps the arms carrying `field` whose recorded
    /// literals admit a label, as [`Self::property_equality_guard`] does for
    /// one comparison. `default` arms pass `matched = false` with every label
    /// of the switch: the field equals none of them. Returns `None` for
    /// non-union locals, discriminants that are not a plain `local.field`
    /// access, and facts that exclude no arm.
    pub(in crate::lowering) fn switch_discriminant_narrowing(
        &mut self,
        discriminant: &Expression<'_>,
        labels: &[Literal],
        matched: bool,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let (name, field_name) = Self::static_member_of_local(discriminant)?;
        if !matched && labels.is_empty() {
            return None;
        }
        self.discriminant_narrowing(&name, &field_name, labels, matched, body)
    }

    /// Return whether a concrete type has a statically modeled property.
    ///
    /// `field` is the SOURCE spelling (`type`), compared with each field's
    /// source name, not its Rust rendering (`type_`).
    fn type_has_known_field(&self, ty: &Type, field: &str) -> bool {
        match ty {
            Type::String | Type::List(_) | Type::Tuple(_) if field == "length" => true,
            Type::Function(_) if matches!(field, "length" | "name" | "prototype") => true,
            Type::Dict(_, _) => true,
            Type::Class { name, .. } => self.ctx.krate.items.iter().any(|item| match item {
                smelt_hir::Item::Class(class) if class.name == *name => class
                    .fields
                    .iter()
                    .any(|candidate| self.ctx.krate.symbols.source(candidate.name) == Some(field)),
                smelt_hir::Item::Interface(interface) if interface.name == *name => interface
                    .fields
                    .iter()
                    .any(|candidate| self.ctx.krate.symbols.source(candidate.name) == Some(field)),
                _ => false,
            }),
            _ => false,
        }
    }
}
