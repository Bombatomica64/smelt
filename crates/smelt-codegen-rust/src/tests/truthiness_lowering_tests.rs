//! Emitted Rust for JavaScript truthiness, non-null assertions and `await`
//! over a value whose type is not a future.
//!
//! The values are checked against Node by the `truthiness_absence_identity`
//! fixture. These tests compare whole functions: which test a condition lowers
//! to is the type-preservation requirement.

use super::{rust_fn, source_for};

/// `item: T` can hold `0`, `""`, `false`, `NaN` or a nullish value, so its
/// guard is a runtime truthiness test.
#[test]
fn a_generic_condition_is_tested_rather_than_folded_to_true() {
    let source = source_for(
        r"
export function compact<T>(items: T[]): T[] {
  const out: T[] = [];
  for (const item of items) {
    if (item) {
      out.push(item);
    }
  }
  return out;
}
",
    );
    assert_eq!(rust_fn(&source, "compact"), r#"fn compact(items: SmeltList<SmeltUnknown>) -> SmeltList<SmeltUnknown> {
let mut item: SmeltUnknown;
let mut _smelt_tmp_5: usize;
let mut _smelt_tmp_6: bool;
let mut _smelt_tmp_7: bool;
let mut _smelt_tmp_8: f64;
let mut out: SmeltList<SmeltUnknown> = Into::<SmeltList<_>>::into(SmeltList::from({ let smelt_list_items: Vec<SmeltUnknown> = vec![]; smelt_list_items }));
let mut _smelt_tmp_4: usize = 0;
loop {
_smelt_tmp_5 = items.len();
_smelt_tmp_6 = _smelt_tmp_4 < _smelt_tmp_5;
if !(_smelt_tmp_6) { break; }
item = items.borrow().get(_smelt_tmp_4).cloned().unwrap_or_else(|| SmeltUnknown::Undefined);
_smelt_tmp_7 = match item.clone() { SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(smelt_value) => smelt_value, SmeltUnknown::Number(smelt_value) => smelt_value != 0.0 && !smelt_value.is_nan(), SmeltUnknown::String(smelt_value) => !smelt_value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true };
if _smelt_tmp_7 {
_smelt_tmp_8 = { let smelt_push_item = item; out.borrow_mut().push(smelt_push_item); out.len() as f64 };
_smelt_tmp_4 = _smelt_tmp_4 + 1;
continue;
} else {
_smelt_tmp_4 = _smelt_tmp_4 + 1;
continue;
}
}
return out;
}"#);
}

/// A function value is always truthy, so its guard folds and the false arm goes.
#[test]
fn a_function_typed_condition_still_folds_to_true() {
    let source = source_for(
        r"
export function describeHandler(handler: () => void): string {
  if (handler) {
    handler();
    return 'yes';
  }
  return 'no';
}
",
    );
    assert_eq!(rust_fn(&source, "describe_handler"), r#"fn describe_handler(handler: &dyn Fn() -> ()) -> JsString {
let _smelt_tmp_1: ();
_smelt_tmp_1 = handler();
return JsString::from("yes");
}"#);
}

/// `row.enabled` is erased, and negating it is a truthiness test, not a tag
/// check: a stored `false` has the boolean tag and is falsy.
#[test]
fn negating_an_erased_callback_value_emits_truthiness_not_a_tag_check() {
    let source = source_for(
        r"
export function firstDisabled(rows: Record<string, unknown>[]): number {
  return rows.findIndex(row => !row.enabled);
}
",
    );
    assert_eq!(rust_fn(&source, "first_disabled"), r#"fn first_disabled(rows: SmeltList<SmeltRecord<JsString, SmeltUnknown>>) -> f64 {
let _smelt_tmp_1 = ::std::rc::Rc::new(|closure_arg_0: SmeltRecord<JsString, SmeltUnknown>, _arg0: i64, _arg1: &SmeltList<SmeltRecord<JsString, SmeltUnknown>>| {
let _smelt_tmp_1: bool = !(match smelt_record_property(&closure_arg_0, &JsString::from("enabled")).clone() { SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(smelt_value) => smelt_value, SmeltUnknown::Number(smelt_value) => smelt_value != 0.0 && !smelt_value.is_nan(), SmeltUnknown::String(smelt_value) => !smelt_value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true });
_smelt_tmp_1
});
let _smelt_tmp_2: f64 = { let smelt_callback = ::std::rc::Rc::new(|closure_arg_0: SmeltRecord<JsString, SmeltUnknown>, _arg0: i64, _arg1: &SmeltList<SmeltRecord<JsString, SmeltUnknown>>| {
let _smelt_tmp_1: bool = !(match smelt_record_property(&closure_arg_0, &JsString::from("enabled")).clone() { SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(smelt_value) => smelt_value, SmeltUnknown::Number(smelt_value) => smelt_value != 0.0 && !smelt_value.is_nan(), SmeltUnknown::String(smelt_value) => !smelt_value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true });
_smelt_tmp_1
}); let smelt_array = rows.clone(); let smelt_value = smelt_array.borrow().iter().enumerate().find_map(|(smelt_index, smelt_item)| if (smelt_callback)(smelt_item.clone(), smelt_index as i64, &smelt_array) { Some(smelt_index as f64) } else { None }).unwrap_or(-1.0); smelt_value };
return _smelt_tmp_2;
}"#);
}

/// `typeof x === 'boolean'` is the tag check.
#[test]
fn a_typeof_boolean_comparison_still_emits_a_tag_check() {
    let source = source_for(
        r"
export function isBool(value: unknown): boolean {
  return typeof value === 'boolean';
}
",
    );
    assert_eq!(rust_fn(&source, "is_bool"), r#"fn is_bool(value: SmeltUnknown) -> bool {
let _smelt_tmp_1: bool = matches!(value.clone(), SmeltUnknown::Bool(_));
return _smelt_tmp_1;
}"#);
}

/// A `SmeltList` is an array on every path, so `Array.isArray` on an optional
/// concrete list only asks whether it is present.
#[test]
fn an_is_array_guard_on_a_concrete_list_checks_only_presence() {
    let source = source_for(
        r"
export function count(values?: string[]): number {
  return Array.isArray(values) ? values.length : 0;
}
",
    );
    assert_eq!(rust_fn(&source, "count"), r#"fn count(values: Option<SmeltList<JsString>>) -> f64 {
let mut _smelt_tmp_2: f64;
let _smelt_tmp_3: f64;
let _smelt_tmp_1: bool = values.clone().is_some();
let mut _smelt_tmp_2: f64 = 0.0;
if _smelt_tmp_1 {
_smelt_tmp_3 = values.as_ref().map_or(0, |smelt_value| smelt_value.len()) as f64;
_smelt_tmp_2 = _smelt_tmp_3;
} else {
_smelt_tmp_2 = 0.0;
}
return _smelt_tmp_2;
}"#);
}

/// An `unknown` needs the runtime array-tag match.
#[test]
fn an_is_array_guard_on_an_erased_value_keeps_its_tag_check() {
    let source = source_for(
        r"
export function isList(value: unknown): boolean {
  return Array.isArray(value);
}
",
    );
    assert_eq!(rust_fn(&source, "is_list"), r#"fn is_list(value: SmeltUnknown) -> bool {
let _smelt_tmp_1: bool = matches!(value.clone(), SmeltUnknown::Array(_));
return _smelt_tmp_1;
}"#);
}

/// `maximum!` is type-level only: the callee's parameter is optional and
/// handles absence, so the caller forwards the value unnarrowed.
#[test]
fn a_non_null_assertion_into_an_optional_parameter_forwards_the_value() {
    let source = source_for(
        r"
function span(minimum: number, maximum?: number): number {
  if (maximum == null) {
    return minimum;
  }
  return maximum - minimum;
}

export function spanFrom(minimum: number, maximum?: number): number {
  return span(minimum, maximum!);
}
",
    );
    assert_eq!(rust_fn(&source, "span_from"), r#"fn span_from(minimum: f64, maximum: Option<f64>) -> f64 {
let _smelt_tmp_2: f64 = span(minimum, maximum.clone());
return _smelt_tmp_2;
}"#);
}

/// An erased tuple element may hold a promise at runtime, so `await` drives
/// the runtime promise chain over it.
#[test]
fn awaiting_an_erased_value_keeps_the_operand() {
    let source = source_for(
        r"
function attempt(func: () => unknown): [unknown, unknown] {
  return [null, func()];
}

export async function run(): Promise<unknown> {
  const [, result] = attempt(async () => 1);
  return await result;
}
",
    );
    assert_eq!(rust_fn(&source, "run"), r#"async fn run() -> Result<SmeltUnknown, Box<dyn std::error::Error>> {
let result: SmeltUnknown;
let _smelt_tmp_5: SmeltUnknown;
let _smelt_tmp_1 = ::std::rc::Rc::new(move || { SmeltFuture::from_future_primed(Box::pin(async move {
let smelt_async_value = {
SmeltUnknown::Number(1.0 as f64)
};
Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_async_value.into_smelt_unknown())
}) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>>) });
let _smelt_tmp_2: (SmeltUnknown, SmeltUnknown) = attempt(&mut { let _smelt_adapted_callback = _smelt_tmp_1.clone(); move || { let smelt_future = (_smelt_adapted_callback)(); SmeltUnknown::Promise(SmeltPromise::from_future(Box::pin(async move { let smelt_value = smelt_future.await?; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_value) }))) } });
result = _smelt_tmp_2.1.clone();
let _smelt_tmp_4: SmeltFuture<SmeltUnknown> = { let smelt_erased_future = (result).into_smelt_unknown(); SmeltFuture::from_future(Box::pin(async move { let smelt_awaited = smelt_await_flatten(smelt_erased_future).await?; Ok::<_, Box<dyn std::error::Error>>(smelt_awaited) })) };
_smelt_tmp_5 = _smelt_tmp_4.await?;
return Ok(_smelt_tmp_5);
}"#);
}
