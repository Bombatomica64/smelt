//! Runtime execution tests for static properties on function declarations.
//!
//! JavaScript functions are objects, so a module can hang a value off one. It is
//! how es-toolkit publishes its placeholder sentinels — `partial.placeholder`,
//! `partialRight.placeholder`, `curry.placeholder`, `curryRight.placeholder`,
//! `bind.placeholder`, `bindKey.placeholder` — and `memoize.Cache = Map`.
//!
//! The sentinel must read back as itself for `partial(fn, placeholder, 'b',
//! placeholder)` to skip the placeholder slots.
//!
//! What matters at runtime is IDENTITY: the value read through the member spelling,
//! the value read through destructuring, and the value the function's own body
//! compares against must all be one thing. A string golden
//! (`a_static_property_on_a_function_declaration_resolves`) proves the read
//! resolves; only running the program proves the three agree.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test function_statics_runtime -- --ignored
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
        "smelt-function-statics-runtime-{}-{seq}",
        std::process::id()
    ))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_fixture(source: &str, crate_name: &str) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    let outcome = std::panic::catch_unwind(|| {
        emit_program(source, crate_name, &crate_dir);
        run_generated_tests(source, &crate_dir);
    });
    // The scratch root holds the emitted crate (its build lives in the
    // harness shared target directories); it is removed on the FAILURE path
    // too, rather than left behind in `/tmp` once per failing case.
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
fn a_function_static_keeps_one_identity_across_spellings() {
    // The sentinel has to be the SAME value however it is reached, because the
    // consumer compares against it by identity. The last case is the shape that
    // actually failed: the function's own body reads its own static property and
    // compares it to the argument the caller passed.
    let source = r"
import { test, expect } from 'vitest';

function pick(value: unknown): unknown {
  return value === pick.sentinel ? 'sentinel' : value;
}

pick.sentinel = Symbol('pick.sentinel');
pick.fallback = 'none';

test('the member spelling and destructuring agree', () => {
  const { sentinel } = pick;
  expect(sentinel).toBe(pick.sentinel);
});
test('a non-symbol static resolves too', () => {
  expect(pick.fallback).toBe('none');
});
test('the function body recognizes its own sentinel', () => {
  expect(pick(pick.sentinel)).toBe('sentinel');
  expect(pick('a')).toBe('a');
});
test('two different statics are distinct', () => {
  expect(pick.sentinel === (pick.fallback as unknown)).toBe(false);
});
";
    run_fixture(source, "smelt_function_static_identity");
}
