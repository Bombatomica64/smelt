//! Shared harness for the "compile the emitted Rust" test tiers.
//!
//! Three tiers drive the same pipeline and must keep driving *exactly* the
//! same one, or a failure in one of them stops being evidence about the
//! others:
//!
//! * `tests/compile_corpus.rs::corpus_emitted_rust_compiles` — the inline
//!   [`&'static str`] corpus of representative programs;
//! * `tests/compile_corpus.rs::callback_generics_fixtures_compile` — the
//!   rescued hand-written fixtures in `tests/fixtures/callback_generics/`;
//! * `tests/shape_grid.rs` — the *generated* callback-generics shape grid.
//!
//! This module owns the three pieces they share: lowering a TypeScript source
//! string through the real pipeline and emitting a crate from it, running
//! `cargo check` on that crate, and counting rustc errors in the captured
//! output. It is a `tests/<dir>/mod.rs`, so cargo does not build it as a test
//! binary of its own; each tier pulls it in with `mod corpus_support;`.

#![allow(
    dead_code,
    reason = "each test binary that includes this module uses a subset of it"
)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "this is a `tests/<dir>/mod.rs` shared by several test binaries, so the \
              module is private inside each of them and clippy reads `pub(crate)` as \
              redundant. Widening to `pub` only trades this lint for rustc's \
              `unreachable_pub`; `pub(crate)` is the honest visibility for a helper \
              that is crate-internal to every binary that includes it"
)]

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use smelt_codegen_rust::{CrateKind, EmitOptions, emit_crate};
use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;

/// Lowers TypeScript `source` through the real pipeline and emits a full
/// program crate named after `name` into `crate_dir` via [`emit_crate`].
///
/// Returns a human-readable error string on any frontend/MIR/emit failure so
/// callers can record it as a corpus failure rather than panicking: a source
/// shape the frontend rejects is a finding, not a broken test.
pub(crate) fn emit_typescript_crate(name: &str, source: &str, crate_dir: &Path) -> Result<(), String> {
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).map_err(|err| format!("HIR lowering failed: {err:?}"))?;
    let mut mir =
        smelt_mir::lower_hir(&ctx.krate).map_err(|err| format!("MIR lowering failed: {err:?}"))?;
    smelt_mir::opt::optimize(&mut mir);
    let options =
        EmitOptions::new(format!("smelt_corpus_{name}")).with_crate_kind(CrateKind::Program);
    emit_crate(&mir, crate_dir, &options).map_err(|err| format!("crate emission failed: {err}"))
}

/// Runs `cargo check` on the emitted crate at `crate_dir`, sharing the given
/// `target_dir` so corpus crates reuse compiled dependencies.
///
/// Returns `Ok(())` when `cargo check` succeeds, otherwise the captured
/// stdout/stderr so the failure can be reported.
pub(crate) fn cargo_check(crate_dir: &Path, target_dir: &Path) -> Result<(), String> {
    let output = Command::new(env!("CARGO"))
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        // Generated crates carry their own lint posture; warnings must not fail
        // the tier, only genuine compile errors should.
        .env("RUSTFLAGS", "-Awarnings")
        .output()
        .map_err(|err| format!("failed to spawn cargo check: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(format!("cargo check failed:\n{stdout}\n{stderr}"))
}

/// Counts rustc errors in captured `cargo check` output.
///
/// Counts diagnostic headers (`error[E0308]: ...` and bare `error: ...`) while
/// skipping cargo's own summary lines, so the number matches what a reader sees
/// when they run the check by hand.
pub(crate) fn rustc_error_count(output: &str) -> usize {
    output
        .lines()
        .filter(|line| {
            if line.starts_with("error[") {
                return true;
            }
            line.starts_with("error:")
                && !line.starts_with("error: aborting")
                && !line.contains("could not compile")
        })
        .count()
}

/// Returns the sorted, deduplicated rustc error codes in captured output.
///
/// `error[E0308]: ...` yields `E0308`; a bare `error: ...` yields `error`. This
/// is the stable part of a failure — the *count* of diagnostics moves with
/// rustc's grouping, but which codes fire does not — so tiers that record
/// known failures compare codes and merely report count drift.
pub(crate) fn rustc_error_codes(output: &str) -> Vec<String> {
    let mut codes: Vec<String> = output
        .lines()
        .filter_map(|line| {
            if let Some(rest) = line.strip_prefix("error[") {
                return rest.split(']').next().map(str::to_owned);
            }
            if line.starts_with("error:")
                && !line.starts_with("error: aborting")
                && !line.contains("could not compile")
            {
                return Some("error".to_owned());
            }
            None
        })
        .collect();
    codes.sort();
    codes.dedup();
    codes
}

/// Returns a unique scratch directory root for this test run.
///
/// Uses the process id and a monotonically increasing counter so repeated runs
/// and parallel cargo invocations do not collide.
pub(crate) fn scratch_root(prefix: &str) -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("{prefix}-{}-{seq}", std::process::id()))
}
