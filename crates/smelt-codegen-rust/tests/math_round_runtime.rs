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

#[path = "harness/mod.rs"]
mod harness;

use harness::{emit_program, run_generated_tests};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

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
    let outcome = std::panic::catch_unwind(|| {
        emit_program(source, crate_name, &crate_dir);
        run_generated_tests(&crate_dir, &target_dir);
    });
    // The scratch root holds a whole nested cargo target directory, so it is
    // removed on the FAILURE path too: leaving one behind per failing case is
    // what fills `/tmp`, and an ENOSPC inside a later nested build reads as a
    // failing assertion rather than as a full disk.
    // `SMELT_KEEP_RUNTIME_SCRATCH=1` keeps it for a debugging session.
    if std::env::var_os("SMELT_KEEP_RUNTIME_SCRATCH").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
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
