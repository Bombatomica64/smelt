//! Runtime execution tests for `Response`/`Request` inits that are not literals.
//!
//! An init whose static type is `ResponseInit`/`RequestInit` — an interface-typed
//! parameter, a variable, a field, or a spread of one — carries exactly as much
//! type information as a literal: the keys are declared, their types are known,
//! and reading them is an ordinary typed field read. Only a genuinely erased
//! init (an `unknown`/`any` value) has nothing to read with its type intact.
//!
//! What needs a *runtime* tier is the defaulting, because every key on an init
//! interface is **optional**. A key read off a typed init has type
//! `Optional<T>` where the same key written in a literal has type `T`, so the
//! absent case has to fall back to the spec's default — 200, an empty reason
//! phrase, `GET`, an empty header list, no body — and picking the wrong default
//! compiles perfectly and serves a plausible wrong value.
//!
//! The source-interface fixture uses only single-word keys, and not because
//! multi-word ones are unsupported here: an object literal assigned to an
//! interface destination silently DROPS every camelCase field
//! (`{ statusText: 'x' }` emits `status_text: None`), which is a pre-existing
//! wrong-value bug recorded in `blocker-logs/interface-literal-camel-case.md`.
//! The ambient `ResponseInit` fixtures above do exercise `statusText`, because
//! there the value crosses as an erased record whose keys keep their original
//! spelling — the same key survives one path and not the other, which is what
//! makes that bug worth its own report.
//!
//! Every expectation below was diffed against Node 22 on the same source. The
//! statuses are all ones Node's constructor accepts: 204/205/304 are null-body
//! statuses and Node rejects a body with them (`Invalid response status code
//! 204`), which is the same unvalidated-constructor gap the `Response` tier
//! records rather than a difference in the init handling.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test fetch_init_runtime -- --ignored
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

/// Runs `cargo test` on the emitted crate.
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
        "generated fetch-init test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-fetch-init-runtime-{}-{seq}", std::process::id()))
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
fn a_response_init_variable_is_read_by_field() {
    let source = r#"
import { test, expect } from 'vitest';

function make(body: string, init: ResponseInit): Response {
  return new Response(body, init);
}

test('a fully populated init supplies every key', () => {
  const response = make('hi', { status: 201, statusText: 'Created' });
  expect(response.status).toBe(201);
  expect(response.statusText).toBe('Created');
  expect(response.ok).toBe(true);
});

test('an absent key falls back to the spec default, not to a wrong value', () => {
  const response = make('hi', { status: 404 });
  expect(response.status).toBe(404);
  expect(response.statusText).toBe('');
  expect(response.ok).toBe(false);
});

test('an empty init is every default', () => {
  const response = make('hi', {});
  expect(response.status).toBe(200);
  expect(response.statusText).toBe('');
  expect(response.ok).toBe(true);
});

test('headers given through a typed init are readable', () => {
  const response = make('hi', { headers: new Headers([['x-a', '1']]) });
  expect(response.headers.get('x-a')).toBe('1');
});

test('a typed init with no headers still has an empty list', () => {
  expect(make('hi', { status: 200 }).headers.get('x-a')).toBe(null);
});
"#;
    run_fixture(source, "response_init_variable_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_spread_init_lets_later_keys_win() {
    let source = r#"
import { test, expect } from 'vitest';

function withStatus(init: ResponseInit, status: number): Response {
  return new Response('hi', { ...init, status });
}

test('the later key overrides the spread source', () => {
  const response = withStatus({ status: 500, statusText: 'Server Error' }, 201);
  expect(response.status).toBe(201);
  // The key the spread supplied and the literal did not is kept.
  expect(response.statusText).toBe('Server Error');
});

test('the spread supplies keys the literal omits', () => {
  const response = withStatus({ statusText: 'Kept' }, 202);
  expect(response.status).toBe(202);
  expect(response.statusText).toBe('Kept');
});
"#;
    run_fixture(source, "response_init_spread_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_request_init_variable_is_read_by_field() {
    let source = r#"
import { test, expect } from 'vitest';

function send(url: string, init: RequestInit): Request {
  return new Request(url, init);
}

test('method and body come from a typed init', async () => {
  const request = send('https://a.test/p', { method: 'post', body: 'payload' });
  // Normalization still applies to a method read off a typed init.
  expect(request.method).toBe('POST');
  expect(await request.text()).toBe('payload');
});

test('an absent method is GET and an absent body is empty', async () => {
  const request = send('https://a.test/p', {});
  expect(request.method).toBe('GET');
  expect(request.url).toBe('https://a.test/p');
  expect(await request.text()).toBe('');
});

test('a header record given through a typed init is readable', () => {
  const request = send('https://a.test/p', { headers: new Headers([['x-b', '2']]) });
  expect(request.headers.get('x-b')).toBe('2');
});
"#;
    run_fixture(source, "request_init_variable_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_user_declared_init_interface_works_the_same_way() {
    let source = r#"
import { test, expect } from 'vitest';

interface PageInit {
  status?: number;
  headers?: Headers;
}

function page(init: PageInit): Response {
  return new Response('page', init);
}

test('a source interface is read by field like the ambient one', () => {
  const response = page({ status: 301 });
  expect(response.status).toBe(301);
  expect(response.ok).toBe(false);
});

test('a source interface supplies a header list too', () => {
  const response = page({ headers: new Headers([['x-c', '3']]) });
  expect(response.headers.get('x-c')).toBe('3');
  expect(response.status).toBe(200);
});
"#;
    run_fixture(source, "user_init_interface_runtime");
}
