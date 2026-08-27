//! Runtime execution tests for self-recursive named function expressions.
//!
//! A named function expression binds its own name inside its own body, and that is
//! how JavaScript writes a self-recursive callback:
//!
//! ```js
//! mergeWith(cloneDeep(target), source, function mergeRecursively(a, b) {
//!   … return mergeWith(clone(a), b, mergeRecursively); …
//! })
//! ```
//!
//! The closure path never bound the name, so the self-reference fell through
//! identifier resolution to the forward-callable fallback and lowered to an EMPTY
//! OBJECT. Calling an empty object collapses to a null callback rather than
//! failing, so the recursion silently did nothing and the caller saw a value that
//! had merely skipped every nested level — the shape of all eight es-toolkit
//! `toMerged` failures.
//!
//! An inline Rust closure cannot express the binding either: it would have to
//! capture the very binding it is being assigned to. The function is therefore
//! lifted to a module-owned item, so the recursion is ordinary `fn` recursion.
//!
//! A string-golden test (`a_self_recursive_named_function_expression_lifts_to_an_item`)
//! proves the item is emitted; only running the program proves the recursion
//! terminates with the right value, and that two same-named lifts in one crate stay
//! distinct.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test named_function_expression_runtime -- --ignored
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
        "generated named-function-expression test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-named-fn-expr-runtime-{}-{seq}",
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
fn a_self_recursive_named_function_expression_recurses() {
    // The recursion has to terminate with the right value, not merely compile. With
    // the pre-fix lowering the self-call answered `undefined`, so `step(3)` came
    // back as `1` (one level, plus `NaN` coerced away) instead of `3`.
    let source = r"
import { test, expect } from 'vitest';

function apply(cb: (n: number) => number, v: number): number {
  return cb(v);
}

test('a named function expression can call itself', () => {
  const depth = apply(function step(n: number): number {
    if (n <= 0) {
      return 0;
    }
    return step(n - 1) + 1;
  }, 3);
  expect(depth).toBe(3);
});
";
    run_fixture(source, "smelt_self_recursive_named_function_expression");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_capturing_self_recursive_closure_recurses_and_escapes() {
    // Two properties in one fixture, because the fix trades between them.
    //
    // `collect` is the shape that leaked: a self-recursive closure capturing an
    // accumulator from its enclosing scope. It now captures a `Weak` to its own cell
    // instead of an `Rc`, which breaks the reference cycle -- so this asserts the
    // recursion still produces the right answer through a `Weak` upgrade.
    //
    // `makeCounter` is the case the fix must NOT break: a self-recursive closure
    // RETURNED from its defining frame and called afterwards. Those keep the strong
    // capture (`closure.escapes`), because a `Weak` there would find the cell gone.
    // If the escape guard were dropped, this half panics with "self-recursive closure
    // called after its defining scope returned" instead of answering 6.
    let source = r"
import { test, expect } from 'vitest';

function collect(rows: number[][]): number[] {
  const result: number[] = [];
  const recurse = (items: number[][], depth: number): void => {
    for (const item of items) {
      if (depth > 0) {
        recurse([item], depth - 1);
      } else {
        result.push(item[0]);
      }
    }
  };
  recurse(rows, 1);
  return result;
}

function makeCountdown(): (n: number) => number {
  const step = (n: number): number => {
    if (n <= 0) {
      return 0;
    }
    return step(n - 1) + 1;
  };
  return step;
}

test('a capturing self-recursive closure accumulates through its capture', () => {
  expect(collect([[7], [8], [9]])).toEqual([7, 8, 9]);
});

test('a self-recursive closure still works after escaping its frame', () => {
  const countdown = makeCountdown();
  expect(countdown(6)).toBe(6);
});
";
    run_fixture(source, "smelt_capturing_self_recursive_closure");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn two_same_named_lifts_stay_distinct() {
    // Two function expressions in one crate may share a source name; each lift is a
    // separate item, and the emitter's name disambiguation must keep them apart. If
    // they collided, one body would answer for both and the second expectation
    // below would read the first's arithmetic.
    let source = r"
import { test, expect } from 'vitest';

function apply(cb: (n: number) => number, v: number): number {
  return cb(v);
}

function countUp(start: number): number {
  return apply(function step(n: number): number {
    if (n <= 0) {
      return 0;
    }
    return step(n - 1) + 1;
  }, start);
}

function countUpByTwo(start: number): number {
  return apply(function step(n: number): number {
    if (n <= 0) {
      return 0;
    }
    return step(n - 1) + 2;
  }, start);
}

test('same-named lifts keep their own bodies', () => {
  expect(countUp(3)).toBe(3);
  expect(countUpByTwo(3)).toBe(6);
});
";
    run_fixture(source, "smelt_same_named_function_expression_lifts");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_expression_name_does_not_leak_into_module_scope() {
    // JavaScript scopes a named function expression's name to its own body. The
    // lift binds the name for exactly the duration of that body's lowering, so a
    // module-scope declaration of the same name keeps its own meaning everywhere
    // else. If the binding leaked, `shout` would call the lifted numeric `step`.
    let source = r"
import { test, expect } from 'vitest';

function apply(cb: (n: number) => number, v: number): number {
  return cb(v);
}

function step(label: string): string {
  return label + '!';
}

function countdown(start: number): number {
  return apply(function step(n: number): number {
    if (n <= 0) {
      return 0;
    }
    return step(n - 1) + 1;
  }, start);
}

test('the module-scope binding of the same name is untouched', () => {
  expect(step('go')).toBe('go!');
  expect(countdown(2)).toBe(2);
});
";
    run_fixture(source, "smelt_named_function_expression_scoping");
}
