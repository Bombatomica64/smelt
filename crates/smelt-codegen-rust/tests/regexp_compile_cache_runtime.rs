//! Runtime execution tests for the memoized regex compiler in the generated prelude.
//!
//! A JavaScript `RegExp` object is built once — typically at module evaluation,
//! from a literal — and reused. The TypeScript frontend instead inlines a `const`
//! initializer into every referencing body, so the generated
//! `SmeltRegExp::new(source, flags)` construction is pasted at each use site and
//! the pattern used to be recompiled on every call. Compiling a Unicode-property
//! pattern dominates the cost of a function that otherwise does almost nothing
//! (es-toolkit's `words`/`camelCase`/`kebabCase`).
//!
//! The wrapper itself, however, cannot be shared. `SmeltRegExp` carries a JS
//! reference identity (`id`) and the observable, mutable `lastIndex` slot, and
//! `Clone` preserves *both* (the slot is an `Rc<RefCell<usize>>`), so handing every
//! use a clone of one cached instance would fuse distinct source objects and let
//! one call site's `/g` scan position leak into another's. Only the pure half is
//! shared: the compiled `fancy_regex` automaton, a function of the pattern text
//! alone, memoized per thread in `SMELT_REGEX_CACHE`.
//!
//! These tests pin the part a string golden cannot: that after the memo, `/g`
//! scanning still behaves like JavaScript. `lastIndex` must advance across
//! successive `exec` calls on ONE object, must be independent between two
//! *different* objects that happen to share a pattern (the case the memo could
//! plausibly have broken), and a caller-written `lastIndex` must still steer the
//! next scan.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test regexp_compile_cache_runtime -- --ignored
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
        "smelt-regexp-compile-cache-runtime-{}-{seq}",
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
fn sharing_a_compiled_pattern_keeps_last_index_per_object() {
    let source = r"
import { test, expect } from 'vitest';

test('lastIndex advances across exec calls on one object', () => {
  const scanner = /[0-9]+/g;
  const first = scanner.exec('a12b345');
  expect(first !== null).toBe(true);
  expect(scanner.lastIndex).toBe(3);
  const second = scanner.exec('a12b345');
  expect(second !== null).toBe(true);
  expect(scanner.lastIndex).toBe(7);
  const third = scanner.exec('a12b345');
  expect(third === null).toBe(true);
});

test('two objects sharing a pattern keep independent lastIndex', () => {
  const left = /[0-9]+/g;
  const right = /[0-9]+/g;
  left.exec('a12b345');
  expect(left.lastIndex).toBe(3);
  expect(right.lastIndex).toBe(0);
  right.exec('a12b345');
  expect(right.lastIndex).toBe(3);
  left.exec('a12b345');
  expect(left.lastIndex).toBe(7);
  expect(right.lastIndex).toBe(3);
});

test('a written lastIndex steers the next scan', () => {
  const scanner = /[0-9]+/g;
  scanner.lastIndex = 4;
  const matched = scanner.exec('a12b345');
  expect(matched !== null).toBe(true);
  expect(scanner.lastIndex).toBe(7);
});

test('repeated matching with the same pattern is stable', () => {
  const words = 'alpha1beta22gamma333';
  const pattern = /[a-z]+|[0-9]+/g;
  const parts = words.match(pattern);
  expect(parts !== null).toBe(true);
  const again = words.match(/[a-z]+|[0-9]+/g);
  expect(again !== null).toBe(true);
});
";
    run_fixture(source, "smelt_regexp_compile_cache");
}
