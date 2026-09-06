//! Runtime execution tests for the WHATWG `Response` and its body.
//!
//! `Response` is modeled as a **concrete generated Rust type** (`SmeltResponse`
//! holding a `SmeltHeaders` and a `SmeltBody`), not a tagged record, so the
//! things a caller actually reads keep their source types: `status` is a
//! number, `ok` a boolean, `headers` a `Headers`, `text()` a `Promise<string>`.
//!
//! The behaviour worth a *runtime* tier is the part that compiles perfectly
//! either way and is only wrong when it runs:
//!
//! * `new Response().statusText` is the EMPTY string, not `"OK"`. Node agrees;
//!   filling in a reason phrase would invent an observable value.
//! * `ok` is DERIVED from the status (200-299). Storing it would let a status
//!   change leave it stale, which no compile step would notice.
//! * a body is SINGLE-USE: `bodyUsed` flips on the first reader and a second
//!   read throws a `TypeError`. Getting this wrong yields a plausible second
//!   read of the same bytes.
//! * `clone()` tees the body, so the two sides read independently — while
//!   assigning a response to another variable shares one body, and reading
//!   through either handle marks both used. Two different copies, both needed.
//! * a `Headers` reached through `response.headers` is the SAME list, so a
//!   mutation through it is visible on the response.
//! * a STRING body implies `text/plain;charset=UTF-8`, which the constructor
//!   appends unless the caller set a `Content-Type` — the spec's "extract a
//!   body" step returns a body and a type together.
//!
//! Every expectation below was diffed against Node 22 on the same source,
//! including the thrown `TypeError`'s message
//! (`Body is unusable: Body has already been read`).
//!
//! **Known gap, deliberately not asserted here.** The spec requires the init
//! status to be in 200-599 and Node throws a `RangeError` outside it
//! (`new Response('a', { status: 199 })` does not construct at all). Smelt
//! accepts it, because a constructor is a stdlib *rvalue* and a fallible
//! rvalue has no throwing edge in MIR to reach an enclosing `try` — the same
//! shape as `JSON.parse` and the URI decoders (see
//! `blocker-logs/hono-h10-uri-and-base64-globals.md`). Every status used below
//! is in range, which is what real code passes.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test response_runtime -- --ignored
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
        "generated Response test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-response-runtime-{}-{seq}", std::process::id()))
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
fn the_status_line_carries_the_specs_defaults() {
    let source = r#"
import { test, expect } from 'vitest';

test('a bare Response is 200 with an empty reason phrase', () => {
  const plain = new Response();
  expect(plain.status).toBe(200);
  expect(plain.statusText).toBe('');
  expect(plain.ok).toBe(true);
  expect(plain.bodyUsed).toBe(false);
});

test('an init literal sets the status line', () => {
  const made = new Response('hello', { status: 201, statusText: 'Created' });
  expect(made.status).toBe(201);
  expect(made.statusText).toBe('Created');
  expect(made.ok).toBe(true);
});

test('ok is derived from the status, not stored', () => {
  expect(new Response('a', { status: 599 }).ok).toBe(false);
  expect(new Response('a', { status: 200 }).ok).toBe(true);
  expect(new Response('a', { status: 299 }).ok).toBe(true);
  expect(new Response('a', { status: 300 }).ok).toBe(false);
  expect(new Response('a', { status: 404 }).ok).toBe(false);
  expect(new Response('a', { status: 500 }).ok).toBe(false);
});
"#;
    run_fixture(source, "response_status_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_body_is_single_use() {
    let source = r#"
import { test, expect } from 'vitest';

test('reading the body yields its text and marks it used', async () => {
  const made = new Response('hello');
  expect(made.bodyUsed).toBe(false);
  expect(await made.text()).toBe('hello');
  expect(made.bodyUsed).toBe(true);
});

test('a body with no bytes reads as the empty string', async () => {
  expect(await new Response().text()).toBe('');
});

test('a second read throws a TypeError', async () => {
  const made = new Response('once');
  expect(await made.text()).toBe('once');
  let caught = 'none';
  try {
    await made.text();
  } catch (error) {
    caught = 'threw';
  }
  expect(caught).toBe('threw');
});
"#;
    run_fixture(source, "response_body_use_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn clone_tees_the_body_but_assignment_shares_it() {
    let source = r#"
import { test, expect } from 'vitest';

test('clone() reads independently of the original', async () => {
  const original = new Response('twice', { status: 404 });
  const copy = original.clone();
  expect(copy.status).toBe(404);
  expect(await original.text()).toBe('twice');
  expect(original.bodyUsed).toBe(true);
  expect(copy.bodyUsed).toBe(false);
  expect(await copy.text()).toBe('twice');
});

test('assigning a response shares one body', async () => {
  const original = new Response('shared');
  const alias = original;
  expect(await alias.text()).toBe('shared');
  expect(original.bodyUsed).toBe(true);
});
"#;
    run_fixture(source, "response_clone_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn headers_reached_through_a_response_are_the_same_list() {
    let source = r#"
import { test, expect } from 'vitest';

test('an init header list is readable off the response', () => {
  const made = new Response('body', {
    headers: new Headers([['content-type', 'text/plain']]),
  });
  expect(made.headers.get('content-type')).toBe('text/plain');
  expect(made.headers.has('x-missing')).toBe(false);
});

test('a response with no body has no content type', () => {
  expect(new Response().headers.get('content-type')).toBe(null);
});

test('a string body implies its content type', () => {
  // The spec's "extract a body" step returns a body AND a type, and the
  // constructor appends the type when the caller set none. Node agrees; a
  // response whose content type went missing would still compile and still
  // serve, just with the wrong header.
  expect(new Response('hi').headers.get('content-type')).toBe('text/plain;charset=UTF-8');
});

test('an explicit content type is not overwritten', () => {
  const made = new Response('{}', {
    headers: new Headers([['content-type', 'application/json']]),
  });
  expect(made.headers.get('content-type')).toBe('application/json');
});

test('mutating the reached list is visible on the response', () => {
  const made = new Response('body');
  const reached = made.headers;
  reached.set('x-trace', 'abc');
  expect(made.headers.get('x-trace')).toBe('abc');
});
"#;
    run_fixture(source, "response_headers_runtime");
}
