//! Runtime execution tests for `arguments` arity and `Function.prototype.length`.
//!
//! A JavaScript `arguments` object is the **actual** argument list of the call, not
//! the function's declared parameter list, and the two differ constantly:
//!
//! ```js
//! function fn(_a, _b, _c) { return Array.from(arguments); }
//! ary(fn, 2)('a', 'b', 'c', 'd');   // fn('a', 'b')      -> ['a', 'b']
//! rest(fn)(1, 2, 3, 4);             // fn(1, 2, [3, 4])  -> [1, 2, [3, 4]]
//! ```
//!
//! Smelt used to emit such a function at its declared arity, and the erased-call
//! boundary padded a short call up to that arity — so `arguments` answered
//! `['a', 'b', null]` where JavaScript answers `['a', 'b']`. The padding cannot
//! simply be trimmed either: a real trailing `undefined` is a legitimate argument
//! (es-toolkit's `partial` placeholder specs expect `[undefined, 'b', undefined]`),
//! so the count has to travel with the call. A function that reads `arguments` is
//! therefore lowered variadically — see `lowering::arguments_forwarding`.
//!
//! `Function.prototype.length` is the other half. It is the *declared* arity, which
//! the variadic rewrite removes from the signature, and it has to survive erasure
//! to `SmeltUnknown::Function(Rc<…>)` where there is no field to hold it. Real code
//! branches on it: es-toolkit `rest(func)` defaults its split point to
//! `func.length - 1`, so an answer of `0` made it `-1`.
//!
//! String goldens (`a_function_reading_arguments_lowers_to_one_rest_parameter`,
//! `an_erased_callable_reports_its_function_length`) prove the wiring is emitted;
//! only running the program proves the counts.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test arguments_arity_runtime -- --ignored
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
        "generated arguments-arity test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-arguments-arity-runtime-{}-{seq}",
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
fn arguments_reflects_the_actual_call_not_the_declared_arity() {
    // Three counts against one three-parameter function: short, exact, and long.
    // The short case is what the declared-arity lowering got wrong (it answered a
    // padded `[a, b, null]`), and the long case proves the list is not truncated to
    // the declared arity either.
    let source = r"
import { test, expect } from 'vitest';

function callWith(fn: (...args: any[]) => any, args: unknown[]): unknown {
  return fn(...args);
}

function probe(_a: unknown, _b: unknown, _c: unknown): unknown[] {
  // eslint-disable-next-line prefer-rest-params
  return Array.from(arguments as any);
}

test('a short call is not padded to the declared arity', () => {
  expect(callWith(probe, ['a', 'b'])).toEqual(['a', 'b']);
});
test('an exact call is unchanged', () => {
  expect(callWith(probe, ['a', 'b', 'c'])).toEqual(['a', 'b', 'c']);
});
test('a long call is not truncated to the declared arity', () => {
  expect(callWith(probe, ['a', 'b', 'c', 'd'])).toEqual(['a', 'b', 'c', 'd']);
});
test('a real trailing undefined is still an argument', () => {
  expect(callWith(probe, ['a', undefined])).toEqual(['a', undefined]);
});
test('the declared parameters still bind positionally', () => {
  function named(a: unknown, b: unknown): unknown {
    // eslint-disable-next-line prefer-rest-params
    const all: unknown[] = Array.from(arguments as any);
    return [a, b, all.length];
  }
  expect(callWith(named, ['x', 'y', 'z'])).toEqual(['x', 'y', 3]);
});
";
    run_fixture(source, "smelt_arguments_actual_arity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn function_length_survives_erasure() {
    // `length` is the DECLARED arity, so it must not follow the variadic rewrite
    // down to the rest list — and it must survive the erasure boundary, where the
    // `SmeltErasedFunction` field that carried it is dropped.
    let source = r"
import { test, expect } from 'vitest';

function arity(func: unknown): number {
  return (func as any).length;
}

function two(_a: unknown, _b: unknown): unknown {
  return _a;
}

function readsArguments(_a: unknown, _b: unknown, _c: unknown): unknown {
  // eslint-disable-next-line prefer-rest-params
  return arguments;
}

function variadic(...args: unknown[]): unknown {
  return args;
}

test('an erased function reports its declared arity', () => {
  expect(arity(two)).toBe(2);
});
test('reading arguments does not change the reported arity', () => {
  expect(arity(readsArguments)).toBe(3);
});
test('a genuinely variadic function reports zero', () => {
  expect(arity(variadic)).toBe(0);
});
";
    run_fixture(source, "smelt_function_length_erasure");
}
