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
    assert!(ts_source.contains(".to_iso_string()"));
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
fn emits_vitest_date_now_mock_state() {
    let source = source_for(
        r#"
import { vi } from "vitest";

vi.useFakeTimers({ now: new Date(2020, 0, 1) });
const initial = Date.now();
vi.setSystemTime(new Date(2020, 0, 2));
const updated = Date.now();
vi.useRealTimers();
"#,
    );

    assert!(source.contains("SMELT_DATE_NOW"), "{source}");
    assert!(source.contains("value.set(Some("), "{source}");
    assert!(source.contains("with(::std::cell::Cell::get)"), "{source}");
    assert!(source.contains("value.set(None)"), "{source}");
}

#[test]
fn isolates_vitest_date_now_state_at_native_test_entry() {
    let source = source_for(
        r#"
import { test, vi } from "vitest";

test("sets clock", () => {
  vi.setSystemTime(new Date(2020, 0, 1));
  Date.now();
});
test("reads real clock", () => {
  Date.now();
});
"#,
    );

    assert!(
        source
            .matches("SMELT_DATE_NOW.with(|value| value.set(None));")
            .count()
            >= 2,
        "{source}"
    );
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
    assert!(
        source.contains("\"__smelt_timezone\".to_owned()"),
        "{source}"
    );
    assert!(manifest.contains("chrono-tz = \"0.10\""), "{manifest}");
}

#[test]
fn preserves_timezone_context_for_iso_output_and_dst_gaps() {
    let source = source_for(
        r#"
import { tz } from "@date-fns/tz";
declare const value: unknown;
const inNewYork = tz("America/New_York");
const result = inNewYork(value);
const iso = result.toISOString();
"#,
    );

    assert!(
        source.contains("\"__smelt_timezone\".to_owned()"),
        "{source}"
    );
    assert!(
        source.contains("chrono::TimeZone::from_local_datetime(&timezone, &local)"),
        "{source}"
    );
    assert!(
        source.contains("local += chrono::Duration::minutes(1)"),
        "{source}"
    );
}

#[test]
fn preserves_erased_date_metadata_across_setters() {
    let source = source_for(
        r#"
declare const value: unknown;
const date = new Date(value);
date.setFullYear(2024);
const iso = date.toISOString();
"#,
    );

    assert!(
        source.contains("if key != \"__smelt_date\""),
        "Date setters must preserve metadata from erased Date receivers: {source}"
    );
    assert!(
        source.contains("result.insert(key, value)"),
        "Date metadata must be copied onto the replacement timestamp object: {source}"
    );
}

#[test]
fn hides_internal_metadata_from_erased_object_projection() {
    let source = source_for(
        r#"
function keyCount(value: unknown): number {
  return Object.keys(value).length;
}
const dateCount = keyCount(new Date(0));
const regexpCount = keyCount(/abc/u);
"#,
    );

    assert!(
        source.contains("smelt_is_for_in_record_key"),
        "erased object projections must filter internal metadata keys: {source}"
    );
    assert!(
        source.contains("\"__smelt_regexp\".to_owned()"),
        "RegExp erasure must keep a marker so source/flags stay non-enumerable: {source}"
    );
    assert!(
        source.contains("SmeltRegExp::new(\"abc\".to_owned(), \"u\".to_owned())"),
        "RegExp literal arguments should lower to RegExp values, not strings: {source}"
    );
}

#[test]
fn omits_private_class_fields_from_erased_object_projection() {
    let source = source_for(
        r#"
class PrivateBox {
  readonly #value = "hidden";
}
function keyCount(value: unknown): number {
  return Object.keys(value).length;
}
const count = keyCount(new PrivateBox());
"#,
    );

    assert!(
        !source.contains("(\"value\".to_owned(), SmeltUnknown::String(self.value))"),
        "private class fields must not become enumerable erased object keys: {source}"
    );
    assert!(
        !source.contains("smelt_object_entries.insert(\"value\".to_owned()"),
        "direct class-to-unknown erasure must also omit private fields: {source}"
    );
}

#[test]
fn omits_class_getters_from_erased_object_projection() {
    let source = source_for(
        r#"
class GetterBox {
  get value(): string {
    return "hidden";
  }
}
function keyCount(value: unknown): number {
  return Object.keys(value).length;
}
const count = keyCount(new GetterBox());
"#,
    );

    assert!(
        !source.contains("smelt_object_entries.insert(\"value\".to_owned()"),
        "class accessors live on the prototype and must not erase as own enumerable fields: {source}"
    );
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
        source.contains("to_iso_string()") && source.contains("SmeltUnknown::String(_smelt_tmp_"),
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
    assert!(source.contains("_ => SmeltRecord::new()"), "{source}");
}

#[test]
fn casts_unknown_arrays_to_string_keyed_records() {
    let source = source_for(
        r#"
declare const value: unknown;
const bag: Record<string, unknown> = value as Record<string, unknown>;
const first = bag["0"];
"#,
    );

    assert!(
        source.contains(
            "SmeltUnknown::Array(values) => values.into_iter().enumerate().map(|(index, value)| (index.to_string(), value)).collect()"
        ),
        "{source}"
    );
}

#[test]
fn emits_javascript_array_property_key_coercion() {
    let source = source_for(
        r#"
const record: Record<string, number> = { short: 5 };
const key = ["short"];
const value = record[key];
"#,
    );

    assert!(
        source.contains(".collect::<Vec<_>>().join(\",\")"),
        "{source}"
    );
    assert!(
        !source.contains("values.get(&\"[object Object]\""),
        "{source}"
    );
}

#[test]
fn preserves_order_when_casting_unknown_objects_to_records() {
    let source = source_for(
        r#"
declare const value: unknown;
const bag: Record<string, unknown> = value as Record<string, unknown>;
const entries = Object.entries(bag);
"#,
    );

    assert!(
        source.contains("fn with_id_from_entries<I: IntoIterator<Item = (K, V)>>"),
        "{source}"
    );
    assert!(
        source.contains(
            "SmeltUnknown::Object(value) => SmeltRecord::with_id_from_entries(value.id, value.into_iter())"
        ),
        "{source}"
    );
}

#[test]
fn preserves_unknown_elements_when_casting_to_erased_type_level_helpers() {
    let source = source_for(
        r#"
import type { Simplify } from "type-fest";

type Entry<T> = Simplify<T>;

declare const value: unknown;
const entries = value as Entry<string>[];
"#,
    );

    assert!(
        source.contains("values.into_iter().map(|value| value).collect::<Vec<_>>()"),
        "erased helper items should preserve unknown values instead of defaulting: {source}"
    );
    assert!(
        !source.contains("values.into_iter().map(|value| Default::default()).collect::<Vec<_>>()"),
        "erased helper items should not be replaced with defaults: {source}"
    );
}

#[test]
fn emits_first_class_object_entries_as_real_projection() {
    let source = source_for(
        r#"
declare function purry(fn: (value: unknown) => unknown, args: readonly unknown[]): unknown;

export function entries(...args: readonly unknown[]): unknown {
  return purry(Object.entries, args);
}
"#,
    );

    assert!(
        source.contains("SmeltRecord::with_id_from_entries"),
        "Object.entries callback should cast its argument through record projection: {source}"
    );
    assert!(
        source.contains(
            "SmeltUnknown::Array(_smelt_tmp_2.clone().into_iter().map(|value| SmeltUnknown::Array"
        ),
        "Object.entries callback should return entry arrays, not a null placeholder: {source}"
    );
    assert!(
        !source
            .contains("::std::rc::Rc::new(|closure_arg_0: SmeltUnknown| {\n    SmeltUnknown::Null"),
        "Object.entries callback should not lower to the static-member placeholder closure: {source}"
    );
}

#[test]
fn emits_first_class_object_from_entries_as_real_conversion() {
    let source = source_for(
        r#"
declare function purry(fn: (value: unknown) => unknown, args: readonly unknown[]): unknown;

export function fromEntries(...args: readonly unknown[]): unknown {
  return purry(Object.fromEntries, args);
}
"#,
    );

    assert!(
        source.contains("collect::<SmeltRecord<String, SmeltUnknown>>()"),
        "Object.fromEntries callback should collect entry arrays into a record: {source}"
    );
    assert!(
        !source
            .contains("::std::rc::Rc::new(|closure_arg_0: SmeltUnknown| {\n    SmeltUnknown::Null"),
        "Object.fromEntries callback should not lower to the static-member placeholder closure: {source}"
    );
}

#[test]
fn emits_array_iterator_next_as_typed_option() {
    let source = source_for(
        r#"
export function firstEntry(values: string[]): string | undefined {
  const iterator = values.entries();
  const result = iterator.next();
  if ("done" in result && result.done) {
    return undefined;
  }
  return result.value[1];
}
"#,
    );

    assert!(
        source.contains("if iterator.is_empty() { None } else { Some(iterator.remove(0)) }"),
        "iterator next should consume into a typed Option: {source}"
    );
    assert!(
        source.contains("result.clone().is_none()"),
        "iterator done should inspect the typed Option: {source}"
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
    assert!(source.contains("\"__smelt_error\".to_owned()"), "{source}");
    assert!(source.contains("\"message\".to_owned()"), "{source}");
}

#[test]
fn emits_error_constructor_values_with_runtime_error_identity() {
    let source = source_for(
        r#"
function makeError(): unknown {
  return new Error("bad");
}

const value = makeError();
const yes = value instanceof Error;
"#,
    );

    assert!(source.contains("\"__smelt_error\".to_owned()"), "{source}");
    assert!(source.contains("\"message\".to_owned()"), "{source}");
    assert!(
        source.contains("value.contains_key(\"__smelt_error\")"),
        "{source}"
    );
    assert!(
        source.contains("object.contains_key(\"__smelt_error\")")
            && source.contains("matches!(key, \"__smelt_error\" | \"message\")"),
        "{source}"
    );
}

#[test]
fn copies_erased_object_rest_destructuring_properties() {
    let source = source_for(
        r#"
declare function getValue(): unknown;
declare function makeCall(): (...args: unknown[]) => void;

const source = getValue() as { call: (...args: unknown[]) => void; cancel: () => void; flush: () => void };
const { call, ...rest } = source;
const callable = makeCall();
const merged = Object.assign(callable, rest);
merged.cancel();
merged.flush();
"#,
    );

    assert!(
        source.contains("SmeltRecord::with_id_from_entries(map.id, map.into_iter())")
            || source.contains("SmeltRecord::with_id_from_entries(values.id"),
        "{source}"
    );
    assert!(source.contains(".remove(&\"call\".to_owned())"), "{source}");
    assert!(source.contains("assigned.extend("), "{source}");
}

#[test]
fn dynamic_object_destructuring_defaults_do_not_require_missing_fields() {
    let source = source_for(
        r#"
export function pick(options: Record<string, unknown>): unknown {
  const { leading = false, trailing = true, maxWait } = options;
  return [leading, trailing, maxWait];
}
"#,
    );

    assert!(
        source.contains(".get(&\"leading\".to_owned()).unwrap_or("),
        "{source}"
    );
    assert!(
        source.contains(".get(&\"trailing\".to_owned()).unwrap_or("),
        "{source}"
    );
    assert!(
        source.contains(".get(&\"maxWait\".to_owned()).unwrap_or("),
        "{source}"
    );
    assert!(!source.contains("expect(\"missing field\")"), "{source}");
}

#[test]
fn typed_option_bag_parameter_defaults_do_not_require_missing_fields() {
    let source = source_for(
        r#"
export function pick(
  {
    leading = false,
    trailing = true,
    maxWait,
  }: {
    readonly leading?: boolean;
    readonly trailing?: boolean;
    readonly maxWait?: number;
  } = {},
): unknown {
  return [leading, trailing, maxWait];
}
"#,
    );

    assert!(
        source.contains(".get(&\"leading\".to_owned()).unwrap_or("),
        "{source}"
    );
    assert!(
        source.contains(".get(&\"trailing\".to_owned()).unwrap_or("),
        "{source}"
    );
    assert!(
        source.contains(".get(&\"maxWait\".to_owned()).unwrap_or("),
        "{source}"
    );
    assert!(!source.contains("expect(\"missing field\")"), "{source}");
}

#[test]
fn emits_runtime_sized_numeric_typed_array_constructors() {
    let source = source_for(
        r#"
export function make(count: number): number[] {
  const output = new Uint8Array(count);
  for (let index = 0; index < count; index += 1) {
    output[index] = index + 1;
  }
  return output;
}
"#,
    );

    assert!(source.contains("vec![0.0; smelt_repeat_count]"), "{source}");
    assert!(!source.contains("vec![0.0, 0.0, 0.0, 0.0"), "{source}");
}

#[test]
fn inserts_unknown_iterable_values_into_typed_sets() {
    let source = source_for(
        r#"
declare function values(): unknown;

export function collect(): Set<number> {
  const results = new Set<number>();
  for (const value of values() as Iterable<unknown>) {
    results.add(value as number);
  }
  return results;
}
"#,
    );

    assert!(
        source.contains("match value.clone().clone()")
            && (source.contains("results.push(") || source.contains("results.insert(")),
        "{source}"
    );
    assert!(!source.contains("Default::default();\n"), "{source}");
}

#[test]
fn parses_javascript_date_to_string_input() {
    let source = source_for(
        r#"
const value = new Date("Wed Jul 02 2014 05:30:15 GMT+0600");
"#,
    );

    assert!(
        source.contains("%a %b %d %Y %H:%M:%S GMT%z"),
        "standard JavaScript Date string input needs its GMT-offset parser: {source}"
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
        source.contains("SmeltUnknown::Object(value) => match value.get(\"__smelt_date\")"),
        "{source}"
    );
    assert!(source.contains("SmeltUnknown::Null => 0.0"), "{source}");
    assert!(source.contains("smelt_text.is_empty() { 0.0 }"), "{source}");
    assert!(
        source.contains("SmeltUnknown::Array(_) | SmeltUnknown::Function(_) => f64::NAN"),
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
        source.contains("(format_long.date.clone())({ let smelt_record_map ="),
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
fn preserves_optional_record_arguments_and_defaults_missing_fields() {
    let source = source_for(
        r#"
function throttle(
  callback: () => void,
  wait = 0,
  { leading = true, trailing = true }: { leading?: boolean; trailing?: boolean } = {},
): boolean {
  callback();
  return leading && trailing && wait >= 0;
}

const explicit = throttle(() => {}, 1, { leading: false });
const defaulted = throttle(() => {});
"#,
    );

    assert!(
        source.contains(".get(&\"leading\".to_owned()).flatten()")
            && source.contains(".unwrap_or(true)"),
        "{source}"
    );
    assert!(
        source.contains("SmeltRecord::from([(\"leading\".to_owned(), Some(false))])"),
        "{source}"
    );
    assert!(
        source.contains(", 1.0, _smelt_tmp_3.clone())"),
        "explicit object argument should remain in the call: {source}"
    );
}

#[test]
fn captures_callable_factory_result_for_recursive_callback_invocation() {
    let source = source_for(
        r#"
type Fn = (value: string) => void;

function wrap(callback: Fn): Fn {
  return callback;
}

export function run(): void {
  const recursive = wrap((value: string) => {
    recursive(value);
  });
  recursive("a");
}
"#,
    );

    assert!(
        source.contains("smelt_capture_recursive"),
        "recursive callable result should be captured by its callback: {source}"
    );
    assert!(
        !source.contains("SmeltRecord::from([])"),
        "recursive callback should not fall back to an empty callable object: {source}"
    );
}

#[test]
fn captures_recursive_callable_factory_result_inside_test_callback() {
    let source = source_for(
        r#"
import { test } from "vitest";

type Fn = (value: string) => void;

function wrap(callback: Fn): Fn {
  return callback;
}

test("recursive", async () => {
  const recursive = wrap((value: string) => {
    recursive(value);
  });
  recursive("a");
});
"#,
    );

    assert!(
        source.contains("smelt_capture_recursive"),
        "test callbacks must retain recursive callable result storage: {source}"
    );
}

#[test]
fn invokes_void_erased_rest_closures_before_returning_null() {
    let source = source_for(
        r#"
type RestCallback = (...args: unknown[]) => void;

export function make(): RestCallback {
  return (...args: unknown[]) => {
    args.push("called");
  };
}
"#,
    );

    assert!(
        source.contains("let smelt_callback = ::std::rc::Rc::new(")
            && source.contains(
                "move |smelt_args: Vec<SmeltUnknown>| { (smelt_callback)(smelt_args"
            )
            && !source.contains("SmeltErasedFunction { callback: { let smelt_callback = ::std::rc::Rc::new(::std::cell::RefCell::new("),
        "an erased void callback wrapper must invoke its source closure through a reentrant handle: {source}"
    );
    assert!(
        source.contains("SmeltUnknown::Null"),
        "an erased void callback wrapper must still produce undefined/null ABI output: {source}"
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
            .matches("let smelt_fn: ::std::rc::Rc<dyn Fn(f64) -> String>")
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
fn unwraps_throwing_array_map_callbacks_before_collecting() {
    let source = source_for(
        r#"
function run(values: string[]): string[] {
  return values.map((value) => {
    if (value === "x") {
      throw new Error(value);
    }
    return value;
  });
}
"#,
    );

    assert!(
        source.contains(".map(|(index, item)| { ((smelt_callback)(")
            && source.contains(
                ")).unwrap_or_else(|error: Box<dyn std::error::Error>| panic!(\"{}\", error))"
            ),
        "{source}"
    );
}

#[test]
fn emits_contextual_string_arrow_number_suffix_addition_as_concat() {
    let source = source_for(
        r#"
type LocalizeFn<Value> = (value: Value, options?: { unit?: string }) => string;
type Localize = {
  ordinalNumber: LocalizeFn<number>;
};

const feminineUnits = ["second", "minute"];

const ordinalNumber: LocalizeFn<number> = (dirtyNumber, options) => {
  const number = Number(dirtyNumber);
  const unit = options?.unit;
  if (number === 0) return "0";
  let suffix;
  if (number === 1) {
    suffix = unit && feminineUnits.includes(unit) ? "ère" : "er";
  } else {
    suffix = "ème";
  }
  return number + suffix;
};

export const localize: Localize = {
  ordinalNumber,
};
"#,
    );

    assert!(
        source.contains("number.clone().to_string() + &match suffix.clone()"),
        "{source}"
    );
    assert!(
        !source.contains("let _smelt_tmp_16: f64")
            && !source.contains("return _smelt_tmp_16.clone();"),
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
        source
            .contains("impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> Default for Localize<T>"),
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
        source
            .contains("impl<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static> ::std::fmt::Debug for Localize<T>"),
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
    assert!(source.contains("|closure_arg_0: String|"), "{source}");
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
        source.contains("implementation: ::std::rc::Rc<dyn Fn"),
        "{source}"
    );
    assert!(source.contains("(implementation)("), "{source}");
    assert!(source.contains("args.get("), "{source}");
    assert!(
        source.contains("args.clone().iter().skip(")
            || source.contains("SmeltUnknown::Array(args.clone().into())"),
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
fn emits_mutable_structural_parameters_when_fields_are_assigned() {
    let source = source_for(
        r#"
interface Flags {
  era?: number;
}

function setEra(flags: Flags, value: number): void {
  flags.era = value;
}

function readEra(): number {
  const flags: Flags = {};
  setEra(flags, 1);
  return flags.era!;
}
"#,
    );

    assert!(
        source.contains("fn set_era(mut flags: &mut Flags, value: f64)"),
        "{source}"
    );
    assert!(source.contains("set_era(&mut flags"), "{source}");
}

#[test]
fn emits_mutable_function_field_parameters_for_structural_objects() {
    let source = source_for(
        r#"
interface Flags {
  era?: number;
}

interface Setter {
  set: (flags: Flags) => number | [number, Flags];
}

function run(setter: Setter): number {
  const flags: Flags = {};
  setter.set(flags);
  return flags.era!;
}
"#,
    );

    assert!(
        source.contains("set: ::std::rc::Rc<dyn Fn(&mut Flags) -> SmeltUnknown>"),
        "{source}"
    );
    assert!(
        source.contains("(setter.set.clone())(&mut flags)"),
        "{source}"
    );
}

#[test]
fn preserves_mutable_structural_parameters_forwarded_to_callbacks() {
    let source = source_for(
        r#"
interface Flags {
  era?: number;
}

interface Setter {
  set: (flags: Flags) => number | [number, Flags];
}

function forward(flags: Flags, setter: Setter): number | [number, Flags] {
  return setter.set(flags);
}

function run(setter: Setter): number {
  const flags: Flags = {};
  forward(flags, setter);
  return flags.era!;
}
"#,
    );

    assert!(
        source.contains("fn forward(mut flags: &mut Flags"),
        "forwarding into a mutable callback must preserve the caller's object: {source}"
    );
    assert!(
        source.contains("(setter.set.clone())(flags)"),
        "the forwarded callback must receive the existing mutable reference: {source}"
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
    assert!(source.contains("value: T,"), "{source}");
    assert!(!source.contains("value: SmeltUnknown,"), "{source}");
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
fn emits_array_entries_for_guarded_erased_generic() {
    let source = source_for(
        r#"
function copy<T>(value: T): unknown[] {
  const copied: unknown[] = [];
  if (Array.isArray(value)) {
    for (const [index, item] of value.entries()) {
      copied[index] = item;
    }
  }
  return copied;
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Array(values) => values.into_iter().enumerate()"),
        "{source}"
    );
    assert!(
        !source.contains("Vec<(i64, SmeltUnknown)> = Default::default()"),
        "{source}"
    );
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

    assert!(
        source.contains("SmeltUnknown::Array(vec![].into())"),
        "{source}"
    );
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

    assert!(source.contains("\"\".to_owned() + &match"), "{source}");
    assert!(
        source.contains("closure_arg_0.clone().to_lowercase()"),
        "{source}"
    );
}

#[test]
fn emits_case_conversion_for_erased_callback_string_indexes() {
    let source = source_for(
        r#"
function initialCaps<T extends string>(values: T[]): string[] {
  return values.map((value) => value[0].toUpperCase());
}
"#,
    );

    assert!(source.contains("SmeltUnknown::String(value)"), "{source}");
    assert!(source.contains("value.chars().nth(index)"), "{source}");
    assert!(source.contains(".to_uppercase()"), "{source}");
    assert!(!source.contains("String::new().to_uppercase()"), "{source}");
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
fn emits_virtual_method_slots_for_abstract_base_adapters() {
    let source = source_for(
        r#"
type ParseResult<T> = { value: T };

abstract class Parser<T> {
  run(input: string): T {
    const result = this.parse(input);
    if (!this.validate(result.value)) {
      return result.value;
    }
    return result.value;
  }

  validate(value: T): boolean {
    return true;
  }

  abstract parse(input: string): ParseResult<T>;
}

class YearParser extends Parser<number> {
  parse(input: string): ParseResult<number> {
    return { value: 2017 };
  }
}

const parsers: Record<string, Parser<any>> = { y: new YearParser() };

export function read(): number {
  return parsers["y"].run("ignored");
}
"#,
    );

    assert!(
        source.contains("parse: ::std::rc::Rc<dyn Fn(String) -> SmeltUnknown>"),
        "{source}"
    );
    assert!(
        source.contains("::std::rc::Rc::new(move |arg0: String|"),
        "{source}"
    );
    assert!(
        source.contains("smelt_method_receiver.parse(arg0.clone())"),
        "{source}"
    );
    assert!(
        source.contains("(self.parse.clone())(input.clone())"),
        "{source}"
    );
    assert!(!source.contains("parse: Default::default()"), "{source}");
}

#[test]
fn calls_base_typed_virtual_methods_through_stored_function_fields() {
    let source = source_for(
        r#"
abstract class Setter {
  validate(value: number): boolean {
    return true;
  }
}

class ValueSetter extends Setter {
  validate(value: number): boolean {
    return value >= 0 && value <= 11;
  }
}

const setter: Setter = new ValueSetter();
const accepted = setter.validate(12);
"#,
    );

    assert!(
        source.contains("(setter.validate.clone())(12.0)"),
        "base-typed virtual calls should dispatch through the stored function field: {source}"
    );
    assert!(
        !source.contains("setter.validate(12.0)"),
        "base inherent method calls lose subclass overrides: {source}"
    );
}

#[test]
fn binds_stored_virtual_methods_when_reerasing_base_class_values() {
    let source = source_for(
        r#"
type Result<T> = { value: T };

abstract class Parser<T> {
  abstract parse(input: string): Result<T>;
}

class ForwardingParser<T> extends Parser<T> {
  constructor(readonly delegate: Parser<T>) {
    super();
  }

  parse(input: string): Result<T> {
    return this.delegate.parse(input);
  }
}

class YearParser extends Parser<number> {
  parse(input: string): Result<number> {
    return { value: 2017 };
  }
}

const parser: Parser<any> = new ForwardingParser(new YearParser());
"#,
    );

    assert!(
        source
            .contains("Parser { parse: { let smelt_virtual_receiver = smelt_struct_value.clone();"),
        "{source}"
    );
    assert!(
        source.contains("smelt_method_receiver.parse(arg0.clone())"),
        "{source}"
    );
    assert!(!source.contains("parse: Default::default()"), "{source}");
}

#[test]
fn adapts_generic_record_fields_with_instantiated_payload_types() {
    let source = source_for(
        r#"
type MatchFnResult<T> = { value: T; rest: string };

declare const genericResult: MatchFnResult<unknown>;
const numericResult = genericResult as MatchFnResult<number>;
const value = numericResult.value;
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Number(value) => value"),
        "{source}"
    );
    assert!(!source.contains("let value: SmeltUnknown"), "{source}");
}

#[test]
fn keeps_unknown_conditionals_erased_before_string_compatible_fallbacks() {
    let source = source_for(
        r#"
declare const value: unknown;
declare const fallback: Date;
const selected: unknown = value ? value : fallback;
"#,
    );

    assert!(
        source.contains("if _smelt_tmp_3.clone() { value.clone() } else { match fallback"),
        "{source}"
    );
    assert!(
        !source.contains("selected = SmeltUnknown::String"),
        "{source}"
    );
}

#[test]
fn wraps_concrete_records_when_casting_to_erased_intersection_aliases() {
    let source = source_for(
        r#"
type A = { locale?: unknown };
type B = { weekStartsOn?: number };
type DefaultOptions = A & B;

let defaultOptions: DefaultOptions = {};

export function getDefaultOptions(): DefaultOptions {
  return defaultOptions;
}
"#,
    );

    assert!(
        source.contains("let mut _smelt_tmp_1: SmeltUnknown = SmeltUnknown::Object"),
        "{source}"
    );
    assert!(
        !source.contains("let mut _smelt_tmp_1: SmeltUnknown = _smelt_tmp_0.clone();"),
        "{source}"
    );
}

#[test]
fn omits_absent_optional_fields_when_erasing_structural_objects() {
    let source = source_for(
        r#"
interface Flags {
  era?: number;
}

declare const flags: Flags;
const erased: unknown = flags;
"#,
    );

    assert!(
        source.contains("if let Some(value) = smelt_object_value.era.clone()"),
        "{source}"
    );
    assert!(
        !source.contains("(\"era\".to_owned(), smelt_object_value.era.clone().map_or"),
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
    assert!(source.contains("smelt_unknown_date_value"), "{source}");
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
        source.contains("fn_: ::std::rc::Rc<dyn Fn(f64, f64) -> f64>"),
        "{source}"
    );
    assert!(
        source.contains("(fn_)(closure_arg_0.clone(), extra.clone())"),
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
fn reinserts_dynamic_record_list_alias_after_push() {
    let source = source_for(
        r#"
function group(values: string[]): Record<string, string[]> {
  const output: Record<string, string[]> = {};
  for (const value of values) {
    const key = value[0];
    const items = output[key];
    if (items === undefined) {
      output[key] = [value];
    } else {
      items.push(value);
    }
  }
  return output;
}
"#,
    );

    assert!(
        source.contains(".cloned().unwrap_or(Vec::new())"),
        "{source}"
    );
    assert!(
        source.contains("output.insert(key.clone().clone(), items.clone());"),
        "{source}"
    );
}

#[test]
fn preserves_computed_symbol_object_literal_properties() {
    let source = source_for(
        r#"
function read(): unknown {
  const SymbolKey = Symbol("kind");
  const item = { [SymbolKey]: "cat", [Symbol("inline")]: "dog", 2: 123 };
  return item[SymbolKey];
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Symbol(\"Symbol(kind)"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Symbol(\"Symbol(inline)"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::String(\"cat\".to_owned())"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::String(\"dog\".to_owned())"),
        "{source}"
    );
}

#[test]
fn preserves_object_has_own_length_on_erased_arrays() {
    let source = source_for(
        r#"
function hasLength(value: unknown): boolean {
  return Object.hasOwn(value, "length");
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Array(values) => smelt_key == \"length\""),
        "{source}"
    );
    assert!(
        !source.contains("SmeltRecord::with_id_from_entries"),
        "{source}"
    );
}

#[test]
fn projects_string_indices_when_casting_unknown_to_record() {
    let source = source_for(
        r#"
function keyCount(value: unknown): number {
  return Object.keys(value).length;
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::String(value) => value.chars().enumerate()"),
        "{source}"
    );
}

#[test]
fn erases_sets_without_dropping_items() {
    let source = source_for(
        r#"
function accept(value: unknown): unknown {
  return value;
}

export function run(): unknown {
  return accept(new Set([1, 2, 3]));
}
"#,
    );

    assert!(
        source.contains("IntoSmeltUnknown for ::std::collections::HashSet<T>"),
        "{source}"
    );
    assert!(
        source.contains("values.sort_by_key(smelt_unknown_stable_hash_key);"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Array(values.into())"),
        "{source}"
    );
    assert!(
        source.contains(".clone().into_iter().map(|value|"),
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
        source.contains("*other = SmeltUnknown::Object(SmeltObject::new(map));"),
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
        source.contains("FormatOptions { locale: smelt_record_map.get(\"locale\").map(|value|"),
        "{source}"
    );
    assert!(source.contains("Locale { code:"), "{source}");
    assert!(!source.contains("FormatOptions { locale: None"), "{source}");
}

#[test]
fn adapts_nested_required_records_when_retyping_mixed_option_bags() {
    let source = source_for(
        r#"
interface InputFormatLong {
  date: (value: string) => string;
}

interface InputLocale {
  formatLong: InputFormatLong;
}

interface OutputFormatLong {
  date: (value: string) => string;
}

interface OutputLocale {
  formatLong: OutputFormatLong;
}

interface FormatOptions {
  locale?: OutputLocale;
  weekStartsOn?: number;
}

function format(options?: FormatOptions): string {
  return options?.locale?.formatLong.date("P") ?? "";
}

function forward(locale: InputLocale): string {
  return format({ locale, weekStartsOn: 0 });
}
"#,
    );

    assert!(source.contains("OutputLocale {"), "{source}");
    assert!(source.contains("OutputFormatLong {"), "{source}");
    assert!(
        !source.contains(
            "locale: smelt_record_map.get(\"locale\").cloned().map(|value| Default::default())"
        ),
        "{source}"
    );
}

#[test]
fn bounds_recursive_locale_callback_option_record_adaptation() {
    let source = source_for(
        r#"
interface CallbackOptions {
  locale?: Locale;
}

interface Locale {
  formatRelative: (token: string, options?: CallbackOptions) => string;
}

function forward(value: unknown): string {
  const locale = value as Locale;
  return locale.formatRelative("today", { locale });
}
"#,
    );

    assert!(source.contains("Locale {"), "{source}");
    assert!(source.contains("format_relative:"), "{source}");
    assert!(
        !source.contains(
            "locale: smelt_record_map.get(\"locale\").cloned().map(|value| Default::default())"
        ),
        "{source}"
    );
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
    assert!(source.contains("(smelt_callback)()"), "{source}");
}

#[test]
fn adapts_shorter_callbacks_returning_records_to_optional_interface_results() {
    let source = source_for(
        r#"
interface MatchResult {
  value: number;
  rest: string;
}

interface Match {
  era: (text: string, options?: unknown) => MatchResult | null;
}

interface Locale {
  match: Match;
}

interface ParseOptions {
  locale?: Locale;
}

function useOptions(options?: ParseOptions): MatchResult | null {
  return options?.locale?.match.era("BC") ?? null;
}

const customLocale = {
  match: {
    era: () => ({ value: 0, rest: " works" }),
  },
};

const result = useOptions({ locale: customLocale });
"#,
    );

    assert!(source.contains("let smelt_adapted:"), "{source}");
    assert!(source.contains("(smelt_callback)()"), "{source}");
    assert!(
        source.contains("Option<MatchResult>> = ::std::rc::Rc::new(move |arg0: String")
            && source.contains("Some({ let smelt_record_map = (smelt_callback)().clone();"),
        "the provided shorter callback must be adapted to the optional result field: {source}"
    );
}

#[test]
fn instantiates_generic_option_defaults_without_leaking_type_params() {
    let source = source_for(
        r#"
interface ContextOptions<DateType extends Date = Date> {
  in?: (value: unknown) => DateType;
}

interface ParseOptions<DateType extends Date = Date> extends ContextOptions<DateType> {
  token?: string;
}

interface IsMatchOptions {
  token?: string;
}

function parseValue(options?: ParseOptions<unknown>): unknown {
  return options?.token ?? null;
}

function isMatch(options?: IsMatchOptions): unknown {
  return parseValue(options);
}
"#,
    );

    assert!(
        source.contains("None::<::std::rc::Rc<dyn Fn(SmeltUnknown) -> SmeltUnknown>>"),
        "generic callback defaults should use the instantiated option payload: {source}"
    );
    assert!(
        source.contains(
            "ParseOptions { in_: None::<::std::rc::Rc<dyn Fn(SmeltUnknown) -> SmeltUnknown>>"
        ),
        "struct literal defaults must not leak declaration type parameters: {source}"
    );
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
    assert!(!source.contains(")(Default::default(),"), "{source}");
    assert!(
        source.contains("smelt_call_args.push(match arg0.clone()"),
        "{source}"
    );
    assert!(source.contains("\"__smelt_date\".to_owned()"), "{source}");
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
        source.contains("user.get(&key.clone().clone()).cloned().unwrap_or(String::new())"),
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
    assert!(
        source.contains("(smelt_callback)(SmeltUnknown::Null, index as f64)"),
        "{source}"
    );
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

    assert!(source.contains("closure_arg_0.clone() as i64"), "{source}");
    assert!(source.contains("usize::try_from(normalized)"), "{source}");
}

#[test]
fn emits_erased_string_key_index_reads_for_arrays_and_strings() {
    let source = source_for(
        r#"
export function read(value: unknown, key: string): unknown {
  return value[key as keyof typeof value];
}
"#,
    );

    assert!(source.contains("SmeltUnknown::Array(values)"), "{source}");
    assert!(source.contains("SmeltUnknown::String(value)"), "{source}");
    assert!(source.contains("smelt_key == \"length\""), "{source}");
    assert!(source.contains("parse::<usize>()"), "{source}");
    assert!(!source.contains("unknown is not object"), "{source}");
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
        source.contains("(closure_arg_1)((left.clone()).into_smelt_unknown(), (right.clone()).into_smelt_unknown())"),
        "{source}"
    );
}

#[test]
fn emits_spread_sort_for_erased_iterable_generic() {
    let source = source_for(
        r#"
type IterableContainer<T> = readonly T[];

const numberComparator = (a: number, b: number): number => a - b;

function sorted<T extends IterableContainer<number>>(data: T): unknown {
  return [...data].sort(numberComparator);
}
"#,
    );

    assert!(source.contains(".sort_by(|left, right|"), "{source}");
    assert!(
        source.contains("SmeltUnknown::Array(value) => smelt_array_sort_method(value)"),
        "{source}"
    );
}

#[test]
fn emits_object_symbol_iterator_generator_as_erased_iterable() {
    let source = source_for(
        r#"
function count(items: Iterable<unknown>): number {
  const erased = items as unknown;
  return [...(erased as Iterable<unknown>)].length;
}

const result = count({
  *[Symbol.iterator]() {
    yield 0;
    yield 1;
  },
});
"#,
    );

    assert!(source.contains("\"__smelt_symbol_iterator\""), "{source}");
    assert!(
        source.contains("iterator(vec![]).unwrap_or(SmeltUnknown::Null)"),
        "{source}"
    );
    assert!(
        !source.contains("else { panic!(\"unknown is not array\") })"),
        "{source}"
    );
}

#[test]
fn adapts_rest_callback_without_flattening_list_arguments() {
    let source = source_for(
        r#"
function purry<T>(
  callback: (...args: unknown[]) => unknown,
): (data: readonly T[], compare: (left: T, right: T) => number) => unknown {
  const n = 2;
  return (data, compare) => callback(data, compare, n);
}

function wrap<T>(
  func: (data: readonly T[], compare: (left: T, right: T) => number, n: number) => unknown,
): unknown {
  const n = 2;
  return purry((...args) => func(...args, n));
}
"#,
    );

    assert!(
        source.contains("match closure_arg_0.get(")
            && source.contains("SmeltUnknown::Array(values) => values.into_iter()"),
        "fixed callback spread calls should read the first fixed parameter from the rest vector: {source}"
    );
    assert!(
        source.contains("match closure_arg_0.get("),
        "fixed callback spread calls should read the second fixed parameter from the rest vector: {source}"
    );
    assert!(
        source.contains("}, n.clone())"),
        "fixed callback spread calls should keep trailing scalar arguments after spread expansion: {source}"
    );
}

#[test]
fn packs_rest_arguments_for_normal_closure_calls() {
    let source = source_for(
        r#"
export function makeIdentity(): (first: unknown, ...rest: unknown[]) => unknown {
  return (first: unknown) => first;
}

export function run(): unknown {
  const identity = makeIdentity();
  return identity("hello");
}
"#,
    );

    assert!(
        source.contains(
            "(identity)(SmeltUnknown::String(\"hello\".to_owned()), _smelt_tmp_2.clone())"
        ) && source.contains("_smelt_tmp_2 = vec![];"),
        "normal closure calls should preserve fixed arguments and pack an empty rest vector: {source}"
    );
}

#[test]
fn emits_void_return_inside_loop_branch() {
    let source = source_for(
        r#"
function stopWhenSorted(items: number[]): void {
  let index = 0;
  while (index < items.length) {
    if (items[index]! >= 0) {
      return;
    }
    index += 1;
  }
}
"#,
    );

    assert!(
        source.contains("if _smelt_tmp_"),
        "expected a loop branch in the generated function: {source}"
    );
    assert!(
        source.contains("return;"),
        "void returns inside loop branches must emit a Rust return: {source}"
    );
}

#[test]
fn emits_non_escaping_closure_that_captures_borrowed_callback_param() {
    let source = source_for(
        r#"
function visit<T>(
  data: readonly T[],
  callback: (value: T, index: number, data: readonly T[]) => boolean,
): number {
  return callback(data[0]!, 0, data) ? 1 : 0;
}

function indicesSeen(
  items: readonly unknown[],
  predicate: (item: unknown, index: number) => boolean,
): number[] {
  const indices: number[] = [];
  visit(items, (pivot, index) => {
    indices.push(index);
    return predicate(pivot, index);
  });
  return indices;
}
"#,
    );

    assert!(
        source.contains("predicate(closure_arg_0.clone(), closure_arg_1.clone())"),
        "{source}"
    );
    assert!(
        source.contains("(*smelt_capture_indices.borrow_mut()).push(closure_arg_1.clone())"),
        "captured push should mutate the outer vector storage: {source}"
    );
    assert!(
        !source.contains("smelt_default_callback"),
        "non-escaping callback parameter capture should not default: {source}"
    );
}

#[test]
fn emits_generic_iterable_spread_concat_through_unknown_lists() {
    let source = source_for(
        r#"
function concatImplementation<T1, T2>(arr1: T1, arr2: T2): unknown[] {
  return [...arr1 as unknown[], ...arr2 as unknown[]];
}
"#,
    );

    assert!(
        source.contains(".iter().cloned().chain("),
        "generic iterable spread concat should not collapse to a default vector: {source}"
    );
    assert!(
        !source.contains("return Default::default();"),
        "generic iterable spread concat should preserve the unknown array values: {source}"
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
        source.contains("fn make_mapper() -> ::std::rc::Rc<dyn Fn(f64) -> f64>"),
        "{source}"
    );
    assert!(
        source.contains("return ::std::rc::Rc::new(")
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
        source.contains("SmeltUnknown::Object(SmeltObject::from_unknown_record(((_smelt_adapted_callback)(arg0)).clone()))"),
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
fn emits_regexp_array_elements_with_flags_preserved() {
    let source = source_for(
        r#"
const patterns = [/x/u, /x/gimu];
const pattern = patterns[1];
const usesGlobal = pattern.global;
const ignoresCase = pattern.ignoreCase;
const usesMultiline = pattern.multiline;
"#,
    );

    assert!(
        source.contains("SmeltRegExp::new(\"x\".to_owned(), \"u\".to_owned())"),
        "{source}"
    );
    assert!(
        source.contains("SmeltRegExp::new(\"x\".to_owned(), \"gimu\".to_owned())"),
        "{source}"
    );
    assert!(source.contains(".has_flag('g')"), "{source}");
    assert!(source.contains(".has_flag('i')"), "{source}");
    assert!(source.contains(".has_flag('m')"), "{source}");
}

#[test]
fn emits_javascript_string_split_empty_separator_and_limit_semantics() {
    let source = source_for(
        r#"
function splitChars(value: string): string[] {
  return value.split("");
}

function splitNegative(value: string): string[] {
  return value.split(",", -1);
}
"#,
    );

    assert!(source.contains("if smelt_separator.is_empty()"), "{source}");
    assert!(
        source.contains("if smelt_haystack.is_empty() { Vec::new() }"),
        "{source}"
    );
    assert!(
        source.contains("else if smelt_limit.is_sign_positive()"),
        "{source}"
    );
    assert!(!source.contains(".max(0.0) as usize"), "{source}");
}

#[test]
fn emits_erased_regexp_test_with_flags_preserved() {
    let source = source_for(
        r#"
const patterns: unknown = [/^n/i];
const first = (patterns as any)[0];
const matches = first.test("Nov");
"#,
    );

    assert!(
        source.contains("SmeltRegExp::new(source.clone(), flags).test(&haystack)"),
        "{source}"
    );
    assert!(
        source.contains("(\"flags\".to_owned(), SmeltUnknown::String(self.flags))"),
        "{source}"
    );
}

#[test]
fn preserves_regex_arrays_inside_static_object_consts() {
    let source = source_for(
        r#"
type Args = {
  parsePatterns: Record<string, readonly RegExp[]>;
  defaultParseWidth: string;
};

function use(args: Args): unknown {
  return args.parsePatterns[args.defaultParseWidth];
}

const parseMonthPatterns = {
  narrow: [/^j/i, /^f/i] as const,
  any: [/^ja/i, /^f/i] as const,
};

const selected = use({
  parsePatterns: parseMonthPatterns,
  defaultParseWidth: "any",
});
"#,
    );

    assert!(
        source.contains("\"any\".to_owned()")
            && source.contains("SmeltRegExp::new(\"^ja\".to_owned(), \"i\".to_owned())"),
        "static regex-array records should preserve their entries: {source}"
    );
    assert!(
        !source.contains("parseMonthPatterns = SmeltRecord::from([])"),
        "static regex-array records must not collapse to an empty record: {source}"
    );
}

#[test]
fn emits_string_match_all_as_indexed_regexp_iteration() {
    let source = source_for(
        r#"
const pattern = /,|\./gu;
const matches = "a,b.c".matchAll(pattern);
for (const { index } of matches) {
  console.log(index);
}
"#,
    );

    assert!(source.contains(".match_all_indices(&"));
    assert!(source.contains("pub fn match_all_indices(&self"));
}

#[test]
fn tests_uninitialized_erased_value_presence_before_numeric_extraction() {
    let source = source_for(
        r#"
function latest(flag: boolean): number | undefined {
  let found;
  if (flag) {
    found = 3;
  }
  return found !== undefined ? found : undefined;
}
"#,
    );

    assert!(source.contains("matches!(found.clone(), SmeltUnknown::Null)"));
    assert!(!source.contains("!(false)"));
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
        source.contains("fn expose(callback: ::std::rc::Rc<dyn Fn(SmeltUnknown) -> SmeltUnknown>)"),
        "{source}"
    );
    assert!(!source.contains("fn expose(callback: &dyn Fn"), "{source}");
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

#[test]
fn emits_optional_constructor_parameter_without_unwrapping_optional_argument() {
    let source = source_for(
        r#"
class Parser {
  subPriority?: number;
  constructor() {}
}
class ValueSetter {
  constructor(subPriority?: number) {}
}
function make(parser: Parser): ValueSetter {
  return new ValueSetter(parser.subPriority);
}
"#,
    );

    assert!(
        source.contains("fn new(sub_priority: Option<f64>) -> Self"),
        "{source}"
    );
    assert!(
        !source.contains(
            "sub_priority.clone().clone().expect(\"optional value was absent after narrowing\")"
        ),
        "{source}"
    );
}

#[test]
fn emits_unknown_index_assignment_as_object_mutation() {
    let source = source_for(
        r#"
function build(key: unknown, value: unknown): unknown {
  const result: unknown = {};
  // @ts-expect-error dynamic index writes are accepted at erased object boundaries.
  result[key] = value;
  return result;
}
"#,
    );

    assert!(source.contains("SmeltUnknown::Object(map)"), "{source}");
    assert!(
        source.contains("map.insert(smelt_key, smelt_value)"),
        "{source}"
    );
    assert!(!source.contains("unknown is not null"), "{source}");
}

#[test]
fn emits_array_destructuring_assignment_as_indexed_writes() {
    let source = source_for(
        r#"
function swap(data: unknown[], i: number, j: number): void {
  [data[i], data[j]] = [data[j], data[i]];
}
"#,
    );

    assert!(source.contains("let __smelt_destructure"), "{source}");
    assert!(source.contains("__smelt_destructure.get"), "{source}");
    assert!(source.contains("index = 1.0"), "{source}");
    assert_eq!(
        source
            .matches("data[index] = __smelt_destructure.get")
            .count(),
        2,
        "{source}"
    );
    assert!(
        !source.contains("data[index] = SmeltUnknown::Array"),
        "{source}"
    );
}

#[test]
fn emits_callback_typeof_unknown_as_runtime_match() {
    let source = source_for(
        r#"
function mapType(values: unknown[]): string[] {
  return values.map((item) => typeof item);
}
"#,
    );

    assert!(
        source.contains("SmeltUnknown::Symbol(_) => \"symbol\""),
        "{source}"
    );
    assert!(
        source.contains(
            "SmeltUnknown::Null | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"object\""
        ),
        "{source}"
    );
}

#[test]
fn emits_array_for_each_with_function_callback_parameter() {
    let source = source_for(
        r#"
function visit(values: number[], callback: (value: number, index: number, data: number[]) => void): void {
  values.forEach(callback);
}
"#,
    );

    assert!(source.contains(".iter().enumerate().for_each"), "{source}");
    assert!(source.contains("(smelt_callback)"), "{source}");
    assert!(!source.contains("Default::default()"), "{source}");
}

#[test]
fn emits_array_flat_map_with_function_callback_parameter() {
    let source = source_for(
        r#"
function expand(values: number[], callback: (value: number, index: number, data: number[]) => number[]): number[] {
  return values.flatMap(callback);
}
"#,
    );

    assert!(source.contains(".iter().enumerate().flat_map"), "{source}");
    assert!(source.contains("(smelt_callback)"), "{source}");
    assert!(!source.contains("Default::default()"), "{source}");
}

#[test]
fn ignores_unused_void_call_result_without_null_cast() {
    let source = source_for(
        r#"
function wrapped(value: unknown): void;
function wrapped(value: unknown): unknown {
  return value;
}

function run(value: unknown): void {
  wrapped(value);
}
"#,
    );

    assert!(source.contains("let _ = wrapped"), "{source}");
    assert!(!source.contains("unknown is not null"), "{source}");
}

#[test]
fn emits_non_destructive_object_getter_reads() {
    let source = source_for(
        r#"
function makeCounter(): { readonly value: number } {
  let value = 0;
  return {
    get value() {
      value += 1;
      return value;
    },
  };
}

const counter = makeCounter();
const first = counter.value;
const second = counter.value;
"#,
    );

    assert!(
        source.contains("match getter.get(\"__smelt_get\")"),
        "{source}"
    );
    assert!(
        !source.contains("getter.remove(\"__smelt_get\")"),
        "{source}"
    );
}

#[test]
fn resets_virtual_timers_at_generated_test_start() {
    let source = source_for(
        r#"
import { test } from "vitest";

test("timer isolation", () => {
  setTimeout(() => {}, 10);
});
"#,
    );

    assert!(source.contains("fn smelt_reset_timers()"), "{source}");
    assert!(source.contains("    smelt_reset_timers();"), "{source}");
}

#[test]
fn drains_virtual_timers_before_async_sleep_can_change_threads() {
    let source = source_for(
        r#"
setTimeout(() => {}, 10);
"#,
    );

    let sleep_body = source
        .split("async fn smelt_sleep_ms(delay_ms: f64) {")
        .nth(1)
        .expect("missing smelt_sleep_ms helper");
    let drain = sleep_body
        .find("        smelt_drain_due_timers();")
        .unwrap();
    let yield_now = sleep_body
        .find("    tokio::task::yield_now().await;")
        .unwrap();
    assert!(drain < yield_now, "{source}");
    assert!(
        source.contains("let target_ms = SMELT_TIMER_NOW_MS.with"),
        "{source}"
    );
    assert!(
        source.contains("filter(|timer| timer.due_ms <= target_ms)"),
        "{source}"
    );
}

#[test]
fn reads_narrowed_unknown_object_length_property() {
    let source = source_for(
        r#"
function lengthOf(value: unknown): number {
  if (typeof value === "object" && value !== null && "length" in value && typeof value.length === "number") {
    return value.length;
  }
  return -1;
}
"#,
    );

    assert!(
        source.contains("smelt_get_object_field(&map, \"length\")"),
        "{source}"
    );
    assert!(
        !source.contains("SmeltUnknown::Object(value) => value.len()"),
        "{source}"
    );
}

#[test]
fn reads_erased_callback_string_length_property() {
    let source = source_for(
        r#"
function project(values: string[], mapper: (value: unknown) => unknown): unknown[] {
  return values.map(mapper);
}

export function run(): unknown[] {
  return project(["aa", "b"], (value) => value.length);
}
"#,
    );

    assert!(
        source.contains(
            "SmeltUnknown::String(value) => SmeltUnknown::Number(value.chars().count() as f64)"
        ),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Array(value) => SmeltUnknown::Number(value.len() as f64)"),
        "{source}"
    );
}

#[test]
fn preserves_unknown_size_as_dynamic_property_access() {
    let source = source_for(
        r#"
function sizeOf(value: unknown): unknown {
  if (typeof value === "object" && value !== null && "size" in value) {
    return value.size;
  }
  return undefined;
}
"#,
    );

    assert!(
        source.contains("smelt_get_object_field(&map, \"size\")"),
        "{source}"
    );
}

#[test]
fn writes_back_list_pushes_through_tuple_indexes() {
    let source = source_for(
        r#"
function partitionLike(values: number[]): [number[], number[]] {
  const result: [number[], number[]] = [[], []];
  result[0].push(values[0]);
  result[1].push(values[1]);
  return result;
}
"#,
    );

    assert!(source.contains("result.0 ="), "{source}");
    assert!(source.contains("result.1 ="), "{source}");
}
