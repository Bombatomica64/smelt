//! Runtime execution tests for the JavaScript `this` receiver.
//!
//! `this` is supplied by the CALL, not by the definition site: one plain
//! function sees a different receiver depending on how it was reached.
//!
//! ```js
//! function who() { return this; }
//! const o = { who };
//! o.who();                 // o
//! who.call(a);             // a
//! who.apply(b, []);        // b
//! who.bind(c)();           // c
//! who();                   // undefined
//! ```
//!
//! Smelt used to have no model for it at all. An unbound `this` was listed with
//! the ambient globals, so it lowered through `module_global_expression` and,
//! for `Type::Unknown`, fabricated an EMPTY OBJECT LITERAL: `return this` in a
//! plain function silently answered `{}` with no diagnostic. The receiver-
//! supplying spellings dropped their leading operand entirely, so
//! `fn.call(thisArg, ..)`, `fn.apply(thisArg, ..)` and `fn.bind(thisArg)` all
//! discarded the context they were given.
//!
//! It is now a dynamically scoped channel: `ExprKind::BindThis` installs a
//! receiver for the duration of one call (restoring the previous binding on
//! scope exit, unwind included) and `ExprKind::ThisRead` reads whatever the
//! innermost active call installed, answering `undefined` for a plain call.
//! That the wiring is EMITTED is covered by the `smelt-frontend-ts` and
//! `smelt-codegen-rust` string goldens; only running the program proves the
//! receiver actually arrives.
//!
//! Two gaps are deliberately OUTSIDE these fixtures, and are gaps in the model
//! rather than in the tests:
//!
//! * The METHOD-call spelling is exercised by the wrapper fixture, whose field
//!   holds an erased-rest callable. A member call whose callee kept a CONCRETE parameter list is invoked as a
//!   typed Rust closure, and its arguments (`&mut` structural parameters
//!   included) travel by that call site's own ABI, so interposing a bound
//!   wrapper would change how the callee is CALLED and drop caller-visible
//!   mutations. Such a call is left unbound -- see `bind_member_call_receiver`
//!   in `smelt-frontend-ts`. The fixtures therefore declare their method fields
//!   at the erased-rest type, which is the surface that carries a receiver.
//! * `fn.apply(thisArg, argsArray)` onto a callee that kept a concrete
//!   parameter list miscompiles its ARGUMENT LIST for reasons unrelated to
//!   `this` (it survives a null receiver, where no bind is emitted at all), so
//!   `apply`'s receiver is exercised here only through an erased-rest callee.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test this_receiver_runtime -- --ignored
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
        "generated `this` receiver test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-this-receiver-runtime-{}-{seq}",
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
fn every_receiver_supplying_spelling_reaches_the_callee() {
    // One callable, reached five ways. The `{}` bug made the first four
    // indistinguishable from each other AND from the receiver-less call, so the
    // interesting assertions are the ones that tell them apart.
    let source = r"
import { test, expect } from 'vitest';

function greeting(this: any, mark: string): unknown {
  if (this === undefined) {
    return `none${mark}`;
  }
  return `${(this as any).msg}${mark}`;
}

const right = { msg: 'right' };

test('call binds its leading operand', () => {
  expect(greeting.call(right, '!')).toBe('right!');
});
test('bind binds its receiver for every later call', () => {
  const bound = greeting.bind(right);
  expect(bound('1')).toBe('right1');
  expect(bound('2')).toBe('right2');
});
test('a receiver-less call sees no receiver', () => {
  expect(greeting('!')).toBe('none!');
});
test('the binding does not outlive the call that installed it', () => {
  expect(greeting.call(right, '!')).toBe('right!');
  expect(greeting('!')).toBe('none!');
});
";
    run_fixture(source, "this_receiver_spellings");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrapper_forwards_the_receiver_it_was_called_with() {
    // The es-toolkit `ary`/`unary`/`spread` shape: a wrapper reads its own
    // `this` and hands it to the wrapped callable through `apply`. Two
    // structurally unrelated receivers reach ONE wrapper allocation, which is
    // why no concrete type, union arm, or scoped generic can carry the
    // receiver -- the wrapper's signature is fixed before either is known.
    let source = r"
import { test, expect } from 'vitest';

function forward(func: (...args: any[]) => any): (...args: any[]) => any {
  return function (this: any, ...args: any[]) {
    return func.apply(this, args);
  };
}

const wrapped = forward(function (this: any, suffix: string): unknown {
  if (this === undefined) {
    return `none${suffix}`;
  }
  return `bound${suffix}`;
});

const first = { run: wrapped };
const second = { run: wrapped };

test('a wrapper hands its own receiver to the wrapped callable', () => {
  expect(first.run('!')).toBe('bound!');
  expect(second.run('?')).toBe('bound?');
});
test('the binding is restored after the inner call returns', () => {
  expect(first.run('1')).toBe('bound1');
  expect(wrapped('2')).toBe('none2');
});
";
    run_fixture(source, "this_receiver_wrapper");
}
