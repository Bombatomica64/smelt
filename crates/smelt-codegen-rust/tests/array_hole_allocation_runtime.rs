//! Runtime execution tests for `Array(n)` hole allocation.
//!
//! `Array(n)` and `new Array(n)` do not build an EMPTY array in JavaScript:
//! they allocate an array of LENGTH `n` whose slots are holes, so `Array(3)`
//! has `.length === 3` and `Array(3)[0]` is `undefined`. The emitter used to
//! lower both spellings to `vec![]` and rely on later indexed writes to grow
//! the list, which silently dropped the length. Every consumer that drives a
//! loop off `.length` therefore ran zero iterations and returned an empty
//! array — `fill(Array(3), 2)`, `zip`, `zipWith` and `unzip` all failed this
//! way, looking like four unrelated array bugs.
//!
//! The companion property is that a hole and an out-of-range read agree: both
//! answer the element type's missing value, so `Array(3)[0]` and `[][0]`
//! cannot disagree for the same list type. Golden string assertions live in
//! `part_4_tests.rs`; only running the program proves the lengths, the loop
//! bodies, and the hole values.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test array_hole_allocation_runtime -- --ignored
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
        "generated array-hole test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-array-hole-runtime-{}-{seq}",
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
fn array_length_allocation_has_that_length() {
    // `Array(n)` / `new Array(n)` allocate LENGTH `n`, and a read of an
    // unwritten slot of an erased list is `undefined` — the same answer an
    // out-of-range read gives. Previously both lengths were `0`.
    let source = r"
import { test, expect } from 'vitest';

function bareLength(n: number): number {
  return Array(n).length;
}

function newLength(n: number): number {
  return new Array(n).length;
}

function holeValue(): unknown {
  const holes = Array(3);
  return holes[0];
}

test('a bare Array(n) has length n', () => {
  expect(bareLength(3)).toBe(3);
  expect(bareLength(0)).toBe(0);
});
test('new Array(n) has length n', () => {
  expect(newLength(2)).toBe(2);
});
test('a hole reads as undefined', () => {
  expect(holeValue()).toBeUndefined();
});
";
    run_fixture(source, "smelt_array_hole_length");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn length_driven_loops_over_an_allocated_array_run() {
    // The defect this fix exists for: `fill`, `zip` and `unzip` all size their
    // result with `Array(n)` and then drive a loop off its `.length`. With an
    // empty allocation those loops ran zero times and the functions returned
    // empty arrays.
    //
    // `zip` is generic here because that is es-toolkit's real signature, and
    // because the element type matters: a miss on an ERASED element list is
    // `undefined`, but a miss on a concretely typed one (`b: string[]`) is that
    // type's missing value (`String::new()`), which then erases to `''` rather
    // than `undefined` when it flows into an `unknown` slot. That gap is in the
    // read/coercion path, not in construction, and is deliberately not covered
    // here.
    let source = r"
import { test, expect } from 'vitest';

function fill(arr: unknown[], value: unknown): unknown[] {
  for (let i = 0; i < arr.length; i++) {
    arr[i] = value;
  }
  return arr;
}

function zip<T, U>(a: readonly T[], b: readonly U[]): unknown[][] {
  const rowCount = a.length > b.length ? a.length : b.length;
  const result = Array(rowCount);
  for (let i = 0; i < rowCount; i++) {
    const row = Array(2);
    row[0] = a[i];
    row[1] = b[i];
    result[i] = row;
  }
  return result;
}

test('fill covers every allocated slot', () => {
  expect(fill(Array(3), 2)).toEqual([2, 2, 2]);
});
test('zip pads the short array with undefined', () => {
  expect(zip([1, 2, 3], ['a', 'b'])).toEqual([[1, 'a'], [2, 'b'], [3, undefined]]);
});
";
    run_fixture(source, "smelt_array_hole_loops");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_argument_list_that_is_not_a_length_builds_elements() {
    // ECMAScript splits on the ARGUMENT LIST, not the callee: exactly one
    // numeric argument is a length, and everything else is an element list.
    // `Array('a')` and `Array(1, 2, 3)` were rejected outright before ("length
    // must be numeric" / "at most one length argument").
    let source = r"
import { test, expect } from 'vitest';

function single(): string[] {
  return Array('a');
}

function several(): number[] {
  return Array(1, 2, 3);
}

function empty(): unknown[] {
  return Array();
}

test('a single non-numeric argument is one element', () => {
  expect(single()).toEqual(['a']);
});
test('several arguments are the elements', () => {
  expect(several()).toEqual([1, 2, 3]);
});
test('no arguments is an empty array', () => {
  expect(empty().length).toBe(0);
});
";
    run_fixture(source, "smelt_array_element_arguments");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_concretely_typed_allocation_stays_concrete() {
    // A hole in a `Vec<f64>` cannot be `undefined`, and it must NOT force the
    // list through `SmeltUnknown` either. The contextual list type is adopted
    // so the holes are the element type's own missing value — the same value
    // an out-of-range read of that list answers, which is the consistency
    // property the whole design rests on.
    let source = r"
import { test, expect } from 'vitest';

function numbers(n: number): number[] {
  const result: number[] = Array(n);
  return result;
}

function readsAgree(): boolean {
  const allocated: number[] = Array(2);
  const empty: number[] = [];
  return allocated[0] === empty[5];
}

function written(n: number): number[] {
  const result: number[] = Array(n);
  for (let i = 0; i < n; i++) {
    result[i] = i * 2;
  }
  return result;
}

test('a typed allocation keeps its length', () => {
  expect(numbers(3).length).toBe(3);
});
test('a hole equals an out-of-range read of the same list type', () => {
  expect(readsAgree()).toBe(true);
});
test('writes over an allocated typed list land in place', () => {
  expect(written(3)).toEqual([0, 2, 4]);
});
";
    run_fixture(source, "smelt_array_hole_typed");
}
