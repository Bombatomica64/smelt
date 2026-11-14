//! Runtime execution tests for JavaScript object *reference* semantics.
//!
//! An object whose shape is statically known lowers to a generated Rust struct
//! with typed fields (see the `smelt_frontend_ts` lowering of object type literals).
//! That is the right *representation* — `row.group` becomes a field load instead
//! of a string hash — but it is only a correct one while the struct keeps the
//! semantics a JavaScript object has:
//!
//! * an object is a **reference** value, so `const b = a; b.x = 1` is observable
//!   through `a`, and passing an object into a function passes the handle;
//! * an object reached back out of an array, a field, or a `Map` is *that*
//!   object, not a copy; and
//! * `{ ...a }` and `Object.assign({}, a)` are genuine **copies**, so a shared
//!   representation must not over-share them.
//!
//! No other test tier can see any of this. `compile_corpus` proves the emitted
//! Rust type-checks, and the string-golden and snapshot tests prove some shape
//! was emitted; a struct that silently copies on assignment passes both, and the
//! lost write simply lands in a copy nobody reads. So each case here is a
//! TypeScript Vitest test whose `expect(...)` calls lower to real assertions,
//! and this tier lowers the program to a crate and runs `cargo test` on it: a
//! green run means the semantics held at runtime.
//!
//! This is the object twin of `list_reference_semantics_runtime`.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test object_shape_reference_semantics_runtime -- --ignored
//! ```

#![expect(
    clippy::expect_used,
    reason = "runtime tests fail fast on invalid fixture setup"
)]

#[path = "harness/mod.rs"]
mod harness;

use harness::{emit_program, run_generated_tests};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-object-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_object_fixture(source: &str, crate_name: &str) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    let outcome = std::panic::catch_unwind(|| {
        emit_program(source, crate_name, &crate_dir);
        run_generated_tests(&crate_dir, &target_dir);
    });
    // Removed on the FAILURE path too -- the root holds a whole nested cargo
    // target directory, and one leaked per failing case is what fills `/tmp`.
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
fn reads_every_field_at_its_own_declared_type() {
    // The representation case: a fully-known shape carries a distinct static
    // type per field. A string-keyed map would have to widen all four into one
    // value type; a struct does not, and every read must come back at the
    // declared type with the declared value.
    let source = r#"
import { test, expect } from "vitest";
function label(row: { id: number; group: string; value: number; flag: boolean }): string {
  return row.group;
}
test("reads every field at its own declared type", () => {
  const row: { id: number; group: string; value: number; flag: boolean } =
    { id: 3, group: "a", value: 1.5, flag: true };
  expect(row.id).toBe(3);
  expect(row.group).toBe("a");
  expect(row.value).toBe(1.5);
  expect(row.flag).toBe(true);
  expect(label(row)).toBe("a");
});
"#;
    run_object_fixture(source, "object_shape_typed_fields");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_local_alias_shares_the_object() {
    // The base case: `const b = a` is a second handle on one object, so a field
    // write through either is visible through the other.
    //
    // Requirement change. This test used to assert the generated test FAILED
    // (`known_gap_a_local_alias_does_not_yet_share_the_object`): a record local
    // was lifted to a shared cell only at parameter sites, so the write landed
    // in a copy, and the inverted assertion pinned that bug as a known gap.
    // Shared records now back such a local at its binding sites too, so the
    // requirement is the JavaScript one. Evidence: the same program in Node
    // prints `2 2` for `a.count, b.count`. The guarantee is strengthened, not
    // relaxed: the inverted form only checked that some assertion failed.
    let source = r#"
import { test, expect } from "vitest";
test("a local alias shares the object", () => {
  const a: { count: number } = { count: 1 };
  const b = a;
  b.count = 2;
  expect(a.count).toBe(2);
  expect(b.count).toBe(2);
});
"#;
    run_object_fixture(source, "object_shape_alias_shares");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_callee_mutates_the_callers_object() {
    // Arguments pass the handle, not a copy: the callee's field write is the
    // caller's object changing.
    let source = r#"
import { test, expect } from "vitest";
function bump(target: { count: number }): void {
  target.count = target.count + 1;
}
test("a callee mutates the caller's object", () => {
  const box: { count: number } = { count: 1 };
  bump(box);
  bump(box);
  expect(box.count).toBe(3);
});
"#;
    run_object_fixture(source, "object_shape_callee_mutates");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_read_back_handle_from_an_array_shares_the_object() {
    // An object read back out of an array element is that object, so a write
    // through the read-back handle is seen through the array.
    //
    // Requirement change. This test used to assert the generated test FAILED
    // (`known_gap_a_read_back_handle_from_an_array_does_not_yet_share`): the
    // element read produced a copy, and the inverted assertion pinned that bug.
    // The element read now yields the shared object, so the requirement is the
    // JavaScript one. Evidence: the same program in Node prints `5` for
    // `rows[0].count`.
    let source = r#"
import { test, expect } from "vitest";
test("a read-back handle from an array shares the object", () => {
  const rows: { count: number }[] = [{ count: 1 }];
  const first = rows[0];
  first.count = 5;
  expect(rows[0].count).toBe(5);
});
"#;
    run_object_fixture(source, "object_shape_array_element_shares");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_spread_copy_is_independent() {
    // The over-sharing guard. `{ ...a }` is a fresh object in JavaScript, so a
    // shared representation must keep the operations JavaScript defines as
    // copies copying. A change that makes every handle share one cell without
    // that passes every case above and is still wrong.
    let source = r#"
import { test, expect } from "vitest";
test("a spread copy is independent", () => {
  const a: { count: number } = { count: 1 };
  const copy: { count: number } = { ...a };
  copy.count = 9;
  expect(a.count).toBe(1);
  expect(copy.count).toBe(9);
});
"#;
    run_object_fixture(source, "object_shape_spread_copies");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_erased_object_keeps_its_source_key_order() {
    // Flowing a shape into a dynamic position is a boundary, not a
    // representation change: the erased object must enumerate the fields in the
    // order the source wrote them, which is the JavaScript own-key order.
    let source = r#"
import { test, expect } from "vitest";
test("an erased object keeps its source key order", () => {
  const row: { id: number; group: string; flag: boolean } = { id: 1, group: "a", flag: false };
  const erased: unknown = row;
  expect(Object.keys(erased as object).join(",")).toBe("id,group,flag");
});
"#;
    run_object_fixture(source, "object_shape_erased_key_order");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn recovering_an_erased_object_keeps_its_reference_identity() {
    // Recovering an erased object at a record type used to REBUILD its entries,
    // which detached the recovered record from the object it came from: every
    // write through it landed in a copy nothing else could see. es-toolkit's
    // `clone` reaches exactly that -- `Object.assign(newError, obj)` on an
    // `unknown` target, whose result it then returns through the target
    // binding, so the merged properties disappeared. An object is a reference
    // value, so the recovery hands back a second handle on the same store.
    let source = r#"
import { test, expect } from "vitest";

function decorate(target: unknown, source: Record<string, unknown>): void {
  Object.assign(target as Record<string, unknown>, source);
}

test("Object.assign through an erased target writes into that object", () => {
  const holder: Record<string, unknown> = { kept: 1 };
  decorate(holder, { added: 2 });
  expect(holder.kept).toBe(1);
  expect(holder.added).toBe(2);
});
test("a property store through a recovered record reaches the object", () => {
  const holder: Record<string, unknown> = { a: 1 };
  const erased: unknown = holder;
  const recovered = erased as Record<string, unknown>;
  recovered.b = 2;
  expect(holder.b).toBe(2);
  expect(Object.keys(holder)).toEqual(["a", "b"]);
});
"#;
    run_object_fixture(source, "object_shape_erased_recovery_identity");
}
