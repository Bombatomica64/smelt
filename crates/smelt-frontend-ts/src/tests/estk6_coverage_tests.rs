//! Additive regression coverage for established TypeScript lowering rules.
//!
//! These tests pin currently-correct behavior for general lowering rules that
//! the conversion campaign relies on but that previously had thin or no
//! dedicated frontend coverage: the four equality kinds, bit-shift operators,
//! string/number/collection method lowerings, and optional chaining. They are
//! intentionally positive (assert the produced HIR shape) plus a few negative
//! checks where a distinct alternative would be the regression to guard
//! against. None of them assert behavior that does not already work.

use super::*;

/// Return whether the module body contains an expression matching `pred`.
fn module_body_has(
    ctx: &HirCtx,
    module_id: ModuleId,
    pred: impl Fn(&ExprKind) -> bool,
) -> Result<bool, String> {
    let module = module(ctx, module_id)?;
    let body = module_body(ctx, module)?;
    Ok(body.exprs.iter().any(|expr| pred(&expr.kind)))
}

/// Return whether any body in the crate contains an expression matching `pred`.
fn crate_has(ctx: &HirCtx, pred: impl Fn(&ExprKind) -> bool) -> bool {
    ctx.krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| pred(&expr.kind)))
}

// ---------------------------------------------------------------------------
// Equality kinds.
//
// `===`/`!==` keep JS reference semantics (`JsStrictEq`/`JsStrictNotEq`) while
// `==`/`!=` lower structurally (`Eq`/`NotEq`). These four ops are distinct and
// must not collapse into one another.
// ---------------------------------------------------------------------------

#[test]
fn triple_equals_lowers_to_js_strict_eq() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const a = 1;
const b = 2;
const same = a === b;
"),
        &mut ctx,
    )?;
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::BinOp {
            op: BinOp::JsStrictEq,
            ..
        }
    ))?);
    // It must NOT degrade to the structural `Eq` used by `==`.
    ensure!(!module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::BinOp { op: BinOp::Eq, .. }
    ))?);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn triple_not_equals_lowers_to_js_strict_not_eq() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const a = 1;
const b = 2;
const diff = a !== b;
"),
        &mut ctx,
    )?;
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::BinOp {
            op: BinOp::JsStrictNotEq,
            ..
        }
    ))?);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn double_equals_lowers_to_structural_eq() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const a = 1;
const b = 2;
const same = a == b;
const diff = a != b;
"),
        &mut ctx,
    )?;
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::BinOp { op: BinOp::Eq, .. }
    ))?);
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::BinOp {
            op: BinOp::NotEq,
            ..
        }
    ))?);
    // `==`/`!=` must NOT borrow the JS-strict reference ops.
    ensure!(!module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::BinOp {
            op: BinOp::JsStrictEq | BinOp::JsStrictNotEq,
            ..
        }
    ))?);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// Bit-shift operators: <<, >>, >>>.
// ---------------------------------------------------------------------------

#[test]
fn bit_shift_operators_lower_to_distinct_ops() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const x = 8;
const left = x << 1;
const right = x >> 1;
const unsigned = x >>> 1;
"),
        &mut ctx,
    )?;
    for expected in [BinOp::Shl, BinOp::Shr, BinOp::UShr] {
        ensure!(
            module_body_has(&ctx, module_id, |kind| matches!(
                kind,
                ExprKind::BinOp { op, .. } if *op == expected
            ))?,
            "expected bit-shift op {expected:?}"
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// String method lowerings.
// ---------------------------------------------------------------------------

#[test]
fn string_slice_and_char_access_lower() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const part = word.slice(1, 3);
const ch = word.charAt(0);
const code = word.charCodeAt(0);
"#),
        &mut ctx,
    )?;
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::StringSlice { .. }
    ))?);
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::StringCharAt { .. }
    ))?);
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::StringCharCodeAt { .. }
    ))?);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn string_includes_lowers_to_string_contains() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const has = word.includes("me");
"#),
        &mut ctx,
    )?;
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::StringContains { .. }
    ))?);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn string_split_and_repeat_lower() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "a,b,c";
const parts = word.split(",");
const echo = "ab".repeat(3);
"#),
        &mut ctx,
    )?;
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::StringSplit { .. }
    ))?);
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::StringRepeat { .. }
    ))?);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn string_replace_first_lowers_to_string_replace() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "hello hello";
const once = word.replace("hello", "hi");
"#),
        &mut ctx,
    )?;
    // A single string-literal `replace` lowers to the first-match string
    // replacement op.
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::StringReplace {
            op: StringReplaceOp::First,
            ..
        }
    ))?);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn string_padstart_and_padend_lower_distinctly() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "7";
const left = word.padStart(3, "0");
const right = word.padEnd(3, "0");
"#),
        &mut ctx,
    )?;
    for expected in [StringPadOp::Start, StringPadOp::End] {
        ensure!(
            module_body_has(&ctx, module_id, |kind| matches!(
                kind,
                ExprKind::StringPad { op, .. } if *op == expected
            ))?,
            "expected string pad op {expected:?}"
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// Numeric lowerings.
// ---------------------------------------------------------------------------

#[test]
fn math_abs_pow_and_sqrt_lower() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const x = -4;
const a = Math.abs(x);
const p = Math.pow(2, 3);
const r = Math.sqrt(16);
"),
        &mut ctx,
    )?;
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::NumericAbs { .. }
    ))?);
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::NumericPow { .. }
    ))?);
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::NumericUnaryFunc {
            op: NumericUnaryFuncOp::Sqrt,
            ..
        }
    ))?);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn number_predicates_lower_to_distinct_ops() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const v = 3.5;
const finite = Number.isFinite(v);
const integer = Number.isInteger(v);
const nan = Number.isNaN(v);
"),
        &mut ctx,
    )?;
    for expected in [
        NumericPredicateOp::IsFinite,
        NumericPredicateOp::IsInteger,
        NumericPredicateOp::IsNaN,
    ] {
        ensure!(
            module_body_has(&ctx, module_id, |kind| matches!(
                kind,
                ExprKind::NumericPredicate { op, .. } if *op == expected
            ))?,
            "expected numeric predicate {expected:?}"
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// Array / list lowerings.
// ---------------------------------------------------------------------------

#[test]
fn array_callback_methods_lower_to_list_callback_ops() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
const xs = [1, 2, 3];
const doubled = xs.map((x) => x * 2);
const evens = xs.filter((x) => x % 2 === 0);
const anyBig = xs.some((x) => x > 2);
const allBig = xs.every((x) => x > 0);
"),
        &mut ctx,
    )?;
    for expected in [
        ListCallbackOp::Map,
        ListCallbackOp::Filter,
        ListCallbackOp::Some,
        ListCallbackOp::Every,
    ] {
        ensure!(
            crate_has(&ctx, |kind| matches!(
                kind,
                ExprKind::ListCallback { op, .. } if *op == expected
            )),
            "expected list callback op {expected:?}"
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn array_includes_lowers_to_list_contains() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const xs = [1, 2, 3];
const has = xs.includes(2);
"),
        &mut ctx,
    )?;
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::ListContains { .. }
    ))?);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn array_slice_concat_and_reverse_lower() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const xs = [1, 2, 3];
const ys = [4, 5];
const part = xs.slice(1);
const joined = xs.concat(ys);
const flipped = xs.reverse();
"),
        &mut ctx,
    )?;
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::ListSlice { .. }
    ))?);
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::ListConcat { .. }
    ))?);
    ensure!(module_body_has(&ctx, module_id, |kind| matches!(
        kind,
        ExprKind::ListReverse { .. }
    ))?);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// Object / dictionary projections.
// ---------------------------------------------------------------------------

#[test]
fn object_values_and_entries_lower_to_dict_projections() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function projectValues(x: Record<string, number>): number[] {
  return Object.values(x);
}
export function projectEntries(x: Record<string, number>): [string, number][] {
  return Object.entries(x);
}
"),
        &mut ctx,
    )?;
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::DictProjection {
            op: DictProjectionOp::Values,
            ..
        }
    )));
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::DictProjection {
            op: DictProjectionOp::Entries,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn object_fromentries_lowers_to_dict_projection() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function build(pairs: [string, number][]): Record<string, number> {
  return Object.fromEntries(pairs);
}
"),
        &mut ctx,
    )?;
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::DictProjection {
            op: DictProjectionOp::FromEntries,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// Set lowerings.
// ---------------------------------------------------------------------------

#[test]
fn set_has_add_and_delete_lower() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function run(): boolean {
  const s = new Set<number>();
  s.add(1);
  const present = s.has(1);
  s.delete(1);
  return present;
}
"),
        &mut ctx,
    )?;
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::SetAdd { .. }
    )));
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::SetContains { .. }
    )));
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::SetRemove {
            op: SetRemoveOp::Delete,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// instanceof / runtime-kind probes.
// ---------------------------------------------------------------------------

#[test]
fn array_isarray_lowers_to_unknown_is_array() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function check(value: unknown): boolean {
  return Array.isArray(value);
}
"),
        &mut ctx,
    )?;
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::UnknownIs {
            kind: smelt_hir::UnknownKind::Array,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn user_class_instanceof_lowers_to_instanceof() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
class Box {
  value: number = 0;
}
export function check(x: unknown): boolean {
  return x instanceof Box;
}
"),
        &mut ctx,
    )?;
    // A user-class `instanceof` must stay a class-identity `InstanceOf`, not a
    // primitive runtime-kind probe.
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::InstanceOf { .. }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// Optional chaining.
// ---------------------------------------------------------------------------

#[test]
fn optional_chaining_member_read_lowers_to_optional_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
interface Inner { name: string; }
interface Outer { inner?: Inner; }
export function readName(o: Outer): string | undefined {
  return o.inner?.name;
}
"),
        &mut ctx,
    )?;
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::OptionalField { .. }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn nullish_coalescing_lowers_to_optional_coalesce() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function withDefault(value: number | undefined): number {
  return value ?? 0;
}
"),
        &mut ctx,
    )?;
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::OptionalCoalesce { .. }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// JSON lowerings.
// ---------------------------------------------------------------------------

#[test]
fn json_stringify_and_parse_lower() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function roundTrip(x: Record<string, number>): unknown {
  const text = JSON.stringify(x);
  return JSON.parse(text);
}
"),
        &mut ctx,
    )?;
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::JsonStringify { .. }
    )));
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::JsonParse { .. }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

// ---------------------------------------------------------------------------
// Builtin interceptor scoping.
//
// The name-matched global interceptors (`isNaN`, `parseInt`, `String`, ...) and
// the string/number method interceptors (`.includes`, `.toString`) fire early,
// before the callee/receiver is resolved against the module's bindings. They
// must respect lexical scope: a value import, module item, or local binding of
// the same name shadows the global, and a namespace/util object receiver marks a
// free-function call rather than a string/number method. es-toolkit relies on
// all of these (`import { isNaN } from './isNaN'`, `ns.includes(coll, value)`).
// ---------------------------------------------------------------------------

/// A bound string method taken as a *value* (`''.slice`, not `''.slice(...)`)
/// resolves to the erased dynamic boundary instead of hard-erroring on field
/// access, mirroring es-toolkit `pick('', 'slice')`.
#[test]
fn bound_string_method_reference_lowers_as_erased_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function pluck(): unknown {
  const method = ''.slice;
  return method;
}
"),
        &mut ctx,
    )?;
    // The value read lowers to a plain field access; it must not be rejected.
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::Field { .. }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A value import of `isNaN` shadows the numeric-global predicate, so the call
/// lowers through the ordinary call path rather than as `NumericPredicate`.
#[test]
fn value_imported_is_nan_is_not_the_numeric_global() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { isNaN } from './isNaN';
export function check(): unknown {
  return isNaN(true);
}
"),
        &mut ctx,
    )?;
    // The shadowing binding is called; the numeric-global predicate op must not
    // be synthesized for the shadowed name.
    ensure!(!crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::NumericPredicate {
            op: NumericPredicateOp::IsNaN,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// The bare-identifier `isNaN` global still lowers to the numeric predicate when
/// it is *not* shadowed, so the shadow guard does not disable the global.
#[test]
fn unshadowed_is_nan_still_lowers_to_numeric_predicate() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
const value: number = 3;
const flag = isNaN(value);
"),
        &mut ctx,
    )?;
    ensure!(crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::NumericPredicate {
            op: NumericPredicateOp::IsNaN,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A value import of `parseInt` shadows the global, so `parseInt(str, undefined)`
/// lowers through the ordinary call path rather than the global parse op.
#[test]
fn value_imported_parse_int_is_not_the_global() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { parseInt } from './parseInt';
export function pi(): unknown {
  return parseInt('10', undefined);
}
"),
        &mut ctx,
    )?;
    // Neither the radix parse op nor the bare `ToInt` cast may be synthesized for
    // the shadowed name.
    ensure!(!crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::ParseIntRadix { .. }
    )));
    ensure!(!crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::PrimitiveCast {
            op: PrimitiveCastOp::ToInt,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A namespace-import `includes`/`toString` is the free-function form, so its
/// second argument is a value, not a numeric string position or radix. It must
/// not be rejected by the string/number method interceptors.
#[test]
fn namespace_includes_and_to_string_are_free_functions() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import * as tk from './lib';
export function useNs(value: unknown): unknown {
  const a = tk.includes(value, '__p');
  const b = tk.toString(value);
  return [a, b];
}
"),
        &mut ctx,
    )?;
    // The string-includes op (which would demand a numeric position) must not
    // fire on the namespace free-function call.
    ensure!(!crate_has(&ctx, |kind| matches!(
        kind,
        ExprKind::StringContains { .. }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
