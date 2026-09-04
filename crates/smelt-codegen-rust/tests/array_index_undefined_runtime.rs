//! Runtime execution tests for JavaScript out-of-range element reads.
//!
//! A JavaScript array read is **total**: `arr[5]` on a three-element array and
//! `arr[-1]` on any array are both `undefined`, never an error. The generated
//! Rust used to disagree in two different ways at once, and `last([])` showed
//! both:
//!
//! * the normalized index went through
//!   `usize::try_from(normalized).expect("negative index out of bounds")`, so a
//!   still-negative index aborted the program instead of missing; and
//! * `arr[i]` has TypeScript type `T`, so a source signature of
//!   `T | undefined` re-wrapped the read as
//!   `Some(arr.get(..).cloned().unwrap_or(Default::default()))` — answering
//!   `Some(0.0)` where JavaScript answers `undefined`.
//!
//! Golden string assertions live in `part_7_tests.rs`; they prove the shape is
//! emitted. Only running the program proves `last([])` is actually `None` and
//! that no read panics.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test array_index_undefined_runtime -- --ignored
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
        "generated array-index test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-array-index-runtime-{}-{seq}",
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
fn an_out_of_range_element_read_is_undefined() {
    let source = r"
import { test, expect } from 'vitest';

function last<T>(arr: readonly T[]): T | undefined {
  return arr[arr.length - 1];
}

function head<T>(arr: readonly T[]): T | undefined {
  return arr[0];
}

function total(arr: number[]): number {
  let sum = 0;
  for (let i = 0; i < arr.length; i++) {
    sum += arr[i];
  }
  return sum;
}

test('an empty array has no last element', () => {
  expect(last([])).toBeUndefined();
  expect(last([1, 2, 3])).toBe(3);
});
test('an empty array has no first element', () => {
  expect(head([])).toBeUndefined();
  expect(head(['a', 'b'])).toBe('a');
});
test('an in-range read is unaffected', () => {
  expect(total([1, 2, 3])).toBe(6);
});
";
    run_fixture(source, "smelt_array_index_undefined");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_negative_element_read_does_not_count_back_from_the_end() {
    // The half of this module's own premise that was still false. A negative
    // subscript is a PROPERTY key in JavaScript, not a position: only
    // `Array.prototype.at` counts back from the end. The shared index
    // normalizer applied Python's `len + index` wrap to both frontends, so
    // `arr[-1]` answered `arr[arr.length - 1]` — a plausible WRONG VALUE with
    // no panic, which is exactly the defect class this tier exists to catch.
    //
    // The indices below are the ones es-toolkit's `at` normalizes to before it
    // reads: `at(['a','b','c'], [-4])` adds the length, reaching `-1`, and must
    // then miss rather than wrap around a second time to `'c'`.
    //
    // A string-keyed read is asserted alongside so the two cannot drift: the
    // wrap made `arr[-1]` and `arr['-1']` answer differently for the same key.
    let source = r"
import { test, expect } from 'vitest';

function read(arr: readonly number[], index: number): number | undefined {
  return arr[index];
}

function readNested(rows: number[][], row: number, column: number): number | undefined {
  return rows[row][column];
}

test('a negative index addresses no element', () => {
  expect(read([10, 20, 30], -1)).toBeUndefined();
  expect(read([10, 20, 30], -3)).toBeUndefined();
  expect(read([10, 20, 30], -100)).toBeUndefined();
});
test('at() still counts back from the end', () => {
  const values = [10, 20, 30];
  expect(values.at(-1)).toBe(30);
  expect(values.at(-3)).toBe(10);
  expect(values.at(-4)).toBeUndefined();
});
test('an in-range read is unaffected', () => {
  expect(read([10, 20, 30], 0)).toBe(10);
  expect(read([10, 20, 30], 2)).toBe(30);
  expect(read([10, 20, 30], 3)).toBeUndefined();
  expect(readNested([[1, 2], [3, 4]], 1, 0)).toBe(3);
  expect(readNested([[1, 2], [3, 4]], 1, -1)).toBeUndefined();
});
";
    run_fixture(source, "smelt_negative_element_read");
}
