//! Regression tests for the TypeScript frontend's drifted duplicate helpers
//! (haiku audit §2.4, `blocker-logs/haiku-audit-followup.md`).
//!
//! Each case is a construct that lowered differently depending on which of two
//! copies of a helper handled it. The copies are now one definition; these
//! goldens pin the emitted Rust of the path that used to take the divergent
//! copy. Where the divergence was a runtime value, a runtime tier runs the
//! program as well (named on each test).

use super::*;

/// F0620. An identifier element of an array literal inside a compact callback
/// resolves like any other callback identifier. A reduced copy of the
/// resolution here knew only params, locals and a forward-callable placeholder,
/// so a module constant read inside a FUNCTION's callback (where it is not a
/// local) became `SmeltUnknown::Null` and erased the list to
/// `SmeltList<SmeltUnknown>`: `[x, LIMIT]` was `[x, null]`.
#[test]
fn a_callback_array_literal_reads_a_module_constant() {
    let source = source_for(
        r"
const LIMIT = 3;
export function pairs(xs: number[]): number[][] {
  return xs.map((x) => [x, LIMIT]);
}
",
    );

    assert!(
        source.contains("let smelt_list_items: Vec<f64> = vec![closure_arg_0, 3.0];"),
        "{source}"
    );
    assert!(!source.contains("SmeltUnknown::Null]"), "{source}");
}

/// F0620. `undefined` as an array element in a `flatMap` callback, which has
/// no closure-body fallback, was "unresolved callback identifier" under the
/// reduced copy; it is the `undefined` literal like everywhere else.
#[test]
fn a_flat_map_callback_array_literal_holds_undefined() {
    let source = source_for(
        r"
export function holes(xs: number[]): number {
  return xs.flatMap((x) => [x, undefined]).length;
}
",
    );

    assert!(source.contains("fn holes("), "{source}");
}
