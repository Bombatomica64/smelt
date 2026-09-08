//! Runtime execution tests for `===` over the modeled runtime classes.
//!
//! Every class backed by a generated runtime type mints an object id on
//! construction and shares it through `Clone` — that is what makes them
//! JavaScript reference values. `===` has to read that id, and it did not: it
//! fell through to the structural `PartialEq` those types derive for the deep
//! comparisons, so two distinct objects with equal contents compared EQUAL
//! where every JavaScript engine answers `false`.
//!
//! Measured against Node 22 before the fix, across the whole family:
//!
//! ```text
//!                                       Node    Smelt
//! new Blob(["x"])   === new Blob(["x"])  false   true
//! new Headers({..}) === new Headers({..}) false  true
//! new FormData()    === new FormData()   false   true
//! new TextEncoder() === new TextEncoder() false  true
//! new URLSearchParams("a=1") === ...     false   true
//! new Response("b") === new Response("b") false  true
//! new Request(url)  === new Request(url) false   true
//! ```
//!
//! Every one of those is a silently wrong `true`: a program keyed on object
//! identity — a cache, a visited set, a "did this change" check — took the
//! wrong branch with no diagnostic.
//!
//! The cases below assert both directions, because only asserting the fixed
//! one would pass for a `===` that answered `false` unconditionally:
//!
//! 1. **Two fresh objects differ** (`a !== b`), for each modeled class.
//! 2. **An alias is the same object** (`a === a`, and `a === alias`), so the
//!    id travels through `Clone` and through a binding.
//! 3. **`Object.is` and `===` still disagree where the spec says they must**:
//!    `Object.is(NaN, NaN)` is `true` while `NaN === NaN` is `false`, and
//!    `Object.is(-0, 0)` is `false` while `-0 === 0` is `true`. The two are
//!    different HIR operators over one identity helper, and routing `===`
//!    through SameValue's numeric arm would silently invert both.
//! 4. **Structural comparison still works** where JavaScript uses it:
//!    `toEqual` compares contents, so the derived `PartialEq` those types keep
//!    must stay reachable from the deep matchers.
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test reference_identity_runtime -- --ignored
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
        "generated reference-identity test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-reference-identity-runtime-{}-{seq}",
        std::process::id()
    ))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_identity_fixture(source: &str, crate_name: &str) {
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
fn two_fresh_modeled_objects_are_not_the_same_reference() {
    let source = r#"
import { test, expect } from "vitest";
test("a Blob is its own reference", () => {
  const a = new Blob(["x"], { type: "text/plain" });
  const b = new Blob(["x"], { type: "text/plain" });
  const alias = a;
  expect(a === b).toBe(false);
  expect(a !== b).toBe(true);
  expect(a === a).toBe(true);
  expect(a === alias).toBe(true);
});
test("a File is its own reference", () => {
  const a = new File(["x"], "note.txt");
  const b = new File(["x"], "note.txt");
  expect(a === b).toBe(false);
  expect(a === a).toBe(true);
});
test("a Headers list is its own reference", () => {
  const a = new Headers({ a: "1" });
  const b = new Headers({ a: "1" });
  expect(a === b).toBe(false);
  expect(a === a).toBe(true);
});
test("a FormData is its own reference", () => {
  const a = new FormData();
  const b = new FormData();
  expect(a === b).toBe(false);
  expect(a === a).toBe(true);
});
test("URLSearchParams and the text codecs are their own references", () => {
  const p = new URLSearchParams("a=1");
  const p2 = new URLSearchParams("a=1");
  expect(p === p2).toBe(false);
  expect(p === p).toBe(true);
  const encoder = new TextEncoder();
  expect(encoder === new TextEncoder()).toBe(false);
  expect(encoder === encoder).toBe(true);
  const decoder = new TextDecoder();
  expect(decoder === new TextDecoder()).toBe(false);
  expect(decoder === decoder).toBe(true);
});
test("a Response and a Request are their own references", () => {
  const response = new Response("body");
  expect(response === new Response("body")).toBe(false);
  expect(response === response).toBe(true);
  const request = new Request("https://a.test/");
  expect(request === new Request("https://a.test/")).toBe(false);
  expect(request === request).toBe(true);
});
test("a byte view is its own reference", () => {
  const bytes = new TextEncoder().encode("ab");
  const same = bytes;
  expect(bytes === same).toBe(true);
  expect(bytes === new TextEncoder().encode("ab")).toBe(false);
});
"#;
    run_identity_fixture(source, "identity_modeled_classes");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn object_is_and_strict_equality_still_disagree_where_the_spec_says_so() {
    let source = r#"
import { test, expect } from "vitest";
test("NaN is SameValue with itself but not strictly equal", () => {
  const nan = Number.NaN;
  expect(nan === nan).toBe(false);
  expect(Object.is(nan, nan)).toBe(true);
});
test("negative zero is strictly equal to zero but not SameValue", () => {
  const negative = -0;
  const positive = 0;
  expect(negative === positive).toBe(true);
  expect(Object.is(negative, positive)).toBe(false);
});
test("Object.is over references agrees with strict equality", () => {
  const a = new Blob(["x"]);
  const b = new Blob(["x"]);
  expect(Object.is(a, a)).toBe(true);
  expect(Object.is(a, b)).toBe(false);
});
"#;
    run_identity_fixture(source, "identity_same_value");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn structural_comparison_still_reaches_the_contents() {
    let source = r#"
import { test, expect } from "vitest";
test("toEqual compares a byte view's contents, not its identity", () => {
  const bytes = new TextEncoder().encode("a");
  // Two distinct objects, so `===` is false while `toEqual` still holds: the
  // derived `PartialEq` has to stay reachable from the deep matchers.
  expect(bytes === new TextEncoder().encode("a")).toBe(false);
  expect(bytes).toEqual(new TextEncoder().encode("a"));
});
test("a Headers list compares structurally through toEqual", () => {
  const headers = new Headers({ a: "1" });
  expect(headers === new Headers({ a: "1" })).toBe(false);
  expect(headers).toEqual(new Headers({ a: "1" }));
});
"#;
    run_identity_fixture(source, "identity_structural");
}
