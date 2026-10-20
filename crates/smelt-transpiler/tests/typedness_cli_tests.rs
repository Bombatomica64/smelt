//! CLI tests for `smelt typedness-report`: the classifier on tiny programs and
//! the no-less-typed ratchet.
//!
//! Design adapted from Bun's `--aot` typedness tests (oven-sh/bun#45631),
//! `test/aot/typedness/run-typedness.ts`.

mod common;

use std::{fs, path::Path, process::Command};

use common::{TempProject, TestResult, ensure};

/// Outcome of one run: success, stdout, stderr.
type RunOutcome = (bool, String, String);

/// Run smelt in `dir`, returning (success, stdout, stderr).
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

/// Measure one program; return the JSON record of `prog.ts`.
fn measure(source: &str) -> TestResult<serde_json::Value> {
    let project = TempProject::new()?;
    fs::create_dir_all(project.path())?;
    fs::write(project.path().join("prog.ts"), source)?;
    let (ok, stdout, stderr) = run(project.path(), &["typedness-report", "prog.ts", "--format", "json"])?;
    ensure(ok, stderr)?;
    let report: serde_json::Value = serde_json::from_str(&stdout)?;
    Ok(report["programs"]["prog.ts"].clone())
}

/// The count of `class` (`concrete`/`generic`/`erased`) for `kind`.
fn count(record: &serde_json::Value, kind: &str, class: &str) -> u64 {
    record[kind][class].as_u64().unwrap_or(u64::MAX)
}

/// A fully annotated program has no erased value, read or call.
#[test]
fn concrete_program_is_fully_typed() -> TestResult {
    let record = measure(
        "class Point {\n  x: number;\n  constructor(x: number) { this.x = x; }\n}\nfunction double(p: Point): number {\n  return p.x * 2;\n}\nconsole.log(double(new Point(2)));\n",
    )?;
    for kind in ["values", "reads", "calls"] {
        ensure(count(&record, kind, "erased") == 0, format!("{kind}: {record}"))?;
    }
    ensure(count(&record, "values", "concrete") > 0, record.to_string())?;
    ensure(count(&record, "reads", "concrete") > 0, record.to_string())
}

/// A free generic function carried as a Rust generic counts as generic.
#[test]
fn generic_function_values_are_generic() -> TestResult {
    let record = measure(
        "function wrap<T>(value: T): T[] {\n  return [value];\n}\nconsole.log(wrap(1).length);\n",
    )?;
    ensure(count(&record, "values", "generic") > 0, record.to_string())?;
    ensure(count(&record, "values", "erased") == 0, record.to_string())
}

/// `unknown` and `any` values are erased, and so is a read through them.
#[test]
fn unknown_values_are_erased() -> TestResult {
    let record = measure(
        "function first(xs: any[]): unknown {\n  return xs[0];\n}\nconsole.log(first([1]));\n",
    )?;
    ensure(count(&record, "values", "erased") > 0, record.to_string())?;
    ensure(count(&record, "reads", "erased") == 1, record.to_string())
}

/// A program that becomes less typed fails `--fail-on-regression` naming the
/// program and kind; `--update-baseline` keeps its old record; a more typed
/// program passes and is re-snapshotted.
#[test]
fn ratchet_rejects_less_typed_programs() -> TestResult {
    let project = TempProject::new()?;
    let dir = project.path();
    fs::create_dir_all(dir)?;
    let typed = "function f(x: number): number {\n  return x + 1;\n}\nconsole.log(f(1));\n";
    let erased = "function f(x: unknown): unknown {\n  return x;\n}\nconsole.log(f(1));\n";
    fs::write(dir.join("prog.ts"), typed)?;
    let (ok, _, stderr) = run(dir, &["typedness-report", "prog.ts", "--baseline", "base.json", "--update-baseline"])?;
    ensure(ok, stderr)?;
    let snapshot = fs::read_to_string(dir.join("base.json"))?;

    fs::write(dir.join("prog.ts"), erased)?;
    let (ok, _, stderr) =
        run(dir, &["typedness-report", "prog.ts", "--baseline", "base.json", "--fail-on-regression"])?;
    ensure(!ok, "a less typed program passed the ratchet")?;
    ensure(stderr.contains("prog.ts: values: erased 0 -> "), stderr.clone())?;
    // Without --fail-on-regression the report is advisory.
    let (ok, stdout, _) = run(dir, &["typedness-report", "prog.ts", "--baseline", "base.json"])?;
    ensure(ok && stdout.contains("Less typed than the baseline"), stdout)?;
    // The update refuses to snapshot the regression.
    let (ok, _, stderr) = run(dir, &["typedness-report", "prog.ts", "--baseline", "base.json", "--update-baseline"])?;
    ensure(ok, stderr)?;
    ensure(fs::read_to_string(dir.join("base.json"))? == snapshot, "regression was snapshotted")?;

    fs::write(dir.join("prog.ts"), typed)?;
    let (ok, _, stderr) =
        run(dir, &["typedness-report", "prog.ts", "--baseline", "base.json", "--fail-on-regression"])?;
    ensure(ok, stderr)
}

/// `smelt build` writes a sidecar the report reads from the output directory.
#[test]
fn build_writes_a_typedness_sidecar() -> TestResult {
    let project = TempProject::new()?;
    let dir = project.path();
    fs::create_dir_all(dir)?;
    fs::write(dir.join("main.ts"), "const n: number = 2;\nconsole.log(n * 3);\n")?;
    fs::write(
        dir.join("Smelt.toml"),
        "[project]\nname = \"typedness-cli\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"main.ts\"]\n\n[output]\ntarget = \"out\"\n\n[runtime]\nclone-strategy = \"always\"\n",
    )?;
    let (ok, _, stderr) = run(dir, &["build"])?;
    ensure(ok, stderr)?;
    let (ok, stdout, stderr) = run(dir, &["typedness-report", "out/src", "--format", "json"])?;
    ensure(ok, stderr)?;
    let report: serde_json::Value = serde_json::from_str(&stdout)?;
    let record = &report["programs"]["main.ts"];
    ensure(count(record, "values", "erased") == 0, stdout.clone())?;
    ensure(count(record, "values", "concrete") > 0, stdout)
}
