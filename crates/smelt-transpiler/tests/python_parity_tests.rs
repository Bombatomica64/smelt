//! Valid Python whose generated Rust must print what `CPython` prints.
//!
//! Each `fixtures/python-parity/<name>.py` runs through a full `smelt build` and a
//! compile of the generated crate, and its stdout is compared with the committed
//! `<name>.stdout` and with `python3` itself. The Python examples contain no
//! lambda, so the byte oracle and example goldens cannot see what these pin.

mod common;

use std::fs;
use std::process::Command;

use common::{
    PYTHON_ORACLE, TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq,
    python_runs_corpus, smelt_in, workspace_root,
};
use smelt_specialize::prereq::missing_prerequisite;

/// Build fixture `name` and check its stdout against the golden and `CPython`.
fn verify_python_parity(name: &str) -> TestResult {
    let fixtures = workspace_root()?.join("crates/smelt-transpiler/tests/fixtures/python-parity");
    let script = fixtures.join(format!("{name}.py"));
    let expected = fs::read_to_string(fixtures.join(format!("{name}.stdout")))?;
    if python_runs_corpus() {
        let output = Command::new("python3").arg("-I").arg(&script).output()?;
        let python_stdout = String::from_utf8(output.stdout)?;
        ensure(
            python_stdout == expected,
            format!(
                "{name}: the golden stdout is not what `python3` prints for the same source\n--- python3\n{python_stdout}--- golden\n{expected}"
            ),
        )?;
    } else {
        missing_prerequisite::<Box<dyn std::error::Error>>(name, PYTHON_ORACLE)?;
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

/// A `str` index counts code points, in `len`, `find`, `rfind` and slicing.
#[test]
fn str_code_point_indices() -> TestResult {
    verify_python_parity("str_code_point_indices")
}

/// A class with `__iter__`/`__next__` drives `for` until `StopIteration`.
#[test]
fn iterator_protocol() -> TestResult {
    verify_python_parity("iterator_protocol")
}

/// A `def` or parameter spelled like a builtin shadows it.
#[test]
fn builtin_names_shadowed() -> TestResult {
    verify_python_parity("builtin_names_shadowed")
}

/// A user method spelled like a builtin collection method is the user's.
#[test]
fn builtin_method_names_on_classes() -> TestResult {
    verify_python_parity("builtin_method_names_on_classes")
}

/// `print` of a container writes each element's Python `repr`.
#[test]
fn print_container_repr() -> TestResult {
    verify_python_parity("print_container_repr")
}

/// A user `open` is the user's function, not the builtin.
#[test]
fn user_open_shadows_builtin() -> TestResult {
    verify_python_parity("user_open_shadows_builtin")
}

/// A dict iterates in insertion order through every view of it, and a key
/// deleted and inserted again moves to the end.
#[test]
fn dict_insertion_order() -> TestResult {
    verify_python_parity("dict_insertion_order")
}

/// User and builtin exception classes: `raise`, `except` by class, tuple and
/// base class, bare `except` and `raise`, `else`, `finally`, `e.args`, and an
/// iterator whose `__next__` raises something other than `StopIteration`.
#[test]
fn exception_classes() -> TestResult {
    verify_python_parity("exception_classes")
}

/// A dict is a reference value: an alias, a list element and a parameter all
/// reach the same container; `dict(d)` copies it.
#[test]
fn dict_aliasing() -> TestResult {
    verify_python_parity("dict_aliasing")
}

/// `import asyncio` with `async def`, `await`, `asyncio.run`, `asyncio.gather`
/// and `asyncio.sleep` builds, and tasks resume in the event loop's order.
#[test]
fn asyncio_programs() -> TestResult {
    verify_python_parity("asyncio_programs")
}

/// `str()`, `repr()` and f-strings write Python text for containers,
/// exceptions and primitives.
#[test]
fn python_text() -> TestResult {
    verify_python_parity("python_text")
}

/// Negative indexes, clamped and stepped slices, and the `IndexError` and
/// `ValueError` `CPython` raises.
#[test]
fn sequence_indexing() -> TestResult {
    verify_python_parity("sequence_indexing")
}

/// `x /= n` rebinds an `int` name to a `float` in straight-line code.
#[test]
fn int_rebinds_to_float() -> TestResult {
    verify_python_parity("int_rebinds_to_float")
}

/// Destructuring `for` targets, tuple unpacking, `dict.pop` and `popitem`.
#[test]
fn dict_destructuring_and_pop() -> TestResult {
    verify_python_parity("dict_destructuring_and_pop")
}
