//! Shared by the generated-runtime integration tests: emit a TypeScript program
//! as a crate and run its generated tests.

#![allow(dead_code, reason = "each integration test binary uses a subset of these helpers")]
#![allow(clippy::redundant_pub_crate, reason = "pub(crate) is correct for a module shared across test binaries; plain pub trips unreachable_pub")]

use std::{
    io,
    path::Path,
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
    exit.map(|status| Output { status, stdout, stderr })
}

/// Run the crate's generated tests; a failing or uncompilable generated test
/// fails the caller with cargo's output, and one still running after
/// [`generated_run_limit`] fails it as a hang.
///
/// The tests are compiled first, unbounded (compile time depends on the
/// machine), so the bound covers only the run.
pub(crate) fn run_generated_tests(crate_dir: &Path, target_dir: &Path) {
    let build = cargo_test(crate_dir, target_dir).arg("--no-run").output().expect("spawn cargo test --no-run");
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
    let Some(output) = finished else { return };
    assert!(
        output.status.success(),
        "generated test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
