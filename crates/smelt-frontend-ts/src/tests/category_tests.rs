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

/// `new` on a still-unmodeled known builtin class is categorized as missing
/// stdlib and left as an explicit blocker (not erased through `SmeltUnknown`).
#[test]
fn new_unresolved_builtin_class_is_missing_stdlib() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("const c = new AbortController();"), &mut ctx)?;
    assert_category(&errors, "AbortController", DiagnosticCategory::MissingStdlib)?;
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("const b = new Blob([]);"), &mut ctx)?;
    assert_category(&errors, "Blob", DiagnosticCategory::MissingStdlib)
}

/// `new ArrayBuffer(n)` lowers to a concrete record carrying the dedicated
/// `__smelt_arraybuffer` marker (and `byteLength`), giving it a distinct identity
/// for `instanceof ArrayBuffer` instead of erasing it to a shapeless value.
#[test]
fn new_arraybuffer_lowers_to_concrete_marker_record() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const buf = new ArrayBuffer(8);"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            (&expr.kind, ctx.krate.types.get(expr.ty)),
            (ExprKind::DictLit(entries), Some(Type::Dict(_, _)))
                if entries.len() == 2
        )),
        "expected `new ArrayBuffer(8)` to lower to a concrete record (DictLit + Dict type)",
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_arraybuffer"
        )),
        "expected the ArrayBuffer record to carry the `__smelt_arraybuffer` marker key",
    );
    Ok(())
}

/// `new Object()` / `Object(...)` lower to the same concrete record (a `Dict`
/// carrying `DictLit`) as an object literal `{}`, never an erased `SmeltUnknown`.
#[test]
fn new_object_constructor_lowers_to_concrete_record() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const o = new Object();"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            (&expr.kind, ctx.krate.types.get(expr.ty)),
            (ExprKind::DictLit(entries), Some(Type::Dict(_, _))) if entries.is_empty()
        )),
        "expected `new Object()` to lower to an empty concrete record (DictLit + Dict type)",
    );
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::UnknownCast { .. })),
        "expected `new Object()` not to route through SmeltUnknown",
    );
    Ok(())
}

/// `Object(value)` for an existing object value returns that value unchanged,
/// matching `Object(obj) === obj`, with no `SmeltUnknown` erasure.
#[test]
fn object_constructor_passes_object_argument_through() -> Result<(), String> {
    let source = ts!("const src = { a: 1 }; const o = new Object(src);");
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::UnknownCast { .. })),
        "expected `Object(obj)` passthrough not to route through SmeltUnknown",
    );
    Ok(())
}

/// A locally declared class shadowing the builtin `Object` name still wins, so
/// `new Object()` resolves to the user class instead of the record fallback.
#[test]
fn new_user_class_shadows_object_builtin() -> Result<(), String> {
    let source = ts!(r#"
class Object {
  value: number = 1;
}
export function make(): Object {
  return new Object();
}
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(source, &mut ctx)?;
    let _module = module(&ctx, module_id)?;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(
                expr.kind,
                ExprKind::New { class, .. } if ctx.krate.symbols.get(class) == Some("Object")
            )),
        "expected `new Object()` to resolve to the user-declared class",
    );
    Ok(())
}
