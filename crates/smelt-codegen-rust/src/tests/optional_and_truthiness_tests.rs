//! Codegen regression tests for absence, narrowing and object truthiness.
//!
//! Each rule surfaced in radash once its `retry`, `get`, `omit` and
//! `mapEntries` reached the emitter with their real types:
//!
//! * `value ?? null` on an optional is the optional itself; it used to unwrap
//!   with the fallback rendered at the INNER type (`Some(b.unwrap_or(0.0))`),
//!   or drop a function value altogether (`None`).
//! * A narrowed `Option<fn>` read at its function type unwraps; it used to be
//!   matched as an erased `SmeltUnknown` (E0308).
//! * An optional read at an erased type parameter keeps its absence as
//!   `SmeltUnknown::Undefined` instead of asserting presence.
//! * A tuple passed where a list is expected (`[] as []` to `T[]`) collects
//!   into a list; it used to pass the tuple through (E0308).
//! * Every object is truthy, maps and lists included; `!record` used to read
//!   `!(false)`.

use super::*;

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
    let number = emitted_function_body(&source, "fn keep_number");
    assert!(
        !number.contains("unwrap_or(0.0)"),
        "`b ?? null` must not invent a present default:\n{number}"
    );
    assert!(
        number.contains("let d: Option<f64> = b.clone();")
            || number.contains("Option<f64> = b.clone();"),
        "`b ?? null` is `b`:\n{number}"
    );
    let function = emitted_function_body(&source, "fn keep_function");
    assert!(
        !function.contains("= None::<"),
        "`b ?? null` must not drop the function:\n{function}"
    );
    assert!(
        function.contains("Option<::std::rc::Rc<dyn Fn(f64) -> f64>> = b.clone();"),
        "`b ?? null` is `b` at its own optional function type:\n{function}"
    );
}

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
    // The `??` keeps a distinct optional local, so the narrowed read reaches
    // MIR as a cast of `Optional<fn>` to `fn` rather than being copy-folded
    // into the parameter (radash `retry`'s `backoff`).
    let body = emitted_function_body(&source, "fn call_it");
    assert!(
        !body.contains("SmeltUnknown::Function"),
        "a concrete `Option<Rc<dyn Fn>>` is not an erased value:\n{body}"
    );
    assert!(
        body.contains("backoff.clone().unwrap_or_else("),
        "the narrowed optional unwraps to its function:\n{body}"
    );
}

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
    let body = emitted_function_body(&source, "fn or_default");
    assert!(
        body.contains("-> SmeltUnknown"),
        "the test relies on `TDefault` rendering erased:\n{body}"
    );
    assert!(
        !body.contains("absent after narrowing"),
        "an absent default is `undefined`, not a panic:\n{body}"
    );
    assert!(
        body.contains(crate::OPTIONAL_UNKNOWN_OR_UNDEFINED_FN),
        "the optional maps absence onto `SmeltUnknown::Undefined`:\n{body}"
    );
}

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
    let body = emitted_function_body(&source, "fn use_count");
    assert!(
        body.contains("count(SmeltList::with_id(smelt_next_object_id(), vec![]))"),
        "an empty tuple becomes an empty list:\n{body}"
    );
    assert!(
        body.contains("vec![t.0") || body.contains("vec![smelt_tuple_src.0"),
        "a pair becomes a two-element list:\n{body}"
    );
}

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
    for signature in ["fn size(", "fn size2(", "fn item_count("] {
        let body = emitted_function_body(&source, signature);
        assert!(
            !body.contains("!(false)"),
            "`!obj` on an object is `false`:\n{body}"
        );
        assert!(body.contains("!(true)"), "`obj` is truthy:\n{body}");
    }
}
