//! Generated Rust for emission of the vitest test harness.
//!
//! What matchers decide is checked against vitest itself in the
//! `assertion-parity` fixtures; these tests pin what only the emitted crate shows.

use super::*;

/// The expected literal is typed from the actual, so both compare as tuple lists.
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

    let program = program_body(&source);
    assert!(
        program.contains("SmeltList<(f64, JsString)>"),
        "the rows must stay tuples\n{program}"
    );
    assert!(
        !program.contains("SmeltList<SmeltList<SmeltUnknown>>"),
        "the expected literal must be typed from the actual, not erased\n{program}"
    );
}

/// The mock registry is emitted for a crate that mocks, and only for one.
#[test]
fn vitest_mock_prelude_is_pay_for_use() {
    let plain = source_for(
        r"
export function double(value: number): number {
  return value * 2;
}
",
    );
    assert!(!plain.contains("SmeltVitestMockState"), "{plain}");
    assert!(!plain.contains("fn rejected("), "{plain}");

    let mocking = source_for(
        r#"
import { it, vi } from "vitest";

it("configures a chain", () => {
  const func = vi.fn().mockRejectedValueOnce(new Error("failure"));
  func();
});
"#,
    );
    assert!(mocking.contains("struct SmeltVitestMockState"), "{mocking}");
    assert!(
        mocking.contains("fn rejected(value: SmeltUnknown)"),
        "{mocking}"
    );
}
