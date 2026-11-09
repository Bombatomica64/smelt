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
    assert_eq!([rust_fn(&ts_source, "main"), rust_fn(&py_source, "read_write")].join("\n\n"), r#"fn main() -> Result<(), Box<dyn std::error::Error>> {
let host: JsString;
let host_with_port: JsString;
let cleaned: JsString;
let cleaned_by_call: JsString;
let cleaned_by_literal: JsString;
let _smelt_tmp_16: SmeltRegExp;
let now: i64 = SMELT_DATE_NOW.with(::std::cell::Cell::get).unwrap_or_else(|| chrono::Utc::now().timestamp_millis().saturating_add(SMELT_VIRTUAL_MS.with(::std::cell::Cell::get) as i64));
let _smelt_tmp_8: SmeltDate = SmeltDate::new((now) as f64);
let iso: JsString = JsString::from((_smelt_tmp_8).to_iso_string());
let _smelt_tmp_10: SmeltUrl = smelt_url_new_throwing(&JsString::from("https://example.com/a?q=1"), None)?;
host = JsString::from(_smelt_tmp_10.hostname());
let _smelt_tmp_12: SmeltUrl = smelt_url_new_throwing(&JsString::from("https://example.com:8443/a?q=1"), None)?;
host_with_port = JsString::from(_smelt_tmp_12.host());
cleaned = SmeltRegExp::new(JsString::from("\\s+"), String::new()).replace_string(&JsString::from("a  b"), &JsString::from("-"), true);
cleaned_by_call = SmeltRegExp::new(JsString::from("\\s+"), String::new()).replace_string(&JsString::from("a  b"), &JsString::from("-"), true);
_smelt_tmp_16 = SmeltRegExp::new(JsString::from("\\s+"), JsString::new());
cleaned_by_literal = _smelt_tmp_16.replace_string(&JsString::from("a  b"), &JsString::from("-"), false);
return Ok(());
}

fn read_write(path: String, text: String) -> Result<String, Box<dyn std::error::Error>> {
let _smelt_tmp_10: String;
let replaced: String = smelt_py_regex_replace(&"\\s+".to_owned(), &text.clone(), &"-".to_owned(), true);
let parts: SmeltList<String> = Into::<SmeltList<_>>::into(fancy_regex::Regex::new(&"\\s+".to_owned()).expect("regex compile failed").split(&text.clone()).filter_map(Result::ok).map(str::to_owned).collect::<Vec<_>>());
let written: i64 = { let smelt_contents = replaced; std::fs::write(&path.clone(), &smelt_contents).expect("file write failed"); smelt_contents.len() as i64 };
let _smelt_tmp_8: String = std::fs::read_to_string(&path.clone()).expect("file read failed");
let _smelt_tmp_9: String = smelt_py_list_read(&parts, 0)?;
_smelt_tmp_10 = _smelt_tmp_8 + &_smelt_tmp_9;
return Ok(_smelt_tmp_10);
}"#);
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

/// A setter on a `D extends Date` that stays erased copies the receiver metadata onto the replacement object.
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

    assert_eq!(rust_fn(&source, "bump"), r#"fn bump(mut date: SmeltUnknown) -> JsString {
let _smelt_tmp_1: SmeltUnknown = { let smelt_date_result = { let smelt_timestamp_ms = ({ use chrono::{Datelike as _, Timelike as _}; let smelt_timestamp_ms = (match date.clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get("__smelt_date") { Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }, SmeltUnknown::String(smelt_value) => { let smelt_value = smelt_value.to_string_lossy(); chrono::DateTime::parse_from_rfc3339(&smelt_value).or_else(|_| chrono::DateTime::parse_from_str(&smelt_value, "%a %b %d %Y %H:%M:%S GMT%z")).map(|smelt_date| smelt_date.timestamp_millis() as f64).or_else(|_| chrono::NaiveDate::parse_from_str(&smelt_value, "%Y/%m/%d").or_else(|_| chrono::NaiveDate::parse_from_str(&smelt_value, "%Y-%m-%d")).map(|smelt_date| smelt_date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() as f64)).unwrap_or_else(|_| smelt_value.parse::<f64>().unwrap_or(f64::NAN)) }, SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }) as f64; if smelt_timestamp_ms.is_finite() && ((2024.0) as f64).is_finite() { chrono::DateTime::<chrono::Utc>::from_timestamp_millis(smelt_timestamp_ms as i64).and_then(|smelt_date| { let smelt_month_index = smelt_date.month0() as i32; let smelt_normalized_year = (2024.0 as i32) + smelt_month_index.div_euclid(12); let smelt_normalized_month0 = smelt_month_index.rem_euclid(12) as u32; chrono::NaiveDate::from_ymd_opt(smelt_normalized_year, smelt_normalized_month0 + 1, 1).and_then(|smelt_base| smelt_base.and_hms_nano_opt(smelt_date.hour(), smelt_date.minute(), smelt_date.second(), smelt_date.nanosecond())).map(|smelt_base| smelt_base + chrono::Duration::days((smelt_date.day() as i64) - 1)).map(|smelt_date| smelt_date.and_utc()) }).map(|smelt_date| smelt_date.timestamp_millis()).unwrap_or(i64::MIN) } else { i64::MIN } }) as f64; let smelt_timestamp_ms = if !smelt_timestamp_ms.is_finite() || smelt_timestamp_ms == i64::MIN as f64 { f64::NAN } else { smelt_timestamp_ms }; SmeltUnknown::Object(SmeltObject::new(Vec::from([("__smelt_date".to_owned(), SmeltUnknown::Number(smelt_timestamp_ms))]))) }; if let (SmeltUnknown::Object(smelt_result), SmeltUnknown::Object(receiver)) = (&smelt_date_result, date.clone()) { for (smelt_key, smelt_value) in receiver.iter() { if smelt_key != "__smelt_date" { smelt_result.insert(smelt_key, smelt_value); } } } smelt_date_result };
date = _smelt_tmp_1;
let _smelt_tmp_2: JsString = JsString::from((date.clone()).to_iso_string());
return _smelt_tmp_2;
}"#);
}

/// `new Date(x)` over an erased value parses at run time; values are in `date_semantics`.
#[test]
fn emits_date_to_iso_string_for_erased_datearg_surfaces() {
    let source = source_for(
        r"
export function isoFrom(value: unknown): string {
  return new Date(value as any).toISOString();
}
",
    );

    assert_eq!(rust_fn(&source, "iso_from"), r#"fn iso_from(value: SmeltUnknown) -> JsString {
let _smelt_tmp_1: f64 = { let smelt_timestamp_ms = (match value.clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get("__smelt_date") { Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }, SmeltUnknown::String(smelt_value) => { let smelt_value = smelt_value.to_string_lossy(); chrono::DateTime::parse_from_rfc3339(&smelt_value).or_else(|_| chrono::DateTime::parse_from_str(&smelt_value, "%a %b %d %Y %H:%M:%S GMT%z")).map(|smelt_date| smelt_date.timestamp_millis() as f64).or_else(|_| chrono::NaiveDate::parse_from_str(&smelt_value, "%Y/%m/%d").or_else(|_| chrono::NaiveDate::parse_from_str(&smelt_value, "%Y-%m-%d")).map(|smelt_date| smelt_date.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp_millis() as f64)).unwrap_or_else(|_| smelt_value.parse::<f64>().unwrap_or(f64::NAN)) }, SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }) as f64; if !smelt_timestamp_ms.is_finite() || smelt_timestamp_ms == i64::MIN as f64 { f64::NAN } else { smelt_timestamp_ms } };
let _smelt_tmp_2: SmeltDate = SmeltDate::new((_smelt_tmp_1) as f64);
let _smelt_tmp_3: JsString = JsString::from((_smelt_tmp_2).to_iso_string());
return _smelt_tmp_3;
}"#);
}

/// `toISOString` on an `any` callback item yields a typed string list.
#[test]
fn emits_to_iso_string_on_erased_callback_items() {
    let source = source_for(
        r"
declare const values: any[];
const isoValues = values.map((value) => value.toISOString());
",
    );

    assert_eq!(rust_fn(&source, "main"), r#"fn main() {
let _smelt_tmp_1: SmeltUnknown = smelt_global_object();
let _smelt_tmp_2: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into({ let smelt_src = smelt_get_unknown_field(&_smelt_tmp_1.clone(), "values").clone().into_smelt_unknown(); match smelt_src { SmeltUnknown::Null | SmeltUnknown::Undefined => SmeltList::new(Vec::new()), SmeltUnknown::Array(smelt_values) => SmeltList::with_storage(smelt_values.id, smelt_values.storage()), SmeltUnknown::String(smelt_value) => SmeltList::new(smelt_value.code_point_strings().into_iter().map(|smelt_ch| SmeltUnknown::String(smelt_ch)).collect::<Vec<_>>()), SmeltUnknown::Object(smelt_value) => if let Some(smelt_bytes) = smelt_host_buffer_elements(&SmeltUnknown::Object(smelt_value.clone())) { SmeltList::new(smelt_bytes) } else if let Some(smelt_args) = smelt_arguments_elements(&smelt_value) { SmeltList::new(smelt_args) } else if let Some(SmeltUnknown::Array(pairs)) = smelt_value.get("__smelt_map") { SmeltList::new(pairs.into_vec()) } else if let Some(SmeltUnknown::Array(members)) = smelt_value.get("__smelt_set") { SmeltList::new(members.into_vec()) } else { match smelt_value.get("__smelt_symbol_iterator") { Some(SmeltUnknown::Function(iterator)) => SmeltList::new(smelt_unknown_iterator_items(iterator(vec![]).unwrap_or(SmeltUnknown::Null))), _ => panic!("unknown is not iterable") } }, _ => panic!("unknown is not iterable") } });
let _smelt_tmp_3 = ::std::rc::Rc::new(|closure_arg_0: &SmeltUnknown, _arg0: i64, _arg1: &SmeltList<SmeltUnknown>| {
let _smelt_tmp_1: JsString = JsString::from((closure_arg_0.clone()).to_iso_string());
_smelt_tmp_1.clone()
});
let iso_values: SmeltList<JsString> = Into::<SmeltList<_>>::into({ let smelt_callback = ::std::rc::Rc::new(|closure_arg_0: &SmeltUnknown, _arg0: i64, _arg1: &SmeltList<SmeltUnknown>| {
let _smelt_tmp_1: JsString = JsString::from((closure_arg_0.clone()).to_iso_string());
_smelt_tmp_1.clone()
}); let smelt_array = _smelt_tmp_2; let smelt_value = smelt_array.borrow().iter().enumerate().map(|(smelt_index, smelt_item)| { (smelt_callback)(smelt_item, smelt_index as i64, &smelt_array) }).collect::<Vec<_>>(); smelt_value });
return;
}"#);
}

/// Date setters take a numeric argument read out of an erased value.
#[test]
fn emits_date_getters_and_setters_for_erased_datearg_surfaces() {
    let source = source_for(
        r"
export function yearRoundTrip(value: unknown): number {
  const date = new Date(0);
  date.setFullYear(value as number);
  return date.getFullYear();
}
",
    );

    assert_eq!(rust_fn(&source, "year_round_trip"), r#"fn year_round_trip(value: SmeltUnknown) -> f64 {
let mut date: SmeltDate = SmeltDate::new((0.0) as f64);
let _smelt_tmp_3: f64 = match value.clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get("__smelt_date") { Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN };
let _smelt_tmp_4: SmeltDate = { let smelt_time_ms = ({ use chrono::{Datelike as _, Timelike as _}; let smelt_timestamp_ms = ((date).get_time()) as f64; if smelt_timestamp_ms.is_finite() && ((_smelt_tmp_3) as f64).is_finite() { chrono::DateTime::<chrono::Utc>::from_timestamp_millis(smelt_timestamp_ms as i64).and_then(|smelt_date| { let smelt_month_index = smelt_date.month0() as i32; let smelt_normalized_year = (_smelt_tmp_3 as i32) + smelt_month_index.div_euclid(12); let smelt_normalized_month0 = smelt_month_index.rem_euclid(12) as u32; chrono::NaiveDate::from_ymd_opt(smelt_normalized_year, smelt_normalized_month0 + 1, 1).and_then(|smelt_base| smelt_base.and_hms_nano_opt(smelt_date.hour(), smelt_date.minute(), smelt_date.second(), smelt_date.nanosecond())).map(|smelt_base| smelt_base + chrono::Duration::days((smelt_date.day() as i64) - 1)).map(|smelt_date| smelt_date.and_utc()) }).map(|smelt_date| smelt_date.timestamp_millis()).unwrap_or(i64::MIN) } else { i64::MIN } }) as f64; (date).set_time(smelt_time_ms); date };
date = _smelt_tmp_4;
let _smelt_tmp_5: f64 = (date).get_time();
let _smelt_tmp_6: f64 = { use chrono::Datelike as _; let smelt_timestamp_ms = (_smelt_tmp_5) as f64; if smelt_timestamp_ms.is_finite() { chrono::DateTime::<chrono::Utc>::from_timestamp_millis(smelt_timestamp_ms as i64).map_or(f64::NAN, |smelt_date| smelt_date.year() as f64) } else { f64::NAN } };
return _smelt_tmp_6;
}"#);
}

/// A date setter stays inside its branch, after the locals it reads.
#[test]
fn keeps_date_setter_side_effects_inside_branch_blocks() {
    let source = source_for(
        r"
function apply(isTwoDigitYear: boolean, year: number, date: Date): Date {
  if (isTwoDigitYear) {
    const normalizedTwoDigitYear = year + 2000;
    date.setFullYear(normalizedTwoDigitYear, 0, 1);
    return date;
  }
  return date;
}
",
    );

    assert_eq!(rust_fn(&source, "apply"), r#"fn apply(is_two_digit_year: bool, year: f64, mut date: SmeltDate) -> SmeltDate {
let normalized_two_digit_year: f64;
let _smelt_tmp_5: SmeltDate;
if is_two_digit_year {
normalized_two_digit_year = year + 2000.0;
_smelt_tmp_5 = { let smelt_time_ms = ({ use chrono::{Datelike as _, Timelike as _}; let smelt_timestamp_ms = ((date.clone()).get_time()) as f64; if smelt_timestamp_ms.is_finite() && ((normalized_two_digit_year) as f64).is_finite() && ((0.0) as f64).is_finite() && ((1.0) as f64).is_finite() { chrono::DateTime::<chrono::Utc>::from_timestamp_millis(smelt_timestamp_ms as i64).and_then(|smelt_date| { let smelt_month_index = 0.0 as i32; let smelt_normalized_year = (normalized_two_digit_year as i32) + smelt_month_index.div_euclid(12); let smelt_normalized_month0 = smelt_month_index.rem_euclid(12) as u32; chrono::NaiveDate::from_ymd_opt(smelt_normalized_year, smelt_normalized_month0 + 1, 1).and_then(|smelt_base| smelt_base.and_hms_nano_opt(smelt_date.hour(), smelt_date.minute(), smelt_date.second(), smelt_date.nanosecond())).map(|smelt_base| smelt_base + chrono::Duration::days((1.0 as i64) - 1)).map(|smelt_date| smelt_date.and_utc()) }).map(|smelt_date| smelt_date.timestamp_millis()).unwrap_or(i64::MIN) } else { i64::MIN } }) as f64; (date.clone()).set_time(smelt_time_ms); date.clone() };
date = _smelt_tmp_5;
return date.clone();
} else {
return date.clone();
}
}"#);
}

/// A `Date` crossing into an erased callable goes through the date boundary adapter.
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

    assert_eq!(rust_fn(&source, "main"), r#"fn main() {
let result: JsString;
let _smelt_tmp_2 = ::std::rc::Rc::new(|closure_arg_0: f64| {
let _smelt_tmp_1: JsString = JsString::from(smelt_number_to_string(closure_arg_0));
_smelt_tmp_1.clone()
});
let _smelt_tmp_3 = ::std::rc::Rc::new(|closure_arg_0: SmeltDate, closure_arg_1: &SmeltList<JsString>| {
let mut _smelt_tmp_5: SmeltList<JsString>;
let _smelt_tmp_6: SmeltList<JsString>;
let _smelt_tmp_2: f64 = (closure_arg_0.clone()).get_time();
let _smelt_tmp_3: f64 = { use chrono::Datelike as _; let smelt_timestamp_ms = (_smelt_tmp_2) as f64; if smelt_timestamp_ms.is_finite() { chrono::DateTime::<chrono::Utc>::from_timestamp_millis(smelt_timestamp_ms as i64).map_or(f64::NAN, |smelt_date| smelt_date.day() as f64) } else { f64::NAN } };
let _smelt_tmp_4: bool = _smelt_tmp_3 == 1.0;
if _smelt_tmp_4 {
_smelt_tmp_5 = Into::<SmeltList<_>>::into(closure_arg_1.clone());
} else {
_smelt_tmp_6 = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<JsString> = vec![]; smelt_list_items }));
_smelt_tmp_5 = Into::<SmeltList<_>>::into(_smelt_tmp_6.clone());
}
_smelt_tmp_5.clone()
});
let _smelt_tmp_4: SmeltRecord<JsString, SmeltUnknown> = SmeltRecord::from([(JsString::from("month"), SmeltUnknown::Function({ let smelt_source_fn = _smelt_tmp_2.clone(); let smelt_callback = smelt_source_fn.clone(); let smelt_erased_fn: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = if let Some(SmeltUnknown::Function(smelt_narrowed_fn)) = smelt_lookup_callable_object(&smelt_source_fn) { smelt_narrowed_fn } else { ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::String(((smelt_callback)(match smelt_args.get(0).cloned().unwrap_or(SmeltUnknown::Null).clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get("__smelt_date") { Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN })).into()))) }; smelt_link_function_identity(&smelt_erased_fn, &smelt_source_fn); smelt_register_function_length(&smelt_erased_fn, 1.0); smelt_erased_fn })), (JsString::from("preprocessor"), SmeltUnknown::Function({ let smelt_source_fn = _smelt_tmp_3.clone(); let smelt_callback = smelt_source_fn.clone(); let smelt_erased_fn: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = if let Some(SmeltUnknown::Function(smelt_narrowed_fn)) = smelt_lookup_callable_object(&smelt_source_fn) { smelt_narrowed_fn } else { ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>({ let smelt_l = (smelt_callback)(<SmeltDate as SmeltFromUnknown>::smelt_from_unknown(smelt_args.get(0).cloned().unwrap_or(SmeltUnknown::Null).clone()), &({ let smelt_src = smelt_args.get(1).cloned().unwrap_or(SmeltUnknown::Null).clone().into_smelt_unknown(); match smelt_src { SmeltUnknown::Null | SmeltUnknown::Undefined => SmeltList::new(Vec::new()), SmeltUnknown::Array(smelt_values) => { let smelt_list_id = smelt_values.id; let smelt_list = SmeltList::with_id(smelt_list_id, smelt_values.into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()); smelt_array_props_adopt(smelt_list_id, &smelt_list.storage()); smelt_list }, SmeltUnknown::String(smelt_value) => SmeltList::new(smelt_value.code_point_strings().into_iter().map(|smelt_ch| smelt_ch).collect::<Vec<_>>()), SmeltUnknown::Object(smelt_value) => if let Some(smelt_bytes) = smelt_host_buffer_elements(&SmeltUnknown::Object(smelt_value.clone())) { SmeltList::new(smelt_bytes.into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()) } else if let Some(smelt_args) = smelt_arguments_elements(&smelt_value) { SmeltList::new(smelt_args.into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()) } else if let Some(SmeltUnknown::Array(pairs)) = smelt_value.get("__smelt_map") { SmeltList::new(pairs.into_vec().into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()) } else if let Some(SmeltUnknown::Array(members)) = smelt_value.get("__smelt_set") { SmeltList::new(members.into_vec().into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()) } else { match smelt_value.get("__smelt_symbol_iterator") { Some(SmeltUnknown::Function(iterator)) => SmeltList::new(smelt_unknown_iterator_items(iterator(vec![]).unwrap_or(SmeltUnknown::Null)).into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()), _ => panic!("unknown is not iterable") } }, _ => panic!("unknown is not iterable") } })); let smelt_id = smelt_l.id(); let smelt_values: Vec<_> = smelt_l.into(); SmeltUnknown::Array(SmeltArray::with_id(smelt_id, smelt_values.into_iter().map(|smelt_value| SmeltUnknown::String(smelt_value.into())).collect::<Vec<_>>())) })) }; smelt_link_function_identity(&smelt_erased_fn, &smelt_source_fn); smelt_register_function_length(&smelt_erased_fn, 2.0); smelt_erased_fn }))]);
let custom_locale: SmeltRecord<JsString, SmeltRecord<JsString, SmeltUnknown>> = SmeltRecord::from([(JsString::from("localize"), _smelt_tmp_4)]);
let _smelt_tmp_6: SmeltRecord<JsString, SmeltRecord<JsString, SmeltRecord<JsString, SmeltUnknown>>> = SmeltRecord::from([(JsString::from("locale"), custom_locale)]);
let _smelt_tmp_7: JsString = format(Some({ let smelt_record_map = _smelt_tmp_6.clone(); FormatOptions { locale: smelt_record_map.get_text("locale").map(|smelt_value| { let smelt_record_map = smelt_value.clone(); Locale { localize: smelt_record_map.get_text("localize").map_or(Default::default(), |smelt_value| { let smelt_record_map = smelt_value.clone(); Localize { month: smelt_erased_record_member(&smelt_record_map, "month").cloned().map_or_else(|| smelt_throw_missing_record_field("value", "Localize", "month"), |smelt_value| { let smelt_source_value = smelt_value.clone(); let smelt_function = match smelt_source_value.clone() { SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get("__smelt_call") { Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }, _ => None }; if let Some(smelt_function) = smelt_function { let smelt_origin_identity = smelt_canonical_function_identity(&smelt_function); let smelt_callback: ::std::rc::Rc<dyn Fn(f64) -> JsString> = if let Some(smelt_original) = smelt_restore_function_origin::<::std::rc::Rc<dyn Fn(f64) -> JsString>>(&smelt_function) { if matches!(smelt_source_value, SmeltUnknown::Object(_)) { ::std::rc::Rc::new(move |smelt_arg0: f64| (smelt_original)(smelt_arg0)) } else { smelt_original } } else { ::std::rc::Rc::new(move |smelt_arg0: f64| -> JsString { let smelt_result = (smelt_function)({ let mut smelt_call_args = Vec::new(); smelt_call_args.push(SmeltUnknown::Number(smelt_arg0 as f64)); smelt_call_args }).unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error)); match smelt_result.clone() { SmeltUnknown::String(smelt_value) => smelt_value, smelt_other => JsString::from(match smelt_other { SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) => "function () { [native code] }".to_owned(), SmeltUnknown::Promise(_) => "[object Promise]".to_owned() }) } }) }; smelt_register_callable_object(&smelt_callback, smelt_source_value); smelt_link_function_identity_key(&smelt_callback, smelt_origin_identity); smelt_callback } else { { let smelt_default_callback: ::std::rc::Rc<dyn Fn(f64) -> JsString> = ::std::rc::Rc::new(move |smelt_arg0: f64| -> JsString { smelt_value_not_callable() }); smelt_default_callback } } }), preprocessor: smelt_erased_record_member(&smelt_record_map, "preprocessor").cloned().and_then(|smelt_value| if smelt_unknown_is_nullish(&smelt_value) { None } else { Some({ let smelt_source_value = smelt_value.clone(); let smelt_function = match smelt_source_value.clone() { SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get("__smelt_call") { Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }, _ => None }; if let Some(smelt_function) = smelt_function { let smelt_origin_identity = smelt_canonical_function_identity(&smelt_function); let smelt_callback: ::std::rc::Rc<dyn Fn(SmeltDate, &SmeltList<JsString>) -> SmeltList<JsString>> = if let Some(smelt_original) = smelt_restore_function_origin::<::std::rc::Rc<dyn Fn(SmeltDate, &SmeltList<JsString>) -> SmeltList<JsString>>>(&smelt_function) { if matches!(smelt_source_value, SmeltUnknown::Object(_)) { ::std::rc::Rc::new(move |smelt_arg0: SmeltDate, smelt_arg1: &SmeltList<JsString>| (smelt_original)(smelt_arg0, smelt_arg1)) } else { smelt_original } } else { ::std::rc::Rc::new(move |smelt_arg0: SmeltDate, smelt_arg1: &SmeltList<JsString>| -> SmeltList<JsString> { let smelt_result = (smelt_function)({ let mut smelt_call_args = Vec::new(); smelt_call_args.push(smelt_arg0.clone().into_smelt_unknown()); smelt_call_args.push({ let smelt_l = smelt_arg1.clone(); let smelt_id = smelt_l.id(); let smelt_values: Vec<_> = smelt_l.into(); SmeltUnknown::Array(SmeltArray::with_id(smelt_id, smelt_values.into_iter().map(|smelt_value| SmeltUnknown::String(smelt_value.into())).collect::<Vec<_>>())) }); smelt_call_args }).unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error)); { let smelt_src = smelt_result.clone().into_smelt_unknown(); match smelt_src { SmeltUnknown::Null | SmeltUnknown::Undefined => SmeltList::new(Vec::new()), SmeltUnknown::Array(smelt_values) => { let smelt_list_id = smelt_values.id; let smelt_list = SmeltList::with_id(smelt_list_id, smelt_values.into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()); smelt_array_props_adopt(smelt_list_id, &smelt_list.storage()); smelt_list }, SmeltUnknown::String(smelt_value) => SmeltList::new(smelt_value.code_point_strings().into_iter().map(|smelt_ch| smelt_ch).collect::<Vec<_>>()), SmeltUnknown::Object(smelt_value) => if let Some(smelt_bytes) = smelt_host_buffer_elements(&SmeltUnknown::Object(smelt_value.clone())) { SmeltList::new(smelt_bytes.into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()) } else if let Some(smelt_args) = smelt_arguments_elements(&smelt_value) { SmeltList::new(smelt_args.into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()) } else if let Some(SmeltUnknown::Array(pairs)) = smelt_value.get("__smelt_map") { SmeltList::new(pairs.into_vec().into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()) } else if let Some(SmeltUnknown::Array(members)) = smelt_value.get("__smelt_set") { SmeltList::new(members.into_vec().into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()) } else { match smelt_value.get("__smelt_symbol_iterator") { Some(SmeltUnknown::Function(iterator)) => SmeltList::new(smelt_unknown_iterator_items(iterator(vec![]).unwrap_or(SmeltUnknown::Null)).into_iter().map(|smelt_value| <JsString as SmeltFromUnknown>::smelt_from_unknown(smelt_value)).collect::<Vec<_>>()), _ => panic!("unknown is not iterable") } }, _ => panic!("unknown is not iterable") } } }) }; smelt_register_callable_object(&smelt_callback, smelt_source_value); smelt_link_function_identity_key(&smelt_callback, smelt_origin_identity); smelt_callback } else { { let smelt_default_callback: ::std::rc::Rc<dyn Fn(SmeltDate, &SmeltList<JsString>) -> SmeltList<JsString>> = ::std::rc::Rc::new(move |smelt_arg0: SmeltDate, smelt_arg1: &SmeltList<JsString>| -> SmeltList<JsString> { smelt_value_not_callable() }); smelt_default_callback } } }) }) } }) } }) } }));
result = _smelt_tmp_7;
return;
}"#);
}
