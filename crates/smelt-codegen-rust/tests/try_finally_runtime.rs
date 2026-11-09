//! Runtime execution tests for `finally` clauses on non-fall-through exits.
//!
//! MIR lowered a `try`/`catch`/`finally` by making the finalizer the *normal
//! exit* block of the `try` body: the body's last block fell through to it. A
//! `return` inside the body (or inside the `catch`) set a `Return` terminator
//! instead, so the fall-through edge was never taken and the finalizer was
//! **skipped entirely**.
//!
//! That silently broke every cleanup-on-return idiom. es-toolkit's
//! `areObjectsEqual` registers each visited pair in a recursion `Map` and clears
//! it in a `finally`, returning from inside the `try`; with the finalizer skipped
//! the stale entries survived, and the next comparison of the same pair took the
//! "already seen, therefore equal" shortcut. `isEqualWith({ constructor: [1] },
//! { constructor: ['1'] })` answered `true`.
//!
//! `lower_return` now re-lowers every enclosing finalizer inline ahead of the
//! `Return` terminator. Only executing the program proves the cleanup statements
//! actually run and run in the right order, which is what this tier does: each
//! case is a TypeScript Vitest test, lowering emits a `#[test]`, and a green
//! `cargo test` on the generated crate means every `expect(...)` held.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test try_finally_runtime -- --ignored
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
        "smelt-try-finally-runtime-{}-{seq}",
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
fn finally_runs_when_the_try_body_returns() {
    // The core defect: a `return` from the `try` body skipped the finalizer, so
    // `log` stayed empty and the cleanup never happened. The returned value must
    // still be the one the `try` computed.
    let source = r#"
import { test, expect } from "vitest";
test("a finalizer runs before a return from the try body", () => {
  const log: string[] = [];
  function withCleanup(): number {
    try {
      log.push("body");
      return 1;
    } finally {
      log.push("cleanup");
    }
  }
  expect(withCleanup()).toBe(1);
  expect(log.length).toBe(2);
  expect(log[0]).toBe("body");
  expect(log[1]).toBe("cleanup");
});
test("every early return runs the finalizer", () => {
  const log: string[] = [];
  function pick(n: number): string {
    try {
      if (n === 0) {
        return "zero";
      }
      if (n === 1) {
        return "one";
      }
      return "many";
    } finally {
      log.push("cleanup");
    }
  }
  expect(pick(0)).toBe("zero");
  expect(pick(1)).toBe("one");
  expect(pick(2)).toBe("many");
  expect(log.length).toBe(3);
});
"#;
    run_fixture(source, "smelt_finally_on_return");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn finally_cleanup_of_a_recursion_map_is_observable() {
    // The es-toolkit `areObjectsEqual` shape, reduced: register a pair in a Map,
    // return from inside the `try`, and clear the Map in the `finally`. With the
    // finalizer skipped the entries leaked and the "already visiting" shortcut
    // fired on a later, unrelated comparison.
    let source = r#"
import { test, expect } from "vitest";
test("a finalizer clears the recursion map even on return", () => {
  const seen = new Map<any, any>();
  const left: any[] = [1];
  const right: any[] = ["1"];
  function visit(): boolean {
    seen.set(left, right);
    seen.set(right, left);
    try {
      return false;
    } finally {
      seen.delete(left);
      seen.delete(right);
    }
  }
  expect(visit()).toBe(false);
  expect(seen.size).toBe(0);
  expect(seen.has(left)).toBe(false);
  expect(seen.has(right)).toBe(false);
});
"#;
    run_fixture(source, "smelt_finally_clears_recursion_map");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn nested_finalizers_run_innermost_first() {
    // Duplicating the finalizer per exit has to walk the whole lexical stack, in
    // the order JavaScript unwinds it: inner cleanup, then outer, then the return.
    let source = r#"
import { test, expect } from "vitest";
test("nested finalizers run inner-to-outer on a return", () => {
  const log: string[] = [];
  function nested(): string {
    try {
      try {
        return "value";
      } finally {
        log.push("inner");
      }
    } finally {
      log.push("outer");
    }
  }
  expect(nested()).toBe("value");
  expect(log.length).toBe(2);
  expect(log[0]).toBe("inner");
  expect(log[1]).toBe("outer");
});
test("a return in the catch clause also runs the finalizer", () => {
  const log: string[] = [];
  function caught(): string {
    try {
      throw new Error("boom");
    } catch {
      return "recovered";
    } finally {
      log.push("cleanup");
    }
  }
  expect(caught()).toBe("recovered");
  expect(log.length).toBe(1);
});
"#;
    run_fixture(source, "smelt_finally_nested_order");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_return_in_the_finalizer_overrides_the_try_body() {
    // JavaScript lets the finalizer's own `return` win. The inline duplication
    // stops as soon as the finalizer terminates the block, dropping the pending
    // return — and the value the `try` computed must not leak out.
    let source = r#"
import { test, expect } from "vitest";
test("the finalizer's return replaces the try body's", () => {
  function overridden(): string {
    try {
      return "try";
    } finally {
      return "finally";
    }
  }
  expect(overridden()).toBe("finally");
});
test("a finalizer that reassigns the returned binding does not change the result", () => {
  function frozen(): number {
    let value = 1;
    try {
      return value;
    } finally {
      value = 2;
    }
  }
  expect(frozen()).toBe(1);
});
test("the fall-through path still reaches the code after the try", () => {
  const log: string[] = [];
  function fallThrough(): number {
    let total = 0;
    try {
      total = 1;
    } finally {
      log.push("cleanup");
      total = total + 10;
    }
    return total;
  }
  expect(fallThrough()).toBe(11);
  expect(log.length).toBe(1);
});
"#;
    run_fixture(source, "smelt_finally_return_precedence");
}
