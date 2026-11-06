//! One instantiation per generic call site.
//!
//! A free function emitted with real Rust generics
//! (`fn iterate<T, F0: Fn(&T, f64) -> T>(count, func: &F0, init: T) -> T`) is
//! instantiated by each call site exactly once. Requirements:
//!
//! * Behaviour: unchanged — the same callback runs on the same values.
//! * Type preservation: when the source proves `T` (radash
//!   `iterate(n, acc => acc + c, '')`, `T = string`), the site monomorphizes:
//!   the init value passes as `String`, the callback adapter is declared over
//!   `&String`, and the call's type is `String`. A callback declaring fewer
//!   parameters than its slot (`acc => ..` for `(value: T, i: number) => T`)
//!   is evidence for the leading parameters and the return; JavaScript calls
//!   it with both arguments and it ignores the second.
//! * A callback that only widens a position (`acc: unknown`) does not stop a
//!   value position from pinning `T`: TypeScript infers `T = number` for
//!   `iterate(5, handler, 0)`, so the call stays concrete and the adapter
//!   erases at the callback's own `unknown` parameter.
//! * Where a site cannot pin `T`, every position instantiates it at
//!   `SmeltUnknown`: a bare-`T` argument erases like the callback adapter
//!   does. Mixing a concrete init value with an erased adapter was E0308.

use super::*;

/// The radash shape: a generic arrow const with a callback and an init value.
const FIXTURE: &str = r#"
export const iterate = <T>(
  count: number,
  func: (currentValue: T, iteration: number) => T,
  initValue: T
) => {
  let value = initValue
  for (let i = 1; i <= count; i++) {
    value = func(value, i)
  }
  return value
}
export const uid = (length: number) => {
  return iterate(length, acc => { return acc + "x" }, '')
}
export function sum(): unknown {
  const handler: (acc: unknown, idx: number) => number = (acc, idx) => (acc as number) + idx
  return iterate(5, handler, 0)
}
export function erasedSum(): unknown {
  const handler: (acc: unknown, idx: number) => unknown = (acc, idx) => (acc as number) + idx
  return iterate(5, handler, 0)
}
"#;

/// A shorter callback plus a concrete init value pins `T = string`.
#[test]
fn shorter_callback_and_init_value_monomorphize_the_call() {
    let source = source_for(FIXTURE);
    let uid = emitted_function_body(&source, "fn uid(");
    assert!(
        uid.contains("move |arg0: &String, arg1: f64| -> String"),
        "the callback adapter is declared at T = String: {uid}"
    );
    assert!(
        !uid.contains("SmeltUnknown"),
        "nothing at this site erases: {uid}"
    );
}

/// A value position pins `T` past a callback that accepts `unknown`: the call
/// stays at `T = number`, and only the callback's own parameter erases.
#[test]
fn value_position_pins_the_call_past_an_unknown_callback_parameter() {
    let source = source_for(FIXTURE);
    let sum = emitted_function_body(&source, "fn sum(");
    let call = sum
        .lines()
        .find(|line| line.contains("iterate("))
        .unwrap_or_else(|| panic!("`sum` calls iterate: {sum}"));
    assert!(
        call.contains("let _smelt_tmp_2: f64 = iterate(5.0, ")
            && call.contains("move |arg0: &f64, arg1: f64| -> f64")
            && call.contains("(&(SmeltUnknown::Number(arg0.clone() as f64)), arg1)")
            && call.trim_end().ends_with(", 0.0);"),
        "the site is monomorphized at f64: {call}"
    );
}

/// A site whose callback returns `unknown` makes `T` unknown, as TypeScript
/// infers it, and erases the init value too.
#[test]
fn demoted_call_site_erases_its_bare_type_param_argument() {
    let source = source_for(FIXTURE);
    let sum = emitted_function_body(&source, "fn erased_sum(");
    let call = sum
        .lines()
        .find(|line| line.contains("iterate("))
        .unwrap_or_else(|| panic!("`erasedSum` calls iterate: {sum}"));
    assert!(
        call.contains("move |arg0: &SmeltUnknown"),
        "the callback adapter is built for the erased T: {call}"
    );
    assert!(
        call.trim_end().ends_with("SmeltUnknown::Number(0.0 as f64));")
            || call.contains(", SmeltUnknown::Number(0"),
        "the init value erases with it: {call}"
    );
}

/// An overloaded generic whose implementation returns `T | undefined` while
/// the selected overload returns `T` (`maxBy` on a provably non-empty list):
/// the call takes the implementation's absence-carrying return INSTANTIATED at
/// the call site's `T`, so a call the backend monomorphizes at `T = f64` keeps
/// an `Option<f64>` result instead of erasing it to `Option<SmeltUnknown>` on
/// the spot (es-toolkit `maxBy([1, 3, 2, NaN], x => x)`).
#[test]
fn overload_absence_return_is_instantiated_at_the_call() {
    let source = source_for(
        r"
export function maxBy<T>(items: readonly [T, ...T[]], getValue: (element: T, index: number) => number): T;
export function maxBy<T>(items: readonly T[], getValue: (element: T, index: number) => number): T | undefined;
export function maxBy<T>(items: readonly T[], getValue: (element: T, index: number) => number): T | undefined {
  let best: T | undefined = undefined;
  let bestValue = -Infinity;
  for (let i = 0; i < items.length; i++) {
    const value = getValue(items[i], i);
    if (value > bestValue) { bestValue = value; best = items[i]; }
  }
  return best;
}
export function top(): number | undefined { return maxBy([1, 3, 2, Number.NaN], x => x); }
",
    );
    let top = emitted_function_body(&source, "fn top(");
    assert!(
        !top.contains("SmeltUnknown"),
        "the call result stays Option<f64>: {top}"
    );
}
