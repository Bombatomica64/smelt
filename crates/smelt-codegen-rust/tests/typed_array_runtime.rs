//! Runtime execution tests for the numeric typed-array views.
//!
//! All eleven views used to share one `Vec<f64>`, so none of them had any view
//! identity at all. Probe-confirmed consequences, every one of them wrong:
//!
//! * `Object.prototype.toString.call(new Float32Array(new ArrayBuffer(8)))`
//!   answered `[object Array]` — the same tag every other view answered, and the
//!   same tag a plain `number[]` answers, so no two views were distinguishable;
//! * its `length` was `8`, the *byte* count, where a `Float32Array` over eight
//!   bytes has two elements and a `Float64Array` has one;
//! * `new Int8Array([-1])` stored `-1.0` in a `f64`, so `.buffer` had no bytes to
//!   report and `new Uint8Array(thatBuffer)[0]` could not be `255`;
//! * `new Float32Array([1.1])[0]` answered the `f64` `1.1`, which no
//!   `Float32Array` can hold.
//!
//! The views are now byte-backed host objects carrying their own registry marker
//! *and their own element type*. That element type is the load-bearing part: the
//! stride turns a byte count into an element count, and the little-endian
//! decode/encode pair is what makes signedness and float precision observable.
//!
//! `length` alone could not carry view identity even in principle — `Uint8Array`
//! and `Uint8ClampedArray` over the same buffer have the *same* element count — so
//! `distinct_views_over_one_buffer_are_distinguishable` below pins the case that
//! forces genuinely distinct tags rather than a length heuristic.
//!
//! String-golden tests in `src/tests/part_7_tests.rs` prove the right helpers,
//! markers and codec arms are *emitted*. Only running the program proves the
//! records decode real elements at the right width and signedness.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test typed_array_runtime -- --ignored
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
        "generated typed-array test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-typed-array-runtime-{}-{seq}",
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
    let outcome = std::panic::catch_unwind(|| {
        emit_program(source, crate_name, &crate_dir);
        run_generated_tests(&crate_dir, &target_dir);
    });
    // The scratch root holds a whole nested cargo target directory, so it is
    // removed on the FAILURE path too: leaving one behind per failing case is
    // what fills `/tmp`, and an ENOSPC inside a later nested build reads as a
    // failing assertion rather than as a full disk.
    // `SMELT_KEEP_RUNTIME_SCRATCH=1` keeps it for a debugging session.
    if std::env::var_os("SMELT_KEEP_RUNTIME_SCRATCH").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn distinct_views_over_one_buffer_are_distinguishable() {
    // The case that rules out every length-based shortcut. Over the same eight
    // bytes a `Float32Array` has two elements, a `Float64Array` has one, and a
    // `Uint8Array` has eight — but `Uint8Array` and `Uint8ClampedArray` have the
    // *same* eight, so those two can only be told apart by their spec tags. All
    // four used to report `[object Array]` with length 8.
    let source = r#"
import { test, expect } from "vitest";
test("each view reports its own spec tag", () => {
  const buffer = new ArrayBuffer(8);
  const tag = (value: unknown): string => Object.prototype.toString.call(value);
  expect(tag(new Float32Array(buffer))).toBe("[object Float32Array]");
  expect(tag(new Float64Array(buffer))).toBe("[object Float64Array]");
  expect(tag(new Uint8Array(buffer))).toBe("[object Uint8Array]");
  expect(tag(new Uint8ClampedArray(buffer))).toBe("[object Uint8ClampedArray]");
  expect(tag(new Int16Array(buffer))).toBe("[object Int16Array]");
  expect(tag(new Int32Array(buffer))).toBe("[object Int32Array]");
  expect(tag(buffer)).toBe("[object ArrayBuffer]");
});
test("length is the element count, not the byte count", () => {
  const buffer = new ArrayBuffer(8);
  expect((new Uint8Array(buffer) as any).length).toBe(8);
  expect((new Int16Array(buffer) as any).length).toBe(4);
  expect((new Float32Array(buffer) as any).length).toBe(2);
  expect((new Float64Array(buffer) as any).length).toBe(1);
  expect((new Float64Array(buffer) as any).byteLength).toBe(8);
});
test("same element count, different identity", () => {
  const buffer = new ArrayBuffer(4);
  const plain = new Uint8Array(buffer) as any;
  const clamped = new Uint8ClampedArray(buffer) as any;
  expect(plain.length).toBe(clamped.length);
  expect(Object.prototype.toString.call(plain)).not.toBe(
    Object.prototype.toString.call(clamped)
  );
});
test("every view is a view and never the storage", () => {
  const buffer = new ArrayBuffer(4);
  expect(ArrayBuffer.isView(new Uint32Array(buffer))).toBe(true);
  expect(ArrayBuffer.isView(new Float64Array(buffer))).toBe(true);
  expect(ArrayBuffer.isView(buffer)).toBe(false);
});
"#;
    run_fixture(source, "smelt_typed_array_identity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn element_typing_is_real_not_a_marker() {
    // These are the assertions a marker-only "view identity" cannot satisfy, and
    // the ones a shared `Vec<f64>` cannot satisfy either. Each one reads a value
    // that only exists because the element type decides how bytes are interpreted:
    //
    // * `Int8Array` is signed, `Uint8Array` is not, over the *same byte*;
    // * `Float32Array` rounds to single precision, so its element is a value no
    //   `f64` list of the source literals would hold;
    // * an integer view wraps modulo its width, which is why an `Int8Array`
    //   holding `-1` and a `Uint8Array` holding `255` have identical buffers —
    //   the exact fact lodash-compatible `isEqual` asserts on `.buffer`;
    // * `Uint8ClampedArray` saturates where `Uint8Array` wraps.
    let source = r#"
import { test, expect } from "vitest";
test("signedness is decided by the element type", () => {
  const signed = new Int8Array([-1]) as any;
  expect(signed[0]).toBe(-1);
  const unsigned = new Uint8Array(signed.buffer) as any;
  expect(unsigned[0]).toBe(255);
  expect(unsigned.length).toBe(1);
});
test("an Int8Array of -1 and a Uint8Array of 255 share their bytes", () => {
  const fromSigned = (new Int8Array([-1]) as any).buffer;
  const fromUnsigned = (new Uint8Array([255]) as any).buffer;
  expect(fromSigned).toEqual(fromUnsigned);
  expect(fromSigned).not.toBe(fromUnsigned);
});
test("Float32Array elements are single precision", () => {
  const single = new Float32Array([1.1]) as any;
  const double = new Float64Array([1.1]) as any;
  expect(single[0]).not.toBe(1.1);
  expect(double[0]).toBe(1.1);
  expect(single.byteLength).toBe(4);
  expect(double.byteLength).toBe(8);
});
test("integer views wrap and clamped views saturate", () => {
  expect((new Uint8Array([300]) as any)[0]).toBe(44);
  expect((new Uint8ClampedArray([300]) as any)[0]).toBe(255);
  expect((new Uint8ClampedArray([-5]) as any)[0]).toBe(0);
  expect((new Int16Array([-1]) as any)[0]).toBe(-1);
  expect((new Uint16Array([-1]) as any)[0]).toBe(65535);
});
test("a wide view decodes wide elements out of its bytes", () => {
  const bytes = new Uint8Array([0, 0, 128, 63]) as any;
  const asFloat = new Float32Array(bytes.buffer) as any;
  expect(asFloat.length).toBe(1);
  expect(asFloat[0]).toBe(1);
});
"#;
    run_fixture(source, "smelt_typed_array_elements");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn views_expose_their_buffer_offset_and_element_slots() {
    // A view is a window onto an `ArrayBuffer`, so it answers `buffer`,
    // `byteOffset` and `byteLength`, its own properties are its element indices,
    // and an element write lands in the shared buffer record without minting a new
    // identity for it. `new Ctor(buffer, byteOffset, length)` — the three-argument
    // view form — used to have its extra arguments lowered for effect and dropped.
    let source = r#"
import { test, expect } from "vitest";
test("a view windows its buffer", () => {
  const buffer = new ArrayBuffer(24);
  const whole = new Uint8Array(buffer) as any;
  const part = new Uint8Array(buffer, 8, 4) as any;
  expect(whole.buffer).toBe(buffer);
  expect(part.buffer).toBe(buffer);
  expect(whole.byteOffset).toBe(0);
  expect(part.byteOffset).toBe(8);
  expect(part.length).toBe(4);
  expect(part.byteLength).toBe(4);
});
test("own keys are the element indices", () => {
  const view = new Uint8Array([4, 5, 6]) as any;
  expect(Object.keys(view)).toEqual(["0", "1", "2"]);
  expect(Object.values(view)).toEqual([4, 5, 6]);
  // `Object.hasOwn(view, '7')` is false because the view has three elements. Its
  // `byteLength`/`length` accessors are deliberately NOT asserted here: one
  // emitter serves both `k in obj` and `Object.hasOwn(obj, k)`, and `in` must
  // answer `true` for a prototype accessor (`'length' in view`). Splitting the
  // two is a pre-existing conflation, unrelated to view identity.
  expect(Object.hasOwn(view, "1")).toBe(true);
  expect(Object.hasOwn(view, "7")).toBe(false);
  expect("length" in view).toBe(true);
});
test("an element write is visible through the buffer", () => {
  const buffer = new ArrayBuffer(2);
  const view = new Uint8Array(buffer) as any;
  view[0] = 9;
  expect(view[0]).toBe(9);
  expect(view.buffer).toBe(buffer);
  expect((new Uint8Array(buffer) as any)[0]).toBe(9);
});
test("a wide element write encodes across its bytes", () => {
  const view = new Int16Array(1) as any;
  view[0] = -2;
  expect(view[0]).toBe(-2);
  const bytes = new Uint8Array(view.buffer) as any;
  expect(bytes.length).toBe(2);
  expect(bytes[0]).toBe(254);
  expect(bytes[1]).toBe(255);
});
test("subarray bounds are element indices", () => {
  const view = new Int16Array([1, 2, 3, 4]) as any;
  const part = view.subarray(1, 3);
  expect(part.length).toBe(2);
  expect(part.byteLength).toBe(4);
  expect(part[0]).toBe(2);
  expect(part[1]).toBe(3);
});
"#;
    run_fixture(source, "smelt_typed_array_views");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn views_are_cloneable_and_comparable_by_identity() {
    // The clone/equality paths a library actually walks. `new Ctor(source)` has two
    // JavaScript meanings that a shapeless byte copy cannot tell apart: over an
    // `ArrayBuffer` it re-*views* the bytes, over another view or an array it
    // *converts* the elements. Both are exercised here, together with the
    // structural equality that library clone specs compare a clone against its
    // original with.
    let source = r#"
import { test, expect } from "vitest";
test("constructing from a view converts elements, not bytes", () => {
  const wide = new Int16Array([1, 2]) as any;
  const narrow = new Uint8Array(wide) as any;
  expect(narrow.length).toBe(2);
  expect(narrow[0]).toBe(1);
  expect(narrow[1]).toBe(2);
});
test("constructing from a buffer re-views bytes", () => {
  const wide = new Int16Array([1, 2]) as any;
  const overBytes = new Uint8Array(wide.buffer) as any;
  expect(overBytes.length).toBe(4);
  expect(overBytes[0]).toBe(1);
  expect(overBytes[1]).toBe(0);
});
test("two independently built views compare equal", () => {
  const left = new Float32Array([1.5, 2.5]) as any;
  const right = new Float32Array([1.5, 2.5]) as any;
  expect(left).toEqual(right);
  expect(left).not.toBe(right);
});
test("views of different element types never compare equal", () => {
  const buffer = new ArrayBuffer(8);
  const asFloat = new Float32Array(buffer) as any;
  const asDouble = new Float64Array(buffer) as any;
  expect(Object.prototype.toString.call(asFloat)).not.toBe(
    Object.prototype.toString.call(asDouble)
  );
  expect(asFloat.length).not.toBe(asDouble.length);
});
test("instanceof resolves through the view identity", () => {
  const view: unknown = new Uint8Array([1]);
  expect(view instanceof Uint8Array).toBe(true);
  expect(view instanceof Float64Array).toBe(false);
  expect(view instanceof ArrayBuffer).toBe(false);
});
"#;
    run_fixture(source, "smelt_typed_array_clone");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn views_over_one_buffer_share_their_storage_across_subarray() {
    // The property that decides the whole shape of the concrete family
    // (increment 3): a view is a WINDOW onto shared storage, not a `Vec<u8>`
    // with a name. `subarray` hands back a second view over the SAME bytes, so
    // a write through either is visible through the other and through the
    // buffer; `slice` copies, so a write through the copy is not. A design that
    // copied bytes per view would pass every test that does not share a buffer
    // and silently diverge on every test that does — which is why this case is
    // pinned at run time rather than in a string golden.
    let source = r#"
import { test, expect } from "vitest";
test("subarray shares storage while slice copies", () => {
  const view = new Uint8Array([1, 2, 3, 4]);
  const shared = view.subarray(1, 3);
  expect(shared.length).toBe(2);
  expect(shared.byteOffset).toBe(1);
  shared[0] = 99;
  expect(view[1]).toBe(99);
  view[2] = 77;
  expect(shared[1]).toBe(77);
  const copied = view.slice(1, 3);
  copied[0] = 5;
  expect(view[1]).toBe(99);
  expect(copied.byteOffset).toBe(0);
});
test("a subarray of a subarray keeps accumulating the offset", () => {
  const view = new Uint8Array(8);
  const first = view.subarray(2);
  const second = first.subarray(3);
  expect(first.byteOffset).toBe(2);
  expect(second.byteOffset).toBe(5);
  expect(second.length).toBe(3);
  second[0] = 42;
  expect(view[5]).toBe(42);
  expect(first[3]).toBe(42);
});
test("views of different widths over one buffer alias its bytes", () => {
  const buffer = new ArrayBuffer(4);
  const bytes = new Uint8Array(buffer);
  const words = new Uint32Array(buffer);
  bytes[0] = 1;
  expect(words[0]).toBe(1);
  words[0] = 256;
  expect(bytes[0]).toBe(0);
  expect(bytes[1]).toBe(1);
  expect(bytes.buffer.byteLength).toBe(4);
});
test("a windowed view writes through to its buffer and back", () => {
  const buffer = new ArrayBuffer(8);
  const window = new Uint8Array(buffer, 2, 4);
  const whole = new Uint8Array(buffer);
  window[0] = 9;
  expect(whole[2]).toBe(9);
  whole[5] = 8;
  expect(window[3]).toBe(8);
  expect(window.byteLength).toBe(4);
  expect(window.buffer.byteLength).toBe(8);
});
"#;
    run_fixture(source, "smelt_typed_array_shared_storage");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn set_copies_elements_while_fill_answers_the_view_it_filled() {
    // The two mutating members, and the two ways they differ from the aliasing
    // ones above: `set` COPIES its source's elements in (so a later write to the
    // source is not visible in the target), and `fill` answers the SAME view it
    // filled rather than a copy — which is what makes `view.fill(0)` an
    // in-place clear and `const cleared = view.fill(0)` an alias of `view`.
    let source = r#"
import { test, expect } from "vitest";
test("set copies elements in without aliasing the source", () => {
  const source = new Uint8Array([1, 2]);
  const target = new Uint8Array(4);
  target.set(source, 1);
  source[0] = 7;
  expect(target[1]).toBe(1);
  expect(target[2]).toBe(2);
  expect(target[0]).toBe(0);
});
test("set converts per element rather than per byte", () => {
  const wide = new Uint32Array([1, 2]);
  const narrow = new Uint8Array(4);
  narrow.set(wide);
  expect(narrow[0]).toBe(1);
  expect(narrow[1]).toBe(2);
  expect(narrow[2]).toBe(0);
});
test("fill answers the same view it filled", () => {
  const view = new Uint8Array(4);
  const filled = view.fill(3, 1, 3);
  expect(filled[1]).toBe(3);
  expect(view[2]).toBe(3);
  expect(view[0]).toBe(0);
  expect(view[3]).toBe(0);
  filled[0] = 6;
  expect(view[0]).toBe(6);
});
"#;
    run_fixture(source, "smelt_typed_array_mutation");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_erased_face_is_reached_only_through_the_boundary_adapters() {
    // The other half of increment 3's claim: the erased byte-backed record is
    // now the concrete family's BOUNDARY form, so a concrete view that has
    // crossed into `SmeltUnknown` answers exactly what a record built by the
    // erased face answers — `ArrayBuffer.isView`, `instanceof`, the index keys,
    // `String()`, `JSON.stringify` — and a concrete view recovers from one.
    //
    // These cases live in the tier rather than in the examples corpus because
    // every flow here is erased by construction (an `unknown` parameter, a
    // `DataView`), and the corpus holds a hard `avoidable == 0` invariant.
    // Increment 5 of the plan folds them back.
    let source = r#"
import { test, expect } from "vitest";
function erase(value: unknown): unknown {
  return value;
}
test("an erased view answers the erased face's questions", () => {
  const buffer = new ArrayBuffer(8);
  const window = new Uint8Array(buffer, 2, 4);
  window[0] = 7;
  expect(ArrayBuffer.isView(erase(window))).toBe(true);
  expect(ArrayBuffer.isView(erase(buffer))).toBe(false);
  expect(erase(window) instanceof Uint8Array).toBe(true);
  expect(erase(buffer) instanceof ArrayBuffer).toBe(true);
  expect(JSON.stringify(erase(window))).toBe('{"0":7,"1":0,"2":0,"3":0}');
  expect(JSON.stringify(erase(buffer))).toBe("{}");
  expect(String(erase(window))).toBe("7,0,0,0");
  expect(Object.keys(erase(window) as Record<string, unknown>).join(",")).toBe("0,1,2,3");
});
test("a concrete view recovers from an erased buffer", () => {
  const buffer = new ArrayBuffer(4);
  const bytes = new Uint8Array(buffer);
  bytes[2] = 5;
  const recovered = new Uint8Array(erase(buffer) as ArrayBuffer);
  expect(recovered.length).toBe(4);
  expect(recovered[2]).toBe(5);
});
test("the byte hosts that stay erased still construct and identify", () => {
  const buffer = new ArrayBuffer(8);
  const dataView = new DataView(buffer);
  expect(dataView instanceof DataView).toBe(true);
  expect(ArrayBuffer.isView(dataView)).toBe(true);
});
test("crypto.subtle.digest takes a view or a buffer and answers storage", async () => {
  const encoded = new TextEncoder().encode("hi");
  const fromView = await crypto.subtle.digest("SHA-256", encoded);
  expect(fromView.byteLength).toBe(32);
  expect(ArrayBuffer.isView(fromView)).toBe(false);
  const fromBuffer = await crypto.subtle.digest("SHA-256", fromView);
  expect(fromBuffer.byteLength).toBe(32);
  const bytes = new Uint8Array(fromView);
  expect(bytes.length).toBe(32);
});
test("crypto.getRandomValues fills a concrete view in place", () => {
  const view = new Uint8Array(8);
  const filled = crypto.getRandomValues(view);
  expect(filled.length).toBe(8);
  expect(filled.byteLength).toBe(8);
});
"#;
    run_fixture(source, "smelt_typed_array_boundary");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_data_view_and_shared_storage_keep_their_identity_across_the_boundary() {
    // The erased half of `examples/typescript/end-to-end/82_data_view_shared_buffer`.
    //
    // The fixture asks its questions through the CONCRETE value, which needs no
    // erasure; asking the same two through an `unknown` PARAMETER — the shape
    // es-toolkit's `isTypedArray` has, and the reason it must exclude a
    // `DataView` from `ArrayBuffer.isView` — erases by construction, and the
    // examples corpus holds a hard `avoidable == 0` invariant. So the erased
    // spelling lives here, beside
    // `the_erased_face_is_reached_only_through_the_boundary_adapters`, which is
    // where `74_typed_array_views` put its own erased half.
    //
    // What is pinned: the `[object X]` tag separates a `SharedArrayBuffer` from
    // an `ArrayBuffer` and a `DataView` from a typed array; `instanceof`
    // narrowing recovers the view's byte length through the boundary adapter;
    // and a non-view argument takes the other arm rather than answering a width.
    let source = r#"
import { test, expect } from "vitest";
function tagOf(value: unknown): string {
  return Object.prototype.toString.call(value);
}
function widthOf(value: unknown): number {
  return value instanceof DataView ? value.byteLength : -1;
}
test("the erased tag separates the byte hosts from one another", () => {
  const shared = new SharedArrayBuffer(8);
  const plain = new ArrayBuffer(8);
  const view = new DataView(plain);
  expect(tagOf(shared)).toBe("[object SharedArrayBuffer]");
  expect(tagOf(plain)).toBe("[object ArrayBuffer]");
  expect(tagOf(view)).toBe("[object DataView]");
  expect(tagOf(new Uint8Array(plain))).toBe("[object Uint8Array]");
});
test("an erased DataView narrows back to its byte length", () => {
  const buffer = new ArrayBuffer(16);
  expect(widthOf(new DataView(buffer))).toBe(16);
  expect(widthOf(new DataView(buffer, 8, 4))).toBe(4);
  expect(widthOf(new DataView(new SharedArrayBuffer(8)))).toBe(8);
  expect(widthOf(buffer)).toBe(-1);
  expect(widthOf(new Uint8Array(buffer))).toBe(-1);
});
"#;
    run_fixture(source, "smelt_data_view_boundary");
}
