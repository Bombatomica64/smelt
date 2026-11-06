//! Logical and nullish operators in value position: `&&`, `||`, `??` and the types
//! their fallbacks join to.

use crate::lowering::peel::{Wrappers, peel};
use crate::lowering::{
    BinOp, Body, Expr, ExprKind, Expression, Literal, LocalDecl, LogicalOperator, ModuleBuilder,
    Pattern, PrimitiveCastOp, SmeltError, Stmt, Type,
};
use oxc::span::GetSpan;
use smelt_hir::ErasureReason;

/// Outcome of the `match || [..]` rule
/// (`ModuleBuilder::match_or_array_fallback_expression`).
pub(in crate::lowering) enum MatchArrayFallback {
    /// The left operand is not a regex match; the fallback was not lowered.
    NotMatch,
    /// The left operand is a match but the lowered fallback is not a list.
    NotArray(smelt_hir::ExprId),
    /// The whole `||` expression, typed as the match's array view.
    Selected(smelt_hir::ExprId),
}

impl ModuleBuilder<'_> {
    /// Lower a logical expression.
    pub(in crate::lowering) fn logical_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // The `(typeof X === 'object' && X) || ...` global-detection chain folds
        // to the global-object value before ordinary `||` lowering, so the dead
        // absent-alias clauses (e.g. `&& window`) are never lowered.
        if let Some(expr) = self.global_detection_chain_expression(logical, body) {
            return Ok(expr);
        }
        // A guard the profile already decides short-circuits the whole
        // expression BEFORE any value-shape helper runs, because several of them
        // lower the right operand first (`logical_and_numeric_value_expression`
        // does) and the dead operand is exactly the one that cannot be lowered.
        if let Some(expr) = self.short_circuited_static_guard(logical, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.logical_or_fallback_expression(logical, body)? {
            return Ok(expr);
        }
        if logical.operator == LogicalOperator::Coalesce {
            return self.nullish_coalesce_expression(logical, body, None);
        }
        if let Some(expr) = self.logical_and_numeric_value_expression(logical, body)? {
            return Ok(expr);
        }
        let op = if logical.operator == LogicalOperator::And {
            BinOp::And
        } else {
            BinOp::Or
        };
        let lhs = self.expression(&logical.left, body)?;
        if let Some(expr) = Self::short_circuited_logical_operand(logical, lhs, body) {
            return Ok(expr);
        }
        let rhs = self.expression(&logical.right, body)?;
        let span = self.span(logical.span.start, logical.span.end);
        if let Some(expr) = self.logical_operand_value_expression(logical, body, lhs, rhs)? {
            return Ok(expr);
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty,
            span,
        }))
    }

    /// Fold `<statically decided guard> && <dead operand>` (and the `||` mirror)
    /// to the guard's constant, without lowering the dead operand at all.
    ///
    /// JavaScript never evaluates the right operand of a `&&` whose left operand
    /// is falsy, nor of a `||` whose left operand is truthy. Smelt folds
    /// existence guards against the target profile to a constant, so the dead
    /// operand is routinely one that CANNOT be lowered — es-toolkit's `isBrowser`
    /// reads `window?.document` behind `typeof window !== 'undefined'`, and the
    /// non-DOM profile provides no `window` binding on purpose. Visiting it
    /// anyway turned a correctly-folded `false` into an `unresolved identifier`
    /// blocker that aborted the whole crate build.
    ///
    /// [`Self::static_guard_value`] answers from the AST alone, so this runs
    /// before the value-shape helpers — several of which lower the right operand
    /// first — and costs one pattern match when it does not apply.
    pub(in crate::lowering) fn short_circuited_static_guard(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let value = self.static_guard_value(&logical.left)?;
        let decides = match logical.operator {
            LogicalOperator::And => !value,
            LogicalOperator::Or => value,
            LogicalOperator::Coalesce => false,
        };
        if !decides {
            return None;
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(value)),
            ty,
            span: self.span(logical.span.start, logical.span.end),
        }))
    }

    /// Return the whole `&&`/`||` result when the LEFT operand already decided it.
    ///
    /// JavaScript's logical operators short-circuit: the right operand of a
    /// `&&` whose left operand is falsy is never evaluated, and neither is the
    /// right operand of a `||` whose left operand is truthy. Smelt folds plenty
    /// of guards to a compile-time literal — a `typeof` probe against the target
    /// profile's absent globals is the common one — and until now it still went
    /// on to LOWER the dead operand, which then had to resolve names that only
    /// exist in the branch JavaScript never takes. es-toolkit's `isBrowser`
    /// (`typeof window !== 'undefined' && window?.document != null`) is the
    /// instance: `typeof window` folds to `false` for the non-DOM profile, and
    /// lowering `window?.document` anyway demands a `window` binding the profile
    /// deliberately does not provide.
    ///
    /// Deliberately narrow: only a `bool` LITERAL left operand short-circuits,
    /// so this folds exactly the guards lowering already reduced to a constant
    /// and never speculates about a runtime value's truthiness. The folded
    /// result is the left operand itself, which is what JavaScript yields.
    fn short_circuited_logical_operand(
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        lhs: smelt_hir::ExprId,
        body: &Body,
    ) -> Option<smelt_hir::ExprId> {
        let index = usize::try_from(lhs.0).ok()?;
        let Some(ExprKind::Literal(Literal::Bool(value))) =
            body.exprs.get(index).map(|expr| &expr.kind)
        else {
            return None;
        };
        let decides = match logical.operator {
            LogicalOperator::And => !value,
            LogicalOperator::Or => *value,
            LogicalOperator::Coalesce => false,
        };
        decides.then_some(lhs)
    }

    /// Lower `a && b` / `a || b` in a VALUE position to the operand JavaScript
    /// actually yields.
    ///
    /// JavaScript's logical operators are selectors, not boolean operators:
    /// `a && b` evaluates to `a` when `a` is falsy and to `b` otherwise, and
    /// `a || b` evaluates to `a` when `a` is truthy and to `b` otherwise. The
    /// static type of the whole expression is therefore the union of the two
    /// operand types, not `boolean`.
    ///
    /// Modelling it as a boolean throws the operand away. es-toolkit's
    /// `expect(error instanceof Error && error.message).toBe('test')` lowered to
    /// a `bool`, so the comparison against a string was statically false and the
    /// assertion folded to `!(false)` — a test that could never fail and never
    /// checked anything.
    ///
    /// Returns `None` (so the caller keeps the boolean `BinOp`) when:
    ///
    /// * both operands are already `bool`, which is the overwhelmingly common
    ///   case and where the boolean lowering is exactly right — widening it
    ///   would push ordinary guards through a union for nothing; or
    /// * the two operand types have no common lowered shape, i.e.
    ///   [`Self::conditional_branch_type`] (the same unification a ternary uses,
    ///   so `a && b` and `a ? b : a` agree) cannot merge them. Degrading to the
    ///   previous boolean shape keeps lowering succeeding where it succeeds
    ///   today rather than erasing the value to `unknown` to force a merge.
    ///
    /// The left operand is bound to a temporary first: it is read twice (once
    /// for the truthiness test, once as the selected value) and re-emitting the
    /// expression would evaluate it twice.
    pub(in crate::lowering) fn logical_operand_value_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let lhs_ty = Self::expr_ty(body, lhs);
        let rhs_ty = Self::expr_ty(body, rhs);
        if lhs_ty == bool_ty && rhs_ty == bool_ty {
            return Ok(None);
        }
        let ty = match self.conditional_branch_type(
            rhs_ty,
            lhs_ty,
            None,
            logical.span.start,
            logical.span.end,
        ) {
            Ok(ty) => ty,
            // No closer common shape exists (a `string` operand beside a
            // `number` one, say). TypeScript's answer for `a && b` is the
            // literal union of the operand types, and a generated union is a
            // concrete Rust enum — not erasure — so build it rather than
            // falling back to a boolean that discards the value.
            Err(_) => self
                .ctx
                .krate
                .types
                .intern(Type::Union(vec![lhs_ty, rhs_ty])),
        };
        // `conditional_branch_type` is allowed to answer `unknown` for a merge
        // it cannot name, which is right when an operand is ALREADY erased (the
        // `error instanceof Error && error.message` case, where `error` is
        // source `unknown`) and wrong when both operands are concrete: erasing
        // two known shapes to reconcile them is the avoidable erasure the ABI
        // rules forbid. A generated union names that merge exactly.
        let ty = if self.ctx.krate.types.get(ty) == Some(&Type::Unknown)
            && !self.type_contains_unknown(lhs_ty)
            && !self.type_contains_unknown(rhs_ty)
        {
            self.ctx
                .krate
                .types
                .intern(Type::Union(vec![lhs_ty, rhs_ty]))
        } else {
            ty
        };
        let span = self.span(logical.span.start, logical.span.end);
        // Bind the left operand so the truthiness test and the selected value
        // read one evaluation.
        let selector = body.push_local(LocalDecl {
            name: Some(self.intern_source_name("smelt_logical")),
            ty: lhs_ty,
            mutable: false,
            span,
        });
        let selector_pat = body.push_pattern(Pattern::Binding(selector));
        let let_stmt = Stmt::Let {
            pat: selector_pat,
            ty: lhs_ty,
            value: Some(lhs),
        };
        self.push_stmt_to_sink(body, let_stmt);
        let selector_read = body.push_expr(Expr {
            kind: ExprKind::Local(selector),
            ty: lhs_ty,
            span,
        });
        let Ok(cond) = self.lowered_condition_expression(selector_read, span, body) else {
            return Ok(None);
        };
        let selector_value = body.push_expr(Expr {
            kind: ExprKind::Local(selector),
            ty: lhs_ty,
            span,
        });
        let (then_expr, else_expr) = if logical.operator == LogicalOperator::And {
            (rhs, selector_value)
        } else {
            (selector_value, rhs)
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            },
            ty,
            span,
        })))
    }

    /// Lower `a && b` / `a || b` that is consumed only for its truthiness.
    ///
    /// A condition observes nothing but the truthiness of the operand
    /// [`Self::logical_operand_value_expression`] selects, and
    /// `truthy(a && b) == truthy(a) && truthy(b)` (likewise for `||`). Lowering
    /// the condition form straight to a boolean `BinOp` therefore keeps
    /// `if (a && b)` as a plain Rust `&&` instead of materializing a union value
    /// and then testing it — the value-yielding rule is only needed where the
    /// value escapes.
    ///
    /// Returns `None` when this is not a plain `&&`/`||`, or when either operand
    /// has no truthiness lowering, so the caller falls back to lowering the
    /// logical expression as a value and testing that.
    pub(in crate::lowering) fn logical_condition_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if logical.operator == LogicalOperator::Coalesce {
            return Ok(None);
        }
        // `(typeof X === 'object' && X) || ...` folds to the detected global
        // before either operand is lowered; its absent-alias operands (`&&
        // window` in a non-browser build) cannot be lowered at all, so the fold
        // has to run first here too.
        if let Some(expr) = self.global_detection_chain_expression(logical, body) {
            return self
                .lowered_condition_expression(
                    expr,
                    self.span(logical.span.start, logical.span.end),
                    body,
                )
                .map(Some);
        }
        if let Some(expr) = self.short_circuited_static_guard(logical, body) {
            return Ok(Some(expr));
        }
        let Ok(cond) = self.condition_expression(&logical.left, body) else {
            return Ok(None);
        };
        if let Some(expr) = Self::short_circuited_logical_operand(logical, cond, body) {
            return Ok(Some(expr));
        }
        // `x instanceof T && x.field` narrows `x` for the right operand exactly
        // as it does in the value form.
        let rhs_narrowing = if logical.operator == LogicalOperator::And {
            self.guard_narrowing(&logical.left, body)
        } else {
            None
        };
        if let Some(narrowing) = rhs_narrowing.clone() {
            self.scope.push_narrowing_scope(narrowing);
        }
        let rhs = self.expression(&logical.right, body);
        if rhs_narrowing.is_some() {
            self.scope.pop_narrowing_scope();
        }
        let Ok(rhs) = rhs else {
            return Ok(None);
        };
        // A `Conditional`, not a boolean `BinOp`: MIR lowers the arms into
        // branches, so the right operand's own statements only run when the
        // left operand did not already decide the answer. A `BinOp` would hoist
        // them and evaluate both sides unconditionally, losing short-circuiting.
        let ty = self.ctx.krate.types.intern(Type::Bool);
        let identity = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(logical.operator == LogicalOperator::Or)),
            ty,
            span: self.expression_span(&logical.left),
        });
        let (then_expr, else_expr) = if logical.operator == LogicalOperator::And {
            (rhs, identity)
        } else {
            (identity, rhs)
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            },
            ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left && numeric` expressions in numeric value contexts.
    ///
    /// JavaScript returns either the falsy left value or the right value. When
    /// the right side is numeric, generated Rust needs a numeric result instead
    /// of the boolean shape used for conditions, so falsy left values are
    /// represented by numeric zero.
    pub(in crate::lowering) fn logical_and_numeric_value_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if logical.operator != LogicalOperator::And {
            return Ok(None);
        }
        let rhs = self.expression(&logical.right, body)?;
        let rhs_ty = Self::expr_ty(body, rhs);
        if !self.is_numeric_like_type(rhs_ty) {
            return Ok(None);
        }
        let cond = self.condition_expression(&logical.left, body)?;
        let zero = body.push_expr(Expr {
            kind: match self.ctx.krate.types.get(rhs_ty) {
                Some(Type::Int) => ExprKind::Literal(Literal::Int(0)),
                _ => ExprKind::Literal(Literal::Float(0.0)),
            },
            ty: rhs_ty,
            span: self.span(logical.left.span().start, logical.left.span().end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: rhs,
                else_expr: zero,
            },
            ty: rhs_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || right` fallback expressions for optional values.
    ///
    /// Date-fns uses this for locale-width defaults. For optional left operands
    /// Smelt preserves the runtime value fallback with the same optional
    /// coalescing HIR shape used by `??`.
    pub(in crate::lowering) fn logical_or_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if logical.operator != LogicalOperator::Or {
            return Ok(None);
        }
        if let Expression::LogicalExpression(left_logical) =
            peel(&logical.left, Wrappers::Parens)
            && let Some(value) =
                self.logical_and_value_fallback_expression(logical, left_logical, body)?
        {
            return Ok(Some(value));
        }
        if let Expression::LogicalExpression(left_logical) =
            peel(&logical.left, Wrappers::Parens)
            && let Some(value) = self.logical_and_numeric_value_expression(left_logical, body)?
        {
            let value_ty = Self::expr_ty(body, value);
            if let Some(expr) =
                self.logical_or_numeric_fallback_expression(logical, body, value, value_ty)?
            {
                return Ok(Some(expr));
            }
        }
        let optional = self.expression(&logical.left, body)?;
        let optional_ty = Self::expr_ty(body, optional);
        if self.ctx.krate.types.get(optional_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(optional_ty)
        {
            let optional_receiver = self.optionalize_index_receiver(optional, body);
            let optional_receiver_ty = Self::expr_ty(body, optional_receiver);
            if self.is_nullishable_type(optional_receiver_ty) {
                let fallback =
                    self.expression_with_hint(&logical.right, body, Some(optional_receiver_ty))?;
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::OptionalCoalesce {
                        optional: optional_receiver,
                        fallback,
                    },
                    // not a new erasure: the operand already contains unknown
                    ty: self.unknown_type(),
                    span: self.span(logical.span.start, logical.span.end),
                })));
            }
        }
        if !self.is_nullishable_type(optional_ty) {
            if let Some(expr) =
                self.logical_or_unknown_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_numeric_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_object_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_string_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            if let Some(expr) =
                self.logical_or_list_fallback_expression(logical, body, optional, optional_ty)?
            {
                return Ok(Some(expr));
            }
            return Ok(None);
        }
        let Some(ty) = self.non_nullish_type(optional_ty) else {
            if matches!(logical.left, Expression::ChainExpression(_)) {
                return self.expression(&logical.right, body).map(Some);
            }
            return Ok(None);
        };
        if matches!(self.ctx.krate.types.get(ty), Some(Type::Function(_))) {
            let fallback = self.expression(&logical.right, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::OptionalCoalesce { optional, fallback },
                ty: self.erase_with(ErasureReason::DynamicOperator, "|| over function"),
                span: self.span(logical.span.start, logical.span.end),
            })));
        }
        let fallback = match self.match_or_array_fallback_expression(logical, body, optional, ty)? {
            MatchArrayFallback::Selected(expr) => return Ok(Some(expr)),
            MatchArrayFallback::NotArray(fallback) => fallback,
            MatchArrayFallback::NotMatch => {
                self.expression_with_hint(&logical.right, body, Some(ty))?
            }
        };
        let fallback_ty = Self::expr_ty(body, fallback);
        // `||` is a TRUTHINESS test, and `OptionalCoalesce` below is a
        // PRESENCE test: the two agree only when every present value is truthy
        // (objects). A present `''`, `0`, `NaN` or `false` is falsy, so for a
        // primitive left operand (`pattern[2] || LABEL`, `opts.port || 80`)
        // the fallback is taken for those too. `optional` is one HIR value,
        // evaluated once and read by both the test and the selected arm.
        if matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::String | Type::Float | Type::Int | Type::Bool)
        ) && (fallback_ty == ty
            || self.is_string_compatible_type(ty) && self.is_string_compatible_type(fallback_ty))
        {
            let span = self.span(logical.span.start, logical.span.end);
            let cond = self.lowered_condition_expression(optional, span, body)?;
            let present = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: optional },
                ty,
                span,
            });
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::Conditional {
                    cond,
                    then_expr: present,
                    else_expr: fallback,
                },
                ty,
                span,
            })));
        }
        let ty = if fallback_ty == ty {
            ty
        } else if matches!(
            self.ctx.krate.types.get(fallback_ty),
            Some(Type::Optional(inner)) if *inner == ty
        ) && !self.type_contains_unknown(ty)
            && self.type_is_always_truthy_object_surface(ty)
        {
            // `record[a] || record[b]` over object values: every present object
            // is truthy, so `||` is presence-coalescing and the result is
            // absent exactly when both operands are. The precise type is the
            // fallback's own `Optional<X>`, not an erased value.
            fallback_ty
        } else if self.ctx.krate.types.get(ty) == Some(&Type::Unknown)
            || self.ctx.krate.types.get(fallback_ty) == Some(&Type::Unknown)
            || self.type_contains_unknown(ty)
            || self.type_contains_unknown(fallback_ty)
        {
            // not a new erasure: an operand already contains unknown
            self.unknown_type()
        } else if self.is_structural_object_surface(ty) {
            // Object values are always truthy in JavaScript; keep the selected
            // runtime value when their fallback widens the expression surface.
            self.erase_with(ErasureReason::DynamicOperator, "|| over object")
        } else if matches!(self.ctx.krate.types.get(ty), Some(Type::Union(_))) {
            // A union left operand (`Pattern | null`, `Pattern` itself a union
            // of a tuple and a string literal) keeps EVERY arm it can yield:
            // the result is the left's present arms plus the fallback's. It
            // was typed `string` because one arm is string-compatible, which
            // stringified the tuple arm (Hono's trie-router `insert`).
            let mut arms = self.flatten_union_member_types(ty);
            for arm in self.flatten_union_member_types(fallback_ty) {
                if !arms.contains(&arm) {
                    arms.push(arm);
                }
            }
            self.union_of_types_or_unknown(arms)
        } else if self.is_string_compatible_type(ty) && self.is_string_compatible_type(fallback_ty)
        {
            self.ctx.krate.types.intern(Type::String)
        } else {
            return Ok(None);
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce { optional, fallback },
            ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower `match || [..]` / `match ?? [..]`: an optional regex match whose
    /// fallback is an array.
    ///
    /// `(s.match(re) || []).map(..)` is the idiomatic "matches or nothing"
    /// spelling. Both operands are arrays, so the result is the match's array
    /// view (see [`Self::match_array_view`]) when present and the fallback
    /// list otherwise — typed `(string | undefined)[]` rather than joined into
    /// an erased value. A present match is an object and so always truthy,
    /// which is why `||` agrees with `??` here: both are presence-coalescing
    /// (`OptionalCoalesce`).
    ///
    /// `present_ty` is the left operand's non-nullish type. Answers
    /// [`MatchArrayFallback::NotMatch`] (fallback not lowered) when it is not
    /// the match class, and [`MatchArrayFallback::NotArray`] with the lowered
    /// fallback when that is not a list, so the caller's general join goes on
    /// without lowering the right operand twice.
    fn match_or_array_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        optional: smelt_hir::ExprId,
        present_ty: smelt_hir::TypeId,
    ) -> Result<MatchArrayFallback, SmeltError> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(present_ty) else {
            return Ok(MatchArrayFallback::NotMatch);
        };
        if self.match_stdlib_class(*name) != Some(smelt_stdlib::StdlibClass::Match) {
            return Ok(MatchArrayFallback::NotMatch);
        }
        let view_ty = self.match_array_view_type();
        let fallback = self.expression_with_hint(&logical.right, body, Some(view_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        if !matches!(self.ctx.krate.types.get(fallback_ty), Some(Type::List(_))) {
            return Ok(MatchArrayFallback::NotArray(fallback));
        }
        let span = self.span(logical.span.start, logical.span.end);
        let fallback = if fallback_ty == view_ty {
            fallback
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty: view_ty,
                span: self.span(logical.right.span().start, logical.right.span().end),
            })
        };
        let optional_view_ty = self.ctx.krate.types.intern(Type::Optional(view_ty));
        let optional = body.push_expr(Expr {
            kind: ExprKind::TypeAssert { value: optional },
            ty: optional_view_ty,
            span: self.span(logical.left.span().start, logical.left.span().end),
        });
        Ok(MatchArrayFallback::Selected(body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce { optional, fallback },
            ty: view_ty,
            span,
        })))
    }

    /// Lower JavaScript `left || right` for object-like values without string coercion.
    ///
    /// Some type aliases that include `null` are represented as object surfaces
    /// after TypeScript lowering. JavaScript still returns the selected operand
    /// for `||`, so object-like operands must branch on runtime truthiness before
    /// the string fallback path can treat classes as string-compatible values.
    pub(in crate::lowering) fn logical_or_object_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_structural_object_surface(value_ty) {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&logical.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        if !self.is_structural_object_surface(fallback_ty) {
            return Ok(None);
        }
        let cond = body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToBool,
                operand: value,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.left.span().start, logical.left.span().end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: self.erase_with(ErasureReason::DynamicOperator, "|| over objects"),
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || right` for erased values without losing the selected operand.
    ///
    /// Dynamic interop and imported structural callbacks can surface as
    /// `unknown` even when the source expression returns an object-like value.
    /// JavaScript `||` returns one of the original operands, so erased values
    /// must branch on runtime truthiness instead of being coerced through a
    /// string or boolean fallback representation.
    pub(in crate::lowering) fn logical_or_unknown_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.type_contains_unknown(value_ty) {
            return Ok(None);
        }
        // not a new erasure: the left operand already contains unknown
        let unknown_ty = self.unknown_type();
        let fallback = self.expression_with_hint(&logical.right, body, Some(unknown_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        if !self.type_contains_unknown(fallback_ty) {
            return Ok(None);
        }
        let cond = body.push_expr(Expr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToBool,
                operand: value,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.left.span().start, logical.left.span().end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: unknown_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `(guard && value) || fallback` as a value fallback.
    ///
    /// The normal logical lowering produces booleans because `&&`/`||` are also
    /// used in conditions. In value positions, JavaScript preserves the selected
    /// operand. This shape appears in option-bag and locale lookup code where a
    /// guarded member access falls back to another member with the same value
    /// type.
    pub(in crate::lowering) fn logical_and_value_fallback_expression(
        &mut self,
        outer: &oxc::ast::ast::LogicalExpression<'_>,
        left: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if left.operator != LogicalOperator::And {
            return Ok(None);
        }
        let value = self.expression(&left.right, body)?;
        let value_ty = Self::expr_ty(body, value);
        if self.is_numeric_like_type(value_ty) {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&outer.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        let Some(result_ty) = self.logical_fallback_result_type(value_ty, fallback_ty) else {
            return Ok(None);
        };
        let cond = self.condition_expression(&outer.left, body)?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: result_ty,
            span: self.span(outer.span.start, outer.span.end),
        })))
    }

    /// Return the common value type for JavaScript logical fallback operands.
    pub(in crate::lowering) fn logical_fallback_result_type(
        &mut self,
        value_ty: smelt_hir::TypeId,
        fallback_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        if value_ty == fallback_ty {
            return Some(value_ty);
        }
        if self.is_string_compatible_type(value_ty) && self.is_string_compatible_type(fallback_ty) {
            return Some(self.ctx.krate.types.intern(Type::String));
        }
        if self.type_contains_unknown(value_ty) || self.type_contains_unknown(fallback_ty) {
            // not a new erasure: an operand already contains unknown
            return Some(self.unknown_type());
        }
        match (
            self.ctx.krate.types.get(value_ty),
            self.ctx.krate.types.get(fallback_ty),
        ) {
            (Some(Type::Optional(value)), _) if *value == fallback_ty => Some(fallback_ty),
            (_, Some(Type::Optional(fallback))) if value_ty == *fallback => Some(value_ty),
            _ => None,
        }
    }

    /// Lower numeric JavaScript `left || right` value fallback expressions.
    ///
    /// Date-fns uses `numeric % 7 || 7` to replace zero with a default value.
    /// Lowering this as boolean `||` loses the numeric result type, so Smelt
    /// models the expression as `left != 0 ? left : right` for numeric operands.
    pub(in crate::lowering) fn logical_or_numeric_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_numeric_like_type(value_ty) {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&logical.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        if !self.numeric_type_compatible(value_ty, fallback_ty) {
            return Ok(None);
        }
        let zero = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(0.0)),
            ty: self.ctx.krate.types.intern(Type::Float),
            span: self.span(logical.span.start, logical.span.end),
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: value,
                rhs: zero,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.span.start, logical.span.end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: value_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || right` fallback expressions selected by string truthiness.
    ///
    /// A numeric fallback remains an erased selected value because expressions
    /// such as `+(parts[index] || 0)` numerically coerce either branch after
    /// selection. Emitting a boolean result would discard the string value.
    pub(in crate::lowering) fn logical_or_string_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !self.is_string_compatible_type(value_ty) {
            return Ok(None);
        }
        // The truthiness test below is `value != ""`, which is only JavaScript
        // truthiness for a STRING. A union that merely HAS a string arm
        // (`Pattern | null`, where `Pattern = [..] | '*'`) is truthy for every
        // array arm too; lowering it here typed the whole `||` as `string` and
        // stringified the array to "[object Object]" (Hono's trie-router
        // `insert`). Such a union takes the general value-preserving path.
        if let Some(Type::Union(items)) = self.ctx.krate.types.get(value_ty)
            && items.iter().any(|item| {
                !matches!(self.ctx.krate.types.get(*item), Some(Type::String | Type::None))
            })
        {
            return Ok(None);
        }
        let fallback = self.expression_with_hint(&logical.right, body, Some(value_ty))?;
        let fallback_ty = Self::expr_ty(body, fallback);
        let result_ty = if self.is_string_compatible_type(fallback_ty) {
            self.ctx.krate.types.intern(Type::String)
        } else if self.is_numeric_like_type(fallback_ty) {
            self.erase_with(ErasureReason::DynamicOperator, "string || number")
        } else {
            return Ok(None);
        };
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let empty = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String(String::new())),
            ty: string_ty,
            span: self.span(logical.span.start, logical.span.end),
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: value,
                rhs: empty,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(logical.span.start, logical.span.end),
        });
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: value,
                else_expr: fallback,
            },
            ty: result_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Lower JavaScript `left || []` fallback expressions for array values.
    pub(in crate::lowering) fn logical_or_list_fallback_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        value: smelt_hir::ExprId,
        value_ty: smelt_hir::TypeId,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(item_ty) = self.list_fallback_item_ty(value_ty) else {
            return Ok(None);
        };
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let fallback = self.expression_with_hint(&logical.right, body, Some(list_ty))?;
        if !Self::is_empty_list_expr(body, fallback) && Self::expr_ty(body, fallback) != list_ty {
            return Ok(None);
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce {
                optional: value,
                fallback,
            },
            ty: list_ty,
            span: self.span(logical.span.start, logical.span.end),
        })))
    }

    /// Return the item type for a value that can participate in an array fallback.
    pub(in crate::lowering) fn list_fallback_item_ty(
        &mut self,
        value_ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(value_ty).cloned() {
            Some(Type::List(item_ty)) => Some(item_ty),
            Some(Type::Optional(inner_ty)) => self.list_fallback_item_ty(inner_ty),
            Some(Type::Union(items)) => items
                .into_iter()
                .find_map(|item| self.list_fallback_item_ty(item)),
            _ => None,
        }
    }

    /// Lower TypeScript nullish coalescing while preserving falsey values.
    pub(in crate::lowering) fn nullish_coalesce_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let optional = self.expression(&logical.left, body)?;
        let optional_ty = Self::expr_ty(body, optional);
        let Some(ty) = self.non_nullish_type(optional_ty) else {
            if self.ctx.krate.types.get(optional_ty) == Some(&Type::None) {
                let fallback = self.expression(&logical.right, body)?;
                return Ok(fallback);
            }
            return Ok(optional);
        };
        let right_hint = match &logical.right {
            Expression::LogicalExpression(right_logical)
                if right_logical.operator == LogicalOperator::Coalesce =>
            {
                type_hint
            }
            _ => Some(ty),
        };
        let mut fallback = match self.match_or_array_fallback_expression(logical, body, optional, ty)? {
            MatchArrayFallback::Selected(expr) => return Ok(expr),
            MatchArrayFallback::NotArray(fallback) => fallback,
            MatchArrayFallback::NotMatch => {
                self.expression_with_hint(&logical.right, body, right_hint)?
            }
        };
        let fallback_ty = Self::expr_ty(body, fallback);
        let ty = if fallback_ty == ty || self.numeric_type_compatible(ty, fallback_ty)
        {
            ty
        } else if self.ctx.krate.types.get(ty) == Some(&Type::Unknown) {
            // This is a genuine dynamic boundary: the successful left operand
            // is source `unknown`, so a concrete fallback must enter its tagged
            // runtime surface through an explicit boundary adapter.
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty,
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            ty
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && self.numeric_type_compatible(ty, fallback_inner)
        {
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && fallback_inner == ty
        {
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if matches!(self.ctx.krate.types.get(optional_ty), Some(Type::Optional(inner)) if *inner == ty)
            && self.erased_or_union_surface(fallback_ty)
        {
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if let Some(fallback_inner) = self.non_nullish_type(fallback_ty)
            && self.nullish_fallback_types_are_structurally_compatible(ty, fallback_inner)
        {
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty: smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty),
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, ty)
        } else if matches!(self.ctx.krate.types.get(ty), Some(Type::TypeParam { .. }))
            && matches!(
                self.ctx.krate.types.get(fallback_ty),
                Some(Type::TypeParam { .. })
            )
        {
            type_hint.unwrap_or_else(|| {
                self.ctx
                    .krate
                    .types
                    .intern(Type::Union(vec![ty, fallback_ty]))
            })
        } else if matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Union(items)) if items.contains(&fallback_ty)
        ) {
            // The fallback is already one arm of the value surface. Selecting
            // it does not narrow every successful left-hand value to that one
            // arm: `value ?? null` must retain the other union members.
            ty
        } else if self.nullish_fallback_matches_union_member(ty, fallback_ty) {
            ty
        } else if self.nullish_fallback_types_are_structurally_compatible(ty, fallback_ty) {
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty,
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            ty
        } else if let Some(hint) = type_hint
            && !self.concrete_type_requires_never_value(hint)
        {
            // `a ?? b` is `NonNullable<typeof a> | typeof b`: it is nullish
            // exactly when the fallback is. A hint that cannot hold the
            // fallback's `undefined` (the other arm of an enclosing ternary,
            // `typeof arg === 'number' ? arg : (arg?.status ?? this.#status)`)
            // has to widen to its optional form, or the absent value is
            // converted to the hint's type — `undefined` became the `NaN`
            // status of every Hono `c.json(..)` without an explicit status.
            if self.is_nullable_value_type(fallback_ty) && !self.is_nullable_value_type(hint) {
                smelt_hir::type_normalize::optional_of(&mut self.ctx.krate.types, hint)
            } else {
                hint
            }
        } else if self.is_declared_record_type(ty)
            && matches!(
                self.ctx.krate.types.get(fallback_ty),
                Some(Type::Bool | Type::Int | Type::Float | Type::String)
            )
        {
            // A RECORD on the left and a primitive fallback: TypeScript's
            // `NonNullable<typeof a> | typeof b` is a genuine two-arm union
            // (`server.address() ?? -1` is `AddressInfo | number`). The
            // class-surface branch below would assert the primitive INTO the
            // record type, a value no record can hold — the generated Rust
            // assigned an `f64` to an `AddressInfo` local (E0308). A record
            // declared by the program has concrete storage, so the union is a
            // concrete generated enum, not an erasure.
            self.ctx
                .krate
                .types
                .intern(Type::Union(vec![ty, fallback_ty]))
        } else if !self.erased_or_union_surface(ty)
            && !self.erased_or_union_surface(fallback_ty)
            && !self.concrete_type_requires_never_value(ty)
            && !self.concrete_type_requires_never_value(fallback_ty)
            && (self.type_assignable_to(fallback_ty, ty)
                || self.function_value_widens_to(fallback_ty, ty))
        {
            // `NonNullable<typeof a> | typeof b` REDUCES to the left arm when
            // the fallback is assignable to it, which is what TypeScript reports
            // for `options.shouldRetry ?? (() => true)`: a callback may ignore
            // trailing parameters, so `() => boolean` already inhabits
            // `(attempt: number) => boolean`. Interning the two-arm union
            // instead produced a generated union whose call site knew only the
            // declared arm, so selecting the fallback panicked with "union guard
            // selected an excluded member" at runtime. Both arms are concrete
            // here (the guards above), so the fallback enters the left arm
            // through the ordinary assertion adapter.
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty,
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            ty
        } else if !self.erased_or_union_surface(ty)
            && !self.erased_or_union_surface(fallback_ty)
            && !self.concrete_type_requires_never_value(ty)
            && !self.concrete_type_requires_never_value(fallback_ty)
        {
            // TypeScript types `a ?? b` as `NonNullable<typeof a> | typeof b`.
            // When both arms are concrete and unrelated — `process.env.PORT ??
            // 3000` is `string | number` — the join is that union. Asserting the
            // fallback into the left arm's type instead would emit
            // `let port: String = 3000.0;`, which is the left arm's type carrying
            // the right arm's value. Erased/`never` surfaces keep the existing
            // boundary-adapter behaviour above and below this branch.
            self.ctx
                .krate
                .types
                .intern(Type::Union(vec![ty, fallback_ty]))
        } else if self.erased_or_union_surface(ty)
            || self.erased_or_union_surface(fallback_ty)
            || !self.concrete_type_requires_never_value(ty)
        {
            fallback = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: fallback },
                ty,
                span: self.span(logical.right.span().start, logical.right.span().end),
            });
            ty
        } else {
            return Err(SmeltError::unsupported(
                self.span(logical.span.start, logical.span.end),
                "nullish coalescing fallback must match the non-nullish value type",
            ));
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::OptionalCoalesce { optional, fallback },
            ty,
            span: self.span(logical.span.start, logical.span.end),
        }))
    }

    /// Return whether a `??` fallback is covered by one member of the non-null union.
    /// Whether a value of type `ty` can be `null`/`undefined` at run time: an
    /// optional, a union with a `None` arm, `None` itself, or an erased surface
    /// (which can hold anything).
    fn is_nullable_value_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(_) | Type::None | Type::Unknown) => true,
            Some(Type::Union(items)) => items
                .iter()
                .any(|item| self.is_nullable_value_type(*item)),
            _ => false,
        }
    }

    pub(in crate::lowering) fn nullish_fallback_matches_union_member(
        &self,
        non_nullish_ty: smelt_hir::TypeId,
        fallback_ty: smelt_hir::TypeId,
    ) -> bool {
        let Some(Type::Union(items)) = self.ctx.krate.types.get(non_nullish_ty) else {
            return false;
        };
        items
            .iter()
            .copied()
            .any(|item| item == fallback_ty || self.numeric_type_compatible(item, fallback_ty))
    }

    /// Return whether `??` may treat the fallback as the optional side's object surface.
    ///
    /// TypeScript uses structural object compatibility, so date-fns can coalesce
    /// an optional `Locale` interface with a concrete exported locale object.
    /// Smelt keeps the optional side's type and inserts a typed assertion around
    /// the fallback expression when both sides are object-like surfaces.
    pub(in crate::lowering) fn nullish_fallback_types_are_structurally_compatible(
        &mut self,
        optional_inner: smelt_hir::TypeId,
        fallback_ty: smelt_hir::TypeId,
    ) -> bool {
        let fallback_ty = self.non_nullish_type(fallback_ty).unwrap_or(fallback_ty);
        self.is_structural_object_surface(optional_inner)
            && self.is_structural_object_surface(fallback_ty)
    }

    /// Return whether a type behaves as a structural object surface.
    pub(in crate::lowering) fn is_structural_object_surface(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(
                Type::Class { .. } | Type::Dict(_, _) | Type::TypeParam { .. } | Type::Unknown,
            ) => true,
            Some(Type::Optional(item)) => self.is_structural_object_surface(*item),
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.is_structural_object_surface(item)),
            _ => false,
        }
    }

    /// Return the type left after removing TypeScript nullish values.
    pub(in crate::lowering) fn non_nullish_type(
        &mut self,
        ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        smelt_hir::type_normalize::non_nullish_type(&mut self.ctx.krate.types, ty)
    }

    /// Return whether `ty` still admits a TypeScript nullish value.
    ///
    /// Used to decide whether a source-level `!` has anything to narrow at a
    /// sink: an `Optional`, the bare `null` type, and a union carrying a `null`
    /// arm all accept the nullish value, so the assertion is purely type-level
    /// there. Checked structurally rather than by comparing
    /// [`Self::non_nullish_type`] against the input, because that helper also
    /// normalizes and so can report a difference for wholly non-nullish types.
    pub(in crate::lowering) fn type_admits_nullish(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(_) | Type::None) => true,
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.type_admits_nullish(item)),
            _ => false,
        }
    }
}
