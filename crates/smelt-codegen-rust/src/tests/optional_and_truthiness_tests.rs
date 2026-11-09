//! Emitted Rust for absence, narrowing and object truthiness.
//!
//! * `value ?? null` on an optional is the optional itself.
//! * A narrowed `Option<fn>` read at its function type unwraps.
//! * An optional read at an erased type parameter keeps its absence as
//!   `SmeltUnknown::Undefined` instead of asserting presence.
//! * A tuple passed where a list is expected collects into a list.
//! * Every object is truthy, maps and lists included.
//! * An all-nullish list instantiates a generic at `()`, which the runtime
//!   can un-erase into.
//!
//! The values are checked against Node by the `truthiness_absence_identity`
//! fixture; these tests pin the emitted types.

use super::*;

/// `b ?? null` is `b` at its own optional type, for data and for functions.
#[test]
fn a_nullish_fallback_of_null_keeps_the_optional_value() {
    let source = source_for(
        r"
export function keepNumber(b?: number): number {
  const d = b ?? null
  if (d) return d + 1
  return -1
}
export function keepFunction(b?: (count: number) => number): number {
  const backoff = b ?? null
  if (backoff) return backoff(2)
  return -1
}
",
    );
    assert_eq!([rust_fn(&source, "keep_number"), rust_fn(&source, "keep_function")].join("\n\n"), r#"fn keep_number(b: Option<f64>) -> f64 {
let _smelt_tmp_4: f64;
let _smelt_tmp_5: f64;
let _smelt_tmp_6: f64;
let d: Option<f64> = b.clone();
let _smelt_tmp_3: bool = d.clone().clone().is_some_and(|value| value != 0.0 && !value.is_nan());
if _smelt_tmp_3 {
_smelt_tmp_4 = d.clone().unwrap_or_else(|| 0.0);
_smelt_tmp_5 = _smelt_tmp_4 + 1.0;
return _smelt_tmp_5;
} else {
_smelt_tmp_6 = -1.0;
return _smelt_tmp_6;
}
}

fn keep_function(b: Option<::std::rc::Rc<dyn Fn(f64) -> f64>>) -> f64 {
let _smelt_tmp_4: ::std::rc::Rc<dyn Fn(f64) -> f64>;
let _smelt_tmp_5: f64;
let _smelt_tmp_6: f64;
let backoff: Option<::std::rc::Rc<dyn Fn(f64) -> f64>> = b.clone();
let _smelt_tmp_3: bool = !(backoff.clone().is_none());
if _smelt_tmp_3 {
_smelt_tmp_4 = backoff.clone().unwrap_or_else(|| { let smelt_default_callback: ::std::rc::Rc<dyn Fn(f64) -> f64> = ::std::rc::Rc::new(move |smelt_arg0: f64| -> f64 { 0.0 }); smelt_default_callback }).clone();
_smelt_tmp_5 = (_smelt_tmp_4)(2.0);
return _smelt_tmp_5;
} else {
_smelt_tmp_6 = -1.0;
return _smelt_tmp_6;
}
}"#);
}

/// A concrete `Option<Rc<dyn Fn>>` narrowed to its function stays concrete.
#[test]
fn a_narrowed_optional_function_unwraps_to_its_function() {
    let source = source_for(
        r"
export function callIt(b?: (count: number) => number): number {
  const backoff = b ?? null
  if (backoff) return backoff(2)
  return -1
}
",
    );
    assert_eq!(rust_fn(&source, "call_it"), r#"fn call_it(b: Option<::std::rc::Rc<dyn Fn(f64) -> f64>>) -> f64 {
let _smelt_tmp_4: ::std::rc::Rc<dyn Fn(f64) -> f64>;
let _smelt_tmp_5: f64;
let _smelt_tmp_6: f64;
let backoff: Option<::std::rc::Rc<dyn Fn(f64) -> f64>> = b.clone();
let _smelt_tmp_3: bool = !(backoff.clone().is_none());
if _smelt_tmp_3 {
_smelt_tmp_4 = backoff.clone().unwrap_or_else(|| { let smelt_default_callback: ::std::rc::Rc<dyn Fn(f64) -> f64> = ::std::rc::Rc::new(move |smelt_arg0: f64| -> f64 { 0.0 }); smelt_default_callback }).clone();
_smelt_tmp_5 = (_smelt_tmp_4)(2.0);
return _smelt_tmp_5;
} else {
_smelt_tmp_6 = -1.0;
return _smelt_tmp_6;
}
}"#);
}

/// An optional read at a type parameter that renders erased maps absence onto
/// `SmeltUnknown::Undefined` rather than panicking.
#[test]
fn an_optional_read_at_an_erased_type_parameter_keeps_undefined() {
    let source = source_for(
        r"
export function orDefault<TDefault = unknown>(value: any, defaultValue?: TDefault): TDefault {
  if (value === null) return defaultValue as TDefault
  return value
}
",
    );
    assert_eq!(rust_fn(&source, "or_default"), r#"fn or_default(value: SmeltUnknown, default_value: Option<SmeltUnknown>) -> SmeltUnknown {
let _smelt_tmp_3: SmeltUnknown;
let _smelt_tmp_2: bool = matches!(value.clone(), SmeltUnknown::Null);
if _smelt_tmp_2 {
_smelt_tmp_3 = smelt_optional_unknown_or_undefined(default_value.clone().clone());
return _smelt_tmp_3;
} else {
return value.clone();
}
}"#);
}

/// Tuples of any width passed to a list parameter become lists of their elements.
#[test]
fn a_tuple_argument_at_a_list_parameter_is_collected_into_a_list() {
    let source = source_for(
        r"
export function count(keys: string[]): number {
  return keys.length
}
export function useCount(): number {
  const t: [string, string] = ['a', 'b']
  return count([] as []) + count(t)
}
",
    );
    assert_eq!(rust_fn(&source, "use_count"), r#"fn use_count() -> f64 {
let _smelt_tmp_5: f64;
let t: (JsString, JsString) = (JsString::from("a"), JsString::from("b"));
let _smelt_tmp_2: () = ();
let _smelt_tmp_3: f64 = count(SmeltList::with_id(smelt_next_object_id(), vec![]));
let _smelt_tmp_4: f64 = count(SmeltList::with_id(smelt_next_object_id(), vec![t.0, t.1]));
_smelt_tmp_5 = _smelt_tmp_3 + _smelt_tmp_4;
return _smelt_tmp_5;
}"#);
}

/// `!obj` on a record or list is a constant `false`.
#[test]
fn every_object_is_truthy_including_maps_and_lists() {
    let source = source_for(
        r"
export function size<TKey extends string | number | symbol, TValue>(obj: Record<TKey, TValue>): number {
  if (!obj) return -1
  return Object.keys(obj).length
}
export function size2(obj: Record<string, number>): number {
  if (!obj) return -1
  return Object.keys(obj).length
}
export function itemCount(items: number[]): number {
  if (!items) return -1
  return items.length
}
",
    );
    // `size`'s value parameter is unconstrained, so only its guard is pinned.
    assert!(rust_fn(&source, "size").contains("let _smelt_tmp_1: bool = !(true);"), "{source}");
    assert_eq!([rust_fn(&source, "size2"), rust_fn(&source, "item_count")].join("\n\n"), r#"fn size2(obj: SmeltRecord<JsString, f64>) -> f64 {
let _smelt_tmp_2: f64;
let _smelt_tmp_3: SmeltList<JsString>;
let _smelt_tmp_4: f64;
let _smelt_tmp_1: bool = !(true);
if _smelt_tmp_1 {
_smelt_tmp_2 = -1.0;
return _smelt_tmp_2;
} else {
_smelt_tmp_3 = Into::<SmeltList<_>>::into(obj.clone().keys().filter(|smelt_key| !smelt_key.starts_with("__smelt_symbol") && smelt_is_for_in_record_key(&obj.clone(), smelt_key)).collect::<Vec<_>>());
_smelt_tmp_4 = _smelt_tmp_3.len() as f64;
return _smelt_tmp_4;
}
}

fn item_count(items: SmeltList<f64>) -> f64 {
let _smelt_tmp_2: f64;
let _smelt_tmp_3: f64;
let _smelt_tmp_1: bool = !(true);
if _smelt_tmp_1 {
_smelt_tmp_2 = -1.0;
return _smelt_tmp_2;
} else {
_smelt_tmp_3 = items.len() as f64;
return _smelt_tmp_3;
}
}"#);
}

/// An all-nullish array literal instantiates a generic at the collapsed
/// nullish type `()` (null and undefined are both `Type::None`), and the
/// runtime can un-erase into it. The list stays typed: it must not fall back
/// to `SmeltList<SmeltUnknown>`.
#[test]
fn an_all_nullish_list_instantiates_a_generic_at_unit() {
    let source = source_for(
        r"
export function uniq<T>(arr: readonly T[]): T[] { return Array.from(new Set(arr)) }
console.log(uniq([null, undefined, null]).length)
",
    );
    assert!(source.contains("impl SmeltFromUnknown for ()"), "{source}");
    assert!(source.contains(": SmeltList<()> = uniq("), "{source}");
    assert!(!source.contains("SmeltList<SmeltUnknown> = uniq("), "{source}");
}
