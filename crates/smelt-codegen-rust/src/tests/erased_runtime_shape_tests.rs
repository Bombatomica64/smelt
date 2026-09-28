//! Regression tests: an erased value keeps the runtime shape JavaScript gives it.
//!
//! Two boundaries where the static type says less than the value is:
//!
//! * a function asserted to a signature with FEWER parameters
//!   (`((ctx, next) => ..) as Next`) is still the wider function, so a call
//!   through an erased `Function` must reach every parameter. The typed
//!   `Rc<dyn Fn() -> ..>` the assertion produces has no slot for the extra
//!   arguments — no concrete type, union or scoped generic of the NARROW
//!   signature can carry them — so the adapter records the full-arity erased
//!   callable, and erasing the adapter answers it;
//! * a member read through a string-keyed record view of an erased value
//!   (`(value as { then?: Function }).then`) reads the value's own property
//!   lookup, so a promise's inherited `then` is found; the record is a copy of
//!   OWN entries and cannot carry inherited members.

use super::*;

#[test]
fn an_arity_narrowing_assertion_keeps_the_full_arity_erased_callable() {
    let source = source_for(
        r"
type Next = () => Promise<void>;
function callWith(fn: Function, label: string, next: Next): Promise<void> {
  return fn(label, next);
}
export async function run(): Promise<void> {
  const narrowed = ((label: string, inner: Next) => inner()) as Next;
  await callWith(narrowed, 'x', async () => {});
}
",
    );

    assert!(
        source.contains("fn smelt_register_narrowed_callable<"),
        "the prelude must define the narrowing registry: {source}"
    );
    let registration = source
        .find("smelt_register_narrowed_callable(&smelt_narrowed_callable, ")
        .unwrap_or_else(|| panic!("the narrowing adapter must register its source: {source}"));
    // The registered value is the erased WIDE callable: it forwards both
    // parameters, which the zero-parameter adapter cannot.
    let registered = &source[registration..];
    assert!(
        registered.contains("SmeltUnknown::Function("),
        "the adapter must register an erased function: {registered}"
    );
}

#[test]
fn an_adapter_with_matching_arity_registers_nothing() {
    let source = source_for(
        r"
type Handler = (label: string) => void;
function callWith(fn: Function, label: string): void {
  fn(label);
}
export function run(): void {
  const widened = ((label: string) => { console.log(label); }) as Handler;
  callWith(widened, 'x');
}
",
    );

    assert!(
        !source.contains("smelt_register_narrowed_callable(&smelt_narrowed_callable"),
        "an adapter that drops no parameter must not register: {source}"
    );
}

#[test]
fn a_member_read_through_a_record_view_reads_the_erased_value() {
    let source = source_for(
        r"
export function isThenable(value: unknown): boolean {
  return !!(value as { then?: Function })?.then;
}
",
    );

    // One property read off the erased value — which answers a promise's
    // inherited `then` — instead of a copy of its own entries.
    assert!(
        source.contains("smelt_get_unknown_field(&value.clone(), \"then\")")
            || source.contains("smelt_get_unknown_field(&value, \"then\")"),
        "the read must go through the erased value: {source}"
    );
    assert!(
        !source.contains("SmeltRecord::with_id_from_entries(values.id"),
        "the record view must not be materialized for a member read: {source}"
    );
    // An absent property is `undefined`, the optional slot's `None`.
    assert!(
        source.contains("smelt_unknown_is_undefined(&"),
        "an undefined read must be absent: {source}"
    );
}
