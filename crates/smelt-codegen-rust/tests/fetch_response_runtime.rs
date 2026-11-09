//! Runtime execution test for `fetch(url)` answering a whole `Response`.
//!
//! `fetch` resolves to a `Response`, and a caller reads the status, the header
//! list and the body separately.
//!
//! This test proves the whole round trip against a **real HTTP server**: a
//! plain `TcpListener` on port 0 (so the kernel picks a free port), speaking
//! enough HTTP/1.1 to answer one request with a chosen status, reason phrase,
//! header and body. The generated crate then fetches it and asserts on every
//! part.
//!
//! A local socket rather than a mock, because the parts being asserted are
//! precisely the ones that come from the transport: `status` from the status
//! line, `statusText` from the reason phrase, `headers` from the response
//! headers, and the body from the payload. A mocked transport would be
//! asserting Smelt's own construction, which the `Response` tier already
//! covers.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test fetch_response_runtime -- --ignored
//! ```

#![expect(
    clippy::expect_used,
    reason = "runtime tests fail fast on invalid fixture setup"
)]

#[path = "harness/mod.rs"]
mod harness;

use harness::{emit_program, run_generated_tests};
use std::{
    io::{Read as _, Write as _},
    net::TcpListener,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    thread,
};



/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-fetch-runtime-{}-{seq}", std::process::id()))
}

/// Serve exactly `count` HTTP/1.1 responses on a fresh port, then stop.
///
/// Returns the bound port and the thread handle. The response is fixed: a 201
/// with the reason phrase `Created`, one `x-smelt` header, and a text body.
/// `Connection: close` plus an explicit `Content-Length` keeps the client from
/// waiting for more.
fn serve_fixed_responses(count: usize) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listener.local_addr().expect("local addr").port();
    let handle = thread::spawn(move || {
        for _ in 0..count {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            // Read just the request head; the fixture sends no body.
            let mut buffer = [0_u8; 1024];
            drop(stream.read(&mut buffer));
            let body = "fetched body";
            let response = format!(
                "HTTP/1.1 201 Created\r\nx-smelt: seen\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            drop(stream.write_all(response.as_bytes()));
            drop(stream.flush());
        }
    });
    (port, handle)
}


#[test]
#[ignore = "slow: emits, compiles and runs a generated crate against a local HTTP server"]
fn fetch_resolves_to_a_response_whose_parts_come_from_the_transport() {
    // Four requests: one per generated test below.
    let (port, server) = serve_fixed_responses(4);
    let source = format!(
        r#"
import {{ test, expect }} from 'vitest';

const url = "http://127.0.0.1:{port}/probe";

test('the status line comes from the response, not from a default', async () => {{
  const response = await fetch(url);
  expect(response.status).toBe(201);
  expect(response.statusText).toBe('Created');
  expect(response.ok).toBe(true);
}});

test('response headers are readable', async () => {{
  const response = await fetch(url);
  expect(response.headers.get('x-smelt')).toBe('seen');
  expect(response.headers.get('content-type')).toBe('text/plain');
}});

test('the body reads once through text()', async () => {{
  const response = await fetch(url);
  expect(response.bodyUsed).toBe(false);
  expect(await response.text()).toBe('fetched body');
  expect(response.bodyUsed).toBe(true);
}});

test('a fetched response clones like any other', async () => {{
  const response = await fetch(url);
  const copy = response.clone();
  expect(await response.text()).toBe('fetched body');
  expect(await copy.text()).toBe('fetched body');
}});
"#
    );

    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    let outcome = std::panic::catch_unwind(|| {
        emit_program(&source, "fetch_response_runtime_crate", &crate_dir);
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
    drop(server.join());
}
