//! Runtime execution tests for JavaScript object *reference* semantics.
//!
//! An object whose shape is statically known lowers to a generated Rust struct
//! with typed fields (see `smelt_frontend_ts::lowering::decls::shape_object`).
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
        "generated object-shape test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-object-runtime-{}-{seq}", std::process::id()))
}

/// Run `source` and return whether its generated assertions all held.
fn generated_tests_pass(crate_dir: &Path, target_dir: &Path) -> bool {
    Command::new(env!("CARGO"))
        .arg("test")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        .env("RUSTFLAGS", "-Awarnings")
        .output()
        .expect("spawn cargo test")
        .status
        .success()
}

/// Emit `source` as a crate and assert its generated tests still FAIL.
///
/// A characterization test for a gap that is real, understood, and not yet
/// closed. Deleting the fixture would hide the gap and asserting the wrong
/// answer would cement it, so the assertion is inverted instead: the day the gap
/// closes this test goes red, which is the prompt to flip it to
/// [`run_object_fixture`].
fn run_object_fixture_expecting_failure(source: &str, crate_name: &str) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    emit_program(source, crate_name, &crate_dir);
    let passed = generated_tests_pass(&crate_dir, &target_dir);
    drop(std::fs::remove_dir_all(&root));
    assert!(
        !passed,
        "{crate_name} now holds at runtime: the object-aliasing gap this test \
         pins is closed, so it should become a `run_object_fixture` case"
    );
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_object_fixture(source: &str, crate_name: &str) {
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
fn known_gap_a_local_alias_does_not_yet_share_the_object() {
    // The base case, and TODAY IT DOES NOT HOLD (inverted assertion, see
    // `run_object_fixture_expecting_failure`). `const b = a` is a second handle
    // on one object, so a field write through either must be visible through the
    // other. A record local whose fields are written is not yet lifted to a
    // shared cell at its *binding* sites, only at its parameter ones, so the
    // write lands in a copy. This is the gap struct-shaped objects must close;
    // see `blocker-logs/struct-shaped-objects.md`.
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
    run_object_fixture_expecting_failure(source, "object_shape_alias_shares");
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
fn known_gap_a_read_back_handle_from_an_array_does_not_yet_share() {
    // An object read back out of an array element is that object — and TODAY IT
    // IS NOT (inverted assertion). Same root cause as the alias case above: the
    // element read produces a copy, so the write never reaches the array.
    let source = r#"
import { test, expect } from "vitest";
test("a read-back handle from an array shares the object", () => {
  const rows: { count: number }[] = [{ count: 1 }];
  const first = rows[0];
  first.count = 5;
  expect(rows[0].count).toBe(5);
});
"#;
    run_object_fixture_expecting_failure(source, "object_shape_array_element_shares");
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
