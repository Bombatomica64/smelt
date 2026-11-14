//! Typed shapes of host constructor values and collection iteration.
//!
//! The behaviour is pinned by the `host_constructor_values` and
//! `collection_iterators` Node-parity fixtures; these pin the TYPES, which a
//! passing fixture cannot see:
//!
//! * a host constructor read as a value is the concrete `SmeltHostConstructor`
//!   (its name), so a list of them is a concrete list, not `SmeltUnknown`;
//! * `for...of` over a Map or Set, directly or through `entries()`, walks it by
//!   position, with no generator or resume call;
//! * `map.keys()` held as a value is a typed `SmeltGenerator` of the key type.

use super::*;

#[test]
fn host_constructor_values_are_concrete() {
    let source = source_for(
        r"
export function views(): number {
  const ctors = [Int8Array, Uint8Array, DataView];
  return ctors.length;
}
",
    );
    let body = emitted_function_body(&source, "fn views");
    assert!(
        body.contains("SmeltList<SmeltHostConstructor>"),
        "a list of host constructors is a concrete list:\n{body}"
    );
    assert!(
        !body.contains("SmeltUnknown"),
        "host constructor values are not erased:\n{body}"
    );
}

#[test]
fn collection_for_of_walks_by_position() {
    let source = source_for(
        r"
export function total(m: Map<string, number>, s: Set<number>): number {
  let sum = 0;
  for (const [, v] of m.entries()) sum += v;
  for (const [, v] of m) sum += v;
  for (const x of s) sum += x;
  return sum;
}
export function keysOf(m: Map<string, number>): Iterator<string> {
  return m.keys();
}
",
    );
    let total = emitted_function_body(&source, "fn total");
    assert!(
        total.contains("smelt_live_len()") && total.contains("smelt_entry_at("),
        "a for...of over a collection reads it by position:\n{total}"
    );
    assert!(
        !total.contains("SmeltGenerator") && !total.contains("try_resume"),
        "a for...of over a collection steps no iterator object:\n{total}"
    );
    let keys = emitted_function_body(&source, "fn keys_of");
    assert!(
        keys.contains("SmeltGenerator<JsString"),
        "`map.keys()` is a typed generator of the key type:\n{keys}"
    );
}
