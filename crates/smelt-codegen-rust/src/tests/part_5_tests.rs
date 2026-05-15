//! Split codegen tests chunk.

use super::*;

#[test]
fn emits_python_pytest_raises_as_result_control_flow() {
    let source = source_for_py_path(
        r#"
import pytest

def test_raises():
    with pytest.raises(Exception):
        raise "boom"
"#,
        "tests/test_raises.py",
    );

    assert!(
        source.contains("#[test]\nfn test_raises() -> Result<(), Box<dyn std::error::Error>> {")
    );
    assert!(source.contains("let mut __smelt_pytest_raised: bool = false;"));
    assert!(source.contains("__smelt_pytest_raised = true;"));
    assert!(source.contains("pytest.raises(...) did not raise"));
    assert!(source.contains("return Ok(());"));
}

#[test]
fn emits_typescript_vitest_test_case_as_rust_test() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("adds numbers", () => {
  expect(1 + 1).toBe(2);
});
"#,
    );

    assert!(
        source.contains(
            "#[test]\nfn test_adds_numbers() -> Result<(), Box<dyn std::error::Error>> {"
        )
    );
    assert!(source.contains("1.0 + 1.0"));
    assert!(source.contains("!= 2.0"));
    assert!(source.contains("return Ok(());"));
}

#[test]
fn emits_typescript_describe_it_as_flattened_rust_test() {
    let source = source_for(
        r#"
import { describe, it, expect } from "vitest";

describe("math helpers", () => {
  it("adds numbers", () => {
    expect(1 + 1).toBe(2);
  });
});
"#,
    );

    assert!(source.contains(
        "#[test]\nfn test_math_helpers_adds_numbers() -> Result<(), Box<dyn std::error::Error>> {"
    ));
    assert!(!source.contains("fn main()"));
    assert!(source.contains("return Ok(());"));
}

#[test]
fn emits_typescript_vitest_common_positive_matchers() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("common matchers", () => {
  expect(1 + 1).toEqual(2);
  expect([1, 2, 3]).toContain(2);
  expect([1, 2, 3]).toHaveLength(3);
  expect(["a"]).toStrictEqual(["a"]);
  expect([1, 2, 3]).not.toContain(4);
  const user: Record<string, string> = { name: "Ada" };
  expect(user).toHaveProperty("name");
  U.deepStrictEqual([1, 2], [1, 2]);
});
"#,
    );

    assert!(source.contains("expect(...).toEqual(...) failed"));
    assert!(source.contains(".contains(&"));
    assert!(source.contains("expect(...).toHaveLength(...) failed"));
    assert!(source.contains("expect(...).toStrictEqual(...) failed"));
    assert!(source.matches('!').count() >= 2);
    assert!(source.contains(".contains_key(&"));
    assert!(source.contains("deepStrictEqual(...) failed"));
}

#[test]
fn emits_set_mutation_methods() {
    let ts_source = source_for(
        r#"
let values: Set<number> = new Set([1, 2]);
const same = values.add(3);
const deleted = values.delete(2);
values.clear();
"#,
    );
    let py_source = source_for_py(
        r#"
values: set[int] = {1, 2}
values.add(3)
values.discard(2)
values.remove(1)
copy: set[int] = values.copy()
values.clear()
"#,
    );

    assert!(ts_source.contains(".insert(3.0)"));
    assert!(ts_source.contains(".remove(&2.0)"));
    assert!(ts_source.contains(".clear(); ()"));
    assert!(py_source.contains(".insert(3)"));
    assert!(py_source.contains(".remove(&2); ()"));
    assert!(py_source.contains("panic!(\"set remove missing item\")"));
    assert!(py_source.contains(".clone()"));
}

#[test]
fn emits_map_and_set_size_properties() {
    let source = source_for(
        r#"
const values: Set<number> = new Set([1, 2]);
const mapping: Map<string, number> = new Map();
const setSize = values.size;
const mapSize = mapping.size;
"#,
    );

    assert!(source.matches(".len() as f64").count() >= 2);
}

#[test]
fn emits_map_and_set_projection_methods() {
    let source = source_for(
        r#"
const values: Set<number> = new Set([1, 2]);
const valueKeys = values.keys();
const valueList = values.values();
const valueEntries = values.entries();
const mapping: Map<string, number> = new Map();
const mapKeys = mapping.keys();
const mapValues = mapping.values();
const mapEntries = mapping.entries();
"#,
    );

    assert!(source.contains(".keys().cloned().collect::<Vec<_>>()"));
    assert!(source.contains(".values().cloned().collect::<Vec<_>>()"));
    assert!(
        source.contains(
            ".iter().map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>()"
        )
    );
    assert!(source.contains(".iter().cloned().collect::<Vec<_>>()"));
    assert!(
        source.contains(".iter().map(|value| (value.clone(), value.clone())).collect::<Vec<_>>()")
    );
}

#[test]
fn emits_python_set_algebra_methods() {
    let source = source_for_py(
        r#"
left: set[int] = {1, 2}
right: set[int] = {2, 3}
merged: set[int] = left.union(right)
common: set[int] = left.intersection(right)
only_left: set[int] = left.difference(right)
exclusive: set[int] = left.symmetric_difference(right)
separate: bool = left.isdisjoint(right)
subset: bool = left.issubset(right)
superset: bool = left.issuperset(right)
"#,
    );

    assert!(source.contains(".union(&"));
    assert!(source.contains(".intersection(&"));
    assert!(source.contains(".difference(&"));
    assert!(source.contains(".symmetric_difference(&"));
    assert!(source.contains(".is_disjoint(&"));
    assert!(source.contains(".is_subset(&"));
    assert!(source.contains(".is_superset(&"));
    assert!(source.contains(".cloned().collect()"));
}

#[test]
fn emits_map_constructor_has_and_get_methods() {
    let source = source_for(
        r#"
const mapping: Map<string, number> = new Map();
const literal = new Map([["a", 1], ["b", 2]]);
const has = mapping.has("a");
const value = mapping.get("a");
"#,
    );

    assert!(source.contains("::std::collections::HashMap<String, f64>"));
    assert!(source.contains("::std::collections::HashMap::from([]);"));
    assert!(source.contains(
        "::std::collections::HashMap::from([(\"a\".to_owned(), 1.0), (\"b\".to_owned(), 2.0)])"
    ));
    assert!(source.contains(".contains_key(&\"a\".to_owned());"));
    assert!(source.contains(".get(&\"a\".to_owned()).cloned();"));
}

#[test]
fn emits_map_mutation_methods() {
    let source = source_for(
        r#"
let mapping: Map<string, number> = new Map();
const same = mapping.set("a", 1);
const deleted = mapping.delete("a");
mapping.clear();
"#,
    );

    assert!(source.contains(".insert(\"a\".to_owned(), 1.0);"));
    assert!(source.contains(".remove(&\"a\".to_owned()).is_some();"));
    assert!(source.contains(".clear(); ()"));
    assert!(source.contains(".clone()"));
}

#[test]
fn emits_string_split_method() {
    let source = source_for(
        r#"
const word = "a,b,c";
const parts = word.split(",");
const limited = word.split(",", 2);
"#,
    );

    assert!(source.contains(".split(&\",\".to_owned()).map(str::to_owned).collect::<Vec<_>>();"));
    assert!(source.contains(".take((2.0 as f64).max(0.0) as usize)"));
}

#[test]
fn emits_array_join_method() {
    let source = source_for(
        r#"
const words: string[] = ["a", "b", "c"];
const joined = words.join("-");
const comma = words.join();
const numbers: number[] = [1, 2, 3];
const numberJoined = numbers.join("-");
"#,
    );

    assert!(source.contains(".join(&\"-\".to_owned());"));
    assert!(source.contains(".join(&\",\".to_owned());"));
    assert!(source.contains(".iter().map(|item| { item.to_string() })"));
}

#[test]
fn emits_array_concat_method() {
    let source = source_for(
        r#"
const left: number[] = [1, 2];
const right: number[] = [3, 4];
const merged = left.concat(right);
"#,
    );

    assert!(source.contains(".iter().cloned().chain("));
    assert!(source.contains(".collect::<Vec<_>>();"));
}

#[test]
fn emits_array_search_methods() {
    let source = source_for(
        r#"
const values: number[] = [1, 2, 3, 2];
const first = values.indexOf(2);
const last = values.lastIndexOf(2);
"#,
    );

    assert!(source.contains(".iter().position(|item| item == &2.0).map_or(-1.0"));
    assert!(source.contains(".iter().rposition(|item| item == &2.0).map_or(-1.0"));
}

#[test]
fn emits_array_and_string_slice_methods() {
    let source = source_for(
        r#"
const values: number[] = [1, 2, 3, 4];
const allValues = values.slice();
const tailValues = values.slice(1);
const midValues = values.slice(1, 3);
const lastValues = values.slice(-2);
const word = "smelting";
const allText = word.slice();
const tailText = word.slice(1);
const midText = word.slice(1, 4);
const lastText = word.slice(-3);
function sliceOptional(start?: number, end?: number): string {
  return word.slice(start, end);
}
"#,
    );

    assert!(source.contains(".iter().skip(0usize).take("));
    assert!(source.contains("let index = 1.0 as i64"));
    assert!(source.contains("clamp(0, len) as usize"));
    assert!(source.contains(".cloned().collect::<Vec<_>>();"));
    assert!(source.contains(".chars().skip(0usize).take("));
    assert!(source.matches("if index < 0").count() >= 2);
    assert!(source.contains(".collect::<String>();"));
    assert!(source.contains(".unwrap_or(0.0)"));
    assert!(source.contains(".chars().count() as f64"));
}

#[test]
fn emits_array_push_method() {
    let source = source_for(
        r#"
let values: number[] = [1, 2];
values.push(3);
const length = values.push(4);
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains("Vec<f64>"));
    assert!(source.contains(".push(3.0);"));
    assert!(source.contains(".push(4.0);"));
    assert!(source.contains(".len() as f64"));
}

#[test]
fn emits_array_unshift_method() {
    let source = source_for(
        r#"
let values: number[] = [2, 3];
const sameLength = values.unshift();
const oneMore = values.unshift(1);
const threeMore = values.unshift(-1, 0);
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".insert(0, 1.0);"));
    assert!(source.contains(".insert(0, 0.0);"));
    assert!(source.matches(".insert(0,").count() >= 3);
    assert!(source.matches(".len() as f64").count() >= 3);
}

#[test]
fn emits_array_reverse_method() {
    let source = source_for(
        r#"
let values: number[] = [1, 2];
values.reverse();
const reversed = values.reverse();
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".reverse();"));
    assert!(source.contains(".clone()"));
}

#[test]
fn emits_array_sort_method() {
    let source = source_for(
        r#"
let values: number[] = [10, 2];
values.sort();
const sorted = values.sort();
const sortedByNumber = values.sort((left, right) => left - right);
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".sort_by(|left, right| left.to_string().cmp(&right.to_string()))"));
    assert!(source.contains("if ordering < 0.0"));
    assert!(source.contains(".clone()"));
}

#[test]
fn emits_modern_array_methods() {
    let source = source_for(
        r#"
let values: number[] = [1, 2, 3, 4];
let nested: number[][] = [[1], [2, 3]];
const spliced = values.splice(1, 2, 9);
const copiedSplice = values.toSpliced(1, 1, 8);
const spreadSplice = values.toSpliced(1, 1, ...[10, 11]);
const filled = values.fill(0, 1, 3);
const copiedWithin = values.copyWithin(0, 1, 3);
const replaced = values.with(1, 7);
const flat = nested.flat();
const flatMapped = values.flatMap((value, index) => [value + index]);
const sorted = values.toSorted((left, right) => right - left);
const reversed = values.toReversed();
const last = values.findLast((value, index) => value > index);
const lastIndex = values.findLastIndex((value, index) => value > index);
const keys = values.keys();
const vals = values.values();
const entries = values.entries();
"#,
    );

    assert!(source.contains(".splice("));
    assert!(source.contains("splice_replacements.extend"));
    assert!(source.contains("copy_items"));
    assert!(source.contains("fill_index"));
    assert!(source.contains("with_items"));
    assert!(source.contains(".flat_map(|items| items.iter().cloned())"));
    assert!(source.contains(".iter().enumerate().flat_map("));
    assert!(source.contains(".iter().enumerate().rev().find("));
    assert!(source.contains(".iter().enumerate().rposition("));
    assert!(source.contains("(0.."));
    assert!(source.contains(".iter().cloned().enumerate().map(|(idx, item)|"));
}
