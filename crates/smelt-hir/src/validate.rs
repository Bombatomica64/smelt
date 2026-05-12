use serde::{Deserialize, Serialize};

use crate::Type;
use crate::expr::{CallbackExpr, CallbackExprKind, ExprKind};
use crate::ids::id_index;
use crate::krate::Crate;

/// A validation error discovered while checking HIR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// Human-readable description of the problem.
    pub message: String,
}

#[must_use]
/// Validates a crate and returns any structural errors.
#[expect(
    clippy::too_many_lines,
    reason = "HIR validation keeps cross-checks in one pass until validation is split by concern"
)]
pub fn validate(krate: &Crate) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let mut async_bodies = vec![false; krate.bodies.len()];

    for item in &krate.items {
        if let crate::Item::Function(function) = item
            && let Some(body) = function.body
            && let Some(slot) = async_bodies.get_mut(body.0 as usize)
        {
            *slot = function.is_async;
        }
    }

    for (body_idx, body) in krate.bodies.iter().enumerate() {
        for (expr_idx, expr) in body.exprs.iter().enumerate() {
            if krate.types.get(expr.ty).is_none() {
                errors.push(ValidationError {
                    message: format!(
                        "body {body_idx} expr {expr_idx} has unknown type {:?}",
                        expr.ty
                    ),
                });
            }
            if let ExprKind::Local(local) = expr.kind
                && body.locals.get(local.0 as usize).is_none()
            {
                errors.push(ValidationError {
                    message: format!(
                        "body {body_idx} expr {expr_idx} reads unknown local {local:?}"
                    ),
                });
            }
            if let ExprKind::Await(inner) = expr.kind {
                if !async_bodies.get(body_idx).copied().unwrap_or(false) {
                    errors.push(ValidationError {
                        message: format!(
                            "body {body_idx} expr {expr_idx} uses await outside an async function"
                        ),
                    });
                }
                let Some(inner_expr) = body.exprs.get(inner.0 as usize) else {
                    errors.push(ValidationError {
                        message: format!(
                            "body {body_idx} expr {expr_idx} awaits unknown expr {inner:?}"
                        ),
                    });
                    continue;
                };
                if !matches!(krate.types.get(inner_expr.ty), Some(Type::Future(_))) {
                    errors.push(ValidationError {
                        message: format!(
                            "body {body_idx} expr {expr_idx} awaits non-future type {:?}",
                            inner_expr.ty
                        ),
                    });
                }
            }
            #[expect(
                clippy::wildcard_enum_match_arm,
                reason = "HIR validation only needs special handling for callback-bearing expressions"
            )]
            match &expr.kind {
                ExprKind::ListCallback { callback, .. } | ExprKind::ListReduce { callback, .. } => {
                    let Some(callback_expr) = body.exprs.get(callback.0 as usize) else {
                        errors.push(ValidationError {
                            message: format!(
                                "body {body_idx} expr {expr_idx} callback references unknown expr {callback:?}"
                            ),
                        });
                        continue;
                    };
                    if !matches!(
                        callback_expr.kind,
                        ExprKind::Closure(_) | ExprKind::Local(_) | ExprKind::Item(_)
                    ) {
                        errors.push(ValidationError {
                            message: format!(
                                "body {body_idx} expr {expr_idx} callback must be a closure or callable value"
                            ),
                        });
                    }
                }
                ExprKind::ListSort {
                    comparator: Some(callback),
                    ..
                } => validate_callback_expr(body_idx, body, callback, &mut errors),
                ExprKind::Closure(closure) => {
                    if krate.bodies.get(closure.body.0 as usize).is_none() {
                        errors.push(ValidationError {
                            message: format!(
                                "body {body_idx} expr {expr_idx} closure references unknown body {:?}",
                                closure.body
                            ),
                        });
                    }
                    for capture in &closure.captures {
                        if body.locals.get(capture.source_local.0 as usize).is_none() {
                            errors.push(ValidationError {
                                message: format!(
                                    "body {body_idx} expr {expr_idx} closure captures unknown local {:?}",
                                    capture.source_local
                                ),
                            });
                        }
                        if krate.types.get(capture.ty).is_none() {
                            errors.push(ValidationError {
                                message: format!(
                                    "body {body_idx} expr {expr_idx} closure capture has unknown type {:?}",
                                    capture.ty
                                ),
                            });
                        }
                        if let Some(body_local) = capture.body_local
                            && krate
                                .bodies
                                .get(closure.body.0 as usize)
                                .and_then(|closure_body| {
                                    closure_body.locals.get(body_local.0 as usize)
                                })
                                .is_none()
                        {
                            errors.push(ValidationError {
                                message: format!(
                                    "body {body_idx} expr {expr_idx} closure capture targets unknown closure local {:?}",
                                    body_local
                                ),
                            });
                        }
                    }
                    if let Some(callback) = &closure.callback_body {
                        validate_callback_expr(body_idx, body, callback, &mut errors);
                    }
                }
                _ => {}
            }
        }

        let body_is_async = async_bodies.get(body_idx).copied().unwrap_or(false);
        match (&body.async_state_machine, body_is_async) {
            (Some(_), false) => errors.push(ValidationError {
                message: format!("body {body_idx} has an async state machine but is not async"),
            }),
            (None, true) => errors.push(ValidationError {
                message: format!("body {body_idx} is async but has no async state machine"),
            }),
            _ => {}
        }
        if let Some(machine) = &body.async_state_machine {
            if machine.states.len() != machine.suspensions.len() + 1 {
                errors.push(ValidationError {
                    message: format!(
                        "body {body_idx} async state count must be suspension count plus one"
                    ),
                });
            }
            for (idx, state) in machine.states.iter().enumerate() {
                if state.id.0 != id_index(idx) {
                    errors.push(ValidationError {
                        message: format!("body {body_idx} async state {idx} has non-linear id"),
                    });
                }
            }
            for suspension in &machine.suspensions {
                let Some(await_expr) = body.exprs.get(suspension.await_expr.0 as usize) else {
                    errors.push(ValidationError {
                        message: format!(
                            "body {body_idx} async suspension references unknown await {:?}",
                            suspension.await_expr
                        ),
                    });
                    continue;
                };
                if !matches!(await_expr.kind, ExprKind::Await(_)) {
                    errors.push(ValidationError {
                        message: format!(
                            "body {body_idx} async suspension {:?} is not an await expression",
                            suspension.await_expr
                        ),
                    });
                }
            }
        }
    }

    errors
}

/// Validate captured locals referenced by an embedded callback expression.
fn validate_callback_expr(
    body_idx: usize,
    body: &crate::Body,
    callback: &CallbackExpr,
    errors: &mut Vec<ValidationError>,
) {
    match &callback.kind {
        CallbackExprKind::Capture(local) => {
            if body.locals.get(local.0 as usize).is_none() {
                errors.push(ValidationError {
                    message: format!("body {body_idx} callback captures unknown local {local:?}"),
                });
            }
        }
        CallbackExprKind::AssignCapture { target, value } => {
            if body.locals.get(target.0 as usize).is_none() {
                errors.push(ValidationError {
                    message: format!("body {body_idx} callback assigns unknown local {target:?}"),
                });
            }
            validate_callback_expr(body_idx, body, value, errors);
        }
        CallbackExprKind::ListLit(items) => {
            for item in items {
                validate_callback_expr(body_idx, body, item, errors);
            }
        }
        CallbackExprKind::Index { receiver, .. } | CallbackExprKind::Field { receiver, .. } => {
            validate_callback_expr(body_idx, body, receiver, errors);
        }
        CallbackExprKind::Unary { operand, .. } => {
            validate_callback_expr(body_idx, body, operand, errors);
        }
        CallbackExprKind::Binary { lhs, rhs, .. } => {
            validate_callback_expr(body_idx, body, lhs, errors);
            validate_callback_expr(body_idx, body, rhs, errors);
        }
        CallbackExprKind::Call { callee, args } => {
            validate_callback_expr(body_idx, body, callee, errors);
            for arg in args {
                validate_callback_expr(body_idx, body, &arg.expr, errors);
            }
        }
        CallbackExprKind::Param(_) | CallbackExprKind::Literal(_) => {}
    }
}
