//! Python callbacks run the way `CPython` runs them.
//!
//! A `lambda` or a nested `def` returning one value lowers its body through
//! ordinary expression lowering, so an operator means the same thing inside a
//! callback as outside one. Each fixture runs under `CPython` and as generated
//! Rust, and the two outputs must match. The fixtures pin the cases where the
//! two used to disagree: true division of `int`s, `%` taking the divisor's sign,
//! and `int`/`float` promotion, all inside callbacks.

use std::{error::Error, fs, path::Path, process::Command};

use smelt_codegen_rust::{CrateKind, EmitOptions, emit_crate};
use smelt_frontend_py::{HirCtx, to_hir_with_path};
use smelt_hir::FileId;
use smelt_specialize::prereq::{locate_executable, missing_prerequisite};

/// Result type of the fixtures and their helpers.
type RuntimeResult<T = ()> = Result<T, Box<dyn Error>>;

/// Arithmetic inside `map` lambdas, a local lambda and a nested `def`.
const ARITHMETIC_SOURCE: &str = r"
from typing import Callable


def main() -> None:
    nums: list[int] = [3, -4, 7]
    offset: int = 10
    print(list(map(lambda x: x / 2, nums)))
    print(list(map(lambda x: x % 3, nums)))
    print(list(map(lambda x: x + 0.5, nums)))
    print(list(map(lambda x: x * 2 - offset, nums)))
    halve: Callable[[int], float] = lambda x: x / 2
    print(list(map(halve, nums)))
    print(halve(7))

    def shift(x: int) -> int:
        return (x + offset) % 4

    print(list(map(shift, nums)))


main()
";

/// Python `%` with every sign combination, on `int` and `float`, in and out of
/// callbacks.
const FLOOR_MOD_SOURCE: &str = r"
def main() -> None:
    nums: list[int] = [3, -4, 7]
    fl: list[float] = [5.5, -0.5]
    m: int = -3
    print(-7 % 3, 7 % -3, -7 % -3, 7 % 3)
    a: float = -7.5
    print(a % 2.0, a % -2.0)
    print(list(map(lambda x: (x + 1) % 3, nums)))
    print(list(map(lambda x: x % m, nums)))
    print(list(map(lambda x: x % 2.0, fl)))
    print(list(map(lambda x: x % 2, fl)))


main()
";

/// Subscripts, defaults and `*args` in callbacks.
const SHAPES_SOURCE: &str = r#"
from typing import Callable


def main() -> None:
    rows: list[dict[str, int]] = [{"k": 1}, {"k": 5}]
    lists: list[list[int]] = [[1, 2], [3, 4]]
    print(list(map(lambda r: r["k"] * 2, rows)))
    print(list(map(lambda xs: xs[1], lists)))
    print(list(map(lambda xs: xs[-1], lists)))
    add: Callable[[int, int], int] = lambda x, y=1: x + y
    print(add(1, 2))
    pack: Callable[[int, int], int] = lambda *args: len(args)
    print(pack(1, 2))
"#;

#[test]
fn arithmetic_in_callbacks_matches_cpython() -> RuntimeResult {
    assert_cpython_parity(ARITHMETIC_SOURCE, "arithmetic")
}

#[test]
fn floor_mod_matches_cpython() -> RuntimeResult {
    assert_cpython_parity(FLOOR_MOD_SOURCE, "floor_mod")
}

#[test]
fn callback_shapes_match_cpython() -> RuntimeResult {
    assert_cpython_parity(&format!("{SHAPES_SOURCE}\nmain()\n"), "shapes")
}

/// Run `source` under `CPython` and as generated Rust, and compare stdout.
fn assert_cpython_parity(source: &str, fixture: &str) -> RuntimeResult {
    let Some(python) = locate_executable("python3", &["/usr/bin/python3", "/usr/local/bin/python3"])
    else {
        return missing_prerequisite(fixture, "python3 interpreter");
    };
    let project = tempfile::tempdir()?;
    let source_path = project.path().join("fixture.py");
    fs::write(&source_path, source)?;
    let expected = stdout_of(Command::new(&python).arg(&source_path), "CPython fixture")?;

    let generated = project.path().join("generated");
    emit_program(source, &source_path, fixture, &generated)?;
    let actual = run_generated(&generated.join("Cargo.toml"), project.path())?;
    if actual != expected {
        let generated_source =
            fs::read_to_string(generated.join("src/main.rs")).unwrap_or_default();
        return Err(format!(
            "CPython and generated Rust output differ\nCPython:\n{expected}\nRust:\n{actual}\nGenerated:\n{generated_source}"
        )
        .into());
    }
    Ok(())
}

/// Lower and emit `source` on a thread with a large stack.
///
/// Lowering and emission recurse over the expression tree; a whole program is
/// deeper than the two megabytes a test thread gets, while the `smelt` binary
/// runs on a process main thread with eight.
fn emit_program(source: &str, path: &Path, fixture: &str, dir: &Path) -> RuntimeResult {
    let source_text = source.to_owned();
    let path_text = path.to_string_lossy().into_owned();
    let crate_name = format!("python_callback_{fixture}");
    let crate_dir = dir.to_path_buf();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || -> Result<(), String> {
            let mut ctx = HirCtx::new();
            to_hir_with_path(&source_text, FileId(0), &path_text, &mut ctx)
                .map_err(|errors| format!("HIR lowering failed: {errors:?}"))?;
            let mut mir = smelt_mir::lower_hir(&ctx.krate)
                .map_err(|errors| format!("MIR lowering: {errors:?}"))?;
            smelt_mir::opt::optimize(&mut mir);
            let options = EmitOptions::new(crate_name).with_crate_kind(CrateKind::Program);
            emit_crate(&mir, &crate_dir, &options).map_err(|error| error.to_string())
        })?
        .join()
        .map_err(|panic| format!("emitter thread panicked: {panic:?}"))??;
    Ok(())
}

/// Run the generated program with its own Cargo target directory.
fn run_generated(manifest: &Path, scratch: &Path) -> RuntimeResult<String> {
    stdout_of(
        Command::new("cargo")
            .args(["run", "--quiet", "--manifest-path"])
            .arg(manifest)
            .env("CARGO_TARGET_DIR", scratch.join("generated-target")),
        "generated Rust fixture",
    )
}

/// The stdout of a command that must succeed.
fn stdout_of(command: &mut Command, label: &str) -> RuntimeResult<String> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "{label} failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}
