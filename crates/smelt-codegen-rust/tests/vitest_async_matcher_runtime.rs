//! Runtime execution tests for `expect(promise).resolves` / `.rejects` matcher
//! chains.
//!
//! This tier exists because the defect it guards was invisible to every other
//! tier. `await expect(p).rejects.toEqual(x)` (and every `.resolves`/`.rejects`
//! matcher except `rejects.toThrow`) used to lower to a bare `Promise<void>`
//! literal: the assertion was dropped, and because the actual was only
//! evaluated into an orphaned HIR expression, *the awaited call itself* was
//! dropped with it. The generated Rust was `let _smelt_tmp_0: () = ();`. It
//! type-checked, `compile_corpus` accepted it, and the generated test passed
//! unconditionally — so the suite reported a pass for an assertion that could
//! not fail.
//!
//! The two properties below are therefore checked by *running* generated
//! crates:
//!
//! 1. The awaited call still happens and the matcher sees the settled value —
//!    a side-effect counter proves the call was not deleted.
//! 2. A false assertion actually fails. `expect_generated_tests_fail` runs a
//!    fixture whose matcher is wrong on purpose and asserts a red run; under
//!    the old lowering it was green, which is precisely the bug.
//!
//! The tier is `#[ignore]`d because it compiles and runs real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test vitest_async_matcher_runtime -- --ignored
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

/// Async helpers shared by the fixtures.
///
/// `calls` counts every entry into `resolving`/`rejecting`, so a fixture can
/// assert that the promise handed to `expect(...)` was really created and
/// awaited rather than optimized away with the assertion.
const HELPERS: &str = r#"
let calls = 0;

async function resolving(): Promise<string> {
  calls += 1;
  return "settled";
}

async function unitResolving(): Promise<void> {
  calls += 1;
}

async function rejecting(): Promise<string> {
  calls += 1;
  throw "boom";
}
"#;

/// Lowers `source` through the real pipeline and emits a runnable program crate.
fn emit_program(source: &str, crate_name: &str, crate_dir: &Path) {
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).expect("HIR lowering");
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering");
    smelt_mir::opt::optimize(&mut mir);
    let options = EmitOptions::new(crate_name.to_owned()).with_crate_kind(CrateKind::Program);
    emit_crate(&mir, crate_dir, &options).expect("crate emission");
}

/// Runs `cargo test` on the emitted crate and returns whether it passed.
fn generated_tests_pass(crate_dir: &Path, target_dir: &Path) -> (bool, String) {
    let output = Command::new(env!("CARGO"))
        .arg("test")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        .env("RUSTFLAGS", "-Awarnings")
        .output()
        .expect("spawn cargo test");
    let report = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), report)
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-vitest-async-runtime-{}-{seq}",
        std::process::id()
    ))
}

/// Emit `source` as a crate, run its generated tests, and return the outcome.
fn run_fixture(source: &str, crate_name: &str) -> (bool, String) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    emit_program(source, crate_name, &crate_dir);
    let outcome = generated_tests_pass(&crate_dir, &target_dir);
    drop(std::fs::remove_dir_all(&root));
    outcome
}

/// Assert the generated suite passes.
fn expect_generated_tests_pass(source: &str, crate_name: &str) {
    let (passed, report) = run_fixture(source, crate_name);
    assert!(passed, "generated suite should pass but failed:\n{report}");
}

/// Assert the generated suite fails.
///
/// A deleted assertion is indistinguishable from a satisfied one unless a
/// deliberately false assertion is observed to fail, so this direction is the
/// load-bearing half of the guard.
fn expect_generated_tests_fail(source: &str, crate_name: &str) {
    let (passed, report) = run_fixture(source, crate_name);
    assert!(
        !passed,
        "generated suite should have failed but passed \
         (the assertion was dropped again):\n{report}"
    );
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn resolves_matchers_await_the_actual_and_assert_the_settled_value() {
    // `calls` proves the awaited call survived lowering: under the old
    // placeholder path both assertions and both calls vanished and `calls`
    // stayed 0.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HELPERS}
test("resolves matchers assert the settled value", async () => {{
  await expect(unitResolving()).resolves.toBeUndefined();
  await expect(resolving()).resolves.toBe("settled");
  expect(calls).toBe(2);
}});
"#
    );
    expect_generated_tests_pass(&source, "smelt_vitest_resolves_settled");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn rejects_matchers_assert_the_rejection_payload() {
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HELPERS}
test("rejects matchers assert the rejection payload", async () => {{
  await expect(rejecting()).rejects.toEqual("boom");
  expect(calls).toBe(1);
}});
"#
    );
    expect_generated_tests_pass(&source, "smelt_vitest_rejects_payload");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrong_resolves_matcher_fails_the_generated_test() {
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HELPERS}
test("a wrong resolves matcher fails", async () => {{
  await expect(resolving()).resolves.toBe("not settled");
}});
"#
    );
    expect_generated_tests_fail(&source, "smelt_vitest_resolves_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrong_rejects_matcher_fails_the_generated_test() {
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HELPERS}
test("a wrong rejects matcher fails", async () => {{
  await expect(rejecting()).rejects.toEqual("not boom");
}});
"#
    );
    expect_generated_tests_fail(&source, "smelt_vitest_rejects_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_promise_that_resolves_fails_a_rejects_matcher() {
    // Without the `did_throw` guard the catch body never runs, so the matcher
    // never runs either and a promise that resolved would pass a `.rejects`
    // assertion vacuously.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HELPERS}
test("a resolving promise fails a rejects matcher", async () => {{
  await expect(resolving()).rejects.toEqual("boom");
}});
"#
    );
    expect_generated_tests_fail(&source, "smelt_vitest_rejects_no_rejection");
}
