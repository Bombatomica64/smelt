//! Runtime execution tests for TypeScript ambient (`declare`) value declarations.
//!
//! A `declare let x: T` asserts that the HOST already provides `x`; it never
//! creates a binding, so a read resolves to the host's value and never to a
//! default of the declared type. es-toolkit's `isNode` (`declare let process:
//! {…} | undefined`) is the real-world instance.
//!
//! Two properties are guarded here.
//!
//! **An ambient declaration does not shadow the host binding.** Declaring
//! `process` ambiently must leave the profile's modeled `process` object visible,
//! so a Node-detection probe reads the profile's answer rather than a fabricated
//! `undefined`.
//!
//! **A statically decided guard does not evaluate its dead operand.** Once the
//! fake binding is gone, `typeof window !== 'undefined' && window?.document` can
//! only be lowered if the dead right operand is never visited — the non-DOM
//! profile provides no `window` on purpose. The observable half of that is
//! ordinary JavaScript short-circuiting, asserted here with a side-effecting
//! operand so the test fails if the dead branch is evaluated.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test ambient_declaration_runtime -- --ignored
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
        "smelt-ambient-declaration-runtime-{}-{seq}",
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
fn an_ambient_declaration_does_not_shadow_the_host_binding() {
    // Verbatim shape of es-toolkit's `isNode`. Before the fix the ambient
    // declaration became a module local initialized to `None`, so the `typeof`
    // guard folded to `false` and the function returned `false` in the
    // Node-compatible profile it is compiled for. No concrete type, union or
    // scoped generic can express this: the declaration is a claim ABOUT the host,
    // and the only right answer is to not create a binding at all.
    let source = r"
import { test, expect } from 'vitest';

declare let process:
  | {
      versions?: {
        node?: unknown;
      };
    }
  | undefined;

export function isNode(): boolean {
  return typeof process !== 'undefined' && process?.versions?.node != null;
}

test('an ambiently declared host global is not replaced by a default value', () => {
  expect(isNode()).toBe(true);
});
";
    run_fixture(source, "smelt_ambient_declaration_host_binding");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_statically_decided_guard_skips_its_dead_operand() {
    // `isBrowser`'s shape. The profile is non-DOM, so the guard is statically
    // false and the right operand is dead; the second case proves the general
    // short-circuit rather than the specific fold, by giving the dead operand an
    // observable side effect.
    let source = r"
import { test, expect } from 'vitest';

declare let window:
  | {
      document: unknown;
    }
  | undefined;

export function isBrowser(): boolean {
  return typeof window !== 'undefined' && window?.document != null;
}

let calls = 0;
function tick(): boolean {
  calls += 1;
  return true;
}

test('a dead operand behind an absent-global guard is not evaluated', () => {
  expect(isBrowser()).toBe(false);
});
test('&& does not evaluate its right operand when the left is falsy', () => {
  calls = 0;
  expect(false && tick()).toBe(false);
  expect(calls).toBe(0);
});
";
    run_fixture(source, "smelt_ambient_declaration_dead_operand");
}
