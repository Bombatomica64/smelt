//! Runtime execution tests for the "erased value -> typed list" conversion.
//!
//! Rebuilding a `SmeltList<T>` from a `SmeltUnknown` is one conceptual
//! operation, but it used to be emitted three times with three different arm
//! sets: the `SmeltUnknown`-element and `String`-element forms iterated a source
//! STRING into characters, and the general form did not — it fell through to
//! `panic!("unknown is not array")`. So the same JavaScript value converted fine
//! where the element type happened to be erased (`groupBy`'s adapter) and blew
//! up where it happened to be concrete (`map`'s), which is a hole no hand-written
//! Rust team would leave between two spellings of one conversion.
//!
//! The arms now come from a single emitter helper, so every element type accepts
//! exactly what JavaScript's iteration protocol accepts.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test erased_to_list_arms_runtime -- --ignored
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
        "generated erased-to-list test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-erased-list-runtime-{}-{seq}", std::process::id()))
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
fn a_string_iterates_into_a_list_whose_element_type_is_concrete() {
    // `countTokens` declares `(string | number)[]`, so the erased adapter rebuilds
    // its argument through the GENERAL element-type lowering rather than the
    // erased-element or all-string ones. Handing it a string has to iterate the
    // string's characters, exactly as the sibling lowerings already did; before
    // the arm sets were unified this panicked with "unknown is not array".
    let source = r#"
import { test, expect } from "vitest";

type Token = string | number;

function countTokens(tokens: Token[]): number {
  return tokens.length;
}

function callErased(fn: unknown, value: unknown): unknown {
  const callable = fn as (tokens: unknown) => unknown;
  return callable(value);
}

test("a string iterates into a list whose element type is concrete", () => {
  expect(callErased(countTokens, "abc")).toBe(3);
  expect(callErased(countTokens, [1, "b"])).toBe(2);
  expect(callErased(countTokens, undefined)).toBe(0);
});
"#;
    run_fixture(source, "erased_to_list_string_arm");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_array_extracted_from_an_erased_value_aliases_the_source_array() {
    // A JavaScript array is a reference value, so the `unknown[]` a callee
    // receives across an erased boundary IS the caller's array — a `push`
    // through the callee's handle must be visible to the caller.
    //
    // `SmeltList<SmeltUnknown>` and `SmeltArray` have the identical
    // `id` + `Rc<RefCell<Vec<SmeltUnknown>>>` representation, so the extraction
    // is a re-wrap of one array and not the construction of a second. It used to
    // rebuild the element vector into a fresh `Rc`, which kept the identity but
    // detached the storage: the write below landed in a copy that died with the
    // call, and the caller observed `[1, 2, 3]`. (That rebuild was also an O(n)
    // memcpy per crossing, which is what made a per-callback erased dispatcher
    // quadratic.)
    let source = r#"
import { test, expect } from "vitest";

function pushInto(values: unknown[]): number {
  values.push(99);
  return values.length;
}

function callErased(fn: unknown, value: unknown): unknown {
  const callable = fn as (values: unknown) => unknown;
  return callable(value);
}

test("an array extracted from an erased value aliases the source array", () => {
  const values: unknown[] = [1, 2, 3];
  expect(callErased(pushInto, values)).toBe(4);
  expect(values.length).toBe(4);
  expect(values[3]).toBe(99);
});

test("a non-array source still builds a fresh list", () => {
  // Only the array arm has a source array to alias; every other arm builds a
  // list that did not exist in the source program and so mints a fresh identity.
  const built: unknown[] = [];
  expect(callErased(pushInto, "ab")).toBe(3);
  expect(callErased(pushInto, undefined)).toBe(1);
  expect(built.length).toBe(0);
});
"#;
    run_fixture(source, "erased_to_list_aliasing");
}
