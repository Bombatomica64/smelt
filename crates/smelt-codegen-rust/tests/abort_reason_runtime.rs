//! Runtime execution tests for `AbortSignal`'s reason, its statics, and the
//! dependent signal a `Request` holds.
//!
//! The existing `abort_signal_runtime` tier covers cancellation ORDER on the
//! virtual clock. This one covers the value half, which had two silent defects:
//! `smelt_abort_method`'s `abort` arm discarded its arguments, so
//! `signal.reason` was always `undefined`; and its fallthrough answered
//! `undefined` for `throwIfAborted`, so a cancellation check that should throw
//! silently passed and the cancelled work ran. Every expectation below was
//! diffed against Node 22 before being written down:
//!
//! 1. **`abort(reason)` carries the reason**, whatever it is — a string, a
//!    number, a plain object — because a reason is any JavaScript value.
//! 2. **`abort()` uses the spec's default**: an `AbortError` whose message is
//!    `This operation was aborted`. An explicit `undefined` is the same
//!    request, which Node confirms.
//! 3. **A second `abort` is a no-op**: the first reason stands, and a listener
//!    registered after the abort never runs.
//! 4. **The reason is already set when a listener runs**, so a handler reading
//!    `signal.reason` sees it rather than `undefined`.
//! 5. **`throwIfAborted()` throws the REASON ITSELF** — a string reason arrives
//!    at the `catch` as that string, not as an `Error` wrapping it — and
//!    returns `undefined` when the signal is not aborted.
//! 6. **`AbortSignal.abort(reason?)`** answers an already-aborted signal, and
//!    **`AbortSignal.timeout(ms)`** one that aborts later with a
//!    `TimeoutError` — the only thing that distinguishes the two statics, which
//!    is why the reason had to land first.
//! 7. **`request.signal` is a DEPENDENT signal**: stable per request, distinct
//!    from the `init.signal` it was built with, and aborted with the same
//!    reason when that one aborts. A source that is already aborted settles it
//!    at construction.
//!
//! Each case is a TypeScript Vitest test lowered to a crate and executed with
//! `cargo test`; a green run means every generated `expect(...)` held. The tier
//! is `#[ignore]`d because it compiles and runs real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test abort_reason_runtime -- --ignored
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
        "generated AbortSignal test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-abort-reason-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_abort_reason_fixture(source: &str, crate_name: &str) {
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
fn abort_carries_its_reason() {
    let source = r#"
import { test, expect } from "vitest";
test("a string reason is carried verbatim", () => {
  const controller = new AbortController();
  expect(controller.signal.aborted).toBe(false);
  expect(String(controller.signal.reason)).toBe("undefined");
  controller.abort("because");
  expect(controller.signal.aborted).toBe(true);
  expect(String(controller.signal.reason)).toBe("because");
});
test("a non-string reason is carried too", () => {
  const controller = new AbortController();
  controller.abort(42);
  expect(String(controller.signal.reason)).toBe("42");
});
test("no reason means the spec's AbortError", () => {
  const controller = new AbortController();
  controller.abort();
  const reason = controller.signal.reason as Error;
  expect(reason.name).toBe("AbortError");
  expect(reason.message).toBe("This operation was aborted");
});
test("an explicit undefined is the same as no reason", () => {
  const controller = new AbortController();
  controller.abort(undefined);
  expect((controller.signal.reason as Error).name).toBe("AbortError");
});
test("a second abort keeps the first reason", () => {
  const controller = new AbortController();
  controller.abort("first");
  controller.abort("second");
  expect(String(controller.signal.reason)).toBe("first");
});
test("a listener sees the reason already set", () => {
  const controller = new AbortController();
  let seen = "not run";
  controller.signal.addEventListener("abort", () => {
    seen = String(controller.signal.reason);
  });
  controller.abort("in-listener");
  expect(seen).toBe("in-listener");
});
test("a listener added after the abort never runs", () => {
  const controller = new AbortController();
  controller.abort("done");
  let ran = "no";
  controller.signal.addEventListener("abort", () => {
    ran = "yes";
  });
  expect(ran).toBe("no");
});
"#;
    run_abort_reason_fixture(source, "abort_reason_carried");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn throw_if_aborted_throws_the_reason_itself() {
    let source = r#"
import { test, expect } from "vitest";
test("an un-aborted signal answers undefined", () => {
  const controller = new AbortController();
  expect(controller.signal.throwIfAborted()).toBe(undefined);
});
test("the thrown value is the reason, not a wrapper around it", () => {
  const controller = new AbortController();
  controller.abort("plain string");
  let caught = "no throw";
  try {
    controller.signal.throwIfAborted();
  } catch (error) {
    caught = String(error);
  }
  // An `Error` wrapping the reason would read "Error: plain string" here, which
  // is what the panic route produced before it carried the thrown value.
  expect(caught).toBe("plain string");
});
test("a default reason throws as the AbortError it is", () => {
  const controller = new AbortController();
  controller.abort();
  let name = "no throw";
  try {
    controller.signal.throwIfAborted();
  } catch (error) {
    name = (error as Error).name;
  }
  expect(name).toBe("AbortError");
});
"#;
    run_abort_reason_fixture(source, "abort_throw_if_aborted");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_two_statics_answer_signals_that_differ_only_in_their_reason() {
    let source = r#"
import { test, expect } from "vitest";
test("AbortSignal.abort is already aborted", () => {
  const given = AbortSignal.abort("preset");
  expect(given.aborted).toBe(true);
  expect(String(given.reason)).toBe("preset");
  const defaulted = AbortSignal.abort();
  expect(defaulted.aborted).toBe(true);
  expect((defaulted.reason as Error).name).toBe("AbortError");
});
test("AbortSignal.timeout aborts later, with a TimeoutError", async () => {
  const signal = AbortSignal.timeout(20);
  expect(signal.aborted).toBe(false);
  let fired = "no";
  signal.addEventListener("abort", () => {
    fired = "yes";
  });
  await new Promise((resolve) => setTimeout(resolve, 60));
  expect(signal.aborted).toBe(true);
  expect(fired).toBe("yes");
  const reason = signal.reason as Error;
  // The reason is the ONLY thing separating a timed-out signal from
  // `AbortSignal.abort()`, which is why it had to land before the statics.
  expect(reason.name).toBe("TimeoutError");
  expect(reason.message).toBe("The operation was aborted due to timeout");
});
"#;
    run_abort_reason_fixture(source, "abort_statics");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_request_holds_a_dependent_signal() {
    let source = r#"
import { test, expect } from "vitest";
test("every request has a stable signal of its own", () => {
  const request = new Request("https://a.test/x");
  expect(request.signal === request.signal).toBe(true);
  expect(request.signal.aborted).toBe(false);
});
test("the request's signal follows init.signal without being it", () => {
  const controller = new AbortController();
  const request = new Request("https://a.test/y", { signal: controller.signal });
  expect(request.signal === controller.signal).toBe(false);
  expect(request.signal.aborted).toBe(false);
  let saw = "no";
  request.signal.addEventListener("abort", () => {
    saw = "yes";
  });
  controller.abort("stop");
  expect(request.signal.aborted).toBe(true);
  expect(String(request.signal.reason)).toBe("stop");
  expect(saw).toBe("yes");
});
test("an already-aborted source settles the request's signal at construction", () => {
  const request = new Request("https://a.test/z", { signal: AbortSignal.abort("pre") });
  expect(request.signal.aborted).toBe(true);
  expect(String(request.signal.reason)).toBe("pre");
});
test("two requests from one source both follow it", () => {
  const controller = new AbortController();
  const first = new Request("https://a.test/1", { signal: controller.signal });
  const second = new Request("https://a.test/2", { signal: controller.signal });
  expect(first.signal === second.signal).toBe(false);
  controller.abort("both");
  expect(first.signal.aborted).toBe(true);
  expect(second.signal.aborted).toBe(true);
  expect(String(second.signal.reason)).toBe("both");
});
"#;
    run_abort_reason_fixture(source, "abort_request_signal");
}
