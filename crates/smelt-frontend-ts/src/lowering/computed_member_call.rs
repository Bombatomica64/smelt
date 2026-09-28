//! A call whose callee is a computed member read of a non-`Function` value.
//!
//! `table[i](args)` / `result[0][0][0][0](c, next)` reads an element and calls
//! it. When the element's static type is a concrete function the generic
//! computed-member arm in `call_dispatch` already emits a typed closure call.
//! Two other callee shapes used to fall through to fallbacks that DROPPED the
//! call and produced a wrong value with no diagnostic:
//!
//! * a callable UNION (`Handler | MiddlewareHandler`, a union of function
//!   types) — the call lowered to its FIRST ARGUMENT, so Hono's single-handler
//!   dispatch `res = matchResult[0][0][0][0](c, next)` assigned the `Context`
//!   itself to `res` and every response body was lost;
//! * an erased callee (`unknown`/`any`, a type parameter, an erased callable
//!   object) — the call lowered to a literal `undefined`.
//!
//! A hand-written port calls the value in both cases. This module applies the
//! same rules the identifier-callee path (`local_callable_call`) applies, so
//! `const f = xs[0]; f(a)` and `xs[0](a)` cannot diverge:
//!
//! * a union / optional / callable-interface surface with a statically
//!   selectable call signature is asserted to that signature and called
//!   typed;
//! * an erased surface goes through the runtime callable ABI (the value is
//!   already `SmeltUnknown`; no new erasure is introduced), packing a spread
//!   argument list as `ClosureCallSpread` does everywhere else.

use oxc::ast::ast::{CallExpression, ComputedMemberExpression};
use smelt_hir::{Body, Expr, ExprKind, Type};

use super::{ModuleBuilder, SmeltError};

impl ModuleBuilder<'_> {
    /// Lower `object[key](args)` when the read's type is a callable
    /// non-function surface; `None` leaves every other shape to the caller.
    ///
    /// `callee` is the already-lowered member read and `callee_ty` its type. A
    /// bare `Type::Function` answers `None` (the caller's typed arm owns it),
    /// as does a type with no call surface at all.
    pub(in crate::lowering) fn computed_member_callable_value_call(
        &mut self,
        call: &CallExpression<'_>,
        member: &ComputedMemberExpression<'_>,
        callee: smelt_hir::ExprId,
        callee_ty: smelt_hir::TypeId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if matches!(self.ctx.krate.types.get(callee_ty), Some(Type::Function(_))) {
            return Ok(None);
        }
        let span = self.span(call.span.start, call.span.end);
        let has_spread = self.call_has_spread_arguments_or_source_spread(call);
        let probed_arg_tys = self.probe_argument_types(&call.arguments, body);
        if !has_spread
            && let Some(function_ty) = self.function_member_type_for_args(
                callee_ty,
                Some(call.arguments.len()),
                &probed_arg_tys,
            )
            && let Some(Type::Function(function)) =
                self.ctx.krate.types.get(function_ty).cloned()
            && call.arguments.len() >= function.required_params.unwrap_or(function.params.len())
        {
            let callee = body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: function_ty,
                span: self.span(member.span.start, member.span.end),
            });
            let mut args = Vec::with_capacity(function.params.len());
            for (index, argument) in call.arguments.iter().enumerate() {
                let arg = self.argument(argument, body)?;
                // Surplus arguments are evaluated for their effects but not
                // passed: the selected signature has no slot for them.
                if index < function.params.len() {
                    args.push(arg);
                }
            }
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: function.return_ty,
                span,
            })));
        }
        let erased = matches!(
            self.ctx.krate.types.get(self.type_param_constraint_or_self(callee_ty)),
            Some(Type::Unknown)
        ) || matches!(self.ctx.krate.types.get(callee_ty), Some(Type::TypeParam { .. }))
            || self.is_callable_object_erased_class(callee_ty);
        if !erased {
            return Ok(None);
        }
        let unknown = self.ctx.krate.types.intern(Type::Unknown);
        let callee = if self.is_callable_object_erased_class(callee_ty) {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: unknown,
                span: self.span(member.span.start, member.span.end),
            })
        } else {
            callee
        };
        if has_spread {
            let args = self.packed_spread_call_arguments(unknown, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread { callee, args },
                ty: unknown,
                span,
            })));
        }
        let args = call
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: unknown,
            span,
        })))
    }
}
