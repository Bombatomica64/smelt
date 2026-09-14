//! Runtime execution tests for a modeled global NAMESPACE used as a value.
//!
//! These globals are normally consumed through their members —
//! `Math.max(..)`, `JSON.stringify(..)`, `crypto.subtle.digest(..)` — and those
//! calls have their own rules. What was missing was the value: three answers
//! that hold of every namespace object whatever Smelt models of its members.
//! Measured against Node 22 before the fix:
//!
//! | expression | Node | Smelt (before) |
//! | --- | --- | --- |
//! | `typeof crypto` | `object` | `undefined` (folded, per-name special case) |
//! | `crypto` as a value | the object | `unresolved identifier crypto` |
//! | `crypto.subtle` | the object | `unresolved identifier crypto` |
//! | `typeof crypto.subtle` | `object` | — |
//! | `crypto === undefined` | `false` | — |
//!
//! The `typeof` fold was the worst of them: the object is unconditionally
//! present in the target profile — which is what
//! `global_member_presence("crypto")` has said since the `WebCrypto` surface
//! landed — so `typeof crypto === "undefined"` folded TRUE and every program
//! guarding on it took its no-crypto branch, silently.
//!
//! Why this tier and not the examples corpus: a namespace object is an erased
//! host value ON PURPOSE. It has no static Rust shape — its members are reached
//! through modeled calls rather than fields, and es-toolkit's
//! `isPlainObject(Math)` depends on observing the host identity at run time
//! (`typeof` is `"object"` yet it is not a plain object). Every line that holds
//! one is therefore erased, and the examples corpus keeps a
//! zero-avoidable-erasure invariant, so the behaviour is pinned here where an
//! erased host value is the point. Same split as
//! `json_stringify_runtime.rs`'s byte shapes.
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test global_namespace_value_runtime -- --ignored
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
        "generated global-namespace test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-global-namespace-{}-{seq}", std::process::id()))
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
    if std::env::var_os("SMELT_KEEP_RUNTIME_SCRATCH").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn every_registry_namespace_is_a_present_object() {
    // One rule, every entry: the answers must not depend on how much of a
    // namespace's member surface Smelt happens to model.
    let source = r#"
import { test, expect } from "vitest";
test("every registry namespace is a present object", () => {
  expect(typeof Math).toBe("object");
  expect(typeof JSON).toBe("object");
  expect(typeof Reflect).toBe("object");
  expect(typeof Intl).toBe("object");
  expect(typeof crypto).toBe("object");

  expect(Boolean(Math)).toBe(true);
  expect(Boolean(JSON)).toBe(true);
  expect(Boolean(crypto)).toBe(true);

  expect(Math === undefined).toBe(false);
  expect(crypto === undefined).toBe(false);
  expect(typeof crypto === "undefined").toBe(false);
});
"#;
    run_fixture(source, "smelt_namespace_present_object");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_namespace_valued_member_is_a_present_object_too() {
    // Hono's `createHash` guard: an `undefined` member read would take the else
    // branch and skip the digest Smelt does model. Both spellings of the same
    // global answer alike.
    let source = r#"
import { test, expect } from "vitest";
test("a namespace-valued member is a present object too", () => {
  expect(typeof crypto.subtle).toBe("object");
  expect(typeof globalThis.crypto).toBe("object");
  expect(crypto.subtle === undefined).toBe(false);

  let reached = false;
  if (crypto && crypto.subtle) {
    reached = true;
  }
  expect(reached).toBe(true);
});
"#;
    run_fixture(source, "smelt_namespace_member_object");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_member_calls_and_local_shadowing_are_unchanged() {
    // The value rule must not touch the calls, and a local binding of the same
    // name keeps its own meaning — a global is only a global where nothing
    // else declares the name.
    let source = r#"
import { test, expect } from "vitest";
function describe(): string {
  const crypto = { subtle: "local" };
  return crypto.subtle;
}
test("the member calls and local shadowing are unchanged", () => {
  expect(Math.max(1, 2)).toBe(2);
  expect(JSON.stringify({ a: 1 })).toBe('{"a":1}');
  expect(describe()).toBe("local");
});
"#;
    run_fixture(source, "smelt_namespace_calls_unchanged");
}
