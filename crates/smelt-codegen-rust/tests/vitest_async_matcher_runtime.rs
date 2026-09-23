//! Runtime execution tests for `expect(promise).resolves` / `.rejects` matcher
//! chains.
//!
//! This tier exists because the defect it guards was invisible to every other
//! tier. `await expect(p).rejects.toEqual(x)` (and every `.resolves`/`.rejects`
//! matcher except `rejects.toThrow`) used to lower to a bare `Promise<void>`
//! literal: the assertion was dropped, and because the actual was only
//! evaluated into an orphaned HIR expression, *the awaited call itself* was
//! dropped with it. The generated Rust was `let _smelt_tmp_0: () = ();`. It
//! type-checked, `compile_corpus` accepted it, and the generated test passed
//! unconditionally — so the suite reported a pass for an assertion that could
//! not fail.
//!
//! The two properties below are therefore checked by *running* generated
//! crates:
//!
//! 1. The awaited call still happens and the matcher sees the settled value —
//!    a side-effect counter proves the call was not deleted.
//! 2. A false assertion actually fails. `expect_generated_tests_fail` runs a
//!    fixture whose matcher is wrong on purpose and asserts a red run; under
//!    the old lowering it was green, which is precisely the bug.
//!
//! The tier is `#[ignore]`d because it compiles and runs real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test vitest_async_matcher_runtime -- --ignored
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

/// Async helpers shared by the fixtures.
///
/// `calls` counts every entry into `resolving`/`rejecting`, so a fixture can
/// assert that the promise handed to `expect(...)` was really created and
/// awaited rather than optimized away with the assertion.
const HELPERS: &str = r#"
let calls = 0;

async function resolving(): Promise<string> {
  calls += 1;
  return "settled";
}

async function unitResolving(): Promise<void> {
  calls += 1;
}

async function rejecting(): Promise<string> {
  calls += 1;
  throw "boom";
}
"#;

/// Lowers `source` through the real pipeline and emits a runnable program crate.
fn emit_program(source: &str, crate_name: &str, crate_dir: &Path) {
    let mut ctx = HirCtx::new();
    to_hir(source, FileId(0), &mut ctx).expect("HIR lowering");
    let mut mir = smelt_mir::lower_hir(&ctx.krate).expect("MIR lowering");
    smelt_mir::opt::optimize(&mut mir);
    let options = EmitOptions::new(crate_name.to_owned()).with_crate_kind(CrateKind::Program);
    emit_crate(&mir, crate_dir, &options).expect("crate emission");
}

/// Runs `cargo test` on the emitted crate and returns whether it passed.
fn generated_tests_pass(crate_dir: &Path, target_dir: &Path) -> (bool, String) {
    let output = Command::new(env!("CARGO"))
        .arg("test")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        .env("RUSTFLAGS", "-Awarnings")
        .output()
        .expect("spawn cargo test");
    let report = format!(
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.success(), report)
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-vitest-async-runtime-{}-{seq}",
        std::process::id()
    ))
}

/// Emit `source` as a crate, run its generated tests, and return the outcome.
fn run_fixture(source: &str, crate_name: &str) -> (bool, String) {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    let emitted = std::panic::catch_unwind(|| {
        emit_program(source, crate_name, &crate_dir);
        generated_tests_pass(&crate_dir, &target_dir)
    });
    // Removed on the FAILURE path too -- the root holds a whole nested cargo
    // target directory, and one leaked per failing case is what fills `/tmp`.
    // `SMELT_KEEP_RUNTIME_SCRATCH=1` keeps it for a debugging session.
    if std::env::var_os("SMELT_KEEP_RUNTIME_SCRATCH").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
    match emitted {
        Ok(outcome) => outcome,
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// Assert the generated suite passes.
fn expect_generated_tests_pass(source: &str, crate_name: &str) {
    let (passed, report) = run_fixture(source, crate_name);
    assert!(passed, "generated suite should pass but failed:\n{report}");
}

/// Assert the generated suite fails.
///
/// A deleted assertion is indistinguishable from a satisfied one unless a
/// deliberately false assertion is observed to fail, so this direction is the
/// load-bearing half of the guard.
fn expect_generated_tests_fail(source: &str, crate_name: &str) {
    let (passed, report) = run_fixture(source, crate_name);
    assert!(
        !passed,
        "generated suite should have failed but passed \
         (the assertion was dropped again):\n{report}"
    );
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn resolves_matchers_await_the_actual_and_assert_the_settled_value() {
    // `calls` proves the awaited call survived lowering: under the old
    // placeholder path both assertions and both calls vanished and `calls`
    // stayed 0.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HELPERS}
test("resolves matchers assert the settled value", async () => {{
  await expect(unitResolving()).resolves.toBeUndefined();
  await expect(resolving()).resolves.toBe("settled");
  expect(calls).toBe(2);
}});
"#
    );
    expect_generated_tests_pass(&source, "smelt_vitest_resolves_settled");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn rejects_matchers_assert_the_rejection_payload() {
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HELPERS}
test("rejects matchers assert the rejection payload", async () => {{
  await expect(rejecting()).rejects.toEqual("boom");
  expect(calls).toBe(1);
}});
"#
    );
    expect_generated_tests_pass(&source, "smelt_vitest_rejects_payload");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrong_resolves_matcher_fails_the_generated_test() {
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HELPERS}
test("a wrong resolves matcher fails", async () => {{
  await expect(resolving()).resolves.toBe("not settled");
}});
"#
    );
    expect_generated_tests_fail(&source, "smelt_vitest_resolves_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrong_rejects_matcher_fails_the_generated_test() {
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HELPERS}
test("a wrong rejects matcher fails", async () => {{
  await expect(rejecting()).rejects.toEqual("not boom");
}});
"#
    );
    expect_generated_tests_fail(&source, "smelt_vitest_rejects_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_promise_that_resolves_fails_a_rejects_matcher() {
    // Without the `did_throw` guard the catch body never runs, so the matcher
    // never runs either and a promise that resolved would pass a `.rejects`
    // assertion vacuously.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{HELPERS}
test("a resolving promise fails a rejects matcher", async () => {{
  await expect(resolving()).rejects.toEqual("boom");
}});
"#
    );
    expect_generated_tests_fail(&source, "smelt_vitest_rejects_no_rejection");
}


// ---------------------------------------------------------------------------
// Matcher-model coverage (`TestMatcher` and the mock-call matchers).
//
// Each matcher is exercised in BOTH directions: a satisfied assertion must
// pass, and a deliberately false one must fail. Only the failing direction
// proves the assertion was actually emitted rather than dropped.
// ---------------------------------------------------------------------------

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn truthiness_matchers_follow_javascript_truthiness() {
    let source = r#"
import { test, expect } from "vitest";

test("truthiness matchers", () => {
  expect(1).toBeTruthy();
  expect("text").toBeTruthy();
  expect(0).toBeFalsy();
  expect("").toBeFalsy();
  expect(1).not.toBeFalsy();
  const value: string | undefined = "here";
  expect(value).toBeDefined();
  expect(value).toBeTruthy();
});
"#;
    expect_generated_tests_pass(source, "smelt_vitest_truthiness");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrong_truthiness_matcher_fails_the_generated_test() {
    let source = r#"
import { test, expect } from "vitest";

test("a wrong truthiness matcher fails", () => {
  expect(0).toBeTruthy();
});
"#;
    expect_generated_tests_fail(source, "smelt_vitest_truthiness_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrong_to_be_defined_matcher_fails_the_generated_test() {
    let source = r#"
import { test, expect } from "vitest";

test("a wrong toBeDefined fails", () => {
  const value: string | undefined = undefined;
  expect(value).toBeDefined();
});
"#;
    expect_generated_tests_fail(source, "smelt_vitest_defined_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn to_match_accepts_a_substring_and_a_regexp() {
    let source = r#"
import { test, expect } from "vitest";

test("toMatch", () => {
  expect("hello world").toMatch("lo wo");
  expect("hello world").toMatch(/^hello/);
  expect("hello world").not.toMatch(/^world/);
});
"#;
    expect_generated_tests_pass(source, "smelt_vitest_to_match");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrong_to_match_matcher_fails_the_generated_test() {
    let source = r#"
import { test, expect } from "vitest";

test("a wrong toMatch fails", () => {
  expect("hello world").toMatch(/^world/);
});
"#;
    expect_generated_tests_fail(source, "smelt_vitest_to_match_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn comparison_and_type_matchers_assert_their_relation() {
    let source = r#"
import { test, expect } from "vitest";

test("comparison matchers", () => {
  expect(1).toBeLessThan(2);
  expect(2).toBeLessThanOrEqual(2);
  expect(3).toBeGreaterThan(2);
  expect(3).toBeGreaterThanOrEqual(3);
  expect("text").toBeTypeOf("string");
  expect(1).toBeTypeOf("number");
});
"#;
    expect_generated_tests_pass(source, "smelt_vitest_comparisons");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrong_comparison_matcher_fails_the_generated_test() {
    let source = r#"
import { test, expect } from "vitest";

test("a wrong comparison fails", () => {
  expect(3).toBeLessThan(2);
});
"#;
    expect_generated_tests_fail(source, "smelt_vitest_comparison_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn to_throw_error_is_the_same_matcher_as_to_throw() {
    let source = r#"
import { test, expect } from "vitest";

test("toThrowError alias", () => {
  expect(() => {
    throw new Error("boom");
  }).toThrowError();
  expect(() => 1).not.toThrowError();
});
"#;
    expect_generated_tests_pass(source, "smelt_vitest_throw_error_alias");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_non_throwing_callback_fails_to_throw_error() {
    let source = r#"
import { test, expect } from "vitest";

test("a non-throwing callback fails toThrowError", () => {
  expect(() => 1).toThrowError();
});
"#;
    expect_generated_tests_fail(source, "smelt_vitest_throw_error_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn to_match_object_is_a_recursive_subset() {
    let source = r#"
import { test, expect } from "vitest";

test("toMatchObject", () => {
  const value = { id: 1, name: "a", nested: { left: 1, right: 2 } };
  expect(value).toMatchObject({ id: 1 });
  expect(value).toMatchObject({ nested: { left: 1 } });
  expect(value).not.toMatchObject({ id: 2 });
});
"#;
    expect_generated_tests_pass(source, "smelt_vitest_match_object");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrong_to_match_object_fails_the_generated_test() {
    let source = r#"
import { test, expect } from "vitest";

test("a wrong toMatchObject fails", () => {
  const value = { id: 1, nested: { left: 1 } };
  expect(value).toMatchObject({ nested: { left: 2 } });
});
"#;
    expect_generated_tests_fail(source, "smelt_vitest_match_object_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn mock_call_matchers_and_their_jest_aliases_agree() {
    let source = r#"
import { test, expect, vi } from "vitest";

test("mock call matchers", () => {
  const spy = vi.fn();
  spy(1);
  expect(spy).toHaveBeenCalled();
  expect(spy).toHaveBeenCalledOnce();
  expect(spy).toBeCalled();
  expect(spy).toBeCalledTimes(1);
  expect(spy).toBeCalledWith(1);
  expect(spy).toHaveBeenNthCalledWith(1, 1);
  const unused = vi.fn();
  expect(unused).not.toHaveBeenCalled();
  expect(unused).not.toBeCalled();
});
"#;
    expect_generated_tests_pass(source, "smelt_vitest_mock_call_matchers");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_uncalled_mock_fails_to_have_been_called() {
    let source = r#"
import { test, expect, vi } from "vitest";

test("an uncalled mock fails toHaveBeenCalled", () => {
  const spy = vi.fn();
  expect(spy).toHaveBeenCalled();
});
"#;
    expect_generated_tests_fail(source, "smelt_vitest_called_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrong_nth_call_fails_the_generated_test() {
    let source = r#"
import { test, expect, vi } from "vitest";

test("a wrong nth call fails", () => {
  const spy = vi.fn();
  spy(1);
  spy(2);
  expect(spy).toHaveBeenNthCalledWith(2, 1);
});
"#;
    expect_generated_tests_fail(source, "smelt_vitest_nth_call_wrong");
}

/// Source of a function returning its argument as an erased `unknown`.
///
/// The actual reaches `expect(..)` already erased, which is the dynamic
/// boundary the erased `toContain` path exists for.
const ERASE_HELPER: &str = r"
function erase(value: unknown): unknown {
  return value;
}
";

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn to_contain_on_an_erased_actual_dispatches_on_its_runtime_kind() {
    // Before the runtime dispatch, an erased actual was projected to an erased
    // LIST, so a string actual panicked and a set never matched.
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{ERASE_HELPER}
test("erased toContain", () => {{
  expect(erase("hello world")).toContain("world");
  expect(erase([1, 2, 3])).toContain(2);
  expect(erase(new Set(["a", "b"]))).toContain("b");
  expect(erase("hello")).not.toContain("bye");
  expect(erase([1, 2])).not.toContain(5);
}});
"#
    );
    expect_generated_tests_pass(&source, "smelt_vitest_erased_contain");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_wrong_to_contain_on_an_erased_actual_fails() {
    let source = format!(
        r#"
import {{ test, expect }} from "vitest";
{ERASE_HELPER}
test("erased toContain miss", () => {{
  expect(erase("hello")).toContain("world");
}});
"#
    );
    expect_generated_tests_fail(&source, "smelt_vitest_erased_contain_wrong");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn to_contain_and_to_match_on_a_nullable_string_narrow_it() {
    // `headers.get(..)` is `string | null`: the matcher narrows the actual and
    // takes the typed substring path (Hono's `toMatch('application/json')`).
    let source = r#"
import { test, expect } from "vitest";
function header(present: boolean): string | null {
  return present ? "application/json; charset=UTF-8" : null;
}
test("nullable toContain", () => {
  expect(header(true)).toContain("json");
  expect(header(true)).toMatch("application/json");
  expect(header(true)).not.toContain("xml");
});
"#;
    expect_generated_tests_pass(source, "smelt_vitest_nullable_contain");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn to_contain_on_a_null_actual_fails() {
    let source = r#"
import { test, expect } from "vitest";
function header(present: boolean): string | null {
  return present ? "application/json" : null;
}
test("null toContain", () => {
  expect(header(false)).toContain("json");
});
"#;
    expect_generated_tests_fail(source, "smelt_vitest_null_contain");
}

/// A test body with `count` sequential `toThrow` assertions, all satisfied
/// except the last when `last_throws` is false.
fn sequential_to_throw_source(count: usize, last_throws: bool) -> String {
    use std::fmt::Write as _;
    let mut assertions = String::new();
    for position in 1..=count {
        let input = if position == count && !last_throws { "ok" } else { "" };
        writeln!(assertions, "  expect(() => parse(\"{input}\")).toThrow();")
            .expect("writing to a String cannot fail");
    }
    format!(
        r#"
import {{ test, expect }} from "vitest";
function parse(value: string): number {{
  if (value.length === 0) {{
    throw new Error("empty");
  }}
  return value.length;
}}
test("sequential toThrow", () => {{
{assertions}}});
"#
    )
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn twelve_sequential_to_throw_assertions_compile_and_run() {
    // 12 is Hono's `utils/cookie.test.ts` count, which used to emit a 616 MB
    // module (each `toThrow` cloned the rest of the test into its 3 arms).
    expect_generated_tests_pass(
        &sequential_to_throw_source(12, true),
        "smelt_vitest_sequential_to_throw",
    );
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_later_to_throw_that_does_not_throw_still_fails() {
    // The shared join must still run every later assertion: the last one is
    // false on purpose.
    expect_generated_tests_fail(
        &sequential_to_throw_source(12, false),
        "smelt_vitest_sequential_to_throw_wrong",
    );
}
