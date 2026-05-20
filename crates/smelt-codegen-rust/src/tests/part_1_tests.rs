//! Split codegen tests chunk.

use super::*;

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
fn emits_typescript_first_class_closure_values() {
    let source = source_for(
        r#"
const offset = 2;
const addOffset = (value: number): number => value + offset;
function apply(value: number, fn: (value: number) => number): number {
  return fn(value);
}
function makeAdder(base: number): (value: number) => number {
  const add = (value: number): number => value + base;
  return add;
}
const direct = addOffset(3);
const passed = apply(4, addOffset);
const adder = makeAdder(5);
const returned = adder(6);
"#,
    );

    assert!(source.contains("Rc<::std::cell::RefCell<dyn FnMut(f64) -> f64>>"));
    assert!(source.contains("(3.0)"));
    assert!(source.contains("apply(4.0,"));
    assert!(source.contains("make_adder(5.0)"));
    assert!(source.contains("(&mut *adder.borrow_mut())(6.0)"));
    assert!(source.contains("move |"));
}

#[test]
fn emits_function_array_some_without_cloning_callbacks() {
    let source = source_for(
        r#"
function anyPass(data: unknown, fns: Array<(value: unknown) => boolean>): boolean {
  return fns.some((fn) => fn(data));
}
"#,
    );

    assert!(source.contains("mut fns:"), "{source}");
    assert!(
        source.contains("fns.iter_mut().enumerate().any"),
        "{source}"
    );
    assert!(!source.contains("let item = (*item).clone()"), "{source}");
    assert!(!source.contains("fns.clone()"), "{source}");
}

#[test]
fn skips_unused_function_callback_item_bindings_in_literal_false_branch() {
    let source = source_for(
        r#"
function lazy(functions: Array<(value: unknown) => unknown>): Array<unknown | null> {
  return functions.map((fn) => false ? fn(null) : null);
}
"#,
    );

    assert!(source.contains(".iter().enumerate().map"), "{source}");
    assert!(!source.contains("let item = (*item).clone()"), "{source}");
    assert!(!source.contains("(&mut *item.borrow_mut())"), "{source}");
}

#[test]
fn emits_string_concat_with_optional_primitive_rhs() {
    let source = source_for(
        r#"
function label(prefix: string, value: string | undefined): string {
  return prefix + value;
}
"#,
    );

    assert!(
        source.contains("prefix.clone() + &value.clone().unwrap_or_default()"),
        "{source}"
    );
}

#[test]
fn emits_unknown_slice_inside_callback_as_runtime_string_match() {
    let source = source_for(
        r#"
const values: unknown[] = ["abc"];
const tails = values.map((value) => value.slice(1));
"#,
    );

    assert!(source.contains("SmeltUnknown::String(value)"), "{source}");
    assert!(!source.contains(".slice(1.0)"), "{source}");
}

#[test]
fn emits_typescript_generic_and_default_closure_values() {
    let source = source_for(
        r#"
const bump = <T extends number>(value: number = 1): number => value + 1;
const defaulted = bump();
const explicit = bump(4);
"#,
    );

    assert!(source.contains("impl FnMut(f64) -> f64") || source.contains("|arg0: f64|"));
    assert!(source.contains("(1.0)"));
    assert!(source.contains("(4.0)"));
}

#[test]
fn emits_typescript_destructured_tuple_callback() {
    let source = source_for(
        r#"
const pairs: [number, number][] = [[1, 2], [3, 4]];
const sums = pairs.map(([left, right]) => left + right);
"#,
    );

    assert!(source.contains(".0.clone() + arg0.1.clone()"));
}

#[test]
fn emits_typescript_destructured_record_callback() {
    let source = source_for(
        r#"
const rows: Record<string, number>[] = [];
const doubled = rows.map(({ value }) => value * 2);
"#,
    );

    assert!(source.contains(".get(\"value\").cloned().unwrap_or(0.0)"));
}

#[test]
fn emits_typescript_async_closure_values() {
    let source = source_for(
        r#"
async function run(): Promise<number> {
  const lift = async (value: number): Promise<number> => value + 1;
  const result = await lift(4);
  return result;
}
"#,
    );

    assert!(source.contains("Box::pin(async move"));
    assert!(source.contains(".await"));
}

#[test]
fn emits_typescript_rest_closure_values() {
    let source = source_for(
        r#"
const sum = (...values: number[]): number => values[0] + values[1];
const total = sum(2, 3, 4);
"#,
    );

    assert!(source.contains("|arg0: Vec<f64>|"));
    assert!(source.contains("vec![2.0, 3.0, 4.0]"));
    assert!(source.contains("arg0[0].clone() + arg0[1].clone()"));
}

#[test]
fn emits_typescript_top_level_rest_functions() {
    let source = source_for(
        r#"
function sum(...values: number[]): number {
  return values[0] + values[1];
}
const total = sum(2, 3, 4);
"#,
    );

    assert!(source.contains("fn sum(values: Vec<f64>) -> f64"));
    assert!(source.contains("vec![2.0, 3.0, 4.0]"));
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
const indexed = values.map((value, index) => value + index);
const factor = 2;
const captured = values.map((value, index) => value * factor + index);
const scale = (value: number): number => value + factor;
const localClosure = values.map(scale);
let mutableTotal = 0;
values.forEach(value => mutableTotal += value);
const noInitial = values.reduce((acc, value, index) => acc + value + index);
"#,
    );

    assert!(source.contains(".iter().enumerate().map(|(index, item)|"));
    assert!(source.contains(".iter().enumerate().filter(|(index, item)|"));
    assert!(source.contains(".iter().enumerate().find(|(index, item)|"));
    assert!(source.contains(".iter().enumerate().position(|(index, item)|"));
    assert!(source.contains(".iter().enumerate().any(|(index, item)|"));
    assert!(source.contains(".iter().enumerate().all(|(index, item)|"));
    assert!(source.matches("loop {").count() >= 2);
    assert!(source.contains(".iter().enumerate().fold("));
    assert!(source.contains("reduce of empty array with no initial value"));
    assert!(source.contains(".collect::<Vec<_>>()"));
    assert!(source.contains(".map_or(-1.0"));
    assert!(source.contains("(item * 2.0)"), "{source}");
    assert!(source.contains("(item + 2.0)"));
    assert!(source.contains("let mut mutable_total"));
    assert!(source.contains("mutable_total.clone() +"));
    assert!(source.contains("mutable_total ="));
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
fn emits_callback_regex_replace_uppercase() {
    let source = source_for(
        r#"
export function format(units: string[]): string[] {
  return units.map((unit) => `x${unit.replace(/(^.)/, (m) => m.toUpperCase())}` as string);
}
"#,
    );

    assert!(source.contains("regex::Regex::new"));
    assert!(source.contains("to_uppercase()"));
}

#[test]
fn emits_callback_string_key_record_access() {
    let source = source_for(
        r#"
export function values(record: Record<string, number>, keys: string[]): number[] {
  return keys.map((key) => record[key]);
}
"#,
    );

    assert!(source.contains(".get(&item.clone())"), "{source}");
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
fn emits_typescript_primitive_conversions() {
    let source = source_for(
        r#"
const value = 42;
const digits = "42";
const asText = String(value);
const asNumber = Number(digits);
const asBool = Boolean("");
"#,
    );

    assert!(source.contains(".to_string()"));
    assert!(source.contains(".parse::<f64>().expect(\"float() parse failed\")"));
    assert!(source.contains(".is_empty()"));
}
