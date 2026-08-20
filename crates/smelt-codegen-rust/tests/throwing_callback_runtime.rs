//! Runtime execution tests for calling a *function-typed value* that throws.
//!
//! The Rust type emitted for a `Type::Function` value must agree with its MIR
//! `FunctionType` in all three refinements — the erased-unknown-rest shape,
//! `may_throw`, and a `Future` return — at every rendering site. Four defects
//! all fell out of one disagreement between the canonical renderer and the
//! sites that re-derived it:
//!
//! 1. a throwing callback lost its `Result` at the parameter boundary, so its
//!    error became a `panic!`;
//! 2. a borrowed erased-rest callback bound to a local was emitted as a bare
//!    `Rc<closure>` while its call site used the `SmeltErasedFunction::call`
//!    ABI (E0658 `fn_traits` + E0308);
//! 3. `ExprKind::ClosureCall` took the unwind-carrying `Terminator::Call` form
//!    only when the callee type's `may_throw` was set — but a callback
//!    parameter of unknown provenance has `may_throw == false`, so every
//!    `try { cb(..) } catch { .. }` discarded its handler outright;
//! 4. the two call ABIs (statement form and terminator form) were decided in
//!    two places, differently, so routing a call between them flipped its ABI.
//!
//! Defect 3 is the load-bearing one and only *execution* shows it: the emitted
//! code compiled perfectly well, it simply never ran the `catch`. A string
//! golden asserting on the call text looked healthy the whole time.
//!
//! Each case is a TypeScript Vitest test, lowering emits a `#[test]`, and a
//! green `cargo test` on the generated crate means every `expect(...)` held.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test throwing_callback_runtime -- --ignored
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
        "generated throwing-callback test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-throwing-callback-runtime-{}-{seq}",
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
fn a_throwing_callbacks_error_reaches_the_catch_clause() {
    // The load-bearing case (defect 1 + defect 3). `guard` calls a callback
    // *parameter* whose declared type says nothing about throwing, which is
    // exactly why the source wrapped the call in `try`. The handler used to be
    // discarded at MIR lowering, so the first throw aborted the process before
    // any assertion could run.
    //
    // Both branches are asserted, plus the call count: `calls == 2` is what
    // proves the second call really ran after the first one threw, rather than
    // the whole test being one long-jump away from its assertions.
    let source = r#"
import { test, expect } from "vitest";

test("a throwing callback is caught by the caller's try/catch", () => {
  let calls = 0;

  function thrower(x: number): string {
    calls++;
    if (x > 0) {
      throw new Error("boom");
    }
    return "ok";
  }

  function guard(cb: (x: number) => string, v: number): string {
    try {
      return cb(v);
    } catch (err) {
      return "caught";
    }
  }

  expect(guard(thrower, 1)).toBe("caught");
  expect(guard(thrower, -1)).toBe("ok");
  expect(calls).toBe(2);
});
"#;
    run_fixture(source, "smelt_throwing_callback_caught");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_erased_rest_callback_bound_to_a_local_is_callable() {
    // Defect 2. `const g = cb` binds a borrowed erased-rest callback parameter
    // to a local whose Rust type is the owned `SmeltErasedFunction` struct.
    // Emitting a bare `Rc::new(move |..| cb(..))` for it disagreed with the
    // `.call(..)` ABI the call site used, and the generated crate did not
    // compile at all (E0658 + E0308) — so the assertion below is reached only
    // if the value and its call ABI agree.
    let source = r#"
import { test, expect } from "vitest";

function viaLocal(cb: (...args: unknown[]) => unknown): unknown {
  const g = cb;
  return g(3, 4);
}

test("an erased-rest callback bound to a local can be called", () => {
  const countArgs = (...args: unknown[]): unknown => args.length;

  expect(viaLocal(countArgs)).toBe(2);
});
"#;
    run_fixture(source, "smelt_erased_rest_local_handle");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_try_around_an_erased_rest_callback_parameter_catches() {
    // Defect 3 in its erased-rest spelling, which is the shape es-toolkit's
    // `attempt`-style helpers use: the callback is `(...args: unknown[]) =>
    // unknown` precisely because its throw behaviour is not statically known.
    // The `catch` clause was dropped entirely, so the fallback value could
    // never be produced.
    let source = r#"
import { test, expect } from "vitest";

test("a try around an erased-rest callback parameter catches", () => {
  function attempt(cb: (...args: unknown[]) => unknown): unknown {
    try {
      return cb(1);
    } catch (err) {
      return "caught-erased";
    }
  }

  const boom = (...args: unknown[]): unknown => {
    throw new Error("erased boom");
  };
  const echo = (...args: unknown[]): unknown => args.length;

  expect(attempt(boom)).toBe("caught-erased");
  expect(attempt(echo)).toBe(1);
});
"#;
    run_fixture(source, "smelt_erased_rest_try_catch");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn both_call_forms_agree_on_the_erased_rest_abi() {
    // Defect 4. The same erased-rest callable is invoked twice: once outside a
    // `try` (the `Rvalue::ClosureCall` statement form) and once inside one (the
    // `Terminator::Call` form). The two forms used to decide the call ABI
    // independently, so moving a call between them flipped `.call(vec![..])`
    // against a direct invocation. Both calls here must reach their handler and
    // return their value, which only holds if one authority answers the ABI
    // question for both.
    let source = r#"
import { test, expect } from "vitest";

// Statement form: no enclosing handler, so this call lowers to
// `Rvalue::ClosureCall`.
function callErased(cb: (...args: unknown[]) => unknown): unknown {
  return cb(1, 2, 3);
}

// Terminator form: the same borrowed parameter, called under a handler.
function guardErased(cb: (...args: unknown[]) => unknown): string {
  try {
    cb(1);
    return "no throw";
  } catch (err) {
    return "caught";
  }
}

test("both call forms agree on the erased-rest call ABI", () => {
  const echo = (...args: unknown[]): unknown => args.length;
  const boom = (...args: unknown[]): unknown => {
    throw new Error("erased boom");
  };

  expect(callErased(echo)).toBe(3);
  expect(guardErased(boom)).toBe("caught");
  expect(guardErased(echo)).toBe("no throw");
});
"#;
    run_fixture(source, "smelt_erased_rest_abi_agreement");
}
