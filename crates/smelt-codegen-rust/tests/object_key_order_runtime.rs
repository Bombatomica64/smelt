//! Runtime execution tests for JavaScript own-property key order.
//!
//! An object's key order is observable through `Object.keys`, `Object.entries`,
//! `for...in` and `JSON.stringify`, so it has to survive every hop a value takes
//! through the generated runtime. Two defects broke that:
//!
//! 1. `SmeltObject` carried an `order` vector, but its constructors took an
//!    unordered `HashMap` and recovered the order by SORTING the keys. Erasing a
//!    record collected the record's ordered `iter()` into that map, so
//!    `{ foo: 1, bar: 2, baz: 3 }` came back as `["bar", "baz", "foo"]` —
//!    alphabetical, which is not any JavaScript order. es-toolkit's `findKey`
//!    ("return the key of the FIRST element that satisfies the predicate")
//!    read the wrong key out of that.
//! 2. Both containers appended new keys, i.e. they modelled plain insertion
//!    order. JavaScript's `OrdinaryOwnPropertyKeys` puts array-index keys FIRST
//!    in ascending numeric order and only then the string keys in insertion
//!    order, so `{ b: 1, 2: "x", a: 3, 1: "y" }` enumerates as `1, 2, b, a`.
//!
//! String-golden tests prove the emitted constructors take an ordered entry
//! sequence; only executing a program proves the order a real value carries.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test object_key_order_runtime -- --ignored
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
        "generated key-order test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-object-key-order-runtime-{}-{seq}",
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
fn erasing_a_record_keeps_its_insertion_order() {
    // The `findKey` shape: the answer is the FIRST key, so an alphabetised
    // enumeration silently returns a different (still plausible) key. The record
    // is erased to an object on the way into the helper, which is exactly where
    // the order used to be re-derived by sorting.
    let source = r#"
import { test, expect } from "vitest";
function firstKey(value: unknown): string {
  return Object.keys(value as object)[0];
}
function keysOf(value: unknown): string[] {
  return Object.keys(value as object);
}
test("an erased record enumerates in insertion order", () => {
  const plain = { foo: 1, bar: 2, baz: 3 };
  expect(keysOf(plain)).toEqual(["foo", "bar", "baz"]);
  expect(firstKey(plain)).toBe("foo");
});
test("insertion order survives JSON.stringify and for...in", () => {
  const plain = { zeta: "z", alpha: "a", mid: "m" };
  expect(JSON.stringify(plain)).toBe('{"zeta":"z","alpha":"a","mid":"m"}');
  const seen: string[] = [];
  for (const key in plain) {
    seen.push(key);
  }
  expect(seen).toEqual(["zeta", "alpha", "mid"]);
});
"#;
    run_fixture(source, "smelt_object_key_order_insertion");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn array_index_keys_enumerate_before_string_keys() {
    // `OrdinaryOwnPropertyKeys`: canonical array indices ascend first, then the
    // string keys in insertion order. Only the canonical decimal spelling of an
    // index counts, so "01" and "1.5" stay string keys in insertion position.
    let source = r#"
import { test, expect } from "vitest";
function keysOf(value: unknown): string[] {
  return Object.keys(value as object);
}
test("integer-like keys lead in ascending order", () => {
  const mixed = { b: 1, 2: "x", a: 3, 1: "y" };
  expect(keysOf(mixed)).toEqual(["1", "2", "b", "a"]);
  const seen: string[] = [];
  for (const key in mixed) {
    seen.push(key);
  }
  expect(seen).toEqual(["1", "2", "b", "a"]);
});
test("only canonical index spellings jump the queue", () => {
  const odd = { b: 1, "01": 2, 10: 3, "1.5": 4, 2: 5 };
  expect(keysOf(odd)).toEqual(["2", "10", "b", "01", "1.5"]);
});
"#;
    run_fixture(source, "smelt_object_key_order_index_keys");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn deleting_and_re_adding_a_key_moves_it_to_the_end() {
    // A re-added string key is a NEW property, so it takes the last position
    // rather than its original one — the difference between remembering the
    // order and reconstructing it.
    // `keysOf` takes `unknown`, so the record reaches `Object.keys` through the
    // erased object representation — the hop that used to re-sort the keys.
    let source = r#"
import { test, expect } from "vitest";
function keysOf(value: unknown): string[] {
  return Object.keys(value as object);
}
test("a re-added key is appended", () => {
  const bag: Record<string, number> = { a: 1, b: 2, c: 3 };
  delete bag.a;
  bag.a = 9;
  expect(keysOf(bag)).toEqual(["b", "c", "a"]);
});
test("overwriting a key keeps its position", () => {
  const bag: Record<string, number> = { a: 1, b: 2, c: 3 };
  bag.a = 9;
  expect(keysOf(bag)).toEqual(["a", "b", "c"]);
  expect(bag.a).toBe(9);
});
"#;
    run_fixture(source, "smelt_object_key_order_readd");
}
