//! Tests for the structured [`DiagnosticCategory`] assigned to lowering errors.
//!
//! Categorization happens where the diagnostic is raised, so tooling (the
//! library probes) can group failures without parsing message text.

use super::*;
use smelt_stdlib::DiagnosticCategory;

/// An unresolved reference to a known JS builtin is categorized as missing stdlib.
#[test]
fn unresolved_builtin_is_missing_stdlib() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("console.log(Reflect);"), &mut ctx)?;
    assert_category(&errors, "Reflect", DiagnosticCategory::MissingStdlib)
}

/// An unresolved reference to an unknown user symbol is categorized as unresolved.
#[test]
fn unresolved_user_symbol_is_unresolved_reference() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("console.log(notDefinedAnywhere);"), &mut ctx)?;
    assert_category(
        &errors,
        "unresolved identifier",
        DiagnosticCategory::UnresolvedReference,
    )
}

/// `new` on an unresolved known builtin class is categorized as missing stdlib.
#[test]
fn new_unresolved_builtin_class_is_missing_stdlib() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("const buf = new ArrayBuffer(8);"), &mut ctx)?;
    assert_category(&errors, "ArrayBuffer", DiagnosticCategory::MissingStdlib)
}
