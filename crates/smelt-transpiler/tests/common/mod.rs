//! Shared integration-test helpers for `smelt-transpiler`.

#![allow(
    dead_code,
    reason = "shared CLI test helpers are used by different test shards"
)]
// `redundant_pub_crate` and rustc's `unreachable_pub` are mutually unsatisfiable
// here: this module is `mod common;`-included into several integration-test
// binaries, so `pub(crate)` reads as redundant to clippy while plain `pub` reads
// as unreachable to rustc. `pub(crate)` is the more accurate of the two -- the
// test binary really is the crate these helpers belong to -- so the clippy side
// is the one suppressed.
#![expect(
    clippy::redundant_pub_crate,
    reason = "pub(crate) is correct for a helper module shared across test binaries; plain pub trips unreachable_pub instead"
)]

use std::{
    error::Error,
    fs, io,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

/// Result type used by integration tests.
pub(crate) type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// Returns the workspace root for the integration tests.
pub(crate) fn workspace_root() -> Result<&'static Path, io::Error> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "workspace root"))
}

/// Runs the `smelt` binary from the workspace root and returns stdout.
pub(crate) fn smelt(args: &[&str]) -> TestResult<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .current_dir(workspace_root()?)
        .args(args)
        .output()?;

    if !output.status.success() {
        return Err(io::Error::other(format!(
            "smelt failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

/// Temporary project directory used by integration tests.
pub(crate) struct TempProject {
    path: PathBuf,
}

impl TempProject {
    /// Creates a unique temporary project path.
    pub(crate) fn new() -> Result<Self, std::time::SystemTimeError> {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        Ok(Self {
            path: std::env::temp_dir()
                .join(format!("smelt-cli-test-{}-{nonce}", std::process::id())),
        })
    }

    /// Returns the temporary project path.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.path));
    }
}

/// Runs `cargo run --manifest-path` for a generated crate and returns stdout.
pub(crate) fn cargo_run_manifest(manifest: &Path) -> TestResult<String> {
    let output = Command::new("cargo")
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(manifest)
        .output()?;

    if !output.status.success() {
        return Err(io::Error::other(format!(
            "generated crate failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

/// Runs `cargo test --manifest-path` for a generated crate and returns stdout.
pub(crate) fn cargo_test_manifest(manifest: &Path) -> TestResult<String> {
    let output = Command::new("cargo")
        .arg("test")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(manifest)
        .output()?;

    if !output.status.success() {
        return Err(io::Error::other(format!(
            "generated crate tests failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }

    Ok(String::from_utf8(output.stdout)?)
}

/// Converts a path to UTF-8 for CLI arguments.
pub(crate) fn utf8_path(path: &Path) -> Result<String, io::Error> {
    path.to_str().map(ToOwned::to_owned).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("path is not valid UTF-8: {}", path.display()),
        )
    })
}

/// Returns the absolute path to an end-to-end example fixture.
pub(crate) fn example_dir(name: &str) -> TestResult<PathBuf> {
    Ok(workspace_root()?
        .join("examples/typescript/end-to-end")
        .join(name))
}

/// Fails the test when `condition` is false.
pub(crate) fn ensure(condition: bool, message: impl Into<String>) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

/// Fails the test when `actual` and `expected` differ.
pub(crate) fn ensure_eq<T>(actual: &T, expected: &T, message: impl Into<String>) -> TestResult
where
    T: PartialEq + std::fmt::Debug,
{
    if actual == expected {
        Ok(())
    } else {
        Err(io::Error::other(message.into()).into())
    }
}

/// Verifies the compiled output for a single end-to-end example fixture.
pub(crate) fn verify_end_to_end_example(name: &str) -> TestResult {
    let example = example_dir(name)?;
    let input = example.join("input.ts");
    let expected_mir = fs::read_to_string(example.join("expected.mir"))?;

    let workspace_root = workspace_root()?;
    let input_path = input.strip_prefix(workspace_root)?;
    let actual_mir = smelt(&["dump-mir", &utf8_path(input_path)?])?;
    ensure_eq(
        &actual_mir,
        &expected_mir,
        format!("MIR mismatch for {name}"),
    )?;

    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("src/main.ts"),
        fs::read_to_string(&input)?,
    )?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "example-app"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "example_app"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;

    let manifest = project_path.join("Smelt.toml");
    let manifest_arg = utf8_path(&manifest)?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;

    let expected_rs = fs::read_to_string(example.join("expected.rs"))?;
    let actual_rs = fs::read_to_string(project_path.join("dist/src/main.rs"))?;
    ensure_eq(
        &actual_rs,
        &expected_rs,
        format!("Rust mismatch for {name}"),
    )?;

    let expected_stdout = fs::read_to_string(example.join("expected.stdout"))?;
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(
        &actual_stdout,
        &expected_stdout,
        format!("runtime stdout mismatch for {name}"),
    )?;

    Ok(())
}
