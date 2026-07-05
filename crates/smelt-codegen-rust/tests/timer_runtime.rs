//! Runtime execution tests for typed timer extra arguments.
//!
//! The `compile_corpus` tier proves the emitted Rust type-checks; this tier
//! proves the typed timer wrapper closures produced for
//! `setTimeout(callback, ms, ...args)` / `setInterval(callback, ms, ...args)`
//! actually run and forward the captured arguments correctly.
//!
//! Each case is a TypeScript Vitest test whose body schedules a timer with typed
//! extra arguments, drives the virtual-time event loop with `await delay(...)`,
//! and asserts on the effect the callback produced. Lowering a Vitest test emits
//! a `#[tokio::test]` function, so this tier lowers each program to a crate and
//! runs `cargo test` on it — a green `cargo test` means every `expect(...)` held
//! at runtime. Vitest tests are the supported async entry point in generated
//! code (a user-defined `async function main` is a separate, known codegen gap),
//! which is why the driver is a test rather than a program `main`.
//!
//! The whole tier is `#[ignore]`d because it compiles and executes real crates,
//! which is slow. CI (and you) run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test timer_runtime -- --ignored
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

/// A `delay` helper reused by every fixture: awaiting it advances the shared
/// virtual-time timer queue so scheduled timers fire deterministically.
const DELAY_HELPER: &str = r"
async function delay(ms: number): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, ms));
}
";

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
        "generated timer test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-timer-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_timer_fixture(source: &str, crate_name: &str) {
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
fn set_timeout_forwards_multiple_typed_args() {
    // Two concretely typed extras (`string`, `number`) must be captured by the
    // synthesized wrapper and forwarded to the callback when the timer fires.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{DELAY_HELPER}
test("setTimeout forwards multiple typed args", async () => {{
  const results: string[] = [];
  const greet = (name: string, count: number): void => {{
    results.push(name + ":" + count);
  }};
  setTimeout(greet, 10, "hi", 3);
  await delay(50);
  expect(results).toContain("hi:3");
}});
"#
    );
    run_timer_fixture(&source, "smelt_timer_runtime_multiple");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn set_timeout_forwards_optional_typed_arg() {
    // A callback with an optional parameter routes through the typed wrapper when
    // every declared parameter (including the optional) is supplied.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{DELAY_HELPER}
test("setTimeout forwards optional typed arg", async () => {{
  const results: string[] = [];
  const note = (prefix: string, suffix?: string): void => {{
    results.push(prefix + (suffix ?? "<none>"));
  }};
  // Both the optional parameter's value and callback identity must survive being
  // captured and invoked later by the synthesized wrapper.
  setTimeout(note, 5, "with:", "extra");
  await delay(50);
  expect(results).toContain("with:extra");
}});
"#
    );
    run_timer_fixture(&source, "smelt_timer_runtime_optional");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn set_interval_forwards_typed_args_and_clears() {
    // `setInterval` shares the typed-wrapper path; the interval fires with the
    // forwarded typed argument on each period and stops once cleared by handle.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{DELAY_HELPER}
test("setInterval forwards typed args and clears", async () => {{
  const results: string[] = [];
  const tick = (label: string): void => {{
    results.push(label + ":" + (results.length + 1));
  }};
  const id = setInterval(tick, 10, "tock");
  await delay(35);
  clearInterval(id);
  await delay(30);
  expect(results).toContain("tock:1");
  expect(results).toContain("tock:3");
  expect(results.length).toBe(3);
}});
"#
    );
    run_timer_fixture(&source, "smelt_timer_runtime_interval");
}
