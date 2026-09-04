//! Runtime execution tests for `Object.create(prototype)`.
//!
//! `Object.create` must mint a NEW object every time. The lowering used to hand
//! back the prototype operand itself once it was erased, which broke the standard
//! clone idiom `Object.assign(Object.create(Object.getPrototypeOf(obj)), obj)` in
//! two different ways:
//!
//! 1. `Object.getPrototypeOf` on an erased value yields one of the opaque
//!    `"__smelt_proto:*"` *string* sentinels, so the created "object" was a
//!    string. Assigning fields onto it was a no-op and `Object.keys` then
//!    enumerated the string's character indices instead of the copied keys —
//!    es-toolkit `clone` / `cloneDeep` / `cloneDeepWith` all returned an object
//!    with no properties.
//! 2. Handed a concrete prototype object, the result ALIASED that prototype, so
//!    the subsequent `Object.assign` mutated the prototype rather than a copy.
//!
//! A string-golden test (`object_create_mints_a_fresh_object_instead_of_aliasing_the_prototype`)
//! proves the helper is emitted; only executing the program proves the resulting
//! object actually carries the assigned keys and is distinct from its prototype.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test object_create_runtime -- --ignored
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
        "generated Object.create test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-object-create-runtime-{}-{seq}",
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
fn object_create_from_a_prototype_sentinel_accepts_assigned_keys() {
    // The es-toolkit `clone` shape. `Object.getPrototypeOf(obj)` is an opaque
    // sentinel; the object created from it must still be a real object that
    // `Object.assign` can write to and `Object.keys` can enumerate. Before the
    // fix `Object.keys(cloned)` read back the character indices of the sentinel
    // string `"__smelt_proto:object"` (twenty entries, "0".."19").
    let source = r#"
import { test, expect } from "vitest";
function shallowClone(obj: any): any {
  return Object.assign(Object.create(Object.getPrototypeOf(obj)), obj);
}
test("clone through Object.create carries the assigned keys", () => {
  const obj: any = { a: 1, b: "two" };
  const cloned = shallowClone(obj);
  expect(Object.keys(cloned).length).toBe(2);
  expect(cloned.a).toBe(1);
  expect(cloned.b).toBe("two");
  expect(cloned).toEqual(obj);
  expect(cloned).not.toBe(obj);
});
"#;
    run_fixture(source, "smelt_object_create_sentinel_assign");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn object_create_writes_do_not_reach_the_prototype() {
    // Handed a concrete prototype object, `Object.create` used to return that
    // same object, so writing to the "created" object mutated the prototype.
    // The prototype's own keys must also stay out of the created object's own-key
    // enumeration, matching JS (they are inherited, not own).
    let source = r#"
import { test, expect } from "vitest";
test("writes through a created object leave the prototype alone", () => {
  const proto: any = { shared: 1 };
  const child: any = Object.create(proto);
  child.own = 2;
  expect(Object.keys(child).length).toBe(1);
  expect(child.own).toBe(2);
  expect(Object.keys(proto).length).toBe(1);
  expect(proto.shared).toBe(1);
  expect(proto.own).toBe(undefined);
  expect(child).not.toBe(proto);
});
test("inherited members are readable but not own keys", () => {
  const proto: any = { shared: 1 };
  const child: any = Object.create(proto);
  expect(child.shared).toBe(1);
  expect(Object.keys(child).length).toBe(0);
  child.shared = 9;
  expect(child.shared).toBe(9);
  expect(proto.shared).toBe(1);
});
"#;
    run_fixture(source, "smelt_object_create_prototype_isolation");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn object_create_returns_a_distinct_object_per_call() {
    // Each `Object.create` call is a fresh reference: two objects created from
    // one prototype are not `===`, and filling one does not fill the other.
    // Returning the prototype operand made every call produce the same identity.
    let source = r#"
import { test, expect } from "vitest";
test("each Object.create call yields a distinct object", () => {
  const proto: any = {};
  const first: any = Object.create(proto);
  const second: any = Object.create(proto);
  expect(first).not.toBe(second);
  first.x = 1;
  expect(second.x).toBe(undefined);
});
test("Object.create(null) yields an empty writable object", () => {
  const bare: any = Object.create(null);
  expect(Object.keys(bare).length).toBe(0);
  bare.k = 7;
  expect(bare.k).toBe(7);
});
"#;
    run_fixture(source, "smelt_object_create_distinct_identities");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn inherited_members_are_not_own_keys_through_a_property_key_record() {
    // `Object.create({a: 1})` stores the inherited member as the marker key
    // `"__smelt_proto:a"`, and the erased-object -> `SmeltJsMap` coercion copies
    // that key in verbatim. Own-key enumeration on the `SmeltJsMap` backing used
    // to filter symbols only, so the marker leaked out as an ordinary string
    // key: es-toolkit `invert` inverted the inherited property and answered
    // `{1: "__smelt_proto:a", 2: "b"}` instead of `{2: "b"}`.
    //
    // A `Record<PropertyKey, unknown>` parameter is what selects that backing
    // (the key type is not `string`), which is why this shape and not a plain
    // `Record<string, unknown>` is the regression.
    let source = r#"
import { test, expect } from "vitest";
function ownKeys(obj: Record<PropertyKey, unknown>): string[] {
  return Object.keys(obj);
}
function ownValues(obj: Record<PropertyKey, unknown>): unknown[] {
  return Object.values(obj);
}
function ownEntryCount(obj: Record<PropertyKey, unknown>): number {
  return Object.entries(obj).length;
}
function forInKeys(obj: Record<PropertyKey, unknown>): string[] {
  const seen: string[] = [];
  for (const key in obj) {
    seen.push(key);
  }
  return seen;
}
test("an inherited member is not an own key", () => {
  const object: any = Object.create({ a: 1 });
  object.b = 2;
  const keys = ownKeys(object);
  expect(keys.length).toBe(1);
  expect(keys[0]).toBe("b");
});
test("an inherited member is not an own value or entry", () => {
  const object: any = Object.create({ a: 1 });
  object.b = 2;
  expect(ownValues(object).length).toBe(1);
  expect(ownValues(object)[0]).toBe(2);
  expect(ownEntryCount(object)).toBe(1);
});
test("for...in does walk the inherited member, unprefixed", () => {
  const object: any = Object.create({ a: 1 });
  object.b = 2;
  const keys = forInKeys(object);
  expect(keys.length).toBe(2);
  expect(keys.indexOf("b") >= 0).toBe(true);
  expect(keys.indexOf("a") >= 0).toBe(true);
  expect(keys.indexOf("__smelt_proto:a")).toBe(-1);
});
test("a class instance's methods are not own keys either", () => {
  class Holder {
    value = 1;
    describe(): string {
      return "held";
    }
  }
  const instance: any = new Holder();
  const keys = ownKeys(instance);
  expect(keys.length).toBe(1);
  expect(keys[0]).toBe("value");
});
"#;
    run_fixture(source, "smelt_own_keys_property_key_record");
}


#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn object_create_keeps_the_prototype_chains_identity() {
    // The prototype CHAIN is observable, not just the inherited members:
    // `Object.getPrototypeOf(Object.create(p))` is `p` itself, so
    // `Object.getPrototypeOf(Object.getPrototypeOf(...))` is `Object.prototype`
    // and not `null`. That two-step walk is exactly what separates a plain
    // object from `Object.create({})`, and both `Object.create({ ... })` (which
    // used to be inlined as bare `__smelt_proto:` entries) and
    // `Object.create(null)` (which used to fold to an empty object literal)
    // answered it wrongly.
    //
    // The recorded prototype is a hidden marker, so it stays out of key
    // enumeration, `for...in`, JSON and structural equality; and it is recorded
    // only when the result's own shape cannot already imply it, so the
    // `Object.create(Object.getPrototypeOf(o))` clone idiom over a plain object
    // still produces a marker-free object.
    let source = r#"
import { test, expect } from "vitest";
function isPlainObject(value: unknown): boolean {
  if (!value || typeof value !== "object") {
    return false;
  }
  const proto = Object.getPrototypeOf(value) as typeof Object.prototype | null;
  const hasObjectPrototype =
    proto === null || proto === Object.prototype || Object.getPrototypeOf(proto) === null;
  if (!hasObjectPrototype) {
    return false;
  }
  return Object.prototype.toString.call(value) === "[object Object]";
}
test("getPrototypeOf answers the very object Object.create was given", () => {
  const parent = { a: 1 };
  const child: any = Object.create(parent);
  expect(Object.getPrototypeOf(child)).toBe(parent);
  expect(Object.getPrototypeOf(Object.create(null))).toBe(null);
  expect(Object.getPrototypeOf({})).toBe(Object.prototype);
  expect(child.a).toBe(1);
});
test("the recorded prototype is not an own property", () => {
  const child: any = Object.create({ a: 1 });
  expect(Object.keys(child)).toEqual([]);
  expect(Object.values(child)).toEqual([]);
  expect(Object.entries(child).length).toBe(0);
  child.b = 2;
  expect(Object.keys(child)).toEqual(["b"]);
  expect(child).toEqual({ b: 2 });
  expect(JSON.stringify(child)).toBe(JSON.stringify({ b: 2 }));
});
test("only a direct Object.prototype (or null) makes a plain object", () => {
  expect(isPlainObject({})).toBe(true);
  expect(isPlainObject({ a: 1 })).toBe(true);
  expect(isPlainObject(Object.create(null))).toBe(true);
  expect(isPlainObject(Object.create({}))).toBe(false);
  expect(isPlainObject(Object.create({ a: 1 }))).toBe(false);
});
test("cloning a plain object through its own prototype adds no marker", () => {
  const original: any = { a: 1, b: "two" };
  const clone: any = Object.assign(Object.create(Object.getPrototypeOf(original)), original);
  expect(clone).toEqual(original);
  expect(Object.keys(clone)).toEqual(["a", "b"]);
  expect(isPlainObject(clone)).toBe(true);
});
test("Object.assign returns its target, so the target's prototype survives", () => {
  // The `withNullPrototype` idiom, and the reason the prototype is recorded by
  // IDENTITY: this value round-trips through a typed `Record<string, number>`
  // recovery, which maps every ENTRY through the value conversion. A prototype
  // held as a hidden entry came back as `NaN` here — neither `null` nor
  // `Object.prototype`, so remeda's `isDeepEqual` refused to compare it.
  const withNullProto = (data: Record<string, number>): Record<string, number> =>
    Object.assign(Object.create(null), data);
  const bare: any = withNullProto({ a: 1 });
  expect(Object.getPrototypeOf(bare)).toBe(null);
  expect(Object.keys(bare)).toEqual(["a"]);
  expect(bare.a).toBe(1);
  expect("a" in bare).toBe(true);
  expect(bare).toEqual({ a: 1 });
  expect(isPlainObject(bare)).toBe(true);
});
"#;
    run_fixture(source, "smelt_object_create_prototype_identity");
}
