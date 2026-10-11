//! Runtime execution tests for the compact callback IR.
//!
//! An inline callback (`xs.map((x) => ...)`) is first lowered through a compact
//! callback IR that keeps the closure's body as a small expression tree; only a
//! fallback-eligible failure retries it as an ordinary closure body. So the
//! compact IR has to give every construct the value the general expression
//! lowering gives it, and where it carried its own reduced copy of a general
//! rule the two drifted. Each case here is a construct that took such a copy
//! and computed a different value (haiku audit §2.4); the expected values are
//! node's.
//!
//! Each case is a TypeScript Vitest test: lowering emits a `#[test]`, and a green
//! `cargo test` on the generated crate means every `expect(...)` held. The tier
//! is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test compact_callback_runtime -- --ignored
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
        "generated compact-callback test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-compact-callback-runtime-{}-{seq}",
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
fn a_callback_array_literal_element_resolves_like_any_identifier() {
    // F0620: the array-literal element arm resolved identifiers with a reduced
    // copy (params, locals, a forward-callable placeholder). Inside a function,
    // a module constant is not a local, so `[x, LIMIT]` became `[x, null]`; and
    // `undefined` in a `flatMap` callback did not lower.
    let source = r#"
import { test, expect } from "vitest";
const LIMIT = 3;
function pairs(xs: number[]): number[][] {
  return xs.map((x) => [x, LIMIT]);
}
function flat(xs: number[]): number[] {
  return xs.flatMap((x) => [x, LIMIT]);
}
function holes(xs: number[]): number {
  return xs.flatMap((x) => [x, undefined]).length;
}
test("a module constant in a callback array literal keeps its value", () => {
  const p = pairs([10, 20]);
  expect(p.length).toBe(2);
  expect(p[0][1]).toBe(3);
  expect(p[1][0] + p[1][1]).toBe(23);
  expect(flat([10, 20]).join(",")).toBe("10,3,20,3");
});
test("undefined is an element of a flatMap callback array literal", () => {
  expect(holes([10, 20])).toBe(4);
});
"#;
    run_fixture(source, "compact_callback_array_identifier");
}
