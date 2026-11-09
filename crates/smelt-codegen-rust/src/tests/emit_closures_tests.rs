//! Generated Rust for closures and callbacks.

use super::*;

#[test]
fn captures_callable_factory_result_for_recursive_callback_invocation() {
    let source = source_for(
        r#"
type Fn = (value: string) => void;

function wrap(callback: Fn): Fn {
  return callback;
}

export function run(): void {
  const recursive = wrap((value: string) => {
    recursive(value);
  });
  recursive("a");
}
"#,
    );

    assert!(
        source.contains("smelt_capture_recursive"),
        "recursive callable result should be captured by its callback: {source}"
    );
    assert!(
        !source.contains("SmeltRecord::from([])"),
        "recursive callback should not fall back to an empty callable object: {source}"
    );
}

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
fn does_not_rewrap_equivalent_erased_rest_function_shapes() {
    let source = source_for(
        r#"
type AnyRest = (...args: unknown[]) => unknown;
type StringRest = (...args: unknown[]) => string;

function read(): StringRest {
  return (...args: unknown[]) => String(args.length);
}

const callback: AnyRest = read();
const value = callback("x");
"#,
    );

    assert!(
        !source.contains("let smelt_adapted: SmeltErasedFunction = ::std::rc::Rc::new"),
        "erased-rest callbacks with equivalent ABI should pass through unchanged\n{source}"
    );
    assert!(
        source.contains("let callback: SmeltErasedFunction = read().clone();")
            || source.contains("let callback: SmeltErasedFunction;")
                && source.contains("callback = _smelt_tmp_")
                && source.contains(".clone();"),
        "erased-rest callback assignment should preserve the shared SmeltErasedFunction\n{source}"
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
    // A self-recursive closure and its cell used to hold each other with `Rc`, a
    // reference cycle that leaked the closure and everything it captured on EVERY
    // call. es-toolkit's `flatten` grew ~21 KiB per call without bound; after this
    // lowering its peak RSS is flat in call count (396 MiB -> 4.6 MiB at ~2k calls).
    // See `benchmarks/FINDINGS.md` finding #2.
    //
    // The absence assertion is the load-bearing one: no strong self-capture means the
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
fn derives_clone_for_function_bearing_interface_storage() {
    let source = source_for(
        r"
interface Callbacks {
  run: () => number;
}
function copy(callbacks: Callbacks): Callbacks {
  return callbacks;
}
",
    );

    assert!(
        source.contains("#[derive(Clone)]\n#[allow(dead_code)]\nstruct Callbacks"),
        "{source}"
    );
}

#[test]
fn emits_closure_call_result_for_optional_destination() {
    let source = source_for(
        r"
function maybeCall(callback: (value: number) => number): number | undefined {
  const value: number | undefined = callback(1);
  return value;
}
",
    );

    assert!(
        source.contains("let value: Option<f64> = Some("),
        "{source}"
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
fn emits_call_bodied_local_arrow_as_real_closure_body() {
    let source = source_for(
        r"
function makeDataLast(fn: (value: number, extra: number) => number, extra: number): (value: number) => number {
  const dataLast = (data: number): number => fn(data, extra);
  return dataLast;
}
",
    );

    assert!(
        source.contains("fn_: ::std::rc::Rc<dyn Fn(f64, f64) -> f64>"),
        "{source}"
    );
    assert!(
        source.contains("(fn_)(closure_arg_0, extra)"),
        "{source}"
    );
}

#[test]
fn emits_callback_dynamic_index_with_non_null_assertion() {
    let source = source_for(
        r"
function sample<T>(data: readonly T[]): T[] {
  const sampleIndices = new Set<number>();
  return [...sampleIndices].sort((a, b) => a - b).map((index) => data[index]!);
}
",
    );

    assert!(source.contains("closure_arg_0 as i64"), "{source}");
    assert!(source.contains("usize::try_from(smelt_normalized)"), "{source}");
}

#[test]
fn emits_sort_with_comparator_function_value() {
    let source = source_for(
        r"
const sortByImplementation = <T>(
  data: readonly T[],
  compareFn: (left: T, right: T) => number,
): T[] => [...data].sort(compareFn);
",
    );

    assert!(
        source.contains("(smelt_comparator)(&(smelt_left.clone()), &(smelt_right.clone()))"),
        "{source}"
    );
    assert!(
        source.contains("if smelt_ordering < 0.0 { std::cmp::Ordering::Less }"),
        "{source}"
    );
}

#[test]
fn packs_rest_arguments_for_normal_closure_calls() {
    let source = source_for(
        r#"
export function makeIdentity(): (first: unknown, ...rest: unknown[]) => unknown {
  return (first: unknown) => first;
}

export function run(): unknown {
  const identity = makeIdentity();
  return identity("hello");
}
"#,
    );

    assert!(
        source.contains("(identity)(&(SmeltUnknown::String(\"hello\".into())), _smelt_tmp_2)")
            && source.contains("_smelt_tmp_2 = Into::<SmeltList<_>>::into(SmeltList::from("),
        "normal closure calls should preserve fixed arguments and pack an empty rest vector: {source}"
    );
}

#[test]
fn emits_non_escaping_closure_that_captures_borrowed_callback_param() {
    let source = source_for(
        r"
function visit<T>(
  data: readonly T[],
  callback: (value: T, index: number, data: readonly T[]) => boolean,
): number {
  return callback(data[0]!, 0, data) ? 1 : 0;
}

function indicesSeen(
  items: readonly unknown[],
  predicate: (item: unknown, index: number) => boolean,
): number[] {
  const indices: number[] = [];
  visit(items, (pivot, index) => {
    indices.push(index);
    return predicate(pivot, index);
  });
  return indices;
}
",
    );

    // `closure_arg_0` is itself spelled `&SmeltUnknown` (the contextual `dyn Fn`
    // passes it by shared reference), and `predicate` takes `&SmeltUnknown`, so
    // the binding is forwarded as-is. Borrowing it again would hand the callee a
    // `&&SmeltUnknown` that only compiles through deref coercion.
    assert!(
        source.contains("predicate(closure_arg_0, closure_arg_1)"),
        "a borrowed callback parameter should forward without a re-borrow: {source}"
    );
    assert!(
        source.contains(
            "let smelt_push_item = closure_arg_1; (*smelt_capture_indices.borrow()).borrow_mut().push(smelt_push_item)"
        ),
        "captured push should mutate the outer vector storage: {source}"
    );
    assert!(
        !source.contains("smelt_default_callback"),
        "non-escaping callback parameter capture should not default: {source}"
    );
}

#[test]
fn boxes_returned_function_values_even_when_mir_types_match() {
    let source = source_for(
        r"
function makeMapper(): (value: number) => number {
  const mapper = (value: number) => value + 1;
  return mapper;
}
",
    );

    assert!(
        source.contains("fn make_mapper() -> ::std::rc::Rc<dyn Fn(f64) -> f64>"),
        "{source}"
    );
    // The returned value is the BINDING, not a second closure: a function value
    // has observable JavaScript identity, so `mapper` is materialized once and
    // read back. Either spelling of that read is fine; what matters is that the
    // return is an `Rc<dyn Fn>` (asserted above) rather than an unboxed closure.
    assert!(
        source.contains("return ::std::rc::Rc::new(")
            || source.contains("return _smelt_tmp_2.clone()")
            || source.contains("return mapper.clone()"),
        "{source}"
    );
}

#[test]
fn coerces_function_adapter_return_values_to_target_return_type() {
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
        source.contains("SmeltUnknown::Object(SmeltObject::from_unknown_record(((_smelt_adapted_callback)(smelt_arg0)).clone()))"),
        "{source}"
    );
}

#[test]
fn coerces_function_adapter_forwarded_arguments_to_source_param_types() {
    let source = source_for(
        r"
function adapt(
  callback: (values: unknown[], index?: number) => unknown,
): (value: unknown, index: number, data: unknown[]) => unknown {
  return callback;
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Array(values) => values"),
        "{source}"
    );
    assert!(source.contains("Some(smelt_arg1)"), "{source}");
}

#[test]
fn wraps_function_return_values_from_unknown_adapters() {
    let source = source_for(
        r"
function outer(make: () => (value: unknown) => unknown): unknown {
  return [make];
}
",
    );

    assert!(source.contains("SmeltUnknown::Function"), "{source}");
    assert!(!source.contains("()).into_smelt_unknown()"), "{source}");
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
fn flattens_optional_chain_over_optional_callback_field() {
    let source = source_for(
        r"
interface Options {
  cb?: (value: number) => number;
}

export function read(options?: Options): ((value: number) => number) | undefined {
  return options?.cb;
}
",
    );

    assert!(source.contains(".as_ref().and_then("), "{source}");
    assert!(!source.contains("Option<Option<::std::rc::Rc"), "{source}");
}

#[test]
fn optional_call_on_absent_callback_short_circuits_to_none() {
    // `customizer?.(value)` where `customizer` is an absent `Option<Fn>` must
    // short-circuit to `undefined` (rendered `None` / `.map(..)`), NOT coerce the
    // callee into a null-returning default callback that is then invoked
    // unconditionally. The latter regression made `cloneDeepWith`/`cloneDeep`
    // return `null` early for arrays and objects (es-toolkit clone family).
    let source = source_for(
        r"
export function apply(customizer?: (value: number) => number): number | undefined {
  return customizer?.(1);
}
",
    );

    // The callee stays an `Option` and is mapped, so an absent callee yields `None`.
    assert!(
        source.contains(".map(|smelt_function|"),
        "optional call must map over the optional callee: {source}"
    );
    // It must NOT substitute a default callback and call it unconditionally.
    assert!(
        !source.contains("smelt_default_callback"),
        "optional call must not fall back to a null-returning default callback: {source}"
    );
}

#[test]
fn emits_callback_typeof_unknown_through_shared_static_str_helper() {
    let source = source_for(
        r"
function mapType(values: unknown[]): string[] {
  return values.map((item) => typeof item);
}
",
    );

    // The tag-to-spelling table is a `&'static str` lookup in the prelude, not a
    // `String`-allocating match re-inlined at every `typeof` in the source.
    assert!(
        source.contains("fn smelt_typeof(value: &SmeltUnknown) -> &'static str"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Symbol(_) => \"symbol\""),
        "{source}"
    );
    assert!(
        source.contains(
            "SmeltUnknown::Null | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Promise(_) => \"object\""
        ),
        "{source}"
    );
    assert!(!source.contains("\"symbol\".to_owned()") && !source.contains("JsString::from(\"symbol\")"), "{source}");
    // Exactly one copy of the table: the helper's own body.
    assert_eq!(
        source
            .matches("SmeltUnknown::Symbol(_) => \"symbol\"")
            .count(),
        1,
        "{source}"
    );
    // The site reads the value by shared reference rather than cloning it. The
    // callback parameter is already spelled `&SmeltUnknown`, so it is passed
    // through instead of being borrowed a second time.
    assert!(
        source.contains("JsString::from(smelt_typeof(closure_arg_0))"),
        "{source}"
    );
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

    // The old runtime cache helper must be gone entirely.
    assert!(
        !source.contains("smelt_function_item_value"),
        "the removed runtime cache helper must not be emitted\n{source}"
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
fn lowers_function_expression_value_rest_parameters() {
    // A `function (...args)` expression used as a returned value (an object-valued
    // function expression position) must lower its rest parameter the same way
    // top-level functions and arrow expressions do, packing trailing arguments
    // into a single list parameter instead of erroring out.
    let source = source_for(
        r"
export function nthArgValue(n: number): (...args: number[]) => number {
  return function (...args: number[]): number {
    return args[n];
  };
}
",
    );

    // The returned closure must accept a single packed rest list
    // (`SmeltList<f64>`), proving the rest parameter was lowered into one list
    // closure parameter rather than rejected.
    assert!(
        source.contains("Fn(SmeltList<f64>) -> f64"),
        "function-expression rest parameters must lower to a packed list closure type\n{source}"
    );
    assert!(
        source.contains("move |closure_arg_0: SmeltList<f64>|"),
        "the returned closure must bind the rest list as its single parameter\n{source}"
    );
    assert!(
        !source.contains("rest parameters are not lowered"),
        "the object-valued function-expression rest blocker must be gone\n{source}"
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
fn lowers_numeric_truthy_condition_inside_callback() {
    // A non-boolean numeric value used as a callback condition (the common
    // `(value, index) => index ? a : b` index-guard idiom) lowers to an
    // explicit `!= 0` test rather than rejecting the callback.
    let source = source_for(
        r"
function pick(values: number[]): number[] {
  return values.map((value, index) => (index ? value * 2 : value));
}
",
    );

    assert!(
        source.contains("!= 0"),
        "numeric callback condition must lower to a non-zero test: {source}"
    );
}

#[test]
fn falls_back_to_closure_body_for_sort_comparator_with_loop() {
    // An `Array.prototype.sort` comparator whose body uses a `for` loop must
    // retry through full closure-body lowering rather than rejecting the file.
    let source = source_for(
        r"
function order(items: number[][]): number[][] {
  return items.slice().sort((a, b) => {
    for (let i = 0; i < a.length; i++) {
      if (a[i] !== b[i]) {
        return a[i] - b[i];
      }
    }
    return 0;
  });
}
",
    );

    assert!(source.contains("sort"), "{source}");
}

#[test]
fn lowers_erased_local_value_passed_as_named_callback() {
    // A local holding a callable value whose static type is erased (`any`) and
    // handed to an array method as a named callback (`values.map(fn)`) lowers to
    // a wrapper closure that calls the captured local, instead of rejecting it
    // for not being an inline arrow / not having a callback entry.
    let source = source_for(
        r"
function apply(values: string[], fn: any): string[] {
  return values.map(fn);
}
",
    );

    assert!(source.contains(".map("), "{source}");
}

#[test]
fn lowers_callback_that_reassigns_its_parameter() {
    // A callback that reassigns its own parameter is not representable in the
    // compact side-effect-free callback IR, but the full closure-body path
    // makes parameters mutable locals. The fallback must retry there instead of
    // failing with "callback parameter assignment is not supported yet".
    let source = source_for(
        r"
export function run(values: number[]): number[] {
  return values.map(value => {
    if (value < 0) {
      value = 0;
    }
    return value + 1;
  });
}
",
    );
    assert!(source.contains("fn run("), "{source}");
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

#[test]
fn falls_back_to_closure_body_for_function_expression_callback() {
    // A `function`-expression callback whose body uses a statement form the
    // compact callback IR cannot represent (here `try`/`catch`) must retry
    // through full closure-body lowering, exactly as the arrow form does. Before
    // issue #86 the closure-body fallback was gated on the argument being an
    // arrow, so this rejected the file.
    let source = source_for(
        r"
function run(values: string[]): Array<string | undefined> {
  return values.map(function (value) {
    try {
      return value;
    } catch (error) {
      return undefined;
    }
  });
}
",
    );

    assert!(source.contains(".map("), "{source}");
}

/// Issue #113: a named `reduce` callback whose declared accumulator parameter is
/// wider than both the seed and the callback return type resolves the reduce
/// result to that declared accumulator type (TypeScript's `U`). The callback
/// returns a `number` that must be coerced back into the `string | number`
/// accumulator on each fold step, so the emitted fold both seeds and folds
/// through the concrete union.
#[test]
fn reduce_named_callback_uses_declared_accumulator_type() {
    let source = source_for(
        r#"
function step(acc: string | number, value: number): number {
  return value;
}
export function run(values: number[]): string | number {
  return values.reduce(step, "seed");
}
"#,
    );
    assert!(source.contains(".iter().enumerate().fold("), "{source}");
    assert!(source.contains("(smelt_callback)(smelt_acc, smelt_item)"), "{source}");
    assert!(source.contains("fn run("), "{source}");
}

/// Issue #113: a named `reduce` callback that widens a concrete seed into an
/// erased `unknown` accumulator (a genuine dynamic boundary) still lowers; the
/// accumulator is the callback's `unknown` return type and the numeric seed is
/// coerced into it.
#[test]
fn reduce_named_callback_widens_to_unknown_accumulator() {
    let source = source_for(
        r"
function step(acc: number, value: number): unknown {
  return acc + value;
}
export function run(values: number[]): unknown {
  return values.reduce(step, 0);
}
",
    );
    assert!(source.contains(".iter().enumerate().fold("), "{source}");
    assert!(source.contains("fn run("), "{source}");
}

/// Issue #113: a genuinely irreconcilable named `reduce` callback — a `string`
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
fn emits_member_store_for_compound_callback_assignment() {
    // `row[0] += suffix` inside a `.map` callback is a member-target compound
    // store; the compact callback IR cannot represent it, so the arrow retries
    // through full closure-body lowering, which emits a real indexed store
    // instead of silently dropping the mutation (or rejecting the file with
    // "callback assignment targets must be captured locals").
    let source = source_for(
        r"
function tag(rows: string[][], suffix: string): string[][] {
  return rows.map(row => {
    row[0] += suffix;
    return row;
  });
}
",
    );

    assert!(
        source.contains("closure_arg_0.borrow_mut()[smelt_assign_index] ="),
        "the compound member assignment should emit an indexed store into the row\n{source}"
    );
    assert!(
        !source.contains("callback assignment targets must be captured locals"),
        "the member-assignment blocker must be gone\n{source}"
    );
}

#[test]
fn callback_parameter_adapter_reborrows_immutably() {
    // Forwarding a borrowed callback parameter to a helper that expects a
    // different callback arity builds a wrapper closure. The wrapper reborrows
    // the parameter, which is bound as an immutable `&dyn Fn`; a `&mut *`
    // reborrow through that shared reference fails to compile (E0596), so the
    // adapter must reborrow immutably with `&*`.
    let source = source_for(
        r"
function uniqBy<T>(arr: T[], mapper: (item: T, index: number, arr: T[]) => unknown): T[] {
  return arr;
}
export function unionBy<T>(arr1: T[], arr2: T[], mapper: (item: T) => unknown): T[] {
  return uniqBy([...arr1, ...arr2], mapper);
}
",
    );

    assert!(
        source.contains("&*mapper"),
        "the callback adapter should reborrow the parameter immutably\n{source}"
    );
    assert!(
        !source.contains("&mut *mapper"),
        "the callback adapter must not mutably reborrow an immutable `&dyn Fn`\n{source}"
    );
}

#[test]
fn borrowed_rest_adapter_binds_owned_callback() {
    // Forwarding an owned callback value to a helper that expects an erased rest
    // callback builds a borrowed (`&mut`) wrapper closure whose body refers to a
    // `smelt_callback` binding. When the source is an owned value (not a borrowed
    // function parameter), the adapter must introduce that binding inside the
    // borrowed temporary, otherwise `smelt_callback` is unresolved (E0425).
    let source = source_for(
        r"
function unzipWith(arrays: number[][], iteratee: (...values: unknown[]) => unknown): unknown[] {
  const result: unknown[] = [];
  for (const group of arrays) {
    result.push(iteratee(group[0], group[1], group[2]));
  }
  return result;
}
export function run(zipped: number[][]): unknown[] {
  return unzipWith(zipped, (item: number, item2: number, item3: number) => item + item2 + item3);
}
",
    );

    assert!(
        source.contains("&mut { let smelt_callback ="),
        "a borrowed owned-callback adapter must bind smelt_callback\n{source}"
    );
}

#[test]
fn erased_rest_function_parameter_is_called_directly_not_via_fn_traits() {
    // Regression: an erased-rest (`(...args) => unknown`) function parameter is
    // emitted as a bare `&dyn Fn(...)` handle. Invoking it as `func.call(args)`
    // resolved to the unstable `Fn::call` trait method (E0658) and expected a
    // tuple argument (E0308). A function-parameter callee must use direct call
    // syntax, taking precedence over the erased-rest inherent `.call()`.
    let source = source_for(
        r"
export function invokeWith(func: (...args: unknown[]) => unknown, args: unknown[]): unknown {
  return func(...args);
}
",
    );

    assert!(
        source.contains("fn invoke_with"),
        "expected the function to be emitted\n{source}"
    );
    let body = &source[source.find("fn invoke_with").unwrap()..];
    assert!(
        !body.contains(".call("),
        "an erased-rest function parameter must not be invoked via the unstable Fn::call trait method\n{body}"
    );
}

#[test]
fn mutated_parameter_captured_by_escaping_closure_uses_shared_cell() {
    // Regression: a function parameter mutated inside an escaping closure (stored
    // as `Rc<dyn Fn>`) needs a shared `Rc<RefCell<..>>` cell so the closure can
    // mutate it through interior mutability. The capture classifier previously
    // excluded parameters, so the closure body borrowed the parameter as mutable
    // through a shared `Rc` (E0596). This mirrors es-toolkit `after`/`before`.
    let source = source_for(
        r"
export function afterN(n: number, func: () => void): () => void {
  return () => {
    n = n - 1;
    if (n <= 0) {
      func();
    }
  };
}
",
    );
    assert!(
        source.contains("smelt_capture_n"),
        "a parameter mutated inside an escaping closure should use a shared capture cell: {source}"
    );
}

#[test]
fn erased_callback_optional_return_adapts_through_checked_boundary() {
    // Regression: invoking an erased callable adapter whose source return type is
    // an optional (`boolean | void`) produced `.call(..).clone().map(..)`, calling
    // `Option` methods on the bare `SmeltUnknown` the erased call yields (E0599),
    // then double-evaluating the argument-consuming call (E0382). The erased call
    // result must be adapted from `Unknown` through a single checked nullish guard.
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
    // Regression (es-toolkit main.rs): a callable interface's synthetic
    // `__smelt_call` storage field is declared `SmeltErasedFunction`, but the
    // generated `Default` impl initialized it with the concrete-signature
    // `Rc<dyn Fn(..)>` default because `default_value_with_scoped_type_params`
    // lacked the erased-function guard that the field's TYPE text applies. The
    // field type and its default must agree (E0308).
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

#[test]
fn apply_on_typed_function_value_emits_a_call_not_a_null() {
    // Regression (es-toolkit curry/partial/partialRight/flow): `fn.apply(this, args)`
    // on a receiver whose type is a concrete `Type::Function` had no lowering arm --
    // `call` did, `apply` did not -- so the member read fell through to the ordinary
    // (absent) field path and the WHOLE call collapsed to a literal `null`, with no
    // diagnostic. Only an erased (`unknown`) receiver reached the runtime
    // `smelt_function_method(.., "apply")` dispatch, so the identical source line
    // behaved differently depending on whether the callee had kept its static type.
    let source = source_for(
        r"
export function forward(func: (...args: any[]) => any, args: any[]): any {
  return func.apply(null, args);
}
",
    );
    let body = source
        .split("fn forward")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or("");
    assert!(
        !body.contains("return SmeltUnknown::Null;"),
        "`apply` on a typed function must not collapse to a null literal: {source}"
    );
    assert!(
        body.contains("func(args"),
        "`apply` should forward the argument array to the callee: {source}"
    );
}

#[test]
fn borrowed_rest_callback_adapter_binds_smelt_callback() {
    // Regression (es-toolkit template): a non-parameter local callback forwarded
    // by mutable reference into an erased variadic-rest parameter is wrapped in a
    // `move |smelt_args| (smelt_callback)(..)` adapter. The borrowed (`&mut`) path
    // must introduce the `let smelt_callback = ..` binding the closure captures;
    // otherwise the emitted closure references an unbound `smelt_callback`
    // (E0425).
    let source = source_for(
        r#"
function attempt(func: (...args: unknown[]) => unknown): unknown {
  return func();
}
export function useAttempt(): unknown {
  return attempt(() => {
    throw new Error("no");
  });
}
"#,
    );
    assert!(
        source.contains("&mut { let smelt_callback ="),
        "the borrowed rest-callback adapter must bind smelt_callback inside the \
         &mut block it captures: {source}"
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

/// Regression (warning-reduction R1): an adapted callback that is only *called*
/// and *cloned* binds without `mut`, matching the erased `.call` path. The
/// binding used to be hardcoded `let mut _smelt_adapted_callback`.
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

/// Erasing a function returned by an adapted callback must evaluate that
/// callback once; evaluating it again moves non-Copy arguments twice.
#[test]
fn adapted_function_return_is_materialized_before_identity_registration() {
    let source = source_for(
        r"
function adapt(
  callback: (key: string) => (record: Record<string, number>) => number,
): (key: string) => unknown {
  return callback;
}
",
    );

    // The adapted callable is materialized (called) before any identity or
    // callable-object bookkeeping. A callable value narrowed from an object is
    // re-erased back to that object when a registration exists; otherwise its
    // canonical identity is read (before the callable is moved into the
    // adapter) and the origin is registered, so the typed callback survives the
    // erased ABI and both spellings denote one JavaScript function.
    assert!(
        source.contains(
            "let smelt_function_value = (_smelt_adapted_callback)(smelt_arg0); if let Some(smelt_callable_object) = smelt_lookup_callable_object(&smelt_function_value) { smelt_callable_object } else { let smelt_origin_identity = smelt_canonical_function_identity(&smelt_function_value); let smelt_function_origin = smelt_function_value.clone();"
        ),
        "{source}"
    );
}

/// A nested closure whose parameter shadows an outer callback parameter must
/// initialize the capture alias referenced by the generated inner body.
#[test]
fn nested_closure_aliases_shadowed_callback_parameters() {
    let source = source_for(
        r"
export function run(): number {
  const objectize =
    (callback: (value: { num: number }) => number) =>
    (num: number) => callback({ num });
  return objectize(value => value.num)(1);
}
",
    );

    assert!(
        source.contains("let smelt_captured_closure_arg_0 = closure_arg_0.clone();"),
        "{source}"
    );
}

/// A mutable outer parameter whose synthetic name is shadowed needs fresh
/// shared capture storage initialized from that outer parameter.
#[test]
fn mutable_shadowed_parameter_initializes_aliased_capture_cell() {
    let source = source_for(
        r"
export function run(): void {
  const append =
    (items: number[]) =>
    (callback: (value: number[]) => void) => {
      items.push(1);
      callback(items);
    };
  append([])(_items => {});
}
",
    );

    assert!(
        source.contains(
            "let smelt_capture_smelt_captured_closure_arg_0 = ::std::rc::Rc::new(::std::cell::RefCell::new(closure_arg_0.clone()));"
        ),
        "{source}"
    );
}

#[test]
fn erased_apply_call_bind_function_receiver() {
    // `func.apply`/`func.call` on an erased receiver must resolve when `func` is
    // a `SmeltUnknown::Function`, not only when it is an object. The plain
    // object-field read returns `Undefined` for a function receiver, which
    // collapses every invocation to a null callback (the `partial`/`partialRight`
    // regression). The read routes through `smelt_function_method`, which binds
    // the callable with `this`-dropping/argument-spreading semantics.
    let source = source_for(
        r"
export function invokeApply(func: any, args: unknown[]): unknown {
  return func.apply(undefined, args);
}
export function invokeCall(func: any, first: unknown): unknown {
  return func.call(undefined, first);
}
",
    );

    assert!(
        source.contains("smelt_function_method(") && source.contains("\"apply\""),
        "erased `.apply` must route through the function-method binder\n{source}"
    );
    assert!(
        source.contains("\"call\""),
        "erased `.call` must route through the function-method binder\n{source}"
    );
    assert!(
        source.contains("fn smelt_function_method("),
        "the runtime prelude must define the function-method binder\n{source}"
    );
}

/// A closure whose body has an `if` guard that mutates captured locals
/// (`once`: `if (!called) { ret = fn(); called = true; }`) must keep the guard.
/// The compact side-effect-free callback IR modeled the guarded assignment as a
/// ternary arm and hoisted the assignment out of the branch, so `fn()` ran on
/// every call and `called` was always set. The mutation must fall back to full
/// closure-body lowering, which emits a real `if` around the captured-state
/// writes.
#[test]
fn closure_if_guard_mutating_captured_locals_keeps_the_branch() {
    let source = source_for(
        r"
export function once<T>(fn: () => T): () => T {
  let called = false;
  let ret: T;
  return () => {
    if (!called) {
      ret = fn();
      called = true;
    }
    return ret;
  };
}
",
    );

    // The guard survives: `fn()` and the `called = true` write live inside an
    // `if` branch, not hoisted ahead of the condition.
    let called_write = source.find("borrow_mut()) = true").unwrap_or_else(|| {
        panic!("expected captured `called = true` write\n{source}");
    });
    let guard = source.find("if ").unwrap_or_else(|| {
        panic!("expected an `if` guard in the closure body\n{source}");
    });
    assert!(
        guard < called_write,
        "the captured-state write must sit inside the `if` guard, not be hoisted before it\n{source}"
    );
    // The invocation must not run unconditionally ahead of the condition: the
    // condition (`!called`) must be computed before the call site.
    let cond = source.find("= !").unwrap_or_else(|| {
        panic!("expected the `!called` condition\n{source}");
    });
    let call = source.find(")()").unwrap_or_else(|| {
        panic!("expected the `fn()` invocation\n{source}");
    });
    assert!(
        cond < call,
        "the `!called` condition must be evaluated before `fn()` is invoked\n{source}"
    );
}

#[test]
fn closure_loop_body_branch_is_not_treated_as_a_nested_loop_header() {
    // A `for` loop inside a closure body whose body contains a plain `if`/`else`
    // must emit exactly one Rust `loop`. The closure emitter used to decide
    // "this switch block is a loop header" with plain reachability, but inside an
    // open loop every body block reaches itself around the back edge, so the
    // nested `if` was wrapped in its own spurious `loop` and the real latch then
    // back-edged into an already-active block, replacing the loop's continue edge
    // with `panic!("recursive closure control flow is not structured yet")`.
    // es-toolkit's `flatten` (and every `flatMap*`/`flatten*` built on it) aborted
    // at runtime because of this.
    let source = source_for(
        r"
export function run(items: number[]): number[] {
  const out: number[] = [];
  const walk = (values: number[]) => {
    for (let i = 0; i < values.length; i++) {
      if (values[i] > 0) {
        out.push(values[i]);
      } else {
        out.push(0);
      }
    }
  };
  walk(items);
  return out;
}
",
    );

    assert!(
        !source.contains("closure control flow is not structured yet"),
        "a for-loop with a branching body must stay structured: {source}"
    );
    let program = program_body(&source);
    assert_eq!(
        program.matches("loop {").count(),
        1,
        "only the `for` header may become a Rust `loop`: {source}"
    );
}

#[test]
fn closure_loop_with_short_circuit_condition_emits_single_loop() {
    // Same defect through the other shape that made it fire in es-toolkit's
    // compat `orderBy`/`sortBy`: a short-circuit (`&&`) inside the loop body
    // lowers to a join block that reconverges before the branch. That join block
    // is reachable from itself only via the enclosing back edge, so it must not
    // be promoted to a loop header of its own.
    let source = source_for(
        r"
export function firstBig(values: number[], limit: number): number {
  const pick = (input: number[]) => {
    for (let i = 0; i < input.length; i++) {
      if (input[i] > limit && input[i] < 100) {
        return input[i];
      }
    }
    return -1;
  };
  return pick(values);
}
",
    );

    assert!(
        !source.contains("closure control flow is not structured yet"),
        "a short-circuit condition inside a closure loop must stay structured: {source}"
    );
    let program = program_body(&source);
    assert_eq!(
        program.matches("loop {").count(),
        1,
        "the `&&` join block must not become a second Rust `loop`: {source}"
    );
}

#[test]
fn a_self_recursive_named_function_expression_lifts_to_an_item() {
    // A named function expression binds its own name inside its own body, and that
    // is how JavaScript writes a self-recursive callback:
    //
    //     mergeWith(cloneDeep(target), source, function mergeRecursively(a, b) {
    //       … return mergeWith(clone(a), b, mergeRecursively); …
    //     })
    //
    // The closure path never bound the name, so the self-reference fell through
    // identifier resolution to the forward-callable fallback and lowered to an
    // EMPTY OBJECT — and calling an empty object collapses to a null callback
    // rather than failing, so the recursion silently did nothing. All eight
    // es-toolkit `toMerged` specs were that one defect.
    //
    // An inline Rust closure cannot express it either: it would have to capture the
    // binding it is being assigned to. So the function is lifted to a module item
    // and the recursion becomes ordinary `fn` recursion.
    let generated = source_for(
        r"
function apply(cb: (n: number) => number, v: number): number {
  return cb(v);
}

export function countdown(start: number): number {
  return apply(function step(n: number): number {
    if (n <= 0) {
      return 0;
    }
    return step(n - 1) + 1;
  }, start);
}
",
    );
    assert!(
        generated.contains("fn step(n: f64) -> f64"),
        "a self-recursive named function expression must lift to a function item:\n{generated}"
    );
    // The recursion has to be inside the lifted item itself, and it has to be a
    // direct item call rather than a dispatch through an erased value — which is
    // what proves the self-reference resolved to the item and not to the
    // forward-callable fallback. Read the program section only: the runtime prelude
    // mentions the erased-record constructors unconditionally.
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    let step_body = program
        .split_once("fn step(n: f64) -> f64")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split_once("\nfn "))
        .map_or_else(String::new, |(body, _)| body.to_owned());
    assert!(
        step_body.contains("step("),
        "the lifted item must call itself:\n{generated}"
    );
    assert!(
        !step_body.contains("SmeltRecord::from([])"),
        "the self-reference must not lower to an empty erased record:\n{generated}"
    );
}

#[test]
fn a_named_function_expression_name_stays_out_of_module_scope() {
    // JavaScript scopes a named function expression's name to its own body, so the
    // lift binds the name only while that body is lowered. A sibling reference to
    // the same name must still resolve to whatever the module actually declares —
    // here a top-level `step` with a different signature, which the lifted item
    // must not have displaced.
    let generated = source_for(
        r#"
function apply(cb: (n: number) => number, v: number): number {
  return cb(v);
}

export function step(label: string): string {
  return `${label}!`;
}

export function countdown(start: number): number {
  return apply(function step(n: number): number {
    if (n <= 0) {
      return 0;
    }
    return step(n - 1) + 1;
  }, start);
}

export function shout(): string {
  return step("go");
}
"#,
    );
    assert!(
        generated.contains("-> String") && generated.contains("fn shout()"),
        "the module-scope `step` must keep its own signature:\n{generated}"
    );
    assert!(
        generated.contains("fn step(n: f64) -> f64") || generated.contains("fn step_"),
        "the lifted item must still exist alongside it:\n{generated}"
    );
}

#[test]
fn a_function_reading_arguments_lowers_to_one_rest_parameter() {
    // A JavaScript `arguments` object is the ACTUAL argument list of the call, not
    // the declared parameter list, and the two differ constantly:
    //
    //     function fn(_a, _b, _c) { return Array.from(arguments); }
    //     ary(fn, 2)('a', 'b', 'c', 'd');   // fn('a', 'b') -> ['a', 'b']
    //
    // A declared-arity signature cannot carry that: the erased-call boundary pads a
    // short call up to the arity, and the padding is indistinguishable from a real
    // trailing `undefined` (which `partial`'s placeholder specs deliberately pass).
    // So such a function is lowered variadically — one rest parameter holding the
    // whole list, each declared name re-bound from it — and `arguments` is that
    // list.
    let generated = source_for(
        r"
export function probe(_a: unknown, _b: unknown, _c: unknown): unknown {
  return arguments;
}
",
    );
    assert!(
        generated.contains("smelt_arguments_object(vec![], Some("),
        "`arguments` must be built from the rest list, not the parameters:\n{generated}"
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    assert!(
        program.contains("fn probe(__smelt_arguments: SmeltList<SmeltUnknown>)"),
        "the signature must become a single rest list:\n{generated}"
    );
    assert!(
        program.contains("let _a: SmeltUnknown = __smelt_arguments"),
        "each declared name must be re-bound from the list:\n{generated}"
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

#[test]
fn an_erased_callable_reports_its_function_length() {
    // A typed callable knows its arity and `SmeltErasedFunction` carries it in a
    // `length` field, but erasing to `SmeltUnknown::Function(Rc<…>)` throws the
    // field away — an `Rc<dyn Fn>` has nowhere to put it — so a `.length` read on
    // an erased callable answered `0`. es-toolkit `rest(func)` defaults its split
    // point to `func.length - 1`, so `0` made it `-1` and every rest-parameter spec
    // reshaped its arguments wrongly. The arity is now parked in a registry keyed by
    // the callable's canonical identity.
    // The fixture erases a real callable so the `SmeltErasedFunction` boundary — the
    // only place that still knows both the arity and the erased allocation — is
    // emitted alongside the read.
    let generated = source_for(
        r"
function target(_a: unknown, _b: unknown): unknown {
  return _a;
}

export function arity(): number {
  const erased: unknown = target;
  return (erased as any).length;
}
",
    );
    assert!(
        generated.contains("fn smelt_function_length(value: &SmeltUnknown) -> f64"),
        "the function-length reader must be emitted:\n{generated}"
    );
    assert!(
        generated.contains("smelt_register_function_length(&smelt_erased_fn"),
        "the erasure boundary must record the arity:\n{generated}"
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    assert!(
        program.contains("smelt_function_length("),
        "an erased `.length` read must consult the registry:\n{generated}"
    );
}

#[test]
fn a_static_property_on_a_function_declaration_resolves() {
    // JavaScript functions are objects, so a module can hang a value off one. It is
    // how es-toolkit publishes its placeholder sentinels — `partial.placeholder`,
    // `curry.placeholder`, `bind.placeholder`, … — and `memoize.Cache = Map`.
    //
    // The assignment used to lower into the module-init body, which nothing calls,
    // with the TARGET dropped outright: the init body bound the right-hand side to a
    // local and discarded it. Every read then answered `SmeltUnknown::Null`, so
    // `partial(fn, placeholder, 'b', placeholder)` passed `null` where a sentinel
    // belonged and the placeholder slots were filled with a real argument.
    let generated = source_for(
        r"
export function partial(func: (...args: any[]) => any, ...args: any[]): (...rest: any[]) => any {
  return (...rest: any[]) => func(...args, ...rest);
}

partial.placeholder = Symbol('partial.placeholder');

export function readMember(): unknown {
  return partial.placeholder;
}

export function readDestructured(): unknown {
  const { placeholder } = partial;
  return placeholder;
}
",
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    let member = program
        .split_once("fn read_member()")
        .map_or("", |(_, rest)| rest);
    // The recorded value is a typed `symbol` (it was the erased
    // `SmeltUnknown::Symbol` literal before symbols were a type).
    assert!(
        member.contains("SmeltSymbol::from_spelling("),
        "a static-property member read must resolve to the recorded value:\n{generated}"
    );
    let destructured = program
        .split_once("fn read_destructured()")
        .map_or("", |(_, rest)| rest);
    assert!(
        destructured.contains("SmeltSymbol::from_spelling("),
        "destructuring a static property off a function must resolve too:\n{generated}"
    );
    assert!(
        !destructured.contains("let placeholder: SmeltUnknown = SmeltUnknown::Null"),
        "the destructured binding must not fall back to null:\n{generated}"
    );
}

/// A `try` around a call to a function-typed *parameter* must emit its handler.
///
/// `ExprKind::ClosureCall` took the unwind-carrying `Terminator::Call` form only
/// when the callee type's `may_throw` was set. A callback parameter of unknown
/// provenance has `may_throw == false` — yet the source wrapped the call in
/// `try` precisely *because* its throw behaviour is not statically known — so
/// the statement form was taken and the active exception handler was discarded
/// outright. The `catch` clause did not merely mis-structure: it was absent, and
/// the first throw aborted the process where JavaScript would have carried on.
#[test]
fn a_try_around_a_callback_parameter_call_emits_its_handler() {
    let source = source_for(
        r"
export function guard(cb: (x: number) => string, v: number): string {
  try {
    return cb(v);
  } catch {
    return 'caught';
  }
}
",
    );

    assert!(
        source.contains("::std::panic::catch_unwind"),
        "the callback call must carry an unwind edge:\n{source}"
    );
    assert!(
        source.contains("return JsString::from(\"caught\");"),
        "the catch clause must be emitted:\n{source}"
    );
}

/// An erased-rest callback parameter bound to a local agrees with its call ABI.
///
/// `const g = cb` binds the parameter to a local whose Rust type is the owned
/// `SmeltErasedFunction` struct, whose callback field is a `'static`
/// `Rc<dyn Fn(Vec<SmeltUnknown>) -> SmeltUnknown>`. Wrapping the borrowed
/// parameter in a bare `Rc::new(move |arg0| cb(arg0))` produced a value that was
/// *not* that struct while the call site still used the erased `.call(..)` ABI —
/// an E0658 (`fn_traits`) plus an E0308 in the generated crate. The parameter
/// must therefore enter owned, so the value and its ABI agree.
#[test]
fn an_erased_rest_callback_parameter_bound_to_a_local_enters_owned() {
    let source = source_for(
        r"
export function viaLocal(cb: (...args: unknown[]) => unknown): unknown {
  const g = cb;
  return g(3, 4);
}
export function guardedViaLocal(cb: (...args: unknown[]) => unknown): unknown {
  const g = cb;
  try {
    return g(3, 4);
  } catch {
    return 'caught';
  }
}
",
    );

    assert!(
        source.contains("fn via_local(cb: SmeltErasedFunction)"),
        "a parameter bound to an erased handle local must enter owned:\n{source}"
    );
    assert!(
        source.contains("g.call("),
        "the erased handle keeps the inherent call ABI:\n{source}"
    );
    // Arity: `SmeltErasedFunction::call` takes the ARGUMENT VECTOR, while
    // lowering hands the emitter the rest-packed `SmeltList` standing for all
    // N source arguments. Nesting that list as a single vector element calls the
    // callback with one array argument instead of two — it compiles and silently
    // passes the wrong arity, so the packed list must be handed over as the
    // vector itself. The statement form takes the list written inside the call
    // (`RestCallFusion`); the terminator form, inside `try`, passes its temporary.
    assert_eq!(
        source.matches(".call(Into::<SmeltList<_>>::into(").count(),
        1,
        "the statement form passes the packed list as the argument vector:\n{source}"
    );
    assert_eq!(
        source.matches(".call(_smelt_tmp_").count(),
        1,
        "the terminator form passes the packed list as the argument vector:\n{source}"
    );
    assert!(
        !source.contains(".call(vec![SmeltUnknown::Array("),
        "the rest-packed argument list must not become one array argument:\n{source}"
    );
    assert!(
        !source.contains("::std::rc::Rc::new(move |arg0: SmeltList<SmeltUnknown>| cb(arg0))"),
        "the borrowed-handle wrapper disagrees with the erased call ABI:\n{source}"
    );
}

/// A callback adapter must never be a constant.
///
/// A throwing arrow whose body only throws has the uninhabited return type
/// `never`, and every coercion out of `never` renders a bare constant because
/// there is no value to convert. The adapter that bridges such an arrow into a
/// non-throwing `&dyn Fn() -> unknown` parameter used that constant as its
/// whole body, so the emitted closure never mentioned — let alone called — the
/// callback it was wrapping:
///
/// ```rust
/// attempt(&mut { let _smelt_adapted_callback = ..; move || SmeltUnknown::Null })
/// ```
///
/// Nothing about that is visible in the types: it compiles and returns a
/// plausible value while the callback is silently discarded. This asserts on
/// the emitted source so the shape cannot come back even where no runtime tier
/// covers it.
#[test]
fn a_never_returning_callback_adapter_still_invokes_the_callback() {
    let source = source_for(
        r#"
function attempt(func: () => unknown): unknown[] {
  try {
    return [null, func()];
  } catch (error) {
    return ["caught", null];
  }
}

export function run(): unknown[] {
  return attempt(() => {
    throw new Error("boom");
  });
}
"#,
    );

    assert!(
        !source.contains("move || SmeltUnknown::Null"),
        "the adapter must not collapse to a constant:\n{source}"
    );
    assert!(
        source.contains("(_smelt_adapted_callback)()"),
        "the adapter body must invoke the wrapped callback:\n{source}"
    );
}

/// The same rule for a `void` source rather than a `never` one.
///
/// `isMatch(target, source)` calls `isMatchWith(target, source, () => undefined)`:
/// a zero-argument `void` arrow adapted into a `(a, b, prop, aParent, bParent,
/// stack) => boolean | undefined` customizer slot. A `void` source has no value
/// to convert either, so the coercion answered with the slot's missing-value
/// constant `None::<bool>` and dropped the call — the same defect as the
/// `never` case wearing a different constant. es-toolkit's `isEqual` (which
/// passes `noop`) has the identical shape.
///
/// The repair is keyed on the source having no value, not on the constant, so
/// both spellings are covered by one rule.
#[test]
fn a_void_callback_adapter_still_invokes_the_callback() {
    let source = source_for(
        r"
function match(
  target: unknown,
  source: unknown,
  customizer: (a: unknown, b: unknown) => boolean | undefined
): boolean {
  return customizer(target, source) ?? false;
}

export function run(target: unknown, source: unknown): boolean {
  return match(target, source, () => undefined);
}
",
    );

    assert!(
        !source.contains("move |arg0: SmeltUnknown, arg1: SmeltUnknown| None::<bool>"),
        "the adapter must not collapse to the slot's missing-value constant:
{source}"
    );
    assert!(
        source.contains("(_smelt_adapted_callback)()"),
        "a `void` source must still be called for its effects:
{source}"
    );
}

/// `Function.prototype.length` on a borrowed callable handle is its declared
/// arity, emitted as a constant.
///
/// Only the `SmeltErasedFunction` struct carries the arity in a `length` field.
/// A callback *parameter* is emitted as a borrowed `&dyn Fn` handle, which has
/// no field storage, so keying the choice on the MIR shape rather than on the
/// emitted representation produced `func.length` on a `&dyn Fn` (E0609).
#[test]
fn typescript_function_length_on_borrowed_handle_is_the_declared_arity() {
    let source = source_for(
        r"
export function arity(cb: (a: number, b: number) => void): number {
  return cb.length;
}
",
    );

    let body = source
        .split("fn arity(")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or_else(|| panic!("expected a generated `arity`:\n{source}"));
    assert!(
        body.contains("2.0"),
        "a borrowed handle's `.length` is its declared parameter count:\n{source}"
    );
    assert!(
        !body.contains("cb.length"),
        "a borrowed `&dyn Fn` has no `length` field to read:\n{source}"
    );
}

/// A callee read out of a `RefCell` is CLONED out of the guard, not moved.
///
/// H17 bound such a callee to a `let` so the read's `Ref` guard drops before the
/// call runs (a class-field arrow's body mutates the same cell). A SHARED
/// CLOSURE CAPTURE renders as `(*cell.borrow())`, whose `Rc<dyn Fn ..>` is not
/// `Copy`, so the plain binding was a move out of a `Ref` deref — E0507, which
/// stopped the whole es-toolkit probe crate from compiling. Cloning bumps a
/// refcount and still drops the guard.
#[test]
fn a_recursive_capture_callee_is_cloned_out_of_its_borrow_guard() {
    let source = source_for(
        r"
export function flattenAll(values: unknown[], depth: number): unknown[] {
  const walk = (items: unknown[], level: number): unknown[] => {
    const out: unknown[] = [];
    for (const item of items) {
      if (Array.isArray(item) && level < depth) {
        for (const nested of walk(item as unknown[], level + 1)) {
          out.push(nested);
        }
      } else {
        out.push(item);
      }
    }
    return out;
  };
  return walk(values, 0);
}
console.log(flattenAll([1, [2, [3]]], 2).length);
",
    );

    assert!(
        !source.contains("let smelt_callable = (*"),
        "a callable is never moved out of a `Ref` deref: {source}"
    );
    if source.contains("smelt_callable") {
        assert!(
            source.contains("let smelt_callable = ::std::clone::Clone::clone(&"),
            "the guarded callee is cloned before the call: {source}"
        );
    }
}
