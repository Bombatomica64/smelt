//! Codegen regression tests for four general emitter rules about *absence* and
//! about *reference identity*.
//!
//! Each one is a rule the emitter had spelled two incompatible ways at two
//! seams, which is why they sit together:
//!
//! * A list-to-list coercion whose element types render to ONE Rust type is not
//!   a coercion. It used to key on `TypeId` inequality and rebuild the backing
//!   buffer, so a by-reference argument stopped aliasing the caller's array and
//!   the callee's mutations were lost.
//! * Reading an erased value out into a `String` slot is not the JS `String(x)`
//!   conversion, so absence must not become the string `"undefined"` there --
//!   the sibling `Null` arm had always answered `String::new()`.
//! * A container of `Type::None` recovers `undefined` vs `null` from its
//!   defining literal. The list arm did; the dict arm emitted a constant
//!   `SmeltUnknown::Null` for every entry.
//! * A method reference read off an instance denotes the one prototype method,
//!   so every read has to resolve to a single canonical identity.

use super::*;

#[test]
fn a_by_reference_list_argument_with_identical_element_rendering_is_not_rebuilt() {
    // The caller's `unknown[]` and the callee's erased `T[]` are different MIR
    // types and one Rust type. Rebuilding the buffer for that pair allocates a
    // fresh `Rc<RefCell<Vec<_>>>`, so the callee splices a temporary -- the
    // caller's array is left untouched. The argument must be the caller's own
    // list.
    let source = source_for(
        r"
function dropFirst<T>(target: T[]): void {
  target.splice(0, 1);
}
export function useDropFirst(): number {
  const values: unknown[] = [1, 2, 3];
  dropFirst(values);
  return values.length;
}
",
    );

    let body = emitted_function_body(&source, "fn use_drop_first");
    assert!(
        !body.contains("SmeltList::with_id("),
        "a by-reference argument must not rebuild the buffer:\n{body}"
    );
    assert!(
        body.contains("drop_first(&mut values"),
        "expected the caller's own list to be borrowed:\n{body}"
    );
}

#[test]
fn extracting_absence_into_a_string_slot_is_not_the_string_conversion() {
    // `value as string` is a type ASSERTION: at runtime the value passes
    // through. So absence reaching a `String` slot must not be given the
    // `String(undefined)` answer `"undefined"` -- the slot cannot represent
    // absence at all, and the typed side writes the type's default for the same
    // situation, which made the two sides of an equality disagree.
    let source = source_for(
        r"
export function readString(value: unknown): string {
  return value as string;
}
",
    );

    let body = emitted_function_body(&source, "fn read_string");
    assert!(
        body.contains("SmeltUnknown::Null | SmeltUnknown::Undefined => String::new()"),
        "absence must extract as the slot's default:\n{body}"
    );
    assert!(
        !body.contains("SmeltUnknown::Undefined => \"undefined\".to_owned()"),
        "the implicit extraction must not emit the String() conversion:\n{body}"
    );
}

#[test]
fn an_object_literal_holding_undefined_erases_as_undefined() {
    // `null` and `undefined` share MIR `Type::None`, so only the defining
    // `Rvalue::Dict` knows which singleton the entries hold. Without that
    // recovery the per-entry erasure was a CONSTANT `SmeltUnknown::Null` (the
    // closure did not even read `value`), and `isJSONObject({ a: undefined })`
    // answered `true` because `null` is valid JSON.
    let source = source_for(
        r"
export function erase(): unknown {
  const holder = { a: undefined };
  return holder;
}
",
    );

    let body = emitted_function_body(&source, "fn erase");
    assert!(
        body.contains("SmeltUnknown::Undefined"),
        "an `{{ a: undefined }}` literal must erase its entry as undefined:\n{body}"
    );
    assert!(
        !body.contains("SmeltUnknown::Null"),
        "an `{{ a: undefined }}` literal must not erase its entry as null:\n{body}"
    );
}

#[test]
fn an_object_literal_holding_null_still_erases_as_null() {
    // The recovery above is keyed on the defining constants, so a genuine
    // `null` literal is untouched. Pinning both directions keeps the rule from
    // degrading into "erase `Type::None` as undefined".
    let source = source_for(
        r"
export function erase(): unknown {
  const holder = { a: null };
  return holder;
}
",
    );

    let body = emitted_function_body(&source, "fn erase");
    assert!(
        body.contains("SmeltUnknown::Null"),
        "an `{{ a: null }}` literal must keep the null tag:\n{body}"
    );
}

#[test]
fn a_class_method_reference_links_one_canonical_identity() {
    // A method reference has to capture its receiver, so each read is a fresh
    // `Rc` and a bare address comparison called two reads of one method
    // unequal. JavaScript disagrees -- the method lives once on the prototype
    // -- so every read links to one canonical id per (defining class, method).
    let source = source_for(
        r"
class Greeter {
  greet(): string {
    return 'hi';
  }
}
export function methodOf(greeter: Greeter): unknown {
  return greeter.greet;
}
",
    );

    let body = emitted_function_body(&source, "fn method_of");
    assert!(
        body.contains("smelt_link_function_identity_key(&smelt_method, smelt_method_identity("),
        "a method reference must link the canonical per-method identity:\n{body}"
    );
    assert!(
        body.contains("Greeter::greet"),
        "the identity key must name the defining class and method:\n{body}"
    );
}

#[test]
fn an_absent_key_on_a_class_receiver_reads_as_undefined() {
    // A keyed read that resolves to "no such property" is `undefined` in
    // JavaScript. It used to share the `null` tag helper, which made
    // `instance['nope'] === null` true and `=== undefined` false -- both
    // backwards.
    let source = source_for(
        r"
class Holder {
  value = 1;
}
export function readMissing(holder: Holder): unknown {
  return (holder as any)['nope'];
}
",
    );

    let body = emitted_function_body(&source, "fn read_missing");
    assert!(
        body.contains("SmeltUnknown::Undefined"),
        "an absent property read must be undefined:\n{body}"
    );
}
