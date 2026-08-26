//! Runtime execution tests for the VALUE a JavaScript `&&` / `||` produces.
//!
//! JavaScript's logical operators are selectors, not boolean operators:
//! `a && b` evaluates to `a` when `a` is falsy and to `b` otherwise, and
//! `a || b` evaluates to `a` when `a` is truthy and to `b` otherwise. The static
//! type of the whole expression is the union of the operand types, not
//! `boolean`.
//!
//! Smelt modelled both as boolean operators, so every value-position use threw
//! its operand away. The failure is invisible in the generated Rust — a `bool`
//! is a perfectly good type — and it silently *strengthens* into a fold:
//! es-toolkit's
//!
//! ```ts
//! expect(error instanceof Error && error.message).toBe('test');
//! ```
//!
//! became a `bool` compared against a string. That comparison is statically
//! false, so the whole assertion folded to `!(false)`: a test that could neither
//! pass nor fail on anything real. Only execution tells the two apart.
//!
//! The boolean case is asserted alongside, because the fix must NOT widen it:
//! when both operands are already boolean the union of the operand types is
//! `bool`, and routing ordinary guards through a union (or through
//! `SmeltUnknown`) would be exactly the erasure the ABI rules forbid.
//!
//! Each case is a TypeScript Vitest test, lowering emits a `#[test]`, and a
//! green `cargo test` on the generated crate means every `expect(...)` held.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test logical_operand_runtime -- --ignored
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
        "generated logical-operand test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-logical-operand-runtime-{}-{seq}",
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
fn logical_operators_yield_an_operand_value() {
    // The five shapes the operators have to get right, all in one crate:
    //
    // * `bool && bool` / `bool || bool` — must stay a plain boolean;
    // * `guard && value` — the es-toolkit shape, must be the value;
    // * `nullable || fallback` — must be the present value, or the fallback;
    // * mixed operand types — must select the operand, not a boolean;
    // * a chain — must associate left and yield the last surviving operand.
    //
    // Every assertion below is written from what Node prints for the same
    // expression, so a boolean model fails them rather than agreeing by
    // accident.
    let source = r#"
import { test, expect } from "vitest";

function bothBooleans(a: boolean, b: boolean): boolean {
  return a && b;
}

function eitherBoolean(a: boolean, b: boolean): boolean {
  return a || b;
}

function guardedMessage(error: unknown): unknown {
  return error instanceof Error && error.message;
}

function withFallback(name: string | null): string {
  return name || "fallback";
}

function mixed(a: string, b: number): string | number {
  return a && b;
}

function chain(a: string, b: string, c: string): string {
  return a && b && c;
}

test("boolean operands keep boolean results", () => {
  expect(bothBooleans(true, false)).toBe(false);
  expect(bothBooleans(true, true)).toBe(true);
  expect(eitherBoolean(false, true)).toBe(true);
  expect(eitherBoolean(false, false)).toBe(false);
});

test("a guard selects the guarded value, not a boolean", () => {
  expect(guardedMessage(new Error("test"))).toBe("test");
  expect(guardedMessage(42)).toBe(false);
});

test("a falsy left operand selects the right operand", () => {
  expect(withFallback(null)).toBe("fallback");
  expect(withFallback("given")).toBe("given");
  expect(mixed("", 5)).toBe("");
  expect(mixed("x", 5)).toBe(5);
});

test("a chain yields the last surviving operand", () => {
  expect(chain("a", "b", "c")).toBe("c");
  expect(chain("a", "", "c")).toBe("");
});
"#;
    run_fixture(source, "smelt_logical_operand_values");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_logical_condition_still_short_circuits() {
    // A condition observes only truthiness, so it keeps the branching shape
    // rather than materializing the selected operand — but it must still
    // SHORT-CIRCUIT. Flattening `if (a && b)` into a boolean `&&` over two
    // already-computed temporaries evaluates the right operand unconditionally;
    // the call counters below are what distinguishes the two, since both
    // spellings agree on the answer.
    let source = r#"
import { test, expect } from "vitest";

test("a logical condition does not evaluate the right operand eagerly", () => {
  let left = 0;
  let right = 0;

  function checkLeft(value: boolean): boolean {
    left++;
    return value;
  }

  function checkRight(value: boolean): boolean {
    right++;
    return value;
  }

  if (checkLeft(false) && checkRight(true)) {
    expect(false).toBe(true);
  }
  expect(left).toBe(1);
  expect(right).toBe(0);

  if (checkLeft(true) || checkRight(true)) {
    expect(true).toBe(true);
  }
  expect(left).toBe(2);
  expect(right).toBe(0);
});
"#;
    run_fixture(source, "smelt_logical_condition_short_circuit");
}
