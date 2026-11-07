//! `smelt check` reports what the embedded TypeScript checker says.
#![cfg(feature = "ts7")]

mod common;

use std::{fs, path::Path, process::Command};

use common::{TempProject, TestResult, ensure};

/// A valid program.
const VALID: &str = "const n: number = 2;\nconsole.log(n);\n";

/// A program `tsc` rejects on line 2, which Smelt can still lower.
const REJECTED: &str = "function f(x: string): void {\n  const n: number = x;\n  console.log(n);\n}\nf(\"a\");\n";

/// Writes a one-entry project.
fn project(dir: &Path, source: &str) -> TestResult {
    fs::create_dir_all(dir.join("src"))?;
    fs::write(dir.join("src/main.ts"), source)?;
    fs::write(
        dir.join("Smelt.toml"),
        "[project]\nname = \"check-cli\"\nversion = \"0.1.0\"\n\n[sources]\nentries = [\"src/main.ts\"]\n\n[output]\ntarget = \"./dist\"\ncrate-name = \"check_cli\"\nbuild = false\n\n[runtime]\nclone-strategy = \"aggressive\"\n",
    )?;
    Ok(())
}

/// Outcome of one run: success, stdout, stderr.
type RunOutcome = (bool, String, String);

/// Runs `smelt` in `dir` with `SMELT_TS7` set to `ts7` when given.
fn run(dir: &Path, ts7: Option<&str>, args: &[&str]) -> TestResult<RunOutcome> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_smelt"));
    command.current_dir(dir).args(args).env_remove("SMELT_TS7");
    if let Some(value) = ts7 {
        command.env("SMELT_TS7", value);
    }
    let output = command.output()?;
    Ok((
        output.status.success(),
        String::from_utf8(output.stdout)?,
        String::from_utf8(output.stderr)?,
    ))
}

/// A checker error is reported with its code and place, and warns by default.
#[test]
fn check_reports_a_type_error_and_passes_by_default() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), REJECTED)?;
    let (ok, _, stderr) = run(temp.path(), None, &["check"])?;
    ensure(ok, stderr.clone())?;
    ensure(stderr.contains("warning[ts-check]"), stderr.clone())?;
    ensure(stderr.contains("src/main.ts:2:"), stderr.clone())?;
    ensure(stderr.contains("TS2322"), stderr)
}

/// `--deny-type-errors` fails on the same program, in both output formats.
#[test]
fn deny_type_errors_fails_the_check() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), REJECTED)?;
    let (ok, _, stderr) = run(temp.path(), None, &["check", "--deny-type-errors"])?;
    ensure(!ok, "a checker error passed --deny-type-errors")?;
    ensure(stderr.contains("error[ts-check]"), stderr.clone())?;
    ensure(stderr.contains("1 error(s)"), stderr)?;

    let (passed, stdout, _) =
        run(temp.path(), None, &["check", "--message-format", "json", "--deny-type-errors"])?;
    ensure(!passed, "a checker error passed --deny-type-errors with JSON output")?;
    let rows: serde_json::Value = serde_json::from_str(&stdout)?;
    let row = rows.get(0).ok_or("no diagnostic row")?;
    ensure(row["category"] == "type-check" && row["severity"] == "error", stdout.clone())?;
    ensure(row["line"] == 2, stdout)
}

/// A valid program reports nothing and passes the strict form.
#[test]
fn a_valid_program_is_clean() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), VALID)?;
    let (ok, _, stderr) = run(temp.path(), None, &["check", "--deny-type-errors"])?;
    ensure(ok, stderr.clone())?;
    ensure(!stderr.contains("ts-check"), stderr)
}

/// `SMELT_TS7=0` runs without the checker, so it has nothing to report.
#[test]
fn smelt_ts7_zero_turns_the_checker_off() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), REJECTED)?;
    let (ok, _, stderr) = run(temp.path(), Some("0"), &["check", "--deny-type-errors"])?;
    ensure(ok, stderr.clone())?;
    ensure(!stderr.contains("ts-check"), stderr)
}

/// `build` does not report or stop on a checker error.
#[test]
fn build_is_not_judged_by_the_checker() -> TestResult {
    let temp = TempProject::new()?;
    project(temp.path(), REJECTED)?;
    let (ok, _, stderr) = run(temp.path(), None, &["build"])?;
    ensure(ok, stderr.clone())?;
    ensure(!stderr.contains("ts-check"), stderr)
}
