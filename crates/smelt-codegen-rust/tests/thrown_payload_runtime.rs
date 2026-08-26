//! Runtime execution tests for the value a `throw` delivers to its `catch`.
//!
//! JavaScript's `throw` is value-preserving for *every* operand: `throw new
//! TypeError(x)`, `throw 'a string'`, `throw {code: 1}` and `throw
//! someCaughtValue` each deliver exactly the value that was written. Smelt's
//! exception-payload ABI (`crate::thrown`) was already able to carry any of
//! them, and `new Error(m)` used as a *value* already lowered to the erased
//! record `{ __smelt_error, message, cause?, errors? }`.
//!
//! The `throw` *operand* was the hole. Two independent lowering paths narrowed
//! it before it ever reached the payload ABI:
//!
//! 1. the throw statement (`throw_message_expression`) replaced a thrown
//!    `new Error(m)` with `m`;
//! 2. the reduced callback expression language (`callback_throw_message`) did
//!    the same for `Error`/`TypeError`/`RangeError` inside an arrow, and
//!    replaced every other construction with the empty string.
//!
//! So every `throw` in a generated crate entered the channel as
//! `smelt_throw(SmeltUnknown::String(..))`. Downstream, `error instanceof Error`
//! was false, `error.message` was `undefined`, `error.name` was unreadable, and
//! a `cause` was gone — while the identical construction bound to a `const` kept
//! all of it. Only *execution* distinguishes the two: the emitted code compiled
//! either way, and a golden asserting on `smelt_throw(` looked healthy the whole
//! time.
//!
//! Each case is a TypeScript Vitest test, lowering emits a `#[test]`, and a
//! green `cargo test` on the generated crate means every `expect(...)` held.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test thrown_payload_runtime -- --ignored
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
        "generated thrown-payload test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-thrown-payload-runtime-{}-{seq}",
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
fn a_caught_error_keeps_its_class_name_and_message() {
    // The headline case. All three reads were broken by the same narrowing:
    // `instanceof Error` tests the `__smelt_error` marker that was thrown away,
    // and `.name`/`.message` are record fields that no longer existed. The
    // `TypeError` half additionally pins the *class* the marker carries: a
    // narrowing that kept only the message could not tell the two apart even if
    // it had happened to preserve one.
    let source = r#"
import { test, expect } from "vitest";

function boom(): number {
  throw new Error("kaboom");
}

function badType(): number {
  throw new TypeError("bad type");
}

test("a caught Error keeps its identity, name and message", () => {
  try {
    boom();
    expect("unreachable").toBe("threw");
  } catch (error: any) {
    expect(error instanceof Error).toBe(true);
    expect(error.name).toBe("Error");
    expect(error.message).toBe("kaboom");
    expect(String(error)).toBe("Error: kaboom");
  }

  try {
    badType();
    expect("unreachable").toBe("threw");
  } catch (error: any) {
    expect(error instanceof Error).toBe(true);
    expect(error.name).toBe("TypeError");
    expect(error.message).toBe("bad type");
  }
});
"#;
    run_fixture(source, "smelt_thrown_error_identity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_thrown_primitive_is_not_wrapped_in_an_error() {
    // The fix is "stop narrowing the operand", not "make every throw an Error".
    // JavaScript does not wrap a thrown primitive, so a `throw 'text'` must still
    // arrive as a string and must *not* answer true to `instanceof Error`. This
    // is the guard against over-correcting the previous defect into its mirror
    // image.
    let source = r#"
import { test, expect } from "vitest";

function boom(): number {
  throw "a bare string";
}

test("a thrown string arrives as a string", () => {
  try {
    boom();
    expect("unreachable").toBe("threw");
  } catch (error: any) {
    expect(typeof error).toBe("string");
    expect(error).toBe("a bare string");
    expect(error instanceof Error).toBe(false);
  }
});
"#;
    run_fixture(source, "smelt_thrown_primitive");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_thrown_plain_object_keeps_its_fields() {
    // `throw {code: 1}` never went through the Error narrowing, so this case
    // documents the behaviour the fix had to leave untouched: an arbitrary
    // object operand reaches the `catch` with its fields intact.
    let source = r#"
import { test, expect } from "vitest";

function boom(): number {
  throw { code: 1, tag: "plain" };
}

test("a thrown plain object keeps its fields", () => {
  try {
    boom();
    expect("unreachable").toBe("threw");
  } catch (error: any) {
    expect(error.code).toBe(1);
    expect(error.tag).toBe("plain");
    expect(error instanceof Error).toBe(false);
  }
});
"#;
    run_fixture(source, "smelt_thrown_plain_object");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_rethrown_value_reaches_the_outer_catch_unchanged() {
    // `throw error` re-enters the channel with a value that is already erased.
    // Preserving the operand has to be idempotent: recovering a payload and
    // throwing it straight back must not re-wrap it or flatten it to its message
    // on the way out.
    let source = r#"
import { test, expect } from "vitest";

function boom(): number {
  throw new RangeError("out of range");
}

function relay(): number {
  try {
    return boom();
  } catch (error: any) {
    throw error;
  }
}

test("a rethrown value reaches the outer catch unchanged", () => {
  try {
    relay();
    expect("unreachable").toBe("threw");
  } catch (error: any) {
    expect(error instanceof Error).toBe(true);
    expect(error.name).toBe("RangeError");
    expect(error.message).toBe("out of range");
  }
});
"#;
    run_fixture(source, "smelt_thrown_rethrow");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_callback_thrown_error_keeps_its_message() {
    // The second narrowing site. A `throw` written inside an arrow lowers through
    // the reduced callback expression language, which stripped `new Error(m)` to
    // `m` independently of the statement path — this is the
    // `attempt(() => { throw new Error(..) })` shape. Fixing only the statement
    // path would have left this red.
    let source = r#"
import { test, expect } from "vitest";

function apply(f: () => number): number {
  try {
    return f();
  } catch (error: any) {
    if (error instanceof Error) {
      return error.message.length;
    }
    return -1;
  }
}

test("an Error thrown inside a callback keeps its message", () => {
  const length = apply(() => {
    throw new Error("callback boom");
  });
  expect(length).toBe(13);
});
"#;
    run_fixture(source, "smelt_thrown_callback_error");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_awaited_rejection_delivers_the_error_object() {
    // An `async` function's throw travels the same channel through a `Future`
    // rather than a direct `Err` return, and `.rejects.toThrow(..)`-style
    // assertions all sit downstream of it. The rejection must arrive as the error
    // object, not as its message.
    let source = r#"
import { test, expect } from "vitest";

async function rejects(): Promise<number> {
  throw new Error("async boom");
}

test("an awaited rejection delivers the error object", async () => {
  try {
    await rejects();
    expect("unreachable").toBe("threw");
  } catch (error: any) {
    expect(error instanceof Error).toBe(true);
    expect(error.message).toBe("async boom");
  }
});
"#;
    run_fixture(source, "smelt_thrown_async_rejection");
}
