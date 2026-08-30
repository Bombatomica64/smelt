//! Runtime execution tests for the JavaScript `__proto__` accessor.
//!
//! `__proto__` is not an ordinary property. It is an accessor inherited from
//! `Object.prototype` whose getter is `Object.getPrototypeOf(Object(this))`, so
//! `x.__proto__` must answer the *prototype*. Smelt lowered it as a plain field
//! read, which looked for an own `__proto__` slot and, finding none, produced
//! `undefined` — es-toolkit's `merge` / `mergeWith` prototype-pollution specs
//! (`expect(result.__proto__).toBe(Object.prototype)`) failed on exactly that.
//!
//! The one case where an own slot is real: a value whose prototype is `null`
//! (`Object.create(null)`) does not inherit the accessor, so a `__proto__` write
//! there stores an ordinary own property that a later read answers, that
//! `Object.keys`, `for...in` and spread all see, and that survives a spread
//! into a fresh object. Smelt represents a null-prototype
//! object as a plain erased object, so the own slot is the only observable trace
//! of that case and the accessor prefers it. `Object.getPrototypeOf` is
//! specified to ignore own properties and keeps its own lowering.
//!
//! Every assertion below was checked against Node before being written down.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test proto_accessor_runtime -- --ignored
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
        "generated __proto__ accessor test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-proto-accessor-runtime-{}-{seq}",
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
fn proto_read_answers_the_prototype_not_an_own_slot() {
    // The plain read. Before the fix every one of these was `undefined`, because
    // `__proto__` was looked up as an ordinary own key.
    let source = r#"
import { test, expect } from "vitest";
test("__proto__ reads the prototype", () => {
  const plain: any = { a: 1 };
  expect(plain.__proto__).toBe(Object.prototype);
  expect(plain.__proto__).toBe(Object.getPrototypeOf(plain));
  const arr: any = [1, 2];
  expect(arr.__proto__).toBe(Object.getPrototypeOf(arr));
  expect(arr.__proto__ === Object.prototype).toBe(false);
});
"#;
    run_fixture(source, "smelt_proto_accessor_reads_prototype");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn proto_write_on_a_null_prototype_object_stays_an_own_property() {
    // es-toolkit's `merge` / `mergeWith` prototype-pollution shape, reduced. The
    // source is a null-prototype object, so `__proto__` really is an own key: it
    // enumerates, serializes and copies like any other. `merge` skips it, so the
    // merged result keeps `Object.prototype` — the assertion that used to fail.
    let source = r#"
import { test, expect } from "vitest";
test("a __proto__ own slot on a null-prototype object is an ordinary key", () => {
  const bare: any = Object.create(null);
  bare.__proto__ = { b: 2 };
  bare.a = 1;
  expect(Object.keys(bare)).toEqual(["__proto__", "a"]);
  expect(bare.__proto__).toEqual({ b: 2 });
  expect(bare.__proto__.b).toBe(2);
  const seen: string[] = [];
  for (const key in bare) {
    seen.push(key);
  }
  expect(seen).toEqual(["__proto__", "a"]);
  const spread: any = { ...bare };
  expect(Object.keys(spread)).toEqual(["__proto__", "a"]);
  expect(spread.__proto__).toEqual({ b: 2 });
  expect(spread).toEqual(bare);
});
test("merge skips the unsafe key, so the result keeps Object.prototype", () => {
  const target: any = { a: 1 };
  const source: any = Object.create(null);
  source.__proto__ = { b: 2 };
  source.a = 2;
  const keys = Object.keys(source);
  for (let i = 0; i < keys.length; i++) {
    const key = keys[i];
    if (key === "__proto__") {
      continue;
    }
    target[key] = source[key];
  }
  expect(target).toEqual({ a: 2 });
  expect(target.__proto__).toBe(Object.prototype);
});
"#;
    run_fixture(source, "smelt_proto_accessor_own_slot");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn ordinary_objects_keep_their_key_semantics() {
    // The accessor must not invent a `__proto__` key. Enumeration, spread, JSON
    // and structural equality on objects that never mention `__proto__` are
    // unchanged. (`JSON.stringify` is deliberately not asserted here: it emits
    // keys in insertion-reversed order and renders integers as `2.0`, both
    // pre-existing and unrelated to the accessor.), and two structurally equal objects stay equal.
    let source = r#"
import { test, expect } from "vitest";
test("objects without a __proto__ write are untouched", () => {
  const left: any = { a: 1, b: 2 };
  const right: any = { a: 1, b: 2 };
  expect(Object.keys(left)).toEqual(["a", "b"]);
  expect(left).toEqual(right);
  expect({ ...left }).toEqual(right);
  const seen: string[] = [];
  for (const key in left) {
    seen.push(key);
  }
  expect(seen).toEqual(["a", "b"]);
  expect(left.__proto__).toBe(right.__proto__);
});
"#;
    run_fixture(source, "smelt_proto_accessor_key_semantics");
}
