//! Runtime execution tests for source-declared class ACCESSORS (`get`/`set`).
//!
//! A getter lowers as a `&self` method plus an accessor descriptor, and a setter
//! as its `__smelt_set_<name>` method. Hono's `src/context.ts`
//! (`set res(_res: Response | undefined)`) is the real-world shape.
//!
//! Two things have to be right, and only execution shows either:
//!
//! * a write to an accessor property must run the SETTER'S BODY, not store a
//!   field. A setter usually does more than assign (Hono's clones the response
//!   and remembers a flag), so a direct field write silently loses that work;
//! * a `#name` private field must NOT be confused with the property of the same
//!   spelling. `#res` beside `get res()/set res()` is exactly Hono's shape: a
//!   private name interns as `#res`, its own namespace, so `set res(v) { this.#res
//!   = v }` writes the slot and does not recurse into the setter.
//!
//! Every expectation is what Node 22 prints for the same source.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test class_accessor_runtime -- --ignored
//! ```

#![expect(
    clippy::expect_used,
    reason = "runtime tests fail fast on invalid fixture setup"
)]

#[path = "harness/mod.rs"]
mod harness;

use harness::{emit_program, run_generated_tests};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-class-accessor-runtime-{}-{seq}",
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
    let outcome = std::panic::catch_unwind(|| {
        emit_program(source, crate_name, &crate_dir);
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
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_write_to_an_accessor_property_runs_the_setter_body() {
    // `#celsius` beside `get celsius()/set celsius()` is Hono's `#res` shape:
    // the private name must not shadow the accessor pair. The `writes` counter
    // is what distinguishes running the setter from storing a field — both
    // agree on the temperature.
    let source = r"
import { test, expect } from 'vitest';

class Temp {
  #celsius = 0;
  writes = 0;

  get celsius(): number {
    return this.#celsius;
  }

  set celsius(value: number) {
    this.writes++;
    this.#celsius = value;
  }

  get fahrenheit(): number {
    return this.#celsius * 1.8 + 32;
  }

  set fahrenheit(value: number) {
    this.celsius = (value - 32) / 1.8;
  }
}

test('a write to an accessor property runs the setter', () => {
  const temp = new Temp();
  temp.celsius = 100;
  expect(temp.celsius).toBe(100);
  expect(temp.fahrenheit).toBe(212);
  expect(temp.writes).toBe(1);
});

test('a setter may write through another accessor', () => {
  const temp = new Temp();
  temp.fahrenheit = 32;
  expect(temp.celsius).toBe(0);
  expect(temp.writes).toBe(1);
});

test('each instance keeps its own accessor state', () => {
  const first = new Temp();
  const second = new Temp();
  first.celsius = 10;
  expect(first.celsius).toBe(10);
  expect(second.celsius).toBe(0);
  expect(second.writes).toBe(0);
});
";
    run_fixture(source, "class_accessor_setter_body");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_setter_may_reject_or_transform_the_written_value() {
    // The reason a write cannot be a field store: a setter validates, clamps,
    // normalizes and keeps side state. A set-ONLY property is here too — it has
    // no getter, and JavaScript reads `undefined` from it while the write still
    // lands where the setter puts it.
    let source = r"
import { test, expect } from 'vitest';

class Gauge {
  #value = 0;
  log: string[] = [];

  get value(): number {
    return this.#value;
  }

  set value(next: number) {
    this.log.push(`set:${next}`);
    this.#value = next < 0 ? 0 : next;
  }

  set doubled(next: number) {
    this.value = next * 2;
  }
}

test('a setter transforms the value it stores', () => {
  const gauge = new Gauge();
  gauge.value = -5;
  expect(gauge.value).toBe(0);
  gauge.value = 7;
  expect(gauge.value).toBe(7);
});

test('a setter keeps its side state', () => {
  const gauge = new Gauge();
  gauge.value = 1;
  gauge.value = 2;
  expect(gauge.log).toEqual(['set:1', 'set:2']);
});

test('a set-only property writes through its setter', () => {
  const gauge = new Gauge();
  gauge.doubled = 4;
  expect(gauge.value).toBe(8);
  expect(gauge.log).toEqual(['set:8']);
});
";
    run_fixture(source, "class_accessor_setter_transform");
}
