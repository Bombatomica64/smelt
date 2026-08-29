use serde::{Deserialize, Serialize};

use crate::Type;
use crate::expr::ExprKind;
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
    for (item_idx, item) in krate.items.iter().enumerate() {
        let crate::Item::Class(class) = item else {
            continue;
        };
        for descriptor in &class.descriptors {
            if krate.types.get(descriptor.read_ty).is_none() {
                errors.push(ValidationError {
                    message: format!(
                        "class item {item_idx} descriptor {:?} has unknown read type {:?}",
                        descriptor.name, descriptor.read_ty
                    ),
                });
            }
            if descriptor
                .write_ty
                .is_some_and(|ty| krate.types.get(ty).is_none())
            {
                errors.push(ValidationError {
                    message: format!(
                        "class item {item_idx} descriptor {:?} has unknown write type",
                        descriptor.name
                    ),
                });
            }
            for (role, callable) in [("getter", descriptor.getter), ("setter", descriptor.setter)] {
                if callable.is_some_and(|callable| {
                    !matches!(
                        krate.items.get(callable.0 as usize),
                        Some(crate::Item::Function(_))
                    )
                }) {
                    errors.push(ValidationError {
                        message: format!(
                            "class item {item_idx} descriptor {:?} references an unknown {role}",
                            descriptor.name
                        ),
                    });
                }
            }
            for field in &descriptor.value_fields {
                if krate.types.get(field.ty).is_none() {
                    errors.push(ValidationError {
                        message: format!(
                            "class item {item_idx} descriptor {:?} value field {:?} has unknown type",
                            descriptor.name, field.name
                        ),
                    });
                }
            }
        }
    }
    for body in &krate.bodies {
        for expr in &body.exprs {
            if let ExprKind::Closure(closure) = &expr.kind
                && krate
                    .bodies
                    .get(closure.body.0 as usize)
                    .is_some_and(|body| body.async_state_machine.is_some())
                && let Some(slot) = async_bodies.get_mut(closure.body.0 as usize)
            {
                *slot = true;
            }
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
                // `Array.prototype.sort`'s comparator is the one callback slot
                // that may legitimately hold an OPTIONAL callable: ECMA-262
                // `SortCompare` step 1 makes `sort(undefined)` identical to
                // `sort()`, so the frontend keeps `compare?: (a, b) => number`
                // as a real `Optional(Function)` and the emitter branches on it
                // instead of erasing the absence into an `undefined` result.
                ExprKind::ListSort {
                    comparator: Some(callback),
                    ..
                } => {
                    let Some(callback_expr) = body.exprs.get(callback.0 as usize) else {
                        errors.push(ValidationError {
                            message: format!(
                                "body {body_idx} expr {expr_idx} callback references unknown expr {callback:?}"
                            ),
                        });
                        continue;
                    };
                    let callback_ty = match krate.types.get(callback_expr.ty) {
                        Some(Type::Optional(inner)) => krate.types.get(*inner),
                        other => other,
                    };
                    if !matches!(callback_ty, Some(Type::Function(_))) {
                        let callback_path = krate
                            .modules
                            .get(callback_expr.span.file.0 as usize)
                            .map_or("<unknown>", |module| module.source.path.as_str());
                        errors.push(ValidationError {
                            message: format!(
                                "body {body_idx} expr {expr_idx} callback must have function type, got {:?} at {callback_path} {:?}",
                                callback_expr.ty, callback_expr.span
                            ),
                        });
                    }
                }
                ExprKind::ListCallback { callback, .. }
                | ExprKind::ListFromLengthMap { callback, .. }
                | ExprKind::ListReduce { callback, .. }
                | ExprKind::ListSort {
                    key: Some(callback),
                    ..
                }
                | ExprKind::ListSorted {
                    key: Some(callback),
                    ..
                } => {
                    let Some(callback_expr) = body.exprs.get(callback.0 as usize) else {
                        errors.push(ValidationError {
                            message: format!(
                                "body {body_idx} expr {expr_idx} callback references unknown expr {callback:?}"
                            ),
                        });
                        continue;
                    };
                    if !matches!(krate.types.get(callback_expr.ty), Some(Type::Function(_))) {
                        let callback_path = krate
                            .modules
                            .get(callback_expr.span.file.0 as usize)
                            .map_or("<unknown>", |module| module.source.path.as_str());
                        errors.push(ValidationError {
                            message: format!(
                                "body {body_idx} expr {expr_idx} callback must have function type, got {:?} at {callback_path} {:?}",
                                callback_expr.ty, callback_expr.span
                            ),
                        });
                    }
                }
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
                                    "body {body_idx} expr {expr_idx} closure capture targets unknown closure local {body_local:?}"
                                ),
                            });
                        }
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
