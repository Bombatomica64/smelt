//! Split codegen tests chunk.

use super::*;

/// Erasing an async callback into an unknown-rest function must wrap its
/// returned future directly. Throws from an async body live inside that future;
/// the callback invocation itself does not return a `Result` to unwrap.
#[test]
fn erased_async_throwing_callback_does_not_unwrap_future_as_result() {
    let input = r#"
export const tryit = <Args extends unknown[], Return>(
  func: (...args: Args) => Return
) => {
  return (...args: Args): unknown => func(...args)
}

export function run(): unknown {
  return tryit(async () => {
    throw new Error("failure")
  })
}
"#;
    let source = source_for(input);

    assert!(source.contains("SmeltUnknown::Promise(SmeltPromise::from_future"), "{source}");
    assert!(
        !source.contains("(smelt_callback)(smelt_args).unwrap_or_else")
            && !source.contains("(smelt_callback)().unwrap_or_else"),
        "{source}"
    );
}

/// Erasing a typed promise value (`SmeltFuture<T>`) to the dynamic carrier goes
/// through `IntoSmeltUnknown`, so the erased-callback promise adapter and the
/// recover-erased-promise-on-await coercion both emit `.into_smelt_unknown()` on
/// a future. The prelude must therefore always define
/// `impl<..> IntoSmeltUnknown for SmeltFuture<T>` for any program that references
/// the future runtime; otherwise the generated call is an E0599 (regression seen
/// in the radash suite, where the impl was absent while the call was emitted).
#[test]
fn future_erasure_emits_into_smelt_unknown_impl() {
    let source = source_for(
        r"
export async function makeValue(): Promise<number> {
  return 1
}

export async function useErased(func: (...args: unknown[]) => unknown): Promise<unknown> {
  return func()
}
",
    );

    assert!(
        source.contains(
            "impl<T: IntoSmeltUnknown + Clone + 'static> IntoSmeltUnknown for SmeltFuture<T>"
        ),
        "future runtime must emit the SmeltFuture IntoSmeltUnknown impl: {source}"
    );
}

/// An async closure whose loop exits only through explicit returns leaves the
/// trailing async-value expression unreachable. Its binding still needs the
/// resolved output type so Rust does not have to infer a value from `!`.
#[test]
fn async_closure_with_returning_loop_annotates_unreachable_tail() {
    let source = source_for(
        r"
export function worker(done: (values: unknown[]) => void): () => Promise<void> {
  return async () => {
    const values: unknown[] = []
    while (true) {
      if (values.length === 0) return done(values)
    }
  }
}
",
    );

    assert!(
        source.contains("let smelt_async_value: () = {")
            || source.contains("let smelt_async_value: SmeltUnknown = {"),
        "{source}"
    );
    assert!(!source.contains("let smelt_async_value = {"), "{source}");
}

/// A dotted write on a record widened by a symbol key must insert through the
/// dictionary API; a `.get(..)` read expression is never an assignable place.
#[test]
fn dotted_write_to_unknown_key_record_inserts_string_key() {
    let source = source_for(
        r"
export function cycle(): unknown {
  const symbolKey = Symbol('key')
  const complex = { loop: null, [symbolKey]: 'symbol' }
  complex.loop = complex
  return complex
}
",
    );

    assert!(
        source.contains(".insert(SmeltUnknown::String(\"loop\".into()),"),
        "{source}"
    );
    assert!(!source.contains("expect(\"missing field\") ="), "{source}");
}

/// A `return <literal>` statement inside an `async` function returns the
/// *resolved* value, not a promise: the async lowering wraps the whole body
/// into the future. When the declared return type is `Promise<[null, T]>` the
/// returned tuple/array literal must lower to the erased value directly, never
/// be coerced into a `SmeltPromise::from_future(..)` around a non-future value
/// (which produced `let _tmp: Pin<Box<dyn Future<..>>> = vec![..];`, E0308).
#[test]
fn async_return_of_tuple_literal_lowers_to_value_not_promise_wrapper() {
    let source = source_for(
        r"
export async function attemptAsync<T, E>(func: () => Promise<T>): Promise<[null, T] | [E, null]> {
  try {
    const result = await func();
    return [null, result];
  } catch (error) {
    return [error as E, null];
  }
}
",
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
        r"
export async function firstEven(values: number[]): Promise<number> {
  for (const value of values) {
    if (value % 2 === 0) {
      return value;
    }
  }
  return -1;
}
",
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

    // The class struct keeps deriving the standard traits (plus `PartialEq`,
    // which the union field's hand-written `PartialEq` satisfies).
    assert!(
        source.contains("#[derive(Clone, Debug, Default, PartialEq)]"),
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
        GeneratedAllocator::System,
        ReleaseProfile::Optimized,
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
        GeneratedAllocator::System,
        ReleaseProfile::Optimized,
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
        source.contains("to_iso_string()") && source.contains("SmeltUnknown::String((_smelt_tmp_"),
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

    // An array's own-key view is its element indices followed by the named
    // properties in its side table, which is exactly `own_entries`.
    assert!(
        source.contains("SmeltUnknown::Array(values) => values.own_entries().into_iter().collect()"),
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

/// Erasing a record to a JavaScript object must carry the record's key order
/// across, not re-derive it.
///
/// `SmeltObject` has always CARRIED an `order` vector — `iter`/`keys`/`values`
/// and the serde impl all read it — but its constructors used to take an
/// unordered `HashMap` and recover an order by SORTING the keys. Every erasure
/// site fed them `record.iter().collect()`, so the ordered entry stream the
/// record had just produced was dropped into a hash map and then alphabetised:
/// `{ foo: 1, bar: 2, baz: 3 }` erased to an object whose `Object.keys` read
/// `["bar", "baz", "foo"]`. The constructors now take the ordered entry
/// sequence, which is the only form that can express a JavaScript object's key
/// order at all.
#[test]
fn erasing_a_record_to_an_object_keeps_the_source_key_order() {
    let source = source_for(
        r"
const plain = { foo: 1, bar: 2, baz: 3 };
const erased: unknown = plain;
",
    );

    assert!(
        source.contains("fn new(entries: Vec<(String, SmeltUnknown)>) -> Self"),
        "{source}"
    );
    assert!(
        source.contains("fn with_id(id: usize, entries: Vec<(String, SmeltUnknown)>) -> Self"),
        "{source}"
    );
    // No constructor may guess an order back out of an unordered map.
    assert!(
        !source.contains("let mut order = values.keys().cloned().collect::<Vec<_>>(); order.sort();"),
        "{source}"
    );
}

/// JavaScript own-key order is not plain insertion order: array-index keys come
/// first in ascending numeric order, then the remaining string keys in insertion
/// order (`OrdinaryOwnPropertyKeys`). Both erased containers must therefore place
/// a newly inserted key rather than push it, so `{ b: 1, 2: "x", a: 3, 1: "y" }`
/// enumerates as `1, 2, b, a`. The ordering is maintained at insert time so
/// `keys()` stays a plain read of one ordered structure.
#[test]
fn object_and_record_inserts_follow_javascript_own_key_order() {
    let source = source_for(
        r#"
const mixed = { b: 1, 2: "x", a: 3, 1: "y" };
const erased: unknown = mixed;
"#,
    );

    assert!(
        source.contains("fn smelt_canonical_array_index(key: &str) -> Option<u32>"),
        "{source}"
    );
    assert!(
        source.contains(
            "fn smelt_js_key_order_position<K: SmeltPropertyKey, V>(entries: &[SmeltFieldEntry<K, V>], key: &K) -> usize"
        ),
        "{source}"
    );
    // Both containers share ONE ordered store, so the placement happens once,
    // in `SmeltFieldStore::insert`; neither appends unconditionally.
    assert_eq!(
        source
            .matches("let position = smelt_js_key_order_position(&self.entries, &key);")
            .count(),
        1,
        "{source}"
    );
    assert!(
        source.contains("self.entries.insert(position, SmeltFieldEntry { fingerprint, key, value });"),
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
            "SmeltArray::with_id(smelt_id, smelt_values.into_iter().map(|value| SmeltUnknown::Array"
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
        source.contains(
            "if iterator.is_empty() { None } else { Some(iterator.borrow_mut().remove(0)) }"
        ),
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
    // `stack` joins the hidden error keys: `Object.keys(err)` must not list it,
    // because it is a non-enumerable own property in JavaScript. es-toolkit
    // `clone` assigns `newError.stack = obj.stack`, which otherwise gave the clone
    // an own `stack` key the original lacked and broke `toEqual`.
    assert!(
        source.contains("object.contains_key(\"__smelt_error\")")
            && source.contains(
                "matches!(key, \"__smelt_error\" | \"message\" | \"cause\" | \"errors\" | \"stack\")"
            ),
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
    assert!(source.contains(".extend("), "{source}");
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
    // `new Uint8Array(count)` allocates `count` *elements*. This used to emit a
    // `vec![0.0; count]` numeric list, which is why every view reported tag
    // `[object Array]` and a byte count for its `length`; the views are now
    // byte-backed host objects, so the runtime length allocation moved into the
    // one shared byte-buffer constructor (which multiplies by the element's
    // `BYTES_PER_ELEMENT`). The assertions below track the same property — a
    // *runtime* count, never an unrolled literal — at its new home.
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

    assert!(
        source.contains(
            "smelt_reflected_construct(\"uint8array\", vec![SmeltUnknown::Number(count as f64)])"
        ),
        "{source}"
    );
    assert!(!source.contains("vec![0.0, 0.0, 0.0, 0.0"), "{source}");
    // The allocation is by element count times the element stride, so a wider
    // view over the same count is wider in bytes.
    assert!(
        source.contains("vec![SmeltUnknown::Number(0.0); count * stride]"),
        "{source}"
    );
}

#[test]
fn emits_bigint_typed_array_constructor_through_the_shared_host_constructor() {
    // `BigInt64Array` / `BigUint64Array` were previously omitted from the
    // typed-array recognizer, so `new BigUint64Array(...)` aborted the build as
    // an "unresolved class". They now share the byte-buffer host-object model with
    // the other nine views — an eight-byte element type, so three elements are 24
    // bytes and `.length` still reads 3. (The assertion moved off "emits a `Vec`
    // literal": the element list is now an argument to the shared constructor
    // rather than the constructed value itself.)
    let source = source_for(
        r"
export function make(): number {
  const values = new BigUint64Array([1, 2, 3]);
  return values.length;
}
",
    );

    assert!(
        source.contains("smelt_reflected_construct(\"biguint64array\""),
        "{source}"
    );
    assert!(!source.contains("unresolved"), "{source}");
}

#[test]
fn emits_typed_array_from_element_literal_through_the_element_codec() {
    // `new Uint8Array([1, 2, 3])` passes its element list to the shared byte-buffer
    // constructor, which encodes each element at the view's own width; an indexed
    // read decodes it back. It used to emit a bare `vec![10.0, 20.0, 30.0]` numeric
    // list, which is why the view had no identity, no `.buffer`, and a byte count
    // for its `length`.
    let source = source_for(
        r"
export function first(): number {
  const values = new Uint8Array([10, 20, 30]);
  return values[0];
}
",
    );

    assert!(source.contains("vec![10.0, 20.0, 30.0]"), "{source}");
    assert!(
        source.contains("smelt_reflected_construct(\"uint8array\""),
        "{source}"
    );
    assert!(source.contains("smelt_host_buffer_element("), "{source}");
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
        source.contains("Child::new(Some(SmeltUnknown::String(\"value\".into())))"),
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

    // The erased projection consults the prototype slots after the own key
    // misses, matching `smelt_get_object_field`; see `crate::class_proto`.
    assert!(
        source.contains(
            "Duration { years: smelt_record_map.get(\"years\").or_else(|| smelt_record_map.get(\"__smelt_proto:years\")).or_else(|| smelt_record_map.get(\"__smelt_method:years\")).cloned().map(|value|"
        ),
        "{source}"
    );
    assert!(
        source.contains(
            "months: smelt_record_map.get(\"months\").or_else(|| smelt_record_map.get(\"__smelt_proto:months\")).or_else(|| smelt_record_map.get(\"__smelt_method:months\")).cloned().map(|value|"
        ),
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
    // A replacer callback whose result is not a `String` is not a valid
    // `Replacer`, so the regex replacement must coerce it with the JavaScript
    // `ToString` match. Here the callback returns an `unknown` read out of an
    // erased parameter, the shape that carries no static type at all.
    //
    // This test used to spell the same idea as `htmlEscapes[match]` against a
    // module-level `Record<string, string>` constant. That read is NOT erased —
    // the constant is a concrete string-to-string map — and it only looked
    // erased because a callback could not resolve an object constant at all and
    // silently read it as `null`. Since that defect was fixed the es-toolkit
    // `escape` shape produces a `String` directly and needs no coercion, so the
    // coercion rule is pinned here on a value that is genuinely dynamic.
    let source = source_for(
        r#"
export function replaceAll(str: string, table: unknown): string {
  return str.replace(/[&<>"']/g, (match) => (table as Record<string, unknown>)[match]);
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
fn self_recursive_closure_captures_its_own_cell_weakly() {
    // A self-recursive closure and its cell used to hold each other with `Rc`, a
    // reference cycle that leaked the closure and everything it captured on EVERY
    // call. es-toolkit's `flatten` grew ~21 KiB per call without bound; after this
    // lowering its peak RSS is flat in call count (396 MiB -> 4.6 MiB at ~2k calls).
    // See `benchmarks/FINDINGS.md` finding #2.
    //
    // The absence assertion is the load-bearing one: no strong self-capture means the
    // cycle cannot form, which is a static property rather than a measurement.
    let source = source_for(
        r"
export function collect(rows: number[][]): number[] {
  const result: number[] = [];
  const recurse = (items: number[][], depth: number): void => {
    for (const item of items) {
      if (depth > 0) {
        recurse([item], depth - 1);
      } else {
        result.push(item[0]);
      }
    }
  };
  recurse(rows, 1);
  return result;
}
",
    );

    // The closure captures a `Weak` to its own cell...
    assert!(
        source.contains("let smelt_capture_recurse = ::std::rc::Rc::downgrade(&smelt_capture_recurse);"),
        "{source}"
    );
    // ...and upgrades it to call itself.
    assert!(
        source.contains("smelt_capture_recurse.upgrade().expect("),
        "{source}"
    );
    // The frame keeps the strong owner, so the cell outlives every call through it.
    assert!(
        source.contains("(*smelt_capture_recurse.borrow_mut()) ="),
        "{source}"
    );
    // No strong self-capture: the cycle cannot form.
    assert!(
        !source.contains("let smelt_capture_recurse = smelt_capture_recurse.clone();"),
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
    // What this test is really about: the ESCAPING closure must own its captured
    // `args`, because it outlives the frame. That capture still clones.
    assert!(source.contains("let args = args.clone();"), "{source}");
    // The immediately-evaluated `args.slice(2)` in the other branch is a different
    // expression, and it reads `args` only through `&self` methods, so it borrows.
    // It used to emit `args.clone().iter().skip(` — a whole-`Vec` copy for a read.
    assert!(
        source.contains("args.borrow().iter().skip(")
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
        source.contains("smelt_forwarded_args.extend(arg2.clone().into_iter()"),
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
fn passes_a_mutated_structural_record_as_a_shared_handle() {
    // A record whose field is written after construction is a JavaScript
    // reference value, so it lowers to the reference-record handle newtype (see
    // `classify::reference_classes`) rather than being threaded through Rust's
    // `&mut` ABI. The callee's write goes through the shared cell, so the caller
    // observes it while the value is still passed by value.
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
        source.contains("struct Flags(::std::rc::Rc<::std::cell::RefCell<FlagsInner>>);"),
        "{source}"
    );
    assert!(source.contains("fn set_era(mut flags: Flags, value: f64)"), "{source}");
    assert!(source.contains("flags.0.borrow_mut().era ="), "{source}");
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
const boxed: Boxed<number> = { value: 1 };
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
    assert!(source.contains("SmeltUnknown::String(\"skip\".into())"));
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
    assert!(source.contains("String(::std::rc::Rc<str>),"));
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
    assert!(source.contains("SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string()"));
    assert!(source.contains("matches!(value.clone(), SmeltUnknown::Array(_))"));
}

#[test]
fn preserves_runtime_value_through_never_assertion() {
    let source = source_for(
        r"
function consume(value: unknown): boolean {
  return value === null;
}
const result = consume(null as unknown as never);
const tuple = [1] as unknown as [never];
const record = {} as Record<string, never>;
",
    );

    assert!(
        source.contains(": SmeltUnknown = SmeltUnknown::Null;")
            && source.contains("consume(_smelt_tmp_")
            && source.contains("SmeltUnknown::Array(vec![SmeltUnknown::Number(1.0 as f64)]")
            && source.contains("SmeltRecord::from([])"),
        "erased direct and compound assertions must retain their runtime values: {source}"
    );
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
            .contains("value.clone().js_strict_eq(&SmeltUnknown::String(\"trailing\".into()))"),
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
        source.contains("SmeltUnknown::String(\"\".into())"),
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
        source.contains("SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string()"),
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
            "smelt_l.into_iter().map(|value| SmeltUnknown::String(value.into())).collect::<Vec<_>>()"
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
        source.contains(".map(|value| SmeltUnknown::String(value.into())).collect::<Vec<_>>()"),
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
    // `out` is a source `Map` (`SmeltJsMap`), so `Object.fromEntries(out)` is a
    // genuine Map->Record conversion (rebuild the entries into the declared
    // `Record<string, number>` backing), not an identity `return out;`. A `JsMap`
    // forces the unknown carrier on, so the string-keyed record target backs onto
    // the identity-bearing `SmeltRecord`.
    assert!(
        source.contains(".collect::<SmeltRecord<String, f64>>()"),
        "{source}"
    );
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
const value: unknown = 1;
const fallback: Date = new Date();
const selected: unknown = value ? value : fallback;
",
    );

    assert!(
        // The temp index is incidental (it moves with the fixture's own
        // statement count); the property is that the truthy arm yields the
        // erased value UNCHANGED and only the `Date` fallback is re-tagged.
        source.contains("{ value } else { match fallback"),
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
        source.contains("let _smelt_tmp_1: SmeltUnknown = SmeltUnknown::Object"),
        "{source}"
    );
    assert!(
        !source.contains("let _smelt_tmp_1: SmeltUnknown = _smelt_tmp_0.clone();"),
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
        source.contains("(right as f64).trunc() as i64"),
        "{source}"
    );
    // The int-from-float coercion keeps only the parentheses `.trunc()` needs
    // as a method receiver; the whole cast is not wrapped again, which drew a
    // spurious `unused_parens` in every value position it landed in.
    assert!(
        !source.contains("((right.clone() as f64).trunc() as i64)"),
        "int-from-float coercion should not re-wrap the whole cast: {source}"
    );
}

#[test]
fn emits_int_to_float_coercion_without_defensive_parentheses() {
    let source = source_for(
        r"
function widen(values: unknown[]): number {
  return values.length;
}
",
    );

    // `values.length` lowers to `values.len() as f64`; the coercion seam must
    // not wrap the cast in defensive parentheses, which the compiler flags as
    // `unused_parens` wherever the value stands alone. Checked on the emitted
    // FUNCTION BODY: the runtime prelude legitimately parenthesizes the same
    // cast where it is an argument (`SmeltUnknown::Number(values.len() as f64)`).
    let body = emitted_function_body(&source, "fn widen");
    assert!(body.contains("as f64"), "{body}");
    assert!(
        !body.contains("(values.len() as f64)"),
        "int-to-float coercion should not wrap the cast in parentheses: {body}"
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
        source.contains("!({ let smelt_number = amount; smelt_number != 0.0 && !smelt_number.is_nan() })"),
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
            ".map_or(String::new(), |value| match value.clone() { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string()"
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
        source.contains("(fn_)(closure_arg_0, extra)"),
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
        source.contains(".cloned().unwrap_or(SmeltList::new(Vec::<String>::new()))"),
        "{source}"
    );
    assert!(
        source.contains("output.insert(key.clone(), items.clone());"),
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
        source.contains("SmeltUnknown::String(\"cat\".into())"),
        "{source}"
    );
    assert!(
        source.contains("SmeltUnknown::String(\"dog\".into())"),
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
    // The `hasOwn` lowering must inspect the erased value directly, not cast it
    // into a typed record. Scope the check to the emitted `has_length` function
    // body: `SmeltRecord::with_id_from_entries` also legitimately appears in the
    // runtime prelude's `SmeltFromUnknown for SmeltRecord` impl, which is
    // unrelated to this program's behavior.
    let function_body = source
        .split("fn has_length")
        .nth(1)
        .expect("generated source defines has_length");
    assert!(
        !function_body.contains("SmeltRecord::with_id_from_entries"),
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
        source.contains("SmeltUnknown::Object(map) => { map.insert(\"name\".to_owned(), SmeltUnknown::String(\"Grace\".into())); }"),
        "{source}"
    );
    assert!(
        source.contains("*other = SmeltUnknown::Object(SmeltObject::new(Vec::from([(\"name\".to_owned(), SmeltUnknown::String(\"Grace\".into()))])));"),
        "{source}"
    );
}

#[test]
fn tuple_to_tuple_coercion_materializes_nontrivial_source_once() {
    // Regression: element-wise tuple coercion references the source once per
    // field. When the source is a non-trivial expression (an inlined call), it
    // must be materialized into a single temporary so by-value arguments are
    // not moved once per element (E0382).
    let source = source_for(
        r"
function pair(values: number[]): [number[], number[]] {
  return [values, values];
}
function widen(values: number[]): [unknown[], unknown[]] {
  return pair(values);
}
",
    );

    // The coercion must never emit the source call more than once: one
    // definition plus one call site. Duplicating it would re-move `values`
    // (E0382). This holds whether the call is bound to a temp (trivial source,
    // inlined coercion) or inlined into the coercion (non-trivial source,
    // materialized into `smelt_tuple_src`).
    assert_eq!(source.matches("pair(").count(), 2, "{source}");
}

#[test]
fn self_aliasing_unknown_field_assignment_evaluates_value_before_borrow() {
    // Regression: JS self-aliasing `value.self = value` must evaluate the
    // assigned value into a temporary BEFORE taking the receiver's mutable
    // borrow, or the value's read of the receiver conflicts with the `&mut`
    // borrow (E0502).
    let source = source_for(
        r"
function link(value: unknown): unknown {
  value.self = value;
  return value;
}
",
    );

    assert!(source.contains("let smelt_value ="), "{source}");
    assert!(source.contains("match &mut value"), "{source}");
    assert!(
        source.contains("map.insert(\"self\".to_owned(), smelt_value);"),
        "{source}"
    );
}

#[test]
fn self_referential_list_push_evaluates_item_before_borrow() {
    // Regression: `array.push(array)` must evaluate the pushed item into a
    // temporary BEFORE taking the list's mutable borrow, or the item's read of
    // the list conflicts with the `&mut` push (E0502).
    let source = source_for(
        r"
function grow(): number {
  const array: unknown[] = [];
  return array.push(array);
}
",
    );

    assert!(source.contains("let smelt_push_item ="), "{source}");
    assert!(source.contains(".push(smelt_push_item)"), "{source}");
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
        source.contains("None::<::std::rc::Rc<dyn Fn(&SmeltUnknown) -> SmeltUnknown>>"),
        "generic callback defaults should use the instantiated option payload: {source}"
    );
    assert!(
        source.contains(
            "ParseOptions { in_: None::<::std::rc::Rc<dyn Fn(&SmeltUnknown) -> SmeltUnknown>>"
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
        source.contains("user.get(&key.clone()).cloned().unwrap_or(String::new())"),
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
        source.contains("(((raw as f64).trunc() as i128) >>"),
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
        source.contains("(smelt_callback)(&(SmeltUnknown::Null), index as f64)"),
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

    assert!(source.contains("closure_arg_0 as i64"), "{source}");
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
        source.contains("(smelt_comparator)(&(left.clone()), &(right.clone()))"),
        "{source}"
    );
    assert!(
        source.contains("if ordering < 0.0 { std::cmp::Ordering::Less }"),
        "{source}"
    );
}

#[test]
fn emits_runtime_branch_for_an_optional_sort_comparator() {
    // ECMA-262 `Array.prototype.sort` step 1: `sort(undefined)` is `sort()`.
    // An optional comparator must therefore stay a real `Option<..>` down to
    // runtime and be matched, NOT be wrapped in an erased callback whose absent
    // call yields `undefined` (which the numeric coercion reads as 0, making
    // every comparison `Equal` and the whole sort a silent no-op).
    let source = source_for(
        r"
export function sortKeysLike(keys: string[], compare?: (a: string, b: string) => number): string[] {
  return keys.slice().sort(compare);
}
",
    );

    assert!(
        source.contains("match compare.clone() { Some(smelt_comparator) =>"),
        "the optional comparator must be matched at runtime: {source}"
    );
    assert!(
        source.contains(
            "None => { _smelt_tmp_2.borrow_mut().sort_by(|left, right| left.to_string().cmp(&right.to_string())); }"
        ),
        "the absent arm must run the default ToString ordering: {source}"
    );
    let body = source
        .split_once("fn sort_keys_like(")
        .expect("generated function")
        .1;
    let body = body.split_once("\nfn ").map_or(body, |(head, _)| head);
    assert!(
        !body.contains("SmeltUnknown"),
        "the optional comparator must not be erased: {body}"
    );
}

#[test]
fn emits_locale_compare_through_the_runtime_collation_helper() {
    // `String.prototype.localeCompare` used to resolve as an ABSENT member and
    // lower to a bare `SmeltUnknown::Null` that was then CALLED, so the
    // comparator answered `NaN` for every pair. It is a modeled builtin now.
    let source = source_for(
        r"
export function collate(a: string, b: string): number {
  return a.localeCompare(b);
}
",
    );

    assert!(
        source.contains("smelt_locale_compare(a.clone().as_str(), b.clone().as_str())"),
        "{source}"
    );
    assert!(
        source.contains("fn smelt_locale_compare(left: &str, right: &str) -> f64"),
        "the runtime helper must be emitted into the prelude: {source}"
    );
    assert!(
        !source.contains("SmeltUnknown::Null"),
        "an unmodeled member must not silently become a value: {source}"
    );
}

#[test]
fn emits_nan_equal_leaves_for_a_nested_float_comparison() {
    // JavaScript deep equality compares numeric leaves with `Object.is`, so
    // `NaN` equals `NaN`. Rust's `f64: PartialEq` is IEEE `==`, under which it
    // does not, and `SmeltList<f64> == SmeltList<f64>` inherits that — so
    // `expect([NaN, 1]).toEqual([NaN, 1])` failed with the right value on both
    // sides. The scalar case already had this rule; this is the nested one.
    let source = source_for(
        r"
export function listsEqual(left: number[], right: number[]): boolean {
  return left == right;
}
",
    );
    assert!(
        source.contains("left_item.is_nan() && right_item.is_nan()"),
        "a float leaf inside a list must compare NaN-equal: {source}"
    );
}

#[test]
fn lowers_an_array_hole_to_undefined_not_null() {
    // A hole in an array literal reads as `undefined` in JavaScript, never as
    // `null` — the index is absent, and an absent property read answers
    // `undefined`. Lowering it to `null` made `[1, , 2]` indistinguishable from
    // `[1, null, 2]`.
    let source = source_for(
        r"
export function holes(): unknown[] {
  return [1, , 2];
}
",
    );

    let body = source.split_once("fn holes(").expect("generated function").1;
    let body = body.split_once("\nfn ").map_or(body, |(head, _)| head);
    assert!(
        body.contains("SmeltUnknown::Undefined"),
        "the hole must be `undefined`: {body}"
    );
    assert!(
        !body.contains("SmeltUnknown::Null"),
        "the hole must not become `null`: {body}"
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

/// Synchronous generators must preserve their suspension points instead of
/// eagerly collecting every yielded value before the caller can resume them.
#[test]
fn emits_resumable_synchronous_generator() {
    let source = source_for(
        r#"
function* sequence(): Generator<number, string, unknown> {
  yield 1;
  yield 2;
  return "done";
}

function* delegated(): Generator<number, string, unknown> {
  return yield* sequence();
}

class IterableValue {
  value = 3;

  *[Symbol.iterator](): Generator<number, string, unknown> {
    yield this.value;
    return "iterable done";
  }
}

function* delegatedIterable(value: IterableValue): Generator<number, string, unknown> {
  return yield* value;
}

function delayedThrow(message: string): Generator<number, string, unknown> {
  return (function* (): Generator<number, string, unknown> {
    yield 4;
    throw new Error(message);
  })();
}

const iterator = sequence();
const first = iterator.next();
const firstDone = first.done;
const firstValue = first.value;
"#,
    );

    assert!(
        source.contains("SmeltGenerator<f64, String, SmeltUnknown>"),
        "{source}"
    );
    assert_eq!(source.matches("co.yield_").count(), 6, "{source}");
    assert!(source.contains("return \"done\".to_owned()"), "{source}");
    assert!(
        source.contains(
            "fn __smelt_symbol_iterator(&self) -> SmeltGenerator<f64, String, SmeltUnknown>"
        ),
        "{source}"
    );
    assert!(source.contains("self_owned.value"), "{source}");
    assert!(source.contains("value.__smelt_symbol_iterator()"), "{source}");
    assert!(
        source.contains("value.unwrap_or_else(|error| panic!(\"{}\", error))"),
        "{source}"
    );
    assert!(
        source.contains(".resume(SmeltGeneratorCommand::Next"),
        "{source}"
    );
    assert!(
        source.contains("SmeltGeneratorResult::Yielded(value) => { co.yield_"),
        "{source}"
    );
    assert!(
        source.contains("matches!(first.clone(), SmeltGeneratorResult::Complete(_))"),
        "{source}"
    );
}

/// Async generators expose promise-valued resumes while retaining the same
/// typed yield/completion carrier as synchronous generators.
#[test]
fn emits_resumable_async_generator() {
    let source = source_for(
        r#"
async function* sequence(): AsyncGenerator<number, string, unknown> {
  yield 1;
  await Promise.resolve(2);
  yield 2;
  return "done";
}

async function consume(): Promise<string> {
  const iterator = sequence();
  const first = await iterator.next();
  const second = await iterator.next();
  const completed = await iterator.next();
  return completed.value as string;
}
"#,
    );

    assert!(
        source.contains("SmeltAsyncGenerator<f64, String, SmeltUnknown>"),
        "{source}"
    );
    assert!(source.contains("smelt_generator.async_resume().await"), "{source}");
    assert!(source.contains("SmeltFuture<SmeltGeneratorResult<f64, String>>"), "{source}");
    assert_eq!(source.matches("co.yield_").count(), 2, "{source}");
    assert!(
        source.matches(".resume(SmeltGeneratorCommand::Next").count() >= 3,
        "{source}"
    );
}

/// A method shared by every arm of a concrete union must dispatch through the
/// generated tagged enum while preserving a generator-valued generic return.
#[test]
fn emits_union_method_returning_generator_for_async_delegation() {
    let source = source_for(
        r"
class Ok<T> {
  constructor(readonly value: T) {}
  safeUnwrap(): Generator<number, T, unknown> {
    const value = this.value;
    return (function* (): Generator<number, T, unknown> { return value; })();
  }
}

class Err<T> {
  constructor(readonly value: T) {}
  safeUnwrap(): Generator<number, T, unknown> {
    const value = this.value;
    return (function* (): Generator<number, T, unknown> { return value; })();
  }
}

type Result<T> = Ok<T> | Err<T>;

async function* unwrap<T>(promise: Promise<Result<T>>): AsyncGenerator<number, T, unknown> {
  return yield* await promise.then((result) => result.safeUnwrap());
}
",
    );

    assert!(source.contains("match closure_arg_0"), "{source}");
    assert!(source.contains("::M0(value) => value.safe_unwrap()"), "{source}");
    assert!(source.contains("::M1(value) => value.safe_unwrap()"), "{source}");
    assert!(source.contains("let smelt_delegate"), "{source}");
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

    // Both fixed parameters are read out of the packed rest vector by index
    // (`closure_arg_0.get({..index..})`). The callable-typed parameter's read
    // is hoisted into a `let smelt_source_value = closure_arg_0.get(..)` binding
    // by the callable-object narrowing, so assert on the shared `.get(` read.
    //
    // `data: readonly T[]` is extracted out of the packed rest vector, and `T`
    // is not a Rust generic in this emission scope, so its elements are already
    // `SmeltUnknown`: the extraction re-wraps the SAME array rather than
    // rebuilding its element vector (see `erased_to_list_text`). That is what
    // keeps a `purry`-style runtime dispatcher from paying an O(n) copy on every
    // crossing, and it is what makes the array a callback receives BE the array
    // being iterated.
    assert!(
        source.contains("closure_arg_0.borrow().get(")
            && source
                .contains("SmeltUnknown::Array(values) => SmeltList::with_storage(values.id,"),
        "fixed callback spread calls should read the first fixed parameter from the rest vector: {source}"
    );
    assert!(
        source.matches("closure_arg_0.borrow().get(").count() >= 2,
        "fixed callback spread calls should read the second fixed parameter from the rest vector: {source}"
    );
    assert!(
        source.contains("}, n)"),
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
        source.contains("(identity)(&(SmeltUnknown::String(\"hello\".into())), _smelt_tmp_2)")
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

    // `closure_arg_0` is itself spelled `&SmeltUnknown` (the contextual `dyn Fn`
    // passes it by shared reference), and `predicate` takes `&SmeltUnknown`, so
    // the binding is forwarded as-is. Borrowing it again would hand the callee a
    // `&&SmeltUnknown` that only compiles through deref coercion.
    assert!(
        source.contains("predicate(closure_arg_0, closure_arg_1)"),
        "a borrowed callback parameter should forward without a re-borrow: {source}"
    );
    assert!(
        source.contains(
            "let smelt_push_item = closure_arg_1; (*smelt_capture_indices.borrow()).borrow_mut().push(smelt_push_item)"
        ),
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
        source.contains("(value, key.clone())"),
        "pushed literal should be a concrete tuple value: {source}"
    );
    assert!(
        source.contains("result.borrow_mut().push("),
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
        source.contains("let smelt_push_item = SmeltUnion") && source.contains("::M0(1.0)"),
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
            && source.contains("SmeltUnknown::String(\"x\".into())"),
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
    // The returned value is the BINDING, not a second closure: a function value
    // has observable JavaScript identity, so `mapper` is materialized once and
    // read back. Either spelling of that read is fine; what matters is that the
    // return is an `Rc<dyn Fn>` (asserted above) rather than an unboxed closure.
    assert!(
        source.contains("return ::std::rc::Rc::new(")
            || source.contains("return _smelt_tmp_2.clone()")
            || source.contains("return mapper.clone()"),
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
        source.contains("let mut values: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into(SmeltList::new(Vec::<SmeltUnknown>::new()));"),
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
        source.contains("SmeltUnknown::Array(values) => values"),
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
        source.contains("SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string()"),
        "{source}"
    );
    assert!(source.contains(".match_string(&match "), "{source}");
}

#[test]
fn erased_receiver_two_callback_match_stays_dynamic_not_string_match() {
    // Regression: an erased receiver whose `.match(okFn, errFn)` call shape is a
    // neverthrow-style two-callback dispatch must NOT be lowered to
    // `String.prototype.match`; only a single regex/string-pattern argument on an
    // erased receiver qualifies. Wrong arity routes to dynamic member dispatch.
    let source = source_for(
        r"
function fold(value: any): number {
  return value.match((ok: number) => ok, (_err: string) => 0);
}
",
    );

    assert!(!source.contains(".match_string(&"), "{source}");
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
        source.contains("SmeltRegExp::new(source.to_string(), flags).test(&haystack)"),
        "{source}"
    );
    assert!(
        source.contains("(\"flags\".to_owned(), SmeltUnknown::String(self.flags.into()))"),
        "{source}"
    );
}

#[test]
fn regexp_metadata_fields_resolve_to_concrete_types() {
    // A data-property read on a concrete `RegExp` receiver must type as the
    // concrete `SmeltRegExp` field (`source`/`flags` are `String`), not the
    // erased `Unknown` boundary. Otherwise passing the read into
    // `new RegExp(...)`'s `String` parameters emits a stringify-of-
    // `SmeltUnknown` match against an already-`String` scrutinee (E0308).
    let source = source_for(
        r"
export function cloneRe(obj: unknown): RegExp {
  const regExp = obj as unknown as RegExp;
  return new RegExp(regExp.source, regExp.flags);
}
",
    );

    assert!(
        source.contains("SmeltRegExp::new(reg_exp.source.clone().clone(), reg_exp.flags.clone().clone())"),
        "{source}"
    );
    // The erased stringify coercion must not be applied to the concrete
    // `String` field reads.
    assert!(
        !source.contains("match reg_exp.source"),
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
        source.contains("fn expose(callback: ::std::rc::Rc<dyn Fn(&SmeltUnknown) -> SmeltUnknown>)"),
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
fn optional_call_on_absent_callback_short_circuits_to_none() {
    // `customizer?.(value)` where `customizer` is an absent `Option<Fn>` must
    // short-circuit to `undefined` (rendered `None` / `.map(..)`), NOT coerce the
    // callee into a null-returning default callback that is then invoked
    // unconditionally. The latter regression made `cloneDeepWith`/`cloneDeep`
    // return `null` early for arrays and objects (es-toolkit clone family).
    let source = source_for(
        r"
export function apply(customizer?: (value: number) => number): number | undefined {
  return customizer?.(1);
}
",
    );

    // The callee stays an `Option` and is mapped, so an absent callee yields `None`.
    assert!(
        source.contains(".map(|smelt_function|"),
        "optional call must map over the optional callee: {source}"
    );
    // It must NOT substitute a default callback and call it unconditionally.
    assert!(
        !source.contains("smelt_default_callback"),
        "optional call must not fall back to a null-returning default callback: {source}"
    );
}

#[test]
fn erased_array_indexed_assignment_sets_element_not_object() {
    // `result[i] = value` where `result` holds a runtime `SmeltUnknown::Array`
    // must set the array element (extending with `undefined`), not convert the
    // array into an object. The regression turned cloned arrays into objects
    // keyed by "0","1",..., breaking round-trips back to a typed list.
    let source = source_for(
        r"
export function fill(): unknown {
  const result: any = [];
  for (let i = 0; i < 3; i++) {
    result[i] = i;
  }
  return result;
}
",
    );

    // The call site routes through the prelude helper, which sets an array
    // element (via `set_index`) instead of converting the array to an object.
    assert!(
        source.contains("smelt_index_assign(&mut result,"),
        "erased indexed assignment must route through the prelude helper: {source}"
    );
    assert!(
        source.contains("array.set_index(index, value)"),
        "the prelude helper must set an array element at a numeric index: {source}"
    );
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
            "sub_priority.clone().expect(\"optional value was absent after narrowing\")"
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

    // The dynamic index write routes through the prelude helper, which inserts
    // an object property (and handles arrays/other values) in one place.
    assert!(source.contains("smelt_index_assign(&mut result,"), "{source}");
    assert!(source.contains("map.insert(key, value)"), "{source}");
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
    assert!(source.contains("__smelt_destructure.borrow().get"), "{source}");
    assert!(source.contains("normalized = 1.0"), "{source}");
    assert_eq!(
        source
            .matches("data.borrow_mut()[smelt_assign_index] = smelt_assign_value")
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
    // The subject is that the function-typed callback parameter is forwarded
    // directly, with no synthesized default standing in for it. Asserted against
    // that emitter's own marker rather than a bare `Default::default()`, which
    // the runtime prelude also contains for unrelated reasons (the field map's
    // hasher) and which would make this pass or fail on prelude text.
    assert!(!source.contains("smelt_default_callback"), "{source}");
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
    // The subject is that the function-typed callback parameter is forwarded
    // directly, with no synthesized default standing in for it. Asserted against
    // that emitter's own marker rather than a bare `Default::default()`, which
    // the runtime prelude also contains for unrelated reasons (the field map's
    // hasher) and which would make this pass or fail on prelude text.
    assert!(!source.contains("smelt_default_callback"), "{source}");
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
        .find("        smelt_drain_due_timers(id_barrier);")
        .unwrap();
    let yield_now = sleep_body
        .find("    tokio::task::yield_now().await;")
        .unwrap();
    assert!(drain < yield_now, "{source}");
    assert!(
        source.contains("let target_ms = smelt_mono_ms().saturating_add(delay_ms);"),
        "{source}"
    );
    assert!(
        source.contains("filter(|timer| timer.due_ms <= target_ms && timer.id < id_barrier)"),
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

    // The erased property read goes through the one prelude helper that knows
    // every receiver shape (object record, array named property,
    // `Object.prototype` fallback); the object arm inside it is still
    // `smelt_get_object_field`.
    let body = emitted_function_body(&source, "fn size_of");
    assert!(body.contains("smelt_get_unknown_field("), "{body}");
    assert!(body.contains("\"size\""), "{body}");
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
        body.contains("let smelt_id = smelt_l.id();"),
        "erasing a list local should read the list's own reference id\n{source}"
    );
    assert!(
        body.contains("SmeltArray::with_id(smelt_id,"),
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
        source.contains("smelt_array.borrow().iter().enumerate().map(|(index, item)|"),
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
        source.contains("(smelt_timer_callback)(smelt_timer_arg_0.clone(), smelt_timer_arg_1)"),
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
        source.contains("(smelt_timer_callback)(smelt_timer_arg_0.clone(), smelt_timer_arg_1)"),
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
fn indexes_concrete_union_list_with_union_missing_default() {
    // Out-of-bounds element access on a `SmeltList<SmeltUnion…>` must default to
    // a union value, not the erased `SmeltUnknown::Undefined`. The list element
    // is a tagged `SmeltUnion…`, so `.get(idx).cloned().unwrap_or(default)`
    // requires the default to be an actual union value (E0308 otherwise).
    let source = source_for(
        r"
export function pick(keys: Array<string | number>, i: number): string | number {
  return keys[i];
}
",
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    assert!(
        source.contains(".cloned().unwrap_or_else(|| SmeltUnion"),
        "missing element default must be a concrete union value: {source}"
    );
    assert!(
        !source.contains(".cloned().unwrap_or(SmeltUnknown::Undefined)"),
        "concrete-union element default must not be the erased undefined: {source}"
    );
}

#[test]
fn unifies_array_destructuring_default_of_different_type() {
    // `const [s, n = 0] = str.split('e')` binds `n` from a `string` element with
    // a numeric `0` default: JavaScript types it `string | number`. The binding
    // must unify into that union (a concrete `SmeltUnion…`) rather than assert
    // the numeric default to `String`, which would leave a runtime `f64` typed
    // as `String` (E0308).
    let source = source_for(
        r#"
export function adjust(value: number): number {
  const [magnitude, exponent = 0] = value.toString().split("e");
  return Number(`${magnitude}e${Number(exponent)}`);
}
"#,
    );

    assert!(source.contains("pub enum SmeltUnion"), "{source}");
    // `source_for` panics on a blocker; reaching codegen and generating a union
    // for the mixed-type default proves the unification path was taken.
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

/// A seeded reduce borrows its receiver for iteration, so the array argument
/// supplied to a four-parameter callback must never be MOVED out of the
/// surrounding function or closure.
///
/// It used to be cloned instead — one whole-list deep copy per element, which is
/// what made a four-parameter reduce O(n^2). The callback now takes that parameter
/// by shared reference (`callback_param_is_shared_reference`), so the fold body
/// binds the same borrow it is already iterating and nothing is copied at all. The
/// no-move assertion is the part of this test that was always the point.
#[test]
fn reduce_borrows_the_array_callback_argument_with_initial_value() {
    let source = source_for(
        r"
export function sum(values: number[]): number {
  const mapped = values.map(value => value);
  return mapped.reduce((acc, value, _index, array) => acc + value + array.length, 0);
}
",
    );

    assert!(source.contains("let array = &mapped;"), "{source}");
    assert!(!source.contains("let array = mapped;"), "{source}");
    assert!(!source.contains("let array = mapped.clone();"), "{source}");
}

/// Seedless reduce has the same ownership requirement after extracting its first
/// element: later iterations still borrow the receiver while invoking the callback,
/// so the array argument is that same borrow and is never moved.
#[test]
fn reduce_borrows_the_array_callback_argument_without_initial_value() {
    let source = source_for(
        r"
export function sum(values: number[]): number {
  const reversed = values.reverse();
  return reversed.reduce((acc, value, _index, array) => acc + value + array.length);
}
",
    );

    assert!(source.contains("let array = &reversed;"), "{source}");
    assert!(!source.contains("let array = reversed;"), "{source}");
    assert!(!source.contains("let array = reversed.clone();"), "{source}");
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
        r"
const smallNumbers = [0, 1].concat([2, 3, 4].slice(1));
export function numberCount(): number {
  const values = smallNumbers;
  return values.length;
}
",
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
        r"
function stubTrue(): boolean {
  return true;
}
export function run(values: number[]): boolean[] {
  return values.map(stubTrue);
}
",
    );
    assert!(source.contains("(smelt_callback)()"), "{source}");
    assert!(source.contains("fn run("), "{source}");
}

/// A zero-parameter named predicate emits a real filter over the receiver
/// instead of the former `Default::default()` placeholder.
#[test]
fn zero_parameter_named_filter_callback_emits_real_iteration() {
    let source = source_for(
        r"
function stubFalse(): boolean {
  return false;
}
export function run(values: number[]): number[] {
  return values.filter(stubFalse);
}
",
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
        r"
function withTail(value: number, index?: number, list?: number[], guard?: number): number {
  return value + (guard ?? 0);
}
export function run(values: number[]): number[] {
  return values.map(withTail);
}
",
    );
    assert!(
        source.contains("with_tail(closure_arg_0, closure_arg_1.clone(), closure_arg_2.clone(), None::<f64>)"),
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
        r"
function sortMixed(values: Array<string | number>): Array<string | number> {
  return values.sort();
}
",
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
        r"
function tag(rows: string[][], suffix: string): string[][] {
  return rows.map(row => {
    row[0] += suffix;
    return row;
  });
}
",
    );

    assert!(
        source.contains("closure_arg_0.borrow_mut()[smelt_assign_index] ="),
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
        r"
export function firstTruthy(guard?: unknown): boolean {
  if (guard) {
    return true;
  }
  return false;
}
",
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
        r"
function uniqBy<T>(arr: T[], mapper: (item: T, index: number, arr: T[]) => unknown): T[] {
  return arr;
}
export function unionBy<T>(arr1: T[], arr2: T[], mapper: (item: T) => unknown): T[] {
  return uniqBy([...arr1, ...arr2], mapper);
}
",
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
fn erased_rest_callback_maps_through_erased_call_abi() {
    // A `.map` callback whose value is an erased rest callable
    // (`SmeltErasedFunction`, e.g. the result of a currying/arity helper) is not
    // a Rust `Fn` and cannot be invoked with call syntax. The array-callback
    // lowering must route it through the erased callable ABI (`.call(..)`) rather
    // than emitting `(smelt_callback)(..)`, which fails to compile (E0618).
    let source = source_for(
        r"
function makeCapped(): (...args: unknown[]) => unknown {
  return (...args: unknown[]) => args[0];
}
export function run(items: string[]): unknown[] {
  const capped = makeCapped();
  return items.map(capped);
}
",
    );

    assert!(
        source.contains("smelt_callback.call("),
        "an erased rest callback must be invoked through the erased ABI\n{source}"
    );
    assert!(
        !source.contains("(smelt_callback)(SmeltList"),
        "an erased rest callback must not be called with call syntax\n{source}"
    );
}

#[test]
fn borrowed_rest_adapter_binds_owned_callback() {
    // Forwarding an owned callback value to a helper that expects an erased rest
    // callback builds a borrowed (`&mut`) wrapper closure whose body refers to a
    // `smelt_callback` binding. When the source is an owned value (not a borrowed
    // function parameter), the adapter must introduce that binding inside the
    // borrowed temporary, otherwise `smelt_callback` is unresolved (E0425).
    let source = source_for(
        r"
function unzipWith(arrays: number[][], iteratee: (...values: unknown[]) => unknown): unknown[] {
  const result: unknown[] = [];
  for (const group of arrays) {
    result.push(iteratee(group[0], group[1], group[2]));
  }
  return result;
}
export function run(zipped: number[][]): unknown[] {
  return unzipWith(zipped, (item: number, item2: number, item3: number) => item + item2 + item3);
}
",
    );

    assert!(
        source.contains("&mut { let smelt_callback ="),
        "a borrowed owned-callback adapter must bind smelt_callback\n{source}"
    );
}

#[test]
fn tuple_length_emits_constant_arity() {
    // A fixed-arity tuple has no Rust `.len()` method (E0599). Its JavaScript
    // `.length` is a compile-time constant, so the length rvalue must emit the
    // arity literal rather than a method call on the tuple.
    let source = source_for(
        r"
export function pairLength(pair: [string, number]): number {
  return pair.length;
}
",
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

#[test]
fn erased_call_reassignment_reuses_binding_without_double_move() {
    // Regression: `predicate = iteratee(predicate)` reassigns an erased local
    // from a call that returns a first-class function value. The erase seam used
    // to re-render the call at the reassignment site while ALSO emitting the
    // call-result binding, evaluating the call twice and moving `predicate` into
    // it twice (E0382). The erase must instead read the existing binding.
    let source = source_for(
        r"
function iteratee(value: unknown): (item: unknown) => unknown {
  return (item: unknown) => item;
}
export function normalize(predicate: unknown): unknown {
  predicate = iteratee(predicate);
  return predicate;
}
",
    );

    assert!(
        source.contains("fn normalize"),
        "expected the normalize function to be emitted\n{source}"
    );
    let normalize = &source[source.find("fn normalize").unwrap()..];
    assert!(
        normalize.matches("iteratee(").count() == 1,
        "the reassigned call must be rendered exactly once, not re-inlined\n{normalize}"
    );
}

#[test]
fn erased_rest_function_parameter_is_called_directly_not_via_fn_traits() {
    // Regression: an erased-rest (`(...args) => unknown`) function parameter is
    // emitted as a bare `&dyn Fn(...)` handle. Invoking it as `func.call(args)`
    // resolved to the unstable `Fn::call` trait method (E0658) and expected a
    // tuple argument (E0308). A function-parameter callee must use direct call
    // syntax, taking precedence over the erased-rest inherent `.call()`.
    let source = source_for(
        r"
export function invokeWith(func: (...args: unknown[]) => unknown, args: unknown[]): unknown {
  return func(...args);
}
",
    );

    assert!(
        source.contains("fn invoke_with"),
        "expected the function to be emitted\n{source}"
    );
    let body = &source[source.find("fn invoke_with").unwrap()..];
    assert!(
        !body.contains(".call("),
        "an erased-rest function parameter must not be invoked via the unstable Fn::call trait method\n{body}"
    );
}

#[test]
fn optional_chain_named_group_read_uses_named_group_owned() {
    // Regression: a named capture group read reached through an optional chain
    // (`m?.groups.result`) lost the `MatchGroups` receiver type inside the
    // `.as_ref().map(..)` closure and fell through to raw struct field access
    // (`_smelt_value.result`), which does not exist on `SmeltMatch` (E0609). The
    // optional field read must keep the typed match accessor.
    let source = source_for(
        r"
export function firstGroup(input: string): string | undefined {
  const m = /(?<result>\w+)/.exec(input);
  return m?.groups.result;
}
",
    );
    assert!(
        source.contains(".named_group_owned(\"result\")"),
        "optional-chain named group read should use named_group_owned: {source}"
    );
}

#[test]
fn mutated_parameter_captured_by_escaping_closure_uses_shared_cell() {
    // Regression: a function parameter mutated inside an escaping closure (stored
    // as `Rc<dyn Fn>`) needs a shared `Rc<RefCell<..>>` cell so the closure can
    // mutate it through interior mutability. The capture classifier previously
    // excluded parameters, so the closure body borrowed the parameter as mutable
    // through a shared `Rc` (E0596). This mirrors es-toolkit `after`/`before`.
    let source = source_for(
        r"
export function afterN(n: number, func: () => void): () => void {
  return () => {
    n = n - 1;
    if (n <= 0) {
      func();
    }
  };
}
",
    );
    assert!(
        source.contains("smelt_capture_n"),
        "a parameter mutated inside an escaping closure should use a shared capture cell: {source}"
    );
}

#[test]
fn erased_callback_optional_return_adapts_through_checked_boundary() {
    // Regression: invoking an erased callable adapter whose source return type is
    // an optional (`boolean | void`) produced `.call(..).clone().map(..)`, calling
    // `Option` methods on the bare `SmeltUnknown` the erased call yields (E0599),
    // then double-evaluating the argument-consuming call (E0382). The erased call
    // result must be adapted from `Unknown` through a single checked nullish guard.
    let source = source_for(
        r"
function run(cb: (a: unknown, b: unknown) => boolean | void, x: unknown, y: unknown): boolean {
  const r = cb(x, y);
  if (r !== undefined) {
    return r;
  }
  return false;
}
export function useRun(fn: (...args: unknown[]) => boolean | void): boolean {
  return run(fn, 1, 2);
}
",
    );
    assert!(
        source.contains("smelt_unknown_is_nullish")
            || source.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => None"),
        "erased optional-return adapter should use a checked nullish boundary: {source}"
    );
}

#[test]
fn callable_interface_call_slot_default_is_erased_function() {
    // Regression (es-toolkit main.rs): a callable interface's synthetic
    // `__smelt_call` storage field is declared `SmeltErasedFunction`, but the
    // generated `Default` impl initialized it with the concrete-signature
    // `Rc<dyn Fn(..)>` default because `default_value_with_scoped_type_params`
    // lacked the erased-function guard that the field's TYPE text applies. The
    // field type and its default must agree (E0308).
    let source = source_for(
        r"
interface Formatter {
  (...args: unknown[]): unknown;
  label: string;
}
export function makeFormatter(): Formatter {
  return { label: 'x' } as Formatter;
}
",
    );
    assert!(
        source.contains("__smelt_call: SmeltErasedFunction {"),
        "callable-interface `__smelt_call` default must be a SmeltErasedFunction, \
         not an Rc<dyn Fn> closure: {source}"
    );
}

#[test]
fn record_to_generic_struct_adapter_erases_out_of_scope_type_params() {
    // Regression (es-toolkit flowRight_spec / curry): a record adapted into a
    // parameterized callable interface (`CurriedFunction1<T1, R>`) at a
    // NON-generic call site rendered its `__smelt_call` field default with the
    // interface's own type param spelled literally (`Rc<dyn Fn() ->
    // CurriedFunction1<T1, SmeltUnknown>>`). `T1` is not in scope in the
    // non-generic caller, so it was an unresolvable name (was E0425). The
    // adapter must only keep type params that are actually in scope for the
    // emitted function and erase the rest to `SmeltUnknown`.
    let source = source_for(
        r"
interface CurriedFunction1<T1, R> {
  (): CurriedFunction1<T1, R>;
  (t1: T1): R;
  tag: string;
}
export function makeCurried(): CurriedFunction1<number, string> {
  const built: CurriedFunction1<number, string> = { tag: 'x' } as CurriedFunction1<number, string>;
  return built;
}
",
    );
    // Only inspect the non-generic `make_curried` body; the generic struct's own
    // `Default`/impl blocks legitimately spell `T1`/`R` because those are in
    // scope there.
    let body = source
        .split("fn make_curried")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or("");
    assert!(
        !body.contains("CurriedFunction1<T1"),
        "record-to-struct adapter must not spell an out-of-scope type param `T1` \
         in the non-generic caller: {source}"
    );
    // `CurriedFunction1` declares two call signatures of different arities, so
    // its single `__smelt_call` slot is now the erased variadic callable (see
    // `add_interface_call_signature_field`) rather than the first signature's
    // concrete `Fn() -> CurriedFunction1<T1, R>`. That removes the interface's
    // own type parameters from the slot entirely, which is a strictly stronger
    // form of the property this test guards: there is no longer any type
    // argument on the slot that COULD be out of scope. The assertion below
    // therefore pins the new representation; the out-of-scope check above --
    // the actual regression -- is unchanged.
    assert!(
        body.contains("__smelt_call: SmeltErasedFunction"),
        "an overloaded callable interface's slot should be the erased variadic \
         callable, leaving no interface type argument to fall out of scope: {source}"
    );
}

#[test]
fn apply_on_typed_function_value_emits_a_call_not_a_null() {
    // Regression (es-toolkit curry/partial/partialRight/flow): `fn.apply(this, args)`
    // on a receiver whose type is a concrete `Type::Function` had no lowering arm --
    // `call` did, `apply` did not -- so the member read fell through to the ordinary
    // (absent) field path and the WHOLE call collapsed to a literal `null`, with no
    // diagnostic. Only an erased (`unknown`) receiver reached the runtime
    // `smelt_function_method(.., "apply")` dispatch, so the identical source line
    // behaved differently depending on whether the callee had kept its static type.
    let source = source_for(
        r"
export function forward(func: (...args: any[]) => any, args: any[]): any {
  return func.apply(null, args);
}
",
    );
    let body = source
        .split("fn forward")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or("");
    assert!(
        !body.contains("return SmeltUnknown::Null;"),
        "`apply` on a typed function must not collapse to a null literal: {source}"
    );
    assert!(
        body.contains("func(args"),
        "`apply` should forward the argument array to the callee: {source}"
    );
}

#[test]
fn overloaded_callable_interface_stores_an_erased_variadic_call_slot() {
    // Regression (es-toolkit compat `curry`): a callable interface's generated
    // struct carries ONE `__smelt_call` slot. When the interface declares several
    // call signatures of DIFFERENT arities, no single concrete signature can hold
    // the value -- which overload runs is decided by the runtime argument list --
    // yet the slot used to be typed from the FIRST signature. Every call site then
    // adapted to that signature and silently discarded the arguments actually
    // passed: es-toolkit's two-argument `curried(2, 3)` emitted `(smelt_callback)()`,
    // running the zero-argument overload and answering a defaulted value with no
    // diagnostic.
    //
    // The overload set now collapses to one erased variadic callable, exactly as a
    // union of differing-arity function types already does. A uniformly-shaped
    // callable interface keeps its precise concrete slot, so nothing is erased that
    // a concrete Rust `Fn` type could have carried.
    let source = source_for(
        r"
interface Overloaded {
  (): Overloaded;
  (t1: number): number;
  (t1: number, t2: number): number;
}
interface Uniform {
  (t1: number): number;
}
export function use_them(a: Overloaded, b: Uniform): number {
  return a(1, 2) + b(3);
}
",
    );
    let overloaded_struct = source
        .split("struct Overloaded")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .unwrap_or("");
    assert!(
        overloaded_struct.contains("__smelt_call: SmeltErasedFunction"),
        "an overload set whose arities differ must store the erased variadic \
         callable, not the first signature: {source}"
    );
    let uniform_struct = source
        .split("struct Uniform")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .unwrap_or("");
    assert!(
        uniform_struct.contains("__smelt_call: ::std::rc::Rc<dyn Fn(f64) -> f64>"),
        "a uniformly-shaped callable interface must keep its concrete slot type: {source}"
    );
}

#[test]
fn tuple_assertion_on_list_value_preserves_list_representation() {
    // Regression (es-toolkit xorBy): a TypeScript tuple assertion applied to a
    // list value (`xs.filter(...) as [T]`) is type-level only. Materializing the
    // tuple would repackage the whole list into a 1-tuple `(SmeltUnknown,)` that
    // no longer satisfies a `SmeltList` consumer (E0308). The list value and its
    // type must be preserved.
    let source = source_for(
        r"
export function pickList(xs: unknown[]): unknown[] {
  const ys = xs.filter(x => x != null) as [unknown];
  return ys;
}
",
    );
    assert!(
        !source.contains("(SmeltUnknown,)"),
        "tuple assertion on a list must not build a 1-tuple: {source}"
    );
}

#[test]
fn branch_join_narrows_optional_reassigned_in_both_arms() {
    // Regression (es-toolkit includes): when both arms of an if/else reassign an
    // optional local to a non-null value, the local is non-null after the join
    // and should narrow to its inner type so later arithmetic/indexing type-checks
    // against the concrete type instead of the declared `Optional<T>`.
    let source = source_for(
        r"
export function clampFrom(guard: boolean, fromIndex?: number): number {
  if (guard || !fromIndex) {
    fromIndex = 0;
  } else {
    fromIndex = fromIndex + 1;
  }
  return fromIndex + 1;
}
",
    );
    // The post-join read of `fromIndex` must be a plain `f64`, not an
    // `Option<f64>` unwrapped at the use site.
    assert!(
        !source.contains("from_index.unwrap")
            && !source.contains("from_index.clone().expect"),
        "branch-join narrowing should leave fromIndex as a concrete f64: {source}"
    );
}

#[test]
fn concrete_list_length_in_map_closure_erases_to_unknown() {
    // Regression (es-toolkit unzipWith): `arr.map(a => a.length)` where the map
    // result feeds an erased (`SmeltUnknown`) list. `.length` on a CONCRETE list
    // renders as `(x.len() as f64)` (an `f64`), so its resolved value type must
    // be `Float` — otherwise the map closure's return is treated as already
    // erased and the collected `Vec<f64>` cannot bridge to `SmeltList<SmeltUnknown>`.
    // With `.length` typed `Float`, the closure return correctly erases the
    // number to `SmeltUnknown::Number(..)`.
    let source = source_for(
        r"
export function maxWidth(rows: unknown[][]): number {
  return Math.max(...rows.map((r) => r.length));
}
",
    );
    assert!(
        source.contains("SmeltUnknown::Number"),
        "concrete list length in a map closure feeding an erased list must erase \
         through SmeltUnknown::Number: {source}"
    );
    assert!(
        !source.contains("Vec<f64>") && !source.contains("SmeltList<f64>"),
        "the map result must not stay a concrete f64 list when the destination \
         element is erased: {source}"
    );
}

#[test]
fn generic_mut_list_forwarded_to_erased_callee_uses_convert_in_place_adapter() {
    // Regression (es-toolkit pull): a generic function forwards its `&mut T[]`
    // parameter into an erased helper whose parameter is `&mut SmeltUnknown[]`.
    // Rust `&mut` is invariant, so the reborrow needs a convert-in-place adapter:
    // build an erased temp list, pass `&mut temp`, write the mutated elements
    // back, and un-erase the returned value. The caller must STAY generic.
    let source = source_for(
        r#"
function eraseInto(target: unknown[], value: unknown): void {
  target.push(value);
}
export function pushTyped<T>(arr: T[], value: T): T[] {
  eraseInto(arr, value);
  return arr;
}
export function useNums(): number[] {
  const xs: number[] = [1, 2, 3];
  return pushTyped(xs, 4);
}
export function useStrs(): string[] {
  const ys: string[] = [""];
  return pushTyped(ys, "a");
}
"#,
    );
    assert!(
        source.contains("fn push_typed<T"),
        "the forwarding function must stay generic: {source}"
    );
    // The `&mut T[]` argument is erased into a temporary `SmeltUnknown` list
    // (`into_smelt_unknown`) and the mutated temp is written back through the
    // reference with per-element un-erasure (`smelt_from_unknown`).
    assert!(
        source.contains("smelt_mut_arg_0")
            && source.contains("into_smelt_unknown")
            && source.contains("smelt_from_unknown"),
        "the forwarded &mut list must be adapted in place through erase/un-erase: {source}"
    );
}

#[test]
fn plain_local_mut_list_argument_to_generic_callee_passes_elements_through() {
    // Regression (es-toolkit pullAt): an ordinary caller passes a concrete local
    // list into a callee that mutates it and IS emitted with real Rust generics
    // (`fn drop_first<T>(arr: &mut SmeltList<T>)`). Rust binds `T` from the
    // caller's `SmeltList<f64>`, so the elements must pass through unconverted and
    // the callee must borrow the caller's local directly — no conversion, no copy,
    // no write-back. Rendering the argument as `&mut <erased clone>` silently
    // discarded the mutation.
    let source = source_for(
        r"
function dropFirst<T>(arr: T[]): T[] {
  return arr.splice(0, 1);
}
export function useDrop(): number[] {
  const xs: number[] = [1, 2, 3];
  dropFirst(xs);
  return xs;
}
export function useDropStrings(): string[] {
  const ys: string[] = ['a', 'b'];
  dropFirst(ys);
  return ys;
}
",
    );
    assert!(
        source.contains("fn drop_first<T"),
        "the mutating callee must stay generic: {source}"
    );
    assert!(
        source.contains("drop_first(&mut xs)") && source.contains("drop_first(&mut ys)"),
        "both concrete locals must be borrowed mutably in place, with no erasing \
         temporary: {source}"
    );
    assert!(
        !source.contains("drop_first(&mut {"),
        "the argument must not be a converted temporary — that would discard the \
         mutation: {source}"
    );
    assert!(
        source.contains("let mut xs:") && source.contains("let mut ys:"),
        "a local borrowed mutably at a call site must be declared `mut`: {source}"
    );
}

#[test]
fn plain_local_mut_list_argument_to_erased_callee_converts_in_place() {
    // Regression (es-toolkit pull/remove): an ordinary caller passes a concrete
    // local list into a callee whose mutable list parameter is erased
    // (`&mut SmeltList<SmeltUnknown>`). `&mut` is invariant, so the elements must
    // be erased into a temporary, the callee must mutate that temporary, and the
    // result must be un-erased back into the caller's local. The write-back must
    // assign the local directly — the place is not a reference, so `*xs = ..`
    // would not compile and `(*xs).clone()` would clone a slice.
    let source = source_for(
        r"
function eraseInto(target: unknown[], value: unknown): void {
  target.push(value);
}
export function useErase(): number[] {
  const xs: number[] = [1, 2];
  eraseInto(xs, 3);
  return xs;
}
",
    );
    assert!(
        source.contains("let mut smelt_mut_arg_0: SmeltList<SmeltUnknown> = xs.clone()")
            && source.contains("into_smelt_unknown"),
        "the concrete local must be erased into the adapter temporary: {source}"
    );
    assert!(
        source.contains("xs = smelt_mut_arg_0")
            && source.contains("smelt_from_unknown")
            && !source.contains("*xs = ")
            && !source.contains("(*xs).clone()"),
        "the mutated temporary must be un-erased back into the owned local, not \
         written through a reference: {source}"
    );
}

#[test]
fn loop_body_if_guard_condition_renders_at_bool_type() {
    // Regression (es-toolkit template): an `if`-guard nested inside a for-of loop
    // is emitted through the structured while-header path when its then-branch
    // can reach the header again via the enclosing loop's back edge. The header
    // condition (a truthiness `PrimitiveCast`/`ToBool` of an optional) must be
    // rendered at the switch local's boolean type; rendering it at the default
    // `none` destination made the cast fall through to the unit default and emit
    // an uncompilable `while ()` (expected `bool`, found `()`).
    let source = source_for(
        r#"
export function joinTruthy(parts: (string | undefined)[]): string {
  let out = "";
  for (const part of parts) {
    if (part) {
      out += part;
    }
  }
  return out;
}
"#,
    );
    assert!(
        !source.contains("while ()"),
        "a loop-body guard must not emit an empty `while ()` condition: {source}"
    );
}

#[test]
fn borrowed_rest_callback_adapter_binds_smelt_callback() {
    // Regression (es-toolkit template): a non-parameter local callback forwarded
    // by mutable reference into an erased variadic-rest parameter is wrapped in a
    // `move |smelt_args| (smelt_callback)(..)` adapter. The borrowed (`&mut`) path
    // must introduce the `let smelt_callback = ..` binding the closure captures;
    // otherwise the emitted closure references an unbound `smelt_callback`
    // (E0425).
    let source = source_for(
        r#"
function attempt(func: (...args: unknown[]) => unknown): unknown {
  return func();
}
export function useAttempt(): unknown {
  return attempt(() => {
    throw new Error("no");
  });
}
"#,
    );
    assert!(
        source.contains("&mut { let smelt_callback ="),
        "the borrowed rest-callback adapter must bind smelt_callback inside the \
         &mut block it captures: {source}"
);
}

#[test]
fn wraps_erased_rest_call_result_in_option_for_optional_return() {
    // The fully-erased `SmeltErasedFunction::call` ABI always yields a bare
    // `SmeltUnknown`, even when the callee's declared return type is
    // `ReturnType<F> | undefined` (which lowers to `Option<SmeltUnknown>`).
    // The call result must therefore be coerced at the assignment seam — a
    // raw `SmeltUnknown` stored into an `Option<SmeltUnknown>` place is a
    // type error (E0308). This mirrors es-toolkit's `after`/`before`.
    let source = source_for(
        r"
type AnyFn = (...args: unknown[]) => unknown;

function after(n: number, func: AnyFn): (...args: unknown[]) => unknown | undefined {
  let count = 0;
  return (...args: unknown[]) => {
    count += 1;
    if (count >= n) {
      return func(...args);
    }
    return undefined;
  };
}

export function run(): unknown | undefined {
  const gated = after(0, () => 1);
  const result = gated();
  return result;
}
",
    );

    assert!(
        source.contains("Option<SmeltUnknown>"),
        "the optional erased return should lower to `Option<SmeltUnknown>`\n{source}"
    );
    assert!(
        source.contains(".call(") && source.contains("Some("),
        "an erased-rest call feeding an optional return must wrap its \
         `SmeltUnknown` result in `Some(..)`\n{source}"
    );
}

/// A `RegExp.lastIndex` write must target the backing `RefCell<usize>` through
/// `borrow_mut()`, narrowing the numeric right-hand side back to `usize`. The
/// former read-path text `(*regex.last_index.borrow() as f64)` is not a valid
/// assignment target (E0070).
#[test]
fn regexp_last_index_write_targets_borrow_mut() {
    let source = source_for(
        r"
export function run(): number {
  const regex = /a/g;
  regex.lastIndex = 10;
  return regex.lastIndex;
}
",
    );

    assert!(
        source.contains("*regex.last_index.borrow_mut() = (") && source.contains(") as usize;"),
        "lastIndex write should go through borrow_mut with a usize cast\n{source}"
    );
    assert!(
        !source.contains("borrow() as f64) = "),
        "the invalid cast-as-lvalue read form must not be used for writes\n{source}"
    );
}

/// Comparing a combinator result (already an erased `Rc<dyn Fn(...) -> _>`)
/// against a locally bound concrete closure by identity must coerce both
/// operands to the common `Rc<dyn Fn(...)>` type before `Rc::ptr_eq`, or the
/// two distinct `Rc<T>` types fail to unify (E0308).
#[test]
fn function_ptr_eq_coerces_operands_to_common_dyn_type() {
    let source = source_for(
        r"
export function run(fns: Array<() => unknown>): boolean {
  const a = fns[0];
  const b = fns[1];
  return a === b;
}
",
    );

    // The comparison goes through `smelt_same_function_identity` rather than a
    // bare `Rc::ptr_eq`: erasing a callable builds a fresh forwarding adapter, so
    // two handles on one source function are distinct allocations and pointer
    // equality alone reports `f === f` as false. The operand coercion this test
    // was written to protect is unchanged — both sides are still bound at a
    // common dyn-Fn type before being compared.
    assert!(
        source.contains("smelt_same_function_identity(&{ let smelt_lhs_fn:")
            && source.contains("let smelt_rhs_fn:"),
        "function identity comparison must coerce both operands to a common \
         dyn-Fn type before comparing identity\n{source}"
    );
}

/// A `never`-returning predicate (`(value: never) => value`) evaluates to a
/// real erased `SmeltUnknown` at runtime. Coercing that result into a concrete
/// `bool` parameter must route through JS-truthiness extraction rather than
/// handing the raw `SmeltUnknown` to a `bool` slot (E0308).
#[test]
fn never_return_coerces_to_bool_via_truthiness() {
    let source = source_for(
        r"
function pickBy(obj: Record<string, unknown>, pred: (v: unknown) => boolean): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const key in obj) {
    if (pred(obj[key])) {
      out[key] = obj[key];
    }
  }
  return out;
}

export function run(): Record<string, unknown> {
  const obj = {};
  const shouldPick = (value: never) => value;
  return pickBy(obj, shouldPick);
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Bool(value) => value")
            && source.contains("=> false"),
        "a never-returning predicate result must be coerced to bool via \
         truthiness\n{source}"
    );
}

/// An object literal passed where a `number | Options` union is expected must
/// be injected into the record-shaped union arm. Previously only collection
/// shapes were shape-matched, so a `Dict`/record source against a class arm
/// found no member and the raw record was passed (E0308). Mirrors es-toolkit's
/// `retry(fn, { retries })`.
#[test]
fn object_literal_injects_into_record_union_arm() {
    let source = source_for(
        r"
interface Options {
  retries?: number;
}

function retry(fn: () => number, options: number | Options): number {
  return fn();
}

export function run(): number {
  return retry(() => 1, { retries: 3 });
}
",
    );

    assert!(
        source.contains("::M1("),
        "an object literal must be injected into the record-shaped union arm\n{source}"
    );
}

/// Slicing a typed list into an erased (`SmeltUnknown`) destination must
/// materialize a real `SmeltList` and erase it, rather than leaking a bare
/// `Vec` into the `SmeltUnknown` slot (E0308). Mirrors es-toolkit's `ary`,
/// which caps a rest-argument list and forwards it through an erased value.
#[test]
fn list_slice_into_unknown_destination_materializes_smelt_list() {
    let source = source_for(
        r"
export function run(args: string[]): unknown {
  const capped: unknown = args.slice(0, 2);
  return capped;
}
",
    );

    // The slice is materialized as an identity-bearing array value and erased,
    // never assigned as a raw `Vec` collected straight into a `SmeltUnknown`.
    assert!(
        source.contains("SmeltArray::with_id") || source.contains("SmeltList::with_id"),
        "a slice into an erased destination must build an identity-bearing list\n{source}"
    );
    assert!(
        !source.contains(": SmeltUnknown = args.clone().iter().skip"),
        "the bare-Vec slice must not be assigned directly to a SmeltUnknown\n{source}"
    );
}

/// A callback that returns a future, adapted to a target whose expected return
/// is a plain (non-future) value, must have its future result wrapped as a
/// `SmeltUnknown::Promise` rather than leaking a raw `Pin<Box<dyn Future>>`
/// into the value slot (E0308). Mirrors es-toolkit's `attempt(async () => 1)`,
/// where the synchronous `attempt` receives the returned promise as its value.
#[test]
fn async_callback_into_sync_slot_wraps_future_as_promise() {
    let source = source_for(
        r"
function attempt(fn: () => unknown): unknown {
  return fn();
}

export function run(): unknown {
  return attempt(async () => 1);
}
",
    );

    assert!(
        source.contains("SmeltUnknown::Promise(SmeltPromise::from_future"),
        "an async callback flowing into a non-future value slot must be wrapped \
         as a promise\n{source}"
    );
}

/// A spread call (`fn(...values)`) must route to the variadic overload rather
/// than a fixed-arity one: the spread's runtime length is unknown, so a
/// rest-less overload cannot claim it. Mirrors es-toolkit's
/// `cartesianProduct(...inputs)`, which must return `number[][]` and not a
/// list of 1-tuples-of-lists.
#[test]
fn spread_call_selects_variadic_overload() {
    let source = source_for(
        r"
export function cartesianProduct<T>(arr1: readonly T[]): Array<[T]>;
export function cartesianProduct<T, U>(arr1: readonly T[], arr2: readonly U[]): Array<[T, U]>;
export function cartesianProduct<T>(...arrs: Array<readonly T[]>): T[][];
export function cartesianProduct<T>(...arrs: Array<readonly T[]>): T[][] {
  return arrs as any;
}

export function run(inputs: number[][]): unknown {
  return cartesianProduct(...inputs);
}
",
    );

    assert!(
        source.contains("SmeltList<SmeltList<f64>>"),
        "a spread call must select the variadic overload returning number[][]\n{source}"
    );
    assert!(
        !source.contains("SmeltList<(SmeltList<f64>,)>"),
        "the spread call must not select the fixed 1-array overload\n{source}"
    );
}

/// `expect(actual).toEqual(literal)` contextually types the literal from the
/// actual value's type, so a nested tuple-list actual and a nested array
/// literal compare at the same Rust type instead of
/// `SmeltList<SmeltList<SmeltUnknown>>` vs `SmeltList<(f64, String)>` (E0308).
#[test]
fn to_equal_contextually_types_expected_from_actual() {
    let source = source_for(
        r"
import { describe, it, expect } from 'vitest';

describe('toEqual', () => {
  it('compares nested tuple lists', () => {
    const actual: Array<[number, string]> = [[1, 'a'], [2, 'b']];
    expect(actual).toEqual([[1, 'a'], [2, 'b']]);
  });
});
",
    );

    // The expected literal's rows lower as (f64, String) tuples, matching the
    // actual, rather than erased inner lists.
    assert!(
        !source.contains("SmeltList<SmeltList<SmeltUnknown>>"),
        "the expected literal must be typed from the actual, not erased\n{source}"
    );
}

/// A fixed-arity user callback (`(item, item2, item3) => ...`) supplied where
/// a variadic `(...args: T[]) => R` is expected must type each fixed parameter
/// as the rest *element* `T`, not the rest list `T[]`. Otherwise `item` is a
/// `SmeltList` and arithmetic on it fails (E0369). Mirrors es-toolkit's
/// `unzipWith`.
#[test]
fn fixed_params_against_variadic_hint_take_element_type() {
    let source = source_for(
        r"
function unzipWith<T, R>(target: readonly T[][], iteratee: (...args: T[]) => R): R[] {
  return target.map(group => iteratee(...group));
}

export function run(zipped: Array<[number, number, number]>): number[] {
  return unzipWith(zipped, (item, item2, item3) => item + item2 + item3);
}
",
    );

    // The three-parameter user iteratee must not receive a list as its first
    // fixed parameter; each fixed param takes the rest element type.
    assert!(
        !source.contains(
            "|closure_arg_0: SmeltList<SmeltUnknown>, closure_arg_1: SmeltUnknown, closure_arg_2: SmeltUnknown|"
        ),
        "a fixed callback param against a variadic hint must not be a list\n{source}"
    );
}

/// A list of tuples flowing into a list-of-lists target (e.g. a `zip` result
/// passed to `unzipWith`'s `readonly T[][]` with `T` erased) must coerce each
/// tuple into a `SmeltList`, not pass it through unchanged (E0308).
#[test]
fn tuple_coerces_into_list_target() {
    let source = source_for(
        r"
function sink(rows: unknown[][]): void {}

export function run(pairs: Array<[number, number]>): void {
  sink(pairs);
}
",
    );

    assert!(
        source.contains("SmeltList::with_id(smelt_next_object_id(), vec!["),
        "a tuple flowing into a list target must be rebuilt as a SmeltList\n{source}"
    );
}

#[test]
fn callable_object_construction_emits_typed_interface_struct_literal() {
    // Callable-object construction: a function-typed local that collects
    // `counter.reset = …` writes and is returned at a callable-interface type
    // must emit a real struct literal carrying the base callable in
    // `__smelt_call` and each collected property in its like-named field — never
    // a `Default::default()` inert struct that drops the writes.
    let source = source_for(
        r"
interface Counter {
  (): number;
  reset(): void;
}
export function makeCounter(): Counter {
  let count = 0;
  const counter = function (): number {
    count = count + 1;
    return count;
  };
  counter.reset = function (): void {
    count = 0;
  };
  return counter;
}
",
    );
    let body = source
        .split("fn make_counter")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or("");
    assert!(
        body.contains("Counter {"),
        "callable construction must build a Counter struct literal: {source}"
    );
    assert!(
        body.contains("__smelt_call:"),
        "the base callable must fill the __smelt_call field: {source}"
    );
    assert!(
        body.contains("reset:"),
        "the collected property must fill the reset field: {source}"
    );
    assert!(
        !body.contains("Default::default()"),
        "callable construction must not fall back to an inert Default struct: {source}"
    );
}





/// Regression (warning-reduction R1): a read-only collection capture in a
/// callback prelude is cloned into a plain, non-`mut` binding. The capture
/// prelude used to force `mut` on every list/set/dict capture regardless of
/// use; mutability now follows `closure_capture_body_writes` only.
#[test]
fn read_only_list_capture_emits_non_mut_clone() {
    let source = source_for(
        r"
export function useCapture(values: number[], extra: number[]): number[] {
  return values.map((n: number): number => n + extra.length);
}
",
    );
    assert!(
        source.contains("let extra = extra.clone();"),
        "read-only list capture must clone into a non-mut binding: {source}"
    );
    assert!(
        !source.contains("let mut extra = extra.clone();"),
        "read-only list capture must not be spuriously mut: {source}"
    );
}

/// Regression (warning-reduction R1): a captured collection that the enclosing
/// function still reassigns (so its source local is mutable) keeps a `mut`
/// clone binding. Guards against under-approximating mutability after the
/// blanket collection rule was dropped.
#[test]
fn mutable_source_list_capture_keeps_mut_clone() {
    let source = source_for(
        r"
export function f(items: number[]): number {
  let acc = items;
  acc = items;
  return [1].map((x: number): number => acc.length + x).length;
}
",
    );
    assert!(
        source.contains("let mut acc = acc.clone();"),
        "a capture whose source local is reassigned must stay a mut clone: {source}"
    );
}

/// Regression (warning-reduction R1): a captured collection that the closure
/// body mutates in place is threaded through shared `RefCell` storage (so the
/// mutation is caller-visible) rather than being silently emitted as a
/// read-only clone. Guards the write-detection path.
#[test]
fn written_list_capture_uses_shared_storage_not_plain_clone() {
    let source = source_for(
        r"
export function g(): number[] {
  const store: number[] = [];
  const add = (x: number): void => { store.push(x); };
  [1, 2, 3].forEach(add);
  return store;
}
",
    );
    assert!(
        source.contains("smelt_capture_store")
            && source.contains("(*smelt_capture_store.borrow()).borrow_mut().push("),
        "a mutated captured list must route writes through shared storage: {source}"
    );
    assert!(
        !source.contains("let store = store.clone();"),
        "a mutated captured list must not be emitted as a read-only clone: {source}"
    );
}

/// Regression (warning-reduction R1): an adapted callback that is only *called*
/// and *cloned* binds without `mut`, matching the erased `.call` path. The
/// binding used to be hardcoded `let mut _smelt_adapted_callback`.
#[test]
fn adapted_callback_binds_without_mut() {
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
        source.contains("let _smelt_adapted_callback = callback.clone();"),
        "the adapted callback must bind without mut: {source}"
    );
    assert!(
        !source.contains("let mut _smelt_adapted_callback"),
        "the adapted callback must not be spuriously mut: {source}"
    );
}

/// Erasing a function returned by an adapted callback must evaluate that
/// callback once; evaluating it again moves non-Copy arguments twice.
#[test]
fn adapted_function_return_is_materialized_before_identity_registration() {
    let source = source_for(
        r"
function adapt(
  callback: (key: string) => (record: Record<string, number>) => number,
): (key: string) => unknown {
  return callback;
}
",
    );

    // The adapted callable is materialized (called) before any identity or
    // callable-object bookkeeping. A callable value narrowed from an object is
    // re-erased back to that object when a registration exists; otherwise the
    // origin is registered so the typed callback survives the erased ABI.
    assert!(
        source.contains(
            "let smelt_function_value = (_smelt_adapted_callback)(arg0); if let Some(smelt_callable_object) = smelt_lookup_callable_object(&smelt_function_value) { smelt_callable_object } else { let smelt_function_origin = smelt_function_value.clone();"
        ),
        "{source}"
    );
}

/// Regression (warning-reduction R1): a predeclared (hoisted) local that is
/// assigned inside a loop body keeps its `mut` binding. Rust's
/// definite-assignment rules reject reassigning an immutable hoisted local from
/// a loop body, so the repeating-region rule must still fire here even after it
/// was scoped to predeclared bindings.
#[test]
fn hoisted_local_assigned_in_loop_keeps_mut() {
    let source = source_for(
        r"
export function lastVal(items: number[]): number {
  let found: number = 0;
  for (const x of items) {
    found = x;
  }
  return found;
}
",
    );
    assert!(
        source.contains("let mut found"),
        "a hoisted local reassigned in a loop must stay mut: {source}"
    );
}

/// A `splice` whose start index is an `i64` (e.g. a callback index parameter)
/// is coerced to `f64`. The coerced cast must be parenthesized so
/// `index as f64 < 0.0` does not parse as `index as (f64 < 0.0)` — rustc reads
/// the `<` as the start of generic arguments after a type. Regression for the
/// remeda `range`/`splice` E0742-style `<`-parse failure.
#[test]
fn splice_index_cast_is_parenthesized_before_comparison() {
    let source = source_for(
        r"
export function trimLast(items: number[]): number[] {
  [0].forEach((_, i) => {
    items.splice(i, 1);
  });
  return items;
}
",
    );
    assert!(
        source.contains("splice_start = if ("),
        "the coerced splice index must be parenthesized before `< 0.0`: {source}"
    );
    assert!(
        !source.contains("as f64 < 0.0"),
        "a bare `x as f64 < 0.0` mis-parses as generic arguments: {source}"
    );
}

/// A `Set.has(needle)` whose needle is an `i64` (a callback index) coerces the
/// needle to the set's `f64` element type. The reference taken for `contains`
/// must wrap the whole coercion (`&(x as f64)`); the buggy `&x as f64` casts a
/// reference (`&i64 as f64`), which is invalid. Regression for remeda `sample`.
#[test]
fn set_has_coerced_needle_reference_is_parenthesized() {
    let source = source_for(
        r"
export function pickByIndex(values: number[]): number[] {
  const seen = new Set<number>([1, 2, 3]);
  return values.filter((_, i) => seen.has(i));
}
",
    );
    assert!(
        source.contains(".contains(&("),
        "a coerced set needle must be referenced as `&(x as f64)`: {source}"
    );
}
#[test]
fn async_closure_await_propagates_future_errors() {
    let source = source_for(
        r"
async function load(): Promise<number> {
  return 1;
}

function retain(callback: () => Promise<number>): () => Promise<number> {
  return callback;
}

export function callback(): () => Promise<number> {
  return retain(async () => {
    const value = await load();
    return value;
  });
}
",
    );

    assert!(
        source.contains(": f64 = _smelt_tmp_0.await?;")
            || source.contains(": f64 = _smelt_tmp_1.await?;")
            || source.contains(": f64 = _smelt_tmp_2.await?;")
            || source.contains("_smelt_tmp_2 = _smelt_tmp_1.await?;"),
        "{source}"
    );
}

/// A nested closure whose parameter shadows an outer callback parameter must
/// initialize the capture alias referenced by the generated inner body.
#[test]
fn nested_closure_aliases_shadowed_callback_parameters() {
    let source = source_for(
        r"
export function run(): number {
  const objectize =
    (callback: (value: { num: number }) => number) =>
    (num: number) => callback({ num });
  return objectize(value => value.num)(1);
}
",
    );

    assert!(
        source.contains("let smelt_captured_closure_arg_0 = closure_arg_0.clone();"),
        "{source}"
    );
}

/// An async nested closure must carry the outer capture alias into its future
/// instead of reinitializing it from the shadowing inner parameter.
#[test]
fn async_nested_closure_preserves_shadowed_capture_alias() {
    let source = source_for(
        r"
export async function run(): Promise<number> {
  const objectize =
    (callback: (value: { num: number }) => Promise<number>) =>
    async (num: number) => callback({ num });
  return await objectize(async value => value.num)(1);
}
",
    );

    assert!(
        source.contains(
            "move |closure_arg_0: f64| { let smelt_captured_closure_arg_0 = smelt_captured_closure_arg_0.clone();"
        ),
        "{source}"
    );
}

/// A zero-parameter async closure returned from a factory remains reusable:
/// each call clones the factory callback before moving it into a fresh future.
#[test]
fn zero_parameter_async_factory_clones_captured_callback_per_call() {
    let source = source_for(
        r"
export async function run(): Promise<number> {
  const factory = (callback: (value: number) => Promise<number>) =>
    async () => callback(0);
  const callback = async (value: number): Promise<number> => value;
  const generated = factory(callback);
  return (await generated()) + (await generated());
}
",
    );

    assert!(
        source.contains(
            "move || { let closure_arg_0 = closure_arg_0.clone(); SmeltFuture::from_future"
        ),
        "{source}"
    );
}

/// A mutable outer parameter whose synthetic name is shadowed needs fresh
/// shared capture storage initialized from that outer parameter.
#[test]
fn mutable_shadowed_parameter_initializes_aliased_capture_cell() {
    let source = source_for(
        r"
export function run(): void {
  const append =
    (items: number[]) =>
    (callback: (value: number[]) => void) => {
      items.push(1);
      callback(items);
    };
  append([])(_items => {});
}
",
    );

    assert!(
        source.contains(
            "let smelt_capture_smelt_captured_closure_arg_0 = ::std::rc::Rc::new(::std::cell::RefCell::new(closure_arg_0.clone()));"
        ),
        "{source}"
    );
}

#[test]
fn erased_apply_call_bind_function_receiver() {
    // `func.apply`/`func.call` on an erased receiver must resolve when `func` is
    // a `SmeltUnknown::Function`, not only when it is an object. The plain
    // object-field read returns `Undefined` for a function receiver, which
    // collapses every invocation to a null callback (the `partial`/`partialRight`
    // regression). The read routes through `smelt_function_method`, which binds
    // the callable with `this`-dropping/argument-spreading semantics.
    let source = source_for(
        r"
export function invokeApply(func: any, args: unknown[]): unknown {
  return func.apply(undefined, args);
}
export function invokeCall(func: any, first: unknown): unknown {
  return func.call(undefined, first);
}
",
    );

    assert!(
        source.contains("smelt_function_method(") && source.contains("\"apply\""),
        "erased `.apply` must route through the function-method binder\n{source}"
    );
    assert!(
        source.contains("\"call\""),
        "erased `.call` must route through the function-method binder\n{source}"
    );
    assert!(
        source.contains("fn smelt_function_method("),
        "the runtime prelude must define the function-method binder\n{source}"
    );
}

#[test]
fn erased_rest_adapter_packs_target_arguments() {
    // Adapting an erased-rest (`SmeltErasedFunction`) source to a fixed-arity
    // target must pack every positional target argument into the erased rest
    // list, forwarding all of them. A per-source-parameter mapping instead
    // coerces `arg0` *into* the list — spreading its elements and dropping
    // `arg1` — which panics on non-iterable values and corrupts multi-argument
    // adapters (the `partial`/`partialRight` two-argument case; the text-path
    // variant of this adapter is exercised end-to-end by the es-toolkit
    // `partial`/`partialRight` suites).
    let source = source_for(
        r"
type ErasedBinary = (a: unknown, b: unknown) => unknown;
function makeErased<R>(f: (...args: unknown[]) => R): (...args: unknown[]) => R {
  return (...args: unknown[]) => f(...args);
}
export function adapt(f: (...args: unknown[]) => unknown): ErasedBinary {
  return makeErased(f);
}
",
    );

    // Both target arguments are forwarded into the erased rest list, in order.
    assert!(
        source.contains("smelt_forwarded_args.push(arg0.clone());")
            && source.contains("smelt_forwarded_args.push(arg1.clone());"),
        "erased-rest adapter must forward every target argument, not drop the tail\n{source}"
    );
    // The scalar target arguments must not be spread as if each were the whole
    // argument list (the corrupting behavior the fix removes).
    assert!(
        !source.contains("panic!(\"unknown is not iterable\")"),
        "erased-rest adapter must not spread a scalar target argument\n{source}"
    );
}

#[test]
fn vitest_mock_chain_constructs_one_stateful_mock() {
    // A `vi.fn().mockRejectedValueOnce(..).mockResolvedValue(..)` chain must
    // construct exactly ONE runtime mock: the chain methods are runtime fields
    // returning the same instance, and HIR interceptor probing's dangling
    // duplicate exprs must never be materialized by MIR.
    let source = source_for(
        r#"
import { it, vi } from "vitest";

it("configures a chain", () => {
  const func = vi
    .fn()
    .mockRejectedValueOnce(new Error("failure"))
    .mockResolvedValue("success");
  func();
});
"#,
    );
    // Count only the emitted PROGRAM, not the prelude: the prelude both defines
    // the helper and calls it from the spy adapter, so a whole-source count
    // moves whenever the runtime grows another caller.
    let program = source
        .split_once("@smelt:prelude-end")
        .map_or(source.as_str(), |(_, program)| program);
    assert_eq!(
        program.matches("smelt_vitest_mock_new(").count(),
        1,
        "chain must construct exactly one mock\n{source}"
    );
    // The gated mock prelude and its `SmeltPromise::rejected` dependency are
    // both emitted for mock-bearing crates.
    assert!(source.contains("struct SmeltVitestMockState"), "{source}");
    assert!(source.contains("fn rejected(value: SmeltUnknown)"), "{source}");
}

#[test]
fn vitest_mock_prelude_is_pay_for_use() {
    // A crate with no `vi.fn()` mock keeps a byte-identical prelude: no mock
    // registry, no `SmeltPromise::rejected`.
    let source = source_for(
        r"
export function double(value: number): number {
  return value * 2;
}
",
    );
    assert!(!source.contains("SmeltVitestMockState"), "{source}");
    assert!(!source.contains("fn rejected("), "{source}");
}

#[test]
fn vitest_called_times_and_called_with_are_real_assertions() {
    // `toHaveBeenCalledTimes` / `toHaveBeenCalledWith` must lower to real
    // failure paths reading the mock's recorded state — a count/argument
    // mismatch returns a test error instead of passing vacuously.
    let source = source_for(
        r#"
import { expect, it, vi } from "vitest";

it("asserts calls", () => {
  const spy = vi.fn();
  spy(1, "a");
  expect(spy).toHaveBeenCalledTimes(1);
  expect(spy).toHaveBeenCalledWith(1, "a");
});
"#,
    );
    assert!(
        source.contains("smelt_vitest_mock_called_times("),
        "{source}"
    );
    assert!(
        source.contains("smelt_vitest_mock_called_with("),
        "{source}"
    );
    assert!(
        source.contains("expect(...).toHaveBeenCalledTimes(...) failed"),
        "count mismatch must produce a test failure\n{source}"
    );
    assert!(
        source.contains("expect(...).toHaveBeenCalledWith(...) failed"),
        "argument mismatch must produce a test failure\n{source}"
    );
}

#[test]
fn a_call_assertion_about_a_non_mock_actual_is_false() {
    // The mock matchers used to answer `true` for an actual carrying no
    // `__smelt_vitest_mock` marker, back when `vi.spyOn` lowered to a plain
    // placeholder and failing it would have failed suites Smelt could not model.
    // A spy is a real mock now, so the honest answer is the one JavaScript
    // implies: a value that is not a mock has recorded no calls, so it was not
    // called with anything and its last result resolved to nothing. Only
    // `toHaveBeenCalledTimes(0)` still holds.
    let source = source_for(
        r#"
import { expect, it, vi } from "vitest";

it("asserts calls", () => {
  const spy = vi.fn();
  spy(1);
  expect(spy).toHaveBeenCalledTimes(1);
});
"#,
    );
    assert!(
        source.contains(
            "fn smelt_vitest_mock_called_times(value: &SmeltUnknown, expected: f64) -> bool { match smelt_vitest_mock_state(value) { Some(state) => state.borrow().calls.len() as f64 == expected, None => expected == 0.0 } }"
        ),
        "a non-mock has zero recorded calls: {source}"
    );
    assert!(
        !source.contains("state.calls.iter().any(call_matches) } }, None => true } }"),
        "a non-mock must not satisfy toHaveBeenCalledWith: {source}"
    );
    assert!(
        source.contains("state.calls.iter().any(call_matches) } }, None => false } }"),
        "a non-mock must not satisfy toHaveBeenCalledWith: {source}"
    );
}

#[test]
fn vitest_last_called_with_lowers_to_last_call_check() {
    // `toHaveBeenLastCalledWith` compares only the most recent recorded call,
    // so it lowers to `smelt_vitest_mock_called_with(.., last=true)`.
    let source = source_for(
        r#"
import { expect, it, vi } from "vitest";

it("asserts last call", () => {
  const spy = vi.fn();
  spy(1);
  spy(2);
  expect(spy).toHaveBeenLastCalledWith(2);
});
"#,
    );
    assert!(
        source.contains("smelt_vitest_mock_called_with(") && source.contains(", true)"),
        "last-call matcher must pass last=true\n{source}"
    );
    assert!(
        source.contains("expect(...).toHaveBeenLastCalledWith(...) failed"),
        "last-call mismatch must produce a test failure\n{source}"
    );
}

#[test]
fn vitest_last_resolved_with_lowers_to_result_check() {
    // `toHaveLastResolvedWith` reads the mock's recorded return value, flattening
    // a resolved promise before deep-equality.
    let source = source_for(
        r#"
import { expect, it, vi } from "vitest";

it("asserts resolved", async () => {
  const spy = vi.fn(async () => 5);
  await spy();
  expect(spy).toHaveLastResolvedWith(5);
});
"#,
    );
    assert!(
        source.contains("smelt_vitest_mock_last_resolved_with("),
        "resolved matcher must lower to the runtime helper\n{source}"
    );
    assert!(
        source.contains("expect(...).toHaveLastResolvedWith(...) failed"),
        "resolved mismatch must produce a test failure\n{source}"
    );
}

#[test]
fn vitest_mock_dot_calls_synthesizes_recorded_activity() {
    // `mockFn.mock.calls` is not an own field of the erased mock object; reading
    // `.mock` must synthesize the recorded activity from the live registry state
    // so `mockFn.mock.calls.length` flows through the ordinary array-length path.
    let source = source_for(
        r#"
import { expect, it, vi } from "vitest";

it("reads mock.calls", () => {
  const spy = vi.fn();
  spy();
  expect(spy.mock.calls.length).toBe(1);
});
"#,
    );
    assert!(
        source.contains("if field == \"mock\""),
        "`.mock` accessor must be synthesized in smelt_get_object_field\n{source}"
    );
}

/// A closure whose body has an `if` guard that mutates captured locals
/// (`once`: `if (!called) { ret = fn(); called = true; }`) must keep the guard.
/// The compact side-effect-free callback IR modeled the guarded assignment as a
/// ternary arm and hoisted the assignment out of the branch, so `fn()` ran on
/// every call and `called` was always set. The mutation must fall back to full
/// closure-body lowering, which emits a real `if` around the captured-state
/// writes.
#[test]
fn closure_if_guard_mutating_captured_locals_keeps_the_branch() {
    let source = source_for(
        r"
export function once<T>(fn: () => T): () => T {
  let called = false;
  let ret: T;
  return () => {
    if (!called) {
      ret = fn();
      called = true;
    }
    return ret;
  };
}
",
    );

    // The guard survives: `fn()` and the `called = true` write live inside an
    // `if` branch, not hoisted ahead of the condition.
    let called_write = source.find("borrow_mut()) = true").unwrap_or_else(|| {
        panic!("expected captured `called = true` write\n{source}");
    });
    let guard = source.find("if ").unwrap_or_else(|| {
        panic!("expected an `if` guard in the closure body\n{source}");
    });
    assert!(
        guard < called_write,
        "the captured-state write must sit inside the `if` guard, not be hoisted before it\n{source}"
    );
    // The invocation must not run unconditionally ahead of the condition: the
    // condition (`!called`) must be computed before the call site.
    let cond = source.find("= !").unwrap_or_else(|| {
        panic!("expected the `!called` condition\n{source}");
    });
    let call = source.find(")()").unwrap_or_else(|| {
        panic!("expected the `fn()` invocation\n{source}");
    });
    assert!(
        cond < call,
        "the `!called` condition must be evaluated before `fn()` is invoked\n{source}"
    );
}

/// An object rest pattern (`const { a, ...rest } = source`) whose source is a
/// named object type that erases to `SmeltUnknown` (here `Handle`, an interface)
/// must copy the source's remaining members into `rest`. Previously only a
/// native `Dict` or a literally `Type::Unknown` source was copied, so a named
/// object type fell through to `Default::default()` and produced an empty rest,
/// dropping spread-out members (the `cancel`/`flush` of an `Object.assign`-
/// wrapped funnel). The copy must route through `into_smelt_unknown()`.
#[test]
fn object_rest_copies_named_object_source_that_erases_to_unknown() {
    let source = source_for(
        r"
interface Handle {
  readonly call: () => void;
  readonly cancel: () => void;
  readonly flush: () => void;
}
declare function make(): Handle;
export function wrap(): Record<string, unknown> {
  const { call, ...rest } = make();
  call();
  return rest;
}
",
    );

    // The rest copy is materialized from the erased object form, not an empty
    // record.
    assert!(
        source.contains(".into_smelt_unknown() { SmeltUnknown::Object(map) => SmeltRecord::with_id_from_entries(map.id, map.into_iter())"),
        "object rest must copy the erased source object, not Default::default()\n{source}"
    );
    // The extracted key is still removed from the copied rest.
    assert!(
        source.contains(".remove(&\"call\".to_owned())"),
        "the destructured `call` key must be removed from rest\n{source}"
    );
}

/// Returns the generated program below the fixed runtime prelude.
///
/// The emitter writes a `// @smelt:prelude-end` sentinel between the shared
/// runtime prelude (which contains its own `loop`s and helpers) and the lowered
/// program, so structural assertions that count Rust constructs must look only
/// at the program half.
fn program_body(source: &str) -> &str {
    source
        .split_once("// @smelt:prelude-end")
        .map_or(source, |(_, program)| program)
}

#[test]
fn closure_loop_body_branch_is_not_treated_as_a_nested_loop_header() {
    // A `for` loop inside a closure body whose body contains a plain `if`/`else`
    // must emit exactly one Rust `loop`. The closure emitter used to decide
    // "this switch block is a loop header" with plain reachability, but inside an
    // open loop every body block reaches itself around the back edge, so the
    // nested `if` was wrapped in its own spurious `loop` and the real latch then
    // back-edged into an already-active block, replacing the loop's continue edge
    // with `panic!("recursive closure control flow is not structured yet")`.
    // es-toolkit's `flatten` (and every `flatMap*`/`flatten*` built on it) aborted
    // at runtime because of this.
    let source = source_for(
        r"
export function run(items: number[]): number[] {
  const out: number[] = [];
  const walk = (values: number[]) => {
    for (let i = 0; i < values.length; i++) {
      if (values[i] > 0) {
        out.push(values[i]);
      } else {
        out.push(0);
      }
    }
  };
  walk(items);
  return out;
}
",
    );

    assert!(
        !source.contains("closure control flow is not structured yet"),
        "a for-loop with a branching body must stay structured: {source}"
    );
    let program = program_body(&source);
    assert_eq!(
        program.matches("loop {").count(),
        1,
        "only the `for` header may become a Rust `loop`: {source}"
    );
}

#[test]
fn closure_loop_with_short_circuit_condition_emits_single_loop() {
    // Same defect through the other shape that made it fire in es-toolkit's
    // compat `orderBy`/`sortBy`: a short-circuit (`&&`) inside the loop body
    // lowers to a join block that reconverges before the branch. That join block
    // is reachable from itself only via the enclosing back edge, so it must not
    // be promoted to a loop header of its own.
    let source = source_for(
        r"
export function firstBig(values: number[], limit: number): number {
  const pick = (input: number[]) => {
    for (let i = 0; i < input.length; i++) {
      if (input[i] > limit && input[i] < 100) {
        return input[i];
      }
    }
    return -1;
  };
  return pick(values);
}
",
    );

    assert!(
        !source.contains("closure control flow is not structured yet"),
        "a short-circuit condition inside a closure loop must stay structured: {source}"
    );
    let program = program_body(&source);
    assert_eq!(
        program.matches("loop {").count(),
        1,
        "the `&&` join block must not become a second Rust `loop`: {source}"
    );
}

/// A derived constructor's `super(...)` must actually run the base constructor
/// against the derived `this`.
///
/// Rust has no inheritance, so the base's fields are flattened into the derived
/// struct; nothing initializes them unless the `super(...)` lowering constructs
/// the base and moves its fields across. Before the fix the call was dropped and
/// `Child::new` returned a struct still holding the Rust type defaults.
#[test]
fn derived_constructor_super_call_runs_the_base_constructor() {
    let source = source_for(
        r"
class Base {
  message: string;
  constructor(message: string) {
    this.message = message;
  }
}
class Child extends Base {
  constructor(message: string) {
    super(message);
  }
}
const child = new Child('hello');
",
    );

    assert!(source.contains("Base::new("), "{source}");
    assert!(
        source.contains("this.message = __smelt_super.message"),
        "{source}"
    );
}

/// `super(...)` chains through every inheritance level.
///
/// Each level reproduces only its immediate base, so a three-level chain must
/// still initialize the base-most field: `C::new` constructs `B`, whose
/// constructor constructs `A`.
#[test]
fn derived_constructor_super_call_composes_across_inheritance_levels() {
    let source = source_for(
        r"
class A {
  a: number;
  constructor() {
    this.a = 1;
  }
}
class B extends A {
  b: number;
  constructor() {
    super();
    this.b = 2;
  }
}
class C extends B {
  c: number;
  constructor() {
    super();
    this.c = 3;
  }
}
const value = new C();
",
    );

    assert!(source.contains("let __smelt_super: A = "), "{source}");
    assert!(source.contains("B::new()"), "{source}");
    // `C::new` copies BOTH inherited slots out of the constructed `B`, so the
    // base-most field reaches the leaf instance.
    assert!(
        source.contains("this.a = __smelt_super.a") && source.contains("this.b = __smelt_super.b"),
        "{source}"
    );
}

/// A class extending a host `Error` constructor gets the base constructor's
/// observable behaviour: `message` from the first argument and `name` from the
/// base constructor's own name.
///
/// The rule is keyed on the resolved base type, not on the subclass name, so
/// the same lowering covers `extends Error` and every standard error subclass.
#[test]
fn error_subclass_super_call_assigns_the_error_base_slots() {
    let source = source_for(
        r"
class ParseFailure extends TypeError {
  constructor(message: string) {
    super(message);
  }
}
const failure = new ParseFailure('bad input');
",
    );

    assert!(
        source.contains(r#"this.name = "TypeError".to_owned()"#),
        "{source}"
    );
    assert!(source.contains("this.message = message"), "{source}");
}

/// The inherited `Error` slots keep concrete types; only `cause` erases.
///
/// TypeScript's lib types are `name: string`, `message: string`,
/// `stack?: string`, `cause?: unknown`, and `tsc` rejects source that violates
/// them before Smelt runs, so three of the four slots are ordinary strings
/// rather than dynamic boundaries. This guards against a future change
/// "consistently" re-erasing them to `SmeltUnknown`.
#[test]
fn error_subclass_inherited_slots_stay_concretely_typed() {
    let source = source_for(
        r"
class ParseFailure extends TypeError {
  constructor(message: string) {
    super(message);
  }
}
const failure = new ParseFailure('bad input');
",
    );

    assert!(source.contains("name: String,"), "{source}");
    assert!(source.contains("message: String,"), "{source}");
    assert!(source.contains("stack: Option<String>,"), "{source}");
    // `cause` is the one genuine dynamic boundary: ES2022 types it `unknown`
    // because it carries an arbitrary thrown value.
    assert!(source.contains("cause: SmeltUnknown,"), "{source}");
    assert!(
        !source.contains("message: SmeltUnknown,") && !source.contains("name: SmeltUnknown,"),
        "{source}"
    );
    // A concrete `String` slot must still erase to `SmeltUnknown::String` when the
    // instance is erased, because the throw path reads `message` off the erased
    // object and expects that variant.
    assert!(
        source.contains(r#"("message".to_owned(), SmeltUnknown::String(self.message.into()))"#),
        "{source}"
    );
}

/// Constructor parameter defaults are applied in the constructor body, exactly
/// like plain function parameter defaults.
///
/// The ABI parameter widens to `Option<T>` and the body applies the declared
/// initializer, so an omitted argument evaluates the source default instead of
/// the Rust type's zero value (`String::new()` before the fix).
#[test]
fn constructor_parameter_default_is_applied_in_the_body() {
    let source = source_for(
        r"
class Greeting {
  text: string;
  constructor(text = 'hi there') {
    this.text = text;
  }
}
const greeting = new Greeting();
",
    );

    assert!(source.contains("fn new(text: Option<String>)"), "{source}");
    assert!(source.contains(r#"unwrap_or("hi there".to_owned())"#), "{source}");
    assert!(source.contains("Greeting::new(None"), "{source}");
}

/// A parameter property with a default stores the applied default, not the
/// `Option` ABI slot.
///
/// The parameter-property assignment is emitted after the default prelude, so
/// the field keeps its declared concrete type.
#[test]
fn defaulted_parameter_property_stores_the_applied_default() {
    let source = source_for(
        r"
class Sized {
  constructor(readonly size = 10) {}
}
const sized = new Sized();
",
    );

    assert!(source.contains("size: f64"), "{source}");
    assert!(source.contains("unwrap_or(10.0)"), "{source}");
    assert!(source.contains("this.size = size"), "{source}");
}

#[test]
fn calling_a_callable_object_value_dispatches_through_its_call_slot() {
    // Regression (es-toolkit debounce/throttle specs): invoking a value whose
    // type is a callable interface (`debouncedFunc()` where `debouncedFunc:
    // DebouncedFunction`) lowers to a `closure_call` whose callee temporary is
    // function-typed. The record -> function coercion had no rule, so the
    // callee fell through to `default_value`, which FABRICATED an empty closure
    // and then called it: the real function never ran, no timer was scheduled,
    // and every `toHaveBeenCalledTimes` assertion failed. The call must read the
    // interface's synthetic `__smelt_call` slot instead.
    let source = source_for(
        r"
export interface Counter {
  (...args: number[]): void;
  reset: () => void;
}
export function makeCounter(): Counter {
  let count = 0;
  const reset = () => {
    count = 0;
  };
  const counter = function (...args: number[]) {
    count = count + args.length;
  };
  counter.reset = reset;
  return counter;
}
export function useCounter(): void {
  const c = makeCounter();
  c();
  c(1, 2);
}
",
    );
    assert!(
        source.contains("__smelt_call.clone()"),
        "calling a callable-object value must dispatch through its `__smelt_call` \
         slot: {source}"
    );
    // The dead-stub shape: a freshly built empty callable ASSIGNED to a
    // temporary (as opposed to a declaration-site default initializer), which is
    // then immediately invoked.
    let dead_stub_assignment = source.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("_smelt_tmp_")
            && line.contains("= { let smelt_default_callback:")
            && line.contains("SmeltList<f64>")
    });
    assert!(
        !dead_stub_assignment,
        "a callable-object call must not materialize a fabricated empty callback \
         at the call site: {source}"
    );
}

#[test]
fn apply_on_a_callable_object_invokes_its_underlying_callable() {
    // Regression (es-toolkit debounce: `func.apply(pendingThis, pendingArgs)`
    // where `func` is a vitest mock). A callable object erases to
    // `SmeltUnknown::Object` carrying a `__smelt_call` slot and has no own
    // `apply` property, so the object arm of `smelt_function_method` returned
    // `undefined` and the call silently became a no-op. `apply`/`call`/`bind`
    // are `Function.prototype` members of the underlying callable and must
    // resolve there when the object itself does not define them.
    let source = source_for(
        r"
export function applyTo(func: unknown, args: unknown[]): unknown {
  return (func as (...values: unknown[]) => unknown).apply(null, args);
}
",
    );
    assert!(
        source.contains("fn smelt_function_method("),
        "expected the Function.prototype method helper: {source}"
    );
    assert!(
        source.contains(r#"match map.get("__smelt_call")"#),
        "`smelt_function_method` must fall back to a callable object's \
         `__smelt_call` slot: {source}"
    );
}

#[test]
fn strict_null_comparison_distinguishes_a_stored_null_from_an_absent_slot() {
    // Regression (es-toolkit debounce `cancelTimer`): `timeoutId !== null` on a
    // `ReturnType<typeof setTimeout> | null` slot lowered to `Option<SmeltUnknown>`
    // answered `true` for a cleared timer, because JS `null` had been folded
    // into `None` while the strict comparison only matched a PRESENT
    // `SmeltUnknown::Null` payload. The `.expect(..)` guarded by that test then
    // panicked. JavaScript keeps `null` and `undefined` distinct under `===`, and
    // an erased `Option` payload has room for both, so an assigned `null` must be
    // stored as `Some(SmeltUnknown::Null)`.
    let source = source_for(
        r"
export function clearSlot(handle: unknown): number {
  let slot: unknown | null = null;
  slot = handle;
  slot = null;
  if (slot !== null) {
    return 1;
  }
  return 0;
}
",
    );
    assert!(
        source.contains("Some(SmeltUnknown::Null)"),
        "an assigned JS `null` must stay observable in an erased Option payload: {source}"
    );
    assert!(
        source.contains("is_some_and(|value| matches!(value, SmeltUnknown::Null))"),
        "strict `!== null` must test for a present Null payload: {source}"
    );
}

#[test]
fn byte_buffer_hosts_construct_through_the_shared_reflected_constructor() {
    // `new ArrayBuffer(8)` / `new SharedArrayBuffer(8)` / `new DataView(buf, 1, 2)`
    // all lower to `smelt_reflected_construct`, the *same* runtime constructor the
    // reflected `new Object.getPrototypeOf(x).constructor(...)` path calls. That
    // shared constructor is what makes a directly built record indistinguishable
    // from a reflectively built one — es-toolkit's `clone` uses the reflected form
    // where its `cloneDeepWith` uses the direct one, and its specs compare the two
    // results against each other.
    for (source, kind) in [
        (
            "export function f() { return new ArrayBuffer(8); }",
            "arraybuffer",
        ),
        (
            "export function f() { return new SharedArrayBuffer(8); }",
            "sharedarraybuffer",
        ),
        (
            "export function f() { return new DataView(new ArrayBuffer(8), 1, 2); }",
            "dataview",
        ),
    ] {
        let generated = source_for(source);
        assert!(
            generated.contains(&format!("smelt_reflected_construct(\"{kind}\"")),
            "expected `{source}` to construct through the shared `{kind}` host constructor:\n{generated}"
        );
    }
}

#[test]
fn erased_slice_routes_a_byte_buffer_receiver_through_the_host_slice_helper() {
    // `buffer.slice(0)` on an erased receiver must be able to answer a FRESH record
    // of the same host identity: the tag-preserving erased slice used to forward any
    // non-array receiver unchanged, so the `clone(buf) { return buf.slice(0) }`
    // shape returned its own argument and `expect(cloned).not.toBe(buffer)` failed.
    // `subarray` is the same operation and routes the same way.
    for source in [
        "export function f(value: any): any { return value.slice(0); }",
        "export function f(value: any): any { return value.subarray(); }",
    ] {
        let generated = source_for(source);
        assert!(
            generated.contains("smelt_host_buffer_slice(&smelt_slice_value"),
            "expected `{source}` to try the host byte-buffer slice first:\n{generated}"
        );
    }
}

#[test]
fn erased_reads_and_writes_reach_a_byte_buffers_bytes() {
    // A byte buffer's indexed slots are its bytes, not ordinary record properties.
    // Index reads used to miss the `bytes` storage and answer `null`, which made the
    // `a[i]` element walk deep equality performs over a typed-array-tagged value
    // compare two different buffers as equal; index writes landed in a property
    // instead of the storage, so the typed-array clone shape
    // `result[i] = clone(source[i])` produced a record with stray numeric keys.
    // `o[k]` and `o.k` are one JavaScript operation, so the erased index read
    // resolves its OBJECT arm through the same `smelt_get_object_field` helper
    // the erased field read uses rather than inlining its own lookup; the byte
    // -buffer attempt lives inside that helper, asserted just below.
    let indexed = source_for("export function f(value: any): any { return value[1]; }");
    assert!(
        indexed.contains("SmeltUnknown::Object(values) => smelt_get_object_field(&values"),
        "an erased index read must resolve an object through the field helper:\n{indexed}"
    );
    let field = source_for("export function f(value: any): any { return value.length; }");
    assert!(
        field.contains("smelt_host_buffer_element(map, field)"),
        "the erased field read helper must try the byte-buffer element first:\n{field}"
    );
    let assign = source_for("export function f(value: any): void { value[1] = 2; }");
    assert!(
        assign.contains("smelt_host_buffer_set_element(map, &key, value.clone())"),
        "an erased index write must offer the byte storage the write first:\n{assign}"
    );
}

#[test]
fn a_dynamic_global_object_read_resolves_a_modeled_builtin_constructor() {
    // `globalThis.Error` normalizes to the modeled constructor at lowering time,
    // but the SAME read spelled with a runtime key used to fold to a constant
    // `undefined`, so `new (globalThis[type])(msg)` fabricated a null-returning
    // closure call and every error it "constructed" compared equal to every
    // other. Both spellings now resolve against one registry: the read reaches
    // the global-object marker record, and the record answers a modeled
    // constructor name with the interned builtin namespace value.
    let source = source_for(
        "export function f(name: any): any { return (globalThis as any)[name]; }",
    );
    assert!(
        source.contains("__smelt_global_object"),
        "a dynamic global read must reach the global-object value:\n{source}"
    );
    assert!(
        source.contains(
            "if map.contains_key(\"__smelt_global_object\") && !map.contains_key(field) && smelt_builtin_construct_kind(field).is_some()"
        ),
        "the global object must resolve a modeled builtin constructor by name:\n{source}"
    );
    // A name this profile models no constructor for stays genuinely absent
    // rather than becoming a fabricated empty namespace record.
    assert!(
        source.contains("smelt_builtin_namespace(field)"),
        "the resolved value must be the interned namespace value:\n{source}"
    );
}

#[test]
fn an_optional_erased_index_read_resolves_synthesized_properties() {
    // `o?.[k]` reads the object arm through the same helper `o.k` uses, so a
    // marker record's synthesized properties (an error's `name`, a Map's `size`,
    // the global object's constructors) are visible to both spellings. An
    // own-field-only `values.get(..)` was blind to every one of them.
    let source = source_for(
        "export function f(value: any, key: string): any { return value?.[key] ?? 1; }",
    );
    assert!(
        source.contains("smelt_get_object_field(&values"),
        "an optional erased index read must use the field helper:\n{source}"
    );
}

#[test]
fn an_absent_erased_slot_defaults_to_undefined_not_null() {
    // `Default::default()` is what every ABSENT erased slot falls back to: an
    // out-of-range element read, a `resize` fill, a `new Array(n)` hole.
    // JavaScript answers `undefined` for all of them; `null` is a value a
    // program has to store deliberately, so defaulting to it made a hole and a
    // stored `null` indistinguishable.
    let source = source_for("export function f(value: any): any { return value; }");
    assert!(
        source.contains("impl Default for SmeltUnknown"),
        "the erased default impl must be emitted:\n{source}"
    );
    let default_impl = source
        .split("impl Default for SmeltUnknown")
        .nth(1)
        .unwrap_or_default();
    assert!(
        default_impl.contains("Self::Undefined"),
        "an absent erased slot must default to `undefined`:\n{default_impl}"
    );
}

#[test]
fn a_deep_equality_matcher_on_a_class_compares_structurally() {
    // Vitest `toEqual` compares own enumerable properties; only `toBe` is
    // identity. A class with reference semantics gets `PartialEq = Rc::ptr_eq`,
    // so lowering `toEqual` to a plain `!=` on the class type asked "is this the
    // same object" — unsatisfiable for any freshly built value, which is why
    // `expect(clone(err)).toEqual(err)` could never pass. Erasing both operands
    // makes the comparison `SmeltUnknown`'s structural walk.
    let source = source_for(
        "import { describe, expect, it } from 'vitest';\n\
         class Point { x: number; constructor(x: number) { this.x = x; } move(): Point { return new Point(this.x); } }\n\
         describe('p', () => { it('eq', () => { const a = new Point(1); expect(a.move()).toEqual(a); }); });",
    );
    assert!(
        source.contains("into_smelt_unknown()"),
        "a class-typed deep-equality comparison must erase both operands:\n{source}"
    );
    assert!(
        !source.contains("a.clone() != a.clone()"),
        "a class-typed deep-equality comparison must not compare by identity:\n{source}"
    );
}

#[test]
fn arraybuffer_is_view_covers_every_registry_view_kind() {
    // `ArrayBuffer.isView(x)` is true for a *view* over byte storage and false for
    // the storage itself, which is exactly the `ByteBufferRole::View` split in the
    // shared host registry. es-toolkit defines `isTypedArray` as
    // `ArrayBuffer.isView(x) && !(x instanceof DataView)`, so a Node `Buffer` (a
    // `Uint8Array` subclass) has to answer `true` here or its clone path is never
    // taken. Checking only `DataView` left `isTypedArray(Buffer.from([1]))` false.
    let generated =
        source_for("export function f(value: any): boolean { return ArrayBuffer.isView(value); }");
    for marker in ["__smelt_buffer", "__smelt_dataview"] {
        assert!(
            generated.contains(&format!("value.contains_key(\"{marker}\")")),
            "`ArrayBuffer.isView` must recognize `{marker}`:\n{generated}"
        );
    }
    assert!(
        !generated.contains("matches!(value.clone().clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_arraybuffer\"))"),
        "`ArrayBuffer.isView` must stay false for byte *storage*:\n{generated}"
    );
}

#[test]
fn typed_array_views_each_report_their_own_spec_tag() {
    // All eleven views used to share one `Vec<f64>`, so every one of them reported
    // `[object Array]` — the same tag a plain `number[]` reports. Two views over
    // the same buffer were then indistinguishable, which is what makes
    // `isEqualWith(new Float32Array(buf), new Float64Array(buf))` wrongly `true`.
    // Length alone cannot separate them either: `Uint8Array` and
    // `Uint8ClampedArray` over one buffer have the *same* element count, so those
    // two need genuinely distinct tags.
    let generated = source_for(
        "export function f(value: any): string { return Object.prototype.toString.call(value); }",
    );
    for (marker, tag) in [
        ("__smelt_int8array", "Int8Array"),
        ("__smelt_uint8array", "Uint8Array"),
        ("__smelt_uint8clampedarray", "Uint8ClampedArray"),
        ("__smelt_int16array", "Int16Array"),
        ("__smelt_uint16array", "Uint16Array"),
        ("__smelt_int32array", "Int32Array"),
        ("__smelt_uint32array", "Uint32Array"),
        ("__smelt_float32array", "Float32Array"),
        ("__smelt_float64array", "Float64Array"),
        ("__smelt_bigint64array", "BigInt64Array"),
        ("__smelt_biguint64array", "BigUint64Array"),
    ] {
        assert!(
            generated.contains(&format!(
                "if map.contains_key(\"{marker}\") {{ return \"[object {tag}]\""
            )),
            "`{marker}` must tag as `[object {tag}]`:\n{generated}"
        );
    }
}

#[test]
fn typed_array_element_codec_covers_every_width_and_signedness() {
    // The element type is the load-bearing half of a typed array: the same eight
    // bytes are two `Float32Array` elements or one `Float64Array` element, and the
    // same byte `0xff` is `255` through `uint8` and `-1` through `int8`. The
    // generated codec must therefore carry one little-endian decode/encode pair per
    // element type, at the platform's `BYTES_PER_ELEMENT`, derived from the shared
    // registry rather than restated per call site.
    let generated =
        source_for("export function f(value: any): any { return (value as any)[0]; }");
    for decode in [
        "i8::from_le_bytes([raw[0]])",
        "i16::from_le_bytes([raw[0], raw[1]])",
        "u16::from_le_bytes([raw[0], raw[1]])",
        "i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])",
        "u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])",
        "f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]])",
        "f64::from_le_bytes(raw)",
        "i64::from_le_bytes(raw)",
        "u64::from_le_bytes(raw)",
    ] {
        assert!(
            generated.contains(decode),
            "the element codec must decode with `{decode}`:\n{generated}"
        );
    }
    // `Uint8ClampedArray` is the one view that saturates and rounds half to even
    // where the other integer views wrap modulo their width.
    assert!(
        generated.contains("number.round_ties_even().clamp(0.0, 255.0) as u8"),
        "`uint8clamped` must saturate rather than wrap:\n{generated}"
    );
    assert!(
        generated.contains("(number as f32).to_le_bytes().to_vec()"),
        "`float32` must encode at single precision:\n{generated}"
    );
    // Widths come from the registry: `(marker, element tag, BYTES_PER_ELEMENT)`.
    for entry in [
        "(\"__smelt_uint8array\", \"uint8\", 1)",
        "(\"__smelt_int16array\", \"int16\", 2)",
        "(\"__smelt_float32array\", \"float32\", 4)",
        "(\"__smelt_float64array\", \"float64\", 8)",
        // Node `Buffer` subclasses `Uint8Array`, so it shares that element type.
        "(\"__smelt_buffer\", \"uint8\", 1)",
    ] {
        assert!(
            generated.contains(entry),
            "the element-kind table must carry `{entry}`:\n{generated}"
        );
    }
    // `ArrayBuffer`/`SharedArrayBuffer`/`DataView` are byte-addressed: their bytes
    // carry no single element type, so they must stay out of the element-kind table
    // (they do still appear in the byte-backed marker arrays and the
    // marker-to-class table, so this inspects the table line itself).
    let table_line = generated
        .lines()
        .find(|line| line.contains("fn smelt_host_buffer_element_kind("))
        .unwrap_or_else(|| panic!("element-kind table must be emitted:\n{generated}"));
    for absent in [
        "__smelt_arraybuffer",
        "__smelt_sharedarraybuffer",
        "__smelt_dataview",
    ] {
        assert!(
            !table_line.contains(absent),
            "byte-addressed kind `{absent}` must have no element type:\n{table_line}"
        );
    }
}

#[test]
fn typed_array_length_is_the_element_count_not_the_byte_count() {
    // `new Float64Array(new ArrayBuffer(8))` has ONE element; the old numeric-list
    // model reported 8, the byte count. The record's `length` is therefore
    // `byteLength / BYTES_PER_ELEMENT`, and a view also records the `buffer` it
    // windows plus its `byteOffset` — a typed array in JavaScript is always a
    // window onto an `ArrayBuffer`.
    let generated =
        source_for("export function f(value: any): any { return (value as any).slice(0); }");
    assert!(
        generated.contains(
            "(\"length\".to_owned(), SmeltUnknown::Number((byte_length / stride) as f64))"
        ),
        "`length` must be the element count:\n{generated}"
    );
    assert!(
        generated.contains(
            "(\"byteLength\".to_owned(), SmeltUnknown::Number(byte_length as f64))"
        ),
        "`byteLength` must stay the byte count:\n{generated}"
    );
    assert!(
        generated.contains("fields.push((\"buffer\".to_owned(), buffer))")
            && generated.contains("fields.push((\"byteOffset\".to_owned()"),
        "a view must record the buffer it windows and its offset:\n{generated}"
    );
    // `slice`/`subarray` bounds are element indices for a view and byte indices for
    // byte-addressed storage — one code path, once the stride comes from the
    // registry.
    assert!(
        generated.contains("let len = (bytes.len() / stride) as i64"),
        "slice bounds must be scaled by the element stride:\n{generated}"
    );
}

#[test]
fn typed_array_construction_views_storage_but_converts_elements() {
    // `new Float32Array(source)` has two distinct JavaScript meanings that a
    // shapeless byte copy cannot tell apart: over an `ArrayBuffer` it re-*views*
    // the bytes (eight bytes become two elements), and over another view or an
    // array it *converts* the elements one by one (so
    // `new Uint8Array(new Int8Array([-1]))` holds `255`). The role split in the
    // shared registry is what selects between them.
    let generated = source_for(
        "export function f(buffer: any): any { return new Float32Array(buffer as any); }",
    );
    assert!(
        generated.contains("smelt_reflected_construct(\"float32array\""),
        "`new Float32Array(x)` must route through the shared host constructor:\n{generated}"
    );
    assert!(
        generated.contains("fn smelt_host_buffer_is_storage(value: &SmeltUnknown) -> bool")
            && generated.contains(
                "[\"__smelt_arraybuffer\", \"__smelt_sharedarraybuffer\"].into_iter().any(|marker| map.contains_key(marker))"
            ),
        "the storage/view split must come from the registry roles:\n{generated}"
    );
    assert!(
        generated.contains("Some(value) if smelt_host_buffer_is_storage(value) =>"),
        "a storage source must be re-viewed byte-for-byte:\n{generated}"
    );
    assert!(
        generated.contains("smelt_host_buffer_encode_element(kind, item)"),
        "a view/array source must be converted element-by-element:\n{generated}"
    );
    // A numeric length allocates ELEMENTS, so a wider view over the same count is
    // wider in bytes.
    assert!(
        generated.contains("vec![SmeltUnknown::Number(0.0); count * stride]"),
        "`new Ctor(n)` must allocate `n` elements:\n{generated}"
    );
}

#[test]
fn typed_array_own_keys_are_its_element_indices() {
    // A typed array's own enumerable properties are exactly its indexed elements;
    // `length`/`byteLength`/`byteOffset`/`buffer` are prototype accessors and
    // `bytes` is internal storage. es-toolkit's `keys(new Uint8Array(1))` asserts
    // `['0']`, and leaking the storage keys instead would make a deep-equality walk
    // over two views compare internal fields — and recurse through `buffer` back
    // into the view.
    // An erased receiver is re-materialized as a `SmeltRecord` before its keys are
    // read, so this is the path both `any` and `Record<string, unknown>` take.
    for source in [
        "export function f(value: any): string[] { return Object.keys(value); }",
        "export function f(value: Record<string, unknown>): string[] { return Object.keys(value); }",
    ] {
        let generated = source_for(source);
        assert!(
            generated.contains("smelt_host_buffer_record_index_keys(&"),
            "`Object.keys` on a view must answer its element indices:\n{generated}"
        );
    }
    // Both key helpers are emitted from one definition, so the tagged-`SmeltUnknown`
    // projection cannot drift from the structural-record one.
    let generated =
        source_for("export function f(value: any): string[] { return Object.keys(value); }");
    assert!(
        generated.contains("fn smelt_host_buffer_index_keys(value: &SmeltUnknown)")
            && generated.contains(
                "fn smelt_host_buffer_record_index_keys(record: &SmeltRecord<String, SmeltUnknown>) -> Option<Vec<String>> { smelt_host_buffer_index_keys("
            ),
        "the record-flavored index keys must delegate to the tagged one:\n{generated}"
    );
    let values = source_for(
        "export function f(value: any): unknown[] { return Object.values(value); }",
    );
    assert!(
        values.contains("smelt_host_buffer_record_elements(&"),
        "`Object.values` on a view must answer its decoded elements:\n{values}"
    );
    let entries = source_for(
        "export function f(value: any): [string, unknown][] { return Object.entries(value); }",
    );
    assert!(
        entries.contains("smelt_host_buffer_record_elements(&"),
        "`Object.entries` on a view must pair indices with decoded elements:\n{entries}"
    );
    // A property test (`k in obj` / `Object.hasOwn`) must also see the indexed
    // elements, which live in `bytes` rather than as record keys.
    let has_own = source_for(
        "export function f(value: any, key: string): boolean { return Object.hasOwn(value, key); }",
    );
    assert!(
        has_own.contains("smelt_host_buffer_element(&values, &smelt_key).is_some()"),
        "a property test on a view must see its element indices:\n{has_own}"
    );
}

#[test]
fn typed_array_instanceof_resolves_through_the_view_marker() {
    // `x instanceof Uint8Array` used to fold to a constant derived from the static
    // type — `true` for any `number[]` — because the numeric-list model left no
    // identity to test. It is now the registry marker probe, and Node's `Buffer`
    // satisfies `instanceof Uint8Array` because the registry records that subclass
    // relation in its spec tag.
    let generated = source_for(
        "export function f(value: unknown): boolean { return value instanceof Uint8Array; }",
    );
    assert!(
        generated.contains("value.contains_key(\"__smelt_uint8array\")"),
        "`instanceof Uint8Array` must probe the view marker:\n{generated}"
    );
    assert!(
        generated.contains("value.contains_key(\"__smelt_buffer\")"),
        "Node `Buffer` subclasses `Uint8Array`, so it must satisfy the probe:\n{generated}"
    );
    let other = source_for(
        "export function f(value: unknown): boolean { return value instanceof Float64Array; }",
    );
    assert!(
        other.contains("value.contains_key(\"__smelt_float64array\")")
            && !other.contains("value.contains_key(\"__smelt_uint8array\")"),
        "each view must probe only its own marker:\n{other}"
    );
}

#[test]
fn buffer_reports_the_uint8array_spec_tag() {
    // Node's `Buffer` subclasses `Uint8Array`, so the platform reports
    // `[object Uint8Array]`. es-toolkit's `isEqualWith` and `cloneDeepWith` dispatch
    // on that tag ("Buffers are also treated as [object Uint8Array]s"), and a
    // `[object Buffer]` tag falls off the end of both `switch` statements — two
    // equal buffers compared unequal and a buffer was not cloneable.
    let generated = source_for(
        "export function f(value: any): string { return Object.prototype.toString.call(value); }",
    );
    assert!(
        generated
            .contains("if map.contains_key(\"__smelt_buffer\") { return \"[object Uint8Array]\""),
        "a `Buffer` record must tag as `[object Uint8Array]`:\n{generated}"
    );
}

#[test]
fn a_bare_host_constructor_reference_is_interned() {
    // JavaScript exposes one object per global builtin name, so `Blob === Blob` and
    // `blob.constructor === Blob` both hold. A record literal mints a fresh identity
    // on construction, so building a `__smelt_builtin_namespace` record per
    // reference made both comparisons false. The interning helper is also what a
    // host record's `.constructor` resolves through, so the two spellings meet.
    let generated = source_for("export function f(): any { return Blob; }");
    assert!(
        generated.contains("smelt_builtin_namespace(\"Blob\")"),
        "a bare host-constructor reference must intern:\n{generated}"
    );
    let field = source_for("export function f(value: any): any { return value.constructor; }");
    assert!(
        field.contains("smelt_marker_constructor_class(map)"),
        "an erased `.constructor` read must resolve a marker record's own global:\n{field}"
    );
}

#[test]
fn arguments_object_carries_parameter_values_and_hides_length() {
    // The `arguments` exotic object stores the actual call arguments under index
    // keys with a non-enumerable `length`. It used to be a `{ length: n }` stand-in
    // holding no values at all, so `Object.keys(arguments)` enumerated
    // `["length"]` — the exact inverse of the real key set — and comparing an
    // `arguments` object against the plain object with the same indexed properties
    // could not work in either direction.
    let generated = source_for(
        r"
export function f(a: number, b: number): any {
  const unused = a + b;
  return arguments;
}
",
    );
    assert!(
        generated.contains("smelt_arguments_object(vec!["),
        "`arguments` must be built from the function's parameters:\n{generated}"
    );
    assert!(
        generated.contains("[object Arguments]"),
        "an `arguments` record must carry the `[object Arguments]` spec tag:\n{generated}"
    );
    assert!(
        generated.contains(
            "!(object.contains_key(\"__smelt_arguments\") && matches!(key, \"__smelt_arguments\" | \"length\"))"
        ),
        "`length` must stay out of an `arguments` object's own-key enumeration:\n{generated}"
    );
}

#[test]
fn an_arguments_object_is_iterable() {
    // A JavaScript `arguments` object is iterable — its `Symbol.iterator` is
    // `Array.prototype.values` — so `Array.from(arguments)` and `[...arguments]`
    // both walk its elements. Smelt models it as an array-like marker record with
    // index keys and a hidden `length`, which carries no
    // `__smelt_symbol_iterator` slot, so the erased iterable-to-list coercion
    // fell straight through to `panic!("unknown is not iterable")`. Every
    // es-toolkit spec helper that reads its own call arguments through
    // `Array.from(arguments)` (`rest`, `ary`, `unary`, `partial`, `flow`, …) died
    // in that panic before asserting anything.
    let generated = source_for(
        r"
export function f(a: number, b: number): unknown[] {
  const unused = a + b;
  return Array.from(arguments as any);
}
",
    );
    assert!(
        generated.contains("fn smelt_arguments_elements(object: &SmeltObject)"),
        "the `arguments` iteration door must be emitted:\n{generated}"
    );
    assert!(
        generated.contains("smelt_arguments_elements(&value)"),
        "the erased iterable-to-list coercion must consult it:\n{generated}"
    );
}

#[test]
fn a_self_recursive_named_function_expression_lifts_to_an_item() {
    // A named function expression binds its own name inside its own body, and that
    // is how JavaScript writes a self-recursive callback:
    //
    //     mergeWith(cloneDeep(target), source, function mergeRecursively(a, b) {
    //       … return mergeWith(clone(a), b, mergeRecursively); …
    //     })
    //
    // The closure path never bound the name, so the self-reference fell through
    // identifier resolution to the forward-callable fallback and lowered to an
    // EMPTY OBJECT — and calling an empty object collapses to a null callback
    // rather than failing, so the recursion silently did nothing. All eight
    // es-toolkit `toMerged` specs were that one defect.
    //
    // An inline Rust closure cannot express it either: it would have to capture the
    // binding it is being assigned to. So the function is lifted to a module item
    // and the recursion becomes ordinary `fn` recursion.
    let generated = source_for(
        r"
function apply(cb: (n: number) => number, v: number): number {
  return cb(v);
}

export function countdown(start: number): number {
  return apply(function step(n: number): number {
    if (n <= 0) {
      return 0;
    }
    return step(n - 1) + 1;
  }, start);
}
",
    );
    assert!(
        generated.contains("fn step(n: f64) -> f64"),
        "a self-recursive named function expression must lift to a function item:\n{generated}"
    );
    // The recursion has to be inside the lifted item itself, and it has to be a
    // direct item call rather than a dispatch through an erased value — which is
    // what proves the self-reference resolved to the item and not to the
    // forward-callable fallback. Read the program section only: the runtime prelude
    // mentions the erased-record constructors unconditionally.
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    let step_body = program
        .split_once("fn step(n: f64) -> f64")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split_once("\nfn "))
        .map_or_else(String::new, |(body, _)| body.to_owned());
    assert!(
        step_body.contains("step("),
        "the lifted item must call itself:\n{generated}"
    );
    assert!(
        !step_body.contains("SmeltRecord::from([])"),
        "the self-reference must not lower to an empty erased record:\n{generated}"
    );
}

#[test]
fn a_named_function_expression_name_stays_out_of_module_scope() {
    // JavaScript scopes a named function expression's name to its own body, so the
    // lift binds the name only while that body is lowered. A sibling reference to
    // the same name must still resolve to whatever the module actually declares —
    // here a top-level `step` with a different signature, which the lifted item
    // must not have displaced.
    let generated = source_for(
        r#"
function apply(cb: (n: number) => number, v: number): number {
  return cb(v);
}

export function step(label: string): string {
  return `${label}!`;
}

export function countdown(start: number): number {
  return apply(function step(n: number): number {
    if (n <= 0) {
      return 0;
    }
    return step(n - 1) + 1;
  }, start);
}

export function shout(): string {
  return step("go");
}
"#,
    );
    assert!(
        generated.contains("-> String") && generated.contains("fn shout()"),
        "the module-scope `step` must keep its own signature:\n{generated}"
    );
    assert!(
        generated.contains("fn step(n: f64) -> f64") || generated.contains("fn step_"),
        "the lifted item must still exist alongside it:\n{generated}"
    );
}

#[test]
fn array_containment_projects_an_optional_union_receiver() {
    // `chars?: string | string[]` interns as `Optional(Union([String, List(String)]))`.
    // The source narrows it with `switch (typeof chars) { case 'object': … }` and the
    // frontend only emits `Rvalue::ListContains` after that narrowing holds, but MIR
    // reads the value through its DECLARING local, so the operand type at emission
    // is still the wide one. `list_contains_text` matched `Type::List` alone and
    // answered a constant `false` for anything else, so es-toolkit's
    // `trim`/`trimStart`/`trimEnd` array-`chars` loops never removed a character —
    // ten specs, silently wrong rather than failing.
    let generated = source_for(
        r"
export function trimIt(str: string, chars?: string | string[]): string {
  if (chars === undefined) {
    return str;
  }
  let startIndex = 0;
  switch (typeof chars) {
    case 'string': {
      while (startIndex < str.length && str[startIndex] === chars) {
        startIndex++;
      }
      break;
    }
    case 'object': {
      while (startIndex < str.length && chars.includes(str[startIndex])) {
        startIndex++;
      }
    }
  }
  return str.substring(startIndex);
}
",
    );
    assert!(
        generated.contains("union guard selected an excluded member"),
        "the containment receiver must project to its list arm:\n{generated}"
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    assert!(
        program.contains(".contains(&str.chars().nth("),
        "containment must compare against the projected `Vec`:\n{generated}"
    );
}

#[test]
fn math_round_uses_the_javascript_tie_rule() {
    // JavaScript rounds a tie toward +∞; Rust's `f64::round` rounds a tie away from
    // zero. They disagree for every negative value whose fraction is exactly 0.5 —
    // `Math.round(-1.5)` is `-1` in JavaScript and `-2.0` in Rust — which is what
    // made es-toolkit's `round` specs disagree. `floor`/`ceil`/`trunc` mean the same
    // thing in both languages and must keep mapping straight to their `f64` methods.
    let generated = source_for("export function f(x: number): number { return Math.round(x); }");
    assert!(
        generated.contains("fn smelt_math_round(value: f64) -> f64"),
        "the JavaScript rounding helper must be emitted:\n{generated}"
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    assert!(
        program.contains("smelt_math_round("),
        "`Math.round` must route through the helper:\n{generated}"
    );
    assert!(
        !program.contains(".round()"),
        "`Math.round` must not use Rust's tie-away-from-zero rounding:\n{generated}"
    );
    let floor = source_for("export function f(x: number): number { return Math.floor(x); }");
    assert!(
        floor.contains(".floor()") && !floor.contains("fn smelt_math_round"),
        "`Math.floor` agrees between the languages and must not pull the helper in:\n{floor}"
    );
}

#[test]
fn an_assertion_overload_still_emits_its_call() {
    // An assertion overload (`asserts condition`) returns void at runtime, and
    // `function_declaration` records exactly that for the implementation signature.
    // `overload_signature` lowered the annotation structurally instead and got
    // `Bool` (a `TSTypePredicate` is boolean-shaped). The selected overload's return
    // type is what types the call's destination, so the call site ended up with a
    // `Bool` destination for a `None`-returning function and the emitter DROPPED the
    // call, leaving only its arguments evaluated. es-toolkit `invariant` is that
    // shape — two `asserts condition` overloads plus an implementation — and all
    // four of its specs asserted against a call that never happened.
    let generated = source_for(
        r"
export function invariant(condition: unknown, message: string): asserts condition;
export function invariant(condition: unknown, error: Error): asserts condition;
export function invariant(condition: unknown, message: string | Error): asserts condition {
  if (condition) {
    return;
  }
  throw new Error('boom');
}

export function run(): void {
  invariant(false, 'boom');
}
",
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    let run_body = program
        .split_once("fn run()")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split_once("\nfn ").map_or(Some(rest), |(body, _)| Some(body)))
        .unwrap_or_default();
    assert!(
        run_body.contains("invariant("),
        "the assertion call must be emitted, not folded away:\n{generated}"
    );
}

#[test]
fn a_function_reading_arguments_lowers_to_one_rest_parameter() {
    // A JavaScript `arguments` object is the ACTUAL argument list of the call, not
    // the declared parameter list, and the two differ constantly:
    //
    //     function fn(_a, _b, _c) { return Array.from(arguments); }
    //     ary(fn, 2)('a', 'b', 'c', 'd');   // fn('a', 'b') -> ['a', 'b']
    //
    // A declared-arity signature cannot carry that: the erased-call boundary pads a
    // short call up to the arity, and the padding is indistinguishable from a real
    // trailing `undefined` (which `partial`'s placeholder specs deliberately pass).
    // So such a function is lowered variadically — one rest parameter holding the
    // whole list, each declared name re-bound from it — and `arguments` is that
    // list.
    let generated = source_for(
        r"
export function probe(_a: unknown, _b: unknown, _c: unknown): unknown {
  return arguments;
}
",
    );
    assert!(
        generated.contains("smelt_arguments_object(vec![], Some("),
        "`arguments` must be built from the rest list, not the parameters:\n{generated}"
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    assert!(
        program.contains("fn probe(__smelt_arguments: SmeltList<SmeltUnknown>)"),
        "the signature must become a single rest list:\n{generated}"
    );
    assert!(
        program.contains("let _a: SmeltUnknown = __smelt_arguments"),
        "each declared name must be re-bound from the list:\n{generated}"
    );
}

#[test]
fn a_function_not_reading_arguments_keeps_its_declared_arity() {
    // The variadic rewrite is scoped to functions that actually read `arguments`;
    // everything else keeps its declared parameters, so the change costs nothing
    // for the overwhelming majority of code. An arrow body inside the function does
    // NOT count as reading its own `arguments` unless it mentions it — but a nested
    // non-arrow `function` reading `arguments` reads its OWN, so the outer function
    // stays fixed-arity.
    let generated = source_for(
        r"
export function outer(a: number, b: number): number {
  function inner(_x: unknown): unknown {
    return arguments;
  }
  const unused = inner(a);
  return a + b;
}
",
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    assert!(
        program.contains("fn outer(a: f64, b: f64)"),
        "a function that never mentions `arguments` must keep its parameters:\n{generated}"
    );
}

#[test]
fn an_erased_callable_reports_its_function_length() {
    // A typed callable knows its arity and `SmeltErasedFunction` carries it in a
    // `length` field, but erasing to `SmeltUnknown::Function(Rc<…>)` throws the
    // field away — an `Rc<dyn Fn>` has nowhere to put it — so a `.length` read on
    // an erased callable answered `0`. es-toolkit `rest(func)` defaults its split
    // point to `func.length - 1`, so `0` made it `-1` and every rest-parameter spec
    // reshaped its arguments wrongly. The arity is now parked in a registry keyed by
    // the callable's canonical identity.
    // The fixture erases a real callable so the `SmeltErasedFunction` boundary — the
    // only place that still knows both the arity and the erased allocation — is
    // emitted alongside the read.
    let generated = source_for(
        r"
function target(_a: unknown, _b: unknown): unknown {
  return _a;
}

export function arity(): number {
  const erased: unknown = target;
  return (erased as any).length;
}
",
    );
    assert!(
        generated.contains("fn smelt_function_length(value: &SmeltUnknown) -> f64"),
        "the function-length reader must be emitted:\n{generated}"
    );
    assert!(
        generated.contains("smelt_register_function_length(&smelt_erased_fn"),
        "the erasure boundary must record the arity:\n{generated}"
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    assert!(
        program.contains("smelt_function_length("),
        "an erased `.length` read must consult the registry:\n{generated}"
    );
}

#[test]
fn a_static_property_on_a_function_declaration_resolves() {
    // JavaScript functions are objects, so a module can hang a value off one. It is
    // how es-toolkit publishes its placeholder sentinels — `partial.placeholder`,
    // `curry.placeholder`, `bind.placeholder`, … — and `memoize.Cache = Map`.
    //
    // The assignment used to lower into the module-init body, which nothing calls,
    // with the TARGET dropped outright: the init body bound the right-hand side to a
    // local and discarded it. Every read then answered `SmeltUnknown::Null`, so
    // `partial(fn, placeholder, 'b', placeholder)` passed `null` where a sentinel
    // belonged and the placeholder slots were filled with a real argument.
    let generated = source_for(
        r"
export function partial(func: (...args: any[]) => any, ...args: any[]): (...rest: any[]) => any {
  return (...rest: any[]) => func(...args, ...rest);
}

partial.placeholder = Symbol('partial.placeholder');

export function readMember(): unknown {
  return partial.placeholder;
}

export function readDestructured(): unknown {
  const { placeholder } = partial;
  return placeholder;
}
",
    );
    let program = generated
        .split_once("@smelt:prelude-end")
        .map_or(generated.as_str(), |(_, program)| program);
    let member = program
        .split_once("fn read_member()")
        .map_or("", |(_, rest)| rest);
    assert!(
        member.contains("SmeltUnknown::Symbol("),
        "a static-property member read must resolve to the recorded value:\n{generated}"
    );
    let destructured = program
        .split_once("fn read_destructured()")
        .map_or("", |(_, rest)| rest);
    assert!(
        destructured.contains("SmeltUnknown::Symbol("),
        "destructuring a static property off a function must resolve too:\n{generated}"
    );
    assert!(
        !destructured.contains("let placeholder: SmeltUnknown = SmeltUnknown::Null"),
        "the destructured binding must not fall back to null:\n{generated}"
    );
}

/// A `return` inside a `try` that has a `finally` must still run the finalizer.
///
/// MIR made the finalizer the *fall-through* exit of the `try` body, so a
/// `Return` terminator bypassed it and the cleanup vanished from the generated
/// Rust altogether. es-toolkit's `areObjectsEqual` clears its recursion `Map` in
/// exactly that shape, and the leaked entries made
/// `isEqualWith({ constructor: [1] }, { constructor: ['1'] })` answer `true`.
/// `lower_return` now re-lowers the finalizer inline ahead of the return, so the
/// cleanup has to appear on the return path.
#[test]
fn finally_body_is_emitted_on_the_return_path() {
    let source = source_for(
        r#"
export function cleanupOnReturn(seen: Map<string, number>): number {
  seen.set("a", 1);
  try {
    return 7;
  } finally {
    seen.delete("a");
  }
}
const out = cleanupOnReturn(new Map<string, number>());
console.log(out);
"#,
    );

    let start = source
        .find("fn cleanup_on_return")
        .expect("cleanupOnReturn present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("cleanupOnReturn closing brace");
    let body = &after[..end];

    // The `seen.delete("a")` cleanup must survive on the return path. Before the
    // fix the function body contained no removal at all.
    assert!(
        body.contains(".remove("),
        "the finalizer's Map delete must be emitted, got:\n{body}"
    );
    assert!(
        body.contains("7"),
        "the try body's return value must survive the finalizer, got:\n{body}"
    );
}

/// Nested finalizers unwind inner-to-outer ahead of the return.
///
/// The inline duplication has to walk the whole lexical finalizer stack in
/// JavaScript's unwind order, not just the innermost clause.
#[test]
fn nested_finally_bodies_are_emitted_inner_to_outer() {
    let source = source_for(
        r#"
export function nestedCleanup(log: string[]): string {
  try {
    try {
      return "value";
    } finally {
      log.push("inner");
    }
  } finally {
    log.push("outer");
  }
}
const out = nestedCleanup([]);
console.log(out);
"#,
    );

    let start = source
        .find("fn nested_cleanup")
        .expect("nestedCleanup present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("nestedCleanup closing brace");
    let body = &after[..end];

    let inner = body.find("\"inner\"").expect("inner finalizer emitted");
    let outer = body.find("\"outer\"").expect("outer finalizer emitted");
    assert!(
        inner < outer,
        "the inner finalizer must be emitted before the outer one on the return \
         path (inner={inner}, outer={outer}):\n{body}"
    );
}

/// `Object.getOwnPropertySymbols` yields symbol VALUES, not descriptions.
///
/// A symbol-keyed property is stored under `"__smelt_symbol:<description>"`, and a
/// symbol value is `SmeltUnknown::Symbol(description)`. The projection used to
/// strip the prefix and hand back a bare `String`, which broke both directions of
/// the round trip: `source[symbols[i]]` looked up the *unprefixed* string key and
/// missed, and `target[symbols[i]] = v` created a plain string property that no
/// symbol lookup or symbol enumeration could see. Re-tagging the description keeps
/// the property-key mapping able to rebuild the internal key.
#[test]
fn reflected_symbol_keys_keep_their_symbol_tag() {
    let source = source_for(
        r"
export function copySymbols(source: any, target: any): void {
  const symbols = Object.getOwnPropertySymbols(source);
  for (let i = 0; i < symbols.length; i++) {
    target[symbols[i]] = source[symbols[i]];
  }
}
const out: any = {};
copySymbols({}, out);
console.log(out);
",
    );

    assert!(
        source.contains("SmeltUnknown::Symbol(description.into())"),
        "the symbols projection must re-tag the stripped description as a symbol \
         value:\n{source}"
    );
    assert!(
        !source.contains("strip_prefix(\"__smelt_symbol:\").map(str::to_owned)"),
        "the symbols projection must not hand back bare descriptions:\n{source}"
    );
}

/// A `(string | symbol)[]` key spread must keep both halves.
///
/// `[...Object.keys(o), ...Object.getOwnPropertySymbols(o)]` chains a
/// `List<String>` onto a `List<Unknown>`. The concat emitter bailed out on
/// mismatched element types and returned `Default::default()` — an EMPTY list — so
/// es-toolkit's `copyProperties` silently copied nothing. The concrete side's
/// elements are erased into the `SmeltUnknown` element type instead.
#[test]
fn mixed_string_and_symbol_key_spread_erases_instead_of_emptying() {
    let source = source_for(
        r"
export function allKeys(source: any): any[] {
  return [...Object.keys(source), ...Object.getOwnPropertySymbols(source)];
}
const keys = allKeys({});
console.log(keys.length);
",
    );

    let start = source.find("fn all_keys").expect("allKeys present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("allKeys closing brace");
    let body = &after[..end];

    assert!(
        !body.contains("Default::default()"),
        "a mixed string/symbol key spread must not collapse to an empty list:\n{body}"
    );
    assert!(
        body.contains(".chain("),
        "both spread halves must be chained into the result list:\n{body}"
    );
}

/// A loop whose body contains a `try`/`catch` around an `await` must stay a loop.
///
/// Two independent gaps combined here. `block_exits_to_loop` — the guard
/// `while_header` uses to decide whether a body is loop-shaped — had no arm for
/// `Terminator::Await` and fell into its `_ => Ok(false)` catch-all, so the
/// header was never recognized. And the throwing-call/throwing-`await` emitters
/// hardcoded `emit_block` for their `Ok`/`Err` continuations, so even once
/// recognized the back edge reached from inside `match fut.await { .. }` was
/// treated as an ordinary goto and re-emitted the loop header inline. The two
/// together made this 12-line function expand to 437 lines with 14 `.await`s and
/// no `loop`, terminated only by the emitter's recursion-depth cap — which
/// silently caps the number of iterations the program can actually perform.
///
/// The load-bearing assertions are the counts, not the presence of `loop`: one
/// `.await` and one `catch_unwind` prove the body was emitted exactly once, which
/// is the difference between a loop and an unrolled prefix of one.
#[test]
fn awaiting_try_catch_in_a_loop_body_emits_one_loop_not_an_unrolled_body() {
    let source = source_for(
        r"
async function flaky(n: number): Promise<number> {
  if (n < 3) { throw new Error('nope'); }
  return n;
}
export async function attempt(limit: number): Promise<number> {
  for (let i = 0; i <= limit; i++) {
    try {
      return await flaky(i);
    } catch (err) {
      // keep going
    }
  }
  return -1;
}
",
    );

    let start = source.find("async fn attempt").expect("attempt present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("attempt closing brace");
    let body = &after[..end];

    assert_eq!(
        body.matches("loop {").count(),
        1,
        "the `for` header must render as a single Rust loop:\n{body}"
    );
    assert_eq!(
        body.matches(".await").count(),
        1,
        "the one source `await` must be emitted once; more copies mean the loop \
         body was unrolled into the back edge:\n{body}"
    );
    assert_eq!(
        body.matches("catch_unwind").count(),
        1,
        "the one source `try` must be emitted once:\n{body}"
    );
    assert!(
        body.contains("continue;"),
        "the back edge out of the `catch` arm must render as `continue`:\n{body}"
    );
    assert!(
        !body.contains("could not structurally emit"),
        "the region must be structured, not surrendered to the recursion cap:\n{body}"
    );
}

/// The same shape without `await` must keep its `catch` handler.
///
/// The loop-body emitters matched `Terminator::Call { unwind: _, .. }` and threw
/// the exception handler away, emitting the call through the plain `?` template.
/// That deleted the `catch` block outright: a synchronous
/// `for (..) { try { return f(i); } catch {} }` propagated the first error out of
/// the enclosing function instead of retrying, and in a non-throwing function the
/// stray `?` did not even compile. Asserting on `catch_unwind` (not on `loop`) is
/// what pins the handler down — the loop was already emitted before the fix.
#[test]
fn a_try_catch_in_a_loop_body_keeps_its_handler() {
    let source = source_for(
        r"
function flaky(n: number): number {
  if (n < 3) { throw new Error('nope'); }
  return n;
}
export function attempt(limit: number): number {
  for (let i = 0; i <= limit; i++) {
    try {
      return flaky(i);
    } catch (err) {
      // keep going
    }
  }
  return -1;
}
console.log(attempt(5));
",
    );

    let start = source.find("fn attempt").expect("attempt present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("attempt closing brace");
    let body = &after[..end];

    assert_eq!(
        body.matches("loop {").count(),
        1,
        "the `for` header must render as a single Rust loop:\n{body}"
    );
    assert!(
        body.contains("catch_unwind"),
        "the `catch` handler must be emitted inside the loop body:\n{body}"
    );
    assert!(
        !body.contains("flaky(i.clone())?"),
        "the throwing call must not be emitted through the error-propagating `?` \
         template, which discards the `catch` arm:\n{body}"
    );
    assert!(
        body.contains("continue;"),
        "the back edge out of the `catch` arm must render as `continue`:\n{body}"
    );
}

/// A `try`/`catch` inside a closure body must keep its handler.
///
/// Closure bodies are rendered by their own recursive emitter,
/// `emit_closure_block_inner`, and its `Call`/`Await` arms matched
/// `unwind: _` — throwing the exception handler away. The call then went through
/// `closure_call_text_for_dest`, whose single difference from the function-level
/// path is that it rewrites a trailing `?` into
/// `unwrap_or_else(|error| panic!(..))`. So a nested function whose body wrapped
/// a throwing call in `try`/`catch` did not run its `catch` at all: it aborted
/// the process on the first throw.
///
/// The load-bearing assertion is the absence of the panicking unwrap together
/// with the presence of `catch_unwind`. Asserting only on `loop {` would prove
/// nothing here — the closure emitter already recognized this loop before the
/// fix and still produced a program that panicked.
#[test]
fn a_try_catch_inside_a_closure_body_keeps_its_handler() {
    let source = source_for(
        r"
function flaky(x: number): number {
  if (x < 3) { throw new Error('nope'); }
  return x;
}
export function outer(n: number): number {
  const inner = (limit: number): number => {
    for (let i = 0; i <= limit; i++) {
      try { return flaky(i); } catch (err) { }
    }
    return -1;
  };
  return inner(n);
}
console.log(outer(5));
",
    );

    let start = source.find("fn outer").expect("outer present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("outer closing brace");
    let body = &after[..end];

    assert_eq!(
        body.matches("loop {").count(),
        1,
        "the closure body's `for` must still render as a single loop:\n{body}"
    );
    assert!(
        body.contains("catch_unwind"),
        "the `catch` handler must be emitted inside the closure body:\n{body}"
    );
    assert!(
        !body.contains("unwrap_or_else(|error| panic!"),
        "a caught throwing call must not be emitted as a panicking unwrap, which \
         is the handler being discarded:\n{body}"
    );
}

/// An awaited rejection inside a closure `try` runs the `catch`.
///
/// The closure emitter's `Await` arm dropped `unwind` exactly like its `Call`
/// arm, so `try { await f() } catch` inside a closure propagated the rejection
/// with `?` instead of running the handler. The awaited call itself is
/// deliberately left on the statement path by lowering — an async callee hands
/// back a future and the rejection surfaces at the `await`, whose terminator
/// already carries the handler — so `.await` appearing exactly once inside a
/// `match` is what proves the single handler landed in the right place.
#[test]
fn an_awaited_rejection_inside_a_closure_try_runs_the_catch() {
    let source = source_for(
        r"
async function flaky(x: number): Promise<number> {
  if (x < 3) { throw new Error('nope'); }
  return x;
}
export function outer(n: number): number {
  const inner = async (limit: number): Promise<number> => {
    for (let i = 0; i <= limit; i++) {
      try { return await flaky(i); } catch (err) { }
    }
    return -1;
  };
  inner(n);
  return 0;
}
",
    );

    assert!(
        source.contains("match _smelt_tmp_6.await {") || source.contains(".await {"),
        "the awaited rejection must be matched, not propagated with `?`:\n{source}"
    );
    assert!(
        !source.contains(".await?;\n    return Ok::<f64"),
        "the awaited value inside a `try` must not be propagated with `?`:\n{source}"
    );
}

/// A `try` around a call to a function-typed *parameter* must emit its handler.
///
/// `ExprKind::ClosureCall` took the unwind-carrying `Terminator::Call` form only
/// when the callee type's `may_throw` was set. A callback parameter of unknown
/// provenance has `may_throw == false` — yet the source wrapped the call in
/// `try` precisely *because* its throw behaviour is not statically known — so
/// the statement form was taken and the active exception handler was discarded
/// outright. The `catch` clause did not merely mis-structure: it was absent, and
/// the first throw aborted the process where JavaScript would have carried on.
#[test]
fn a_try_around_a_callback_parameter_call_emits_its_handler() {
    let source = source_for(
        r"
export function guard(cb: (x: number) => string, v: number): string {
  try {
    return cb(v);
  } catch {
    return 'caught';
  }
}
",
    );

    assert!(
        source.contains("::std::panic::catch_unwind"),
        "the callback call must carry an unwind edge:\n{source}"
    );
    assert!(
        source.contains("return \"caught\".to_owned();"),
        "the catch clause must be emitted:\n{source}"
    );
}

/// A throwing call inside a *fallible* closure body keeps its `?`.
///
/// Adapting a throwing function into a callback slot wraps it in a closure whose
/// Rust signature is `-> Result<T, Box<dyn std::error::Error>>`. The call inside
/// that body was still rewritten from `?` to
/// `unwrap_or_else(|error| panic!(..))`, which turned a recoverable JavaScript
/// exception into an abort even though the enclosing signature could carry it.
/// The `panic!` belongs only where the surrounding Rust signature genuinely
/// cannot carry an error.
#[test]
fn a_throwing_call_in_a_fallible_closure_body_propagates_with_question_mark() {
    let source = source_for(
        r"
function thrower(x: number): string {
  if (x > 0) { throw new Error('boom'); }
  return 'ok';
}
export function guard(cb: (x: number) => string, v: number): string {
  try {
    return cb(v);
  } catch {
    return 'caught';
  }
}
export function useIt(): string {
  return guard(thrower, 1);
}
",
    );

    assert!(
        source.contains("thrower(closure_arg_0)?"),
        "the fallible wrapper closure must propagate the throw:\n{source}"
    );
    assert!(
        !source.contains("thrower(closure_arg_0.clone()).unwrap_or_else"),
        "a fallible closure body must not abort on a recoverable throw:\n{source}"
    );
}

/// An erased-rest callback parameter bound to a local agrees with its call ABI.
///
/// `const g = cb` binds the parameter to a local whose Rust type is the owned
/// `SmeltErasedFunction` struct, whose callback field is a `'static`
/// `Rc<dyn Fn(Vec<SmeltUnknown>) -> SmeltUnknown>`. Wrapping the borrowed
/// parameter in a bare `Rc::new(move |arg0| cb(arg0))` produced a value that was
/// *not* that struct while the call site still used the erased `.call(..)` ABI —
/// an E0658 (`fn_traits`) plus an E0308 in the generated crate. The parameter
/// must therefore enter owned, so the value and its ABI agree.
#[test]
fn an_erased_rest_callback_parameter_bound_to_a_local_enters_owned() {
    let source = source_for(
        r"
export function viaLocal(cb: (...args: unknown[]) => unknown): unknown {
  const g = cb;
  return g(3, 4);
}
export function guardedViaLocal(cb: (...args: unknown[]) => unknown): unknown {
  const g = cb;
  try {
    return g(3, 4);
  } catch {
    return 'caught';
  }
}
",
    );

    assert!(
        source.contains("fn via_local(cb: SmeltErasedFunction)"),
        "a parameter bound to an erased handle local must enter owned:\n{source}"
    );
    assert!(
        source.contains("g.call("),
        "the erased handle keeps the inherent call ABI:\n{source}"
    );
    // Arity: `SmeltErasedFunction::call` takes the ARGUMENT VECTOR, while
    // lowering hands the emitter the rest-packed `SmeltList` standing for all
    // N source arguments. Nesting that list as a single vector element calls the
    // callback with one array argument instead of two — it compiles and silently
    // passes the wrong arity, so the packed list must be handed over as the
    // vector itself. Both call forms must render the same argument text.
    assert_eq!(
        source.matches(".call(_smelt_tmp_2)").count(),
        2,
        "both call forms pass the packed argument list as the argument vector:\n{source}"
    );
    assert!(
        !source.contains(".call(vec![SmeltUnknown::Array("),
        "the rest-packed argument list must not become one array argument:\n{source}"
    );
    assert!(
        !source.contains("::std::rc::Rc::new(move |arg0: SmeltList<SmeltUnknown>| cb(arg0))"),
        "the borrowed-handle wrapper disagrees with the erased call ABI:\n{source}"
    );
}

/// Both indirect-call forms answer the erased-rest ABI question the same way.
///
/// `Rvalue::ClosureCall` (statement form) had an explicit precedence ladder —
/// function-parameter place, function-parameter name, borrowed callback capture,
/// *then* erased-rest — while `call_text`'s `Callee::Indirect` (terminator form)
/// had none and emitted `.call(..)` for any erased-rest callee type. Routing a
/// call from one form to the other therefore flipped its ABI: a borrowed
/// `&dyn Fn` parameter is not the `SmeltErasedFunction` struct, so `.call(..)` on
/// it resolves to the unstable `Fn::call` trait method (E0658). One helper,
/// `callee_uses_erased_call_method`, now answers for both.
#[test]
fn a_borrowed_erased_rest_parameter_is_called_directly_in_both_forms() {
    let source = source_for(
        r"
export function guardedErased(cb: (...args: unknown[]) => unknown): unknown {
  try {
    return cb(1);
  } catch {
    return 'caught';
  }
}
export function plainErased(cb: (...args: unknown[]) => unknown): unknown {
  return cb(2);
}
",
    );

    assert!(
        source.contains("fn guarded_erased(cb: &dyn Fn(SmeltList<SmeltUnknown>) -> SmeltUnknown)"),
        "a borrowed erased-rest parameter stays a bare handle:\n{source}"
    );
    assert!(
        source.contains("(cb)(_smelt_tmp_1)"),
        "the terminator form must invoke the borrowed handle directly:\n{source}"
    );
    assert!(
        !source.contains("cb.call(") && !source.contains("(cb).call("),
        "a borrowed `&dyn Fn` must never take the inherent erased call ABI:\n{source}"
    );
}

/// A borrowed callback parameter renders its return through the canonical
/// function-value logic.
///
/// `param_type_text` re-derived the return type instead of sharing the
/// `Type::Function` arm's refinements, so the Rust type of a borrowed callback
/// could disagree with the Rust type of the very value bound to it. Both now
/// call `function_value_return_type_text`: a `Future` return is the promise
/// value `SmeltFuture<T>` (an async throw is a rejected future, so no outer
/// `Result` is added), and `may_throw` otherwise wraps the return in
/// `Result<T, Box<dyn std::error::Error>>`.
#[test]
fn a_borrowed_callback_parameter_renders_a_future_return_as_a_promise_value() {
    let source = source_for(
        r"
export function fireAndForget(cb: () => Promise<string>): void {
  cb();
}
",
    );

    assert!(
        source.contains("fn fire_and_forget(cb: &dyn Fn() -> SmeltFuture<String>)"),
        "a borrowed callback's future return is the promise value:\n{source}"
    );
    assert!(
        !source.contains("Result<SmeltFuture<String>"),
        "an async throw is a rejected future, not an outer Result:\n{source}"
    );
}

/// A call through a function *value* pads the parameters the source omitted.
///
/// A callee's optional trailing parameters are part of the Rust `dyn Fn` type
/// the value lowered to, so JavaScript's "omitted arguments are `undefined`"
/// has to be materialized at the call site. `indirect_call_args_text` rendered
/// only the arguments the source supplied, so a `try`-wrapped `work()` against
/// an `Rc<dyn Fn(Option<String>) -> f64>` emitted `(work)()` and rustc rejected
/// it with E0057 ("this function takes 1 argument but 0 arguments were
/// supplied") — the radash `async.test.ts` blocker, where a captured
/// `const fakeWork = (name?: string) => …` was invoked as `fakeWork()` inside a
/// `try`.
///
/// The `try` matters: it routes the call through the throwing-call terminator,
/// which renders its callee with `call_text` instead of the statement path that
/// receives an already-padded argument list from MIR. The load-bearing
/// assertion is the emitted `None::<String>` inside `catch_unwind`: the
/// static-call ladder has always padded missing trailing parameters, and this
/// proves the value-callable form now agrees with it instead of dropping the
/// arity.
#[test]
fn a_value_call_pads_the_optional_parameters_the_source_omitted() {
    let source = source_for(
        r"
export function outer(): number {
  const work = (name?: string): number => 7;
  const runA = (): number => {
    try {
      return work();
    } catch (e) {
      return 0;
    }
  };
  const runB = (): number => {
    try {
      return work();
    } catch (e) {
      return 1;
    }
  };
  return runA() + runB();
}
",
    );

    assert!(
        source.contains("|closure_arg_0: Option<String>|"),
        "the callback's optional parameter must survive into its Rust type:\n{source}"
    );
    assert!(
        source.contains("(work)(None::<String>)"),
        "the omitted optional argument must be padded with its default, not \
         dropped (E0057):\n{source}"
    );
    assert!(
        !source.contains("(work)()"),
        "a zero-argument call against a one-parameter `Fn` does not compile:\n{source}"
    );
}

/// A `try`/`catch` inside an `if` arm ends the arm; it does not `continue`.
///
/// `emit_block_until_goto` serves two structurally different regions: the body
/// of a generated `loop`, where the stop block is the loop header and an edge to
/// it is the back edge, and a forward `if`/`else` arm, where the stop block is
/// the join the caller emits once after the region. Its throwing-call and
/// throwing-`await` forks hardcoded `Continuation::InLoop { continue_target:
/// stop, .. }` for both, so a `try`/`catch` in an `if` arm rendered the edge to
/// the join as `continue;` with no enclosing loop — rustc E0268 (`continue`
/// outside of a loop), the radash `async.ts` `guard` blocker. `RegionExit` now
/// tells the two shapes apart at every call site.
///
/// The load-bearing assertions are the absence of `continue`/`break` together
/// with the presence of the `catch_unwind` fork: dropping the fork would also
/// remove the `continue`, and would silently delete the `catch` clause.
#[test]
fn a_try_catch_inside_an_if_arm_does_not_emit_continue_outside_a_loop() {
    let source = source_for(
        r"
export function guarded(func: () => any, recover: (err: any) => any): any {
  const isPromise = (result: any): result is Promise<any> =>
    result instanceof Promise;
  try {
    const result = func();
    return isPromise(result) ? result.catch(recover) : result;
  } catch (err) {
    return recover(err);
  }
}
",
    );

    let start = source.find("fn guarded").expect("guarded present");
    let after = &source[start..];
    let end = after.find("\n}\n").expect("guarded closing brace");
    let body = &after[..end];

    assert!(
        !body.contains("loop {") && !body.contains("while "),
        "the source has no loop, so the emitted body must have none either:\n{body}"
    );
    assert!(
        !body.contains("continue;"),
        "an edge to a forward join is not a back edge; `continue` here is \
         E0268 (`continue` outside of a loop):\n{body}"
    );
    assert!(
        !body.contains("break;"),
        "there is no loop to break out of either:\n{body}"
    );
    assert!(
        body.contains("catch_unwind"),
        "the `try`/`catch` fork must still be emitted, not dropped along with \
         the bogus back edge:\n{body}"
    );
}

#[test]
fn an_element_read_in_an_optional_slot_stays_fallible() {
    // `arr[i]` has TypeScript type `T` (there is no
    // `noUncheckedIndexedAccess` in play), so `last<T>(arr: T[]): T | undefined`
    // used to lower to an INFALLIBLE read that was then re-wrapped:
    // `Some(arr.get(..).cloned().unwrap_or(Default::default()))`. That makes
    // `last([])` answer `Some(0.0)` where JavaScript answers `undefined`. The
    // read is the natural `Option` producer, so a coercion into an `Option<T>`
    // slot must keep the miss a miss.
    let source = source_for(
        r"
export function last<T>(arr: readonly T[]): T | undefined {
  return arr[arr.length - 1];
}

export function head<T>(arr: readonly T[]): T | undefined {
  return arr[0];
}
",
    );

    assert!(
        !source.contains("Some(arr.get("),
        "the read must not be made total and re-wrapped in `Some(..)`:\n{source}"
    );
    assert!(
        !source.contains("unwrap_or(Default::default())"),
        "an out-of-range element must be `None`, not the element default:\n{source}"
    );
    assert_eq!(
        source.matches(".cloned();").count(),
        2,
        "both functions must return the bare `get(..).cloned()` option:\n{source}"
    );
}

#[test]
fn an_element_read_never_panics_on_a_negative_normalized_index() {
    // A positive out-of-range index already produced the JavaScript answer
    // (`Vec::get` misses and the emitter substitutes the missing value), but a
    // still-negative normalized index went through
    // `usize::try_from(normalized).expect("negative index out of bounds")` and
    // aborted the program. `[][-1]` is `undefined` in JavaScript, so both
    // directions of out-of-range must agree. `usize::MAX` is never a live slot
    // of a `Vec`, so converting the miss to it makes the following `get` miss.
    let source = source_for(
        r"
export function pick(arr: number[], index: number): number {
  return arr[index];
}
",
    );

    assert!(
        !source.contains("negative index out of bounds"),
        "an element READ must not panic on an out-of-range index:\n{source}"
    );
    assert!(
        source.contains("usize::try_from(normalized).unwrap_or(usize::MAX)"),
        "the normalized index must degrade to a miss:\n{source}"
    );
}

#[test]
fn a_typescript_element_read_does_not_wrap_a_negative_index() {
    // `arr[-1]` is `undefined` in JavaScript. It is a PROPERTY key, not a
    // position: only `Array.prototype.at` counts back from the end. The shared
    // index normalizer applied Python's `len + index` wrap to both frontends, so
    // a generated TypeScript `arr[-1]` silently answered `arr[arr.length - 1]` —
    // a wrong VALUE, with no panic and no diagnostic to notice it by. This is
    // what moved es-toolkit's `at` rows: `at(['a','b','c'], [-4])` normalizes to
    // `-1` in source and must then miss, not wrap around to `'c'`.
    let source = source_for(
        r"
export function pick(arr: number[], index: number): number {
  return arr[index];
}
",
    );

    assert!(
        !source.contains("if index < 0 { len + index }"),
        "a TypeScript element read must not count a negative index from the end:\n{source}"
    );
    assert!(
        source.contains("let normalized = index as i64;"),
        "the index must reach the out-of-range machinery unchanged:\n{source}"
    );
}

#[test]
fn a_python_element_read_still_wraps_a_negative_index() {
    // The contrast that makes the rule above a per-site policy rather than a
    // global one: Python's `xs[-1]` IS the last element. One crate can mix
    // TypeScript and Python modules, so the wrap cannot be switched off in the
    // emitter — it is carried on the lowered place, from the source language of
    // the file the expression was written in.
    let source = source_for_py(
        r"
values: list[int] = [1, 2, 3]
last_value: int = values[-1]
",
    );

    assert!(
        source.contains("let normalized = if index < 0 { len + index } else { index }"),
        "a Python element read must still count a negative index from the end:\n{source}"
    );
}

#[test]
fn an_element_write_still_rejects_a_negative_normalized_index() {
    // The read relaxation must not leak into the WRITE path. A store to a slot
    // that does not exist cannot silently pick a different slot, and
    // `usize::MAX` would ask the resize helper for an impossible allocation, so
    // the write keeps the panic.
    let source = source_for(
        r"
export function put(arr: number[], index: number, value: number): void {
  arr[index] = value;
}
",
    );

    assert!(
        source.contains("usize::try_from(normalized).expect(\"negative index out of bounds\")"),
        "a write to a negative normalized index must still fail loudly:\n{source}"
    );
}

#[test]
fn an_element_read_in_an_erased_slot_stays_fallible() {
    // Sibling of `an_element_read_in_an_optional_slot_stays_fallible`, and the
    // two must agree: an out-of-range element read is `undefined` in
    // JavaScript, so the `Option<..>` target answers `None` and the ERASED
    // target must answer `SmeltUnknown::Undefined`.
    //
    // The erased target used to make the read TOTAL first and erase afterwards,
    // so the miss became the element type's own missing value and erased as
    // THAT: `row[i] = b[i]` for `b: string[]` stored `''`, and for
    // `number[]` it stored `0`. Both are values JavaScript never produces here.
    let source = source_for(
        r"
export function fill(b: string[], n: number[], nested: string[][], i: number): unknown[] {
  const row: unknown[] = [0, 0, 0];
  row[0] = b[i];
  row[1] = n[i];
  row[2] = nested[i];
  return row;
}
",
    );

    assert!(
        !source.contains("SmeltUnknown::String(b.get("),
        "the read must not be made total and erased as the element default:\n{source}"
    );
    assert!(
        !source.contains("b.get({ let len = b.len()")
            || !source.contains("unwrap_or(String::new()).clone())"),
        "a missing `string` element must not erase as the empty string:\n{source}"
    );
    assert!(
        !source.contains("SmeltUnknown::Number(n.get("),
        "a missing `number` element must not erase as zero:\n{source}"
    );
    assert!(
        !source.contains("{ let smelt_l = nested.get("),
        "a missing nested-list element must not erase as an empty array:\n{source}"
    );
    assert_eq!(
        source.matches(".cloned().map(|value| ").count(),
        3,
        "each of the three reads keeps its own fallibility:\n{source}"
    );
    for tail in [
        ".cloned().map(|value| SmeltUnknown::String(value.into())).unwrap_or(SmeltUnknown::Undefined)",
        ".cloned().map(|value| SmeltUnknown::Number(value as f64)).unwrap_or(SmeltUnknown::Undefined)",
        "collect::<Vec<_>>())) }).unwrap_or(SmeltUnknown::Undefined)",
    ] {
        assert!(
            source.contains(tail),
            "the miss must erase as `undefined`, not as the element default \
             (`{tail}`):\n{source}"
        );
    }
}

#[test]
fn an_element_read_in_a_concrete_slot_stays_total() {
    // The erased-target rule must not leak into a CONCRETE destination. There
    // is no `undefined` to put in a `Vec<f64>` or a `Vec<String>`, so a store
    // into a concrete list keeps the existing total read and its element
    // missing value. Making that read fallible would not type-check, and
    // widening the slot to hold a hole is a storage question, not a
    // read-coercion one.
    let source = source_for(
        r"
export function copy(b: string[], n: number[], i: number): void {
  const s: string[] = [''];
  const m: number[] = [0];
  s[0] = b[i];
  m[0] = n[i];
}
",
    );

    // The subject is that the read stays TOTAL (`.cloned().unwrap_or(default)`,
    // not a fallible read). The trailing `.clone()` these once carried was the
    // redundant second copy of an already-owned index read, removed by
    // `index_place_read_is_owned`, and is asserted absent below.
    assert!(
        source.contains(".cloned().unwrap_or_else(|| String::new())"),
        "a concrete `string` slot keeps the total read:\n{source}"
    );
    assert!(
        source.contains(".cloned().unwrap_or_else(|| 0.0);"),
        "a concrete `number` slot keeps the total read:\n{source}"
    );
    assert!(
        !source.contains(".cloned().map(|value| "),
        "no fallible erased read belongs in a concrete slot:\n{source}"
    );
}

#[test]
fn an_element_read_on_an_erased_receiver_misses_to_undefined() {
    // The same rule with the receiver erased instead of the destination: when
    // the base is a `SmeltUnknown` the read goes through `unknown_index_text`,
    // which answered `SmeltUnknown::Null` for an out-of-range array or string
    // index. That is the `zipWith` defect — ragged inputs produced `"3null"`
    // where JavaScript produces `"3undefined"`. A missing OBJECT PROPERTY is a
    // separate question and deliberately still answers `Null`.
    let source = source_for(
        r"
export function pick(value: unknown, index: number): unknown {
  return (value as any)[index];
}
",
    );

    assert!(
        source.contains(
            "and_then(|index| values.get(index).cloned()).unwrap_or(SmeltUnknown::Undefined)"
        ),
        "a missing array element on an erased receiver is `undefined`:\n{source}"
    );
    assert!(
        source.contains(
            "value.chars().nth(index).map(|ch| SmeltUnknown::String(ch.to_string().into()))).unwrap_or(SmeltUnknown::Undefined)"
        ),
        "a missing string character on an erased receiver is `undefined`:\n{source}"
    );
}

/// A callback adapter must never be a constant.
///
/// A throwing arrow whose body only throws has the uninhabited return type
/// `never`, and every coercion out of `never` renders a bare constant because
/// there is no value to convert. The adapter that bridges such an arrow into a
/// non-throwing `&dyn Fn() -> unknown` parameter used that constant as its
/// whole body, so the emitted closure never mentioned — let alone called — the
/// callback it was wrapping:
///
/// ```rust
/// attempt(&mut { let _smelt_adapted_callback = ..; move || SmeltUnknown::Null })
/// ```
///
/// Nothing about that is visible in the types: it compiles and returns a
/// plausible value while the callback is silently discarded. This asserts on
/// the emitted source so the shape cannot come back even where no runtime tier
/// covers it.
#[test]
fn a_never_returning_callback_adapter_still_invokes_the_callback() {
    let source = source_for(
        r#"
function attempt(func: () => unknown): unknown[] {
  try {
    return [null, func()];
  } catch (error) {
    return ["caught", null];
  }
}

export function run(): unknown[] {
  return attempt(() => {
    throw new Error("boom");
  });
}
"#,
    );

    assert!(
        !source.contains("move || SmeltUnknown::Null"),
        "the adapter must not collapse to a constant:\n{source}"
    );
    assert!(
        source.contains("(_smelt_adapted_callback)()"),
        "the adapter body must invoke the wrapped callback:\n{source}"
    );
}

/// The same rule for a `void` source rather than a `never` one.
///
/// `isMatch(target, source)` calls `isMatchWith(target, source, () => undefined)`:
/// a zero-argument `void` arrow adapted into a `(a, b, prop, aParent, bParent,
/// stack) => boolean | undefined` customizer slot. A `void` source has no value
/// to convert either, so the coercion answered with the slot's missing-value
/// constant `None::<bool>` and dropped the call — the same defect as the
/// `never` case wearing a different constant. es-toolkit's `isEqual` (which
/// passes `noop`) has the identical shape.
///
/// The repair is keyed on the source having no value, not on the constant, so
/// both spellings are covered by one rule.
#[test]
fn a_void_callback_adapter_still_invokes_the_callback() {
    let source = source_for(
        r"
function match(
  target: unknown,
  source: unknown,
  customizer: (a: unknown, b: unknown) => boolean | undefined
): boolean {
  return customizer(target, source) ?? false;
}

export function run(target: unknown, source: unknown): boolean {
  return match(target, source, () => undefined);
}
",
    );

    assert!(
        !source.contains("move |arg0: SmeltUnknown, arg1: SmeltUnknown| None::<bool>"),
        "the adapter must not collapse to the slot's missing-value constant:
{source}"
    );
    assert!(
        source.contains("let _ = (_smelt_adapted_callback)()"),
        "a `void` source must still be called for its effects:
{source}"
    );
}

/// JavaScript `&&` and `||` select an OPERAND; the result type is the union of
/// the operand types, not `boolean`.
///
/// Modelling them as boolean operators discards the value. es-toolkit's
/// `expect(error instanceof Error && error.message).toBe('test')` became a
/// `bool` compared against a string — statically false, so the assertion folded
/// to `!(false)` and tested nothing. Here the guard's right operand is a
/// concrete `String`, so the selected value must reach the caller as a string.
#[test]
fn a_logical_and_emits_the_selected_operand_not_a_boolean() {
    let source = source_for(
        r"
export function pick(flag: boolean, value: string): string | boolean {
  return flag && value;
}
",
    );

    assert!(
        source.contains("fn pick(flag: bool, value: String) -> SmeltUnion"),
        "`boolean && string` is the union of its operand types:\n{source}"
    );
    assert!(
        source.contains("{ SmeltUnion2::M1(value.clone()) } else { SmeltUnion2::M0(flag) }"),
        "each arm must carry the operand JavaScript selects, not a boolean:\n{source}"
    );
}

/// The operand-selecting rule must leave the common case alone.
///
/// Both operands boolean means the union of the operand types IS `bool`, so the
/// existing boolean lowering is exactly right. Widening it would route ordinary
/// guards through a union — and one step further through `SmeltUnknown` — for
/// no gain.
#[test]
fn a_boolean_logical_and_stays_a_plain_boolean() {
    let source = source_for(
        r"
export function both(a: boolean, b: boolean): boolean {
  return a && b;
}
",
    );

    assert!(
        source.contains("fn both(a: bool, b: bool) -> bool"),
        "a boolean `&&` keeps its boolean signature:\n{source}"
    );
    assert!(
        !source.contains("SmeltUnknown"),
        "a boolean `&&` must not erase anything:\n{source}"
    );
}

/// A condition only observes truthiness, so it keeps the short-circuiting
/// branch shape rather than materializing the selected operand.
///
/// `truthy(a && b) == truthy(a) && truthy(b)`, so `if (a && b)` has no reason to
/// build a union value and test it. The branch form also keeps the right
/// operand's own statements inside the branch: flattening the condition to a
/// single `&&` over two already-computed temporaries evaluates the right-hand
/// call unconditionally.
#[test]
fn a_logical_condition_short_circuits_the_right_operand() {
    let source = source_for(
        r"
function valid(value: number): boolean {
  return value > 0;
}

export function count(a: number, b: number): number {
  if (valid(a) && valid(b)) {
    return 1;
  }
  return 0;
}
",
    );

    let first = source
        .find("valid(a)")
        .expect("the left guard should be called");
    let second = source
        .find("valid(b)")
        .expect("the right guard should be called");
    let branch = source
        .find("if _smelt_tmp")
        .expect("the guard should branch");
    assert!(
        first < branch && branch < second,
        "the right operand must be evaluated inside the branch, not before it:\n{source}"
    );
}

/// A subclass's `impl` block carries the methods it inherits.
///
/// Smelt flattens inheritance — a subclass struct stores its base's fields
/// inline — but Rust has no method inheritance, so a base method is only
/// callable on a subclass receiver if it is emitted into the subclass's own
/// `impl`. Without that, a body calling an inherited method lowered fine in the
/// frontend and then failed to compile with `no method named 'fetch' found for
/// reference '&B'`.
#[test]
fn subclass_impl_block_carries_inherited_methods() {
    let source = source_for_py(
        r"
class A:
    def __init__(self, x: int) -> None:
        self.x = x
    def fetch(self) -> int:
        return self.x

class B(A):
    def __init__(self, x: int, y: int) -> None:
        self.x = x
        self.y = y
    def total(self) -> int:
        return self.fetch() + self.y
",
    );

    let impl_b = source
        .split("impl B {")
        .nth(1)
        .unwrap_or_else(|| panic!("expected an `impl B` block:\n{source}"));
    assert!(
        impl_b.contains("fn fetch(&self)"),
        "`B` must carry the inherited `fetch`, or `self.fetch()` cannot \
         compile:\n{source}"
    );
}

/// An override replaces the inherited slot instead of emitting a second method
/// of the same name, which would not compile.
#[test]
fn subclass_override_replaces_the_inherited_method() {
    let source = source_for_py(
        r"
class A:
    def __init__(self, x: int) -> None:
        self.x = x
    def fetch(self) -> int:
        return self.x

class B(A):
    def fetch(self) -> int:
        return 99
",
    );

    let impl_b = source
        .split("impl B {")
        .nth(1)
        .unwrap_or_else(|| panic!("expected an `impl B` block:\n{source}"));
    assert_eq!(
        impl_b.matches("fn fetch(&self)").count(),
        1,
        "the override must replace the inherited slot, not duplicate it:\n{source}"
    );
    assert!(
        impl_b.contains("return 99"),
        "the surviving `fetch` must be the override:\n{source}"
    );
}

/// Python `super().method()` calls the immediate base implementation without
/// recursing into the derived override.
#[test]
fn python_super_method_uses_a_typed_base_alias() {
    let source = source_for_py(
        r"
class A:
    def greet(self, value: int) -> int:
        return value + 1

class B(A):
    def greet(self, value: int) -> int:
        return super().greet(value) + 10
",
    );

    let impl_b = source
        .split("impl B {")
        .nth(1)
        .unwrap_or_else(|| panic!("expected an `impl B` block:\n{source}"));
    assert!(
        impl_b.contains("fn __smelt_super_greet(&self, value: i64) -> i64"),
        "the base implementation must be available under a typed alias:\n{source}"
    );
    assert!(
        impl_b.contains("self.__smelt_super_greet(value)"),
        "super().greet(value) must call the base alias, not the override:\n{source}"
    );
}

/// A Python `__add__` method becomes a concrete Rust `Add<Rhs>` implementation.
#[test]
fn python_add_dunder_emits_a_typed_rust_trait_impl() {
    let source = source_for_py(
        r#"
class Vector:
    def __init__(self, x: int) -> None:
        self.x = x
    def __add__(self, other: "Vector") -> "Vector":
        return Vector(self.x + other.x)

def combine(left: Vector, right: Vector) -> Vector:
    return left + right
"#,
    );

    assert!(
        source.contains("impl ::std::ops::Add<Vector> for Vector"),
        "Python __add__ must map to Rust's typed Add trait:\n{source}"
    );
    assert!(
        source.contains("type Output = Vector;")
            && source.contains("fn add(self, rhs: Vector) -> Self::Output { self.__add__(rhs) }"),
        "the Rust adapter must preserve the concrete operand and output types:\n{source}"
    );
    assert!(
        source.contains("left.clone() + right.clone()"),
        "Python + must use the typed Rust operator without consuming source bindings:\n{source}"
    );
}

/// A Python `super().__init__(..)` runs the base constructor and copies the
/// flattened base slots onto the derived instance, composing across levels.
#[test]
fn python_super_init_composes_across_inheritance_levels() {
    let source = source_for_py(
        r"
class A:
    def __init__(self, x: int) -> None:
        self.x = x

class B(A):
    def __init__(self, x: int, y: int) -> None:
        super().__init__(x)
        self.y = y

class C(B):
    def __init__(self) -> None:
        super().__init__(1, 2)
        self.z = 3
",
    );

    assert!(
        source.contains("let __smelt_super: B = "),
        "`C` must construct its immediate base `B`:\n{source}"
    );
    // `C::new` copies BOTH inherited slots out of the constructed `B`, so the
    // base-most field reaches the leaf instance.
    assert!(
        source.contains("self_.x = __smelt_super.x") && source.contains("self_.y = __smelt_super.y"),
        "both inherited slots must reach the leaf instance:\n{source}"
    );
}

/// A Python `@property` getter emits a Rust method, and a read through the
/// property's *field* syntax emits the call.
///
/// Python spells the definition as a method and the use as a field; Rust has no
/// properties, so the hand-written mapping is a plain method plus a call at the
/// use site. The property must not become a struct field of its own.
#[test]
fn python_property_emits_a_getter_method_and_call() {
    let source = source_for_py(
        r"
class Ok:
    def __init__(self, value: int) -> None:
        self._value = value

    @property
    def ok_value(self) -> int:
        return self._value

def read(o: Ok) -> int:
    return o.ok_value
",
    );

    assert!(
        source.contains("fn ok_value(&self) -> i64"),
        "the property getter must emit as a method:\n{source}"
    );
    assert!(
        source.contains("o.ok_value()"),
        "a property read must emit the getter call, not a field access:\n{source}"
    );
    let struct_body = source
        .split("struct Ok {")
        .nth(1)
        .and_then(|rest| rest.split('}').next())
        .unwrap_or_else(|| panic!("expected a generated `struct Ok`:\n{source}"));
    assert!(
        !struct_body.contains("ok_value"),
        "a property is computed, so it must not also become a struct field:\n{source}"
    );
}

/// A method call on a union receiver dispatches by matching the tagged enum,
/// with no dynamic erasure.
///
/// The emitter has always been able to do this (`union_method_text`); what was
/// missing was the Python frontend emitting an ordinary `Method` expression for
/// a union-typed receiver instead of rejecting it. The arms are concrete
/// classes, so the call must stay concrete — routing it through `SmeltUnknown`
/// would be exactly the "reconcile concrete union arms" erasure the project
/// forbids.
#[test]
fn python_union_receiver_method_dispatches_statically() {
    let source = source_for_py(
        r"
class Ok:
    def is_ok(self) -> bool:
        return True

class Err:
    def is_ok(self) -> bool:
        return False

def check(r: Ok | Err) -> bool:
    return r.is_ok()
",
    );

    // Cut at the end of `check` itself: the tail of the file holds the class
    // `impl` blocks, whose erased-prototype adapters legitimately mention
    // `SmeltUnknown`, and the assertion below is about this body only.
    let check_body = source
        .split("fn check(")
        .nth(1)
        .and_then(|tail| tail.split_once("\n}\n").map(|(body, _)| body))
        .unwrap_or_else(|| panic!("expected a generated `check`:\n{source}"));
    assert!(
        check_body.contains("M0(value) => value.is_ok()")
            && check_body.contains("M1(value) => value.is_ok()"),
        "each union arm must call its own concrete method:\n{source}"
    );
    assert!(
        !check_body.contains("SmeltUnknown"),
        "a concrete union receiver must not erase to a dynamic value:\n{source}"
    );
}

/// A TypeScript method call on a union receiver dispatches by matching the
/// tagged enum instead of erasing the receiver.
///
/// `resolve_method` has always computed the per-arm result type for a union and
/// documented that "MIR retains the typed call and dispatches over the concrete
/// tagged-union arms". What defeated it was the callable-field path running
/// first: its concreteness guard demanded a single method item, which a union
/// deliberately never has, so the call was lowered as a dynamic field read and
/// invoked through the `SmeltUnknown` call ABI.
#[test]
fn typescript_union_receiver_method_dispatches_statically() {
    let source = source_for(
        r"
class Ok { isOk(): boolean { return true; } }
class Err { isOk(): boolean { return false; } }
function check(r: Ok | Err): boolean { return r.isOk(); }
const v = check(new Ok());
",
    );

    let check_body = source
        .split("fn check(")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or_else(|| panic!("expected a generated `check`:\n{source}"));
    assert!(
        check_body.contains("M0(value) => value.is_ok()")
            && check_body.contains("M1(value) => value.is_ok()"),
        "each union arm must call its own concrete method:\n{source}"
    );
    assert!(
        !check_body.contains("SmeltUnknown"),
        "a concrete union receiver must not erase to a dynamic value:\n{source}"
    );
    // The dynamic call ABI is fallible, so erasing also infected the signature
    // with a `Result`. Static dispatch cannot throw.
    assert!(
        check_body.starts_with("r: SmeltUnion5) -> bool"),
        "static dispatch must not wrap the return type in `Result`:\n{source}"
    );
}

/// A callable object invoked inside a callback body must call its underlying
/// callable, not silently evaluate to `null`.
///
/// `Rvalue::ClosureCall` used to answer `default_value(dest_ty)` for every
/// callee that was not a `Type::Function`, so a callable-object record — which
/// *is* callable, through its synthetic `__smelt_call` slot — had its entire
/// call replaced by a null with no diagnostic. es-toolkit `memoize` (whose
/// result type is `F & { cache }`) lost every `memoized(value)` inside a
/// `props.map(..)` callback that way.
#[test]
fn typescript_callable_object_call_in_callback_invokes_the_call_slot() {
    let source = source_for(
        r"
interface Memo {
  (value: string): string;
  cache: number;
}

function makeMemo(fn: (value: string) => string): Memo {
  const memoized = function (value: string): string {
    return fn(value);
  };
  memoized.cache = 0;
  return memoized as Memo;
}

export function run(): string[] {
  const memoized = makeMemo(v => v);
  return ['a', 'b'].map(value => memoized(value));
}
",
    );

    let run_body = source
        .split("fn run(")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or_else(|| panic!("expected a generated `run`:\n{source}"));
    assert!(
        run_body.contains("__smelt_call"),
        "calling a callable object must go through its `__smelt_call` slot:\n{source}"
    );
    assert!(
        !run_body.contains("let _smelt_tmp_4: SmeltUnknown = SmeltUnknown::Null;\n    _smelt_tmp_4"),
        "the call must not be replaced by a null default:\n{source}"
    );
}

/// `Function.prototype.length` on a borrowed callable handle is its declared
/// arity, emitted as a constant.
///
/// Only the `SmeltErasedFunction` struct carries the arity in a `length` field.
/// A callback *parameter* is emitted as a borrowed `&dyn Fn` handle, which has
/// no field storage, so keying the choice on the MIR shape rather than on the
/// emitted representation produced `func.length` on a `&dyn Fn` (E0609).
#[test]
fn typescript_function_length_on_borrowed_handle_is_the_declared_arity() {
    let source = source_for(
        r"
export function arity(cb: (a: number, b: number) => void): number {
  return cb.length;
}
",
    );

    let body = source
        .split("fn arity(")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or_else(|| panic!("expected a generated `arity`:\n{source}"));
    assert!(
        body.contains("2.0"),
        "a borrowed handle's `.length` is its declared parameter count:\n{source}"
    );
    assert!(
        !body.contains("cb.length"),
        "a borrowed `&dyn Fn` has no `length` field to read:\n{source}"
    );
}

/// A callback parameter captured into a returned callable object must enter its
/// function owned.
///
/// The returned record OWNS its `__smelt_call` slot as a `'static` handle, so a
/// borrowed `&dyn Fn` parameter captured into it cannot satisfy the coercion
/// ("coercion requires that `'1` must outlive `'static`" in es-toolkit `curry`).
/// The escape analysis walks container types for exactly this reason; a class or
/// interface is storage in the same way.
#[test]
fn typescript_callback_captured_into_returned_callable_object_is_owned() {
    let source = source_for(
        r"
interface Wrapped {
  (value: string): string;
  tag: number;
}

export function wrap(fn: (value: string) => string): Wrapped {
  const wrapped = function (value: string): string {
    return fn(value);
  };
  wrapped.tag = 1;
  return wrapped as Wrapped;
}
",
    );

    let signature = source
        .split("fn wrap(")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .unwrap_or_else(|| panic!("expected a generated `wrap`:\n{source}"));
    assert!(
        !signature.contains("&dyn Fn"),
        "a callback retained by the returned record must not stay borrowed:\n{source}"
    );
}

/// A module-level regex `const` referenced from several functions must compile
/// its pattern exactly once.
///
/// The TypeScript frontend inlines an importable `const` initializer into every
/// referencing body, so the `SmeltRegExp::new(..)` construction is pasted at each
/// use site. That construction must stay per-use: `SmeltRegExp` carries JS
/// reference identity (`id`) and observable mutable state (`lastIndex`), and
/// `Clone` preserves both, so handing every use a clone of one shared instance
/// would fuse distinct source objects. What *is* shared is the pure half — the
/// compiled `fancy_regex` automaton, a function of the pattern text alone — which
/// the prelude memoizes in `SMELT_REGEX_CACHE`. The invariant this test pins is
/// therefore: exactly ONE `fancy_regex::Regex::new` call site exists in the whole
/// emitted crate (the memo), no matter how many times the const is inlined.
#[test]
fn module_level_regex_const_compiles_its_pattern_once() {
    let source = source_for(
        r"
const CASE_SPLIT_PATTERN = /[a-z]+|[0-9]+/g;

export function first(text: string): number {
  return text.split(CASE_SPLIT_PATTERN).length;
}

export function second(text: string): number {
  return text.split(CASE_SPLIT_PATTERN).length;
}
",
    );

    assert!(
        source.contains("static SMELT_REGEX_CACHE:"),
        "the prelude must declare the compiled-automaton memo\n{source}"
    );
    assert_eq!(
        source.matches("fancy_regex::Regex::new(").count(),
        1,
        "the emitted crate must hold exactly one regex compile site (the memo), \
         so an inlined module-level const never recompiles its pattern\n{source}"
    );
    assert!(
        source.matches("SmeltRegExp::new(").count() >= 2,
        "each use site must still build its own `SmeltRegExp` wrapper so JS \
         reference identity and `lastIndex` stay per-object\n{source}"
    );
    assert!(
        source.contains("cache.borrow().get(&pattern).cloned()")
            && source.contains("cache.borrow_mut().insert(pattern, compiled.clone())"),
        "`try_compiled` must read through and populate the memo\n{source}"
    );
}

/// A conditional whose branches are two *declared* classes unifies to the
/// generated tagged union, not to `String`.
///
/// `Type::Class` spells both a declared class and an unresolved opaque name, so
/// `is_string_compatible_type` accepts any class — a JS value can always be
/// coerced to a string. Applying that to *unification* made
/// `flag ? new A() : new B()` come out as `String`, and the emitter then
/// declared a `String` local and assigned the class values into it: output that
/// does not compile. Two declared classes do have a concrete common
/// representation, so they unify to their union.
#[test]
fn conditional_over_declared_classes_unifies_to_a_union() {
    let source = source_for(
        r"
class A { v(): number { return 1; } }
class B { v(): number { return 2; } }
function pick(flag: boolean): A | B { return flag ? new A() : new B(); }
function use1(x: A | B): number { return x.v(); }
const r = use1(pick(true));
",
    );

    let pick_body = source
        .split("fn pick(")
        .nth(1)
        .and_then(|rest| rest.split("\nfn ").next())
        .unwrap_or_else(|| panic!("expected a generated `pick`:\n{source}"));
    assert!(
        !pick_body.contains(": String"),
        "two declared classes must not unify to `String`:\n{source}"
    );
    assert!(
        pick_body.contains("::M0(") && pick_body.contains("::M1("),
        "each branch must be wrapped into its union arm:\n{source}"
    );
}

/// An unannotated arrow passed to a *generic* function is contextually typed
/// from the instantiation its sibling arguments imply, not from the callee's
/// raw type parameters.
///
/// The declared parameter type of `fn` is `(item: T) => U`. Handing that to the
/// arrow as its contextual type bound the closure's parameter to a type
/// variable outside its own scope, which lowered to `SmeltUnknown` — and then
/// the already-concrete `number[]` argument had to be erased to match the
/// instantiation, and the result un-erased again. One missing annotation cost
/// four erasure sites. The bindings are recoverable from the value arguments,
/// which is what this asserts.
#[test]
fn unannotated_arrow_into_generic_callee_stays_concrete() {
    let source = source_for(
        r"
function mapArr<T, U>(arr: T[], fn: (item: T) => U): U[] {
  const out: U[] = [];
  for (const item of arr) { out.push(fn(item)); }
  return out;
}
const nums: number[] = [1, 2, 3];
const doubled = mapArr(nums, (x) => x * 2);
",
    );

    let main_body = source
        .split("fn main()")
        .nth(1)
        .unwrap_or_else(|| panic!("expected a generated `main`:\n{source}"));
    assert!(
        !main_body.contains("SmeltUnknown"),
        "the call site must stay concrete, with no erasure round-trip:\n{source}"
    );
    assert!(
        main_body.contains("closure_arg_0: f64"),
        "the arrow parameter must be contextually typed as `f64`:\n{source}"
    );
    assert!(
        main_body.contains("SmeltList<f64>"),
        "the concrete argument must not be re-erased to call the instantiation:\n{source}"
    );
}

/// A constrained type parameter erases only itself; an unconstrained sibling
/// still lifts to a real Rust generic.
///
/// `function_emits_rust_generics` used to demote a whole function the moment any
/// type parameter carried a constraint, so `K extends string` erased `T` too and
/// every position here came out `SmeltUnknown` — even `fallback: T` and the
/// return, which are directly inferable. 215 of es-toolkit's 800 generic
/// exported functions carry a constrained parameter.
#[test]
fn constrained_type_param_erases_without_demoting_its_siblings() {
    let source = source_for(
        r"
function pickFirst<T, K extends string>(arr: T[], key: (item: T) => K, fallback: T): T {
  if (arr.length > 0) { return arr[0]; }
  return fallback;
}
const nums: number[] = [1, 2, 3];
const v = pickFirst(nums, (n) => (n > 1 ? 'big' : 'small'), 0);
",
    );

    let signature = source
        .split("fn pick_first")
        .nth(1)
        .and_then(|rest| rest.split('{').next())
        .unwrap_or_else(|| panic!("expected a generated `pick_first`:\n{source}"));
    assert!(
        signature.contains("arr: SmeltList<T>") && signature.contains("fallback: T"),
        "the unconstrained `T` must lift to a real generic:\n{source}"
    );
    assert!(
        signature.contains("SmeltUnknown"),
        "the constrained `K` must still erase:\n{source}"
    );
}

/// A type parameter inspected inside a *closure* body does not lift, even when
/// the enclosing function body only ever moves it.
///
/// `classes::type_param_only_moved` used to walk `function.blocks` alone. A
/// closure is a separate MIR body with its own locals and blocks, so an
/// inspection inside one was invisible and the parameter lifted anyway.
/// es-toolkit's `flatten<T, D extends number>` is exactly this shape: `T` lifted
/// out of `arr: T[]`, while the `Array.isArray(item)` that inspects it lives in
/// the inner recursive closure. The emitted
/// `matches!(item, SmeltUnknown::Array(_))` then ran against a `T` and the
/// generated library failed to compile with "expected type parameter `T`, found
/// `SmeltUnknown`".
#[test]
fn type_param_inspected_inside_a_closure_does_not_lift() {
    let source = source_for(
        r"
function scan<T, K extends string>(arr: T[], key: (item: T) => K): T[] {
  const out: T[] = [];
  arr.forEach((item) => { if (typeof item === 'string') { out.push(item); } });
  return out;
}
const r = scan(['a', 'b'], (s) => 'k');
",
    );

    let signature = source
        .split("fn scan")
        .nth(1)
        .and_then(|rest| rest.split('{').next())
        .unwrap_or_else(|| panic!("expected a generated `scan`:\n{source}"));
    // The generic parameter list is what must be absent; `SmeltList<..>` in the
    // parameter types legitimately contains angle brackets.
    assert!(
        !signature.starts_with('<'),
        "a closure that inspects `T` must not declare Rust generics:\n{source}"
    );
    assert!(
        signature.contains("arr: SmeltList<SmeltUnknown>"),
        "the inspected `T` must not reach the parameter list:\n{source}"
    );
}

/// A type parameter moved into a differently-typed container does not lift.
///
/// `Rvalue::Use` used to be whitelisted as "a bare move", which short-circuited
/// the destination check. But a move is opacity-preserving only when source and
/// destination agree about `T`: moving a `T` into a slot of another type erases
/// it. es-toolkit's `unzipWith` is exactly this shape — `group` comes from
/// `new Array(n)` so its type is `unknown[]`, each `T` element is moved into it,
/// and the erased list is then passed to a callback whose bound reads
/// `Fn(SmeltList<T>)`, so the generated library failed with "expected `Vec<T>`,
/// found `Vec<SmeltUnknown>`".
#[test]
fn type_param_moved_into_an_erased_container_does_not_lift() {
    let source = source_for(
        r"
function collect<T, K extends string>(rows: T[][], key: (vals: T[]) => K): K[] {
  const out: K[] = [];
  for (let i = 0; i < rows.length; i++) {
    const group = new Array(rows.length);
    for (let j = 0; j < rows.length; j++) { group[j] = rows[j][i]; }
    out.push(key(group));
  }
  return out;
}
const r = collect([[1, 2]], (v) => 'k');
",
    );

    let signature = source
        .split("fn collect")
        .nth(1)
        .and_then(|rest| rest.split('{').next())
        .unwrap_or_else(|| panic!("expected a generated `collect`:\n{source}"));
    assert!(
        !signature.starts_with('<'),
        "a `T` moved into an `unknown[]` must not lift:\n{source}"
    );
}

/// A generated record emits the inbound half of the erasure round-trip.
///
/// Every lifted type parameter is bounded by `SmeltFromUnknown`, so a class that
/// lacks the impl cannot be used as a generic argument at all — es-toolkit's
/// `meanBy`/`medianBy` specs call generic helpers with `Person[]` and failed
/// with "the trait bound `Person: SmeltFromUnknown` is not satisfied". Only
/// `IntoSmeltUnknown` was ever emitted, so concrete class values could flow out
/// to erased code but never back.
#[test]
fn generated_records_emit_from_smelt_unknown() {
    let source = source_for(
        r"
class Person { name: string = ''; age: number = 0; }
function firstOf<T>(items: T[], fallback: T): T {
  if (items.length > 0) { return items[0]; }
  return fallback;
}
const people: Person[] = [];
const p = firstOf(people, new Person());
",
    );

    assert!(
        source.contains("impl SmeltFromUnknown for Person"),
        "a generated record must be recoverable from its erased view:\n{source}"
    );
    assert!(
        source.contains("impl IntoSmeltUnknown for Person"),
        "the outbound half must still be emitted:\n{source}"
    );
}

/// A generated union emits `SmeltJsKeyEq`, so it can be used as a map key.
///
/// Map keys are compared through the erased JavaScript key-equality projection.
/// Without the impl, any generated map keyed by a union fails with "the trait
/// bound `SmeltUnionN: SmeltJsKeyEq` is not satisfied" — which is what
/// es-toolkit's `keyBy` specs hit.
#[test]
fn generated_unions_emit_js_key_equality() {
    let source = source_for(
        r"
function pick(flag: boolean): number | string { return flag ? 1 : 'a'; }
const m = new Map<number | string, number>();
m.set(pick(true), 1);
",
    );

    assert!(
        source.contains("SmeltJsKeyEq for SmeltUnion"),
        "a generated union must support JavaScript key equality:\n{source}"
    );
    assert!(
        source.contains("SmeltFromUnknown for SmeltUnion"),
        "a generated union must be recoverable from its erased view:\n{source}"
    );
}

/// Shared TypeScript prelude for the callable-interface overload tests.
///
/// It mirrors the shape es-toolkit's `curry` declares: a callable interface with
/// several overloads, an implementation whose declared return type is the
/// intersection of a plain callable and its own properties (which is how the
/// underlying function object is modeled), and an overload signature that
/// narrows that value to the interface.
const CALLABLE_OVERLOAD_PRELUDE: &str = r#"
const curryPlaceholder: unique symbol = Symbol('curry.placeholder');
type __ = typeof curryPlaceholder;

interface Curried1<T1, R> {
  (): Curried1<T1, R>;
  (t1: T1): R;
}

interface Curried2<T1, T2, R> {
  (): Curried2<T1, T2, R>;
  (t1: T1): Curried1<T2, R>;
  (t1: __, t2: T2): Curried1<T1, R>;
  (t1: T1, t2: T2): R;
}

export function curry2<T1, T2, R>(fn: (a: T1, b: T2) => R): Curried2<T1, T2, R>;
export function curry2(fn: (...args: any[]) => any): ((...args: any[]) => any) & { placeholder: unknown } {
  const wrapper = function (...args: any[]): any {
    return fn(args[0], args[1]);
  };
  wrapper.placeholder = curryPlaceholder;
  return wrapper;
}

export interface ArityReporter {
  (): string;
  (t1: number): string;
}

export function arityReporter(): ArityReporter;
export function arityReporter(): ((...args: any[]) => any) & { placeholder: unknown } {
  const wrapper = function (...args: any[]): any {
    return String(args.length);
  };
  wrapper.placeholder = curryPlaceholder;
  return wrapper;
}

export interface ByArgumentType {
  (value: string): string;
  (value: number): number;
}

export function byArgumentType(): ByArgumentType;
export function byArgumentType(): ((...args: any[]) => any) & { placeholder: unknown } {
  const wrapper = function (value: any): any {
    return value;
  };
  wrapper.placeholder = curryPlaceholder;
  return wrapper;
}
"#;

/// A call to an overloaded callable interface must not stop at the first
/// overload that merely shares the call's arity.
///
/// es-toolkit's `CurriedFunction2` declares `(t1: __, t2: T2)` — a placeholder
/// position typed by a `unique symbol` — before `(t1: T1, t2: T2): R`. Smelt
/// carries symbols as opaque runtime values, so both parameter positions read
/// as `unknown` and no static rule separates them: which overload runs is
/// decided by the callee comparing its argument against a sentinel. Taking the
/// first of them made `curried(2, 3)` claim to return a `Curried1`, so the
/// comparison against a number const-folded to `false`. Only what every
/// surviving overload agrees on is reported instead.
#[test]
fn ambiguous_same_arity_call_signatures_answer_the_erased_call_result() {
    let source = source_for(&format!(
        "{CALLABLE_OVERLOAD_PRELUDE}
const curried = curry2((a: number, b: number) => a + b);
const total = curried(2, 3);
"
    ));

    assert!(
        source.contains(
            "smelt_callback.call(vec![arg0.clone(), (arg1.clone()).into_smelt_unknown()])"
        ),
        "both arguments must reach the erased callable:\n{source}"
    );
    assert!(
        !source.contains("total: Curried1"),
        "the ambiguous call must not claim one overload's return type:\n{source}"
    );
}

/// A call with more arguments than any declared overload accepts must still
/// pass every argument.
///
/// JavaScript forwards the whole argument list regardless of declared arity,
/// and an overloaded callable interface stores its implementation in one erased
/// variadic `__smelt_call` slot, so the call is executable. Falling back to the
/// first declared signature truncated the argument list at the adapter and
/// called the callee with nothing.
#[test]
fn call_beyond_every_declared_overload_arity_keeps_its_arguments() {
    let source = source_for(&format!(
        "{CALLABLE_OVERLOAD_PRELUDE}
const reporter = arityReporter();
// @ts-ignore
const seen = reporter(1, 2, 3);
"
    ));

    assert!(
        source.contains("smelt_callback.call(vec![arg0.clone(), arg1.clone(), arg2.clone()])"),
        "all three arguments must reach the erased callable:\n{source}"
    );
}

/// Same-arity overloads that differ only by parameter type are selected by the
/// argument's type, and the interface's call slot is erased so one field can
/// store either of the two incompatible Rust `Fn` shapes.
#[test]
fn same_arity_call_signatures_are_selected_by_argument_type() {
    let source = source_for(&format!(
        "{CALLABLE_OVERLOAD_PRELUDE}
const pick = byArgumentType();
const numeric = pick(41);
const text = pick('a');
"
    ));

    assert!(
        source.contains("__smelt_call: SmeltErasedFunction"),
        "an overload set that differs by parameter type needs the erased slot:\n{source}"
    );
    assert!(
        source.contains("numeric: f64") && source.contains("text: String"),
        "each call must take the overload its argument type matches:\n{source}"
    );
}

/// Narrowing a callable object to a callable interface that declares fewer
/// members keeps the dropped members readable as own properties of the
/// underlying function value, the way JavaScript does.
#[test]
fn narrowed_callable_object_keeps_its_own_properties() {
    let source = source_for(&format!(
        "{CALLABLE_OVERLOAD_PRELUDE}
const curried = curry2((a: number, b: number) => a + b);
// @ts-ignore
const marker = curried.placeholder;
"
    ));

    assert!(
        source.contains("smelt_with_properties(vec![(\"placeholder\".to_owned()"),
        "the dropped property must be carried onto the erased callable:\n{source}"
    );
    assert!(
        source.contains(".__smelt_call.smelt_property(\"placeholder\")"),
        "the undeclared member read must resolve through the callable's properties:\n{source}"
    );
}

#[test]
fn guarded_default_insert_becomes_a_single_entry_probe() {
    // `smelt_mir::opt::DictDefaultInsertElision` deletes the
    // `if (!Object.hasOwn(m, k)) { m[k] = []; }` guard, because the entry
    // mutation that follows inserts the same empty list through
    // `entry_or_insert`. The emitted group loop must therefore hash the key
    // once, not three times (probe, guarded insert, entry).
    let source = source_for(
        r"
export function groupBy<T, K extends PropertyKey>(
  arr: readonly T[],
  getKeyFromItem: (item: T, index: number) => K
): Record<K, T[]> {
  const result = {} as Record<K, T[]>;
  for (let i = 0; i < arr.length; i++) {
    const item = arr[i];
    const key = getKeyFromItem(item, i);
    if (!Object.hasOwn(result, key)) {
      result[key] = [];
    }
    result[key].push(item);
  }
  return result;
}
",
    );
    let body = emitted_function_body(&source, "fn group_by(");

    assert!(
        !body.contains("contains_key"),
        "the membership probe is gone: {body}"
    );
    assert_eq!(
        body.matches("entry_or_insert").count(),
        1,
        "exactly one entry probe remains: {body}"
    );
}

#[test]
fn count_by_accumulator_becomes_a_single_entry_probe() {
    // `smelt_mir::opt::DictEntryUpdate` fuses the
    // `result[key] = (result[key] ?? 0) + 1` read/compute/write-back triple, so
    // the emitted loop hashes the key ONCE (one entry accessor) instead of
    // twice (a `get` plus an `insert`).
    let source = source_for(
        r"
export function countBy<T, K extends PropertyKey>(
  arr: readonly T[],
  mapper: (item: T) => K
): Record<K, number> {
  const result = {} as Record<K, number>;
  for (let i = 0; i < arr.length; i++) {
    const key = mapper(arr[i]);
    result[key] = (result[key] ?? 0) + 1;
  }
  return result;
}
",
    );
    let body = emitted_function_body(&source, "fn count_by<");

    assert_eq!(
        body.matches("entry_or_insert").count(),
        1,
        "exactly one entry probe remains: {body}"
    );
    assert!(
        !body.contains(".insert("),
        "the write-back probe is gone: {body}"
    );
    assert!(
        !body.contains(".get(&"),
        "the read probe is gone: {body}"
    );
}
