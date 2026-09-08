//! Runtime execution tests for DESTRUCTURED callback parameters.
//!
//! A destructured parameter is a projection of the argument, and the projection
//! nests: `entries.map(([[, route]]) => route.path)` reads `arg[0][1].path`, and
//! `pairs.map(({ key: { name } }) => name)` reads `arg.key.name`. The compact
//! callback IR bound only ONE level — an array element or an object property
//! whose value was a bare identifier — and rejected anything deeper with
//! `nested callback parameter destructuring needs closure-body lowering`. That
//! blocker is what stopped Hono's `src/request.ts` (`matchedRoutes`,
//! `routePath`) from lowering at all, and the same shape appears in ordinary
//! utility code over `Object.entries`-style tuples.
//!
//! The binder now recurses, so each level wraps the projection built so far and
//! carries the PROJECTED type down: a tuple element gets the tuple's element
//! type, an object property the field's own type. Types are what these tests
//! defend — a binding typed as its container compiles perfectly well and then
//! answers a coerced or defaulted value, which only execution catches.
//!
//! Each case is a TypeScript Vitest test, lowering emits a `#[test]`, and a
//! green `cargo test` on the generated crate means every `expect(...)` held.
//! The expectations are what Node 22 prints for the same source.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test callback_destructuring_runtime -- --ignored
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
        "generated callback-destructuring test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-callback-destructuring-runtime-{}-{seq}",
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
fn a_nested_destructured_callback_parameter_reads_the_nested_value() {
    // The four nesting shapes, all over typed tuples and interfaces so every
    // binding has a resolvable type:
    //
    // * an array pattern inside an array pattern, with an elided element —
    //   Hono's `matchResult[0].map(([[, route]]) => route)`;
    // * an object pattern inside an object pattern;
    // * an object pattern inside an array pattern (an entries loop);
    // * an array pattern inside an object pattern.
    //
    // The one-level shapes are asserted alongside, because the recursion must
    // not change what they already answered.
    let source = r#"
import { test, expect } from "vitest";

interface Route {
  path: string;
}

type Entry = [[number, Route], number];

function routePaths(entries: Entry[]): string[] {
  return entries.map(([[, route]]) => route.path);
}

function ranks(entries: Entry[]): number[] {
  return entries.map(([[rank]]) => rank);
}

interface Key {
  name: string;
  size: number;
}

interface Pair {
  key: Key;
  value: number;
}

function names(pairs: Pair[]): string[] {
  return pairs.map(({ key: { name } }) => name);
}

function sizes(pairs: Pair[]): number[] {
  return pairs.map(({ key: { size } }) => size + 1);
}

interface Flat {
  a: number;
  b: number;
}

function nestedObjectInArray(rows: [Flat, number][]): number[] {
  return rows.map(([{ a, b }, extra]) => a + b + extra);
}

interface Holder {
  label: string;
  scores: number[];
}

function nestedArrayInObject(holders: Holder[]): string[] {
  return holders.map(({ label, scores: [first, second] }) => `${label}:${first + second}`);
}

function tuplePairs(rows: [string, number][]): string[] {
  return rows.map(([key, value]) => `${key}=${value}`);
}

function flatFields(rows: Flat[]): number[] {
  return rows.map(({ a, b }) => a + b);
}

test("an array pattern nested in an array pattern reads the inner element", () => {
  const entries: Entry[] = [
    [[1, { path: "/a" }], 10],
    [[2, { path: "/b" }], 20],
  ];
  expect(routePaths(entries)).toEqual(["/a", "/b"]);
  expect(ranks(entries)).toEqual([1, 2]);
});

test("an object pattern nested in an object pattern reads the inner field", () => {
  const pairs: Pair[] = [
    { key: { name: "n1", size: 1 }, value: 1 },
    { key: { name: "n2", size: 2 }, value: 2 },
  ];
  expect(names(pairs)).toEqual(["n1", "n2"]);
  expect(sizes(pairs)).toEqual([2, 3]);
});

test("patterns nest either way round", () => {
  expect(
    nestedObjectInArray([
      [{ a: 1, b: 2 }, 10],
      [{ a: 3, b: 4 }, 20],
    ])
  ).toEqual([13, 27]);
  expect(
    nestedArrayInObject([
      { label: "x", scores: [1, 2] },
      { label: "y", scores: [3, 4] },
    ])
  ).toEqual(["x:3", "y:7"]);
});

test("one-level destructuring still answers what it did", () => {
  expect(tuplePairs([["a", 1], ["b", 2]])).toEqual(["a=1", "b=2"]);
  expect(flatFields([{ a: 1, b: 2 }, { a: 3, b: 4 }])).toEqual([3, 7]);
});
"#;
    run_fixture(source, "smelt_callback_destructuring_nesting");
}
