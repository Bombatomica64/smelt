//! Flow narrowing from `typeof`: reading a `typeof x === "kind"` comparison and
//! computing the type each branch, or `switch (typeof x)` arm, leaves behind.

use crate::lowering::ModuleBuilder;
use crate::lowering::js_text::js_str_text;
use oxc::ast::ast::Expression;
use oxc::syntax::operator::{BinaryOperator, UnaryOperator};
use smelt_hir::{Body, ErasureReason, FunctionType, Type};

impl ModuleBuilder<'_> {
    /// Recognize `typeof value === "kind"` guards whose true branch exits.
    pub(in crate::lowering) fn typeof_inverse_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let (name, kind, matches_kind) = Self::typeof_comparison(expression)?;
        if matches_kind {
            self.typeof_excluded_type(&name, &kind, body)
        } else {
            self.typeof_matched_type(&name, &kind, body)
        }
    }

    /// Return a type with members matching a `typeof` kind removed.
    pub(in crate::lowering) fn remove_typeof_member(
        &mut self,
        ty: smelt_hir::TypeId,
        kind: &str,
    ) -> Option<smelt_hir::TypeId> {
        let resolved_ty = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(resolved_ty).cloned() {
            Some(Type::Union(items)) => {
                let remaining = items
                    .into_iter()
                    .filter(|item| !self.type_matches_typeof(*item, kind))
                    .collect::<Vec<_>>();
                match remaining.as_slice() {
                    [] => None,
                    [single] => Some(*single),
                    _ => Some(self.ctx.krate.types.intern(Type::Union(remaining))),
                }
            }
            Some(Type::Optional(item)) if kind == "undefined" => Some(item),
            Some(_) if self.type_matches_typeof(resolved_ty, kind) => None,
            Some(_) => Some(resolved_ty),
            None => None,
        }
    }

    /// Return whether a value of HIR type `ty` can answer the JavaScript
    /// `typeof` string `kind`.
    ///
    /// A union can when any member can. `None` and an optional can be `null`
    /// (`"object"`) or `undefined`. Every other type answers through the shared
    /// concrete-type map, [`Type::runtime_kind`], so a `Map` or a generator is
    /// an `"object"` here exactly as it is in `typeof_type_name` and in codegen.
    pub(in crate::lowering) fn type_matches_typeof(
        &self,
        ty: smelt_hir::TypeId,
        kind: &str,
    ) -> bool {
        let resolved_ty = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(resolved_ty) {
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.type_matches_typeof(item, kind)),
            Some(Type::None | Type::Optional(_)) => matches!(kind, "undefined" | "object"),
            Some(concrete) => concrete
                .runtime_kind()
                .is_some_and(|actual| actual.typeof_name() == kind),
            None => false,
        }
    }

    /// Recognize `typeof value === "kind"` guard expressions.
    pub(in crate::lowering) fn typeof_guard(
        &mut self,
        expression: &Expression<'_>,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let (name, kind, matches_kind) = Self::typeof_comparison(expression)?;
        if matches_kind {
            self.typeof_matched_type(&name, &kind, body)
        } else {
            self.typeof_excluded_type(&name, &kind, body)
        }
    }

    /// Return a normalized local `typeof` comparison.
    ///
    /// JavaScript code commonly writes both `typeof value === "kind"` and
    /// `"kind" !== typeof value`; the boolean indicates whether the expression
    /// proves that the local matches the `typeof` kind.
    pub(in crate::lowering) fn typeof_comparison(
        expression: &Expression<'_>,
    ) -> Option<(String, String, bool)> {
        let Expression::BinaryExpression(binary) = expression else {
            return None;
        };
        let matches_kind = match binary.operator {
            BinaryOperator::StrictEquality | BinaryOperator::Equality => true,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality => false,
            _ => return None,
        };
        let left = Self::typeof_identifier_name(&binary.left);
        let right = Self::typeof_identifier_name(&binary.right);
        match (left, &binary.right, right, &binary.left) {
            (Some(name), Expression::StringLiteral(kind), _, _)
            | (_, _, Some(name), Expression::StringLiteral(kind)) => {
                Some((name, js_str_text(kind.value).into_owned(), matches_kind))
            }
            _ => None,
        }
    }

    /// Return a string literal's value, if the expression is one.
    ///
    /// Used to read `switch (typeof x)` case labels (`case 'string':`) as raw
    /// `typeof` kind strings for per-arm narrowing.
    pub(in crate::lowering) fn string_literal_value(expression: &Expression<'_>) -> Option<String> {
        match expression {
            Expression::StringLiteral(literal) => Some(js_str_text(literal.value).into_owned()),
            _ => None,
        }
    }

    /// Return the identifier operand of a `typeof name` expression.
    pub(in crate::lowering) fn typeof_identifier_name(
        expression: &Expression<'_>,
    ) -> Option<String> {
        let Expression::UnaryExpression(unary) = expression else {
            return None;
        };
        if unary.operator != UnaryOperator::Typeof {
            return None;
        }
        let Expression::Identifier(identifier) = &unary.argument else {
            return None;
        };
        Some(identifier.name.to_string())
    }

    /// Return the local type proven by a positive `typeof` comparison.
    pub(in crate::lowering) fn typeof_matched_type(
        &mut self,
        name: &str,
        kind: &str,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let ty = match kind {
            "boolean" => self.ctx.krate.types.intern(Type::Bool),
            "number" => self.ctx.krate.types.intern(Type::Float),
            "bigint" => self.ctx.krate.types.intern(Type::BigInt),
            "string" => self.ctx.krate.types.intern(Type::String),
            "function" => {
                let local_ty = self.scope.lookup(name).map(|local| {
                    self.narrowed_type(name)
                        .unwrap_or_else(|| Self::local_ty(body, local))
                });
                local_ty
                    .and_then(|ty| self.function_member_type(ty))
                    .unwrap_or_else(|| {
                        let unknown_ty =
                            self.erase_with(ErasureReason::RuntimeNarrowing, "typeof function");
                        self.ctx.krate.types.intern(Type::Function(FunctionType {
                            params: vec![unknown_ty],
                            rest: None,
                            required_params: None,
                            mutable_params: Vec::new(),
                            return_ty: unknown_ty,
                            is_async: false,
                            may_throw: false,
                        }))
                    })
            }
            "undefined" => self.ctx.krate.types.intern(Type::None),
            // `typeof x === 'object'` on a UNION keeps the object-kinded arms
            // rather than erasing the value. Answering `Unknown` here threw
            // away everything the union knew — Hono's
            // `typeof arg === 'object' && arg.headers`, on
            // `StatusCode | ResponseInit | Response`, left `arg` erased inside
            // its own guard, so the member read became a runtime property
            // lookup and every consumer downstream saw `SmeltUnknown`. The
            // guard PROVES the value is one of the object arms, which is
            // strictly more than "some runtime value".
            //
            // A union with no object arm (or a non-union receiver) still
            // answers `Unknown`: there is nothing more precise to say, and a
            // tagged runtime value is the honest type for it.
            "object" => self
                .typeof_object_arms(name, body)
                .unwrap_or_else(|| {
                    self.erase_with(ErasureReason::RuntimeNarrowing, "typeof object")
                }),
            _ => return None,
        };
        Some((name.to_owned(), ty))
    }

    /// The object-kinded arms of a union local, as proven by
    /// `typeof local === 'object'`.
    ///
    /// `None` when the local is not a union, or when no arm is object-kinded —
    /// both cases where the caller has nothing better than the erased boundary
    /// to narrow to.
    ///
    /// An `Optional` wrapper is dropped rather than retained: the absent value
    /// of an `x?: T` parameter is `undefined`, whose `typeof` is `"undefined"`,
    /// so the guard excludes it. A `T | null` spelling is a union WITH a `None`
    /// arm instead, and that arm is retained — `typeof null === "object"` in
    /// JavaScript.
    fn typeof_object_arms(&mut self, name: &str, body: &Body) -> Option<smelt_hir::TypeId> {
        let local = self.scope.lookup(name)?;
        let local_ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let inner = match self.ctx.krate.types.get(local_ty).cloned() {
            Some(Type::Optional(inner)) => inner,
            _ => local_ty,
        };
        let Some(Type::Union(items)) = self.ctx.krate.types.get(inner).cloned() else {
            return None;
        };
        let retained = items
            .into_iter()
            .filter(|item| self.type_matches_typeof(*item, "object"))
            .collect::<Vec<_>>();
        match retained.as_slice() {
            [] => None,
            [single] => Some(*single),
            _ => Some(self.ctx.krate.types.intern(Type::Union(retained))),
        }
    }

    /// Return the local type proven by excluding one `typeof` kind.
    pub(in crate::lowering) fn typeof_excluded_type(
        &mut self,
        name: &str,
        kind: &str,
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let local = self.scope.lookup(name)?;
        let local_ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        let remaining = self.remove_typeof_member(local_ty, kind)?;
        Some((name.to_owned(), remaining))
    }

    /// Compute the fact proven inside a `switch (typeof x) { case 'k': … }` arm.
    ///
    /// A `typeof` switch discriminates the same union that a chain of
    /// `if (typeof x === 'k')` guards would: each arm proves `x` is the
    /// member(s) whose runtime `typeof` matches the arm's label(s). Grouped
    /// labels (`case 'a': case 'b':`) union their matching members. When `x`'s
    /// static type is a union (optionally wrapped in `Optional`), the members are
    /// filtered structurally via [`Self::type_matches_typeof`] so a
    /// `case 'object'` arm keeps the array/record/class members instead of
    /// widening to `unknown`; a non-union local falls back to the canonical
    /// single-kind type from [`Self::typeof_matched_type`] (a narrowing no-op
    /// when the local already has that type). `kinds` are the arm's string
    /// labels; an empty result (no member matches, or grouped labels we cannot
    /// read as kinds) records no fact.
    pub(in crate::lowering) fn switch_typeof_case_narrowing(
        &mut self,
        name: &str,
        kinds: &[String],
        body: &Body,
    ) -> Option<(String, smelt_hir::TypeId)> {
        let local = self.scope.lookup(name)?;
        let current_ty = self
            .narrowed_type(name)
            .unwrap_or_else(|| Self::local_ty(body, local));
        // Look through an `Optional` wrapper: a `typeof` case label never matches
        // the absent (`undefined`) member unless it is an explicit
        // `case 'undefined'`, handled by the single-kind fallback below.
        let inner_ty = match self.ctx.krate.types.get(current_ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => current_ty,
        };
        if let Some(Type::Union(items)) = self.ctx.krate.types.get(inner_ty).cloned() {
            let retained = items
                .into_iter()
                .filter(|item| {
                    // `typeof null === 'object'`, so a `None` member matches the
                    // `'object'` kind — but a value proven present enough to be
                    // indexed/field-accessed in this arm is never nullish (that is
                    // the null guards' domain). Drop `None` so a `case 'object'`
                    // narrows to the real object-shaped arm, not `string[] | null`.
                    //
                    // A dynamic member is any kind at run time, so it stays.
                    !matches!(self.ctx.krate.types.get(*item), Some(Type::None))
                        && (matches!(self.ctx.krate.types.get(*item), Some(Type::Unknown)) || kinds
                            .iter()
                            .any(|kind| self.type_matches_typeof(*item, kind)))
                })
                .collect::<Vec<_>>();
            if let Some(narrowed) = self.intern_filtered_union(retained) {
                return Some((name.to_owned(), narrowed));
            }
        }
        // Non-union local (or no arm matched the label): the canonical primitive
        // for a single kind still narrows a widened scrutinee usefully.
        if let [kind] = kinds {
            return self.typeof_matched_type(name, kind, body);
        }
        None
    }
}
