//! Runtime execution tests for the erasure of modeled host values.
//!
//! Six modeled classes are backed by generated runtime types whose state lives
//! in closures and cells rather than in declared fields: the two text codecs,
//! the `node:events` emitter, and the three `node:http` types. Erasing one used
//! to fall through to the GENERIC STRUCT path, which reads declared fields and
//! stamps `__smelt_class` — and a prelude type has no declared fields, so the
//! erased value carried no host identity and no state. The consequences were
//! silent:
//!
//! 1. **`instanceof` could not be answered**, so codegen folded it to `false`
//!    and the whole branch disappeared with no diagnostic. That is the defect
//!    round 10 turned into a blocker and this round retires; these cases are
//!    what prove the branch now runs.
//! 2. **`Object.prototype.toString` had no tag**, because the tag table is
//!    driven by the same registry marker.
//! 3. **Narrowing an erased value produced nothing usable**, since there was no
//!    state in the record to rebuild from.
//!
//! The emitter case is the one that shows why the erasure RETAINS the live value
//! rather than rebuilding an equal one. A listener list cannot round-trip
//! through a record, and JavaScript does not rebuild anything anyway: erasing a
//! value and narrowing it back yields the SAME object. So the erased record
//! carries the value's own object id, the live value is retained under that id,
//! and narrowing hands that value back — which is why a listener registered on
//! the original fires through the narrowed handle.
//!
//! This lives in a tier rather than in the example corpus on purpose. The
//! examples carry a hard "avoidable erasure == 0" invariant, and a fixture whose
//! whole subject is the erased boundary is 53 lines of deliberate erasure; it
//! would either break that invariant or force the classifier to be weakened for
//! every corpus. A tier is not scanned by it, and it can also assert runtime
//! behaviour a golden cannot see.
//!
//! Each case is a TypeScript Vitest test; lowering emits a `#[test]`, and this
//! tier emits the crate and runs `cargo test` on it, so a green run means the
//! generated `expect(...)` calls held at runtime. The tier is `#[ignore]`d
//! because it compiles and executes a real crate. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test host_erasure_runtime -- --ignored
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
        "generated host-erasure test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-host-erasure-runtime-{}-{seq}", std::process::id()))
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
fn instanceof_holds_on_an_erased_codec() {
    let source = r#"
import { test, expect } from "vitest";
test("an erased TextEncoder is still a TextEncoder", () => {
  const value: unknown = new TextEncoder();
  expect(value instanceof TextEncoder).toBe(true);
  // The branch has to actually run: folding the check to `false` is what made
  // this silently disappear.
  let ran = false;
  if (value instanceof TextEncoder) {
    ran = true;
    expect(value.encode("abc").length).toBe(3);
  }
  expect(ran).toBe(true);
});
test("an erased TextDecoder is still a TextDecoder, and decodes", () => {
  const value: unknown = new TextDecoder();
  expect(value instanceof TextDecoder).toBe(true);
  if (value instanceof TextDecoder) {
    expect(value.decode(new TextEncoder().encode("hi"))).toBe("hi");
  }
});
test("a codec is not an instance of the other codec", () => {
  const encoder: unknown = new TextEncoder();
  expect(encoder instanceof TextDecoder).toBe(false);
});
test("a non-host value is not an instance of a codec", () => {
  const plain: unknown = "text";
  expect(plain instanceof TextEncoder).toBe(false);
});
"#;
    run_fixture(source, "erased_codec_identity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_erased_codec_carries_its_spec_tag_and_encoding() {
    let source = r#"
import { test, expect } from "vitest";
test("the erased codecs carry their spec toStringTag", () => {
  const encoder: unknown = new TextEncoder();
  const decoder: unknown = new TextDecoder();
  expect(Object.prototype.toString.call(encoder)).toBe("[object TextEncoder]");
  expect(Object.prototype.toString.call(decoder)).toBe("[object TextDecoder]");
});
test("the erased record exposes the spec's readable data property", () => {
  const encoder: unknown = new TextEncoder();
  if (encoder instanceof TextEncoder) {
    expect(encoder.encoding).toBe("utf-8");
  }
});
test("the identity marker is not enumerable", () => {
  const encoder: unknown = new TextEncoder();
  // A host record enumerates nothing: the marker hides the whole record from
  // key enumeration, so the `encoding` property added for readers cannot leak
  // into `Object.keys`.
  expect(Object.keys(encoder as object).length).toBe(0);
});
"#;
    run_fixture(source, "erased_codec_tag");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn narrowing_an_erased_emitter_reaches_the_same_listener_list() {
    let source = r#"
import { test, expect } from "vitest";
import { EventEmitter } from "node:events";

test("a listener registered before erasure fires through the narrowed handle", () => {
  const emitter = new EventEmitter();
  let seen = "none";
  emitter.on("ping", () => {
    seen = "heard";
  });

  const erased: unknown = emitter;
  expect(erased instanceof EventEmitter).toBe(true);
  if (erased instanceof EventEmitter) {
    erased.emit("ping", []);
  }
  // The whole point of retaining the live value rather than rebuilding one: a
  // rebuilt emitter would have no listeners and this would still be "none".
  expect(seen).toBe("heard");
});

test("a listener registered through the narrowed handle fires on the original", () => {
  const emitter = new EventEmitter();
  let seen = "none";
  const erased: unknown = emitter;
  if (erased instanceof EventEmitter) {
    erased.on("pong", () => {
      seen = "heard";
    });
  }
  emitter.emit("pong", []);
  expect(seen).toBe("heard");
});

test("erasing one emitter twice yields one object", () => {
  const emitter = new EventEmitter();
  const first: unknown = emitter;
  const second: unknown = emitter;
  expect(first === second).toBe(true);
});

test("two distinct emitters are distinct erased", () => {
  const first: unknown = new EventEmitter();
  const second: unknown = new EventEmitter();
  expect(first === second).toBe(false);
});
"#;
    run_fixture(source, "erased_emitter_identity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_erased_codec_survives_a_round_trip_through_a_function() {
    let source = r#"
import { test, expect } from "vitest";

function encodeThrough(value: unknown, text: string): number {
  if (value instanceof TextEncoder) {
    return value.encode(text).length;
  }
  return -1;
}

test("a codec passed as unknown and narrowed inside a function still encodes", () => {
  const encoder = new TextEncoder();
  expect(encodeThrough(encoder, "héllo")).toBe(6);
  // -1 is the "narrowing failed" answer, which is what a folded `false` gave
  // for every call before the marker existed.
  expect(encodeThrough("not a codec", "héllo")).toBe(-1);
});
"#;
    run_fixture(source, "erased_codec_across_a_function");
}
