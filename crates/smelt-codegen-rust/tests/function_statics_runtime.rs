//! Runtime execution tests for static properties on function declarations.
//!
//! JavaScript functions are objects, so a module can hang a value off one. It is
//! how es-toolkit publishes its placeholder sentinels — `partial.placeholder`,
//! `partialRight.placeholder`, `curry.placeholder`, `curryRight.placeholder`,
//! `bind.placeholder`, `bindKey.placeholder` — and `memoize.Cache = Map`.
//!
//! The assignment used to lower into the module-init body, which nothing calls,
//! with the target dropped outright, so every read answered `null`. A sentinel that
//! reads `null` is worse than a missing one: `partial(fn, placeholder, 'b',
//! placeholder)` filled the placeholder slots with a real argument instead of
//! skipping them, and the spec saw a plausible-but-wrong argument list.
//!
//! What matters at runtime is IDENTITY: the value read through the member spelling,
//! the value read through destructuring, and the value the function's own body
//! compares against must all be one thing. A string golden
//! (`a_static_property_on_a_function_declaration_resolves`) can prove the read no
//! longer answers null; only running the program proves the three agree.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test function_statics_runtime -- --ignored
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
        "generated function-statics test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-function-statics-runtime-{}-{seq}",
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
fn a_function_static_keeps_one_identity_across_spellings() {
    // The sentinel has to be the SAME value however it is reached, because the
    // consumer compares against it by identity. The last case is the shape that
    // actually failed: the function's own body reads its own static property and
    // compares it to the argument the caller passed.
    let source = r"
import { test, expect } from 'vitest';

function pick(value: unknown): unknown {
  return value === pick.sentinel ? 'sentinel' : value;
}

pick.sentinel = Symbol('pick.sentinel');
pick.fallback = 'none';

test('the member spelling and destructuring agree', () => {
  const { sentinel } = pick;
  expect(sentinel).toBe(pick.sentinel);
});
test('a non-symbol static resolves too', () => {
  expect(pick.fallback).toBe('none');
});
test('the function body recognizes its own sentinel', () => {
  expect(pick(pick.sentinel)).toBe('sentinel');
  expect(pick('a')).toBe('a');
});
test('two different statics are distinct', () => {
  expect(pick.sentinel === (pick.fallback as unknown)).toBe(false);
});
";
    run_fixture(source, "smelt_function_static_identity");
}
