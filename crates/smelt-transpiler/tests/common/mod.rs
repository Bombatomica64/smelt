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
    process::{Command, Output, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use smelt_specialize::prereq::{locate_executable, missing_prerequisite};

/// Result type used by integration tests.
pub(crate) type TestResult<T = ()> = Result<T, Box<dyn Error>>;

/// Whether `test` must stop because one of `tools` is not on `PATH`.
///
/// The missing tool goes through [`missing_prerequisite`], so `Ok(true)` is a
/// reported skip and strict mode makes it an error instead.
pub(crate) fn tool_missing(test: &str, tools: &[&str]) -> TestResult<bool> {
    let Some(tool) = tools.iter().find(|tool| locate_executable(tool, &[]).is_none()) else {
        return Ok(false);
    };
    missing_prerequisite::<Box<dyn Error>>(test, tool)?;
    Ok(true)
}

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
        // The clock alone can repeat across parallel tests, and two tests sharing
        // a directory run each other's sources.
        static SEQUENCE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        Ok(Self {
            path: std::env::temp_dir()
                .join(format!("smelt-cli-test-{}-{sequence}-{nonce}", std::process::id())),
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

/// A generated crate whose dependencies build in this test binary's shared
/// Cargo build directory instead of a fresh one.
///
/// Every generated crate is its own `[workspace]` in a throwaway directory, so
/// by default each one rebuilds its dependencies from scratch. For the corpus
/// loops that was the whole cost of the test suite: 114 of the 161 end-to-end
/// examples depend on chrono, chrono-tz, fancy-regex and tokio, and compiling
/// them 114 times took ~30 of the CI coverage job's ~39 minutes. Pointing
/// every crate at one `build.build-dir` compiles each dependency set once;
/// final binaries still land in the crate's own `target/`.
///
/// THE CRATE NAME MUST BE UNIQUE among the crates sharing the directory. Cargo
/// does not hash a workspace-root package's path into its unit metadata, so two
/// crates with the same name and version share one unit: when the second
/// crate's sources are not newer than the first's build, Cargo calls the unit
/// fresh and hands back the FIRST crate's binary, and the test checks the
/// wrong program. [`SharedDepsCrate::claim`] enforces uniqueness within the
/// process, and [`cargo_run_shared`] refuses a manifest whose package name is
/// not the claimed one. The directory is per test binary, so binaries that
/// run in parallel (nextest) never share it.
pub(crate) struct SharedDepsCrate {
    /// The claimed `[package] name` (also the `[output] crate-name`).
    name: String,
}

impl SharedDepsCrate {
    /// Claims `name` for one generated crate in this process, failing when it
    /// was already claimed.
    pub(crate) fn claim(name: impl Into<String>) -> TestResult<Self> {
        static CLAIMED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
            std::sync::OnceLock::new();
        let crate_name = name.into();
        ensure(
            !crate_name.contains('-'),
            format!("shared crate name `{crate_name}` must not contain `-` (see the Drop cleanup)"),
        )?;
        let newly_claimed = CLAIMED
            .get_or_init(Default::default)
            .lock()
            .map_err(|poisoned| {
                io::Error::other(format!("shared crate-name registry is poisoned: {poisoned}"))
            })?
            .insert(crate_name.clone());
        if !newly_claimed {
            return Err(io::Error::other(format!(
                "generated crate name `{crate_name}` is already used in the shared build dir; \
                 a reused name can run another crate's binary"
            ))
            .into());
        }
        Ok(Self { name: crate_name })
    }

    /// The claimed crate name, for the test's `Smelt.toml`.
    pub(crate) fn name(&self) -> &str {
        &self.name
    }

    /// Points `command`'s Cargo (or `smelt build`'s nested Cargo) at the
    /// shared build directory.
    #[expect(
        clippy::unused_self,
        reason = "taking the claim is what proves the crate's name is unique in the directory"
    )]
    fn apply<'command>(&self, command: &'command mut Command) -> &'command mut Command {
        // Incremental state is useless for a crate built once, and at ~15 MB
        // per crate it was a third of the directory's size.
        command
            .env("CARGO_BUILD_BUILD_DIR", Self::build_dir())
            .env("CARGO_INCREMENTAL", "0")
    }

    /// The shared build directory of this test binary.
    fn build_dir() -> PathBuf {
        Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("generated-build")
            .join(env!("CARGO_CRATE_NAME"))
    }
}

impl Drop for SharedDepsCrate {
    /// Removes the crate's OWN units (`<name>-<hash>` binaries, dep-info and
    /// fingerprints) from the shared directory, keeping the dependencies.
    ///
    /// Without this the directory kept every corpus program's ~10 MB debug
    /// binary, several GB per run, where the throwaway target dirs used to be
    /// deleted with their project. Claimed names never contain `-`, so the
    /// `<name>-` prefix cannot match another crate's units.
    fn drop(&mut self) {
        let prefix = format!("{}-", self.name);
        for kind in ["deps", ".fingerprint", "incremental"] {
            let Ok(entries) = fs::read_dir(Self::build_dir().join("debug").join(kind)) else {
                continue;
            };
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with(&prefix) {
                    let path = entry.path();
                    drop(if path.is_dir() {
                        fs::remove_dir_all(&path)
                    } else {
                        fs::remove_file(&path)
                    });
                }
            }
        }
    }
}

/// Runs `smelt` from the workspace root like [`smelt`], with a manifest whose
/// `build = true` builds the generated crate in the shared build directory.
pub(crate) fn smelt_shared(args: &[&str], shared: &SharedDepsCrate) -> TestResult<String> {
    let output = shared
        .apply(Command::new(env!("CARGO_BIN_EXE_smelt")).current_dir(workspace_root()?))
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

/// Runs `cargo run --manifest-path` for a generated crate whose dependencies
/// build in the shared build directory, and returns stdout.
///
/// Fails before building when the manifest's package name is not the claimed
/// one, since only a claimed name is guaranteed unique (see [`SharedDepsCrate`]).
pub(crate) fn cargo_run_shared(manifest: &Path, shared: &SharedDepsCrate) -> TestResult<String> {
    let package_line = format!("name = \"{}\"", shared.name());
    ensure(
        fs::read_to_string(manifest)?
            .lines()
            .any(|line| line == package_line),
        format!(
            "{} does not declare the claimed package name `{}`",
            manifest.display(),
            shared.name()
        ),
    )?;
    run_generated_crate(shared.apply(&mut Command::new("cargo")), manifest)
}

/// Runs `cargo run --manifest-path` for a generated crate and returns stdout.
pub(crate) fn cargo_run_manifest(manifest: &Path) -> TestResult<String> {
    run_generated_crate(&mut Command::new("cargo"), manifest)
}

/// How long one run of a program under test (Node, `python3` or a generated
/// binary) may take before the harness kills it.
///
/// The programs these tests run finish in well under a second; the bound only
/// exists so a defect that makes either side loop fails the test with a clear
/// message instead of hanging the whole suite. `SMELT_TEST_RUN_TIMEOUT_SECS`
/// overrides it for a slow machine.
pub(crate) fn program_run_limit() -> Duration {
    const DEFAULT_SECS: u64 = 60;
    let secs = std::env::var("SMELT_TEST_RUN_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_SECS);
    Duration::from_secs(secs)
}

/// Reads `pipe` to its end on its own thread; `None` reads nothing.
fn drain<R: io::Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut reader) = pipe {
            drop(reader.read_to_end(&mut bytes));
        }
        bytes
    })
}

/// Runs `command` to completion like [`Command::output`], but kills it once it
/// has run for `limit` and reports that as an error naming `what`.
///
/// Both pipes are drained on their own threads while the child runs, so a
/// child that fills a pipe buffer cannot deadlock against the wait, and the
/// output it printed before it was killed is part of the error.
pub(crate) fn output_within(command: &mut Command, limit: Duration, what: &str) -> TestResult<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout_reader = drain(child.stdout.take());
    let stderr_reader = drain(child.stderr.take());
    let started = Instant::now();
    let exit = loop {
        if let Some(finished) = child.try_wait()? {
            break Some(finished);
        }
        if started.elapsed() >= limit {
            drop(child.kill());
            drop(child.wait());
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let joined = |handle: std::thread::JoinHandle<Vec<u8>>| {
        handle.join().map_err(|panic| {
            io::Error::other(format!("reading the output of {what} panicked: {panic:?}"))
        })
    };
    let stdout = joined(stdout_reader)?;
    let stderr = joined(stderr_reader)?;
    let Some(status) = exit else {
        return Err(io::Error::other(format!(
            "{what} did not finish within {}s and was killed (set SMELT_TEST_RUN_TIMEOUT_SECS to \
             raise the bound); a program that never ends is a defect on that side\n\
             stdout so far:\n{}\nstderr so far:\n{}",
            limit.as_secs(),
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        ))
        .into());
    };
    Ok(Output { status, stdout, stderr })
}

/// Builds a generated crate with `command` (a `cargo` command, possibly with
/// environment already set) and returns the path of its binary.
///
/// The build itself is not bounded: compile time depends on the machine and the
/// cache, and rustc does not loop on generated code. Only the run is.
fn build_generated_crate(command: &mut Command, manifest: &Path) -> TestResult<PathBuf> {
    build_generated_artifacts(command.arg("build"), manifest)?
        .pop()
        .ok_or_else(|| io::Error::other(format!("cargo built {} but reported no binary", manifest.display())).into())
}

/// Compiles a generated crate's test harness with `command` (a `cargo`
/// command) and returns the paths of its test binaries, unrun.
pub(crate) fn build_generated_tests(command: &mut Command, manifest: &Path) -> TestResult<Vec<PathBuf>> {
    build_generated_artifacts(command.arg("test").arg("--no-run"), manifest)
}

/// Runs each test binary of `binaries` under [`program_run_limit`] (with
/// `configure` applied) and returns their combined output, failing on the
/// first that is killed. The exit status is that of the first binary that
/// failed, or success.
pub(crate) fn run_generated_tests(
    binaries: &[PathBuf],
    configure: impl Fn(&mut Command),
) -> TestResult<Output> {
    let mut combined: Option<Output> = None;
    for binary in binaries {
        let mut run = Command::new(binary);
        configure(&mut run);
        let output = output_within(&mut run, program_run_limit(), "the generated tests")?;
        combined = Some(match combined {
            None => output,
            Some(mut acc) => {
                if acc.status.success() {
                    acc.status = output.status;
                }
                acc.stdout.extend(output.stdout);
                acc.stderr.extend(output.stderr);
                acc
            }
        });
    }
    combined.ok_or_else(|| io::Error::other("cargo built no test binary").into())
}

/// Runs `command` (a `cargo build` or `cargo test --no-run` already carrying
/// its subcommand) for `manifest` and returns every executable it reports, in
/// build order.
fn build_generated_artifacts(command: &mut Command, manifest: &Path) -> TestResult<Vec<PathBuf>> {
    let output = command
        .arg("--quiet")
        .arg("--message-format=json-render-diagnostics")
        .arg("--manifest-path")
        .arg(manifest)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "generated crate failed to build\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ))
        .into());
    }
    let mut executables = Vec::new();
    for line in String::from_utf8(output.stdout)?.lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(serde_json::Value::as_str) == Some("compiler-artifact")
            && let Some(path) = message.get("executable").and_then(serde_json::Value::as_str)
        {
            executables.push(PathBuf::from(path));
        }
    }
    Ok(executables)
}

/// Builds the generated crate through `command`, then runs its binary under
/// [`program_run_limit`] with `configure` applied, and returns stdout.
fn run_generated_crate_with(
    command: &mut Command,
    manifest: &Path,
    configure: impl FnOnce(&mut Command),
) -> TestResult<String> {
    let binary = build_generated_crate(command, manifest)?;
    let mut run = Command::new(&binary);
    configure(&mut run);
    let output = output_within(&mut run, program_run_limit(), "the generated program")?;
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

/// Builds a generated crate through `command` (a `cargo` command, possibly
/// with environment already set), runs it bounded, and returns stdout.
fn run_generated_crate(command: &mut Command, manifest: &Path) -> TestResult<String> {
    run_generated_crate_with(command, manifest, |_| {})
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

/// What [`node_runs_typescript`] looks for, as a skip names it.
pub(crate) const NODE_ORACLE: &str = "a `node` that runs TypeScript";

/// Whether `npm` is on `PATH`, probed once per test binary.
///
/// Needed only by an oracle program with npm dependencies (see
/// [`node_typescript_stdout`]); without it that program's oracle check is
/// skipped with one note, like a missing `node`.
#[expect(clippy::print_stderr, reason = "a skipped oracle check must say so in the test output")]
pub(crate) fn npm_available() -> bool {
    static PROBE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *PROBE.get_or_init(|| {
        let available = Command::new("npm")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success());
        if !available {
            eprintln!("note: `npm` is not on PATH; skipping the Node oracle checks of programs with npm dependencies");
        }
        available
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
///
/// # npm dependencies
///
/// A program that imports an npm package at run time (not `import type`, which
/// Node strips) lists the package in a `package.json` among `files`, pinned to
/// an exact version. The oracle installs it with `npm install` before running
/// Node; Smelt itself never reads that file (it models or lowers the package).
/// [`ensure_node_agrees`] skips such a program when `npm` is absent, and an
/// install that fails is an error, not a skip.
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
    if files.iter().any(|(relative, _)| *relative == "package.json") {
        let output = Command::new("npm")
            .current_dir(&root)
            .args(["install", "--no-audit", "--no-fund", "--ignore-scripts", "--loglevel=error"])
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "npm install failed for the {entry} oracle\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
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
    let output = output_within(&mut command, program_run_limit(), &format!("node on {entry}"))?;
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
        return missing_prerequisite(name, NODE_ORACLE);
    }
    if files.iter().any(|(relative, _)| *relative == "package.json") && !npm_available() {
        return missing_prerequisite(name, "npm");
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
    run_generated_crate_with(&mut Command::new("cargo"), manifest, |run| {
        for (key, value) in env {
            run.env(key, value);
        }
        for key in unset {
            run.env_remove(key);
        }
    })
}

/// Builds a generated crate's tests, runs them under [`program_run_limit`],
/// and returns stdout.
pub(crate) fn cargo_test_manifest(manifest: &Path) -> TestResult<String> {
    let binaries = build_generated_tests(&mut Command::new("cargo"), manifest)?;
    let output = run_generated_tests(&binaries, |_| {})?;

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
    let shared = SharedDepsCrate::claim(format!("py_e2e_{name}"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        format!(
            r#"[project]
name = "py-example"
version = "0.1.0"

[sources]
entries = ["src/main.py"]

[output]
target = "./dist"
crate-name = "{crate_name}"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
            crate_name = shared.name()
        ),
    )?;
    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    smelt(&["--manifest-path", &manifest_arg, "build"])?;
    let expected_stdout = fs::read_to_string(example.join("expected.stdout"))?;
    let actual_stdout = cargo_run_shared(&project_path.join("dist/Cargo.toml"), &shared)?;
    ensure(
        actual_stdout == expected_stdout,
        format!("{name}: runtime stdout mismatch\n--- generated\n{actual_stdout}--- golden\n{expected_stdout}"),
    )?;
    ensure_python_agrees(&source, &expected_stdout, name)
}

/// The first `CPython` release with structural pattern matching (`match`), which
/// the Python corpus uses.
const PYTHON_MIN: (u32, u32) = (3, 10);

/// What [`python_runs_corpus`] looks for, as a skip names it.
pub(crate) const PYTHON_ORACLE: &str = "a `python3` able to run the corpus";

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
        return missing_prerequisite(name, PYTHON_ORACLE);
    }
    let project = TempProject::new()?;
    let root = project.path().join("python-oracle");
    fs::create_dir_all(&root)?;
    fs::write(root.join("main.py"), source)?;
    let output = output_within(
        Command::new("python3").current_dir(&root).arg("-I").arg("main.py"),
        program_run_limit(),
        &format!("python3 on {name}"),
    )?;
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
    let shared = SharedDepsCrate::claim(format!("e2e_{name}"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        format!(
            r#"[project]
name = "example-app"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]

[output]
target = "./dist"
crate-name = "{crate_name}"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
            crate_name = shared.name()
        ),
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
    let actual_stdout = cargo_run_shared(&project_path.join("dist/Cargo.toml"), &shared)?;
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
