//! Runtime execution tests for the "erased value -> typed list" conversion.
//!
//! Rebuilding a `SmeltList<T>` from a `SmeltUnknown` is one operation: the arms
//! come from a single emitter helper, so every element type accepts exactly what
//! JavaScript's iteration protocol accepts, whether the element type is erased
//! (`groupBy`'s adapter) or concrete (`map`'s).
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test erased_to_list_arms_runtime -- --ignored
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
        "smelt-erased-list-runtime-{}-{seq}",
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
fn a_string_iterates_into_a_list_whose_element_type_is_concrete() {
    // `countTokens` declares `(string | number)[]`, so the erased adapter rebuilds
    // its argument through the GENERAL element-type lowering rather than the
    // erased-element or all-string ones. Handing it a string has to iterate the
    // string's characters, exactly as the sibling lowerings already did; before
    // the arm sets were unified this panicked with "unknown is not array".
    let source = r#"
import { test, expect } from "vitest";

type Token = string | number;

function countTokens(tokens: Token[]): number {
  return tokens.length;
}

function callErased(fn: unknown, value: unknown): unknown {
  const callable = fn as (tokens: unknown) => unknown;
  return callable(value);
}

test("a string iterates into a list whose element type is concrete", () => {
  expect(callErased(countTokens, "abc")).toBe(3);
  expect(callErased(countTokens, [1, "b"])).toBe(2);
  expect(callErased(countTokens, undefined)).toBe(0);
});
"#;
    run_fixture(source, "erased_to_list_string_arm");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_array_extracted_from_an_erased_value_aliases_the_source_array() {
    // A JavaScript array is a reference value, so the `unknown[]` a callee
    // receives across an erased boundary IS the caller's array — a `push`
    // through the callee's handle must be visible to the caller.
    //
    // `SmeltList<SmeltUnknown>` and `SmeltArray` have the identical
    // `id` + `Rc<RefCell<Vec<SmeltUnknown>>>` representation, so the extraction
    // is a re-wrap of one array and not the construction of a second, which also
    // keeps each crossing O(1) for a per-callback erased dispatcher.
    let source = r#"
import { test, expect } from "vitest";

function pushInto(values: unknown[]): number {
  values.push(99);
  return values.length;
}

function callErased(fn: unknown, value: unknown): unknown {
  const callable = fn as (values: unknown) => unknown;
  return callable(value);
}

test("an array extracted from an erased value aliases the source array", () => {
  const values: unknown[] = [1, 2, 3];
  expect(callErased(pushInto, values)).toBe(4);
  expect(values.length).toBe(4);
  expect(values[3]).toBe(99);
});

test("a non-array source still builds a fresh list", () => {
  // Only the array arm has a source array to alias; every other arm builds a
  // list that did not exist in the source program and so mints a fresh identity.
  const built: unknown[] = [];
  expect(callErased(pushInto, "ab")).toBe(3);
  expect(callErased(pushInto, undefined)).toBe(1);
  expect(built.length).toBe(0);
});
"#;
    run_fixture(source, "erased_to_list_aliasing");
}
