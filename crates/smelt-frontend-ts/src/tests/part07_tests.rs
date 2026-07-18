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
        body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(found)) if found == value),
        )
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
        ts!(r"
export function keysOf(x: Record<string, number>): string[] {
  return globalThis.Object.keys(x);
}
"),
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
        ts!(r"
export function isArr(value: unknown): boolean {
  const g = globalThis;
  return g.Array.isArray(value);
}
"),
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
        ts!(r"
export function roundTrip(x: number): number {
  return globalThis.Math.floor(x);
}
"),
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
        ts!(r"
function run(size: number, length: number): number[] {
  const result: number[] = [];
  for (let step = length - size, resultIndex = 0; step < length; step++, resultIndex++) {
    result[resultIndex] = step;
  }
  return result;
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_statement_sequence_through_assignment_dispatch() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function run(): number {
  let left = 0;
  let right = 0;
  (left = 1, right = 2);
  return left + right;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure_eq!(
        body.stmts
            .iter()
            .filter(|statement| matches!(statement, Stmt::Assign { .. }))
            .count(),
        2
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn statement_sequence_spawns_each_future() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function run(): void {
  (Promise.resolve(1), Promise.resolve(2));
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure_eq!(
        body.exprs
            .iter()
            .filter(|expr| matches!(
                expr.kind,
                ExprKind::AsyncOp {
                    op: smelt_hir::AsyncOp::SpawnLocal,
                    ..
                }
            ))
            .count(),
        2
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn comma_separated_tests_register_independently() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(
        ts!(r#"
import { describe, test } from "vitest";

describe("group", () => {
  test("first", () => {}), test("second", () => {});
});
"#),
        "src/sequence.test.ts",
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure_eq!(
        ctx.krate
            .items
            .iter()
            .filter(|item| matches!(item, Item::Function(function) if function.is_test))
            .count(),
        2
    );
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
        ts!(r"
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
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn dynamic_global_computed_read_lowers_to_erased_undefined() -> Result<(), String> {
    // A dynamic computed key (`globalThis[key]`, `key: string`) names no
    // statically-known global property. It is a genuine dynamic boundary: the
    // value could be any global (a constructor, an object, a number, or
    // absent), so no concrete type, union, or scoped generic can represent it —
    // it must be `SmeltUnknown`. Smelt's deterministic profile models no runtime
    // global-object property store, so the read resolves to the JS-correct
    // `undefined`, cast to `Unknown` for the downstream erased-value paths.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function dyn(key: string): unknown {
  return globalThis[key];
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::Literal(Literal::Undefined)
    )));
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::UnknownCast { .. }
    )));
    Ok(())
}

#[test]
fn literal_key_global_computed_read_normalizes_to_builtin() -> Result<(), String> {
    // A statically-known string-literal key that names a modeled JavaScript
    // global normalizes to the concrete builtin value, exactly like the
    // static-member spelling `globalThis.Array`, so the modeled global keeps its
    // shape instead of erasing to a dynamic `undefined` read.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function arr(): unknown {
  return globalThis['Array'];
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    // The literal-key normalization does not emit the dynamic-read `undefined`.
    ensure!(!crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::Literal(Literal::Undefined)
    )));
    Ok(())
}

#[test]
fn dynamic_global_constructor_read_supports_new_construction() -> Result<(), String> {
    // The erased dynamic global read flows into the existing dynamic-`new`
    // machinery: `const Ctor = globalThis[key]; new Ctor(arg)` constructs
    // through the erased closure-call ABI rather than aborting the build.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function build(key: string, arg: number): unknown {
  const Ctor = globalThis[key];
  return new Ctor(arg);
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::ClosureCall { .. }
    )));
    Ok(())
}

#[test]
fn lowers_callback_body_reading_non_callable_value_item() -> Result<(), String> {
    // A callback body that reads a non-callable module item as an ordinary value
    // (`value !== whitespace`, where `whitespace` is a module-scoped `string`
    // const) cannot be modeled by the compact callback IR, which only resolves
    // callable item references. The full closure-body fallback routes the
    // identifier through the general expression path, so it lowers instead of
    // aborting the build.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
const whitespace = [' ', '\t'].join('');
export function run(values: string[]): number[] {
  return values.map(value => (value !== whitespace ? 1 : 0));
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_zero_parameter_named_function_array_callback() -> Result<(), String> {
    // JavaScript callbacks adapt arity at the call site: a named function
    // declaring no parameters (`values.map(stubTrue)`, the lodash-style stub
    // shape) simply ignores the supplied `(value, index, array)` arguments.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function stubTrue(): boolean {
  return true;
}
export function run(values: number[]): boolean[] {
  return values.map(stubTrue);
}
"),
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
        ts!(r"
function stubFalse(): boolean {
  return false;
}
export function run(values: number[]): number[] {
  return values.filter(stubFalse);
}
"),
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
        ts!(r"
export function run(values: string[]): number[] {
  const localStub = () => 42;
  return values.map(localStub);
}
"),
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
        ts!(r"
function withTail(value: number, index?: number, list?: number[], guard?: number): number {
  return value + (guard ?? 0);
}
export function run(values: number[]): number[] {
  return values.map(withTail);
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn callable_local_property_writes_lower_to_typed_callable_object_assign() -> Result<(), String> {
    // A function-typed local (`counter`) that receives straight-line
    // `counter.method = …` writes and is then returned at a callable-interface
    // type must lower to a `CallableObjectAssign` typed at the interface class,
    // carrying the collected property writes — not leak the writes.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
interface Counter {
  (): number;
  reset(): void;
}
export function makeCounter(): Counter {
  let count = 0;
  const counter = function (): number {
    count = count + 1;
    return count;
  };
  counter.reset = function (): void {
    count = 0;
  };
  return counter;
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "make_counter")?;
    let body = function_body(&ctx, function)?;
    let assign = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            ExprKind::CallableObjectAssign { props, .. } => Some((expr.ty, props.len())),
            _ => None,
        })
        .ok_or_else(|| "expected a CallableObjectAssign expression".to_owned())?;
    // Exactly one property (`reset`) was collected.
    ensure_eq!(assign.1, 1usize);
    // The synthesized value is typed at the callable interface class.
    ensure!(matches!(
        ctx.krate.types.get(assign.0),
        Some(Type::Class { name, .. }) if ctx.krate.symbols.get(*name) == Some("Counter")
    ));
    Ok(())
}

#[test]
fn callable_local_conditional_property_write_falls_through() -> Result<(), String> {
    // Regression: a property write onto a callable local inside a conditional
    // (non-root) block is a documented punt the collection cannot claim, but it
    // must NOT abort the crate. It falls through to normal (pre-feature)
    // assignment lowering — the fieldless static-member write is discarded — and
    // lowering succeeds with no `CallableObjectAssign`. This mirrors the
    // `partial.placeholder = …` shape in es-toolkit that regressed to a hard
    // error and blocked the whole crate.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
interface Counter {
  (): number;
  reset(): void;
}
export function makeCounter(flag: boolean): Counter {
  const counter = function (): number {
    return 0;
  };
  if (flag) {
    counter.reset = function (): void {};
  }
  return counter;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "make_counter")?;
    let body = function_body(&ctx, function)?;
    // The conditional write was not claimed into a typed struct: no
    // CallableObjectAssign is synthesized for the fall-through shape.
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::CallableObjectAssign { .. }))
    );
    Ok(())
}

#[test]
fn callable_local_property_write_after_escape_is_unsupported() -> Result<(), String> {
    // Once a callable local has escaped (been read for anything other than the
    // consuming coercion), a later property write cannot be bundled and is a
    // documented punt.
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
interface Counter {
  (): number;
  reset(): void;
}
export function makeCounter(): Counter {
  const counter = function (): number {
    return 0;
  };
  counter.reset = function (): void {};
  const alias = counter;
  counter.reset = function (): void {};
  return counter;
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "after it escapes")
}

#[test]
fn callable_property_collection_does_not_leak_across_arrow_item_lowering() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
type Throttled = {
  (): void;
  isThrottled(): boolean;
};

export const throttle = () => {
  let timer: number | undefined = undefined;
  const throttled: Throttled = () => {
    timer = 1;
  };
  throttled.isThrottled = () => timer !== undefined;
  return throttled;
};
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn annotated_local_callback_adapts_unknown_list_elements() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function collect(values: unknown[]): string[] {
  const getStrings = (items: unknown[]): string[] => {
    return items.flatMap(item => item as any);
  };
  return getStrings(values);
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn recursive_local_callback_lowers_flat_map_symbol() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function collect(value: object): string[] {
  const getStrings = (nested: any, paths: string[]): string[] => {
    if (Array.isArray(nested)) {
      return nested.flatMap((item, index) =>
        getStrings(item, [...paths, String(index)])
      );
    }
    return [paths.join(".")];
  };
  return getStrings(value, []);
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn callable_object_type_alias_remains_callable() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
type Debounced<TArgs extends any[]> = {
  (...args: TArgs): void;
  cancel(): void;
};

function invoke(func: Debounced<any>): void {
  func();
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn plain_function_local_without_property_writes_is_untouched() -> Result<(), String> {
    // A function-typed local that receives no property writes never enters the
    // callable-object collection and lowers with no CallableObjectAssign.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
export function run(): number {
  const helper = function (): number {
    return 1;
  };
  return helper();
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "run")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::CallableObjectAssign { .. }))
    );
    Ok(())
}

#[test]
fn nested_callback_uses_remapped_callable_closure_binding() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function list(start: number, end: number): number[] { return [start, end]; }
export function series(items: number[]) {
  const indexes: Record<number, number> = {};
  const next = (current: number): number => indexes[current] + items.length;
  const previous = (current: number): number => indexes[current] - items.length;
  const spin = (current: number, num: number): number => {
    return list(0, 1).reduce(
      acc => num > 0 ? next(acc) : previous(acc),
      current
    );
  };
  return spin;
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn namespace_function_members_are_materialized_in_value_position() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function helper(value: number): number { return value; }
const api = { helper };
export function inspect(): unknown { return api.helper; }
"),
        &mut ctx,
    )?;
    ensure!(ctx.krate.bodies.iter().any(|body| {
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Closure(_)))
    }));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn namespace_rest_calls_hint_every_trailing_argument_with_item_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function total(...values: number[]): number { return values[0]; }
const api = { total };
export function run(): number { return api.total(1, 2, 3); }
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn typeof_of_erased_value_emits_runtime_tag_check_not_object_literal() -> Result<(), String> {
    // `typeof a === typeof b` on two `any` params must compare runtime tags, not
    // fold both sides to the literal `"object"`. Folding (the historical
    // `.unwrap_or("object")`) made `isEqualWith(1, '1', ...)` mis-report equal
    // typeof and left the primitive `switch (typeof a)` arms dead.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function sameType(a: any, b: any): boolean {
  return typeof a === typeof b;
}
"#),
        &mut ctx,
    )?;
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::TypeofValue { .. }
    )));
    // No side of the comparison folded to a bare `"object"` string literal.
    ensure!(!crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::Literal(Literal::String(s)) if s == "object"
    )));
    Ok(())
}

#[test]
fn switch_typeof_on_erased_value_keeps_all_arms_live() -> Result<(), String> {
    // `switch (typeof x)` on an `any` value must inspect the tag at runtime so the
    // number/string/boolean/function arms stay reachable, rather than the scrutinee
    // collapsing to a constant `"object"` that only the object arm can match.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function classify(x: any): string {
  switch (typeof x) {
    case "number":
      return "n";
    case "string":
      return "s";
    default:
      return "o";
  }
}
"#),
        &mut ctx,
    )?;
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::TypeofValue { .. }
    )));
    Ok(())
}

#[test]
fn typeof_of_concrete_string_still_folds_to_literal() -> Result<(), String> {
    // A statically-known `typeof` spelling must keep folding: a `string` value's
    // `typeof` is always `"string"`, so no runtime tag inspection is emitted.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function tag(s: string): string {
  return typeof s;
}
"#),
        &mut ctx,
    )?;
    ensure!(!crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::TypeofValue { .. }
    )));
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::Literal(Literal::String(s)) if s == "string"
    )));
    Ok(())
}

#[test]
fn mixed_runtime_and_test_sequence_is_rejected_explicitly() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
import { describe, test } from "vitest";
function setup(): void {}
describe("suite", () => {
  setup(), test("case", () => {});
});
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "mixed test registration and runtime sequence")
}
