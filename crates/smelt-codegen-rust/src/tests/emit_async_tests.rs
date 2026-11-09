//! Generated Rust for async functions, promises and timers.

use super::*;

/// Erasing an async callback into an unknown-rest function must wrap its
/// returned future directly. Throws from an async body live inside that future;
/// the callback invocation itself does not return a `Result` to unwrap.
#[test]
fn erased_async_throwing_callback_does_not_unwrap_future_as_result() {
    let input = r#"
export const tryit = <Args extends unknown[], Return>(
  func: (...args: Args) => Return
) => {
  return (...args: Args): unknown => func(...args)
}

export function run(): unknown {
  return tryit(async () => {
    throw new Error("failure")
  })
}
"#;
    let source = source_for(input);

    assert!(source.contains("SmeltUnknown::Promise(SmeltPromise::from_future"), "{source}");
    assert!(
        !source.contains("(smelt_callback)(smelt_args).unwrap_or_else")
            && !source.contains("(smelt_callback)().unwrap_or_else"),
        "{source}"
    );
}

/// Erasing a typed promise value (`SmeltFuture<T>`) to the dynamic carrier goes
/// through `IntoSmeltUnknown`, so the erased-callback promise adapter and the
/// recover-erased-promise-on-await coercion both emit `.into_smelt_unknown()` on
/// a future. The prelude must therefore always define
/// `impl<..> IntoSmeltUnknown for SmeltFuture<T>` for any program that references
/// the future runtime; otherwise the generated call is an E0599 (regression seen
/// in the radash suite, where the impl was absent while the call was emitted).
#[test]
fn future_erasure_emits_into_smelt_unknown_impl() {
    let source = source_for(
        r"
export async function makeValue(): Promise<number> {
  return 1
}

export async function useErased(func: (...args: unknown[]) => unknown): Promise<unknown> {
  return func()
}
",
    );

    assert!(
        source.contains(
            "impl<T: IntoSmeltUnknown + Clone + 'static> IntoSmeltUnknown for SmeltFuture<T>"
        ),
        "future runtime must emit the SmeltFuture IntoSmeltUnknown impl: {source}"
    );
}

/// An async closure whose loop exits only through explicit returns leaves the
/// trailing async-value expression unreachable. Its binding still needs the
/// resolved output type so Rust does not have to infer a value from `!`.
#[test]
fn async_closure_with_returning_loop_annotates_unreachable_tail() {
    let source = source_for(
        r"
export function worker(done: (values: unknown[]) => void): () => Promise<void> {
  return async () => {
    const values: unknown[] = []
    while (true) {
      if (values.length === 0) return done(values)
    }
  }
}
",
    );

    assert!(
        source.contains("let smelt_async_value: () = {")
            || source.contains("let smelt_async_value: SmeltUnknown = {"),
        "{source}"
    );
    assert!(!source.contains("let smelt_async_value = {"), "{source}");
}

/// A `return <literal>` statement inside an `async` function returns the
/// *resolved* value, not a promise: the async lowering wraps the whole body
/// into the future. When the declared return type is `Promise<[null, T]>` the
/// returned tuple/array literal must lower to the erased value directly, never
/// be coerced into a `SmeltPromise::from_future(..)` around a non-future value
/// (which produced `let _tmp: Pin<Box<dyn Future<..>>> = vec![..];`, E0308).
#[test]
fn async_return_of_tuple_literal_lowers_to_value_not_promise_wrapper() {
    let source = source_for(
        r"
export async function attemptAsync<T, E>(func: () => Promise<T>): Promise<[null, T] | [E, null]> {
  try {
    const result = await func();
    return [null, result];
  } catch (error) {
    return [error as E, null];
  }
}
",
    );

    // The success return lowers to an erased array value, not a future wrapper.
    assert!(
        source.contains("SmeltUnknown::Array(vec![SmeltUnknown::Null, result"),
        "{source}"
    );
    // No promise-from-future wrapper is emitted around the returned tuple, and
    // no return temporary is typed as a boxed future initialized from a value.
    assert!(
        !source.contains("SmeltPromise::from_future(Box::pin(async move { let smelt_value = smelt_future.await"),
        "{source}"
    );
    assert!(
        !source.contains("Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>> = vec!"),
        "{source}"
    );
}

/// An `async` function desugars to a future whose `Output` is
/// `Result<T, Box<dyn Error>>`, even when the source body never throws. A
/// `return value;` inside such a body must therefore be wrapped in `Ok(..)`;
/// emitting a bare `return value;` mismatched the `Result` output (E0308). This
/// exercises a non-throwing async function returning a concrete value.
#[test]
fn async_non_throwing_return_is_ok_wrapped() {
    let source = source_for(
        r"
export async function firstEven(values: number[]): Promise<number> {
  for (const value of values) {
    if (value % 2 === 0) {
      return value;
    }
  }
  return -1;
}
",
    );

    assert!(source.contains("async fn first_even"), "{source}");
    // Both the mid-body and trailing return are wrapped in Ok(..).
    assert!(source.contains("return Ok("), "{source}");
    // No bare numeric return leaks past the Result output.
    assert!(!source.contains("return -1.0;"), "{source}");
}

#[test]
fn awaits_a_value_or_promise_union_to_its_joined_type() {
    // `await` unwraps the promise arms and passes the others through. The value
    // arm has to survive: asserting the whole union to be a future selected the
    // promise arm and `unreachable!`-ed the half that needs no waiting.
    let source = source_for(
        r"
class Cell {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
}

function pick(sync: boolean): Cell | Promise<Cell> {
  return sync ? new Cell(1) : Promise.resolve(new Cell(2));
}

export async function run(): Promise<number> {
  const cell = await pick(true);
  return cell.value;
}
",
    );

    assert!(source.contains("let cell: Cell"), "{source}");
    assert!(
        source.contains("SmeltFuture::<Cell>::resolved(value)"),
        "the value arm must become an already-resolved handle: {source}"
    );
    assert!(
        !source.contains("let cell: SmeltUnknown"),
        "the awaited union must not erase: {source}"
    );
}

/// Async generators expose promise-valued resumes while retaining the same
/// typed yield/completion carrier as synchronous generators.
#[test]
fn emits_resumable_async_generator() {
    let source = source_for(
        r#"
async function* sequence(): AsyncGenerator<number, string, unknown> {
  yield 1;
  await Promise.resolve(2);
  yield 2;
  return "done";
}

async function consume(): Promise<string> {
  const iterator = sequence();
  const first = await iterator.next();
  const second = await iterator.next();
  const completed = await iterator.next();
  return completed.value as string;
}
"#,
    );

    assert!(
        source.contains("SmeltAsyncGenerator<f64, JsString, SmeltUnknown>"),
        "{source}"
    );
    assert!(source.contains("smelt_generator.async_resume().await"), "{source}");
    assert!(source.contains("SmeltFuture<SmeltGeneratorResult<f64, JsString>>"), "{source}");
    assert_eq!(source.matches("smelt_co.yield_").count(), 2, "{source}");
    assert!(
        source.matches(".resume(SmeltGeneratorCommand::Next").count() >= 3,
        "{source}"
    );
}

/// A method shared by every arm of a concrete union must dispatch through the
/// generated tagged enum while preserving a generator-valued generic return.
#[test]
fn emits_union_method_returning_generator_for_async_delegation() {
    let source = source_for(
        r"
class Ok<T> {
  constructor(readonly value: T) {}
  safeUnwrap(): Generator<number, T, unknown> {
    const value = this.value;
    return (function* (): Generator<number, T, unknown> { return value; })();
  }
}

class Err<T> {
  constructor(readonly value: T) {}
  safeUnwrap(): Generator<number, T, unknown> {
    const value = this.value;
    return (function* (): Generator<number, T, unknown> { return value; })();
  }
}

type Result<T> = Ok<T> | Err<T>;

async function* unwrap<T>(promise: Promise<Result<T>>): AsyncGenerator<number, T, unknown> {
  return yield* await promise.then((result) => result.safeUnwrap());
}
",
    );

    assert!(source.contains("match closure_arg_0"), "{source}");
    assert!(source.contains("::M0(value) => value.safe_unwrap()"), "{source}");
    assert!(source.contains("::M1(value) => value.safe_unwrap()"), "{source}");
    assert!(source.contains("let smelt_delegate"), "{source}");
}

#[test]
fn emits_rejects_to_throw_as_awaited_result_match() {
    let source = source_for(
        r#"
import { expect, test } from "vitest";

async function fail(): Promise<void> {
  throw "boom";
}

test("rejects", async () => {
  await expect(fail()).rejects.toThrow("boom");
});
"#,
    );

    assert!(source.contains("match _smelt_tmp_"), "{source}");
    assert!(source.contains(".await {"), "{source}");
    assert!(source.contains("Err(__smelt_error)"), "{source}");
    assert!(
        source.contains("contains(&JsString::from(\"boom\"))"),
        "{source}"
    );
}

#[test]
fn resets_virtual_timers_at_generated_test_start() {
    let source = source_for(
        r#"
import { test } from "vitest";

test("timer isolation", () => {
  setTimeout(() => {}, 10);
});
"#,
    );

    assert!(source.contains("fn smelt_reset_timers()"), "{source}");
    assert!(source.contains("    smelt_reset_timers();"), "{source}");
}

#[test]
fn set_interval_registers_repeating_timer_and_clear_interval_cancels() {
    // `setInterval`/`clearInterval` must lower onto the same virtual-time timer
    // queue as `setTimeout`, with the interval re-arming itself after each fire.
    let source = source_for(
        r"
const id = setInterval(() => {}, 10);
clearInterval(id);
",
    );

    // The dedicated repeating-timer helper is emitted and called.
    assert!(source.contains("fn smelt_set_interval("), "{source}");
    assert!(source.contains("smelt_set_interval("), "{source}");
    // `clearInterval` reuses the cancel-by-id timer path.
    assert!(source.contains("fn smelt_clear_interval"), "{source}");
    assert!(source.contains("smelt_clear_interval("), "{source}");
    // Interval timers carry a period and re-arm in the drain loop; one-shot
    // timeouts carry `period_ms: None`.
    assert!(source.contains("period_ms: Some(period_ms)"), "{source}");
    assert!(
        source.contains("if let Some(period_ms) = timer.period_ms {"),
        "{source}"
    );
}

#[test]
fn drains_virtual_timers_before_async_sleep_can_change_threads() {
    let source = source_for(
        r"
setTimeout(() => {}, 10);
",
    );

    let sleep_body = source
        .split("async fn smelt_sleep_ms(delay_ms: f64) {")
        .nth(1)
        .expect("missing smelt_sleep_ms helper");
    let drain = sleep_body
        .find("        smelt_drain_due_timers(id_barrier);")
        .unwrap();
    let yield_now = sleep_body
        .find("    tokio::task::yield_now().await;")
        .unwrap();
    assert!(drain < yield_now, "{source}");
    assert!(
        source.contains("let target_ms = smelt_mono_ms().saturating_add(delay_ms);"),
        "{source}"
    );
    assert!(
        source.contains("filter(|timer| timer.due_ms <= target_ms && timer.id < id_barrier)"),
        "{source}"
    );
}

#[test]
fn abort_signal_timeout_carries_the_timer_helpers_it_calls() {
    // `AbortSignal.timeout(ms)` arms a timer: it schedules its own abort on the
    // promise-task queue (`smelt_spawn_promise_task` + `smelt_sleep_ms`). Its
    // RESULT is a signal and mentions nothing async, so the timer prelude's
    // demand analysis — which keyed on `AsyncOp` rvalues — did not see it, and a
    // program whose only timer is a timeout signal emitted both calls against a
    // prelude that defined neither: E0425 twice, in a crate that otherwise
    // compiled. The program below is that isolate; it has no other async
    // surface at all.
    let source = source_for(
        r"
const timed = AbortSignal.timeout(5);
const aborted = timed.aborted;
",
    );

    assert!(
        source.contains("async fn smelt_sleep_ms(")
            && source.contains("fn smelt_spawn_promise_task("),
        "a timeout signal must bring the timer helpers it calls\n{source}"
    );
    assert!(
        source.contains("smelt_abort_timeout_reason"),
        "the scheduled abort must fire with the spec's TimeoutError reason\n{source}"
    );
}

#[test]
fn timer_typed_extra_args_use_wrapper_closure_not_erased_list() {
    // `setTimeout(callback, ms, ...args)` with a statically typed callback and
    // concretely typed extras must capture the extras in a synthesized
    // zero-argument wrapper closure and call the callback directly, producing no
    // erased `Vec<SmeltUnknown>` argument pack.
    let source = source_for(
        r#"
function greet(name: string, count: number): void {
  console.log(name);
  console.log(count);
}
setTimeout(greet, 10, "hi", 3);
"#,
    );

    // Extras keep their concrete Rust types in per-argument bindings; the erased
    // `smelt_timer_args: Vec<SmeltUnknown>` pack of the dynamic path is absent.
    assert!(source.contains("smelt_timer_arg_0"), "{source}");
    assert!(source.contains("smelt_timer_arg_1"), "{source}");
    assert!(
        source.contains("let smelt_timer_arg_0: JsString"),
        "typed extra should be a concrete String binding: {source}"
    );
    assert!(
        source.contains("let smelt_timer_arg_1: f64"),
        "typed extra should be a concrete f64 binding: {source}"
    );
    assert!(
        !source.contains("smelt_timer_args"),
        "typed timer extras must not pack into an erased Vec<SmeltUnknown>: {source}"
    );
    // The wrapper captures by value (move) and invokes the callback directly.
    assert!(source.contains("move ||"), "{source}");
    assert!(
        source.contains("(smelt_timer_callback)(smelt_timer_arg_0.clone(), smelt_timer_arg_1)"),
        "{source}"
    );
    assert!(source.contains("smelt_set_timeout("), "{source}");
}

#[test]
fn timer_optional_typed_extra_arg_uses_wrapper_closure() {
    // A single concretely typed extra (matching an optional callback parameter)
    // still routes through the typed wrapper and forwards exactly one argument.
    let source = source_for(
        r#"
function note(prefix: string, suffix?: string): void {
  console.log(prefix);
}
setTimeout(note, 10, "with-optional", "extra");
"#,
    );

    assert!(
        !source.contains("smelt_timer_args"),
        "typed optional timer extras must not pack into an erased Vec<SmeltUnknown>: {source}"
    );
    assert!(source.contains("smelt_timer_arg_0"), "{source}");
    assert!(source.contains("smelt_timer_arg_1"), "{source}");
}

#[test]
fn timer_untyped_extra_arg_keeps_erased_list_path() {
    // An `unknown`-typed extra is a genuine dynamic boundary, so the erased
    // `Vec<SmeltUnknown>` dispatch path must be preserved for it.
    let source = source_for(
        r"
function handle(value: unknown): void {}
function fire(payload: unknown): void {
  setTimeout(handle, 10, payload);
}
",
    );

    assert!(
        source.contains("smelt_timer_args"),
        "untyped timer extras must keep the erased Vec<SmeltUnknown> dispatch path: {source}"
    );
    assert!(
        !source.contains("smelt_timer_arg_0"),
        "untyped path must not synthesize per-argument typed bindings: {source}"
    );
}

#[test]
fn timer_unknown_callback_param_keeps_erased_list_path() {
    // A concretely typed extra whose callback parameter is `unknown` is still a
    // dynamic boundary: forwarding the `String` directly would drop it into a
    // `SmeltUnknown` parameter slot without the boundary conversion. The wrapper
    // path must be declined so the erased `Vec<SmeltUnknown>` ABI (which boxes
    // each argument) is used instead.
    let source = source_for(
        r#"
function handle(value: unknown): void {}
setTimeout(handle, 10, "x");
"#,
    );

    assert!(
        source.contains("smelt_timer_args"),
        "unknown callback parameters must keep the erased Vec<SmeltUnknown> dispatch path: {source}"
    );
    assert!(
        !source.contains("smelt_timer_arg_0"),
        "unknown callback parameter path must not synthesize typed per-argument bindings: {source}"
    );
}

/// A callback that returns a future, adapted to a target whose expected return
/// is a plain (non-future) value, must have its future result wrapped as a
/// `SmeltUnknown::Promise` rather than leaking a raw `Pin<Box<dyn Future>>`
/// into the value slot (E0308). Mirrors es-toolkit's `attempt(async () => 1)`,
/// where the synchronous `attempt` receives the returned promise as its value.
#[test]
fn async_callback_into_sync_slot_wraps_future_as_promise() {
    let source = source_for(
        r"
function attempt(fn: () => unknown): unknown {
  return fn();
}

export function run(): unknown {
  return attempt(async () => 1);
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Promise(SmeltPromise::from_future"),
        "an async callback flowing into a non-future value slot must be wrapped \
         as a promise\n{source}"
    );
}

#[test]
fn async_closure_await_propagates_future_errors() {
    let source = source_for(
        r"
async function load(): Promise<number> {
  return 1;
}

function retain(callback: () => Promise<number>): () => Promise<number> {
  return callback;
}

export function callback(): () => Promise<number> {
  return retain(async () => {
    const value = await load();
    return value;
  });
}
",
    );

    assert!(
        source.contains(": f64 = _smelt_tmp_0.await?;")
            || source.contains(": f64 = _smelt_tmp_1.await?;")
            || source.contains(": f64 = _smelt_tmp_2.await?;")
            || source.contains("value = _smelt_tmp_1.await?;"),
        "{source}"
    );
}

/// An async nested closure must carry the outer capture alias into its future
/// instead of reinitializing it from the shadowing inner parameter.
#[test]
fn async_nested_closure_preserves_shadowed_capture_alias() {
    let source = source_for(
        r"
export async function run(): Promise<number> {
  const objectize =
    (callback: (value: { num: number }) => Promise<number>) =>
    async (num: number) => callback({ num });
  return await objectize(async value => value.num)(1);
}
",
    );

    assert!(
        source.contains(
            "move |closure_arg_0: f64| { let smelt_captured_closure_arg_0 = smelt_captured_closure_arg_0.clone();"
        ),
        "{source}"
    );
}

/// A zero-parameter async closure returned from a factory remains reusable:
/// each call clones the factory callback before moving it into a fresh future.
#[test]
fn zero_parameter_async_factory_clones_captured_callback_per_call() {
    let source = source_for(
        r"
export async function run(): Promise<number> {
  const factory = (callback: (value: number) => Promise<number>) =>
    async () => callback(0);
  const callback = async (value: number): Promise<number> => value;
  const generated = factory(callback);
  return (await generated()) + (await generated());
}
",
    );

    assert!(
        source.contains(
            "move || { let closure_arg_0 = closure_arg_0.clone(); SmeltFuture::from_future"
        ),
        "{source}"
    );
}

/// A loop whose body contains a `try`/`catch` around an `await` must stay a loop.
///
/// Two independent gaps combined here. `block_exits_to_loop` — the guard
/// `while_header` uses to decide whether a body is loop-shaped — had no arm for
/// `Terminator::Await` and fell into its `_ => Ok(false)` catch-all, so the
/// header was never recognized. And the throwing-call/throwing-`await` emitters
/// hardcoded `emit_block` for their `Ok`/`Err` continuations, so even once
/// recognized the back edge reached from inside `match fut.await { .. }` was
/// treated as an ordinary goto and re-emitted the loop header inline. The two
/// together made this 12-line function expand to 437 lines with 14 `.await`s and
/// no `loop`, terminated only by the emitter's recursion-depth cap — which
/// silently caps the number of iterations the program can actually perform.
///
/// The load-bearing assertions are the counts, not the presence of `loop`: one
/// `.await` and one `catch_unwind` prove the body was emitted exactly once, which
/// is the difference between a loop and an unrolled prefix of one.
#[test]
fn awaiting_try_catch_in_a_loop_body_emits_one_loop_not_an_unrolled_body() {
    let source = source_for(
        r"
async function flaky(n: number): Promise<number> {
  if (n < 3) { throw new Error('nope'); }
  return n;
}
export async function attempt(limit: number): Promise<number> {
  for (let i = 0; i <= limit; i++) {
    try {
      return await flaky(i);
    } catch (err) {
      // keep going
    }
  }
  return -1;
}
",
    );

    let start = source.find("async fn attempt").expect("attempt present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("attempt closing brace");
    let body = &after[..end];

    assert_eq!(
        body.matches("loop {").count(),
        1,
        "the `for` header must render as a single Rust loop:\n{body}"
    );
    assert_eq!(
        body.matches(".await").count(),
        1,
        "the one source `await` must be emitted once; more copies mean the loop \
         body was unrolled into the back edge:\n{body}"
    );
    assert_eq!(
        body.matches("catch_unwind").count(),
        1,
        "the one source `try` must be emitted once:\n{body}"
    );
    assert!(
        body.contains("continue;"),
        "the back edge out of the `catch` arm must render as `continue`:\n{body}"
    );
    assert!(
        !body.contains("could not structurally emit"),
        "the region must be structured, not surrendered to the recursion cap:\n{body}"
    );
}

/// An awaited rejection inside a closure `try` runs the `catch`.
///
/// The closure emitter's `Await` arm dropped `unwind` exactly like its `Call`
/// arm, so `try { await f() } catch` inside a closure propagated the rejection
/// with `?` instead of running the handler. The awaited call itself is
/// deliberately left on the statement path by lowering — an async callee hands
/// back a future and the rejection surfaces at the `await`, whose terminator
/// already carries the handler — so `.await` appearing exactly once inside a
/// `match` is what proves the single handler landed in the right place.
#[test]
fn an_awaited_rejection_inside_a_closure_try_runs_the_catch() {
    let source = source_for(
        r"
async function flaky(x: number): Promise<number> {
  if (x < 3) { throw new Error('nope'); }
  return x;
}
export function outer(n: number): number {
  const inner = async (limit: number): Promise<number> => {
    for (let i = 0; i <= limit; i++) {
      try { return await flaky(i); } catch (err) { }
    }
    return -1;
  };
  inner(n);
  return 0;
}
",
    );

    assert!(
        source.contains("match _smelt_tmp_6.await {") || source.contains(".await {"),
        "the awaited rejection must be matched, not propagated with `?`:\n{source}"
    );
    assert!(
        !source.contains(".await?;\n    return Ok::<f64"),
        "the awaited value inside a `try` must not be propagated with `?`:\n{source}"
    );
}

/// A borrowed callback parameter renders its return through the canonical
/// function-value logic.
///
/// `param_type` re-derived the return type instead of sharing the
/// `Type::Function` arm's refinements, so the Rust type of a borrowed callback
/// could disagree with the Rust type of the very value bound to it. Both now
/// call `function_value_return_type`: a `Future` return is the promise
/// value `SmeltFuture<T>` (an async throw is a rejected future, so no outer
/// `Result` is added), and `may_throw` otherwise wraps the return in
/// `Result<T, Box<dyn std::error::Error>>`.
#[test]
fn a_borrowed_callback_parameter_renders_a_future_return_as_a_promise_value() {
    let source = source_for(
        r"
export function fireAndForget(cb: () => Promise<string>): void {
  cb();
}
",
    );

    assert!(
        source.contains("fn fire_and_forget(cb: &dyn Fn() -> SmeltFuture<JsString>)"),
        "a borrowed callback's future return is the promise value:\n{source}"
    );
    assert!(
        !source.contains("Result<SmeltFuture<JsString>"),
        "an async throw is a rejected future, not an outer Result:\n{source}"
    );
}

/// Calling an async function VALUE that can throw renders no `?`.
///
/// An async function reports even a synchronous throw as a REJECTED future, so
/// its emitted Rust return type is `SmeltFuture<T>` with no `Result` wrapper
/// (see `function_value_return_type`). A call site that read `may_throw`
/// directly emitted `(boom)()?` on a `SmeltFuture<f64>`, which needs `Try`
/// (E0277).
#[test]
fn calling_a_throwing_async_closure_value_is_not_fallible() {
    let source = source_for(
        r"
export async function run(): Promise<number> {
  const boom = async (): Promise<number> => {
    throw new Error('boom');
  };
  const pending = boom();
  return await pending;
}
",
    );

    for line in source.lines() {
        assert!(
            !line.contains("(boom)()?"),
            "an async callee answers a future, so its call is infallible:\n{line}"
        );
    }
}
