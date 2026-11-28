//! Build-and-run tests for which generic free functions keep real Rust
//! generics (`smelt_codegen_rust::generic_lifting`, the generic trial).
//!
//! Each program is valid TypeScript whose output is compared with Node's, and
//! each test pins the emitted signature that the rule decides. They live here
//! rather than in the end-to-end examples because each program needs an
//! erased value by construction (a source `unknown`, or a callee that compares
//! its `T`s), and that corpus must stay free of avoidable erasure.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, ensure_node_agrees, smelt,
    utf8_path,
};

/// Build `source` as `src/main.ts`, run it, and return (generated Rust, stdout).
fn build_and_run(crate_name: &str, source: &str) -> TestResult<(String, String)> {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        format!(
            r#"[project]
name = "generic-lifting"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "{crate_name}"
build = true

[runtime]
clone-strategy = "aggressive"
"#
        ),
    )?;
    fs::write(project_path.join("src/main.ts"), source)?;
    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;
    let generated = fs::read_to_string(project_path.join("dist/src/source_main.rs"))?;
    let stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    Ok((generated, stdout))
}

/// Rule: in the generic trial, a rendered type that mentions no type parameter
/// in scope (`FunctionEmitter::carrier_type_concerns_trial`) and a whole `T`
/// converted to or from the carrier through its declared `IntoSmeltUnknown` /
/// `SmeltFromUnknown` bound no longer count against the body.
///
/// Behaviour: the program prints Node's output. Type preservation: `tagAll`
/// keeps `T` beside its `unknown` values. Before the rule it was emitted as
/// `tag_all(items: SmeltList<SmeltUnknown>, ..) -> SmeltList<SmeltUnknown>`.
#[test]
fn opaque_type_parameter_beside_erased_values() -> TestResult {
    let source = r#"// `T` is only moved: pushed, handed to an `unknown` parameter, parked in an
// `unknown` slot and cast back. The body also holds `unknown` values that have
// nothing to do with `T` (`label`). None of that inspects a `T`, so `tagAll`
// keeps its type parameter; the `unknown` slots convert through the bounds
// every lifted parameter declares (`IntoSmeltUnknown`, `SmeltFromUnknown`).
function describe(value: unknown): string {
  return typeof value === "number" ? `number ${value}` : `other ${String(value)}`;
}

function tagAll<T>(items: readonly T[], label: unknown): T[] {
  const out: T[] = [];
  for (const item of items) {
    console.log(describe(item), describe(label));
    const held: unknown = item;
    out.push(held as T);
  }
  return out;
}

const numbers = tagAll([1, 2], "x");
console.log(numbers[0] + numbers[1]);
const words = tagAll(["a", "b"], 3);
console.log(words.join("-").toUpperCase());
"#;
    let expected =
        "number 1 other x\nnumber 2 other x\n3\nother a number 3\nother b number 3\nA-B\n";
    let (generated, stdout) = build_and_run("opaque_type_parameter_beside_erased_values", source)?;
    ensure(
        generated.contains(r"pub(crate) fn tag_all<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(items: SmeltList<T>, label: SmeltUnknown) -> SmeltList<T> {"),
        format!("missing `pub(crate) fn tag_all<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(items: SmeltList<T>, label: SmeltUnknown) -> SmeltList<T> {{` in:\n{generated}"),
    )?;
    ensure_eq(&stdout, &expected.to_owned(), "unexpected stdout")?;
    ensure_node_agrees(
        &[("main.ts", source)],
        "main.ts",
        expected,
        "opaque_type_parameter_beside_erased_values",
    )
}

/// Rule: a type argument at a position the emitted struct drops
/// (`crate::generic_elision`) is no inference source for a lifted parameter
/// (`generic_lifting::type_param_preserved_in_emitted_type`).
///
/// Behaviour: the program compiles and prints Node's output; before the rule
/// `empty_of<T>(tagged: Tagged) -> SmeltList<T>` was emitted and the erased
/// call in `report` failed with E0282. Type preservation: nothing in the
/// emitted signature can carry `T`, so `emptyOf` keeps the erased signature
/// (a turbofish at pinned call sites is the open follow-up).
#[test]
fn elided_argument_is_no_inference_source() -> TestResult {
    let source = r#"// `Tagged<T>` never stores a `T`, so its emitted struct drops the parameter.
// `emptyOf`'s only `T` in a parameter sits at that dropped position, which
// gives Rust nothing to infer `T` from; at a call whose result is then erased
// a generic `emptyOf<T>` would not compile (E0282). It keeps the erased
// signature instead.
interface Tagged<T> {
  tag: string;
}

function emptyOf<T>(tagged: Tagged<T>): T[] {
  console.log(tagged.tag);
  return [];
}

function report(tagged: Tagged<number>): unknown {
  return emptyOf(tagged);
}

console.log(JSON.stringify(report({ tag: "numbers" })));
"#;
    let expected = "numbers\n[]\n";
    let (generated, stdout) = build_and_run("elided_argument_is_no_inference_source", source)?;
    ensure(
        generated.contains(r"pub(crate) fn empty_of(tagged: Tagged) -> SmeltList<SmeltUnknown> {"),
        format!(
            "missing `pub(crate) fn empty_of(tagged: Tagged) -> SmeltList<SmeltUnknown> {{` in:\n{generated}"
        ),
    )?;
    ensure_eq(&stdout, &expected.to_owned(), "unexpected stdout")?;
    ensure_node_agrees(
        &[("main.ts", source)],
        "main.ts",
        expected,
        "elided_argument_is_no_inference_source",
    )
}

/// Guard on the rule above: only a whole `T` converts for free. Handing a
/// generic caller's `SmeltList<T>` to an erased callee's `&mut` list rebuilds
/// the storage, and on a throwing call there is no write-back adapter, so that
/// conversion still demotes the caller.
///
/// Behaviour: the callee's removals reach the caller's list (`numbers 3`); a
/// generic `withoutRepeats` would pass a converted temporary and print
/// `numbers 5`. This is remeda's `dropFirstBy`/`heapMaybeInsert` shape.
#[test]
fn erased_throwing_callee_keeps_mutable_list_caller_erased() -> TestResult {
    let source = r#"// `dropCopiesOfFirst` compares its `T`s, so it keeps the erased signature
// (`&mut SmeltList<SmeltUnknown>`), and it can throw. `withoutRepeats` only
// moves its `T`, but it hands its own list to that callee to be mutated in
// place. On a throwing call there is no write-back adapter, so a generic
// `withoutRepeats` would pass a converted temporary and the removals would be
// lost; it keeps the erased signature instead and borrows `copy` directly.
function dropCopiesOfFirst<T>(list: T[]): void {
  if (list.length === 0) {
    throw new Error("empty list");
  }
  for (let index = list.length - 1; index > 0; index--) {
    if (list[index] === list[0]) {
      list.splice(index, 1);
    }
  }
}

function withoutRepeats<T>(data: readonly T[], label: unknown): T[] {
  const copy = data.slice();
  dropCopiesOfFirst(copy);
  console.log(String(label), copy.length);
  return copy;
}

console.log(withoutRepeats([1, 2, 1, 3, 1], "numbers").join(","));
"#;
    let expected = "numbers 3\n1,2,3\n";
    let (generated, stdout) = build_and_run(
        "erased_throwing_callee_keeps_mutable_list_caller_erased",
        source,
    )?;
    ensure(
        generated.contains(
            r"pub(crate) fn without_repeats(data: SmeltList<SmeltUnknown>, label: SmeltUnknown)",
        ),
        format!(
            "missing `pub(crate) fn without_repeats(data: SmeltList<SmeltUnknown>, label: SmeltUnknown)` in:\n{generated}"
        ),
    )?;
    ensure(
        generated.contains(r"let _ = drop_copies_of_first(&mut copy)?;"),
        format!("missing `let _ = drop_copies_of_first(&mut copy)?;` in:\n{generated}"),
    )?;
    ensure_eq(&stdout, &expected.to_owned(), "unexpected stdout")?;
    ensure_node_agrees(
        &[("main.ts", source)],
        "main.ts",
        expected,
        "erased_throwing_callee_keeps_mutable_list_caller_erased",
    )
}

/// Rule: a place that projects a member off a value whose type is the bare
/// type parameter (`obj[key]` with `obj: T`) inspects that `T` wherever the
/// place appears, a `return` operand included
/// (`generic_lifting::place_projects_into_type_param`), so the parameter does
/// not lift.
///
/// Behaviour: every function compiles and prints Node's output. Before the
/// rule `pluck` was emitted as `pluck<T: ..>(obj: T, key: SmeltUnknown)` with a
/// body that indexed `obj` as the erased carrier: 6 x E0308 on a valid
/// program. Type preservation: `second`, which reads an element out of a
/// `T[]` (a container read), keeps its `T` beside the erased `K`.
///
/// `second`'s `K` is bounded by `PropertyKey` so that it stays erased and the
/// partial lift is what the test exercises. It was `K extends string` until
/// a parameter bounded by one primitive keyword started lowering as that
/// primitive (`lowering/ty/primitive_params.rs`): `_label` would be a
/// `JsString` and `second` fully typed, which that rule's own example
/// (`201_primitive_bounded_type_parameter`) covers. The pinned signature is
/// unchanged.
///
/// Why this is not an end-to-end example: `T[K]` has no Rust type, so each
/// member-reading function here is erased by construction, and that corpus
/// must stay free of erased entries.
#[test]
fn member_read_off_a_type_parameter_is_an_inspection() -> TestResult {
    let source = r#"// A member read off a value whose type is a bare type parameter (`obj[key]`
// with `obj: T`) is a lookup on the value, so the function cannot keep `T`
// opaque: every shape below must still compile and print what Node prints.
interface Point {
  x: number;
  y: number;
  label: string;
}

// The read is the returned operand itself.
function pluck<T, K extends keyof T>(obj: T, key: K): T[K] {
  return obj[key];
}

// The read lands in a local first.
function pluckLocal<T, K extends keyof T>(obj: T, key: K): T[K] {
  const value = obj[key];
  return value;
}

// The read goes through an element of a `T[]`.
function firstField<T, K extends keyof T>(items: T[], key: K): T[K] {
  return items[0][key];
}

// The member is written, not read.
function assign<T, K extends keyof T>(obj: T, key: K, value: T[K]): T {
  obj[key] = value;
  return obj;
}

// The read feeds a comparison.
function hasKey<T, K extends keyof T>(obj: T, key: K): boolean {
  return obj[key] !== undefined;
}

// A read off a CONTAINER of `T` stays a move: `T` is still a Rust generic.
function second<T, K extends PropertyKey>(items: T[], _label: K): T {
  return items[1];
}

const point: Point = { x: 1, y: 2, label: "origin" };
console.log(pluck(point, "x"));
console.log(pluck(point, "label"));
console.log(pluckLocal(point, "y"));
console.log(firstField([point], "label"));
const moved = assign(point, "x", 7);
console.log(moved.x, point.x);
console.log(hasKey(point, "y"));
console.log(second([1, 2, 3], "n"));
console.log(second(["a", "b"], "s"));
"#;
    let expected = "1\norigin\n2\norigin\n7 7\ntrue\n2\nb\n";
    let (generated, stdout) =
        build_and_run("member_read_off_a_type_parameter_is_an_inspection", source)?;
    for signature in [
        r"pub(crate) fn pluck(obj: SmeltUnknown, key: SmeltUnknown) -> SmeltUnknown {",
        r"pub(crate) fn second<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(items: SmeltList<T>, _label: SmeltUnknown) -> T {",
    ] {
        ensure(
            generated.contains(signature),
            format!("missing `{signature}` in:\n{generated}"),
        )?;
    }
    ensure_eq(&stdout, &expected.to_owned(), "unexpected stdout")?;
    ensure_node_agrees(
        &[("main.ts", source)],
        "main.ts",
        expected,
        "member_read_off_a_type_parameter_is_an_inspection",
    )
}

/// Rule (`smelt_codegen_rust::generic_inspection::reinterprets_type_param`):
/// a body that hands a `T` value, narrowed by `Array.isArray`, to a callback
/// reading it as `T[]` reinterprets its `T` and stays erased, although the
/// guard alone is a scalar inspection that would keep `T` generic.
///
/// Behaviour: the program prints Node's output for a `T` instantiated with
/// `number[]` (the nested lists are walked, not rebuilt as empty lists).
/// Type preservation: `depthFirst` is erased, and `countArrays`, which only
/// asks the guard, keeps its `T`.
///
/// Planted defect: with `reinterprets_type_param` answering `false`,
/// `depthFirst` lifts and prints `0` for the nested input.
#[test]
fn narrowed_type_parameter_read_as_a_container_stays_erased() -> TestResult {
    let source = r"// `item` is a `T`; after the guard TypeScript lets it be passed as a `T[]`.
function depthFirst<T>(items: readonly T[]): number {
  let leaves = 0;
  const visit = (arr: readonly T[]) => {
    for (const item of arr) {
      if (Array.isArray(item)) {
        visit(item);
      } else {
        leaves += 1;
      }
    }
  };
  visit(items);
  return leaves;
}

// Only the guard: `T` stays a Rust generic.
function countArrays<T>(items: readonly T[]): number {
  let arrays = 0;
  for (const item of items) {
    if (Array.isArray(item)) {
      arrays += 1;
    }
  }
  return arrays;
}

const nested: number[][] = [[1, 2], [3], []];
console.log(depthFirst(nested), depthFirst([1, 2, 3]));
console.log(countArrays(nested), countArrays([1, 2, 3]));
";
    let expected = "3 3\n3 0\n";
    let (generated, stdout) = build_and_run(
        "narrowed_type_parameter_read_as_a_container_stays_erased",
        source,
    )?;
    for signature in [
        r"pub(crate) fn depth_first(items: SmeltList<SmeltUnknown>) -> f64 {",
        r"pub(crate) fn count_arrays<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(items: SmeltList<T>) -> f64 {",
    ] {
        ensure(
            generated.contains(signature),
            format!("missing `{signature}` in:\n{generated}"),
        )?;
    }
    ensure_eq(&stdout, &expected.to_owned(), "unexpected stdout")?;
    ensure_node_agrees(
        &[("main.ts", source)],
        "main.ts",
        expected,
        "narrowed_type_parameter_read_as_a_container_stays_erased",
    )
}

/// Rule (`smelt_codegen_rust::generic_inspection`): a scalar inspection of an
/// OPTIONAL type parameter value (`value?: T`) is answered on the erased copy
/// of the whole optional, so an absent argument, a falsy one and a present
/// one each answer as JavaScript does and the function keeps its `T`.
///
/// Behaviour: stdout equals Node's, including a class instance (recovering a
/// `T` from `undefined` for the absent side of the test would panic there).
/// Type preservation: both signatures keep `T`.
///
/// Why this is not in example `202_inspected_type_parameter_stays_generic`:
/// a call passing a plain `T` where the parameter is `T | undefined` does not
/// monomorphize today (the call site instantiates `T` at the erased value),
/// which is avoidable erasure the examples corpus may not hold. The
/// definition is what this rule is about; the call site is a separate gap.
#[test]
fn optional_type_parameter_is_inspected_through_its_erased_copy() -> TestResult {
    let source = r"class Box {
  constructor(public size: number) {}
}
function describe<T>(value?: T): string {
  return value ? 'set' : 'unset';
}
function isGiven<T>(value?: T): boolean {
  return value !== undefined;
}
const box = new Box(2);
console.log(describe<number>(), describe(0), describe(1), describe('x'), describe(box));
console.log(isGiven<string>(), isGiven(0), isGiven(''), isGiven(box));
";
    let expected = "unset unset set set set\nfalse true true true\n";
    let (generated, stdout) = build_and_run(
        "optional_type_parameter_is_inspected_through_its_erased_copy",
        source,
    )?;
    for signature in [
        r"pub(crate) fn describe<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(value: Option<T>) -> JsString {",
        r"pub(crate) fn is_given<T: Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static>(value: Option<T>) -> bool {",
    ] {
        ensure(
            generated.contains(signature),
            format!("missing `{signature}` in:\n{generated}"),
        )?;
    }
    ensure_eq(&stdout, &expected.to_owned(), "unexpected stdout")?;
    ensure_node_agrees(
        &[("main.ts", source)],
        "main.ts",
        expected,
        "optional_type_parameter_is_inspected_through_its_erased_copy",
    )
}

/// The program of [`any_parameter_passed_along_is_an_implicit_generic`].
const ANY_PARAMETERS_SOURCE: &str = r#"function sink(x: any, label: string): string {
  return label + String(x);
}
function forward(x: any, label: string): string {
  return sink(x, label);
}
function variadic(target: any, ...rest: any[]): number {
  console.log(sink(target, "v"));
  return rest.length;
}
function holder<T>(v: T, w: any): T[] {
  const out: T[] = [v];
  console.log(w == null);
  return out;
}
function rebind(x: any): string {
  x = sink(x, "r");
  return sink(x, "again");
}
function isNothing(x: any): boolean {
  return x === undefined;
}
export function main(): void {
  const n = 5;
  console.log(forward(n, "a"));
  console.log(forward("s", "b"));
  const xs = [1, 2];
  console.log(holder(n, xs).length);
  console.log(variadic(n, 1, 2));
  console.log(rebind(7));
  console.log(isNothing(undefined), isNothing(null), isNothing(3));
}
main();
"#;

/// Rule: an `any` parameter the body only passes along is an implicit type
/// parameter (`docs/design/any-as-generic.md`,
/// `smelt_codegen_rust::settle_implicit_generics`).
///
/// Behaviour: the program prints Node's output, including `null` and
/// `undefined` kept apart and an `any` reassigned or inspected in the body.
/// Type preservation:
/// - `forward` (hands `x` to an erased callee), `holder` (beside a declared
///   `T`, asks only `w == null`) and `variadic` (a rest list beside it) take
///   `x` as a Rust generic, and their callers pass `5`, `"s"` and `[1, 2]`
///   at their own types, not through the erased carrier;
/// - `sink` (`String(x)` reads the value) and `rebind` (reassigns `x`) keep
///   `SmeltUnknown`;
/// - `isNothing` keeps `SmeltUnknown`: a caller passes `undefined`, and a
///   generic `()` would lose `undefined` against `null`.
#[test]
fn any_parameter_passed_along_is_an_implicit_generic() -> TestResult {
    let source = ANY_PARAMETERS_SOURCE;
    let expected = "a5\nbs\nfalse\n1\nv5\n2\nagainr7\ntrue false false\n";
    let (generated, stdout) = build_and_run("any_parameter_passed_along", source)?;
    let bounds = "Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static";
    for signature in [
        format!("pub(crate) fn forward<Any0: {bounds}>(x: Any0, label: JsString) -> JsString {{"),
        format!(
            "pub(crate) fn holder<T: {bounds}, Any0: {bounds}>(v: T, w: Any0) -> SmeltList<T> {{"
        ),
        format!(
            "pub(crate) fn variadic<Any0: {bounds}>(target: Any0, rest: SmeltList<SmeltUnknown>) -> f64 {{"
        ),
        "pub(crate) fn sink(x: SmeltUnknown, label: JsString) -> JsString {".to_owned(),
        "pub(crate) fn rebind(mut x: SmeltUnknown) -> JsString {".to_owned(),
        "pub(crate) fn is_nothing(x: SmeltUnknown) -> bool {".to_owned(),
        "forward(n, JsString::from(\"a\"))".to_owned(),
        "holder(n, xs)".to_owned(),
    ] {
        ensure(
            generated.contains(&signature),
            format!("missing `{signature}` in:\n{generated}"),
        )?;
    }
    ensure_eq(&stdout, &expected.to_owned(), "unexpected stdout")?;
    ensure_node_agrees(
        &[("main.ts", source)],
        "main.ts",
        expected,
        "any_parameter_passed_along_is_an_implicit_generic",
    )
}
