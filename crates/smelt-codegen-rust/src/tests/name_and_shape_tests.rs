//! Codegen regression tests for four general rules about *names* and *shapes*.
//!
//! * A JavaScript property key is case-sensitive, so the `camelCase` ->
//!   `snake_case` fold that produces a Rust identifier must never decide symbol
//!   identity. A declaration `Foo` and a property `foo` used to intern to one
//!   symbol whose recorded spelling was last-writer-wins, so every erased
//!   `.foo` read in the crate was keyed `"Foo"` and answered `undefined`.
//! * An overload whose parameter is a fixed-arity tuple or a non-empty array
//!   states a *length* requirement. Only a call-site array literal proves a
//!   length; a plain `T[]` value proves nothing, and matching it against the
//!   tuple parameter selects a signature TypeScript would not.
//! * An array literal's item-type hint taken from a callee's own type parameter
//!   is not in scope at the call site. Using it made the concat operands
//!   untypable, and the emitter answered with an empty list.
//! * `A & B` over two record types is the merged record, not the union of the
//!   two. A union recovery then had to pick an arm by `SmeltUnknown` tag and
//!   silently retyped every leaf of the value it recovered.

use super::*;

#[test]
fn an_erased_property_read_keys_on_the_exact_source_spelling() {
    // `function Foo` renders as the Rust identifier `foo`, which is also the
    // rendering of the property `foo`. Interning on the rendering aliased the
    // two, and the function -- lowered last -- donated its spelling to the
    // property read.
    let source = source_for(
        r"
export function readFoo(x: unknown, y: unknown): boolean {
  return (x as any).foo === (y as any).foo;
}
function Foo(value: unknown) {
  return value;
}
export function useFoo(): unknown {
  return Foo(1);
}
",
    );

    // The erased read renders through the `smelt_get_unknown_field` helper
    // (the `Object.prototype` lookup fallback lives behind it), so the
    // assertion names the KEY rather than the surrounding shape: what this
    // test is about is which spelling the key carries.
    assert!(
        source.contains("smelt_get_unknown_field(&x.clone(), \"foo\")"),
        "an erased `.foo` read must key on the source spelling:\n{source}"
    );
    assert!(
        !source.contains(", \"Foo\")"),
        "a declaration named `Foo` must not rename the property `foo`:\n{source}"
    );
}

#[test]
fn a_tuple_overload_does_not_swallow_a_plain_array_argument() {
    // `initial`'s real overload set: the one-element tuple returns `[]`, the
    // array returns `T[]`. A `number[]` variable proves no length, so the
    // tuple signature is inapplicable and the call must keep a list result --
    // not the empty tuple, which lowers to Rust `()` and threw the call away.
    let source = source_for(
        r"
function head<T>(arr: readonly [T]): [];
function head<T>(arr: readonly T[]): T[];
function head<T>(arr: readonly T[]): T[] {
  return arr.slice(0, 1);
}
export function useHead(values: number[]): number {
  return head(values).length;
}
",
    );

    let body = emitted_function_body(&source, "fn use_head");
    assert!(
        !body.contains("smelt_tuple_values"),
        "a plain array argument must not select the tuple overload:\n{body}"
    );
}

#[test]
fn a_non_empty_array_overload_keeps_the_optional_return_for_a_plain_array() {
    // `readonly [T, ...T[]]` is TypeScript's non-empty array: it guarantees an
    // element, so its overload returns `T`. A plain `T[]` cannot prove that and
    // must select the `T | undefined` overload; collapsing the `Option` with
    // `map_or(Default::default(), ..)` manufactures a value for an empty input.
    let source = source_for(
        r"
function best<T>(items: readonly [T, ...T[]]): T;
function best<T>(items: readonly T[]): T | undefined;
function best<T>(items: readonly T[]): T | undefined {
  return items[0];
}
export function useBest(items: number[]): number | undefined {
  return best(items);
}
",
    );

    let body = emitted_function_body(&source, "fn use_best");
    assert!(
        !body.contains("map_or(Default::default()"),
        "a non-empty-array overload must not be selected for a plain array:\n{body}"
    );
}

#[test]
fn an_array_spread_at_a_callee_type_parameter_hint_still_concatenates() {
    // The call's contextual hint is `readonly T[]` with `T` the *callee's* type
    // parameter, which does not exist at the call site. Adopting it made both
    // concat operands untypable and the emitter substituted an empty list, so
    // `[...a, ...b]` silently became `[]`.
    let source = source_for(
        r"
function total<T>(items: readonly T[], getValue: (item: T) => number): number {
  let sum = 0;
  for (const item of items) {
    sum += getValue(item);
  }
  return sum;
}
export function useTotal(): number {
  const first: Array<{ a: number }> = [{ a: 1 }];
  const second = [{ a: 2 }];
  return total([...first, ...second], x => x.a);
}
",
    );

    let body = emitted_function_body(&source, "fn use_total");
    assert!(
        body.contains(".chain("),
        "a spread of two lists must emit a concatenation:\n{body}"
    );
    assert!(
        !body.contains("SmeltList::new(Vec::<SmeltUnknown>::new())"),
        "the spread operands must not collapse to an empty list:\n{body}"
    );
}

#[test]
fn a_record_intersection_merges_the_two_field_types() {
    // `A & B` describes a value that belongs to NEITHER arm of `A | B`, so
    // lowering it to a union forced a runtime recovery to pick one arm by tag
    // and retype its leaves -- turning the number `36` into the string `"36"`.
    let source = source_for(
        r"
type Named = { entries: Array<{ name: string }> };
type Aged = { entries: Array<{ age: number }> };
function merge<T, S>(left: T, right: S): T & S {
  return Object.assign({}, left, right) as T & S;
}
export function useMerge(left: Named, right: Aged): unknown {
  const merged = merge(left, right);
  return merged.entries;
}
",
    );

    let body = emitted_function_body(&source, "fn use_merge");
    assert!(
        !body.contains("from_smelt_unknown"),
        "a record intersection must not lower to a union needing tag recovery:\n{body}"
    );
}
