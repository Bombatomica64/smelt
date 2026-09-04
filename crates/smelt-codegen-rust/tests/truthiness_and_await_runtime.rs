//! Runtime execution tests for four lowering rules whose defects produced Rust
//! that compiled cleanly and computed the wrong value.
//!
//! 1. **JavaScript truthiness of an erased or type-parameter value.** A
//!    boolean-position operand inside a lowered callback body was answered with
//!    `matches!(x, SmeltUnknown::Bool(_))` — a `typeof x === "boolean"` tag
//!    check, which is close to the *inverse* of truthiness: it is `true` for
//!    `false` and `false` for a truthy string or number. In the ordinary
//!    condition path an unconstrained type parameter was folded to the constant
//!    `true`, so `if (item)` over a generic `T` never filtered anything. Both
//!    now lower to the same `ToBool` primitive cast the concrete path uses.
//!
//! 2. **A user's own export is not replaced by a constant.** A name-keyed
//!    stdlib shim replaced any call to an imported identifier spelled `negate`
//!    with a null constant typed as a predicate, so a project that exports its
//!    own `negate` had every call to it silently answer `null`. Deleting the
//!    shim leaves the ordinary imported-item path, and a callback built by
//!    calling such a factory (`xs.filter(negate(isEven))`) is now evaluated once
//!    and called, instead of being modeled as a fabricated null callee.
//!
//! 3. **A non-null assertion into a nullish-accepting parameter is a no-op.**
//!    `f(x!)` where `f`'s parameter is optional narrowed `x` to its non-nullish
//!    type, which the emitter renders as `.expect(...)` — so passing an absent
//!    value panicked, where JavaScript passes `undefined` to a callee that
//!    handles it. TypeScript's `!` is type-level only and has no runtime effect.
//!
//! 4. **`await` of a value whose type is not a future.** The awaited operand was
//!    replaced by a `null` constant and *discarded*, deleting the awaited
//!    computation from the program. An erased operand may still hold a promise
//!    at runtime, and `await v` of a concrete non-thenable is `v`.
//!
//! Each case is a TypeScript Vitest test: lowering emits a `#[test]`, and a
//! green `cargo test` on the generated crate means every `expect(...)` held.
//! None of these could be pinned by a string golden — the wrong value is
//! produced by Rust that looks entirely healthy.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test truthiness_and_await_runtime -- --ignored
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
        "generated truthiness/await test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-truthiness-await-runtime-{}-{seq}",
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
fn a_generic_value_in_boolean_position_is_tested_for_truthiness() {
    // `item: T` for an unconstrained `T` is the case no concrete type can stand
    // in for: the same call site receives `0`, `""`, `false`, `NaN`, `null` and
    // real values, and JavaScript's answer depends on the runtime value, not on
    // the static type. Folding the guard to `true` (the old rule) makes the
    // function an identity, which the length assertion catches.
    let source = r#"
import { test, expect } from "vitest";

function compact<T>(items: T[]): T[] {
  const out: T[] = [];
  for (const item of items) {
    if (item) {
      out.push(item);
    }
  }
  return out;
}

test("a generic value in boolean position uses JavaScript truthiness", () => {
  const kept = compact([0, 1, false, 2, "", 3, null, undefined, NaN, 4]);
  expect(kept.length).toBe(4);
  expect(kept.map(v => String(v)).join(",")).toBe("1,2,3,4");
  // Every element is truthy, so nothing is dropped.
  expect(compact([1, "a", true]).length).toBe(3);
  // Every element is falsy, so everything is dropped.
  expect(compact([0, "", false]).length).toBe(0);
});
"#;
    run_fixture(source, "smelt_generic_truthiness_guard");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn negating_an_erased_value_inside_a_callback_uses_truthiness_not_a_tag_check() {
    // `entry.enabled` is read off a `Record<string, unknown>`, so the operand is
    // genuinely erased — the record's values are dynamic, which is why the
    // boundary is legitimate here. The old tag check answered `true` for the
    // stored `false`, i.e. the exact inverse of `!entry.enabled` on the value
    // that matters.
    let source = r#"
import { test, expect } from "vitest";

function firstDisabled(rows: Record<string, unknown>[]): number {
  return rows.findIndex(row => !row.enabled);
}

test("negating an erased value inside a callback is JavaScript truthiness", () => {
  expect(firstDisabled([{ enabled: true }, { enabled: false }])).toBe(1);
  expect(firstDisabled([{ enabled: false }, { enabled: true }])).toBe(0);
  // `0`, `""` and a missing key are falsy; a non-empty string is truthy.
  expect(firstDisabled([{ enabled: "yes" }, { enabled: 0 }])).toBe(1);
  expect(firstDisabled([{ enabled: "yes" }, { enabled: "" }])).toBe(1);
  expect(firstDisabled([{ enabled: 1 }, {}])).toBe(1);
  expect(firstDisabled([{ enabled: true }, { enabled: 2 }])).toBe(-1);
});
"#;
    run_fixture(source, "smelt_erased_callback_truthiness");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_user_defined_predicate_combinator_is_called_not_replaced_by_a_constant() {
    // `negate` is a plain user function; the name used to be intercepted by a
    // stdlib shim that answered `null`. Both spellings matter: calling the
    // returned predicate directly, and handing it to `filter`, where the factory
    // call itself sits in callback position.
    let source = r#"
import { test, expect } from "vitest";

function negate<F extends (...args: any[]) => boolean>(func: F): F {
  return ((...args: any[]) => !func(...args)) as F;
}

function isEven(n: number): boolean {
  return n % 2 === 0;
}

test("a user-defined negate combinator runs the wrapped predicate", () => {
  expect(negate(() => true)()).toBe(false);
  expect(negate(() => false)()).toBe(true);
  expect(typeof negate(isEven)).toBe("function");
});

test("a predicate built by calling a combinator filters with the real predicate", () => {
  const odds = [1, 2, 3, 4, 5, 6].filter(negate(isEven));
  expect(odds.length).toBe(3);
  expect(odds.join(",")).toBe("1,3,5");
});
"#;
    run_fixture(source, "smelt_user_predicate_combinator");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_non_null_assertion_into_an_optional_parameter_stays_a_no_op() {
    // `maximum!` is type-level only. The callee's parameter is optional and
    // handles the absent case itself, so the assertion must not narrow: doing so
    // rendered `.expect(...)` on a `None` and panicked at runtime.
    let source = r#"
import { test, expect } from "vitest";

function span(minimum: number, maximum?: number): number {
  if (maximum == null) {
    return minimum;
  }
  return maximum - minimum;
}

function spanFrom(minimum: number, maximum?: number): number {
  return span(minimum, maximum!);
}

test("a non-null assertion into an optional parameter forwards the absent value", () => {
  expect(spanFrom(5)).toBe(5);
  expect(spanFrom(2, 7)).toBe(5);
});
"#;
    run_fixture(source, "smelt_non_null_into_optional_param");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn awaiting_a_value_whose_type_is_not_a_future_keeps_the_operand() {
    // The awaited operand is element 1 of an erased tuple destructure, i.e.
    // `unknown` — a real dynamic boundary, because the same slot holds a promise
    // in one call and a plain value in the next, which is precisely what JS
    // `await` decides at runtime. The old rule replaced the whole `await` with
    // `null` and dropped the operand, so neither answer could be observed.
    let source = r#"
import { test, expect } from "vitest";

function attempt(func: () => unknown): [unknown, unknown] {
  return [null, func()];
}

test("awaiting an erased promise drives it", async () => {
  const [error, result] = attempt(async () => 1);
  expect(error).toBeNull();
  expect(await result).toBe(1);
});

test("awaiting an erased non-thenable is the identity", async () => {
  const [error, result] = attempt(() => 7);
  expect(error).toBeNull();
  expect(await result).toBe(7);
});

test("awaiting a concrete non-future value is the identity", async () => {
  const plain = 3;
  expect(await plain).toBe(3);
});
"#;
    run_fixture(source, "smelt_await_non_future_value");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_async_closure_returning_a_promise_yields_its_resolved_value() {
    // Three emitter rules meet in one shape, and all three only became
    // reachable once `await` stopped discarding a non-future-typed operand --
    // the computation it used to delete now reaches the emitter:
    //
    // * The closure's declared item type is erased (`unknown`) while the value
    //   it returns is a `Promise<unknown[]>`, so the returned future has to be
    //   ADAPTED -- awaited, its item coerced, re-wrapped -- rather than handed
    //   over at the wrong item type.
    // * A `SmeltFuture`'s output is a `Result`, so the wrapper awaiting that
    //   returned future needs its own `?` on top of the fallible closure's.
    // * The inner callback is adapted (it can throw where the parameter it fills
    //   cannot), and because the adapter's body becomes a `'static` future its
    //   by-reference parameter is rebound to an owned clone; the forwarding into
    //   the wrapped callback has to re-borrow that rebinding.
    //
    // Each one produced generated Rust that did not compile, so a green run of
    // this fixture is the whole assertion.
    let source = r#"
import { test, expect } from "vitest";

async function parallel(
  values: readonly unknown[],
  worker: (item: unknown) => Promise<unknown>,
): Promise<unknown[]> {
  const results: unknown[] = [];
  for (const value of values) {
    results.push(await worker(value));
  }
  if (results.length > 1000) {
    throw new Error("too many");
  }
  return results;
}

function tryit(func: () => Promise<unknown>): () => Promise<unknown> {
  return func;
}

function list(): unknown[] {
  return [1, 2, 3];
}

test("an async closure returning a promise resolves through it", async () => {
  const run = tryit(async () => {
    return parallel(list(), async (item: unknown) => {
      if (item === 99) {
        throw new Error("nope");
      }
      return `hi_${item}`;
    });
  });
  expect(await run()).toEqual(["hi_1", "hi_2", "hi_3"]);
});
"#;
    run_fixture(source, "smelt_async_closure_returns_promise");
}
