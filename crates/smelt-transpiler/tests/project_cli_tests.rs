//! `smelt new` and `smelt clean`.

mod common;

use std::{fs, path::Path, process::Command};

use common::{TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq};

/// Outcome of one run: success, stdout, stderr.
type RunOutcome = (bool, String, String);

/// Runs `smelt` in `dir`.
fn run(dir: &Path, args: &[&str]) -> TestResult<RunOutcome> {
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .current_dir(dir)
        .args(args)
        .output()?;
    Ok((
        output.status.success(),
        String::from_utf8(output.stdout)?,
        String::from_utf8(output.stderr)?,
    ))
}

/// `smelt new` writes the layout specs/cli.md names, and the project builds and
/// prints its greeting.
#[test]
fn new_scaffolds_a_typescript_project_that_runs() -> TestResult {
    let temp = TempProject::new()?;
    fs::create_dir_all(temp.path())?;
    let (created, message, failure) = run(temp.path(), &["new", "hello-app"])?;
    ensure(created, failure)?;
    ensure(message.contains("hello-app"), message)?;
    let root = temp.path().join("hello-app");
    for file in ["Smelt.toml", ".gitignore", "README.md", "src/main.ts"] {
        ensure(root.join(file).is_file(), format!("missing {file}"))?;
    }
    ensure_eq(
        &fs::read_to_string(root.join(".gitignore"))?,
        &"dist/\n.smelt-cache/\n".to_owned(),
        ".gitignore",
    )?;
    let (built, _, build_errors) = run(&root, &["build"])?;
    ensure(built, build_errors)?;
    let greeting = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(
        &greeting,
        &"Hello from hello-app!\n".to_owned(),
        "the scaffolded program's output",
    )
}

/// `--python` scaffolds `src/main.py` instead, and that project runs too.
#[test]
fn new_python_scaffolds_a_python_project_that_runs() -> TestResult {
    let temp = TempProject::new()?;
    fs::create_dir_all(temp.path())?;
    let (created, _, failure) = run(temp.path(), &["new", "py_app", "--python"])?;
    ensure(created, failure)?;
    let root = temp.path().join("py_app");
    ensure(
        root.join("src/main.py").is_file() && !root.join("src/main.ts").exists(),
        "wrong entry",
    )?;
    let (built, _, build_errors) = run(&root, &["build"])?;
    ensure(built, build_errors)?;
    let greeting = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(
        &greeting,
        &"Hello from py_app!\n".to_owned(),
        "the scaffolded program's output",
    )
}

/// An existing non-empty directory and an invalid name are refused, and
/// nothing is written.
#[test]
fn new_refuses_a_non_empty_directory_and_a_bad_name() -> TestResult {
    let temp = TempProject::new()?;
    fs::create_dir_all(temp.path().join("taken"))?;
    fs::write(temp.path().join("taken/keep.txt"), "mine")?;
    let (taken, _, taken_error) = run(temp.path(), &["new", "taken"])?;
    ensure(!taken && taken_error.contains("not empty"), taken_error)?;
    ensure(
        !temp.path().join("taken/Smelt.toml").exists(),
        "wrote into a non-empty directory",
    )?;

    let (escaped, _, escape_error) = run(temp.path(), &["new", "../escape"])?;
    ensure(
        !escaped && escape_error.contains("invalid project name"),
        escape_error,
    )?;

    fs::create_dir_all(temp.path().join("empty"))?;
    let (filled, _, fill_error) = run(temp.path(), &["new", "empty"])?;
    ensure(filled, fill_error)
}

/// A project whose output target is `target`, with one built-free entry.
fn project_with_target(root: &Path, target: &str) -> TestResult {
    fs::create_dir_all(root.join("src"))?;
    fs::write(root.join("src/main.ts"), "console.log(1);\n")?;
    fs::write(
        root.join("Smelt.toml"),
        format!(
            "[project]\nname = \"clean-me\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\n\n\
             [output]\ntarget = \"{target}\"\ncrate-name = \"clean_me\"\nbuild = false\n"
        ),
    )?;
    Ok(())
}

/// A target holding only what smelt wrote is removed without asking; a missing
/// one is nothing to do.
#[test]
fn clean_removes_generated_output() -> TestResult {
    let temp = TempProject::new()?;
    let root = temp.path();
    project_with_target(root, "./dist")?;
    let (built, _, build_errors) = run(root, &["build"])?;
    ensure(built, build_errors)?;
    ensure(
        root.join("dist/src/main.rs").is_file(),
        "nothing was generated",
    )?;
    let (cleaned, message, failure) = run(root, &["clean"])?;
    ensure(cleaned, failure)?;
    ensure(message.contains("Removed"), message)?;
    ensure(!root.join("dist").exists(), "dist is still there")?;
    ensure(root.join("src/main.ts").is_file(), "the source was touched")?;
    let (again, repeat, repeat_error) = run(root, &["clean"])?;
    ensure(again && repeat.contains("Nothing to clean"), repeat_error)
}

/// A file smelt did not write stops the clean when nobody can confirm, and
/// `--force` removes it.
#[test]
fn clean_asks_before_removing_foreign_files() -> TestResult {
    let temp = TempProject::new()?;
    let root = temp.path();
    project_with_target(root, "./dist")?;
    let (built, _, build_errors) = run(root, &["build"])?;
    ensure(built, build_errors)?;
    fs::write(root.join("dist/notes.txt"), "mine")?;
    let (cleaned, _, refusal) = run(root, &["clean"])?;
    ensure(!cleaned, "foreign files were removed without confirmation")?;
    ensure(
        refusal.contains("notes.txt") && refusal.contains("--force"),
        refusal,
    )?;
    ensure(
        root.join("dist/notes.txt").is_file(),
        "the foreign file is gone",
    )?;
    let (forced, _, force_error) = run(root, &["clean", "--force"])?;
    ensure(forced, force_error)?;
    ensure(!root.join("dist").exists(), "--force left dist")
}

/// A target that holds the project is refused even with `--force`.
#[test]
fn clean_never_removes_the_project() -> TestResult {
    let temp = TempProject::new()?;
    let root = temp.path().join("app");
    project_with_target(&root, ".")?;
    let (cleaned, _, refusal) = run(&root, &["clean", "--force"])?;
    ensure(!cleaned && refusal.contains("holds the project"), refusal)?;
    project_with_target(&root, "..")?;
    let (outer, _, outer_refusal) = run(&root, &["clean", "--force"])?;
    ensure(
        !outer && outer_refusal.contains("holds the project"),
        outer_refusal,
    )?;
    ensure(
        root.join("src/main.ts").is_file(),
        "the project was touched",
    )
}

/// Manifest of a project whose only source is `file`, tested with vitest globs.
fn unmodelled_method_manifest(file: &str) -> String {
    format!(
        r#"[project]
name = "unmodelled"
version = "0.1.0"

[sources]
roots = ["src"]
test-globs = ["**/*.test.ts"]
entries = ["src/{file}"]

[output]
target = "./dist"
crate-name = "unmodelled"
build = false

[runtime]
clone-strategy = "aggressive"
"#
    )
}

/// Outside a test file, a method the runtime does not model stops the build
/// and names the method.
#[test]
fn an_unmodelled_method_fails_a_non_test_file() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("src/main.ts"),
        "const s: string = \"abc\";\nconsole.log(s.anchor(\"x\"));\n",
    )?;
    fs::write(
        root.join("Smelt.toml"),
        unmodelled_method_manifest("main.ts"),
    )?;
    let (ok, stdout, stderr) = run(root, &["build"])?;
    ensure(!ok, format!("build must fail\nstdout:\n{stdout}"))?;
    ensure(
        stderr.contains("`String.prototype.anchor` is not modelled"),
        format!("the diagnostic names the method:\n{stderr}"),
    )
}

/// In a test file the same method blocks only the test that reaches it: the
/// crate builds, that test fails naming the model, and its sibling passes.
#[test]
fn an_unmodelled_method_blocks_only_its_test() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("src/page.test.ts"),
        r#"import { expect, it } from "vitest";

it("reaches the missing method", () => {
  const s: string = "abc";
  expect(s.anchor("x")).toBe("anything");
});

it("does not reach it", () => {
  expect(1 + 1).toBe(2);
});
"#,
    )?;
    fs::write(
        root.join("Smelt.toml"),
        unmodelled_method_manifest("page.test.ts"),
    )?;
    let (ok, stdout, stderr) = run(root, &["build"])?;
    ensure(
        ok,
        format!("build must succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"),
    )?;
    // A dedicated target directory: an inherited one is shared by every crate
    // that happens to be named alike.
    let output = Command::new("cargo")
        .arg("test")
        .arg("--manifest-path")
        .arg(root.join("dist/Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("dist/target"))
        .output()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    ensure(
        !output.status.success(),
        format!("the blocked test must fail:\n{text}"),
    )?;
    ensure(
        text.contains("1 passed; 1 failed"),
        format!("only one test is blocked:\n{text}"),
    )?;
    ensure(
        text.contains("`String.prototype.anchor` is not modelled"),
        format!("the failure names the model:\n{text}"),
    )
}

/// The generated struct of a mapped utility holds only the properties the
/// utility leaves, for a literal key set and for one a generic call fixes.
#[test]
fn utility_records_emit_only_their_fields() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("src/main.ts"),
        r#"interface Base { keep: string; gone: number }
function pick<T, K extends keyof T>(obj: T, keys: K[]): Pick<T, K> {
  const out = {} as Pick<T, K>;
  for (const key of keys) { out[key] = obj[key]; }
  return out;
}
const literal: Omit<Base, "gone"> = { keep: "a" };
const called = pick({ keep: "b", gone: 1 } as Base, ["gone"]);
console.log(literal.keep, called.gone);
"#,
    )?;
    fs::write(
        root.join("Smelt.toml"),
        unmodelled_method_manifest("main.ts"),
    )?;
    let (ok, stdout, stderr) = run(root, &["build"])?;
    ensure(
        ok,
        format!("build failed\nstdout:\n{stdout}\nstderr:\n{stderr}"),
    )?;
    let mut emitted = String::new();
    for entry in fs::read_dir(root.join("dist/src"))? {
        emitted.push_str(&fs::read_to_string(entry?.path())?);
    }
    let body = |name: &str| -> Option<String> {
        let start = emitted.find(&format!("struct {name} {{"))?;
        let rest = emitted.get(start..)?;
        rest.get(..rest.find('}')?).map(str::to_owned)
    };
    let omitted = body("OmitBase").ok_or_else(|| format!("no OmitBase struct:\n{emitted}"))?;
    ensure(
        omitted.contains("keep: JsString") && !omitted.contains("gone"),
        format!("OmitBase: {omitted}"),
    )?;
    let picked = body("PickBase").ok_or_else(|| format!("no PickBase struct:\n{emitted}"))?;
    ensure(
        picked.contains("gone: f64") && !picked.contains("keep"),
        format!("PickBase: {picked}"),
    )
}

/// Runs the built `dist` crate under `root` and returns its stdout, whatever
/// its exit status, so a test can compare what it printed before a failure.
fn dist_stdout(root: &Path) -> TestResult<String> {
    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(root.join("dist/Cargo.toml"))
        .env("CARGO_TARGET_DIR", root.join("dist/target"))
        .output()?;
    Ok(String::from_utf8(output.stdout)?)
}

/// Deliberate divergence from JavaScript, which only misbehaves at the read: an
/// erased value that does not have a record's shape throws a catchable
/// `TypeError` at the conversion, naming the expected type and the path. Node
/// would print `undefined` here, so this is a Smelt-side test and not a parity
/// fixture.
#[test]
fn a_shape_mismatch_throws_at_the_record_conversion() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("src/main.ts"),
        r#"interface Data { count: number; name: string }
function raw(value: unknown): any { return value; }
function report(label: string, action: () => void): void {
  try { action(); console.log(label, "no throw"); }
  catch (error) { console.log(label, error instanceof TypeError, (error as Error).message); }
}
report("cast", () => { const d = raw({ name: "x" }) as Data; console.log(d.name); });
report("number", () => { const d = raw(5) as Data; console.log(d.name); });
report("list", () => { const d = raw([{ name: "x" }]) as Data[]; console.log(d.length); });
try { const bound: Data = raw({ name: "y" }); console.log(bound.name); }
catch (error) { console.log("let", (error as Error).message); }
function take(d: Data): number { return d.count; }
try { console.log(take(raw({ name: "z" }))); }
catch (error) { console.log("arg", (error as Error).message); }
interface Leaf { count: number }
interface Inner { items: Leaf[]; leaf: Leaf }
interface Outer { inner: Inner }
report("nested-list", () => { const o = raw({ inner: { items: [{ count: 1 }, {}], leaf: { count: 1 } } }) as Outer; console.log(o.inner); });
report("nested-field", () => { const o = raw({ inner: { items: [], leaf: {} } }) as Outer; console.log(o.inner); });
report("nested-kind", () => { const o = raw({ inner: { items: [], leaf: 3 } }) as Outer; console.log(o.inner); });
report("null", () => { const d = raw(null) as Data; console.log(d.name); });
report("not-a-function", () => { const c = raw({ tag: 1 }) as { (): number; tag: number }; console.log(c()); });
"#,
    )?;
    fs::write(
        root.join("Smelt.toml"),
        unmodelled_method_manifest("main.ts"),
    )?;
    let (ok, stdout, stderr) = run(root, &["build"])?;
    ensure(
        ok,
        format!("build must succeed\nstdout:\n{stdout}\nstderr:\n{stderr}"),
    )?;
    let printed = dist_stdout(root)?;
    ensure_eq(
        &printed,
        &"cast true expected number, got undefined at Data.count\n\
         number true expected Data, got number\n\
         list true expected number, got undefined at Data[0].count\n\
         let expected number, got undefined at Data.count\n\
         arg expected number, got undefined at Data.count\n\
         nested-list true expected number, got undefined at Outer.inner.items[1].count\n\
         nested-field true expected number, got undefined at Outer.inner.leaf.count\n\
         nested-kind true expected Leaf, got number at Outer.inner.leaf\n\
         null true expected Data, got null\n\
         not-a-function true value is not a function\n"
            .to_owned(),
        "conversion throws",
    )
}

/// Type half of the presence rule: a parameter whose body tests it for presence
/// is `Option` of its concrete record, and one that is never tested stays the
/// plain record, so the rule cannot pass by erasing or by widening everything.
#[test]
fn a_presence_tested_parameter_is_an_option_of_its_record() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::write(
        root.join("src/main.ts"),
        r"interface Options { times?: number }
function tested(options: Options): number { return options?.times ?? 3; }
function plain(options: Options): number { return options.times ?? 3; }
console.log(tested({}), plain({}));
",
    )?;
    fs::write(
        root.join("Smelt.toml"),
        unmodelled_method_manifest("main.ts"),
    )?;
    let (ok, stdout, stderr) = run(root, &["build"])?;
    ensure(
        ok,
        format!("build failed\nstdout:\n{stdout}\nstderr:\n{stderr}"),
    )?;
    let mut emitted = String::new();
    for entry in fs::read_dir(root.join("dist/src"))? {
        emitted.push_str(&fs::read_to_string(entry?.path())?);
    }
    ensure(
        emitted.contains("fn tested(options: Option<Options>)"),
        format!("tested signature:\n{emitted}"),
    )?;
    ensure(
        emitted.contains("fn plain(options: Options)"),
        format!("plain signature:\n{emitted}"),
    )
}
