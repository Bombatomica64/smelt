//! Runtime execution tests for catchable `URIError` from the URI decoders.
//!
//! `decodeURI` / `decodeURIComponent` throw a catchable `URIError` on malformed
//! percent-encoding. They used to emit
//! `smelt_decode_uri(..).expect("URIError: URI malformed")`, which does not
//! merely fail to be catchable: a fallible *rvalue* has no unwind edge in MIR,
//! so the `catch` block ends up with no predecessor, MIR drops it, and the
//! fallback the source wrote is **absent from the generated crate**. They now
//! lower to `Terminator::Call { Callee::Builtin(BuiltinFn::UriDecode(op)),
//! unwind }`, the same shape `JSON.parse` has always used.
//!
//! These have to execute. A type-level test cannot tell a dropped handler from
//! a live one — both type-check — so the assertion is that a malformed input
//! returns the fallback *value*.
//!
//! **Known gap, deliberately not covered here:** a decoder reached through a
//! callback VALUE (`tryDecode(str, decodeURIComponent)`, Hono's real shape)
//! still aborts. Marking the closure `may_throw` makes the value incompatible
//! with its declared parameter type and breaks compilation instead; see
//! `blocker-logs/hono-fallible-ops.md` §8.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test uri_decode_throw_runtime -- --ignored
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
        .env("CARGO_INCREMENTAL", "0")
        .env("RUSTFLAGS", "-Awarnings")
        .output()
        .expect("spawn cargo test");
    assert!(
        output.status.success(),
        "generated URI-decode test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-uri-decode-throw-runtime-{}-{seq}",
        std::process::id()
    ))
}

/// Emit `source` as a crate and run its generated tests.
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
fn a_malformed_decode_uri_component_takes_the_catch_fallback() {
    let source = r"
import { test, expect } from 'vitest';

function decodeOrFallback(value: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    return value;
  }
}

test('a well-formed input decodes', () => {
  expect(decodeOrFallback('a%20b')).toBe('a b');
});
test('a malformed input takes the fallback instead of aborting', () => {
  expect(decodeOrFallback('%E0%A4%A')).toBe('%E0%A4%A');
});
test('a lone percent takes the fallback too', () => {
  expect(decodeOrFallback('%')).toBe('%');
});
";
    run_fixture(source, "uri_decode_component_throw");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_malformed_decode_uri_takes_the_catch_fallback() {
    // `decodeURI` is a separate `UriTranscodeOp`, so it gets its own adapter and
    // its own arm in the emitter; only a fixture per op catches one of them
    // regressing alone.
    let source = r"
import { test, expect } from 'vitest';

function decodeOrFallback(value: string): string {
  try {
    return decodeURI(value);
  } catch {
    return value;
  }
}

test('a well-formed input decodes', () => {
  expect(decodeOrFallback('a%20b')).toBe('a b');
});
test('a malformed input takes the fallback instead of aborting', () => {
  expect(decodeOrFallback('%')).toBe('%');
});
";
    run_fixture(source, "uri_decode_throw");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_thrown_uri_error_is_a_real_catchable_error_value() {
    // The payload has to be the same `URIError` record a hand-written
    // `throw new URIError(..)` builds, or a `catch (error)` binding could tell
    // a runtime-raised error from a source-level one.
    let source = r"
import { test, expect } from 'vitest';

function decodeAndName(value: string): string {
  try {
    decodeURIComponent(value);
    return 'no throw';
  } catch (error) {
    return (error as Error).name;
  }
}

test('the caught value is a URIError', () => {
  expect(decodeAndName('%E0%A4%A')).toBe('URIError');
});
test('a well-formed input does not throw', () => {
  expect(decodeAndName('a%20b')).toBe('no throw');
});
";
    run_fixture(source, "uri_decode_error_identity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn encoding_stays_infallible_and_needs_no_handler() {
    // The negative half: `encodeURI`/`encodeURIComponent` are infallible for
    // well-formed UTF-16 input and keep the cheaper `Rvalue::UriTranscode`
    // form. Without this a later change could quietly route every transcode
    // through the throwing terminator and nothing would notice.
    let source = r"
import { test, expect } from 'vitest';

function encodeBoth(value: string): string {
  return encodeURI(value) + '|' + encodeURIComponent(value);
}

test('encoding runs with no handler in sight', () => {
  expect(encodeBoth('a b')).toBe('a%20b|a%20b');
});
";
    run_fixture(source, "uri_encode_infallible");
}
