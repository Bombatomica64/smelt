//! Runtime execution tests for calls that reach a value through an erased type.
//!
//! Two defects in the same family — a callable's *runtime* identity being traded
//! for a *static* claim about it — are guarded here.
//!
//! **A chained call through an `any` result must not vanish.** `f(..)(..)` where
//! `f`'s declared return type is `any` has no static function type for the outer
//! call, but the value is still callable: JavaScript looks the call up on the
//! value. Lowering the outer call to `undefined` discarded it, its arguments'
//! side effects, and left every assertion over the result comparing the wrong
//! value.
//!
//! **A variadic implementation keeps its arity behind a fixed-arity overload.**
//! TypeScript overloads check *arguments*; the value a call produces is whatever
//! the single implementation body returned. When the implementation returns
//! `(...args: any[]) => R` and the matched overload declares `(a, b) => R`,
//! adopting the overload's shape forces a two-argument Rust closure around a
//! rest-parameter runtime value — which drops surplus arguments and reports the
//! declared arity from `Function.length` instead of the callable's own `0`.
//! es-toolkit's `partial`/`partialRight` are the real-world instance: their docs
//! say outright that a partially applied function has `length === 0`.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test erased_call_dispatch_runtime -- --ignored
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
        "generated erased-call-dispatch test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-erased-call-dispatch-runtime-{}-{seq}",
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
fn a_call_through_an_erased_call_result_still_happens() {
    // Every assertion here failed by *construction* before the fix: the outer
    // call lowered to the literal `undefined`, so `expect(makeAdder(2)(3))`
    // compared `undefined` against `5`. The counter case proves the arguments
    // are evaluated exactly once rather than dropped with the call.
    let source = r"
import { test, expect } from 'vitest';

function makeAdder(a: number): any {
  return (b: number) => a + b;
}

let calls = 0;
function tick(): number {
  calls += 1;
  return calls;
}

test('the outer call runs and returns its value', () => {
  expect(makeAdder(2)(3)).toBe(5);
});
test('the outer call evaluates its arguments exactly once', () => {
  calls = 0;
  expect(makeAdder(10)(tick())).toBe(11);
  expect(calls).toBe(1);
});
";
    run_fixture(source, "smelt_erased_call_result_dispatch");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_variadic_implementation_keeps_its_arity_behind_a_fixed_arity_overload() {
    // The first overload matches `applyFirst(fn, 'a')` and declares a
    // two-parameter result; the implementation returns a rest-parameter closure.
    // A concrete union/tuple type cannot express "exactly the callable the body
    // produced", and a scoped generic cannot either — the overload is the only
    // thing that names two parameters, and it is the thing that is wrong about
    // the value — so the implementation's own return type is the representation
    // that has to survive.
    let source = r"
import { test, expect } from 'vitest';

export function applyFirst<T1, T2, T3, R>(
  func: (t1: T1, t2: T2, t3: T3) => R,
  a: T1
): (t2: T2, t3: T3) => R;
export function applyFirst<F extends (...args: any[]) => any>(
  func: F,
  ...args: any[]
): (...rest: any[]) => ReturnType<F>;
export function applyFirst<F extends (...args: any[]) => any>(
  func: F,
  ...args: any[]
): (...rest: any[]) => ReturnType<F> {
  return function (...rest: any[]) {
    return func.apply(null, args.concat(rest));
  };
}

test('a call carrying more arguments than the overload declares keeps them', () => {
  const collect = function (..._: any[]) {
    // eslint-disable-next-line prefer-rest-params
    return Array.from(arguments as any);
  };
  let applied: any = null;
  applied = applyFirst(collect, 'a');
  expect(applied('b', 'c', 'd')).toEqual(['a', 'b', 'c', 'd']);
});
test('the applied function reports its own Function.length', () => {
  const three = function (_a: string, _b: string, _c: string) {};
  const applied = applyFirst(three, 'a');
  expect(applied.length).toBe(0);
});
";
    run_fixture(source, "smelt_variadic_impl_behind_fixed_overload");
}
