//! Runtime execution tests for two JavaScript value semantics that only a
//! *running* program can distinguish, because each defect produced Rust that
//! compiled cleanly and merely computed the wrong value.
//!
//! 1. **A missing property reads as `undefined`, not `null`.** The inline
//!    emitters for a dynamic member read substituted `SmeltUnknown::Null` when a
//!    key was absent, while the runtime prelude helper `smelt_get_object_field`
//!    already answered `SmeltUnknown::Undefined`. The two are distinguishable
//!    under `===`, so every `value === undefined` guard written over an erased
//!    record silently took the wrong branch.
//!
//! 2. **`Array.prototype.concat` decides spread-vs-append per argument at
//!    runtime.** When both the receiver's element type and the argument type were
//!    the erased `unknown`, the frontend matched its "scalar, append" arm and
//!    wrapped the argument in a singleton list, so `a.concat(b)` with `b` an
//!    array at runtime produced `[...a, b]` instead of `[...a, ...b]`. The choice
//!    is JavaScript's `IsConcatSpreadable` and is only knowable at runtime, so it
//!    now lowers to `ExprKind::ConcatSpread`.
//!
//! A third defect in the same family is characterized but deliberately NOT
//! fixed here: a contextually-typed callback whose body returns only `null` or
//! `undefined` infers `void`, collapsing both onto Rust `()`, and the adapter
//! that widens it into an erased slot then substitutes a constant. Taking the
//! contextual return type instead only trades *which* of the two is wrong,
//! because `Constant::Undefined` is already indistinguishable from `null` by the
//! time the return is coerced. Separating them is the all-or-nothing workstream
//! in `specs/distinct-undefined.md`.
//!
//! Each case is a TypeScript Vitest test: lowering emits a `#[test]`, and a green
//! `cargo test` on the generated crate means every `expect(...)` held. A string
//! golden could not stand in for any of these — the wrong value is produced by
//! Rust that looks entirely healthy.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test js_value_semantics_runtime -- --ignored
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
        "generated JS value semantics test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-js-value-semantics-runtime-{}-{seq}",
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
fn an_absent_property_reads_as_undefined_and_a_stored_null_stays_null() {
    // The three outcomes have to be told apart by the SAME read, which is why the
    // probe returns a tag rather than the value: a `null`/`undefined` mix-up is
    // invisible to anything that only checks "nullish". `SmeltUnknown` cannot be
    // avoided here — the receiver is a `Record<string, any>` whose values are
    // genuinely dynamic — so this pins the erased read's *value*, not its typing.
    let source = r#"
import { test, expect } from "vitest";

function probe(obj: Record<string, any>, key: string): string {
  const v = obj[key];
  if (v === undefined) {
    return "undefined";
  }
  if (v === null) {
    return "null";
  }
  return "value";
}

test("an absent key is undefined while a stored null stays null", () => {
  expect(probe({ a: 1 }, "missing")).toBe("undefined");
  expect(probe({ a: null }, "a")).toBe("null");
  expect(probe({ a: 1 }, "a")).toBe("value");
});
"#;
    run_fixture(source, "smelt_absent_property_is_undefined");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn concat_spreads_an_erased_array_argument_and_appends_an_erased_scalar() {
    // The receiver is narrowed by `Array.isArray`, so its element type is the
    // erased `unknown`; the argument is `unknown` too. That pair is exactly the
    // case a concrete type, a union, or a scoped generic CANNOT decide: the same
    // call site receives an array in one test and a scalar in the next, so no
    // static type can pick spread-vs-append. Only the runtime tag can, which is
    // what makes this a genuine dynamic boundary rather than avoidable erasure.
    //
    // The result is reported as `length:elements` so the assertion does not
    // depend on `JSON.stringify` number formatting.
    let source = r#"
import { test, expect } from "vitest";

function describeConcat(a: unknown, b: unknown): string {
  if (Array.isArray(a)) {
    const out = a.concat(b);
    return String(out.length) + ":" + out.map((v: any) => String(v)).join(",");
  }
  return "not-array";
}

function typedConcat(a: number[], b: number[]): string {
  const out = a.concat(b);
  return String(out.length) + ":" + out.map(v => String(v)).join(",");
}

test("concat spreads an array argument and appends a scalar one", () => {
  // Same call site, same static types, different runtime shapes.
  expect(describeConcat([1], [3])).toBe("2:1,3");
  expect(describeConcat([1], 3)).toBe("2:1,3");
  // A concretely-typed receiver keeps its existing statically-decided lowering.
  expect(typedConcat([1], [3])).toBe("2:1,3");
});

test("concat spreads a multi-element erased array argument", () => {
  expect(describeConcat([1, 2], [3, 4])).toBe("4:1,2,3,4");
  expect(describeConcat([], [1])).toBe("1:1");
});
"#;
    run_fixture(source, "smelt_concat_is_concat_spreadable");
}
