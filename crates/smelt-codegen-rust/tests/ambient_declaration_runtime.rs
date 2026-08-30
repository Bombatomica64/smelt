//! Runtime execution tests for TypeScript ambient (`declare`) value declarations.
//!
//! A `declare let x: T` asserts that the HOST already provides `x`; it never
//! creates a binding. Smelt used to lower it as an ordinary declaration, which
//! minted a module local seeded with the declared type's default AND registered
//! the name as a module global, so every read of it resolved to that default.
//! es-toolkit's `isNode` is the real-world instance: `declare let process: {…} |
//! undefined` made `typeof process !== 'undefined'` fold to `false` before any
//! host lookup could happen, so `isNode()` answered the opposite of the target
//! profile it was compiled for.
//!
//! Two properties are guarded here.
//!
//! **An ambient declaration does not shadow the host binding.** Declaring
//! `process` ambiently must leave the profile's modeled `process` object visible,
//! so a Node-detection probe reads the profile's answer rather than a fabricated
//! `undefined`.
//!
//! **A statically decided guard does not evaluate its dead operand.** Once the
//! fake binding is gone, `typeof window !== 'undefined' && window?.document` can
//! only be lowered if the dead right operand is never visited — the non-DOM
//! profile provides no `window` on purpose. The observable half of that is
//! ordinary JavaScript short-circuiting, asserted here with a side-effecting
//! operand so the test fails if the dead branch is evaluated.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test ambient_declaration_runtime -- --ignored
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
        "generated ambient-declaration test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-ambient-declaration-runtime-{}-{seq}",
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
fn an_ambient_declaration_does_not_shadow_the_host_binding() {
    // Verbatim shape of es-toolkit's `isNode`. Before the fix the ambient
    // declaration became a module local initialized to `None`, so the `typeof`
    // guard folded to `false` and the function returned `false` in the
    // Node-compatible profile it is compiled for. No concrete type, union or
    // scoped generic can express this: the declaration is a claim ABOUT the host,
    // and the only right answer is to not create a binding at all.
    let source = r"
import { test, expect } from 'vitest';

declare let process:
  | {
      versions?: {
        node?: unknown;
      };
    }
  | undefined;

export function isNode(): boolean {
  return typeof process !== 'undefined' && process?.versions?.node != null;
}

test('an ambiently declared host global is not replaced by a default value', () => {
  expect(isNode()).toBe(true);
});
";
    run_fixture(source, "smelt_ambient_declaration_host_binding");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_statically_decided_guard_skips_its_dead_operand() {
    // `isBrowser`'s shape. The profile is non-DOM, so the guard is statically
    // false and the right operand is dead; the second case proves the general
    // short-circuit rather than the specific fold, by giving the dead operand an
    // observable side effect.
    let source = r"
import { test, expect } from 'vitest';

declare let window:
  | {
      document: unknown;
    }
  | undefined;

export function isBrowser(): boolean {
  return typeof window !== 'undefined' && window?.document != null;
}

let calls = 0;
function tick(): boolean {
  calls += 1;
  return true;
}

test('a dead operand behind an absent-global guard is not evaluated', () => {
  expect(isBrowser()).toBe(false);
});
test('&& does not evaluate its right operand when the left is falsy', () => {
  calls = 0;
  expect(false && tick()).toBe(false);
  expect(calls).toBe(0);
});
";
    run_fixture(source, "smelt_ambient_declaration_dead_operand");
}
