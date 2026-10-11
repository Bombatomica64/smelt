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

/// F0684. A logical assignment that is one element of a statement-position
/// comma expression stores conditionally, like the same assignment standing
/// alone. The comma elements went through a copy of the expression-statement
/// dispatch that lacked the logical-assignment branch, so they reached the
/// plain assignment path: `(rec["k"] ??= 2, ..)` read a default and never
/// stored, and `(d ??= side(), ..)` evaluated `side()` unconditionally.
#[test]
fn a_logical_assignment_in_a_comma_statement_stores_conditionally() {
    let source = source_for(
        r#"
function side(): number {
  return 5;
}
export function fill(d: number | undefined, rec: Record<string, number>): number {
  let b = 0;
  (d ??= side(), b = 6);
  (rec["k"] ??= 2, b = 7);
  return (d ?? 0) + b;
}
"#,
    );

    // The conditional store temporaries `lower_logical_assignment` emits, one
    // per logical assignment, and the record write inside the guard.
    assert_eq!(
        source.matches("let __smelt_logical_value_").count(),
        2,
        "{source}"
    );
    assert!(
        source.contains("rec.insert(\"k\".to_owned(), __smelt_logical_value_"),
        "{source}"
    );
}

/// F0794. A ternary in argument position joins its arms the way the same
/// ternary does in value position: two concrete unrelated arms are a union.
/// The argument path (`conditional_expression`) asked the shared
/// reconcilability query `conditional_branch_type` instead of the ternary's
/// own join, so `console.log(c ? 1 : "a")` and `count(c ? recordA : recordB)`
/// failed to lower ("branches must have the same lowered type") and a
/// class/string pair was erased to `SmeltUnknown`, while `const v = c ? 1 :
/// "a"` was already a union.
#[test]
fn an_argument_position_ternary_joins_like_a_value_position_one() {
    let source = source_for(
        r#"
class Doc {
  title = "doc";
}
function show(v: unknown): string {
  return typeof v === "string" ? v : "obj";
}
function count(r: Record<string, number> | Record<string, string>): number {
  return Object.keys(r).length;
}
export function run(flag: boolean, r1: Record<string, number>, r2: Record<string, string>): string {
  console.log(flag ? 1 : "a");
  return show(flag ? new Doc() : "text") + count(flag ? r1 : r2);
}
"#,
    );

    // `1 | "a"` and the two records: generated unions built at the ternary.
    assert!(source.matches("= if flag { SmeltUnion").count() >= 2, "{source}");
    // `Doc | "text"`: the string arm is a union variant, not an erased value.
    assert!(source.contains("::M0(\"text\".to_owned());"), "{source}");
}
