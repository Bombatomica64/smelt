//! Runtime execution tests for `AbortSignal` cancellation and promise settling
//! order on the virtual clock.
//!
//! Three defects in this family were only observable at *runtime* — every case
//! below emitted Rust that type-checked and that the `compile_corpus` tier
//! accepted, yet produced the wrong value or never settled:
//!
//! 1. **An aborted signal did not cancel anything.** `AbortController` /
//!    `AbortSignal` erase to marker-bearing records whose methods are bound by
//!    the `smelt_abort_method` runtime helper. The direct-receiver field read
//!    (`place.rs`) knew that; the optional-chain field read
//!    (`field_access_text`) did not, so `signal?.addEventListener('abort', h)` —
//!    the spelling every `AbortSignal`-aware API uses, because `signal` is an
//!    optional option — resolved to the record's *own* fields, found none, and
//!    collapsed to a no-op default callback. The handler was never registered
//!    and `controller.abort()` fired nothing.
//! 2. **An `async` function returning a promise did not adopt it.** JavaScript
//!    settles `async function f(): Promise<T> { return p; }` with `p`'s result.
//!    Smelt coerced the `Future<T>` into the `T` return slot by erasing it to a
//!    `SmeltUnknown::Promise`, so `await f()` handed back the promise object
//!    itself and a rejection inside it never reached the caller.
//! 3. **`Promise.race` picked a random winner.** It was backed by
//!    `tokio::select!`, which polls branches in randomized order and returns the
//!    first branch that reports `Ready` in a poll round. On the virtual clock
//!    each racer's spin loop advances time by one timer step per poll, so both
//!    racers settle within one round and the winner was a coin flip.
//!
//! Each case is a TypeScript Vitest test lowered to a crate and executed with
//! `cargo test`; a green run means every generated `expect(...)` held. The tier
//! is `#[ignore]`d because it compiles and runs real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test abort_signal_runtime -- --ignored
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

/// A cancellable delay shared by the abort fixtures.
///
/// This is the shape every `AbortSignal`-aware library helper has: the signal
/// arrives as an *optional* value, so every access to it is optional-chained.
/// `wait` resolves with `"completed"` when its timer fires and rejects with
/// `"aborted"` when the signal fires first, which lets a fixture tell
/// "rejected because the signal aborted" apart from "rejected eagerly".
const WAIT_HELPER: &str = r#"
function wait(ms: number, signal?: AbortSignal): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    if (signal?.aborted) {
      reject(new Error("aborted"));
      return;
    }
    setTimeout(() => {
      resolve("completed");
    }, ms);
    signal?.addEventListener("abort", () => {
      reject(new Error("aborted"));
    });
  });
}

async function settle(promise: Promise<string>): Promise<string> {
  try {
    return await promise;
  } catch (error: any) {
    return "rejected:" + error.message;
  }
}
"#;

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
        "generated abort-signal test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-abort-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_abort_fixture(source: &str, crate_name: &str) {
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
fn optional_chained_abort_listener_cancels_a_pending_timer() {
    // The load-bearing case. `signal?.addEventListener('abort', handler)` used to
    // read the erased abort record's *own* fields; the abort methods live on the
    // host prototype, not in the record, so the read answered `null` and the
    // optional-call coercion substituted a no-op default callback. Nothing was
    // registered, `controller.abort()` fired no listeners, and a `wait(...)` with
    // a signal simply ran to completion.
    //
    // The three cases below distinguish "rejected because the signal aborted"
    // from "rejected eagerly": only the run whose abort lands *before* the timer
    // rejects, and the two whose signal never fires (or fires late) must still
    // resolve with "completed".
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{WAIT_HELPER}
test("an aborted signal cancels a pending timer", async () => {{
  const early = new AbortController();
  setTimeout(() => early.abort(), 20);
  expect(await settle(wait(100, early.signal))).toBe("rejected:aborted");

  const quiet = new AbortController();
  expect(await settle(wait(50, quiet.signal))).toBe("completed");

  const late = new AbortController();
  setTimeout(() => late.abort(), 200);
  expect(await settle(wait(50, late.signal))).toBe("completed");
}});
"#
    );
    run_abort_fixture(&source, "smelt_abort_runtime_listener_cancels");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn already_aborted_signal_rejects_without_scheduling_a_timer() {
    // The `signal?.aborted` guard is a plain data-field read and always worked;
    // this pins it so a change to the optional-chain field path cannot regress
    // the synchronous branch while fixing the method branch. The trailing
    // no-signal case proves the guard did not start rejecting unconditionally.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{WAIT_HELPER}
test("an already-aborted signal rejects immediately", async () => {{
  const controller = new AbortController();
  controller.abort();
  expect(controller.signal.aborted).toBe(true);
  expect(await settle(wait(100, controller.signal))).toBe("rejected:aborted");
  expect(await settle(wait(10))).toBe("completed");
}});
"#
    );
    run_abort_fixture(&source, "smelt_abort_runtime_already_aborted");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn every_registered_abort_listener_fires_once() {
    // `addEventListener` appends to the signal's shared listener array and
    // `abort()` drains it, so two distinct handlers must both run exactly once
    // and a second `abort()` must be a no-op. Registering the *same* handler
    // twice registers it twice, exactly as the DOM does without `once`.
    let source = r#"
import { test, expect } from "vitest";
test("each registered abort listener fires once", async () => {
  const controller = new AbortController();
  const seen: string[] = [];
  controller.signal.addEventListener("abort", () => {
    seen.push("first");
  });
  controller.signal.addEventListener("abort", () => {
    seen.push("second");
  });
  controller.abort();
  controller.abort();
  expect(seen.length).toBe(2);
  expect(seen[0]).toBe("first");
  expect(seen[1]).toBe("second");
});
"#;
    run_abort_fixture(source, "smelt_abort_runtime_listener_identity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn async_function_adopts_a_returned_promise() {
    // `return p` inside an `async function` ADOPTS `p`: the caller observes `p`'s
    // settled value, never the promise object. Smelt used to erase the returned
    // `Future<T>` into the `T` return slot as a `SmeltUnknown::Promise`, so
    // `await adopt()` produced "[object Promise]" and a rejection inside the
    // returned promise never reached the caller's `catch`. Both directions are
    // asserted: the resolved value must flow through, and the rejection must too.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{WAIT_HELPER}
async function adopt(): Promise<string> {{
  return wait(20);
}}

async function adoptRejection(): Promise<string> {{
  const controller = new AbortController();
  setTimeout(() => controller.abort(), 10);
  return wait(100, controller.signal);
}}

test("an async function adopts the promise it returns", async () => {{
  expect(await adopt()).toBe("completed");

  let caught = "none";
  try {{
    await adoptRejection();
  }} catch (error: any) {{
    caught = error.message;
  }}
  expect(caught).toBe("aborted");
}});
"#
    );
    run_abort_fixture(&source, "smelt_abort_runtime_async_adopt");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn promise_race_settles_with_the_earliest_racer() {
    // `tokio::select!` randomizes its branch poll order, so which racer won was a
    // coin flip once both had settled inside the same poll round — and on the
    // virtual clock they always do, because each poll of a promise spin loop
    // advances time by one timer step. The loop runs the race repeatedly in both
    // argument orders: a randomized winner cannot survive sixteen draws.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{WAIT_HELPER}
async function label(ms: number, name: string): Promise<string> {{
  await wait(ms);
  return name;
}}

test("Promise.race settles with the racer that finishes first", async () => {{
  for (let round = 0; round < 8; round++) {{
    const fastFirst = await Promise.race([label(20, "fast"), label(80, "slow")]);
    expect(fastFirst).toBe("fast");
    const slowFirst = await Promise.race([label(80, "slow"), label(20, "fast")]);
    expect(slowFirst).toBe("fast");
  }}
}});
"#
    );
    run_abort_fixture(&source, "smelt_abort_runtime_race_order");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn timeout_race_rejects_when_the_deadline_wins() {
    // The `withTimeout` shape end to end: an async function whose whole body is
    // `return Promise.race([work(), deadline(ms)])`. It needs promise adoption
    // (or the caller gets the promise object), a deterministic race (or the
    // winner is a coin flip), and abort-listener registration (or an aborted
    // deadline still rejects). Both outcomes are asserted so a change that makes
    // the timeout always win, or never win, fails.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{WAIT_HELPER}
async function deadline(ms: number, signal?: AbortSignal): Promise<string> {{
  const reached = await settle(wait(ms, signal));
  if (reached === "completed") {{
    throw new Error("timed out");
  }}
  return "disarmed";
}}

async function withDeadline(workMs: number, ms: number): Promise<string> {{
  return Promise.race([label(workMs), deadline(ms)]);
}}

async function label(ms: number): Promise<string> {{
  await wait(ms);
  return "work";
}}

test("a deadline race rejects only when the deadline wins", async () => {{
  expect(await withDeadline(20, 100)).toBe("work");

  let caught = "none";
  try {{
    await withDeadline(1000, 50);
  }} catch (error: any) {{
    caught = error.message;
  }}
  expect(caught).toBe("timed out");
}});
"#
    );
    run_abort_fixture(&source, "smelt_abort_runtime_deadline_race");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_abort_handler_written_before_the_timer_handle_still_clears_it() {
    // es-toolkit's `timeout` shape: the abort handler is declared BEFORE the
    // timer handle it clears, which JavaScript allows because the handler only
    // runs later. Smelt lowers statements in source order, so the handler's
    // read of `timeoutId` found no binding and fell through to the
    // module-global fallback, which FABRICATED an empty object for an
    // `unknown` type -- `clearTimeout({})` matched no numeric handle and
    // cleared nothing, so disarming the timeout was a silent no-op and the
    // deadline still fired.
    let source = r#"
import { test, expect } from "vitest";

function armed(ms: number, signal: AbortSignal): Promise<string> {
  return new Promise<string>((resolve, reject) => {
    const abortHandler = () => {
      clearTimeout(timeoutId);
    };
    const timeoutId = setTimeout(() => {
      signal.removeEventListener('abort', abortHandler);
      reject(new Error('deadline'));
    }, ms);
    signal.addEventListener('abort', abortHandler, { once: true });
  });
}

async function settle(promise: Promise<string>): Promise<string> {
  try {
    await promise;
    return "resolved";
  } catch {
    return "rejected";
  }
}

test("aborting before the deadline disarms the timer", async () => {
  const controller = new AbortController();
  setTimeout(() => controller.abort(), 20);
  const winner = await Promise.race([
    armed(100, controller.signal).then(() => "deadline", () => "deadline"),
    new Promise<string>(resolve => setTimeout(() => resolve("work"), 60)),
  ]);
  expect(winner).toBe("work");
});
test("without an abort the deadline still rejects", async () => {
  const quiet = new AbortController();
  expect(await settle(armed(20, quiet.signal))).toBe("rejected");
});
"#;
    run_abort_fixture(source, "smelt_abort_runtime_forward_timer_handle");
}
