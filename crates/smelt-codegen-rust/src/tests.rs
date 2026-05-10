use super::*;
use smelt_frontend_py as py_frontend;
use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;

/// Converts TypeScript source to generated Rust source.
fn source_for(ts: &str) -> String {
    let mut ctx = HirCtx::new();
    assert!(to_hir(ts, FileId(0), &mut ctx).is_ok(), "HIR");
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);
    match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    }
}

/// Converts Python source to generated Rust source.
fn source_for_py(py: &str) -> String {
    let mut ctx = py_frontend::HirCtx::new();
    assert!(py_frontend::to_hir(py, FileId(0), &mut ctx).is_ok(), "HIR");
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);
    match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    }
}

/// Converts Python source at `path` to generated Rust source.
fn source_for_py_path(py: &str, path: &str) -> String {
    let mut ctx = py_frontend::HirCtx::new();
    assert!(
        py_frontend::to_hir_with_path(py, FileId(0), path, &mut ctx).is_ok(),
        "HIR"
    );
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);
    match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    }
}

#[test]
fn emits_main_with_console_log() {
    let source = source_for("let count = 42;\nconsole.log(count);\n");

    assert!(source.contains("fn main() {"));
    assert!(source.contains("let count: f64 = 42.0;"));
    assert!(source.contains("let _smelt_tmp_1: () = { println!(\"{}\", count.clone()); };"));
}

#[test]
fn emits_owned_string_literals() {
    let source = source_for("const message = \"hello smelt\";\nconsole.log(message);\n");

    assert!(source.contains("let message: String = \"hello smelt\".to_owned();"));
}

#[test]
fn emits_direct_length_lowering() {
    let source = source_for(
        r#"
const values: number[] = [1, 2, 3];
const count = values.length;
const word = "smelt";
const letters = word.length;
"#,
    );

    assert!(source.contains(".len() as f64;"));
    assert!(source.contains(".chars().count() as f64;"));
}

#[test]
fn emits_math_abs_call() {
    let source = source_for(
        r#"
const value = -5;
const positive = Math.abs(value);
"#,
    );

    assert!(source.contains(".abs();"));
}

#[test]
fn emits_math_rounding_calls() {
    let source = source_for(
        r#"
const value = 5.5;
const floor = Math.floor(value);
const ceil = Math.ceil(value);
const round = Math.round(value);
const trunc = Math.trunc(value);
"#,
    );

    assert!(source.contains(".floor();"));
    assert!(source.contains(".ceil();"));
    assert!(source.contains(".round();"));
    assert!(source.contains(".trunc();"));
}

#[test]
fn emits_math_extrema_calls() {
    let source = source_for(
        r#"
const first = 1;
const second = 2;
const highest = Math.max(first, second, 3);
const lowest = Math.min(first, second, -1);
"#,
    );

    assert!(source.contains(".max("));
    assert!(source.contains(".min("));
}

#[test]
fn emits_array_callback_methods() {
    let source = source_for(
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
"#,
    );

    assert!(source.contains(".iter().map(|item|"));
    assert!(source.contains(".iter().filter(|item|"));
    assert!(source.contains(".iter().find(|item|"));
    assert!(source.contains(".iter().position(|item|"));
    assert!(source.contains(".iter().any(|item|"));
    assert!(source.contains(".iter().all(|item|"));
    assert!(source.contains(".iter().for_each(|item|"));
    assert!(source.contains(".iter().fold("));
    assert!(source.contains(".collect::<Vec<_>>()"));
    assert!(source.contains(".map_or(-1.0"));
}

#[test]
fn emits_string_index_and_for_of() {
    let source = source_for(
        r#"
const word = "abc";
const first = word[0];
const last = word.at(-1);
let joined = "";
for (let ch: string of word) {
  joined = joined + ch;
}
"#,
    );

    assert!(source.contains("let normalized = if index < 0 { len + index } else { index }"));
    assert!(source.contains(".chars().nth({ let len = word.chars().count() as i64;"));
    assert!(source.contains(".chars().count() as f64"));
    assert!(source.contains("let index = _smelt_tmp_"));
}

#[test]
fn emits_string_case_methods() {
    let source = source_for(
        r#"
const word = "Smelt";
const lower = word.toLowerCase();
const upper = word.toUpperCase();
"#,
    );

    assert!(source.contains(".to_lowercase();"));
    assert!(source.contains(".to_uppercase();"));
}

#[test]
fn emits_string_trim_method() {
    let source = source_for(
        r#"
const word = " Smelt ";
const trimmed = word.trim();
const left = word.trimStart();
const right = word.trimEnd();
"#,
    );

    assert!(source.contains(".trim().to_owned();"));
    assert!(source.contains(".trim_start().to_owned();"));
    assert!(source.contains(".trim_end().to_owned();"));
}

#[test]
fn emits_string_prefix_suffix_methods() {
    let source = source_for(
        r#"
const word = "Smelt";
const starts = word.startsWith("Sm");
const ends = word.endsWith("lt");
"#,
    );

    assert!(source.contains(".starts_with(&"));
    assert!(source.contains(".ends_with(&"));
}

#[test]
fn emits_string_search_methods() {
    let source = source_for(
        r#"
const word = "Smelt";
const first = word.indexOf("m");
const last = word.lastIndexOf("t");
"#,
    );

    assert!(source.contains(".find(&"));
    assert!(source.contains(".rfind(&"));
    assert!(source.contains(".map_or(-1.0"));
}

#[test]
fn emits_string_replace_method() {
    let source = source_for(
        r#"
const word = "hello hello";
const replaced = word.replace("hello", "hi");
"#,
    );

    assert!(source.contains(".replacen(&"));
    assert!(source.contains(", 1);"));
}

#[test]
fn emits_string_repeat_method() {
    let source = source_for(
        r#"
const word = "ha";
const repeated = word.repeat(3);
"#,
    );

    assert!(source.contains(".repeat(3.0 as usize);"));
}

#[test]
fn emits_string_padding_methods() {
    let source = source_for(
        r#"
const word = "7";
const paddedStart = word.padStart(3, "0");
const paddedEnd = word.padEnd(3);
"#,
    );

    assert!(source.contains("pad.chars().cycle().take(needed).collect()"));
    assert!(source.contains("format!(\"{}{}\", padding, value)"));
    assert!(source.contains("format!(\"{}{}\", value, padding)"));
}

#[test]
fn emits_string_char_at_method() {
    let source = source_for(
        r#"
const word = "Smelt";
const char = word.charAt(1);
const code = word.charCodeAt(2);
"#,
    );

    assert!(
        source.contains(".chars().nth(1.0 as usize).map(|ch| ch.to_string()).unwrap_or_default();")
    );
    assert!(source.contains(".chars().nth(2.0 as usize).map_or(f64::NAN, |ch| ch as u32 as f64);"));
}

#[test]
fn emits_math_sqrt_pow_sign() {
    let source = source_for(
        r#"
const value = 4;
const root = Math.sqrt(value);
const cubeRoot = Math.cbrt(value);
const raised = Math.pow(value, 2);
const signed = Math.sign(value);
const sine = Math.sin(value);
const cosine = Math.cos(value);
const tangent = Math.tan(value);
const arcsine = Math.asin(value);
const arccosine = Math.acos(value);
const arctangent = Math.atan(value);
const arctangentTwo = Math.atan2(value, 2);
const logged = Math.log(value);
const logTen = Math.log10(value);
const logTwo = Math.log2(value);
const exponent = Math.exp(value);
const distance = Math.hypot(value, 3);
const sample = Math.random();
"#,
    );

    assert!(source.contains(".sqrt();"));
    assert!(source.contains(".cbrt();"));
    assert!(source.contains(".powf("));
    assert!(source.contains(".signum();"));
    assert!(source.contains(".sin();"));
    assert!(source.contains(".cos();"));
    assert!(source.contains(".tan();"));
    assert!(source.contains(".asin();"));
    assert!(source.contains(".acos();"));
    assert!(source.contains(".atan();"));
    assert!(source.contains(".atan2("));
    assert!(source.contains(".ln();"));
    assert!(source.contains(".log10();"));
    assert!(source.contains(".log2();"));
    assert!(source.contains(".exp();"));
    assert!(source.contains("0.0f64.hypot("));
    assert!(source.contains(".hypot(3.0);"));
    assert!(source.contains("rand::random::<f64>();"));
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

#[test]
fn emits_python_list_copy_method() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2]
copied: list[int] = values.copy()
"#,
    );

    assert!(source.contains("let values: Vec<i64>"));
    assert!(source.contains(".clone();"));
}

#[test]
fn emits_python_container_constructors() {
    let source = source_for_py(
        r#"
values: list[int] = [1, 2]
copied_values: list[int] = list(values)
empty_values: list[int] = list()
value_set: set[int] = set(values)
items: set[int] = {1, 2}
copied_items: set[int] = set(items)
empty_items: set[int] = set()
names: dict[str, int] = {"Ada": 1}
copied_names: dict[str, int] = dict(names)
empty_names: dict[str, int] = dict()
item_list: list[int] = list(items)
name_keys: list[str] = list(names)
coords: tuple[int, int] = (1, 2)
coord_list: list[int] = list(coords)
coord_set: set[int] = set(coords)
"#,
    );

    assert!(source.matches(".clone().clone()").count() >= 3);
    assert!(source.contains("vec![]"));
    assert!(source.contains("::std::collections::HashSet::from([])"));
    assert!(source.contains("::std::collections::HashMap::from([])"));
    assert!(source.contains(".iter().cloned().collect::<Vec<_>>()"));
    assert!(source.contains(".keys().cloned().collect::<Vec<_>>()"));
    assert!(source.contains(".iter().cloned().collect::<::std::collections::HashSet<_>>()"));
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

    assert!(source.contains(".keys().cloned().collect::<Vec<_>>();"));
    assert!(source.contains(".values().cloned().collect::<Vec<_>>();"));
    assert!(
        source.contains(
            ".iter().map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>();"
        )
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
floored: int = math.floor(value)
ceiled: int = math.ceil(value)
whole: int = math.trunc(value)
finite: bool = math.isfinite(value)
nan_value: bool = math.isnan(value)
sample: float = random.random()
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
    assert!(source.contains(".floor() as i64;"));
    assert!(source.contains(".ceil() as i64;"));
    assert!(source.contains(".trunc() as i64;"));
    assert!(source.contains(".is_finite();"));
    assert!(source.contains(".is_nan();"));
    assert!(source.contains("rand::random::<f64>();"));
    assert!(source.contains(".0 == "));
    assert!(source.contains("::std::collections::HashSet::from(["));
    assert!(source.contains(".contains(&1)"));
    assert!(source.contains(".contains_key(&"));
}

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
"#,
    );

    assert!(source.contains(".split(&\",\".to_owned()).map(str::to_owned).collect::<Vec<_>>();"));
}

#[test]
fn emits_array_join_method() {
    let source = source_for(
        r#"
const words: string[] = ["a", "b", "c"];
const joined = words.join("-");
const comma = words.join();
"#,
    );

    assert!(source.contains(".join(&\"-\".to_owned());"));
    assert!(source.contains(".join(&\",\".to_owned());"));
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
"#,
    );

    assert!(source.contains(".iter().skip(0usize).take("));
    assert!(source.contains("let index = 1.0 as i64"));
    assert!(source.contains("clamp(0, len) as usize"));
    assert!(source.contains(".cloned().collect::<Vec<_>>();"));
    assert!(source.contains(".chars().skip(0usize).take("));
    assert!(source.matches("if index < 0").count() >= 2);
    assert!(source.contains(".collect::<String>();"));
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
fn emits_array_pop_method() {
    let source = source_for(
        r#"
let values: number[] = [1, 2];
values.pop();
const item = values.pop();
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains("Option<f64>"));
    assert!(source.contains(".pop();"));
}

#[test]
fn emits_array_shift_method() {
    let source = source_for(
        r#"
let values: string[] = ["a", "b"];
values.shift();
const item = values.shift();
"#,
    );

    assert!(source.contains("let mut"));
    assert!(source.contains("Option<String>"));
    assert!(source.contains(".is_empty()"));
    assert!(source.contains("Some("));
    assert!(source.contains(".remove(0)"));
}

#[test]
fn emits_array_is_array_as_static_boolean() {
    let source = source_for(
        r#"
const values: number[] = [1, 2, 3];
const yes = Array.isArray(values);
const no = Array.isArray(1);
"#,
    );

    assert!(source.contains(" = true;"));
    assert!(source.contains(" = false;"));
}

#[test]
fn emits_object_projection_methods() {
    let source = source_for(
        r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const keys = Object.keys(mapping);
const values = Object.values(mapping);
const entries = Object.entries(mapping);
"#,
    );

    assert!(source.contains(".keys().cloned().collect::<Vec<_>>();"));
    assert!(source.contains(".values().cloned().collect::<Vec<_>>();"));
    assert!(
        source.contains(
            ".iter().map(|(key, value)| (key.clone(), value.clone())).collect::<Vec<_>>();"
        )
    );
}

#[test]
fn emits_object_has_own_methods() {
    let source = source_for(
        r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const first = Object.hasOwn(mapping, "a");
const second = mapping.hasOwnProperty("b");
"#,
    );

    assert!(source.contains(".contains_key(&"));
}

#[test]
fn emits_static_function_with_params_and_return_value() {
    let source = source_for(
        "function add(a: number, b: number): number {
  return a + b;
}
const result = add(2, 3);
console.log(result);
",
    );

    assert!(source.contains("fn add(arg_0: f64, arg_1: f64) -> f64 {"));
    assert!(source.contains("arg_0.clone() + arg_1.clone()"));
    assert!(source.contains("let _smelt_tmp_1: f64 = add(2.0, 3.0);"));
}

#[test]
fn emits_async_function_and_await() {
    let source = source_for(
        "async function lift(value: number): Promise<number> {
  return value;
}

async function run(): Promise<number> {
  return await lift(5);
}
",
    );

    assert!(source.contains("async fn lift(arg_0: f64) -> f64 {"));
    assert!(source.contains("async fn run() -> f64 {"));
    assert!(source.contains("let _smelt_tmp_0 = lift(5.0);"));
    assert!(source.contains("let _smelt_tmp_1: f64 = _smelt_tmp_0.await;"));
}

#[test]
fn emits_promise_all_with_tokio_join() {
    let source = source_for(
        "async function lift(value: number): Promise<number> {
  return value;
}

async function run(): Promise<[number, number]> {
  return await Promise.all([lift(1), lift(2)]);
}
",
    );

    assert!(source.contains("async fn run() -> (f64, f64) {"));
    assert!(source.contains("tokio::join!(_smelt_tmp_0, _smelt_tmp_1)"));
    assert!(
            source.contains(
                "let _smelt_tmp_2: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = (f64, f64)>>> = Box::pin(async move { tokio::join!(_smelt_tmp_0, _smelt_tmp_1) });"
            )
        );
}

#[test]
fn emits_if_else_control_flow() {
    let source = source_for(
        "function max(a: number, b: number): number {
  if (a > b) {
    return a;
  }
  return b;
}
const result = max(2, 3);
console.log(result);
",
    );

    assert!(source.contains("if _smelt_tmp_2.clone() {"));
    assert!(source.contains("return arg_0.clone();"));
    assert!(source.contains("return arg_1.clone();"));
}

#[test]
fn emits_switch_as_rust_match() {
    let source = source_for(
        "function label(status: \"pending\" | \"approved\" | \"rejected\"): string {
  switch (status) {
    case \"pending\":
      return \"Waiting\";
    case \"approved\":
      return \"Approved\";
    case \"rejected\":
      return \"Rejected\";
  }
}
const result = label(\"approved\");
console.log(result);
",
    );

    assert!(source.contains("match arg_0.as_str() {"));
    assert!(source.contains("\"pending\" => {"));
    assert!(source.contains("return \"Waiting\".to_owned();"));
    assert!(source.contains("_ => unreachable!(),"));
}

#[test]
fn emits_uncaught_throw_as_result() {
    let source = source_for(
        "function fail(): void {
  throw \"boom\";
}
fail();
",
    );

    assert!(source.contains("fn fail() -> Result<(), Box<dyn std::error::Error>> {"));
    assert!(source.contains("return Err(std::io::Error::new("));
    assert!(source.contains("fn main() -> Result<(), Box<dyn std::error::Error>> {"));
    assert!(source.contains("let _smelt_tmp_0: () = fail()?;"));
    assert!(source.contains("return Ok(());"));
}

#[test]
fn emits_fetch_as_reqwest_get_text_future() {
    let source = source_for(
        "async function load(): Promise<string> {
  return await fetch(\"https://example.com\");
}
",
    );

    assert!(source.contains(
            "reqwest::get(\"https://example.com\".to_owned()).await.expect(\"HTTP GET failed\").text().await.expect(\"HTTP response body read failed\")"
        ));
}

#[test]
fn emits_python_requests_get_as_blocking_reqwest_text() {
    let mut ctx = py_frontend::HirCtx::new();
    assert!(
            py_frontend::to_hir(
                "import requests\n\ndef load() -> str:\n    return requests.get(\"https://example.com\")\n",
                FileId(0),
                &mut ctx,
            )
            .is_ok(),
            "HIR"
        );
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(_) => panic!("MIR lowering failed"),
    };
    smelt_mir::opt::optimize(&mut mir);
    let source = match emit_source(&mir) {
        Ok(source) => source,
        Err(err) => panic!("Rust source: {err}"),
    };

    assert!(source.contains(
            "reqwest::blocking::get(\"https://example.com\".to_owned()).expect(\"HTTP GET failed\").text().expect(\"HTTP response body read failed\")"
        ));
}

#[test]
fn injects_reqwest_dependency_for_http_mapping() {
    let manifest = cargo_toml(
        &EmitOptions::default(),
        &[GeneratedDep::Tokio, GeneratedDep::Reqwest],
    );

    assert!(manifest.contains("tokio = { version = \"1\""));
    assert!(manifest.contains("reqwest = { version = \"0.12\""));
}

#[test]
fn injects_serde_json_dependency_for_json_mapping() {
    let manifest = cargo_toml(&EmitOptions::default(), &[GeneratedDep::SerdeJson]);

    assert!(manifest.contains("serde_json = \"1\""));
}

#[test]
fn injects_rand_dependency_for_random_mapping() {
    let manifest = cargo_toml(&EmitOptions::default(), &[GeneratedDep::Rand]);

    assert!(manifest.contains("rand = \"0.9\""));
}

#[test]
fn injects_regex_dependency_for_regex_mapping() {
    let manifest = cargo_toml(&EmitOptions::default(), &[GeneratedDep::Regex]);

    assert!(manifest.contains("regex = \"1\""));
}

#[test]
fn emits_caught_throw_without_result_signature() {
    let source = source_for(
        "try {
  throw \"boom\";
} catch (err: string) {
  console.log(err);
}
",
    );

    assert!(source.contains("fn main() {"));
    assert!(!source.contains("Box<dyn std::error::Error>"));
    assert!(source.contains("let err: String = \"boom\".to_owned();"));
}

#[test]
fn emits_record_field_assignment_as_insert() {
    let source = source_for(
        "let user: Record<string, string> = { name: \"Ada\" };
user.name = \"Grace\";
console.log(user.name);
",
    );

    assert!(
        source.contains("let mut user: ::std::collections::HashMap<String, String>"),
        "{source}"
    );
    assert!(
        source.contains("user.insert(\"name\".to_owned(), \"Grace\".to_owned());"),
        "{source}"
    );
    assert!(
        source.contains("user.get(\"name\").cloned().expect(\"missing field\")"),
        "{source}"
    );
}

#[test]
fn emits_record_index_assignment_as_insert() {
    let source = source_for(
        "let user: Record<string, string> = { name: \"Ada\" };
let key = \"name\";
user[key] = \"Grace\";
console.log(user[key]);
",
    );

    assert!(
        source.contains("user.insert(key.clone(), \"Grace\".to_owned());"),
        "{source}"
    );
    assert!(
        source.contains("user.get(&key.clone()).cloned().expect(\"index out of bounds\")"),
        "{source}"
    );
}
