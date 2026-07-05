//! Tests for the structured [`DiagnosticCategory`] assigned to Python errors.
//!
//! The missing-annotation cases only apply without the `ty` feature: with `ty`
//! on, those annotations are resolved instead of erroring (see
//! `ty_resolution_tests`). Their imports are therefore gated too.

#[cfg(not(feature = "ty"))]
use super::{first_error, lower_errors};
#[cfg(not(feature = "ty"))]
use crate::HirCtx;
#[cfg(not(feature = "ty"))]
use smelt_stdlib::DiagnosticCategory;

/// A function missing its return type annotation is a typed-subset violation,
/// not an unimplemented lowering.
///
/// Only meaningful without the `ty` feature: with `ty` on, the missing
/// annotation is resolved from the body instead of erroring (see
/// `ty_resolution_tests`).
#[cfg(not(feature = "ty"))]
#[test]
fn missing_return_annotation_is_type_constraint() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(py!("def inc(x: int):\n    return x + 1\n"), &mut ctx)?;
    let error = first_error(&errors)?;
    if !error.message.contains("explicit return type annotation") {
        return Err(format!("unexpected message: {}", error.message));
    }
    if error.category != DiagnosticCategory::TypeConstraint {
        return Err(format!("expected TypeConstraint, got {:?}", error.category));
    }
    Ok(())
}

/// A parameter missing its type annotation is also a typed-subset violation.
///
/// Only meaningful without the `ty` feature (see above).
#[cfg(not(feature = "ty"))]
#[test]
fn missing_param_annotation_is_type_constraint() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(py!("def add(x, y) -> int:\n    return x + y\n"), &mut ctx)?;
    let error = first_error(&errors)?;
    if error.category != DiagnosticCategory::TypeConstraint {
        return Err(format!(
            "expected TypeConstraint for `{}`, got {:?}",
            error.message, error.category
        ));
    }
    Ok(())
}
