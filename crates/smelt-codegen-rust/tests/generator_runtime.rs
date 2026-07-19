//! Runtime execution tests for TypeScript generator lowering.
//!
//! These fixtures exercise emitted crates so suspension, resumption, delegated
//! iteration, and completion ordering are validated beyond HIR/source shape.

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

/// Lower a TypeScript Vitest fixture and emit a runnable generated crate.
fn emit_program(source: &str, crate_name: &str, crate_dir: &Path) {
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).expect("HIR lowering");
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering");
    smelt_mir::opt::optimize(&mut mir);
    let options = EmitOptions::new(crate_name.to_owned()).with_crate_kind(CrateKind::Program);
    emit_crate(&mir, crate_dir, &options).expect("crate emission");
}

/// Compile and execute every generated test in a fixture crate.
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
        "generated generator test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Return a unique scratch root for a generated fixture.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-generator-runtime-{}-{seq}",
        std::process::id()
    ))
}

/// Emit and run a generator fixture, then remove its scratch directory.
fn run_generator_fixture(source: &str, crate_name: &str) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    emit_program(source, crate_name, &crate_dir);
    run_generated_tests(&crate_dir, &target_dir);
    if std::env::var_os("SMELT_KEEP_GENERATOR_FIXTURE").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn synchronous_generators_suspend_resume_and_delegate_exactly_once() {
    let source = r#"
import { test, expect } from "vitest";

function* inner(): Generator<number, string, unknown> {
  yield 10;
  yield 20;
  return "done";
}

function* outer(): Generator<number, string, unknown> {
  const result = yield* inner();
  yield 30;
  return result;
}

test("generator suspension", () => {
  const iterator = outer();
  const first = iterator.next();
  expect(first.done).toBe(false);
  expect(first.value).toBe(10);
  const second = iterator.next();
  expect(second.done).toBe(false);
  expect(second.value).toBe(20);
  const third = iterator.next();
  expect(third.done).toBe(false);
  expect(third.value).toBe(30);
  const fourth = iterator.next();
  expect(fourth.done).toBe(true);
  expect(fourth.value).toBe("done");
});
"#;
    run_generator_fixture(source, "smelt_generator_runtime_sync");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn built_in_arrays_and_strings_are_typed_delegates() {
    let source = r#"
import { test, expect } from "vitest";

function* values(): Generator<number, void, unknown> {
  yield* [1, 2];
}

function* letters(): Generator<string, void, unknown> {
  yield* "ab";
}

function* setValues(): Generator<number, void, unknown> {
  yield* new Set<number>([3, 4]);
}

function* tupleValues(): Generator<number, void, unknown> {
  const pair: [number, number] = [5, 6];
  yield* pair;
}

test("built-in delegates", () => {
  const numbers = values();
  expect(numbers.next().value).toBe(1);
  expect(numbers.next().value).toBe(2);
  expect(numbers.next().done).toBe(true);

  const text = letters();
  expect(text.next().value).toBe("a");
  expect(text.next().value).toBe("b");
  expect(text.next().done).toBe(true);

  const setIterator = setValues();
  expect(setIterator.next().value).toBe(3);
  expect(setIterator.next().value).toBe(4);
  expect(setIterator.next().done).toBe(true);

  const tupleIterator = tupleValues();
  expect(tupleIterator.next().value).toBe(5);
  expect(tupleIterator.next().value).toBe(6);
  expect(tupleIterator.next().done).toBe(true);
});
"#;
    run_generator_fixture(source, "smelt_generator_runtime_builtins");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn unannotated_generator_infers_concrete_yield_and_return_types() {
    let source = r#"
import { test, expect } from "vitest";

function* inferred() {
  yield 7;
  return "ok";
}

test("inferred generator", () => {
  const iterator = inferred();
  expect(iterator.next().value).toBe(7);
  const complete = iterator.next();
  expect(complete.done).toBe(true);
  expect(complete.value).toBe("ok");
});
"#;
    run_generator_fixture(source, "smelt_generator_runtime_inferred");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn unannotated_async_generator_preserves_async_resume_protocol() {
    let source = r#"
import { test, expect } from "vitest";

async function* inferredAsync() {
  yield 8;
  return "async-ok";
}

test("inferred async generator", async () => {
  const iterator = inferredAsync();
  const first = await iterator.next();
  expect(first.done).toBe(false);
  expect(first.value).toBe(8);
  const complete = await iterator.next();
  expect(complete.done).toBe(true);
  expect(complete.value).toBe("async-ok");
});
"#;
    run_generator_fixture(source, "smelt_generator_runtime_inferred_async");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn heterogeneous_union_delegation_selects_each_arm_protocol() {
    let source = r#"
import { test, expect } from "vitest";

function* syncArm(): Generator<number, string, unknown> {
  yield 11;
  return "sync-done";
}

async function* asyncArm(): AsyncGenerator<string, number, unknown> {
  yield "async-yield";
  return 12;
}

function choose(flag: boolean): Generator<number, string, unknown> | AsyncGenerator<string, number, unknown> {
  return flag ? syncArm() : asyncArm();
}

async function* delegated(flag: boolean): AsyncGenerator<number | string, string | number, unknown> {
  return yield* choose(flag);
}

test("union generator protocols", async () => {
  const sync = delegated(true);
  expect((await sync.next()).value).toBe(11);
  expect((await sync.next()).value).toBe("sync-done");

  const asyncIterator = delegated(false);
  expect((await asyncIterator.next()).value).toBe("async-yield");
  expect((await asyncIterator.next()).value).toBe(12);
});
"#;
    run_generator_fixture(source, "smelt_generator_runtime_union");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn generator_throw_occurs_only_after_the_yield_is_resumed() {
    let source = r#"
import { test, expect } from "vitest";

function* delayedThrow(): Generator<number, void, unknown> {
  yield 1;
  throw new Error("boom");
}

test("delayed generator throw", () => {
  const iterator = delayedThrow();
  expect(iterator.next().value).toBe(1);
  expect(() => iterator.next()).toThrow("boom");
});
"#;
    run_generator_fixture(source, "smelt_generator_runtime_delayed_throw");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn annotated_non_void_generator_fallthrough_completes_with_undefined() {
    let source = r#"
import { test, expect } from "vitest";

function* fallsThrough(): Generator<number, string, unknown> {
  yield 1;
}

test("generator fallthrough", () => {
  const iterator = fallsThrough();
  expect(iterator.next().value).toBe(1);
  const complete = iterator.next();
  expect(complete.done).toBe(true);
  expect(complete.value).toBeUndefined();
});
"#;
    run_generator_fixture(source, "smelt_generator_runtime_fallthrough");
}
