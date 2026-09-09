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
//! A decoder reached through a callback VALUE (`tryDecode(str, decodeURI)`,
//! Hono's real shape) takes a second route: the argument closure's own type says
//! `may_throw: false`, so it reports the throw by panicking and the enclosing
//! `try` catches it with `catch_unwind`. That route used to panic with
//! `format!("{}", error)`, losing the error's class, so a `catch` binding
//! observed a bare `Error` where JavaScript gives a `URIError`. It now carries a
//! `Send` class-plus-message payload (`thrown::emit_panic_route_support`), which
//! is what `the_callback_value_route_keeps_the_uri_error_identity` pins --
//! alongside the direct route's assertion, because the two routes recover the
//! payload through different code. See `blocker-logs/hono-fallible-ops.md` §9.
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

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_callback_value_route_keeps_the_uri_error_identity() {
    // Hono's `tryDecode` shape. The decoder arrives as a callback VALUE, so the
    // adapter closure Smelt builds for it is typed `(value: string) => string`
    // -- `may_throw: false` -- and cannot propagate a `Result`. It therefore
    // panics, and `tryDecode`'s own `try` catches the panic. The class has to
    // survive that unwind: `panic_any` needs `Send` and a `SmeltUnknown` holds
    // `Rc`, so the payload is the class brand plus the message rather than the
    // thrown value itself.
    let source = r"
import { test, expect } from 'vitest';

const tryDecode = (value: string, decoder: (value: string) => string): string => {
  try {
    return decoder(value);
  } catch (error) {
    return (error as Error).name;
  }
};

const viaValue = (value: string): string => tryDecode(value, decodeURIComponent);
const viaValueUri = (value: string): string => tryDecode(value, decodeURI);

test('the callback-value route reports the URIError class', () => {
  expect(viaValue('%E0%A4%A')).toBe('URIError');
});
test('decodeURI through the same parameter reports it too', () => {
  expect(viaValueUri('%')).toBe('URIError');
});
test('a well-formed input still decodes through the callback', () => {
  expect(viaValue('a%20b')).toBe('a b');
});
";
    run_fixture(source, "uri_decode_callback_identity");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_panic_route_keeps_a_user_error_class_identity() {
    // The fix is general, not per-builtin: any throw routed through a panic
    // keeps its class. A user error class reports through its `name` property,
    // which is what JavaScript reports for `error.name` on a subclass of
    // `Error`, so the projection reads that when no `__smelt_error` brand is
    // present.
    let source = r"
import { test, expect } from 'vitest';

const apply = (value: string, callback: (value: string) => string): string => {
  try {
    return callback(value);
  } catch (error) {
    return (error as Error).name + '/' + (error as Error).message;
  }
};

const throwTypeError = (value: string): string => {
  throw new TypeError('bad ' + value);
};

test('a TypeError thrown through a non-throwing callback keeps its class', () => {
  expect(apply('x', throwTypeError)).toBe('TypeError/bad x');
});
";
    run_fixture(source, "panic_route_user_class_identity");
}
