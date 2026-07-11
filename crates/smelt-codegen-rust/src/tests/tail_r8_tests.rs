//! Codegen regression tests for the es-toolkit "tail round 8" fixes.
//!
//! These cover two general lowering fixes surfaced by the generated
//! es-toolkit crate:
//!
//! * `indexOf`/`lastIndexOf` with a `fromIndex` hoist their match predicate
//!   into a standalone `let smelt_predicate = ...` binding. Because the closure
//!   is no longer inline in the iterator chain, Rust cannot infer its parameter
//!   type from `&_`, so the element type must be spelled out (was E0282 in
//!   `indexOf`).
//! * `flat`/`flatMap` depth normalization applied `.max(0.0)` directly to the
//!   depth expression. When the depth is a bare float literal the receiver type
//!   is ambiguous, so the depth expression must be cast to `f64` first (was
//!   E0689 in `invoke`).

use super::*;

#[test]
fn index_of_from_index_predicate_has_concrete_param_type() {
    // `array.indexOf(x, from)` lowers through the `fromIndex` window form that
    // hoists the predicate into its own binding; the parameter must be typed.
    let source = source_for(
        r"
function find(xs: unknown[], x: unknown, from: number): number {
  return xs.indexOf(x, from);
}
",
    );
    assert!(
        !source.contains("|item: &_|"),
        "hoisted predicate must not use an uninferable `&_` param:\n{source}"
    );
    assert!(
        source.contains("let smelt_predicate = |item: &SmeltUnknown|"),
        "expected concrete predicate param type, got:\n{source}"
    );
}

#[test]
fn flat_depth_literal_disambiguates_float_receiver() {
    // A `flat`/`flatMap` whose depth is a bare literal must cast to `f64` before
    // calling `.max`, otherwise the numeric receiver type is ambiguous.
    let source = source_for(
        r"
function flatten(xs: unknown[]): unknown[] {
  return xs.flat(1);
}
",
    );
    assert!(
        source.contains("as f64).max(0.0).floor() as i64"),
        "expected depth cast to f64 before `.max`, got:\n{source}"
    );
}
