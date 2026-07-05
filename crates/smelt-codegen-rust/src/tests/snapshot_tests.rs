//! Selective whole-module snapshots for stable representative emitter cases.

use super::*;

/// Snapshot complete compact modules, or the complete case-specific module tail
/// after replacing a large shared runtime prelude with an explicit marker.
///
/// Shared with sibling whole-output snapshot modules (`snapshot_tests_*`) so the
/// emitted module is always the asserted surface rather than a guessed fragment.
pub(super) fn assert_emitted_source_snapshot(name: &str, source: &str, case_start: Option<&str>) {
    let emitted = source_for(source);
    assert_no_legacy_callback_fallbacks(&emitted);
    let emitted = match case_start {
        Some(case_start) => {
            let start = emitted
                .find(case_start)
                .unwrap_or_else(|| panic!("missing case-specific module start `{case_start}`"));
            format!(
                "{}\n\n// ... shared generated runtime prelude omitted from selective snapshot ...\n\n{}",
                emitted.lines().take(2).collect::<Vec<_>>().join("\n"),
                &emitted[start..]
            )
        }
        // No case marker: snapshot the whole emitted module as-is.
        None => emitted,
    };
    assert!(
        emitted.lines().count() <= 450,
        "selective emitter snapshot grew beyond 450 lines"
    );
    insta::assert_snapshot!(name, emitted);
}

/// Snapshot a complete emitted module compiled from Python source, applying the
/// same line-budget guard and case-specific prelude elision as the TypeScript
/// helper so Python-frontend coverage stays whole-output too.
pub(super) fn assert_emitted_py_source_snapshot(
    name: &str,
    source: &str,
    case_start: Option<&str>,
) {
    let emitted = source_for_py(source);
    assert_no_legacy_callback_fallbacks(&emitted);
    let emitted = match case_start {
        Some(case_start) => {
            let start = emitted
                .find(case_start)
                .unwrap_or_else(|| panic!("missing case-specific module start `{case_start}`"));
            format!(
                "{}\n\n// ... shared generated runtime prelude omitted from selective snapshot ...\n\n{}",
                emitted.lines().take(2).collect::<Vec<_>>().join("\n"),
                &emitted[start..]
            )
        }
        // No case marker: snapshot the whole emitted module as-is.
        None => emitted,
    };
    assert!(
        emitted.lines().count() <= 450,
        "selective emitter snapshot grew beyond 450 lines"
    );
    insta::assert_snapshot!(name, emitted);
}

/// Assert callback CFG emission does not fall back to legacy placeholder text.
fn assert_no_legacy_callback_fallbacks(emitted: &str) {
    assert!(
        !emitted.contains("callback_body"),
        "generated Rust contains legacy callback_body fallback text"
    );
    assert!(
        !emitted.contains("callback-body"),
        "generated Rust contains legacy callback-body fallback text"
    );
}

/// Snapshot basic function and direct-call emission.
#[test]
fn basic_function_emission() {
    assert_emitted_source_snapshot(
        "basic_function_emission",
        r"
function add(left: number, right: number): number {
  return left + right;
}
const total = add(2, 3);
",
        Some("fn add"),
    );
}

/// Snapshot structured conditional and loop control flow.
#[test]
fn control_flow_emission() {
    assert_emitted_source_snapshot(
        "control_flow_emission",
        r"
function countPositive(values: number[]): number {
  let count = 0;
  for (const value of values) {
    if (value > 0) count = count + 1;
  }
  return count;
}
",
        None,
    );
}

/// Snapshot a closure that captures an enclosing immutable value.
#[test]
fn closure_capture_emission() {
    assert_emitted_source_snapshot(
        "closure_capture_emission",
        r"
function makeAdder(base: number): (value: number) => number {
  return (value: number): number => value + base;
}
",
        None,
    );
}

/// Snapshot a statically typed coercion from an erased input.
#[test]
fn typed_coercion_emission() {
    assert_emitted_source_snapshot(
        "typed_coercion_emission",
        r"
function asNumber(value: unknown): number {
  return value as number;
}
",
        Some("fn as_number"),
    );
}

/// Snapshot an erased coercion boundary retained as unknown.
#[test]
fn erased_coercion_emission() {
    assert_emitted_source_snapshot(
        "erased_coercion_emission",
        r"
function erase(value: number): unknown {
  return value as unknown;
}
",
        Some("fn erase"),
    );
}

/// Snapshot list construction and mutation emission.
#[test]
fn list_collection_emission() {
    assert_emitted_source_snapshot(
        "list_collection_emission",
        r"
function append(values: number[], value: number): number[] {
  values.push(value);
  return values;
}
",
        None,
    );
}

/// Snapshot map construction and lookup emission.
#[test]
fn map_collection_emission() {
    assert_emitted_source_snapshot(
        "map_collection_emission",
        r#"
function lookup(): number | undefined {
  const values = new Map<string, number>([["a", 1]]);
  return values.get("a");
}
"#,
        None,
    );
}

/// Snapshot set construction and membership emission.
#[test]
fn set_collection_emission() {
    assert_emitted_source_snapshot(
        "set_collection_emission",
        r"
function contains(): boolean {
  const values = new Set<number>([1, 2]);
  return values.has(2);
}
",
        None,
    );
}

/// Snapshot a generated Vitest-compatible test function.
#[test]
fn generated_test_function_emission() {
    assert_emitted_source_snapshot(
        "generated_test_function_emission",
        r#"
import { expect, test } from "vitest";
test("adds", () => {
  expect(1 + 2).toBe(3);
});
"#,
        None,
    );
}

/// Snapshot callback-driven list projection emission.
#[test]
fn collection_callback_emission() {
    assert_emitted_source_snapshot(
        "collection_callback_emission",
        r"
function double(values: number[]): number[] {
  return values.map((value) => value * 2);
}
",
        Some("fn double"),
    );
}

/// Snapshot migrated callback CFG shapes that are prone to legacy rendering regressions.
#[test]
fn migrated_callback_cfg_emission() {
    assert_emitted_source_snapshot(
        "migrated_callback_cfg_emission",
        r"
function plusOne(values: number[]): number[] {
  return values.map((x) => x + 1);
}

function lower(values: string[]): string[] {
  return values.map((value) => value.toLowerCase());
}

function spread(values: number[], args: number[], func: (...items: number[]) => number): number[] {
  return values.map((n) => func(...args, n));
}

function collectIndices(values: number[]): number[] {
  const indices: number[] = [];
  values.forEach((_value, index) => {
    indices.push(index);
  });
  return indices;
}

function iso(values: Date[]): string[] {
  return values.map((value: unknown) => (value as Date).toISOString());
}
",
        Some("fn plus_one"),
    );
}
