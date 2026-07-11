//! Codegen regression tests for the es-toolkit "tail round 3" fixes.
//!
//! These cover three general lowering fixes surfaced by the generated
//! es-toolkit crate:
//!
//! * `.length` read on a CONCRETE collection (typed list) must project to the
//!   backing length as `f64` instead of a raw struct-field access that no
//!   collection type has (was E0609 in `unzipWith`).
//! * A JS relational comparison (`<`/`>`/`<=`/`>=`) with an optional operand
//!   whose held value is erased must coerce both sides with `ToNumber` and
//!   compare as `f64` instead of comparing an `Option<…>` against a float (was
//!   E0277 in `repeat`/`range`).
//! * An `Object.is` / SameValueZero comparison whose operand is a CONCRETE
//!   union must project the union to `SmeltUnknown` (`into_smelt_unknown()`)
//!   before `same_js_key`, since the tagged enum has no such method (was E0599
//!   in `decimalAdjust`).

use super::*;

#[test]
fn length_read_on_typed_list_projects_to_len_f64() {
    // `group` is an erased-element list (`SmeltList<SmeltUnknown>`); `.length`
    // must not fall through to a raw `group.length` struct field.
    let source = source_for(
        r"
function outer(rows: unknown[][]): number {
  const g = (group: unknown[]): number => group.length;
  return g(rows[0]);
}
",
    );
    assert!(
        source.contains(".len() as f64)"),
        "expected `.length` to lower to `.len() as f64`, got:\n{source}"
    );
    assert!(
        !source.contains(".length.clone()"),
        "raw list field access `.length` must not be emitted:\n{source}"
    );
}

#[test]
fn relational_compare_on_optional_erased_coerces_to_number() {
    // `n?: any` lowers to `Option<SmeltUnknown>`; `n < 1` must ToNumber-coerce
    // rather than compare `Option<SmeltUnknown>` against a float.
    let source = source_for(
        r"
function repeatish(n?: any): boolean {
  return n < 1;
}
",
    );
    // A ToNumber coercion match carries the `f64::NAN` non-numeric fallback.
    assert!(
        source.contains("f64::NAN"),
        "expected ToNumber coercion before relational compare, got:\n{source}"
    );
    assert!(
        !source.contains("n.clone() < 1"),
        "raw `Option<…> < float` comparison must not be emitted:\n{source}"
    );
}

#[test]
fn object_is_on_concrete_union_projects_to_unknown() {
    // `x` is a concrete union (`SmeltUnion…`); `Object.is` lowers to
    // `same_js_key`, which only exists on `SmeltUnknown`, so the union must be
    // projected via `into_smelt_unknown()` first.
    let source = source_for(
        r"
function sameZero(x: number | string): boolean {
  return Object.is(x, 0);
}
",
    );
    assert!(
        source.contains(".into_smelt_unknown().same_js_key(")
            || source.contains("into_smelt_unknown()")
                && source.contains(".same_js_key("),
        "expected concrete union to be erased before same_js_key, got:\n{source}"
    );
}
