//! Runtime execution tests for symbol-keyed property round trips.
//!
//! A symbol-keyed property lives in an erased record under
//! `"__smelt_symbol:<description>"`, and a symbol *value* is
//! `SmeltUnknown::Symbol(description)`. `Object.getOwnPropertySymbols` broke that
//! correspondence: it returned the bare *descriptions* as `String`s. Reading
//! `source[symbols[i]]` then looked up the unprefixed string key and missed, and
//! writing `target[symbols[i]] = v` created a plain string property that no later
//! symbol lookup or symbol enumeration could find.
//!
//! Two knock-on defects fell out of fixing the element type:
//!
//! 1. `[...Object.keys(o), ...getSymbols(o)]` is a `(string | symbol)[]` —
//!    `List<String>` chained onto `List<Unknown>`. The concat emitter bailed on
//!    mismatched element types and produced an EMPTY list, silently dropping every
//!    string key. It now erases the concrete side's elements instead.
//! 2. es-toolkit's `getSymbols` gates each symbol on
//!    `Object.prototype.propertyIsEnumerable.call(o, s)`, which was read off the
//!    opaque `Object.prototype` sentinel, answered `undefined`, and filtered every
//!    symbol away. That probe now lowers to the own-key check, which is the only
//!    thing Smelt's record model can distinguish.
//!
//! Each case is a TypeScript Vitest test; lowering emits a `#[test]`, so this tier
//! lowers the program to a crate and runs `cargo test` on it — a green run means
//! every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test symbol_key_runtime -- --ignored
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
        "generated symbol-key test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-symbol-key-runtime-{}-{seq}",
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
fn a_reflected_symbol_key_reads_and_writes_the_same_property() {
    // The core round trip. Before the fix `typeof symbols[0]` was `"string"`,
    // `source[symbols[0]]` was `undefined`, and the write landed under a plain
    // string key so `Object.getOwnPropertySymbols(target)` came back empty.
    let source = r#"
import { test, expect } from "vitest";
test("a symbol from getOwnPropertySymbols still indexes the object", () => {
  const key = Symbol("k");
  const source: any = { a: 1 };
  source[key] = 7;
  const symbols = Object.getOwnPropertySymbols(source);
  expect(symbols.length).toBe(1);
  expect(typeof symbols[0]).toBe("symbol");
  expect(source[symbols[0]]).toBe(7);
});
test("writing through a reflected symbol creates a symbol-keyed property", () => {
  const key = Symbol("k");
  const source: any = {};
  source[key] = 1;
  const symbols = Object.getOwnPropertySymbols(source);
  const target: any = {};
  target[symbols[0]] = 9;
  expect(target[key]).toBe(9);
  expect(Object.getOwnPropertySymbols(target).length).toBe(1);
  // A symbol key is never a string key, so it stays out of Object.keys.
  expect(Object.keys(target).length).toBe(0);
});
"#;
    run_fixture(source, "smelt_symbol_key_round_trip");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_mixed_string_and_symbol_key_spread_keeps_both_halves() {
    // `[...Object.keys(o), ...getOwnPropertySymbols(o)]` chains `List<String>`
    // onto `List<Unknown>`. The emitter used to give up on the mismatch and yield
    // an empty list, so es-toolkit's `copyProperties` copied nothing at all.
    let source = r#"
import { test, expect } from "vitest";
test("a string-plus-symbol key spread keeps every key", () => {
  const key = Symbol("k");
  const source: any = { a: 1, b: 2 };
  source[key] = 3;
  const keys: any[] = [...Object.keys(source), ...Object.getOwnPropertySymbols(source)];
  expect(keys.length).toBe(3);
  const target: any = {};
  for (let i = 0; i < keys.length; i++) {
    target[keys[i]] = source[keys[i]];
  }
  expect(target.a).toBe(1);
  expect(target.b).toBe(2);
  expect(target[key]).toBe(3);
  expect(Object.keys(target).length).toBe(2);
  expect(Object.getOwnPropertySymbols(target).length).toBe(1);
});
"#;
    run_fixture(source, "smelt_symbol_key_mixed_spread");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn property_is_enumerable_answers_for_string_and_symbol_keys() {
    // `Object.prototype.propertyIsEnumerable.call(o, k)` used to be read off the
    // opaque prototype sentinel and answered `null`, so es-toolkit's `getSymbols`
    // filtered away every symbol it had just found.
    let source = r#"
import { test, expect } from "vitest";
test("propertyIsEnumerable sees own string and symbol keys", () => {
  const key = Symbol("k");
  const source: any = { a: 1 };
  source[key] = 2;
  expect(Object.prototype.propertyIsEnumerable.call(source, "a")).toBe(true);
  expect(Object.prototype.propertyIsEnumerable.call(source, "missing")).toBe(false);
  expect(Object.prototype.propertyIsEnumerable.call(source, key)).toBe(true);
  expect(source.propertyIsEnumerable("a")).toBe(true);
});
test("the getSymbols idiom keeps enumerable symbol keys", () => {
  const key = Symbol("k");
  const source: any = { a: 1 };
  source[key] = 2;
  const symbols = Object.getOwnPropertySymbols(source).filter(symbol =>
    Object.prototype.propertyIsEnumerable.call(source, symbol)
  );
  expect(symbols.length).toBe(1);
  expect(source[symbols[0]]).toBe(2);
});
"#;
    run_fixture(source, "smelt_symbol_key_property_is_enumerable");
}
