//! Runtime execution tests for the WHATWG `Request`.
//!
//! `Request` is a concrete generated Rust type (`SmeltRequest`) holding the same
//! `SmeltBody` a `Response` holds, so the two share the single-use body and the
//! `Content-Type` a string body implies, and differ only where the spec differs
//! (a url and a method instead of a status line).
//!
//! The behaviour worth a *runtime* tier is what compiles either way:
//!
//! * `url` reads back the WHATWG **serialization**, not the input string:
//!   `new Request('https://a.test').url` is `https://a.test/`. Storing the
//!   input verbatim gives a plausible url that is missing a path.
//! * `method` is normalized for exactly the spec's list
//!   (`DELETE GET HEAD OPTIONS POST PUT`) and left alone otherwise, so `post`
//!   becomes `POST` while `patch` stays `patch`. Upper-casing everything is the
//!   easy wrong answer, and Node keeps `patch` lower-case.
//! * the default method is `GET`.
//! * the body is single-use and tees on `clone()`, exactly as a response's is.
//!
//! Every expectation below was diffed against Node 22 on the same source.
//!
//! **Known gaps, deliberately not asserted here.** Node's constructor throws a
//! `TypeError` for a relative url (`new Request('/p')`) and for a `GET`/`HEAD`
//! with a body; Smelt accepts both, because a constructor is a stdlib *rvalue*
//! and a fallible rvalue has no throwing edge in MIR to reach an enclosing
//! `try` — the same shape as `JSON.parse`, the URI decoders, and the
//! `Response` status range. An unparseable url is kept verbatim rather than
//! silently rewritten, so the value a program reads is at least what it wrote.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test request_runtime -- --ignored
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
        "generated Request test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-request-runtime-{}-{seq}", std::process::id()))
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
fn url_reads_back_the_whatwg_serialization() {
    let source = r#"
import { test, expect } from 'vitest';

test('a bare origin gains its root path', () => {
  expect(new Request('https://a.test').url).toBe('https://a.test/');
});

test('a path, query and fragment survive intact', () => {
  expect(new Request('https://a.test/p?q=1#f').url).toBe('https://a.test/p?q=1#f');
});

test('url is a string, so string methods apply to it', () => {
  // `blocker-logs/hono-fetch-demand.md` item 6: `request.url.indexOf(':')` was
  // rejected because the read had no type. This is that call.
  const request = new Request('https://a.test/p');
  expect(request.url.indexOf(':')).toBe(5);
  expect(request.url.startsWith('https://')).toBe(true);
});
"#;
    run_fixture(source, "request_url_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_method_is_normalized_for_exactly_the_specs_list() {
    let source = r#"
import { test, expect } from 'vitest';

test('the default method is GET', () => {
  expect(new Request('https://a.test/p').method).toBe('GET');
});

test('the spec list is upper-cased whatever the case written', () => {
  expect(new Request('https://a.test/p', { method: 'post' }).method).toBe('POST');
  expect(new Request('https://a.test/p', { method: 'get' }).method).toBe('GET');
  expect(new Request('https://a.test/p', { method: 'delete' }).method).toBe('DELETE');
  expect(new Request('https://a.test/p', { method: 'PUT' }).method).toBe('PUT');
});

test('a method outside the list keeps its written case', () => {
  // `patch` is NOT in the spec's normalize list, and Node keeps it lower-case.
  expect(new Request('https://a.test/p', { method: 'patch' }).method).toBe('patch');
  expect(new Request('https://a.test/p', { method: 'weird' }).method).toBe('weird');
});
"#;
    run_fixture(source, "request_method_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_request_body_behaves_like_a_response_body() {
    let source = r#"
import { test, expect } from 'vitest';

test('reading the body yields its text and marks it used', async () => {
  const request = new Request('https://a.test/p', { method: 'POST', body: 'hello' });
  expect(request.bodyUsed).toBe(false);
  expect(await request.text()).toBe('hello');
  expect(request.bodyUsed).toBe(true);
});

test('a string body implies its content type', () => {
  const request = new Request('https://a.test/p', { method: 'POST', body: 'hello' });
  expect(request.headers.get('content-type')).toBe('text/plain;charset=UTF-8');
});

test('a request with no body has no content type', () => {
  expect(new Request('https://a.test/p').headers.get('content-type')).toBe(null);
});

test('init headers are readable off the request', () => {
  const request = new Request('https://a.test/p', {
    headers: new Headers([['x-a', '1']]),
  });
  expect(request.headers.get('x-a')).toBe('1');
});

test('clone() reads independently of the original', async () => {
  const original = new Request('https://a.test/p', { method: 'POST', body: 'twice' });
  const copy = original.clone();
  expect(copy.method).toBe('POST');
  expect(copy.url).toBe('https://a.test/p');
  expect(await original.text()).toBe('twice');
  expect(original.bodyUsed).toBe(true);
  expect(copy.bodyUsed).toBe(false);
  expect(await copy.text()).toBe('twice');
});
"#;
    run_fixture(source, "request_body_runtime");
}
