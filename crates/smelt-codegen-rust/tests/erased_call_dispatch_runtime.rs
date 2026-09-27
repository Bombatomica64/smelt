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
//! **A prototype method on an erased receiver is the value's own method.**
//! `anyValue.reduceRight(f, 0)` reads `reduceRight` off the runtime value and
//! calls it: an erased array answers `Array.prototype`, an erased string
//! `String.prototype`, and anything else throws `TypeError: x.m is not a
//! function` (see `erased_method_prelude`). The read used to answer
//! `undefined` and the call a `null` stub.
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

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_variadic_array_callback_receives_every_array_method_argument() {
    // `(...args: any[]) => any` is a genuine dynamic boundary: its one Rust
    // parameter is the erased rest list, and nothing narrower describes "every
    // argument the caller happens to pass". An array method calling it
    // supplies `(item, index, array)`; binding the ELEMENT to the rest slot
    // instead converted an object element into an argument list and panicked
    // "unknown is not iterable" (radash `chain(getName, upperCase)` mapped over
    // `User[]`).
    let source = r"
import { test, expect } from 'vitest';

function chain(...funcs: ((...args: any[]) => any)[]) {
  return (...args: any[]) => {
    return funcs.slice(1).reduce((acc, fn) => fn(acc), funcs[0](...args));
  };
}

type User = { id: number; name: string };

test('an erased variadic mapper sees the element first', () => {
  const users: User[] = [
    { id: 1, name: 'ada' },
    { id: 2, name: 'grace' },
  ];
  const getName = (item: User) => item.name;
  const upper = (text: string) => text.toUpperCase();
  const mapper = chain(getName, upper);
  expect(users.map(mapper)).toEqual(['ADA', 'GRACE']);
});
test('an erased variadic mapper also receives the index', () => {
  const both = (...args: any[]) => `${args[0]}@${args[1]}`;
  expect(['x', 'y'].map(both)).toEqual(['x@0', 'y@1']);
});
";
    run_fixture(source, "smelt_variadic_array_callback_arguments");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_jest_global_mock_is_the_vitest_mock() {
    // Jest exposes the mock API Vitest calls `vi` as the `jest` global. A suite
    // written against Jest globals must get a real counting mock, not an
    // erased read of an unmodeled `jest` object (which made every
    // `toHaveBeenCalledTimes` see zero calls).
    let source = r"
import { test, expect } from 'vitest';

test('jest.fn counts its calls', () => {
  const mock = jest.fn();
  mock();
  mock();
  expect(mock).toHaveBeenCalledTimes(2);
});
";
    run_fixture(source, "smelt_jest_global_mock");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn array_methods_on_an_erased_array_run_the_list_operation() {
    // An `any` receiver has no static shape, so `value.reduceRight(..)` is a
    // property read through `smelt_get_unknown_field` followed by the erased
    // call ABI. The read used to answer `undefined` for every
    // `Array.prototype` method and the call returned a `null` stub. The
    // runtime now binds the method to the array it meets; callbacks see
    // `(element, index, array)` exactly as in JavaScript.
    let source = r"
import { test, expect } from 'vitest';

function box(value: any): any {
  return { items: value };
}

test('iteration methods pass element, index and array', () => {
  const o = box([1, 2, 3]);
  expect(o.items.reduceRight((acc: number, x: number) => acc * 10 + x, 0)).toBe(321);
  expect(o.items.findLast((x: number) => x < 3)).toBe(2);
  expect(o.items.findLastIndex((x: number) => x < 3)).toBe(1);
  expect(o.items.at(-1)).toBe(3);
  expect(o.items.flatMap((x: number, i: number) => [x, i])).toEqual([1, 0, 2, 1, 3, 2]);
  expect(o.items.includes(2)).toBe(true);
  expect(o.items.indexOf(3)).toBe(2);
  expect(o.items.slice(1)).toEqual([2, 3]);
});
test('mutating methods mutate the caller array', () => {
  const o = box([3, 1, 2]);
  expect(o.items.push(9)).toBe(4);
  expect(o.items.length).toBe(4);
  o.items.sort((a: number, b: number) => a - b);
  expect(o.items[0]).toBe(1);
  expect(o.items.splice(1, 2)).toEqual([2, 3]);
  expect(o.items.length).toBe(2);
  expect(o.items.pop()).toBe(9);
  expect(o.items.length).toBe(1);
});
test('an empty reduce without an initial value throws its TypeError', () => {
  const o = box([]);
  let message = '';
  try {
    o.items.reduceRight((acc: number, x: number) => acc + x);
  } catch (error) {
    message = (error as Error).message;
  }
  expect(message).toBe('Reduce of empty array with no initial value');
});
";
    run_fixture(source, "smelt_erased_array_methods");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn shared_array_and_string_method_names_resolve_on_the_runtime_value() {
    // `slice`/`includes`/`indexOf`/`at` exist on BOTH prototypes, so on an
    // erased receiver the name cannot choose the operation statically: a list
    // slice turned an `any` string into an array of characters and string
    // containment rejected an `any` array. The runtime value decides.
    let source = r"
import { test, expect } from 'vitest';

function box(value: any): any {
  return { value };
}

test('a string receiver takes String.prototype', () => {
  const s = box('abcab').value;
  expect(s.slice(1, 3)).toBe('bc');
  expect(s.includes('ca')).toBe(true);
  expect(s.indexOf('b', 2)).toBe(4);
  expect(s.lastIndexOf('a')).toBe(3);
  expect(s.at(-1)).toBe('b');
  expect(s.padStart(7, '-')).toBe('--abcab');
});
test('an array receiver takes Array.prototype', () => {
  const xs = box([1, 2, 3]).value;
  expect(xs.slice(-2)).toEqual([2, 3]);
  expect(xs.includes(3)).toBe(true);
  expect(xs.indexOf(4)).toBe(-1);
  expect(xs.concat([4], 5)).toEqual([1, 2, 3, 4, 5]);
});
";
    run_fixture(source, "smelt_erased_shared_method_names");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_missing_method_on_an_erased_value_is_a_type_error() {
    // JavaScript calls the value it read; a value with no such method throws
    // `TypeError: x.at is not a function` rather than returning a stub.
    let source = r"
import { test, expect } from 'vitest';

function box(value: any): any {
  return { value };
}

test('calling an absent method throws TypeError naming the callee', () => {
  const x = box(5).value;
  let isTypeError = false;
  let message = '';
  try {
    x.at(0);
  } catch (error) {
    isTypeError = error instanceof TypeError;
    message = (error as Error).message;
  }
  expect(isTypeError).toBe(true);
  expect(message).toBe('x.at is not a function');
});
";
    run_fixture(source, "smelt_erased_missing_method_type_error");
}
