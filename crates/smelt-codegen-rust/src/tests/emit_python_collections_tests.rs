//! Generated Rust for Python dicts, lists and sets.

use super::*;

#[test]
fn emits_python_list_copy_method() {
    let source = source_for_py(
        r"
def run() -> None:
    values: list[int] = [1, 2]
    copied: list[int] = values.copy()
",
    );

    assert_eq!(rust_fn(&source, "run"), r#"fn run() -> () {
let values: SmeltList<i64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<i64> = vec![1, 2]; smelt_list_items }));
let copied: SmeltList<i64> = Into::<SmeltList<_>>::into(values.fresh_copy());
return;
}"#);
}

#[test]
fn emits_python_container_constructors() {
    let source = source_for_py(
        r#"
def run() -> None:
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

    assert_eq!(rust_fn(&source, "run"), r#"fn run() -> () {
let values: SmeltList<i64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<i64> = vec![1, 2]; smelt_list_items }));
let copied_values: SmeltList<i64> = Into::<SmeltList<_>>::into(values.clone().fresh_copy());
let value_tuple: (i64, i64) = { let smelt_tuple_items = values.clone(); if smelt_tuple_items.len() != 2 { panic!("tuple() length mismatch"); } (smelt_tuple_items.borrow()[0].clone(), smelt_tuple_items.borrow()[1].clone()) };
let empty_values: SmeltList<i64> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<i64> = vec![]; smelt_list_items }));
let value_set: SmeltPrimSet<i64> = values.borrow().iter().cloned().collect::<SmeltPrimSet<_>>();
let items: SmeltPrimSet<i64> = SmeltPrimSet::from([1, 2]);
let copied_items: SmeltPrimSet<i64> = items.fresh_copy();
let empty_items: SmeltPrimSet<i64> = SmeltPrimSet::new();
let names: SmeltRecord<String, i64> = SmeltRecord::from([("Ada".to_owned(), 1)]);
let copied_names: SmeltRecord<String, i64> = names.iter().collect::<SmeltRecord<_, _>>();
let empty_names: SmeltRecord<String, i64> = SmeltRecord::from([]);
let _smelt_tmp_29: (String, i64) = ("Ada".to_owned(), 1);
let pairs: SmeltList<(String, i64)> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<(String, i64)> = vec![_smelt_tmp_29.clone()]; smelt_list_items }));
let pair_names: SmeltRecord<String, i64> = pairs.borrow().iter().cloned().collect::<SmeltRecord<String, i64>>();
let item_list: SmeltList<i64> = Into::<SmeltList<_>>::into(items.iter().collect::<Vec<_>>());
let name_keys: SmeltList<String> = Into::<SmeltList<_>>::into(names.keys().collect::<Vec<_>>());
let coords: (i64, i64) = (1, 2);
let coord_list: SmeltList<i64> = Into::<SmeltList<_>>::into(vec![coords.clone().0.clone(), coords.clone().1.clone()]);
let coord_set: SmeltPrimSet<i64> = SmeltPrimSet::from([coords.0.clone(), coords.1.clone()]);
return;
}"#);
}

/// `set(tuple)` builds the container its `set[...]` slot declares. A tuple of
/// `float` is a `set[float]`, which is `SmeltJsSet<f64>` (an `f64` has no
/// `Eq + Hash`, so it cannot key the hashed `SmeltPrimSet`). The `str` tuple
/// beside it keeps the hashed container.
#[test]
fn python_set_of_tuple_builds_the_declared_set_container() {
    let source = source_for_py(
        r#"
def run() -> None:
    coords: tuple[float, float] = (1.5, 2.5)
    coord_set: set[float] = set(coords)
    names: tuple[str, str] = ("a", "b")
    name_set: set[str] = set(names)
"#,
    );

    assert_eq!(rust_fn(&source, "run"), r#"fn run() -> () {
let coords: (f64, f64) = (1.5, 2.5);
let coord_set: SmeltJsSet<f64> = SmeltJsSet::from([coords.0.clone(), coords.1.clone()]);
let names: (String, String) = ("a".to_owned(), "b".to_owned());
let name_set: SmeltPrimSet<String> = SmeltPrimSet::from([names.0.clone(), names.1.clone()]);
return;
}"#);
}

#[test]
fn emits_python_dict_get_method() {
    let source = source_for_py(
        r#"
def run() -> None:
    mapping: dict[str, int] = {"a": 1}
    maybe: int | None = mapping.get("a")
    fallback: int = mapping.get("b", 0)
"#,
    );

    assert_eq!(rust_fn(&source, "run"), r#"fn run() -> () {
let mapping: SmeltRecord<String, i64> = SmeltRecord::from([("a".to_owned(), 1)]);
let maybe: Option<i64> = mapping.get(&"a".to_owned());
let fallback: i64 = mapping.get(&"b".to_owned()).unwrap_or(0);
return;
}"#);
}

#[test]
fn emits_python_dict_setdefault_method() {
    let source = source_for_py(
        r#"
def run() -> None:
    mapping: dict[str, int] = {"a": 1}
    value: int = mapping.setdefault("b", 2)
"#,
    );

    assert_eq!(rust_fn(&source, "run"), r#"fn run() -> () {
let mut mapping: SmeltRecord<String, i64> = SmeltRecord::from([("a".to_owned(), 1)]);
let value: i64 = { if let Some(smelt_value) = mapping.get(&"b".to_owned()) { smelt_value } else { mapping.insert("b".to_owned(), 2.clone()); 2 } };
return;
}"#);
}

#[test]
fn emits_python_dict_copy_method() {
    let source = source_for_py(
        r#"
def run() -> None:
    mapping: dict[str, int] = {"a": 1}
    copied: dict[str, int] = mapping.copy()
"#,
    );

    assert_eq!(rust_fn(&source, "run"), r#"fn run() -> () {
let mapping: SmeltRecord<String, i64> = SmeltRecord::from([("a".to_owned(), 1)]);
let copied: SmeltRecord<String, i64> = mapping.iter().collect::<SmeltRecord<_, _>>();
return;
}"#);
}

#[test]
fn emits_python_ternary_expression() {
    let source = source_for_py(
        r"
def choose(flag: bool, left: int, right: int) -> int:
    return left if flag else right
",
    );

    assert_eq!(rust_fn(&source, "choose"), r#"fn choose(flag: bool, left: i64, right: i64) -> i64 {
let mut _smelt_tmp_3: i64;
let _smelt_tmp_3: i64 = if flag { left } else { right };
return _smelt_tmp_3;
}"#);
}
