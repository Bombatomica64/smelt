//! Generated Rust for `Date` and timezone emission.

use super::*;

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
    // A `URL` is the modeled `SmeltUrl`, parsed by the `url` crate, and its
    // `host` getter is the WHATWG accessor (`url::quirks::host`), which keeps
    // a non-default port: `example.com:8443`. The hand-written `host:port`
    // formatting this used to assert is now the crate's.
    assert!(ts_source.contains("url::Url::parse"));
    assert!(ts_source.contains("smelt_url_new_throwing("));
    assert!(ts_source.contains(".host()"));
    assert!(ts_source.contains("::url::quirks::host("));
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
        &EmitOptions::default().with_release_profile(ReleaseProfile::Optimized),
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
function bump<D extends Date>(date: D): string {
  date.setFullYear(2024);
  return date.toISOString();
}
const iso = bump(value as any);
",
    );
    // Old fixture: `new Date(value).setFullYear(..)` on an erased `value`. A
    // `new Date(..)` is now a fresh concrete `SmeltDate` (as in JavaScript, it
    // keeps none of its argument's other properties), so the erased receiver
    // this test is about is a `D extends Date` parameter that stays erased
    // because its call site passes an erased argument.

    assert!(
        source.contains("if smelt_key != \"__smelt_date\""),
        "Date setters must preserve metadata from erased Date receivers: {source}"
    );
    assert!(
        source.contains("smelt_result.insert(smelt_key, smelt_value)"),
        "Date metadata must be copied onto the replacement timestamp object: {source}"
    );
}

#[test]
fn emits_date_to_iso_string_for_erased_datearg_surfaces() {
    let source = source_for(
        r"
declare const value: unknown;
const isoFromUnknown = new Date(value).toISOString();
",
    );

    assert!(
        source.contains("SmeltUnknown::Number(value) => value"),
        "{source}"
    );
    assert!(
        source.contains(
            "SmeltUnknown::String(smelt_value) => { let smelt_value = smelt_value.to_string_lossy(); chrono::DateTime::parse_from_rfc3339(&smelt_value)"
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
    assert!(source.contains("to_iso_string()"), "{source}");
    assert!(source.contains("iso_values: SmeltList<JsString>"), "{source}");
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
        .find("let smelt_normalized_year = (normalized_two_digit_year as i32)")
        .unwrap_or_else(|| panic!("{source}"));
    assert!(normalized < setter, "{source}");
    assert!(source.contains(" as f64"), "{source}");
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
    // Old requirement: the erased `Date` argument was re-tagged with a marker
    // `match`. New (`Date` is the concrete `SmeltDate`): it crosses into the
    // erased callable through the date's boundary adapter, whose record still
    // carries the `__smelt_date` marker.
    assert!(
        source.contains("smelt_call_args.push(smelt_arg0.clone().into_smelt_unknown())"),
        "{source}"
    );
    assert!(source.contains("\"__smelt_date\".to_owned()"), "{source}");
}
