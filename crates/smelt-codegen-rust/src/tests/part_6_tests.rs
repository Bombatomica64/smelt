//! Split codegen tests chunk.

use super::*;

#[test]
fn emits_array_pop_method() {
    let source = source_for(
        r"
let values: number[] = [1, 2];
values.pop();
const item = values.pop();
",
    );

    assert!(source.contains("let mut"));
    assert!(source.contains("Option<f64>"));
    assert!(source.contains(".pop();"));
}

#[test]
fn emits_array_shift_method() {
    let source = source_for(
        r#"
let values: string[] = ["a", "b"];
values.shift();
const item = values.shift();
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains("Option<String>"));
    assert!(source.contains(".is_empty()"));
    assert!(source.contains("Some("));
    assert!(source.contains(".remove(0)"));
}

/// A `.slice()` on an erased receiver (a generic / `unknown` value that may be
/// an array, typed array, or array buffer at runtime) must runtime-dispatch on
/// the value tag rather than ToString-coercing the value to `"[object Object]"`
/// and char-slicing that string. An array receiver yields a fresh
/// `SmeltUnknown::Array`; a string yields a `SmeltUnknown::String`; anything
/// else (marker buffer objects) is forwarded unchanged.
#[test]
fn emits_erased_slice_as_runtime_tag_dispatch() {
    let source = source_for(
        r"
export function cloneSlice(obj: unknown): unknown {
  return (obj as any).slice(0);
}
",
    );

    assert!(source.contains("let smelt_slice_value ="));
    assert!(
        source.contains("SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id()"),
        "array slice must materialize a fresh SmeltUnknown::Array"
    );
    assert!(
        source.contains("smelt_other => smelt_other"),
        "non-array/string receivers must be forwarded unchanged"
    );
    // The old bug lowered the erased `.slice` to a `StringSlice`, ToString-
    // coercing the receiver and char-slicing it (`.chars().skip(...)` with no
    // `smelt_slice_value` tag match). The tag-dispatch above proves that path
    // is gone.
    assert!(
        source.contains("SmeltUnknown::String(value) => { let chars ="),
        "erased slice must string-slice via the value-tag match, not a ToString coercion"
    );
}

/// A `for (const [k, v] of map)` over an erased `__smelt_map` marker object must
/// iterate the stored `[k, v]` pairs rather than panicking "unknown is not
/// iterable". The erased-iteration projection gains a `__smelt_map` arm next to
/// the existing `__smelt_set` / `__smelt_symbol_iterator` handling.
#[test]
fn emits_erased_map_marker_iteration() {
    let source = source_for(
        r"
export function toEntries(x: unknown): unknown[] {
  const out: unknown[] = [];
  for (const e of (x as any)) { out.push(e); }
  return out;
}
",
    );

    assert!(
        source.contains("value.get(\"__smelt_map\")"),
        "erased iteration must accept __smelt_map marker objects"
    );
    assert!(source.contains("value.get(\"__smelt_set\")"));
}

/// A `void`-returning callback erased into a callable value returns JavaScript
/// `undefined`, not `null`. Downstream `!== undefined` guards (e.g.
/// cloneDeepWith's customizer wrapper) rely on this to treat the result as "no
/// value" rather than a real `null`.
#[test]
fn emits_void_callback_erasure_as_undefined() {
    let source = source_for(
        r"
type Customizer = (value: unknown, key: unknown) => unknown;
function runWith(fn: Customizer): unknown { return fn(1, 2); }
export function callVoid(): unknown {
  const noop: () => void = () => {};
  return runWith(noop as unknown as Customizer);
}
",
    );

    assert!(
        source.contains("; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Undefined) }"),
        "void callback erasure must yield SmeltUnknown::Undefined, not Null"
    );
}

#[test]
fn emits_array_is_array_as_static_boolean() {
    let source = source_for(
        r"
const values: number[] = [1, 2, 3];
const yes = Array.isArray(values);
const no = Array.isArray(1);
",
    );

    assert!(source.contains(" = true;"));
    assert!(source.contains(" = false;"));
}

#[test]
fn emits_first_class_array_is_array_runtime_probe() {
    let source = source_for(
        r#"
const isArray = Array.isArray;
const arrayResult = isArray([1]);
const stringResult = isArray("value");
"#,
    );

    assert!(
        source.contains("matches!(closure_arg_0.clone(), SmeltUnknown::Array(_))"),
        "expected the first-class builtin to retain its runtime array probe: {source}"
    );
    assert!(
        source.contains("SmeltUnknown::Array("),
        "expected concrete arrays to adapt into the runtime-inspection boundary: {source}"
    );
    assert!(
        source.contains("SmeltUnknown::String("),
        "expected concrete primitives to adapt into the runtime-inspection boundary: {source}"
    );
}

#[test]
fn emits_object_projection_methods() {
    let source = source_for(
        r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const keys = Object.keys(mapping);
const values = Object.values(mapping);
const entries = Object.entries(mapping);
const rebuilt = Object.fromEntries([["a", 1], ["b", 2]]);
"#,
    );

    assert!(
        source.contains(
            ".keys().filter(|key| !key.starts_with(\"__smelt_symbol:\") && smelt_is_for_in_record_key(&mapping.clone(), key)).collect::<Vec<_>>()"
        ),
        "{source}"
    );
    assert!(
        source.contains(
            ".iter().filter(|(key, _)| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").map(|(_, value)| value).collect::<Vec<_>>()"
        ),
        "{source}"
    );
    assert!(
        source.contains(
            ".iter().filter(|(key, _)| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").collect::<Vec<_>>()"
        ),
        "{source}"
    );
    assert!(
        source.contains("SmeltRecord::from([(\"a\".to_owned(), 1.0), (\"b\".to_owned(), 2.0)])"),
        "{source}"
    );
}

#[test]
fn emits_object_assign_call() {
    let source = source_for(
        r"
const source: Record<string, number> = { a: 1 };
const merged = Object.assign({}, source, { b: 2 });
",
    );

    // `Object.assign(target, ...)` merges each source into the target in place
    // and evaluates to the (cloned) merged dict. The `{}` target is a fresh temp
    // local, so the merge extends that local directly rather than a throwaway
    // clone.
    assert!(source.contains(".extend("), "{source}");
    assert!(source.contains(".clone() }"), "{source}");
}

#[test]
fn object_assign_into_local_target_mutates_in_place() {
    // JavaScript `Object.assign(target, source)` mutates `target` in place and
    // returns it. When the target is a mutable local, the merge must extend that
    // local directly so a discarded call still updates the caller's binding,
    // rather than merging into a throwaway `assigned` clone that is dropped.
    let source = source_for(
        r"
function merge(source: Record<string, number>): Record<string, number> {
  const result: Record<string, number> = {};
  Object.assign(result, source);
  return result;
}
",
    );

    assert!(
        source.contains("result.extend("),
        "expected Object.assign into a local to extend the local in place: {source}"
    );
    assert!(
        !source.contains("let mut assigned = result"),
        "expected no throwaway clone of the local target: {source}"
    );
}

#[test]
fn emits_callable_interface_smelt_call_field_and_apply_routing() {
    // A callable interface (a call signature plus data/method members) lowers
    // to a record struct carrying a synthetic `__smelt_call` slot for the
    // underlying callable. `.apply` on such a value must read that slot and
    // erase it rather than accessing a non-existent `apply` struct field.
    let source = source_for(
        r"
interface Callable {
  (x: number): number;
  tag(): string;
}
function useApply(c: Callable): number {
  return c.apply(undefined, [1]) as number;
}
",
    );

    assert!(
        source.contains("__smelt_call"),
        "expected a synthetic __smelt_call field on the callable interface struct: {source}"
    );
    assert!(
        source.contains(".__smelt_call.clone()") && source.contains("into_smelt_unknown()"),
        "expected .apply to erase the __smelt_call slot: {source}"
    );
    assert!(
        !source.contains(".apply.clone()") && !source.contains(".apply)"),
        "expected no direct `.apply` struct field access: {source}"
    );
}

#[test]
fn emits_object_assign_call_on_callable_target() {
    let source = source_for(
        r"
const fnValue = (value: number): number => value;
const assigned = Object.assign(fnValue, { lazy: fnValue });
",
    );

    assert!(
        source.contains("let assigned") || source.contains("let mut assigned"),
        "{source}"
    );
    assert!(source.contains("_smelt_tmp_"));
}

#[test]
fn emits_inline_callable_object_assign_call_body() {
    let source = source_for(
        r"
function wrap(call: (...args: string[]) => void): unknown {
  let cached: string | undefined;
  return Object.assign(
    (...args: string[]) => {
      call(...args);
      return cached;
    },
    { flush: () => cached },
  );
}
",
    );

    assert!(
        source.contains("smelt_object.insert(\"__smelt_call\".to_owned()"),
        "{source}"
    );
    assert!(
        !source.contains("move |_smelt_args: Vec<SmeltUnknown>| SmeltUnknown::Null"),
        "{source}"
    );
    assert!(
        source.contains("let _smelt_tmp_3: () = (call)("),
        "{source}"
    );
}

#[test]
fn emits_promise_constructor_executor_result_future() {
    let source = source_for(
        r"
function makeValue(): Promise<number> {
  return new Promise<number>((resolve) => {
    resolve(1);
  });
}
",
    );

    assert!(source.contains("smelt_promise_result"), "{source}");
    assert!(source.contains("smelt_resolve"), "{source}");
    assert!(source.contains("break result;"), "{source}");
}

/// An async return supplies the resolved `T` as the contextual hint for an
/// unparameterized `new Promise`; generated settlement storage must retain it.
#[test]
fn untyped_promise_in_async_return_keeps_concrete_output() {
    let source = source_for(
        r"
async function makeValue(): Promise<number> {
  return new Promise((resolve) => {
    resolve(1);
  });
}
",
    );

    assert!(
        source.contains("Option<Result<f64, Box<dyn std::error::Error>>>"),
        "{source}"
    );
    assert!(
        !source.contains("Option<Result<(), Box<dyn std::error::Error>>>"),
        "{source}"
    );
}

#[test]
fn tuple_asserted_array_literal_renders_as_heterogeneous_tuple() {
    let source = source_for(
        r"
export function pair(promise: Promise<unknown>): [null, Promise<unknown>] {
  return [null, promise] as [null, Promise<unknown>];
}
",
    );
    assert!(source.contains("((), promise)"), "{source}");
    assert!(!source.contains("vec![(), promise]"), "{source}");
}

#[test]
fn named_promise_executor_supplies_resolved_list_type() {
    let source = source_for(
        r"
export function makeQueue(): Promise<number[]> {
  const processor = async (resolve: (value: number[]) => void) => {
    while (true) { return resolve([1, 2]); }
  };
  return new Promise(processor);
}
",
    );
    assert!(
        source.contains("Option<Result<SmeltList<f64>, Box<dyn std::error::Error>>>")
            && source.contains("Rc<dyn Fn(SmeltList<f64>) -> ()>"),
        "{source}"
    );
}

#[test]
fn awaited_conditional_call_uses_asserted_resolved_type_hint() {
    let source = source_for(
        r"
const attempt = <Return>(func: () => Return) => {
  return (): Return extends Promise<any>
    ? Promise<[unknown, unknown]>
    : [unknown, unknown] => undefined as any;
};
export async function run(func: () => Promise<string>): Promise<[unknown, string]> {
  return (await attempt(func)()) as [unknown, string];
}
",
    );
    assert!(
        !source.contains("SmeltUnknown::Array(smelt_tuple_values) = ().clone()"),
        "{source}"
    );
}

#[test]
fn nullish_unknown_fallback_uses_explicit_boundary_adapter() {
    let source = source_for(
        r"
export function fallback(value: unknown): unknown {
  return value ?? 'fallback';
}
",
    );
    assert!(
        source.contains("SmeltUnknown::String(\"fallback\".to_owned())"),
        "{source}"
    );
}

#[test]
fn computed_numeric_record_key_coerces_to_string_destination_key() {
    let source = source_for(
        r"
export function add(index: number, value: unknown): Record<string, unknown> {
  return { [index]: value };
}
",
    );
    assert!(source.contains("index.clone().to_string()"), "{source}");
}

/// Calls to emitted native `async fn`s inside generated closures are wrapped
/// in Smelt's stable future representation before assignment or adaptation.
#[test]
fn static_async_call_in_closure_is_wrapped_as_smelt_future() {
    let source = source_for(
        r"
async function load(): Promise<number> {
  return 1;
}

export function make(): () => Promise<number> {
  return async () => await load();
}
",
    );

    assert!(
        source.contains("SmeltFuture::from_future(Box::pin(load()))"),
        "{source}"
    );
}

/// An async *instance* method is emitted as a synchronous
/// `fn(&self, ..) -> SmeltFuture<T>` whose body already runs inside a moved
/// `async` block, so calling it yields a `SmeltFuture<T>` directly. Unlike a
/// free `async fn` (which yields an `impl Future` needing the stable-ABI
/// wrapper), an instance-method call must NOT be re-wrapped in
/// `SmeltFuture::from_future(Box::pin(..))` — `SmeltFuture<T>` is not a
/// `Future`, so the double wrap fails to compile (E0277). Regression guard for
/// the es-toolkit `Semaphore.acquire` double-wrap.
#[test]
fn async_instance_method_call_is_not_double_wrapped() {
    let source = source_for(
        r#"
class Semaphore {
  private available: number = 1;
  async acquire(): Promise<number> {
    return this.available;
  }
}
async function run(): Promise<number> {
  const sema = new Semaphore();
  return await sema.acquire();
}
"#,
    );

    assert!(
        source.contains("fn acquire(&self) -> SmeltFuture<f64>"),
        "{source}"
    );
    assert!(
        !source.contains("SmeltFuture::from_future(Box::pin(sema.acquire()))"),
        "async instance-method call must not be re-wrapped: {source}"
    );
    assert!(source.contains("sema.acquire()"), "{source}");
}

#[test]
fn emits_promise_constructor_executor_ignoring_callbacks() {
    // `new Promise(() => {})` declares a zero-parameter executor. The executor is
    // still called by the promise plumbing, so it must be invoked with no
    // arguments (not the fallback two-callback call) or Rust reports an arity
    // error against the `Fn()` closure.
    let source = source_for(
        r"
function makeValue(): Promise<void> {
  return new Promise<void>(() => {});
}
",
    );

    assert!(source.contains("smelt_promise_result"), "{source}");
    // Both callbacks are marked used and the executor is invoked with no args.
    assert!(source.contains("let _ = &smelt_resolve;"), "{source}");
    assert!(source.contains("let _ = &smelt_reject;"), "{source}");
}

#[test]
fn smelt_from_unknown_impls_cover_generic_containers() {
    // Un-erasing a generic return into a typed container relies on
    // `SmeltFromUnknown` impls for the runtime collection types.
    let source = source_for(
        r"
function identity<T>(value: T): T {
  return value;
}
function useList(values: number[]): number[] {
  return identity(values);
}
function useRecord(values: Record<string, string>): Record<string, string> {
  return identity(values);
}
",
    );

    assert!(
        source.contains("impl<T: SmeltFromUnknown> SmeltFromUnknown for SmeltList<T>"),
        "{source}"
    );
    assert!(
        source.contains("SmeltFromUnknown for SmeltRecord<K, V>"),
        "{source}"
    );
}

#[test]
fn promise_settimeout_resolving_value_threads_resolve_not_bare_sleep() {
    // `new Promise(resolve => setTimeout(() => resolve(v), ms))` must flow
    // through the executor (AsyncOp::Promise) so the resolved value survives.
    // Collapsing it to a bare `Sleep` (the old behavior) silently dropped `v`
    // and returned the output type's default.
    let source = source_for(
        r"
function makeValue(): Promise<number> {
  return new Promise<number>((resolve) => {
    setTimeout(() => resolve(7), 0);
  });
}
",
    );

    assert!(source.contains("smelt_promise_result"), "{source}");
    assert!(
        source.contains("Rc<dyn Fn(f64) -> ()>") && source.contains("closure_arg_0(7.0)"),
        "{source}"
    );
}

#[test]
fn promise_all_flattens_erased_promise_results() {
    // `Promise.all` over async functions that themselves return a `Promise`
    // settles each element to an erased `SmeltUnknown::Promise`. Awaiting must
    // flatten those to their resolved values (driving the scheduler) instead of
    // collecting the unresolved promise objects.
    let source = source_for(
        r"
function defer(value: unknown): Promise<unknown> {
  return new Promise<unknown>((resolve) => {
    setTimeout(() => resolve(value), 0);
  });
}

async function makeOne(value: unknown): Promise<unknown> {
  return defer(value);
}

async function run(): Promise<unknown[]> {
  return await Promise.all([makeOne(1), makeOne(2)]);
}
",
    );

    assert!(source.contains("smelt_await_flatten"), "{source}");
}

#[test]
fn promise_resolve_pushes_into_zero_arg_callback_queue() {
    // A `Promise<void>` executor's `resolve` is typed `(value?) => void`, so it
    // must satisfy a `() => void` FIFO queue slot (the semaphore deferred-task
    // pattern). The emitted push wraps the 1-arg resolve closure in a 0-arg
    // adapter that forwards a default value, rather than failing the build.
    let source = source_for(
        r"
class Sema {
  private deferredTasks: Array<() => void> = [];
  acquire(): Promise<void> {
    return new Promise<void>((resolve) => {
      this.deferredTasks.push(resolve);
    });
  }
}
export { Sema };
",
    );

    assert!(source.contains("smelt_promise_result"), "{source}");
    // The resolve callback is adapted into the zero-argument queue element type.
    assert!(source.contains("smelt_adapted"), "{source}");
}

#[test]
fn promise_bare_delay_settimeout_lowers_to_sleep() {
    // The pure-delay shape `new Promise(resolve => setTimeout(resolve, ms))`
    // resolves `undefined` after the delay and carries no value, so collapsing
    // it to `Sleep` stays correct (and keeps the cheap delay-shim fast path).
    let source = source_for(
        r"
function delay(): Promise<void> {
  return new Promise<void>((resolve) => setTimeout(resolve, 5));
}
",
    );

    assert!(source.contains("smelt_sleep_ms"), "{source}");
    assert!(!source.contains("smelt_promise_result"), "{source}");
}

#[test]
fn emits_object_has_own_methods() {
    let source = source_for(
        r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const first = Object.hasOwn(mapping, "a");
const second = mapping.hasOwnProperty("b");
"#,
    );

    assert!(source.contains(".contains_key(&"));
}

#[test]
fn emits_object_prototype_has_own_call_for_erased_generics() {
    let source = source_for(
        r"
function findKey<Value, Obj extends { [key in string | number]: Value }>(
  object: Obj,
  predicate: (value: Value) => boolean,
): string | undefined {
  for (const key in object) {
    if (
      Object.prototype.hasOwnProperty.call(object, key) &&
      predicate(object[key])
    ) {
      return key;
    }
  }
  return undefined;
}
",
    );

    assert!(
        source.contains("values.contains_key(&smelt_key)"),
        "{source}"
    );
    assert!(!source.contains("_smelt_tmp_7 = false;"), "{source}");
}

#[test]
fn prototype_sentinel_chain_terminates_at_null() {
    // Regression: `isPlainObject`/`Object.getPrototypeOf` walks the prototype
    // chain with `while (Object.getPrototypeOf(proto) !== null) proto = ...`,
    // re-invoking `smelt_prototype_sentinel` on the sentinel it just returned.
    // The sentinel used to answer `"__smelt_proto:object"` for every non-array,
    // non-promise, non-class value -- including the sentinel strings themselves
    // -- so the walk never reached `Null` and hung (es-toolkit toSnakeCaseKeys).
    // Each sentinel string must now advance one link toward `null`.
    let source = source_for(
        r"
function protoDepth(value: object): number {
  let depth = 0;
  let proto = Object.getPrototypeOf(value);
  while (Object.getPrototypeOf(proto) !== null) {
    proto = Object.getPrototypeOf(proto);
    depth = depth + 1;
  }
  return depth;
}
",
    );

    assert!(
        source.contains(
            "SmeltUnknown::String(marker) if marker == \"__smelt_proto:object\" => SmeltUnknown::Null"
        ),
        "object-prototype sentinel must terminate the chain at Null: {source}"
    );
    assert!(
        source.contains(
            "if marker == \"__smelt_proto:array\" || marker == \"__smelt_proto:promise\" || marker == \"__smelt_proto:class\" => SmeltUnknown::String(\"__smelt_proto:object\".to_owned())"
        ),
        "array/promise/class sentinels must advance toward Object.prototype: {source}"
    );
}

#[test]
fn emits_static_function_with_params_and_return_value() {
    let source = source_for(
        "function add(a: number, b: number): number {
  return a + b;
}
const result = add(2, 3);
console.log(result);
",
    );

    assert!(source.contains("fn add(a: f64, b: f64) -> f64 {"));
    assert!(source.contains("a.clone() + b.clone()"));
    assert!(source.contains("let _smelt_tmp_1: f64 = add(2.0, 3.0);"));
}

#[test]
fn emits_async_function_and_await() {
    let source = source_for(
        "async function lift(value: number): Promise<number> {
  return value;
}

async function run(): Promise<number> {
  return await lift(5);
}
",
    );

    assert!(
        source.contains("async fn lift(value: f64) -> Result<f64, Box<dyn std::error::Error>> {")
    );
    assert!(source.contains("async fn run() -> Result<f64, Box<dyn std::error::Error>> {"));
    assert!(source.contains(
        "let _smelt_tmp_0 = SmeltFuture::from_future(Box::pin(lift(5.0)));"
    ));
    assert!(source.contains("_smelt_tmp_1"));
    assert!(source.contains("_smelt_tmp_0.await?"));
}

#[test]
fn emits_promise_all_with_tokio_join() {
    let source = source_for(
        "async function lift(value: number): Promise<number> {
  return value;
}

async function run(): Promise<[number, number]> {
  return await Promise.all([lift(1), lift(2)]);
}
",
    );

    assert!(source.contains("tokio::join!(_smelt_tmp_0, _smelt_tmp_1)"));
    assert!(source.contains("Ok::<_, Box<dyn std::error::Error>>"));
}

#[test]
fn emits_if_else_control_flow() {
    let source = source_for(
        "function max(a: number, b: number): number {
  if (a > b) {
    return a;
  }
  return b;
}
const result = max(2, 3);
console.log(result);
",
    );

    assert!(source.contains("if _smelt_tmp_2 {"));
    assert!(source.contains("return a.clone();"));
    assert!(source.contains("return b.clone();"));
}

#[test]
fn emits_switch_as_rust_match() {
    let source = source_for(
        "function label(status: \"pending\" | \"approved\" | \"rejected\"): string {
  switch (status) {
    case \"pending\":
      return \"Waiting\";
    case \"approved\":
      return \"Approved\";
    case \"rejected\":
      return \"Rejected\";
  }
}
const result = label(\"approved\");
console.log(result);
",
    );

    assert!(source.contains("match status.as_str() {"));
    assert!(source.contains("\"pending\" => {"));
    assert!(source.contains("return \"Waiting\".to_owned();"));
    assert!(source.contains("_ => {"));
}

#[test]
fn emits_switch_inside_closure_as_rust_match() {
    let source = source_for(
        "const label = (status: \"pending\" | \"approved\"): string => {
  switch (status) {
    case \"pending\":
      return \"Waiting\";
    case \"approved\":
      return \"Approved\";
  }
};
const result = label(\"approved\");
console.log(result);
",
    );

    assert!(
        source.contains("_smelt_tmp_2 = ::std::rc::Rc::new(|closure_arg_0: String| {"),
        "{source}"
    );
    assert!(source.contains("match closure_arg_0.as_str() {"));
    assert!(source.contains("\"pending\" => {"));
    assert!(source.contains("\"Waiting\".to_owned()"));
    assert!(source.contains("_ => {"));
}

#[test]
fn emits_switch_with_heterogeneous_arm_successors() {
    // A switch whose arms have different control-flow tails: one case loops and
    // then falls through, one case returns early, and the default falls through.
    // The arm blocks therefore reach the shared continuation only transitively
    // (through the loop) or not at all (the early return), so no single join
    // block can be hoisted. This previously aborted emission with "match codegen
    // requires all non-terminating arms to share one join block"; the emitter now
    // gives each arm its own control-flow tail.
    let source = source_for(
        "function pick(kind: string, n: number): number {
  let total = 0;
  switch (kind) {
    case \"loop\": {
      while (n > 0) {
        total += n;
        n--;
      }
      break;
    }
    case \"double\":
      return n * 2;
    default:
      total = n;
  }
  return total;
}
const result = pick(\"loop\", 3);
console.log(result);
",
    );

    assert!(source.contains("match kind.as_str() {"), "{source}");
    assert!(source.contains("\"loop\" => {"), "{source}");
    assert!(source.contains("\"double\" => {"), "{source}");
    // The early-return arm diverges inline.
    assert!(source.contains("return"), "{source}");
    // The loop arm's own control-flow tail is emitted inside the arm.
    assert!(source.contains("while"), "{source}");
}

#[test]
fn emits_switch_arm_with_loop_and_conditional_throw() {
    // Mirrors es-toolkit's `trimEnd`: a `typeof` switch whose first arm branches
    // (a conditional throw) and then runs a loop before rejoining, while a later
    // arm is itself a loop. The arm regions converge on the trailing `return`
    // only transitively, which is exactly the heterogeneous-successor shape that
    // the join-block emitter must handle without a single hoisted join.
    let source = source_for(
        "function trimEnd(str: string, chars: string | string[]): string {
  let endIndex = str.length;
  switch (typeof chars) {
    case \"string\": {
      if (chars.length !== 1) {
        throw new Error(\"bad\");
      }
      while (endIndex > 0 && str[endIndex - 1] === chars) {
        endIndex--;
      }
      break;
    }
    case \"object\": {
      while (endIndex > 0 && chars.includes(str[endIndex - 1])) {
        endIndex--;
      }
    }
  }
  return str.substring(0, endIndex);
}
const result = trimEnd(\"aaa\", \"a\");
console.log(result);
",
    );

    // Emission succeeds (no EmitError) and the trailing continuation is emitted
    // as each arm's own tail rather than one hoisted join.
    assert!(source.contains("=> {"), "{source}");
    assert!(source.contains("while"), "{source}");
    assert!(source.contains("return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new("), "{source}");
}

#[test]
fn labeled_branch_reconstruction_before_loop_keeps_trailing_return() {
    // Mirrors es-toolkit `has`/`slice`/`updateWith`: a leading if/else-if chain
    // that assigns a variable lowers to a labeled-block reconstruction
    // (`'smelt_branch: { if ... break; ... }`), and the shared forward
    // continuation (the `for` loop plus the trailing `return`) is not re-emitted
    // after that block. A `return` emitted inside the labeled block (its own
    // fall-through default) set the divergence flag, which previously suppressed
    // `emit_body`'s trailing fallthrough return and left the labeled-block
    // statement's `()` value in tail position — the function then "implicitly
    // returns ()" and failed to type-check (E0308). The reconstruction must fall
    // through, so a terminating `return` still closes the body.
    let source = source_for(
        "export function pathExists(object: any, path: PropertyKey | PropertyKey[]): boolean {
  let resolvedPath;
  if (Array.isArray(path)) {
    resolvedPath = path;
  } else if (typeof path === 'string') {
    resolvedPath = [path];
  } else {
    resolvedPath = [path];
  }
  let current = object;
  for (let i = 0; i < resolvedPath.length; i++) {
    const key = resolvedPath[i];
    if (current == null) {
      return false;
    }
    current = current[key];
  }
  return true;
}
const ok = pathExists({ a: 1 }, ['a']);
console.log(ok);
",
    );

    // The reconstructed function body must end in a terminating `return`, not a
    // labeled-block statement whose value is `()`.
    let body_start = source
        .find("fn path_exists(object")
        .expect("path_exists function present");
    let after = &source[body_start..];
    let body_end = after.find("\n}\n").expect("function closing brace");
    let body = &after[..body_end];
    let last_line = body
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .expect("non-empty body");
    assert!(
        last_line.trim_start().starts_with("return "),
        "pathExists body must end in a return, got last line: {last_line:?}\n{source}"
    );
}

#[test]
fn emits_uncaught_throw_as_result() {
    let source = source_for(
        "function fail(): void {
  throw \"boom\";
}
fail();
",
    );

    assert!(source.contains("fn fail() -> Result<(), Box<dyn std::error::Error>> {"));
    assert!(source.contains("return Err::<_, Box<dyn std::error::Error>>(std::io::Error::new("));
    assert!(source.contains("fn main() -> Result<(), Box<dyn std::error::Error>> {"));
    assert!(source.contains("let _ = fail()?;"));
    assert!(source.contains("return Ok(());"));
}

#[test]
fn emits_fetch_as_reqwest_get_text_future() {
    let source = source_for(
        "async function load(): Promise<string> {
  return await fetch(\"https://example.com\");
}
",
    );

    assert!(source.contains(
            "reqwest::get(\"https://example.com\".to_owned()).await.expect(\"HTTP GET failed\").text().await.expect(\"HTTP response body read failed\")"
        ));
}

#[test]
fn emits_python_requests_get_as_blocking_reqwest_text() {
    let mut ctx = py_frontend::HirCtx::new();
    assert!(
            py_frontend::to_hir(
                "import requests\n\ndef load() -> str:\n    return requests.get(\"https://example.com\")\n",
                FileId(0),
                &mut ctx,
            )
            .is_ok(),
            "HIR"
        );
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);
    let source = match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    };

    assert!(source.contains(
            "reqwest::blocking::get(\"https://example.com\".to_owned()).expect(\"HTTP GET failed\").text().expect(\"HTTP response body read failed\")"
        ));
}

#[test]
fn injects_reqwest_dependency_for_http_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[
            GeneratedDep::Tokio,
            GeneratedDep::Stdlib(BackendDependency::Reqwest),
        ],
    );

    assert!(manifest.contains("tokio = { version = \"1\""));
    assert!(manifest.contains("reqwest = { version = \"0.12\""));
}

/// A generated crate that emits `genawaiter::rc::Gen` generator bodies must
/// also declare the `genawaiter` dependency. Regression for PR #178: the
/// dependency was gated on a `Type::Generator` in the types table, but a
/// generator function whose generator type is erased still emits genawaiter
/// bodies — so the es-toolkit spec compiled to code that could not build. The
/// dependency detection now matches the emission condition (`is_generator`).
#[test]
fn injects_genawaiter_dependency_for_generator_emission() {
    let mut ctx = HirCtx::new();
    assert!(
        to_hir(
            "function* values(limit: number): Generator<number> {\n\
             \x20 for (let i = 0; i < limit; i += 1) {\n\
             \x20   yield i;\n\
             \x20 }\n\
             }\n\
             function total(): number {\n\
             \x20 let sum = 0;\n\
             \x20 for (const v of values(5)) {\n\
             \x20   sum += v;\n\
             \x20 }\n\
             \x20 return sum;\n\
             }\n",
            FileId(0),
            &mut ctx,
        )
        .is_ok(),
        "HIR"
    );
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);

    let source = match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    };
    assert!(
        source.contains("genawaiter::rc::Gen"),
        "generator function should emit a genawaiter state machine: {source}"
    );
    // The runtime prelude that the emitted body references must be defined in
    // the same crate; otherwise the generated code references an undeclared
    // `SmeltGeneratorResult` (the es-toolkit `cargo test --no-run` failure).
    assert!(
        source.contains("pub enum SmeltGeneratorResult"),
        "genawaiter emission must also emit the generator runtime prelude: {source}"
    );

    let manifest =
        deps::cargo_toml(&EmitOptions::default().crate_name, &crate::generated_deps(&mir));
    assert!(
        manifest.contains("genawaiter = \"0.99.1\""),
        "a crate that emits genawaiter bodies must declare the dependency: {manifest}"
    );
}

/// A closure field on a *type-alias* object receiver must stay a closure call
/// even inside callback-migrated bodies. Regression for PR #178: the new
/// `callback_receiver_declares_method` treated `type_alias_fields` entries as
/// class methods, but that map also stores genuine alias fields (remeda's
/// `Funnel.flush`), which have no class item — MIR method resolution then
/// failed with "class method `flush` is not resolvable".
#[test]
fn alias_closure_field_call_in_callback_body_stays_closure_call() {
    let source = source_for(
        "type Funnel = {\n\
           readonly call: () => void;\n\
           readonly flush: () => void;\n\
           readonly isIdle: boolean;\n\
         };\n\
         function funnel(cb: () => void): Funnel {\n\
           let idle = true;\n\
           return {\n\
             call: () => { idle = false; cb(); },\n\
             flush: () => { idle = true; cb(); },\n\
             get isIdle() { return idle; },\n\
           };\n\
         }\n\
         export function makeDebouncer(fn: () => void) {\n\
           let cached: number | undefined;\n\
           const debouncingFunnel = funnel(fn);\n\
           return {\n\
             call: () => { debouncingFunnel.call(); return cached; },\n\
             flush: () => { debouncingFunnel.flush(); return cached; },\n\
             get isPending() { return !debouncingFunnel.isIdle; },\n\
           };\n\
         }\n",
    );
    // Reaching here means MIR lowering resolved every call; the alias field
    // dispatches as a stored closure, not a class method item.
    assert!(!source.is_empty());
}

/// A `function*` expression stored in an erased slot crosses the dynamic
/// boundary at construction and must erase through the `IntoSmeltUnknown`
/// iterator adapter. Regression for PR #178: the generator closure returned a
/// concrete `SmeltGenerator` where the erased signature required
/// `SmeltUnknown` (E0271 in remeda's `length` iterable test, E0308 in
/// es-toolkit's `isFunction` spec). The boundary is genuinely dynamic — a live
/// resumable state machine has no concrete/union/generic representation on
/// the erased side, only the iterator protocol the adapter reproduces.
#[test]
fn generator_in_erased_slot_erases_through_into_smelt_unknown() {
    let source = source_for(
        "function length<T>(items: Iterable<T>): number {\n\
           return [...items].length;\n\
         }\n\
         export function countYields(): number {\n\
           return length({\n\
             *[Symbol.iterator]() {\n\
               yield 0;\n\
               yield 1;\n\
               yield 2;\n\
             },\n\
           });\n\
         }\n",
    );
    assert!(
        source.contains("genawaiter::rc::Gen"),
        "generator expression should emit a genawaiter state machine: {source}"
    );
    assert!(
        source.contains(").into_smelt_unknown()"),
        "erased-slot generator must erase through IntoSmeltUnknown: {source}"
    );
    assert!(
        source.contains("impl<Y: IntoSmeltUnknown + 'static, R: IntoSmeltUnknown + Clone + 'static, N: Default + 'static> IntoSmeltUnknown for SmeltGenerator<Y, R, N>"),
        "prelude must define the generator boundary adapter: {source}"
    );
    // Consumers of an erased iterable must drain the iterator protocol the
    // adapter exposes, not just accept a materialized array.
    assert!(
        source.contains("fn smelt_unknown_iterator_items"),
        "list extraction must drain the erased iterator protocol: {source}"
    );
}

#[test]
fn injects_serde_json_dependency_for_json_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::SerdeJson)],
    );

    assert!(manifest.contains("serde_json = \"1\""));
}

#[test]
fn injects_rand_dependency_for_random_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Rand)],
    );

    assert!(manifest.contains("rand = \"0.9\""));
}

#[test]
fn injects_regex_dependency_for_regex_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Regex)],
    );

    assert!(manifest.contains("regex = \"1\""));
}

#[test]
fn injects_chrono_dependency_for_date_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Chrono)],
    );

    assert!(manifest.contains("chrono = \"0.4\""));
}

#[test]
fn emits_date_fns_date_parts() {
    let source = source_for(
        r"
const date = new Date(2014, 8, 2, 11, 55, 0);
const timestamp = date.getTime();
const year = date.getFullYear();
const month = date.getMonth();
const day = date.getDate();
date.setFullYear(year, month, day + 1);
date.setMonth(0, 1);
date.setDate(2);
",
    );

    assert!(source.contains("chrono::NaiveDate::from_ymd_opt"));
    assert!(source.contains(".year() as f64"));
    assert!(source.contains(".month0() as f64"));
    assert!(source.contains(".day() as f64"));
    assert!(source.contains("normalized_year"));
    assert!(source.contains("normalized_month0"));
    assert!(source.contains("chrono::Duration::days"));
}

#[test]
fn emits_invalid_date_parts_without_panicking() {
    let source = source_for(
        r"
export function isExists(year: number, month: number, day: number): boolean {
  const date = new Date(year, month, day);
  return (
    date.getFullYear() === year &&
    date.getMonth() === month &&
    date.getDate() === day
  );
}
",
    );

    assert!(source.contains("unwrap_or(i64::MIN)"));
    assert!(source.contains("map_or(f64::NAN"));
    assert!(source.contains("timestamp_ms.is_finite()"));
    assert!(!source.contains("timestamp out of range"));
}

#[test]
fn preserves_invalid_date_through_setters_and_iso_conversion() {
    let source = source_for(
        r"
export function keepInvalid(): string {
  const date = new Date(NaN);
  date.setHours(0);
  return date.toISOString();
}
",
    );

    assert!(source.contains("timestamp_ms.is_finite()"), "{source}");
    assert!(source.contains("else { i64::MIN }"), "{source}");
    assert!(
        source.contains("timestamp_ms == i64::MIN as f64 { f64::NAN }"),
        "{source}"
    );
    assert!(source.contains(".to_iso_string()"), "{source}");
}

#[test]
fn emits_overflowing_time_setters_as_duration_arithmetic() {
    let source = source_for(
        r"
export function nextHour(): number {
  const date = new Date(2020, 0, 1, 23, 30, 0, 0);
  return date.setHours(date.getHours() + 1);
}
",
    );

    assert!(
        source.contains("chrono::Duration::milliseconds"),
        "{source}"
    );
    assert!(source.contains("* 3_600_000"), "{source}");
    assert!(
        !source.contains("with_hour"),
        "setHours must preserve JavaScript overflow semantics: {source}"
    );
}

/// A class extending the `Error` builtin must declare the inherited dynamic
/// marker fields (`name`/`message`/`stack`/`cause`) so its constructor's
/// `this.name = ...` assignment references a real struct field (regression for
/// generated `E0609: no field \`name\``).
#[test]
fn error_subclass_declares_inherited_marker_fields() {
    let source = source_for(
        r#"
class HttpError extends Error {
  code: number;
  constructor(message: string, code: number) {
    super(message);
    this.name = 'CustomError';
    this.code = code;
  }
}
export function make(): HttpError { return new HttpError('x', 400); }
"#,
    );

    assert!(source.contains("name:"), "missing name field: {source}");
    assert!(source.contains("message:"), "missing message field: {source}");
    assert!(
        source.contains("this.name ="),
        "constructor should assign name: {source}"
    );
}

/// An empty collection literal used as a class field initializer must adopt the
/// declared field's key/value types in a generic class rather than defaulting to
/// `Unknown`, so the assignment does not need a lossy key conversion that cannot
/// collect into the generic field type (regression for generated `E0277` on
/// `SmeltJsMap<T, String>`).
#[test]
fn empty_map_field_initializer_adopts_declared_types() {
    let source = source_for(
        r#"
class Cache<T> {
  private data: Map<T, string> = new Map();
  size(): number { return this.data.size; }
}
export function make(): Cache<string> { return new Cache<string>(); }
"#,
    );

    assert!(
        !source.contains("SmeltJsMap<SmeltUnknown, SmeltUnknown> = SmeltJsMap::from([])"),
        "empty map field initializer should not default to Unknown keys: {source}"
    );
}

/// `console.log` of a value with no Rust `Display` impl (a generic type
/// parameter, class instance, union, or function) must erase to the runtime
/// `SmeltUnknown` form, which implements `Display` (regression for generated
/// `E0277: \`A\` doesn't implement Display`).
#[test]
fn console_log_of_non_display_value_erases_to_unknown() {
    let source = source_for(
        r#"
export function show<A>(value: A): void {
  console.log(value);
}
"#,
    );

    assert!(
        source.contains("into_smelt_unknown()"),
        "console.log of a generic value should erase to SmeltUnknown: {source}"
    );
}

/// A borrowed callback parameter that flows into a callee's owned callback sink
/// (an `Optional<fn(..)>` parameter lowered to `Option<Rc<dyn Fn>>`) must itself
/// be emitted as an owned `Rc<dyn Fn>`, otherwise codegen wraps the non-`'static`
/// borrow in an escaping `Rc::new(..)` adapter that fails borrowck (regression
/// for "lifetime may not live long enough").
#[test]
fn callback_flowing_into_owned_sink_is_owned() {
    let source = source_for(
        r"
function inner(cb?: (x: number) => number): number { return cb ? cb(1) : 0; }
export function outer(cb: (x: number) => number): number { return inner(cb); }
",
    );

    let outer_sig = source
        .lines()
        .find(|line| line.contains("fn outer"))
        .unwrap_or("");
    assert!(
        outer_sig.contains("Rc<dyn Fn"),
        "outer's callback param should be owned: {outer_sig}"
    );
}

/// A closure whose body only ever throws must carry an explicit
/// `-> Result<_, Box<dyn Error>>` return annotation, otherwise neither the `Ok`
/// nor the error type can be inferred from the diverging body (regression for
/// `E0282`/`E0283` on a throw-only arrow).
#[test]
fn throw_only_closure_annotates_result_return_type() {
    let source = source_for(
        r#"
export function makeThrower(): () => number {
  return () => { throw new Error('nope'); };
}
"#,
    );

    assert!(
        source.contains("-> Result<") && source.contains("Box<dyn std::error::Error>"),
        "throw-only closure should annotate its Result return type: {source}"
    );
}

#[test]
fn promise_then_hoists_captured_callback_out_of_async_move() {
    // `.then(cb)` runs its callback inside a `Box::pin(async move { .. })` block,
    // which captures referenced outer bindings by move. A callback naming a live
    // outer variable must be cloned into a binding OUTSIDE the async block (JS
    // `.then` does not consume `cb`), so the source binding stays usable after
    // the call (was E0382: borrow of moved value).
    let source = source_for(
        r"
async function run(p: Promise<number>, cb: (v: number) => void): Promise<void> {
  await p.then(cb);
  cb(1);
}
",
    );
    assert!(
        source.contains("let smelt_promise_callback = (cb).clone();"),
        "then callback should be hoisted and cloned before the async block: {source}"
    );
    // The clone binding precedes the async block, never inside it.
    let hoist = source
        .find("let smelt_promise_callback")
        .expect("hoist binding present");
    let async_block = source[hoist..]
        .find("async move")
        .expect("async block after hoist");
    assert!(async_block > 0, "hoist must come before async move: {source}");
}

/// A zero-arity `.then`/`.catch` continuation callback must be invoked with NO
/// arguments even though the promise machinery always has a resolved value on
/// hand: JavaScript passes the value, but a 0-parameter Rust closure would
/// reject it (was E0057). The value is dropped through `let _ = ..`.
#[test]
fn promise_then_zero_arity_callback_dropped_value() {
    let source = source_for(
        r"
async function run(p: Promise<number>): Promise<void> {
  const cb = () => { return; };
  await p.then(cb);
}
",
    );
    assert!(
        source.contains("let _ = smelt_value; (smelt_promise_callback)()"),
        "0-arity then callback must be called with no args, value dropped: {source}"
    );
}

/// A throwing async callback returns a future whose Rust type is
/// `-> SmeltFuture<..>` with no outer `Result` (the throw lives in the future's
/// output). An adapter chain / promise erasure around such a callback must NOT
/// re-wrap the forwarded future in `Ok(..)`, or the await seam observes a nested
/// `Result<Result<Future, _>, _>` (was E0277).
#[test]
fn throwing_async_callback_future_not_double_wrapped() {
    let source = source_for(
        r"
export function limitAsync(
  fn: (...args: unknown[]) => Promise<unknown>,
  concurrency: number
): (...args: unknown[]) => Promise<unknown> {
  return fn;
}

async function run(): Promise<void> {
  const callback = async (item: number): Promise<number> => {
    if (item === 2) {
      throw new Error('fail');
    }
    return item;
  };
  const limited = limitAsync(callback, 2);
  await limited(1);
}
",
    );
    assert!(
        !source.contains("Ok::<_, Box<dyn std::error::Error>>((smelt_callback)(smelt_args"),
        "future-returning adapter must forward the bare future, not re-wrap in Ok: {source}"
    );
}

/// A `const x = a || (b && c);` initializer inside a `switch` case lowers the
/// short-circuit to a `Conditional` (nested `Conditional` for the `&&` arm).
/// The conditional's join block must rank above every block its arms introduce,
/// otherwise the else arm's nested blocks outrank the join, the `goto join`
/// becomes a spurious back-edge, and the Rust emitter drops the whole tail that
/// follows the initializer in the same case body — the es-toolkit `isEqualWith`
/// `[object Object]` arm lost its `if (!areEqualInstances) return false;`, the
/// `Object.keys` walk, and the final `return true`.
#[test]
fn short_circuit_initializer_keeps_switch_case_tail() {
    let source = source_for(
        "function f(a: unknown, b: unknown, tag: string, ks: number[]): boolean {
  switch (tag) {
    case \"x\": {
      const flag = g(a) || (h(a) && h(b));
      if (!flag) {
        return false;
      }
      for (const k of ks) {
        if (k === 0) {
          return false;
        }
      }
      return true;
    }
    default:
      return false;
  }
}
function g(x: unknown): boolean { return x === null; }
function h(x: unknown): boolean { return x === undefined; }
const r = f(1, 2, \"x\", [1]);
console.log(r);
",
    );

    // The post-initializer tail must survive: the `!flag` guard, the loop that
    // walks `ks`, and the terminating `return true` all follow the short-circuit
    // assignment inside the same case body.
    assert!(source.contains("flag = "), "flag assignment dropped: {source}");
    assert!(
        source.contains("!(flag)") || source.contains("!flag"),
        "`if (!flag)` guard dropped: {source}"
    );
    assert!(
        source.contains("ks.len()"),
        "`Object.keys`-style tail loop dropped: {source}"
    );
    assert!(
        source.contains("return true;"),
        "terminating `return true` dropped: {source}"
    );
}

/// A local declared with an explicit `any` annotation is the erased dynamic
/// boundary by source spelling, so a later concrete object assignment must not
/// flow-narrow it to that value's record type. Otherwise a self-referential
/// write (`o.b = o`) demands a concrete record value the erased shape cannot
/// supply (was E0308: expected `f64`, found `SmeltRecord<String, f64>`).
#[test]
fn explicit_any_local_not_narrowed_by_assignment() {
    let source = source_for(
        r#"
export function run(): void {
  let o: any = { a: true };
  o = { a: 1, b: 2, c: 3 };
  o.b = o;
}
"#,
    );
    assert!(
        source.contains("let mut o: SmeltUnknown"),
        "explicit any local must keep its erased SmeltUnknown storage type: {source}"
    );
    assert!(
        source.contains("match &mut o { SmeltUnknown::Object(map)"),
        "write through an explicit any local must go through the erased boundary: {source}"
    );
}

/// A `const x = f() || (g() && h());` whose join continuation (the `<tail>;
/// return true`) is non-terminating (contains a loop) previously lowered to a
/// labeled-block short-circuit reconstruction: `'smelt_branch: { if lhs { x =
/// true; break 'smelt_branch; } <else + tail>; return true; }`. The then-arm's
/// `break` skipped the ENTIRE shared join tail and control fell off the label
/// into the function epilogue (`return SmeltUnknown::Null` -> coerced false),
/// silently deleting the join — the root cause of the es-toolkit isEqualWith /
/// isEqual deep-object failures. The join must now be emitted once, after a
/// plain structured `if/else`, so BOTH the then-arm and the else-arm resume at
/// it. Assert no label/break traps the join and the tail is reachable in order.
#[test]
fn short_circuit_or_join_resumes_at_shared_tail() {
    let source = source_for(
        "function f(a: any): boolean { return (a as any).x === 1; }
function g(a: any): boolean { return (a as any).y === 2; }
export function deepEq(a: any, b: any): boolean {
  const areEqual = f(a) || (g(a) && g(b));
  if (!areEqual) {
    return false;
  }
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b);
  if (aKeys.length !== bKeys.length) {
    return false;
  }
  for (let i = 0; i < aKeys.length; i++) {
    const key = aKeys[i];
    if ((a as any)[key] !== (b as any)[key]) {
      return false;
    }
  }
  return true;
}
const ok = deepEq({ a: 1 }, { a: 1 });
console.log(ok);
",
    );

    let start = source.find("fn deep_eq").expect("deep_eq present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("deep_eq closing brace");
    let body = &after[..end];

    // The join must not be trapped inside a synthetic label that the then-arm
    // `break`s over. Correct lowering is a plain structured `if/else`.
    assert!(
        !body.contains("break 'smelt_branch"),
        "the short-circuit join must not be trapped in a labeled block the \
         then-arm breaks over:\n{body}"
    );

    // The join (`are_equal = ...`) and the whole tail must be emitted after the
    // if/else and reachable, in source order, ending in the terminating tail.
    let then_true = body
        .find("= true;")
        .expect("then-arm assigns the short-circuit result to true");
    let join = body
        .find("are_equal = ")
        .expect("join assignment emitted");
    let guard = body
        .find("return false;")
        .expect("`if (!areEqual) return false` guard emitted");
    let tail = body
        .rfind("return true;")
        .expect("terminating `return true` tail emitted");
    assert!(
        then_true < join && join < guard && guard < tail,
        "then-arm result, join, guard, and terminating tail must appear in \
         reachable source order (then={then_true}, join={join}, guard={guard}, \
         tail={tail}):\n{body}"
    );
}

/// Sibling of the case documented at the labeled-block reconstruction in
/// `control_flow.rs`: es-toolkit `some`'s non-array branch is a then-branch
/// that always diverges (a loop that always `return`s). The short-circuit
/// forward-join reconstruction silently discards the then-branch's own `Goto`
/// target, which would delete that whole diverging region and let control fall
/// through into the else continuation, reading its loop counter uninitialized
/// (E0381). Such a diverging then-branch must be lowered as a structured `if`
/// arm that keeps the region intact, with the array continuation emitted after.
#[test]
fn diverging_then_branch_keeps_its_region() {
    let source = source_for(
        "export function some(source: any, predicate: (v: any, k: any, s: any) => boolean): boolean {
  if (!Array.isArray(source)) {
    for (const key in source) {
      if (predicate((source as any)[key], key, source)) {
        return true;
      }
    }
    return false;
  }
  for (let i = 0; i < (source as any[]).length; i++) {
    if (predicate((source as any[])[i], i, source)) {
      return true;
    }
  }
  return false;
}
const ok = some([1, 2, 3], (v: any) => v === 2);
console.log(ok);
",
    );

    let start = source.find("fn some").expect("some present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("some closing brace");
    let body = &after[..end];

    // The diverging then-branch must not be dropped, and it must not be
    // reconstructed via a label the array continuation escapes past.
    assert!(
        !body.contains("break 'smelt_branch"),
        "diverging then-branch must be a structured `if` arm, not a labeled \
         block:\n{body}"
    );
    // Both the object-key walk (then-branch, the diverging region) and the
    // array index walk (the continuation) must survive.
    let for_in = body
        .find("smelt_is_for_in_record_key")
        .expect("object-key (for-in) loop of the diverging then-branch preserved");
    let array_guard = body
        .rfind("if !(")
        .expect("array index loop continuation preserved");
    assert!(
        for_in < array_guard,
        "the diverging then-branch region must precede the array continuation \
         (for_in={for_in}, array_guard={array_guard}):\n{body}"
    );
}
