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
fn emits_string_index_and_for_of() {
    let source = source_for(
        r#"
const word = "abc";
const first = word[0];
let joined = "";
for (let ch: string of word) {
  joined = joined + ch;
}
"#,
    );

    assert!(source.contains(".chars().nth(0.0 as usize).map(|ch| ch.to_string()).expect"));
    assert!(source.contains(".chars().count() as f64"));
    assert!(source.contains(".chars().nth(_smelt_tmp_"));
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
const distance = Math.hypot(value, 3);
"#,
    );

    assert!(source.contains(".sqrt();"));
    assert!(source.contains(".cbrt();"));
    assert!(source.contains(".powf("));
    assert!(source.contains(".signum();"));
    assert!(source.contains("0.0f64.hypot("));
    assert!(source.contains(".hypot(3.0);"));
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
word: str = "smelting"
all_text: str = word[:]
tail_text: str = word[1:]
mid_text: str = word[1:4]
"#,
    );

    assert!(source.contains(".iter().skip(0usize).take("));
    assert!(source.contains(".iter().skip((1 as usize)).take("));
    assert!(source.contains(".take((3 as usize).saturating_sub((1 as usize)))"));
    assert!(source.contains(".cloned().collect::<Vec<_>>();"));
    assert!(source.contains(".chars().skip(0usize).take("));
    assert!(source.contains(".chars().skip((1 as usize)).take("));
    assert!(source.contains(".collect::<String>();"));
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
value: float = 4.0
root: float = math.sqrt(value)
raised: float = math.pow(value, 2.0)
whole: int = math.trunc(value)
values: tuple[int, int] = (1, 2)
has_tuple: bool = 2 in values
mapping: dict[str, int] = {"a": 1}
has_key: bool = "a" in mapping
"#,
    );

    assert!(source.contains(".sqrt();"));
    assert!(source.contains(".powf("));
    assert!(source.contains(".trunc() as i64;"));
    assert!(source.contains(".0 == "));
    assert!(source.contains(".contains_key(&"));
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
const word = "smelting";
const allText = word.slice();
const tailText = word.slice(1);
const midText = word.slice(1, 4);
"#,
    );

    assert!(source.contains(".iter().skip(0usize).take("));
    assert!(source.contains(".iter().skip((1.0 as usize)).take("));
    assert!(source.contains(".take((3.0 as usize).saturating_sub((1.0 as usize)))"));
    assert!(source.contains(".cloned().collect::<Vec<_>>();"));
    assert!(source.contains(".chars().skip(0usize).take("));
    assert!(source.contains(".chars().skip((1.0 as usize)).take("));
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
    let manifest = cargo_toml(&EmitOptions::default(), true, true);

    assert!(manifest.contains("tokio = { version = \"1\""));
    assert!(manifest.contains("reqwest = { version = \"0.12\""));
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
