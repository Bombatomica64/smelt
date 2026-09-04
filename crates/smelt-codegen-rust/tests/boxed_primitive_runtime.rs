//! Runtime execution tests for boxed primitives and `Object.prototype.valueOf`.
//!
//! `Object(1)` and `new Number(1)` erase to a marker object
//! `{ __smelt_number: true, value: 1 }`. Two gaps made that representation
//! unusable from source code:
//!
//! 1. `Object(value)` called as a FUNCTION was not recognized at all, so it
//!    produced a null-ish value with an `[object Null]` tag instead of a wrapper.
//! 2. `.valueOf()` resolved through the ordinary own-field read.
//!    `Object.prototype.valueOf` exists on every value but is an own property of
//!    none, so the read found nothing and fell through to the null callback:
//!    `Object(1).valueOf()` answered `null`, and so did `(1).valueOf()` on an
//!    erased number.
//!
//! Deep-equality code branches on exactly this pair. es-toolkit `isEqualWith`
//! compares a boxed against an unboxed primitive by matching their
//! `Object.prototype.toString` tags and then comparing
//! `Object.is(a.valueOf(), b.valueOf())`, so both halves have to work for
//! `isEqualWith(1, Object(1), noop)` to answer `true`.
//!
//! The `Object('a')` CALL deliberately does not box — see `smelt_box_value` in
//! the prelude for why — so it stays a plain string and compares equal to `'a'`
//! through the primitive path rather than the wrapper path. `new String('a')` is
//! the other spelling and does box: `new` builds an object, so the wrapper has
//! reference identity of its own and is never `===` its payload.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test boxed_primitive_runtime -- --ignored
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
        "generated boxed-primitive test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-boxed-primitive-runtime-{}-{seq}",
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
fn object_call_boxes_primitives_with_the_matching_tag() {
    // `Object(value)` must produce a wrapper whose
    // `Object.prototype.toString` tag matches the primitive's, so tag-first
    // deep-equality code reaches its numeric/boolean branch instead of bailing
    // out on mismatched tags. `typeof` sees an object; the tag sees the
    // primitive's class. Before the fix the tag read `[object Null]`.
    let source = r#"
import { test, expect } from "vitest";
test("boxed primitives keep their primitive tag", () => {
  const boxedNumber: any = Object(1);
  expect(typeof boxedNumber).toBe("object");
  expect(Object.prototype.toString.call(boxedNumber)).toBe("[object Number]");
  const boxedBool: any = Object(true);
  expect(typeof boxedBool).toBe("object");
  expect(Object.prototype.toString.call(boxedBool)).toBe("[object Boolean]");
});
test("Object of an object is that object", () => {
  const source: any = { a: 1 };
  expect(Object(source)).toBe(source);
});
test("Object of null or undefined is a fresh empty object", () => {
  const fromNull: any = Object(null);
  expect(Object.keys(fromNull).length).toBe(0);
  expect(Object.prototype.toString.call(fromNull)).toBe("[object Object]");
  expect(Object(null)).not.toBe(fromNull);
});
"#;
    run_fixture(source, "smelt_object_call_boxes_primitives");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn new_string_boxes_its_payload_with_its_own_identity() {
    // `new String(x)` is a construction, so it produces a wrapper OBJECT — the
    // same treatment `new Number`/`new Boolean` already got. Returning the
    // primitive instead cost the wrapper everything that distinguishes it: two
    // constructions were the same value, the wrapper was `===` its payload, and
    // a clone of it could not be a distinct object. The payload's own
    // properties (`length`, the indexed characters) stay readable through the
    // wrapper, and the bare `String(x)` CAST is unaffected.
    let source = r#"
import { test, expect } from "vitest";
test("a boxed string is an object holding its payload", () => {
  const boxed: any = new String("ab");
  expect(typeof boxed).toBe("object");
  expect(boxed.valueOf()).toBe("ab");
  expect(boxed.length).toBe(2);
  expect(boxed[0]).toBe("a");
});
test("a boxed string has its own identity", () => {
  const boxed: any = new String("ab");
  expect(boxed).not.toBe("ab");
  expect(new String("a")).not.toBe(new String("a"));
});
test("the String cast still yields a primitive", () => {
  const cast: any = String("ab");
  expect(typeof cast).toBe("string");
  expect(cast).toBe("ab");
});
test("an empty boxed string holds the empty payload", () => {
  const boxed: any = new String();
  expect(boxed.valueOf()).toBe("");
  expect(boxed.length).toBe(0);
});
"#;
    run_fixture(source, "smelt_new_string_boxes");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn value_of_unwraps_wrappers_and_is_identity_on_primitives() {
    // `valueOf` is an own property of no value, so it can only resolve through
    // the runtime helper. Every erased receiver must answer: a wrapper unwraps to
    // the primitive it holds, a bare primitive is its own `valueOf`, a Date
    // unwraps to its epoch milliseconds, and a plain object is itself. Before the
    // fix every one of these answered `null`.
    let source = r#"
import { test, expect } from "vitest";
test("valueOf unwraps boxed primitives", () => {
  const boxedNumber: any = Object(42);
  expect(boxedNumber.valueOf()).toBe(42);
  const boxedBool: any = Object(false);
  expect(boxedBool.valueOf()).toBe(false);
  const constructed: any = new Number(7);
  expect(constructed.valueOf()).toBe(7);
});
test("valueOf on a primitive is the primitive", () => {
  const value: any = 3;
  expect(value.valueOf()).toBe(3);
  const flag: any = true;
  expect(flag.valueOf()).toBe(true);
});
test("valueOf on a Date is its epoch milliseconds", () => {
  const date: any = new Date(0);
  expect(date.valueOf()).toBe(0);
});
test("an own valueOf still wins", () => {
  const custom: any = { valueOf: () => 99 };
  expect(custom.valueOf()).toBe(99);
});
"#;
    run_fixture(source, "smelt_value_of_unwraps_wrappers");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn boxed_and_unboxed_primitives_compare_equal_through_value_of() {
    // The composed shape es-toolkit `isEqualWith` uses: match the
    // `Object.prototype.toString` tags, then compare the unwrapped values. This
    // is the assertion that fails if either half of the fix is missing, and it is
    // deliberately written the way the library writes it rather than calling the
    // pieces separately.
    let source = r#"
import { test, expect } from "vitest";
function tagEqual(a: any, b: any): boolean {
  if (Object.prototype.toString.call(a) !== Object.prototype.toString.call(b)) {
    return false;
  }
  return Object.is(a.valueOf(), b.valueOf());
}
test("a wrapper equals its primitive through valueOf", () => {
  expect(tagEqual(1, Object(1))).toBe(true);
  expect(tagEqual(Object(0), Object(0))).toBe(true);
  expect(tagEqual(true, Object(true))).toBe(true);
  expect(tagEqual(NaN, Object(NaN))).toBe(true);
});
test("distinct values stay unequal", () => {
  expect(tagEqual(1, Object(2))).toBe(false);
  expect(tagEqual(1, "1")).toBe(false);
  expect(tagEqual(true, Object(false))).toBe(false);
});
"#;
    run_fixture(source, "smelt_boxed_primitive_value_equality");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_valueless_return_reads_as_undefined_not_null() {
    // Falling off the end of a JavaScript function evaluates to `undefined`. The
    // value-less return terminator used to emit `SmeltUnknown::Null`, and at an
    // erased return type that difference is observable — which is what broke the
    // customizer protocol es-toolkit's `cloneDeepWith` uses: it treats
    // `undefined` as "no custom value, keep cloning" and anything else, `null`
    // included, as the replacement. A fall-through customizer therefore replaced
    // the whole clone with `null` on its first call.
    //
    // The `apply` helper below is that protocol in miniature. An explicit
    // `return null` must stay `null`, so both spellings are asserted together.
    //
    // The null-returning customizer is spelled with an explicit `: any` return
    // annotation on purpose. A bare `(v) => null` infers `Type::None`, which HIR
    // still conflates with `void`, so the callback adapter drops the value and
    // substitutes `undefined` — a separate, pre-existing gap (the `null`/`void`
    // conflation tracked by `specs/distinct-undefined.md`), not this fix.
    let source = r#"
import { test, expect } from "vitest";
function apply(value: any, customize: (v: any) => any): any {
  const custom = customize(value);
  if (custom !== undefined) {
    return custom;
  }
  return value;
}
test("a fall-through customizer defers instead of replacing", () => {
  const doubleNumbers = (v: any) => {
    if (typeof v === "number") {
      return v * 2;
    }
  };
  expect(apply(3, doubleNumbers)).toBe(6);
  expect(apply("keep", doubleNumbers)).toBe("keep");
});
test("an explicit null still replaces", () => {
  const nullify = (v: any): any => {
    return null;
  };
  expect(apply("gone", nullify)).toBe(null);
});
test("a bare return and a missing return both read as undefined", () => {
  const bare = (v: any) => {
    return;
  };
  const empty = (v: any) => {};
  expect(bare(1)).toBe(undefined);
  expect(empty(1)).toBe(undefined);
  expect(bare(1)).not.toBe(null);
});
"#;
    run_fixture(source, "smelt_valueless_return_is_undefined");
}
