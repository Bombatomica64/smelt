//! Closure-table and capture-ABI validation.
//!
//! These checks enforce the closure ABI contract published by the
//! `lower::closures` module: closure-table indices are self-consistent, every
//! referenced local/type exists, and escaping closures capture by value so Rust
//! codegen can emit an owning `move` closure that does not borrow stack locals
//! past their owner's return.

use crate::Mir;

use super::{ValidationError, error, validate_type};

/// Validate MIR closure table entries and escape-specific capture rules.
///
/// The escaping-closure capture check below enforces invariant 1 of the closure
/// ABI contract published by the `lower::closures` module: every closure with
/// `escapes == true` must capture by value so Rust codegen can emit an owning
/// `move` closure that does not borrow stack locals past their owner's return.
pub(super) fn validate_closures(mir: &Mir, errors: &mut Vec<ValidationError>) {
    for (idx, closure) in mir.closures.iter().enumerate() {
        if usize::try_from(closure.id.0).ok() != Some(idx) {
            errors.push(error(format!(
                "closure index {idx} has mismatched id {:?}",
                closure.id
            )));
        }
        for local in &closure.locals {
            validate_type(mir, local.ty, errors);
        }
        for param in &closure.params {
            if usize::try_from(param.0)
                .ok()
                .and_then(|index| closure.locals.get(index))
                .is_none()
            {
                errors.push(error(format!(
                    "closure {:?} parameter references unknown local {:?}",
                    closure.id, param
                )));
            }
        }
        for capture in &closure.captures {
            validate_type(mir, capture.ty, errors);
            if let Some(target) = capture.target_local
                && usize::try_from(target.0)
                    .ok()
                    .and_then(|index| closure.locals.get(index))
                    .is_none()
            {
                errors.push(error(format!(
                    "closure {:?} capture targets unknown local {:?}",
                    closure.id, target
                )));
            }
            // Closure ABI contract invariant 1 (see `lower::closures`):
            // escaping closures capture by value only.
            if closure.escapes && capture.mode != smelt_hir::CaptureMode::ByValue {
                errors.push(error(format!(
                    "escaping closure {:?} captures {:?} without owning it",
                    closure.id, capture.source_local
                )));
            }
        }
        validate_type(mir, closure.return_ty, errors);
    }
}
