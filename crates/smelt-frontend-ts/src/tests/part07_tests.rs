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
fn dynamic_global_computed_read_reads_the_global_object() -> Result<(), String> {
    // A dynamic computed key (`globalThis[key]`, `key: string`) names no
    // STATICALLY-known global property, but it does name one at runtime. It is a
    // genuine dynamic boundary — the value could be any global (a constructor,
    // an object, a number, or absent) — so the RESULT is `SmeltUnknown`; what it
    // is not is a compile-time constant.
    //
    // This assertion previously required the read to fold to `undefined`,
    // encoding "the profile models no global-object property store". That was
    // the defect, not the contract: it made the two spellings of one JavaScript
    // operation disagree, since `globalThis.Error` normalized to the modeled
    // constructor while `globalThis[name]` with `name === "Error"` answered
    // `undefined`. The read now lowers to a real erased index read on the
    // global-object value, which resolves a modeled builtin constructor by name
    // at runtime and still answers `undefined` for an unmodeled name.
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
        ExprKind::Index { .. }
    )));
    // The receiver is the global-object marker value, not a fabricated empty
    // record: its `__smelt_global_object` key is what the runtime property
    // resolution keys off.
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::Literal(Literal::String(text)) if text == "__smelt_global_object"
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
        ExprKind::Construct { .. }
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

/// Every distinct `Literal::Symbol` spelling anywhere in the lowered crate.
///
/// A symbol's runtime identity IS its spelling (comparisons are string
/// comparisons on the opaque tag), so a source symbol that reaches two lowering
/// paths must produce one spelling, not two.
fn distinct_symbol_literals(ctx: &HirCtx) -> Vec<String> {
    let mut spellings = Vec::new();
    for body in &ctx.krate.bodies {
        for expr in &body.exprs {
            if let ExprKind::Literal(Literal::Symbol(value)) = &expr.kind
                && !spellings.contains(value)
            {
                spellings.push(value.clone());
            }
        }
    }
    spellings.sort();
    spellings
}

/// A static property written onto a module-level function declaration round
/// trips: the read resolves to the written value, and a `unique symbol` keeps
/// the identity (description included) it had at its definition site.
///
/// The definition and the read reach the symbol through different lowering
/// paths — the ordinary call path and the const-initializer folding path — so
/// the test asserts the crate contains exactly ONE symbol spelling. Two
/// spellings mean the read compares unequal to the value that was written,
/// which is the placeholder-sentinel defect.
#[test]
fn module_function_static_property_read_matches_the_written_symbol() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
const marker: unique symbol = Symbol('marker');
function outer(n: number): number {
  return n + 1;
}
outer.marker = marker;
export function probe(): boolean {
  const x: unknown = 1;
  return x === outer.marker;
}
"),
        &mut ctx,
    )?;
    let spellings = distinct_symbol_literals(&ctx);
    ensure_eq!(spellings.len(), 1);
    ensure!(
        spellings
            .first()
            .is_some_and(|spelling| spelling.starts_with("Symbol(marker)@")),
        "symbol spelling lost its description: {spellings:?}"
    );
    Ok(())
}

/// Repeated writes to the same static property are last-write-wins in source
/// order: the read resolves to the final value, not the first.
#[test]
fn module_function_static_property_last_write_wins() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function outer(n: number): number {
  return n + 1;
}
outer.tag = 'first';
outer.tag = 'second';
export function probe(): string {
  return outer.tag;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "probe")?;
    let body = function_body(&ctx, function)?;
    let strings = body
        .exprs
        .iter()
        .filter_map(|expr| match &expr.kind {
            ExprKind::Literal(Literal::String(value)) => Some(value.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    ensure_eq!(strings, vec!["second".to_owned()]);
    Ok(())
}

/// The read inside a *callback* body resolves to the same value as the read in
/// an ordinary body.
///
/// This is the es-toolkit `curry` shape: the sentinel is compared inside
/// `args.filter(item => item === curry.placeholder)`. The compact callback IR is
/// lowered by its own member path, which used to project a positional field off
/// the function value and answer with a null, so the filter never matched.
#[test]
fn function_static_property_read_inside_a_callback_matches_the_written_value()
-> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function curry(n: number): number {
  return n;
}
export function probe(items: unknown[]): unknown[] {
  return items.filter(item => item === curry.placeholder);
}
const curryPlaceholder: unique symbol = Symbol('curry.placeholder');
curry.placeholder = curryPlaceholder;
"),
        &mut ctx,
    )?;
    let spellings = distinct_symbol_literals(&ctx);
    ensure_eq!(spellings.len(), 1);
    ensure!(
        spellings
            .first()
            .is_some_and(|spelling| spelling.starts_with("Symbol(curry.placeholder)@")),
        "symbol spelling lost its description: {spellings:?}"
    );
    // The source has no record/class receiver, so ANY field projection in the
    // lowered crate is the bogus one this test exists to prevent, and the
    // sentinel must appear inside the callback body rather than only in the
    // module initializer.
    let mut field_reads = 0_usize;
    let mut symbol_reads = 0_usize;
    for body in &ctx.krate.bodies {
        for expr in &body.exprs {
            match &expr.kind {
                ExprKind::Field { .. } => field_reads += 1,
                ExprKind::Literal(Literal::Symbol(_)) => symbol_reads += 1,
                _ => {}
            }
        }
    }
    ensure_eq!(field_reads, 0);
    ensure!(
        symbol_reads >= 2,
        "expected the sentinel in both the module initializer and the callback, saw {symbol_reads}"
    );
    Ok(())
}

/// Reading a static property that was never written is a diagnostic, not a
/// silent positional field access on a value that has no fields.
#[test]
fn read_of_unwritten_function_static_property_is_diagnosed() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
function outer(n: number): number {
  return n + 1;
}
outer.written = 1;
export function probe(): number {
  return outer.unwritten;
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "is not a modeled property of a function value")
}

/// The universal `Function.prototype` members stay resolvable on any function
/// value, so the unmodeled-property rejection does not swallow them.
#[test]
fn universal_function_members_still_resolve_on_a_function_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function outer(n: number): number {
  return n + 1;
}
export function probe(): unknown {
  return outer.prototype;
}
"),
        &mut ctx,
    )?;
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

/// The `type X = { (): void; m(): boolean }` spelling of a callable object
/// lowers to the same callable-interface class the `interface` spelling
/// produces, so the collected `throttled.isThrottled` write is consumed into a
/// typed `CallableObjectAssign` instead of being dropped.
///
/// The return position here carries no type hint at all (the arrow's return
/// type is inferred), so consumption falls back to the interface the local was
/// *declared* at — the `debounce`/`throttle` shape in radash.
#[test]
fn callable_object_type_alias_spelling_consumes_property_writes() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
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
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "throttle")?;
    let body = function_body(&ctx, function)?;
    let assign_ty = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            ExprKind::CallableObjectAssign { props, .. } if props.len() == 1 => Some(expr.ty),
            _ => None,
        })
        .ok_or_else(|| "expected a CallableObjectAssign expression".to_owned())?;
    // The alias name is the interface name: one spelling, one generated class.
    ensure!(matches!(
        ctx.krate.types.get(assign_ty),
        Some(Type::Class { name, .. }) if ctx.krate.symbols.get(*name) == Some("Throttled")
    ));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// An *intersection* of a call signature with an object type is the same
/// callable object, so it too consumes its property writes — the es-toolkit
/// `curry` shape, whose return type is
/// `((...args: any[]) => any) & { placeholder: … }`.
///
/// The surface is anonymous, so it lowers to a synthetic interface named from
/// its structure rather than from any source name.
#[test]
fn callable_object_intersection_spelling_consumes_property_writes() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const curryPlaceholder: unique symbol = Symbol('curry.placeholder');

export function curry(
  func: (...args: any[]) => any
): ((...args: any[]) => any) & { placeholder: typeof curryPlaceholder } {
  const wrapper = function (...partialArgs: any[]) {
    return func(...partialArgs);
  };
  wrapper.placeholder = curryPlaceholder;
  return wrapper;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "curry")?;
    let body = function_body(&ctx, function)?;
    let assign_ty = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            ExprKind::CallableObjectAssign { props, .. } if props.len() == 1 => Some(expr.ty),
            _ => None,
        })
        .ok_or_else(|| "expected a CallableObjectAssign expression".to_owned())?;
    let Some(Type::Class { name, .. }) = ctx.krate.types.get(assign_ty) else {
        return Err("callable object construction is not typed at a class".to_owned());
    };
    // A synthetic interface, carrying both the call slot and the written member.
    let interface = ctx
        .krate
        .items
        .iter()
        .find_map(|item| match item {
            smelt_hir::Item::Interface(interface) if interface.name == *name => Some(interface),
            _ => None,
        })
        .ok_or_else(|| "expected a synthesized interface item".to_owned())?;
    let field_names = interface
        .fields
        .iter()
        .filter_map(|field| ctx.krate.symbols.get(field.name))
        .collect::<Vec<_>>();
    ensure!(field_names.contains(&"placeholder"));
    ensure!(field_names.contains(&"__smelt_call"));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Two occurrences of the same anonymous callable-object surface share one
/// synthesized interface rather than generating a struct per occurrence.
#[test]
fn identical_anonymous_callable_object_surfaces_share_one_interface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function first(): ((value: number) => number) & { tag: string } {
  const wrapper = function (value: number): number {
    return value;
  };
  wrapper.tag = 'first';
  return wrapper;
}
export function second(): ((value: number) => number) & { tag: string } {
  const wrapper = function (value: number): number {
    return value;
  };
  wrapper.tag = 'second';
  return wrapper;
}
"),
        &mut ctx,
    )?;
    let synthesized = ctx
        .krate
        .items
        .iter()
        .filter(|item| {
            matches!(item, smelt_hir::Item::Interface(interface)
                if ctx.krate.symbols.get(interface.name)
                    .is_some_and(|name| name.starts_with("SmeltCallableObject")))
        })
        .count();
    ensure_eq!(synthesized, 1usize);
    Ok(())
}

/// A type literal with only call signatures stays a plain function type: the
/// synthesis claims a surface only when it is callable *and* carries members.
#[test]
fn call_signature_only_type_literal_stays_a_function_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
type Plain = { (value: number): number };
export function apply(func: Plain, value: number): number {
  return func(value);
}
"),
        &mut ctx,
    )?;
    let synthesized = ctx
        .krate
        .items
        .iter()
        .filter(|item| matches!(item, smelt_hir::Item::Interface(_)))
        .count();
    ensure_eq!(synthesized, 0usize);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// The `interface` spelling of the same callable object still consumes its
/// property writes into a typed `CallableObjectAssign`, so the
/// collected-but-never-consumed check does not fire on the working path.
#[test]
fn callable_interface_alias_spelling_consumes_property_writes() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
interface Throttled {
  (): void;
  isThrottled(): boolean;
}

export const throttle = (): Throttled => {
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
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "throttle")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::CallableObjectAssign { .. }))
    );
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
        ts!(r"
export function sameType(a: any, b: any): boolean {
  return typeof a === typeof b;
}
"),
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
        ts!(r"
export function tag(s: string): string {
  return typeof s;
}
"),
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

/// Return the operand of the first `Stmt::Throw` anywhere in the crate.
///
/// `Stmt::Throw` may live directly in a body's statement list or inside a
/// nested block, and the throw of interest may sit in a closure body, so every
/// body's flat statement arena is scanned rather than one block's statement ids.
fn first_throw_expr_kind(ctx: &HirCtx) -> Option<(ExprKind, usize)> {
    for (body_index, body) in ctx.krate.bodies.iter().enumerate() {
        for stmt in &body.stmts {
            if let Stmt::Throw(expr) = stmt {
                return Some((body.exprs[expr.0 as usize].kind.clone(), body_index));
            }
        }
    }
    None
}

/// Return whether a HIR expression is the erased `Error` record.
///
/// The record is a `DictLit` whose first entry is the `__smelt_error` class
/// marker, optionally wrapped in the `UnknownCast` that erases it for the
/// exception channel.
fn is_error_record(body: &smelt_hir::Body, kind: &ExprKind) -> bool {
    let kind = match kind {
        ExprKind::UnknownCast { value, .. } => &body.exprs[value.0 as usize].kind,
        other => other,
    };
    let ExprKind::DictLit(entries) = kind else {
        return false;
    };
    entries.iter().any(|(key, _)| {
        matches!(
            &body.exprs[key.0 as usize].kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_error"
        )
    })
}

#[test]
fn throwing_an_error_keeps_the_error_object() -> Result<(), String> {
    // `throw` is value-preserving in JavaScript, but the throw *statement* used
    // to narrow `new Error(m)` down to `m`, so every `catch` received a bare
    // string. `error instanceof Error` was then false, `error.message` was
    // `undefined`, and `error.name` was unreadable — while the very same
    // construction written as a value (`const e = new Error(m)`) kept the whole
    // record. This pins the throw statement to that same record.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function boom(): number {
  throw new Error("kaboom");
}
"#),
        &mut ctx,
    )?;
    let (kind, body_index) = first_throw_expr_kind(&ctx).ok_or("no throw statement lowered")?;
    ensure!(
        is_error_record(&ctx.krate.bodies[body_index], &kind),
        "a thrown `new Error(..)` must stay the erased error record, not its message",
    );
    Ok(())
}

#[test]
fn throwing_an_error_subclass_records_the_spelled_class() -> Result<(), String> {
    // The `__smelt_error` marker carries the *spelled* class name so `.name`
    // reads truthfully and a `catch` can tell a `TypeError` from an `Error`.
    // Narrowing to the message erased that distinction along with everything
    // else.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function boom(): number {
  throw new TypeError("bad type");
}
"#),
        &mut ctx,
    )?;
    let (kind, body_index) = first_throw_expr_kind(&ctx).ok_or("no throw statement lowered")?;
    let body = &ctx.krate.bodies[body_index];
    ensure!(is_error_record(body, &kind), "expected an error record");
    ensure!(
        crate_has_expr(&ctx, |kind| matches!(
            kind,
            ExprKind::Literal(Literal::String(text)) if text == "TypeError"
        )),
        "the record must carry the spelled class name",
    );
    Ok(())
}

#[test]
fn throwing_a_bare_string_is_not_wrapped_in_an_error() -> Result<(), String> {
    // JavaScript does not wrap a thrown primitive: `throw 'a string'` delivers
    // exactly that string. Preserving the operand must not tip over into
    // synthesizing an `Error` for operands that never had one.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function boom(): number {
  throw "a bare string";
}
"#),
        &mut ctx,
    )?;
    let (kind, body_index) = first_throw_expr_kind(&ctx).ok_or("no throw statement lowered")?;
    ensure!(
        !is_error_record(&ctx.krate.bodies[body_index], &kind),
        "a thrown primitive must not be wrapped into an error record",
    );
    ensure!(
        matches!(&kind, ExprKind::Literal(Literal::String(text)) if text == "a bare string"),
        "a thrown string literal must stay that literal",
    );
    Ok(())
}

#[test]
fn throwing_an_error_inside_a_callback_keeps_the_error_object() -> Result<(), String> {
    // A `throw` written inside an arrow lowers through the reduced callback
    // expression language, which had its own, separate narrowing: it stripped
    // `new Error(m)` to `m` and replaced every other construction with the empty
    // string. Fixing only the statement path would have left every
    // `attempt(() => { throw new Error(..) })`-shaped callback still throwing a
    // bare string.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function run(apply: (f: () => number) => number): number {
  return apply(() => {
    throw new Error("callback boom");
  });
}
"#),
        &mut ctx,
    )?;
    let (kind, body_index) = first_throw_expr_kind(&ctx).ok_or("no throw statement lowered")?;
    ensure!(
        is_error_record(&ctx.krate.bodies[body_index], &kind),
        "a callback-thrown `new Error(..)` must stay the erased error record",
    );
    Ok(())
}
