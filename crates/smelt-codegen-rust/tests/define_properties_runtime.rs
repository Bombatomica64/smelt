//! Runtime execution tests for `Object.defineProperty` / `Object.defineProperties`,
//! object-literal method shorthand, and `Number.isSafeInteger`.
//!
//! All three used to collapse to a VALUE rather than being modeled, which is the
//! defect class this repository keeps finding:
//!
//! * `Object.defineProperties` was not lowered at all, so the call read
//!   `defineProperties` off the fabricated empty object standing in for the
//!   ambient `Object` global, got `undefined`, and invoked a fabricated default
//!   closure returning `SmeltUnknown::Null`. `Object.defineProperty` *was*
//!   listed, but only as an opaque no-op that evaluated its arguments and
//!   dropped the mutation.
//! * An object-literal METHOD SHORTHAND (`{ f() { .. } }`) was replaced by
//!   `null` unless a source-text probe found `[Symbol.iterator]` in its span, so
//!   a descriptor's `get() { return 2 }` had no body left to call.
//! * `Number.isSafeInteger` had no lowering, so es-toolkit's
//!   `isLength = Number.isSafeInteger(v) && v >= 0` answered `false` for every
//!   input.
//!
//! A string-golden test can only prove the right helper is emitted; only
//! executing the program proves the property is actually installed, the method
//! actually runs, and the predicate actually answers. The tier is `#[ignore]`d
//! because it compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test define_properties_runtime -- --ignored
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
        "generated property-definition test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-define-properties-runtime-{}-{seq}",
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
fn define_properties_installs_data_and_accessor_descriptors() {
    // The es-toolkit `cloneDeep should clone read-only properties` shape. Both
    // descriptors are enumerable, so both keys must exist on the object and
    // survive a spread copy. The accessor descriptor's `get` is a method
    // shorthand, which is why this case also exercises the shorthand lowering.
    let source = r#"
import { test, expect } from "vitest";
test("defineProperties installs value and getter descriptors", () => {
  const object: any = {};
  Object.defineProperties(object, {
    first: { enumerable: true, writable: true, value: 1 },
    second: { enumerable: true, get() { return 2; } },
  });
  object.third = 3;
  expect(object.first).toBe(1);
  expect(object.second).toBe(2);
  expect(object.third).toBe(3);
  expect(Object.keys(object).length).toBe(3);
  expect({ ...object }).toEqual({ first: 1, second: 2, third: 3 });
});
test("defineProperty installs one descriptor", () => {
  const object: any = {};
  Object.defineProperty(object, "a", { enumerable: true, writable: true, value: 7 });
  expect(object.a).toBe(7);
  expect(Object.keys(object)).toEqual(["a"]);
});
"#;
    run_fixture(source, "smelt_define_properties_descriptors");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn define_properties_leaves_a_non_enumerable_property_uninstalled() {
    // An erased object is a flat key/value store with no per-property attribute
    // table, so every key it holds is enumerable by construction. Installing a
    // `enumerable: false` property anyway would wrongly surface it in
    // `Object.keys`, in spread and in structural equality -- which is exactly
    // what several es-toolkit `forOwn` / `assignIn` specs assert must not
    // happen. The property is therefore left uninstalled.
    let source = r#"
import { test, expect } from "vitest";
test("a non-enumerable definition does not become an own key", () => {
  const object: any = { visible: 1 };
  Object.defineProperty(object, "hidden", { enumerable: false, value: 2 });
  expect(Object.keys(object)).toEqual(["visible"]);
  expect({ ...object }).toEqual({ visible: 1 });
});
"#;
    run_fixture(source, "smelt_define_properties_non_enumerable");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn object_literal_method_shorthand_is_callable() {
    // `{ f() {} }` and `{ f: function () {} }` build the same object. The
    // shorthand used to lower to `null`, so calling it produced a null-callback
    // panic or a silent `null` result.
    let source = r#"
import { test, expect } from "vitest";
test("a method shorthand is a callable function property", () => {
  const table: any = {
    zero() { return 0; },
    add(a: number, b: number) { return a + b; },
    greet(name: string) { return "hi " + name; },
  };
  expect(table.zero()).toBe(0);
  expect(table.add(2, 3)).toBe(5);
  expect(table.greet("es-toolkit")).toBe("hi es-toolkit");
  expect(typeof table.add).toBe("function");
});
"#;
    run_fixture(source, "smelt_object_method_shorthand");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn number_is_safe_integer_answers_without_coercing() {
    // ECMAScript's `Number.isSafeInteger` performs no ToNumber step: every
    // non-Number argument is `false`, so `'1'` is not a safe integer even though
    // `Number('1')` is. The erased operand is tested on its runtime tag.
    let source = r#"
import { test, expect } from "vitest";
function isLength(value?: any): boolean {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}
test("safe integers inside the double range", () => {
  expect(isLength(0)).toBe(true);
  expect(isLength(3)).toBe(true);
  expect(isLength(Number.MAX_SAFE_INTEGER)).toBe(true);
});
test("non-integers, negatives and non-numbers are not lengths", () => {
  expect(isLength(-1)).toBe(false);
  expect(isLength("1")).toBe(false);
  expect(isLength(1.1)).toBe(false);
  expect(isLength(Number.MAX_SAFE_INTEGER + 1)).toBe(false);
});
"#;
    run_fixture(source, "smelt_number_is_safe_integer");
}
