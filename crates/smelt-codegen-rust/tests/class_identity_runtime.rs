//! Runtime execution tests for erased class identity via `instance.constructor`.
//!
//! JavaScript exposes `instance.constructor`, and real code branches on it.
//! es-toolkit's `isEqualWith` gates instance comparison on
//!
//! ```js
//! areObjectsEqual(a.constructor, b.constructor, …) || (isPlainObject(a) && isPlainObject(b))
//! ```
//!
//! The hidden class marker used to be `Bool(true)` for every class, so an erased
//! instance had no way to name its class and `.constructor` read `undefined`. With
//! both sides `undefined`, `Object.is(undefined, undefined)` held, so two instances
//! of *different* classes compared equal — a false positive in a deep-equality
//! primitive.
//!
//! The marker now records the class name and `.constructor` interns one value per
//! name. It is a `Function` deliberately: `areObjectsEqual` tags it
//! `[object Function]` and that branch decides by identity, so it never recurses
//! into the constructor's own `.constructor` the way an object value would.
//!
//! A string-golden test (`class_instances_expose_a_per_class_constructor_identity`)
//! proves the wiring is emitted; only running the program proves the identities
//! actually agree and differ in the right places.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test class_identity_runtime -- --ignored
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
        "generated class-identity test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-class-identity-runtime-{}-{seq}",
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
fn declared_class_instances_compare_by_class() {
    // Two instances of one class share a constructor identity; two classes do not.
    // A plain object keeps `undefined`, which is what lets the
    // `isPlainObject(a) && isPlainObject(b)` arm handle plain objects instead.
    let source = r#"
import { test, expect } from "vitest";
class Alpha {
  a = 1;
}
class Beta {
  a = 1;
}
test("same class shares one constructor identity", () => {
  const first: any = new Alpha();
  const second: any = new Alpha();
  expect(first.constructor).toBe(second.constructor);
});
test("different classes have different constructor identities", () => {
  const alpha: any = new Alpha();
  const beta: any = new Beta();
  expect(alpha.constructor).not.toBe(beta.constructor);
});
test("a plain object has no constructor identity", () => {
  const plain: any = { a: 1 };
  expect(plain.constructor).toBe(undefined);
});
"#;
    run_fixture(source, "smelt_declared_class_constructor_identity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn constructor_function_instances_compare_by_class() {
    // The JavaScript constructor-function idiom, which is what es-toolkit's
    // `isEqualWith` spec actually writes: a plain `function` plus a `prototype`
    // member, constructed with `new`. The frontend already synthesizes a class for
    // this shape, so the instances carry own fields — what was missing was the
    // constructor identity that distinguishes `new Foo()` from `new Bar()`.
    //
    // The declarations sit INSIDE the test body on purpose. Constructor-function
    // synthesis is wired for function-body scopes (`predeclare_local_function_declarations`)
    // and test-setup scopes, not for module top level — a module-scope
    // `function Foo(){this.a=1}` with `new Foo()` still fails to lower with
    // "unresolved class Foo". That is a separate gap, and it is the shape
    // build-time specialization would materialize.
    let source = r#"
import { test, expect } from "vitest";
test("constructor-function instances carry their own fields", () => {
  function ZzFoo() {
    // @ts-ignore
    this.a = 1;
  }
  ZzFoo.prototype.a = 1;
  // @ts-ignore
  const instance: any = new ZzFoo();
  expect(instance.a).toBe(1);
  expect(Object.keys(instance).length).toBe(1);
});
test("two instances of one constructor function are the same class", () => {
  function ZzFoo() {
    // @ts-ignore
    this.a = 1;
  }
  ZzFoo.prototype.a = 1;
  // @ts-ignore
  const first: any = new ZzFoo();
  // @ts-ignore
  const second: any = new ZzFoo();
  expect(first.constructor).toBe(second.constructor);
});
test("instances of different constructor functions are different classes", () => {
  function ZzFoo() {
    // @ts-ignore
    this.a = 1;
  }
  ZzFoo.prototype.a = 1;
  function ZzBar() {
    // @ts-ignore
    this.a = 1;
  }
  ZzBar.prototype.a = 2;
  // @ts-ignore
  const foo: any = new ZzFoo();
  // @ts-ignore
  const bar: any = new ZzBar();
  expect(foo.constructor).not.toBe(bar.constructor);
});
"#;
    run_fixture(source, "smelt_constructor_function_identity");
}
