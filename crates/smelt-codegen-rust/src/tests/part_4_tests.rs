//! Split codegen tests chunk.

use super::*;

#[test]
fn emits_python_sum_builtin() {
    let source = source_for_py(
        r#"
ints: list[int] = [1, 2]
int_total: int = sum(ints)
floats: list[float] = [1.0, 2.0]
float_total: float = sum(floats)
"#,
    );

    assert!(source.contains(".iter().copied().sum::<i64>()"));
    assert!(source.contains(".iter().copied().sum::<f64>()"));
}

#[test]
fn emits_python_all_any_builtins() {
    let source = source_for_py(
        r#"
values: list[bool] = [True, False]
all_values: bool = all(values)
any_values: bool = any(values)
"#,
    );

    assert!(source.contains(".iter().copied().all(|value| value)"));
    assert!(source.contains(".iter().copied().any(|value| value)"));
}

#[test]
fn emits_python_sorted_builtin() {
    let source = source_for_py(
        r#"
ints: list[int] = [2, 1]
ordered_ints: list[int] = sorted(ints)
floats: list[float] = [2.0, 1.0]
ordered_floats: list[float] = sorted(floats)
"#,
    );

    assert!(source.contains(".clone(); sorted.sort(); sorted"));
    assert!(source.contains(
        ".clone(); sorted.sort_by(|left, right| left.partial_cmp(right).expect(\"sorted incomparable float\")); sorted"
    ));
}

#[test]
fn emits_python_reversed_builtin() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2]
flipped: list[int] = reversed(values)
"#,
    );

    assert!(source.contains(".iter().rev().cloned().collect::<Vec<_>>()"));
}

#[test]
fn emits_python_enumerate_builtin() {
    let source = source_for_py(
        r#"
values: list[str] = ["a", "b"]
pairs: list[tuple[int, str]] = enumerate(values)
names: dict[str, int] = {"Ada": 1}
name_pairs: list[tuple[int, str]] = enumerate(names)
items: set[int] = {1, 2}
item_pairs: list[tuple[int, int]] = enumerate(items)
"#,
    );

    assert!(source.contains(".iter().cloned().enumerate()"));
    assert!(source.contains("idx as i64"));
    assert!(source.contains(".keys().cloned().collect::<Vec<_>>()"));
    assert!(source.contains(".iter().cloned().collect::<Vec<_>>()"));
}

#[test]
fn emits_python_zip_builtin() {
    let source = source_for_py(
        r#"
names: list[str] = ["Ada", "Linus"]
scores: list[int] = [1, 2]
pairs: list[tuple[str, int]] = zip(names, scores)
lookup: dict[str, int] = {"Ada": 1}
items: set[int] = {1, 2}
mixed: list[tuple[str, int]] = zip(lookup, items)
"#,
    );

    assert!(source.contains(".iter().cloned().zip("));
    assert!(source.contains(".keys().cloned().collect::<Vec<_>>()"));
    assert!(source.contains(".iter().cloned().collect::<Vec<_>>()"));
}

#[test]
fn emits_python_primitive_conversions() {
    let source = source_for_py(
        r#"
flag: bool = True
digits: str = "42"
ratio_text: str = "2.5"
as_text: str = str(flag)
as_int: int = int(3.8)
parsed_int: int = int(digits)
as_float: float = float(7)
parsed_float: float = float(ratio_text)
as_bool: bool = bool("")
"#,
    );

    assert!(source.contains("\"True\".to_owned()"));
    assert!(source.contains(".trunc() as i64"));
    assert!(source.contains(".parse::<i64>().expect(\"int() parse failed\")"));
    assert!(source.contains("as f64"));
    assert!(source.contains(".parse::<f64>().expect(\"float() parse failed\")"));
    assert!(source.contains(".is_empty()"));
}

#[test]
fn emits_python_range_builtin() {
    let source = source_for_py(
        r#"
first: list[int] = range(3)
middle: list[int] = range(1, 4)
stepped: list[int] = range(5, 1, -2)
total: int = 0
for value in range(3):
    total = total + value
"#,
    );

    assert!(source.contains("let mut values = Vec::new();"));
    assert!(source.contains("while current < end"));
    assert!(source.contains("while current > end"));
    assert!(source.contains("panic!(\"range() arg 3 must not be zero\")"));
}

#[test]
fn emits_json_stringify_calls() {
    let ts_source = source_for(
        r#"
const values: number[] = [1, 2];
const text = JSON.stringify(values);
"#,
    );
    let py_source = source_for_py(
        r#"
import json
values: list[int] = [1, 2]
text: str = json.dumps(values)
"#,
    );

    assert!(ts_source.contains("serde_json::to_string(&"));
    assert!(py_source.contains("serde_json::to_string(&"));
    assert!(ts_source.contains(".expect(\"JSON serialization failed\")"));
    assert!(py_source.contains(".expect(\"JSON serialization failed\")"));
}

#[test]
fn emits_json_parse_calls() {
    let ts_source = source_for(
        r#"
const text = "[1,2]";
const values = JSON.parse<number[]>(text);
"#,
    );
    let py_source = source_for_py(
        r#"
import json
text: str = "[1,2]"
values: list[int] = json.loads(text)
"#,
    );

    assert!(ts_source.contains("serde_json::from_str::<Vec<f64>>(&"));
    assert!(py_source.contains("serde_json::from_str::<Vec<i64>>(&"));
    assert!(ts_source.contains(".expect(\"JSON parse failed\")"));
    assert!(py_source.contains(".expect(\"JSON parse failed\")"));
}

#[test]
fn emits_regex_match_calls() {
    let ts_source = source_for(
        r#"
const text = "abc123";
const pattern = "\\d+";
const hasDigits = new RegExp(pattern).test(text);
"#,
    );
    let py_source = source_for_py(
        r#"
import re
text: str = "abc123"
pattern: str = "\\d+"
found: bool = re.search(pattern, text)
starts: bool = re.match(pattern, text)
full: bool = re.fullmatch(pattern, text)
"#,
    );

    assert!(ts_source.contains("regex::Regex::new(&"));
    assert!(ts_source.contains(".is_match(&"));
    assert!(py_source.contains(".is_match(&"));
    assert!(py_source.contains(".find(&"));
    assert!(py_source.contains("m.start() == 0"));
    assert!(py_source.contains("m.end() =="));
}

#[test]
fn emits_string_includes_method() {
    let source = source_for(
        r#"
const word = "Smelt";
const has = word.includes("mel");
"#,
    );

    assert!(source.contains(".contains(&\"mel\".to_owned());"));
}

#[test]
fn emits_array_includes_method() {
    let source = source_for(
        r#"
const values: number[] = [1, 2, 3];
const has = values.includes(2);
"#,
    );

    assert!(source.contains(".contains(&2.0);"));
}

#[test]
fn emits_set_constructor_and_has_method() {
    let source = source_for(
        r#"
const values: Set<number> = new Set([1, 2, 3]);
const has = values.has(2);
const empty: Set<string> = new Set();
"#,
    );

    assert!(source.contains("::std::collections::HashSet<f64>"));
    assert!(source.contains("::std::collections::HashSet::from(["));
    assert!(source.contains(".contains(&2.0);"));
    assert!(source.contains("::std::collections::HashSet::from([]);"));
}

#[test]
fn emits_set_for_of_iteration() {
    let source = source_for(
        r#"
const values: Set<number> = new Set([1, 2]);
let total = 0;
for (let item: number of values) {
  total = total + item;
}
"#,
    );

    assert!(source.contains(".iter().cloned().collect::<Vec<_>>()"));
    assert!(source.contains("while"));
    assert!(source.contains("total ="));
}

#[test]
fn emits_map_for_of_iteration() {
    let source = source_for(
        r#"
const mapping: Map<string, number> = new Map([["a", 1], ["b", 2]]);
let last: [string, number] = ["", 0];
for (const entry: [string, number] of mapping) {
  last = entry;
}
"#,
    );

    assert!(
        source.contains(
            ".iter().map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>()"
        )
    );
    assert!(source.contains("while"));
    assert!(source.contains("last = entry.clone();"));
}

#[test]
fn emits_python_set_and_dict_for_loops() {
    let source = source_for_py(
        r#"
items: set[int] = {1, 2}
total: int = 0
for item in items:
    total = total + item
names: dict[str, int] = {"Ada": 1}
last: str = ""
for name in names:
    last = name
"#,
    );

    assert!(source.contains(".iter().cloned().collect::<Vec<_>>()"));
    assert!(source.contains(".keys().cloned().collect::<Vec<_>>()"));
    assert!(source.matches("while").count() >= 2);
    assert!(source.contains("total ="));
    assert!(source.contains("last = name.clone();"));
}

#[test]
fn emits_python_pytest_function_as_rust_test() {
    let source = source_for_py_path(
        r#"
def test_truth():
    assert True
"#,
        "tests/test_truth.py",
    );

    assert!(
        source.contains("#[test]\nfn test_truth() -> Result<(), Box<dyn std::error::Error>> {")
    );
    assert!(source.contains("return Err(std::io::Error::new"));
    assert!(source.contains("return Ok(());"));
}
