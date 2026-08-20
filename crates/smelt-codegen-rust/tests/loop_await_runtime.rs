//! Runtime execution tests for loops whose body contains `try`/`catch`.
//!
//! A `for`/`while` body containing a `try`/`catch` — with or without an `await`
//! inside it — was not emitted as a Rust `loop`. Two gaps combined:
//!
//! * `block_exits_to_loop`, the guard `while_header` consults to decide whether a
//!   body is loop-shaped, had no arm for `Terminator::Await` and answered `false`
//!   for any body containing one, so the header was never recognized; and
//! * the throwing-call and throwing-`await` emitters hardcoded `emit_block` for
//!   their `Ok`/`Err` continuations, so the back edge reached from inside
//!   `match fut.await { .. }` re-emitted the loop header inline instead of
//!   rendering `continue`.
//!
//! The emitter's recursion-depth cap is what stopped the resulting expansion,
//! which is the reason this is a silent *correctness* cliff and not merely a
//! code-size one: the generated function performs only as many iterations as
//! there were inlined copies, then falls into a default return. es-toolkit's
//! `retry` is exactly this shape.
//!
//! A string golden can prove a `loop` was emitted. Only running the program
//! proves the loop iterates the right number of times and returns the right
//! value, which is what this tier does: each case is a TypeScript Vitest test,
//! lowering emits a `#[test]`, and a green `cargo test` on the generated crate
//! means every `expect(...)` held.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test loop_await_runtime -- --ignored
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
        "generated loop/await test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-loop-await-runtime-{}-{seq}",
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
fn an_awaiting_retry_loop_iterates_until_it_succeeds() {
    // The reduced `retry` shape. `attempt(5)` must observe four calls and answer
    // 3 (the first argument `flaky` accepts); `attempt(1)` must exhaust the range
    // and answer -1. Before the fix the loop body was inlined a bounded number of
    // times and the tail became a default return, so the call count and the
    // returned value were both whatever the recursion cap happened to allow.
    //
    // Every function here is declared at module scope on purpose: a function
    // nested inside the test body lowers to a closure, and the closure-body
    // emitter structures neither loops nor `try`/`catch` at all (a separate,
    // wider gap this change does not touch). Module scope is what exercises the
    // block emitter under test.
    let source = r#"
import { test, expect } from "vitest";

let calls = 0;

async function flaky(n: number): Promise<number> {
  calls++;
  if (n < 3) {
    throw new Error("nope");
  }
  return n;
}

async function attempt(limit: number): Promise<number> {
  for (let i = 0; i <= limit; i++) {
    try {
      return await flaky(i);
    } catch (err) {
      // keep going
    }
  }
  return -1;
}

test("an awaiting try/catch loop retries until the call succeeds", async () => {
  calls = 0;
  expect(await attempt(5)).toBe(3);
  expect(calls).toBe(4);
  calls = 0;
  expect(await attempt(1)).toBe(-1);
  expect(calls).toBe(2);
});
"#;
    run_fixture(source, "smelt_awaiting_retry_loop");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_awaiting_loop_runs_more_iterations_than_the_recursion_cap() {
    // The recursion cap allowed on the order of a dozen inlined copies, so a
    // short loop could accidentally produce the right answer. Twenty iterations
    // is past that: only a real `loop` sums 0..19 to 190 and reaches the exit
    // block having run the body exactly twenty times. Both halves are packed into
    // one number so a wrong iteration count cannot hide behind a right sum.
    let source = r#"
import { test, expect } from "vitest";

async function identity(n: number): Promise<number> {
  return n;
}

async function total(count: number): Promise<number> {
  let sum = 0;
  let iterations = 0;
  for (let i = 0; i < count; i++) {
    try {
      sum += await identity(i);
    } catch (err) {
      sum = -1;
    }
    iterations++;
  }
  return sum * 1000 + iterations;
}

test("an awaiting try/catch loop runs every iteration", async () => {
  expect(await total(20)).toBe(190020);
});
"#;
    run_fixture(source, "smelt_awaiting_loop_iterations");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_synchronous_try_catch_in_a_loop_still_catches() {
    // The synchronous half of the same defect: the loop-body emitters discarded a
    // `Call` terminator's `unwind` handler, so the `catch` block vanished and the
    // throwing call was emitted through the error-propagating `?` template. The
    // first failure escaped the function instead of being retried — and in a
    // non-throwing function the stray `?` did not even compile.
    let source = r#"
import { test, expect } from "vitest";

let calls = 0;

function flaky(n: number): number {
  calls++;
  if (n < 3) {
    throw new Error("nope");
  }
  return n;
}

function attempt(limit: number): number {
  for (let i = 0; i <= limit; i++) {
    try {
      return flaky(i);
    } catch (err) {
      // keep going
    }
  }
  return -1;
}

test("a synchronous try/catch inside a loop catches every iteration", () => {
  calls = 0;
  expect(attempt(5)).toBe(3);
  expect(calls).toBe(4);
  calls = 0;
  expect(attempt(1)).toBe(-1);
  expect(calls).toBe(2);
});
"#;
    run_fixture(source, "smelt_sync_try_catch_loop");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_catch_that_breaks_out_of_the_loop_still_breaks() {
    // The `Err` arm's continuation can also be the loop's exit block rather than
    // its header, so `emit_continuation` has to render `break` there. Without that
    // arm the exit region would be inlined inside the `catch` and then fall
    // through into a second copy of the loop body. `flaky` rejects on 2, so the
    // accumulated total must be 0 + 1 = 1 and not the full 0..9 sum.
    let source = r#"
import { test, expect } from "vitest";

async function flaky(n: number): Promise<number> {
  if (n === 2) {
    throw new Error("stop");
  }
  return n;
}

async function upTo(limit: number): Promise<number> {
  let seen = 0;
  for (let i = 0; i < limit; i++) {
    try {
      seen += await flaky(i);
    } catch (err) {
      break;
    }
  }
  return seen;
}

test("a catch clause can break out of the enclosing loop", async () => {
  expect(await upTo(10)).toBe(1);
});
"#;
    run_fixture(source, "smelt_catch_breaks_loop");
}
