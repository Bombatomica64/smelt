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

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn array_of_length_gives_each_slot_its_own_array() {
    // `Array(n)` used to emit `vec![<hole>; n]`, which is `Clone`-based — and a
    // Smelt reference value clones by SHARING since the shared-buffer change. So
    // every slot was a handle to ONE array, carrying one JavaScript identity, and
    // pushing through `a[0]` was visible through `a[1]`.
    //
    // This is the assertion that catches over-sharing at construction, the same
    // way the spread/slice fixture catches it at copy. Both directions matter: a
    // reference value that shares too little breaks aliasing, and one that shares
    // too much invents it.
    // NOT asserted here: `Array(n).fill(a)` genuinely aliasing, which is what
    // JavaScript does and what `list_repeat_text`'s `vec![x; n]` is designed for.
    // Smelt cannot deliver it today — `fill` erases `a` to `SmeltUnknown::Array`
    // and the conversion back rebuilds every element from scratch, so the shared
    // identity is lost at the round trip. That is the copying erasure boundary in
    // blocker-logs/smeltlist-shared-buffer.md, not this rule; a fixture for it
    // belongs with that work.
    let source = r#"
import { test, expect } from "vitest";
test("Array(n) slots are independent arrays", () => {
  const rows: number[][] = Array(3);
  for (let i = 0; i < 3; i += 1) {
    rows[i] = [];
  }
  rows[0].push(1);
  expect(rows[0].join(",")).toBe("1");
  expect(rows[1].join(",")).toBe("");
  expect(rows[2].join(",")).toBe("");
});
test("writing one slot of a fresh Array(n) does not write the others", () => {
  const grid: number[][] = Array(3);
  grid[0] = [];
  grid[1] = [];
  grid[0].push(7);
  grid[1].push(8);
  expect(grid[0].join(",")).toBe("7");
  expect(grid[1].join(",")).toBe("8");
});
"#;
    run_list_fixture(source, "array_of_length_slots");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn erasing_a_list_into_an_unknown_slot_keeps_the_same_array() {
    // The erasure boundary, which is where a `SmeltList` used to stop being a
    // reference. `From<SmeltList<SmeltUnknown>> for SmeltArray` carried the list's
    // `id` but rebuilt its buffer (`with_id(list.id(), list.into_vec())`), so the
    // erased value was a HALF reference: a frozen snapshot wearing the live
    // array's identity.
    //
    // Nothing else in the suite catches that. `===` still answered `true` because
    // it compares ids, and every read taken before the erasure agreed, so the only
    // observable is a write made AFTER it — which is what both fixtures below do.
    // In JavaScript an array put into an `unknown[]` slot IS the array, so a later
    // push through the typed handle must be visible through the erased element.
    //
    // The second fixture is the shape this was found on: a self-referential array.
    // `a[0] = a` can only mean anything if the erased element can BE `a`; with a
    // snapshot it stored a copy of `a` as it was one statement earlier — empty —
    // which is why es-toolkit's two `isEqualWith` circular-reference tests could
    // not pass.
    let source = r#"
import { test, expect } from "vitest";
test("a write after erasure is visible through the erased element", () => {
  const inner: unknown[] = [];
  const outer: unknown[] = [inner];
  inner.push(1);
  inner.push(2);
  const seen = outer[0] as unknown[];
  expect(seen.length).toBe(2);
});
test("a self-referential array holds itself, not a snapshot of itself", () => {
  const a: unknown[] = [];
  a[0] = a;
  const inner = a[0] as unknown[];
  expect(a.length).toBe(1);
  expect(inner.length).toBe(1);
});
"#;
    run_list_fixture(source, "list_erasure_shares");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_by_reference_argument_whose_element_type_only_looks_different_still_aliases() {
    // The caller's array is `unknown[]`; the callee's parameter is a generic
    // `T[]` the callee ended up rendering erased. Those are two different MIR
    // `TypeId`s and ONE Rust type (`SmeltList<SmeltUnknown>`), so the
    // list-to-list coercion, which keyed on `TypeId` inequality, rebuilt the
    // buffer -- a fresh `Rc<RefCell<Vec<_>>>`. The callee then spliced a
    // temporary and the caller's array never changed, which is how es-toolkit's
    // in-place `remove` silently did nothing.
    //
    // Nothing needs converting when the two element types render alike, so the
    // argument must be the caller's own list.
    let source = r#"
import { test, expect } from "vitest";
function dropNullish<T>(target: T[]): T[] {
  const removed: T[] = [];
  for (let i = target.length - 1; i >= 0; i--) {
    if (target[i] === undefined) {
      removed.push(target[i]);
      target.splice(i, 1);
    }
  }
  return removed;
}
function appendTo<T>(target: T[], value: T): void {
  target.push(value);
}
test("an in-place removal through an erased-element parameter is observed", () => {
  const values: unknown[] = [1, undefined, 3, undefined, 5];
  const removed = dropNullish(values);
  expect(values).toEqual([1, 3, 5]);
  expect(removed.length).toBe(2);
});
test("an in-place push through an erased-element parameter is observed", () => {
  const values: unknown[] = [1];
  appendTo(values, 2);
  expect(values.length).toBe(2);
  expect(values[1]).toBe(2);
});
"#;
    run_list_fixture(source, "list_identity_coercion_aliases");
}
