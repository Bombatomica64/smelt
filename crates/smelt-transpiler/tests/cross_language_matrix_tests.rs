//! The cross-language matrix: TypeScript importing Python and Python
//! importing TypeScript, checked end to end against Node and `CPython`.
//!
//! Each case under `fixtures/cross-language/<case>/` has three parts:
//!
//! * `smelt/`: a mixed program, `main.ts` or `main.py` plus the module it
//!   imports from the OTHER language;
//! * `oracle/`: the same program written in the entry's language alone (the
//!   imported module translated by hand), which Node or `python3` runs;
//! * `expected.stdout`: what that oracle prints.
//!
//! A case passes when (1) the oracle still prints `expected.stdout`, which
//! keeps the golden honest, and (2) `smelt build`, with the type gate at its
//! default (no `--allow-type-errors`), produces a crate whose run prints the
//! same. The rows cover each kind of thing a module exports (functions,
//! classes, constants, re-exports, generics), a throw crossing the seam and
//! async code crossing it, in both directions.

mod common;

use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use common::{
    PYTHON_ORACLE, SharedDepsCrate, TempProject, TestResult, cargo_run_shared, ensure_eq,
    node_runs_typescript, node_typescript_stdout, python_runs_corpus, smelt, utf8_path,
    workspace_root,
};
use smelt_specialize::prereq::missing_prerequisite;

/// Every file under `dir`, as `(path relative to dir, contents)`, sorted.
/// A package (`pkg/__init__.py`) or barrel directory keeps its layout.
fn read_files(dir: &Path) -> TestResult<Vec<(String, String)>> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        for entry in fs::read_dir(&current)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                // A bytecode cache a local oracle run left is not a source.
                if entry.file_name() != "__pycache__" {
                    pending.push(path);
                }
            } else {
                let relative = path.strip_prefix(dir)?.to_string_lossy().into_owned();
                files.push((relative, fs::read_to_string(&path)?));
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Writes `files` (relative paths) under `root`, creating directories.
fn write_files(root: &Path, files: &[(String, String)]) -> TestResult {
    for (name, text) in files {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
    }
    Ok(())
}

/// What `python3` prints running `main.py` among `files`.
///
/// `-E -s` keeps the environment and user site out; `-I` would also drop the
/// script's directory from `sys.path`, which the oracle's sibling module
/// needs. `-B` writes no `__pycache__` into the temp copy.
fn python_stdout(files: &[(String, String)]) -> TestResult<String> {
    let project = TempProject::new()?;
    let root = project.path().join("python-oracle");
    write_files(&root, files)?;
    let mut command = Command::new("python3");
    command.current_dir(&root).env_clear().args(["-E", "-s", "-B", "main.py"]);
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "python3 failed on the oracle\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Checks the oracle of `case` still prints `expected`; skips (with a note)
/// when the oracle's runtime is absent.
fn ensure_oracle_agrees(case: &str, oracle: &Path, entry: &str, expected: &str) -> TestResult {
    let files = read_files(oracle)?;
    let stdout = if entry == "main.ts" {
        if !node_runs_typescript() {
            return Ok(missing_prerequisite::<Box<dyn std::error::Error>>(
                case,
                common::NODE_ORACLE,
            )?);
        }
        let borrowed = files
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str()))
            .collect::<Vec<_>>();
        node_typescript_stdout(&borrowed, entry)?
    } else {
        if !python_runs_corpus() {
            return Ok(missing_prerequisite::<Box<dyn std::error::Error>>(case, PYTHON_ORACLE)?);
        }
        python_stdout(&files)?
    };
    ensure_eq(&stdout, &expected.to_owned(), format!("{case}: the oracle's stdout is not the golden"))
}

/// Runs matrix case `case`: the oracle check, then the Smelt build and run.
fn verify_case(case: &str) -> TestResult {
    let fixture = workspace_root()?
        .join("crates/smelt-transpiler/tests/fixtures/cross-language")
        .join(case);
    let expected = fs::read_to_string(fixture.join("expected.stdout"))?;
    let sources = read_files(&fixture.join("smelt"))?;
    let entry = if sources.iter().any(|(name, _)| name == "main.ts") {
        "main.ts"
    } else {
        "main.py"
    };
    ensure_oracle_agrees(case, &fixture.join("oracle"), entry, &expected)?;

    let project = TempProject::new()?;
    let root = project.path();
    write_files(&root.join("src"), &sources)?;
    let shared = SharedDepsCrate::claim(format!("xlang_{case}"))?;
    fs::write(
        root.join("Smelt.toml"),
        format!(
            r#"[project]
name = "cross-language-matrix"
version = "0.1.0"

[sources]
entries = ["src/{entry}"]

[output]
target = "./dist"
crate-name = "{crate_name}"
build = false
"#,
            crate_name = shared.name()
        ),
    )?;
    let manifest = utf8_path(&root.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest, "build"])?;
    let stdout = cargo_run_shared(&root.join("dist/Cargo.toml"), &shared)?;
    ensure_eq(&stdout, &expected, format!("{case}: generated Rust stdout"))
}

/// TypeScript calls Python functions: `int`/`float`/`str`/`list` parameters,
/// and a defaulted parameter the caller omits or passes.
#[test]
fn ts_imports_py_functions() -> TestResult {
    verify_case("ts_imports_py_functions")
}

/// TypeScript constructs a Python class, calls its methods and reads and
/// writes its fields, `camelCase` names included.
#[test]
fn ts_imports_py_classes() -> TestResult {
    verify_case("ts_imports_py_classes")
}

/// TypeScript reads Python module constants.
#[test]
fn ts_imports_py_constants() -> TestResult {
    verify_case("ts_imports_py_constants")
}

/// TypeScript imports names a Python package re-exports from its submodules.
#[test]
fn ts_imports_py_reexports() -> TestResult {
    verify_case("ts_imports_py_reexports")
}

/// TypeScript calls a generic Python function, which keeps its type parameter.
#[test]
fn ts_imports_py_generics() -> TestResult {
    verify_case("ts_imports_py_generics")
}

/// A Python `raise` is caught by a TypeScript `try`.
#[test]
fn ts_imports_py_errors() -> TestResult {
    verify_case("ts_imports_py_errors")
}

/// TypeScript awaits a Python coroutine.
#[test]
fn ts_imports_py_async() -> TestResult {
    verify_case("ts_imports_py_async")
}

/// Python calls TypeScript functions, `camelCase` names included.
#[test]
fn py_imports_ts_functions() -> TestResult {
    verify_case("py_imports_ts_functions")
}

/// Python constructs a TypeScript class, calls its methods and reads its
/// fields, `camelCase` names included.
#[test]
fn py_imports_ts_classes() -> TestResult {
    verify_case("py_imports_ts_classes")
}

/// Python reads TypeScript `export const`s: numbers, strings and an array.
#[test]
fn py_imports_ts_constants() -> TestResult {
    verify_case("py_imports_ts_constants")
}

/// Python imports names a TypeScript barrel re-exports.
#[test]
fn py_imports_ts_reexports() -> TestResult {
    verify_case("py_imports_ts_reexports")
}

/// Python calls a generic TypeScript function, which keeps its type parameter.
#[test]
fn py_imports_ts_generics() -> TestResult {
    verify_case("py_imports_ts_generics")
}

/// A TypeScript `throw` is caught by a Python `try`.
#[test]
fn py_imports_ts_errors() -> TestResult {
    verify_case("py_imports_ts_errors")
}

/// Python awaits a TypeScript async function.
#[test]
fn py_imports_ts_async() -> TestResult {
    verify_case("py_imports_ts_async")
}
