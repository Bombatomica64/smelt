//! Shared by the generated-runtime integration tests: emit a TypeScript program
//! as a crate and run its generated tests.

#![allow(
    dead_code,
    reason = "each integration test binary uses a subset of these helpers"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "pub(crate) is correct for a module shared across test binaries; plain pub trips unreachable_pub"
)]

use std::{
    fs::File,
    io,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use smelt_codegen_rust::{CrateKind, EmitOptions, emit_crate};
use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;

/// Emit `source` as a program crate named `crate_name` into `crate_dir`.
pub(crate) fn emit_program(source: &str, crate_name: &str, crate_dir: &Path) {
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).expect("HIR lowering");
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering");
    smelt_mir::opt::optimize(&mut mir);
    let options = EmitOptions::new(crate_name.to_owned()).with_crate_kind(CrateKind::Program);
    emit_crate(&mir, crate_dir, &options).expect("crate emission");
}

/// How long the generated tests may run once compiled before they are killed.
///
/// They finish in well under a second; the bound turns a defect that makes a
/// generated program loop into a failure naming it, instead of a hung suite.
/// `SMELT_TEST_RUN_TIMEOUT_SECS` overrides it for a slow machine.
fn generated_run_limit() -> Duration {
    let secs = std::env::var("SMELT_TEST_RUN_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(60);
    Duration::from_secs(secs)
}

/// How many shared target directories generated crates build in: the most
/// generated builds that run at once, machine-wide.
///
/// Every generated crate depends on the same runtime stack (`chrono-tz`,
/// `tokio`, ...), roughly 0.8 GB of compiler memory to build. A target
/// directory per fixture rebuilt that stack once per fixture, all at once:
/// the fourteen `generator_runtime` fixtures peaked at 12 GB together and were
/// OOM-killed (or thrashed, which looked like a hang) while each passed alone.
/// `SMELT_GENERATED_TARGETS` overrides the default of three, CI's
/// `--test-threads` for the runtime tiers.
pub(crate) fn generated_target_count() -> usize {
    std::env::var("SMELT_GENERATED_TARGETS")
        .ok()
        .and_then(|value| value.trim().parse::<usize>().ok())
        .filter(|count| *count > 0)
        .unwrap_or(3)
}

/// The directory the shared generated-crate target directories live in: the
/// integration tests' own scratch directory inside the workspace target
/// directory, so the dependency builds are reused across runs and removed by
/// `cargo clean`. The runtime value wins over the compile-time one, for a test
/// archive unpacked at another path.
pub(crate) fn generated_targets_root() -> PathBuf {
    std::env::var_os("CARGO_TARGET_TMPDIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_TARGET_TMPDIR")), PathBuf::from)
        .join("generated-crates")
}

/// One of the [`generated_target_count`] shared target directories, held
/// exclusively (a file lock, so other test processes wait too) until dropped.
///
/// The dependency artifacts stay in the directory for the next fixture; the
/// fixture's own package is removed on drop (also while unwinding from a
/// failed assertion), so the directory does not grow with every fixture run.
pub(crate) struct SharedTarget {
    /// The target directory to build in.
    dir: PathBuf,
    /// The manifest of the crate built here, cleaned on drop.
    manifest: PathBuf,
    /// The held lock; released when the guard drops.
    _lock: File,
}

impl SharedTarget {
    /// Takes a free shared target directory for the crate in `crate_dir`,
    /// waiting for one when all are taken.
    pub(crate) fn acquire(crate_dir: &Path) -> Self {
        let root = generated_targets_root();
        std::fs::create_dir_all(&root).expect("create the shared generated-crate target root");
        let count = generated_target_count();
        let open = |slot: usize| {
            let lock = File::options()
                .create(true)
                .truncate(false)
                .write(true)
                .open(root.join(format!("slot{slot}.lock")))
                .expect("open a shared target lock");
            (slot, lock)
        };
        let free = (0..count)
            .map(open)
            .find(|(_, lock)| lock.try_lock().is_ok());
        let (slot, lock) = free.unwrap_or_else(|| {
            // All taken: wait on one, spreading waiters over the slots.
            let spread = usize::try_from(std::process::id()).unwrap_or_default();
            let (slot, lock) = open(spread % count);
            lock.lock().expect("wait for a shared target directory");
            (slot, lock)
        });
        Self {
            dir: root.join(format!("slot{slot}")),
            manifest: crate_dir.join("Cargo.toml"),
            _lock: lock,
        }
    }

    /// The target directory to pass as `CARGO_TARGET_DIR`.
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }
}

impl Drop for SharedTarget {
    fn drop(&mut self) {
        let Some(package) = std::fs::read_to_string(&self.manifest)
            .ok()
            .and_then(|manifest| package_name(&manifest))
        else {
            return;
        };
        // Best effort: a leftover package only costs disk, never a result.
        drop(
            Command::new(env!("CARGO"))
                .args(["clean", "--quiet", "--offline", "--manifest-path"])
                .arg(&self.manifest)
                .args(["--package", &package])
                .env("CARGO_TARGET_DIR", &self.dir)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
        );
    }
}

/// The `[package]` name of a generated `Cargo.toml`: its first `name = "..."`.
fn package_name(manifest: &str) -> Option<String> {
    manifest.lines().find_map(|line| {
        let value = line
            .trim()
            .strip_prefix("name")?
            .trim_start()
            .strip_prefix('=')?;
        Some(value.trim().trim_matches('"').to_owned())
    })
}

/// The generated crate's `cargo test` command, unrun.
fn cargo_test(crate_dir: &Path, target_dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO"));
    command
        .arg("test")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        .env("RUSTFLAGS", "-Awarnings");
    command
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

/// Runs `command` to completion, or kills it after `limit` and returns `None`.
/// Both pipes are drained on their own threads so a full pipe cannot stall it.
fn output_within(command: &mut Command, limit: Duration) -> Option<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cargo test");
    let stdout_reader = drain(child.stdout.take());
    let stderr_reader = drain(child.stderr.take());
    let started = Instant::now();
    let exit = loop {
        if let Some(finished) = child.try_wait().expect("wait for cargo test") {
            break Some(finished);
        }
        if started.elapsed() >= limit {
            drop(child.kill());
            drop(child.wait());
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();
    exit.map(|status| Output {
        status,
        stdout,
        stderr,
    })
}

/// What one run of a generated crate's `cargo test` reported.
pub(crate) struct GeneratedRun {
    /// Whether `cargo test` exited successfully.
    pub(crate) success: bool,
    /// The libtest summary counts summed over every test binary, or `None` when
    /// no `test result:` line was printed (the suite never ran to its end).
    pub(crate) counts: Option<TestCounts>,
    /// Cargo's stdout and stderr, for failure messages.
    pub(crate) report: String,
}

/// Passed and failed counts from libtest's `test result:` lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TestCounts {
    /// Tests that ran and passed.
    pub(crate) passed: usize,
    /// Tests that ran and failed.
    pub(crate) failed: usize,
}

/// Sums every libtest summary line in `stdout`.
///
/// A crate's `cargo test` prints one `test result: ok. N passed; M failed; ...`
/// line per test binary (the program's unit tests, and doc-tests when there is a
/// library target). `None` means no line at all: the run was killed or never got
/// past compiling, which a caller must not read as zero tests passing.
pub(crate) fn parse_test_counts(stdout: &str) -> Option<TestCounts> {
    stdout
        .lines()
        .filter_map(|line| line.trim().strip_prefix("test result:"))
        .filter_map(|rest| {
            let count_before = |label: &str| -> Option<usize> {
                let (head, _) = rest.split_once(label)?;
                head.split_whitespace().last()?.parse().ok()
            };
            Some(TestCounts {
                passed: count_before(" passed")?,
                failed: count_before(" failed")?,
            })
        })
        .reduce(|sum, counts| TestCounts {
            passed: sum.passed.saturating_add(counts.passed),
            failed: sum.failed.saturating_add(counts.failed),
        })
}

/// One significant token of TypeScript source, as [`declared_test_count`]
/// needs it: literals and comments are dropped, whitespace is skipped.
#[derive(Debug, PartialEq, Eq)]
enum Token {
    /// An identifier or keyword.
    Word(String),
    /// A string or template literal (its text does not matter).
    Literal,
    /// Any other character.
    Punct(char),
}

/// Splits `source` into [`Token`]s, skipping comments and the contents of
/// string and template literals (a `${}` inside a template is skipped too).
fn tokens(source: &str) -> Vec<Token> {
    let mut chars = source.chars().peekable();
    let mut out = Vec::new();
    while let Some(ch) = chars.next() {
        match ch {
            '/' if chars.peek() == Some(&'/') => {
                chars.by_ref().find(|next| *next == '\n');
            }
            '/' if chars.peek() == Some(&'*') => {
                let mut previous = ' ';
                chars.by_ref().find(|next| {
                    let closes = previous == '*' && *next == '/';
                    previous = *next;
                    closes
                });
            }
            '\'' | '"' | '`' => {
                let mut escaped = false;
                chars.by_ref().find(|next| {
                    let closes = !escaped && *next == ch;
                    escaped = !escaped && *next == '\\';
                    closes
                });
                out.push(Token::Literal);
            }
            _ if ch.is_ascii_alphabetic() || ch == '_' || ch == '$' => {
                let mut word = String::from(ch);
                while let Some(next) = chars
                    .next_if(|next| next.is_ascii_alphanumeric() || *next == '_' || *next == '$')
                {
                    word.push(next);
                }
                out.push(Token::Word(word));
            }
            _ if ch.is_whitespace() => {}
            _ => out.push(Token::Punct(ch)),
        }
    }
    out
}

/// Counts the test registrations a TypeScript fixture declares: calls of a bare
/// `test(...)` or `it(...)`, at any nesting depth (inside `describe` too).
///
/// This is the oracle the generated crate is held to, so it reads the SOURCE,
/// not the emitted Rust: a lowering that drops a `test(...)` registration, or an
/// emitter that loses `#[test]`, then shows up as a count below this one. String
/// and template literals and comments are skipped, so a test NAME containing
/// `test(` is not counted, and a member call such as `re.test(s)` is not a
/// registration. Fixtures that use `test.each`/`test.skip` would need extending
/// this; none do today, and such a fixture fails loudly on its count.
pub(crate) fn declared_test_count(source: &str) -> usize {
    let mut stream = vec![Token::Punct(';')];
    stream.extend(tokens(source));
    stream
        .windows(3)
        .filter(|window| match window {
            [previous, Token::Word(name), Token::Punct('(')] => {
                (name == "test" || name == "it")
                    && *previous != Token::Punct('.')
                    && *previous != Token::Word("function".to_owned())
            }
            _ => false,
        })
        .count()
}

/// Compiles the crate's tests, then runs them under [`generated_run_limit`].
///
/// A compile failure or a hang fails the caller here; the run's own outcome is
/// returned for the caller to judge. The tests are compiled first, unbounded
/// (compile time depends on the machine), so the bound covers only the run.
///
/// Both run inside a [`SharedTarget`], held throughout so the run's bound
/// never includes waiting for another fixture's build.
pub(crate) fn run_generated_suite(crate_dir: &Path) -> GeneratedRun {
    let target = SharedTarget::acquire(crate_dir);
    let target_dir = target.dir();
    let build = cargo_test(crate_dir, target_dir)
        .arg("--no-run")
        .output()
        .expect("spawn cargo test --no-run");
    assert!(
        build.status.success(),
        "generated test failed to compile:\nstderr:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let limit = generated_run_limit();
    let finished = output_within(&mut cargo_test(crate_dir, target_dir), limit);
    assert!(
        finished.is_some(),
        "generated tests did not finish within {}s and were killed (set SMELT_TEST_RUN_TIMEOUT_SECS to \
             raise the bound); a generated program that never ends is a defect",
        limit.as_secs()
    );
    let Some(output) = finished else {
        return GeneratedRun {
            success: false,
            counts: None,
            report: String::new(),
        };
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    GeneratedRun {
        success: output.status.success(),
        counts: parse_test_counts(&stdout),
        report: format!(
            "stdout:\n{stdout}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}

/// A judge of an emitted crate: [`run_generated_tests`] or
/// [`run_generated_tests_expecting_failure`], as `(source, crate_dir)`.
pub(crate) type Judge = fn(&str, &Path);

/// Run the crate's generated tests and hold them to the fixture `source`.
///
/// Every test the source declares ([`declared_test_count`]) must have run and
/// passed, none may fail, and the source must declare at least one: exit status
/// alone passes a crate that runs `0 tests`, which is exactly what a dropped
/// `test(...)` registration or a lost `#[test]` produces. A failing or
/// uncompilable generated test, or one still running after
/// [`generated_run_limit`], fails the caller with cargo's output.
pub(crate) fn run_generated_tests(source: &str, crate_dir: &Path) {
    run_generated_tests_plus(source, 0, crate_dir);
}

/// [`run_generated_tests`] for a crate that also carries `extra` hand-written
/// Rust `#[test]`s appended after emission (a probe of runtime internals that
/// TypeScript cannot reach): those must run and pass as well.
pub(crate) fn run_generated_tests_plus(source: &str, extra: usize, crate_dir: &Path) {
    let declared = declared_test_count(source);
    assert!(
        declared > 0,
        "the fixture declares no `test(...)`/`it(...)`: a runtime tier with nothing to run proves nothing"
    );
    let run = run_generated_suite(crate_dir);
    let report = &run.report;
    assert!(run.success, "generated test failed:\n{report}");
    assert!(
        run.counts.is_some(),
        "generated tests printed no `test result:` summary:\n{report}"
    );
    let Some(counts) = run.counts else { return };
    assert_eq!(
        counts,
        TestCounts {
            passed: declared.saturating_add(extra),
            failed: 0
        },
        "the generated crate must run and pass every test the fixture declares ({declared}, plus {extra} \
         appended Rust tests):\n{report}"
    );
}

/// Run the crate's generated tests and require that they FAIL on an assertion.
///
/// A deleted assertion is indistinguishable from a satisfied one unless a
/// deliberately false one is seen to fail, so this is the should-fail control.
/// The crate must compile (a compile error is not the failure under test), every
/// declared test must run, and at least one must report failed.
pub(crate) fn run_generated_tests_expecting_failure(source: &str, crate_dir: &Path) {
    let declared = declared_test_count(source);
    let run = run_generated_suite(crate_dir);
    let report = &run.report;
    assert!(
        !run.success,
        "generated suite should have failed but passed (the assertion was dropped again):\n{report}"
    );
    assert!(
        run.counts.is_some(),
        "generated tests printed no `test result:` summary:\n{report}"
    );
    let Some(counts) = run.counts else { return };
    assert!(
        counts.failed > 0 && counts.passed.saturating_add(counts.failed) == declared,
        "expected an assertion failure among the {declared} declared tests, got {counts:?}:\n{report}"
    );
}
