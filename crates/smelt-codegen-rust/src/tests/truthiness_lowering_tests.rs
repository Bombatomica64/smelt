//! Emitted-text tests for JavaScript truthiness, non-null assertions and
//! `await` over a value whose type is not a future.
//!
//! Each of these rules used to produce Rust that compiled cleanly and computed
//! the wrong value, so what has to be pinned is the *shape* of the emitted
//! code: the truthiness `match`, the absence of a `typeof`-style tag check in
//! boolean position, the absence of a folded `if true`, the absence of an
//! `.expect(...)` narrowing, and the presence of the awaited operand. The
//! runtime tier `tests/truthiness_and_await_runtime.rs` proves the values;
//! these tests are the cheap guard that runs on every `cargo test`.

use super::{emitted_function_body, source_for};
use crate::PRELUDE_END_MARKER;

/// Returns only the generated program, dropping the runtime prelude.
///
/// The prelude carries every `SmeltUnknown` pattern the runtime needs, so an
/// assertion over the whole emitted file would match its text rather than the
/// lowered program's.
fn program_of(source: &str) -> &str {
    source
        .split_once(PRELUDE_END_MARKER)
        .map_or(source, |(_, program)| program)
}

/// Fragment of the JS truthiness `match` the `ToBool` primitive cast emits.
const TRUTHINESS_MATCH: &str = "SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan()";

/// The tag check that must never stand in for a truthiness test.
const BOOL_TAG_CHECK: &str = "SmeltUnknown::Bool(_)";

#[test]
fn a_generic_condition_is_tested_rather_than_folded_to_true() {
    // `item: T` is unconstrained, so it can hold `0`, `""`, `false`, `NaN` or a
    // nullish value; folding the guard to `true` made the function an identity.
    let source = source_for(
        r"
export function compact<T>(items: T[]): T[] {
  const out: T[] = [];
  for (const item of items) {
    if (item) {
      out.push(item);
    }
  }
  return out;
}
",
    );
    let program = program_of(&source);
    assert!(
        program.contains(TRUTHINESS_MATCH),
        "an unconstrained type parameter in boolean position must emit the JS truthiness match:\n{source}"
    );
    assert!(
        !program.contains("if true {"),
        "a generic condition must not fold to a constant:\n{source}"
    );
}

#[test]
fn a_function_typed_condition_still_folds_to_true() {
    // The narrowing must not go too far: a function value is always truthy in
    // JavaScript, so its guard stays folded and no runtime test is emitted.
    let source = source_for(
        r"
export function describeHandler(handler: () => void): string {
  if (handler) {
    return 'yes';
  }
  return 'no';
}
",
    );
    let body = emitted_function_body(&source, "fn describe_handler");
    assert!(
        !body.contains(TRUTHINESS_MATCH),
        "a function value is always truthy and needs no runtime test:\n{body}"
    );
    // The fold is constant, so MIR optimization drops the false arm entirely.
    assert!(
        !body.contains("\"no\""),
        "a function-typed condition stays folded to `true`, dropping the false arm:\n{body}"
    );
}

#[test]
fn negating_an_erased_callback_value_emits_truthiness_not_a_tag_check() {
    // `row.enabled` reads off a `Record<string, unknown>`, so the operand is
    // erased. `matches!(x, SmeltUnknown::Bool(_))` answers `true` for a stored
    // `false`, i.e. the inverse of `!row.enabled` on the value that matters.
    let source = source_for(
        r"
export function firstDisabled(rows: Record<string, unknown>[]): number {
  return rows.findIndex(row => !row.enabled);
}
",
    );
    let program = program_of(&source);
    assert!(
        program.contains(TRUTHINESS_MATCH),
        "an erased operand in a callback's boolean position must emit the truthiness match:\n{source}"
    );
    assert!(
        !program.contains(BOOL_TAG_CHECK),
        "a truthiness test must not lower to a `typeof === \"boolean\"` tag check:\n{source}"
    );
}

#[test]
fn a_typeof_boolean_comparison_still_emits_a_tag_check() {
    // The tag check must stay reachable from the source spelling that means it.
    let source = source_for(
        r"
export function isBool(value: unknown): boolean {
  return typeof value === 'boolean';
}
",
    );
    let program = program_of(&source);
    assert!(
        program.contains(BOOL_TAG_CHECK),
        "`typeof x === 'boolean'` must still emit the boolean tag check:\n{source}"
    );
}

#[test]
fn a_non_null_assertion_into_an_optional_parameter_forwards_the_value() {
    // `maximum!` is type-level only. The callee's parameter is optional and
    // handles the absent case, so narrowing here would render `.expect(...)` on
    // a value that is legitimately absent. Only the *caller* is asserted on:
    // `span`'s own body narrows `maximum` after a real nullish guard, where the
    // same `.expect(...)` is provably safe and must stay.
    let source = source_for(
        r"
function span(minimum: number, maximum?: number): number {
  if (maximum == null) {
    return minimum;
  }
  return maximum - minimum;
}

export function spanFrom(minimum: number, maximum?: number): number {
  return span(minimum, maximum!);
}
",
    );
    let caller = emitted_function_body(&source, "fn span_from");
    assert!(
        !caller.contains("optional value was absent after narrowing"),
        "a non-null assertion into an optional parameter must not narrow:\n{caller}"
    );
    assert!(
        !caller.contains(".expect("),
        "the asserted argument must be forwarded as-is:\n{caller}"
    );
}

#[test]
fn awaiting_an_erased_value_keeps_the_operand() {
    // The awaited operand is an erased tuple element, which may hold a promise
    // at runtime. Replacing the `await` with `null` deleted the computation.
    let source = source_for(
        r"
function attempt(func: () => unknown): [unknown, unknown] {
  return [null, func()];
}

export async function run(): Promise<unknown> {
  const [, result] = attempt(async () => 1);
  return await result;
}
",
    );
    let program = program_of(&source);
    assert!(
        program.contains("smelt_await_flatten"),
        "awaiting an erased value must drive the runtime promise chain:\n{source}"
    );
}
