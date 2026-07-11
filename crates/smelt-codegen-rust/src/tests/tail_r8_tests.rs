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
fn generic_class_without_args_gets_inference_placeholders() {
    // A local typed as a generic class but with no resolved type arguments must
    // emit inference placeholders so the reference is not missing its generics
    // (was E0107 in the generated `ImmutableCache`).
    let source = source_for(
        r"
class Cache<T> {
  data: T;
  constructor(d: T) { this.data = d; }
  clear(): Cache<T> { return new Cache<T>(this.data); }
}
",
    );
    assert!(
        !source.contains(": Cache ="),
        "generic class annotation must not drop its generic args:\n{source}"
    );
    assert!(
        source.contains("Cache<_>"),
        "expected inference placeholder for the class generic, got:\n{source}"
    );
}

#[test]
fn function_value_spread_into_record_is_erased_first() {
    // Spreading a bare function value into an object literal extracts it as a
    // record. The erased-function Rust type has no `IntoSmeltUnknown` impl, so
    // the operand must be wrapped into `SmeltUnknown::Function` before the
    // record-conversion `into_smelt_unknown()` match (was E0599).
    let source = source_for(
        r"
function f(): Record<string, unknown> {
  const g = (x: unknown): unknown => x;
  return { ...g };
}
",
    );
    assert!(
        source.contains("SmeltUnknown::Function("),
        "expected function operand to be erased into SmeltUnknown::Function:\n{source}"
    );
    // No `into_smelt_unknown()` should be applied to a raw closure binding.
    assert!(
        !source.contains("(_smelt_tmp_0).into_smelt_unknown()"),
        "into_smelt_unknown must not be called on a bare closure:\n{source}"
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
