//! Runtime execution tests for the parts of the JavaScript object model Smelt
//! used to model as something simpler than it is.
//!
//! Four rules, each checked against Node before being written down:
//!
//! * **An array is an object with named properties.** `const a = ['1']; a.x = 2`
//!   keeps `a` an array — `Array.isArray(a)`, `a[0] === '1'`, `a.length === 1` —
//!   and adds a property that reads back, enumerates after the index keys, and
//!   is invisible to array equality (JavaScript compares arrays index-wise).
//!   Both store seams used to REPLACE the array with a one-property object,
//!   losing every element.
//! * **`Object.prototype`'s members are inherited by every object.**
//!   `'toString' in {}` is `true`, `({}).toString` is a function, and two reads
//!   of it are `===` because the function lives once on the prototype. They are
//!   a lookup fallback, never entries: `Object.keys`, `for...in` and structural
//!   equality must not see them.
//! * **A well-known symbol is a value AND a key.** `typeof Symbol.iterator` is
//!   `'symbol'`, and `obj[Symbol.iterator]` names the member an inline
//!   `[Symbol.iterator]` key declares — one shared table relates the two
//!   spellings. A string-valued `[Symbol.toStringTag]` also wins over the
//!   builtin tag (ES2024 §20.1.3.6), so a tagged object is not a plain object.
//! * **A subclass of a builtin error is an error.** `class C extends Error {}`
//!   satisfies `value instanceof Error` even after the value has crossed an
//!   erasure boundary into an `unknown` parameter, while `instanceof TypeError`
//!   stays `false`.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test object_model_runtime -- --ignored
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
        "generated object-model test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-object-model-runtime-{}-{seq}",
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
fn an_array_carries_named_properties_and_stays_an_array() {
    // Every fixture writes and reads through an ERASED receiver, which is the
    // only place a named property on an array is observable: TypeScript's `T[]`
    // has no named members, so source that sets one has to lose the array type
    // first (es-toolkit's `merge` receives its target as `unknown`, and
    // `isEqualWith`'s spec declares `let array1: any`). `erase` is the boundary
    // hop that models it -- a value typed `unknown` end to end.
    //
    // Both store spellings are covered: the dotted `a.x = 2` (the static-member
    // store) and the computed `a[k] = v` (the runtime index-assign helper).
    // Before the fix either one replaced the array with `{ x: 2 }`.
    let source = r#"
import { test, expect } from "vitest";
function erase(value: unknown): unknown {
  return value;
}
test("a dotted named write keeps the array", () => {
  const a: any = erase(["1"]);
  a.x = 2;
  expect(Array.isArray(a)).toBe(true);
  expect(a[0]).toBe("1");
  expect(a.length).toBe(1);
  expect(a.x).toBe(2);
  expect(a["x"]).toBe(2);
  expect(Object.keys(a)).toEqual(["0", "x"]);
  expect("x" in a).toBe(true);
  expect("y" in a).toBe(false);
  a.x = 3;
  expect(a.x).toBe(3);
  expect(Object.keys(a)).toEqual(["0", "x"]);
});
test("a computed named write keeps the array, and an index write is still an element", () => {
  const a: any = erase([1, 2, 3]);
  const key = "note";
  a[key] = "kept";
  a[1] = 9;
  expect(Array.isArray(a)).toBe(true);
  expect(a.length).toBe(3);
  expect(a[1]).toBe(9);
  expect(a[key]).toBe("kept");
  expect(Object.keys(a)).toEqual(["0", "1", "2", "note"]);
  expect(Object.values(a)).toEqual([1, 9, 3, "kept"]);
});
test("named properties are shared by every handle on the array", () => {
  const a: any = erase([1]);
  const alias: any = a;
  alias.tag = "shared";
  expect(a.tag).toBe("shared");
});
test("arrays with equal elements and different named properties are equal", () => {
  const left: any = erase([1, 2, 3]);
  const right: any = erase([1, 2, 3]);
  left.every = null;
  right.concat = null;
  expect(left).toEqual(right);
});
"#;
    run_fixture(source, "smelt_object_model_array_named_properties");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn object_prototype_members_resolve_as_an_inherited_fallback() {
    // Read through an ERASED receiver, the boundary where a property name that
    // is not a declared member can be asked for at all (a typed
    // `Record<string, number>` cannot answer `.toString` with a function, and
    // TypeScript would reject the read). The fallback answers a read and a
    // presence check, has ONE identity per member, and stays out of enumeration
    // and structural equality.
    let source = r#"
import { test, expect } from "vitest";
function erase(value: unknown): unknown {
  return value;
}
test("every object inherits Object.prototype's members", () => {
  const o: any = erase({ a: 1 });
  expect("toString" in o).toBe(true);
  expect("valueOf" in o).toBe(true);
  expect(typeof o.toString).toBe("function");
  expect(o.toString).toBe(Object.prototype.toString);
  const other: any = erase({ b: 2 });
  expect(o.toString).toBe(other.toString);
  expect(typeof Object.prototype.hasOwnProperty).toBe("function");
});
test("the inherited members are not entries", () => {
  const o: any = erase({ a: 1 });
  expect(Object.keys(o)).toEqual(["a"]);
  const seen: string[] = [];
  for (const key in o) {
    seen.push(key);
  }
  expect(seen).toEqual(["a"]);
  expect(o).toEqual({ a: 1 });
});
test("an own property still shadows the inherited one", () => {
  const shadowed: any = erase({ toString: 42 });
  expect(shadowed.toString).toBe(42);
  expect(Object.keys(shadowed)).toEqual(["toString"]);
});
"#;
    run_fixture(source, "smelt_object_model_object_prototype");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_well_known_symbol_is_a_value_and_a_key() {
    // The value half (`typeof`, identity, holding it in a variable) and the key
    // half (a declaration and a read agreeing on one member) must come from one
    // table, or a symbol used as a value and the same symbol used as a key name
    // different things.
    let source = r#"
import { test, expect } from "vitest";
test("a well-known symbol is a symbol value", () => {
  expect(typeof Symbol.iterator).toBe("symbol");
  expect(typeof Symbol.toStringTag).toBe("symbol");
  const held: any = Symbol.iterator;
  expect(typeof held).toBe("symbol");
  expect(held).toBe(Symbol.iterator);
  expect(Symbol.iterator === Symbol.asyncIterator).toBe(false);
});
test("the value and the key agree about which member they name", () => {
  const tagged: any = { [Symbol.toStringTag]: "tagged" };
  const key: any = Symbol.toStringTag;
  expect(tagged[Symbol.toStringTag]).toBe("tagged");
  expect(tagged[key]).toBe("tagged");
  expect(Object.prototype.toString.call(tagged)).toBe("[object tagged]");
  expect(Object.prototype.toString.call({ a: 1 })).toBe("[object Object]");
});
"#;
    run_fixture(source, "smelt_object_model_well_known_symbols");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_subclass_of_a_builtin_error_is_an_error_after_erasure() {
    // The value crosses an erasure boundary (an `unknown` parameter), so only
    // the markers stamped at erasure survive: the nearest builtin error base's
    // name, which answers `instanceof Error` and `instanceof TypeError`
    // correctly without claiming every error class.
    let source = r#"
import { test, expect } from "vitest";
class CustomError extends Error {}
class CustomTypeError extends TypeError {}
function isError(value: unknown): boolean {
  return value instanceof Error;
}
function isTypeError(value: unknown): boolean {
  return value instanceof TypeError;
}
test("an erased subclass instance is still an Error", () => {
  expect(isError(new CustomError())).toBe(true);
  expect(isError(new Error())).toBe(true);
  expect(isError(new CustomTypeError())).toBe(true);
  expect(isError({ message: "not an error" })).toBe(false);
});
test("a subclass of a specific builtin error keeps that base's identity", () => {
  expect(isTypeError(new CustomTypeError())).toBe(true);
  expect(isTypeError(new CustomError())).toBe(false);
  expect(isTypeError(new Error())).toBe(false);
});
"#;
    run_fixture(source, "smelt_object_model_error_subclass");
}
