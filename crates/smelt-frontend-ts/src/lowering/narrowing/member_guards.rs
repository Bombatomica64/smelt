//! Flow narrowing from the shape of a value: `Array.isArray`, the `in` operator and
//! discriminant-property comparisons, in `if` conditions and `switch` arms.

use crate::lowering::ModuleBuilder;
use crate::lowering::js_text::js_str_text;
use oxc::ast::ast::{Argument, Expression};
use oxc::syntax::operator::BinaryOperator;
use smelt_hir::{Body, ErasureReason, Type};

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

    /// Recognize `value.field === literal` discriminant guards.
    ///
    /// TypeScript discriminated unions are narrowed by comparing a shared
    /// discriminant property against a literal. Smelt erases string/number
    /// literal *types* to `String`/`Float`, so arms cannot be told apart by the
    /// discriminant field's value type. What Smelt can prove structurally is
    /// which arms even *carry* the accessed field: comparing `value.field` to a
    /// literal is only meaningful for arms that expose `field`, so the true
    /// branch narrows the union to those arms. This mirrors [`in_operator_guard`]
    /// but keys off a property comparison rather than an `in` test.
    ///
    /// The narrowing is intentionally conservative: it only fires when at least
    /// one union arm lacks the field (so the guard actually excludes a member).
    /// When every arm carries the field the comparison proves nothing about the
    /// union shape and no fact is recorded.
    pub(in crate::lowering) fn property_equality_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictEquality | BinaryOperator::Equality
        ) {
            return None;
        }
        let (name, field_name) = Self::member_literal_comparison(binary)?;
        let local = self.scope.lookup(name.as_str())?;
        let ty = self
            .narrowed_type(&name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        // Only narrow when the comparison distinguishes union arms: some arm must
        // carry the field and some other arm must not. `filtered_union_members`
        // returns `None` for non-unions, so single concrete types are untouched.
        let Type::Union(items) = self.ctx.krate.types.get(ty)?.clone() else {
            return None;
        };
        let all_have_field = items.iter().all(|item| {
            self.ctx
                .krate
                .types
                .get(*item)
                .is_some_and(|member| self.type_has_known_field(member, &field_name))
        });
        if all_have_field {
            return None;
        }
        let retained = self
            .filtered_union_members(ty, |member| self.type_has_known_field(member, &field_name))?;
        let narrowed = self.intern_filtered_union(retained)?;
        Some((name, narrowed))
    }

    /// Return the local name and property of a `value.field <op> literal` test.
    ///
    /// Recognizes both operand orders (`value.field === "x"` and
    /// `"x" === value.field`). Only static member access on a plain identifier
    /// compared against a literal counts; anything else yields `None`.
    fn member_literal_comparison(
        binary: &oxc::ast::ast::BinaryExpression<'_>,
    ) -> Option<(String, String)> {
        let member_side = Self::static_member_of_local(&binary.left)
            .filter(|_| Self::is_comparison_literal(&binary.right))
            .or_else(|| {
                Self::static_member_of_local(&binary.right)
                    .filter(|_| Self::is_comparison_literal(&binary.left))
            })?;
        Some(member_side)
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

    /// Compute the fact proven by a `switch (value.field)` discriminant.
    ///
    /// Inside every labeled case a `switch (value.field)` proves that `value`
    /// carries `field`, so the union narrows to arms exposing it. As with
    /// [`property_equality_guard`], the fact is only recorded when it actually
    /// excludes an arm (some union member lacks the field); otherwise the switch
    /// proves nothing about the union shape. Returns `None` for non-union locals
    /// and for discriminants that are not a plain `local.field` access.
    pub(in crate::lowering) fn switch_discriminant_narrowing(
        &mut self,
        discriminant: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let (name, field_name) = Self::static_member_of_local(discriminant)?;
        let local = self.scope.lookup(name.as_str())?;
        let ty = self
            .narrowed_type(&name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let Type::Union(items) = self.ctx.krate.types.get(ty)?.clone() else {
            return None;
        };
        let all_have_field = items.iter().all(|item| {
            self.ctx
                .krate
                .types
                .get(*item)
                .is_some_and(|member| self.type_has_known_field(member, &field_name))
        });
        if all_have_field {
            return None;
        }
        let retained = self
            .filtered_union_members(ty, |member| self.type_has_known_field(member, &field_name))?;
        let narrowed = self.intern_filtered_union(retained)?;
        Some((name, narrowed))
    }

    /// Return whether a concrete type has a statically modeled property.
    fn type_has_known_field(&self, ty: &Type, field: &str) -> bool {
        match ty {
            Type::String | Type::List(_) | Type::Tuple(_) if field == "length" => true,
            Type::Function(_) if matches!(field, "length" | "name" | "prototype") => true,
            Type::Dict(_, _) => true,
            Type::Class { name, .. } => self.ctx.krate.items.iter().any(|item| match item {
                smelt_hir::Item::Class(class) if class.name == *name => class
                    .fields
                    .iter()
                    .any(|candidate| self.ctx.krate.symbols.get(candidate.name) == Some(field)),
                smelt_hir::Item::Interface(interface) if interface.name == *name => interface
                    .fields
                    .iter()
                    .any(|candidate| self.ctx.krate.symbols.get(candidate.name) == Some(field)),
                _ => false,
            }),
            _ => false,
        }
    }
}
