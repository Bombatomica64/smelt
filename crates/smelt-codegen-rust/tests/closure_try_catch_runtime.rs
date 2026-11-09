//! Runtime execution tests for `try`/`catch` inside a closure body.
//!
//! A function nested inside another function body lowers to a MIR closure, and
//! closure bodies are rendered by their own recursive emitter,
//! `emit_closure_block_inner`. Its `Call` and `Await` arms matched `unwind: _`
//! and threw the exception handler away. The call then went through
//! `closure_call_text_for_dest`, whose only difference from the function-level
//! path is that it rewrites a trailing `?` into
//! `unwrap_or_else(|error| panic!(..))`.
//!
//! So a `try`/`catch` inside a closure did not merely mis-structure — it did not
//! run at all. The first throw aborted the process through a Rust `panic!` where
//! JavaScript would have entered the `catch` and carried on. The loop around it
//! was emitted correctly the whole time, which is exactly why a string golden
//! asserting on `loop {` would have looked healthy: only executing the program
//! shows the handler is missing.
//!
//! Each case is a TypeScript Vitest test, lowering emits a `#[test]`, and a
//! green `cargo test` on the generated crate means every `expect(...)` held.
//! The helpers are declared **inside** a function body on purpose — that is the
//! shape that lowers to a closure, and the shape the sibling
//! `loop_await_runtime.rs` tier had to avoid because of this defect.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test closure_try_catch_runtime -- --ignored
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
        "smelt-closure-try-catch-runtime-{}-{seq}",
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
fn a_closure_body_retry_loop_catches_and_retries() {
    // `attempt` is declared inside the test body, so it lowers to a closure.
    // Before the fix the `catch` was absent and the first throw panicked, so this
    // test could not reach its first assertion at all. The call count is asserted
    // as well as the value: it is what proves the `catch` arm went back round the
    // loop rather than exiting it.
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

test("a try/catch inside a closure body retries", () => {
  const attempt = (limit: number): number => {
    for (let i = 0; i <= limit; i++) {
      try {
        return flaky(i);
      } catch (err) {
        // keep going
      }
    }
    return -1;
  };
  calls = 0;
  expect(attempt(5)).toBe(3);
  expect(calls).toBe(4);
  calls = 0;
  expect(attempt(1)).toBe(-1);
  expect(calls).toBe(2);
});
"#;
    run_fixture(source, "smelt_closure_retry_loop");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_closure_catch_observes_the_thrown_payload() {
    // The `Ok(Err(..))` arm of the shared throwing emitter is the real Smelt error
    // channel, so a `catch` binding in a closure observes the payload.
    //
    // `err.message` is `"kaboom"` because the thrown operand is the whole `Error`
    // object. `String(err)` is `"Error: kaboom"` because an erased error stringifies
    // through `Error.prototype.toString`. Both are non-empty only if the Smelt error
    // channel ran at all.
    let source = r#"
import { test, expect } from "vitest";

function boom(): number {
  throw new Error("kaboom");
}

test("a catch binding inside a closure sees the thrown payload", () => {
  const run = (): string => {
    try {
      boom();
      return "no throw";
    } catch (err: any) {
      return String(err);
    }
  };
  const message = (): string => {
    try {
      boom();
      return "no throw";
    } catch (err: any) {
      return err.message;
    }
  };
  expect(run()).toBe("Error: kaboom");
  expect(message()).toBe("kaboom");
});
"#;
    run_fixture(source, "smelt_closure_catch_payload");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_closure_try_catch_without_a_loop_still_catches() {
    // The handler was dropped unconditionally, not only inside loops, so the
    // straight-line shape is its own case. `total` must be 2 (one success, one
    // caught failure contributing nothing) rather than a panic.
    let source = r#"
import { test, expect } from "vitest";

function halve(n: number): number {
  if (n % 2 !== 0) {
    throw new Error("odd");
  }
  return n / 2;
}

test("a straight-line try/catch inside a closure catches", () => {
  const safeHalve = (n: number): number => {
    try {
      return halve(n);
    } catch (err) {
      return 0;
    }
  };
  expect(safeHalve(4)).toBe(2);
  expect(safeHalve(3)).toBe(0);
});
"#;
    run_fixture(source, "smelt_closure_straight_line_catch");
}
