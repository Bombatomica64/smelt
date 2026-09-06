//! Runtime execution tests for the four ECMA-262 URI transcoding globals.
//!
//! `encodeURI`, `encodeURIComponent`, `decodeURI` and `decodeURIComponent` come
//! from two algorithms parameterized by one character set, and the pairs differ
//! *only* in whether the URI reserved separators `; / ? : @ & = + $ , #` count
//! as data:
//!
//! ```js
//! encodeURI('a/b?c')            // 'a/b?c'          — structure preserved
//! encodeURIComponent('a/b?c')   // 'a%2Fb%3Fc'      — structure escaped
//! decodeURI('a%2Fb')            // 'a%2Fb'          — stays escaped
//! decodeURIComponent('a%2Fb')   // 'a/b'
//! ```
//!
//! Only `encodeURI` was modeled; the other three were rejected outright
//! (`unresolved identifier decodeURI`) whether called or passed as a value. All
//! four now lower through one `UriTranscode` op carrying which variant it is.
//!
//! Picking the wrong variant compiles perfectly and produces a *plausible*
//! string, which is exactly the defect class only a runtime tier catches: every
//! fixture below is chosen so the two variants of a pair disagree.
//!
//! **Known gap, deliberately not asserted here.** Both decoders throw a
//! `URIError` on malformed input, and Smelt renders that as an `.expect(...)`
//! panic rather than a catchable throw — the same shape as the existing
//! `JSON.parse` emission, because a fallible stdlib *rvalue* has no throwing
//! edge in MIR. See `blocker-logs/hono-h10-uri-and-base64-globals.md`. The
//! character-set boundary itself is unit-tested in `smelt-runtime`'s
//! `uri::tests::malformed_input_is_rejected`.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test uri_transcode_runtime -- --ignored
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
        "generated URI transcoding test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-uri-transcode-runtime-{}-{seq}",
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
fn the_two_encoders_differ_by_exactly_the_reserved_separators() {
    let source = r"
import { test, expect } from 'vitest';

export const whole = (value: string): string => encodeURI(value);
export const component = (value: string): string => encodeURIComponent(value);

test('encodeURI leaves URI structure intact', () => {
  expect(whole('https://a.example/p q?x=1&y=2#f')).toBe('https://a.example/p%20q?x=1&y=2#f');
});
test('encodeURIComponent escapes the separators too', () => {
  expect(component('https://a.example/p q?x=1&y=2#f')).toBe(
    'https%3A%2F%2Fa.example%2Fp%20q%3Fx%3D1%26y%3D2%23f'
  );
});
test('both share the unreserved set', () => {
  const unreserved = `azAZ09-_.!~*'()`;
  expect(whole(unreserved)).toBe(unreserved);
  expect(component(unreserved)).toBe(unreserved);
});
test('both encode a multi-byte character per UTF-8 byte', () => {
  expect(whole('é')).toBe('%C3%A9');
  expect(component('€')).toBe('%E2%82%AC');
});
";
    run_fixture(source, "uri_encoders");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_two_decoders_differ_by_exactly_the_reserved_separators() {
    let source = r"
import { test, expect } from 'vitest';

export const whole = (value: string): string => decodeURI(value);
export const component = (value: string): string => decodeURIComponent(value);

test('decodeURI keeps an escaped separator escaped', () => {
  // Decoding `%2F` here would turn a literal slash inside a path segment into
  // a separator, which is the whole reason the two decoders exist.
  expect(whole('a%2Fb%3Fc')).toBe('a%2Fb%3Fc');
});
test('decodeURIComponent decodes every escape', () => {
  expect(component('a%2Fb%3Fc')).toBe('a/b?c');
});
test('both decode everything outside the reserved set', () => {
  expect(whole('Hello%20World')).toBe('Hello World');
  expect(component('Hello%20World')).toBe('Hello World');
});
test('both decode a multi-byte escape run as one character', () => {
  expect(whole('%C3%A9')).toBe('é');
  expect(component('%F0%9F%98%80')).toBe('😀');
});
test('unescaped text passes through untouched', () => {
  expect(whole('a/é?b')).toBe('a/é?b');
  expect(component('a/é?b')).toBe('a/é?b');
});
";
    run_fixture(source, "uri_decoders");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn each_global_round_trips_through_its_own_partner() {
    let source = r"
import { test, expect } from 'vitest';

export const wholeRound = (value: string): string => decodeURI(encodeURI(value));
export const componentRound = (value: string): string =>
  decodeURIComponent(encodeURIComponent(value));

test('encodeURI then decodeURI is the identity', () => {
  expect(wholeRound('https://a.example/p q?x=1#f')).toBe('https://a.example/p q?x=1#f');
});
test('encodeURIComponent then decodeURIComponent is the identity', () => {
  expect(componentRound('https://a.example/p q?x=1#f')).toBe('https://a.example/p q?x=1#f');
});
test('the round trip survives multi-byte text', () => {
  expect(componentRound('héllo wörld 😀')).toBe('héllo wörld 😀');
});
";
    run_fixture(source, "uri_round_trip");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn each_global_works_as_a_first_class_value() {
    // Hono's `utils/url.ts` writes `tryDecode(str, decodeURI)` and
    // `export const decodeURIComponent_ = decodeURIComponent` (with the comment
    // "`decodeURIComponent` is a long name"). Both are value positions: the
    // first passes the global to a higher-order function, the second aliases it
    // as a module const. Neither lowered before, and both must run the SAME
    // character-set rule as the direct call — passing the value form through a
    // generic erased callable would silently pick the wrong variant.
    let source = r"
import { test, expect } from 'vitest';

type Coder = (value: string) => string;

const apply = (value: string, coder: Coder): string => coder(value);

export const decodeURIComponent_ = decodeURIComponent;
export const encodeURIComponent_ = encodeURIComponent;

test('a global passed as a value keeps its own character set', () => {
  expect(apply('a%2Fb', decodeURI)).toBe('a%2Fb');
  expect(apply('a%2Fb', decodeURIComponent)).toBe('a/b');
  expect(apply('a/b', encodeURI)).toBe('a/b');
  expect(apply('a/b', encodeURIComponent)).toBe('a%2Fb');
});
test('a module const aliasing a global keeps its character set', () => {
  expect(decodeURIComponent_('a%2Fb')).toBe('a/b');
  expect(encodeURIComponent_('a/b')).toBe('a%2Fb');
});
test('an aliased global also works through a higher-order call', () => {
  expect(apply('a%2Fb', decodeURIComponent_)).toBe('a/b');
});
test('mapping a global over a list applies it per element', () => {
  expect(['a b', 'c/d'].map(encodeURIComponent)).toEqual(['a%20b', 'c%2Fd']);
});
";
    run_fixture(source, "uri_value_forms");
}
