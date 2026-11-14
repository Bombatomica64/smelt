//! Runtime execution tests for `JSON.stringify` of an erased value.
//!
//! Four wrong values lived in one emitted `Serialize` impl, all of them program
//! OUTPUT and so invisible to every compile gate:
//!
//! 1. a **host object leaked Smelt's internals**:
//!    `JSON.stringify(new Headers([['a','b']]))` produced
//!    `{"__smelt_headers":true,"entries":[["a","b"]]}` where Node produces
//!    `{}`. The marker is an implementation detail and it was reaching program
//!    output;
//! 2. **numbers used Rust float formatting**: `{"a":1.0}` where Node writes
//!    `{"a":1}`. ECMA-262 renders a number with the JavaScript
//!    number-to-string algorithm, so an integral value has no fraction;
//! 3. **key order was destroyed** by collecting into a `HashMap`, where
//!    JavaScript preserves insertion order;
//! 4. a property whose value is `undefined` was written as `null` instead of
//!    being **omitted**.
//!
//! The fix is one rule rather than four patches: `JSON.stringify` writes an
//! object's own enumerable properties, in order, which is the same rule
//! `for...in` uses — so both read one predicate. A host object has no own
//! enumerable properties, hence `{}`.
//!
//! Every expectation below was diffed against Node 22 on the same source.
//! Inside an ARRAY, `undefined` still serializes as `null`, which is why the
//! omission rule is applied only to object properties.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test json_stringify_fidelity_runtime -- --ignored
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
    std::env::temp_dir().join(format!("smelt-json-fidelity-{}-{seq}", std::process::id()))
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
fn a_host_object_serializes_as_an_empty_object() {
    let source = r"
import { test, expect } from 'vitest';

test('a Headers list writes no internals', () => {
  const erased: unknown = new Headers([['a', 'b']]);
  expect(JSON.stringify(erased)).toBe('{}');
});

test('a URLSearchParams writes no internals', () => {
  const erased: unknown = new URLSearchParams('a=1');
  expect(JSON.stringify(erased)).toBe('{}');
});

test('a statically typed host object serializes the same way', () => {
  expect(JSON.stringify(new Headers([['a', 'b']]))).toBe('{}');
});

test('a Response writes no internals either', () => {
  const erased: unknown = new Response('body', { status: 201 });
  expect(JSON.stringify(erased)).toBe('{}');
});
";
    run_fixture(source, "json_host_object_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn numbers_use_the_javascript_number_to_string_algorithm() {
    let source = r#"
import { test, expect } from 'vitest';

test('an integral number has no fraction', () => {
  const value: unknown = { a: 1 };
  expect(JSON.stringify(value)).toBe('{"a":1}');
});

test('a fractional number keeps its fraction', () => {
  const value: unknown = { a: 1.5 };
  expect(JSON.stringify(value)).toBe('{"a":1.5}');
});

test('negative zero is zero', () => {
  const value: unknown = { a: -0 };
  expect(JSON.stringify(value)).toBe('{"a":0}');
});

test('array elements follow the same rule', () => {
  const value: unknown = [1, 2.5, 3];
  expect(JSON.stringify(value)).toBe('[1,2.5,3]');
});

test('a nested object follows it too', () => {
  const value: unknown = { z: [1, 2.5, { y: 3 }] };
  expect(JSON.stringify(value)).toBe('{"z":[1,2.5,{"y":3}]}');
});
"#;
    run_fixture(source, "json_number_format_runtime");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn key_order_is_insertion_order_and_undefined_is_omitted() {
    let source = r#"
import { test, expect } from 'vitest';

test('keys keep their insertion order', () => {
  const value: unknown = { b: 1, a: 2, c: 3 };
  expect(JSON.stringify(value)).toBe('{"b":1,"a":2,"c":3}');
});

test('a null property is written', () => {
  const value: unknown = { a: null };
  expect(JSON.stringify(value)).toBe('{"a":null}');
});
"#;
    run_fixture(source, "json_key_order_runtime");
}
