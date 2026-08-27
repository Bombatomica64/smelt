//! Runtime execution tests for JavaScript `Set` membership and iteration order.
//!
//! `SmeltJsSet` keeps its members in an insertion-ordered `Vec` and finds them
//! through a hash index (`smelt_js_member_hash_key`) instead of scanning. Those
//! two halves have to agree, and the ways they can disagree are invisible to the
//! `compile_corpus` tier (which only proves the emitted Rust type-checks) and to
//! the string-golden tests (which only prove a shape was emitted):
//!
//! 1. **Insertion order** is observable in JavaScript — iterating a `Set` yields
//!    members in the order they were added, and a `delete` must not reshuffle the
//!    survivors. The hash index stores *positions* into that `Vec`, so a removal
//!    has to shift every later position down by one; if it does not, members
//!    after the hole are looked up at the wrong slot.
//! 2. **`NaN` is a member of itself** under SameValueZero, unlike `f64`
//!    `PartialEq`, so every `NaN` has to hash as one canonical `NaN`.
//! 3. **`+0` and `-0` are one member**, and their `f64` bit patterns differ, so
//!    the hash has to normalize the sign of zero.
//! 4. **Objects are members by reference identity**, not structure: two
//!    structurally equal literals are two distinct members, so the hash must be
//!    taken over the stable object `id` rather than the contents.
//!
//! A hash index that gets any of these wrong silently loses members rather than
//! failing loudly, which is why these are runtime tests: each case is a
//! TypeScript Vitest test, lowering it emits a `#[test]`, and this tier lowers
//! the program to a crate and runs `cargo test` on it — a green run means every
//! `expect(...)` held at runtime. The tier is `#[ignore]`d because it compiles
//! and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test set_membership_runtime -- --ignored
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
        "generated set test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-set-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_set_fixture(source: &str, crate_name: &str) {
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
fn set_iteration_follows_insertion_order_across_a_delete() {
    // Iteration order is insertion order, and `delete` closes the hole without
    // reordering the survivors. The hash index stores positions into the
    // insertion-ordered backing `Vec`, so this is the case that catches an index
    // that was not re-aligned after the removal: every member added *after* the
    // deleted one moves down a slot, and a stale index would either report it
    // missing or match it against the wrong member.
    let source = r#"
import { test, expect } from "vitest";
test("iteration is insertion order after a delete and later adds", () => {
  const values = new Set<number>();
  values.add(10);
  values.add(20);
  values.add(30);
  values.add(40);
  expect(values.delete(20)).toBe(true);
  values.add(50);
  const seen: number[] = [];
  for (const value of values) {
    seen.push(value);
  }
  expect(seen.join(",")).toBe("10,30,40,50");
  expect(values.size).toBe(4);
});
test("every survivor is still found after a delete", () => {
  const values = new Set<number>();
  values.add(1);
  values.add(2);
  values.add(3);
  values.add(4);
  values.delete(2);
  expect(values.has(1)).toBe(true);
  expect(values.has(2)).toBe(false);
  expect(values.has(3)).toBe(true);
  expect(values.has(4)).toBe(true);
});
test("re-adding a deleted member appends it at the end", () => {
  const values = new Set<number>();
  values.add(1);
  values.add(2);
  values.add(3);
  values.delete(1);
  values.add(1);
  const seen: number[] = [];
  for (const value of values) {
    seen.push(value);
  }
  expect(seen.join(",")).toBe("2,3,1");
  expect(values.size).toBe(3);
});
test("deleting a member that is not present changes nothing", () => {
  const values = new Set<number>();
  values.add(7);
  expect(values.delete(8)).toBe(false);
  expect(values.size).toBe(1);
  expect(values.has(7)).toBe(true);
});
"#;
    run_set_fixture(source, "smelt_set_insertion_order");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn set_membership_is_same_value_zero_for_numbers() {
    // SameValueZero, not `f64` `PartialEq`: `NaN` is a member of itself (so a set
    // never holds two of them), and `+0`/`-0` collapse to a single member that
    // either spelling finds. A hash taken over raw `f64` bits breaks both — `NaN`
    // bit patterns are not required to be unique and `-0.0` has its own sign bit
    // — so both are normalized before hashing.
    let source = r#"
import { test, expect } from "vitest";
test("NaN is a single member and is found by NaN", () => {
  const values = new Set<number>();
  values.add(NaN);
  values.add(NaN);
  expect(values.size).toBe(1);
  expect(values.has(NaN)).toBe(true);
});
test("NaN is deletable", () => {
  const values = new Set<number>();
  values.add(NaN);
  values.add(1);
  expect(values.delete(NaN)).toBe(true);
  expect(values.has(NaN)).toBe(false);
  expect(values.size).toBe(1);
});
test("positive and negative zero are one member", () => {
  const values = new Set<number>();
  values.add(0);
  values.add(-0);
  expect(values.size).toBe(1);
  expect(values.has(0)).toBe(true);
  expect(values.has(-0)).toBe(true);
});
test("negative zero finds a member added as positive zero", () => {
  const values = new Set<number>();
  values.add(-0);
  values.add(1);
  expect(values.size).toBe(2);
  expect(values.has(0)).toBe(true);
  expect(values.delete(0)).toBe(true);
  expect(values.size).toBe(1);
});
test("distinct numbers stay distinct", () => {
  const values = new Set<number>();
  values.add(1);
  values.add(2);
  values.add(1);
  expect(values.size).toBe(2);
  expect(values.has(3)).toBe(false);
});
"#;
    run_set_fixture(source, "smelt_set_same_value_zero");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn set_membership_of_erased_members_is_identity_and_variant_tagged() {
    // Members typed `unknown` flow as erased `SmeltUnknown`, the widest element
    // type a Set can hold. Two properties matter for the hash index:
    //
    // * objects are members by REFERENCE identity, so two structurally identical
    //   literals are two members and only the original object is found again.
    //   This is why the hash is taken over the stable object `id` and not over the
    //   contents (a structural hash would also go stale when a member mutates).
    // * a value of one runtime variant is never a member matching another, so
    //   `1`, `"1"` and `true` are three members.
    let source = r#"
import { test, expect } from "vitest";
test("look-alike objects are distinct members", () => {
  const first: unknown = { v: 1 };
  const second: unknown = { v: 1 };
  const values = new Set<unknown>();
  values.add(first);
  values.add(second);
  expect(values.size).toBe(2);
  expect(values.has(first)).toBe(true);
  expect(values.has(second)).toBe(true);
});
test("the same object added twice is one member", () => {
  const only: unknown = { v: 1 };
  const values = new Set<unknown>();
  values.add(only);
  values.add(only);
  expect(values.size).toBe(1);
  expect(values.has(only)).toBe(true);
});
test("a fresh look-alike object is not a member", () => {
  const only: unknown = { v: 1 };
  const values = new Set<unknown>();
  values.add(only);
  expect(values.has({ v: 1 })).toBe(false);
});
test("a member is still found after it is mutated", () => {
  const box: { v: number } = { v: 1 };
  const member: unknown = box;
  const values = new Set<unknown>();
  values.add(member);
  box.v = 2;
  expect(values.has(member)).toBe(true);
  expect(values.size).toBe(1);
});
test("values of different runtime kinds are different members", () => {
  const values = new Set<unknown>();
  values.add(1);
  values.add("1");
  values.add(true);
  values.add(null);
  values.add(undefined);
  expect(values.size).toBe(5);
  expect(values.has(1)).toBe(true);
  expect(values.has("1")).toBe(true);
  expect(values.has(true)).toBe(true);
  expect(values.has(null)).toBe(true);
  expect(values.has(undefined)).toBe(true);
  expect(values.has(false)).toBe(false);
  expect(values.has(2)).toBe(false);
});
"#;
    run_set_fixture(source, "smelt_set_erased_members");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn set_built_from_a_duplicated_array_keeps_first_occurrences() {
    // `new Set(array)` is the shape `uniq`/`difference`/`intersection` are built
    // on: it collects through `FromIterator`, which routes every element through
    // the same `insert` the hash index serves. A JS `Set` keeps the FIRST
    // occurrence of each duplicate, in that first position, and `clear` empties
    // the index alongside the members.
    let source = r#"
import { test, expect } from "vitest";
test("constructing from an array dedupes and keeps first positions", () => {
  const values = new Set<number>([3, 1, 3, 2, 1, 3]);
  expect(values.size).toBe(3);
  const seen: number[] = [];
  for (const value of values) {
    seen.push(value);
  }
  expect(seen.join(",")).toBe("3,1,2");
});
test("clear empties the set and membership afterwards is false", () => {
  const values = new Set<number>([1, 2, 3]);
  values.clear();
  expect(values.size).toBe(0);
  expect(values.has(1)).toBe(false);
  values.add(1);
  expect(values.size).toBe(1);
  expect(values.has(1)).toBe(true);
});
"#;
    run_set_fixture(source, "smelt_set_from_array");
}
