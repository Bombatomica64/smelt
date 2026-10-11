//! Runtime execution tests for type tests whose answer is only known at runtime.
//!
//! Two runtime type tests used to be answered at COMPILE time with a constant,
//! because the frontend folded them on an operand type that did not actually
//! settle the question:
//!
//! * `Array.isArray(value)` over an `Optional(Unknown)` parameter — the
//!   `value?: any` signature of es-toolkit's `isArray`. The fold exempted only
//!   bare `Unknown`/`TypeParam`/`Union`, so the helper emitted as
//!   `fn is_array(value: Option<SmeltUnknown>) -> bool { return false; }` and
//!   the array branch of `toCamelCaseKeys`/`toSnakeCaseKeys` became dead code,
//!   returning array elements unconverted.
//! * `typeof value === 'string'` inside an ARROW predicate. The callback
//!   lowering path folded with `type_matches_typeof` ("could any runtime
//!   variant match?"), which answers `true` for `number | string`, so
//!   `omitBy`/`pickBy` predicates became the constant `true`.
//!
//! Both failures are silent WRONG ANSWERS rather than emission failures, so a
//! string golden (`part_6_tests`) can prove the probe is emitted but not that
//! it selects the right arm. This tier executes the generated crate.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test runtime_type_test_runtime -- --ignored
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
        "generated runtime-type-test crate failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-runtime-type-test-{}-{seq}",
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
fn is_array_over_an_optional_erased_parameter_answers_at_runtime() {
    // `deepDouble` is the shape of `toCamelCaseKeys`: it recurses and depends on
    // the array branch being reachable. The negative cases pin the other
    // direction so a probe that always answered `true` would fail too.
    let source = r"
import { test, expect } from 'vitest';

function isArrayValue(value?: any): boolean {
  return Array.isArray(value);
}

function deepDouble(value: unknown): unknown {
  if (isArrayValue(value)) {
    return (value as unknown[]).map(item => deepDouble(item));
  }
  return ((value as number) * 2) as unknown;
}

test('an optional erased operand sees the array at runtime', () => {
  expect(isArrayValue([1, 2])).toBe(true);
  expect(isArrayValue([])).toBe(true);
});
test('non-arrays still answer false', () => {
  expect(isArrayValue('nope')).toBe(false);
  expect(isArrayValue(7)).toBe(false);
});
test('the array branch of a recursive converter runs', () => {
  expect(deepDouble([1, 2, 3])).toEqual([2, 4, 6]);
  expect(deepDouble([[1], [2]])).toEqual([[2], [4]]);
});
";
    run_fixture(source, "smelt_optional_is_array_probe");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn arrow_predicate_typeof_narrowing_answers_at_runtime() {
    // The `countBy` predicate fixture is the shape of es-toolkit's `omitBy`/`pickBy`:
    // a union-typed arrow predicate decides each entry. A predicate folded to a
    // constant counts every entry or none, which the three tests (expecting 3,
    // 3 and 2 of the five entries) catch.
    let source = r"
import { test, expect } from 'vitest';

function countBy(values: Array<number | string>, pred: (value: number | string) => boolean): number {
  let total = 0;
  for (let i = 0; i < values.length; i++) {
    if (pred(values[i])) {
      total++;
    }
  }
  return total;
}

test('an arrow-const predicate narrows the union at runtime', () => {
  const isString = (value: number | string) => typeof value === 'string';
  expect(countBy([1, 'a', 2, 'b', 'c'], isString)).toBe(3);
});
test('an inline arrow predicate narrows the union at runtime', () => {
  expect(countBy([1, 'a', 2, 'b', 'c'], (value: number | string) => typeof value === 'string')).toBe(3);
});
test('the negated predicate is the complement, not another constant', () => {
  expect(countBy([1, 'a', 2, 'b', 'c'], (value: number | string) => typeof value !== 'string')).toBe(2);
});
";
    run_fixture(source, "smelt_arrow_typeof_narrowing");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn typeof_object_answers_for_map_and_generator_values() {
    // A `Map` and a generator are objects to `typeof`, and every site that
    // answers a runtime-kind question about a concrete type now reads one map
    // (`Type::runtime_kind`). Before, separate copies disagreed (haiku audit
    // F0310): the frontend's `typeof` fold and narrowing, the concrete-union
    // arm filter, the union `typeof` string match and the union's erased
    // reconstruction all left `Map` out of "object". So:
    //
    // * `typeof v === 'object'` on `Map | string` folded to `false` and the
    //   Map branch was dead (`describe(m)` answered "str:[object Object]");
    // * `switch (typeof v)` matched the Map arm against "undefined";
    // * `typeof g === 'object'` on a generator folded to `false`.
    //
    // `fromErased` routes an erased Map back into a union whose Map arm is
    // not the last (fallback) arm, which is where a wrong reconstruction
    // pattern for that arm is visible. Expected values are node's.
    let source = r"
import { test, expect } from 'vitest';

function describe(v: Map<string, number> | string): string {
  if (typeof v === 'object') {
    return 'map:' + v.size;
  }
  return 'str:' + v;
}

function viaSwitch(v: Map<string, number> | string): string {
  switch (typeof v) {
    case 'object':
      return 'map:' + v.size;
    default:
      return 'other';
  }
}

function kindOf(v: Map<string, number> | number[] | boolean): string {
  if (typeof v === 'boolean') {
    return 'bool';
  }
  if (Array.isArray(v)) {
    return 'arr:' + v.length;
  }
  return 'map:' + v.size;
}

function fromErased(u: unknown): string {
  return kindOf(u as Map<string, number> | number[] | boolean);
}

function* counter(): Generator<number> {
  yield 1;
}

test('typeof a Map union arm is object', () => {
  const m = new Map<string, number>();
  m.set('a', 1);
  m.set('b', 2);
  expect(describe(m)).toBe('map:2');
  expect(describe('x')).toBe('str:x');
  expect(typeof m === 'object').toBe(true);
});
test('switch on typeof selects the Map arm', () => {
  const m = new Map<string, number>();
  m.set('a', 1);
  expect(viaSwitch(m)).toBe('map:1');
  expect(viaSwitch('x')).toBe('other');
});
test('an erased Map is reconstructed into the Map arm', () => {
  const m = new Map<string, number>();
  m.set('a', 1);
  m.set('b', 2);
  expect(fromErased(m)).toBe('map:2');
  expect(fromErased([1, 2, 3])).toBe('arr:3');
  expect(fromErased(true)).toBe('bool');
});
test('typeof a generator is object', () => {
  const g = counter();
  expect(typeof g === 'object').toBe(true);
  expect(typeof g).toBe('object');
});
";
    run_fixture(source, "smelt_typeof_object_map_generator");
}
