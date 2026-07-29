//! Runtime execution tests for JavaScript `Map` reference semantics and
//! object-identity (`SameValueZero`) key comparison.
//!
//! The `compile_corpus` tier proves the emitted Rust type-checks; string-golden
//! tests prove a specific shape is emitted. Neither observes runtime behavior.
//! These regression tests prove that `SmeltJsMap` behaves like a real JS `Map`:
//!
//! 1. A `Map` is a REFERENCE value — mutating it through one binding (or after
//!    it has flowed through an expression/function that codegen `.clone()`s) is
//!    visible through every alias. This is the property whose absence made
//!    `isEqualWith`'s cycle guard (`stack.set(a, b)` before recursing) silently
//!    drop its write, so circular inputs recursed forever and aborted the whole
//!    test binary (SIGABRT). Value-copy backing stores cannot express it.
//! 2. Object keys compare by reference identity, not structure: two structurally
//!    equal but distinct object literals are two distinct keys, while the *same*
//!    object is found again by `has`/`get`. Primitives compare by value.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because
//! it compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test map_reference_runtime -- --ignored
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
        "generated map test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-map-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_map_fixture(source: &str, crate_name: &str) {
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
fn map_object_keys_compare_by_identity_not_structure() {
    // Two structurally-equal but distinct object literals are two distinct keys
    // (SameValueZero on objects = reference identity), so the Map holds both.
    // The *same* object is found again by `has`/`get`; a fresh look-alike is not.
    // Primitive keys still compare by value. The keys are typed `unknown` so they
    // flow as erased `SmeltUnknown` — the exact key representation `isEqualWith`'s
    // cycle-guard `stack` uses, where object identity is resolved by
    // `SmeltJsKeyEq::same_js_key`'s object arm (stable `id`), not structure.
    let source = r#"
import { test, expect } from "vitest";
test("distinct object keys held separately", () => {
  const a: unknown = { v: 1 };
  const b: unknown = { v: 1 };
  const m = new Map<unknown, string>();
  m.set(a, "a");
  m.set(b, "b");
  expect(m.size).toBe(2);
});
test("same object key found again", () => {
  const a: unknown = { v: 1 };
  const m = new Map<unknown, string>();
  m.set(a, "a");
  expect(m.has(a)).toBe(true);
  expect(m.get(a)).toBe("a");
});
test("fresh look-alike object key not found", () => {
  const a: unknown = { v: 1 };
  const m = new Map<unknown, string>();
  m.set(a, "a");
  expect(m.has({ v: 1 })).toBe(false);
});
test("primitive keys compare by value", () => {
  const prim = new Map<number, string>();
  prim.set(1, "one");
  prim.set(1, "uno");
  expect(prim.size).toBe(1);
  expect(prim.get(1)).toBe("uno");
});
"#;
    run_map_fixture(source, "smelt_map_identity_keys");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn map_concrete_object_keys_erase_instead_of_folding() {
    // Regression: keys are *concrete* object literals (`const a = { v: 1 }`),
    // not `unknown`, flowing into a `Map<object, string>` whose runtime key type
    // is the erased `SmeltJsMap<SmeltUnknown, String>`. The concrete `SmeltRecord`
    // key must be erased to `SmeltUnknown::Object(..)` at each Map operation.
    // Before the fix, codegen compared the concrete key type against the erased
    // key type for exact equality, found them unequal, and folded every call to a
    // constant: `set` dropped the insert (kept a bare `m.clone()`), `has`
    // emitted the literal `false`, and `get` emitted `Default::default()` (None).
    // A green run proves the erasure adapter fires so the Map behaves like JS.
    let source = r#"
import { test, expect } from "vitest";
test("concrete object keys held separately by identity", () => {
  const a = { v: 1 };
  const b = { v: 1 };
  const m = new Map<object, string>();
  m.set(a, "a");
  m.set(b, "b");
  expect(m.size).toBe(2);
});
test("same concrete object key found again", () => {
  const a = { v: 1 };
  const m = new Map<object, string>();
  m.set(a, "a");
  expect(m.has(a)).toBe(true);
  expect(m.get(a)).toBe("a");
});
test("fresh concrete look-alike object key not found", () => {
  const a = { v: 1 };
  const m = new Map<object, string>();
  m.set(a, "a");
  expect(m.has({ v: 1 })).toBe(false);
  expect(m.get({ v: 1 })).toBe(undefined);
});
test("re-setting the same concrete object key overwrites", () => {
  const a = { v: 1 };
  const m = new Map<object, string>();
  m.set(a, "a");
  m.set(a, "z");
  expect(m.size).toBe(1);
  expect(m.get(a)).toBe("z");
});
"#;
    run_map_fixture(source, "smelt_map_concrete_object_keys");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn map_mutation_is_visible_through_aliases() {
    // A `Map` is a reference: a helper that receives it and mutates it changes
    // the caller's Map. This is the exact shape `isEqualWith` relies on when it
    // records `stack.set(a, b)` in a shared cycle guard before recursing. A
    // value-copy backing store would drop the write and this would read 0.
    let source = r#"
import { test, expect } from "vitest";
function fill(m: Map<string, number>): void {
  m.set("x", 1);
  m.set("y", 2);
}
test("map mutation visible through aliases", () => {
  const m = new Map<string, number>();
  const alias = m;
  fill(m);
  expect(m.size).toBe(2);
  expect(alias.size).toBe(2);
  expect(alias.get("x")).toBe(1);
  expect(alias.get("y")).toBe(2);
});
"#;
    run_map_fixture(source, "smelt_map_alias_mutation");
}
