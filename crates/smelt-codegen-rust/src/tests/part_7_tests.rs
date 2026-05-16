//! Split codegen tests chunk.

use super::*;

#[test]
fn injects_url_dependency_for_url_mapping() {
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[GeneratedDep::Stdlib(BackendDependency::Url)],
    );

    assert!(manifest.contains("url = \"2\""));
}

#[test]
fn emits_dependency_backed_date_url_io_and_regex_operations() {
    let ts_source = source_for(
        r#"
const now = Date.now();
const iso = new Date(now).toISOString();
const host = new URL("https://example.com/a?q=1").hostname;
const hostWithPort = new URL("https://example.com:8443/a?q=1").host;
const cleaned = "a  b".replaceAll(new RegExp("\\s+"), "-");
const cleanedByCall = "a  b".replaceAll(RegExp("\\s+"), "-");
const cleanedByLiteral = "a  b".replace(/\s+/, "-");
"#,
    );
    assert!(ts_source.contains("chrono::Utc::now().timestamp_millis()"));
    assert!(ts_source.contains("chrono::DateTime::<chrono::Utc>::from_timestamp_millis"));
    assert!(ts_source.contains("url::Url::parse"));
    assert!(ts_source.contains("format!(\"{}:{}\", host, port)"));
    assert!(ts_source.contains("replace_all"));

    let py_source = source_for_py(
        r#"
import re

def read_write(path: str, text: str) -> str:
    replaced: str = re.sub(r"\s+", "-", text)
    parts: list[str] = re.split(r"\s+", text)
    written: int = open(path, "w").write(replaced)
    return open(path).read() + parts[0]
"#,
    );
    assert!(py_source.contains("regex::Regex::new"));
    assert!(py_source.contains("std::fs::read_to_string"));
    assert!(py_source.contains("std::fs::write"));
}

#[test]
fn emits_date_to_iso_string_for_erased_datearg_surfaces() {
    let source = source_for(
        r#"
declare const value: unknown;
const isoFromUnknown = new Date(value).toISOString();
const formatter = Intl.DateTimeFormat("en-US");
const isoFromFormatter = formatter.format(value);
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Number(value) => value"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN)"),
        "{source}"
    );
    assert!(source.contains("chrono::DateTime::<chrono::Utc>::from_timestamp_millis"));
}

#[test]
fn emits_no_arg_external_constructor_with_valid_empty_arg_tuple() {
    let source = source_for(
        r#"
import { UTCDate } from "@date-fns/utc";
const date = new UTCDate();
"#,
    );

    assert!(
        source.contains("let _smelt_external_args = ();"),
        "{source}"
    );
    assert!(
        !source.contains("let _smelt_external_args = (,);"),
        "{source}"
    );
}

#[test]
fn emits_date_getters_and_setters_for_erased_datearg_surfaces() {
    let source = source_for(
        r#"
declare const value: unknown;
const date = new Date(value);
const year = date.getFullYear();
date.setFullYear(value);
"#,
    );

    assert!(source.contains("date.year() as f64"), "{source}");
    assert!(source.contains("date.with_year("), "{source}");
    assert!(
        source.contains("SmeltUnknown::Number(value) => value"),
        "{source}"
    );
}

#[test]
fn keeps_date_setter_side_effects_inside_branch_blocks() {
    let source = source_for(
        r#"
function apply(isTwoDigitYear: boolean, year: number, date: number): number {
  if (isTwoDigitYear) {
    const normalizedTwoDigitYear = year + 2000;
    date.setFullYear(normalizedTwoDigitYear, 0, 1);
    return date;
  }
  return date;
}
"#,
    );

    let normalized = source
        .find("normalized_two_digit_year: f64 =")
        .unwrap_or_else(|| panic!("{source}"));
    let setter = source
        .find("date.with_year(normalized_two_digit_year.clone()")
        .unwrap_or_else(|| panic!("{source}"));
    assert!(normalized < setter, "{source}");
}

#[test]
fn emits_delete_on_erased_object_surfaces() {
    let source = source_for(
        r#"
function removeKey(value: unknown, key: string): boolean {
  return delete value[key];
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Object(map) => map.remove"),
        "{source}"
    );
    assert!(source.contains("_ => true"), "{source}");
}

#[test]
fn emits_unknown_record_literals_with_tagged_values() {
    let source = source_for(
        r#"
const value: Record<string, unknown> = { done: false, name: "skip" };
"#,
    );

    assert!(source.contains("::std::collections::HashMap<String, SmeltUnknown>"));
    assert!(source.contains("SmeltUnknown::Bool(false)"));
    assert!(source.contains("SmeltUnknown::String(\"skip\".to_owned())"));
}

#[test]
fn emits_typescript_unknown_as_tagged_type() {
    let source = source_for(
        "function identity(value: unknown): unknown {
  return value;
}

function passthrough(values: readonly unknown[]): readonly unknown[] {
  return values;
}
",
    );

    assert!(source.contains("pub enum SmeltUnknown"));
    assert!(source.contains("String(String),"));
    assert!(source.contains("fn identity(value: SmeltUnknown) -> SmeltUnknown"));
    assert!(source.contains("fn passthrough(values: Vec<SmeltUnknown>) -> Vec<SmeltUnknown>"));
}

#[test]
fn emits_typescript_unknown_wrap_checks_and_casts() {
    let source = source_for(
        r#"
function boxString(): unknown {
  return "ready";
}

function readString(value: unknown): string {
  if (typeof value === "string") {
    return value;
  }
  return value as string;
}

function isArray(value: unknown): boolean {
  return Array.isArray(value);
}
"#,
    );

    assert!(source.contains("SmeltUnknown::String"));
    assert!(source.contains("matches!(value.clone(), SmeltUnknown::String(_))"));
    assert!(source.contains("if let SmeltUnknown::String("));
    assert!(source.contains("matches!(value.clone(), SmeltUnknown::Array(_))"));
}

#[test]
fn emits_unknown_equality_against_concrete_values() {
    let source = source_for(
        r#"
function isTrailing(value: unknown): boolean {
  return value === "trailing";
}

function isNotOne(value: unknown): boolean {
  return value !== 1;
}
"#,
    );

    assert!(
        source.contains("value.clone() == SmeltUnknown::String(\"trailing\".to_owned())"),
        "{source}"
    );
    assert!(
        source.contains("!(value.clone() == SmeltUnknown::Number(1.0 as f64))"),
        "{source}"
    );
}

#[test]
fn emits_union_equality_against_concrete_values() {
    let source = source_for(
        r#"
function isEmpty(value: string | number): boolean {
  return value === "";
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::String(\"\".to_owned())"),
        "{source}"
    );
}

#[test]
fn emits_erased_dict_index_assignment_with_key_coercion() {
    let source = source_for(
        r#"
function assign(out: Record<string, unknown>, key: unknown, value: unknown): Record<string, unknown> {
  out[key as string] = value;
  return out;
}
"#,
    );

    assert!(
        source.contains("if let SmeltUnknown::String(value) = key.clone()"),
        "{source}"
    );
    assert!(
        source.contains("out.insert(_smelt_tmp_3.clone(), value.clone());"),
        "{source}"
    );
}

#[test]
fn emits_list_operand_item_coercion() {
    let source = source_for(
        r#"
function widen(values: string[]): unknown[] {
  return values;
}
"#,
    );

    assert!(
        source.contains(
            "values.clone().into_iter().map(|value| SmeltUnknown::String(value)).collect::<Vec<_>>()"
        ),
        "{source}"
    );
}

#[test]
fn emits_string_chars_into_unknown_list_destination() {
    let source = source_for(
        r#"
function chars(value: string): unknown[] {
  return [...value];
}
"#,
    );

    assert!(
        source.contains(".map(|value| SmeltUnknown::String(value)).collect::<Vec<_>>()"),
        "{source}"
    );
}

#[test]
fn emits_erased_value_wrapped_for_optional_erased_destination() {
    let source = source_for(
        r#"
function maybe(value: unknown): unknown | undefined {
  return value;
}
"#,
    );

    assert!(source.contains("Some(value.clone())"), "{source}");
}

#[test]
fn emits_list_literal_for_union_destination_as_unknown_array() {
    let source = source_for(
        r#"
function empty(flag: boolean): unknown[] | unknown {
  return flag ? [] : "none";
}
"#,
    );

    assert!(source.contains("SmeltUnknown::Array(vec![])"), "{source}");
}

#[test]
fn emits_loop_with_join_blocks_as_while() {
    let source = source_for(
        r#"
function countPresent(values: string[]): Record<string, number> {
  const out = new Map<string, number>();
  for (const value of values) {
    const count = out.get(value);
    if (count === undefined) {
      out.set(value, 1);
    } else {
      out.set(value, count + 1);
    }
  }
  return Object.fromEntries(out);
}
"#,
    );

    assert!(source.contains("while "), "{source}");
    assert!(source.contains("return out.clone();"), "{source}");
}

#[test]
fn emits_closure_call_result_for_optional_destination() {
    let source = source_for(
        r#"
function maybeCall(callback: (value: number) => number): number | undefined {
  const value: number | undefined = callback(1);
  return value;
}
"#,
    );

    assert!(
        source.contains("let value: Option<f64> = Some("),
        "{source}"
    );
}

#[test]
fn emits_callback_string_concat_with_borrowed_rhs() {
    let source = source_for(
        r#"
function label(values: string[]): string[] {
  return values.map((value, index) => "" + (index === 0 ? value : value.toLowerCase()));
}
"#,
    );

    assert!(source.contains("format!(\"{}{}\""), "{source}");
}

#[test]
fn emits_synthetic_default_for_missing_callback_arguments() {
    let source = source_for(
        r#"
function invoke(callback: (value: number) => number, fallback?: (value: number) => number): number {
  const chosen = (undefined as unknown) as (value: number) => number;
  return chosen(1);
}
"#,
    );

    assert!(
        source.contains("Box::new(move |arg0: f64| 0.0)"),
        "{source}"
    );
}

#[test]
fn emits_option_into_unknown_runtime_conversion() {
    let source = source_for(
        r#"
function maybe(flag: boolean): unknown {
  const value: string | undefined = flag ? "ready" : undefined;
  return value;
}
"#,
    );

    assert!(
        source.contains("impl<T: IntoSmeltUnknown> IntoSmeltUnknown for Option<T>"),
        "{source}"
    );
    assert!(
        source.contains("map_or(SmeltUnknown::Null, IntoSmeltUnknown::into_smelt_unknown)"),
        "{source}"
    );
}

#[test]
fn emits_generic_hash_map_into_unknown_runtime_conversion() {
    let source = source_for(
        r#"
function keyed(key: unknown): unknown {
  const values = new Map<unknown, unknown>([[key, "ready"]]);
  return Object.fromEntries(values);
}
"#,
    );

    assert!(
        source.contains("impl<K, T> IntoSmeltUnknown for ::std::collections::HashMap<K, T>"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Number(value) => value.to_string()"),
        "{source}"
    );
}

#[test]
fn emits_tuple_into_unknown_runtime_conversion() {
    let source = source_for(
        r#"
function pair(value: unknown): unknown {
  return [value, value] as [unknown, unknown];
}
"#,
    );

    assert!(
        source
            .contains("impl<A: IntoSmeltUnknown, B: IntoSmeltUnknown> IntoSmeltUnknown for (A, B)"),
        "{source}"
    );
}

#[test]
fn emits_unknown_partial_ordering_runtime_support() {
    let source = source_for(
        r#"
function before(left: unknown, right: unknown): boolean {
  return left < right;
}
"#,
    );

    assert!(
        source.contains("impl PartialOrd for SmeltUnknown"),
        "{source}"
    );
    assert!(source.contains("smelt_unknown_rank"), "{source}");
}

#[test]
fn emits_numeric_binary_operands_coerced_to_destination() {
    let source = source_for(
        r#"
function addUnknown(total: number, value: unknown): number {
  const narrowed = value as number;
  return total + narrowed;
}

function truncateDifference(left: bigint, right: number): bigint {
  return left - right;
}
"#,
    );

    assert!(source.contains("right.clone().trunc() as i64"), "{source}");
}

#[test]
fn emits_optional_erased_value_coerced_to_concrete_destination() {
    let source = source_for(
        r#"
function read(output: Record<string, unknown[]>, key: unknown | undefined): unknown[] {
  return output[key as string];
}
"#,
    );

    assert!(
        source
            .contains(".map_or(String::new(), |value| if let SmeltUnknown::String(value) = value"),
        "{source}"
    );
}

#[test]
fn emits_erased_nullish_coalescing_as_unknown_match() {
    let source = source_for(
        r#"
function fallback<T>(value: T, fallbackValue: T): T | undefined {
  return value ?? fallbackValue;
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Null => fallback_value.clone()"),
        "{source}"
    );
}

#[test]
fn emits_erased_nullish_coalescing_into_concrete_destination() {
    let source = source_for(
        r#"
function fallback(value: unknown): boolean {
  const result: boolean = value ?? false;
  return result;
}
"#,
    );

    assert!(
        source.contains("if let SmeltUnknown::Bool(value) = match value.clone()"),
        "{source}"
    );
}

#[test]
fn emits_boolean_cast_for_typescript_unknown() {
    let source = source_for(
        r#"
function truthy(value: unknown): boolean {
  return Boolean(value);
}
"#,
    );

    assert!(source.contains("SmeltUnknown::Null => false"));
    assert!(source.contains("SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => true"));
}

#[test]
fn emits_call_bodied_local_arrow_as_real_closure_body() {
    let source = source_for(
        r#"
function makeDataLast(fn: (value: number, extra: number) => number, extra: number): (value: number) => number {
  const dataLast = (data: number): number => fn(data, extra);
  return dataLast;
}
"#,
    );

    assert!(source.contains("|closure_arg_0: f64| {"), "{source}");
    assert!(
        source.contains("fn_(closure_arg_0.clone(), extra.clone())"),
        "{source}"
    );
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
    assert!(source.contains("err = \"boom\".to_owned();"));
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

#[test]
fn emits_radix_to_string_and_numeric_shift_surface() {
    let source = source_for(
        r#"
const binary = (10n).toString(2);
const left = 1n << 8n;
const right = left >> 1n;
const pivot = (4 + 10) >>> 1;
function shiftRaw(raw: bigint): bigint {
  return raw >> 1n;
}
"#,
    );

    assert!(
        source.contains("let radix = ((2.0 as f64).trunc() as u32).clamp(2, 36);"),
        "{source}"
    );
    assert!(source.contains("<<"), "{source}");
    assert!(source.contains(">>"), "{source}");
    assert!(source.contains("fn shift_raw(raw: i64) -> i64"), "{source}");
    assert!(
        source.contains("(((raw.clone() as f64).trunc() as i128) >>"),
        "{source}"
    );
    assert!(source.contains("rem_euclid(4294967296.0)"), "{source}");
}

#[test]
fn emits_array_from_length_mapper() {
    let source = source_for(
        r#"
function range(start: number, length: number, step: number): number[] {
  return Array.from({ length }, (_, i) => (i === 0 ? start : start + i * step));
}
"#,
    );

    assert!(source.contains("array_from_length"), "{source}");
    assert!(source.contains("(0..array_from_length).map"), "{source}");
    assert!(source.contains("let index = index as f64"), "{source}");
}

#[test]
fn emits_callback_dynamic_index_with_non_null_assertion() {
    let source = source_for(
        r#"
function sample<T>(data: readonly T[]): T[] {
  const sampleIndices = new Set<number>();
  return [...sampleIndices].sort((a, b) => a - b).map((index) => data[index]!);
}
"#,
    );

    assert!(source.contains("callback_index_receiver"), "{source}");
    assert!(
        source.contains(".get(callback_index).cloned().expect(\"index out of bounds\")"),
        "{source}"
    );
}

#[test]
fn emits_sort_with_comparator_function_value() {
    let source = source_for(
        r#"
const sortByImplementation = <T>(
  data: readonly T[],
  compareFn: (left: T, right: T) => number,
): T[] => [...data].sort(compareFn);
"#,
    );

    assert!(
        source.contains("closure_arg_1(left.clone(), right.clone())"),
        "{source}"
    );
}

#[test]
fn boxes_returned_function_values_even_when_mir_types_match() {
    let source = source_for(
        r#"
function makeMapper(): (value: number) => number {
  const mapper = (value: number) => value + 1;
  return mapper;
}
"#,
    );

    assert!(
        source.contains("fn make_mapper() -> Box<dyn FnMut(f64) -> f64>"),
        "{source}"
    );
    assert!(source.contains("return Box::new("), "{source}");
}

#[test]
fn coerces_function_adapter_return_values_to_target_return_type() {
    let source = source_for(
        r#"
function adapt(
  callback: (value: unknown) => { next: unknown },
): (value: unknown, index: number, data: unknown[]) => unknown {
  return callback;
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Object((&mut *callback)(arg0))"),
        "{source}"
    );
}

#[test]
fn emits_default_for_none_constant_assigned_to_concrete_destination() {
    let source = source_for(
        r#"
function choose(flag: boolean): unknown[] {
  let values: unknown[];
  if (flag) {
    values = ["ready"];
  } else {
    values = [];
  }
  return values;
}
"#,
    );

    assert!(
        source.contains("let mut values: Vec<SmeltUnknown> = Vec::new();"),
        "{source}"
    );
}

#[test]
fn emits_first_assignment_to_uninitialized_local_as_declaration() {
    let source = source_for(
        r#"
function choose(flag: boolean): number {
  let result: number;
  if (flag) {
    result = 1;
  } else {
    result = 2;
  }
  return result;
}
"#,
    );

    assert!(
        source.contains("let mut result: f64 = 0.0;") || source.contains("let mut result: i64 = 0"),
        "{source}"
    );
    assert!(source.contains("result = "), "{source}");
}

#[test]
fn coerces_function_adapter_forwarded_arguments_to_source_param_types() {
    let source = source_for(
        r#"
function adapt(
  callback: (values: unknown[], index?: number) => unknown,
): (value: unknown, index: number, data: unknown[]) => unknown {
  return callback;
}
"#,
    );

    assert!(
        source.contains("if let SmeltUnknown::Array(value) = arg0"),
        "{source}"
    );
    assert!(source.contains("Some(arg1)"), "{source}");
}

#[test]
fn emits_regex_find_with_erased_haystack_string_coercion() {
    let source = source_for(
        r#"
function matchUnknown(value: unknown): string[] | undefined {
  return (value as any).match(/a+/);
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::String(value) => value"),
        "{source}"
    );
    assert!(source.contains(".find(&match "), "{source}");
}

#[test]
fn coerces_rendered_list_values_to_tuple_destinations() {
    let source = source_for(
        r#"
function invoke(
  values: unknown[],
  callback: (pair: [unknown, unknown]) => unknown,
): unknown {
  return callback(values);
}
"#,
    );

    assert!(
        source.contains("let smelt_tuple_values = values.clone()"),
        "{source}"
    );
    assert!(source.contains("smelt_tuple_values.get(0)"), "{source}");
    assert!(source.contains("smelt_tuple_values.get(1)"), "{source}");
}
