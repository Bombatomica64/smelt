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

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_erased_handle_receives_the_source_arguments_not_one_packed_array() {
    // The two callable ABIs disagree about what a rest argument list *is*.
    // `&dyn Fn(SmeltList<SmeltUnknown>) -> _` takes the packed list as its one
    // parameter, so lowering hands the emitter a single `SmeltList` operand
    // standing for all N source arguments. `SmeltErasedFunction::call` takes the
    // *argument vector*. Erasing the packed list into a single vector element
    // produced `.call(vec![SmeltUnknown::Array([3, 4])])` — one argument that
    // happens to be an array, not two arguments. That COMPILES and returns a
    // plausible value, so only an arity/identity assertion catches it.
    //
    // Both call forms are exercised: `guarded` goes through the unwind-carrying
    // terminator form (where the defect was) and `plain` through the statement
    // form (which was already right), and both must agree.
    let source = r#"
import { test, expect } from "vitest";

function guardedViaLocal(cb: (...args: unknown[]) => unknown): unknown {
  const g = cb;
  try {
    return g(3, 4);
  } catch (err) {
    return "caught";
  }
}

function plainViaLocal(cb: (...args: unknown[]) => unknown): unknown {
  const g = cb;
  return g(3, 4);
}

test("an erased handle receives each source argument separately", () => {
  const arity = (...args: unknown[]): unknown => args.length;
  const first = (...args: unknown[]): unknown => args[0];
  const second = (...args: unknown[]): unknown => args[1];

  // Arity: two source arguments must arrive as two arguments.
  expect(guardedViaLocal(arity)).toBe(2);
  expect(plainViaLocal(arity)).toBe(2);

  // Identity: each element must be the argument that was written, in order.
  expect(guardedViaLocal(first)).toBe(3);
  expect(guardedViaLocal(second)).toBe(4);
  expect(plainViaLocal(first)).toBe(3);
  expect(plainViaLocal(second)).toBe(4);
});
"#;
    run_fixture(source, "smelt_erased_handle_argument_vector");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_throwing_arrow_adapted_into_a_non_throwing_slot_still_runs() {
    // A throwing arrow whose body ONLY throws has the uninhabited return type
    // `never`. Every coercion out of `never` renders a bare constant, because
    // there is no value to convert — and the callback adapter that bridges the
    // arrow into a non-throwing `&dyn Fn() -> unknown` parameter used to return
    // that constant as its whole body:
    //
    // ```rust
    // attempt(&mut { let _smelt_adapted_callback = ..; move || SmeltUnknown::Null })
    // ```
    //
    // The wrapped callback is never mentioned, so it is never called: the throw
    // does not happen, the caller's handler never fires, and `attempt` reports
    // success with a null result. Nothing about the generated Rust looks wrong —
    // it compiles and returns a plausible value — so only execution catches it.
    // This is es-toolkit's `attempt(() => { throw new Error('test') })`.
    //
    // The non-throwing arrow in the same position is asserted beside it: that
    // shape always worked, and it is what proves the fix did not simply route
    // every adapter through a new path.
    let source = r#"
import { test, expect } from "vitest";

function attempt(func: () => unknown): unknown[] {
  try {
    return [null, func()];
  } catch (error) {
    return ["caught", null];
  }
}

test("a throwing arrow adapted into a non-throwing parameter still throws", () => {
  const thrown = attempt(() => {
    throw new Error("boom");
  });
  expect(thrown[0]).toBe("caught");

  const returned = attempt(() => 7);
  expect(returned[0]).toBeNull();
  expect(returned[1]).toBe(7);
});
"#;
    run_fixture(source, "smelt_never_arrow_adapter_runs");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_throwing_arrow_reaches_a_throwing_parameter_unchanged() {
    // The same throwing arrow where the parameter itself propagates: the callee
    // has no handler of its own, so the throw must travel through it to the
    // caller's `catch`. The adapter fix must not turn a propagated throw into a
    // swallowed one (or into a value), so both the propagating and the
    // non-throwing case are asserted against the same callee.
    let source = r#"
import { test, expect } from "vitest";

function forward(func: () => unknown): unknown {
  return func();
}

test("a throwing arrow propagates through a callee that does not catch", () => {
  let outcome = "not run";
  try {
    forward(() => {
      throw new Error("boom");
    });
  } catch (error) {
    outcome = "caught";
  }
  expect(outcome).toBe("caught");
  expect(forward(() => 7)).toBe(7);
});
"#;
    run_fixture(source, "smelt_never_arrow_propagates");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_void_arrow_adapted_into_an_optional_slot_still_runs() {
    // The `never` case's sibling: a source callback with no value to convert
    // because it returns `void`, not because it diverges. es-toolkit's
    // `isMatch` passes `() => undefined` into a `boolean | undefined`
    // customizer slot, so the coercion answered with that slot's missing-value
    // constant `None::<bool>` and dropped the call. Returning `undefined` IS
    // the right answer for a `void` customizer — "no opinion, fall back to the
    // structural comparison" — but the callback still has to run.
    //
    // The counter is the whole point: the constant and the call agree on the
    // returned value, so only an observed side effect distinguishes a called
    // callback from a discarded one.
    let source = r#"
import { test, expect } from "vitest";

let calls = 0;

function matchWith(
  a: unknown,
  b: unknown,
  customizer: (a: unknown, b: unknown) => boolean | undefined
): boolean {
  const decided = customizer(a, b);
  if (decided === undefined) {
    return a === b;
  }
  return decided;
}

test("a void customizer is invoked even though it decides nothing", () => {
  const result = matchWith(1, 1, () => {
    calls++;
    return undefined;
  });

  expect(result).toBe(true);
  expect(calls).toBe(1);
});
"#;
    run_fixture(source, "smelt_void_arrow_optional_adapter");
}
