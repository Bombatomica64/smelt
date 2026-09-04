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

/// `new WeakMap()` / `new WeakSet()` lower to concrete marker-bearing records so
/// `instanceof` keeps a distinct identity for each host type, rather than erasing
/// to a shapeless `SmeltUnknown::Object` (which the `isWeakMap`/`isWeakSet`
/// predicates inspect).
///
/// These are the host objects with *no* retained structural surface. The ones that
/// do retain fields have their own construction: `File`/`Blob` through
/// `BlobFromParts` (see `new_file_lowers_to_blob_from_parts_with_name`) and the
/// byte buffers through `HostConstruct` (see
/// `new_byte_buffer_host_lowers_to_the_shared_host_constructor`).
#[test]
fn new_marker_only_host_builtins_lower_to_concrete_marker_records() -> Result<(), String> {
    for (source, marker) in [
        ("const w = new WeakMap();", "__smelt_weakmap"),
        ("const w = new WeakSet();", "__smelt_weakset"),
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
        ts!(r"
export function isWeakMap(value: unknown): boolean {
  return value instanceof WeakMap;
}
"),
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

/// `Buffer.from([...])` / `Buffer.alloc(n)` / `Buffer.concat([...])` / `new
/// Buffer(...)` lower to concrete marker-bearing byte-buffer records carrying the
/// `__smelt_buffer` identity marker, so `Buffer.isBuffer` / `instanceof Buffer`
/// resolve through that key instead of erasing the value to a shapeless dynamic.
///
/// The record used to be built inline as a `DictLit` here. It now goes through
/// `HostConstruct`, the one shared byte-buffer constructor that `new Uint8Array`,
/// `new ArrayBuffer` and the reflected `getPrototypeOf(x).constructor` path also
/// call, so a directly-built `Buffer` and a reflectively-built clone of it are
/// byte-for-byte the same shape — which es-toolkit's `clone`/`cloneDeep` buffer
/// specs compare with structural equality. The marker still comes from the shared
/// registry, keyed by the class name this node carries.
#[test]
fn buffer_constructors_lower_to_concrete_marker_records() -> Result<(), String> {
    for source in [
        "const b = Buffer.from([1, 2, 3]);",
        "const b = Buffer.alloc(4);",
        "const b = Buffer.concat([Buffer.from([1]), Buffer.from([2])]);",
        "const b = new Buffer([9, 8]);",
    ] {
        let mut ctx = HirCtx::new();
        let module_id = lower_ok(source, &mut ctx)?;
        let module = module(&ctx, module_id)?;
        let body = module_body(&ctx, module)?;
        ensure!(
            body.exprs.iter().any(|expr| matches!(
                &expr.kind,
                ExprKind::HostConstruct { class_name, .. } if class_name == "Buffer"
            )),
            "expected `{source}` to lower to a `Buffer` HostConstruct",
        );
        ensure!(
            smelt_stdlib::host_object_marker("Buffer") == Some("__smelt_buffer"),
            "the shared registry must still key `Buffer` to `__smelt_buffer`",
        );
    }
    Ok(())
}

/// `Buffer.isBuffer(x)` over an erased value lowers to a `Buffer` marker
/// `InstanceOf` predicate (the same identity check as `x instanceof Buffer`), so
/// es-toolkit's `isBuffer` returns `true` for real buffers instead of folding to
/// a constant `false`.
#[test]
fn buffer_is_buffer_lowers_to_instanceof_predicate() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function isBuf(value: unknown): boolean {
  return Buffer.isBuffer(value);
}
"),
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
                    if ctx.krate.symbols.get(*class) == Some("Buffer")
            )),
        "expected `Buffer.isBuffer(value)` to lower to a Buffer InstanceOf predicate",
    );
    Ok(())
}

/// `value instanceof Buffer` over an erased `unknown` lowers to a `Buffer` marker
/// `InstanceOf` predicate rather than failing to resolve the target class.
#[test]
fn instanceof_buffer_lowers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function isBuf(value: unknown): boolean {
  return value instanceof Buffer;
}
"),
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
                    if ctx.krate.symbols.get(*class) == Some("Buffer")
            )),
        "expected `value instanceof Buffer` to lower to a Buffer InstanceOf predicate",
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
/// `Promise`, ...) used as a *value* lowers to the *interned* host-object value
/// for that name, not an unresolved identifier and not a shapeless erased object,
/// so `isPlainObject(JSON)` has a concrete argument.
///
/// Interning is what makes `Blob === Blob` and `blob.constructor === Blob` hold:
/// JavaScript exposes one object per global name, whereas a record literal mints
/// a fresh identity on every construction.
#[test]
fn bare_builtin_namespace_value_lowers_to_interned_namespace() -> Result<(), String> {
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
                ExprKind::BuiltinNamespace { name: spelled } if spelled == name
            )),
            "expected bare `{name}` value to lower to the interned `{name}` namespace",
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
            ExprKind::Literal(Literal::Float(value)) if (value - std::f64::consts::PI).abs() < 1e-12_f64
        )),
        "expected `Math.PI` to fold to the PI double literal",
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::Float(value)) if (value - std::f64::consts::E).abs() < 1e-12_f64
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
                op: DictProjectionOp::Keys,
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
    // A chain-initialized `const` records a global-object ALIAS, so the marker
    // record materializes where the alias is READ rather than at the
    // declaration (which emits no local at all).
    let module_id = lower_ok(
        ts!(r"
const g: any = (typeof globalThis === 'object' && globalThis) || 1;
export function readGlobal(): unknown {
  return g;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "read_global")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_global_object"
        )),
        "expected bare `globalThis` value to carry the `__smelt_global_object` marker",
    );
    Ok(())
}

/// The full es-toolkit `_internal/globalThis.ts` detection chain lowers to the
/// global-object value, short-circuiting through the `typeof window === 'object'
/// && window` clause without resolving the absent `window` identifier.
///
/// JavaScript never evaluates a `&&` right operand whose `typeof` guard is
/// statically `false`, so the dead `window` reference must not be lowered (it is
/// absent in the non-DOM profile and would otherwise be an unresolved
/// identifier). The first clause (`globalThis`, present) is truthy, so the whole
/// `||` chain resolves to the global-object marker record.
#[test]
fn global_detection_chain_short_circuits_absent_window() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const globalThis_: any =
  (typeof globalThis === 'object' && globalThis) ||
  (typeof window === 'object' && window) ||
  (typeof self === 'object' && self) ||
  (typeof global === 'object' && global) ||
  1;
export function readGlobal(): unknown {
  return globalThis_;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "read_global")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_global_object"
        )),
        "expected the global-detection chain to fold to the `__smelt_global_object` value",
    );
    Ok(())
}

/// A module-level `const` initialized by the global-detection chain records a
/// global-object ALIAS, and an importing module's binding of it is the global
/// object too — so `globalThis.Buffer.isBuffer(x)` through an imported shim
/// resolves against the modeled global instead of reading a field off a fresh
/// empty record.
///
/// This is the shape every "universal global" shim ships (es-toolkit's
/// `_internal/globalThis.ts` re-exports the chain-initialized binding under the
/// name `globalThis`), so recognizing only the bare spelling made the answer
/// depend on which spelling the source happened to use.
#[test]
fn imported_global_object_alias_resolves_modeled_globals() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r"
const globalThis_: any =
  (typeof globalThis === 'object' && globalThis) ||
  (typeof self === 'object' && self) ||
  1;
export { globalThis_ as globalThis };
"),
        "./shim.ts",
        &mut ctx,
    )?;
    let module_id = lower_path_ok(
        ts!(r"
import { globalThis } from './shim';
export function isBuffer(x: unknown): boolean {
  return globalThis.Buffer.isBuffer(x);
}
"),
        "./use.ts",
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "is_buffer")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::InstanceOf { .. })),
        "expected `globalThis.Buffer.isBuffer(x)` through an imported alias to lower to the Buffer identity check",
    );
    ensure!(
        !body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_global_object"
        )),
        "the alias must resolve the member statically, not materialize the global object",
    );
    Ok(())
}

/// `typeof <erased> !== 'undefined'` must emit a runtime tag test, never a
/// folded constant.
///
/// An `unknown` operand pins no `typeof` answer, so the static matcher's `false`
/// ("this type is not `undefined`") turned the whole comparison into the
/// constant `true` — a presence guard that always passed. The rule is the one
/// `typeof_expression` already documents: only fold when the type pins a single
/// spelling.
#[test]
fn typeof_undefined_on_erased_operand_stays_a_runtime_test() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export function isPresent(x: unknown): boolean {
  return typeof x !== 'undefined';
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "is_present")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::UnknownIs {
                kind: smelt_hir::UnknownKind::Undefined,
                ..
            }
        )),
        "expected an undefined-tag runtime test for an erased operand",
    );
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::Literal(Literal::Bool(_)))),
        "the presence guard must not fold to a constant",
    );
    Ok(())
}

/// A concrete operand still folds: `typeof 1 === 'undefined'` has one answer, so
/// the constant is the honest lowering and the runtime test would be waste.
#[test]
fn typeof_undefined_on_concrete_operand_still_folds() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export function check(x: number): boolean {
  return typeof x === 'undefined';
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "check")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::Bool(false))
        )),
        "expected a concrete numeric operand to fold its `typeof` presence guard",
    );
    Ok(())
}

/// A zero-parameter `function` value in an object literal is only readable as a
/// getter when its body is a single `return`. A generator (and any other body)
/// is a function the property HOLDS, so it must erase to a function value rather
/// than to `null` — es-toolkit's JSON validators ask `typeof value ===
/// 'function'` about exactly this shape.
#[test]
fn object_property_generator_stays_a_function_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("const table: any = { a: function* () {}, b: function () { const x = 1; } };"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let closures = body
        .exprs
        .iter()
        .filter(|expr| matches!(&expr.kind, ExprKind::Closure(_)))
        .count();
    ensure_eq!(closures, 2);
    Ok(())
}

/// An ordinary `||` fallback that is not a global-detection chain is left to
/// normal lowering and never folds to the global-object value. This guards the
/// detection-chain folder against over-matching unrelated `||` shapes.
#[test]
fn ordinary_or_fallback_is_not_folded_to_global() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("function pick(a: string, b: string): string { return a || b; }"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        !body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_global_object"
        )),
        "an ordinary `||` fallback must not fold to the global-object value",
    );
    Ok(())
}

/// `typeof Buffer !== 'undefined'` folds to a constant `true`: `Buffer` is now a
/// modeled host object (concrete byte-buffer record with a working `instanceof`
/// / `Buffer.isBuffer` identity), so it is reported *present*. es-toolkit's
/// `isBuffer` support guard then proceeds to the real identity check instead of
/// short-circuiting to a constant `false`.
#[test]
fn typeof_present_buffer_global_folds_true() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function bufferPresent(): boolean {
  return typeof Buffer !== 'undefined';
}
"),
        &mut ctx,
    )?;
    ensure!(
        ctx.krate
            .bodies
            .iter()
            .flat_map(|body| body.exprs.iter())
            .any(|expr| matches!(
                &expr.kind,
                ExprKind::Literal(Literal::Bool(true))
            )),
        "expected `typeof Buffer !== 'undefined'` to fold to a constant true (present modeled host object)",
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

/// The byte-backed host constructors (`ArrayBuffer`, `SharedArrayBuffer`,
/// `DataView`) lower to `ExprKind::HostConstruct`, which runs the *same* runtime
/// constructor the reflected `Object.getPrototypeOf(x).constructor` path calls.
///
/// A shared constructor is what makes a directly built record indistinguishable
/// from a reflectively built one, and it is where the record gets real byte
/// storage — an identity-only marker record could not answer `slice(0)`,
/// `byteLength`, `byteOffset`, or an indexed element read.
#[test]
fn new_byte_buffer_host_lowers_to_the_shared_host_constructor() -> Result<(), String> {
    for (source, class_name, arg_count) in [
        (ts!("const buf = new ArrayBuffer(8);"), "ArrayBuffer", 1),
        (
            ts!("const buf = new SharedArrayBuffer(8);"),
            "SharedArrayBuffer",
            1,
        ),
        (
            ts!("const view = new DataView(new ArrayBuffer(8), 1, 2);"),
            "DataView",
            3,
        ),
    ] {
        let mut ctx = HirCtx::new();
        let module_id = lower_ok(source, &mut ctx)?;
        let module = module(&ctx, module_id)?;
        let body = module_body(&ctx, module)?;
        ensure!(
            body.exprs.iter().any(|expr| matches!(
                &expr.kind,
                ExprKind::HostConstruct { class_name: spelled, args }
                    if spelled == class_name && args.len() == arg_count
            )),
            "expected `{source}` to lower to a `{class_name}` HostConstruct with {arg_count} argument(s)",
        );
    }
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

/// `new Blob(parts, options)` lowers to a `BlobFromParts` construction (the
/// `smelt_blob_record_from_parts` runtime helper builds the marker record with
/// real `type`/`size`/`content`), retaining the spelled options `type` string,
/// so `instanceof Blob` keeps a distinct identity and field reads observe real
/// values instead of a shapeless erased object.
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
            (
                ExprKind::BlobFromParts {
                    name: None,
                    last_modified: None,
                    ..
                },
                Some(Type::Unknown)
            )
        )),
        "expected `new Blob(...)` to lower to a BlobFromParts construction",
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
    let source = ts!(r"
export function isBlob(x: unknown): x is Blob {
  if (typeof Blob === 'undefined') {
    return false;
  }
  return x instanceof Blob;
}
");
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

/// The `isFile` predicate shape (`typeof File === 'undefined'` presence guard
/// plus `x instanceof File`) lowers like the `isBlob` shape: the presence guard
/// folds to a constant and the `instanceof` resolves through the modeled
/// `__smelt_file` marker instead of aborting on an unmodeled class.
#[test]
fn is_file_predicate_shape_lowers() -> Result<(), String> {
    let source = ts!(r"
export function isFile(x: unknown): x is File {
  if (typeof File === 'undefined') {
    return false;
  }
  return x instanceof File;
}
");
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
                ExprKind::InstanceOf { class, .. } if ctx.krate.symbols.get(class) == Some("File")
            )),
        "expected `x instanceof File` to lower to a File InstanceOf predicate",
    );
    Ok(())
}

/// `new File(parts, name, options)` lowers to a `BlobFromParts` construction
/// that retains the spelled `name` and `lastModified` expressions, so the
/// modeled record observes real `.name`/`.type`/`.size`/`.lastModified` reads
/// (the `clone`/`cloneDeepWith` File round-trip).
#[test]
fn new_file_lowers_to_blob_from_parts_with_name() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"const f = new File(['content'], 'file.txt', { type: 'text/plain', lastModified: 3 });"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            (&expr.kind, ctx.krate.types.get(expr.ty)),
            (
                ExprKind::BlobFromParts {
                    name: Some(_),
                    last_modified: Some(_),
                    ..
                },
                Some(Type::Unknown)
            )
        )),
        "expected `new File(...)` to lower to BlobFromParts retaining name and lastModified",
    );
    Ok(())
}

/// A `Blob` constructor options `type` spelled as a non-literal expression
/// (`{ type: source.type }`, the `cloneDeepWith` Blob clone arm) is retained on
/// the modeled record instead of collapsing to the empty MIME default.
#[test]
fn new_blob_retains_dynamic_options_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export function cloneBlob(source: Blob): Blob {
  return new Blob([source], { type: source.type });
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    let retained_dynamic_type = ctx.krate.bodies.iter().any(|body| {
        body.exprs.iter().any(|expr| {
            matches!(
                &expr.kind,
                ExprKind::BlobFromParts { blob_type, name: None, .. }
                    if !matches!(
                        body.exprs.get(blob_type.0 as usize).map(|blob_type_expr| &blob_type_expr.kind),
                        Some(ExprKind::Literal(Literal::String(text))) if text.is_empty()
                    )
            )
        })
    });
    ensure!(
        retained_dynamic_type,
        "expected `new Blob([source], {{ type: source.type }})` to retain the dynamic type expression",
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
    let source = ts!(r"
class Object {
  value: number = 1;
}
export function make(): Object {
  return new Object();
}
");
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

/// Every typed-array view constructor — including the BigInt-backed
/// `BigInt64Array` / `BigUint64Array` that the previous inline recognizer
/// omitted and which aborted the es-toolkit build as `unresolved class
/// BigUint64Array` — lowers to the one shared byte-buffer `HostConstruct` keyed by
/// its own class name.
///
/// This replaces the old numeric-list model, where all eleven views shared one
/// `Vec<f64>`: every one of them then reported `Object.prototype.toString` tag
/// `[object Array]` with the *byte* count as its `length`, so a `Float32Array` and
/// a `Float64Array` over the same eight bytes were indistinguishable. Naming the
/// class here is what lets the runtime resolve the view's marker and its element
/// type from the shared registry.
#[test]
fn typed_array_constructors_lower_to_byte_buffer_host_constructs() -> Result<(), String> {
    for name in smelt_stdlib::TYPED_ARRAY_CLASS_NAMES {
        let source = format!("const value = new {name}(8);");
        let mut ctx = HirCtx::new();
        let module_id = lower_ok(&source, &mut ctx)?;
        let module = module(&ctx, module_id)?;
        let body = module_body(&ctx, module)?;
        ensure!(
            body.exprs.iter().any(|expr| matches!(
                &expr.kind,
                ExprKind::HostConstruct { class_name, .. } if class_name == name
            )),
            "expected `new {name}(8)` to lower to a `{name}` HostConstruct",
        );
        // The registry must know the view, or the runtime cannot resolve either
        // its identity marker or the element width that makes `length` the
        // element count.
        ensure!(
            smelt_stdlib::host_object_marker(name).is_some()
                && smelt_stdlib::typed_array_element(name).is_some(),
            "expected `{name}` to carry a registry marker and element type",
        );
    }
    Ok(())
}

/// `new Uint8Array([1, 2, 3])` passes its element list straight to the shared
/// byte-buffer constructor, which encodes each element at the view's own width.
///
/// The element list is still an ordinary `ListLit` — only its *destination*
/// changed: it is now a `HostConstruct` argument rather than the constructed value
/// itself, which is what makes `new Uint8Array([1, 2, 3]).buffer` and
/// `Object.prototype.toString.call(...)` answer like a real view.
#[test]
fn typed_array_from_literal_passes_its_elements_to_the_host_construct() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const value = new Uint8Array([1, 2, 3]);"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let host_args = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            ExprKind::HostConstruct { class_name, args } if class_name == "Uint8Array" => {
                Some(args.clone())
            }
            _ => None,
        })
        .ok_or_else(|| "expected a `Uint8Array` HostConstruct".to_owned())?;
    let [elements] = host_args.as_slice() else {
        return Err("expected exactly one constructor argument".to_owned());
    };
    let elements = body
        .exprs
        .get(usize::try_from(elements.0).unwrap_or(usize::MAX))
        .ok_or_else(|| "constructor argument is missing".to_owned())?;
    ensure!(
        matches!(
            (&elements.kind, ctx.krate.types.get(elements.ty)),
            (ExprKind::ListLit(_), Some(Type::List(_)))
        ),
        "expected the element form to pass a list literal to the host construct",
    );
    Ok(())
}

/// A typed-array constructor used as a bare *value* (an `instanceof` /
/// `toBeInstanceOf` target, or a helper argument) resolves to a `Type::Class`
/// reference instead of failing as an `unresolved identifier`, mirroring the
/// `Date` bare-value model. This is the fix for the `unresolved identifier
/// Uint8Array` blocker in `clone.spec.ts`.
#[test]
fn typed_array_name_resolves_as_bare_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("function ctor(): unknown { return Uint8Array; }"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "ctor")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            ctx.krate.types.get(expr.ty),
            Some(Type::Class { name, .. }) if ctx.krate.symbols.get(*name) == Some("Uint8Array")
        )),
        "expected a bare `Uint8Array` to resolve to a Uint8Array class reference",
    );
    Ok(())
}

/// `x instanceof Uint8Array` resolves through the view's identity marker like
/// every other byte-backed host object.
///
/// It used to fold to a *constant* derived from the static type — `true` for any
/// `number[]`, `false` for anything else — because the numeric-list model left no
/// view identity to test at runtime, so a plain `number[]` was reported as a typed
/// array and a real erased view was not. Now the erased operand emits a real
/// `InstanceOf` (the marker probe), and only a statically concrete operand that
/// cannot carry a marker still folds to `false`.
#[test]
fn instanceof_typed_array_resolves_through_the_view_marker() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        "function check(x: unknown): boolean { return x instanceof Uint8Array; }",
        &mut ctx,
    )?;
    let erased_module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, erased_module, "check")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::InstanceOf { class, .. }
                if ctx.krate.symbols.get(*class) == Some("Uint8Array")
        )),
        "expected an erased `x instanceof Uint8Array` to emit a marker InstanceOf",
    );
    // A plain `number[]` used to fold to `true` here, which was simply wrong: a
    // `number[]` is not a `Uint8Array`, and answering `true` is what made
    // `isTypedArray([1, 2, 3])` claim a plain array was a view. A concrete
    // non-class operand now raises the same honest blocker every other host-object
    // `instanceof` raises, instead of inventing an answer.
    let mut list_ctx = HirCtx::new();
    let errors = lowering_errors(
        "function check(x: number[]): boolean { return x instanceof Uint8Array; }",
        &mut list_ctx,
    )?;
    assert_category(
        &errors,
        "instanceof requires a concrete class-typed left operand",
        DiagnosticCategory::UnsupportedLowering,
    )
}

/// The es-toolkit `isTypedArray` body (`ArrayBuffer.isView(x) && !(x instanceof
/// DataView)`) lowers without a blocker, exercising the typed-array
/// `ArrayBuffer.isView` and `instanceof DataView` paths together.
#[test]
fn is_typed_array_predicate_body_lowers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function isTypedArray(x: unknown): boolean {
  return ArrayBuffer.isView(x) && !(x instanceof DataView);
}
"),
        &mut ctx,
    )?;
    Ok(())
}

/// A bare `Proxy` reference used as a value (`if (Proxy)`, `isFunction(Proxy)`,
/// a `Proxy` entry in a value table) lowers to a first-class closure — the
/// transparent-construction value form matching `new Proxy(target, handler)`
/// resolving to `target` — instead of an unresolved identifier.
#[test]
fn bare_proxy_value_lowers_to_transparent_constructor_closure() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const p = Proxy;"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::Closure(_))),
        "expected bare `Proxy` to lower to a first-class closure value",
    );
    Ok(())
}

/// A bare `Intl` reference used as a value lowers to the shared interned
/// builtin-namespace value, like `Math`/`JSON`/`Reflect`, so namespace-identity
/// probes observe a host object rather than an unresolved identifier.
#[test]
fn bare_intl_namespace_value_lowers_to_interned_namespace() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const i = Intl;"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::BuiltinNamespace { name } if name == "Intl"
        )),
        "expected bare `Intl` value to lower to the interned `Intl` namespace",
    );
    Ok(())
}

/// `new Intl.<Constructor>(...)` (the ECMA-402 namespace constructors) lowers
/// to a marker-only host-object record through the shared registry, keyed by
/// the full qualified path, so `isPlainObject(new Intl.Locale('en'))` observes
/// a host identity instead of an unresolved `Intl`.
#[test]
fn new_intl_namespace_constructor_lowers_to_marker_record() -> Result<(), String> {
    for (source, marker) in [
        ("const l = new Intl.Locale('en');", "__smelt_intl_locale"),
        ("const c = new Intl.Collator('en');", "__smelt_intl_collator"),
        (
            "const n = new Intl.NumberFormat('en');",
            "__smelt_intl_numberformat",
        ),
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
            "expected `{source}` to stamp the `{marker}` host marker",
        );
    }
    Ok(())
}

/// A `new Intl.<Member>()` spelling for an unmodeled member falls through to
/// the ordinary member-callee construction (an `ExprKind::New` naming the
/// member, resolved or rejected by the later class-resolution passes) instead
/// of being silently stamped with an Intl host marker.
#[test]
fn new_unmodeled_intl_member_falls_through_without_marker() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const x = new Intl.Nonexistent('en');"), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        !body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text.starts_with("__smelt_intl")
        )),
        "expected `new Intl.Nonexistent(...)` to keep the ordinary construction path",
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::New { class, .. }
                if ctx.krate.symbols.get(*class) == Some("Nonexistent")
        )),
        "expected `new Intl.Nonexistent(...)` to lower as an ordinary member construction",
    );
    Ok(())
}

/// `encodeURI(value)` lowers to the dedicated URI percent-encoding IR op, and
/// the bare `encodeURI` value form lowers to a first-class closure running the
/// same op, instead of an unresolved identifier.
#[test]
fn encode_uri_call_and_value_forms_lower_to_uri_encode() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(ts!("const s = encodeURI('a b');"), &mut ctx)?;
    let call_module = module(&ctx, module_id)?;
    let call_body = module_body(&ctx, call_module)?;
    ensure!(
        call_body
            .exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::UriEncode { .. })),
        "expected `encodeURI('a b')` to lower to the UriEncode op",
    );

    let mut value_ctx = HirCtx::new();
    let value_module_id = lower_ok(ts!("const f = encodeURI;"), &mut value_ctx)?;
    let value_module = module(&value_ctx, value_module_id)?;
    let value_body = module_body(&value_ctx, value_module)?;
    ensure!(
        value_body
            .exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::Closure(_))),
        "expected bare `encodeURI` to lower to a first-class closure value",
    );
    Ok(())
}

/// A bare `setTimeout` reference (e.g. `const original = globalThis.setTimeout;`
/// before mocking) lowers to a first-class closure over the shared timer op,
/// instead of an unresolved identifier. The `globalThis.` spelling normalizes
/// to the bare name first (see `global_alias_member_read`).
#[test]
fn bare_set_timeout_value_lowers_to_timer_closure() -> Result<(), String> {
    for source in [
        ts!("const st = setTimeout;"),
        ts!("const st = globalThis.setTimeout;"),
    ] {
        let mut ctx = HirCtx::new();
        let module_id = lower_ok(source, &mut ctx)?;
        let module = module(&ctx, module_id)?;
        let body = module_body(&ctx, module)?;
        ensure!(
            body.exprs
                .iter()
                .any(|expr| matches!(&expr.kind, ExprKind::Closure(_))),
            "expected `{source}` to lower to a first-class timer closure",
        );
    }
    Ok(())
}

/// `Object.prototype.toString.call(x)` lowers to the dedicated
/// `"[object Tag]"` probe op rather than mis-reading `toString`/`call` as
/// fields of the prototype sentinel.
#[test]
fn object_prototype_to_string_call_lowers_to_tag_probe() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("export function tag(x: unknown): string { return Object.prototype.toString.call(x); }"),
        &mut ctx,
    )?;
    let has_probe = ctx.krate.bodies.iter().any(|body| {
        body.exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::ObjectToStringTag { .. }))
    });
    ensure!(
        has_probe,
        "expected `Object.prototype.toString.call(x)` to lower to ObjectToStringTag",
    );
    Ok(())
}

/// `new Error(message, { cause })` (ES2022 options) retains the `cause` on the
/// error record alongside the `__smelt_error` marker and `message`, and
/// `new AggregateError(errors, message, options?)` retains the leading
/// `errors` list, instead of rejecting the options argument.
#[test]
fn error_options_constructor_retains_cause_and_aggregate_errors() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("const e = new Error('boom', { cause: 'root' });"),
        &mut ctx,
    )?;
    let error_module = module(&ctx, module_id)?;
    let error_body = module_body(&ctx, error_module)?;
    for key in ["__smelt_error", "cause"] {
        ensure!(
            error_body.exprs.iter().any(|expr| matches!(
                &expr.kind,
                ExprKind::Literal(Literal::String(text)) if text == key
            )),
            "expected `new Error(msg, {{ cause }})` to store the `{key}` key",
        );
    }

    let mut aggregate_ctx = HirCtx::new();
    let aggregate_module_id = lower_ok(
        ts!("const e = new AggregateError([new Error('a')], 'many', { cause: 'root' });"),
        &mut aggregate_ctx,
    )?;
    let aggregate_module = module(&aggregate_ctx, aggregate_module_id)?;
    let aggregate_body = module_body(&aggregate_ctx, aggregate_module)?;
    for key in ["__smelt_error", "cause", "errors"] {
        ensure!(
            aggregate_body.exprs.iter().any(|expr| matches!(
                &expr.kind,
                ExprKind::Literal(Literal::String(text)) if text == key
            )),
            "expected `new AggregateError(errors, msg, {{ cause }})` to store the `{key}` key",
        );
    }
    Ok(())
}

/// A non-literal Error options argument stays an honest blocker: whether a
/// `cause` is attached depends on `"cause" in options`, which a general static
/// rule can only answer for a literal spelling.
#[test]
fn error_options_non_literal_argument_stays_a_blocker() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("const opts = { cause: 'root' }; const e = new Error('boom', opts);"),
        &mut ctx,
    )?;
    ensure!(
        errors.iter().any(|error| error
            .message
            .contains("Error constructor options must be an object literal")),
        "expected a clear blocker for non-literal Error options, got {errors:?}",
    );
    Ok(())
}
