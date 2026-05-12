//! Split codegen tests chunk.

use super::*;

#[test]
fn emits_typescript_number_to_string_method() {
    let source = source_for(
        r#"
const value = 42;
const text = value.toString();
"#,
    );

    assert!(source.contains(".to_string()"));
}

#[test]
fn emits_typescript_number_parse_float() {
    let source = source_for(
        r#"
const value = Number.parseFloat("42.5");
"#,
    );

    assert!(source.contains(".parse::<f64>().expect(\"float() parse failed\")"));
}

#[test]
fn emits_typescript_number_parse_int() {
    let source = source_for(
        r#"
const value = Number.parseInt("42");
"#,
    );

    assert!(source.contains(".parse::<i64>().expect(\"int() parse failed\") as f64"));
}

#[test]
fn emits_typescript_infinity_identifier() {
    let source = source_for(
        r#"
const upper = Infinity;
const lower = -Infinity;
const missing = NaN;
"#,
    );

    assert!(source.contains("f64::INFINITY"));
    assert!(source.contains("f64::NAN"));
}

#[test]
fn emits_typescript_instanceof_as_class_check() {
    let source = source_for(
        r#"
class Box {
  constructor() {}
}
class Other {
  constructor() {}
}
const value = new Box();
const yes = value instanceof Box;
const no = value instanceof Other;
"#,
    );

    assert!(source.contains(": bool = true;"));
    assert!(source.contains(": bool = false;"));
}

#[test]
fn emits_typescript_global_numeric_parse_calls() {
    let source = source_for(
        r#"
const intValue = parseInt("42");
const floatValue = parseFloat("42.5");
"#,
    );

    assert!(source.contains(".parse::<i64>().expect(\"int() parse failed\") as f64"));
    assert!(source.contains(".parse::<f64>().expect(\"float() parse failed\")"));
}

#[test]
fn emits_number_predicate_calls() {
    let source = source_for(
        r#"
const value = 4;
const finite = Number.isFinite(value);
const nan = Number.isNaN(value);
"#,
    );

    assert!(source.contains(".is_finite();"));
    assert!(source.contains(".is_nan();"));
}

#[test]
fn emits_python_string_search_as_int() {
    let source = source_for_py(
        r#"
word: str = "Smelt"
first: int = word.find("m")
last: int = word.rfind("t")
"#,
    );

    assert!(source.contains(".find(&"));
    assert!(source.contains(".rfind(&"));
    assert!(source.contains(".map_or(-1,"));
}

#[test]
fn emits_python_list_and_string_slices() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2, 3, 4]
all_values: list[int] = values[:]
tail_values: list[int] = values[1:]
mid_values: list[int] = values[1:3]
last_values: list[int] = values[-2:]
word: str = "smelting"
all_text: str = word[:]
tail_text: str = word[1:]
mid_text: str = word[1:4]
last_text: str = word[-3:]
"#,
    );

    assert!(source.contains(".iter().skip(0usize).take("));
    assert!(source.contains("let index = 1 as i64"));
    assert!(source.contains("clamp(0, len) as usize"));
    assert!(source.contains(".cloned().collect::<Vec<_>>();"));
    assert!(source.contains(".chars().skip(0usize).take("));
    assert!(source.matches("if index < 0").count() >= 2);
    assert!(source.contains(".collect::<String>();"));
}

#[test]
fn emits_python_negative_list_and_string_indexes() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2, 3]
last_value: int = values[-1]
word: str = "abc"
last_char: str = word[-1]
"#,
    );

    assert!(source.contains("let normalized = if index < 0 { len + index } else { index }"));
    assert!(source.contains(".get({ let len = values.len() as i64;"));
    assert!(source.contains(".chars().nth({ let len = word.chars().count() as i64;"));
}

#[test]
fn emits_python_tuple_index_and_slice() {
    let source = source_for_py(
        r#"
pair: tuple[str, int] = ("Ada", 1)
name: str = pair[0]
rank: int = pair[-1]
tail: tuple[int] = pair[1:]
empty: tuple[()] = pair[:0]
"#,
    );

    assert!(source.contains(".0.clone()"));
    assert!(source.contains(".1.clone()"));
    assert!(source.contains(".1.clone(),)"));
    assert!(source.contains(": () = ();"));
}

#[test]
fn emits_typescript_tuple_index() {
    let source = source_for(
        r#"
const pair: [string, number] = ["Ada", 1];
const name = pair[0];
const count = pair[1];
"#,
    );

    assert!(source.contains(".0.clone();"));
    assert!(source.contains(".1.clone();"));
}

#[test]
fn emits_python_list_append_method() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2]
result: None = values.append(3)
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains("Vec<i64>"));
    assert!(source.contains(".push(3);"));
    assert!(source.contains("()"));
}

#[test]
fn emits_python_list_extend_method() {
    let source = source_for_py(
        r#"
left: list[int] = [1, 2]
right: list[int] = [3, 4]
result: None = left.extend(right)
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".extend("));
    assert!(source.contains(".iter().cloned());"));
    assert!(source.contains("()"));
}

#[test]
fn emits_python_list_insert_method() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2]
result: None = values.insert(1, 0)
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains("let insert_index = usize::try_from(1)"));
    assert!(source.contains(".insert(insert_index, 0);"));
    assert!(source.contains("()"));
}

#[test]
fn emits_python_list_reverse_method() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2]
result: None = values.reverse()
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".reverse();"));
    assert!(source.contains("()"));
}

#[test]
fn emits_python_list_pop_method() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2]
item: int = values.pop()
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".pop().expect(\"pop from empty list\")"));
}

#[test]
fn emits_python_collection_clear_methods() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2]
list_result: None = values.clear()
mapping: dict[str, int] = {"a": 1}
dict_result: None = mapping.clear()
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.matches(".clear();").count() >= 2);
    assert!(source.matches("()").count() >= 2);
}
