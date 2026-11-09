//! Generated Rust for async functions, promises and timers.
//!
//! What these programs compute is in the `async_forms` Node-parity fixture and
//! the `assertion-parity` matchers. These tests pin what running cannot show:
//! concrete types through `await` and generators, the typed timer path, and a
//! loop around `await` emitted once.

use super::*;

/// `await` on `Cell | Promise<Cell>` is a `Cell`, not an erased value.
#[test]
fn awaits_a_value_or_promise_union_to_its_joined_type() {
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

/// Async generators keep the same typed yield and completion as synchronous ones.
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
    assert!(source.contains("SmeltFuture<SmeltGeneratorResult<f64, JsString>>"), "{source}");
}

/// A method shared by every arm of a concrete union dispatches through the
/// generated tagged enum, keeping a generator-valued generic return.
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

/// Each generated test starts with an empty virtual timer queue.
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

    assert!(source.contains("    smelt_reset_timers();"), "{source}");
}

/// Typed timer extras stay typed: one binding each, called directly.
#[test]
fn timer_typed_extra_args_use_wrapper_closure_not_erased_list() {
    let source = source_for(
        r#"
function greet(name: string, count: number): void {
  console.log(name);
  console.log(count);
}
setTimeout(greet, 10, "hi", 3);
"#,
    );

    assert!(
        source.contains("let smelt_timer_arg_0: JsString"),
        "typed extra should be a concrete String binding: {source}"
    );
    assert!(
        source.contains("let smelt_timer_arg_1: f64"),
        "typed extra should be a concrete f64 binding: {source}"
    );
    assert!(
        source.contains("(smelt_timer_callback)(smelt_timer_arg_0.clone(), smelt_timer_arg_1)"),
        "{source}"
    );
    assert!(
        !source.contains("smelt_timer_args"),
        "typed timer extras must not pack into an erased Vec<SmeltUnknown>: {source}"
    );
}

/// An extra matching an optional callback parameter takes the typed path too.
#[test]
fn timer_optional_typed_extra_arg_uses_wrapper_closure() {
    let source = source_for(
        r#"
function note(prefix: string, suffix?: string): void {
  console.log(prefix);
}
setTimeout(note, 10, "with-optional", "extra");
"#,
    );

    assert!(source.contains("smelt_timer_arg_0"), "{source}");
    assert!(source.contains("smelt_timer_arg_1"), "{source}");
    assert!(
        !source.contains("smelt_timer_args"),
        "typed optional timer extras must not pack into an erased Vec<SmeltUnknown>: {source}"
    );
}

/// An `unknown`-typed extra is a real dynamic boundary and keeps the erased pack.
#[test]
fn timer_untyped_extra_arg_keeps_erased_list_path() {
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

/// A concrete extra into an `unknown` callback parameter crosses the boundary
/// through the erased pack, which boxes each argument.
#[test]
fn timer_unknown_callback_param_keeps_erased_list_path() {
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

/// A loop around `try { await }` is one Rust loop: the counts distinguish it from
/// an unrolled prefix of a loop, which would compute the same answer for a small
/// limit.
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

    let body = rust_fn(&source, "attempt");
    assert_eq!(body.matches("loop {").count(), 1, "{body}");
    assert_eq!(body.matches(".await").count(), 1, "{body}");
    assert_eq!(body.matches("catch_unwind").count(), 1, "{body}");
    assert!(body.contains("continue;"), "{body}");
}

/// A borrowed callback's future return is the promise value, with no outer `Result`.
#[test]
fn a_borrowed_callback_parameter_renders_a_future_return_as_a_promise_value() {
    let source = source_for(
        r"
export function fireAndForget(cb: () => Promise<string>): void {
  cb();
}
",
    );

    assert_eq!(rust_fn(&source, "fire_and_forget"), r#"fn fire_and_forget(cb: &dyn Fn() -> SmeltFuture<JsString>) -> () {
let _smelt_tmp_1: SmeltFuture<JsString> = cb();
let _smelt_tmp_2: () = { smelt_spawn_promise_task(Box::pin(async move { let _ = _smelt_tmp_1.await; })); () };
return;
}"#);
}
