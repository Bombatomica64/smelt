//! Generated Rust for closures and callbacks.

use super::*;

#[test]
fn captures_recursive_callable_factory_result_inside_test_callback() {
    let source = source_for(
        r#"
import { test } from "vitest";

type Fn = (value: string) => void;

function wrap(callback: Fn): Fn {
  return callback;
}

test("recursive", async () => {
  const recursive = wrap((value: string) => {
    recursive(value);
  });
  recursive("a");
});
"#,
    );

    assert!(
        source.contains("smelt_capture_recursive"),
        "test callbacks must retain recursive callable result storage: {source}"
    );
}

#[test]
fn invokes_void_erased_rest_closures_before_returning_null() {
    let source = source_for(
        r#"
type RestCallback = (...args: unknown[]) => void;

export function make(): RestCallback {
  return (...args: unknown[]) => {
    args.push("called");
  };
}
"#,
    );

    assert!(
        source.contains("let smelt_callback = ::std::rc::Rc::new(")
            && source.contains(
                "move |smelt_args: Vec<SmeltUnknown>| { (smelt_callback)(smelt_args"
            )
            && !source.contains("SmeltErasedFunction { callback: { let smelt_callback = ::std::rc::Rc::new(::std::cell::RefCell::new("),
        "an erased void callback wrapper must invoke its source closure through a reentrant handle: {source}"
    );
    assert!(
        source.contains("SmeltUnknown::Null"),
        "an erased void callback wrapper must still produce undefined/null ABI output: {source}"
    );
}

#[test]
fn emits_manual_default_for_function_field_interfaces() {
    let source = source_for(
        r"
interface Localize<T> {
  ordinalNumber: (value: number) => string;
  value?: T;
}

function read(localize: Localize<string>): string {
  return localize.ordinalNumber(1);
}
",
    );

    assert!(
        source
            .contains("impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> Default for Localize<T>"),
        "{source}"
    );
    assert!(
        source.contains("ordinal_number: { let smelt_default_callback:"),
        "{source}"
    );
    assert!(
        source.contains("_smelt_phantom: ::std::marker::PhantomData,"),
        "{source}"
    );
    assert!(
        source
            .contains("impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> ::std::fmt::Debug for Localize<T>"),
        "{source}"
    );
}

#[test]
fn self_recursive_closure_captures_its_own_cell_weakly() {
    // A self-recursive closure and its cell must not hold each other with `Rc`, or
    // the cycle leaks the closure and everything it captured on every call. The
    // absence assertion is the load-bearing one: no strong self-capture means the
    // cycle cannot form, which is a static property rather than a measurement.
    let source = source_for(
        r"
export function collect(rows: number[][]): number[] {
  const result: number[] = [];
  const recurse = (items: number[][], depth: number): void => {
    for (const item of items) {
      if (depth > 0) {
        recurse([item], depth - 1);
      } else {
        result.push(item[0]);
      }
    }
  };
  recurse(rows, 1);
  return result;
}
",
    );

    // The closure captures a `Weak` to its own cell...
    assert!(
        source.contains("let smelt_capture_recurse = ::std::rc::Rc::downgrade(&smelt_capture_recurse);"),
        "{source}"
    );
    // ...and upgrades it to call itself.
    assert!(
        source.contains("smelt_capture_recurse.upgrade().expect("),
        "{source}"
    );
    // The frame keeps the strong owner, so the cell outlives every call through it.
    assert!(
        source.contains("(*smelt_capture_recurse.borrow_mut()) ="),
        "{source}"
    );
    // No strong self-capture: the cycle cannot form.
    assert!(
        !source.contains("let smelt_capture_recurse = smelt_capture_recurse.clone();"),
        "{source}"
    );
}

#[test]
fn preserves_mutable_structural_parameters_forwarded_to_callbacks() {
    let source = source_for(
        r"
interface Flags {
  era?: number;
}

interface Setter {
  set: (flags: Flags) => number | [number, Flags];
}

function forward(flags: Flags, setter: Setter): number | [number, Flags] {
  return setter.set(flags);
}

function run(setter: Setter): number {
  const flags: Flags = {};
  forward(flags, setter);
  return flags.era!;
}
",
    );

    assert!(
        source.contains("fn forward(mut flags: &mut Flags"),
        "forwarding into a mutable callback must preserve the caller's object: {source}"
    );
    assert!(
        source.contains("(setter.set.clone())(flags)"),
        "the forwarded callback must receive the existing mutable reference: {source}"
    );
}

#[test]
fn emits_default_callback_for_erased_non_function_callback_cast() {
    let source = source_for(
        r"
function invoke(callback: (value: number) => number, fallback?: (value: number) => number): number {
  const chosen = (undefined as unknown) as (value: number) => number;
  return chosen(1);
}
",
    );

    assert!(source.contains("smelt_default_callback"), "{source}");
}

#[test]
fn owns_callback_params_that_escape_through_unknown_values() {
    let source = source_for(
        r"
function expose(callback: (value: unknown) => unknown): unknown {
  return { callback };
}
",
    );

    assert!(
        source.contains("fn expose(callback: ::std::rc::Rc<dyn Fn(&SmeltUnknown) -> SmeltUnknown>)"),
        "{source}"
    );
    assert!(!source.contains("fn expose(callback: &dyn Fn"), "{source}");
    assert!(source.contains("SmeltUnknown::Function"), "{source}");
}

#[test]
fn function_item_value_references_share_one_accessor() {
    // JavaScript reference identity requires every reference to the same named
    // function value to be the SAME runtime value. The frontend wraps a
    // function item used as a value in a fresh forwarding closure per
    // reference, so the two `func1` arguments below each lower to their own
    // `ExprKind::Closure`. Both wrappers are tagged with the same source item,
    // so when they are erased to `SmeltUnknown` codegen must route them through
    // one per-item compile-time accessor `__smelt_fn_value_<key>()` with a
    // shared key, and emit that accessor (with its `OnceCell` cache) exactly
    // once for the crate.
    let source = source_for(
        r"
function func1(): void {}
function takesTwo(a: unknown, b: unknown): boolean { return true; }
const r = takesTwo(func1, func1);
",
    );

    // Both func1 arguments live in `fn main`; both must call the SAME accessor.
    // Scope the call-count to the `main` body only (up to the accessor's own
    // definition, which is appended after the function loop) so the definition's
    // `fn __smelt_fn_value_<key>()` header is not miscounted as a call site.
    let main_body = source
        .split_once("fn main")
        .map(|(_prelude, body)| body)
        .and_then(|body| body.split_once("\nfn __smelt_fn_value_"))
        .map(|(main_body, _accessors)| main_body)
        .expect("emitted source has a main function and an accessor definition");
    let first_call = main_body
        .match_indices("__smelt_fn_value_")
        .next()
        .map(|(index, _)| index)
        .expect("a function-item value accessor call is emitted");
    let key = main_body[first_call + "__smelt_fn_value_".len()..]
        .split('(')
        .next()
        .map(str::to_owned)
        .expect("the accessor call has a key");
    let accessor_call = format!("__smelt_fn_value_{key}()");
    let call_count = main_body.matches(&accessor_call).count();
    assert_eq!(
        call_count, 2,
        "both func1 references must call the same accessor {accessor_call}; got {call_count}\n{source}"
    );

    // The accessor must be defined exactly once, lazily caching ONE erased
    // `SmeltUnknown::Function` value behind a `OnceCell`.
    let definition = format!("fn __smelt_fn_value_{key}() -> SmeltUnknown {{");
    assert_eq!(
        source.matches(&definition).count(),
        1,
        "the accessor must be defined exactly once\n{source}"
    );
    let accessor_def = source
        .split_once(&definition)
        .map(|(_, tail)| tail)
        .expect("the accessor definition is emitted");
    assert!(
        accessor_def.contains("::std::cell::OnceCell"),
        "the accessor must cache its value in a OnceCell\n{source}"
    );
    assert!(
        accessor_def.contains("SmeltUnknown::Function"),
        "the accessor must build an erased SmeltUnknown::Function value\n{source}"
    );

}

#[test]
fn function_item_erased_fn_references_share_one_accessor() {
    // A named function item used as a value in a TYPED erased-rest function
    // context (e.g. Remeda's `doNothing`) lowers to a concrete
    // `SmeltErasedFunction`, not the `SmeltUnknown` erased value. Building it
    // inline mints a fresh callback `Rc` per evaluation, so two references would
    // never be `Rc::ptr_eq`. JavaScript returns one shared singleton. Both
    // references below carry the same source item key, so codegen must route the
    // build through one per-item `__smelt_fn_erased_<key>()` accessor (with a
    // `OnceCell<SmeltErasedFunction>` cache) emitted exactly once for the crate.
    let source = source_for(
        r"
function doesNothing(...args: unknown[]): void {}
function takesTwo(a: (...args: unknown[]) => void, b: (...args: unknown[]) => void): boolean { return true; }
const r = takesTwo(doesNothing, doesNothing);
",
    );

    // Both arguments live in `fn main`; both must call the SAME accessor. Scope
    // the call count to the `main` body only (up to the accessor's own
    // definition appended after the function loop) so the `fn __smelt_fn_erased_`
    // header is not miscounted as a call site.
    let main_body = source
        .split_once("fn main")
        .map(|(_prelude, body)| body)
        .and_then(|body| body.split_once("\nfn __smelt_fn_erased_"))
        .map(|(main_body, _accessors)| main_body)
        .expect("emitted source has a main function and an erased-fn accessor definition");
    let first_call = main_body
        .match_indices("__smelt_fn_erased_")
        .next()
        .map(|(index, _)| index)
        .expect("a function-item erased-fn accessor call is emitted");
    let key = main_body[first_call + "__smelt_fn_erased_".len()..]
        .split('(')
        .next()
        .map(str::to_owned)
        .expect("the accessor call has a key");
    let accessor_call = format!("__smelt_fn_erased_{key}()");
    let call_count = main_body.matches(&accessor_call).count();
    assert_eq!(
        call_count, 2,
        "both doesNothing references must call the same accessor {accessor_call}; got {call_count}\n{source}"
    );

    // The accessor must be defined exactly once, lazily caching ONE
    // `SmeltErasedFunction` behind a `OnceCell` so every call shares one inner
    // callback `Rc`.
    let definition = format!("fn __smelt_fn_erased_{key}() -> SmeltErasedFunction {{");
    assert_eq!(
        source.matches(&definition).count(),
        1,
        "the erased-fn accessor must be defined exactly once\n{source}"
    );
    let accessor_def = source
        .split_once(&definition)
        .map(|(_, tail)| tail)
        .expect("the accessor definition is emitted");
    assert!(
        accessor_def.contains("::std::cell::OnceCell"),
        "the accessor must cache its value in a OnceCell\n{source}"
    );
    assert!(
        accessor_def.contains("SmeltErasedFunction {"),
        "the accessor must build a SmeltErasedFunction value\n{source}"
    );
}

#[test]
fn user_arrow_forwarding_a_function_emits_no_accessor() {
    // When a user arrow forwards to a named function (`(x) => func1(x)`), the
    // arrow itself stays a fresh closure value and the inner `func1(x)` is a
    // direct typed call, not a function-item-as-value wrapper erased to
    // `SmeltUnknown`. Identity is only stabilized at the erase site, so no
    // per-item accessor is emitted for this shape.
    let source = source_for(
        r"
function func1(x: number): number { return x; }
function takesOne(a: (value: number) => number): boolean { return true; }
const r = takesOne((x) => func1(x));
",
    );

    let body = source
        .split_once("fn main")
        .map(|(_prelude, body)| body)
        .expect("emitted source has a main function");
    assert!(
        !source.contains("__smelt_fn_value_"),
        "no function-item value accessor should be emitted for a forwarding arrow\n{source}"
    );
    // The outer arrow is still a plain closure value.
    assert!(
        body.contains("::std::rc::Rc::new(|closure_arg_0"),
        "the user arrow must remain a plain closure value\n{source}"
    );
}

#[test]
fn lowers_imported_function_value_as_filter_callback() {
    // An array method whose callback is a bare imported function value (a named
    // local, not an inline arrow) must lower to a real callback closure instead
    // of failing with "array callback local callback `X` is not in scope". The
    // import is opaque here, so the closure body resolves like a direct call:
    // it produces a conservative value that the predicate path coerces to bool.
    let source = source_for(
        r#"
import { isNotNil } from "./isNotNil";
function run(): unknown {
  const arr = [1, 2, 3];
  return arr.filter(isNotNil);
}
"#,
    );

    assert!(
        source.contains(".filter_map(|(smelt_index, smelt_item)|"),
        "imported filter callback should lower to a filtering closure\n{source}"
    );
    assert!(
        source.contains("(smelt_callback)(smelt_item.clone(), smelt_index as i64,"),
        "the closure body should call the resolved callback value\n{source}"
    );
}

#[test]
fn lowers_foreach_callback_without_fixed_item_parameter() {
    // `forEach((...args) => ...)` and `forEach(() => ...)` have no fixed item
    // parameter, so the statement-loop shortcut declines and the general
    // callback lowering handles them instead of failing with "array forEach
    // callbacks require an item parameter".
    let source = source_for(
        r"
export function run(sources: unknown[], apply: (...args: unknown[]) => void): void {
  sources.forEach((...args: unknown[]) => apply(...args));
}
export function tick(values: number[], onTick: () => void): void {
  values.forEach(() => onTick());
}
",
    );
    assert!(source.contains("fn run("), "{source}");
    assert!(source.contains("fn tick("), "{source}");
}

#[test]
fn lowers_lodash_two_argument_collection_callback_form() {
    // `import * as _ from "lodash"; _.map(values, cb)` is the lodash
    // free-function form: collection first, iteratee second. The receiver is an
    // opaque imported namespace, so the call stays a placeholder, but both the
    // collection and the iteratee must lower (no "require exactly one callback
    // argument" rejection of the trailing iteratee).
    let source = source_for(
        r#"
import * as _ from "lodash";
export function run(values: number[]): void {
  _.map(values, value => value + 1);
  _.some(values, value => value > 0);
}
"#,
    );
    assert!(source.contains("fn run("), "{source}");
}

/// A genuinely irreconcilable named `reduce` callback — a `string`
/// accumulator with a `boolean` return type that shares no common concrete
/// shape — is still rejected, so the reconciliation does not silently widen
/// unrelated types through a `SmeltUnknown` shortcut.
#[test]
fn reduce_named_callback_rejects_irreconcilable_return_type() {
    let mut ctx = HirCtx::new();
    let result = to_hir(
        r#"
function step(acc: string, value: number): boolean {
  return true;
}
export function run(values: number[]): string {
  return values.reduce(step, "seed");
}
"#,
        FileId(0),
        &mut ctx,
    );
    let errors = result.expect_err("irreconcilable reduce callback must be rejected");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("callback returns an unsupported type")),
        "expected the reduce return-type rejection, got: {errors:?}"
    );
}

#[test]
fn erased_callback_optional_return_adapts_through_checked_boundary() {
    // The result of invoking an erased callable adapter whose source return type is
    // an optional (`boolean | void`) is a bare `SmeltUnknown`, so it is adapted through
    // a single checked nullish guard rather than `Option` methods.
    let source = source_for(
        r"
function run(cb: (a: unknown, b: unknown) => boolean | void, x: unknown, y: unknown): boolean {
  const r = cb(x, y);
  if (r !== undefined) {
    return r;
  }
  return false;
}
export function useRun(fn: (...args: unknown[]) => boolean | void): boolean {
  return run(fn, 1, 2);
}
",
    );
    assert!(
        source.contains("smelt_unknown_is_nullish")
            || source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => None"),
        "erased optional-return adapter should use a checked nullish boundary: {source}"
    );
}

#[test]
fn callable_interface_call_slot_default_is_erased_function() {
    // A callable interface's synthetic `__smelt_call` storage field is declared
    // `SmeltErasedFunction`; its `Default` must initialize the same type.
    let source = source_for(
        r"
interface Formatter {
  (...args: unknown[]): unknown;
  label: string;
}
export function makeFormatter(): Formatter {
  return { label: 'x' } as Formatter;
}
",
    );
    assert!(
        source.contains("__smelt_call: SmeltErasedFunction::default()"),
        "callable-interface `__smelt_call` default must be a SmeltErasedFunction, \
         not an Rc<dyn Fn> closure: {source}"
    );
}

/// Comparing a combinator result (already an erased `Rc<dyn Fn(...) -> _>`)
/// against a locally bound concrete closure by identity must coerce both
/// operands to the common `Rc<dyn Fn(...)>` type before `Rc::ptr_eq`, or the
/// two distinct `Rc<T>` types fail to unify (E0308).
#[test]
fn function_ptr_eq_coerces_operands_to_common_dyn_type() {
    let source = source_for(
        r"
export function run(fns: Array<() => unknown>): boolean {
  const a = fns[0];
  const b = fns[1];
  return a === b;
}
",
    );

    // The comparison goes through `smelt_same_function_identity` rather than a
    // bare `Rc::ptr_eq`: erasing a callable builds a fresh forwarding adapter, so
    // two handles on one source function are distinct allocations and pointer
    // equality alone reports `f === f` as false. The operand coercion this test
    // was written to protect is unchanged — both sides are still bound at a
    // common dyn-Fn type before being compared.
    assert!(
        source.contains("smelt_same_function_identity(&{ let smelt_lhs_fn:")
            && source.contains("let smelt_rhs_fn:"),
        "function identity comparison must coerce both operands to a common \
         dyn-Fn type before comparing identity\n{source}"
    );
}

/// An adapted callback that is only *called* and *cloned* binds without `mut`,
/// matching the erased `.call` path.
#[test]
fn adapted_callback_binds_without_mut() {
    let source = source_for(
        r"
function adapt(
  callback: (value: unknown) => { next: unknown },
): (value: unknown, index: number, data: unknown[]) => unknown {
  return callback;
}
",
    );
    assert!(
        source.contains("let _smelt_adapted_callback = callback.clone();"),
        "the adapted callback must bind without mut: {source}"
    );
    assert!(
        !source.contains("let mut _smelt_adapted_callback"),
        "the adapted callback must not be spuriously mut: {source}"
    );
}

#[test]
fn a_function_not_reading_arguments_keeps_its_declared_arity() {
    // The variadic rewrite is scoped to functions that actually read `arguments`;
    // everything else keeps its declared parameters, so the change costs nothing
    // for the overwhelming majority of code. An arrow body inside the function does
    // NOT count as reading its own `arguments` unless it mentions it — but a nested
    // non-arrow `function` reading `arguments` reads its OWN, so the outer function
    // stays fixed-arity.
    let generated = source_for(
        r"
export function outer(a: number, b: number): number {
  function inner(_x: unknown): unknown {
    return arguments;
  }
  const unused = inner(a);
  return a + b;
}
",
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    assert!(
        program.contains("fn outer(a: f64, b: f64)"),
        "a function that never mentions `arguments` must keep its parameters:\n{generated}"
    );
}
