//! When a closure local gets a Rust binding.
//!
//! Requirements:
//!
//! * Behaviour: a closure stored through an operation that names its local
//!   (`Array.prototype.push`) is declared before that read; without the binding
//!   valid source was E0425.
//! * Type preservation: the stored closure keeps its concrete `Rc<dyn Fn>` slot.
//! * A callback the emitter re-renders at the use site (`reduce`, `sort`) gets no
//!   binding, since nothing would read it.

use super::*;

/// The local the push reads, from `let smelt_push_item = <local>.clone();`.
fn pushed_local(body: &str) -> &str {
    let (_, rest) = body
        .split_once("let smelt_push_item = ")
        .unwrap_or_else(|| panic!("the push renders its item: {body}"));
    rest.split_once(".clone()")
        .unwrap_or_else(|| panic!("the push clones its item: {body}"))
        .0
}

/// A closure read by name is declared ahead of the read.
#[test]
fn pushed_closure_is_declared_before_the_push() {
    let source = source_for(
        r"
export function collect(): number {
  const handlers: Array<(n: number) => number> = [];
  handlers.push((n: number) => n + 1);
  return handlers.length;
}
",
    );
    let body = emitted_function_body(&source, "fn collect(");
    let local = pushed_local(body);
    let declaration = body
        .find(&format!("let {local} = ::std::rc::Rc::new("))
        .unwrap_or_else(|| panic!("`{local}` is declared: {body}"));
    let push = body
        .find("let smelt_push_item")
        .unwrap_or_else(|| panic!("the push is emitted: {body}"));
    assert!(declaration < push, "declared before the push: {body}");
    assert!(
        body.contains("SmeltList<::std::rc::Rc<dyn Fn(f64) -> f64>>"),
        "the list keeps its concrete callable type: {body}"
    );
    assert!(!body.contains("SmeltUnknown"), "nothing erases: {body}");
}

/// A `reduce` callback renders inline, so a binding for it would be dead.
#[test]
fn reduce_callback_has_no_binding() {
    let source = source_for(
        r"
export function total(values: number[]): number {
  return values.reduce((sum: number, value: number) => sum + value, 0);
}
",
    );
    let body = emitted_function_body(&source, "fn total(");
    assert_eq!(
        body.matches("::std::rc::Rc::new(").count(),
        1,
        "the closure is rendered once, at the fold: {body}"
    );
    assert!(
        body.contains("let smelt_callback = ::std::rc::Rc::new("),
        "{body}"
    );
}

/// A `sort` comparator renders inline, so a binding for it would be dead.
#[test]
fn sort_comparator_has_no_binding() {
    let source = source_for(
        r"
export function ordered(values: number[]): number[] {
  return values.sort((left: number, right: number) => left - right);
}
",
    );
    let body = emitted_function_body(&source, "fn ordered(");
    assert_eq!(
        body.matches("::std::rc::Rc::new(").count(),
        1,
        "the comparator is rendered once, at the sort: {body}"
    );
}
