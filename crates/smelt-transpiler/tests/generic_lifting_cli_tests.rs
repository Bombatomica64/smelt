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
