//! Runtime execution tests for JavaScript `Map` lookup, ordering, and aliasing.
//!
//! `SmeltJsMap` keeps its entries in an insertion-ordered `Vec` and finds a key
//! through a hash index (`SmeltJsKeyEq::js_key_hash`) instead of scanning. Those
//! two halves have to agree, and the ways they can disagree are invisible to the
//! `compile_corpus` tier (which only proves the emitted Rust type-checks) and to
//! the string-golden tests (which only prove a shape was emitted):
//!
//! 1. **Insertion order** is observable in JavaScript — iterating a `Map` yields
//!    entries in the order their keys were first set, re-setting an existing key
//!    keeps its original position, and a `delete` must not reshuffle the
//!    survivors. The index stores *positions* into that `Vec`, so a removal has
//!    to shift every later position down by one; if it does not, entries after
//!    the hole are looked up at the wrong slot.
//! 2. **`NaN` is its own key** under SameValueZero, unlike `f64` `PartialEq`, so
//!    every `NaN` key has to hash as one canonical `NaN`.
//! 3. **`+0` and `-0` are one key**, and their `f64` bit patterns differ, so the
//!    hash has to normalize the sign of zero.
//! 4. **Objects are keys by reference identity**, not structure: two structurally
//!    equal literals are two distinct keys, so the hash must be taken over the
//!    stable object `id` rather than the contents.
//! 5. **A `Map` is a reference value** — the index lives inside the same shared
//!    `RefCell` as the entries, so a write through one alias must be visible to
//!    every other alias, index included. An index kept outside the shared store
//!    would go stale the moment another handle inserted.
//!
//! An index that gets any of these wrong silently loses entries rather than
//! failing loudly, which is why these are runtime tests: each case is a
//! TypeScript Vitest test, lowering it emits a `#[test]`, and this tier lowers
//! the program to a crate and runs `cargo test` on it — a green run means every
//! `expect(...)` held at runtime. The tier is `#[ignore]`d because it compiles
//! and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test map_lookup_runtime -- --ignored
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
fn map_iteration_follows_insertion_order_across_a_delete() {
    // Iteration order is first-set order, and `delete` closes the hole without
    // reordering the survivors. The hash index stores positions into the
    // insertion-ordered backing `Vec`, so this is the case that catches an index
    // that was not re-aligned after the removal: every entry set *after* the
    // deleted one moves down a slot, and a stale index would either report its
    // key missing or match it against the wrong entry.
    let source = r#"
import { test, expect } from "vitest";
test("iteration is insertion order after a delete and later sets", () => {
  const scores = new Map<string, number>();
  scores.set("a", 1);
  scores.set("b", 2);
  scores.set("c", 3);
  scores.set("d", 4);
  expect(scores.delete("b")).toBe(true);
  scores.set("e", 5);
  const seen: string[] = [];
  for (const key of scores.keys()) {
    seen.push(key);
  }
  expect(seen.join(",")).toBe("a,c,d,e");
  expect(scores.size).toBe(4);
});
test("every survivor is still found after a delete", () => {
  const scores = new Map<string, number>();
  scores.set("a", 1);
  scores.set("b", 2);
  scores.set("c", 3);
  scores.set("d", 4);
  scores.delete("b");
  expect(scores.get("a")).toBe(1);
  expect(scores.has("b")).toBe(false);
  expect(scores.get("c")).toBe(3);
  expect(scores.get("d")).toBe(4);
});
test("re-setting an existing key keeps its position and replaces the value", () => {
  const scores = new Map<string, number>();
  scores.set("a", 1);
  scores.set("b", 2);
  scores.set("c", 3);
  scores.set("a", 99);
  const seen: string[] = [];
  for (const key of scores.keys()) {
    seen.push(key);
  }
  expect(seen.join(",")).toBe("a,b,c");
  expect(scores.size).toBe(3);
  expect(scores.get("a")).toBe(99);
});
test("re-setting a deleted key appends it at the end", () => {
  const scores = new Map<string, number>();
  scores.set("a", 1);
  scores.set("b", 2);
  scores.set("c", 3);
  scores.delete("a");
  scores.set("a", 4);
  const seen: string[] = [];
  for (const key of scores.keys()) {
    seen.push(key);
  }
  expect(seen.join(",")).toBe("b,c,a");
  expect(scores.get("a")).toBe(4);
});
test("deleting a key that is not present changes nothing", () => {
  const scores = new Map<string, number>();
  scores.set("a", 1);
  expect(scores.delete("z")).toBe(false);
  expect(scores.size).toBe(1);
  expect(scores.get("a")).toBe(1);
});
test("clear empties the map and later sets still resolve", () => {
  const scores = new Map<string, number>();
  scores.set("a", 1);
  scores.set("b", 2);
  scores.clear();
  expect(scores.size).toBe(0);
  expect(scores.has("a")).toBe(false);
  scores.set("c", 3);
  expect(scores.get("c")).toBe(3);
  expect(scores.size).toBe(1);
});
"#;
    run_map_fixture(source, "smelt_map_insertion_order");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn map_keys_are_same_value_zero_for_numbers() {
    // SameValueZero, not `f64` `PartialEq`: `NaN` is its own key (so a map never
    // holds two `NaN` entries), and `+0`/`-0` collapse to a single key that either
    // spelling finds. A hash taken over raw `f64` bits breaks both — `NaN` bit
    // patterns are not required to be unique and `-0.0` has its own sign bit — so
    // both are normalized before hashing.
    let source = r#"
import { test, expect } from "vitest";
test("NaN is a single key and is found by NaN", () => {
  const scores = new Map<number, string>();
  scores.set(NaN, "first");
  scores.set(NaN, "second");
  expect(scores.size).toBe(1);
  expect(scores.get(NaN)).toBe("second");
  expect(scores.has(NaN)).toBe(true);
});
test("NaN keys are deletable", () => {
  const scores = new Map<number, string>();
  scores.set(NaN, "n");
  scores.set(1, "one");
  expect(scores.delete(NaN)).toBe(true);
  expect(scores.has(NaN)).toBe(false);
  expect(scores.get(1)).toBe("one");
  expect(scores.size).toBe(1);
});
test("positive and negative zero are one key", () => {
  const scores = new Map<number, string>();
  scores.set(0, "plus");
  scores.set(-0, "minus");
  expect(scores.size).toBe(1);
  expect(scores.get(0)).toBe("minus");
  expect(scores.get(-0)).toBe("minus");
});
test("negative zero finds a key set as positive zero", () => {
  const scores = new Map<number, string>();
  scores.set(-0, "zero");
  scores.set(1, "one");
  expect(scores.size).toBe(2);
  expect(scores.has(0)).toBe(true);
  expect(scores.delete(0)).toBe(true);
  expect(scores.size).toBe(1);
});
test("distinct numbers stay distinct keys", () => {
  const scores = new Map<number, number>();
  for (let index = 0; index < 64; index += 1) {
    scores.set(index, index * 2);
  }
  expect(scores.size).toBe(64);
  expect(scores.get(0)).toBe(0);
  expect(scores.get(63)).toBe(126);
  expect(scores.has(64)).toBe(false);
});
"#;
    run_map_fixture(source, "smelt_map_same_value_zero");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn map_object_keys_compare_by_reference_identity() {
    // An object key is the object, not its shape: two structurally identical
    // literals are two distinct keys, and only the very binding that was set finds
    // its entry. The hash therefore has to be taken over the stable object `id`
    // (what `same_js_key` compares) rather than the contents — a structural hash
    // would bucket two distinct keys together needlessly and, worse, would move a
    // key out of its bucket the moment the object it names were mutated after
    // being set, which the mutation case below is what pins down.
    //
    // The keys here are object literals (`SmeltRecord`). A `SmeltList`-typed key
    // (an array) or a class instance does not compile at all — neither has a
    // `SmeltJsKeyEq` impl — which predates the index and is a separate gap.
    let source = r#"
import { test, expect } from "vitest";
test("structurally equal object keys are distinct keys", () => {
  const first = { x: 1 };
  const second = { x: 1 };
  const seen = new Map<{ x: number }, string>();
  seen.set(first, "first");
  seen.set(second, "second");
  expect(seen.size).toBe(2);
  expect(seen.get(first)).toBe("first");
  expect(seen.get(second)).toBe("second");
});
test("deleting one of two equally shaped keys leaves the other", () => {
  const first = { x: 1 };
  const second = { x: 1 };
  const seen = new Map<{ x: number }, string>();
  seen.set(first, "first");
  seen.set(second, "second");
  expect(seen.delete(first)).toBe(true);
  expect(seen.has(first)).toBe(false);
  expect(seen.get(second)).toBe("second");
  expect(seen.size).toBe(1);
});
test("a mutated object key is still found", () => {
  const key = { x: 1 };
  const seen = new Map<{ x: number }, string>();
  seen.set(key, "value");
  key.x = 2;
  expect(seen.get(key)).toBe("value");
  expect(seen.size).toBe(1);
});
test("an object key that was never set is absent", () => {
  const key = { x: 1 };
  const other = { x: 2 };
  const seen = new Map<{ x: number }, string>();
  seen.set(key, "value");
  expect(seen.has(other)).toBe(false);
  expect(seen.delete(other)).toBe(false);
  expect(seen.size).toBe(1);
});
"#;
    run_map_fixture(source, "smelt_map_object_keys");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn map_writes_are_visible_through_every_alias() {
    // A JavaScript `Map` is a reference value: every binding that names one Map
    // shares a single backing store, so a write through any alias is observable
    // through all of them. The hash index lives inside that same shared `RefCell`
    // precisely so it cannot fall behind — an index held outside the shared store
    // (as `SmeltJsSet`'s copy-on-write one is) would go stale the moment another
    // handle inserted, and the newly written key would read back as absent.
    let source = r#"
import { test, expect } from "vitest";
function record(target: Map<string, number>, key: string, value: number): void {
  target.set(key, value);
}
test("a write through an alias is seen by the original", () => {
  const scores = new Map<string, number>();
  const alias = scores;
  alias.set("a", 1);
  expect(scores.get("a")).toBe(1);
  expect(scores.size).toBe(1);
  scores.set("b", 2);
  expect(alias.get("b")).toBe(2);
  expect(alias.size).toBe(2);
});
test("a write through a callee is seen by the caller", () => {
  const scores = new Map<string, number>();
  record(scores, "a", 1);
  record(scores, "b", 2);
  expect(scores.size).toBe(2);
  expect(scores.get("a")).toBe(1);
  expect(scores.get("b")).toBe(2);
});
test("a delete through an alias is seen by the original", () => {
  const scores = new Map<string, number>();
  scores.set("a", 1);
  scores.set("b", 2);
  const alias = scores;
  alias.delete("a");
  expect(scores.has("a")).toBe(false);
  expect(scores.get("b")).toBe(2);
  expect(scores.size).toBe(1);
});
"#;
    run_map_fixture(source, "smelt_map_aliasing");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn map_built_from_entries_indexes_every_key() {
    // `new Map(entries)` lowers through the bulk constructors, which build the key
    // index up front instead of inserting one key at a time. Every key that went in
    // must therefore be findable, and the entries must keep the order they were
    // given.
    let source = r#"
import { test, expect } from "vitest";
test("a map built from entries finds every key it was given", () => {
  const scores = new Map<string, number>([["a", 1], ["b", 2], ["c", 3]]);
  expect(scores.size).toBe(3);
  expect(scores.get("a")).toBe(1);
  expect(scores.get("b")).toBe(2);
  expect(scores.get("c")).toBe(3);
  expect(scores.has("d")).toBe(false);
});
test("a map built from entries keeps their order", () => {
  const scores = new Map<string, number>([["c", 3], ["a", 1], ["b", 2]]);
  const seen: string[] = [];
  for (const key of scores.keys()) {
    seen.push(key);
  }
  expect(seen.join(",")).toBe("c,a,b");
});
test("a map built from entries accepts later writes and deletes", () => {
  const scores = new Map<string, number>([["a", 1], ["b", 2]]);
  scores.set("c", 3);
  expect(scores.get("c")).toBe(3);
  expect(scores.delete("a")).toBe(true);
  expect(scores.has("a")).toBe(false);
  expect(scores.get("b")).toBe(2);
  expect(scores.get("c")).toBe(3);
  expect(scores.size).toBe(2);
});
"#;
    run_map_fixture(source, "smelt_map_from_entries");
}
