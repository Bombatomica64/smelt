//! Runtime execution tests for the `WebCrypto` members Smelt models.
//!
//! Two of the three members had no implementation at all, and the third had
//! something worse: `crypto.getRandomValues(view)` type-checked, reported no
//! blocker, and answered its own ARGUMENT unchanged — so a program that asked
//! for random bytes got a view full of zeros and no way to tell. Every case
//! below is behaviour only a run can see, and each expectation was diffed
//! against Node 22 before being written down:
//!
//! 1. **`randomUUID` answers a real v4 UUID**: 36 characters, dashes at the
//!    four fixed offsets, version nibble `4`, variant nibble one of 8/9/a/b,
//!    lowercase — and two calls differ, which is the half a constant would
//!    pass every other assertion while failing.
//! 2. **`getRandomValues` FILLS the view**, in place. The test reads the bytes
//!    back through the caller's own reference and through the returned one, and
//!    checks that a 32-byte fill is not all zeros — the exact shape the previous
//!    no-op lowering produced.
//! 3. **It answers the SAME object**, so `getRandomValues(view) === view`, and
//!    a zero-length view stays legal and empty.
//! 4. **`subtle.digest` matches the published vectors** for SHA-1, SHA-256,
//!    SHA-384 and SHA-512 — of `"abc"` and of the empty input — so a wrong
//!    algorithm choice or a truncated digest cannot pass.
//! 5. **The algorithm name is case-insensitive** (`"sha-256"` is `"SHA-256"`)
//!    but not hyphen-insensitive: Node rejects `"SHA256"` as readily as
//!    `"MD5"`, and both are the spec's catchable `NotSupportedError`.
//! 6. **Each digest is a buffer** of the algorithm's own width, and hashing the
//!    same bytes twice answers equal contents.
//!
//! One property is deliberately NOT asserted here: that two digests of the same
//! bytes are two distinct OBJECTS (`first === second` is `false` in Node).
//! `===` between two values of a modeled runtime class currently compares them
//! structurally rather than by the JS reference id the runtime type carries, so
//! it answers `true` — a reference-identity gap shared by every one of those
//! classes (`Blob`, `Headers`, `FormData`, the byte view), not something
//! `WebCrypto` does. `reference_identity_text` in `emitter::binary_ops` is where
//! it would be fixed, and it needs its own change: it would alter `===` for the
//! whole family at once.
//!
//! Each case is a TypeScript Vitest test; lowering emits a `#[test]`, and this
//! tier emits the crate and runs `cargo test` on it, so a green run means the
//! generated `expect(...)` calls held at runtime. The tier is `#[ignore]`d
//! because it compiles and executes a real crate. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test web_crypto_runtime -- --ignored
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

/// The source helper every digest case shares.
///
/// `subtle.digest` answers an `ArrayBuffer`, which is not indexable, so a
/// program that wants the bytes wraps it in a view first — exactly as the
/// end-to-end fixture does. Keeping the helper in one string keeps each case
/// below to the assertion it is actually about.
const HEX_HELPER: &str = r#"
function hex(digest: ArrayBuffer): string {
  const view = new Uint8Array(digest);
  let out = "";
  for (let index = 0; index < view.length; index = index + 1) {
    out = out + Number(view[index]).toString(16).padStart(2, "0");
  }
  return out;
}
"#;

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
        "generated WebCrypto test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-web-crypto-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a crate and run its generated Vitest tests.
///
/// The scratch root is removed whether the run passed or failed, because it
/// holds a whole cargo target directory: leaving it behind on failure filled
/// `/tmp` during this tier's own development. `SMELT_KEEP_RUNTIME_SCRATCH=1`
/// keeps it for the debugging session that actually wants it, and the panic
/// message names the directory so it can be found.
fn run_web_crypto_fixture(source: &str, crate_name: &str) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    let outcome = std::panic::catch_unwind(|| {
        emit_program(source, crate_name, &crate_dir);
        run_generated_tests(&crate_dir, &target_dir);
    });
    if std::env::var_os("SMELT_KEEP_RUNTIME_SCRATCH").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn random_uuid_is_a_fresh_v4_uuid() {
    let source = r#"
import { test, expect } from "vitest";
test("the shape is the spec's hyphenated v4 form", () => {
  const id = crypto.randomUUID();
  expect(id.length).toBe(36);
  expect(id[8]).toBe("-");
  expect(id[13]).toBe("-");
  expect(id[18]).toBe("-");
  expect(id[23]).toBe("-");
  expect(id[14]).toBe("4");
  expect("89ab".includes(id[19])).toBe(true);
  expect(id.toLowerCase()).toBe(id);
});
test("two calls answer different values", () => {
  const seen = new Set<string>();
  for (let index = 0; index < 16; index = index + 1) {
    seen.add(crypto.randomUUID());
  }
  expect(seen.size).toBe(16);
});
"#;
    run_web_crypto_fixture(source, "web_crypto_uuid");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn get_random_values_fills_the_view_in_place() {
    let source = r#"
import { test, expect } from "vitest";
test("the caller's own reference sees the bytes", () => {
  const view = new Uint8Array(32);
  crypto.getRandomValues(view);
  let sum = 0;
  for (let index = 0; index < 32; index = index + 1) {
    sum = sum + Number(view[index]);
  }
  // A no-op fill leaves every byte zero. 32 random bytes summing to exactly
  // zero has probability 256**-32, so this is the assertion that the previous
  // lowering -- which answered the argument unchanged -- could never pass.
  expect(sum > 0).toBe(true);
});
test("the returned value is the same object", () => {
  const view = new Uint8Array(8);
  const filled = crypto.getRandomValues(view);
  expect(filled === view).toBe(true);
  expect(filled.length).toBe(8);
});
test("two fills of two views differ", () => {
  const left = new Uint8Array(32);
  const right = new Uint8Array(32);
  crypto.getRandomValues(left);
  crypto.getRandomValues(right);
  let same = 0;
  for (let index = 0; index < 32; index = index + 1) {
    if (Number(left[index]) === Number(right[index])) {
      same = same + 1;
    }
  }
  expect(same < 32).toBe(true);
});
test("a zero-length view stays legal and empty", () => {
  const view = new Uint8Array(0);
  expect(crypto.getRandomValues(view).length).toBe(0);
});
"#;
    run_web_crypto_fixture(source, "web_crypto_random_values");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn digest_matches_the_published_vectors() {
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HEX_HELPER}
test("the four widths hash \"abc\" to their published vectors", async () => {{
  const message = new TextEncoder().encode("abc");
  expect(hex(await crypto.subtle.digest("SHA-1", message)))
    .toBe("a9993e364706816aba3e25717850c26c9cd0d89d");
  expect(hex(await crypto.subtle.digest("SHA-256", message)))
    .toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
  expect(hex(await crypto.subtle.digest("SHA-384", message)))
    .toBe("cb00753f45a35e8bb5a03d699ac65007272c32ab0eded1631a8b605a43ff5bed8086072ba1e7cc2358baeca134c825a7");
  expect(hex(await crypto.subtle.digest("SHA-512", message)))
    .toBe("ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f");
}});
test("the empty input has its own published vectors", async () => {{
  const empty = new TextEncoder().encode("");
  expect(hex(await crypto.subtle.digest("SHA-1", empty)))
    .toBe("da39a3ee5e6b4b0d3255bfef95601890afd80709");
  expect(hex(await crypto.subtle.digest("SHA-256", empty)))
    .toBe("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}});
test("each digest is a fresh buffer of the algorithm's own width", async () => {{
  const message = new TextEncoder().encode("abc");
  const first = await crypto.subtle.digest("SHA-256", message);
  const second = await crypto.subtle.digest("SHA-256", message);
  expect(first.byteLength).toBe(32);
  expect(second.byteLength).toBe(32);
  expect(hex(first)).toBe(hex(second));
  expect((await crypto.subtle.digest("SHA-512", message)).byteLength).toBe(64);
}});
"#
    );
    run_web_crypto_fixture(&source, "web_crypto_digest_vectors");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_algorithm_name_is_case_insensitive_but_not_hyphen_insensitive() {
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HEX_HELPER}
test("case does not matter", async () => {{
  const message = new TextEncoder().encode("abc");
  const expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
  expect(hex(await crypto.subtle.digest("sha-256", message))).toBe(expected);
  expect(hex(await crypto.subtle.digest("Sha-256", message))).toBe(expected);
  expect(hex(await crypto.subtle.digest("SHA-256", message))).toBe(expected);
}});
test("a name the spec does not list is a catchable NotSupportedError", async () => {{
  const message = new TextEncoder().encode("abc");
  let name = "no throw";
  try {{
    await crypto.subtle.digest("MD5", message);
  }} catch (error) {{
    name = (error as Error).name;
  }}
  expect(name).toBe("NotSupportedError");
}});
test("the hyphen is part of the name, so SHA256 is not SHA-256", async () => {{
  const message = new TextEncoder().encode("abc");
  let name = "no throw";
  try {{
    await crypto.subtle.digest("SHA256", message);
  }} catch (error) {{
    name = (error as Error).name;
  }}
  expect(name).toBe("NotSupportedError");
}});
"#
    );
    run_web_crypto_fixture(&source, "web_crypto_digest_names");
}
