//! Runtime execution tests for the WHATWG `Blob`/`File` value.
//!
//! `Blob` and `File` were marker-bearing records whose content was stored as a
//! `String`; they are now the byte-backed concrete `SmeltBlob`. Every case
//! below is behaviour that representation change makes observable and that no
//! golden or compile-only corpus can see:
//!
//! 1. **`size` is a BYTE count**, so `"héllo 😀"` is eleven and not seven, and
//!    a blob built from several parts is the sum of their byte lengths.
//! 2. **`slice` follows the spec**: a negative index counts from the end, an
//!    out-of-range one clamps, an inverted range is an EMPTY blob rather than
//!    an error, the content type comes from `slice`'s own third argument and
//!    never from the source, and the result is a `Blob` even when the source is
//!    a `File` — so `blob.slice(..) instanceof File` is false.
//! 3. **A `File` is a `Blob`**: both `instanceof` tests hold on a file and only
//!    the first holds on a blob. The two share one Rust type, so this is the
//!    check that the optional-name modeling did not collapse the distinction.
//! 4. **The body readers round-trip bytes losslessly**, including bytes that are
//!    not valid UTF-8 — which the old `String`-backed record could not hold at
//!    all — and `arrayBuffer()`/`bytes()` answer the concrete byte view.
//! 5. **A blob part contributes BYTES.** A nested blob contributes its content,
//!    a string its UTF-8 bytes, and a `Uint8Array` its raw bytes: the last one
//!    used to be stringified, so a binary part was corrupted.
//! 6. **An erased blob is the record every existing consumer reads.** The
//!    `clone`/`cloneDeepWith` idiom — `value instanceof Blob` then
//!    `new Blob([value], { type: value.type })` over an `unknown` — has to keep
//!    working on the erased form, and `type` has to read back off it. (`size`
//!    is not asserted through a cast: see
//!    `blocker-logs/standards-erased-cast-size-collision.md`.)
//!
//! Each case is a TypeScript Vitest test; lowering emits a `#[test]`, and this
//! tier emits the crate and runs `cargo test` on it, so a green run means the
//! generated `expect(...)` calls held at runtime. The tier is `#[ignore]`d
//! because it compiles and executes a real crate. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test blob_runtime -- --ignored
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
        "generated blob test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-blob-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_blob_fixture(source: &str, crate_name: &str) {
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
fn size_is_a_byte_count() {
    let source = r#"
import { test, expect } from "vitest";
test("size counts bytes, not characters", () => {
  expect(new Blob(["hello"]).size).toBe(5);
  expect(new Blob(["héllo"]).size).toBe(6);
  expect(new Blob(["héllo 😀"]).size).toBe(11);
  expect(new Blob([]).size).toBe(0);
});
test("several parts sum their byte lengths", () => {
  expect(new Blob(["a", "bc", "déf"]).size).toBe(7);
});
test("type is the empty string when no options were supplied", () => {
  expect(new Blob(["a"]).type).toBe("");
  expect(new Blob(["a"], { type: "text/plain" }).type).toBe("text/plain");
});
"#;
    run_blob_fixture(source, "blob_size");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn slice_follows_the_spec() {
    let source = r#"
import { test, expect } from "vitest";
test("a negative index counts from the end and an out-of-range one clamps", () => {
  const blob = new Blob(["hello, world"], { type: "text/plain" });
  expect(blob.slice(0, 5).size).toBe(5);
  expect(blob.slice(-5).size).toBe(5);
  expect(blob.slice(7).size).toBe(5);
  expect(blob.slice(0, 99).size).toBe(12);
  expect(blob.slice(-99).size).toBe(12);
});
test("an inverted range is an empty blob", () => {
  const blob = new Blob(["hello, world"]);
  expect(blob.slice(9, 2).size).toBe(0);
  expect(blob.slice(5, 5).size).toBe(0);
});
test("the content type comes from slice, never from the source", () => {
  const blob = new Blob(["hello"], { type: "text/plain" });
  expect(blob.slice(0, 2).type).toBe("");
  expect(blob.slice(0, 2, "text/html").type).toBe("text/html");
});
test("slicing a File answers a Blob", () => {
  const file = new File(["report"], "report.csv", { type: "text/csv" });
  const part = file.slice(0, 3);
  expect(part instanceof Blob).toBe(true);
  expect(part instanceof File).toBe(false);
});
"#;
    run_blob_fixture(source, "blob_slice");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_file_is_a_blob() {
    let source = r#"
import { test, expect } from "vitest";
test("both instanceof tests hold on a file and only one on a blob", () => {
  const blob = new Blob(["a"]);
  const file = new File(["a"], "a.txt");
  expect(blob instanceof Blob).toBe(true);
  expect(blob instanceof File).toBe(false);
  expect(file instanceof Blob).toBe(true);
  expect(file instanceof File).toBe(true);
});
test("a file carries the two extra data properties", () => {
  const file = new File(["report"], "report.csv", { type: "text/csv", lastModified: 42 });
  expect(file.name).toBe("report.csv");
  expect(file.lastModified).toBe(42);
  expect(file.type).toBe("text/csv");
  expect(file.size).toBe(6);
});
test("lastModified defaults to 0 rather than the wall clock", () => {
  expect(new File(["a"], "a.txt").lastModified).toBe(0);
});
test("a file does not equal a same-bytes blob", () => {
  expect(new File(["a"], "a.txt")).not.toEqual(new Blob(["a"]));
  expect(new Blob(["a"])).toEqual(new Blob(["a"]));
});
"#;
    run_blob_fixture(source, "blob_file_identity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_body_readers_round_trip_bytes() {
    let source = r#"
import { test, expect } from "vitest";
test("text decodes the bytes", async () => {
  expect(await new Blob(["hello, world"]).text()).toBe("hello, world");
  expect(await new Blob(["héllo 😀"]).text()).toBe("héllo 😀");
  expect(await new Blob([]).text()).toBe("");
});
test("arrayBuffer and bytes answer the byte view", async () => {
  const blob = new Blob(["héllo"]);
  expect((await blob.arrayBuffer()).byteLength).toBe(6);
  expect((await blob.bytes()).length).toBe(6);
});
test("bytes that are not valid UTF-8 survive the round trip", async () => {
  // 0xFF is not a valid UTF-8 lead byte; the old String-backed record could
  // not hold it at all.
  const blob = new Blob([new Uint8Array([104, 105, 255])]);
  expect(blob.size).toBe(3);
  const bytes = await blob.bytes();
  expect(bytes.length).toBe(3);
});
test("a sliced blob reads back only its range", async () => {
  const blob = new Blob(["hello, world"]);
  expect(await blob.slice(0, 5).text()).toBe("hello");
  expect(await blob.slice(-5).text()).toBe("world");
});
"#;
    run_blob_fixture(source, "blob_body_readers");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn blob_parts_contribute_bytes() {
    let source = r#"
import { test, expect } from "vitest";
test("a nested blob contributes its bytes", async () => {
  const inner = new Blob(["hello, world"]);
  const outer = new Blob([inner, "!"]);
  expect(outer.size).toBe(13);
  expect(await outer.text()).toBe("hello, world!");
});
test("a Uint8Array part contributes its raw bytes", async () => {
  const bytes = new TextEncoder().encode("héllo");
  const blob = new Blob([new Uint8Array([104, 105]), " "]);
  expect(blob.size).toBe(3);
  expect(await blob.text()).toBe("hi ");
  expect(new Blob([bytes]).size).toBe(6);
});
test("a nested File contributes its bytes and not its metadata", async () => {
  const file = new File(["report"], "report.csv");
  const wrapped = new Blob([file]);
  expect(wrapped.size).toBe(6);
  expect(wrapped instanceof File).toBe(false);
  expect(await wrapped.text()).toBe("report");
});
"#;
    run_blob_fixture(source, "blob_parts");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_erased_blob_keeps_the_record_its_consumers_read() {
    let source = r#"
import { test, expect } from "vitest";
test("an erased blob is still a Blob and still reports its type", () => {
  const blob = new Blob(["hello"], { type: "text/plain" });
  const erased: unknown = blob;
  expect(erased instanceof Blob).toBe(true);
  // `size` is deliberately NOT read here: an interface literal carrying a
  // `size` field lowers to the same Dict a `Map` does, so `.size` on the cast
  // value is claimed by the map-size path. That collision is recorded in
  // `blocker-logs/standards-erased-cast-size-collision.md` and is unrelated to
  // how a blob erases — the record does carry `size`.
  const record = erased as { type: string };
  expect(record.type).toBe("text/plain");
});
test("an erased file keeps both identities and its metadata", () => {
  const file = new File(["report"], "report.csv", { type: "text/csv", lastModified: 7 });
  const erased: unknown = file;
  expect(erased instanceof Blob).toBe(true);
  expect(erased instanceof File).toBe(true);
  const record = erased as { name: string; lastModified: number };
  expect(record.name).toBe("report.csv");
  expect(record.lastModified).toBe(7);
});
test("the clone idiom over an unknown rebuilds an equal blob", async () => {
  const source: unknown = new Blob(["hello"], { type: "text/plain" });
  const cloned = source instanceof Blob
    ? new Blob([source], { type: (source as { type: string }).type })
    : new Blob([]);
  expect(cloned.size).toBe(5);
  expect(cloned.type).toBe("text/plain");
  expect(await cloned.text()).toBe("hello");
});
"#;
    run_blob_fixture(source, "blob_erased");
}
