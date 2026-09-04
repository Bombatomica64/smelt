//! Runtime execution tests for JavaScript construction through a function value.
//!
//! A JavaScript function is an object, and every non-arrow function is a
//! constructor. Smelt used to model neither, and the three gaps compounded:
//!
//! * **`f.prototype` read as `undefined`.** An erased property read on a
//!   `SmeltUnknown::Function` answered `undefined` for every field, so the
//!   standard `if (func.prototype) { wrapper.prototype = Object.create(func.prototype) }`
//!   idiom (es-toolkit `partialImpl`) never took its branch.
//! * **A property write onto a function value was DISCARDED.** The write place
//!   had no slot for an undeclared member, so `wrapper.prototype = …` emitted
//!   `let _ = value;`.
//! * **`new f(args)` lowered to a plain call**, dropping the object allocation,
//!   the receiver, and the "return the allocated object unless the body returned
//!   an object" rule; and **`x instanceof f` folded to `false`** at compile
//!   time for every function-valued target.
//!
//! The four rules checked here, each against Node first:
//!
//! * `new C()` yields a fresh object that `instanceof C` accepts.
//! * A constructor that returns an object yields THAT object; one that returns a
//!   primitive yields the allocated one.
//! * The allocated object is the constructor's `this`, so `this.x = 1` is
//!   readable on the result.
//! * A wrapper whose `prototype` was replaced by `Object.create(C.prototype)`
//!   constructs instances that are still `instanceof C` — the es-toolkit
//!   `partial` shape, and the case that needs the prototype LINK rather than a
//!   copy of the prototype's members. (What the wrapper's own body observes as
//!   its `this` is the separate receiver-channel rule, so this case asserts only
//!   the chain.)
//! * `f.prototype` is an object whose `constructor` is `f` itself, and the same
//!   object on every read.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test construct_semantics_runtime -- --ignored
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
        "generated construction test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-construct-semantics-runtime-{}-{seq}",
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
fn new_on_a_function_value_allocates_an_instance_of_it() {
    // The base case both halves of the feature need: construction allocates an
    // object, and `instanceof` finds the constructor's `prototype` on that
    // object's chain. `new (C as any)()` used to be a plain call returning
    // `undefined`, and `o instanceof C` folded to a compile-time `false`.
    let source = r#"
import { test, expect } from "vitest";
test("new through a function value yields an instance of it", () => {
  function C() {}
  const made: any = new (C as any)();
  expect(typeof made).toBe("object");
  expect(made instanceof C).toBe(true);
  const other: any = new (C as any)();
  expect(other instanceof C).toBe(true);
  expect(other).not.toBe(made);
});
test("an unrelated constructor is not on the chain", () => {
  function C() {}
  function D() {}
  const made: any = new (C as any)();
  expect(made instanceof D).toBe(false);
});
"#;
    run_fixture(source, "smelt_construct_allocates_instance");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_constructor_body_can_override_or_populate_the_allocated_object() {
    // Two spec rules that a plain call cannot express. An object RETURN wins
    // over the allocated object; a primitive return is ignored and the allocated
    // object is answered instead. And the allocated object is the body's `this`,
    // so fields the body assigns are readable on the result.
    let source = r#"
import { test, expect } from "vitest";
test("an object return wins over the allocated object", () => {
  function C() {
    return { tag: 1 };
  }
  const made: any = new (C as any)();
  expect(made.tag).toBe(1);
  expect(made instanceof C).toBe(false);
});
test("a primitive return is ignored", () => {
  function C() {
    return 7;
  }
  const made: any = new (C as any)();
  expect(made instanceof C).toBe(true);
});
test("the allocated object is the constructor's receiver", () => {
  function C(this: any) {
    this.x = 1;
  }
  const made: any = new (C as any)();
  expect(made.x).toBe(1);
  expect(made instanceof C).toBe(true);
});
test("constructor arguments reach the body", () => {
  function C(this: any, value: number) {
    this.x = value;
  }
  const made: any = new (C as any)(5);
  expect(made.x).toBe(5);
});
"#;
    run_fixture(source, "smelt_construct_return_and_receiver");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrapper_whose_prototype_was_replaced_still_constructs_instances_of_the_original() {
    // The es-toolkit `partial` shape, and the reason `Object.create(proto)` must
    // record a LINK to `proto` and not merely copy its members: the chain of the
    // object `new w()` allocates has to reach `C.prototype` through the
    // intermediate object. It also exercises the write side — an undeclared
    // property write onto a function value used to be discarded outright.
    let source = r#"
import { test, expect } from "vitest";
test("a wrapper with a derived prototype constructs instances of the original", () => {
  function C(this: any) {
    this.tag = "c";
  }
  const w = function (this: any) {
    return (C as any).apply(this, []);
  };
  (w as any).prototype = Object.create((C as any).prototype);
  const made: any = new (w as any)();
  expect(made instanceof C).toBe(true);
  const plain: any = new (C as any)();
  expect(plain instanceof C).toBe(true);
});
test("a property written onto a function value reads back", () => {
  function C() {}
  (C as any).marker = 42;
  expect((C as any).marker).toBe(42);
});
"#;
    run_fixture(source, "smelt_construct_derived_prototype");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_function_owns_one_prototype_object_carrying_its_constructor() {
    // `f.prototype` is an OWN property of the function, created once: two reads
    // answer the same object, and its `constructor` is the function itself. This
    // is the read that `if (func.prototype)` branches on, and it used to be
    // `undefined` for every function value.
    let source = r#"
import { test, expect } from "vitest";
test("a function's prototype is one object whose constructor is the function", () => {
  function C() {}
  const proto: any = (C as any).prototype;
  expect(typeof proto).toBe("object");
  expect(proto.constructor).toBe(C);
  expect((C as any).prototype).toBe(proto);
});
test("a prototype read on an erased function value sees the same object", () => {
  function C() {}
  function readPrototype(f: unknown): unknown {
    return (f as any).prototype;
  }
  expect(readPrototype(C)).toBe((C as any).prototype);
});
"#;
    run_fixture(source, "smelt_construct_function_prototype");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_three_prototype_spellings_read_one_chain() {
    // `Object.getPrototypeOf(v)`, `v.__proto__` and `x instanceof F` are three
    // spellings of ONE question, so they must read one prototype link. They were
    // modeled twice at one point — a hidden `__smelt_proto:__proto__` entry on
    // the object and an identity-keyed table — which let them disagree: an
    // erased object round-tripped through a typed record loses hidden entries
    // but keeps its identity, so one spelling advanced the chain and the other
    // did not. The link now lives only in the identity-keyed table
    // (`SMELT_OBJECT_PROTOTYPES`), and this pins all three to it.
    let source = r#"
import { test, expect } from "vitest";
test("getPrototypeOf and __proto__ answer the same Object.create argument", () => {
  const base: any = { a: 1 };
  const made: any = Object.create(base);
  expect(Object.getPrototypeOf(made)).toBe(base);
  expect(made.__proto__).toBe(base);
  expect(Object.getPrototypeOf(Object.getPrototypeOf(made))).toBe(Object.prototype);
});
test("instanceof walks the same chain Object.getPrototypeOf reports", () => {
  function C(this: any) {
    this.tag = "c";
  }
  const made: any = new (C as any)();
  expect(Object.getPrototypeOf(made)).toBe((C as any).prototype);
  expect(made.__proto__).toBe((C as any).prototype);
  expect(made instanceof C).toBe(true);
});
test("an inherited member does not surface as a __proto__ key", () => {
  const made: any = Object.create({ a: 1 });
  const keys: string[] = [];
  for (const key in made) {
    keys.push(key);
  }
  expect(keys).toEqual(["a"]);
  expect(Object.keys(made)).toEqual([]);
});
"#;
    run_fixture(source, "smelt_construct_one_prototype_chain");
}
