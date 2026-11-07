//! `smelt new` and `smelt clean`.

mod common;

use std::{fs, path::Path, process::Command};

use common::{TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq};

/// Outcome of one run: success, stdout, stderr.
type RunOutcome = (bool, String, String);

/// Runs `smelt` in `dir`.
fn run(dir: &Path, args: &[&str]) -> TestResult<RunOutcome> {
    let output = Command::new(env!("CARGO_BIN_EXE_smelt")).current_dir(dir).args(args).output()?;
    Ok((output.status.success(), String::from_utf8(output.stdout)?, String::from_utf8(output.stderr)?))
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
    ensure_eq(&fs::read_to_string(root.join(".gitignore"))?, &"dist/\n.smelt-cache/\n".to_owned(), ".gitignore")?;
    let (built, _, build_errors) = run(&root, &["build"])?;
    ensure(built, build_errors)?;
    let greeting = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&greeting, &"Hello from hello-app!\n".to_owned(), "the scaffolded program's output")
}

/// `--python` scaffolds `src/main.py` instead, and that project runs too.
#[test]
fn new_python_scaffolds_a_python_project_that_runs() -> TestResult {
    let temp = TempProject::new()?;
    fs::create_dir_all(temp.path())?;
    let (created, _, failure) = run(temp.path(), &["new", "py_app", "--python"])?;
    ensure(created, failure)?;
    let root = temp.path().join("py_app");
    ensure(root.join("src/main.py").is_file() && !root.join("src/main.ts").exists(), "wrong entry")?;
    let (built, _, build_errors) = run(&root, &["build"])?;
    ensure(built, build_errors)?;
    let greeting = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&greeting, &"Hello from py_app!\n".to_owned(), "the scaffolded program's output")
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
    ensure(!temp.path().join("taken/Smelt.toml").exists(), "wrote into a non-empty directory")?;

    let (escaped, _, escape_error) = run(temp.path(), &["new", "../escape"])?;
    ensure(!escaped && escape_error.contains("invalid project name"), escape_error)?;

    fs::create_dir_all(temp.path().join("empty"))?;
    let (filled, _, fill_error) = run(temp.path(), &["new", "empty"])?;
    ensure(filled, fill_error)
}
