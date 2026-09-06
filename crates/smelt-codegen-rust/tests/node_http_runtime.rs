//! Runtime execution tests for `node:http`'s server surface.
//!
//! `node:http` is modeled on hyper 1 with three **concrete generated Rust
//! types** — `SmeltHttpServer`, `SmeltIncomingMessage`, `SmeltServerResponse`.
//! Nothing in the module is erased: the request handler and the listening
//! callback have signatures fixed by `node:http`, so they are stored at their
//! real Rust types rather than through the erased callable ABI. The one
//! `SmeltUnknown` any of this reaches arrives through the `SmeltEventEmitter`
//! that `SmeltIncomingMessage` composes, and that store's boundary is argued
//! where it lives.
//!
//! Everything asserted below was diffed against Node 22 running the same
//! TypeScript, because most of it compiles perfectly either way and is only
//! wrong when it runs:
//!
//! * `listen(0)` binds BEFORE it returns, so `address()` has a real port
//!   immediately — the whole test is addressable only because that holds;
//! * the listening callback runs in the same turn as the bind;
//! * a request body reaches the handler through the EMITTER inheritance —
//!   `req.on('data', ..)` then `req.on('end', ..)` — and `data` does not fire
//!   for an empty body, so a GET sees `''` rather than one empty chunk;
//! * the body is delivered AFTER the handler returns, so a handler that
//!   registers its listeners at the very end still sees every byte;
//! * `writeHead` MERGES its header object over what `setHeader` already set,
//!   rather than replacing the list;
//! * `setHeader` REPLACES case-insensitively, so setting `Content-Type` after
//!   `content-type` leaves one header, not two;
//! * a handler that throws ENDS THE PROGRAM, exit 1, as it does in Node --
//!   answering 500 was the tempting shape and it is measurably wrong;
//! * `close()` releases the process — without it, the exit drain's
//!   ref'd-handle rule would keep the program alive forever, which is exactly
//!   what makes a plain `listen(3000)` serve.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test node_http_runtime -- --ignored
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
///
/// Run on a thread with a large stack. The lowering and emission passes recurse
/// over the expression tree, and a `node:http` fixture is a whole program —
/// nested handler closures around an awaiting module body — which is deeper
/// than the two megabytes a Rust test thread gets by default. The `smelt`
/// binary never hit this because a process main thread has eight; without the
/// bigger stack here the tier aborts on a stack overflow that says nothing
/// about the code under test.
fn emit_program(source: &str, crate_name: &str, crate_dir: &Path) {
    let source = source.to_owned();
    let crate_name = crate_name.to_owned();
    let crate_dir = crate_dir.to_path_buf();
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let mut ctx = HirCtx::new();
            to_hir(&source, FileId(0), &mut ctx).expect("HIR lowering");
            let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering");
            smelt_mir::opt::optimize(&mut mir);
            let options =
                EmitOptions::new(crate_name).with_crate_kind(CrateKind::Program);
            emit_crate(&mir, &crate_dir, &options).expect("crate emission");
        })
        .expect("spawn emitter thread")
        .join()
        .expect("emitter thread");
}

/// Runs the emitted program and returns its stdout.
///
/// A program rather than a test crate: a `node:http` fixture is a server and
/// its own client on one current-thread runtime, and that runtime is the
/// generated `main`'s. Running it under `cargo test` would put the body inside
/// a `#[tokio::test]` instead, which is a different runtime shape from the one
/// under test.
fn run_generated_program(crate_dir: &Path, target_dir: &Path) -> String {
    let output = Command::new(env!("CARGO"))
        .arg("run")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        .env("RUSTFLAGS", "-Awarnings")
        .output()
        .expect("spawn cargo run");
    assert!(
        output.status.success(),
        "generated node:http program failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-node-http-runtime-{}-{seq}", std::process::id()))
}

/// Emit `source` as a program crate, run it, and assert its whole stdout.
fn assert_program_output(source: &str, crate_name: &str, expected: &str) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    emit_program(source, crate_name, &crate_dir);
    let stdout = run_generated_program(&crate_dir, &target_dir);
    assert_eq!(stdout, expected, "generated node:http program output");
    drop(std::fs::remove_dir_all(&root));
}

/// Emit and run a program expected to FAIL, answering its stdout and stderr.
///
/// Its own runner because a throwing handler is supposed to end the process:
/// asserting that through [`assert_program_output`] would report the intended
/// behaviour as a broken fixture.
fn run_failing_program(source: &str, crate_name: &str) -> (String, String) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    emit_program(source, crate_name, &crate_dir);
    // `cargo run` reports the program's own exit code, so a build failure and a
    // deliberate `exit(1)` would look alike. Build first and run the binary
    // directly, and the two stay distinguishable.
    let build = Command::new(env!("CARGO"))
        .arg("build")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", &target_dir)
        .env("RUSTFLAGS", "-Awarnings")
        .output()
        .expect("spawn cargo build");
    assert!(
        build.status.success(),
        "generated node:http program did not compile:\n{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let output = Command::new(target_dir.join("debug").join(crate_name))
        .output()
        .expect("run generated program");
    assert!(
        !output.status.success(),
        "the program was expected to end with a failure status"
    );
    let result = (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    );
    drop(std::fs::remove_dir_all(&root));
    result
}

#[test]
#[ignore = "slow: emits, compiles and runs a generated server crate; run in CI via --ignored"]
fn listen_binds_before_it_returns_and_reports_its_port() {
    // The bind is synchronous in Node -- `listen(0)` then `address()` is a
    // legal pair -- and everything else here depends on that being true, since
    // the port is what the client builds its URL from. The port itself varies,
    // so what is asserted is that it exists, is in range, and is gone after
    // `close`.
    let source = r#"
import { createServer } from 'node:http';

const server = createServer((req, res) => {
  res.end('ok');
});

const before = server.address() ?? -1;
server.listen(0, () => { console.log('callback ran in the same turn'); });
const bound = server.address() ?? -1;
console.log(before);
console.log(bound > 0 && bound < 65536);
server.close();
console.log(server.address() ?? -1);
"#;
    assert_program_output(
        source,
        "http_listen_runtime",
        "callback ran in the same turn\n-1\ntrue\n-1\n",
    );
}

#[test]
#[ignore = "slow: emits, compiles and runs a generated server crate; run in CI via --ignored"]
fn a_body_reaches_the_handler_through_the_emitter_inheritance() {
    // The coupling that made `node:http` one commit with `node:events`:
    // `IncomingMessage` extends `EventEmitter`, so this is how a body is read.
    // The listeners are registered at the END of the handler on purpose --
    // the body is delivered after the handler returns, so registering last
    // must still see every byte.
    let source = r#"
import { createServer } from 'node:http';

const server = createServer((req, res) => {
  let received = '';
  let ended = false;
  let chunks = 0;
  req.on('data', (chunk) => { received += chunk; chunks += 1; });
  req.on('end', () => {
    ended = true;
    res.end(`${req.method} ${chunks} ${ended} ${received}`);
  });
});

server.listen(0);
const port = server.address() ?? 0;

const posted = await fetch(`http://127.0.0.1:${port}/p`, { method: 'POST', body: 'abc' });
console.log(await posted.text());

const got = await fetch(`http://127.0.0.1:${port}/g`);
console.log(await got.text());

server.close();
"#;
    // The GET sees ZERO `data` events, not one empty chunk: Node emits `data`
    // only for bytes that exist.
    assert_program_output(
        source,
        "http_body_runtime",
        "POST 1 true abc\nGET 0 true \n",
    );
}

#[test]
#[ignore = "slow: emits, compiles and runs a generated server crate; run in CI via --ignored"]
fn header_writes_replace_case_insensitively_and_write_head_merges() {
    let source = r#"
import { createServer } from 'node:http';

const server = createServer((req, res) => {
  res.setHeader('content-type', 'text/plain');
  // Same header under another spelling: this REPLACES rather than appends.
  res.setHeader('Content-Type', 'application/json');
  res.setHeader('x-kept', 'yes');
  // `writeHead` merges over the list rather than replacing it, so `x-kept`
  // survives while `content-type` is overwritten again.
  res.writeHead(202, { 'content-type': 'text/csv' });
  res.end(JSON.stringify({
    type: res.getHeader('content-type'),
    kept: res.getHeader('x-kept'),
    missing: res.getHeader('x-absent'),
    status: res.statusCode,
  }));
});

server.listen(0);
const port = server.address() ?? 0;
const answer = await fetch(`http://127.0.0.1:${port}/h`);
console.log(answer.status);
console.log(answer.headers.get('content-type'));
console.log(answer.headers.get('x-kept'));
console.log(await answer.text());
server.close();
"#;
    assert_program_output(
        source,
        "http_headers_runtime",
        "202\ntext/csv\nyes\n{\"type\":\"text/csv\",\"kept\":\"yes\",\"missing\":null,\"status\":202}\n",
    );
}

#[test]
#[ignore = "slow: emits, compiles and runs a generated server crate; run in CI via --ignored"]
fn a_throwing_handler_ends_the_program_as_it_does_in_node() {
    // Measured against Node 22, and the opposite of the obvious guess: an
    // uncaught exception in a request handler is NOT a 500. It reaches the
    // process as an uncaught exception, prints, and exits 1 -- the server stops
    // serving entirely. Answering 500 would let a generated program keep
    // running where the original had died, turning a crash into a stream of
    // quiet failures.
    let source = r#"
import { createServer } from 'node:http';

const server = createServer((req, res) => {
  if (req.url === '/boom') {
    throw new Error('handler failed');
  }
  res.end('fine');
});

server.listen(0);
const port = server.address() ?? 0;

// The healthy request first, so the failure below cannot be mistaken for a
// server that never worked.
const good = await fetch(`http://127.0.0.1:${port}/ok`);
console.log(good.status);
console.log(await good.text());

await fetch(`http://127.0.0.1:${port}/boom`);
console.log('unreachable');
server.close();
"#;
    let (stdout, stderr) = run_failing_program(source, "http_throwing_runtime");
    assert_eq!(
        stdout, "200\nfine\n",
        "the healthy exchange should complete before the throwing one ends the program"
    );
    assert!(
        stderr.contains("handler failed"),
        "the uncaught error should reach stderr, as Node prints it: {stderr}"
    );
}

#[test]
#[ignore = "slow: emits, compiles and runs a generated server crate; run in CI via --ignored"]
fn a_request_carries_its_method_url_and_lower_cased_headers() {
    // `req.headers` is a plain lower-cased object, not a `Headers`: it is
    // indexed, and it has no `get`. Sending the name in mixed case proves the
    // lower-casing is the runtime's and not the client's.
    let source = r#"
import { createServer } from 'node:http';

const server = createServer((req, res) => {
  const headers = req.headers;
  res.end(JSON.stringify({
    method: req.method,
    url: req.url,
    custom: headers['x-mixed-case'],
  }));
});

server.listen(0);
const port = server.address() ?? 0;
const answer = await fetch(`http://127.0.0.1:${port}/path?query=1`, {
  method: 'PUT',
  headers: { 'X-Mixed-Case': 'sent' },
  body: 'ignored',
});
console.log(await answer.text());
server.close();
"#;
    assert_program_output(
        source,
        "http_request_parts_runtime",
        "{\"method\":\"PUT\",\"url\":\"/path?query=1\",\"custom\":\"sent\"}\n",
    );
}
