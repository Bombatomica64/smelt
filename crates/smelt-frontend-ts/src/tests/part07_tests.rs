//! Ambient global-object lowering tests (plan Phase 1).
//!
//! Covers compile-time erasure of global feature probes, namespace-path
//! normalization through `globalThis`/aliases, and the conservative denylist
//! that keeps erasure from running when global identity, dynamic access, or
//! shadowing is observable.

use super::*;

/// Return whether any body in the crate contains a folded boolean literal.
fn crate_has_bool_literal(ctx: &HirCtx, value: bool) -> bool {
    ctx.krate.bodies.iter().any(|body| {
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(found)) if found == value))
    })
}

/// Return whether any body in the crate contains an expression matching `pred`.
fn crate_has_expr(ctx: &HirCtx, pred: impl Fn(&ExprKind) -> bool) -> bool {
    ctx.krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| pred(&expr.kind)))
}

#[test]
fn folds_typeof_globalthis_not_undefined_to_true() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function supported(): boolean {
  return typeof globalThis !== "undefined";
}
"#),
        &mut ctx,
    )?;
    ensure!(crate_has_bool_literal(&ctx, true));
    Ok(())
}

#[test]
fn folds_typeof_globalthis_equals_object_to_true() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function isObject(): boolean {
  return typeof globalThis === "object";
}
"#),
        &mut ctx,
    )?;
    ensure!(crate_has_bool_literal(&ctx, true));
    Ok(())
}

#[test]
fn folds_typeof_global_alias_existence_probe() -> Result<(), String> {
    // `global` is a recognized non-DOM alias just like `globalThis`.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function hasGlobal(): boolean {
  return typeof global !== "undefined";
}
"#),
        &mut ctx,
    )?;
    ensure!(crate_has_bool_literal(&ctx, true));
    Ok(())
}

#[test]
fn folds_present_member_in_globalthis_to_true() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function hasMap(): boolean {
  return "Map" in globalThis;
}
"#),
        &mut ctx,
    )?;
    ensure!(crate_has_bool_literal(&ctx, true));
    Ok(())
}

#[test]
fn folds_absent_member_in_globalthis_to_false() -> Result<(), String> {
    // `window` is DOM-only, so it is absent in the non-DOM profile.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function hasWindow(): boolean {
  return "window" in globalThis;
}
"#),
        &mut ctx,
    )?;
    ensure!(crate_has_bool_literal(&ctx, false));
    Ok(())
}

#[test]
fn keeps_unknown_member_in_globalthis_probe_unfolded() -> Result<(), String> {
    // An unmodeled member must not fold; its presence cannot be decided. Inside a
    // function body the receiver is the ambient global, which has no concrete
    // shape, so this stays an honest unsupported blocker rather than guessing.
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
export function hasFragment(): boolean {
  return "DocumentFragment" in globalThis;
}
"#),
        &mut ctx,
    )?;
    ensure!(!errors.is_empty());
    Ok(())
}

#[test]
fn normalizes_globalthis_namespace_call() -> Result<(), String> {
    // `globalThis.Object.keys(x)` must lower exactly like `Object.keys(x)`.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function keysOf(x: Record<string, number>): string[] {
  return globalThis.Object.keys(x);
}
"#),
        &mut ctx,
    )?;
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::DictProjection {
            op: DictProjectionOp::Keys | DictProjectionOp::ForInKeys,
            ..
        }
    )));
    Ok(())
}

#[test]
fn normalizes_global_alias_namespace_call() -> Result<(), String> {
    // `const g = globalThis; g.Array.isArray(value)` lowers like `Array.isArray`.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function isArr(value: unknown): boolean {
  const g = globalThis;
  return g.Array.isArray(value);
}
"#),
        &mut ctx,
    )?;
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::UnknownIs {
            kind: smelt_hir::UnknownKind::Array,
            ..
        }
    )));
    Ok(())
}

#[test]
fn normalizes_globalthis_member_read_to_bare_value() -> Result<(), String> {
    // `globalThis.Math` reads the same concrete value as bare `Math`.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function roundTrip(x: number): number {
  return globalThis.Math.floor(x);
}
"#),
        &mut ctx,
    )?;
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::NumericRound {
            op: NumericRoundOp::Floor,
            ..
        }
    )));
    Ok(())
}

#[test]
fn folds_exported_const_global_probe() -> Result<(), String> {
    // Exported consts fold through a dedicated literal evaluator; the probe must
    // fold there too instead of becoming an unresolved-const blocker.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export const supported: boolean = typeof globalThis !== "undefined";
export const hasMap: boolean = "Map" in globalThis;
"#),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn does_not_treat_imported_globalthis_as_ambient() -> Result<(), String> {
    // es-toolkit imports its own `globalThis` shim. That binding is an ordinary
    // value, not the ambient global, so `globalThis.Buffer` must NOT normalize to
    // bare `Buffer`; it stays an ordinary (unknown) member access on the import.
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r#"
import { globalThis } from "./shim";
export function bufferish(x: unknown): boolean {
  return globalThis.Buffer != null;
}
"#),
        "main.ts",
        &mut ctx,
    )?;
    // The imported receiver flows through unknown member access, never producing
    // a bare-`Buffer` resolution failure.
    Ok(())
}

#[test]
fn does_not_erase_typeof_of_user_local_named_global() -> Result<(), String> {
    // A user local named `self` shadows the ambient alias, so its `typeof` probe
    // is NOT folded to the present-object answer.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function check(self: string): boolean {
  return typeof self === "object";
}
"#),
        &mut ctx,
    )?;
    // A shadowed `self: string` is statically a string, so the probe folds to
    // `false` (string is not "object"), never to the ambient-global `true`.
    ensure!(crate_has_bool_literal(&ctx, false));
    ensure!(!crate_has_bool_literal(&ctx, true));
    Ok(())
}

#[test]
fn lowers_for_loop_with_comma_sequence_update() -> Result<(), String> {
    // A C-style `for` update clause may be a comma sequence of increments
    // (`step++, resultIndex++`); each sub-update must lower into its own
    // loop-body assignment, mirroring es-toolkit `sampleSize`.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function run(size: number, length: number): number[] {
  const result: number[] = [];
  for (let step = length - size, resultIndex = 0; step < length; step++, resultIndex++) {
    result[resultIndex] = step;
  }
  return result;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_switch_case_label_from_folded_string_const() -> Result<(), String> {
    // Switch case labels may reference module string constants
    // (`case stringTag:` where `const stringTag = '[object String]'`); these
    // fold to the same literal through the exported-const folder.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const stringTag = '[object String]';
const numberTag = '[object Number]';

function classify(tag: string): number {
  switch (tag) {
    case stringTag:
      return 1;
    case numberTag:
      return 2;
    default:
      return 0;
  }
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn keeps_computed_global_access_unfolded() -> Result<(), String> {
    // A dynamic computed key is on the erasure denylist: it must not fold and
    // must not normalize, since the key is not statically known.
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
export function dyn(key: string): unknown {
  return globalThis[key];
}
"#),
        &mut ctx,
    )?;
    ensure!(!errors.is_empty());
    Ok(())
}

#[test]
fn lowers_zero_parameter_named_function_array_callback() -> Result<(), String> {
    // JavaScript callbacks adapt arity at the call site: a named function
    // declaring no parameters (`values.map(stubTrue)`, the lodash-style stub
    // shape) simply ignores the supplied `(value, index, array)` arguments.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function stubTrue(): boolean {
  return true;
}
export function run(values: number[]): boolean[] {
  return values.map(stubTrue);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_zero_parameter_named_function_array_predicate() -> Result<(), String> {
    // The predicate path (`filter`/`some`/`every`) accepts the same
    // zero-parameter named callback shape as `map`.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function stubFalse(): boolean {
  return false;
}
export function run(values: number[]): number[] {
  return values.filter(stubFalse);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_zero_parameter_local_arrow_array_callback() -> Result<(), String> {
    // A zero-parameter *local* callback binding follows the same JavaScript
    // arity rule as named items: extra supplied arguments are ignored.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function run(values: string[]): number[] {
  const localStub = () => 42;
  return values.map(localStub);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_named_array_callback_with_optional_parameter_tail() -> Result<(), String> {
    // A named callback declaring *more* parameters than the receiver supplies
    // (`xs.map(orderBy)` with a four-parameter `orderBy`) receives `undefined`
    // for the unsupplied optional tail, so the wrapper truncates to the
    // receiver's supplied arity instead of rejecting the reference.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function withTail(value: number, index?: number, list?: number[], guard?: number): number {
  return value + (guard ?? 0);
}
export function run(values: number[]): number[] {
  return values.map(withTail);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
