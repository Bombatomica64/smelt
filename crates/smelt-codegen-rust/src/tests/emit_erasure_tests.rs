//! Generated Rust for the erased `SmeltUnknown` boundary.

use super::*;

#[test]
fn preserves_unknown_elements_when_casting_to_erased_type_level_helpers() {
    let source = source_for(
        r#"
import type { Simplify } from "type-fest";

type Entry<T> = Simplify<T>;

declare const value: unknown;
const entries = value as Entry<string>[];
"#,
    );

    assert!(
        source.contains(
            "smelt_values.into_iter().map(|smelt_value| smelt_value).collect::<Vec<_>>()"
        ),
        "erased helper items should preserve unknown values instead of defaulting: {source}"
    );
    assert!(
        !source.contains("values.into_iter().map(|value| Default::default()).collect::<Vec<_>>()"),
        "erased helper items should not be replaced with defaults: {source}"
    );
}

#[test]
fn emits_js_numeric_conversion_for_erased_unary_plus() {
    let source = source_for(
        r"
declare const value: unknown;
const number = +value;
",
    );

    assert!(
        source.contains("SmeltUnknown::Number(value) => value"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Object(value) => match value.get(\"__smelt_date\")"),
        "{source}"
    );
    assert!(source.contains("SmeltUnknown::Null => 0.0"), "{source}");
    // A string goes through the shared `StringToNumber` helper (empty text is
    // 0 there), not an inline trim-and-parse.
    assert!(
        source.contains("SmeltUnknown::String(value) => smelt_js_string_to_number(&value), SmeltUnknown::Bool(value) => if value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_)"),
        "{source}"
    );
    assert!(
        source.contains(
            "SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN"
        ),
        "{source}"
    );
}

#[test]
fn emits_typescript_unknown_as_tagged_type() {
    let source = source_for(
        "function identity(value: unknown): unknown {
  return value;
}

function passthrough(values: readonly unknown[]): readonly unknown[] {
  return values;
}
",
    );

    assert!(source.contains("pub enum SmeltUnknown"));
    assert!(source.contains("String(JsString),"));
    assert!(source.contains("fn identity(value: SmeltUnknown) -> SmeltUnknown"));
    assert!(
        source
            .contains("fn passthrough(values: SmeltList<SmeltUnknown>) -> SmeltList<SmeltUnknown>")
    );
}

#[test]
fn emits_typescript_unknown_wrap_checks_and_casts() {
    let source = source_for(
        r#"
function boxString(): unknown {
  return "ready";
}

function readString(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  return value as string;
}

function isArray(value: unknown): boolean {
  return Array.isArray(value);
}
"#,
    );

    assert!(source.contains("SmeltUnknown::String"));
    assert!(source.contains("matches!(value.clone(), SmeltUnknown::String(_))"));
    assert!(source.contains("SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value)"));
    assert!(source.contains("matches!(value.clone(), SmeltUnknown::Array(_))"));
}

#[test]
fn emits_unknown_equality_against_concrete_values() {
    let source = source_for(
        r#"
function isTrailing(value: unknown): boolean {
  return value === "trailing";
}

function isNotOne(value: unknown): boolean {
  return value !== 1;
}
"#,
    );

    // `===`/`!==` on erased values use JS strict equality (`js_strict_eq`):
    // reference identity for objects, value for primitives, NaN-unequal — NOT
    // SmeltUnknown's structural `==` (which `==`/`!=` and `isDeepEqual` use).
    assert!(
        source.contains("value.clone().js_strict_eq(&SmeltUnknown::String(\"trailing\".into()))"),
        "{source}"
    );
    assert!(
        source.contains("!(value.clone().js_strict_eq(&SmeltUnknown::Number(1.0 as f64)))"),
        "{source}"
    );
}

#[test]
fn emits_erased_value_wrapped_for_optional_erased_destination() {
    let source = source_for(
        r"
function maybe(value: unknown): unknown | undefined {
  return value;
}
",
    );

    assert!(source.contains("Some(value.clone())"), "{source}");
}

#[test]
fn emits_option_into_unknown_runtime_conversion() {
    let source = source_for(
        r#"
function maybe(flag: boolean): unknown {
  const value: string | undefined = flag ? "ready" : undefined;
  return value;
}
"#,
    );

    assert!(
        source.contains("impl<T: IntoSmeltUnknown> IntoSmeltUnknown for Option<T>"),
        "{source}"
    );
    assert!(
        source.contains("map_or(SmeltUnknown::Undefined, IntoSmeltUnknown::into_smelt_unknown)"),
        "{source}"
    );
}

#[test]
fn emits_cycle_safe_unknown_structural_equality_runtime() {
    let source = source_for(
        r"
declare const left: unknown;
declare const right: unknown;
const same = left === right;
",
    );

    assert!(
        source.contains("fn smelt_unknown_structural_eq"),
        "{source}"
    );
    assert!(source.contains("fn smelt_object_structural_eq"), "{source}");
    assert!(
        source.contains("if !seen.insert(key) { return true; }"),
        "{source}"
    );
    assert!(
        source.contains("left.is_nan() && right.is_nan()"),
        "{source}"
    );
}

#[test]
fn emits_cycle_safe_unknown_structural_hash_runtime() {
    let source = source_for(
        r"
declare const value: unknown;
const values = new Set<unknown>([value]);
",
    );

    assert!(
        source.contains("fn smelt_unknown_structural_hash"),
        "{source}"
    );
    assert!(
        source.contains("fn smelt_object_structural_hash"),
        "{source}"
    );
    assert!(source.contains("if !seen.insert(object.id)"), "{source}");
}

#[test]
fn emits_unknown_partial_ordering_runtime_support() {
    let source = source_for(
        r"
function before(left: unknown, right: unknown): boolean {
  return left < right;
}
",
    );

    assert!(
        source.contains("impl PartialOrd for SmeltUnknown"),
        "{source}"
    );
    assert!(source.contains("smelt_unknown_rank"), "{source}");
    assert!(source.contains("smelt_unknown_date_value"), "{source}");
}

#[test]
fn emits_numeric_binary_operands_coerced_to_destination() {
    let source = source_for(
        r"
function addUnknown(total: number, value: unknown): number {
  const narrowed = value as number;
  return total + narrowed;
}

function elapsedSince(right: number): number {
  return Date.now() - right;
}
",
    );

    // The `i64` destination comes from `Date.now()`, which is an integer
    // timestamp. This used to be `bigint - number` typed `Int`, which `tsc`
    // rejects (a bigint never mixes with a number) and which is no longer an
    // `Int` at all: a `bigint` is its own unbounded type. The coercion under
    // test -- a `number` operand truncated into an integer destination -- is
    // unchanged, so a valid program pins it now.
    assert!(source.contains("(right as f64).trunc() as i64"), "{source}");
    // The int-from-float coercion keeps only the parentheses `.trunc()` needs
    // as a method receiver; the whole cast is not wrapped again, which drew a
    // spurious `unused_parens` in every value position it landed in.
    assert!(
        !source.contains("((right.clone() as f64).trunc() as i64)"),
        "int-from-float coercion should not re-wrap the whole cast: {source}"
    );
}

#[test]
fn emits_int_to_float_coercion_without_defensive_parentheses() {
    let source = source_for(
        r"
function widen(values: unknown[]): number {
  return values.length;
}
",
    );

    // `values.length` lowers to `values.len() as f64`; the coercion seam must
    // not wrap the cast in defensive parentheses, which the compiler flags as
    // `unused_parens` wherever the value stands alone. Checked on the emitted
    // FUNCTION BODY: the runtime prelude legitimately parenthesizes the same
    // cast where it is an argument (`SmeltUnknown::Number(values.len() as f64)`).
    let body = emitted_function_body(&source, "fn widen");
    assert!(body.contains("as f64"), "{body}");
    assert!(
        !body.contains("(values.len() as f64)"),
        "int-to-float coercion should not wrap the cast in parentheses: {body}"
    );
}

#[test]
fn emits_optional_erased_value_coerced_to_concrete_destination() {
    let source = source_for(
        r"
function read(output: Record<string, unknown[]>, key: unknown | undefined): unknown[] {
  return output[key as string];
}
",
    );

    assert!(
        source.contains(
            ".map_or(JsString::new(), |smelt_value| match smelt_value.clone() { SmeltUnknown::String(smelt_value) => smelt_value, smelt_other => JsString::from(match smelt_other { SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value)"
        ),
        "{source}"
    );
}

#[test]
fn emits_erased_nullish_coalescing_as_unknown_match() {
    let source = source_for(
        r"
function fallback<T>(value: T, fallbackValue: T): T | undefined {
  return value ?? fallbackValue;
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => fallback_value.clone()"),
        "{source}"
    );
}

#[test]
fn emits_erased_nullish_coalescing_into_concrete_destination() {
    let source = source_for(
        r"
function fallback(value: unknown): boolean {
  const result: boolean = value ?? false;
  return result;
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value"),
        "{source}"
    );
}

#[test]
fn emits_boolean_cast_for_typescript_unknown() {
    let source = source_for(
        r"
function truthy(value: unknown): boolean {
  return Boolean(value);
}
",
    );

    assert!(source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => false"));
    assert!(source.contains(
        "SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true"
    ));
}

#[test]
fn erases_sets_without_dropping_items() {
    let source = source_for(
        r"
function accept(value: unknown): unknown {
  return value;
}

export function run(): unknown {
  return accept(new Set([1, 2, 3]));
}
",
    );

    assert!(
        source.contains("IntoSmeltUnknown for ::std::collections::HashSet<T>"),
        "{source}"
    );
    assert!(
        source.contains("values.sort_by_key(smelt_unknown_stable_hash_key);"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Array(values.into())"),
        "{source}"
    );
    // Every member is erased, not dropped: the adapter maps each item through
    // its own erasure. (This assertion used to match `.clone().into_iter()
    // .map(|value|`, which was the erased-Set `entries` method's text in the
    // runtime prelude, not this adapter; that method is now a live iterator.)
    assert!(
        source.contains("self.into_iter().map(IntoSmeltUnknown::into_smelt_unknown)"),
        "{source}"
    );
}

#[test]
fn self_aliasing_unknown_field_assignment_evaluates_value_before_borrow() {
    // Regression: JS self-aliasing `value.self = value` must evaluate the
    // assigned value into a temporary BEFORE taking the receiver's mutable
    // borrow, or the value's read of the receiver conflicts with the `&mut`
    // borrow (E0502).
    let source = source_for(
        r"
function link(value: unknown): unknown {
  value.self = value;
  return value;
}
",
    );

    assert!(source.contains("let smelt_value ="), "{source}");
    assert!(source.contains("match &mut value"), "{source}");
    assert!(
        source.contains("map.insert(\"self\".to_owned(), smelt_value);"),
        "{source}"
    );
}

#[test]
fn coerces_optional_unknown_field_to_optional_callable_destination() {
    let source = source_for(
        r"
type Context = ((value: unknown) => unknown) | undefined;
interface Options {
  in?: Context;
}

function apply(value: unknown, context?: Context): unknown {
  return value;
}

function run(options?: Options): unknown {
  return apply(null, options?.in);
}
",
    );

    assert!(
        source.contains(".as_ref().and_then(|_smelt_value|"),
        "{source}"
    );
    assert!(
        !source.contains("= options.clone().as_ref().map(|_smelt_value| SmeltUnknown::Null);"),
        "{source}"
    );
    assert!(source.contains("Option<::std::rc::Rc"), "{source}");
}

#[test]
fn tests_uninitialized_erased_value_presence_before_numeric_extraction() {
    let source = source_for(
        r"
function latest(flag: boolean): number | undefined {
  let found;
  if (flag) {
    found = 3;
  }
  return found !== undefined ? found : undefined;
}
",
    );

    assert!(source.contains("matches!(found.clone(), SmeltUnknown::Undefined)"));
    assert!(!source.contains("!(false)"));
}

#[test]
fn emits_optional_callable_logical_or_as_selected_unknown_value() {
    let source = source_for(
        r"
function select(value: unknown, context?: (value: unknown) => unknown): unknown {
  return context || value;
}
",
    );

    assert!(source.contains(".map_or_else("), "{source}");
    assert!(source.contains("SmeltUnknown::Function"), "{source}");
    // Scoped to the source after `fn select`: the runtime prelude's own-property
    // authority spells its own `.is_some() ||` for the byte-view element probe,
    // which is unrelated to this program's selection.
    let function_body = source
        .split("fn select")
        .nth(1)
        .expect("generated source defines select");
    assert!(!function_body.contains(".is_some() ||"), "{source}");
}

#[test]
fn ignores_unused_void_call_result_without_null_cast() {
    let source = source_for(
        r"
function wrapped(value: unknown): void;
function wrapped(value: unknown): unknown {
  return value;
}

function run(value: unknown): void {
  wrapped(value);
}
",
    );

    assert!(source.contains("let _ = wrapped"), "{source}");
    assert!(!source.contains("unknown is not null"), "{source}");
}

#[test]
fn preserves_unknown_size_as_dynamic_property_access() {
    let source = source_for(
        r#"
function sizeOf(value: unknown): unknown {
  if (typeof value === "object" && value !== null && "size" in value) {
    return value.size;
  }
  return undefined;
}
"#,
    );

    // The erased property read goes through the one prelude helper that knows
    // every receiver shape (object record, array named property,
    // `Object.prototype` fallback); the object arm inside it is still
    // `smelt_get_object_field`.
    let body = emitted_function_body(&source, "fn size_of");
    assert!(body.contains("smelt_get_unknown_field("), "{body}");
    assert!(body.contains("\"size\""), "{body}");
}

#[test]
fn optional_unknown_truthiness_covers_undefined() {
    // An optional `unknown` used in a boolean position coerces through
    // `optional_truthy_expr`. The generated `match` must treat both
    // `Some(Null)` and `Some(Undefined)` as falsy; omitting the `Undefined`
    // arm produced a non-exhaustive match (E0004) because `SmeltUnknown` has a
    // dedicated `Undefined` variant.
    let source = source_for(
        r"
export function firstTruthy(guard?: unknown): boolean {
  if (guard) {
    return true;
  }
  return false;
}
",
    );

    assert!(
        source.contains("Some(SmeltUnknown::Null) | Some(SmeltUnknown::Undefined) => false"),
        "optional-unknown truthiness must cover the Undefined variant\n{source}"
    );
}

#[test]
fn erased_call_reassignment_reuses_binding_without_double_move() {
    // Regression: `predicate = iteratee(predicate)` reassigns an erased local
    // from a call that returns a first-class function value. The erase seam used
    // to re-render the call at the reassignment site while ALSO emitting the
    // call-result binding, evaluating the call twice and moving `predicate` into
    // it twice (E0382). The erase must instead read the existing binding.
    let source = source_for(
        r"
function iteratee(value: unknown): (item: unknown) => unknown {
  return (item: unknown) => item;
}
export function normalize(predicate: unknown): unknown {
  predicate = iteratee(predicate);
  return predicate;
}
",
    );

    assert!(
        source.contains("fn normalize"),
        "expected the normalize function to be emitted\n{source}"
    );
    let normalize = &source[source.find("fn normalize").unwrap()..];
    assert!(
        normalize.matches("iteratee(").count() == 1,
        "the reassigned call must be rendered exactly once, not re-inlined\n{normalize}"
    );
}

#[test]
fn overloaded_callable_interface_stores_an_erased_variadic_call_slot() {
    // Regression (es-toolkit compat `curry`): a callable interface's generated
    // struct carries ONE `__smelt_call` slot. When the interface declares several
    // call signatures of DIFFERENT arities, no single concrete signature can hold
    // the value -- which overload runs is decided by the runtime argument list --
    // yet the slot used to be typed from the FIRST signature. Every call site then
    // adapted to that signature and silently discarded the arguments actually
    // passed: es-toolkit's two-argument `curried(2, 3)` emitted `(smelt_callback)()`,
    // running the zero-argument overload and answering a defaulted value with no
    // diagnostic.
    //
    // The overload set now collapses to one erased variadic callable, exactly as a
    // union of differing-arity function types already does. A uniformly-shaped
    // callable interface keeps its precise concrete slot, so nothing is erased that
    // a concrete Rust `Fn` type could have carried.
    let source = source_for(
        r"
interface Overloaded {
  (): Overloaded;
  (t1: number): number;
  (t1: number, t2: number): number;
}
interface Uniform {
  (t1: number): number;
}
export function use_them(a: Overloaded, b: Uniform): number {
  return a(1, 2) + b(3);
}
",
    );
    let overloaded_struct = source
        .split("struct Overloaded")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .unwrap_or("");
    assert!(
        overloaded_struct.contains("__smelt_call: SmeltErasedFunction"),
        "an overload set whose arities differ must store the erased variadic \
         callable, not the first signature: {source}"
    );
    let uniform_struct = source
        .split("struct Uniform")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .unwrap_or("");
    assert!(
        uniform_struct.contains("__smelt_call: ::std::rc::Rc<dyn Fn(f64) -> f64>"),
        "a uniformly-shaped callable interface must keep its concrete slot type: {source}"
    );
}

#[test]
fn wraps_erased_rest_call_result_in_option_for_optional_return() {
    // The fully-erased `SmeltErasedFunction::call` ABI always yields a bare
    // `SmeltUnknown`, even when the callee's declared return type is
    // `ReturnType<F> | undefined` (which lowers to `Option<SmeltUnknown>`).
    // The call result must therefore be coerced at the assignment seam — a
    // raw `SmeltUnknown` stored into an `Option<SmeltUnknown>` place is a
    // type error (E0308). This mirrors es-toolkit's `after`/`before`.
    let source = source_for(
        r"
type AnyFn = (...args: unknown[]) => unknown;

function after(n: number, func: AnyFn): (...args: unknown[]) => unknown | undefined {
  let count = 0;
  return (...args: unknown[]) => {
    count += 1;
    if (count >= n) {
      return func(...args);
    }
    return undefined;
  };
}

export function run(): unknown | undefined {
  const gated = after(0, () => 1);
  const result = gated();
  return result;
}
",
    );

    assert!(
        source.contains("Option<SmeltUnknown>"),
        "the optional erased return should lower to `Option<SmeltUnknown>`\n{source}"
    );
    assert!(
        source.contains(".call(") && source.contains("Some("),
        "an erased-rest call feeding an optional return must wrap its \
         `SmeltUnknown` result in `Some(..)`\n{source}"
    );
}

/// A `never`-returning predicate (`(value: never) => value`) evaluates to a
/// real erased `SmeltUnknown` at runtime. Coercing that result into a concrete
/// `bool` parameter must route through JS-truthiness extraction rather than
/// handing the raw `SmeltUnknown` to a `bool` slot (E0308).
#[test]
fn never_return_coerces_to_bool_via_truthiness() {
    let source = source_for(
        r"
function pickBy(obj: Record<string, unknown>, pred: (v: unknown) => boolean): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const key in obj) {
    if (pred(obj[key])) {
      out[key] = obj[key];
    }
  }
  return out;
}

export function run(): Record<string, unknown> {
  const obj = {};
  const shouldPick = (value: never) => value;
  return pickBy(obj, shouldPick);
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Bool(value) => value") && source.contains("=> false"),
        "a never-returning predicate result must be coerced to bool via \
         truthiness\n{source}"
    );
}

#[test]
fn erased_rest_adapter_packs_target_arguments() {
    // Adapting an erased-rest (`SmeltErasedFunction`) source to a fixed-arity
    // target must pack every positional target argument into the erased rest
    // list, forwarding all of them. A per-source-parameter mapping instead
    // coerces `arg0` *into* the list — spreading its elements and dropping
    // `arg1` — which panics on non-iterable values and corrupts multi-argument
    // adapters (the `partial`/`partialRight` two-argument case; the text-path
    // variant of this adapter is exercised end-to-end by the es-toolkit
    // `partial`/`partialRight` suites).
    let source = source_for(
        r"
type ErasedBinary = (a: unknown, b: unknown) => unknown;
function makeErased<R>(f: (...args: unknown[]) => R): (...args: unknown[]) => R {
  return (...args: unknown[]) => f(...args);
}
export function adapt(f: (...args: unknown[]) => unknown): ErasedBinary {
  return makeErased(f);
}
",
    );

    // Both target arguments are forwarded into the erased rest list, in order.
    assert!(
        source.contains("smelt_forwarded_args.push(smelt_arg0.clone());")
            && source.contains("smelt_forwarded_args.push(smelt_arg1.clone());"),
        "erased-rest adapter must forward every target argument, not drop the tail\n{source}"
    );
    // The scalar target arguments must not be spread as if each were the whole
    // argument list (the corrupting behavior the fix removes).
    assert!(
        !source.contains("panic!(\"unknown is not iterable\")"),
        "erased-rest adapter must not spread a scalar target argument\n{source}"
    );
}

/// A base-constructor slot answering `this` at the BASE type is upcast.
///
/// Hono's `onError = (h): Hono<..> => { ..; return this }` stored by the base
/// constructor: in the initializer copy the receiver is `Self` (the derived
/// struct), a different Rust type from the declared base return, so the value
/// is rebuilt at the base type through the member-wise structural adapter
/// (was E0271, "expected `Hono_1`, found `Hono`").
#[test]
fn a_base_initializer_slot_returning_this_upcasts_the_derived_receiver() {
    let source = source_for(
        r"
class Router<E = string> {
  handler: string = 'default';
  onError = (handler: string): Router<E> => {
    this.handler = handler;
    return this;
  };
}
class App<E = string> extends Router<E> {
  constructor() {
    super();
  }
}
console.log(new App<number>().onError('x').handler);
",
    );

    let init = source
        .split("fn __smelt_init_Router(smelt_receiver: Self) -> Self {")
        .nth(1)
        .and_then(|rest| rest.split("\n    fn ").next())
        .unwrap_or_default();
    assert!(
        init.contains("Router(::std::rc::Rc::new(::std::cell::RefCell::new(RouterInner {"),
        "the returned receiver is rebuilt at the base type: {source}"
    );
}

#[test]
fn erased_slice_routes_a_byte_buffer_receiver_through_the_host_slice_helper() {
    // `buffer.slice(0)` on an erased receiver must be able to answer a FRESH record
    // of the same host identity: the tag-preserving erased slice used to forward any
    // non-array receiver unchanged, so the `clone(buf) { return buf.slice(0) }`
    // shape returned its own argument and `expect(cloned).not.toBe(buffer)` failed.
    // `subarray` is the same operation and routes the same way.
    for source in [
        "export function f(value: any): any { return value.slice(0); }",
        "export function f(value: any): any { return value.subarray(); }",
    ] {
        let generated = source_for(source);
        assert!(
            generated.contains("smelt_host_buffer_slice(&smelt_slice_value"),
            "expected `{source}` to try the host byte-buffer slice first:\n{generated}"
        );
    }
}

#[test]
fn erased_reads_and_writes_reach_a_byte_buffers_bytes() {
    // A byte buffer's indexed slots are its bytes, not ordinary record properties.
    // Index reads used to miss the `bytes` storage and answer `null`, which made the
    // `a[i]` element walk deep equality performs over a typed-array-tagged value
    // compare two different buffers as equal; index writes landed in a property
    // instead of the storage, so the typed-array clone shape
    // `result[i] = clone(source[i])` produced a record with stray numeric keys.
    // `o[k]` and `o.k` are one JavaScript operation, so the erased index read
    // resolves its OBJECT arm through the same `smelt_get_object_field` helper
    // the erased field read uses rather than inlining its own lookup; the byte
    // -buffer attempt lives inside that helper, asserted just below.
    let indexed = source_for("export function f(value: any): any { return value[1]; }");
    assert!(
        indexed
            .contains("SmeltUnknown::Object(smelt_values) => smelt_get_object_field(&smelt_values"),
        "an erased index read must resolve an object through the field helper:\n{indexed}"
    );
    let field = source_for("export function f(value: any): any { return value.length; }");
    assert!(
        field.contains("smelt_host_buffer_element(map, field)"),
        "the erased field read helper must try the byte-buffer element first:\n{field}"
    );
    let assign = source_for("export function f(value: any): void { value[1] = 2; }");
    assert!(
        assign
            .contains("smelt_host_buffer_set_element(map, &key.to_string_lossy(), value.clone())"),
        "an erased index write must offer the byte storage the write first:\n{assign}"
    );
}

#[test]
fn an_optional_erased_index_read_resolves_synthesized_properties() {
    // `o?.[k]` reads the object arm through the same helper `o.k` uses, so a
    // marker record's synthesized properties (an error's `name`, a Map's `size`,
    // the global object's constructors) are visible to both spellings. An
    // own-field-only `values.get(..)` was blind to every one of them.
    let source =
        source_for("export function f(value: any, key: string): any { return value?.[key] ?? 1; }");
    assert!(
        source.contains("smelt_get_object_field(&smelt_values"),
        "an optional erased index read must use the field helper:\n{source}"
    );
}

#[test]
fn an_absent_erased_slot_defaults_to_undefined_not_null() {
    // `Default::default()` is what every ABSENT erased slot falls back to: an
    // out-of-range element read, a `resize` fill, a `new Array(n)` hole.
    // JavaScript answers `undefined` for all of them; `null` is a value a
    // program has to store deliberately, so defaulting to it made a hole and a
    // stored `null` indistinguishable.
    let source = source_for("export function f(value: any): any { return value; }");
    assert!(
        source.contains("impl Default for SmeltUnknown"),
        "the erased default impl must be emitted:\n{source}"
    );
    let default_impl = source
        .split("impl Default for SmeltUnknown")
        .nth(1)
        .unwrap_or_default();
    assert!(
        default_impl.contains("Self::Undefined"),
        "an absent erased slot must default to `undefined`:\n{default_impl}"
    );
}

/// Both indirect-call forms answer the erased-rest ABI question the same way.
///
/// `Rvalue::ClosureCall` (statement form) had an explicit precedence ladder —
/// function-parameter place, function-parameter name, borrowed callback capture,
/// *then* erased-rest — while `rendered_call`'s `Callee::Indirect` (terminator form)
/// had none and emitted `.call(..)` for any erased-rest callee type. Routing a
/// call from one form to the other therefore flipped its ABI: a borrowed
/// `&dyn Fn` parameter is not the `SmeltErasedFunction` struct, so `.call(..)` on
/// it resolves to the unstable `Fn::call` trait method (E0658). One helper,
/// `callee_uses_erased_call_method`, now answers for both.
#[test]
fn a_borrowed_erased_rest_parameter_is_called_directly_in_both_forms() {
    let source = source_for(
        r"
export function guardedErased(cb: (...args: unknown[]) => unknown): unknown {
  try {
    return cb(1);
  } catch {
    return 'caught';
  }
}
export function plainErased(cb: (...args: unknown[]) => unknown): unknown {
  return cb(2);
}
",
    );

    assert!(
        source.contains("fn guarded_erased(cb: &dyn Fn(SmeltList<SmeltUnknown>) -> SmeltUnknown)"),
        "a borrowed erased-rest parameter stays a bare handle:\n{source}"
    );
    assert!(
        source.contains("(cb)(_smelt_tmp_1)"),
        "the terminator form must invoke the borrowed handle directly:\n{source}"
    );
    assert!(
        !source.contains("cb.call(") && !source.contains("(cb).call("),
        "a borrowed `&dyn Fn` must never take the inherent erased call ABI:\n{source}"
    );
}

#[test]
fn an_element_read_in_an_erased_slot_stays_fallible() {
    // Sibling of `an_element_read_in_an_optional_slot_stays_fallible`, and the
    // two must agree: an out-of-range element read is `undefined` in
    // JavaScript, so the `Option<..>` target answers `None` and the ERASED
    // target must answer `SmeltUnknown::Undefined`.
    //
    // The erased target used to make the read TOTAL first and erase afterwards,
    // so the miss became the element type's own missing value and erased as
    // THAT: `row[i] = b[i]` for `b: string[]` stored `''`, and for
    // `number[]` it stored `0`. Both are values JavaScript never produces here.
    let source = source_for(
        r"
export function fill(b: string[], n: number[], nested: string[][], i: number): unknown[] {
  const row: unknown[] = [0, 0, 0];
  row[0] = b[i];
  row[1] = n[i];
  row[2] = nested[i];
  return row;
}
",
    );

    assert!(
        !source.contains("SmeltUnknown::String(b.get("),
        "the read must not be made total and erased as the element default:\n{source}"
    );
    assert!(
        !source.contains("b.get({ let len = b.len()")
            || !(source.contains("unwrap_or(String::new()).clone())")
                || source.contains("unwrap_or(JsString::new()).clone())")),
        "a missing `string` element must not erase as the empty string:\n{source}"
    );
    assert!(
        !source.contains("SmeltUnknown::Number(n.get("),
        "a missing `number` element must not erase as zero:\n{source}"
    );
    assert!(
        !source.contains("{ let smelt_l = nested.get("),
        "a missing nested-list element must not erase as an empty array:\n{source}"
    );
    assert_eq!(
        source.matches(".cloned().map(|smelt_value| ").count(),
        3,
        "each of the three reads keeps its own fallibility:\n{source}"
    );
    for tail in [
        ".cloned().map(|smelt_value| SmeltUnknown::String(smelt_value.into())).unwrap_or(SmeltUnknown::Undefined)",
        ".cloned().map(|smelt_value| SmeltUnknown::Number(smelt_value as f64)).unwrap_or(SmeltUnknown::Undefined)",
        "collect::<Vec<_>>())) }).unwrap_or(SmeltUnknown::Undefined)",
    ] {
        assert!(
            source.contains(tail),
            "the miss must erase as `undefined`, not as the element default \
             (`{tail}`):\n{source}"
        );
    }
}

#[test]
fn an_element_read_on_an_erased_receiver_misses_to_undefined() {
    // The same rule with the receiver erased instead of the destination: when
    // the base is a `SmeltUnknown` the read goes through `unknown_index_expr`,
    // which answered `SmeltUnknown::Null` for an out-of-range array or string
    // index. That is the `zipWith` defect — ragged inputs produced `"3null"`
    // where JavaScript produces `"3undefined"`. A missing OBJECT PROPERTY is a
    // separate question and deliberately still answers `Null`.
    let source = source_for(
        r"
export function pick(value: unknown, index: number): unknown {
  return (value as any)[index];
}
",
    );

    assert!(
        source.contains(
            "and_then(|smelt_position| smelt_values.get(smelt_position).cloned()).unwrap_or(SmeltUnknown::Undefined)"
        ),
        "a missing array element on an erased receiver is `undefined`:\n{source}"
    );
    assert!(
        source.contains(
            "smelt_value.char_at(smelt_position).map(|smelt_ch| SmeltUnknown::String(smelt_ch.into()))).unwrap_or(SmeltUnknown::Undefined)"
        ),
        "a missing string character on an erased receiver is `undefined`:\n{source}"
    );
}

/// A call to an overloaded callable interface must not stop at the first
/// overload that merely shares the call's arity.
///
/// es-toolkit's `CurriedFunction2` declares `(t1: __, t2: T2)` — a placeholder
/// position typed by a `unique symbol` — before `(t1: T1, t2: T2): R`. Taking
/// the first of them made `curried(2, 3)` claim to return a `Curried1`, so the
/// comparison against a number const-folded to `false`.
///
/// Old requirement: both positions read as `unknown` (symbols were erased), so
/// no static rule separated the two overloads and the call answered the erased
/// result every surviving overload agreed on. New requirement: `__` is a real
/// `symbol`, a number argument cannot be one, so the call selects
/// `(t1: T1, t2: T2): R` and its result is `R` itself (`number`) -- still
/// never `Curried1`, and still passing both arguments to the erased callable.
#[test]
fn ambiguous_same_arity_call_signatures_answer_the_erased_call_result() {
    let source = source_for(&format!(
        "{CALLABLE_OVERLOAD_PRELUDE}
const curried = curry2((a: number, b: number) => a + b);
const total = curried(2, 3);
"
    ));

    assert!(
        source.contains(
            "smelt_callback.call(vec![(smelt_arg0.clone()).into_smelt_unknown(), (smelt_arg1.clone()).into_smelt_unknown()])"
        ),
        "both arguments must reach the erased callable:\n{source}"
    );
    assert!(
        !source.contains("total: Curried1"),
        "the call must not claim the placeholder overload's return type:\n{source}"
    );
    assert!(
        source.contains("let total: f64"),
        "a number argument selects the value overload, whose result is the number:\n{source}"
    );
}
