//! Tests for the structured [`DiagnosticCategory`] assigned to lowering errors.
//!
//! Categorization happens where the diagnostic is raised, so tooling (the
//! library probes) can group failures without parsing message text.

use super::*;
use smelt_stdlib::DiagnosticCategory;

/// An unresolved reference to a still-unmodeled JS builtin is categorized as
/// missing stdlib. (`Reflect`/`Math`/`JSON` now resolve as namespace values, so
/// this uses `structuredClone`, which has no runtime implementation yet.)
#[test]
fn unresolved_builtin_is_missing_stdlib() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("const f = structuredClone;"), &mut ctx)?;
    assert_category(&errors, "structuredClone", DiagnosticCategory::MissingStdlib)
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
    // `WeakRef` is still unmodeled and must remain an explicit missing-stdlib
    // blocker rather than being erased through `SmeltUnknown` (`ArrayBuffer`,
    // `Blob`, boxed `Number`, `AbortController`, `WeakMap`/`WeakSet`/`DataView`/
    // `SharedArrayBuffer`/`File` are now modeled).
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("const s = new WeakRef({});"), &mut ctx)?;
    assert_category(&errors, "WeakRef", DiagnosticCategory::MissingStdlib)
}

/// `new WeakMap()` / `new WeakSet()` / `new DataView()` / `new SharedArrayBuffer()`
/// / `new File()` lower to concrete marker-bearing records so `instanceof` keeps a
/// distinct identity for each host type, rather than erasing to a shapeless
/// `SmeltUnknown::Object` (which the `isWeakMap`/`isWeakSet`/`isTypedArray`/`clone`
/// predicates inspect).
#[test]
fn new_marker_only_host_builtins_lower_to_concrete_marker_records() -> Result<(), String> {
    for (source, marker) in [
        ("const w = new WeakMap();", "__smelt_weakmap"),
        ("const w = new WeakSet();", "__smelt_weakset"),
        ("const d = new DataView(new ArrayBuffer(8));", "__smelt_dataview"),
        ("const s = new SharedArrayBuffer(8);", "__smelt_sharedarraybuffer"),
        (r#"const f = new File(["x"], "n.txt");"#, "__smelt_file"),
    ] {
        let mut ctx = HirCtx::new();
        let module_id = lower_ok(source, &mut ctx)?;
        let module = module(&ctx, module_id)?;
        let body = module_body(&ctx, module)?;
        ensure!(
            body.exprs.iter().any(|expr| matches!(
                &expr.kind,
                ExprKind::Literal(Literal::String(text)) if text == marker
            )),
            "expected `{source}` to carry the `{marker}` marker key",
        );
        ensure!(
            body.exprs.iter().any(|expr| matches!(
                (&expr.kind, ctx.krate.types.get(expr.ty)),
                (ExprKind::DictLit(_), Some(Type::Dict(_, _)))
            )),
            "expected `{source}` to lower to a concrete record (DictLit + Dict type)",
        );
    }
    Ok(())
}

/// `value instanceof WeakMap` (and the other marker-only host builtins) over an
/// erased `unknown` lowers to a marker `InstanceOf` predicate rather than failing
/// to resolve the target class, so the `isWeakMap`/`isWeakSet` predicates lower.
#[test]
fn instanceof_marker_only_host_builtin_lowers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function isWeakMap(value: unknown): boolean {
  return value instanceof WeakMap;
}
"#),
        &mut ctx,
    )?;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(
                &expr.kind,
                ExprKind::InstanceOf { class, .. }
                    if ctx.krate.symbols.get(*class) == Some("WeakMap")
            )),
        "expected `value instanceof WeakMap` to lower to a WeakMap InstanceOf predicate",
    );
    Ok(())
}

/// `value instanceof Boolean` / `String` / `Symbol` over an erased `any` lowers
/// to a boxed-wrapper marker `InstanceOf` predicate rather than failing to
/// resolve the target class, so the `isBoolean`/`isString`/`isSymbol` compat
/// predicates lower. A primitive value carries no marker, so the check is the
/// correct `false`; the leading `typeof` branch handles real primitives.
#[test]
fn instanceof_boxed_primitive_wrapper_lowers() -> Result<(), String> {
    for (target, source) in [
        ("Boolean", "export function f(value: any): boolean { return value instanceof Boolean; }"),
        ("String", "export function f(value: any): boolean { return value instanceof String; }"),
        ("Symbol", "export function f(value: any): boolean { return value instanceof Symbol; }"),
    ] {
        let mut ctx = HirCtx::new();
        lower_ok(source, &mut ctx)?;
        ensure!(
            ctx.krate
                .bodies
                .iter()
                .flat_map(|body| body.exprs.iter())
                .any(|expr| matches!(
                    &expr.kind,
                    ExprKind::InstanceOf { class, .. }
                        if ctx.krate.symbols.get(*class) == Some(target)
                )),
            "expected `value instanceof {target}` to lower to a {target} InstanceOf predicate",
        );
    }
    Ok(())
}

/// A bare reference to a global namespace object (`Math`, `JSON`, `Reflect`,
/// `Promise`, ...) used as a *value* lowers to a marker-bearing host-object
/// record (`__smelt_builtin_namespace`), not an unresolved identifier and not a
/// shapeless erased object, so `isPlainObject(JSON)` has a concrete argument.
#[test]
fn bare_builtin_namespace_value_lowers_to_marker_record() -> Result<(), String> {
    for (source, name) in [
        ("const m = Math;", "Math"),
        ("const j = JSON;", "JSON"),
        ("const r = Reflect;", "Reflect"),
    ] {
        let mut ctx = HirCtx::new();
        let module_id = lower_ok(source, &mut ctx)?;
        let module = module(&ctx, module_id)?;
        let body = module_body(&ctx, module)?;
        ensure!(
            body.exprs.iter().any(|expr| matches!(
                &expr.kind,
                ExprKind::Literal(Literal::String(text)) if text == "__smelt_builtin_namespace"
            )),
            "expected bare `{name}` value to carry the `__smelt_builtin_namespace` marker",
        );
        ensure!(
            body.exprs.iter().any(|expr| matches!(
                &expr.kind,
                ExprKind::Literal(Literal::String(text)) if text == name
            )),
            "expected bare `{name}` value to retain its source `name`",
        );
    }
    Ok(())
}

/// `Math.PI` and the other `Math.*` numeric constants fold to their concrete
/// IEEE-754 double literal (a value, not a callable), so a bare `Math.PI`
/// reference resolves instead of leaving an unresolved `Math` identifier.
#[test]
fn math_numeric_constants_fold_to_literals() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const p = Math.PI; const e = Math.E;"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::Float(value)) if (value - std::f64::consts::PI).abs() < 1e-12
        )),
        "expected `Math.PI` to fold to the PI double literal",
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::Float(value)) if (value - std::f64::consts::E).abs() < 1e-12
        )),
        "expected `Math.E` to fold to the E double literal",
    );
    Ok(())
}

/// `Reflect.ownKeys(record)` lowers to the same `DictProjection`/`Keys` operation
/// as `Object.keys(record)` (a concrete `List<string>`), since Smelt records
/// carry no non-enumerable or symbol keys.
#[test]
fn reflect_own_keys_lowers_like_object_keys() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("const r = { a: 1, b: 2 }; const keys = Reflect.ownKeys(r);"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::DictProjection {
                op: smelt_hir::DictProjectionOp::Keys,
                ..
            }
        )),
        "expected `Reflect.ownKeys(record)` to lower to a Keys DictProjection",
    );
    Ok(())
}

/// A bare ambient-global-object reference (`globalThis`/`global`/`self`) used as
/// a *value* lowers to a marker-bearing host-object record
/// (`__smelt_global_object`), not an unresolved identifier — covering the
/// es-toolkit `_internal/globalThis.ts` escaping-identity shim.
#[test]
fn bare_global_object_value_lowers_to_marker_record() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("const g: any = (typeof globalThis === 'object' && globalThis) || 1;"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_global_object"
        )),
        "expected bare `globalThis` value to carry the `__smelt_global_object` marker",
    );
    Ok(())
}

/// `typeof globalThis.Buffer !== 'undefined'` folds to a constant `false`: the
/// default deterministic non-Node profile models `Buffer` as absent, so the
/// `isBuffer` support guard short-circuits instead of resolving `Buffer` to a
/// fabricated value.
#[test]
fn typeof_absent_buffer_global_folds_false() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function bufferPresent(): boolean {
  return typeof Buffer !== 'undefined';
}
"#),
        &mut ctx,
    )?;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(
                &expr.kind,
                ExprKind::Literal(Literal::Bool(false))
            )),
        "expected `typeof Buffer !== 'undefined'` to fold to a constant false (absent global)",
    );
    Ok(())
}

/// `new AbortController()` lowers to a concrete, marker-bearing record carrying a
/// shared `signal` (itself a `__smelt_abortsignal` record with a mutable
/// `aborted` flag), giving it a distinct identity and shared cancellation state
/// instead of erasing it to a shapeless `SmeltUnknown`.
#[test]
fn new_abort_controller_lowers_to_concrete_marker_record() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const c = new AbortController();"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    for marker in [
        "__smelt_abortcontroller",
        "__smelt_abortsignal",
        "aborted",
        "signal",
    ] {
        ensure!(
            body.exprs.iter().any(|expr| matches!(
                &expr.kind,
                ExprKind::Literal(Literal::String(text)) if text == marker
            )),
            "expected the AbortController record to carry the `{marker}` key",
        );
    }
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            (&expr.kind, ctx.krate.types.get(expr.ty)),
            (ExprKind::DictLit(_), Some(Type::Dict(_, _)))
        )),
        "expected `new AbortController()` to lower to concrete records (DictLit + Dict type)",
    );
    Ok(())
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
