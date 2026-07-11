//! Split codegen tests chunk.

use super::*;

/// A `return <literal>` statement inside an `async` function returns the
/// *resolved* value, not a promise: the async lowering wraps the whole body
/// into the future. When the declared return type is `Promise<[null, T]>` the
/// returned tuple/array literal must lower to the erased value directly, never
/// be coerced into a `SmeltPromise::from_future(..)` around a non-future value
/// (which produced `let _tmp: Pin<Box<dyn Future<..>>> = vec![..];`, E0308).
#[test]
fn async_return_of_tuple_literal_lowers_to_value_not_promise_wrapper() {
    let source = source_for(
        r#"
export async function attemptAsync<T, E>(func: () => Promise<T>): Promise<[null, T] | [E, null]> {
  try {
    const result = await func();
    return [null, result];
  } catch (error) {
    return [error as E, null];
  }
}
"#,
    );

    // The success return lowers to an erased array value, not a future wrapper.
    assert!(
        source.contains("SmeltUnknown::Array(vec![SmeltUnknown::Null, result"),
        "{source}"
    );
    // No promise-from-future wrapper is emitted around the returned tuple, and
    // no return temporary is typed as a boxed future initialized from a value.
    assert!(
        !source.contains("SmeltPromise::from_future(Box::pin(async move { let smelt_value = smelt_future.await"),
        "{source}"
    );
    assert!(
        !source.contains("Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>> = vec!"),
        "{source}"
    );
}

/// An `async` function desugars to a future whose `Output` is
/// `Result<T, Box<dyn Error>>`, even when the source body never throws. A
/// `return value;` inside such a body must therefore be wrapped in `Ok(..)`;
/// emitting a bare `return value;` mismatched the `Result` output (E0308). This
/// exercises a non-throwing async function returning a concrete value.
#[test]
fn async_non_throwing_return_is_ok_wrapped() {
    let source = source_for(
        r#"
export async function firstEven(values: number[]): Promise<number> {
  for (const value of values) {
    if (value % 2 === 0) {
      return value;
    }
  }
  return -1;
}
"#,
    );

    assert!(source.contains("async fn first_even"), "{source}");
    // Both the mid-body and trailing return are wrapped in Ok(..).
    assert!(source.contains("return Ok("), "{source}");
    // No bare numeric return leaks past the Result output.
    assert!(!source.contains("return -1.0;"), "{source}");
}

#[test]
fn emits_concrete_union_enum_and_projects_typeof_narrowed_local() {
    let source = source_for(
        r#"
function resolvePath(path: string | (() => string)): string {
  if (typeof path === "string" && path.includes(".")) {
    return path;
  }
  if (typeof path === "string") {
    return path + ".ts";
  }
  return path();
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(source.contains("M0(") && source.contains("M1("), "{source}");
    assert!(
        source.contains("matches!(path.clone(), SmeltUnion"),
        "{source}"
    );
    assert!(
        source.contains("union guard selected an excluded member"),
        "{source}"
    );
}

#[test]
fn narrows_concrete_union_locals_with_in_array_and_instanceof_guards() {
    let source = source_for(
        r#"
interface Named { name: string; }
interface LengthBearing { length: number; }
function lengthOf(value: Named | LengthBearing): number {
  if ("length" in value) return value.length;
  return 0;
}

function values(source: number[] | Record<string, number>): number[] {
  return Array.isArray(source) ? source : Object.values(source);
}

class Left { left: string = "left"; }
class Right { right: string = "right"; }
function read(value: Left | Right): string {
  if (value instanceof Left) return value.left;
  return "right";
}
"#,
    );

    assert!(source.matches("pub enum SmeltUnion").count() >= 3, "{source}");
    assert!(
        source.contains("union guard selected an excluded member"),
        "{source}"
    );
    assert!(source.contains("matches!(value.clone(), SmeltUnion"), "{source}");
}

/// A class field typed as a concrete union (`string | number`) lowers the field
/// to a tagged `SmeltUnion*` enum. The class struct derives `Clone, Debug,
/// Default`, so the union enum must supply `Debug` and `Default` as well — a
/// data-carrying enum can derive neither. The enum keeps `#[derive(Clone)]` and
/// gains a hand-written `Debug` (reusing the erased `SmeltUnknown` view, exactly
/// like its `PartialEq`) and a hand-written `Default` selecting the first arm.
/// Without these impls the struct's `Debug`/`Default` derives fail to compile.
#[test]
fn emits_debug_and_default_for_union_typed_class_field() {
    let source = source_for(
        r"
class Holder {
  value: string | number = 0;
}
export function getVal(h: Holder): string | number {
  return h.value;
}
",
    );

    // The class struct keeps deriving the standard traits.
    assert!(
        source.contains("#[derive(Clone, Debug, Default)]"),
        "{source}"
    );
    // The union enum carries hand-written Debug and Default impls.
    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(
        source.contains("impl ::std::fmt::Debug for SmeltUnion"),
        "{source}"
    );
    assert!(
        source.contains("impl Default for SmeltUnion"),
        "{source}"
    );
    // Default selects the first union arm.
    assert!(source.contains("Self::M0("), "{source}");
}

/// Issue #84: a class string index signature `[key: string]: T` emits Rust that
/// compiles and carries a real runtime keyed store. A pure-index class
/// (`StringBag`) exposes keyed access whose value type drives an honest
/// `Option<T>` read; a mixed class (`MixedBag`) keeps its declared named field
/// concretely typed (`size: f64`) alongside the index signature. The index
/// signature's value type stays concrete, never erased to `SmeltUnknown`.
///
/// The backing store is a synthesized private `__smelt_index_store` field: a
/// keyed write inserts into it and a keyed read looks it up, so `x[k] = v; x[k]`
/// round-trips at runtime (asserted end-to-end by the CLI integration test
/// `build_round_trips_class_index_signature_keyed_store`).
#[test]
fn emits_class_index_signature_named_fields_and_keyed_store() {
    let source = source_for(
        r"
class StringBag {
  [key: string]: string;
}

class MixedBag {
  size: number = 0;
  [key: string]: number;
}

export function readBag(bag: StringBag, key: string): string | undefined {
  return bag[key];
}

export function writeBag(bag: StringBag, key: string, value: string): void {
  bag[key] = value;
}

export function mixedSize(bag: MixedBag): number {
  return bag.size;
}
",
    );

    // Both classes emit concrete Rust structs.
    assert!(source.contains("struct StringBag"), "{source}");
    assert!(source.contains("struct MixedBag"), "{source}");
    // The mixed class keeps its named field concretely typed.
    assert!(source.contains("size: f64"), "{source}");
    // Each index-signature class carries the synthesized backing store field.
    assert!(source.contains("__smelt_index_store"), "{source}");
    // The keyed read is the honest `Option<String>`, not an erased value.
    assert!(
        source.contains("fn read_bag") && source.contains("-> Option<String>"),
        "{source}"
    );
    // The keyed read routes to the store lookup.
    assert!(
        source.contains(".__smelt_index_store.get(&"),
        "{source}"
    );
    // The keyed write routes to a store insert so it round-trips.
    assert!(
        source.contains(".__smelt_index_store.insert("),
        "{source}"
    );
    // Named-field access on the mixed class stays a concrete `f64` field read.
    assert!(
        source.contains("fn mixed_size") && source.contains("-> f64"),
        "{source}"
    );
}

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
        r"
declare const value: unknown;
const date = new Date(value);
date.setFullYear(2024);
const iso = date.toISOString();
",
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
        r"
function keyCount(value: unknown): number {
  return Object.keys(value).length;
}
const dateCount = keyCount(new Date(0));
const regexpCount = keyCount(/abc/u);
",
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
        r"
declare const values: unknown[];
const isoValues = values.map((value) => value.toISOString());
",
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
        r"
declare const value: unknown;
const text: string = value as string;
const count: number = value as number;
const flag: boolean = value as boolean;
const bag: Record<string, unknown> = value as Record<string, unknown>;
",
    );

    assert!(
        source.contains("SmeltUnknown::Null => String::new()"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => false"),
        "{source}"
    );
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
        r"
declare const value: unknown;
const bag: Record<string, unknown> = value as Record<string, unknown>;
const entries = Object.entries(bag);
",
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
        r"
declare function purry(fn: (value: unknown) => unknown, args: readonly unknown[]): unknown;

export function entries(...args: readonly unknown[]): unknown {
  return purry(Object.entries, args);
}
",
    );

    assert!(
        source.contains("SmeltRecord::with_id_from_entries"),
        "Object.entries callback should cast its argument through record projection: {source}"
    );
    assert!(
        source.contains(
            "SmeltArray::with_id(smelt_l.id(), smelt_l.into_iter().map(|value| SmeltUnknown::Array"
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
        r"
declare function purry(fn: (value: unknown) => unknown, args: readonly unknown[]): unknown;

export function fromEntries(...args: readonly unknown[]): unknown {
  return purry(Object.fromEntries, args);
}
",
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
            && source
                .contains("matches!(key, \"__smelt_error\" | \"message\" | \"cause\" | \"errors\")"),
        "{source}"
    );
}

#[test]
fn copies_erased_object_rest_destructuring_properties() {
    let source = source_for(
        r"
declare function getValue(): unknown;
declare function makeCall(): (...args: unknown[]) => void;

const source = getValue() as { call: (...args: unknown[]) => void; cancel: () => void; flush: () => void };
const { call, ...rest } = source;
const callable = makeCall();
const merged = Object.assign(callable, rest);
merged.cancel();
merged.flush();
",
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
        r"
export function pick(options: Record<string, unknown>): unknown {
  const { leading = false, trailing = true, maxWait } = options;
  return [leading, trailing, maxWait];
}
",
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
        r"
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
",
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
        r"
export function make(count: number): number[] {
  const output = new Uint8Array(count);
  for (let index = 0; index < count; index += 1) {
    output[index] = index + 1;
  }
  return output;
}
",
    );

    assert!(source.contains("vec![0.0; smelt_repeat_count]"), "{source}");
    assert!(!source.contains("vec![0.0, 0.0, 0.0, 0.0"), "{source}");
}

#[test]
fn emits_bigint_typed_array_constructor_as_numeric_list() {
    // `BigInt64Array` / `BigUint64Array` were previously omitted from the
    // typed-array recognizer, so `new BigUint64Array(...)` aborted the build as
    // an "unresolved class". They now share the numeric-list model with the
    // other views: the element form emits a `Vec` literal and `.length` reads a
    // list length.
    let source = source_for(
        r"
export function make(): number {
  const values = new BigUint64Array([1, 2, 3]);
  return values.length;
}
",
    );

    assert!(source.contains("vec!["), "{source}");
    assert!(!source.contains("unresolved"), "{source}");
}

#[test]
fn emits_typed_array_from_element_literal_as_vec_literal() {
    // `new Uint8Array([1, 2, 3])` reuses the array-literal lowering, so it emits
    // a concrete `Vec` literal that supports `.length` and integer indexing.
    let source = source_for(
        r"
export function first(): number {
  const values = new Uint8Array([10, 20, 30]);
  return values[0];
}
",
    );

    assert!(source.contains("vec![10.0, 20.0, 30.0]"), "{source}");
}

#[test]
fn inserts_unknown_iterable_values_into_typed_sets() {
    let source = source_for(
        r"
declare function values(): unknown;

export function collect(): Set<number> {
  const results = new Set<number>();
  for (const value of values() as Iterable<unknown>) {
    results.add(value as number);
  }
  return results;
}
",
    );

    assert!(
        source.contains("match value")
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
        r"
declare const value: unknown;
const number = +value;
",
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
        source.contains(
            "SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN"
        ),
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
        r"
interface Duration {
  years?: number;
  months?: number;
}

export function make(): Duration {
  return {};
}
",
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
        r"
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
",
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
        source.contains(", 1.0, _smelt_tmp_3)"),
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
fn does_not_rewrap_equivalent_erased_rest_function_shapes() {
    let source = source_for(
        r#"
type AnyRest = (...args: unknown[]) => unknown;
type StringRest = (...args: unknown[]) => string;

function read(): StringRest {
  return (...args: unknown[]) => String(args.length);
}

const callback: AnyRest = read();
const value = callback("x");
"#,
    );

    assert!(
        !source.contains("let smelt_adapted: SmeltErasedFunction = ::std::rc::Rc::new"),
        "erased-rest callbacks with equivalent ABI should pass through unchanged\n{source}"
    );
    assert!(
        source.contains("let callback: SmeltErasedFunction = read().clone();")
            || source.contains("let callback: SmeltErasedFunction;")
                && source.contains("callback = _smelt_tmp_")
                && source.contains(".clone();"),
        "erased-rest callback assignment should preserve the shared SmeltErasedFunction\n{source}"
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
        source.contains("number.to_string() + &match suffix"),
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
        r"
interface Localize<T> {
  ordinalNumber: (value: number) => string;
  value?: T;
}

function read(localize: Localize<string>): string {
  return localize.ordinalNumber(1);
}
",
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
        r"
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
",
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
        r"
interface Options {
  comparison?: number;
}

function read(options?: Options): number {
  return options?.comparison ?? 0;
}

function positive(options?: Options): boolean {
  return options.comparison > 0;
}
",
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
    // A callback that already returns `String` is a valid `Replacer`, so it is
    // handed to `replace_all` without a ToString wrapper.
    assert!(
        !source.contains("|caps: &regex::Captures<'_>| match ("),
        "{source}"
    );
}

#[test]
fn coerces_non_string_regex_replace_callback_result() {
    // The callback's result flows through an erased record lookup, so it is
    // typed `unknown` (`SmeltUnknown`), which is not `AsRef<str>`. The regex
    // replacement must ToString it so the `Replacer` closure yields a `String`.
    let source = source_for(
        r#"
const htmlEscapes: Record<string, string> = { "&": "&amp;" };
export function escape(str: string): string {
  return str.replace(/[&<>"']/g, (match) => htmlEscapes[match]);
}
"#,
    );

    assert!(source.contains("replace_all"), "{source}");
    // The replacement is wrapped in the SmeltUnknown -> String ToString match.
    assert!(
        source.contains("|caps: &regex::Captures<'_>| match ("),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Undefined => \"undefined\".to_owned()"),
        "{source}"
    );
}

#[test]
fn emits_escaping_closure_spread_calls_with_owned_callback_state() {
    let source = source_for(
        r"
export function purryOn(
  isArg: (x: unknown) => boolean,
  implementation: (data: unknown, first: unknown, ...rest: Array<unknown>) => unknown,
  args: Array<unknown>,
): unknown {
  return isArg(args[0])
    ? (data: unknown) => implementation(data, ...args)
    : implementation(args[0], args[1], args.slice(2));
}
",
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
        r"
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
",
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
        r"
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

",
    );

    assert!(
        source.contains("smelt_args.iter().skip(1).cloned().collect::<SmeltList<_>>()"),
        "{source}"
    );
    assert!(!source.contains("SmeltUnknown::Array(arg1)"), "{source}");
}

#[test]
fn does_not_spread_array_parameters_across_erased_function_boundaries() {
    let source = source_for(
        r"
export function wrapArrayParam(): unknown {
  const callback: unknown = (data: unknown, values: Array<unknown>) => values;
  return callback;
}
",
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
        r"
class Parser {
  set(date: number, value: number): number {
    date = value + 1;
    return date;
  }
}
",
    );

    assert!(
        source.contains("fn set(&self, mut date: f64, value: f64) -> f64"),
        "{source}"
    );
}

#[test]
fn emits_mutable_constructor_parameters_when_reassigned() {
    let source = source_for(
        r"
class Box {
  value: number;
  constructor(value: number) {
    value = value + 1;
    this.value = value;
  }
}
",
    );

    assert!(
        source.contains("fn new(mut value: f64) -> Self"),
        "{source}"
    );
}

#[test]
fn emits_mutable_structural_parameters_when_fields_are_assigned() {
    let source = source_for(
        r"
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
",
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
        r"
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
",
    );

    assert!(
        source.contains("set: ::std::rc::Rc<dyn Fn(&mut Flags) -> SmeltUnion"),
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
        r"
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
",
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
        r"
interface Options {
  flag?: boolean;
}
function enabled(options?: Options): boolean {
  return true;
}
",
    );

    assert!(source.contains("struct Options"), "{source}");
    assert!(source.contains("flag: Option<bool>,"), "{source}");
    assert!(!source.contains("serde::Serialize"), "{source}");
}

#[test]
fn derives_clone_for_function_bearing_interface_storage() {
    let source = source_for(
        r"
interface Callbacks {
  run: () => number;
}
function copy(callbacks: Callbacks): Callbacks {
  return callbacks;
}
",
    );

    assert!(
        source.contains("#[derive(Clone)]\n#[allow(dead_code)]\nstruct Callbacks"),
        "{source}"
    );
}

#[test]
fn emits_generic_interface_storage_with_phantom_parameter() {
    let source = source_for(
        r"
interface Boxed<T> {
  value: T;
}
declare const boxed: Boxed<number>;
const copied: Boxed<number> = boxed;
",
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
        r"
declare const value: unknown;
const date = new Date(value);
const year = date.getFullYear();
date.setFullYear(value);
",
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
        r"
function apply(isTwoDigitYear: boolean, year: number, date: number): number {
  if (isTwoDigitYear) {
    const normalizedTwoDigitYear = year + 2000;
    date.setFullYear(normalizedTwoDigitYear, 0, 1);
    return date;
  }
  return date;
}
",
    );

    let normalized = source
        .find("normalized_two_digit_year =")
        .or_else(|| source.find("normalized_two_digit_year: f64 ="))
        .unwrap_or_else(|| panic!("{source}"));
    let setter = source
        .find("let normalized_year = (normalized_two_digit_year as i32)")
        .unwrap_or_else(|| panic!("{source}"));
    assert!(normalized < setter, "{source}");
    assert!(source.contains(" as f64"), "{source}");
}

#[test]
fn emits_delete_on_erased_object_surfaces() {
    let source = source_for(
        r"
function removeKey(value: unknown, key: string): boolean {
  return delete value[key];
}
",
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
    assert!(
        source
            .contains("fn passthrough(values: SmeltList<SmeltUnknown>) -> SmeltList<SmeltUnknown>")
    );
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
        r"
function copy<T>(value: T): unknown[] {
  const copied: unknown[] = [];
  if (Array.isArray(value)) {
    for (const [index, item] of value.entries()) {
      copied[index] = item;
    }
  }
  return copied;
}
",
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
        r"
function first<S extends string>(value: S): string {
  return value[0];
}
",
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

    // `===`/`!==` on erased values use JS strict equality (`js_strict_eq`):
    // reference identity for objects, value for primitives, NaN-unequal — NOT
    // SmeltUnknown's structural `==` (which `==`/`!=` and `isDeepEqual` use).
    assert!(
        source
            .contains("value.clone().js_strict_eq(&SmeltUnknown::String(\"trailing\".to_owned()))"),
        "{source}"
    );
    assert!(
        source.contains("!(value.clone().js_strict_eq(&SmeltUnknown::Number(1.0 as f64)))"),
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
        r"
function assign(out: Record<string, unknown>, key: unknown, value: unknown): Record<string, unknown> {
  out[key as string] = value;
  return out;
}
",
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
        r"
function widen(values: string[]): unknown[] {
  return values;
}
",
    );

    assert!(
        source.contains(
            "smelt_l.into_iter().map(|value| SmeltUnknown::String(value)).collect::<Vec<_>>()"
        ),
        "{source}"
    );
}

#[test]
fn emits_string_chars_into_unknown_list_destination() {
    let source = source_for(
        r"
function chars(value: string): unknown[] {
  return [...value];
}
",
    );

    assert!(
        source.contains(".map(|value| SmeltUnknown::String(value)).collect::<Vec<_>>()"),
        "{source}"
    );
}

#[test]
fn emits_erased_value_wrapped_for_optional_erased_destination() {
    let source = source_for(
        r"
function maybe(value: unknown): unknown | undefined {
  return value;
}
",
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
        r"
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
",
    );

    assert!(source.contains("loop {"), "{source}");
    assert!(source.contains("return out;"), "{source}");
}

#[test]
fn emits_closure_call_result_for_optional_destination() {
    let source = source_for(
        r"
function maybeCall(callback: (value: number) => number): number | undefined {
  const value: number | undefined = callback(1);
  return value;
}
",
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
        r"
function initialCaps<T extends string>(values: T[]): string[] {
  return values.map((value) => value[0].toUpperCase());
}
",
    );

    assert!(source.contains("SmeltUnknown::String(value)"), "{source}");
    assert!(source.contains("value.chars().nth(index)"), "{source}");
    assert!(source.contains(".to_uppercase()"), "{source}");
    assert!(!source.contains("String::new().to_uppercase()"), "{source}");
}

#[test]
fn emits_default_callback_for_erased_non_function_callback_cast() {
    let source = source_for(
        r"
function invoke(callback: (value: number) => number, fallback?: (value: number) => number): number {
  const chosen = (undefined as unknown) as (value: number) => number;
  return chosen(1);
}
",
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
        source.contains("map_or(SmeltUnknown::Undefined, IntoSmeltUnknown::into_smelt_unknown)"),
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
        r"
function pair(value: unknown): unknown {
  return [value, value] as [unknown, unknown];
}
",
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
        r"
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
",
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
        r"
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
",
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
        r"
type MatchFnResult<T> = { value: T; rest: string };

declare const genericResult: MatchFnResult<unknown>;
const numericResult = genericResult as MatchFnResult<number>;
const value = numericResult.value;
",
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
        r"
declare const value: unknown;
declare const fallback: Date;
const selected: unknown = value ? value : fallback;
",
    );

    assert!(
        source.contains("if _smelt_tmp_3 { value } else { match fallback"),
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
        r"
type A = { locale?: unknown };
type B = { weekStartsOn?: number };
type DefaultOptions = A & B;

let defaultOptions: DefaultOptions = {};

export function getDefaultOptions(): DefaultOptions {
  return defaultOptions;
}
",
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
        r"
interface Flags {
  era?: number;
}

declare const flags: Flags;
const erased: unknown = flags;
",
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
        r"
declare const left: unknown;
declare const right: unknown;
const same = left === right;
",
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
        r"
declare const value: unknown;
const values = new Set<unknown>([value]);
",
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
        r"
function before(left: unknown, right: unknown): boolean {
  return left < right;
}
",
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
        r"
function addUnknown(total: number, value: unknown): number {
  const narrowed = value as number;
  return total + narrowed;
}

function truncateDifference(left: bigint, right: number): bigint {
  return left - right;
}
",
    );

    assert!(
        source.contains("((right.clone() as f64).trunc() as i64)"),
        "{source}"
    );
}

#[test]
fn emits_numeric_not_as_parenthesized_truthiness() {
    let source = source_for(
        r"
function isZero(amount: number): boolean {
  return !amount;
}
",
    );

    assert!(
        source.contains("!({ let smelt_number = amount.clone(); smelt_number != 0.0 && !smelt_number.is_nan() })"),
        "{source}"
    );
}

#[test]
fn emits_optional_erased_value_coerced_to_concrete_destination() {
    let source = source_for(
        r"
function read(output: Record<string, unknown[]>, key: unknown | undefined): unknown[] {
  return output[key as string];
}
",
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
        r"
function fallback<T>(value: T, fallbackValue: T): T | undefined {
  return value ?? fallbackValue;
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => fallback_value.clone()"),
        "{source}"
    );
}

#[test]
fn emits_erased_nullish_coalescing_into_concrete_destination() {
    let source = source_for(
        r"
function fallback(value: unknown): boolean {
  const result: boolean = value ?? false;
  return result;
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value"),
        "{source}"
    );
}

#[test]
fn emits_optional_nullish_coalescing_with_erased_fallback_without_panicking() {
    let source = source_for(
        r"
declare const defaults: Record<string, unknown>;
interface Options {
  weekStartsOn?: number;
}
function read(options?: Options): number {
  return options?.weekStartsOn ?? defaults.weekStartsOn ?? 0;
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => None, value => Some("),
        "{source}"
    );
    assert!(source.contains(".or(match"), "{source}");
}

#[test]
fn emits_boolean_cast_for_typescript_unknown() {
    let source = source_for(
        r"
function truthy(value: unknown): boolean {
  return Boolean(value);
}
",
    );

    assert!(source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => false"));
    assert!(source.contains(
        "SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true"
    ));
}

#[test]
fn emits_nan_truthiness_without_invalid_nan_comparison() {
    let source = source_for(
        r"
function truthy(): boolean {
  return Boolean(NaN);
}
",
    );

    assert!(source.contains("!smelt_number.is_nan()"), "{source}");
    assert!(!source.contains("f64::NAN != 0.0"), "{source}");
}

#[test]
fn emits_call_bodied_local_arrow_as_real_closure_body() {
    let source = source_for(
        r"
function makeDataLast(fn: (value: number, extra: number) => number, extra: number): (value: number) => number {
  const dataLast = (data: number): number => fn(data, extra);
  return dataLast;
}
",
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
        r"
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
",
    );

    assert!(
        source.contains(".cloned().unwrap_or(SmeltList::new(Vec::new()))"),
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
        r"
function keyCount(value: unknown): number {
  return Object.keys(value).length;
}
",
    );

    assert!(
        source.contains("SmeltUnknown::String(value) => value.chars().enumerate()"),
        "{source}"
    );
}

#[test]
fn erases_sets_without_dropping_items() {
    let source = source_for(
        r"
function accept(value: unknown): unknown {
  return value;
}

export function run(): unknown {
  return accept(new Set([1, 2, 3]));
}
",
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
        r"
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
",
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
        r"
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
",
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
        r"
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
",
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
        r"
const binary = (10n).toString(2);
const left = 1n << 8n;
const right = left >> 1n;
const pivot = (4 + 10) >>> 1;
function shiftRaw(raw: bigint): bigint {
  return raw >> 1n;
}
",
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
        r"
function range(start: number, length: number, step: number): number[] {
  return Array.from({ length }, (_, i) => (i === 0 ? start : start + i * step));
}
",
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
        r"
function sample<T>(data: readonly T[]): T[] {
  const sampleIndices = new Set<number>();
  return [...sampleIndices].sort((a, b) => a - b).map((index) => data[index]!);
}
",
    );

    assert!(source.contains("closure_arg_0.clone() as i64"), "{source}");
    assert!(source.contains("usize::try_from(normalized)"), "{source}");
}

#[test]
fn emits_erased_string_key_index_reads_for_arrays_and_strings() {
    let source = source_for(
        r"
export function read(value: unknown, key: string): unknown {
  return value[key as keyof typeof value];
}
",
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
        r"
const sortByImplementation = <T>(
  data: readonly T[],
  compareFn: (left: T, right: T) => number,
): T[] => [...data].sort(compareFn);
",
    );

    assert!(
        source.contains("(smelt_comparator)(left.clone(), right.clone())"),
        "{source}"
    );
    assert!(
        source.contains("if ordering < 0.0 { std::cmp::Ordering::Less }"),
        "{source}"
    );
}

#[test]
fn emits_spread_sort_for_erased_iterable_generic() {
    let source = source_for(
        r"
type IterableContainer<T> = readonly T[];

const numberComparator = (a: number, b: number): number => a - b;

function sorted<T extends IterableContainer<number>>(data: T): unknown {
  return [...data].sort(numberComparator);
}
",
    );

    // The spread of the erased, list-constrained `data` materializes a fresh,
    // CONCRETE `Vec<f64>` (its `SmeltUnknown` elements unwrapped to the `number`
    // element type from the `IterableContainer<number>` constraint) instead of
    // staying an erased alias. That concrete binding is what lets the following
    // `.sort(cmp)` take the typed in-place path rather than the dynamic path
    // whose sorted result is discarded.
    assert!(source.contains(": SmeltList<f64> ="), "{source}");
    assert!(source.contains(".sort_by(|left, right|"), "{source}");
}

#[test]
fn emits_object_symbol_iterator_generator_as_erased_iterable() {
    let source = source_for(
        r"
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
",
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
        r"
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
",
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
        source.contains("(identity)(SmeltUnknown::String(\"hello\".to_owned()), _smelt_tmp_2)")
            && source.contains("_smelt_tmp_2 = Into::<SmeltList<_>>::into(SmeltList::from("),
        "normal closure calls should preserve fixed arguments and pack an empty rest vector: {source}"
    );
}

#[test]
fn emits_void_return_inside_loop_branch() {
    let source = source_for(
        r"
function stopWhenSorted(items: number[]): void {
  let index = 0;
  while (index < items.length) {
    if (items[index]! >= 0) {
      return;
    }
    index += 1;
  }
}
",
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
        r"
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
",
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
        r"
function concatImplementation<T1, T2>(arr1: T1, arr2: T2): unknown[] {
  return [...arr1 as unknown[], ...arr2 as unknown[]];
}
",
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
fn emits_tuple_element_push_as_concrete_tuple_value() {
    // Pushing a bare `[value, key]` literal into an `Array<[number, string]>`
    // emits a real `(f64, String)` tuple value rather than widening the pushed
    // item to a `SmeltUnknown` list that could never re-type into the tuple.
    let source = source_for(
        r"
export function collect(value: number, key: string): Array<[number, string]> {
  const result: Array<[number, string]> = [];
  result.push([value, key]);
  return result;
}
",
    );

    assert!(
        source.contains("SmeltList<(f64, String)>"),
        "tuple-element array should keep its concrete tuple element type: {source}"
    );
    assert!(
        source.contains("(value.clone(), key.clone())"),
        "pushed literal should be a concrete tuple value: {source}"
    );
    assert!(
        source.contains("result.push("),
        "the tuple value should be pushed onto the array: {source}"
    );
    assert!(
        !source.contains("SmeltList<SmeltUnknown>"),
        "tuple-element push must not widen the array to SmeltUnknown: {source}"
    );
}

#[test]
fn emits_union_element_push_as_concrete_union_injections() {
    // Mixed-literal pushes into an `Array<number | string>` inject each argument
    // into the concrete union enum instead of routing through SmeltUnknown.
    let source = source_for(
        r#"
export function mixed(): Array<number | string> {
  const xs: Array<number | string> = [];
  xs.push(1);
  xs.push("a");
  return xs;
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(
        source.contains(".push(SmeltUnion") && source.contains("::M0(1.0)"),
        "numeric push should inject the concrete union member: {source}"
    );
    assert!(
        source.contains("::M1(\"a\".to_owned())"),
        "string push should inject the concrete union member: {source}"
    );
}

#[test]
fn emits_union_receiver_concat_by_extracting_to_unknown_list() {
    // `concat` on an erased/union array-like receiver extracts the receiver to a
    // concrete `SmeltList<SmeltUnknown>` and chains the appended values — a
    // genuine dynamic boundary, not a silent widening of statically-typed data.
    let source = source_for(
        r#"
export function widen(a: number[] | string[]): unknown[] {
  return (a as unknown[]).concat(1, "x");
}
"#,
    );

    assert!(
        source.contains("SmeltList<SmeltUnknown>"),
        "erased concat receiver should extract to an unknown list: {source}"
    );
    assert!(
        source.contains(".iter().cloned().chain("),
        "concat should chain the appended values onto the receiver: {source}"
    );
    assert!(
        source.contains("SmeltUnknown::Number(1.0")
            && source.contains("SmeltUnknown::String(\"x\".to_owned())"),
        "appended scalars should be boxed into the unknown element type: {source}"
    );
}

#[test]
fn boxes_returned_function_values_even_when_mir_types_match() {
    let source = source_for(
        r"
function makeMapper(): (value: number) => number {
  const mapper = (value: number) => value + 1;
  return mapper;
}
",
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
        r"
function adapt(
  callback: (value: unknown) => { next: unknown },
): (value: unknown, index: number, data: unknown[]) => unknown {
  return callback;
}
",
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
        source.contains("let mut values: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into(SmeltList::new(Vec::new()));"),
        "{source}"
    );
}

#[test]
fn emits_first_assignment_to_uninitialized_local_as_declaration() {
    let source = source_for(
        r"
function choose(flag: boolean): number {
  let result: number;
  if (flag) {
    result = 1;
  } else {
    result = 2;
  }
  return result;
}
",
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
        r"
function adapt(
  callback: (values: unknown[], index?: number) => unknown,
): (value: unknown, index: number, data: unknown[]) => unknown {
  return callback;
}
",
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
        r"
function matchUnknown(value: unknown): string[] | undefined {
  return (value as any).match(/a+/);
}
",
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
        r"
function parts(value: string): string[] | undefined {
  const tokens = /a+|b/g;
  return value.match(tokens);
}
",
    );

    assert!(
        source.contains("SmeltRegExp::new(\"a+|b\".to_owned(), \"g\".to_owned())"),
        "{source}"
    );
    assert!(source.contains("tokens.match_string(&"), "{source}");
}

#[test]
fn emits_regexp_array_elements_with_flags_preserved() {
    let source = source_for(
        r"
const patterns = [/x/u, /x/gimu];
const pattern = patterns[1];
const usesGlobal = pattern.global;
const ignoresCase = pattern.ignoreCase;
const usesMultiline = pattern.multiline;
",
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
    // matchAll yields concrete `SmeltMatch` values whose consumer reads stay
    // typed: the destructured `.index` read is a typed `.index()` accessor, and
    // the list keeps its concrete `SmeltMatch` element type. The value never
    // crosses the erased `SmeltUnknown` boundary, so no `into_smelt_unknown`
    // adapter is emitted for this program.
    assert!(source.contains("pub struct SmeltMatch"));
    assert!(
        source.contains("SmeltList<SmeltMatch>"),
        "matchAll result should keep its concrete SmeltMatch element type: {source}"
    );
    assert!(
        source.contains(".index()"),
        "destructured `.index` read should use the typed accessor: {source}"
    );
    // No erasure adapter is applied at the matchAll site, and the adapter impl
    // is not emitted for this program (the only textual mention of
    // `SmeltMatch::into_smelt_unknown` is the type's docstring).
    assert!(
        !source.contains(".map(SmeltMatch::into_smelt_unknown)"),
        "matchAll must not erase its concrete result: {source}"
    );
    assert!(
        !source.contains("impl IntoSmeltUnknown for SmeltMatch"),
        "typed match reads must not require the SmeltUnknown erasure adapter: {source}"
    );
    assert!(
        source.contains("-> Vec<SmeltMatch>"),
        "match_all_indices should return concrete SmeltMatch values: {source}"
    );
}

#[test]
fn emits_regex_exec_as_concrete_smelt_match() {
    let source = source_for(
        r#"
const re = /(?<year>\d{4})-(\d{2})/;
const m = re.exec("2024-07");
console.log(m === null);
"#,
    );

    // exec returns a concrete `Option<SmeltMatch>` and the result stays typed:
    // the `m === null` check is a plain `.is_none()` on the concrete option, so
    // the match never crosses the erased `SmeltUnknown` boundary and no
    // `into_smelt_unknown` adapter is emitted for this program.
    assert!(
        source.contains("pub fn exec(&self, haystack: &str) -> Option<SmeltMatch>"),
        "exec should return a concrete SmeltMatch option: {source}"
    );
    assert!(source.contains("pub struct SmeltMatch"));
    assert!(
        source.contains(".exec(&\"2024-07\".to_owned())")
            && !source.contains(".map(SmeltMatch::into_smelt_unknown)"),
        "typed exec result must not erase to SmeltUnknown: {source}"
    );
    assert!(
        !source.contains("impl IntoSmeltUnknown for SmeltMatch"),
        "typed exec result must not require the SmeltUnknown erasure adapter: {source}"
    );
    assert!(
        source.contains(".is_none()"),
        "`m === null` should lower to a typed `.is_none()` check: {source}"
    );
    // The numbered-group / named-group / index / input shape is modeled with
    // concrete fields, not assembled as an untyped object during exec.
    assert!(source.contains("groups: Vec<Option<String>>"));
    assert!(source.contains("named: ::std::collections::HashMap<String, Option<String>>"));
    assert!(source.contains("fn from_captures(regex: &fancy_regex::Regex"));
}

#[test]
fn types_regex_match_consumer_reads_against_smelt_match() {
    // The consumer-side reads of a match value are typed against the concrete
    // `SmeltMatch` type: numbered groups (`m[0]`, `m[2]`) read `group_owned`,
    // named groups (`m.groups.letter`) read `named_group_owned`, `m.index` and
    // `m.input` read their typed accessors, and array destructuring binds the
    // numbered groups. None of these cross the erased `SmeltUnknown` boundary.
    let source = source_for(
        r#"
const pattern = /(?<letter>[a-z])(\d)?/g;
const matches = "a1 b".matchAll(pattern);
for (const found of matches) {
  const whole = found[0];
  const letter = found.groups.letter;
  const digit = found[2];
  const [full, first] = found;
  console.log(whole);
  console.log(letter);
  console.log(digit);
  console.log(full);
  console.log(first);
  console.log(found.index);
  console.log(found.input);
}
"#,
    );

    // Numbered group reads (including the destructured bindings) route through
    // the typed `group_owned` accessor; the whole match is index 0.
    assert!(
        source.contains(".group_owned(0.0 as usize)"),
        "numbered group read should use group_owned: {source}"
    );
    assert!(
        source.contains(".group_owned(2.0 as usize)"),
        "optional numbered group read should use group_owned: {source}"
    );
    // Named group reads route through `named_group_owned`.
    assert!(
        source.contains(".named_group_owned(\"letter\")"),
        "named group read should use named_group_owned: {source}"
    );
    // `.index` / `.input` read the typed accessors.
    assert!(
        source.contains(".index()"),
        "match index read should use the typed accessor: {source}"
    );
    assert!(
        source.contains(".input_owned()"),
        "match input read should use the typed accessor: {source}"
    );
    // The match list keeps its concrete element type and the reads never erase
    // to `SmeltUnknown`.
    assert!(
        source.contains("SmeltList<SmeltMatch>"),
        "matchAll result should keep its concrete SmeltMatch element type: {source}"
    );
    assert!(
        !source.contains(".map(SmeltMatch::into_smelt_unknown)"),
        "typed match reads must not erase the matchAll result: {source}"
    );
    assert!(
        !source.contains("impl IntoSmeltUnknown for SmeltMatch"),
        "typed match reads must not require the SmeltUnknown erasure adapter: {source}"
    );
}

#[test]
fn erases_regex_match_value_only_at_a_dynamic_boundary() {
    // A match value that genuinely flows into `unknown` is erased through the
    // single explicit `into_smelt_unknown` adapter, while the sibling typed
    // reads stay concrete.
    let source = source_for(
        r#"
const re = /(?<letter>[a-z])/;
const m = re.exec("a");
if (m !== null) {
  const letter = m.groups.letter;
  const boxed: unknown = m;
  console.log(letter);
  console.log(boxed);
}
"#,
    );

    assert!(
        source.contains(".named_group_owned(\"letter\")"),
        "typed named group read should stay concrete: {source}"
    );
    assert!(
        source.contains(".clone().into_smelt_unknown()"),
        "a match value flowing into unknown should use the explicit erasure adapter: {source}"
    );
    assert!(
        source.contains("impl IntoSmeltUnknown for SmeltMatch"),
        "the match erasure adapter should be emitted when the boundary is used: {source}"
    );
}

#[test]
fn tests_uninitialized_erased_value_presence_before_numeric_extraction() {
    let source = source_for(
        r"
function latest(flag: boolean): number | undefined {
  let found;
  if (flag) {
    found = 3;
  }
  return found !== undefined ? found : undefined;
}
",
    );

    assert!(source.contains("matches!(found.clone(), SmeltUnknown::Undefined)"));
    assert!(!source.contains("!(false)"));
}

#[test]
fn emits_javascript_any_character_regex_translation() {
    let source = source_for(
        r"
function inner(value: string): string[] | undefined {
  return value.match(/^'([^]*?)'?$/);
}
",
    );

    assert!(source.contains("replace(\"[^]\", \"(?s:.)\")"), "{source}");
    assert!(source.contains(".match_string(&"), "{source}");
}

#[test]
fn coerces_rendered_list_values_to_tuple_destinations() {
    let source = source_for(
        r"
function invoke(
  values: unknown[],
  callback: (pair: [unknown, unknown]) => unknown,
): unknown {
  return callback(values);
}
",
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
        r"
function outer(make: () => (value: unknown) => unknown): unknown {
  return [make];
}
",
    );

    assert!(source.contains("SmeltUnknown::Function"), "{source}");
    assert!(!source.contains("()).into_smelt_unknown()"), "{source}");
}

#[test]
fn owns_callback_params_that_escape_through_unknown_values() {
    let source = source_for(
        r"
function expose(callback: (value: unknown) => unknown): unknown {
  return { callback };
}
",
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
        r"
interface Options {
  cb?: (value: number) => number;
}

export function read(options?: Options): ((value: number) => number) | undefined {
  return options?.cb;
}
",
    );

    assert!(source.contains(".as_ref().and_then("), "{source}");
    assert!(!source.contains("Option<Option<::std::rc::Rc"), "{source}");
}

#[test]
fn preserves_optional_callable_values_across_compatible_parameter_adaptation() {
    let source = source_for(
        r"
interface Options {
  in?: (value: unknown) => unknown;
}

function consume(context?: (value: Date) => unknown): void {}

export function run(options?: Options): void {
  consume(options?.in);
}
",
    );

    assert!(source.contains(".map(|value|"), "{source}");
    assert!(!source.contains("map_or(None::<"), "{source}");
}

#[test]
fn emits_optional_callable_logical_or_as_selected_unknown_value() {
    let source = source_for(
        r"
function select(value: unknown, context?: (value: unknown) => unknown): unknown {
  return context || value;
}
",
    );

    assert!(source.contains(".map_or_else("), "{source}");
    assert!(source.contains("SmeltUnknown::Function"), "{source}");
    assert!(!source.contains(".is_some() ||"), "{source}");
}

#[test]
fn flattens_python_nested_optional_annotations() {
    let source = source_for_py(
        r"
from typing import Optional

def read(value: Optional[Optional[int]]) -> Optional[int]:
    return value
",
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
        r"
function read(values: Array<number | undefined>, index: number): number | undefined {
  return values?.[index];
}
",
    );

    assert!(source.contains(".cloned().flatten()"), "{source}");
    assert!(!source.contains("Option<Option<f64>>"), "{source}");
}

#[test]
fn emits_optional_constructor_parameter_without_unwrapping_optional_argument() {
    let source = source_for(
        r"
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
",
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
        r"
function build(key: unknown, value: unknown): unknown {
  const result: unknown = {};
  // @ts-expect-error dynamic index writes are accepted at erased object boundaries.
  result[key] = value;
  return result;
}
",
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
        r"
function swap(data: unknown[], i: number, j: number): void {
  [data[i], data[j]] = [data[j], data[i]];
}
",
    );

    assert!(source.contains("let __smelt_destructure"), "{source}");
    assert!(source.contains("__smelt_destructure.get"), "{source}");
    assert!(source.contains("index = 1.0"), "{source}");
    assert_eq!(
        source
            .matches("data[smelt_assign_index] = __smelt_destructure.get")
            .count(),
        2,
        "{source}"
    );
    assert!(
        !source.contains("data[smelt_assign_index] = SmeltUnknown::Array"),
        "{source}"
    );
}

#[test]
fn emits_callback_typeof_unknown_as_runtime_match() {
    let source = source_for(
        r"
function mapType(values: unknown[]): string[] {
  return values.map((item) => typeof item);
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Symbol(_) => \"symbol\""),
        "{source}"
    );
    assert!(
        source.contains(
            "SmeltUnknown::Null | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Promise(_) => \"object\""
        ),
        "{source}"
    );
}

#[test]
fn emits_array_for_each_with_function_callback_parameter() {
    let source = source_for(
        r"
function visit(values: number[], callback: (value: number, index: number, data: number[]) => void): void {
  values.forEach(callback);
}
",
    );

    assert!(source.contains(".iter().enumerate().for_each"), "{source}");
    assert!(source.contains("(smelt_callback)"), "{source}");
    assert!(!source.contains("Default::default()"), "{source}");
}

#[test]
fn emits_array_flat_map_with_function_callback_parameter() {
    let source = source_for(
        r"
function expand(values: number[], callback: (value: number, index: number, data: number[]) => number[]): number[] {
  return values.flatMap(callback);
}
",
    );

    assert!(source.contains(".iter().enumerate().flat_map"), "{source}");
    assert!(source.contains("(smelt_callback)"), "{source}");
    assert!(!source.contains("Default::default()"), "{source}");
}

#[test]
fn ignores_unused_void_call_result_without_null_cast() {
    let source = source_for(
        r"
function wrapped(value: unknown): void;
function wrapped(value: unknown): unknown {
  return value;
}

function run(value: unknown): void {
  wrapped(value);
}
",
    );

    assert!(source.contains("let _ = wrapped"), "{source}");
    assert!(!source.contains("unknown is not null"), "{source}");
}

#[test]
fn emits_non_destructive_object_getter_reads() {
    let source = source_for(
        r"
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
",
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
fn instanceof_boxed_primitive_wrapper_emits_marker_check() {
    // `value instanceof Boolean` / `String` / `Symbol` over an erased value emits
    // a marker-key check on `SmeltUnknown::Object`. A primitive bool/string/symbol
    // is not an `Object`, so the check is the correct `false`.
    for (target, marker) in [
        ("Boolean", "__smelt_boolean"),
        ("String", "__smelt_string"),
        ("Symbol", "__smelt_symbol"),
    ] {
        let source = source_for(&format!(
            "export function f(value: any): boolean {{ return value instanceof {target}; }}"
        ));
        assert!(
            source.contains(&format!("value.contains_key(\"{marker}\")")),
            "expected `instanceof {target}` to emit a `{marker}` marker check:\n{source}"
        );
    }
}

#[test]
fn instanceof_buffer_and_is_buffer_emit_marker_check() {
    // `value instanceof Buffer` and `Buffer.isBuffer(value)` over an erased value
    // both emit the shared `__smelt_buffer` marker-key check on
    // `SmeltUnknown::Object`; a non-buffer never carries the marker, so the check
    // is the correct `false`.
    for source in [
        "export function f(value: any): boolean { return value instanceof Buffer; }",
        "export function f(value: any): boolean { return Buffer.isBuffer(value); }",
    ] {
        let generated = source_for(source);
        assert!(
            generated.contains("value.contains_key(\"__smelt_buffer\")"),
            "expected `{source}` to emit a `__smelt_buffer` marker check:\n{generated}"
        );
    }
}

#[test]
fn buffer_from_emits_marker_record() {
    // `Buffer.from([...])` erases to a marker-bearing record carrying the
    // `__smelt_buffer` identity key so downstream `instanceof`/`isBuffer` resolve.
    let generated =
        source_for("export function f() { const b = Buffer.from([1, 2, 3]); return b; }");
    assert!(
        generated.contains("\"__smelt_buffer\""),
        "expected `Buffer.from` to emit a `__smelt_buffer` marker record:\n{generated}"
    );
}

#[test]
fn runtime_host_marker_registry_hides_boxed_primitive_wrappers() {
    // The runtime `for...in` / `Object.keys` filter (`smelt_object_has_host_marker`
    // / `smelt_record_has_host_marker`) is generated from the shared
    // `smelt_stdlib::host_object` registry. It must include the boxed-primitive
    // markers (`__smelt_boolean`, `__smelt_string`, `__smelt_symbol`) so a boxed
    // wrapper object never leaks its internal marker key as an enumerable
    // property — a case the previously hand-maintained list omitted for
    // `__smelt_boolean`/`__smelt_string`/`__smelt_symbol`.
    let source = source_for("export function f(value: any): boolean { return value instanceof Boolean; }");
    let host_marker_fn = source
        .split("fn smelt_object_has_host_marker(")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .expect("missing smelt_object_has_host_marker helper");
    for marker in [
        "__smelt_arraybuffer",
        "__smelt_buffer",
        "__smelt_weakmap",
        "__smelt_number",
        "__smelt_boolean",
        "__smelt_string",
        "__smelt_symbol",
    ] {
        assert!(
            host_marker_fn.contains(&format!("\"{marker}\"")),
            "runtime host-marker registry must include `{marker}`:\n{host_marker_fn}"
        );
    }
}

/// The private host-marker array builder emits every registry marker plus the
/// runtime-owned abort/namespace markers, so the frontend construction path, the
/// `instanceof` codegen path, and this runtime filter share one source of truth.
#[test]
fn host_marker_registry_array_covers_registry_and_runtime_markers() {
    let array = host_marker_registry_array();
    for marker in smelt_stdlib::host_object_markers() {
        assert!(
            array.contains(&format!("\"{marker}\"")),
            "host-marker array missing registry marker `{marker}`: {array}"
        );
    }
    for marker in [
        "__smelt_abortcontroller",
        "__smelt_abortsignal",
        "__smelt_builtin_namespace",
        "__smelt_global_object",
    ] {
        assert!(
            array.contains(&format!("\"{marker}\"")),
            "host-marker array missing runtime marker `{marker}`: {array}"
        );
    }
}

#[test]
fn set_interval_registers_repeating_timer_and_clear_interval_cancels() {
    // `setInterval`/`clearInterval` must lower onto the same virtual-time timer
    // queue as `setTimeout`, with the interval re-arming itself after each fire.
    let source = source_for(
        r"
const id = setInterval(() => {}, 10);
clearInterval(id);
",
    );

    // The dedicated repeating-timer helper is emitted and called.
    assert!(source.contains("fn smelt_set_interval("), "{source}");
    assert!(source.contains("smelt_set_interval("), "{source}");
    // `clearInterval` reuses the cancel-by-id timer path.
    assert!(source.contains("fn smelt_clear_interval"), "{source}");
    assert!(source.contains("smelt_clear_interval("), "{source}");
    // Interval timers carry a period and re-arm in the drain loop; one-shot
    // timeouts carry `period_ms: None`.
    assert!(source.contains("period_ms: Some(period_ms)"), "{source}");
    assert!(
        source.contains("if let Some(period_ms) = timer.period_ms {"),
        "{source}"
    );
}

#[test]
fn drains_virtual_timers_before_async_sleep_can_change_threads() {
    let source = source_for(
        r"
setTimeout(() => {}, 10);
",
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
        r"
function partitionLike(values: number[]): [number[], number[]] {
  const result: [number[], number[]] = [[], []];
  result[0].push(values[0]);
  result[1].push(values[1]);
  return result;
}
",
    );

    assert!(source.contains("result.0 ="), "{source}");
    assert!(source.contains("result.1 ="), "{source}");
}

#[test]
fn function_item_value_references_share_one_accessor() {
    // JavaScript reference identity requires every reference to the same named
    // function value to be the SAME runtime value. The frontend wraps a
    // function item used as a value in a fresh forwarding closure per
    // reference, so the two `func1` arguments below each lower to their own
    // `ExprKind::Closure`. Both wrappers are tagged with the same source item,
    // so when they are erased to `SmeltUnknown` codegen must route them through
    // one per-item compile-time accessor `__smelt_fn_value_<key>()` with a
    // shared key, and emit that accessor (with its `OnceCell` cache) exactly
    // once for the crate.
    let source = source_for(
        r"
function func1(): void {}
function takesTwo(a: unknown, b: unknown): boolean { return true; }
const r = takesTwo(func1, func1);
",
    );

    // Both func1 arguments live in `fn main`; both must call the SAME accessor.
    // Scope the call-count to the `main` body only (up to the accessor's own
    // definition, which is appended after the function loop) so the definition's
    // `fn __smelt_fn_value_<key>()` header is not miscounted as a call site.
    let main_body = source
        .split_once("fn main")
        .map(|(_prelude, body)| body)
        .and_then(|body| body.split_once("\nfn __smelt_fn_value_"))
        .map(|(main_body, _accessors)| main_body)
        .expect("emitted source has a main function and an accessor definition");
    let first_call = main_body
        .match_indices("__smelt_fn_value_")
        .next()
        .map(|(index, _)| index)
        .expect("a function-item value accessor call is emitted");
    let key = main_body[first_call + "__smelt_fn_value_".len()..]
        .split('(')
        .next()
        .map(str::to_owned)
        .expect("the accessor call has a key");
    let accessor_call = format!("__smelt_fn_value_{key}()");
    let call_count = main_body.matches(&accessor_call).count();
    assert_eq!(
        call_count, 2,
        "both func1 references must call the same accessor {accessor_call}; got {call_count}\n{source}"
    );

    // The accessor must be defined exactly once, lazily caching ONE erased
    // `SmeltUnknown::Function` value behind a `OnceCell`.
    let definition = format!("fn __smelt_fn_value_{key}() -> SmeltUnknown {{");
    assert_eq!(
        source.matches(&definition).count(),
        1,
        "the accessor must be defined exactly once\n{source}"
    );
    let accessor_def = source
        .split_once(&definition)
        .map(|(_, tail)| tail)
        .expect("the accessor definition is emitted");
    assert!(
        accessor_def.contains("::std::cell::OnceCell"),
        "the accessor must cache its value in a OnceCell\n{source}"
    );
    assert!(
        accessor_def.contains("SmeltUnknown::Function"),
        "the accessor must build an erased SmeltUnknown::Function value\n{source}"
    );

    // The old runtime cache helper must be gone entirely.
    assert!(
        !source.contains("smelt_function_item_value"),
        "the removed runtime cache helper must not be emitted\n{source}"
    );
}

#[test]
fn function_item_erased_fn_references_share_one_accessor() {
    // A named function item used as a value in a TYPED erased-rest function
    // context (e.g. Remeda's `doNothing`) lowers to a concrete
    // `SmeltErasedFunction`, not the `SmeltUnknown` erased value. Building it
    // inline mints a fresh callback `Rc` per evaluation, so two references would
    // never be `Rc::ptr_eq`. JavaScript returns one shared singleton. Both
    // references below carry the same source item key, so codegen must route the
    // build through one per-item `__smelt_fn_erased_<key>()` accessor (with a
    // `OnceCell<SmeltErasedFunction>` cache) emitted exactly once for the crate.
    let source = source_for(
        r"
function doesNothing(...args: unknown[]): void {}
function takesTwo(a: (...args: unknown[]) => void, b: (...args: unknown[]) => void): boolean { return true; }
const r = takesTwo(doesNothing, doesNothing);
",
    );

    // Both arguments live in `fn main`; both must call the SAME accessor. Scope
    // the call count to the `main` body only (up to the accessor's own
    // definition appended after the function loop) so the `fn __smelt_fn_erased_`
    // header is not miscounted as a call site.
    let main_body = source
        .split_once("fn main")
        .map(|(_prelude, body)| body)
        .and_then(|body| body.split_once("\nfn __smelt_fn_erased_"))
        .map(|(main_body, _accessors)| main_body)
        .expect("emitted source has a main function and an erased-fn accessor definition");
    let first_call = main_body
        .match_indices("__smelt_fn_erased_")
        .next()
        .map(|(index, _)| index)
        .expect("a function-item erased-fn accessor call is emitted");
    let key = main_body[first_call + "__smelt_fn_erased_".len()..]
        .split('(')
        .next()
        .map(str::to_owned)
        .expect("the accessor call has a key");
    let accessor_call = format!("__smelt_fn_erased_{key}()");
    let call_count = main_body.matches(&accessor_call).count();
    assert_eq!(
        call_count, 2,
        "both doesNothing references must call the same accessor {accessor_call}; got {call_count}\n{source}"
    );

    // The accessor must be defined exactly once, lazily caching ONE
    // `SmeltErasedFunction` behind a `OnceCell` so every call shares one inner
    // callback `Rc`.
    let definition = format!("fn __smelt_fn_erased_{key}() -> SmeltErasedFunction {{");
    assert_eq!(
        source.matches(&definition).count(),
        1,
        "the erased-fn accessor must be defined exactly once\n{source}"
    );
    let accessor_def = source
        .split_once(&definition)
        .map(|(_, tail)| tail)
        .expect("the accessor definition is emitted");
    assert!(
        accessor_def.contains("::std::cell::OnceCell"),
        "the accessor must cache its value in a OnceCell\n{source}"
    );
    assert!(
        accessor_def.contains("SmeltErasedFunction {"),
        "the accessor must build a SmeltErasedFunction value\n{source}"
    );
}

#[test]
fn user_arrow_does_not_route_through_an_accessor() {
    // A user-written arrow is NOT a bare function-item-as-value wrapper. It must
    // keep JavaScript's fresh identity (a new closure value each evaluation), so
    // erasing it to `SmeltUnknown` must NOT route it through a per-item
    // accessor; it keeps its plain per-reference erased wrapper.
    let source = source_for(
        r"
function takesOne(a: unknown): boolean { return true; }
const g = (x: unknown) => x;
const r = takesOne(g);
",
    );

    assert!(
        !source.contains("__smelt_fn_value_"),
        "a user arrow must not route through a function-item value accessor\n{source}"
    );
}

#[test]
fn user_arrow_forwarding_a_function_emits_no_accessor() {
    // When a user arrow forwards to a named function (`(x) => func1(x)`), the
    // arrow itself stays a fresh closure value and the inner `func1(x)` is a
    // direct typed call, not a function-item-as-value wrapper erased to
    // `SmeltUnknown`. Identity is only stabilized at the erase site, so no
    // per-item accessor is emitted for this shape.
    let source = source_for(
        r"
function func1(x: number): number { return x; }
function takesOne(a: (value: number) => number): boolean { return true; }
const r = takesOne((x) => func1(x));
",
    );

    let body = source
        .split_once("fn main")
        .map(|(_prelude, body)| body)
        .expect("emitted source has a main function");
    assert!(
        !source.contains("__smelt_fn_value_"),
        "no function-item value accessor should be emitted for a forwarding arrow\n{source}"
    );
    // The outer arrow is still a plain closure value.
    assert!(
        body.contains("::std::rc::Rc::new(|closure_arg_0"),
        "the user arrow must remain a plain closure value\n{source}"
    );
}

#[test]
fn reuses_a_stable_identity_when_erasing_a_source_list_local() {
    // Erasing the SAME list local twice must reuse one erased-array id so the
    // two erasures compare `===` equal (arrays compare by id). The typed list
    // (`SmeltList`) carries its own reference id, and a `.clone()` shares it, so
    // each erasure reuses the list's `id()` rather than an address-keyed sidecar.
    let source = source_for(
        r"
const data: number[] = [1, 2, 3];
const same = (data as unknown) === (data as unknown);
",
    );

    let body = source
        .split_once("fn main")
        .map(|(_prelude, body)| body)
        .expect("emitted source has a main function");

    assert!(
        body.contains("let smelt_l = data.clone();"),
        "erasing a list local should bind it to read its reference id\n{source}"
    );
    assert!(
        body.contains("SmeltArray::with_id(smelt_l.id(),"),
        "erasing a list local should build the array with the list's own identity\n{source}"
    );
}

#[test]
fn keeps_a_fresh_identity_when_erasing_a_list_literal() {
    // A fresh list expression (here a literal) must keep `SmeltArray::new` (via
    // `.into()`) so distinct array literals are never `===`, matching JS.
    let source = source_for(
        r"
const other = ([1, 2, 3] as unknown) === ([1, 2, 3] as unknown);
",
    );

    let body = source
        .split_once("fn main")
        .map(|(_prelude, body)| body)
        .expect("emitted source has a main function");

    assert!(
        body.contains("SmeltUnknown::Array(vec![") && body.contains("].into())"),
        "a list literal should erase through the fresh-id `.into()` path\n{source}"
    );
    assert!(
        !body.contains("smelt_list_identity("),
        "a list literal must not be keyed to a reused identity\n{source}"
    );
    assert!(
        !body.contains("SmeltArray::with_id("),
        "a list literal must use a fresh `SmeltArray::new` identity\n{source}"
    );
}

#[test]
fn lowers_function_expression_value_rest_parameters() {
    // A `function (...args)` expression used as a returned value (an object-valued
    // function expression position) must lower its rest parameter the same way
    // top-level functions and arrow expressions do, packing trailing arguments
    // into a single list parameter instead of erroring out.
    let source = source_for(
        r"
export function nthArgValue(n: number): (...args: number[]) => number {
  return function (...args: number[]): number {
    return args[n];
  };
}
",
    );

    // The returned closure must accept a single packed rest list
    // (`SmeltList<f64>`), proving the rest parameter was lowered into one list
    // closure parameter rather than rejected.
    assert!(
        source.contains("Fn(SmeltList<f64>) -> f64"),
        "function-expression rest parameters must lower to a packed list closure type\n{source}"
    );
    assert!(
        source.contains("move |closure_arg_0: SmeltList<f64>|"),
        "the returned closure must bind the rest list as its single parameter\n{source}"
    );
    assert!(
        !source.contains("rest parameters are not lowered"),
        "the object-valued function-expression rest blocker must be gone\n{source}"
    );
}

#[test]
fn lowers_imported_function_value_as_filter_callback() {
    // An array method whose callback is a bare imported function value (a named
    // local, not an inline arrow) must lower to a real callback closure instead
    // of failing with "array callback local callback `X` is not in scope". The
    // import is opaque here, so the closure body resolves like a direct call:
    // it produces a conservative value that the predicate path coerces to bool.
    let source = source_for(
        r#"
import { isNotNil } from "./isNotNil";
function run(): unknown {
  const arr = [1, 2, 3];
  return arr.filter(isNotNil);
}
"#,
    );

    assert!(
        source.contains(".filter_map(|(index, item)|"),
        "imported filter callback should lower to a filtering closure\n{source}"
    );
    assert!(
        source.contains("(smelt_callback)(item.clone(), index as i64,"),
        "the closure body should call the resolved callback value\n{source}"
    );
}

#[test]
fn lowers_imported_function_value_as_map_callback() {
    // The same support extends to value-returning array methods like `map`: a
    // bare imported function passed as the callback lowers to a mapping closure
    // rather than being rejected for not being an inline arrow.
    let source = source_for(
        r#"
import { deburr } from "./deburr";
function run(): unknown {
  const arr = ["a", "b"];
  return arr.map(deburr);
}
"#,
    );

    assert!(
        source.contains("smelt_array.iter().enumerate().map(|(index, item)|"),
        "imported map callback should lower to a mapping closure\n{source}"
    );
    assert!(
        source.contains("(smelt_callback)(item.clone(), index as i64,"),
        "the closure body should call the resolved callback value\n{source}"
    );
}

#[test]
fn lowers_array_at_inside_callback_body() {
    // `data.at(i)` called inside a `.map` callback body must lower through the
    // same optional-index path used in statement position (es-toolkit
    // `src/array/at.spec.ts`), rather than failing closure-body lowering.
    let source = source_for(
        r"
const data = [10, 20, 30];
const indices = [0, 1, 2];
const out = indices.map(i => data.at(i));
",
    );

    assert!(
        source.contains(".get(") || source.contains("optional"),
        "array .at inside a callback should lower to a checked optional index\n{source}"
    );
}

#[test]
fn lowers_array_join_inside_callback_body() {
    // `parts.join('-')` inside a callback body lowers to the same `StringJoin`
    // the direct path emits.
    let source = source_for(
        r#"
const rows: string[][] = [["a", "b"], ["c", "d"]];
const joined = rows.map(parts => parts.join("-"));
"#,
    );

    assert!(
        source.contains(".join(&\"-\".to_owned())"),
        "array .join inside a callback should lower to a Rust join\n{source}"
    );
}

#[test]
fn lowers_array_join_default_separator_inside_callback_body() {
    // A bare `.join()` inside a callback body defaults to the `","` separator,
    // matching the statement-position lowering.
    let source = source_for(
        r"
const rows: number[][] = [[1, 2], [3, 4]];
const joined = rows.map(parts => parts.join());
",
    );

    assert!(
        source.contains(".join(&\",\".to_owned())"),
        "array .join() inside a callback should default to a comma separator\n{source}"
    );
}

#[test]
fn lowers_map_has_inside_callback_body() {
    // A typed `Map` receiver's `.has(key)` inside a callback body lowers to the
    // same `contains_key` check the direct `map_has_call` path emits (es-toolkit
    // `src/map/every.spec.ts`).
    let source = source_for(
        r"
export function every<K, V>(
  map: Map<K, V>,
  doesMatch: (value: V, key: K, map: Map<K, V>) => boolean
): boolean {
  for (const [key, value] of map) {
    if (!doesMatch(value, key, map)) {
      return false;
    }
  }
  return true;
}

const m = new Map<string, number>();
const r = every(m, (value, key, originalMap) => originalMap.has(key) && value > 0);
",
    );

    assert!(
        source.contains("contains_key"),
        "Map .has inside a callback should lower to a contains_key check\n{source}"
    );
}

#[test]
fn lowers_string_starts_with_inside_callback_body() {
    // `value.startsWith(prefix)` inside a callback body lowers to the same
    // prefix test the direct `string_affix_call` path emits.
    let source = source_for(
        r#"
const words = ["apple", "banana"];
const flags = words.map(word => word.startsWith("a"));
"#,
    );

    assert!(
        source.contains("starts_with"),
        "string .startsWith inside a callback should lower to a starts_with test\n{source}"
    );
}

#[test]
fn lowers_string_ends_with_on_erased_callback_receiver() {
    // An erased (`unknown`) callback receiver reaching the string-only
    // `endsWith` method is coerced to a string before the suffix test.
    let source = source_for(
        r#"
declare const values: unknown[];
const flags = values.map(value => value.endsWith("x"));
"#,
    );

    assert!(
        source.contains("ends_with"),
        "string .endsWith on an erased callback receiver should still lower to ends_with\n{source}"
    );
}

#[test]
fn emits_materialized_property_reads_and_writes_as_accessor_calls() {
    let mut ctx = py_frontend::HirCtx::new();
    let source = r"
class Model:
    _value: int
    value: int

    def __init__(self, value: int) -> None:
        self._value = value

    def __smelt_get_value(self) -> int:
        return self._value

    def __smelt_set_value(self, value: int) -> None:
        self._value = value

def update(model: Model) -> int:
    model.value = 7
    return model.value
";
    assert!(
        py_frontend::to_hir(source, FileId(0), &mut ctx).is_ok(),
        "HIR"
    );
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering");
    let class_name = mir
        .classes
        .iter()
        .find(|class| mir.symbols.get(class.name) == Some("Model"))
        .map(|class| class.name)
        .expect("Model class");
    let value_name = mir
        .classes
        .iter()
        .find(|class| class.name == class_name)
        .and_then(|class| {
            class
                .fields
                .iter()
                .find(|field| mir.symbols.get(field.name) == Some("value"))
                .map(|field| field.name)
        })
        .expect("value field");
    let value_ty = mir
        .classes
        .iter()
        .find(|class| class.name == class_name)
        .and_then(|class| {
            class
                .fields
                .iter()
                .find(|field| field.name == value_name)
                .map(|field| field.ty)
        })
        .expect("value type");
    let method = |expected: &str| {
        let function = mir
            .functions
            .iter()
            .find(|function| {
                matches!(
                    function.origin,
                    HirOrigin::ClassMethod { class, method, .. }
                        if class == class_name && mir.symbols.get(method) == Some(expected)
                )
            })
            .unwrap_or_else(|| panic!("{expected} method"));
        function.id
    };
    let getter = method("__smelt_get_value");
    let setter = method("__smelt_set_value");
    let class = mir
        .classes
        .iter_mut()
        .find(|class| class.name == class_name)
        .expect("mutable Model class");
    class.descriptors.push(smelt_mir::MirDescriptor {
        name: value_name,
        read_ty: value_ty,
        write_ty: Some(value_ty),
        getter: Some(getter),
        setter: Some(setter),
        data_descriptor: true,
        is_static: false,
        value_fields: Vec::new(),
    });

    smelt_mir::opt::optimize(&mut mir);
    let emitted = emit_source(&mir).expect("Rust source");
    assert!(emitted.contains("model.__smelt_set_value(7);"), "{emitted}");
    assert!(emitted.contains("model.__smelt_get_value()"), "{emitted}");
    assert!(
        !emitted.contains("model.value ="),
        "descriptor retained direct storage assignment\n{emitted}"
    );

    mir.classes
        .iter_mut()
        .find(|class| class.name == class_name)
        .and_then(|class| class.descriptors.first_mut())
        .expect("Model descriptor")
        .data_descriptor = false;
    let emitted = emit_source(&mir).expect("non-data descriptor Rust source");
    // `Model` mutates a field (`update` writes `model.value`, the setter writes
    // `self._value`), so it is lifted to a reference class: instance storage that
    // shadows a non-data descriptor is written through the shared cell.
    assert!(
        emitted.contains("model.0.borrow_mut().value = 7;"),
        "instance storage must shadow a non-data descriptor\n{emitted}"
    );
}

#[test]
fn emits_abort_controller_concrete_cancellation_model() {
    // `new AbortController()` emits two marker-bearing records sharing a mutable
    // `aborted` flag; `controller.abort()` routes through the runtime helper that
    // flips the flag and fires listeners; `instanceof` checks the markers.
    let source = source_for(
        r#"
const controller = new AbortController();
const signal = controller.signal;
signal.addEventListener("abort", () => {});
controller.abort();
const aborted = signal.aborted;
const isController = controller instanceof AbortController;
const isSignal = signal instanceof AbortSignal;
"#,
    );

    assert!(
        source.contains("__smelt_abortcontroller") && source.contains("__smelt_abortsignal"),
        "AbortController/AbortSignal records must carry their identity markers\n{source}"
    );
    assert!(
        source.contains("smelt_abort_method"),
        "abort/addEventListener reads must route through the runtime abort method helper\n{source}"
    );
    assert!(
        source.contains("smelt_abort_signal_fire"),
        "the runtime prelude must define the abort-firing helper\n{source}"
    );
    assert!(
        source.contains(
            "matches!(controller.clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_abortcontroller\"))"
        ),
        "instanceof AbortController must check the controller marker\n{source}"
    );
    assert!(
        source.contains(
            "matches!(signal.clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_abortsignal\"))"
        ),
        "instanceof AbortSignal must check the signal marker\n{source}"
    );
}

#[test]
fn lowers_numeric_truthy_condition_inside_callback() {
    // A non-boolean numeric value used as a callback condition (the common
    // `(value, index) => index ? a : b` index-guard idiom) lowers to an
    // explicit `!= 0` test rather than rejecting the callback.
    let source = source_for(
        r"
function pick(values: number[]): number[] {
  return values.map((value, index) => (index ? value * 2 : value));
}
",
    );

    assert!(
        source.contains("!= 0"),
        "numeric callback condition must lower to a non-zero test: {source}"
    );
}

#[test]
fn falls_back_to_closure_body_for_try_catch_callback() {
    // A `.map` callback whose body uses a statement form the side-effect-free
    // callback IR cannot represent (here `try`/`catch`) must retry through full
    // closure-body lowering instead of failing the whole file. `source_for`
    // panics on any blocker, so reaching codegen proves the fallback fired.
    let source = source_for(
        r"
function run(values: string[]): Array<string | undefined> {
  return values.map((value) => {
    try {
      return value;
    } catch (e) {
      return undefined;
    }
  });
}
",
    );

    assert!(source.contains(".map("), "{source}");
}

#[test]
fn falls_back_to_closure_body_for_sort_comparator_with_loop() {
    // An `Array.prototype.sort` comparator whose body uses a `for` loop must
    // retry through full closure-body lowering rather than rejecting the file.
    let source = source_for(
        r"
function order(items: number[][]): number[][] {
  return items.slice().sort((a, b) => {
    for (let i = 0; i < a.length; i++) {
      if (a[i] !== b[i]) {
        return a[i] - b[i];
      }
    }
    return 0;
  });
}
",
    );

    assert!(source.contains("sort"), "{source}");
}

#[test]
fn lowers_erased_local_value_passed_as_named_callback() {
    // A local holding a callable value whose static type is erased (`any`) and
    // handed to an array method as a named callback (`values.map(fn)`) lowers to
    // a wrapper closure that calls the captured local, instead of rejecting it
    // for not being an inline arrow / not having a callback entry.
    let source = source_for(
        r"
function apply(values: string[], fn: any): string[] {
  return values.map(fn);
}
",
    );

    assert!(source.contains(".map("), "{source}");
}

#[test]
fn lowers_array_concat_with_multiple_arguments() {
    // `Array.prototype.concat` accepts any number of arguments; each array
    // argument is spread and each scalar is appended, chained left to right.
    let source = source_for(
        r"
export function joinAll(a: number[], b: number[], c: number[]): number[] {
  return a.concat(b, c);
}
export function appendScalars(a: number[], x: number, y: number): number[] {
  return a.concat(x, y);
}
export function copy(a: number[]): number[] {
  return a.concat();
}
",
    );
    // Two right-arguments produce chained list concatenations (`.chain(...)`),
    // and the no-argument form is a shallow copy.
    assert!(source.matches(".chain(").count() >= 3, "{source}");
    assert!(source.contains("a.clone()"), "{source}");
}

#[test]
fn lowers_typeof_in_array_element_position() {
    // `typeof x` appearing as an array literal element (which routes through the
    // generic `unary_expression` path rather than the top-level expression
    // dispatcher) must lower instead of rejecting the file.
    let source = source_for(
        r#"
export function tags(x: unknown): string[] {
  return [typeof x, "tail"];
}
"#,
    );
    assert!(source.contains("vec!"), "{source}");
}

#[test]
fn folds_number_constant_in_exported_const() {
    // An exported const aliasing a well-known numeric member constant folds to a
    // numeric literal instead of rejecting the initializer as non-foldable.
    let source = source_for(
        r"
export const SAFE = Number.MAX_SAFE_INTEGER;
export function ceiling(): number {
  return SAFE;
}
",
    );
    assert!(
        source.contains("9007199254740991") || source.contains("9_007_199_254_740_991"),
        "{source}"
    );
}

#[test]
fn lowers_array_push_with_multiple_arguments() {
    // `Array.prototype.push` accepts any number of arguments, appending each in
    // order and returning the new length; spread arguments extend the list.
    let source = source_for(
        r"
export function appendThree(a: number[]): number {
  return a.push(1, 2, 3);
}
export function appendSpreadAndScalar(a: number[], b: number[]): void {
  a.push(...b, 9);
}
",
    );
    // Three pushes for the scalar case, plus an extend + push for the second.
    assert!(source.matches(".push(").count() >= 4, "{source}");
    assert!(source.contains("extend") || source.contains(".chain("), "{source}");
}

#[test]
fn lowers_array_length_from_optional_and_member_numeric() {
    // `new Array(n)` where `n` is an optional/number-typed parameter, and
    // `new Array(xs.length)` where the length is a numeric member read, both
    // preallocate a list instead of rejecting the length as non-numeric.
    let source = source_for(
        r"
export function makeOptional(n?: number): number[] {
  const result = new Array(n);
  return result;
}
export function makeFromLength<T>(keys: T[]): T[] {
  const result = new Array(keys.length);
  return result;
}
",
    );
    assert!(source.contains("Vec::new") || source.contains("SmeltList"), "{source}");
}

#[test]
fn lowers_new_set_from_optional_and_erased_iterables() {
    // `new Set(iterable)` accepts an optional array, an existing set (copy), and
    // an erased iterable surface, instead of requiring a concretely-typed array.
    let source = source_for(
        r"
export function fromOptional(xs?: string[]): Set<string> {
  return new Set(xs);
}
export function fromSet(s: Set<number>): Set<number> {
  return new Set(s);
}
",
    );
    assert!(source.contains("HashSet") || source.contains("SmeltSet") || source.contains("Set"), "{source}");
}

#[test]
fn lowers_object_has_own_on_optional_array_and_generic_receivers() {
    // `Object.hasOwn` lowers for an array receiver (numeric in-bounds check), an
    // optional record receiver (asserted to its inner shape), and a generic
    // `T extends object` receiver (treated as a string-keyed record).
    let source = source_for(
        r"
export function indexPresent(arr: number[], i: number): boolean {
  return Object.hasOwn(arr, i);
}
export function keyPresent(obj?: Record<string, number>, key?: string): boolean {
  return obj != null && key != null && Object.hasOwn(obj, key);
}
export function genericKey<T extends object>(object: T, key: string): boolean {
  return Object.hasOwn(object, key);
}
",
    );
    // Array case becomes a length-bounded comparison.
    assert!(source.contains(".len()") || source.contains("len("), "{source}");
}

#[test]
fn lowers_string_methods_on_coercible_receivers() {
    // String padding/charAt/repeat/trim and prefix/suffix-with-position accept
    // string-compatible receivers (generic `T extends string`, erased returns),
    // numeric-like length/index/count arguments, and an optional position.
    let source = source_for(
        r"
export function pad(s: string, n: number, c: string): string {
  return s.padStart(n, c);
}
export function firstChar<T extends string>(s: T): string {
  return s.charAt(0);
}
export function repeated(s: string, n: number): string {
  return s.repeat(n);
}
export function startsAt(s: string, t: string, p: number): boolean {
  return s.startsWith(t, p);
}
",
    );
    assert!(source.contains("starts_with") || source.contains("StringAffix") || source.contains("char"), "{source}");
}

#[test]
fn lowers_for_in_over_generic_object_receiver() {
    // `for...in` over an unconstrained generic / erased object receiver casts to
    // a string-keyed record and iterates its keys.
    let source = source_for(
        r"
export function keysOf<T>(object: T): string[] {
  const out: string[] = [];
  for (const key in object) {
    out.push(key);
  }
  return out;
}
",
    );
    assert!(source.contains("for "), "{source}");
}

#[test]
fn lowers_array_fill_with_compatible_value() {
    // `Array.prototype.fill` coerces an assignment-compatible value (generic
    // element type) instead of requiring an exact element-type match.
    let source = source_for(
        r"
export function fillAll<T>(a: T[], v: T): T[] {
  return a.fill(v);
}
export function fillRange(a: number[]): number[] {
  return a.fill(0, 1, 2);
}
",
    );
    assert!(source.contains("fill"), "{source}");
}

#[test]
fn lowers_truthy_guard_on_param_with_dependent_default() {
    // A default-parameter initializer (`end = array ? array.length : 0`) can
    // register a name in the lexical `locals` map whose `LocalId` is not
    // materialized in the function body. A later bare-identifier truthy guard
    // (`if (!end) { ... }`) must not panic looking that local up; it falls back
    // to "no narrowing" instead. Regression for the es-toolkit `compat/array/
    // fill.ts` frontend panic.
    let source = source_for(
        r"
export function fillRange<T>(
  array: T[] | null | undefined,
  value: T,
  start = 0,
  end = array ? array.length : 0
): T[] {
  start = Math.floor(start);
  end = Math.floor(end);
  if (!start) {
    start = 0;
  }
  if (!end) {
    end = 0;
  }
  return [];
}
",
    );
    assert!(source.contains("fill_range"), "{source}");
}

#[test]
fn lowers_callback_that_reassigns_its_parameter() {
    // A callback that reassigns its own parameter is not representable in the
    // compact side-effect-free callback IR, but the full closure-body path
    // makes parameters mutable locals. The fallback must retry there instead of
    // failing with "callback parameter assignment is not supported yet".
    let source = source_for(
        r"
export function run(values: number[]): number[] {
  return values.map(value => {
    if (value < 0) {
      value = 0;
    }
    return value + 1;
  });
}
",
    );
    assert!(source.contains("fn run("), "{source}");
}

#[test]
fn lowers_foreach_callback_without_fixed_item_parameter() {
    // `forEach((...args) => ...)` and `forEach(() => ...)` have no fixed item
    // parameter, so the statement-loop shortcut declines and the general
    // callback lowering handles them instead of failing with "array forEach
    // callbacks require an item parameter".
    let source = source_for(
        r"
export function run(sources: unknown[], apply: (...args: unknown[]) => void): void {
  sources.forEach((...args: unknown[]) => apply(...args));
}
export function tick(values: number[], onTick: () => void): void {
  values.forEach(() => onTick());
}
",
    );
    assert!(source.contains("fn run("), "{source}");
    assert!(source.contains("fn tick("), "{source}");
}

#[test]
fn lowers_named_opaque_predicate_for_array_some() {
    // A named/opaque predicate passed to `some` (`xs.some(matchFunc)`) lowers to
    // a wrapper closure returning an erased value. The truthy-predicate path must
    // accept the erased return type instead of rejecting it with "array callback
    // callback returns an unsupported type".
    let source = source_for(
        r"
export function run(values: unknown[], matchFunc: (value: unknown) => unknown): boolean {
  return values.some(matchFunc);
}
",
    );
    assert!(source.contains("fn run("), "{source}");
    assert!(source.contains(".any("), "expected a some/any lowering: {source}");
}

#[test]
fn lowers_conditionally_selected_array_callback() {
    // A callback chosen at runtime between callable values must lower as an
    // opaque element-forwarding callback instead of being rejected with
    // "array callback methods currently require arrow function callbacks".
    let source = source_for(
        r#"
import { identity } from "./identity";
export function run(values: unknown[], flag: boolean): unknown[] {
  return values.map(flag ? Object : identity);
}
"#,
    );
    assert!(source.contains("fn run("), "{source}");
}

#[test]
fn lowers_lodash_two_argument_collection_callback_form() {
    // `import * as _ from "lodash"; _.map(values, cb)` is the lodash
    // free-function form: collection first, iteratee second. The receiver is an
    // opaque imported namespace, so the call stays a placeholder, but both the
    // collection and the iteratee must lower (no "require exactly one callback
    // argument" rejection of the trailing iteratee).
    let source = source_for(
        r#"
import * as _ from "lodash";
export function run(values: number[]): void {
  _.map(values, value => value + 1);
  _.some(values, value => value > 0);
}
"#,
    );
    assert!(source.contains("fn run("), "{source}");
}

#[test]
fn lowers_compact_unsupported_method_inside_callback_through_closure_body() {
    // `String.prototype.repeat` is modeled by the general method-call lowering
    // but not by the restricted compact-callback method dispatcher. An
    // expression-bodied `map` callback first lowers through the compact path,
    // which rejects `repeat` with "is not lowered into closure bodies yet"; the
    // fallback must retry through the full closure-body path, which routes the
    // receiver through the general `expression` lowering and emits the real
    // `.repeat(...)` call against the closure's element-typed parameter.
    let source = source_for(
        r"
export function cbRepeat(xs: string[]): string[] {
  return xs.map(value => value.repeat(2));
}
",
    );
    assert!(
        source.contains("closure_arg_0.clone().repeat("),
        "callback body should emit the real string repeat call: {source}"
    );
}

#[test]
fn timer_typed_extra_args_use_wrapper_closure_not_erased_list() {
    // `setTimeout(callback, ms, ...args)` with a statically typed callback and
    // concretely typed extras must capture the extras in a synthesized
    // zero-argument wrapper closure and call the callback directly, producing no
    // erased `Vec<SmeltUnknown>` argument pack.
    let source = source_for(
        r#"
function greet(name: string, count: number): void {
  console.log(name);
  console.log(count);
}
setTimeout(greet, 10, "hi", 3);
"#,
    );

    // Extras keep their concrete Rust types in per-argument bindings; the erased
    // `smelt_timer_args: Vec<SmeltUnknown>` pack of the dynamic path is absent.
    assert!(source.contains("smelt_timer_arg_0"), "{source}");
    assert!(source.contains("smelt_timer_arg_1"), "{source}");
    assert!(
        source.contains("let smelt_timer_arg_0: String"),
        "typed extra should be a concrete String binding: {source}"
    );
    assert!(
        source.contains("let smelt_timer_arg_1: f64"),
        "typed extra should be a concrete f64 binding: {source}"
    );
    assert!(
        !source.contains("smelt_timer_args"),
        "typed timer extras must not pack into an erased Vec<SmeltUnknown>: {source}"
    );
    // The wrapper captures by value (move) and invokes the callback directly.
    assert!(source.contains("move ||"), "{source}");
    assert!(
        source.contains("(smelt_timer_callback)(smelt_timer_arg_0.clone(), smelt_timer_arg_1.clone())"),
        "{source}"
    );
    assert!(source.contains("smelt_set_timeout("), "{source}");
}

#[test]
fn set_interval_typed_extra_args_use_wrapper_closure() {
    // `setInterval` shares the typed-wrapper path with `setTimeout`.
    let source = source_for(
        r#"
function tick(label: string, n: number): void {
  console.log(label);
}
setInterval(tick, 5, "tock", 2);
"#,
    );

    assert!(
        !source.contains("smelt_timer_args"),
        "typed interval extras must not pack into an erased Vec<SmeltUnknown>: {source}"
    );
    assert!(source.contains("smelt_timer_arg_0"), "{source}");
    assert!(source.contains("smelt_timer_arg_1"), "{source}");
    assert!(
        source.contains("(smelt_timer_callback)(smelt_timer_arg_0.clone(), smelt_timer_arg_1.clone())"),
        "{source}"
    );
    assert!(source.contains("smelt_set_interval("), "{source}");
}

#[test]
fn timer_optional_typed_extra_arg_uses_wrapper_closure() {
    // A single concretely typed extra (matching an optional callback parameter)
    // still routes through the typed wrapper and forwards exactly one argument.
    let source = source_for(
        r#"
function note(prefix: string, suffix?: string): void {
  console.log(prefix);
}
setTimeout(note, 10, "with-optional", "extra");
"#,
    );

    assert!(
        !source.contains("smelt_timer_args"),
        "typed optional timer extras must not pack into an erased Vec<SmeltUnknown>: {source}"
    );
    assert!(source.contains("smelt_timer_arg_0"), "{source}");
    assert!(source.contains("smelt_timer_arg_1"), "{source}");
}

#[test]
fn timer_untyped_extra_arg_keeps_erased_list_path() {
    // An `unknown`-typed extra is a genuine dynamic boundary, so the erased
    // `Vec<SmeltUnknown>` dispatch path must be preserved for it.
    let source = source_for(
        r"
function handle(value: unknown): void {}
function fire(payload: unknown): void {
  setTimeout(handle, 10, payload);
}
",
    );

    assert!(
        source.contains("smelt_timer_args"),
        "untyped timer extras must keep the erased Vec<SmeltUnknown> dispatch path: {source}"
    );
    assert!(
        !source.contains("smelt_timer_arg_0"),
        "untyped path must not synthesize per-argument typed bindings: {source}"
    );
}

#[test]
fn structural_in_guard_projects_to_concrete_union_discriminant() {
    // Issue #55: a `"field" in value` guard over a concrete class union must
    // lower to a tagged-enum discriminant check, not an erased runtime object
    // lookup. Inside the true branch the value projects into the matching arm.
    let source = source_for(
        r#"
class Circle { radius: number = 1; }
class Square { side: number = 2; }
function describe(shape: Circle | Square): number {
  if ("radius" in shape) {
    return shape.radius;
  }
  return 0;
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    // The `in` check is a concrete discriminant test, never a SmeltUnknown map.
    assert!(
        source.contains("matches!(shape.clone(), SmeltUnion"),
        "structural `in` should emit a concrete tag check: {source}"
    );
    assert!(
        !source.contains("SmeltUnknown::Object(values) => values.contains_key"),
        "structural `in` on a concrete union must not erase to an object lookup: {source}"
    );
    // The narrowed read projects into the concrete arm.
    assert!(
        source.contains("union guard selected an excluded member"),
        "{source}"
    );
}

#[test]
fn property_equality_after_in_guard_projects_concrete_arm() {
    // Issue #55: property-equality discriminant comparison works once the value
    // has been narrowed to a concrete arm. The `in` guard narrows `shape` to
    // `Circle`, then `shape.tag === "c"` reads the concrete arm's field.
    let source = source_for(
        r#"
class Circle { tag: string = "c"; radius: number = 1; }
class Square { side: number = 2; }
function describe(shape: Circle | Square): number {
  if ("tag" in shape && shape.tag === "c") {
    return shape.radius;
  }
  return 0;
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(
        source.contains("matches!(shape.clone(), SmeltUnion"),
        "{source}"
    );
    // The discriminant field comparison reads the projected concrete arm.
    assert!(
        source.contains(".tag.clone() == \"c\".to_owned()"),
        "narrowed discriminant comparison should read the concrete field: {source}"
    );
}

#[test]
fn timer_unknown_callback_param_keeps_erased_list_path() {
    // A concretely typed extra whose callback parameter is `unknown` is still a
    // dynamic boundary: forwarding the `String` directly would drop it into a
    // `SmeltUnknown` parameter slot without the boundary conversion. The wrapper
    // path must be declined so the erased `Vec<SmeltUnknown>` ABI (which boxes
    // each argument) is used instead.
    let source = source_for(
        r#"
function handle(value: unknown): void {}
setTimeout(handle, 10, "x");
"#,
    );

    assert!(
        source.contains("smelt_timer_args"),
        "unknown callback parameters must keep the erased Vec<SmeltUnknown> dispatch path: {source}"
    );
    assert!(
        !source.contains("smelt_timer_arg_0"),
        "unknown callback parameter path must not synthesize typed per-argument bindings: {source}"
    );
}

#[test]
fn reassigning_narrowed_union_local_stays_within_narrowed_arm() {
    // Issue #55 invalidation rule: writing a value that still inhabits the
    // narrowed arm refines the narrowing rather than dropping it, so the write
    // re-injects the concrete union variant and later reads still project it.
    let source = source_for(
        r#"
function resolve(path: string | (() => string)): string {
  if (typeof path === "string") {
    path = path + "x";
    return path;
  }
  return path();
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    // The assignment re-injects the concrete arm, proving the fact survived the
    // widening-compatible write.
    assert!(
        source.contains("path = SmeltUnion"),
        "assignment of a narrowed-compatible value should re-inject the arm: {source}"
    );
    assert!(
        source.contains("union guard selected an excluded member"),
        "{source}"
    );
}

#[test]
fn erases_concrete_union_operand_before_truthiness_extraction() {
    // A destructuring default over a `boolean | number` field lowers `flag` to a
    // concrete `SmeltUnion…` enum. Using it in a boolean position must first
    // project the tagged enum back to its erased value (`into_smelt_unknown()`)
    // because the truthiness `match` operates over `SmeltUnknown` discriminants.
    // Regression for the concrete-union boundary: without the erase the emitted
    // `match flag.clone() { SmeltUnknown::… }` would not type-check against the
    // `SmeltUnion…` storage.
    let source = source_for(
        r"
function pick(opts: { flag?: boolean | number }): boolean {
  const { flag = false } = opts;
  return !flag;
}
",
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(
        source.contains("let flag: SmeltUnion"),
        "flag should keep its concrete-union storage: {source}"
    );
    assert!(
        source.contains("match flag.into_smelt_unknown()"),
        "a concrete-union operand must be erased before the truthiness match: {source}"
    );
}

#[test]
fn injects_concrete_union_at_nullish_default_sink() {
    // An erased object-field read defaulted with `??` flows into a concrete
    // `boolean | number` sink. The coalesced `SmeltUnknown` value must be
    // reconstructed into the tagged union (`SmeltUnion…::from_smelt_unknown`)
    // rather than left erased. Regression for the concrete-union boundary: the
    // sink stores `SmeltUnion…`, so leaving the value as `SmeltUnknown` fails to
    // type-check.
    let source = source_for(
        r#"
function opt(record: Record<string, unknown>): boolean | number {
  const value: boolean | number = (record["k"] as boolean | number) ?? false;
  return value;
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(
        source.contains("::from_smelt_unknown("),
        "an erased value flowing into a concrete-union sink must be reconstructed: {source}"
    );
}

#[test]
fn lowers_function_expression_array_callback() {
    // A `function (...) { ... }` expression callback (issue #86) must lower into
    // the array method's callback closure just like the equivalent arrow, rather
    // than being rejected with "array callback methods currently require arrow
    // function callbacks". `source_for` panics on any blocker, so reaching
    // codegen proves the non-arrow form was accepted.
    let source = source_for(
        r"
function increment(values: number[]): number[] {
  return values.map(function (value) {
    return value + 1;
  });
}
",
    );

    assert!(source.contains(".map("), "{source}");
}

#[test]
fn falls_back_to_closure_body_for_function_expression_callback() {
    // A `function`-expression callback whose body uses a statement form the
    // compact callback IR cannot represent (here `try`/`catch`) must retry
    // through full closure-body lowering, exactly as the arrow form does. Before
    // issue #86 the closure-body fallback was gated on the argument being an
    // arrow, so this rejected the file.
    let source = source_for(
        r"
function run(values: string[]): Array<string | undefined> {
  return values.map(function (value) {
    try {
      return value;
    } catch (error) {
      return undefined;
    }
  });
}
",
    );

    assert!(source.contains(".map("), "{source}");
}

#[test]
fn lowers_named_function_item_array_callback() {
    // A bare identifier naming a module-level function item (`values.map(square)`)
    // must lower into the callback closure by calling the function by name, with
    // its typed signature preserved (issue #86).
    let source = source_for(
        r"
function square(value: number): number {
  return value * value;
}

function squares(values: number[]): number[] {
  return values.map(square);
}
",
    );

    assert!(source.contains(".map("), "{source}");
    assert!(source.contains("square"), "{source}");
}

#[test]
fn lowers_local_function_variable_array_callback() {
    // A local/parameter binding whose static type is a `Type::Function`, handed
    // to an array method by name (`values.map(transform)`), must lower into the
    // callback closure by calling the captured local (issue #86).
    let source = source_for(
        r"
function scale(values: number[]): number[] {
  const transform = (value: number): number => value * 3;
  return values.map(transform);
}
",
    );

    assert!(source.contains(".map("), "{source}");
}

#[test]
fn lowers_asserted_identifier_array_callback() {
    // A named callback wrapped in an erased TypeScript assertion
    // (`values.map(square as (value: number) => number)`) must resolve through
    // the same named-reference path as the unwrapped form, so the assertion is
    // transparent instead of hitting the arrow-only gate (issue #86).
    let source = source_for(
        r"
function square(value: number): number {
  return value * value;
}

function squares(values: number[]): number[] {
  return values.map(square as (value: number) => number);
}
",
    );

    assert!(source.contains(".map("), "{source}");
    assert!(source.contains("square"), "{source}");
}

#[test]
fn lowers_optional_class_field_to_option_with_explicit_construction() {
    // An optional TypeScript class field (`y?: number`) must lower to a concrete
    // `Option<f64>` struct slot. Construction that supplies the field passes
    // `Some(..)`; construction that omits it passes `None::<f64>`, mirroring how
    // optional interface fields already lower. The named non-optional field
    // (`x: number`) stays concrete `f64` with no `Option` wrapper.
    let source = source_for(
        r"
class Point {
  x: number;
  y?: number;
  constructor(x: number, y?: number) {
    this.x = x;
    this.y = y;
  }
  total(): number {
    return this.x + (this.y ?? 0);
  }
}

function make(): number {
  const a = new Point(1, 2);
  const b = new Point(3);
  return a.total() + b.total();
}
",
    );

    assert!(source.contains("struct Point"), "{source}");
    assert!(source.contains("x: f64,"), "{source}");
    assert!(source.contains("y: Option<f64>,"), "{source}");
    // Construction with the field present wraps the value in `Some(..)`.
    assert!(source.contains("Point::new(1.0, Some(2.0))"), "{source}");
    // Construction that omits the trailing optional field supplies a typed `None`.
    assert!(source.contains("Point::new(3.0, None::<f64>)"), "{source}");
    // Reading the field through `??` consumes the `Option<f64>` directly.
    assert!(source.contains("unwrap_or(0.0)"), "{source}");
}

#[test]
fn lowers_optional_dataclass_field_to_option_with_explicit_construction() {
    // A Python dataclass field annotated `Optional[int]` (with a `None` default)
    // must lower to a concrete `Option<i64>` struct slot, and construction that
    // omits the field must synthesize the typed `None` default while a present
    // argument is wrapped as `Some(..)`. The required `int` field stays concrete.
    let source = source_for_py(
        r"
from dataclasses import dataclass
from typing import Optional


@dataclass
class Point:
    x: int
    y: Optional[int] = None


def make() -> Optional[int]:
    a = Point(1, 2)
    b = Point(3)
    return a.y
",
    );

    assert!(source.contains("struct Point"), "{source}");
    assert!(source.contains("x: i64,"), "{source}");
    assert!(source.contains("y: Option<i64>,"), "{source}");
    assert!(source.contains("Point::new(1, Some(2))"), "{source}");
    assert!(source.contains("Point::new(3, None::<i64>)"), "{source}");
    // Reading the optional field yields the `Option<i64>` value unchanged.
    assert!(source.contains("a.y.clone()"), "{source}");
}

/// Issue #98: a TypeScript `static` method lowers to a receiver-free associated
/// function and a qualified static call resolves to `Class::method(..)`.
#[test]
fn emits_typescript_static_method_as_associated_function() {
    let source = source_for(
        r"
class MathUtils {
  static square(value: number): number {
    return value * value;
  }
}
export function area(radius: number): number {
  return MathUtils.square(radius);
}
",
    );

    // The static method is emitted inside the class impl with no `self`.
    assert!(source.contains("fn square(value: f64) -> f64"), "{source}");
    assert!(
        !source.contains("fn square(&self") && !source.contains("fn square(&mut self"),
        "static method must not take a receiver: {source}"
    );
    // The qualified call resolves to the associated function.
    assert!(source.contains("MathUtils::square("), "{source}");
}

/// Issue #98: a TypeScript `static` class constant lowers to a materialized
/// static field and a qualified read resolves to its concrete literal value.
#[test]
fn emits_typescript_static_const_read() {
    let source = source_for(
        r#"
class Config {
  static readonly LIMIT: number = 42;
  static readonly NAME: string = "smelt";
}
export function limit(): number {
  return Config.LIMIT;
}
export function label(): string {
  return Config.NAME;
}
"#,
    );

    // Static constants become receiver-free associated accessors.
    assert!(
        source.contains("fn __smelt_static_limit() -> f64"),
        "{source}"
    );
    assert!(
        source.contains("fn __smelt_static_name() -> String"),
        "{source}"
    );
    // Qualified reads resolve to the concrete literal values.
    assert!(source.contains("return 42"), "{source}");
    assert!(source.contains(r#""smelt".to_owned()"#), "{source}");
}

/// Issue #98: a Python `@staticmethod` lowers to a receiver-free associated
/// function and a class-level variable lowers to a static field read.
#[test]
fn emits_python_static_method_and_class_var() {
    let source = source_for_py(
        r"
class MathUtils:
    PI = 3

    @staticmethod
    def square(value: float) -> float:
        return value * value

def area(radius: float) -> float:
    return MathUtils.square(radius) * MathUtils.PI
",
    );

    assert!(source.contains("fn square(value: f64) -> f64"), "{source}");
    assert!(
        !source.contains("fn square(&self") && !source.contains("fn square(&mut self"),
        "static method must not take a receiver: {source}"
    );
    assert!(source.contains("MathUtils::square("), "{source}");
    // The class variable `PI = 3` reads back as its concrete integer literal.
    assert!(source.contains("fn __smelt_static_PI"), "{source}");
    assert!(source.contains(" 3"), "{source}");
}

/// Issue #113: a named `reduce` callback whose declared return type is not
/// identical to the initial value's type but statically reconciles with it must
/// not be rejected with "array reduce callback returns an unsupported type". The
/// accumulator widens to the reconciled type (`string | number`), the initial
/// `0` is coerced into that concrete union, and the emitted `fold` invokes the
/// callback with only the two arguments it declares (not the four the runtime
/// supplies) so the closure call type-checks.
#[test]
fn reduce_named_callback_reconciles_union_return_type() {
    let source = source_for(
        r"
function step(acc: string | number, value: number): string | number {
  return acc;
}
export function run(values: number[]): string | number {
  return values.reduce(step, 0);
}
",
    );
    assert!(source.contains(".iter().enumerate().fold("), "{source}");
    // The named callback declares two parameters, so the emitted fold calls it
    // with exactly two arguments rather than the runtime's `(acc, item, index,
    // array)` four-argument shape.
    assert!(
        source.contains("(smelt_callback)(acc, item)"),
        "reduce should call the two-parameter callback with two args: {source}"
    );
    assert!(source.contains("fn run("), "{source}");
}

/// Issue #113: a named `reduce` callback whose declared accumulator parameter is
/// wider than both the seed and the callback return type resolves the reduce
/// result to that declared accumulator type (TypeScript's `U`). The callback
/// returns a `number` that must be coerced back into the `string | number`
/// accumulator on each fold step, so the emitted fold both seeds and folds
/// through the concrete union.
#[test]
fn reduce_named_callback_uses_declared_accumulator_type() {
    let source = source_for(
        r#"
function step(acc: string | number, value: number): number {
  return value;
}
export function run(values: number[]): string | number {
  return values.reduce(step, "seed");
}
"#,
    );
    assert!(source.contains(".iter().enumerate().fold("), "{source}");
    assert!(source.contains("(smelt_callback)(acc, item)"), "{source}");
    assert!(source.contains("fn run("), "{source}");
}

/// Issue #113: a named `reduce` callback that widens a concrete seed into an
/// erased `unknown` accumulator (a genuine dynamic boundary) still lowers; the
/// accumulator is the callback's `unknown` return type and the numeric seed is
/// coerced into it.
#[test]
fn reduce_named_callback_widens_to_unknown_accumulator() {
    let source = source_for(
        r"
function step(acc: number, value: number): unknown {
  return acc + value;
}
export function run(values: number[]): unknown {
  return values.reduce(step, 0);
}
",
    );
    assert!(source.contains(".iter().enumerate().fold("), "{source}");
    assert!(source.contains("fn run("), "{source}");
}

/// Issue #113: a genuinely irreconcilable named `reduce` callback — a `string`
/// accumulator with a `boolean` return type that shares no common concrete
/// shape — is still rejected, so the reconciliation does not silently widen
/// unrelated types through a `SmeltUnknown` shortcut.
#[test]
fn reduce_named_callback_rejects_irreconcilable_return_type() {
    let mut ctx = HirCtx::new();
    let result = to_hir(
        r#"
function step(acc: string, value: number): boolean {
  return true;
}
export function run(values: number[]): string {
  return values.reduce(step, "seed");
}
"#,
        FileId(0),
        &mut ctx,
    );
    let errors = result.expect_err("irreconcilable reduce callback must be rejected");
    assert!(
        errors
            .iter()
            .any(|error| error.message.contains("callback returns an unsupported type")),
        "expected the reduce return-type rejection, got: {errors:?}"
    );
}

/// A module const whose initializer is an array spread (es-toolkit's
/// `arrayViews = [...typedArrays, 'DataView']` shape) inlines into a function
/// body as a concrete `SmeltList<String>` concat chain. The homogeneous
/// literal must keep its `String` item type — the previous blanket
/// `List<Unknown>` item type made the concat operands disagree and the
/// emitter silently produced an empty `SmeltList::default()` value.
#[test]
fn inlined_spread_const_emits_concrete_string_list_concat() {
    let source = source_for(
        r#"
const typedNames = ["Float32Array", "Int8Array", "Uint8Array"];
const viewNames = [...typedNames, "DataView"];
export function describeViews(): string {
  const names = viewNames;
  return names.join(",");
}
"#,
    );
    assert!(source.contains("fn describe_views()"), "{source}");
    assert!(
        source.contains("\"DataView\".to_owned()"),
        "expected the spread tail literal to survive codegen: {source}"
    );
    assert!(
        source.contains("let names: SmeltList<String>"),
        "expected the inlined spread const to stay a concrete string list: {source}"
    );
    assert!(
        !source.contains("SmeltList::default()"),
        "expected no silently-defaulted list value in the concat chain: {source}"
    );
}

/// A module const whose initializer is a concat/slice method chain
/// (es-toolkit's `empties = [[], {}].concat(falsey.slice(1))` shape) inlines
/// into a function body and emits the full chain instead of erroring as an
/// unsupported const item expression shape.
#[test]
fn inlined_method_chain_const_emits_concat_and_slice() {
    let source = source_for(
        r#"
const smallNumbers = [0, 1].concat([2, 3, 4].slice(1));
export function numberCount(): number {
  const values = smallNumbers;
  return values.length;
}
"#,
    );
    assert!(source.contains("fn number_count()"), "{source}");
    assert!(
        source.contains(".chain("),
        "expected the inlined concat chain to emit list concatenation: {source}"
    );
    assert!(
        source.contains(".skip("),
        "expected the inlined slice argument to emit a skip-based slice: {source}"
    );
}

/// A zero-parameter named callback (`values.map(stubTrue)`) is called with no
/// arguments at all — JavaScript ignores the supplied `(value, index, array)`
/// triple when the callback declares no parameters.
#[test]
fn zero_parameter_named_map_callback_calls_with_no_arguments() {
    let source = source_for(
        r#"
function stubTrue(): boolean {
  return true;
}
export function run(values: number[]): boolean[] {
  return values.map(stubTrue);
}
"#,
    );
    assert!(source.contains("(smelt_callback)()"), "{source}");
    assert!(source.contains("fn run("), "{source}");
}

/// A zero-parameter named predicate emits a real filter over the receiver
/// instead of the former `Default::default()` placeholder.
#[test]
fn zero_parameter_named_filter_callback_emits_real_iteration() {
    let source = source_for(
        r#"
function stubFalse(): boolean {
  return false;
}
export function run(values: number[]): number[] {
  return values.filter(stubFalse);
}
"#,
    );
    assert!(source.contains("stub_false()"), "{source}");
    assert!(source.contains(".iter().enumerate().filter_map("), "{source}");
}

/// A named callback declaring more parameters than the receiver supplies is
/// wrapped at the supplied arity; the item call pads the unsupplied optional
/// tail with its default (`None`), matching the JavaScript `undefined` tail.
#[test]
fn over_arity_named_map_callback_pads_optional_tail_with_none() {
    let source = source_for(
        r#"
function withTail(value: number, index?: number, list?: number[], guard?: number): number {
  return value + (guard ?? 0);
}
export function run(values: number[]): number[] {
  return values.map(withTail);
}
"#,
    );
    assert!(
        source.contains("with_tail(closure_arg_0.clone(), closure_arg_1.clone(), closure_arg_2.clone(), None::<f64>)"),
        "{source}"
    );
}


#[test]
fn emits_string_coercion_default_sort_for_union_elements() {
    // A comparator-less `sort()` on a union-element list follows JavaScript's
    // default ordering: elements compare by their `ToString` coercion. The
    // concrete union projects through `into_smelt_unknown` before the coercion
    // match, and the sort itself is a stable `sort_by` over the coerced keys.
    let source = source_for(
        r#"
function sortMixed(values: Array<string | number>): Array<string | number> {
  return values.sort();
}
"#,
    );

    assert!(
        source.contains(".sort_by(|left, right| (match left.clone().into_smelt_unknown()"),
        "union default sort should compare erased string coercions\n{source}"
    );
    assert!(
        source.contains("SmeltUnknown::Number(value) => value.to_string()"),
        "the coercion match should stringify numeric elements\n{source}"
    );
}

#[test]
fn emits_member_store_for_compound_callback_assignment() {
    // `row[0] += suffix` inside a `.map` callback is a member-target compound
    // store; the compact callback IR cannot represent it, so the arrow retries
    // through full closure-body lowering, which emits a real indexed store
    // instead of silently dropping the mutation (or rejecting the file with
    // "callback assignment targets must be captured locals").
    let source = source_for(
        r#"
function tag(rows: string[][], suffix: string): string[][] {
  return rows.map(row => {
    row[0] += suffix;
    return row;
  });
}
"#,
    );

    assert!(
        source.contains("closure_arg_0[smelt_assign_index] ="),
        "the compound member assignment should emit an indexed store into the row\n{source}"
    );
    assert!(
        !source.contains("callback assignment targets must be captured locals"),
        "the member-assignment blocker must be gone\n{source}"
    );
}

#[test]
fn optional_unknown_truthiness_covers_undefined() {
    // An optional `unknown` used in a boolean position coerces through
    // `optional_truthy_text`. The generated `match` must treat both
    // `Some(Null)` and `Some(Undefined)` as falsy; omitting the `Undefined`
    // arm produced a non-exhaustive match (E0004) because `SmeltUnknown` has a
    // dedicated `Undefined` variant.
    let source = source_for(
        r#"
export function firstTruthy(guard?: unknown): boolean {
  if (guard) {
    return true;
  }
  return false;
}
"#,
    );

    assert!(
        source.contains("Some(SmeltUnknown::Null) | Some(SmeltUnknown::Undefined) => false"),
        "optional-unknown truthiness must cover the Undefined variant\n{source}"
    );
}

#[test]
fn callback_parameter_adapter_reborrows_immutably() {
    // Forwarding a borrowed callback parameter to a helper that expects a
    // different callback arity builds a wrapper closure. The wrapper reborrows
    // the parameter, which is bound as an immutable `&dyn Fn`; a `&mut *`
    // reborrow through that shared reference fails to compile (E0596), so the
    // adapter must reborrow immutably with `&*`.
    let source = source_for(
        r#"
function uniqBy<T>(arr: T[], mapper: (item: T, index: number, arr: T[]) => unknown): T[] {
  return arr;
}
export function unionBy<T>(arr1: T[], arr2: T[], mapper: (item: T) => unknown): T[] {
  return uniqBy([...arr1, ...arr2], mapper);
}
"#,
    );

    assert!(
        source.contains("&*mapper"),
        "the callback adapter should reborrow the parameter immutably\n{source}"
    );
    assert!(
        !source.contains("&mut *mapper"),
        "the callback adapter must not mutably reborrow an immutable `&dyn Fn`\n{source}"
    );
}

#[test]
fn tuple_length_emits_constant_arity() {
    // A fixed-arity tuple has no Rust `.len()` method (E0599). Its JavaScript
    // `.length` is a compile-time constant, so the length rvalue must emit the
    // arity literal rather than a method call on the tuple.
    let source = source_for(
        r#"
export function pairLength(pair: [string, number]): number {
  return pair.length;
}
"#,
    );

    assert!(
        source.contains("2 as f64"),
        "a tuple's `.length` should emit its constant arity\n{source}"
    );
    assert!(
        !source.contains("pair.len()"),
        "a tuple must not call the list `.len()` method\n{source}"
    );
}
