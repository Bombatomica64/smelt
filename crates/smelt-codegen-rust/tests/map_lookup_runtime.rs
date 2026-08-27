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

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn dict_entry_push_mutates_the_stored_group_in_place() {
    // `groups[key].push(item)` used to copy the whole stored list out of the
    // dict, push onto the copy, and copy it back — twice over, because the push
    // wrote the entry back and the statement after it wrote the same entry back
    // again. `smelt_mir::opt::DictEntryInPlaceMutation` now retargets the push at
    // the entry itself, so the group is mutated through the container and
    // neither copy is emitted.
    //
    // Deleting those copies is only correct if the in-place write is what every
    // later reader sees, so these cases pin the behavior the copies used to
    // provide. This fixture uses the plain `HashMap` backing a string-keyed dict
    // takes when the program needs no erased runtime; the two fixtures below
    // cover the `SmeltRecord` and `SmeltJsMap` backings.
    let source = r#"
import { test, expect } from "vitest";
test("grouping collects every item under its own key", () => {
  const items = [1, 2, 3, 4, 5, 6];
  const groups: Record<string, number[]> = {};
  for (const item of items) {
    const key = item % 2 === 0 ? "even" : "odd";
    if (!Object.hasOwn(groups, key)) {
      groups[key] = [];
    }
    groups[key].push(item);
  }
  expect(groups["odd"].join(",")).toBe("1,3,5");
  expect(groups["even"].join(",")).toBe("2,4,6");
});
test("distinct keys never share a group", () => {
  const groups: Record<string, number[]> = { a: [], b: [] };
  groups["a"].push(1);
  groups["b"].push(2);
  groups["a"].push(3);
  expect(groups["a"].join(",")).toBe("1,3");
  expect(groups["b"].join(",")).toBe("2");
});
test("pushing under an absent key creates that group", () => {
  // Smelt gives an index read a total lowering: a missing entry reads as the
  // value type's default rather than `undefined`. The fused push has to keep
  // that, which is the insert branch of the entry accessor.
  const groups: Record<string, number[]> = {};
  groups["fresh"].push(7);
  expect(groups["fresh"].join(",")).toBe("7");
});
test("a named group binding still observes its own pushes", () => {
  // A user binding is NOT fused — it keeps the copy-out/copy-back form — so both
  // it and the container must show the pushes.
  const groups: Record<string, number[]> = { a: [] };
  groups["b"] = [];
  const group = groups["a"];
  group.push(1);
  group.push(2);
  expect(group.join(",")).toBe("1,2");
  expect(groups["a"].join(",")).toBe("1,2");
});
test("pushing a value read out of the same container works", () => {
  // The re-borrow trap: the pushed item reads the SAME container. Taking the
  // entry handle before evaluating the item would panic at runtime with
  // "already borrowed" over the record and map backings.
  const groups: Record<string, number[]> = { a: [1, 2] };
  groups["a"].push(groups["a"].length);
  groups["a"].push(groups["a"][0]);
  expect(groups["a"].join(",")).toBe("1,2,2,1");
});
"#;
    run_map_fixture(source, "smelt_dict_entry_push");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn dict_entry_push_mutates_a_record_backed_group_in_place() {
    // The same fused push over the `SmeltRecord` backing, which a string-keyed
    // dict takes once the program also needs the erased runtime (the `unknown`
    // binding below forces it). A record keeps JavaScript own-key order and is a
    // reference value, so this is where key ordering and alias visibility are
    // observable: the in-place write must be seen through every binding, exactly
    // as the deleted copy-back made it.
    let source = r#"
import { test, expect } from "vitest";
test("grouping through a record keeps first-seen key order", () => {
  const erased: unknown = "force the erased runtime";
  expect(typeof erased).toBe("string");
  const items = [1, 2, 3, 4, 5, 6];
  const groups: Record<string, number[]> = {};
  for (const item of items) {
    const key = item % 2 === 0 ? "even" : "odd";
    if (!Object.hasOwn(groups, key)) {
      groups[key] = [];
    }
    groups[key].push(item);
  }
  expect(groups["odd"].join(",")).toBe("1,3,5");
  expect(groups["even"].join(",")).toBe("2,4,6");
  expect(Object.keys(groups).join(",")).toBe("odd,even");
});
test("a record push through one binding is visible through another", () => {
  const erased: unknown = 1;
  expect(typeof erased).toBe("number");
  const groups: Record<string, number[]> = { a: [] };
  const alias = groups;
  groups["a"].push(1);
  alias["a"].push(2);
  expect(groups["a"].join(",")).toBe("1,2");
  expect(alias["a"].join(",")).toBe("1,2");
});
test("a record push reading the same container works", () => {
  const erased: unknown = true;
  expect(typeof erased).toBe("boolean");
  const groups: Record<string, number[]> = { a: [1, 2] };
  groups["a"].push(groups["a"].length);
  groups["a"].push(groups["a"][0]);
  expect(groups["a"].join(",")).toBe("1,2,2,1");
});
"#;
    run_map_fixture(source, "smelt_dict_entry_push_record");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn dict_entry_push_mutates_a_map_backed_group_in_place() {
    // The `SmeltJsMap` backing, which a dict takes when its key is not a plain
    // string — here a `K extends PropertyKey` type parameter, the shape every
    // generic `groupBy` has and the one the O(n^2) copying was measured on. The
    // map's entries live behind a shared `RefCell`, so the entry handle is a live
    // borrow: these cases prove the fused push neither loses a group nor
    // re-borrows the store while the handle is held.
    let source = r#"
import { test, expect } from "vitest";
function groupBy<T, K extends PropertyKey>(items: T[], keyOf: (item: T) => K): Record<K, T[]> {
  const result = {} as Record<K, T[]>;
  for (const item of items) {
    const key = keyOf(item);
    if (!Object.hasOwn(result, key)) {
      result[key] = [];
    }
    result[key].push(item);
  }
  return result;
}
function joinGroup<K extends PropertyKey>(groups: Record<K, number[]>, key: K): string {
  return groups[key].join(",");
}
function pushThroughAlias<K extends PropertyKey>(key: K): string {
  const result = {} as Record<K, number[]>;
  result[key] = [];
  const alias = result;
  result[key].push(1);
  alias[key].push(2);
  return alias[key].join(",");
}
function pushOwnLength<K extends PropertyKey>(key: K): string {
  const result = {} as Record<K, number[]>;
  result[key] = [5];
  result[key].push(result[key].length);
  return result[key].join(",");
}
test("a map-backed grouping collects every item under its own key", () => {
  const grouped = groupBy([1, 2, 3, 4, 5, 6], (item) => (item % 2 === 0 ? "even" : "odd"));
  expect(joinGroup(grouped, "odd")).toBe("1,3,5");
  expect(joinGroup(grouped, "even")).toBe("2,4,6");
});
test("a map-backed grouping keeps distinct keys apart", () => {
  const grouped = groupBy([10, 21, 32, 43, 54], (item) => item % 2);
  expect(joinGroup(grouped, 0)).toBe("10,32,54");
  expect(joinGroup(grouped, 1)).toBe("21,43");
});
test("a map-backed push through one binding is visible through another", () => {
  expect(pushThroughAlias("a")).toBe("1,2");
});
test("a map-backed push reading the same container works", () => {
  expect(pushOwnLength("a")).toBe("5,1");
});
"#;
    run_map_fixture(source, "smelt_dict_entry_push_map");
}
