//! Build-and-run tests for the indexed access over a type parameter that a
//! call evaluates (`smelt_frontend_ts`, `lowering/ty/deferred_index.rs`).
//!
//! Each program is valid TypeScript whose output is compared with Node's, and
//! each test pins the Rust type the call sites are emitted with. They live
//! here rather than in the end-to-end examples because every program holds an
//! erased value by construction: the function whose return is `T[K]` reads a
//! member off its `T`, so its own body and signature are erased
//! (`generic_lifting_cli_tests`, `member_read_off_a_type_parameter_is_an_inspection`),
//! and that corpus must stay free of erased entries.

mod common;

use std::fs;

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, ensure_node_agrees, smelt,
    utf8_path,
};

/// Function items whose declared return indexes a type parameter.
const ITEM_CALLS: &str = r#"// A declared return that indexes a type parameter is evaluated by each call
// that binds the parameter; the declaration itself stays as it was.
interface Point {
  x: number;
  y: number;
}

interface Person {
  name: string;
  age: number;
}

interface Profile {
  id: number;
  nickname?: string;
}

interface Line {
  from: Point;
  label: string;
}

// The key is a type parameter a literal argument fixes.
function pluck<T, K extends keyof T>(obj: T, key: K): T[K] {
  return obj[key];
}

// The access sits under an optional.
function pluckUnless<T extends object, K extends keyof T>(obj: T, key: K, skip: boolean): T[K] | undefined {
  return skip ? undefined : obj[key];
}

// Any key of the object: the union of its property types.
function lastValue<T extends object>(obj: T): T[keyof T] {
  const keys = Object.keys(obj);
  return (obj as any)[keys[keys.length - 1]];
}

// The access is an array's element.
function allValues<T extends object>(obj: T): Array<T[keyof T]> {
  return Object.values(obj) as Array<T[keyof T]>;
}

// An access over an access.
function pluckTwice<T, A extends keyof T, B extends keyof T[A]>(obj: T, a: A, b: B): T[A][B] {
  return obj[a][b];
}

interface Tagged {
  a: Point;
  b: number;
}

// Two keys, of which the result reads only the first.
function pluckTagged<T, A extends keyof T, B extends keyof T>(obj: T, a: A, _b: B): T[A] {
  return obj[a];
}

function run(): void {
  const point: Point = { x: 1, y: 2 };
  const person: Person = { name: "ada", age: 36 };
  const line: Line = { from: point, label: "l" };
  const profile: Profile = { id: 7 };
  const scores: Record<string, number> = { a: 10, b: 20 };

  const x = pluck(point, "x");
  const name = pluck(person, "name");
  const from = pluck(line, "from");
  const nickname = pluck(profile, "nickname");
  const score = pluck(scores, "b");
  console.log(x + 1, name.toUpperCase(), from.x + from.y, nickname === undefined, score * 2);

  const kept = pluckUnless(point, "y", false);
  const skipped = pluckUnless(point, "y", true);
  console.log(kept === undefined ? "none" : kept + 1, skipped === undefined ? "none" : skipped + 1);

  const last = lastValue(point);
  const lastOfPerson = lastValue(person);
  const values = allValues(point);
  console.log(last + 1, typeof lastOfPerson, values.length, values[0] + values[1]);

  const fromY = pluckTwice(line, "from", "y");
  console.log(fromY + 1);

  // The key is not a literal: TypeScript still knows it is a key of `Point`.
  let key: keyof Point = "x";
  if (point.y > 1) {
    key = "y";
  }
  const byKey = pluck(point, key);
  console.log(byKey + 1);

  // A literal after a spread is not at the position it is written at: `"b"`
  // is the third parameter here, so the key of the result is `"a"`.
  const tagged: Tagged = { a: point, b: 5 };
  const head: [Tagged, "a"] = [tagged, "a"];
  const afterSpread = pluckTagged(...head, "b");
  console.log(afterSpread.x + afterSpread.y);
}

run();
"#;

/// Overloads whose return indexes a type parameter, over `any` implementations.
const OVERLOAD_CALLS: &str = r#"// An overload's return that indexes a type parameter is evaluated by each
// call that selects the overload; the `any` implementation is untouched.
interface Point {
  x: number;
  y: number;
}

interface Person {
  name: string;
  age: number;
}

function first<T>(value: readonly T[]): T;
function first<T extends object>(value: T): T[keyof T];
function first(value: any): any {
  return Array.isArray(value) ? value[0] : value[Object.keys(value)[0]];
}

function field<T extends object, K extends keyof T>(value: T, key: K): T[K];
function field(value: any, key: any): any {
  return value[key];
}

// The implementation has no return annotation: it takes the overload's.
function values<T extends object>(value: T): Array<T[keyof T]>;
function values(value: any) {
  return Object.values(value);
}

function run(): void {
  const point: Point = { x: 1, y: 2 };
  const person: Person = { name: "ada", age: 36 };
  const ofList = first([4, 5]);
  const ofPoint = first(point);
  const ofPerson = first(person);
  const age = field(person, "age");
  const name = field(person, "name");
  const all = values(point);
  console.log(ofList + 1, ofPoint + 1, typeof ofPerson, age + 1, name.length, all.length, all[0] + all[1]);
}

run();
"#;

/// Calls the rule does not evaluate.
const UNEVALUATED_CALLS: &str = r#"// Calls the rule does not evaluate keep the erased result they always had.
interface Point {
  x: number;
  y: number;
}

class Counter {
  count = 2;
  bump(): number {
    this.count += 1;
    return this.count;
  }
}

function pluck<T, K extends keyof T>(obj: T, key: K): T[K] {
  return obj[key];
}

function lastValue<T extends object>(obj: T): T[keyof T] {
  const keys = Object.keys(obj);
  return (obj as any)[keys[keys.length - 1]];
}

// Inside `forward`, the caller's own type parameter binds `T`: that call has
// nothing to evaluate. A call of `forward` itself does.
function forward<U extends object, K extends keyof U>(obj: U, key: K): U[K] {
  const inner = pluck(obj, key);
  return inner;
}

function run(): void {
  const point: Point = { x: 1, y: 2 };
  // `keyof Counter` names a method too, so no union of fields describes it.
  const ofCounter = lastValue(new Counter());
  const forwarded = forward(point, "y");
  const mixed: Record<string, unknown> = { a: 1, b: "two" };
  const ofUnknown = pluck(mixed, "b");
  console.log(ofCounter, forwarded + 1, ofUnknown);
}

run();
"#;

/// Build `source` as `src/main.ts`, run it, and return (generated Rust, stdout).
fn build_and_run(crate_name: &str, source: &str) -> TestResult<(String, String)> {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        format!(
            r#"[project]
name = "deferred-index"
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

/// `generated` declares every local of `locals` (`name`, Rust type) with that type.
fn ensure_locals(generated: &str, locals: &[(&str, &str)]) -> TestResult {
    for (name, ty) in locals {
        let declaration = format!("    let {name}: {ty};");
        ensure(
            generated.contains(&declaration),
            format!("missing `{declaration}` in:\n{generated}"),
        )?;
    }
    Ok(())
}

/// Rule (`deferred_index.rs`): the declared return of a function item that
/// indexes a type parameter is evaluated by each call that binds it.
///
/// Behaviour: the program prints Node's output; `pluck` is emitted exactly as
/// before (erased, the signature pinned below). Type preservation: each call
/// result has the property's type: a literal key selects one property
/// (`f64`, `JsString`, the `Point` record, `Option<JsString>` for an optional
/// one), a `Record<string, number>` gives its value, `T[keyof T]` over a
/// record whose properties share a type gives that type, the access keeps
/// its place under `| undefined` and `Array<..>`, an access over an access
/// evaluates inside out, a key that is no literal takes the type the
/// TypeScript checker resolved for the call, and a literal written after a
/// spread argument is not read as the key at its written position.
#[test]
fn indexed_access_return_is_typed_at_each_call() -> TestResult {
    let source = ITEM_CALLS;
    let expected = "2 ADA 3 true 40\n3 none\n3 number 2 3\n3\n3\n3\n";
    let (generated, stdout) = build_and_run("indexed_access_return_is_typed_at_each_call", source)?;
    ensure(
        generated.contains(
            "pub(crate) fn pluck(obj: SmeltUnknown, key: SmeltUnknown) -> SmeltUnknown {",
        ),
        format!("`pluck` is no longer emitted erased:\n{generated}"),
    )?;
    ensure_locals(
        &generated,
        &[
            ("x", "f64"),
            ("name", "JsString"),
            ("from", "Point"),
            ("nickname", "Option<JsString>"),
            ("score", "f64"),
            ("kept", "Option<f64>"),
            ("skipped", "Option<f64>"),
            ("last", "f64"),
            ("values", "SmeltList<f64>"),
            ("from_y", "f64"),
            ("by_key", "f64"),
            ("after_spread", "Point"),
        ],
    )?;
    // `string | number`: a generated union, not the erased value.
    ensure(
        generated.contains("    let last_of_person: SmeltUnion"),
        format!("`lastValue(person)` is not a generated union:\n{generated}"),
    )?;
    ensure_eq(&stdout, &expected.to_owned(), "unexpected stdout")?;
    ensure_node_agrees(
        &[("main.ts", source)],
        "main.ts",
        expected,
        "indexed_access_return_is_typed_at_each_call",
    )
}

/// Rule (`deferred_index.rs`): an overload's return that indexes a type
/// parameter is evaluated by each call that selects the overload, whatever
/// the implementation is typed (`any` here), and also when the implementation
/// has no return annotation and takes the overload's erased return.
///
/// Behaviour: the program prints Node's output. Type preservation: each call
/// result has the evaluated type; the overload whose return is a plain `T`
/// is instantiated as before.
#[test]
fn overload_indexed_access_return_is_typed_at_each_call() -> TestResult {
    let source = OVERLOAD_CALLS;
    let expected = "5 2 string 37 3 2 3\n";
    let (generated, stdout) = build_and_run(
        "overload_indexed_access_return_is_typed_at_each_call",
        source,
    )?;
    ensure_locals(
        &generated,
        &[
            ("of_list", "f64"),
            ("of_point", "f64"),
            ("age", "f64"),
            ("name", "JsString"),
            ("all", "SmeltList<f64>"),
        ],
    )?;
    ensure(
        generated.contains("    let of_person: SmeltUnion"),
        format!("`first(person)` is not a generated union:\n{generated}"),
    )?;
    ensure(
        generated
            .contains("pub(crate) fn values(value: SmeltUnknown) -> SmeltList<SmeltUnknown> {"),
        format!("the unannotated implementation no longer takes the erased return:\n{generated}"),
    )?;
    ensure_eq(&stdout, &expected.to_owned(), "unexpected stdout")?;
    ensure_node_agrees(
        &[("main.ts", source)],
        "main.ts",
        expected,
        "overload_indexed_access_return_is_typed_at_each_call",
    )
}

/// Requirement (`deferred_index.rs`): a call the rule does not evaluate keeps
/// the erased result it always had, and nothing else changes for it.
///
/// Three such calls: `keyof` of a class with a method (no union of its fields
/// describes `T[keyof T]`), a call whose object is the caller's own type
/// parameter (inside `forward`), and a dictionary of `unknown` (the property
/// type says nothing). `forward(point, "y")` itself binds a record and is
/// typed.
#[test]
fn unevaluated_indexed_access_keeps_the_erased_result() -> TestResult {
    let source = UNEVALUATED_CALLS;
    let expected = "2 3 two\n";
    let (generated, stdout) =
        build_and_run("unevaluated_indexed_access_keeps_the_erased_result", source)?;
    ensure_locals(
        &generated,
        &[
            ("of_counter", "SmeltUnknown"),
            ("inner", "SmeltUnknown"),
            ("of_unknown", "SmeltUnknown"),
            ("forwarded", "f64"),
        ],
    )?;
    ensure_eq(&stdout, &expected.to_owned(), "unexpected stdout")?;
    ensure_node_agrees(
        &[("main.ts", source)],
        "main.ts",
        expected,
        "unevaluated_indexed_access_keeps_the_erased_result",
    )
}
