//! Codegen coverage for module-level mutable globals.
//!
//! Each lifted mutable global emits one `thread_local!` cell (const-init `Cell`
//! for copy primitives, `RefCell<String>` for strings); reads go through
//! `.with(::std::cell::Cell::get)` / `.with(|value| value.borrow().clone())`
//! and writes through a block that stores and evaluates to the stored value.
//! Emission is gated on the MIR globals table being non-empty.

use super::*;

#[test]
fn mutated_module_let_emits_const_init_cell_thread_local() {
    let source = source_for(
        r#"
let idCounter = 0;

export function uniqueId(): number {
  return ++idCounter;
}
"#,
    );
    assert!(
        source.contains(
            "static SMELT_GLOBAL_ID_COUNTER_0: ::std::cell::Cell<f64> = const { ::std::cell::Cell::new(0.0) };"
        ),
        "expected const-init Cell thread_local, got:\n{source}"
    );
    assert!(
        source.contains("SMELT_GLOBAL_ID_COUNTER_0.with(::std::cell::Cell::get)"),
        "expected Cell get read, got:\n{source}"
    );
    assert!(
        source.contains("SMELT_GLOBAL_ID_COUNTER_0.with(|value| value.set(smelt_global_value))"),
        "expected Cell set write returning the stored value, got:\n{source}"
    );
}

#[test]
fn mutated_module_string_let_emits_refcell_thread_local() {
    let source = source_for(
        r#"
let lastTag = '';

export function remember(tag: string): string {
  lastTag = tag;
  return lastTag;
}
"#,
    );
    assert!(
        source.contains(
            "static SMELT_GLOBAL_LAST_TAG_0: ::std::cell::RefCell<String> = ::std::cell::RefCell::new(\"\".to_owned());"
        ),
        "expected RefCell<String> thread_local, got:\n{source}"
    );
    assert!(
        source.contains("SMELT_GLOBAL_LAST_TAG_0.with(|value| value.borrow().clone())"),
        "expected RefCell clone read, got:\n{source}"
    );
    assert!(
        source.contains(
            "SMELT_GLOBAL_LAST_TAG_0.with(|value| *value.borrow_mut() = smelt_global_value.clone())"
        ),
        "expected RefCell replace write, got:\n{source}"
    );
}

#[test]
fn bool_global_emits_bool_cell() {
    let source = source_for(
        r#"
let enabled = false;

export function toggle(): boolean {
  enabled = !enabled;
  return enabled;
}
"#,
    );
    assert!(
        source.contains(
            "static SMELT_GLOBAL_ENABLED_0: ::std::cell::Cell<bool> = const { ::std::cell::Cell::new(false) };"
        ),
        "expected bool Cell thread_local, got:\n{source}"
    );
}

#[test]
fn no_globals_emits_no_global_thread_locals() {
    let source = source_for(
        r#"
let greeting = 'hello';

export function greet(): string {
  return greeting;
}
"#,
    );
    assert!(
        !source.contains("SMELT_GLOBAL_"),
        "non-mutated module let must not emit global thread_locals, got:\n{source}"
    );
}

#[test]
fn delete_on_list_receiver_emits_noop_true() {
    // JavaScript `delete list[i]` punches a hole; Smelt lists are dense
    // `Vec<T>` with no hole representation, so the delete lowers to a
    // successful no-op (`true`) — the explicit-deferral style shared with
    // no-op list `length` growth. Regression for the es-toolkit compat
    // `unset(list, index)` specialization.
    let source = source_for(
        r#"
export function unsetIndex(values: number[], index: number): boolean {
  // @ts-expect-error delete on a dense array models a sparse hole
  return delete values[index];
}
"#,
    );
    assert!(
        source.contains("fn unset_index"),
        "expected the function to emit, got:\n{source}"
    );
    assert!(
        source.contains("bool = true"),
        "expected the delete to emit a successful no-op `true`, got:\n{source}"
    );
}
