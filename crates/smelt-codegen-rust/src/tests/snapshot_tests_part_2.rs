//! Additive whole-output emitter snapshots for the bug-prone codegen families.
//!
//! These complement the substring (`src.contains(..)`) assertions in
//! `part_1..7_tests.rs`: the existing tests pin individual fragments, while
//! these capture the entire emitted module so reformatting and regressions show
//! up as a reviewable diff against a checked-in `.snap` fixture. Source inputs
//! are lifted verbatim from the corresponding `part_N` tests so the two surfaces
//! describe the same emission. Nothing here removes or weakens an existing test.

use super::snapshot_tests::{assert_emitted_py_source_snapshot, assert_emitted_source_snapshot};

// --- Control flow ---------------------------------------------------------

/// Snapshot a nested `while`/`for-of` region with `break`/`continue`, the shape
/// most prone to structural-loop regressions.
#[test]
fn nested_loop_control_flow_emission() {
    assert_emitted_source_snapshot(
        "nested_loop_control_flow_emission",
        r#"
export function collect(values: number[], flags: boolean[]): number {
  let total = 0;
  let index = 0;

  while (index < flags.length) {
    if (flags[index]) {
      index = index + 1;
      continue;
    }

    for (const value of values) {
      if (value > 2) {
        break;
      }
      total = total + value;
    }

    index = index + 1;
  }

  return total;
}
"#,
        Some("fn collect"),
    );
}

/// Snapshot a `do { } while` loop, which must lower to a structured `loop`
/// without recursive-region fallbacks or default returns.
#[test]
fn do_while_control_flow_emission() {
    assert_emitted_source_snapshot(
        "do_while_control_flow_emission",
        r#"
export function firstOutside(value: number): number {
  let previous = value;
  let current = value;
  do {
    previous = current;
    current = current - 1;
  } while (current > 0);
  return previous;
}
"#,
        Some("fn first_outside"),
    );
}

/// Snapshot a short-circuit guard that returns `NaN` then continues into a loop,
/// pinning continuation placement after the guard.
#[test]
fn short_circuit_guard_control_flow_emission() {
    assert_emitted_source_snapshot(
        "short_circuit_guard_control_flow_emission",
        r#"
function valid(value: boolean): boolean {
  return value;
}

export function count(a: boolean, b: boolean): number {
  if (!valid(a) || !valid(b)) return NaN;
  let result = 0;
  while (a) {
    result += 1;
    break;
  }
  return result;
}
"#,
        Some("fn valid"),
    );
}

// --- List / string operations --------------------------------------------

/// Snapshot the full array-callback method surface (map/filter/find/some/every/
/// reduce, captured closures, bound callbacks). This is the densest cluster of
/// fragment assertions in `part_1_tests.rs`.
#[test]
fn array_callback_methods_emission() {
    assert_emitted_source_snapshot(
        "array_callback_methods_emission",
        r#"
const values: number[] = [1, 2, 3];
const mapped = values.map(value => value + 1);
const filtered = values.filter(value => value > 1);
const found = values.find(value => value > 1);
const foundIndex = values.findIndex(value => value > 1);
const hasAny = values.some(value => value > 1);
const hasEvery = values.every(value => value > 0);
values.forEach(value => value + 1);
const total = values.reduce((acc, value) => acc + value, 0);
const indexed = values.map((value, index) => value + index);
const noInitial = values.reduce((acc, value, index) => acc + value + index);
"#,
        Some("fn main"),
    );
}

/// Snapshot string case/trim/prefix/search/replace/repeat/pad/charAt lowering in
/// one module so the JS string-method ABI is captured whole.
#[test]
fn string_method_surface_emission() {
    assert_emitted_source_snapshot(
        "string_method_surface_emission",
        r#"
const word = "Smelt";
const lower = word.toLowerCase();
const upper = word.toUpperCase();
const trimmed = word.trim();
const starts = word.startsWith("Sm");
const ends = word.endsWith("lt");
const first = word.indexOf("m");
const last = word.lastIndexOf("t");
const replaced = word.replace("Sm", "sm");
const repeated = word.repeat(3);
const paddedStart = word.padStart(8, "0");
const ch = word.charAt(1);
const code = word.charCodeAt(2);
"#,
        Some("fn main"),
    );
}

/// Snapshot string indexing plus `for..of` character iteration, including the
/// negative-index normalization runtime.
#[test]
fn string_index_and_for_of_emission() {
    assert_emitted_source_snapshot(
        "string_index_and_for_of_emission",
        r#"
const word = "abc";
const first = word[0];
const last = word.at(-1);
let joined = "";
for (let ch: string of word) {
  joined = joined + ch;
}
"#,
        Some("fn main"),
    );
}

// Note: a `Record`-indexing callback case was considered here but its emitted
// module (record runtime helpers) exceeds the 450-line selective-snapshot
// budget; the callback/record interaction is covered by the substring tests in
// part_1_tests.rs and by the other callback snapshots below.

// --- Closures / callbacks -------------------------------------------------

/// Snapshot first-class TypeScript closure values: generic, default, and stored
/// arrow closures.
#[test]
fn first_class_closure_value_emission() {
    assert_emitted_source_snapshot(
        "first_class_closure_value_emission",
        r#"
function makeAdder(base: number): (value: number) => number {
  const offset = base + 1;
  return (value: number): number => value + offset;
}
const adder = makeAdder(2);
const result = adder(3);
"#,
        Some("fn make_adder"),
    );
}

/// Snapshot an async function awaiting an inner async arrow closure.
#[test]
fn async_closure_emission() {
    assert_emitted_source_snapshot(
        "async_closure_emission",
        r#"
async function run(): Promise<number> {
  const lift = async (value: number): Promise<number> => value + 1;
  const result = await lift(4);
  return result;
}
"#,
        Some("fn run"),
    );
}

/// Snapshot a captured multi-argument callback invoked inside `Array.map`,
/// pinning that the four-argument ABI survives capture.
#[test]
fn captured_multi_arg_callback_emission() {
    assert_emitted_source_snapshot(
        "captured_multi_arg_callback_emission",
        r#"
function zip(
  first: unknown[],
  second: unknown[],
  fn: (first: unknown, second: unknown, index: number, data: unknown[]) => unknown,
): unknown[] {
  return first.map((item, index) => fn(item, second[index], index, first));
}
"#,
        Some("fn zip"),
    );
}

// --- Async / awaited result ------------------------------------------------

/// Snapshot a Vitest `rejects.toThrow` assertion lowered to an awaited result
/// match, exercising the async error-result path end to end.
#[test]
fn async_rejects_to_throw_emission() {
    assert_emitted_source_snapshot(
        "async_rejects_to_throw_emission",
        r#"
import { expect, test } from "vitest";

async function fail(): Promise<void> {
  throw "boom";
}

test("rejects", async () => {
  await expect(fail()).rejects.toThrow("boom");
});
"#,
        Some("async fn fail"),
    );
}

// --- Coercion / SmeltUnknown paths ----------------------------------------

/// Snapshot the `unknown` tag type plus typed passthrough signatures so the
/// `SmeltUnknown` boundary surface is captured whole.
#[test]
fn unknown_tagged_type_emission() {
    assert_emitted_source_snapshot(
        "unknown_tagged_type_emission",
        r#"
function identity(value: unknown): unknown {
  return value;
}

function passthrough(values: readonly unknown[]): readonly unknown[] {
  return values;
}
"#,
        Some("fn identity"),
    );
}

/// Snapshot `typeof`/`Array.isArray` narrowing and `as` casts over `unknown`,
/// the runtime-narrowing coercion family.
#[test]
fn unknown_narrowing_and_cast_emission() {
    assert_emitted_source_snapshot(
        "unknown_narrowing_and_cast_emission",
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
        Some("fn box_string"),
    );
}

// --- Python frontend families ---------------------------------------------

/// Snapshot a Python nested `def` closure that captures an enclosing parameter,
/// covering the Python-frontend closure path whole-output.
#[test]
fn python_nested_def_closure_emission() {
    assert_emitted_py_source_snapshot(
        "python_nested_def_closure_emission",
        r#"
from typing import Callable

def make_adder(base: int) -> Callable[[int], int]:
    def add(value: int) -> int:
        return value + base
    return add

adder: Callable[[int], int] = make_adder(5)
result: int = adder(6)
"#,
        Some("fn make_adder"),
    );
}

/// Snapshot Python `map`/`filter` lambda callbacks, the Python list-operation
/// callback family.
#[test]
fn python_map_filter_lambda_emission() {
    assert_emitted_py_source_snapshot(
        "python_map_filter_lambda_emission",
        r#"
values: list[int] = [1, 2, 3]
doubled: list[int] = list(map(lambda value: value * 2, values))
positives: list[int] = list(filter(lambda value: value > 1, values))
"#,
        Some("fn main"),
    );
}
