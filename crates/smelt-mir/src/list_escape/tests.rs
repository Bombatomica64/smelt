//! Fixture tests for the list escape analysis.
//!
//! Each test lowers a small TypeScript program through the real frontend and
//! the real MIR optimization pipeline, then asserts the class of one named
//! list binding. Going through the whole pipeline is deliberate: the analysis
//! is meant to answer "what does codegen actually see", so a fixture that
//! hand-built MIR could drift from what lowering really produces.

use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;

use super::summary::{CallSummaries, FunctionSummary};
use super::{FunctionBody, ListLocalClass, analyze_body, analyze_list_escapes};
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
fn list_passed_to_a_retaining_call_is_escaping() {
    // `keep` hands its parameter straight back, so the caller's buffer really
    // is observable after the call returns.
    let (class, reason) = class_of(
        "function keep(values: number[]): number[] { return values; }\n\
         export function run(): number {\n\
         \x20 const numbers: number[] = [1, 2, 3];\n\
         \x20 return keep(numbers).length;\n\
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

// ---------------------------------------------------------------------------
// Interprocedural summaries
// ---------------------------------------------------------------------------

/// The computed summary of the function named `function`.
///
/// Panics when no such function exists, so a fixture that stops producing the
/// body it is about fails loudly.
fn summary_of(source: &str, function: &str) -> FunctionSummary {
    let mir = mir_of(source);
    let summaries = CallSummaries::compute(&mir);
    for (index, candidate) in mir.functions.iter().enumerate() {
        if mir.symbols.get(candidate.name) == Some(function) {
            return summaries
                .resolved(crate::FuncId(u32::try_from(index).expect("function index fits u32")))
                .unwrap_or_else(|| panic!("`{function}` has no usable summary"))
                .clone();
        }
    }
    panic!("no function named `{function}`");
}

/// The class of `binding` in `function` when every call is treated as an
/// unknown callee, i.e. with the purely per-body analysis.
///
/// Pairs with [`class_of`] so a test can pin that a verdict really moved
/// *because* of the interprocedural summaries and not for some other reason.
fn per_body_class_of(source: &str, function: &str, binding: &str) -> ListLocalClass {
    let mir = mir_of(source);
    let summaries = CallSummaries::none(&mir);
    for candidate in &mir.functions {
        if mir.symbols.get(candidate.name) != Some(function) {
            continue;
        }
        let body = FunctionBody::from_function(candidate);
        for fact in analyze_body(&mir, &summaries, &body) {
            if fact.name.as_deref() == Some(binding) {
                return fact.class;
            }
        }
    }
    panic!("no list local named `{binding}` in `{function}`");
}

#[test]
fn a_callee_that_retains_its_parameter_reports_the_parameter_as_escaping() {
    let summary = summary_of(
        "export function keep(values: number[]): number[] { return values; }\n",
        "keep",
    );
    assert_eq!(summary.param_escapes, vec![true]);
}

#[test]
fn a_callee_that_only_reads_its_parameter_reports_it_as_confined() {
    let summary = summary_of(
        "export function total(values: number[]): number { return values.length; }\n",
        "total",
    );
    assert_eq!(summary.param_escapes, vec![false]);
    assert_eq!(summary.param_mutated, vec![false]);
}

#[test]
fn an_argument_to_a_non_retaining_callee_is_confined_in_the_caller() {
    // The whole point of the summaries: per-body this is `escaping` with reason
    // `call-argument`, and `total` provably keeps no handle.
    let source = "function total(values: number[]): number { return values.length; }\n\
                  export function run(): number {\n\
                  \x20 const numbers: number[] = [1, 2, 3];\n\
                  \x20 return total(numbers);\n\
                  }\n";
    assert_eq!(
        per_body_class_of(source, "run", "numbers"),
        ListLocalClass::Escaping,
        "the per-body analysis must still be pessimistic here"
    );
    let (class, reason) = class_of(source, "run", "numbers");
    assert_eq!(class, ListLocalClass::LocalImmutable);
    assert_eq!(reason, None);
}

#[test]
fn a_callee_that_mutates_its_parameter_marks_the_argument_mutated() {
    // Not an escape — but the caller must still count the write, or the
    // immutable/mutated split would under-report in-place mutation.
    let summary = summary_of(
        "export function seed(values: number[]): void { values.push(1); }\n",
        "seed",
    );
    assert_eq!(summary.param_escapes, vec![false]);
    assert_eq!(summary.param_mutated, vec![true]);

    let (class, _) = class_of(
        "function seed(values: number[]): void { values.push(1); }\n\
         export function run(): number {\n\
         \x20 const numbers: number[] = [];\n\
         \x20 seed(numbers);\n\
         \x20 return numbers.length;\n\
         }\n",
        "run",
        "numbers",
    );
    assert_eq!(class, ListLocalClass::LocalMutated);
}

#[test]
fn a_fresh_list_returned_by_a_callee_is_confined_in_the_caller() {
    let source = "function make(count: number): number[] {\n\
                  \x20 const acc: number[] = [];\n\
                  \x20 acc.push(count);\n\
                  \x20 return acc;\n\
                  }\n\
                  export function run(): number {\n\
                  \x20 const numbers = make(3);\n\
                  \x20 numbers.push(4);\n\
                  \x20 return numbers.length;\n\
                  }\n";
    assert!(summary_of(source, "make").returns_fresh_list);
    assert_eq!(
        per_body_class_of(source, "run", "numbers"),
        ListLocalClass::Escaping,
        "the per-body analysis must still be pessimistic here"
    );
    let (class, reason) = class_of(source, "run", "numbers");
    assert_eq!(class, ListLocalClass::LocalMutated);
    assert_eq!(reason, None);
}

#[test]
fn a_callee_that_hands_back_a_container_element_does_not_return_a_fresh_list() {
    // `rows[0]` is a handle on a buffer that still lives inside `rows`, so the
    // caller is not its only owner.
    let source = "function pick(rows: number[][]): number[] { return rows[0]; }\n\
                  export function run(rows: number[][]): number {\n\
                  \x20 const row = pick(rows);\n\
                  \x20 return row.length;\n\
                  }\n";
    assert!(!summary_of(source, "pick").returns_fresh_list);
    let (class, reason) = class_of(source, "run", "row");
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::UnprovenDefinition));
}

#[test]
fn mutual_recursion_terminates_and_confines_a_read_only_argument() {
    // Two bodies that call each other. Starting optimistic and rising to the
    // least fixpoint has to terminate here, and the answer has to be the
    // correct one: neither body keeps a handle.
    let source = "function even(values: number[], n: number): number {\n\
                  \x20 if (n <= 0) { return values.length; }\n\
                  \x20 return odd(values, n - 1);\n\
                  }\n\
                  function odd(values: number[], n: number): number {\n\
                  \x20 if (n <= 0) { return 0; }\n\
                  \x20 return even(values, n - 1);\n\
                  }\n\
                  export function run(): number {\n\
                  \x20 const numbers: number[] = [1, 2, 3];\n\
                  \x20 return even(numbers, 4);\n\
                  }\n";
    assert_eq!(summary_of(source, "even").param_escapes[0], false);
    assert_eq!(summary_of(source, "odd").param_escapes[0], false);
    let (class, _) = class_of(source, "run", "numbers");
    assert_eq!(class, ListLocalClass::LocalImmutable);
}

#[test]
fn mutual_recursion_propagates_a_real_escape_through_the_cycle() {
    // The sound half of the pair above: `odd` publishes the array into a
    // container it returns, and the fixpoint has to carry that back through
    // `even` to the call site.
    let source = "function even(values: number[], n: number): number[][] {\n\
                  \x20 if (n <= 0) { return []; }\n\
                  \x20 return odd(values, n - 1);\n\
                  }\n\
                  function odd(values: number[], n: number): number[][] {\n\
                  \x20 if (n <= 0) { return [values]; }\n\
                  \x20 return even(values, n - 1);\n\
                  }\n\
                  export function run(): number {\n\
                  \x20 const numbers: number[] = [1, 2, 3];\n\
                  \x20 return even(numbers, 4).length;\n\
                  }\n";
    assert_eq!(summary_of(source, "odd").param_escapes[0], true);
    assert_eq!(
        summary_of(source, "even").param_escapes[0],
        true,
        "the escape has to travel back around the cycle"
    );
    let (class, reason) = class_of(source, "run", "numbers");
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::CallArgument));
}

#[test]
fn self_recursion_returning_a_fresh_list_stays_fresh() {
    // The `returns_fresh_list` half of the fixpoint. Every base case mints a
    // buffer, so the recursive case inherits freshness rather than poisoning it.
    let source = "function build(n: number): number[] {\n\
                  \x20 if (n <= 0) { return []; }\n\
                  \x20 return build(n - 1);\n\
                  }\n\
                  export function run(): number {\n\
                  \x20 const numbers = build(3);\n\
                  \x20 return numbers.length;\n\
                  }\n";
    assert!(summary_of(source, "build").returns_fresh_list);
    let (class, _) = class_of(source, "run", "numbers");
    assert_eq!(class, ListLocalClass::LocalImmutable);
}

#[test]
fn self_recursion_whose_base_case_leaks_makes_the_result_unproven() {
    // The sound counterpart: one base case hands back a buffer that also lives
    // inside its argument, and the recursion inherits that.
    let source = "function build(rows: number[][], n: number): number[] {\n\
                  \x20 if (n <= 0) { return rows[0]; }\n\
                  \x20 return build(rows, n - 1);\n\
                  }\n\
                  export function run(rows: number[][]): number {\n\
                  \x20 const numbers = build(rows, 3);\n\
                  \x20 return numbers.length;\n\
                  }\n";
    assert!(!summary_of(source, "build").returns_fresh_list);
    let (class, reason) = class_of(source, "run", "numbers");
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::UnprovenDefinition));
}

#[test]
fn a_closure_call_is_an_unknown_callee_and_its_argument_escapes() {
    // Nothing names the body a closure value will run, so no summary applies.
    let (class, reason) = class_of(
        "export function run(): number {\n\
         \x20 const numbers: number[] = [1, 2, 3];\n\
         \x20 const total = (values: number[]): number => values.length;\n\
         \x20 return total(numbers);\n\
         }\n",
        "run",
        "numbers",
    );
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::CallArgument));
}

#[test]
fn the_fixpoint_visits_every_resolvable_body_at_least_once() {
    // Termination is proven by this test returning at all; the count pins that
    // the worklist really did seed every body rather than converging early on
    // an empty queue.
    let mir = mir_of(
        "function total(values: number[]): number { return values.length; }\n\
         function keep(values: number[]): number[] { return values; }\n\
         export function run(): number {\n\
         \x20 const numbers: number[] = [1, 2, 3];\n\
         \x20 return total(numbers) + keep(numbers).length;\n\
         }\n",
    );
    let summaries = CallSummaries::compute(&mir);
    assert!(summaries.analyses() >= mir.functions.len());
}

#[test]
fn an_async_callee_is_an_unknown_callee() {
    // An `async` body stores its parameters into suspended state that outlives
    // the call, so its summary is never usable.
    let mir = mir_of(
        "export async function total(values: number[]): Promise<number> {\n\
         \x20 return values.length;\n\
         }\n",
    );
    let summaries = CallSummaries::compute(&mir);
    for (index, function) in mir.functions.iter().enumerate() {
        if mir.symbols.get(function.name) == Some("total") {
            assert!(function.is_async, "fixture must lower to an async body");
            assert!(
                summaries
                    .resolved(crate::FuncId(
                        u32::try_from(index).expect("function index fits u32")
                    ))
                    .is_none(),
                "an async callee must have no usable summary"
            );
        }
    }
}

#[test]
fn a_callee_that_hands_back_its_parameter_does_not_return_a_fresh_list() {
    // The result is the caller's own argument, not a buffer minted inside the
    // callee, so treating it as uniquely owned would be plainly wrong: the
    // caller would end up with two names for one buffer and only one of them
    // reported.
    let source = "function keep(values: number[]): number[] { return values; }\n\
                  export function run(): number {\n\
                  \x20 const first: number[] = [1, 2, 3];\n\
                  \x20 const second = keep(first);\n\
                  \x20 return first.length + second.length;\n\
                  }\n";
    assert!(!summary_of(source, "keep").returns_fresh_list);
    let (class, reason) = class_of(source, "run", "second");
    assert_eq!(class, ListLocalClass::Escaping);
    assert_eq!(reason, Some(EscapeReason::UnprovenDefinition));
}
