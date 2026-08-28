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
        "generated RegExp lastIndex test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

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
    emit_program(source, crate_name, &crate_dir);
    run_generated_tests(&crate_dir, &target_dir);
    drop(std::fs::remove_dir_all(&root));
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
