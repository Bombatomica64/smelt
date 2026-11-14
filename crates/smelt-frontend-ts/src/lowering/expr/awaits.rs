//! `await` expressions: the awaited value's type, including awaited unions.

use super::super::ModuleBuilder;
use crate::SmeltError;
use crate::lowering::erasure::NotNewErasure;
use oxc::span::GetSpan;
use smelt_hir::{Body, ErasureReason, Expr, ExprKind, Type};

impl ModuleBuilder<'_> {
    /// Lower a JavaScript `await` operand into a HIR await, or into the operand
    /// itself when it provably cannot be a thenable.
    ///
    /// Three cases, in order:
    ///
    /// 1. the operand's type is a future — await it and take the future's
    ///    output type;
    /// 2. the operand's type is erased or a union — it may still hold a runtime
    ///    promise, so assert it to a future and await it through the checked
    ///    extraction path, whose runtime helper drains promise chains and
    ///    passes non-thenables through unchanged. The awaited value takes the
    ///    contextual type when there is one and stays erased otherwise;
    /// 3. the operand's type is concrete and not a future — `await v` is `v`.
    ///
    /// No case discards the operand: doing so silently deletes the awaited
    /// computation, and its side effects, from the program.
    ///
    /// Shared by the ordinary expression path and the call-argument path so
    /// both spellings of `await x` lower through one rule.
    pub(in crate::lowering) fn await_expression(
        &mut self,
        await_expr: &oxc::ast::ast::AwaitExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if !self.fn_ctx.is_async {
            return Err(SmeltError::unsupported(
                self.span(await_expr.span.start, await_expr.span.end),
                "await expressions are only lowered inside async functions",
            ));
        }
        // The context describes the resolved value; the operand must therefore
        // produce a future of that value.
        let awaited_hint = type_hint.map(|hint| self.ctx.krate.types.intern(Type::Future(hint)));
        let awaited = self.expression_with_hint(&await_expr.argument, body, awaited_hint)?;
        let awaited_ty = Self::expr_ty(body, awaited);
        let Some(ty) = self.future_inner_type(awaited_ty) else {
            // A union with a promise arm needs the await even when the union
            // itself is not an erased surface: `number | Promise<number>` is a
            // concrete tagged enum, and handing the union on unawaited would leave
            // the next read matching only the value arm.
            let awaited_union_ty = self.awaited_union_type(awaited_ty);
            if self.erased_or_union_surface(awaited_ty) || awaited_union_ty.is_some() {
                // A union operand awaits to the join of its arms: `await`
                // unwraps the promise arms and passes the others through. The
                // operand is still asserted to be a future of that join, and the
                // emitter's union-to-future projection turns each value arm into
                // an already-resolved handle -- see
                // `project_union_value`. Without the join the awaited value
                // erased to `SmeltUnknown`, so Hono's
                // `new Response(null, await this.#dispatch(..))` reported an
                // erased init for an operand whose awaited type is one class.
                let resolved_ty = type_hint.or(awaited_union_ty).unwrap_or_else(|| {
                    if matches!(self.ctx.krate.types.get(awaited_ty), Some(Type::Unknown)) {
                        // not a new erasure: the awaited operand is already erased
                        self.unknown_type(NotNewErasure::ErasedUpstream)
                    } else {
                        self.erase_with(ErasureReason::DynamicOperator, "await union")
                    }
                });

                let future_ty = self.ctx.krate.types.intern(Type::Future(resolved_ty));
                let future = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: awaited },
                    ty: future_ty,
                    span: self.span(
                        await_expr.argument.span().start,
                        await_expr.argument.span().end,
                    ),
                });
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Await(future),
                    ty: resolved_ty,
                    span: self.span(await_expr.span.start, await_expr.span.end),
                }));
            }
            return Ok(awaited);
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::Await(awaited),
            ty,
            span: self.span(await_expr.span.start, await_expr.span.end),
        }))
    }

    /// Return the type `await` produces for a union operand.
    ///
    /// `await` unwraps each promise arm and passes every other arm through
    /// unchanged, then joins: `Response | Promise<Response>` awaits to
    /// `Response`, and `A | Promise<B>` awaits to `A | B`. Hono's
    /// `new Response(null, await this.#dispatch(..))` is the first shape, and
    /// without this it erased to `SmeltUnknown` -- so the value was reported as
    /// erased for an operand whose awaited type is one named class.
    ///
    /// `None` when the operand is not a union, when no arm is a promise (there
    /// is nothing to unwrap, and `await` on a plain value answers it unchanged),
    /// or when any arm is itself erased: joining an erased arm answers `Unknown`
    /// anyway, and the existing fallback already says so.
    fn awaited_union_type(&mut self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        let Some(Type::Union(members)) = self.ctx.krate.types.get(ty).cloned() else {
            return None;
        };
        if !members
            .iter()
            .any(|member| self.future_inner_type(*member).is_some())
        {
            return None;
        }
        let mut resolved: Vec<smelt_hir::TypeId> = Vec::new();
        for member in members {
            let inner = self.future_inner_type(member).unwrap_or(member);
            if matches!(
                self.ctx.krate.types.get(inner),
                Some(Type::Unknown | Type::TypeParam { .. })
            ) {
                return None;
            }
            if !resolved.contains(&inner) {
                resolved.push(inner);
            }
        }
        match resolved.as_slice() {
            [] => None,
            [single] => Some(*single),
            _ => Some(self.ctx.krate.types.intern(Type::Union(resolved))),
        }
    }
}
