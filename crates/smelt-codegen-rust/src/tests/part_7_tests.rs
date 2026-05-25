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
fn emits_vitest_date_timezone_offset_mock_state() {
    let source = source_for(
        r#"
import { vi } from "vitest";

const spy = vi.spyOn(Date.prototype, "getTimezoneOffset");
spy.mockReturnValue(480);
const offset = new Date().getTimezoneOffset();
spy.mockRestore();
"#,
    );

    assert!(source.contains("SMELT_DATE_TIMEZONE_OFFSET"), "{source}");
    assert!(source.contains("value.set(480.0)"), "{source}");
    assert!(source.contains("with(::std::cell::Cell::get)"), "{source}");
    assert!(source.contains("value.set(0.0)"), "{source}");
}

#[test]
fn emits_date_fns_timezone_context_with_iana_conversion() {
    let source = source_for(
        r#"
import { tz } from "@date-fns/tz";
const context = tz("Pacific/Midway");
"#,
    );
    let manifest = deps::cargo_toml(
        &EmitOptions::default().crate_name,
        &[
            GeneratedDep::Stdlib(BackendDependency::Chrono),
            GeneratedDep::Stdlib(BackendDependency::ChronoTz),
        ],
    );

    assert!(source.contains("chrono_tz::Tz"), "{source}");
    assert!(
        source.contains("with_timezone(&smelt_timezone)"),
        "{source}"
    );
    assert!(manifest.contains("chrono-tz = \"0.10\""), "{manifest}");
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
        source.contains(
            "SmeltUnknown::String(value) => chrono::DateTime::parse_from_rfc3339(&value)"
        ),
        "{source}"
    );
    assert!(source.contains("chrono::DateTime::<chrono::Utc>::from_timestamp_millis"));
}

#[test]
fn emits_to_iso_string_on_erased_callback_items() {
    let source = source_for(
        r#"
declare const values: unknown[];
const isoValues = values.map((value) => value.toISOString());
"#,
    );

    assert!(
        source.contains("pub fn to_iso_string(&self) -> String"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::String(item.to_iso_string())"),
        "{source}"
    );
}

#[test]
fn emits_total_unknown_primitive_and_object_extraction() {
    let source = source_for(
        r#"
declare const value: unknown;
const text: string = value as string;
const count: number = value as number;
const flag: boolean = value as boolean;
const bag: Record<string, unknown> = value as Record<string, unknown>;
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Null => String::new()"),
        "{source}"
    );
    assert!(source.contains("SmeltUnknown::Null => false"), "{source}");
    assert!(
        source.contains("value.parse::<f64>().unwrap_or(f64::NAN)"),
        "{source}"
    );
    assert!(
        source.contains("_ => ::std::collections::HashMap::new()"),
        "{source}"
    );
}

#[test]
fn keeps_vitest_assertions_inside_catch_blocks() {
    let source = source_for(
        r#"
import { expect, test } from "vitest";

function fail(): never {
  throw new RangeError("bad");
}

test("catch assertion", () => {
  try {
    fail();
  } catch (e) {
    expect(e instanceof RangeError).toBe(true);
  }
});
"#,
    );

    let err_arm = source
        .find("Err(__smelt_error)")
        .expect("generated try/catch should have an Err arm");
    let assertion = source
        .find("expect(...).toBe(...) failed")
        .expect("catch assertion should lower to a test failure branch");
    assert!(
        assertion > err_arm,
        "catch assertion escaped before the catch binding:\n{source}"
    );
}

#[test]
fn emits_calls_through_local_export_aliases_as_static_calls() {
    let source = source_for(
        r#"
export function format(value: string): string {
  return value;
}
export { format as formatDate };
const result = formatDate("ok");
"#,
    );

    assert!(source.contains("format(\"ok\".to_owned())"), "{source}");
    assert!(
        !source.contains("let smelt_function_value"),
        "same-module function export aliases must not erase to dynamic calls:\n{source}"
    );
}

#[test]
fn emits_default_derived_class_constructor_with_optional_forwarded_arg() {
    let source = source_for(
        r#"
class Base {}
class Child extends Base {}
const withArg = new Child("value");
const withoutArg = new Child();
const ctor = withArg.constructor;
"#,
    );

    assert!(
        source.contains("fn new(_smelt_super_arg: Option<SmeltUnknown>) -> Self"),
        "{source}"
    );
    assert!(
        source.contains("Child::new(Some(SmeltUnknown::String(\"value\".to_owned())))"),
        "{source}"
    );
    assert!(
        source.contains("Child::new(None::<SmeltUnknown>)"),
        "{source}"
    );
    assert!(
        source.contains("ctor = SmeltUnknown::Null.clone()"),
        "{source}"
    );
}

#[test]
fn adapts_throwing_callback_to_non_throwing_function_field() {
    let source = source_for(
        r#"
import { expect } from "vitest";

type Locale = {
  formatDistance: (token: string) => string;
};

const localizeDistance = (token: string) => {
  expect(token).toBe("x");
  return "ok";
};

const locale: Locale = { formatDistance: localizeDistance };
"#,
    );

    assert!(
        source.contains("unwrap_or_else(|error| panic!(\"{}\", error))"),
        "{source}"
    );
    assert!(
        source.contains("formatDistance") || source.contains("format_distance"),
        "{source}"
    );
}

#[test]
fn emits_js_numeric_conversion_for_erased_unary_plus() {
    let source = source_for(
        r#"
declare const value: unknown;
const number = +value;
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Number(value) => value"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => f64::NAN"),
        "{source}"
    );
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
fn emits_object_literal_as_destination_interface_record() {
    let source = source_for(
        r#"
interface Options {
  width?: string;
}

function format(options: Options): string {
  return options.width ?? "full";
}

export function run(): string {
  return format({ width: "short" });
}
"#,
    );

    assert!(
        source.contains(
            "Options { width: smelt_record_map.get(\"width\").cloned().map(|value| value) }"
        ),
        "{source}"
    );
    assert!(
        !source.contains("format(::std::collections::HashMap::from"),
        "{source}"
    );
}

#[test]
fn adapts_object_literal_for_interface_callback_field_argument() {
    let source = source_for(
        r#"
interface Options {
  width?: string;
}

interface FormatLong {
  date: (options: Options) => string;
}

export function run(formatLong: FormatLong): string {
  return formatLong.date({ width: "short" });
}
"#,
    );

    assert!(
        source.contains("date.borrow_mut())({ let smelt_record_map ="),
        "{source}"
    );
    assert!(
        source.contains(
            "Options { width: smelt_record_map.get(\"width\").cloned().map(|value| value) }"
        ),
        "{source}"
    );
}

#[test]
fn emits_empty_object_literal_as_optional_interface_record_defaults() {
    let source = source_for(
        r#"
interface Duration {
  years?: number;
  months?: number;
}

export function make(): Duration {
  return {};
}
"#,
    );

    assert!(
        source.contains("Duration { years: smelt_record_map.get(\"years\").cloned().map(|value|"),
        "{source}"
    );
    assert!(
        source.contains("months: smelt_record_map.get(\"months\").cloned().map(|value|"),
        "{source}"
    );
}

#[test]
fn emits_optional_string_match_with_some_patterns() {
    let source = source_for(
        r#"
export function label(value?: string): string {
  switch (value) {
    case "a":
      return "A";
    default:
      return "other";
  }
}
"#,
    );

    assert!(
        source.contains("match value.clone().as_deref()"),
        "{source}"
    );
    assert!(source.contains("Some(\"a\") =>"), "{source}");
}

#[test]
fn coerces_function_typed_dict_literal_values_to_trait_objects() {
    let source = source_for(
        r#"
export function callbacks(): Record<string, (value: number) => string> {
  const suffix = "!";
  return {
    a: (value: number) => value.toString(),
    b: (value: number) => value.toString() + suffix,
  };
}
"#,
    );

    assert!(
        source
            .matches("let smelt_fn: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut(f64) -> String>>")
            .count()
            >= 2,
        "{source}"
    );
}

#[test]
fn coerces_string_values_to_regexp_destinations() {
    let source = source_for(
        r#"
const pattern: RegExp = "\\d+";
"#,
    );

    assert!(
        source.contains("SmeltRegExp::new(\"\\\\d+\".to_owned(), String::new())"),
        "{source}"
    );
}

#[test]
fn keeps_python_constructor_call_result_as_class_value() {
    let source = source_for_py(
        r#"
class Obj:
    id: str

    def __init__(self, id: str) -> None:
        self.id = id

obj: Obj = Obj("a")
"#,
    );

    assert!(source.contains("Obj::new(\"a\".to_owned())"), "{source}");
    assert!(
        !source.contains("obj: Obj = Default::default()"),
        "{source}"
    );
}

#[test]
fn emits_manual_default_for_function_field_interfaces() {
    let source = source_for(
        r#"
interface Localize<T> {
  ordinalNumber: (value: number) => string;
  value?: T;
}

function read(localize: Localize<string>): string {
  return localize.ordinalNumber(1);
}
"#,
    );

    assert!(
        source.contains("impl<T> Default for Localize<T>"),
        "{source}"
    );
    assert!(
        source.contains("ordinal_number: { let smelt_default_callback:"),
        "{source}"
    );
    assert!(
        source.contains("_smelt_phantom: ::std::marker::PhantomData,"),
        "{source}"
    );
    assert!(
        source.contains("impl<T> ::std::fmt::Debug for Localize<T>"),
        "{source}"
    );
}

#[test]
fn flattens_interface_extends_into_generated_storage() {
    let source = source_for(
        r#"
interface WeekOptions {
  weekStartsOn?: number;
}

interface FirstWeekContainsDateOptions {
  firstWeekContainsDate?: number;
}

interface LocaleOptions extends WeekOptions, FirstWeekContainsDateOptions {}

function read(options: LocaleOptions): number {
  return options.weekStartsOn ?? options.firstWeekContainsDate ?? 0;
}
"#,
    );

    assert!(source.contains("struct LocaleOptions"), "{source}");
    assert!(source.contains("week_starts_on: Option<f64>"), "{source}");
    assert!(
        source.contains("first_week_contains_date: Option<f64>"),
        "{source}"
    );
}

#[test]
fn projects_fields_from_optional_interface_records() {
    let source = source_for(
        r#"
interface Options {
  comparison?: number;
}

function read(options?: Options): number {
  return options?.comparison ?? 0;
}

function positive(options?: Options): boolean {
  return options.comparison > 0;
}
"#,
    );

    assert!(
        source.contains(
            "options.clone().as_ref().and_then(|_smelt_value| _smelt_value.comparison.clone())"
        ),
        "{source}"
    );
    assert!(source.contains(".unwrap_or(0.0) > 0.0"), "{source}");
}

#[test]
fn emits_regex_replace_callbacks_as_closures() {
    let source = source_for(
        r#"
export function localize(enNumber: number): string {
  const suffix = "!";
  return enNumber.toString().replace(/\d/g, (match) => match + suffix);
}
"#,
    );

    assert!(source.contains("replace_all"), "{source}");
    assert!(source.contains("|arg0: String|"), "{source}");
    assert!(
        !source.contains("|caps: &regex::Captures<'_>| (_smelt_tmp_"),
        "{source}"
    );
}

#[test]
fn emits_escaping_closure_spread_calls_with_owned_callback_state() {
    let source = source_for(
        r#"
export function purryOn(
  isArg: (x: unknown) => boolean,
  implementation: (data: unknown, first: unknown, ...rest: Array<unknown>) => unknown,
  args: Array<unknown>,
): unknown {
  return isArg(args[0])
    ? (data: unknown) => implementation(data, ...args)
    : implementation(args[0], args[1], args.slice(2));
}
"#,
    );

    assert!(
        source.contains("implementation: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut"),
        "{source}"
    );
    assert!(source.contains("implementation.borrow_mut()"), "{source}");
    assert!(source.contains("args.clone().get(0).cloned()"), "{source}");
    assert!(
        source.contains("args.clone().into_iter().skip(1).collect::<Vec<_>>()"),
        "{source}"
    );
    assert!(
        !source.contains("SmeltUnknown::Array(args.clone()).get"),
        "{source}"
    );
    assert!(
        !source.contains("SmeltUnknown::Array(args.clone()).clone().into_iter()"),
        "{source}"
    );
}

#[test]
fn adapts_rest_callback_shape_to_trailing_list_parameter() {
    let source = source_for(
        r#"
function purryOn(
  isArg: (x: unknown) => boolean,
  implementation: (data: unknown, first: unknown, rest: Array<unknown>) => unknown,
  args: Array<unknown>,
): unknown {
  return isArg(args[0])
    ? (data: unknown) => implementation(data, ...args)
    : implementation(args[0], args[1], args.slice(2));
}

function implementation(data: unknown, ...cases: Array<unknown>): unknown {
  return cases;
}

export function conditional(args: Array<unknown>): unknown {
  return purryOn((x) => true, implementation, args);
}
"#,
    );

    assert!(
        source.contains("smelt_forwarded_args.push(arg1"),
        "{source}"
    );
    assert!(
        source.contains("smelt_forwarded_args.extend(arg2.into_iter()"),
        "{source}"
    );
    assert!(
        !source.contains("if let SmeltUnknown::Array(values) = arg1.clone()"),
        "{source}"
    );
}

#[test]
fn spreads_trailing_rest_vectors_across_erased_function_boundaries() {
    let source = source_for(
        r#"
function callConcrete(
  callback: (data: unknown, ...rest: Array<unknown>) => unknown,
  args: Array<unknown>,
): unknown {
  return callback(args[0], ...args.slice(1));
}

export function useConcrete(): unknown {
  return callConcrete((data, ...rest) => rest, [1, 2, 3]);
}

export function wrapConcrete(): unknown {
  const callback: unknown = (data: unknown, ...rest: Array<unknown>) => rest;
  return callback;
}

"#,
    );

    assert!(
        source.contains("smelt_args.iter().skip(1).cloned().collect::<Vec<_>>()"),
        "{source}"
    );
    assert!(!source.contains("SmeltUnknown::Array(arg1)"), "{source}");
}

#[test]
fn does_not_spread_array_parameters_across_erased_function_boundaries() {
    let source = source_for(
        r#"
export function wrapArrayParam(): unknown {
  const callback: unknown = (data: unknown, values: Array<unknown>) => values;
  return callback;
}
"#,
    );

    assert!(
        source.contains("smelt_args.get(1).cloned().unwrap_or(SmeltUnknown::Null)"),
        "{source}"
    );
    assert!(
        !source.contains("smelt_args.iter().skip(1).cloned().collect::<Vec<_>>()"),
        "{source}"
    );
}

#[test]
fn emits_mutable_class_method_parameters_when_reassigned() {
    let source = source_for(
        r#"
class Parser {
  set(date: number, value: number): number {
    date = value + 1;
    return date;
  }
}
"#,
    );

    assert!(
        source.contains("fn set(&self, mut date: f64, value: f64) -> f64"),
        "{source}"
    );
}

#[test]
fn emits_mutable_constructor_parameters_when_reassigned() {
    let source = source_for(
        r#"
class Box {
  value: number;
  constructor(value: number) {
    value = value + 1;
    this.value = value;
  }
}
"#,
    );

    assert!(
        source.contains("fn new(mut value: f64) -> Self"),
        "{source}"
    );
}

#[test]
fn deduplicates_interface_fields_after_inheritance_expansion() {
    let source = source_for(
        r#"
interface ContextOptions {
  in?: unknown;
}
interface FormatOptions extends ContextOptions {
  locale?: string;
  in?: unknown;
}
const options: FormatOptions = { locale: "en", in: null };
const text = JSON.stringify(options);
"#,
    );

    assert!(source.contains("struct FormatOptions"), "{source}");
    let format_options = source
        .split("struct FormatOptions")
        .nth(1)
        .and_then(|text| text.split("}\n").next())
        .expect("FormatOptions struct should be emitted");
    assert_eq!(
        format_options.matches("in_: Option<SmeltUnknown>,").count(),
        1,
        "{source}"
    );
}

#[test]
fn emits_interface_storage_without_json_dependency() {
    let source = source_for(
        r#"
interface Options {
  flag?: boolean;
}
function enabled(options?: Options): boolean {
  return true;
}
"#,
    );

    assert!(source.contains("struct Options"), "{source}");
    assert!(source.contains("flag: Option<bool>,"), "{source}");
    assert!(!source.contains("serde::Serialize"), "{source}");
}

#[test]
fn derives_clone_for_function_bearing_interface_storage() {
    let source = source_for(
        r#"
interface Callbacks {
  run: () => number;
}
function copy(callbacks: Callbacks): Callbacks {
  return callbacks;
}
"#,
    );

    assert!(
        source.contains("#[derive(Clone)]\n#[allow(dead_code)]\nstruct Callbacks"),
        "{source}"
    );
}

#[test]
fn emits_generic_interface_storage_with_phantom_parameter() {
    let source = source_for(
        r#"
interface Boxed<T> {
  value: T;
}
declare const boxed: Boxed<number>;
const copied: Boxed<number> = boxed;
"#,
    );

    assert!(source.contains("struct Boxed<T>"), "{source}");
    assert!(
        source.contains("_smelt_phantom: ::std::marker::PhantomData<(T)>,"),
        "{source}"
    );
    assert!(source.contains("boxed: Boxed<f64>"), "{source}");
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
    assert!(
        source.contains("chrono::NaiveDate::from_ymd_opt"),
        "{source}"
    );
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
        .find("normalized_two_digit_year =")
        .or_else(|| source.find("normalized_two_digit_year: f64 ="))
        .unwrap_or_else(|| panic!("{source}"));
    let setter = source
        .find("let normalized_year = (normalized_two_digit_year.clone() as i32)")
        .unwrap_or_else(|| panic!("{source}"));
    assert!(normalized < setter, "{source}");
    assert!(source.contains(" as f64"), "{source}");
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
    assert!(source.contains("SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value"));
    assert!(source.contains("matches!(value.clone(), SmeltUnknown::Array(_))"));
}

#[test]
fn emits_runtime_index_for_erased_string_generics() {
    let source = source_for(
        r#"
function first<S extends string>(value: S): string {
  return value[0];
}
"#,
    );

    assert!(source.contains("SmeltUnknown::String(value)"), "{source}");
    assert!(source.contains("value.chars().nth(index)"), "{source}");
    assert!(!source.contains("SmeltUnknown::Null.clone()"), "{source}");
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
        source.contains("SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value"),
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
fn emits_loop_with_join_blocks_as_loop() {
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

    assert!(source.contains("loop {"), "{source}");
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
fn emits_default_callback_for_erased_non_function_callback_cast() {
    let source = source_for(
        r#"
function invoke(callback: (value: number) => number, fallback?: (value: number) => number): number {
  const chosen = (undefined as unknown) as (value: number) => number;
  return chosen(1);
}
"#,
    );

    assert!(source.contains("smelt_default_callback"), "{source}");
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
fn emits_cycle_safe_unknown_structural_equality_runtime() {
    let source = source_for(
        r#"
declare const left: unknown;
declare const right: unknown;
const same = left === right;
"#,
    );

    assert!(
        source.contains("fn smelt_unknown_structural_eq"),
        "{source}"
    );
    assert!(source.contains("fn smelt_object_structural_eq"), "{source}");
    assert!(
        source.contains("if !seen.insert(key) { return true; }"),
        "{source}"
    );
    assert!(
        source.contains("left.is_nan() && right.is_nan()"),
        "{source}"
    );
}

#[test]
fn emits_cycle_safe_unknown_structural_hash_runtime() {
    let source = source_for(
        r#"
declare const value: unknown;
const values = new Set<unknown>([value]);
"#,
    );

    assert!(
        source.contains("fn smelt_unknown_structural_hash"),
        "{source}"
    );
    assert!(
        source.contains("fn smelt_object_structural_hash"),
        "{source}"
    );
    assert!(source.contains("if !seen.insert(object.id)"), "{source}");
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

    assert!(
        source.contains("((right.clone() as f64).trunc() as i64)"),
        "{source}"
    );
}

#[test]
fn emits_numeric_not_as_parenthesized_truthiness() {
    let source = source_for(
        r#"
function isZero(amount: number): boolean {
  return !amount;
}
"#,
    );

    assert!(
        source.contains("!({ let smelt_number = amount.clone(); smelt_number != 0.0 && !smelt_number.is_nan() })"),
        "{source}"
    );
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
        source.contains(
            ".map_or(String::new(), |value| match value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value"
        ),
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
        source.contains("SmeltUnknown::Null => false, SmeltUnknown::Bool(value) => value"),
        "{source}"
    );
}

#[test]
fn emits_optional_nullish_coalescing_with_erased_fallback_without_panicking() {
    let source = source_for(
        r#"
declare const defaults: Record<string, unknown>;
interface Options {
  weekStartsOn?: number;
}
function read(options?: Options): number {
  return options?.weekStartsOn ?? defaults.weekStartsOn ?? 0;
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Null => None, value => Some("),
        "{source}"
    );
    assert!(source.contains(".or(match"), "{source}");
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
    assert!(source.contains(
        "SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => true"
    ));
}

#[test]
fn emits_nan_truthiness_without_invalid_nan_comparison() {
    let source = source_for(
        r#"
function truthy(): boolean {
  return Boolean(NaN);
}
"#,
    );

    assert!(source.contains("!smelt_number.is_nan()"), "{source}");
    assert!(!source.contains("f64::NAN != 0.0"), "{source}");
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

    assert!(
        source.contains("fn_: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut(f64, f64) -> f64>>"),
        "{source}"
    );
    assert!(
        source.contains("(&mut *fn_.borrow_mut())(closure_arg_0.clone(), extra.clone())"),
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
        source.contains("user.get(&\"name\".to_owned()).cloned().expect(\"missing field\")"),
        "{source}"
    );
}

#[test]
fn emits_unknown_field_assignment_as_object_insert() {
    let source = source_for(
        r#"
function assign(value: unknown): unknown {
  value.name = "Grace";
  return value;
}
"#,
    );

    assert!(source.contains("match &mut value"), "{source}");
    assert!(
        source.contains("SmeltUnknown::Object(map) => { map.insert(\"name\".to_owned(), SmeltUnknown::String(\"Grace\".to_owned())); }"),
        "{source}"
    );
    assert!(
        source.contains("*other = SmeltUnknown::Object(map);"),
        "{source}"
    );
}

#[test]
fn coerces_optional_unknown_field_to_optional_callable_destination() {
    let source = source_for(
        r#"
type Context = ((value: unknown) => unknown) | undefined;
interface Options {
  in?: Context;
}

function apply(value: unknown, context?: Context): unknown {
  return value;
}

function run(options?: Options): unknown {
  return apply(null, options?.in);
}
"#,
    );

    assert!(
        source.contains(".as_ref().and_then(|_smelt_value|"),
        "{source}"
    );
    assert!(
        !source.contains("= options.clone().as_ref().map(|_smelt_value| SmeltUnknown::Null);"),
        "{source}"
    );
    assert!(source.contains("Option<::std::rc::Rc"), "{source}");
}

#[test]
fn adapts_structural_option_bags_at_call_boundaries() {
    let source = source_for(
        r#"
interface IsWeekendOptions {
  in?: (value: unknown) => unknown;
}

interface AddBusinessDaysOptions<DateType> {
  in?: (value: DateType) => DateType;
}

function isWeekend(date: unknown, options?: IsWeekendOptions): boolean {
  return false;
}

function addBusinessDays<DateType>(date: DateType, options?: AddBusinessDaysOptions<DateType>): boolean {
  return isWeekend(date, options);
}
"#,
    );

    assert!(source.contains("IsWeekendOptions {"), "{source}");
    assert!(
        source.contains("in_: smelt_struct_value.in_.clone()"),
        "{source}"
    );
    assert!(
        !source.contains("map_or(SmeltUnknown::Null, |value| value)"),
        "{source}"
    );
}

#[test]
fn adapts_nested_object_literal_option_fields_to_structural_records() {
    let source = source_for(
        r#"
interface Locale {
  code: string;
}

interface FormatOptions {
  locale?: Locale;
}

function format(options?: FormatOptions): string {
  return options?.locale?.code ?? "";
}

const customLocale = { code: "fr" };
const result = format({ locale: customLocale });
"#,
    );

    assert!(
        source.contains("locale: smelt_record_map.get(\"locale\")"),
        "{source}"
    );
    assert!(source.contains("Locale { code:"), "{source}");
    assert!(!source.contains("FormatOptions { locale: None"), "{source}");
}

#[test]
fn adapts_shorter_object_literal_callbacks_to_interface_field_signatures() {
    let source = source_for(
        r#"
interface Locale {
  localize: Localize;
}

interface Localize {
  month: (value: number, options?: string) => string;
}

interface FormatOptions {
  locale?: Locale;
}

function useLocale(options?: FormatOptions): string {
  return options?.locale?.localize.month(0) ?? "";
}

const customLocale = { localize: { month: () => "works" } };
const result = useLocale({ locale: customLocale });
"#,
    );

    assert!(source.contains("let smelt_adapted:"), "{source}");
    assert!(source.contains("move |arg0: f64"), "{source}");
    assert!(source.contains("borrow_mut())()"), "{source}");
}

#[test]
fn preserves_erased_date_values_when_retyping_unknown_callback_fields() {
    let source = source_for(
        r#"
interface Localize {
  month: (value: number) => string;
  preprocessor?: (date: Date, parts: string[]) => string[];
}

interface Locale {
  localize: Localize;
}

interface FormatOptions {
  locale?: Locale;
}

function format(options?: FormatOptions): string {
  return "";
}

const customLocale = {
  localize: {
    month: (value: number) => String(value),
    preprocessor: (date: Date, parts: string[]) =>
      date.getDate() === 1 ? parts : [],
  },
};
const result = format({ locale: customLocale });
"#,
    );

    assert!(source.contains("preprocessor:"), "{source}");
    assert!(
        !source.contains("borrow_mut())(Default::default(),"),
        "{source}"
    );
    assert!(source.contains("smelt_call_args.push(arg0);"), "{source}");
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
        source.contains("user.get(&key.clone().clone()).cloned().expect(\"index out of bounds\")"),
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
        source.contains("(&mut *closure_arg_1.borrow_mut())((left.clone()).into_smelt_unknown(), (right.clone()).into_smelt_unknown())"),
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
        source.contains(
            "fn make_mapper() -> ::std::rc::Rc<::std::cell::RefCell<dyn FnMut(f64) -> f64>>"
        ),
        "{source}"
    );
    assert!(
        source.contains("return ::std::rc::Rc::new(::std::cell::RefCell::new(")
            || source.contains("return _smelt_tmp_2.clone()"),
        "{source}"
    );
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
        source.contains("SmeltUnknown::Object((&mut *_smelt_adapted_callback.borrow_mut())(arg0))"),
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
        source.contains("SmeltUnknown::Array(value) => value"),
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
        source.contains("SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value"),
        "{source}"
    );
    assert!(source.contains(".match_string(&match "), "{source}");
}

#[test]
fn emits_string_match_with_regexp_flags_preserved() {
    let source = source_for(
        r#"
function parts(value: string): string[] | undefined {
  const tokens = /a+|b/g;
  return value.match(tokens);
}
"#,
    );

    assert!(
        source.contains("SmeltRegExp::new(\"a+|b\".to_owned(), \"g\".to_owned())"),
        "{source}"
    );
    assert!(source.contains("tokens.clone().match_string(&"), "{source}");
}

#[test]
fn emits_javascript_any_character_regex_translation() {
    let source = source_for(
        r#"
function inner(value: string): string[] | undefined {
  return value.match(/^'([^]*?)'?$/);
}
"#,
    );

    assert!(source.contains("replace(\"[^]\", \"(?s:.)\")"), "{source}");
    assert!(source.contains(".match_string(&"), "{source}");
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

#[test]
fn wraps_function_return_values_from_unknown_adapters() {
    let source = source_for(
        r#"
function outer(make: () => (value: unknown) => unknown): unknown {
  return [make];
}
"#,
    );

    assert!(source.contains("SmeltUnknown::Function"), "{source}");
    assert!(!source.contains("()).into_smelt_unknown()"), "{source}");
}

#[test]
fn owns_callback_params_that_escape_through_unknown_values() {
    let source = source_for(
        r#"
function expose(callback: (value: unknown) => unknown): unknown {
  return { callback };
}
"#,
    );

    assert!(
        source.contains("fn expose(callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut(SmeltUnknown) -> SmeltUnknown>>)"),
        "{source}"
    );
    assert!(
        !source.contains("fn expose(callback: &mut dyn FnMut"),
        "{source}"
    );
    assert!(source.contains("SmeltUnknown::Function"), "{source}");
}

#[test]
fn emits_rejects_to_throw_as_awaited_result_match() {
    let source = source_for(
        r#"
import { expect, test } from "vitest";

async function fail(): Promise<void> {
  throw "boom";
}

test("rejects", async () => {
  await expect(fail()).rejects.toThrow("boom");
});
"#,
    );

    assert!(source.contains("match _smelt_tmp_"), "{source}");
    assert!(source.contains(".await {"), "{source}");
    assert!(source.contains("Err(__smelt_error)"), "{source}");
    assert!(
        source.contains("contains(&\"boom\".to_owned())"),
        "{source}"
    );
}

#[test]
fn flattens_optional_chain_over_optional_callback_field() {
    let source = source_for(
        r#"
interface Options {
  cb?: (value: number) => number;
}

export function read(options?: Options): ((value: number) => number) | undefined {
  return options?.cb;
}
"#,
    );

    assert!(source.contains(".as_ref().and_then("), "{source}");
    assert!(!source.contains("Option<Option<::std::rc::Rc"), "{source}");
}

#[test]
fn preserves_optional_callable_values_across_compatible_parameter_adaptation() {
    let source = source_for(
        r#"
interface Options {
  in?: (value: unknown) => unknown;
}

function consume(context?: (value: Date) => unknown): void {}

export function run(options?: Options): void {
  consume(options?.in);
}
"#,
    );

    assert!(source.contains(".clone().map(|value|"), "{source}");
    assert!(!source.contains("map_or(None::<"), "{source}");
}

#[test]
fn emits_optional_callable_logical_or_as_selected_unknown_value() {
    let source = source_for(
        r#"
function select(value: unknown, context?: (value: unknown) => unknown): unknown {
  return context || value;
}
"#,
    );

    assert!(source.contains(".map_or_else("), "{source}");
    assert!(source.contains("SmeltUnknown::Function"), "{source}");
    assert!(!source.contains(".is_some() ||"), "{source}");
}

#[test]
fn flattens_python_nested_optional_annotations() {
    let source = source_for_py(
        r#"
from typing import Optional

def read(value: Optional[Optional[int]]) -> Optional[int]:
    return value
"#,
    );

    assert!(source.contains("value: Option<i64>"), "{source}");
    assert!(!source.contains("Option<Option<i64>>"), "{source}");
}

#[test]
fn flattens_python_dict_get_optional_value_type() {
    let source = source_for_py(
        r#"
from typing import Optional

def read(values: dict[str, Optional[int]]) -> Optional[int]:
    return values.get("x")
"#,
    );

    assert!(
        source.contains("values: ::std::collections::HashMap<String, Option<i64>>"),
        "{source}"
    );
    assert!(!source.contains("Option<Option<i64>>"), "{source}");
}

#[test]
fn flattens_optional_list_index_over_optional_items() {
    let source = source_for(
        r#"
function read(values: Array<number | undefined>, index: number): number | undefined {
  return values?.[index];
}
"#,
    );

    assert!(source.contains(".cloned().flatten()"), "{source}");
    assert!(!source.contains("Option<Option<f64>>"), "{source}");
}
