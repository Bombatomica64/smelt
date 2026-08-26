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
    assert!(source.contains("return Ok(());"));
}

#[test]
fn emits_python_pytest_raises_match_as_regex_check() {
    let source = source_for_py_path(
        r#"
import pytest

def test_raises_match():
    with pytest.raises(Exception, match="bo+m"):
        raise "booom"
"#,
        "tests/test_raises_match.py",
    );

    assert!(source.contains("regex::Regex::new(&"));
    assert!(source.contains("pytest.raises(...) match failed"));
    assert!(source.contains("__smelt_pytest_exception"));
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
    assert!(source.contains("lhs.is_nan() && rhs.is_nan()"));
    assert!(source.contains("return Ok(());"));
}

#[test]
fn emits_nullish_vitest_matchers_as_success_for_null_literals() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("nullish", () => {
  expect(undefined).toBeUndefined();
  expect(null).toBeNull();
});
"#,
    );

    assert!(source.contains("!(true)"), "{source}");
    assert!(!source.contains("!(false)"), "{source}");
}

#[test]
fn emits_optional_unknown_to_be_undefined_without_unwrap() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

function maybeValue(flag: boolean): unknown | undefined {
  if (flag) {
    return undefined;
  }
  return "x";
}

test("optional unknown", () => {
  expect(maybeValue(true)).toBeUndefined();
});
"#,
    );

    assert!(
        source.contains(".as_ref().map_or(true, |value| matches!(value, SmeltUnknown::Undefined))"),
        "{source}"
    );
    assert!(
        !source.contains("expect(\"optional value was absent after narrowing\")"),
        "{source}"
    );
}

#[test]
fn erases_explicit_undefined_literal_to_undefined_tag() {
    let source = source_for(
        r"
function value(): unknown {
  return undefined;
}
",
    );

    assert!(source.contains("SmeltUnknown::Undefined"), "{source}");
}

#[test]
fn erases_void_operator_to_undefined_tag() {
    let source = source_for(
        r"
function value(): unknown {
  return void 0;
}
",
    );

    assert!(source.contains("SmeltUnknown::Undefined"), "{source}");
}

#[test]
fn keeps_null_literal_erasure_as_null_tag() {
    let source = source_for(
        r"
function value(): unknown {
  return null;
}
",
    );

    assert!(source.contains("SmeltUnknown::Null"), "{source}");
    assert!(source.contains("return SmeltUnknown::Null;"), "{source}");
    assert!(
        !source.contains("return SmeltUnknown::Undefined;"),
        "{source}"
    );
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
    assert!(source.contains("fn main()"));
    assert!(source.contains("return Ok(());"));
}

#[test]
fn emits_distinct_classes_for_same_named_sibling_suite_bindings() {
    let source = source_for(
        r#"
import { describe, expect, it } from "vitest";
describe("outer", () => {
  describe("first", () => {
    class Person { constructor() {} }
    it("constructs", () => { expect(new Person()).toBeInstanceOf(Person); });
  });
  describe("second", () => {
    class Person {
      name: string;
      friends: Person[] = [];
      self?: Person;
      constructor(name: string) { this.name = name; }
    }
    it("keeps fields", () => {
      const person = new Person("jake");
      person.self = person;
      person.friends = [person];
      expect(person.name).toBe("jake");
    });
  });
});
"#,
    );

    let person_structs = source
        .lines()
        .filter(|line| {
            let Some(name) = line
                .strip_prefix("struct ")
                .and_then(|rest| rest.split([' ', '{', '(']).next())
            else {
                return false;
            };
            name.starts_with("PersonSmeltSuiteF") && !name.ends_with("Inner")
        })
        .count();
    assert_eq!(person_structs, 2, "{source}");
    assert!(source.contains("::new(\"jake\".to_owned())"), "{source}");
    assert!(source.contains(".self_ ="), "{source}");
    assert!(source.contains(".friends ="), "{source}");
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
  expect(new Date()).toBeInstanceOf(Date);
  const user: Record<string, string> = { name: "Ada" };
  expect(user).toHaveProperty("name");
  U.deepStrictEqual([1, 2], [1, 2]);
});
"#,
    );

    assert!(source.contains("expect(...).toEqual(...) failed"));
    assert!(
        source.contains("item.is_nan() && smelt_needle.is_nan()"),
        "{source}"
    );
    assert!(source.contains("expect(...).toHaveLength(...) failed"));
    assert!(source.contains("expect(...).toStrictEqual(...) failed"));
    assert!(source.matches('!').count() >= 2);
    assert!(source.contains(".contains_key(&"));
    assert!(source.contains("deepStrictEqual(...) failed"));
}

/// A destructured callback parameter binds the FIELD's type, not the
/// parameter's.
///
/// The compact callback IR resolved a destructured field's type for dicts,
/// maps and classes, and fell back to the *parameter's own type* for
/// everything else. Over `T[][]`, `arrays.map(({ length }) => length)`
/// therefore typed `length` as `T[]`, so the callback claimed to return a
/// list and the emitter coerced the number into a one-element list. That is
/// how radash's `zip` stopped compiling: `Math.max(..)` over the mapped
/// lengths got `expected f64, found SmeltList<SmeltUnknown>`. `length` is a
/// number on every list and string.
#[test]
fn a_destructured_callback_parameter_binds_the_field_type() {
    let source = source_for(
        r"
export function lengths<T>(arrays: T[][]): number[] {
  return arrays.map(({ length }) => length);
}
",
    );

    assert!(
        !source.contains("SmeltList::from(vec![SmeltUnknown::Number("),
        "the callback must yield `length` itself, not a one-element list \
         holding it:\n{source}"
    );
    // An ERASED receiver must keep reading the field at runtime. Its binding
    // really is `unknown`, so the old parameter-type fallback happened to be
    // right there -- and routing it to the closure-body fallback instead is
    // worse, because that path binds the destructured name to
    // `Default::default()` and never reads the field. remeda's
    // `binarySearchCutoffIndex(["a", "ab", ..], ({ length }) => length < 3)`
    // answered from a default and returned the wrong index.
    let erased = source_for(
        r"
export function cutoff<T>(array: readonly T[], predicate: (value: T) => boolean): number {
  return array.filter(predicate).length;
}

export function run(): number {
  return cutoff(['a', 'ab', 'abc'], ({ length }) => length < 3);
}
",
    );
    assert!(
        !erased.contains("let length: SmeltUnknown = "),
        "a destructured field of an erased parameter must be read, not bound to \
         a default:\n{erased}"
    );

    // The equivalent member access has always been right; the two spellings
    // must agree.
    let member = source_for(
        r"
export function lengths<T>(arrays: T[][]): number[] {
  return arrays.map(a => a.length);
}
",
    );
    assert!(
        !member.contains("SmeltList::from(vec![SmeltUnknown::Number("),
        "control: the member-access spelling must not wrap either:\n{member}"
    );
}

/// Vitest compares primitive numbers with `Object.is` under every equality
/// matcher, so `NaN` equals `NaN`.
///
/// Only `toBe` used the `Object.is` comparison; `toEqual` and `toStrictEqual`
/// emitted a plain `!=`, which on `f64` reports `NaN != NaN`. Assertions like
/// `expect(mean([])).toEqual(NaN)` therefore failed on the value they wanted.
/// Objects and arrays keep structural comparison under the deep matchers --
/// only `toBe` compares those by reference.
#[test]
fn deep_matchers_compare_numbers_with_object_is() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

function mean(values: number[]): number {
  return values.length === 0 ? NaN : values[0];
}

test("nan", () => {
  expect(mean([])).toEqual(NaN);
  expect(mean([])).toStrictEqual(NaN);
  expect([1]).toEqual([1]);
});
"#,
    );

    let same_value = source.matches("is_nan() && ").count();
    assert!(
        same_value >= 2,
        "toEqual/toStrictEqual on numbers must use the Object.is comparison: {source}"
    );
    // The list assertion stays structural rather than becoming a reference check.
    assert!(source.contains("expect(...).toEqual(...) failed"), "{source}");
}

/// A failed assertion must name the source assertion and its location.
///
/// Generated suites throw a plain string, so without the snippet and
/// `path:line:column` suffix a large generated suite reports only which
/// matcher failed, which is not enough to find the offending spec line.
#[test]
fn generated_assertion_failures_carry_source_snippet_and_location() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("located", () => {
  expect(1 + 1).toEqual(2);
  expect([
    1,
    2,
  ]).toHaveLength(2);
});
"#,
    );

    assert!(
        source.contains("expect(...).toEqual(...) failed: expect(1 + 1).toEqual(2) (<memory>:5:3)"),
        "{source}"
    );
    // A multi-line assertion collapses onto one line so the message stays scannable.
    assert!(
        source.contains(
            "expect(...).toHaveLength(...) failed: expect([ 1, 2, ]).toHaveLength(2) (<memory>:6:3)"
        ),
        "{source}"
    );
}

#[test]
fn emits_identity_bearing_erased_arrays_for_strict_matchers() {
    let source = source_for(
        r#"
import { test, expect } from "vitest";

test("array reference identity", () => {
  const original: unknown = [1];
  const alias: unknown = original;
  const copy: unknown = [1];
  expect(alias).toBe(original);
  expect(copy).not.toBe(original);
  expect(copy).toStrictEqual(original);
});
"#,
    );

    assert!(source.contains("Array(SmeltArray)"), "{source}");
    assert!(
        source.contains(
            "(SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.id == right.id"
        ),
        "{source}"
    );
    assert!(source.contains("SmeltUnknown::Array(vec!["), "{source}");
    assert!(source.contains(".same_js_key(&"), "{source}");
}

#[test]
fn emits_same_value_zero_identity_for_erased_object_is_or_strict_equality() {
    let source = source_for(
        r"
export function same(data: unknown, other: unknown): boolean {
  return data === other || Object.is(data, other);
}
",
    );

    assert!(source.contains(".same_js_key(&"), "{source}");
    assert!(
        !source.contains("data.clone() == other.clone()"),
        "{source}"
    );
}

#[test]
fn emits_strict_identity_for_optional_records() {
    let source = source_for(
        r#"
import { expect } from "vitest";

function compare(left?: { a: number }, right?: { a: number }): void {
  expect(left).toBe(right);
  expect(left).not.toBe(right);
}
"#,
    );

    assert!(
        source.contains("(Some(left), Some(right)) => left.id == right.id"),
        "{source}"
    );
}

#[test]
fn emits_set_mutation_methods() {
    let ts_source = source_for(
        r"
let values: Set<number> = new Set([1, 2]);
const same = values.add(3);
const deleted = values.delete(2);
values.clear();
",
    );
    let py_source = source_for_py(
        r"
values: set[int] = {1, 2}
values.add(3)
values.discard(2)
values.remove(1)
copy: set[int] = values.copy()
values.clear()
",
    );

    assert!(ts_source.contains(".clone()"));
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
        r"
const values: Set<number> = new Set([1, 2]);
const mapping: Map<string, number> = new Map();
const setSize = values.size;
const mapSize = mapping.size;
",
    );

    assert!(source.matches(".len() as f64").count() >= 2);
}

#[test]
fn emits_map_and_set_projection_methods() {
    let source = source_for(
        r"
const values: Set<number> = new Set([1, 2]);
const valueKeys = values.keys();
const valueList = values.values();
const valueEntries = values.entries();
const mapping: Map<string, number> = new Map();
const mapKeys = mapping.keys();
const mapValues = mapping.values();
const mapEntries = mapping.entries();
",
    );

    // A source `Map` backs onto `SmeltJsMap` regardless of key type, so its
    // projections take the symbol-only filter (SmeltJsMap never carries the
    // internal `__smelt_symbol:`/`__smelt_class` marker keys that only a
    // `SmeltRecord` object accumulates, and `smelt_is_for_in_record_key` is not
    // defined over `SmeltJsMap`). Keys therefore have no record-marker filter.
    assert!(
        source.contains(
            ".keys().filter(|key| !key.starts_with(\"__smelt_symbol:\")).collect::<Vec<_>>()"
        ),
        "{source}"
    );
    // The map keys projection must not apply the `SmeltRecord`-only marker
    // filter (`smelt_is_for_in_record_key`, which is defined in the prelude but
    // must not appear in a `SmeltJsMap` keys projection).
    assert!(
        !source.contains(
            ".keys().filter(|key| !key.starts_with(\"__smelt_symbol:\") && smelt_is_for_in_record_key(&"
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
    assert!(source.contains(".iter().cloned().collect::<Vec<_>>()"));
    assert!(
        source.contains(".iter().map(|value| (value.clone(), value.clone())).collect::<Vec<_>>()")
    );
}

#[test]
fn emits_python_set_algebra_methods() {
    let source = source_for_py(
        r"
left: set[int] = {1, 2}
right: set[int] = {2, 3}
merged: set[int] = left.union(right)
common: set[int] = left.intersection(right)
only_left: set[int] = left.difference(right)
exclusive: set[int] = left.symmetric_difference(right)
separate: bool = left.isdisjoint(right)
subset: bool = left.issubset(right)
superset: bool = left.issuperset(right)
",
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

    // A source `Map` backs onto `SmeltJsMap` (not `HashMap`) even when
    // string-keyed, so erasure can stamp the `__smelt_map` marker. `SmeltJsMap`
    // stores owned entries, so `get` returns `Option<V>` directly with no
    // `.cloned()`.
    assert!(source.contains("SmeltJsMap<String, f64>"));
    assert!(source.contains("SmeltJsMap::from([]);"));
    assert!(source.contains(
        "SmeltJsMap::from([(\"a\".to_owned(), 1.0), (\"b\".to_owned(), 2.0)])"
    ));
    assert!(source.contains(".contains_key(&\"a\".to_owned());"));
    assert!(source.contains(".get(&\"a\".to_owned());"));
    assert!(!source.contains(".get(&\"a\".to_owned()).cloned();"));
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
fn emits_statement_position_map_set_on_erased_value_slot() {
    // Regression: a `Map<_, unknown>` (or inferred `Map<any, any>`) value slot is
    // erased to `SmeltUnknown`. A statement-position `map.set(k, concreteValue)`
    // whose result (the map) is unused must STILL emit the insert. The value-type
    // guard in `dict_set_text` previously required an exact operand/slot type
    // match and, on the concrete-vs-erased mismatch, folded the whole call to a
    // plain receiver read — silently dropping the insert. es-toolkit's
    // `isEqualWith` map comparisons then saw two empty maps and mis-compared.
    let source = source_for(
        r#"
const mapping: Map<string, unknown> = new Map();
mapping.set("a", 1);
mapping.set("b", 2);
"#,
    );

    let inserts = source.matches(".insert(").count();
    assert!(
        inserts >= 2,
        "both statement-position Map.set inserts on an erased value slot must be emitted (found {inserts})\n{source}"
    );
}

#[test]
fn emits_statement_position_set_add_on_erased_element_slot() {
    // Sibling of `emits_statement_position_map_set_on_erased_value_slot`: a
    // statement-position `set.add(concreteValue)` on an erased element slot must
    // likewise emit the insert rather than drop the observable mutation.
    let source = source_for(
        r"
const bag: Set<unknown> = new Set();
bag.add(1);
bag.add(2);
",
    );

    let inserts = source.matches(".insert(").count();
    assert!(
        inserts >= 2,
        "both statement-position Set.add inserts on an erased element slot must be emitted (found {inserts})\n{source}"
    );
}

#[test]
fn emits_optional_chained_map_methods_as_guarded_modeled_ops() {
    // `recv?.has/get/set/delete(...)` on an optional Map receiver desugars to the
    // same modeled dict operation the non-optional receiver produces, guarded by
    // a presence test and narrowed via the `optional value was absent` unwrap.
    // It must NOT fall through to a generic `.get("has")` field access.
    let source = source_for(
        r"
export function probe(stack: Map<string, number> | undefined, key: string): boolean | undefined {
  return stack?.has(key);
}
export function fetch(stack: Map<string, number> | undefined, key: string): number | undefined {
  return stack?.get(key);
}
export function store(stack: Map<string, number> | undefined, key: string): void {
  stack?.set(key, 1);
}
export function drop(stack: Map<string, number> | undefined, key: string): boolean | undefined {
  return stack?.delete(key);
}
",
    );

    assert!(
        source.contains(".is_none()"),
        "optional receiver presence test should emit is_none\n{source}"
    );
    assert!(
        source.contains(".expect(\"optional value was absent after narrowing\")"),
        "narrowed receiver should unwrap the optional in the present branch\n{source}"
    );
    assert!(
        source.contains(".contains_key(&"),
        "optional Map.has should lower to a modeled contains_key\n{source}"
    );
    assert!(
        source.contains(".insert("),
        "optional Map.set should lower to a modeled insert\n{source}"
    );
    assert!(
        source.contains(".remove(&"),
        "optional Map.delete should lower to a modeled remove\n{source}"
    );
    assert!(
        !source.contains(".get(\"has\")"),
        "optional Map.has must not misroute to an erased field access\n{source}"
    );
}

#[test]
fn emits_optional_chained_set_has_as_guarded_modeled_op() {
    // `recv?.has(value)` on an optional Set receiver desugars to a guarded
    // modeled `contains` check rather than a generic erased field access.
    let source = source_for(
        r"
export function probe(seen: Set<string> | undefined, value: string): boolean | undefined {
  return seen?.has(value);
}
",
    );

    assert!(
        source.contains(".is_none()"),
        "optional receiver presence test should emit is_none\n{source}"
    );
    assert!(
        source.contains(".contains(&"),
        "optional Set.has should lower to a modeled contains check\n{source}"
    );
    assert!(
        !source.contains(".get(\"has\")"),
        "optional Set.has must not misroute to an erased field access\n{source}"
    );
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

    assert!(source.contains("if smelt_separator.is_empty()"));
    assert!(
        source.contains(
            "smelt_haystack.split(&smelt_separator).map(str::to_owned).collect::<Vec<_>>()"
        )
    );
    assert!(source.contains("else if smelt_limit.is_sign_positive()"));
}

#[test]
fn emits_string_split_with_erased_union_limit() {
    let source = source_for(
        r#"
function splitWord(word: string, limit: number | undefined | string): string[] {
  return word.split(",", limit);
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Number(value) => Some(value)"),
        "{source}"
    );
    assert!(
        source.contains("else if split_limit.is_sign_positive()"),
        "{source}"
    );
}

#[test]
fn emits_regexp_string_split_from_static_object_separator() {
    let source = source_for(
        r"
function parts(value: string): string[] {
  return value.split(patterns.separator);
}
const patterns = { separator: /[T ]/i };
",
    );

    assert!(
        source.contains("SmeltRegExp::new(\"[T ]\".to_owned(), \"i\".to_owned())"),
        "{source}"
    );
    assert!(source.contains(".split_string(&"), "{source}");
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
        r"
const left: number[] = [1, 2];
const right: number[] = [3, 4];
const merged = left.concat(right);
",
    );

    assert!(source.contains(".iter().cloned().chain("));
    assert!(source.contains(".collect::<Vec<_>>()"));
}

#[test]
fn emits_array_search_methods() {
    let source = source_for(
        r"
const values: number[] = [1, 2, 3, 2];
const first = values.indexOf(2);
const last = values.lastIndexOf(2);
",
    );

    assert!(
        source.contains(".iter().position(|item| *item == smelt_needle"),
        "{source}"
    );
    assert!(
        source.contains(".iter().rposition(|item| *item == smelt_needle"),
        "{source}"
    );
    assert!(
        source.contains("item.is_nan() && smelt_needle.is_nan()"),
        "{source}"
    );
}

#[test]
fn emits_array_search_methods_with_from_index() {
    let source = source_for(
        r"
export function findFrom(values: readonly number[], target: number, from: number): number {
  return values.indexOf(target, from);
}
export function findLastFrom(values: readonly number[], target: number, from: number): number {
  return values.lastIndexOf(target, from);
}
",
    );

    // `indexOf` translates a negative `fromIndex` into an offset from the end,
    // clamps the start to zero, and scans forward from there.
    assert!(
        source.contains("if smelt_raw < 0 { (smelt_len + smelt_raw).max(0) } else { smelt_raw } as usize"),
        "{source}"
    );
    assert!(source.contains(".enumerate().skip(smelt_start)"), "{source}");
    // `lastIndexOf` clamps the inclusive end and searches backward, returning -1
    // when the end falls fully before the start of the array.
    assert!(
        source.contains("if smelt_raw < 0 { smelt_len + smelt_raw } else { smelt_raw.min(smelt_len - 1) }"),
        "{source}"
    );
    assert!(
        source.contains("if smelt_end < 0 { -1.0 }"),
        "{source}"
    );
    assert!(
        source.contains(".enumerate().take(smelt_take).rev()"),
        "{source}"
    );
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
    assert!(source.contains(".cloned().collect::<Vec<_>>()"));
    assert!(source.contains(".chars().skip(0usize).take("));
    assert!(source.matches("if index < 0").count() >= 2);
    assert!(source.contains(".collect::<String>();"));
    assert!(source.contains(".unwrap_or(0.0)"));
    assert!(source.contains(".chars().count() as f64"));
}

#[test]
fn emits_array_push_method() {
    let source = source_for(
        r"
let values: number[] = [1, 2];
values.push(3);
const length = values.push(4);
",
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
        r"
let values: number[] = [2, 3];
const sameLength = values.unshift();
const oneMore = values.unshift(1);
const threeMore = values.unshift(-1, 0);
",
    );

    assert!(source.contains(".insert(0, 1.0);"));
    assert!(source.contains(".insert(0, 0.0);"));
    assert!(source.matches(".insert(0,").count() >= 3);
    assert!(source.matches(".len() as f64").count() >= 3);
}

#[test]
fn emits_array_reverse_method() {
    let source = source_for(
        r"
let values: number[] = [1, 2];
values.reverse();
const reversed = values.reverse();
",
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".reverse();"));
    assert!(source.contains(".clone()"));
}

#[test]
fn emits_array_sort_method() {
    let source = source_for(
        r"
let values: number[] = [10, 2];
values.sort();
const sorted = values.sort();
const sortedByNumber = values.sort((left, right) => left - right);
",
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
let tupleNested: [word: string][] = [["one"], ["two"]];
let erasedNested: unknown[] = [[1], [[2]]];
const spliced = values.splice(1, 2, 9);
const copiedSplice = values.toSpliced(1, 1, 8);
const spreadSplice = values.toSpliced(1, 1, ...[10, 11]);
const filled = values.fill(0, 1, 3);
const copiedWithin = values.copyWithin(0, 1, 3);
const replaced = values.with(1, 7);
const flat = nested.flat();
const flatTuple = tupleNested.flat();
const deepFlat = erasedNested.flat(2);
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
    assert!(source.contains(".flat_map(|items| vec![items.0.clone()])"));
    assert!(source.contains("fn smelt_flat_values"));
    assert!(source.contains("smelt_flat_depth"));
    assert!(source.contains(".iter().enumerate().flat_map("));
    assert!(source.contains(".iter().enumerate().rev().find_map("));
    assert!(source.contains("(0.."));
    assert!(source.contains(".iter().cloned().enumerate().map(|(idx, item)|"));
}

/// A source `Map` and a plain object literal erase differently: only the `Map`
/// carries the `__smelt_map` identity marker.
///
/// This is the both-directions guard for stage-2 Map identity. A `new Map(...)`
/// backs onto `SmeltJsMap`, whose `IntoSmeltUnknown` stamps the `__smelt_map`
/// marker so the erased value stays observable as a Map (`isMap`, `[object
/// Map]`, structural `isEqual`). An object literal (a `Record` internally) backs
/// onto `SmeltRecord`, whose erasure is an ordinary unmarked object — no
/// `__smelt_map`. If Map stamping ever leaked onto object literals (or a Map
/// lost its `SmeltJsMap` backing), one of these type bindings would flip.
#[test]
fn erases_map_with_marker_but_object_literal_unmarked() {
    let source = source_for(
        r#"
const m = new Map<string, number>([["a", 1]]);
const asMap: unknown = m;
const obj = { a: 1 };
const asRecord: unknown = obj;
"#,
    );

    // Direction 1: the Map is backed by the marker-stamping `SmeltJsMap`.
    assert!(
        source.contains("let m: SmeltJsMap<String, f64> ="),
        "{source}"
    );
    // Direction 2: the object literal stays an unmarked `SmeltRecord`, never a
    // `SmeltJsMap`.
    assert!(
        source.contains("let obj: SmeltRecord<String, f64> ="),
        "{source}"
    );
    assert!(
        !source.contains("let obj: SmeltJsMap"),
        "an object literal must not acquire the Map backing:\n{source}"
    );
    // The `__smelt_map` marker exists only inside `SmeltJsMap`'s erasure adapter
    // (a legitimate dynamic boundary), never in `SmeltRecord`'s.
    assert!(
        source.contains(
            "let object = Vec::from([(\"__smelt_map\".to_owned(), SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), pairs)))])"
        ),
        "{source}"
    );
}

/// An array literal that writes both `null` and `undefined` must keep the two
/// spellings apart.
///
/// They share one HIR type (`Type::None`), so the element-type join used to
/// answer `Optional(bool)` -- a single empty state for two distinct values.
/// Both then lowered to `None::<bool>` and the rows became byte-identical:
/// es-toolkit's `isEqualWith` primitives table answered `true` for its
/// `[null, undefined, false]` row because it generated the same Rust as
/// `[null, null, true]`.
///
/// The element type is `unknown` here by necessity, not convenience -- see
/// `array_literal_mixes_nullish_spellings` for why no concrete type, union, or
/// scoped generic can hold `bool | null | undefined` under the canonical
/// optional/union flattening.
#[test]
fn mixed_nullish_array_literal_keeps_both_spellings() {
    let source = source_for(
        r"
export function pairs(): boolean[] {
  const pairs = [
    [null, undefined, false],
    [undefined, null, false],
  ];
  return pairs.map(pair => pair[0] === pair[1]);
}
",
    );
    assert!(
        !source.contains("vec![None::<bool>, None::<bool>"),
        "`null` and `undefined` must not collapse into the same `Option` empty \
         state:\n{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Null, SmeltUnknown::Undefined")
            && source.contains("SmeltUnknown::Undefined, SmeltUnknown::Null"),
        "each row must keep its own nullish tags in order:\n{source}"
    );
}

/// A literal whose only nullish spelling is uniform still uses `Option`.
///
/// The mixed-spelling boundary above must not widen every nullable literal --
/// one spelling has one empty state, so `Optional(T)` represents it exactly and
/// the concrete element type is kept.
#[test]
fn uniformly_nullish_array_literal_stays_optional() {
    let source = source_for(
        r"
export function rows(): number {
  const row = [1, 2, null];
  return row.length;
}
",
    );
    assert!(
        source.contains("Vec<Option<f64>>"),
        "a single nullish spelling must stay a concrete `Option`:\n{source}"
    );
}

