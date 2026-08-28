//! Runtime execution tests for JavaScript array *reference* semantics.
//!
//! In JavaScript an array is a reference value. `const b = a` makes `b` another
//! handle on the same array, so `b.push(x)` is observable through `a`; passing an
//! array into a function passes the handle, so the callee's mutations are visible
//! to the caller; and an array reached back out of an object, a `Map`, or an outer
//! array is that same array, not a copy.
//!
//! A typed array lowers to `SmeltList<T>`. Whether `SmeltList` actually *has*
//! those semantics is invisible to every other test tier: the `compile_corpus`
//! tier only proves the emitted Rust type-checks, and the string-golden and
//! snapshot tests only prove some shape was emitted. Getting it wrong does not
//! fail loudly — a write simply lands in a copy nobody reads. So each case here
//! is a TypeScript Vitest test whose `expect(...)` calls lower to real
//! assertions, and this tier lowers the program to a crate and runs `cargo test`
//! on it: a green run means the semantics held at runtime.
//!
//! The last test is the one that catches *over*-sharing, which is the way a move
//! towards a shared backing buffer breaks correctness silently: `[...a]` and
//! `a.slice()` are genuine copies in JavaScript and must NOT alias. A change that
//! makes every handle share one buffer without keeping the copy operations
//! copying passes every other test in this file and is still wrong.
//!
//! What these tests deliberately do NOT assert is `===` between two typed
//! arrays; see the comment on the last test for the pre-existing `===` lowering
//! gap that makes it unreliable independently of how the elements are stored.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test list_reference_semantics_runtime -- --ignored
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
        "generated list test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-list-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_list_fixture(source: &str, crate_name: &str) {
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
fn a_local_alias_shares_the_array() {
    // The base case: `const b = a` is a second handle on one array, so a push
    // through either is visible through the other, and `a === b`.
    let source = r#"
import { test, expect } from "vitest";
test("a local alias shares the array", () => {
  const a: number[] = [1];
  const b = a;
  b.push(2);
  expect(a.length).toBe(2);
  expect(a[1]).toBe(2);
  expect(b.length).toBe(2);
  expect(a === b).toBe(true);
});
"#;
    run_list_fixture(source, "list_alias_shares");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_callee_mutates_the_callers_array() {
    // Arguments pass the handle, not a copy: the callee's `push` is the caller's
    // array growing. This is the case a by-value `Vec` payload gets wrong while
    // still type-checking, because the write lands in the callee's copy.
    let source = r#"
import { test, expect } from "vitest";
function addTo(target: number[], value: number): void {
  target.push(value);
}
test("a callee mutates the caller's array", () => {
  const values: number[] = [1];
  addTo(values, 2);
  addTo(values, 3);
  expect(values.length).toBe(3);
  expect(values.join(",")).toBe("1,2,3");
});
"#;
    run_list_fixture(source, "list_callee_mutates");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_read_back_handle_from_an_object_shares_the_array() {
    // An array read back out of a field is that array. This is the motivating
    // case for the write-back machinery in `emitter::list_mutation`: without
    // sharing, `const bucket = box.items; bucket.push(..)` mutates a copy and the
    // field never changes.
    let source = r#"
import { test, expect } from "vitest";
test("a read-back handle from an object shares the array", () => {
  const box: { items: number[] } = { items: [1] };
  const bucket = box.items;
  bucket.push(2);
  expect(box.items.length).toBe(2);
  expect(box.items.join(",")).toBe("1,2");
  box.items.push(3);
  expect(bucket.length).toBe(3);
});
"#;
    run_list_fixture(source, "list_object_field_shares");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_read_back_handle_from_a_map_shares_the_array() {
    // The `groupBy` shape: look the bucket up, push into it, and expect the map
    // to hold the grown array. Two handles are live here — the map's stored one
    // and the local — so a copy-on-write buffer would copy and lose the write.
    let source = r#"
import { test, expect } from "vitest";
test("a read-back handle from a map shares the array", () => {
  const buckets = new Map<string, number[]>();
  buckets.set("a", []);
  const bucket = buckets.get("a")!;
  bucket.push(1);
  bucket.push(2);
  expect(buckets.get("a")!.length).toBe(2);
  expect(buckets.get("a")!.join(",")).toBe("1,2");
});
"#;
    run_list_fixture(source, "list_map_value_shares");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_nested_array_shares_with_its_outer_array() {
    // An array element of an array is a handle too: `outer[0]` and the local both
    // name one inner array.
    let source = r#"
import { test, expect } from "vitest";
test("a nested array shares with its outer array", () => {
  const outer: number[][] = [[1]];
  const inner = outer[0];
  inner.push(2);
  expect(outer[0].length).toBe(2);
  expect(outer[0].join(",")).toBe("1,2");
  outer[0].push(3);
  expect(inner.length).toBe(3);
});
"#;
    run_list_fixture(source, "list_nested_shares");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_spread_and_a_slice_are_independent_copies() {
    // The over-sharing guard. `[...a]` and `a.slice()` are fresh arrays, so
    // writes on either side must not cross. Sharing one buffer between every
    // handle is only correct while the operations JavaScript defines as copies
    // stay copies, and this is the case that catches a copy that quietly became
    // an alias — the main way shared storage breaks correctness silently.
    //
    // The *identity* half of the same rule (`a === [...a]` must be `false`) is
    // NOT asserted here because it does not hold, for a reason that predates
    // shared storage and is independent of it: the emitter lowers a source `===`
    // between two typed lists to `BinOp::JsStrictEq`, which falls through to
    // `SmeltList`'s structural `PartialEq` instead of the id comparison
    // `strict_identity_text`/`reference_identity_text` already implement for
    // `BinOp::StrictEq`. Two equal-contents arrays therefore read as `===` even
    // when they are separate arrays with separate buffers. Fixing that is a
    // change to `===` lowering, not to the list representation.
    let source = r#"
import { test, expect } from "vitest";
test("a spread and a slice are independent copies", () => {
  const a: number[] = [1, 2];
  const spread = [...a];
  const sliced = a.slice();
  spread.push(3);
  sliced.push(4);
  a.push(5);
  expect(a.join(",")).toBe("1,2,5");
  expect(spread.join(",")).toBe("1,2,3");
  expect(sliced.join(",")).toBe("1,2,4");
});
"#;
    run_list_fixture(source, "list_copies_are_independent");
}
