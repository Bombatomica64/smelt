//! Runtime execution tests for promise *values*: the value a promise settles
//! with, the value it rejects with, and when the work behind it starts.
//!
//! Six defects in this family were only observable at *runtime* — every case
//! below emitted Rust that type-checked and that the `compile_corpus` tier
//! accepted, yet produced the wrong value or the wrong schedule:
//!
//! 1. **An absent optional member became a callable that answered `false`.** An
//!    optional-chain field read whose static field type is erased (an `unknown`
//!    receiver, a union, an erased class) was emitted as `Option::map`, so the
//!    `Option` modeled only the *receiver's* nullishness. A missing property
//!    then had to be coerced to the destination type, and coercing "absent" to a
//!    callback destination synthesizes a default closure returning `false`. So
//!    `options?.shouldRetry ?? DEFAULT` bound that stub instead of falling
//!    through to the `??`, and a retry loop guarded by it never retried.
//! 2. **`Promise.reject(reason)` compiled to nothing.** It was missing from the
//!    static-call recognition table, so it fell through to the host-namespace
//!    path: the `Promise` namespace object has no `reject` member, the read
//!    answered `undefined`, and the callable coercion substituted a default
//!    closure returning `null`. `await Promise.reject(new Error("boom"))` ran
//!    straight on to the next statement and no `catch` ever fired.
//! 3. **A rejection reason was reduced to a string.** The settled state of an
//!    erased promise stored its rejection in a `String`, so every reason that
//!    was not exactly its own `message` was destroyed: `Promise.reject({ status:
//!    400 })` settled as "[object Object]" and was re-inflated on await as a
//!    synthetic `{ __smelt_error, message }` record with `status` gone.
//! 4. **`Promise.resolve(v)` dropped `v`.** It lowered to a bare sleep, which
//!    keeps only the *type* of `v`; the `Future<()>` -> `Future<T>` coercion then
//!    invented a `T`, so `Promise.resolve(1)` settled as `0` and
//!    `Promise.resolve("hello")` as `""`.
//! 5. **A `Promise.all` element expression was dropped.** Any element that was
//!    not *statically* a future lowered to the same bare sleep, so the element
//!    expression — and every side effect in it — never ran:
//!    `Promise.all([f(), g()])` on erased callables called neither.
//! 6. **Adapted async callbacks lost their concurrency, and a primed prefix
//!    owned the clock.** A callback adapter emitted the inner async call inside
//!    a *lazy* future body, so a batch of adapted callbacks started one at a
//!    time as the combinator awaited them. Hoisting the call exposed the other
//!    half: the virtual-clock sleep advances time to its own deadline as soon as
//!    it is driven, so an eager prefix containing `delay(1000)` jumped the clock
//!    a full second before any later deadline was armed.
//!
//! Each case is a TypeScript Vitest test lowered to a crate and executed with
//! `cargo test`; a green run means every generated `expect(...)` held. The tier
//! is `#[ignore]`d because it compiles and runs real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test promise_value_fidelity_runtime -- --ignored
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
        "generated promise-value test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-promise-value-{}-{seq}", std::process::id()))
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
fn an_absent_optional_member_falls_through_to_the_nullish_default() {
    // The load-bearing case. `options` is a *union* (`number | Options`), so the
    // member read has no static field type and lowers to a dynamic lookup. The
    // `Options` arm below omits `shouldRetry` entirely, so that lookup answers
    // `undefined` — which must reach the `??` and select `DEFAULT_SHOULD_RETRY`.
    //
    // The concrete-struct spelling (`pick(options?: Options)`) already worked,
    // so a union base is what separates the two paths; both are asserted here so
    // a change to either cannot silently diverge from the other. The final case
    // proves the fix did not start ignoring a member that IS present.
    let source = r#"
import { test, expect } from "vitest";

interface Options {
  shouldRetry?: (attempt: number) => boolean;
}

const DEFAULT_SHOULD_RETRY = () => true;

function pickFromUnion(options?: number | Options): boolean {
  const shouldRetry = (options as Options | undefined)?.shouldRetry ?? DEFAULT_SHOULD_RETRY;
  return shouldRetry(0);
}

function pickFromStruct(options?: Options): boolean {
  const shouldRetry = options?.shouldRetry ?? DEFAULT_SHOULD_RETRY;
  return shouldRetry(0);
}

test("an absent optional member selects the ?? fallback", () => {
  expect(pickFromUnion({})).toBe(true);
  expect(pickFromUnion()).toBe(true);
  expect(pickFromStruct({})).toBe(true);
  expect(pickFromStruct()).toBe(true);

  expect(pickFromUnion({ shouldRetry: () => false })).toBe(false);
  expect(pickFromStruct({ shouldRetry: () => false })).toBe(false);
});
"#;
    run_fixture(source, "smelt_promise_value_optional_member_fallback");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_rejection_reason_keeps_its_own_properties() {
    // JavaScript rejects with any value, not with a message. A plain object
    // reason must arrive at the `catch` with every property it was thrown with;
    // reducing the settle state to a `String` dropped `status` and re-inflated a
    // synthetic `Error` record in its place. Both a non-`Error` reason and a
    // real `Error` are asserted so restoring the payload cannot regress the
    // ordinary message path.
    //
    // This also pins `Promise.reject` itself: unregistered, it compiled to a
    // no-op and every `catch` below was simply never entered, which is why the
    // final case asserts the *resolving* path too — a rejection that silently
    // does not happen and one that carries the wrong payload both have to fail
    // this fixture.
    let source = r#"
import { test, expect } from "vitest";

test("a rejection reason keeps its own properties", async () => {
  let plain: any = null;
  try {
    await Promise.reject({ status: 400, message: "Bad Request" });
  } catch (error: any) {
    plain = error;
  }
  expect(plain.status).toBe(400);
  expect(plain.message).toBe("Bad Request");

  let thrown: any = null;
  try {
    await Promise.reject(new Error("boom"));
  } catch (error: any) {
    thrown = error;
  }
  expect(thrown.message).toBe("boom");

  let settled: any = null;
  try {
    settled = await Promise.resolve(1);
  } catch (error: any) {
    settled = "threw";
  }
  expect(settled).toBe(1);
});
"#;
    run_fixture(source, "smelt_promise_value_rejection_reason");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn promise_resolve_settles_with_its_argument() {
    // `Promise.resolve(v)` lowered to a bare sleep, which keeps only `v`'s type;
    // the coercion into the declared item type then produced that type's default
    // value. Every primitive default is a *plausible* value, which is exactly
    // why this was invisible: `1` became `0`, `"hello"` became `""`, `true`
    // became `false`. The no-argument form must still settle with `undefined`.
    let source = r#"
import { test, expect } from "vitest";

test("Promise.resolve settles with its argument", async () => {
  expect(await Promise.resolve(1)).toBe(1);
  expect(await Promise.resolve("hello")).toBe("hello");
  expect(await Promise.resolve(true)).toBe(true);
  expect(await Promise.resolve()).toBe(undefined);

  const values = await Promise.all([Promise.resolve(1), Promise.resolve("hello"), Promise.resolve(true)]);
  expect(values[0]).toBe(1);
  expect(values[1]).toBe("hello");
  expect(values[2]).toBe(true);
});
"#;
    run_fixture(source, "smelt_promise_value_resolve_argument");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn promise_all_evaluates_every_element_expression() {
    // An element that is not *statically* a future was replaced by a bare sleep
    // of its type, so the element expression never ran at all. `run` here is
    // erased (`any`), so its calls take exactly that path: before the fix `calls`
    // stayed at 0 and the settled values were the type's defaults. Plain
    // non-promise values are asserted alongside, since `Promise.all` adopts them
    // unchanged and they travel the same lowering.
    let source = r#"
import { test, expect } from "vitest";

test("Promise.all evaluates every element expression", async () => {
  let calls = 0;
  const run: any = async (value: number) => {
    calls++;
    return value * 2;
  };

  const values = await Promise.all([run(1), run(2), 7]);

  expect(calls).toBe(2);
  expect(values[0]).toBe(2);
  expect(values[1]).toBe(4);
  expect(values[2]).toBe(7);
});
"#;
    run_fixture(source, "smelt_promise_value_all_elements");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn adapted_async_callbacks_start_eagerly_without_owning_the_clock() {
    // Two halves of the same schedule.
    //
    // `mapConcurrent` passes its callback through an arity/type adapter, which
    // used to emit the inner call inside a *lazy* future body. The call then did
    // not happen until the combinator awaited that element, so ten callbacks
    // started one at a time and `maxRunning` was 1 instead of 10.
    //
    // Hoisting the call out of the body makes the prefix run at call time, as
    // JavaScript does — and that must NOT make virtual time pass. `raceTimeout`
    // pins the other half: `slow()`'s primed prefix arms a 1000ms delay before
    // the 50ms deadline exists, so if priming advanced the clock to its own
    // deadline the timeout could never win the race.
    let source = r#"
import { test, expect } from "vitest";
import { setTimeout as _unusedSetTimeout } from "timers";

function delay(ms: number): Promise<void> {
  return new Promise<void>(resolve => {
    setTimeout(() => resolve(), ms);
  });
}

function mapConcurrent(items: number[], callback: (item: number) => Promise<number>): Promise<number[]> {
  return Promise.all(items.map(callback));
}

function timeoutAfter(ms: number): Promise<string> {
  return new Promise<string>((_resolve, reject) => {
    setTimeout(() => reject(new Error("timed out")), ms);
  });
}

async function raceTimeout(): Promise<string> {
  const slow = async (): Promise<string> => {
    await delay(1000);
    return "slow";
  };
  try {
    return await Promise.race([slow(), timeoutAfter(50)]);
  } catch (error: any) {
    return "rejected:" + error.message;
  }
}

test("adapted async callbacks run concurrently", async () => {
  let running = 0;
  let maxRunning = 0;
  const items = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

  const results = await mapConcurrent(items, async (item: number) => {
    running++;
    if (running > maxRunning) {
      maxRunning = running;
    }
    await delay(20);
    running--;
    return item;
  });

  expect(maxRunning).toBe(10);
  expect(results.length).toBe(10);
  expect(results[0]).toBe(1);
  expect(results[9]).toBe(10);
});

test("an eager prefix does not advance the virtual clock", async () => {
  expect(await raceTimeout()).toBe("rejected:timed out");
});
"#;
    run_fixture(source, "smelt_promise_value_adapter_schedule");
}
