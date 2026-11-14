//! Runtime execution tests for optional fields in a record-to-interface projection.
//!
//! An object literal passed where an interface is expected is emitted as a
//! record and then projected field by field into the target struct. For a field
//! declared `T | undefined` the projection asked whether the record's value type
//! could render as the INNER `T` — and when it could not, wrote `None` and moved
//! on. A record whose value type is already `Option<T>` (an object literal in
//! which some property is itself optional, so every value is optional-shaped)
//! failed that question, so a property that was plainly present was dropped and
//! the callee saw the field's default.
//!
//! That is the "unmodeled member silently becomes a value" class, and it is
//! invisible to every gate but execution: the projection type-checks, compiles,
//! and quietly answers the default. es-toolkit's `debounce` / `throttle` lost
//! their `{ edges: [...] }` option to it, so both behaved with default edges —
//! `edges: ['leading']` never fired the leading edge and `edges: ['trailing']`
//! fired the leading edge it was told to suppress.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so this
//! tier lowers the program to a crate and runs `cargo test` on it — a green run
//! means every `expect(...)` held at runtime. The tier is `#[ignore]`d because it
//! compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test optional_record_field_runtime -- --ignored
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
        "smelt-optional-record-field-runtime-{}-{seq}",
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
fn an_optional_interface_field_survives_an_optional_shaped_record() {
    // The reduced es-toolkit `debounce({ edges: ['leading'] })` shape: an options
    // interface whose every member is optional, so the object literal's record
    // value type is optional-shaped too. Before the fix `options.edges` arrived
    // as `undefined` and the callee took its default branch.
    let source = r#"
import { test, expect } from "vitest";
interface Options {
  edges?: Array<"leading" | "trailing">;
  label?: string;
}
function describeOptions({ edges, label }: Options = {}): string {
  const leading = edges != null && edges.includes("leading");
  const trailing = edges == null || edges.includes("trailing");
  return `${label ?? "none"}:${leading}:${trailing}`;
}
test("an optional array option reaches the callee", () => {
  expect(describeOptions({ edges: ["leading"] })).toBe("none:true:false");
  expect(describeOptions({ edges: ["trailing"] })).toBe("none:false:true");
  expect(describeOptions({ edges: ["leading", "trailing"] })).toBe("none:true:true");
  expect(describeOptions({})).toBe("none:false:true");
  expect(describeOptions()).toBe("none:false:true");
});
test("a second optional option in the same literal also reaches the callee", () => {
  expect(describeOptions({ edges: ["leading"], label: "hi" })).toBe("hi:true:false");
  expect(describeOptions({ label: "hi" })).toBe("hi:false:true");
});
"#;
    run_fixture(source, "smelt_optional_record_field_options");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn optional_scalar_and_object_fields_survive_too() {
    // Nothing about the defect was array-specific: any optional field whose
    // record value arrives already optional-shaped was dropped. An absent
    // property must still read as `undefined`, which is what the dropped-field
    // behaviour was indistinguishable from.
    let source = r#"
import { test, expect } from "vitest";
interface Nested {
  n?: number;
}
interface Config {
  count?: number;
  name?: string;
  flag?: boolean;
  nested?: Nested;
}
function readConfig(config: Config): string {
  return [
    config.count === undefined ? "-" : String(config.count),
    config.name === undefined ? "-" : config.name,
    config.flag === undefined ? "-" : String(config.flag),
    config.nested === undefined ? "-" : String(config.nested.n),
  ].join("/");
}
test("optional scalars and nested objects survive the projection", () => {
  expect(readConfig({ count: 3, name: "a", flag: false, nested: { n: 7 } })).toBe("3/a/false/7");
  expect(readConfig({ count: 0 })).toBe("0/-/-/-");
  expect(readConfig({ flag: true })).toBe("-/-/true/-");
  expect(readConfig({})).toBe("-/-/-/-");
});
"#;
    run_fixture(source, "smelt_optional_record_field_scalars");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_absent_optional_field_of_a_wide_union_record_reads_undefined() {
    // An inline options type mixing `number`, `number | null` and a callback
    // lowers to a record whose values are erased; the source type still says
    // every key is optional. An optional-chained read of an ABSENT key must
    // reach the `??` fallback: it used to `.expect("missing field")` and, once
    // past that, decode `undefined` as `Some(<union>)` (radash `retry`).
    let source = r"
import { test, expect } from 'vitest';

const plan = (options: {
  times?: number
  delay?: number | null
  backoff?: (count: number) => number
}): string => {
  const times = options?.times ?? 3
  const delay = options?.delay
  const backoff = options?.backoff ?? null
  const wait = backoff ? backoff(times) : 0
  return `${times} ${wait}` + (delay === undefined ? ' no-delay' : ' delay')
}

test('every key absent', () => {
  expect(plan({})).toBe('3 0 no-delay');
});
test('some keys present', () => {
  expect(plan({ times: 2, delay: 5 })).toBe('2 0 delay');
});
test('the callback key present', () => {
  expect(plan({ backoff: (count: number) => count * 10 })).toBe('3 30 no-delay');
});
";
    run_fixture(source, "smelt_optional_wide_union_record_field");
}
