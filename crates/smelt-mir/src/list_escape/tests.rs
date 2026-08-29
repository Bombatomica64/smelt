//! Fixture tests for the list escape analysis.
//!
//! Each test lowers a small TypeScript program through the real frontend and
//! the real MIR optimization pipeline, then asserts the class of one named
//! list binding. Going through the whole pipeline is deliberate: the analysis
//! is meant to answer "what does codegen actually see", so a fixture that
//! hand-built MIR could drift from what lowering really produces.

use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;

use super::{ListLocalClass, analyze_list_escapes};
use crate::{BodyKey, EscapeReason, Mir, lower_hir, opt};

/// Lower TypeScript source to optimized MIR, panicking with context on failure.
fn mir_of(source: &str) -> Mir {
    let mut ctx = HirCtx::new();
    if let Err(error) = to_hir(source, FileId(0), &mut ctx) {
        panic!("HIR lowering failed: {error:?}");
    }
    let mut mir = match lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(errors) => panic!("MIR lowering failed: {errors:?}"),
    };
    opt::optimize(&mut mir);
    mir
}

/// The class of the list local named `binding` in the function named `function`.
///
/// Panics when the binding is absent, so a fixture that stops producing the
/// local it is about fails loudly instead of silently asserting nothing.
fn class_of(source: &str, function: &str, binding: &str) -> (ListLocalClass, Option<EscapeReason>) {
    let mir = mir_of(source);
    let bodies = analyze_list_escapes(&mir);
    for body in &bodies {
        if !matches!(body.key, BodyKey::Function(_)) || body.name != function {
            continue;
        }
        for fact in &body.locals {
            if fact.name.as_deref() == Some(binding) {
                return (fact.class, fact.reason);
            }
        }
    }
    panic!(
        "no list local named `{binding}` in `{function}`; bodies: {:?}",
        bodies
            .iter()
            .map(|body| (&body.name, &body.locals))
            .collect::<Vec<_>>()
    )
}

#[test]
fn returned_list_is_escaping() {
    let (class, reason) = class_of(
        "export function build(count: number): number[] {\n\
         \x20 const acc: number[] = [];\n\
         \x20 for (let index = 0; index < count; index++) { acc.push(index); }\n\
         \x20 return acc;\n\
         }\n",
        "build",
        "acc",
    );
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::Returned));
}

#[test]
fn list_stored_into_an_object_is_escaping() {
    let (class, reason) = class_of(
        "export function wrap(value: number): { items: number[] } {\n\
         \x20 const items: number[] = [value];\n\
         \x20 const box = { items: items };\n\
         \x20 return box;\n\
         }\n",
        "wrap",
        "items",
    );
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::StoredInContainer));
}

#[test]
fn list_passed_to_a_call_is_escaping() {
    let (class, reason) = class_of(
        "function total(values: number[]): number { return values.length; }\n\
         export function run(): number {\n\
         \x20 const numbers: number[] = [1, 2, 3];\n\
         \x20 return total(numbers);\n\
         }\n",
        "run",
        "numbers",
    );
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::CallArgument));
}

#[test]
fn list_captured_by_a_closure_is_escaping() {
    let (class, reason) = class_of(
        "export function run(): number {\n\
         \x20 const numbers: number[] = [1, 2, 3];\n\
         \x20 const read = (): number => numbers.length;\n\
         \x20 return read();\n\
         }\n",
        "run",
        "numbers",
    );
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::Captured));
}

#[test]
fn parameter_list_is_escaping_only_because_it_cannot_be_proven_local() {
    // A caller-supplied array may be aliased anywhere in the caller's frame.
    // Proving otherwise needs interprocedural information this pass does not
    // have, so the verdict is conservative rather than genuine — which is
    // exactly what `EscapeReason::is_genuine` reports.
    let (class, reason) = class_of(
        "export function firstOf(values: number[]): number {\n\
         \x20 return values.length;\n\
         }\n",
        "first_of",
        "values",
    );
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::UnprovenDefinition));
    assert!(
        !reason.is_some_and(EscapeReason::is_genuine),
        "a parameter is a conservative escape, not a genuine one"
    );
}

#[test]
fn list_read_out_of_a_container_is_escaping_because_it_is_a_shared_handle() {
    // `rows[0]` hands back the inner array that still lives inside `rows`, so
    // the local names a buffer another value also names.
    let (class, reason) = class_of(
        "export function inner(rows: number[][]): number {\n\
         \x20 const row = rows[0];\n\
         \x20 return row.length;\n\
         }\n",
        "inner",
        "row",
    );
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::UnprovenDefinition));
}

#[test]
fn two_names_for_one_buffer_are_aliased() {
    let (class, reason) = class_of(
        "export function run(): number {\n\
         \x20 const first: number[] = [1, 2, 3];\n\
         \x20 const second = first;\n\
         \x20 second.push(4);\n\
         \x20 return first.length + second.length;\n\
         }\n",
        "run",
        "first",
    );
    assert_eq!(class, ListLocalClass::Aliased);
    assert_eq!(reason, None);
}

#[test]
fn confined_mutated_list_is_local_mutated() {
    let (class, reason) = class_of(
        "export function run(count: number): number {\n\
         \x20 const acc: number[] = [];\n\
         \x20 for (let index = 0; index < count; index++) { acc.push(index); }\n\
         \x20 return acc.length;\n\
         }\n",
        "run",
        "acc",
    );
    assert_eq!(class, ListLocalClass::LocalMutated);
    assert_eq!(reason, None);
}

#[test]
fn confined_unmutated_list_is_local_immutable() {
    let (class, reason) = class_of(
        "export function run(): number {\n\
         \x20 const numbers: number[] = [1, 2, 3];\n\
         \x20 return numbers.length;\n\
         }\n",
        "run",
        "numbers",
    );
    assert_eq!(class, ListLocalClass::LocalImmutable);
    assert_eq!(reason, None);
}

#[test]
fn in_place_sort_keeps_the_receiver_and_its_result_in_one_group() {
    // `sort()` returns the receiver, so binding its result introduces a second
    // name for the same buffer. Missing that edge would be unsound: the sorted
    // name would look freshly minted and the receiver would look untouched.
    let (class, _) = class_of(
        "export function run(): number {\n\
         \x20 const numbers: number[] = [3, 1, 2];\n\
         \x20 const sorted = numbers.sort();\n\
         \x20 return sorted.length + numbers.length;\n\
         }\n",
        "run",
        "numbers",
    );
    assert_eq!(class, ListLocalClass::Aliased);
}

#[test]
fn a_fresh_copy_is_its_own_local_group() {
    // `slice()` allocates a new buffer, so the copy is confined even though the
    // source it was copied from is a parameter that escapes.
    let (class, _) = class_of(
        "export function run(values: number[]): number {\n\
         \x20 const copy = values.slice();\n\
         \x20 copy.push(1);\n\
         \x20 return copy.length;\n\
         }\n",
        "run",
        "copy",
    );
    assert_eq!(class, ListLocalClass::LocalMutated);
}

#[test]
fn a_map_receiver_escapes_into_its_callback() {
    // JavaScript calls `cb(item, index, array)`, and codegen emits that third
    // argument, so `map` hands the callback a handle on the receiver. Reading
    // the receiver as merely "inspected" here would be unsound: the callback
    // could store the array.
    let (class, reason) = class_of(
        "export function run(): number {\n\
         \x20 const numbers: number[] = [1, 2, 3];\n\
         \x20 const doubled = numbers.map(value => value * 2);\n\
         \x20 return doubled.length;\n\
         }\n",
        "run",
        "numbers",
    );
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::CallArgument));
}

#[test]
fn a_sort_comparator_does_not_make_the_receiver_escape() {
    // A comparator only ever sees two elements, so — unlike `map` — the sorted
    // list stays confined. This is the pair to the test above: it pins that the
    // callback rule is about which callbacks receive the array, not about
    // callbacks in general.
    let (class, _) = class_of(
        "export function run(values: number[]): number {\n\
         \x20 const sorted = values.slice().sort((left, right) => left - right);\n\
         \x20 return sorted.length;\n\
         }\n",
        "run",
        "sorted",
    );
    assert_eq!(class, ListLocalClass::LocalMutated);
}

#[test]
fn every_body_reports_each_list_local_exactly_once() {
    let mir = mir_of(
        "export function run(values: number[]): number {\n\
         \x20 const copy = values.slice();\n\
         \x20 const other: number[] = [];\n\
         \x20 other.push(copy.length);\n\
         \x20 return other.length;\n\
         }\n",
    );
    for body in analyze_list_escapes(&mir) {
        let mut seen = body.locals.iter().map(|fact| fact.local).collect::<Vec<_>>();
        let before = seen.len();
        seen.sort_unstable_by_key(|local| local.0);
        seen.dedup();
        assert_eq!(before, seen.len(), "duplicate facts in `{}`", body.name);
    }
}
