//! Regression coverage for the es-toolkit whole-crate transpile-gate lowerings.
//!
//! Each test pins a general lowering rule cleared while unblocking es-toolkit's
//! first whole-crate transpile: the `Request` marker host object, zero-argument
//! `new RegExp()`, `for...in` / `Object.hasOwn` over function values carrying
//! attached properties, `x instanceof Array`, `String.raw` tagged templates, and
//! folding an always-present constructor-presence-guard ternary. They are
//! positive HIR-shape assertions plus one negative check per family where a
//! distinct alternative would be the regression to guard against.

use super::*;
use smelt_hir::{DictProjectionOp, Literal, UnknownKind};

/// Return whether any body in the crate contains an expression matching `pred`.
fn crate_has(ctx: &HirCtx, pred: impl Fn(&ExprKind) -> bool) -> bool {
    ctx.krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| pred(&expr.kind)))
}

/// Return whether any body in the crate contains a string literal `text`.
fn crate_has_string_literal(ctx: &HirCtx, text: &str) -> bool {
    ctx.krate.bodies.iter().any(|body| {
        body.exprs.iter().any(|expr| {
            matches!(&expr.kind, ExprKind::Literal(Literal::String(value)) if value == text)
        })
    })
}

// ---------------------------------------------------------------------------
// `Request` host identity at the erased boundary.
//
// es-toolkit's `isPlainObject` spec constructs `new Request('http://localhost')`
// only to probe host identity, and the probe reads the `__smelt_request`
// marker. `Request` now has a concrete runtime type, so construction produces
// a typed value and the marker is stamped by that type's `IntoSmeltUnknown`
// adapter when the value crosses into an `unknown` position -- which is where
// `isPlainObject` sees it. The guarantee is unchanged; only the place that
// carries it moved. This half asserts the lowering is typed; the marker itself
// is asserted where it is now emitted, in
// `smelt-codegen-rust`'s `fetch_types_tests::request_erasure_stamps_the_host_identity_marker`.
// ---------------------------------------------------------------------------

#[test]
fn request_construction_carries_no_marker_record() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function make(): unknown {
  return new Request('http://localhost');
}
"),
        &mut ctx,
    )?;
    ensure!(
        !crate_has_string_literal(&ctx, "__smelt_request"),
        "construction must not stamp a marker record any more",
    );
    ensure!(
        ctx.krate.bodies.iter().any(|body| body
            .exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::RequestNew { .. }))),
        "construction must lower to a concrete request",
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Zero-argument `new RegExp()`.
//
// Legal JavaScript for the empty pattern `/(?:)/`. es-toolkit's `merge` spec
// constructs it as a representative non-plain-object value.
// ---------------------------------------------------------------------------

#[test]
fn zero_argument_regexp_lowers_to_empty_pattern() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function make() {
  return new RegExp();
}
"),
        &mut ctx,
    )?;
    ensure!(
        crate_has(&ctx, |kind| matches!(
            kind,
            ExprKind::New { .. }
        )),
        "`new RegExp()` should still lower to a RegExp construction",
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// `for...in` and `Object.hasOwn` over a function value.
//
// `function fn() {}; for (const key in fn)` is the lodash-compat idiom
// es-toolkit's `defaultsDeep` spec pairs with `Object.hasOwn`. Smelt models a
// function as a property-less closure, so `for...in` iterates the empty list and
// `Object.hasOwn(fn, key)` folds to `false` — the sound projection for Smelt's
// function representation, and it compiles without casting the closure to a
// record (a closure has no `SmeltUnknown` view).
// ---------------------------------------------------------------------------

#[test]
fn for_in_over_function_value_iterates_empty() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function run(): Record<string, number> {
  function fn() {}
  const result: Record<string, number> = {};
  for (const key in fn) {
    if (Object.hasOwn(fn, key)) {
      result[key] = 1;
    }
  }
  return result;
}
"),
        &mut ctx,
    )?;
    // The function value must NOT be cast to a record for key projection: a
    // closure has no `SmeltUnknown` view, so that would emit non-compiling Rust.
    ensure!(!crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::DictProjection {
            op: DictProjectionOp::ForInKeys,
            ..
        }
    )));
    // `Object.hasOwn(fn, key)` folds to a constant `false`.
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::Literal(Literal::Bool(false))
    )));
    Ok(())
}

// ---------------------------------------------------------------------------
// `x instanceof Array`.
//
// Folds like `Array.isArray(x)`: a list/tuple operand is `true`, an erased
// operand resolves through the runtime `UnknownIs { Array }` probe.
// ---------------------------------------------------------------------------

#[test]
fn instanceof_array_on_list_folds_true() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function check(values: number[]): boolean {
  return values instanceof Array;
}
"),
        &mut ctx,
    )?;
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::Literal(Literal::Bool(true))
    )));
    // It must NOT emit a nominal `InstanceOf` node for the built-in `Array`.
    ensure!(!crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::InstanceOf { .. }
    )));
    Ok(())
}

#[test]
fn instanceof_array_on_unknown_probes_runtime() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function check(value: unknown): boolean {
  return value instanceof Array;
}
"),
        &mut ctx,
    )?;
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::UnknownIs {
            kind: UnknownKind::Array,
            ..
        }
    )));
    Ok(())
}

// ---------------------------------------------------------------------------
// `String.raw` tagged template.
//
// The one tagged template Smelt models: the raw text with substitutions
// interpolated. es-toolkit's `isPlainObject` spec uses the substitution-free
// ``String.raw`rawtemplate` ``.
// ---------------------------------------------------------------------------

#[test]
fn string_raw_tagged_template_lowers_to_raw_text() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function raw(): string {
  return String.raw`rawtemplate`;
}
"),
        &mut ctx,
    )?;
    ensure!(
        crate_has_string_literal(&ctx, "rawtemplate"),
        "`String.raw` should lower to its raw quasi text",
    );
    Ok(())
}

#[test]
fn non_string_raw_tagged_template_still_rejected() -> Result<(), String> {
    // Only `String.raw` is modeled; an arbitrary tag keeps failing to lower
    // rather than silently dropping its call-with-strings-array protocol.
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
declare function tag(strings: TemplateStringsArray): string;
export function use(): string {
  return tag`hello`;
}
"),
        &mut ctx,
    )?;
    ensure!(
        errors
            .iter()
            .any(|error| error.message.contains("tagged template literals are only supported for the `String.raw`")),
        "non-`String.raw` tagged templates should still be rejected: {errors:?}",
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Constructor-presence-guard ternary fold.
//
// `Ctor ? new Ctor(...) : fallback` where `Ctor` is a host constructor Smelt
// always models as present folds to its consequent, keeping the concrete shape
// instead of reconciling the mismatched branches through `SmeltUnknown`.
// ---------------------------------------------------------------------------

#[test]
fn typed_array_presence_guard_folds_to_consequent() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function make() {
  const typedArray = Uint8Array ? new Uint8Array([1]) : { buffer: [1] };
  return { value: typedArray };
}
"),
        &mut ctx,
    )?;
    // The always-true probe is folded away: no conditional node survives.
    ensure!(!crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::Conditional { .. }
    )));
    Ok(())
}
