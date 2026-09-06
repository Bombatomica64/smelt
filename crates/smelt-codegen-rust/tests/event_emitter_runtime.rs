//! Runtime execution tests for `node:events`' `EventEmitter`.
//!
//! An emitter is modeled as a **concrete generated Rust type**
//! (`SmeltEventEmitter`, an `Rc<RefCell<Vec<SmeltEventListener>>>` with a JS
//! reference identity), not a tagged record. Only the listener *store* is
//! erased, and that is a genuine dynamic boundary: a listener's signature is
//! decided by the event name at run time (`on('data', cb)` takes a chunk,
//! `on('end', cb)` takes nothing), one emitter holds listeners for many events
//! at once, and `emit(name, ...args)` builds its positional list at the
//! emitting site rather than from the emitter's type. No concrete type,
//! generated union, or scoped generic can express a heterogeneous set keyed by
//! a run-time string, so the store uses the existing erased callable ABI.
//!
//! The behaviour worth a *runtime* tier is the part that compiles perfectly
//! either way and is only wrong when it runs:
//!
//! * every registration and removal answers THE EMITTER, which is what makes
//!   `e.on(..).on(..)` chain and `e.on(..) === e` hold;
//! * listeners fire in REGISTRATION order;
//! * `emit` answers whether any listener ran, so an event nobody listens for
//!   is `false` rather than `true`;
//! * `emit`'s tail arguments reach the listener positionally;
//! * a `once` listener runs exactly once and is gone afterwards;
//! * `off` removes ONE instance of a function registered twice, not both;
//! * `emit` iterates a SNAPSHOT: a listener **added** during an emit does not
//!   run in that emit, and a listener **removed** during it **still runs**.
//!
//! Those last two are the ones no compile step can see and the ones a
//! hand-written port gets wrong by walking the live list. Every expectation
//! below was diffed against Node 22 running the same TypeScript.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test event_emitter_runtime -- --ignored
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
        "generated EventEmitter test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-emitter-runtime-{}-{seq}", std::process::id()))
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
fn registration_answers_the_emitter_and_keeps_order() {
    let source = r#"
import { test, expect } from 'vitest';
import { EventEmitter } from 'node:events';

test('on answers the emitter itself, so registration chains', () => {
  const emitter = new EventEmitter();
  const same = emitter.on('a', () => {});
  expect(same === emitter).toBe(true);
});

test('listeners fire in registration order', () => {
  const order: string[] = [];
  const emitter = new EventEmitter();
  emitter.on('a', () => { order.push('first'); });
  emitter.on('a', () => { order.push('second'); });
  emitter.on('a', () => { order.push('third'); });
  emitter.emit('a');
  expect(order.join(',')).toBe('first,second,third');
});

test('addListener is the same operation under another name', () => {
  const order: string[] = [];
  const emitter = new EventEmitter();
  emitter.addListener('a', () => { order.push('added'); });
  emitter.emit('a');
  expect(order.join(',')).toBe('added');
});

test('listeners are scoped to their event name', () => {
  const order: string[] = [];
  const emitter = new EventEmitter();
  emitter.on('a', () => { order.push('a'); });
  emitter.on('b', () => { order.push('b'); });
  emitter.emit('b');
  expect(order.join(',')).toBe('b');
});
"#;
    run_fixture(source, "emitter_registration_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn emit_reports_whether_a_listener_ran_and_forwards_its_tail() {
    let source = r#"
import { test, expect } from 'vitest';
import { EventEmitter } from 'node:events';

test('emit answers false when nobody is listening', () => {
  const emitter = new EventEmitter();
  expect(emitter.emit('nobody')).toBe(false);
});

test('emit answers true when a listener ran', () => {
  const emitter = new EventEmitter();
  emitter.on('a', () => {});
  expect(emitter.emit('a')).toBe(true);
});

test('the tail arguments reach the listener positionally', () => {
  const seen: string[] = [];
  const emitter = new EventEmitter();
  emitter.on('data', (chunk: string, index: number) => {
    seen.push(`${chunk}:${index}`);
  });
  emitter.emit('data', 'payload', 42);
  expect(seen.join(',')).toBe('payload:42');
});
"#;
    run_fixture(source, "emitter_emit_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn once_runs_exactly_once() {
    let source = r#"
import { test, expect } from 'vitest';
import { EventEmitter } from 'node:events';

test('a once listener runs on the first emit and is gone after it', () => {
  const ran: string[] = [];
  const emitter = new EventEmitter();
  emitter.once('go', () => { ran.push('ran'); });
  expect(emitter.listenerCount('go')).toBe(1);
  expect(emitter.emit('go')).toBe(true);
  expect(emitter.emit('go')).toBe(false);
  expect(ran.length).toBe(1);
  expect(emitter.listenerCount('go')).toBe(0);
});
"#;
    run_fixture(source, "emitter_once_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn removal_takes_one_instance_and_removeall_takes_the_event() {
    let source = r#"
import { test, expect } from 'vitest';
import { EventEmitter } from 'node:events';

test('off removes one instance of a function registered twice', () => {
  const emitter = new EventEmitter();
  const listener = () => {};
  emitter.on('x', listener);
  emitter.on('x', listener);
  expect(emitter.listenerCount('x')).toBe(2);
  emitter.off('x', listener);
  expect(emitter.listenerCount('x')).toBe(1);
  emitter.removeListener('x', listener);
  expect(emitter.listenerCount('x')).toBe(0);
});

test('removing a listener that was never added is not an error', () => {
  const emitter = new EventEmitter();
  emitter.off('x', () => {});
  expect(emitter.listenerCount('x')).toBe(0);
});

test('removeAllListeners drops one event and leaves the others', () => {
  const emitter = new EventEmitter();
  emitter.on('y', () => {});
  emitter.on('y', () => {});
  emitter.on('z', () => {});
  emitter.removeAllListeners('y');
  expect(emitter.listenerCount('y')).toBe(0);
  expect(emitter.listenerCount('z')).toBe(1);
});

test('listenerCount of an unknown event is zero', () => {
  expect(new EventEmitter().listenerCount('none')).toBe(0);
});
"#;
    run_fixture(source, "emitter_removal_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn emit_iterates_a_snapshot_of_the_listener_list() {
    let source = r#"
import { test, expect } from 'vitest';
import { EventEmitter } from 'node:events';

test('a listener added during an emit waits for the next one', () => {
  const seen: string[] = [];
  const emitter = new EventEmitter();
  emitter.on('m', () => {
    seen.push('outer');
    emitter.on('m', () => { seen.push('added-during'); });
  });
  emitter.emit('m');
  expect(seen.join(',')).toBe('outer');
  emitter.emit('m');
  expect(seen.join(',')).toBe('outer,outer,added-during');
});

test('a listener removed during an emit still runs in it', () => {
  const seen: string[] = [];
  const emitter = new EventEmitter();
  const second = () => { seen.push('second'); };
  emitter.on('n', () => {
    seen.push('first');
    emitter.off('n', second);
  });
  emitter.on('n', second);
  emitter.emit('n');
  expect(seen.join(',')).toBe('first,second');
  expect(emitter.listenerCount('n')).toBe(1);
});
"#;
    run_fixture(source, "emitter_snapshot_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn two_handles_are_one_emitter() {
    let source = r#"
import { test, expect } from 'vitest';
import { EventEmitter } from 'node:events';

test('assigning an emitter shares its listener list', () => {
  const seen: string[] = [];
  const emitter = new EventEmitter();
  const alias = emitter;
  alias.on('a', () => { seen.push('through-alias'); });
  expect(emitter.listenerCount('a')).toBe(1);
  expect(emitter.emit('a')).toBe(true);
  expect(seen.join(',')).toBe('through-alias');
});

test('two separate emitters do not share listeners', () => {
  const left = new EventEmitter();
  const right = new EventEmitter();
  left.on('a', () => {});
  expect(right.listenerCount('a')).toBe(0);
  expect(right.emit('a')).toBe(false);
});
"#;
    run_fixture(source, "emitter_identity_runtime");
}
