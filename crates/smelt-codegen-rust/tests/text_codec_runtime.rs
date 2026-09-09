//! Runtime execution tests for the WHATWG text codecs.
//!
//! `TextEncoder`/`TextDecoder` and the byte view the encoder answers are
//! modeled as concrete generated Rust types, and the value of that choice is
//! behavioural in ways no golden or compile-only corpus can observe:
//!
//! 1. **`encode` counts UTF-8 BYTES, not UTF-16 code units.** `"héllo"` is five
//!    JavaScript characters and six bytes, and `"😀"` is one character, two
//!    UTF-16 code units, and four bytes. A model that answered `String::len`
//!    over chars, or that erased the result into a plain list of code units,
//!    gets both wrong.
//! 2. **`length` and `byteLength` agree for a byte view** and are both the byte
//!    count — the one place the two spec members coincide.
//! 3. **The round trip is lossless for any string**, including astral-plane
//!    characters, because both directions are UTF-8.
//! 4. **`decode` substitutes U+FFFD for ill-formed input** rather than throwing
//!    or truncating: that is the spec's non-`fatal` behaviour, and it is
//!    observable only by handing the decoder bytes that are not valid UTF-8
//!    (here through a `Uint8Array`, which is also the corpora's commonest
//!    spelling and exercises the byte-view boundary adapter).
//! 5. **`encoding` is the canonical label**, so `new TextDecoder("UTF8")`
//!    reports `"utf-8"` — the encoding standard's label table is a lookup, not
//!    a passthrough.
//! 6. **A byte view is a reference value with structural equality**: two views
//!    over the same bytes are `toEqual`, and a view keeps its bytes when it
//!    crosses an `unknown` boundary and comes back — the failure mode a
//!    shared-cell type has under the generic struct-erasure path, where the
//!    declared fields are empty and the value round-trips as nothing.
//! 7. **An erased byte view is the same value as an erased `Uint8Array`**, so
//!    `instanceof Uint8Array` and the `[object Uint8Array]` tag hold on the
//!    concrete view once it is erased. This is what keeps the concrete type and
//!    the byte-backed host record the typed-array views still use from being
//!    two different kinds of byte array to a consumer.
//!
//! Each case is a TypeScript Vitest test; lowering emits a `#[test]`, and this
//! tier emits the crate and runs `cargo test` on it, so a green run means the
//! generated `expect(...)` calls held at runtime. The tier is `#[ignore]`d
//! because it compiles and executes a real crate. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test text_codec_runtime -- --ignored
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
        "generated text-codec test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-text-codec-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
fn run_codec_fixture(source: &str, crate_name: &str) {
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
fn encode_counts_utf8_bytes() {
    let source = r#"
import { test, expect } from "vitest";
test("encode answers the UTF-8 byte length, not the character count", () => {
  const encoder = new TextEncoder();
  expect(encoder.encode("hello").length).toBe(5);
  expect(encoder.encode("héllo").length).toBe(6);
  expect(encoder.encode("日本語").length).toBe(9);
  expect(encoder.encode("").length).toBe(0);
});
test("an astral-plane character is four bytes and one string iteration step", () => {
  const bytes = new TextEncoder().encode("😀");
  expect(bytes.length).toBe(4);
  expect(bytes.byteLength).toBe(4);
});
test("length and byteLength agree for a byte view", () => {
  const bytes = new TextEncoder().encode("abc");
  expect(bytes.length).toBe(3);
  expect(bytes.byteLength).toBe(3);
});
"#;
    run_codec_fixture(source, "encode_byte_counts");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_round_trip_is_lossless() {
    let source = r#"
import { test, expect } from "vitest";
test("every string survives an encode/decode round trip", () => {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  expect(decoder.decode(encoder.encode("plain ascii"))).toBe("plain ascii");
  expect(decoder.decode(encoder.encode("héllo wörld"))).toBe("héllo wörld");
  expect(decoder.decode(encoder.encode("日本語のテキスト"))).toBe("日本語のテキスト");
  expect(decoder.decode(encoder.encode("😀🎉"))).toBe("😀🎉");
  expect(decoder.decode(encoder.encode(""))).toBe("");
});
test("one decoder reads many views", () => {
  const decoder = new TextDecoder();
  const encoder = new TextEncoder();
  expect(decoder.decode(encoder.encode("one"))).toBe("one");
  expect(decoder.decode(encoder.encode("two"))).toBe("two");
});
"#;
    run_codec_fixture(source, "codec_round_trip");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn decode_substitutes_the_replacement_character() {
    let source = r#"
import { test, expect } from "vitest";
test("ill-formed UTF-8 becomes U+FFFD rather than throwing", () => {
  const decoder = new TextDecoder();
  // 0xFF is not a valid UTF-8 lead byte anywhere.
  const broken = new Uint8Array([104, 105, 255]);
  expect(decoder.decode(broken)).toBe("hi�");
});
test("a truncated multi-byte sequence becomes one replacement character", () => {
  const decoder = new TextDecoder();
  // 0xE6 opens a three-byte sequence that never finishes.
  const truncated = new Uint8Array([230]);
  expect(decoder.decode(truncated)).toBe("�");
});
test("a Uint8Array of valid bytes decodes like an encoded view", () => {
  const decoder = new TextDecoder();
  const bytes = new Uint8Array([104, 195, 169, 108, 108, 111]);
  expect(decoder.decode(bytes)).toBe("héllo");
});
"#;
    run_codec_fixture(source, "decode_replacement");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn encoding_labels_are_canonicalized() {
    let source = r#"
import { test, expect } from "vitest";
test("an encoder always reports utf-8", () => {
  expect(new TextEncoder().encoding).toBe("utf-8");
});
test("a decoder reports the canonical label, not the spelling it was given", () => {
  expect(new TextDecoder().encoding).toBe("utf-8");
  expect(new TextDecoder("utf-8").encoding).toBe("utf-8");
  expect(new TextDecoder("UTF8").encoding).toBe("utf-8");
  expect(new TextDecoder("unicode-1-1-utf-8").encoding).toBe("utf-8");
});
"#;
    run_codec_fixture(source, "codec_encoding_labels");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_byte_view_is_a_reference_value_that_survives_erasure() {
    let source = r#"
import { test, expect } from "vitest";
test("two views over the same bytes are structurally equal", () => {
  const encoder = new TextEncoder();
  expect(encoder.encode("abc")).toEqual(encoder.encode("abc"));
});
test("a view keeps its bytes across an unknown boundary", () => {
  const bytes = new TextEncoder().encode("héllo");
  const erased: unknown = bytes;
  const record = erased as { byteLength: number; length: number };
  expect(record.byteLength).toBe(6);
  expect(record.length).toBe(6);
});
test("an erased view is a Uint8Array, exactly as a constructed one is", () => {
  const bytes = new TextEncoder().encode("abc");
  const erased: unknown = bytes;
  expect(erased instanceof Uint8Array).toBe(true);
  expect(Object.prototype.toString.call(erased)).toBe("[object Uint8Array]");
});
"#;
    run_codec_fixture(source, "byte_view_identity");
}
