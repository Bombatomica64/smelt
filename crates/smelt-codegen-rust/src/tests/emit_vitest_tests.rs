//! Generated Rust for emission of the vitest test harness.

use super::*;

#[test]
fn keeps_vitest_assertions_inside_catch_blocks() {
    let source = source_for(
        r#"
import { expect, test } from "vitest";

function fail(): never {
  throw new RangeError("bad");
}

test("catch assertion", () => {
  try {
    fail();
  } catch (e) {
    expect(e instanceof RangeError).toBe(true);
  }
});
"#,
    );

    let err_arm = source
        .find("Err(__smelt_error)")
        .expect("generated try/catch should have an Err arm");
    let assertion = source
        .find("expect(...).toBe(...) failed")
        .expect("catch assertion should lower to a test failure branch");
    assert!(
        assertion > err_arm,
        "catch assertion escaped before the catch binding:\n{source}"
    );
    assert!(source.contains("\"__smelt_error\".to_owned()"), "{source}");
    assert!(source.contains("\"message\".to_owned()"), "{source}");
}

/// `expect(actual).toEqual(literal)` contextually types the literal from the
/// actual value's type, so a nested tuple-list actual and a nested array
/// literal compare at the same Rust type instead of
/// `SmeltList<SmeltList<SmeltUnknown>>` vs `SmeltList<(f64, String)>` (E0308).
#[test]
fn to_equal_contextually_types_expected_from_actual() {
    let source = source_for(
        r"
import { describe, it, expect } from 'vitest';

describe('toEqual', () => {
  it('compares nested tuple lists', () => {
    const actual: Array<[number, string]> = [[1, 'a'], [2, 'b']];
    expect(actual).toEqual([[1, 'a'], [2, 'b']]);
  });
});
",
    );

    // The expected literal's rows lower as (f64, String) tuples, matching the
    // actual, rather than erased inner lists.
    assert!(
        !source.contains("SmeltList<SmeltList<SmeltUnknown>>"),
        "the expected literal must be typed from the actual, not erased\n{source}"
    );
}

#[test]
fn vitest_mock_chain_constructs_one_stateful_mock() {
    // A `vi.fn().mockRejectedValueOnce(..).mockResolvedValue(..)` chain must
    // construct exactly ONE runtime mock: the chain methods are runtime fields
    // returning the same instance, and HIR interceptor probing's dangling
    // duplicate exprs must never be materialized by MIR.
    let source = source_for(
        r#"
import { it, vi } from "vitest";

it("configures a chain", () => {
  const func = vi
    .fn()
    .mockRejectedValueOnce(new Error("failure"))
    .mockResolvedValue("success");
  func();
});
"#,
    );
    // Count only the emitted PROGRAM, not the prelude: the prelude both defines
    // the helper and calls it from the spy adapter, so a whole-source count
    // moves whenever the runtime grows another caller.
    let program = source
        .split_once("@smelt:prelude-end")
        .map_or(source.as_str(), |(_, program)| program);
    assert_eq!(
        program.matches("smelt_vitest_mock_new(").count(),
        1,
        "chain must construct exactly one mock\n{source}"
    );
    // The gated mock prelude and its `SmeltPromise::rejected` dependency are
    // both emitted for mock-bearing crates.
    assert!(source.contains("struct SmeltVitestMockState"), "{source}");
    assert!(source.contains("fn rejected(value: SmeltUnknown)"), "{source}");
}

#[test]
fn vitest_mock_prelude_is_pay_for_use() {
    // A crate with no `vi.fn()` mock keeps a byte-identical prelude: no mock
    // registry, no `SmeltPromise::rejected`.
    let source = source_for(
        r"
export function double(value: number): number {
  return value * 2;
}
",
    );
    assert!(!source.contains("SmeltVitestMockState"), "{source}");
    assert!(!source.contains("fn rejected("), "{source}");
}

#[test]
fn vitest_called_times_and_called_with_are_real_assertions() {
    // `toHaveBeenCalledTimes` / `toHaveBeenCalledWith` must lower to real
    // failure paths reading the mock's recorded state — a count/argument
    // mismatch returns a test error instead of passing vacuously.
    let source = source_for(
        r#"
import { expect, it, vi } from "vitest";

it("asserts calls", () => {
  const spy = vi.fn();
  spy(1, "a");
  expect(spy).toHaveBeenCalledTimes(1);
  expect(spy).toHaveBeenCalledWith(1, "a");
});
"#,
    );
    assert!(
        source.contains("smelt_vitest_mock_called_times("),
        "{source}"
    );
    assert!(
        source.contains("smelt_vitest_mock_called_with("),
        "{source}"
    );
    assert!(
        source.contains("expect(...).toHaveBeenCalledTimes(...) failed"),
        "count mismatch must produce a test failure\n{source}"
    );
    assert!(
        source.contains("expect(...).toHaveBeenCalledWith(...) failed"),
        "argument mismatch must produce a test failure\n{source}"
    );
}

#[test]
fn a_call_assertion_about_a_non_mock_actual_is_false() {
    // The mock matchers used to answer `true` for an actual carrying no
    // `__smelt_vitest_mock` marker, back when `vi.spyOn` lowered to a plain
    // placeholder and failing it would have failed suites Smelt could not model.
    // A spy is a real mock now, so the honest answer is the one JavaScript
    // implies: a value that is not a mock has recorded no calls, so it was not
    // called with anything and its last result resolved to nothing. Only
    // `toHaveBeenCalledTimes(0)` still holds.
    let source = source_for(
        r#"
import { expect, it, vi } from "vitest";

it("asserts calls", () => {
  const spy = vi.fn();
  spy(1);
  expect(spy).toHaveBeenCalledTimes(1);
});
"#,
    );
    assert!(
        source.contains(
            "fn smelt_vitest_mock_called_times(value: &SmeltUnknown, expected: f64) -> bool { match smelt_vitest_mock_state(value) { Some(state) => state.borrow().calls.len() as f64 == expected, None => expected == 0.0 } }"
        ),
        "a non-mock has zero recorded calls: {source}"
    );
    assert!(
        !source.contains("state.calls.iter().any(call_matches) } }, None => true } }"),
        "a non-mock must not satisfy toHaveBeenCalledWith: {source}"
    );
    assert!(
        source.contains("state.calls.iter().any(call_matches) } }, None => false } }"),
        "a non-mock must not satisfy toHaveBeenCalledWith: {source}"
    );
}

#[test]
fn vitest_last_called_with_lowers_to_last_call_check() {
    // `toHaveBeenLastCalledWith` compares only the most recent recorded call,
    // so it lowers to `smelt_vitest_mock_called_with(.., last=true)`.
    let source = source_for(
        r#"
import { expect, it, vi } from "vitest";

it("asserts last call", () => {
  const spy = vi.fn();
  spy(1);
  spy(2);
  expect(spy).toHaveBeenLastCalledWith(2);
});
"#,
    );
    assert!(
        source.contains("smelt_vitest_mock_called_with(") && source.contains(", true)"),
        "last-call matcher must pass last=true\n{source}"
    );
    assert!(
        source.contains("expect(...).toHaveBeenLastCalledWith(...) failed"),
        "last-call mismatch must produce a test failure\n{source}"
    );
}

#[test]
fn vitest_last_resolved_with_lowers_to_result_check() {
    // `toHaveLastResolvedWith` reads the mock's recorded return value, flattening
    // a resolved promise before deep-equality.
    let source = source_for(
        r#"
import { expect, it, vi } from "vitest";

it("asserts resolved", async () => {
  const spy = vi.fn(async () => 5);
  await spy();
  expect(spy).toHaveLastResolvedWith(5);
});
"#,
    );
    assert!(
        source.contains("smelt_vitest_mock_last_resolved_with("),
        "resolved matcher must lower to the runtime helper\n{source}"
    );
    assert!(
        source.contains("expect(...).toHaveLastResolvedWith(...) failed"),
        "resolved mismatch must produce a test failure\n{source}"
    );
}

#[test]
fn vitest_mock_dot_calls_synthesizes_recorded_activity() {
    // `mockFn.mock.calls` is not an own field of the erased mock object; reading
    // `.mock` must synthesize the recorded activity from the live registry state
    // so `mockFn.mock.calls.length` flows through the ordinary array-length path.
    let source = source_for(
        r#"
import { expect, it, vi } from "vitest";

it("reads mock.calls", () => {
  const spy = vi.fn();
  spy();
  expect(spy.mock.calls.length).toBe(1);
});
"#,
    );
    assert!(
        source.contains("if field == \"mock\""),
        "`.mock` accessor must be synthesized in smelt_get_object_field\n{source}"
    );
}

#[test]
fn a_deep_equality_matcher_on_a_class_compares_structurally() {
    // Vitest `toEqual` compares own enumerable properties; only `toBe` is
    // identity. A class with reference semantics gets `PartialEq = Rc::ptr_eq`,
    // so lowering `toEqual` to a plain `!=` on the class type asked "is this the
    // same object" — unsatisfiable for any freshly built value, which is why
    // `expect(clone(err)).toEqual(err)` could never pass. Erasing both operands
    // makes the comparison `SmeltUnknown`'s structural walk.
    let source = source_for(
        "import { describe, expect, it } from 'vitest';\n\
         class Point { x: number; constructor(x: number) { this.x = x; } move(): Point { return new Point(this.x); } }\n\
         describe('p', () => { it('eq', () => { const a = new Point(1); expect(a.move()).toEqual(a); }); });",
    );
    assert!(
        source.contains("into_smelt_unknown()"),
        "a class-typed deep-equality comparison must erase both operands:\n{source}"
    );
    assert!(
        !source.contains("a.clone() != a.clone()"),
        "a class-typed deep-equality comparison must not compare by identity:\n{source}"
    );
}
