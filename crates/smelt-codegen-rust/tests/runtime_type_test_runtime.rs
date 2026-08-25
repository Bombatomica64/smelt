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
    emit_program(source, crate_name, &crate_dir);
    run_generated_tests(&crate_dir, &target_dir);
    drop(std::fs::remove_dir_all(&root));
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn is_array_over_an_optional_erased_parameter_answers_at_runtime() {
    // `deepConvert` is the shape of `toCamelCaseKeys`: it recurses and depends on
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
    // `omitStrings`/`pickStrings` are the shape of es-toolkit's `omitBy`/`pickBy`:
    // a union-typed arrow predicate decides each entry. A predicate folded to a
    // constant keeps everything or nothing, which both fixtures catch.
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
