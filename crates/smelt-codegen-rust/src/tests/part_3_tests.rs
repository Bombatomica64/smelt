//! Split codegen tests chunk.

use super::*;

#[test]
fn emits_python_list_copy_method() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2]
copied: list[int] = values.copy()
"#,
    );

    assert!(source.contains("let values: SmeltList<i64>"));
    assert!(source.contains(".fresh_copy()"));
}

#[test]
fn emits_python_container_constructors() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2]
copied_values: list[int] = list(values)
value_tuple: tuple[int, int] = tuple(values)
empty_values: list[int] = list()
value_set: set[int] = set(values)
items: set[int] = {1, 2}
copied_items: set[int] = set(items)
empty_items: set[int] = set()
names: dict[str, int] = {"Ada": 1}
copied_names: dict[str, int] = dict(names)
empty_names: dict[str, int] = dict()
pairs: list[tuple[str, int]] = [("Ada", 1)]
pair_names: dict[str, int] = dict(pairs)
item_list: list[int] = list(items)
name_keys: list[str] = list(names)
coords: tuple[int, int] = (1, 2)
coord_list: list[int] = list(coords)
coord_set: set[int] = set(coords)
"#,
    );

    assert!(source.matches(".clone().clone()").count() >= 1);
    assert!(source.contains("vec![]"));
    assert!(source.contains("::std::collections::HashSet::new()"));
    assert!(source.contains("::std::collections::HashMap::from([])"));
    assert!(source.contains(".iter().cloned().collect::<Vec<_>>()"));
    assert!(
        source.contains(
            ".keys().filter(|key| !key.starts_with(\"__smelt_symbol:\")).cloned().collect::<Vec<_>>()"
        ),
        "{source}"
    );
    assert!(source.contains(".iter().cloned().collect::<::std::collections::HashSet<_>>()"));
    assert!(source.contains(".iter().cloned().collect::<::std::collections::HashMap<_, _>>()"));
    assert!(source.contains("panic!(\"tuple() length mismatch\")"));
    assert!(source.contains("vec!["));
    assert!(source.contains("::std::collections::HashSet::from(["));
}

#[test]
fn emits_python_list_count_method() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2, 1]
count: int = values.count(1)
"#,
    );

    assert!(source.contains(".iter().filter(|item| *item == &1).count() as i64;"));
}

#[test]
fn emits_python_list_index_method() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2, 1]
index: int = values.index(2)
"#,
    );

    assert!(source.contains(".iter().position(|item| item == &2)"));
    assert!(source.contains(".expect(\"list index missing item\") as i64;"));
}

#[test]
fn emits_python_list_remove_method() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2, 1]
result: None = values.remove(2)
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".iter().position(|item| item == &2)"));
    assert!(source.contains(".expect(\"list remove missing item\")"));
    assert!(source.contains(".remove(remove_index);"));
    assert!(source.contains("()"));
}

#[test]
fn emits_python_list_sort_method() {
    let source = source_for_py(
        r#"
ints: list[int] = [2, 1]
int_result: None = ints.sort()
floats: list[float] = [2.0, 1.0]
float_result: None = floats.sort()
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".sort();"));
    assert!(source.contains(".sort_by(|left, right| left.partial_cmp(right)"));
    assert!(source.contains(".expect(\"list sort incomparable float\")"));
    assert!(source.matches("()").count() >= 2);
}

#[test]
fn emits_python_dict_pop_method() {
    let source = source_for_py(
        r#"
mapping: dict[str, int] = {"a": 1}
value: int = mapping.pop("a")
fallback: int = mapping.pop("b", 0)
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".remove(&"));
    assert!(source.contains(".expect(\"dict pop missing key\")"));
    assert!(source.contains(".unwrap_or(0)"));
}

#[test]
fn emits_python_dict_get_method() {
    let source = source_for_py(
        r#"
mapping: dict[str, int] = {"a": 1}
maybe: int | None = mapping.get("a")
fallback: int = mapping.get("b", 0)
"#,
    );

    assert!(source.contains("let maybe: Option<i64>"));
    assert!(source.contains(".get(&\"a\".to_owned()).cloned();"));
    assert!(source.contains(".get(&\"b\".to_owned()).cloned().unwrap_or(0);"));
}

#[test]
fn emits_python_dict_setdefault_method() {
    let source = source_for_py(
        r#"
mapping: dict[str, int] = {"a": 1}
value: int = mapping.setdefault("b", 2)
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".entry(\"b\".to_owned())"));
    assert!(source.contains(".or_insert(2)"));
    assert!(source.contains(".clone()"));
}

#[test]
fn emits_python_dict_update_method() {
    let source = source_for_py(
        r#"
left: dict[str, int] = {"a": 1}
right: dict[str, int] = {"b": 2}
result: None = left.update(right)
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains(".extend("));
    assert!(source.contains(".iter().map(|(key, value)| (key.clone(), value.clone()))"));
    assert!(source.contains("()"));
}

#[test]
fn emits_python_dict_copy_method() {
    let source = source_for_py(
        r#"
mapping: dict[str, int] = {"a": 1}
copied: dict[str, int] = mapping.copy()
"#,
    );

    assert!(source.contains("let mapping: ::std::collections::HashMap<String, i64>"));
    assert!(source.contains(".clone();"));
}

#[test]
fn emits_python_string_replace_method() {
    let source = source_for_py(
        r#"
word: str = "hello hello"
replaced: str = word.replace("hello", "hi")
"#,
    );

    assert!(source.contains(".replace(&"));
}

#[test]
fn emits_python_string_remove_affix_methods() {
    let source = source_for_py(
        r#"
word: str = "pre-value-suf"
without_prefix: str = word.removeprefix("pre-")
without_suffix: str = word.removesuffix("-suf")
"#,
    );

    assert!(source.contains(".strip_prefix(&"));
    assert!(source.contains(".strip_suffix(&"));
    assert!(source.contains(".to_owned();"));
}

#[test]
fn emits_python_string_predicate_methods() {
    let source = source_for_py(
        r#"
word: str = "abc123"
digits: bool = word.isdigit()
letters: bool = word.isalpha()
alnum: bool = word.isalnum()
"#,
    );

    assert!(source.contains("char::is_ascii_digit"));
    assert!(source.contains("char::is_alphabetic"));
    assert!(source.contains("char::is_alphanumeric"));
}

#[test]
fn emits_python_string_join_method() {
    let source = source_for_py(
        r#"
parts: list[str] = ["a", "b", "c"]
joined: str = "-".join(parts)
"#,
    );

    assert!(source.contains(".join(&\"-\".to_owned());"));
}

#[test]
fn emits_python_dict_projection_methods() {
    let source = source_for_py(
        r#"
mapping: dict[str, int] = {"a": 1, "b": 2}
keys: list[str] = mapping.keys()
values: list[int] = mapping.values()
items: list[tuple[str, int]] = mapping.items()
"#,
    );

    assert!(
        source.contains(
            ".keys().filter(|key| !key.starts_with(\"__smelt_symbol:\")).cloned().collect::<Vec<_>>()"
        ),
        "{source}"
    );
    assert!(
        source.contains(
            ".iter().filter(|(key, _)| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").map(|(_, value)| value.clone()).collect::<Vec<_>>()"
        ),
        "{source}"
    );
    assert!(
        source.contains(
            ".iter().filter(|(key, _)| !key.starts_with(\"__smelt_symbol:\") && key != \"__smelt_class\").map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>()"
        ),
        "{source}"
    );
}

#[test]
fn emits_python_math_and_contains_helpers() {
    let source = source_for_py(
        r#"
import math
import random
value: float = 4.0
root: float = math.sqrt(value)
raised: float = math.pow(value, 2.0)
angle: float = math.atan2(value, 2.0)
pi_value: float = math.pi
e_value: float = math.e
tau_value: float = math.tau
floored: int = math.floor(value)
ceiled: int = math.ceil(value)
whole: int = math.trunc(value)
finite: bool = math.isfinite(value)
nan_value: bool = math.isnan(value)
sample: float = random.random()
roll: int = random.randint(1, 6)
choices: list[str] = ["a", "b"]
picked: str = random.choice(choices)
values: tuple[int, int] = (1, 2)
has_tuple: bool = 2 in values
unique: set[int] = {1, 2}
has_set: bool = 1 in unique
mapping: dict[str, int] = {"a": 1}
has_key: bool = "a" in mapping
"#,
    );

    assert!(source.contains(".sqrt();"));
    assert!(source.contains(".powf("));
    assert!(source.contains(".atan2("));
    assert!(source.contains("3.141592653589793"));
    assert!(source.contains("2.718281828459045"));
    assert!(source.contains("6.283185307179586"));
    assert!(source.contains(".floor() as i64;"));
    assert!(source.contains(".ceil() as i64;"));
    assert!(source.contains(".trunc() as i64;"));
    assert!(source.contains(".is_finite();"));
    assert!(source.contains(".is_nan();"));
    assert!(source.contains("rand::random::<f64>();"));
    assert!(source.contains("rand::random_range(1..=6);"));
    assert!(source.contains("rand::random_range(0..choice_items.len())"));
    assert!(source.contains("random.choice() from empty list"));
    assert!(source.contains(".0 == "));
    assert!(source.contains("::std::collections::HashSet::from(["));
    assert!(source.contains(".contains(&1)"));
    assert!(source.contains(".contains_key(&"));
}

#[test]
fn emits_python_ternary_expression() {
    let source = source_for_py(
        r#"
def choose(flag: bool, left: int, right: int) -> int:
    return left if flag else right
"#,
    );

    assert!(source.contains("if "));
    assert!(source.contains(" else "));
}
