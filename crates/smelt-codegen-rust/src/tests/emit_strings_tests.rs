//! Generated Rust for string operations.

use super::*;

/// A string predicate yields a concrete `bool`; the callback never erases.
#[test]
fn types_a_string_predicate_callback_result_as_a_bool() {
    let source = source_for(
        r#"
export function firstA(names: string[]): string | undefined {
  return names.find((name: string) => name.startsWith("a"));
}
"#,
    );

    assert_eq!(rust_fn(&source, "first_a"), r#"fn first_a(names: SmeltList<JsString>) -> Option<JsString> {
let _smelt_tmp_1 = ::std::rc::Rc::new(|closure_arg_0: JsString, _arg0: i64, _arg1: &SmeltList<JsString>| {
let _smelt_tmp_2: JsString = JsString::from("a");
let _smelt_tmp_3: bool = closure_arg_0.clone().starts_with(&_smelt_tmp_2.clone());
_smelt_tmp_3
});
let _smelt_tmp_2: Option<JsString> = { let smelt_callback = ::std::rc::Rc::new(|closure_arg_0: JsString, _arg0: i64, _arg1: &SmeltList<JsString>| {
let _smelt_tmp_2: JsString = JsString::from("a");
let _smelt_tmp_3: bool = closure_arg_0.clone().starts_with(&_smelt_tmp_2.clone());
_smelt_tmp_3
}); let smelt_array = names.clone(); let smelt_value = smelt_array.borrow().iter().enumerate().find_map(|(smelt_index, smelt_item)| if (smelt_callback)(smelt_item.clone(), smelt_index as i64, &smelt_array) { Some(smelt_item.clone()) } else { None }); smelt_value };
return _smelt_tmp_2;
}"#);
}

/// A `switch` over an optional string matches `Some("literal")` patterns.
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

    assert_eq!(rust_fn(&source, "label"), r#"fn label(value: Option<JsString>) -> JsString {
match value.clone().map(|smelt_text| smelt_text.to_string_lossy()).as_deref() {
Some("a") => {
return JsString::from("A");
}
_ => {
return JsString::from("other");
}
}
}"#);
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
        source.contains("smelt_number_to_string(number)) + &match suffix"),
        "{source}"
    );
    assert!(
        !source.contains("let _smelt_tmp_16: f64")
            && !source.contains("return _smelt_tmp_16.clone();"),
        "{source}"
    );
}

/// String concatenation in a callback borrows its right-hand side.
#[test]
fn emits_callback_string_concat_with_borrowed_rhs() {
    let source = source_for(
        r#"
function label(values: string[]): string[] {
  return values.map((value, index) => "" + (index === 0 ? value : value.toLowerCase()));
}
"#,
    );

    assert_eq!(rust_fn(&source, "label"), r#"fn label(values: SmeltList<JsString>) -> SmeltList<JsString> {
let _smelt_tmp_1 = ::std::rc::Rc::new(|closure_arg_0: JsString, closure_arg_1: i64, _arg0: &SmeltList<JsString>| {
let mut _smelt_tmp_3: JsString;
let _smelt_tmp_4: JsString;
let _smelt_tmp_5: JsString;
let _smelt_tmp_2: bool = (closure_arg_1 as f64) == 0.0;
if _smelt_tmp_2 {
_smelt_tmp_3 = closure_arg_0.clone();
} else {
_smelt_tmp_4 = closure_arg_0.clone().to_lower_case();
_smelt_tmp_3 = _smelt_tmp_4.clone();
}
_smelt_tmp_5 = JsString::new() + &_smelt_tmp_3.clone();
_smelt_tmp_5.clone()
});
let _smelt_tmp_2: SmeltList<JsString> = Into::<SmeltList<_>>::into({ let smelt_callback = ::std::rc::Rc::new(|closure_arg_0: JsString, closure_arg_1: i64, _arg0: &SmeltList<JsString>| {
let mut _smelt_tmp_3: JsString;
let _smelt_tmp_4: JsString;
let _smelt_tmp_5: JsString;
let _smelt_tmp_2: bool = (closure_arg_1 as f64) == 0.0;
if _smelt_tmp_2 {
_smelt_tmp_3 = closure_arg_0.clone();
} else {
_smelt_tmp_4 = closure_arg_0.clone().to_lower_case();
_smelt_tmp_3 = _smelt_tmp_4.clone();
}
_smelt_tmp_5 = JsString::new() + &_smelt_tmp_3.clone();
_smelt_tmp_5.clone()
}); let smelt_array = values.clone(); let smelt_value = smelt_array.borrow().iter().enumerate().map(|(smelt_index, smelt_item)| { (smelt_callback)(smelt_item.clone(), smelt_index as i64, &smelt_array) }).collect::<Vec<_>>(); smelt_value });
return _smelt_tmp_2;
}"#);
}

/// A character read off a bounded `T extends string` is erased text with the JavaScript case conversion.
#[test]
fn emits_case_conversion_for_erased_callback_string_indexes() {
    let source = source_for(
        r"
function initialCaps<T extends string>(values: T[]): string[] {
  return values.map((value) => value[0].toUpperCase());
}
",
    );

    // The element type is left open here: `T extends string` has a knowable
    // shape, so only the conversion is the requirement.
    let item = rust_fn(&source, "initial_caps");
    assert!(item.contains("smelt_value.char_at(smelt_position)"), "{item}");
    assert!(item.contains(".to_upper_case()"), "{item}");
}

/// The truthy arm yields the erased value unchanged; only the `Date` fallback crosses its boundary adapter.
#[test]
fn keeps_unknown_conditionals_erased_before_string_compatible_fallbacks() {
    let source = source_for(
        r"
const value: unknown = 1;
const fallback: Date = new Date();
const selected: unknown = value ? value : fallback;
",
    );

    assert_eq!(rust_fn(&source, "main"), r#"fn main() {
let selected: SmeltUnknown;
let mut _smelt_tmp_6: SmeltUnknown;
let value: SmeltUnknown = SmeltUnknown::Number(1.0 as f64);
let _smelt_tmp_3: i64 = SMELT_DATE_NOW.with(::std::cell::Cell::get).unwrap_or_else(|| chrono::Utc::now().timestamp_millis().saturating_add(SMELT_VIRTUAL_MS.with(::std::cell::Cell::get) as i64));
let fallback: SmeltDate = SmeltDate::new((_smelt_tmp_3) as f64);
let _smelt_tmp_5: bool = match value.clone() { SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(smelt_value) => smelt_value, SmeltUnknown::Number(smelt_value) => smelt_value != 0.0 && !smelt_value.is_nan(), SmeltUnknown::String(smelt_value) => !smelt_value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true };
let _smelt_tmp_6: SmeltUnknown = if _smelt_tmp_5 { value } else { fallback.clone().into_smelt_unknown() };
selected = _smelt_tmp_6;
return;
}"#);
}

/// A `number` shift is int32 (`ToInt32(a)` by `ToUint32(b) & 31`); a `bigint` shift is the unbounded, fallible runtime shift.
#[test]
fn emits_radix_to_string_and_numeric_shift_surface() {
    let source = source_for(
        r"
const binary = (10n).toString(2);
const left = 1n << 8n;
const right = left >> 1n;
const pivot = (4 + 10) >>> 1;
const numberLeft = 1 << 31;
const numberRight = -8 >> 1;
function shiftRaw(raw: bigint): bigint {
  return raw >> 1n;
}
",
    );

    assert_eq!([rust_fn(&source, "shift_raw"), rust_fn(&source, "main")].join("\n\n"), r#"fn shift_raw(raw: ::num_bigint::BigInt) -> Result<::num_bigint::BigInt, Box<dyn std::error::Error>> {
let _smelt_tmp_1: ::num_bigint::BigInt = smelt_bigint_shr(&raw, &::num_bigint::BigInt::from(1_i64))?;
return Ok(_smelt_tmp_1);
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
let binary: JsString;
let left: ::num_bigint::BigInt;
let right: ::num_bigint::BigInt;
let pivot: f64;
let number_left: f64;
let number_right: f64;
let _smelt_tmp_9: f64;
let _smelt_tmp_12: f64;
let _smelt_tmp_6: JsString = smelt_bigint_to_string_radix(&::num_bigint::BigInt::from(10_i64), 2.0).map(JsString::from).map(JsString::from)?;
binary = _smelt_tmp_6;
let _smelt_tmp_7: ::num_bigint::BigInt = smelt_bigint_shl(&::num_bigint::BigInt::from(1_i64), &::num_bigint::BigInt::from(8_i64))?;
left = _smelt_tmp_7;
let _smelt_tmp_8: ::num_bigint::BigInt = smelt_bigint_shr(&left, &::num_bigint::BigInt::from(1_i64))?;
right = _smelt_tmp_8;
_smelt_tmp_9 = 4.0 + 10.0;
pivot = { let smelt_shift_value = (_smelt_tmp_9 as f64).trunc(); let smelt_shift_value = if smelt_shift_value.is_finite() { smelt_shift_value.rem_euclid(4294967296.0) as u32 } else { 0_u32 }; let smelt_shift_count = (1.0 as f64).trunc(); let smelt_shift_count = if smelt_shift_count.is_finite() { smelt_shift_count.rem_euclid(4294967296.0) as u32 } else { 0_u32 }; (smelt_shift_value >> (smelt_shift_count & 31)) as f64 };
number_left = ((({ let smelt_int32_v = (1.0 as f64).trunc(); if smelt_int32_v.is_finite() { smelt_int32_v.rem_euclid(4294967296.0) as u32 as i32 } else { 0_i32 } }).wrapping_shl({ let smelt_uint32_v = (31.0 as f64).trunc(); if smelt_uint32_v.is_finite() { smelt_uint32_v.rem_euclid(4294967296.0) as u32 } else { 0_u32 } } & 31)) as f64);
_smelt_tmp_12 = -8.0;
number_right = ((({ let smelt_int32_v = (_smelt_tmp_12 as f64).trunc(); if smelt_int32_v.is_finite() { smelt_int32_v.rem_euclid(4294967296.0) as u32 as i32 } else { 0_i32 } }).wrapping_shr({ let smelt_uint32_v = (1.0 as f64).trunc(); if smelt_uint32_v.is_finite() { smelt_uint32_v.rem_euclid(4294967296.0) as u32 } else { 0_u32 } } & 31)) as f64);
return Ok(());
}"#);
}

/// Only a single regex or string argument on an erased receiver is `String.prototype.match`; two callbacks are a dynamic member call.
#[test]
fn erased_receiver_two_callback_match_stays_dynamic_not_string_match() {
    let source = source_for(
        r"
function fold(value: any): number {
  return value.match((ok: number) => ok, (_err: string) => 0);
}
",
    );

    assert_eq!(rust_fn(&source, "fold"), r#"fn fold(value: SmeltUnknown) -> f64 {
let _smelt_tmp_1: SmeltUnknown = smelt_get_unknown_field(&value.clone(), "match").clone();
let _smelt_tmp_2 = { let smelt_source_value = _smelt_tmp_1.clone(); let smelt_function = match smelt_source_value.clone() { SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get("__smelt_call") { Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }, _ => None }; if let Some(smelt_function) = smelt_function { let smelt_origin_identity = smelt_canonical_function_identity(&smelt_function); let smelt_callback: ::std::rc::Rc<dyn Fn(::std::rc::Rc<dyn Fn(f64) -> f64>, ::std::rc::Rc<dyn Fn(JsString) -> f64>) -> SmeltUnknown> = if let Some(smelt_original) = smelt_restore_function_origin::<::std::rc::Rc<dyn Fn(::std::rc::Rc<dyn Fn(f64) -> f64>, ::std::rc::Rc<dyn Fn(JsString) -> f64>) -> SmeltUnknown>>(&smelt_function) { if matches!(smelt_source_value, SmeltUnknown::Object(_)) { ::std::rc::Rc::new(move |smelt_arg0: ::std::rc::Rc<dyn Fn(f64) -> f64>, smelt_arg1: ::std::rc::Rc<dyn Fn(JsString) -> f64>| (smelt_original)(smelt_arg0, smelt_arg1)) } else { smelt_original } } else { ::std::rc::Rc::new(move |smelt_arg0: ::std::rc::Rc<dyn Fn(f64) -> f64>, smelt_arg1: ::std::rc::Rc<dyn Fn(JsString) -> f64>| -> SmeltUnknown { let smelt_result = (smelt_function)({ let mut smelt_call_args = Vec::new(); smelt_call_args.push({ let smelt_function_value = smelt_arg0; if let Some(smelt_callable_object) = smelt_lookup_callable_object(&smelt_function_value) { smelt_callable_object } else { let smelt_origin_identity = smelt_canonical_function_identity(&smelt_function_value); let smelt_function_origin = smelt_function_value.clone(); let smelt_erased_function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Number((smelt_function_value)(match smelt_args.get(0).cloned().unwrap_or(SmeltUnknown::Null).clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get("__smelt_date") { Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }) as f64))); smelt_register_function_origin(&smelt_erased_function, smelt_function_origin); smelt_link_function_identity_key(&smelt_erased_function, smelt_origin_identity); smelt_register_function_length_once(&smelt_erased_function, 1.0); SmeltUnknown::Function(smelt_erased_function) } }); smelt_call_args.push({ let smelt_function_value = smelt_arg1; if let Some(smelt_callable_object) = smelt_lookup_callable_object(&smelt_function_value) { smelt_callable_object } else { let smelt_origin_identity = smelt_canonical_function_identity(&smelt_function_value); let smelt_function_origin = smelt_function_value.clone(); let smelt_erased_function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>(SmeltUnknown::Number((smelt_function_value)(match smelt_args.get(0).cloned().unwrap_or(SmeltUnknown::Null).clone() { SmeltUnknown::String(smelt_value) => smelt_value, smelt_other => JsString::from(match smelt_other { SmeltUnknown::String(smelt_value) => smelt_value.to_string(), SmeltUnknown::Symbol(smelt_value) => smelt_symbol_to_string(&smelt_value), SmeltUnknown::Number(smelt_value) => smelt_number_to_string(smelt_value), SmeltUnknown::Bool(smelt_value) => smelt_value.to_string(), SmeltUnknown::Null | SmeltUnknown::Undefined => String::new(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => "[object Object]".to_owned(), SmeltUnknown::Function(_) => "function () { [native code] }".to_owned(), SmeltUnknown::Promise(_) => "[object Promise]".to_owned() }) }) as f64))); smelt_register_function_origin(&smelt_erased_function, smelt_function_origin); smelt_link_function_identity_key(&smelt_erased_function, smelt_origin_identity); smelt_register_function_length_once(&smelt_erased_function, 1.0); SmeltUnknown::Function(smelt_erased_function) } }); smelt_call_args }).unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error)); smelt_result }) }; smelt_register_callable_object(&smelt_callback, smelt_source_value); smelt_link_function_identity_key(&smelt_callback, smelt_origin_identity); smelt_callback } else { { let smelt_default_callback: ::std::rc::Rc<dyn Fn(::std::rc::Rc<dyn Fn(f64) -> f64>, ::std::rc::Rc<dyn Fn(JsString) -> f64>) -> SmeltUnknown> = ::std::rc::Rc::new(move |smelt_arg0: ::std::rc::Rc<dyn Fn(f64) -> f64>, smelt_arg1: ::std::rc::Rc<dyn Fn(JsString) -> f64>| -> SmeltUnknown { smelt_value_not_callable() }); smelt_default_callback } } };
let _smelt_tmp_3 = ::std::rc::Rc::new(|closure_arg_0: f64| {
closure_arg_0
});
let _smelt_tmp_4 = ::std::rc::Rc::new(|closure_arg_0: JsString| {
0.0
});
let _smelt_tmp_5: SmeltUnknown = (_smelt_tmp_2)(_smelt_tmp_3.clone(), _smelt_tmp_4.clone());
return match _smelt_tmp_5.clone() { SmeltUnknown::Number(smelt_value) => smelt_value, SmeltUnknown::Object(smelt_value) => match smelt_value.get("__smelt_date") { Some(SmeltUnknown::Number(smelt_value)) => smelt_value, _ => f64::NAN }, SmeltUnknown::String(smelt_value) => smelt_js_string_to_number(&smelt_value), SmeltUnknown::Bool(smelt_value) => if smelt_value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN };
}"#);
}

/// `split` keeps the JavaScript empty-separator and negative-limit behaviour; values are in `string_method_semantics`.
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

    assert_eq!([rust_fn(&source, "split_chars"), rust_fn(&source, "split_negative")].join("\n\n"), r#"fn split_chars(value: JsString) -> SmeltList<JsString> {
let _smelt_tmp_1: SmeltList<JsString> = Into::<SmeltList<_>>::into({ let smelt_haystack = value.clone(); let smelt_separator = JsString::new(); smelt_haystack.split(&smelt_separator, usize::MAX) });
return _smelt_tmp_1;
}

fn split_negative(value: JsString) -> SmeltList<JsString> {
let _smelt_tmp_1: f64 = -1.0;
let _smelt_tmp_2: SmeltList<JsString> = Into::<SmeltList<_>>::into({ let mut smelt_parts = { let smelt_haystack = value.clone(); let smelt_separator = JsString::from(","); smelt_haystack.split(&smelt_separator, usize::MAX) }; let smelt_limit = _smelt_tmp_1 as f64; if !smelt_limit.is_finite() || smelt_limit == 0.0 { smelt_parts.truncate(0); } else if smelt_limit.is_sign_positive() { smelt_parts.truncate(smelt_limit.floor() as usize); } smelt_parts });
return _smelt_tmp_2;
}"#);
}

/// A `typeof` result returned as `string` keeps the owned form.
#[test]
fn keeps_an_owned_string_when_a_typeof_local_escapes() {
    let source = source_for(
        r"
export function kindOf(value: unknown): string {
  return typeof value;
}
",
    );

    assert_eq!(rust_fn(&source, "kind_of"), r#"fn kind_of(value: SmeltUnknown) -> JsString {
let _smelt_tmp_1: JsString = JsString::from(smelt_typeof(&value));
return _smelt_tmp_1;
}"#);
}

/// `.length` on an erased callback value dispatches on its runtime tag.
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

    assert_eq!(rust_fn(&source, "run"), r#"fn run() -> SmeltList<SmeltUnknown> {
let _smelt_tmp_0: SmeltList<JsString> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<JsString> = vec![JsString::from("aa"), JsString::from("b")]; smelt_list_items }));
let _smelt_tmp_1 = ::std::rc::Rc::new(|closure_arg_0: &SmeltUnknown| {
(match closure_arg_0.clone() { SmeltUnknown::String(smelt_value) => SmeltUnknown::Number(smelt_value.len() as f64), SmeltUnknown::Array(smelt_value) => SmeltUnknown::Number(smelt_value.len() as f64), smelt_value @ SmeltUnknown::Function(_) => SmeltUnknown::Number(smelt_function_length(&smelt_value)), SmeltUnknown::Object(smelt_map) => match smelt_get_object_field(&smelt_map, "length") { SmeltUnknown::Undefined | SmeltUnknown::Null if smelt_map.contains_key("__smelt_call") => SmeltUnknown::Number(smelt_function_length(&SmeltUnknown::Object(smelt_map))), smelt_value => smelt_value }, _ => SmeltUnknown::Null }).clone()
});
let _smelt_tmp_2: SmeltList<SmeltUnknown> = project(_smelt_tmp_0, &*_smelt_tmp_1);
return _smelt_tmp_2;
}"#);
}

/// `startsWith` in a callback body is the typed prefix test.
#[test]
fn lowers_string_starts_with_inside_callback_body() {
    let source = source_for(
        r#"
const words = ["apple", "banana"];
const flags = words.map(word => word.startsWith("a"));
"#,
    );

    assert_eq!(rust_fn(&source, "main"), r#"fn main() {
let words: SmeltList<JsString> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<JsString> = vec![JsString::from("apple"), JsString::from("banana")]; smelt_list_items }));
let _smelt_tmp_3 = ::std::rc::Rc::new(|closure_arg_0: JsString, _arg0: i64, _arg1: &SmeltList<JsString>| {
let _smelt_tmp_2: JsString = JsString::from("a");
let _smelt_tmp_3: bool = closure_arg_0.clone().starts_with(&_smelt_tmp_2.clone());
_smelt_tmp_3
});
let flags: SmeltList<bool> = Into::<SmeltList<_>>::into({ let smelt_callback = ::std::rc::Rc::new(|closure_arg_0: JsString, _arg0: i64, _arg1: &SmeltList<JsString>| {
let _smelt_tmp_2: JsString = JsString::from("a");
let _smelt_tmp_3: bool = closure_arg_0.clone().starts_with(&_smelt_tmp_2.clone());
_smelt_tmp_3
}); let smelt_array = words; let smelt_value = smelt_array.borrow().iter().enumerate().map(|(smelt_index, smelt_item)| { (smelt_callback)(smelt_item.clone(), smelt_index as i64, &smelt_array) }).collect::<Vec<_>>(); smelt_value });
return;
}"#);
}

/// `endsWith` on an `any` callback receiver coerces to a string first.
#[test]
fn lowers_string_ends_with_on_erased_callback_receiver() {
    let source = source_for(
        r#"
declare const values: any[];
const flags = values.map(value => value.endsWith("x"));
"#,
    );

    assert_eq!(rust_fn(&source, "main"), r#"fn main() {
let _smelt_tmp_1: SmeltUnknown = smelt_global_object();
let _smelt_tmp_2: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into({ let smelt_src = smelt_get_unknown_field(&_smelt_tmp_1.clone(), "values").clone().into_smelt_unknown(); match smelt_src { SmeltUnknown::Null | SmeltUnknown::Undefined => SmeltList::new(Vec::new()), SmeltUnknown::Array(smelt_values) => SmeltList::with_storage(smelt_values.id, smelt_values.storage()), SmeltUnknown::String(smelt_value) => SmeltList::new(smelt_value.code_point_strings().into_iter().map(|smelt_ch| SmeltUnknown::String(smelt_ch)).collect::<Vec<_>>()), SmeltUnknown::Object(smelt_value) => if let Some(smelt_bytes) = smelt_host_buffer_elements(&SmeltUnknown::Object(smelt_value.clone())) { SmeltList::new(smelt_bytes) } else if let Some(smelt_args) = smelt_arguments_elements(&smelt_value) { SmeltList::new(smelt_args) } else if let Some(SmeltUnknown::Array(pairs)) = smelt_value.get("__smelt_map") { SmeltList::new(pairs.into_vec()) } else if let Some(SmeltUnknown::Array(members)) = smelt_value.get("__smelt_set") { SmeltList::new(members.into_vec()) } else { match smelt_value.get("__smelt_symbol_iterator") { Some(SmeltUnknown::Function(iterator)) => SmeltList::new(smelt_unknown_iterator_items(iterator(vec![]).unwrap_or(SmeltUnknown::Null))), _ => panic!("unknown is not iterable") } }, _ => panic!("unknown is not iterable") } });
let _smelt_tmp_3 = ::std::rc::Rc::new(|closure_arg_0: &SmeltUnknown, _arg0: i64, _arg1: &SmeltList<SmeltUnknown>| {
let _smelt_tmp_1: SmeltUnknown = smelt_get_unknown_field(&closure_arg_0.clone(), "endsWith").clone();
let _smelt_tmp_2: SmeltUnknown = { let smelt_function_value = _smelt_tmp_1.clone(); let smelt_call_args: Vec<SmeltUnknown> = Into::into(vec![SmeltUnknown::String("x".into())]); let smelt_callable = match smelt_function_value { SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get("__smelt_call") { Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function.clone()), _ => None }, _ => None }; if let Some(smelt_function) = smelt_callable { (smelt_function)(smelt_call_args).unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error)) } else { smelt_throw_not_callable("value.endsWith") } };
_smelt_tmp_2.clone()
});
let flags: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into({ let smelt_callback = ::std::rc::Rc::new(|closure_arg_0: &SmeltUnknown, _arg0: i64, _arg1: &SmeltList<SmeltUnknown>| {
let _smelt_tmp_1: SmeltUnknown = smelt_get_unknown_field(&closure_arg_0.clone(), "endsWith").clone();
let _smelt_tmp_2: SmeltUnknown = { let smelt_function_value = _smelt_tmp_1.clone(); let smelt_call_args: Vec<SmeltUnknown> = Into::into(vec![SmeltUnknown::String("x".into())]); let smelt_callable = match smelt_function_value { SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get("__smelt_call") { Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function.clone()), _ => None }, _ => None }; if let Some(smelt_function) = smelt_callable { (smelt_function)(smelt_call_args).unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error)) } else { smelt_throw_not_callable("value.endsWith") } };
_smelt_tmp_2.clone()
}); let smelt_array = _smelt_tmp_2; let smelt_value = smelt_array.borrow().iter().enumerate().map(|(smelt_index, smelt_item)| { (smelt_callback)(smelt_item, smelt_index as i64, &smelt_array) }).collect::<Vec<_>>(); smelt_value });
return;
}"#);
}

/// A string operand asked for at `bool` is a truthiness test, at the operand entry point of the coercion too.
#[test]
fn a_string_operand_at_bool_is_a_truthiness_test() {
    let source = source_for(
        r"
export function pick(parts: string[], index: number): string | null {
  const p: string = parts[index];
  const nextP: string | undefined = parts[index + 1];
  return nextP === undefined && p && p.indexOf('*') === p.length - 1 ? p : null;
}
",
    );

    assert_eq!(rust_fn(&source, "pick"), r#"fn pick(parts: SmeltList<JsString>, index: f64) -> Option<JsString> {
let mut _smelt_tmp_6: bool;
let mut _smelt_tmp_7: bool;
let _smelt_tmp_8: f64;
let _smelt_tmp_9: f64;
let _smelt_tmp_10: f64;
let _smelt_tmp_11: bool;
let mut _smelt_tmp_12: Option<JsString>;
let p: JsString = parts.borrow().get({ let smelt_normalized = index as i64; usize::try_from(smelt_normalized).unwrap_or(usize::MAX) }).cloned().unwrap_or_else(|| JsString::new());
let _smelt_tmp_4: f64 = index + 1.0;
let next_p: Option<JsString> = parts.borrow().get({ let smelt_normalized = _smelt_tmp_4 as i64; usize::try_from(smelt_normalized).unwrap_or(usize::MAX) }).cloned();
let _smelt_tmp_5: bool = next_p.is_none();
let _smelt_tmp_6: bool = if _smelt_tmp_5 { !(p.clone()).is_empty() } else { false };
let mut _smelt_tmp_7: bool = false;
if _smelt_tmp_6 {
_smelt_tmp_8 = p.index_of(&JsString::from("*"), 0).map_or(-1.0, |smelt_index| smelt_index as f64);
_smelt_tmp_9 = p.len() as f64;
_smelt_tmp_10 = _smelt_tmp_9 - 1.0;
_smelt_tmp_11 = _smelt_tmp_8 == _smelt_tmp_10;
_smelt_tmp_7 = _smelt_tmp_11;
} else {
_smelt_tmp_7 = false;
}
let _smelt_tmp_12: Option<JsString> = if _smelt_tmp_7 { Some(p) } else { None::<JsString> };
return _smelt_tmp_12;
}"#);
}
