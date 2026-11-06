//! Valid Python whose generated Rust must print what CPython prints.
//!
//! Each `fixtures/python-parity/<name>.py` runs through a full `smelt build` and a
//! compile of the generated crate, and its stdout is compared with the committed
//! `<name>.stdout` and with `python3` itself. The Python examples contain no
//! lambda, so the byte oracle and example goldens cannot see what these pin.

mod common;

use std::fs;
use std::process::Command;

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, smelt_in, workspace_root,
};

/// Build fixture `name` and check its stdout against the golden and CPython.
fn verify_python_parity(name: &str) -> TestResult {
    let fixtures = workspace_root()?.join("crates/smelt-transpiler/tests/fixtures/python-parity");
    let script = fixtures.join(format!("{name}.py"));
    let expected = fs::read_to_string(fixtures.join(format!("{name}.stdout")))?;
    if let Ok(output) = Command::new("python3").arg("-I").arg(&script).output() {
        let python_stdout = String::from_utf8(output.stdout)?;
        ensure(
            python_stdout == expected,
            format!(
                "{name}: the golden stdout is not what `python3` prints for the same source\n--- python3\n{python_stdout}--- golden\n{expected}"
            ),
        )?;
    }
    let project = TempProject::new()?;
    let root = project.path();
    fs::create_dir_all(root.join("src"))?;
    fs::copy(&script, root.join("src/main.py"))?;
    fs::write(
        root.join("Smelt.toml"),
        r#"[project]
name = "python-parity"
version = "0.1.0"

[sources]
entries = ["src/main.py"]

[output]
target = "./dist"
crate-name = "python_parity"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    smelt_in(root, &["build"])?;
    let stdout = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &expected, format!("{name}: generated Rust stdout"))
}

/// A lambda body is an ordinary expression, and `/`, `//` and `%` are Python's.
#[test]
fn lambda_arithmetic() -> TestResult {
    verify_python_parity("lambda_arithmetic")
}

/// A raise from a `map`, `filter` or `sorted` callback reaches the enclosing `try`.
#[test]
fn try_around_raising_callback() -> TestResult {
    verify_python_parity("try_around_raising_callback")
}

/// `xs[i]` raises `IndexError`, `print` draws `repr`, and `x /= 2` makes a `float`.
#[test]
fn list_semantics() -> TestResult {
    verify_python_parity("list_semantics")
}
