//! `smelt build` lowers only programs the type checkers accept (the type gate).
//!
//! An ill-typed TypeScript or Python program fails the build before anything is
//! written; `--allow-type-errors` or `[build] type-errors = "allow"` lowers it
//! anyway, printing the errors as warnings, and the frontend's checked-program
//! shortcuts then refuse what they would otherwise miscompile.

mod common;

use std::{fs, path::Path, process::Command};

use common::{TempProject, TestResult, ensure};

/// Writes a one-entry project whose entry is `entry` holding `source`, with
/// `extra` appended to the manifest.
fn project(dir: &Path, entry: &str, source: &str, extra: &str) -> TestResult {
    fs::create_dir_all(dir.join("src"))?;
    fs::write(dir.join("src").join(entry), source)?;
    fs::write(
        dir.join("Smelt.toml"),
        format!(
            "[project]\nname = \"gate\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/{entry}\"]\n\n[output]\ntarget = \"./dist\"\ncrate-name = \"gate\"\nbuild = false\n\n[runtime]\nclone-strategy = \"aggressive\"\n{extra}"
        ),
    )?;
    Ok(())
}

/// Outcome of one run: success and stderr.
type RunOutcome = (bool, String);

/// Runs `smelt` in `dir` with the checker on.
fn run(dir: &Path, args: &[&str]) -> TestResult<RunOutcome> {
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .current_dir(dir)
        .args(args)
        .env_remove("SMELT_TS7")
        .output()?;
    Ok((output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned()))
}

/// The refusal every gated failure carries.
const REFUSAL: &str = "Smelt lowers only programs that type-check";

/// A TypeScript program `tsc` rejects (TS2322 on line 2) that Smelt can lower.
const ILL_TYPED_TS: &str = "function f(x: string): void {\n  const n: number = x;\n  console.log(n);\n}\nf(\"a\");\n";

/// A TypeScript checker error fails `build` before the output exists.
#[cfg(feature = "ts7")]
#[test]
fn build_refuses_ill_typed_typescript() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.ts", ILL_TYPED_TS, "")?;
    let (ok, stderr) = run(temp.path(), &["build"])?;
    ensure(!ok, "an ill-typed program built")?;
    ensure(stderr.contains("error[type-check]") && stderr.contains("TS2322"), stderr.clone())?;
    ensure(stderr.contains(REFUSAL), stderr)?;
    ensure(!temp.path().join("dist").exists(), "a refused build wrote its output")
}

/// `--allow-type-errors` lowers it, with the errors as warnings; so does the
/// manifest setting.
#[cfg(feature = "ts7")]
#[test]
fn allowing_type_errors_builds_ill_typed_typescript() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.ts", ILL_TYPED_TS, "")?;
    let (ok, stderr) = run(temp.path(), &["build", "--allow-type-errors"])?;
    ensure(ok, stderr.clone())?;
    ensure(stderr.contains("warning[type-check]") && stderr.contains("TS2322"), stderr)?;
    ensure(temp.path().join("dist/src/main.rs").is_file(), "the allowed build wrote no crate")?;

    let manifest = TempProject::new()?;
    project(manifest.path(), "main.ts", ILL_TYPED_TS, "\n[build]\ntype-errors = \"allow\"\n")?;
    let (manifest_ok, manifest_stderr) = run(manifest.path(), &["build"])?;
    ensure(manifest_ok, manifest_stderr)
}

/// A well-typed program builds, and the checker says nothing.
#[cfg(feature = "ts7")]
#[test]
fn a_well_typed_program_builds() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.ts", "const n: number = 2;\nconsole.log(n);\n", "")?;
    let (ok, stderr) = run(temp.path(), &["build"])?;
    ensure(ok, stderr.clone())?;
    ensure(!stderr.contains("type-check"), stderr)
}

/// With type errors allowed, a shortcut that relies on a checked program
/// refuses the source instead of miscompiling it: two implementations of one
/// function (TS2393), where hoisting the first would make the early call run
/// it while JavaScript runs the last.
#[cfg(feature = "ts7")]
#[test]
fn an_allowed_ill_typed_program_never_miscompiles_silently() -> TestResult {
    let temp = TempProject::new()?;
    let source = "console.log(f());\nfunction f(): number { return 1; }\nfunction f(): number { return 2; }\n";
    project(temp.path(), "main.ts", source, "")?;
    let (ok, stderr) = run(temp.path(), &["build", "--allow-type-errors"])?;
    ensure(!ok, "a duplicate implementation lowered")?;
    ensure(stderr.contains("duplicate function implementation `f`"), stderr)
}

/// A Python program ty rejects (an `int` parameter given a `str`).
const ILL_TYPED_PY: &str = "def twice(x: int) -> int:\n    return x * 2\n\n\nprint(twice(\"3\"))\n";

/// A ty error fails `build` before the output exists.
#[cfg(feature = "ty")]
#[test]
fn build_refuses_ill_typed_python() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.py", ILL_TYPED_PY, "")?;
    let (ok, stderr) = run(temp.path(), &["build"])?;
    ensure(!ok, "an ill-typed Python program built")?;
    ensure(stderr.contains("error[type-check]") && stderr.contains("invalid-argument-type"), stderr.clone())?;
    ensure(stderr.contains("src/main.py:5:"), stderr.clone())?;
    ensure(stderr.contains(REFUSAL), stderr)?;
    ensure(!temp.path().join("dist").exists(), "a refused build wrote its output")
}

/// With type errors allowed, the ty error is a warning and the gate does not
/// refuse the Python program.
#[cfg(feature = "ty")]
#[test]
fn allowing_type_errors_passes_the_gate_for_python() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.py", ILL_TYPED_PY, "")?;
    let (_, stderr) = run(temp.path(), &["build", "--allow-type-errors"])?;
    ensure(stderr.contains("warning[type-check]") && stderr.contains("invalid-argument-type"), stderr.clone())?;
    ensure(!stderr.contains(REFUSAL), stderr)
}

/// A well-typed Python program builds, and ty says nothing.
#[cfg(feature = "ty")]
#[test]
fn a_well_typed_python_program_builds() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), "main.py", "def twice(x: int) -> int:\n    return x * 2\n\n\nprint(twice(3))\n", "")?;
    let (ok, stderr) = run(temp.path(), &["build"])?;
    ensure(ok, stderr.clone())?;
    ensure(!stderr.contains("type-check"), stderr)
}
