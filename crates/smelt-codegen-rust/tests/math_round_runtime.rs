//! Runtime execution tests for JavaScript `Math.round` tie behaviour.
//!
//! JavaScript rounds a tie toward **+∞**; Rust's `f64::round` rounds a tie away
//! from zero. They disagree for every negative value whose fractional part is
//! exactly `0.5`: `Math.round(-1.5)` is `-1` in JavaScript and `-2.0` in Rust.
//! es-toolkit's `round` specs assert the JavaScript answer, and they say so in a
//! source comment because it surprises people.
//!
//! Two edges come with the rule and are pinned here too. Very large magnitudes must
//! pass through unchanged — the ECMA-262 note on `Math.round` calls out that the
//! naive `floor(x + 0.5)` is wrong there, because adding `0.5` is not
//! representable. And `-0` must survive: `Math.round(-0.5)` is `-0` in JavaScript,
//! and `Object.is(-0, 0)` is `false`, so a caller that inspects the sign of a zero
//! must see the JavaScript answer.
//!
//! A string golden (`math_round_uses_the_javascript_tie_rule`) proves the helper is
//! emitted and that `Math.floor` still maps straight to `f64::floor`; only running
//! the program proves the arithmetic.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test math_round_runtime -- --ignored
//! ```

#![expect(
    clippy::expect_used,
    reason = "runtime tests fail fast on invalid fixture setup"
)]

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use smelt_codegen_rust::{CrateKind, EmitOptions, emit_crate};
use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;

/// Lowers `source` through the real pipeline and emits a runnable program crate.
fn emit_program(source: &str, crate_name: &str, crate_dir: &Path) {
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).expect("HIR lowering");
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering");
    smelt_mir::opt::optimize(&mut mir);
    let options = EmitOptions::new(crate_name.to_owned()).with_crate_kind(CrateKind::Program);
    emit_crate(&mir, crate_dir, &options).expect("crate emission");
}

/// Runs `cargo test` on the emitted crate; a passing run means every generated
/// `expect(...)` assertion held at runtime.
fn run_generated_tests(crate_dir: &Path, target_dir: &Path) {
    let output = Command::new(env!("CARGO"))
        .arg("test")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        .env("RUSTFLAGS", "-Awarnings")
        .output()
        .expect("spawn cargo test");
    assert!(
        output.status.success(),
        "generated Math.round test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-math-round-runtime-{}-{seq}",
        std::process::id()
    ))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_fixture(source: &str, crate_name: &str) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    emit_program(source, crate_name, &crate_dir);
    run_generated_tests(&crate_dir, &target_dir);
    drop(std::fs::remove_dir_all(&root));
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn math_round_matches_javascript() {
    let source = r"
import { test, expect } from 'vitest';

test('a tie rounds toward +infinity', () => {
  expect(Math.round(1.5)).toBe(2);
  expect(Math.round(-1.5)).toBe(-1);
  expect(Math.round(-2.5)).toBe(-2);
  expect(Math.round(0.5)).toBe(1);
});
test('a non-tie rounds to the nearest integer', () => {
  expect(Math.round(1.4)).toBe(1);
  expect(Math.round(-1.4)).toBe(-1);
  expect(Math.round(1.999)).toBe(2);
  expect(Math.round(-1.999)).toBe(-2);
});
test('a very large magnitude passes through unchanged', () => {
  expect(Math.round(9007199254740993)).toBe(9007199254740993);
  expect(Math.round(-9007199254740993)).toBe(-9007199254740993);
});
test('negative zero survives', () => {
  expect(Object.is(Math.round(-0.5), -0)).toBe(true);
});
test('floor, ceil and trunc are unchanged', () => {
  expect(Math.floor(-1.5)).toBe(-2);
  expect(Math.ceil(-1.5)).toBe(-1);
  expect(Math.trunc(-1.5)).toBe(-1);
});
";
    run_fixture(source, "smelt_math_round_semantics");
}
