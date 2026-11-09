//! Generated Rust for throw and catch.

use super::*;

#[test]
fn adapts_throwing_callback_to_non_throwing_function_field() {
    let source = source_for(
        r#"
import { expect } from "vitest";

type Locale = {
  formatDistance: (token: string) => string;
};

const localizeDistance = (token: string) => {
  expect(token).toBe("x");
  return "ok";
};

const locale: Locale = { formatDistance: localizeDistance };
"#,
    );

    assert!(
        source.contains("unwrap_or_else(|error| smelt_panic_throw(error))"),
        "{source}"
    );
    assert!(
        source.contains("formatDistance") || source.contains("format_distance"),
        "{source}"
    );
}

#[test]
fn emits_optional_nullish_coalescing_with_erased_fallback_without_panicking() {
    let source = source_for(
        r"
declare const defaults: Record<string, unknown>;
interface Options {
  weekStartsOn?: number;
}
function read(options?: Options): number {
  return options?.weekStartsOn ?? defaults.weekStartsOn ?? 0;
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => None, smelt_value => Some("),
        "{source}"
    );
    assert!(source.contains(".or(match"), "{source}");
}

#[test]
fn emits_caught_throw_without_result_signature() {
    let source = source_for(
        "try {
  throw \"boom\";
} catch (err: string) {
  console.log(err);
}
",
    );

    assert!(source.contains("fn main() {"));
    assert!(!source.contains("Box<dyn std::error::Error>"));
    assert!(source.contains("err = JsString::from(\"boom\");"));
}

#[test]
fn falls_back_to_closure_body_for_try_catch_callback() {
    // A `.map` callback whose body uses a statement form the side-effect-free
    // callback IR cannot represent (here `try`/`catch`) must retry through full
    // closure-body lowering instead of failing the whole file. `source_for`
    // panics on any blocker, so reaching codegen proves the fallback fired.
    let source = source_for(
        r"
function run(values: string[]): Array<string | undefined> {
  return values.map((value) => {
    try {
      return value;
    } catch (e) {
      return undefined;
    }
  });
}
",
    );

    assert!(source.contains(".map("), "{source}");
}

/// The same shape without `await` must keep its `catch` handler.
///
/// The loop-body emitters matched `Terminator::Call { unwind: _, .. }` and threw
/// the exception handler away, emitting the call through the plain `?` template.
/// That deleted the `catch` block outright: a synchronous
/// `for (..) { try { return f(i); } catch {} }` propagated the first error out of
/// the enclosing function instead of retrying, and in a non-throwing function the
/// stray `?` did not even compile. Asserting on `catch_unwind` (not on `loop`) is
/// what pins the handler down — the loop was already emitted before the fix.
#[test]
fn a_try_catch_in_a_loop_body_keeps_its_handler() {
    let source = source_for(
        r"
function flaky(n: number): number {
  if (n < 3) { throw new Error('nope'); }
  return n;
}
export function attempt(limit: number): number {
  for (let i = 0; i <= limit; i++) {
    try {
      return flaky(i);
    } catch (err) {
      // keep going
    }
  }
  return -1;
}
console.log(attempt(5));
",
    );

    let start = source.find("fn attempt").expect("attempt present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("attempt closing brace");
    let body = &after[..end];

    assert_eq!(
        body.matches("loop {").count(),
        1,
        "the `for` header must render as a single Rust loop:\n{body}"
    );
    assert!(
        body.contains("catch_unwind"),
        "the `catch` handler must be emitted inside the loop body:\n{body}"
    );
    assert!(
        !body.contains("flaky(i.clone())?"),
        "the throwing call must not be emitted through the error-propagating `?` \
         template, which discards the `catch` arm:\n{body}"
    );
    assert!(
        body.contains("continue;"),
        "the back edge out of the `catch` arm must render as `continue`:\n{body}"
    );
}

/// A `try`/`catch` inside a closure body must keep its handler.
///
/// Closure bodies are rendered by their own recursive emitter,
/// `emit_closure_block_inner`, and its `Call`/`Await` arms matched
/// `unwind: _` — throwing the exception handler away. The call then went through
/// `closure_call_expr_for_dest`, whose single difference from the function-level
/// path is that it rewrites a trailing `?` into
/// `unwrap_or_else(|error| smelt_panic_throw(error))`. So a nested function whose body wrapped
/// a throwing call in `try`/`catch` did not run its `catch` at all: it aborted
/// the process on the first throw.
///
/// The load-bearing assertion is the absence of the panicking unwrap together
/// with the presence of `catch_unwind`. Asserting only on `loop {` would prove
/// nothing here — the closure emitter already recognized this loop before the
/// fix and still produced a program that panicked.
#[test]
fn a_try_catch_inside_a_closure_body_keeps_its_handler() {
    let source = source_for(
        r"
function flaky(x: number): number {
  if (x < 3) { throw new Error('nope'); }
  return x;
}
export function outer(n: number): number {
  const inner = (limit: number): number => {
    for (let i = 0; i <= limit; i++) {
      try { return flaky(i); } catch (err) { }
    }
    return -1;
  };
  return inner(n);
}
console.log(outer(5));
",
    );

    let start = source.find("fn outer").expect("outer present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("outer closing brace");
    let body = &after[..end];

    assert_eq!(
        body.matches("loop {").count(),
        1,
        "the closure body's `for` must still render as a single loop:\n{body}"
    );
    assert!(
        body.contains("catch_unwind"),
        "the `catch` handler must be emitted inside the closure body:\n{body}"
    );
    assert!(
        !body.contains("unwrap_or_else(|error| smelt_panic_throw"),
        "a caught throwing call must not be emitted as a panicking unwrap, which \
         is the handler being discarded:\n{body}"
    );
}

/// A throwing call inside a *fallible* closure body keeps its `?`.
///
/// Adapting a throwing function into a callback slot wraps it in a closure whose
/// Rust signature is `-> Result<T, Box<dyn std::error::Error>>`. The call inside
/// that body was still rewritten from `?` to
/// `unwrap_or_else(|error| smelt_panic_throw(error))`, which turned a recoverable JavaScript
/// exception into an abort even though the enclosing signature could carry it.
/// The `panic!` belongs only where the surrounding Rust signature genuinely
/// cannot carry an error.
#[test]
fn a_throwing_call_in_a_fallible_closure_body_propagates_with_question_mark() {
    let source = source_for(
        r"
function thrower(x: number): string {
  if (x > 0) { throw new Error('boom'); }
  return 'ok';
}
export function guard(cb: (x: number) => string, v: number): string {
  try {
    return cb(v);
  } catch {
    return 'caught';
  }
}
export function useIt(): string {
  return guard(thrower, 1);
}
",
    );

    assert!(
        source.contains("thrower(closure_arg_0)?"),
        "the fallible wrapper closure must propagate the throw:\n{source}"
    );
    assert!(
        !source.contains("thrower(closure_arg_0.clone()).unwrap_or_else"),
        "a fallible closure body must not abort on a recoverable throw:\n{source}"
    );
}

/// A `try`/`catch` inside an `if` arm ends the arm; it does not `continue`.
///
/// `emit_block_until_goto` serves two structurally different regions: the body
/// of a generated `loop`, where the stop block is the loop header and an edge to
/// it is the back edge, and a forward `if`/`else` arm, where the stop block is
/// the join the caller emits once after the region. Its throwing-call and
/// throwing-`await` forks hardcoded `Continuation::InLoop { continue_target:
/// stop, .. }` for both, so a `try`/`catch` in an `if` arm rendered the edge to
/// the join as `continue;` with no enclosing loop — rustc E0268 (`continue`
/// outside of a loop), the radash `async.ts` `guard` blocker. `RegionExit` now
/// tells the two shapes apart at every call site.
///
/// The load-bearing assertions are the absence of `continue`/`break` together
/// with the presence of the `catch_unwind` fork: dropping the fork would also
/// remove the `continue`, and would silently delete the `catch` clause.
#[test]
fn a_try_catch_inside_an_if_arm_does_not_emit_continue_outside_a_loop() {
    let source = source_for(
        r"
export function guarded(func: () => any, recover: (err: any) => any): any {
  const isPromise = (result: any): result is Promise<any> =>
    result instanceof Promise;
  try {
    const result = func();
    return isPromise(result) ? result.catch(recover) : result;
  } catch (err) {
    return recover(err);
  }
}
",
    );

    let start = source.find("fn guarded").expect("guarded present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("guarded closing brace");
    let body = &after[..end];

    assert!(
        !body.contains("loop {") && !body.contains("while "),
        "the source has no loop, so the emitted body must have none either:\n{body}"
    );
    assert!(
        !body.contains("continue;"),
        "an edge to a forward join is not a back edge; `continue` here is \
         E0268 (`continue` outside of a loop):\n{body}"
    );
    assert!(
        !body.contains("break;"),
        "there is no loop to break out of either:\n{body}"
    );
    assert!(
        body.contains("catch_unwind"),
        "the `try`/`catch` fork must still be emitted, not dropped along with \
         the bogus back edge:\n{body}"
    );
}

#[test]
fn an_element_read_never_panics_on_a_negative_normalized_index() {
    // A positive out-of-range index already produced the JavaScript answer
    // (`Vec::get` misses and the emitter substitutes the missing value), but a
    // still-negative normalized index went through
    // `usize::try_from(normalized).expect("negative index out of bounds")` and
    // aborted the program. `[][-1]` is `undefined` in JavaScript, so both
    // directions of out-of-range must agree. `usize::MAX` is never a live slot
    // of a `Vec`, so converting the miss to it makes the following `get` miss.
    let source = source_for(
        r"
export function pick(arr: number[], index: number): number {
  return arr[index];
}
",
    );

    assert!(
        !source.contains("negative index out of bounds"),
        "an element READ must not panic on an out-of-range index:\n{source}"
    );
    assert!(
        source.contains("usize::try_from(smelt_normalized).unwrap_or(usize::MAX)"),
        "the normalized index must degrade to a miss:\n{source}"
    );
}
