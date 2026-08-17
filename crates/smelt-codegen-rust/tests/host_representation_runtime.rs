//! Runtime execution tests for the byte-backed host objects, host-constructor
//! identity, and the `arguments` exotic object.
//!
//! These three families used to be identity-only stand-ins, and every clone or
//! deep-equality path over binary data walks straight into what was missing:
//!
//! * `ArrayBuffer` / `SharedArrayBuffer` / Node `Buffer` / `DataView` carried a
//!   marker and nothing else. `buffer.slice(0)` handed the *same* object back, so
//!   the standard `clone(buf) { return buf.slice(0) }` shape returned its own
//!   argument; `byteLength`, `byteOffset` and indexed element reads answered
//!   `undefined`; and `new Uint8Array(arrayBuffer)` panicked outright.
//! * A bare `Blob` reference built a *fresh* record per mention, so `Blob === Blob`
//!   was false, and a host record's `.constructor` was `undefined`, so
//!   `clonedBlob.constructor === Blob` could never hold.
//! * `arguments` was a `{ length: n }` stand-in holding no argument values, so
//!   `Object.keys(arguments)` enumerated `["length"]` rather than the index keys.
//!   Comparing an `arguments` object with the plain object of the same indexed
//!   properties — which is what `isEqual(toArgs([1, 2, 3]), { 0: 1, 1: 2, 2: 3 })`
//!   does — could not work.
//!
//! String-golden tests in `src/tests/part_7_tests.rs` prove the right helpers and
//! markers are *emitted*. Only running the program proves the resulting records
//! carry real bytes, keep distinct identities, and enumerate the right keys.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test host_representation_runtime -- --ignored
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
        "smelt-host-representation-runtime-{}-{seq}",
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
fn byte_buffer_slice_returns_a_distinct_record_of_the_same_host_kind() {
    // `buffer.slice(0)` is *the* shallow-clone shape for a byte buffer, so it has
    // to answer a new object of the same host identity with the same bytes.
    // Previously the erased-slice lowering forwarded any non-array receiver
    // unchanged, so `cloned === buffer` and `expect(cloned).not.toBe(buffer)`
    // could not hold. A slice must also report *its own* length, not its source's.
    let source = r#"
import { test, expect } from "vitest";
test("ArrayBuffer.slice yields a distinct ArrayBuffer with the same bytes", () => {
  const buffer = new ArrayBuffer(8);
  const cloned = buffer.slice(0);
  expect(cloned).not.toBe(buffer);
  expect(cloned).toEqual(buffer);
  expect(cloned instanceof ArrayBuffer).toBe(true);
  expect(cloned.byteLength).toBe(8);
});
test("SharedArrayBuffer keeps its own byte length", () => {
  const buffer = new SharedArrayBuffer(8);
  expect(buffer.byteLength).toBe(8);
  expect(buffer instanceof SharedArrayBuffer).toBe(true);
});
test("a partial slice reports the sliced length, not the source length", () => {
  const buffer = new ArrayBuffer(8);
  const part = buffer.slice(2, 5);
  expect(part.byteLength).toBe(3);
  expect(buffer.byteLength).toBe(8);
});
test("Buffer.subarray yields a distinct Buffer with the same bytes", () => {
  const buffer = Buffer.from([1, 2, 3]);
  const sub = (buffer as any).subarray();
  expect(sub).not.toBe(buffer);
  expect(sub).toEqual(buffer);
  expect(sub.length).toBe(3);
});
"#;
    run_fixture(source, "smelt_byte_buffer_slice");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn byte_buffer_elements_are_readable_writable_and_viewable() {
    // A byte buffer's indexed slots are its bytes. Index reads used to miss the
    // `bytes` storage and answer `null`, which made the `a[i]` element walk that
    // deep-equality performs over a typed-array-tagged value compare two different
    // buffers as equal. `new Uint8Array(arrayBuffer)` panicked with
    // "unknown is not array" instead of viewing the buffer's bytes.
    let source = r#"
import { test, expect } from "vitest";
test("Buffer index reads see the bytes", () => {
  const buffer = Buffer.from([7, 8, 9]);
  const asAny = buffer as any;
  expect(asAny[0]).toBe(7);
  expect(asAny[2]).toBe(9);
  expect(asAny.length).toBe(3);
});
test("index writes land in the byte storage", () => {
  const buffer = Buffer.from([0, 0, 0]);
  const asAny = buffer as any;
  asAny[1] = 5;
  expect(asAny[1]).toBe(5);
  expect(asAny[0]).toBe(0);
});
test("a typed array over an ArrayBuffer views its bytes", () => {
  const buffer = new ArrayBuffer(4);
  const view = new Uint8Array(buffer);
  expect(view.length).toBe(4);
  expect(view[0]).toBe(0);
});
test("ArrayBuffer.isView distinguishes views from storage", () => {
  expect(ArrayBuffer.isView(new ArrayBuffer(4))).toBe(false);
  expect(ArrayBuffer.isView(new SharedArrayBuffer(4))).toBe(false);
  expect(ArrayBuffer.isView(Buffer.from([1]))).toBe(true);
  expect(ArrayBuffer.isView(new DataView(new ArrayBuffer(4)))).toBe(true);
});
"#;
    run_fixture(source, "smelt_byte_buffer_elements");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn dataview_keeps_its_buffer_offset_and_length() {
    // A `DataView` is a *window* over separate storage, so it must retain the
    // buffer it was built over plus its offset and length — all three of which the
    // reflected clone shape `new Constructor(view.buffer.slice(0))` reads back.
    let source = r#"
import { test, expect } from "vitest";
test("a DataView over a whole buffer reports the full window", () => {
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer);
  expect(view.byteOffset).toBe(0);
  expect(view.byteLength).toBe(8);
  expect(view.buffer instanceof ArrayBuffer).toBe(true);
  expect(view instanceof DataView).toBe(true);
});
test("an offset DataView reports its own window", () => {
  const buffer = new ArrayBuffer(8);
  const view = new DataView(buffer, 1, 2);
  expect(view.byteOffset).toBe(1);
  expect(view.byteLength).toBe(2);
});
// The binding is deliberately NOT named `Constructor`: a local with that exact
// name makes the frontend resolve a `.constructor` field read to the key
// `"Constructor"` instead, a pre-existing source-name interning quirk unrelated
// to host representation.
test("the reflected constructor rebuilds a DataView, not a bare copy", () => {
  const view: any = new DataView(new ArrayBuffer(8));
  const Ctor: any = Object.getPrototypeOf(view).constructor;
  const rebuilt: any = new Ctor(view.buffer.slice(0));
  expect(rebuilt).not.toBe(view);
  expect(rebuilt instanceof DataView).toBe(true);
  expect(rebuilt.byteLength).toBe(8);
  expect(rebuilt).toEqual(view);
});
"#;
    run_fixture(source, "smelt_dataview_window");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn host_constructor_values_are_interned_and_reachable_through_constructor() {
    // JavaScript exposes one object per global builtin name, so `Blob === Blob`
    // and `blob.constructor === Blob` both hold. A record literal mints a fresh
    // identity on construction, so building a namespace record per reference made
    // both comparisons false — and a host record answered `undefined` for
    // `.constructor` besides. A plain object must still answer `undefined`, which
    // is what makes two plain objects compare as equal instances.
    let source = r#"
import { test, expect } from "vitest";
test("a bare host constructor reference is one object", () => {
  expect((Blob as any) === (Blob as any)).toBe(true);
  expect((ArrayBuffer as any) === (ArrayBuffer as any)).toBe(true);
});
test("a host record's constructor is its own global", () => {
  const blob: any = new Blob(["hello"], { type: "text/plain" });
  expect(blob.constructor === Blob).toBe(true);
  const buffer: any = new ArrayBuffer(4);
  expect(buffer.constructor === ArrayBuffer).toBe(true);
});
test("a plain object's constructor stays undefined", () => {
  const plain: any = { a: 1 };
  expect(plain.constructor).toBe(undefined);
});
test("a host record does not leak its marker keys", () => {
  const blob: any = new Blob(["hello"]);
  expect(Object.keys(blob).length).toBe(0);
});
"#;
    run_fixture(source, "smelt_host_constructor_identity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn arguments_object_carries_values_with_a_non_enumerable_length() {
    // The `arguments` exotic object stores the actual call arguments under index
    // keys and hides `length` from own-key enumeration. Both halves are
    // load-bearing: comparing an `arguments` object against the plain object with
    // the same indexed properties compares their key *sets*, so an enumerable
    // `length` (or missing index keys) breaks the comparison in opposite
    // directions.
    let source = r#"
import { test, expect } from "vitest";
function toArgs(array: unknown[]): any {
  return (function (..._: unknown[]) {
    return arguments;
  })(...array);
}
test("arguments enumerates its index keys and not length", () => {
  const args: any = toArgs([1, 2, 3]);
  expect(Object.keys(args).length).toBe(3);
  expect(args.length).toBe(3);
  expect(args[0]).toBe(1);
  expect(args[2]).toBe(3);
});
test("an empty arguments object has no own keys", () => {
  const args: any = (function () {
    return arguments;
  })();
  expect(Object.keys(args).length).toBe(0);
  expect(args.length).toBe(0);
});
test("arguments tags as Arguments", () => {
  const args: any = toArgs([1]);
  expect(Object.prototype.toString.call(args)).toBe("[object Arguments]");
});
test("positional parameters also become arguments elements", () => {
  function twoParams(a: number, b: number): any {
    const unused = a + b;
    return arguments;
  }
  const args: any = twoParams(4, 5);
  expect(Object.keys(args).length).toBe(2);
  expect(args[0]).toBe(4);
  expect(args[1]).toBe(5);
});
"#;
    run_fixture(source, "smelt_arguments_object");
}
