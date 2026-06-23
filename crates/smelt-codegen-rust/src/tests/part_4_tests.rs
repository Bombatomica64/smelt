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
fn emits_python_map_filter_lambda_callbacks() {
    let source = source_for_py(
        r#"
factor: int = 2
values: list[int] = [1, 2, 3]
scaled: list[int] = list(map(lambda value: value * factor, values))
filtered: list[int] = list(filter(lambda value: value > factor, values))
"#,
    );

    assert!(source.contains(".iter().enumerate().map("));
    assert!(source.contains("* factor"));
    assert!(source.contains(".iter().enumerate().filter_map("));
    assert!(source.contains("> factor"));
}

#[test]
fn emits_python_first_class_closure_values() {
    let source = source_for_py(
        r#"
from typing import Callable

offset: int = 2
add_offset: Callable[[int], int] = lambda value: value + offset

def apply(value: int, fn: Callable[[int], int]) -> int:
    return fn(value)

def make_adder(base: int) -> Callable[[int], int]:
    add: Callable[[int], int] = lambda value: value + base
    return add

direct: int = add_offset(3)
passed: int = apply(4, add_offset)
adder: Callable[[int], int] = make_adder(5)
returned: int = adder(6)
"#,
    );

    assert!(source.contains("Rc<dyn Fn(i64) -> i64>"));
    assert!(source.contains("(3)"));
    assert!(source.contains("apply(4,"));
    assert!(source.contains("make_adder(5)"));
    assert!(source.contains("(adder)(6)"));
    assert!(source.contains("move |"));
}

#[test]
fn emits_nested_loop_inside_loop_branch_as_structured_loop() {
    let source = source_for(
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
    );

    assert!(
        source.matches("loop {").count() >= 2,
        "expected outer while and nested for-of to both emit structured loops:\n{source}"
    );
}

#[test]
fn keeps_continuation_after_short_circuit_call_guard() {
    let source = source_for(
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
    );

    assert!(source.contains("return f64::NAN;"), "{source}");
    assert!(source.contains("result.clone() + 1.0"), "{source}");
    assert!(source.contains("return result.clone();"), "{source}");
}

#[test]
fn emits_postfix_index_update_inside_while_loop_body() {
    let source = source_for(
        r#"
export function read(values: number[]): number[] {
  const result: number[] = [];
  let index = 0;
  while (index < values.length) {
    const value = values[index++];
    result.push(value);
  }
  return result;
}
"#,
    );

    let loop_start = source.find("loop {").expect("expected emitted loop");
    let increment = source[loop_start..]
        .find("index = ")
        .map(|offset| loop_start + offset)
        .expect("expected postfix increment in loop");
    let value_binding = source[loop_start..]
        .find("value = values.get")
        .map(|offset| loop_start + offset)
        .expect("expected indexed value binding in loop");
    assert!(
        !source[..loop_start].contains("index = ")
            && increment > value_binding
            && increment > loop_start,
        "postfix update escaped the loop body:\n{source}"
    );
}

#[test]
fn emits_do_while_value_iteration_without_defaulting_after_recursive_expansion() {
    let source = source_for(
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
    );

    assert!(source.contains("loop {"), "{source}");
    assert!(
        !source
            .contains("Smelt could not structurally emit this recursive control-flow region yet"),
        "{source}"
    );
    assert!(!source.contains("return Default::default()"), "{source}");
}

#[test]
fn emits_python_default_lambda_closure_values() {
    let source = source_for_py(
        r#"
from typing import Callable

bump: Callable[[int], int] = lambda value=1: value + 1
defaulted: int = bump()
explicit: int = bump(4)
"#,
    );

    assert!(source.contains("(1)"));
    assert!(source.contains("(4)"));
}

#[test]
fn emits_python_nested_def_closure_values() {
    let source = source_for_py(
        r#"
from typing import Callable

def make_adder(base: int) -> Callable[[int], int]:
    def add(value: int) -> int:
        return value + base
    return add

adder: Callable[[int], int] = make_adder(5)
result: int = adder(6)
"#,
    );

    assert!(source.contains("move |"));
    assert!(source.contains("make_adder(5)"));
    assert!(source.contains("(adder)(6)"));
}

#[test]
fn emits_python_vararg_lambda_closure_values() {
    let source = source_for_py(
        r#"
from typing import Callable

sum_two: Callable[[int, int], int] = lambda *values: values[0] + values[1]
result: int = sum_two(2, 3)
"#,
    );

    assert!(source.contains("|closure_arg_0: i64, closure_arg_1: i64|"));
    assert!(source.contains("vec![closure_arg_0.clone(), closure_arg_1.clone()]"));
    assert!(
        source.contains("usize::try_from(normalized).expect(\"negative index out of bounds\")")
    );
    assert!(source.contains("(2, 3)"));
}

#[test]
fn emits_python_nested_def_vararg_and_kwarg_closure_values() {
    let source = source_for_py(
        r#"
def run() -> int:
    def sum_values(*values: int) -> int:
        return values[0] + values[1]

    def pick(**kwargs: int) -> int:
        return kwargs["value"] + kwargs["bonus"]

    total: int = sum_values(2, 3, 4)
    extras: dict[str, int] = {"bonus": 6}
    chosen: int = pick(value=5, **extras)
    return total + chosen
"#,
    );

    assert!(source.contains("|closure_arg_0: Vec<i64>|"));
    assert!(
        source.contains("usize::try_from(normalized).expect(\"negative index out of bounds\")")
    );
    assert!(source.contains("vec![2, 3, 4]"));
    assert!(
        source.contains(
            "closure_arg_0.get(&\"value\".to_owned()).cloned().expect(\"missing field\")"
        )
    );
    assert!(source.contains("::std::collections::HashMap::from([(\"value\".to_owned(), 5)])"));
    assert!(source.contains("assigned.extend"));
}

#[test]
fn emits_python_top_level_vararg_and_kwarg_functions() {
    let source = source_for_py(
        r#"
def sum_values(*values: int) -> int:
    return values[0] + values[1]

def pick(**kwargs: int) -> int:
    return kwargs["value"] + kwargs["bonus"]

total: int = sum_values(2, 3, 4)
extras: dict[str, int] = {"bonus": 6}
chosen: int = pick(value=5, **extras)
"#,
    );

    assert!(source.contains("fn sum_values(values: Vec<i64>) -> i64"));
    assert!(source.contains("vec![2, 3, 4]"));
    assert!(source.contains("fn pick(kwargs: ::std::collections::HashMap<String, i64>) -> i64"));
    assert!(source.contains("::std::collections::HashMap::from([(\"value\".to_owned(), 5)])"));
    assert!(source.contains("assigned.extend"));
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
fn emits_python_sorted_key_and_reverse() {
    let source = source_for_py(
        r#"
bias: int = 10
values: list[int] = [2, 1]
ordered: list[int] = sorted(values, key=lambda value: value + bias, reverse=True)
"#,
    );

    assert!(source.contains("let mut smelt_sort_key ="));
    assert!(source.contains("let left_key = (smelt_sort_key)"));
    assert!(source.contains("right_key.cmp(&left_key)"));
}

#[test]
fn emits_python_list_sort_key() {
    let source = source_for_py(
        r#"
values: list[int] = [2, 1]
ordered: None = values.sort(key=lambda value: value * 2)
"#,
    );

    assert!(source.contains("let mut smelt_sort_key ="));
    assert!(source.contains(".sort_by(|left, right|"));
    assert!(source.contains("left_key.cmp(&right_key)"));
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
    assert!(
        source.contains(
            ".keys().filter(|key| !key.starts_with(\"__smelt_symbol:\")).cloned().collect::<Vec<_>>()"
        ),
        "{source}"
    );
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
    assert!(
        source.contains(
            ".keys().filter(|key| !key.starts_with(\"__smelt_symbol:\")).cloned().collect::<Vec<_>>()"
        ),
        "{source}"
    );
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
fn emits_untyped_json_parse_as_unknown_record() {
    let ts_source = source_for(
        r#"
const text = "{\"enabled\":true}";
const values = JSON.parse(text);
"#,
    );

    assert!(
        ts_source.contains("serde_json::from_str::<SmeltRecord<String, SmeltUnknown>>(&"),
        "{ts_source}"
    );
    assert!(ts_source.contains("impl<'de> serde::Deserialize<'de> for SmeltUnknown"));
}

#[test]
fn emits_json_parse_assertion_targets() {
    let ts_source = source_for(
        r#"
interface ServerConfig {
  host: string;
  port: number;
}

const text = "{\"host\":\"localhost\",\"port\":1337}";
const config = JSON.parse(text) as ServerConfig;
const bag = JSON.parse(text) as Record<string, unknown>;
"#,
    );

    assert!(
        ts_source
            .contains("#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]")
    );
    assert!(
        ts_source.contains("serde_json::from_str::<ServerConfig>(&")
            || ts_source.contains("serde_json::from_str::<SmeltUnknown>(&")
    );
    assert!(
        ts_source.contains("serde_json::from_str::<SmeltRecord<String, SmeltUnknown>>(&"),
        "{ts_source}"
    );
    assert!(ts_source.contains("impl<'de> serde::Deserialize<'de> for SmeltUnknown"));
}

#[test]
fn emits_regex_match_calls() {
    let ts_source = source_for(
        r#"
const text = "abc123";
const pattern = "\\d+";
const hasDigits = new RegExp(pattern).test(text);
const alsoHasDigits = RegExp(pattern).test(text);
const literalHasDigits = /\d+/.test(text);
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

    assert!(
        source.contains("item.is_nan() && smelt_needle.is_nan()"),
        "{source}"
    );
}

#[test]
fn emits_set_constructor_and_has_method() {
    let source = source_for(
        r#"
const values: Set<number> = new Set([1, 2, 3]);
const has = values.has(2);
const empty: Set<string> = new Set();
const source: number[] = [1, 2, 3];
const fromSource = new Set(source);
"#,
    );

    assert!(source.contains("let values: Vec<f64>"), "{source}");
    assert!(
        source.contains("let empty: ::std::collections::HashSet<String>"),
        "{source}"
    );
    assert!(source.contains(".iter().any(|value| *value == 2.0);"));
    assert!(source.contains("::std::collections::HashSet::new();"));
    assert!(source.contains(".iter().cloned().collect::<::std::collections::HashSet<_>>()"));
}

#[test]
fn emits_set_has_inside_callback() {
    let source = source_for(
        r#"
const selected: Set<number> = new Set([1, 2]);
const values: number[] = [1, 2, 3];
const hits = values.filter((value) => selected.has(value));
"#,
    );

    assert!(
        source.contains(".iter().any(|value| *value == closure_arg_0.clone())"),
        "{source}"
    );
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
    assert!(source.contains("loop {"));
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
            ".iter().filter(|(key, _)| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>()"
        ),
        "{source}"
    );
    assert!(source.contains("loop {"));
    assert!(source.contains("entry ="));
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
    assert!(
        source.contains(
            ".keys().filter(|key| !key.starts_with(\"__smelt_symbol:\")).cloned().collect::<Vec<_>>()"
        ),
        "{source}"
    );
    assert!(source.matches("loop {").count() >= 2);
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
