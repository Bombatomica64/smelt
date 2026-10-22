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

/// Runs the `smelt` binary from `dir` and returns stdout.
///
/// The companion of [`smelt`], which always runs from the workspace root. A
/// manifest path is resolved against the process's working directory, and
/// everything INSIDE the manifest is resolved against the manifest's own
/// directory — so a test that wants to prove the two are not confused has to
/// choose the working directory itself.
pub(crate) fn smelt_in(dir: &Path, args: &[&str]) -> TestResult<String> {
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .current_dir(dir)
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
    /// Remove the temporary project directory, ignoring errors.
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

/// The first Node.js release that runs TypeScript with
/// `--experimental-transform-types` (type stripping plus the transforms for
/// enums, namespaces and parameter properties).
const NODE_TRANSFORM_TYPES_MIN: (u32, u32) = (22, 7);

/// Whether a `node` able to run TypeScript is on `PATH`, probed once per test
/// binary.
///
/// The real-runtime oracle is skip-if-absent, like every other test here that
/// needs an external tool: a missing `node`, or one older than
/// [`NODE_TRANSFORM_TYPES_MIN`], skips the oracle half of a check with one
/// `eprintln` note rather than failing it. CI installs a pinned Node (see the
/// `actions/setup-node` steps in `.github/workflows/ci.yml`), so there the
/// oracle always runs.
#[expect(clippy::print_stderr, reason = "a skipped oracle check must say so in the test output")]
pub(crate) fn node_runs_typescript() -> bool {
    static PROBE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PROBE.get_or_init(|| {
        let Ok(output) = Command::new("node").arg("--version").output() else {
            eprintln!("note: `node` is not on PATH; skipping the Node oracle checks");
            return false;
        };
        let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let mut parts = version
            .trim_start_matches('v')
            .split('.')
            .map(|part| part.parse::<u32>().unwrap_or(0));
        let found = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
        if !output.status.success() || found < NODE_TRANSFORM_TYPES_MIN {
            eprintln!(
                "note: `node` {version} cannot run --experimental-transform-types (needs >= {}.{}); skipping the Node oracle checks",
                NODE_TRANSFORM_TYPES_MIN.0, NODE_TRANSFORM_TYPES_MIN.1
            );
            return false;
        }
        true
    })
}

/// Runs a TypeScript program under real Node and returns its stdout.
///
/// `files` are `(relative path, source)` pairs written into a fresh directory;
/// `entry` is the one Node runs, as
/// `node --experimental-transform-types --no-warnings <entry>`.
///
/// # The environment is the target profile's, not the host's
///
/// Smelt compiles for a fixed Node profile whose `TZ` is fixed at COMPILE time
/// (see `smelt_stdlib::NODE_PROFILE_TIMEZONE`); every other `process.env`
/// variable is read at run time. The oracle runs Node under a cleared
/// environment with only `TZ` set, so a golden cannot depend on the machine or
/// CI runner it was recorded on.
/// Only `PATH` is carried over, so `node` itself and anything it spawns
/// resolve.
pub(crate) fn node_typescript_stdout(files: &[(&str, &str)], entry: &str) -> TestResult<String> {
    let project = TempProject::new()?;
    let root = project.path().join("node-oracle");
    for (relative, source) in files {
        let path = root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, source)?;
    }
    let mut command = Command::new("node");
    command
        .current_dir(&root)
        .env_clear()
        .env("TZ", smelt_stdlib::NODE_PROFILE_TIMEZONE)
        .arg("--experimental-transform-types")
        .arg("--no-warnings")
        .arg(entry);
    if let Some(path) = std::env::var_os("PATH") {
        command.env("PATH", path);
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "node failed on {entry}\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    Ok(String::from_utf8(output.stdout)?)
}

/// Asserts a golden stdout is what real Node prints for the same program.
///
/// Design adapted from Bun's `--aot` (oven-sh/bun#45631), which debugs compiled
/// output by comparing it with the same program run by the runtime
/// (`docs/bundler/aot-debugging.mdx`); here the runtime is Node itself.
///
/// The generated program is compared against the golden elsewhere; this is the
/// other half that keeps the golden honest. Without it a golden recorded from
/// Smelt's own output (`[1.0, 2.0, 3.0]` for `console.log([1, 2, 3])`) passed
/// for as long as Smelt kept printing it. Skips, with a note, when no
/// TypeScript-capable `node` is available.
pub(crate) fn ensure_node_agrees(
    files: &[(&str, &str)],
    entry: &str,
    expected_stdout: &str,
    name: &str,
) -> TestResult {
    if !node_runs_typescript() {
        return Ok(());
    }
    let node_stdout = node_typescript_stdout(files, entry)?;
    ensure(
        node_stdout == expected_stdout,
        format!(
            "{name}: the golden stdout is not what `node` prints for the same source\n--- node\n{node_stdout}--- golden\n{expected_stdout}"
        ),
    )
}

/// Runs the `smelt` binary from the workspace root with extra environment.
pub(crate) fn smelt_with_env(args: &[&str], env: &[(&str, &Path)]) -> TestResult<String> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_smelt"));
    command.current_dir(workspace_root()?).args(args);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output()?;
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

/// Runs `cargo run --manifest-path` with `env` set (and every name in `unset`
/// removed) for the generated program, and returns stdout.
pub(crate) fn cargo_run_manifest_with_env(
    manifest: &Path,
    env: &[(&str, &str)],
    unset: &[&str],
) -> TestResult<String> {
    let mut command = Command::new("cargo");
    command.arg("run").arg("--quiet").arg("--manifest-path").arg(manifest);
    for (key, value) in env {
        command.env(key, value);
    }
    for key in unset {
        command.env_remove(key);
    }
    let output = command.output()?;
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

/// Verifies the HIR and MIR dumps of one example against its golden files.
///
/// Split out of the end-to-end verifier so the Python corpus, which has no
/// generated-Rust or runtime tier yet, can reuse the same golden comparison.
///
/// The HIR golden exists because the `smelt-hir` formatter was otherwise almost
/// unexercised: `dump-hir` ran in exactly two CLI tests, one of them over a
/// one-line file, which left `format/call.rs` at 1.9% line coverage and
/// `format/map.rs` at 0% while `ExprKind` has many variants. The formatter is
/// what `skills/smelt-debug-workflow` and every human or agent debugging a
/// lowering issue reads, so a `todo!()` or a silently wrong arm in it is
/// expensive and was invisible.
pub(crate) fn verify_example_dumps(name: &str, example: &Path, input: &Path) -> TestResult {
    let workspace_root = workspace_root()?;
    let input_path = input.strip_prefix(workspace_root)?;
    let input_arg = utf8_path(input_path)?;

    ensure_dump_matches("HIR", name, &example.join("expected.hir"), "dump-hir", &input_arg)?;
    ensure_dump_matches("MIR", name, &example.join("expected.mir"), "dump-mir", &input_arg)?;

    Ok(())
}

/// Compares one `smelt dump-*` output against its committed golden.
///
/// The HIR and MIR halves of `verify_example_dumps` were the same three lines
/// twice over, which also put `expected_hir`/`expected_mir` and
/// `actual_hir`/`actual_mir` in one scope — four bindings distinguished only by
/// a three-letter suffix. One helper says it once.
fn ensure_dump_matches(
    kind: &str,
    name: &str,
    golden_path: &Path,
    dump_command: &str,
    input_arg: &str,
) -> TestResult {
    let golden = fs::read_to_string(golden_path)?;
    let dumped = smelt(&[dump_command, input_arg])?;
    ensure_eq(&dumped, &golden, format!("{kind} mismatch for {name}"))
}

/// Returns the absolute path to a Python end-to-end example fixture.
pub(crate) fn python_example_dir(name: &str) -> TestResult<PathBuf> {
    Ok(workspace_root()?
        .join("examples/python/end-to-end")
        .join(name))
}

/// Verifies the HIR and MIR dumps for a single Python end-to-end example.
///
/// The Python corpus shipped as fixtures that no test read, so nothing noticed
/// if the Python frontend stopped lowering them; this is the tier that reads
/// them.
pub(crate) fn verify_python_end_to_end_example(name: &str) -> TestResult {
    let example = python_example_dir(name)?;
    let input = example.join("input.py");
    verify_example_dumps(name, &example, &input)?;

    // The runtime tier: build the generated crate, run it, and compare its
    // stdout with the golden, which is in turn checked against `python3`.
    // The manifest is the harness's own minimal one (as on the TypeScript
    // path) rather than the example's `Smelt.toml`, whose `[rust.dependencies]`
    // would compile a web stack the program never uses.
    let source = fs::read_to_string(&input)?;
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(project_path.join("src/main.py"), &source)?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "py-example"
version = "0.1.0"

[sources]
entries = ["src/main.py"]

[output]
target = "./dist"
crate-name = "py_example"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;
    let expected_stdout = fs::read_to_string(example.join("expected.stdout"))?;
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure(
        actual_stdout == expected_stdout,
        format!("{name}: runtime stdout mismatch\n--- generated\n{actual_stdout}--- golden\n{expected_stdout}"),
    )?;
    ensure_python_agrees(&source, &expected_stdout, name)
}

/// The first `CPython` release with structural pattern matching (`match`), which
/// the Python corpus uses.
const PYTHON_MIN: (u32, u32) = (3, 10);

/// Whether a `python3` able to run the corpus is on `PATH`, probed once.
///
/// Skip-if-absent, like [`node_runs_typescript`]: a missing or too-old
/// interpreter skips the oracle half of the check with one note.
#[expect(clippy::print_stderr, reason = "a skipped oracle check must say so in the test output")]
pub(crate) fn python_runs_corpus() -> bool {
    static PROBE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PROBE.get_or_init(|| {
        let Ok(output) = Command::new("python3").arg("--version").output() else {
            eprintln!("note: `python3` is not on PATH; skipping the Python oracle checks");
            return false;
        };
        let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let mut parts = version
            .trim_start_matches("Python ")
            .split('.')
            .map(|part| part.parse::<u32>().unwrap_or(0));
        let found = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
        if !output.status.success() || found < PYTHON_MIN {
            eprintln!(
                "note: `python3` {version} is older than {}.{}; skipping the Python oracle checks",
                PYTHON_MIN.0, PYTHON_MIN.1
            );
            return false;
        }
        true
    })
}

/// Asserts a Python golden stdout is what `python3` prints for the source.
///
/// Runs the program isolated (`-I`: no user site, no environment-driven
/// imports) from a fresh directory, so nothing on the host can change what it
/// prints.
pub(crate) fn ensure_python_agrees(source: &str, expected_stdout: &str, name: &str) -> TestResult {
    if !python_runs_corpus() {
        return Ok(());
    }
    let project = TempProject::new()?;
    let root = project.path().join("python-oracle");
    fs::create_dir_all(&root)?;
    fs::write(root.join("main.py"), source)?;
    let output = Command::new("python3")
        .current_dir(&root)
        .arg("-I")
        .arg("main.py")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "python3 failed on {name}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let python_stdout = String::from_utf8(output.stdout)?;
    ensure(
        python_stdout == expected_stdout,
        format!(
            "{name}: the golden stdout is not what `python3` prints for the same source\n--- python3\n{python_stdout}--- golden\n{expected_stdout}"
        ),
    )
}

/// The env var that turns the golden comparison into a golden REWRITE.
///
/// `scripts/regen-example-rust.sh` sets it. Owning the concatenation format in
/// exactly one place is the point: a shell script that rebuilt the same
/// sectioned text would be a second implementation to drift from this one, and
/// the golden it produced would differ from the one the test demands in ways
/// nothing would notice until a regeneration broke the suite.
const UPDATE_EXAMPLE_RUST_VAR: &str = "SMELT_UPDATE_EXAMPLE_RUST";

/// The env var that narrows a run to a few examples, comma-separated.
///
/// Only useful together with the rewrite above: the corpus is verified by ONE
/// `#[test]` looping over every name, so `cargo test`'s own name filter cannot
/// select a fixture.
const EXAMPLE_ONLY_VAR: &str = "SMELT_EXAMPLE_ONLY";

/// What the temp project's own directory is written as in a golden.
const EXAMPLE_PROJECT_PLACEHOLDER: &str = "<example>";

/// Whether this run rewrites the generated-Rust goldens instead of asserting.
fn update_example_rust() -> bool {
    std::env::var_os(UPDATE_EXAMPLE_RUST_VAR).is_some()
}

/// Whether `name` is one of the examples this run was narrowed to.
///
/// Unset means every example, which is what `cargo test` always wants.
pub(crate) fn example_is_selected(name: &str) -> bool {
    std::env::var(EXAMPLE_ONLY_VAR)
        .map_or(true, |only| only.split(',').any(|selected| selected.trim() == name))
}

/// Concatenate every generated `.rs` file under `dist_src` into one text.
///
/// The order is deterministic and readable rather than alphabetical:
/// `main.rs` comes first because it is the crate root — it carries the runtime
/// prelude and the `mod` declarations that name the rest — and the remaining
/// files follow sorted by name.
///
/// The FIRST section carries no header, so a single-file program's text is
/// byte-for-byte its `main.rs` and the goldens of the 48 fixtures that never
/// split did not move when this replaced the `main.rs`-only comparison. Each
/// later section is introduced by `// ==== <file>`, which is a Rust comment, so
/// the golden as a whole still reads as Rust.
///
/// `project_root` is redacted out of the text. A split module carries a
/// `// source: <path>` provenance comment naming the file it was lowered from,
/// and under this harness that path is the temp project's — a directory whose
/// name holds the test process's PID and a nanosecond timestamp, so leaving it
/// in would make every golden differ from itself on the next run. The
/// placeholder is the test's own scratch location standing in for itself and
/// nothing else; every other byte is the emitter's.
fn generated_rust_sections(dist_src: &Path, project_root: &Path) -> TestResult<String> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dist_src)? {
        let path = entry?.path();
        if path.extension().is_some_and(|extension| extension == "rs")
            && let Some(name) = path.file_name().and_then(|name| name.to_str())
        {
            names.push(name.to_owned());
        }
    }
    names.sort();
    if let Some(index) = names.iter().position(|name| name == "main.rs") {
        let root = names.remove(index);
        names.insert(0, root);
    }
    let mut sections = String::new();
    for (index, name) in names.iter().enumerate() {
        if index > 0 {
            sections.push('\n');
            sections.push_str("// ==== ");
            sections.push_str(name);
            sections.push('\n');
        }
        sections.push_str(&fs::read_to_string(dist_src.join(name))?);
    }
    let root = utf8_path(project_root)?;
    Ok(sections.replace(&root, EXAMPLE_PROJECT_PLACEHOLDER))
}

/// Verifies the compiled output for a single end-to-end example fixture.
pub(crate) fn verify_end_to_end_example(name: &str) -> TestResult {
    let example = example_dir(name)?;
    let input = example.join("input.ts");
    verify_example_dumps(name, &example, &input)?;

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

    // EVERY generated file, not just `main.rs`. A program whose lowering
    // splits into modules puts its user code in `source_<entry>.rs` and leaves
    // `main.rs` holding the prelude and a `mod` declaration — so comparing
    // `main.rs` alone golden-checked the PRELUDE and nothing the fixture was
    // written to exercise. 32 of the corpus's 82 fixtures were in that state,
    // which is why a feature could change its own emitted Rust with every
    // golden still passing.
    let golden_path = example.join("expected.rs");
    let actual_rs = generated_rust_sections(&project_path.join("dist/src"), project_path)?;
    if update_example_rust() {
        fs::write(&golden_path, &actual_rs)?;
    } else {
        let expected_rs = fs::read_to_string(&golden_path)?;
        ensure_eq(
            &actual_rs,
            &expected_rs,
            format!("Rust mismatch for {name}"),
        )?;
    }

    let expected_stdout = fs::read_to_string(example.join("expected.stdout"))?;
    let actual_stdout = cargo_run_manifest(&project_path.join("dist/Cargo.toml"))?;
    ensure_eq(
        &actual_stdout,
        &expected_stdout,
        format!("runtime stdout mismatch for {name}"),
    )?;
    // The golden itself must be Node's answer, or the comparison above only
    // proves Smelt agrees with an earlier Smelt.
    let source = fs::read_to_string(&input)?;
    ensure_node_agrees(&[("main.ts", &source)], "main.ts", &expected_stdout, name)?;

    Ok(())
}
