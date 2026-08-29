//! Runtime execution tests for values that cross the erasure boundary and come
//! back — the round trip every `clone`/`cloneDeep`/`structuredClone` helper
//! performs on a host object.
//!
//! Three defects here type-checked, compiled, and produced the WRONG value:
//!
//! 1. **A `RegExp` lost `lastIndex`.** The `SmeltRegExp` erasure adapter wrote
//!    only `source` and `flags`, and the inverse rebuilt the pattern with
//!    `SmeltRegExp::new(..)` — whose `lastIndex` is 0. So any regexp that went
//!    through erased dataflow silently rewound.
//! 2. **A match result came back EMPTY.** `unknown -> <class>` had no arm for
//!    the concrete `SmeltMatch` type, so it fell to the generic class fallback
//!    and answered `Default::default()`: an empty match, with no diagnostic.
//!    `SmeltMatch` also derived `PartialEq` over its object `id`, so a match
//!    could never compare equal to a copy of itself.
//! 3. **A reflected clone of an error was not equal to its source.** An error
//!    record materialized `stack`/`cause` only when it had a value, while every
//!    clone helper writes the whole layout back — and a property store always
//!    creates the key, so the copy came back with MORE properties than the
//!    original. `Object.getPrototypeOf(e).constructor` also answered the base
//!    `Error` for every subclass, so an `AggregateError` was rebuilt through the
//!    `(message, options)` signature and lost its `errors`.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it. The tier is
//! `#[ignore]`d because it compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test erased_host_roundtrip_runtime -- --ignored
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
        "generated host-representation test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-erased-host-roundtrip-runtime-{}-{seq}",
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

/// A generic identity helper whose body forces its argument through the erased
/// `unknown` carrier and back — the shape every `clone`/`cloneDeep` helper has.
const ROUND_TRIP: &str = r#"
function roundTrip<T>(value: T): T {
  const erased = value as unknown;
  return erased as T;
}
"#;

/// es-toolkit `clone`'s error branch, written out: read the value's prototype,
/// call its `constructor`, then copy `stack` across.
const REBUILD_ERROR: &str = r#"
function rebuild<T>(obj: T): T {
  const prototype = Object.getPrototypeOf(obj);
  const Ctor = prototype.constructor;
  if (obj instanceof Error) {
    let newError;
    if (obj instanceof AggregateError) {
      newError = new Ctor(obj.errors, obj.message, { cause: obj.cause });
    } else {
      newError = new Ctor(obj.message, { cause: obj.cause });
    }
    newError.stack = obj.stack;
    return newError;
  }
  return obj;
}
"#;

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_regexp_keeps_its_last_index_across_the_erasure_boundary() {
    // `lastIndex` is a writable own property of a JavaScript RegExp, so it has to
    // survive a round trip through erased dataflow exactly as `source`/`flags`
    // do. The erasure adapter wrote only `source` and `flags`, so the rebuilt
    // pattern silently rewound to 0. It must also stay NON-enumerable: adding it
    // to the record must not make it an own key.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{ROUND_TRIP}
test("a round-tripped regexp keeps source, flags and lastIndex", () => {{
  const regex = /abc/gsu;
  regex.lastIndex = 10;
  const restored = roundTrip(regex);
  expect(restored.source).toBe("abc");
  expect(restored.flags).toBe("gsu");
  expect(restored.lastIndex).toBe(10);
}});
test("lastIndex is not an enumerable own key", () => {{
  const regex = /abc/g;
  regex.lastIndex = 3;
  expect(Object.keys(regex as unknown as Record<string, unknown>)).toEqual([]);
}});
"#
    );
    run_fixture(&source, "smelt_regexp_last_index_roundtrip");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_match_result_survives_the_erasure_boundary() {
    // The generic `unknown -> class` fallback answered `Default::default()`, so a
    // match that crossed the boundary came back EMPTY — no groups, index 0, empty
    // input — with no diagnostic. `SmeltMatch` also derived `PartialEq` over its
    // object `id`, so a match could never be equal to a copy of itself while
    // still being a distinct reference; both halves are asserted here.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{ROUND_TRIP}
test("two equal matches compare equal without being the same reference", () => {{
  const first = /t(e)st/.exec("hello test");
  const second = /t(e)st/.exec("hello test");
  expect(first).toEqual(second);
  expect(first).not.toBe(second);
}});
test("a round-tripped match keeps its groups and offsets", () => {{
  const matched = /t(e)st/.exec("hello test");
  const restored = roundTrip(matched);
  expect(restored).toEqual(matched);
  expect(restored?.index).toBe(6);
  expect(restored?.input).toBe("hello test");
  expect(restored?.[1]).toBe("e");
}});
"#
    );
    run_fixture(&source, "smelt_match_result_roundtrip");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_reflectively_rebuilt_error_equals_the_error_it_was_built_from() {
    // The rebuilt record must be structurally equal to its source, which needs
    // the whole `Error` layout present on both: an error that materialized
    // `stack`/`cause` only when it had a value came back from the rebuild with
    // MORE properties than the original, because a property STORE always creates
    // the key. An `AggregateError` must also rebuild through ITS constructor
    // rather than the base `Error` one, or the leading `errors` argument lands in
    // the message slot and `errors` is lost outright.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{REBUILD_ERROR}
test("a rebuilt Error equals the error it came from", () => {{
  const error = new Error("boom", {{ cause: "why" }});
  const rebuilt = rebuild(error);
  expect(rebuilt).toEqual(error);
  expect(rebuilt).not.toBe(error);
  expect(rebuilt.message).toBe("boom");
  expect(rebuilt.cause).toBe("why");
}});
test("an Error without a cause still equals its rebuild", () => {{
  const error = new Error("boom");
  expect(rebuild(error)).toEqual(error);
}});
test("a rebuilt AggregateError keeps its errors and its message", () => {{
  const aggregate = new AggregateError([new Error("first")], "several");
  const rebuilt = rebuild(aggregate);
  expect(rebuilt).toEqual(aggregate);
  expect(rebuilt.message).toBe("several");
  expect(rebuilt.errors).toEqual(aggregate.errors);
}});
test("the error layout slots are not enumerable own keys", () => {{
  const error = new Error("boom");
  expect(Object.keys(error as unknown as Record<string, unknown>)).toEqual([]);
}});
"#
    );
    run_fixture(&source, "smelt_reflected_error_rebuild");
}
