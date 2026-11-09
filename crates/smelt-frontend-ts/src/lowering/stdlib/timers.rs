//! `setTimeout`, `setInterval` and their `clear` forms.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::Argument;
use smelt_hir::{AsyncOp, Body, ErasureReason, Expr, ExprKind, Literal, Type};

impl ModuleBuilder<'_> {
    /// Lower small TypeScript timer shims used by async fixtures.
    pub(in crate::lowering) fn timer_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(callee) = self.host_global(&call.callee) else {
            return Ok(None);
        };
        match callee.name() {
            "setTimeout" if call.arguments.len() == 1 => {
                let Some(duration_argument) = call.arguments.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "setTimeout lowering supports the Smelt timer shim shape setTimeout(milliseconds)",
                    ));
                };
                let first = self.argument(duration_argument, body)?;
                // `setTimeout(callback)` is the ordinary timer with its delay
                // omitted, which JavaScript reads as `0`. Only a non-callable
                // argument is the shim's `setTimeout(milliseconds)`.
                if matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, first)),
                    Some(Type::Function(_))
                ) {
                    let float_ty = self.ctx.krate.types.intern(Type::Float);
                    let span = self.span(call.span.start, call.span.end);
                    let zero = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::Float(0.0)),
                        ty: float_ty,
                        span,
                    });
                    let ty = self.erase_with(ErasureReason::UntypedBuiltin, "timer id");
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::AsyncOp {
                            op: AsyncOp::SetTimeout,
                            args: vec![first, zero],
                        },
                        ty,
                        span,
                    })));
                }
                let none_ty = self.ctx.krate.types.intern(Type::None);
                let ty = self.ctx.krate.types.intern(Type::Future(none_ty));
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::Sleep,
                        args: vec![first],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "setTimeout" if call.arguments.len() == 2 => {
                let Some(callback) = call.arguments.first() else {
                    return Ok(None);
                };
                let Some(duration) = call.arguments.get(1) else {
                    return Ok(None);
                };
                let callback = self.argument(callback, body)?;
                let duration = self.argument(duration, body)?;
                let ty = self.erase_with(ErasureReason::UntypedBuiltin, "timer id");
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::SetTimeout,
                        args: vec![callback, duration],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            "clearTimeout" if call.arguments.len() == 1 => {
                let Some(timeout) = call.arguments.first() else {
                    return Ok(None);
                };
                let timeout = self.argument(timeout, body)?;
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::ClearTimeout,
                        args: vec![timeout],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            // `setInterval(callback, period)` registers a repeating timer that
            // re-arms itself after every fire, mirroring the two-argument
            // `setTimeout` shape. The shared virtual-time timer queue drives it,
            // so the only difference at codegen time is the re-arm; see
            // `AsyncOp::SetInterval` in the call emitter.
            "setInterval" if call.arguments.len() == 2 => {
                let Some(callback) = call.arguments.first() else {
                    return Ok(None);
                };
                let Some(duration) = call.arguments.get(1) else {
                    return Ok(None);
                };
                let callback = self.argument(callback, body)?;
                let duration = self.argument(duration, body)?;
                let ty = self.erase_with(ErasureReason::UntypedBuiltin, "timer id");
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::SetInterval,
                        args: vec![callback, duration],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            // `clearInterval(id)` cancels a repeating timer by handle. Intervals
            // share the timer queue with timeouts, so this is the same
            // cancel-by-id as `clearTimeout`.
            "clearInterval" if call.arguments.len() == 1 => {
                let Some(timer) = call.arguments.first() else {
                    return Ok(None);
                };
                let timer = self.argument(timer, body)?;
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::ClearInterval,
                        args: vec![timer],
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })))
            }
            // `setTimeout(callback, ms, ...args)` / `setInterval(callback, ms,
            // ...args)` forward the extra arguments to the callback when the
            // timer fires.
            //
            // When the callback is a statically typed function and every extra is
            // a concretely typed positional argument (no source spread), the
            // extras are captured by a synthesized zero-argument wrapper closure
            // that calls the callback directly — no erased `Vec<SmeltUnknown>` is
            // produced. A source spread or an untyped extra is a genuine dynamic
            // boundary, so those keep the erased-list path where the extras pack
            // into one operand dispatched through the dynamic callback ABI.
            "setTimeout" | "setInterval" if call.arguments.len() >= 3 => {
                let (Some(callback_arg), Some(duration_arg)) =
                    (call.arguments.first(), call.arguments.get(1))
                else {
                    return Ok(None);
                };
                let callback = self.argument(callback_arg, body)?;
                let duration = self.argument(duration_arg, body)?;
                let unknown_ty = self.erase_with(ErasureReason::UntypedBuiltin, "timer id");
                let span = self.span(call.span.start, call.span.end);
                let op = if callee.is("setTimeout") {
                    AsyncOp::SetTimeout
                } else {
                    AsyncOp::SetInterval
                };

                let extra_args = call.arguments.get(2..).unwrap_or_default();
                let has_spread = extra_args
                    .iter()
                    .any(|arg| matches!(arg, Argument::SpreadElement(_)));
                if !has_spread {
                    // Lower each extra exactly once. Either the typed wrapper
                    // consumes them, or they pack into the erased list below —
                    // never both, so source side effects run once.
                    let extras = extra_args
                        .iter()
                        .map(|arg| self.argument(arg, body))
                        .collect::<Result<Vec<_>, _>>()?;
                    if let Some(wrapper) =
                        self.timer_typed_wrapper_closure(callback, &extras, span, body)
                    {
                        return Ok(Some(body.push_expr(Expr {
                            kind: ExprKind::AsyncOp {
                                op,
                                args: vec![wrapper, duration],
                            },
                            ty: unknown_ty,
                            span,
                        })));
                    }
                    let list_ty = self.ctx.krate.types.intern(Type::List(unknown_ty));
                    let extra = body.push_expr(Expr {
                        kind: ExprKind::ListLit(extras),
                        ty: list_ty,
                        span,
                    });
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::AsyncOp {
                            op,
                            args: vec![callback, duration, extra],
                        },
                        ty: unknown_ty,
                        span,
                    })));
                }

                let extra = self.packed_spread_arguments(
                    unknown_ty,
                    extra_args,
                    span,
                    body,
                )?;
                Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::AsyncOp {
                        op,
                        args: vec![callback, duration, extra],
                    },
                    ty: unknown_ty,
                    span,
                })))
            }
            "setTimeout" | "clearTimeout" | "setInterval" | "clearInterval" => {
                Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "timer lowering supports setTimeout(milliseconds), setTimeout(callback, milliseconds), clearTimeout(id), setInterval(callback, milliseconds), and clearInterval(id)",
                ))
            }
            _ => Ok(None),
        }
    }
}
