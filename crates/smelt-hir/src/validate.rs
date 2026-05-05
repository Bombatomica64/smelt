use serde::{Deserialize, Serialize};

use crate::expr::ExprKind;
use crate::krate::Crate;

/// A validation error discovered while checking HIR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    /// Human-readable description of the problem.
    pub message: String,
}

#[must_use]
/// Validates a crate and returns any structural errors.
pub fn validate(krate: &Crate) -> Vec<ValidationError> {
    let mut errors = Vec::new();

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
        }
    }

    errors
}
