//! Runtime execution tests for a CONDITIONAL throw and for a throw that leaves
//! through an `await` — H70 and the defect found under it.
//!
//! The subject is radash's `guard` verbatim, driven the way `async.test.ts`
//! drives it. Before this round the program printed nothing and exited 101, and
//! two independent defects were in the way. Both are silently-wrong-value
//! defects of the kind only a RUNNING program exposes, which is why they live in
//! a runtime tier rather than in an emitter golden.
//!
//! 1. **The guard's condition vanished.** `(err) => { if (c) { throw err }
//!    return undefined }` lowered through the compact callback IR, where
//!    `CallbackExprKind::Throw` emits a `Stmt::Throw` STATEMENT rather than a
//!    value. Inside a ternary arm the statement hoists out of the guard, so the
//!    whole closure became an unconditional `throw err` — the condition, the
//!    branch and the tail return all gone — and `guard` rethrew every error it
//!    was written to swallow. A conditionally-evaluated throw now surfaces the
//!    same fallback-eligible error a conditionally-evaluated capture assignment
//!    does, and the arrow retries through full closure-body lowering, which
//!    emits a real branch.
//!
//! 2. **The rethrow escaped through the `await`.** The binding `const _guard =
//!    (err) => ...` was typed `may_throw: false` before its body was lowered, so
//!    every call that passed it on saw a non-throwing callback. Handed to the
//!    erased promise's `.catch(..)`, the throwing closure was adapted DOWN to
//!    that ABI through the panic route — and a panic route only reaches a source
//!    `catch` when the `catch_unwind` is around the INVOCATION. A promise
//!    continuation is invoked later, during the `await`, where no `catch_unwind`
//!    is in scope, so the throw left the program. The binding now carries the
//!    lowered closure's own `may_throw`, and the error rides the ordinary
//!    `Result` channel the `await` already propagates.
//!
//! Both halves are load-bearing and the fixtures separate them: the swallow case
//! fails without (1) and the rethrow case fails without (2).
//!
//! This shape is deliberately NOT an `examples/typescript/end-to-end` fixture:
//! `guard`'s signature is `TFunction extends () => any` over an `err: any`
//! handler, so the generated crate carries 43 genuinely-`any` erased lines, and
//! that corpus holds a hard avoidable-erasure-zero invariant.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test guarded_throw_runtime -- --ignored
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

/// radash's `guard`, verbatim, plus the async producer the tests drive it with.
///
/// Kept as one string so every fixture below exercises the same source shape
/// the defect was found on rather than a paraphrase of it.
const GUARD: &str = r#"
import { test, expect } from "vitest";

const guard = <TFunction extends () => any>(
  func: TFunction,
  shouldGuard?: (err: any) => boolean,
): ReturnType<TFunction> extends Promise<any>
  ? Promise<Awaited<ReturnType<TFunction>> | undefined>
  : ReturnType<TFunction> | undefined => {
  const _guard = (err: any) => {
    if (shouldGuard && !shouldGuard(err)) {
      throw err;
    }
    return undefined as any;
  };
  const isPromise = (result: any): result is Promise<any> =>
    result instanceof Promise;
  try {
    const result = func();
    return isPromise(result) ? result.catch(_guard) : result;
  } catch (err) {
    return _guard(err);
  }
};

const makeFetchUser = (id: string) => async (): Promise<string> => {
  if (id === "missing") {
    throw new Error("not found");
  }
  if (id === "boom") {
    throw new Error("unknown error");
  }
  return `user${id}`;
};

async function fetchUser(id: string): Promise<string> {
  const user = await guard(
    makeFetchUser(id),
    (err) => err.message === "not found",
  );
  return user ?? "default-user";
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
        "generated guarded-throw test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-guarded-throw-runtime-{}-{seq}",
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
    // removed on the FAILURE path too; `SMELT_KEEP_RUNTIME_SCRATCH=1` keeps it.
    if std::env::var_os("SMELT_KEEP_RUNTIME_SCRATCH").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_guard_swallows_the_error_its_predicate_accepts() {
    // Defect 1. `shouldGuard` answers true for this error, so `_guard` must
    // return `undefined` and the caller's `??` must apply. With the guarded
    // throw hoisted out of its ternary the closure rethrew unconditionally and
    // this value never arrived.
    //
    // The no-throw case is asserted beside it so a fix that made every guarded
    // call swallow would not pass either.
    let source = format!(
        r#"{GUARD}
test("a guard swallows the error its predicate accepts", async () => {{
  expect(await fetchUser("1")).toBe("user1");
  expect(await fetchUser("missing")).toBe("default-user");
}});
"#
    );
    run_fixture(&source, "smelt_guard_swallows");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_guarded_rethrow_reaches_the_source_catch_through_an_await() {
    // Defect 2, and H70 proper. `shouldGuard` answers false, so `_guard`
    // rethrows — from inside a promise continuation, which runs during the
    // `await` and not at the call the `try` wraps. The rethrow has to leave
    // through the `await`, propagate out of `fetchUser`, and land in this
    // `catch`. Routed through a panic instead, it left the process (exit 101)
    // with nothing printed.
    //
    // `error.message` is asserted rather than just "something was caught":
    // the payload has to survive the whole path for `guard`'s contract to hold.
    let source = format!(
        r#"{GUARD}
test("a guarded rethrow reaches the source catch through an await", async () => {{
  let caught = "none";
  try {{
    await fetchUser("boom");
  }} catch (error) {{
    caught = error instanceof Error ? error.message : "other";
  }}
  expect(caught).toBe("unknown error");
}});
"#
    );
    run_fixture(&source, "smelt_guarded_rethrow");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_conditional_throw_keeps_its_branch_in_a_plain_closure() {
    // The minimal shape of defect 1, with no promise and no `await` in it, so a
    // later change that fixes only the async half still fails here. The tail
    // return and BOTH sides of the condition are asserted: the collapse lost all
    // three.
    let source = r#"
import { test, expect } from "vitest";

function classify(value: number, strict: boolean): string {
  const check = (n: number): string => {
    if (strict && n < 0) {
      throw new Error("negative");
    }
    return "ok";
  };
  try {
    return check(value);
  } catch (error) {
    return error instanceof Error ? error.message : "other";
  }
}

test("a conditional throw keeps its branch", () => {
  expect(classify(1, true)).toBe("ok");
  expect(classify(-1, true)).toBe("negative");
  expect(classify(-1, false)).toBe("ok");
});
"#;
    run_fixture(source, "smelt_conditional_throw_branch");
}
