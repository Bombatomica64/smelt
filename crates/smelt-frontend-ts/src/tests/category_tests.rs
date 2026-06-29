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
    // `SharedArrayBuffer` is still unmodeled and must remain an explicit
    // missing-stdlib blocker rather than being erased through `SmeltUnknown`.
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("const s = new SharedArrayBuffer(8);"), &mut ctx)?;
    assert_category(&errors, "SharedArrayBuffer", DiagnosticCategory::MissingStdlib)
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

/// `new Blob(parts, options)` lowers to a concrete record carrying the
/// dedicated `__smelt_blob` marker (and observable `type`), giving it a distinct
/// identity for `instanceof Blob` instead of erasing it to a shapeless value.
#[test]
fn new_blob_lowers_to_concrete_marker_record() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"const b = new Blob(["content"], { type: "text/plain" });"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            (&expr.kind, ctx.krate.types.get(expr.ty)),
            (ExprKind::DictLit(entries), Some(Type::Dict(_, _))) if entries.len() == 2
        )),
        "expected `new Blob(...)` to lower to a concrete record (DictLit + Dict type)",
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_blob"
        )),
        "expected the Blob record to carry the `__smelt_blob` marker key",
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "text/plain"
        )),
        "expected the Blob record to retain the options `type` string",
    );
    Ok(())
}

/// The `isBlob` shape — `typeof Blob !== 'undefined' && x instanceof Blob` over
/// an erased `unknown` — lowers cleanly: the support guard folds to a constant
/// and the `instanceof Blob` becomes a marker `InstanceOf` predicate.
#[test]
fn is_blob_predicate_shape_lowers() -> Result<(), String> {
    let source = ts!(r#"
export function isBlob(x: unknown): x is Blob {
  if (typeof Blob === 'undefined') {
    return false;
  }
  return x instanceof Blob;
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
                ExprKind::InstanceOf { class, .. } if ctx.krate.symbols.get(class) == Some("Blob")
            )),
        "expected `x instanceof Blob` to lower to a Blob InstanceOf predicate",
    );
    Ok(())
}

/// The boxed-object form `new Number(value)` lowers to a concrete record with a
/// dedicated `__smelt_number` marker plus the wrapped value, erased to
/// `SmeltUnknown` so its runtime `typeof` is `"object"` (not `"number"`). The
/// `Number(x)` coercion call and `Number` statics are a separate path and are
/// unaffected.
#[test]
fn new_boxed_number_lowers_to_concrete_marker_record() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const n = new Number(42);"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            (&expr.kind, ctx.krate.types.get(expr.ty)),
            (ExprKind::DictLit(entries), Some(Type::Dict(_, _))) if entries.len() == 2
        )),
        "expected `new Number(42)` to lower to a concrete record (DictLit + Dict type)",
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_number"
        )),
        "expected the boxed Number record to carry the `__smelt_number` marker key",
    );
    Ok(())
}

/// `Number(x)` coercion still lowers to a numeric value, not the boxed-object
/// record — adding the `new Number` object model must not capture the call form.
#[test]
fn number_coercion_call_stays_numeric() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!(r#"const n = Number("42");"#), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        !body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_number"
        )),
        "expected `Number(\"42\")` coercion not to build the boxed Number marker record",
    );
    Ok(())
}

/// `new Proxy(target, handler)` lowers transparently to its `target` operand —
/// a Proxy reports the identity of its target and `x instanceof Proxy` is
/// invalid JS, so this preserves behavior instead of inventing a wrong marker.
#[test]
fn new_proxy_lowers_to_target() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"const target = { key: "value" }; const p = new Proxy(target, {});"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::UnknownCast { .. })),
        "expected `new Proxy(target, handler)` not to route the target through SmeltUnknown",
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            (&expr.kind, ctx.krate.types.get(expr.ty)),
            (ExprKind::DictLit(entries), Some(Type::Dict(_, _))) if entries.len() == 1
        )),
        "expected the proxied target record (DictLit + Dict type) to flow through unchanged",
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
