//! The contract every generated-runtime tier is held to, checked on its own.
//!
//! `harness::run_generated_tests` used to assert only `cargo test`'s exit status,
//! so a generated crate that ran `0 tests` passed every `*_runtime` tier: a
//! lowering that dropped `test(...)` registrations, or an emitter that lost
//! `#[test]`, left the whole family green. The harness now requires the libtest
//! summary to show exactly the tests the TypeScript fixture declares, all passed.
//!
//! The fast tests pin the two pieces that decide that verdict (the summary
//! parser and the source-side test counter). The `#[ignore]`d ones are planted
//! defects on a real emitted crate: its `#[test]` attributes are stripped, so it
//! compiles, exits 0 and runs nothing, and the harness must still fail it.
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test harness_contract_runtime -- --include-ignored
//! ```

#![expect(
    clippy::expect_used,
    reason = "runtime tests fail fast on invalid fixture setup"
)]

use std::{
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

mod harness;

use harness::{
    TestCounts, declared_test_count, emit_program, parse_test_counts, run_generated_tests,
    run_generated_tests_expecting_failure,
};

#[test]
fn the_summary_sums_every_test_binary() {
    let stdout = "\nrunning 3 tests\n...\ntest result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n\n\
                  running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n";
    assert_eq!(
        parse_test_counts(stdout),
        Some(TestCounts {
            passed: 3,
            failed: 0
        })
    );
    let failing = "test result: FAILED. 1 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out";
    assert_eq!(
        parse_test_counts(failing),
        Some(TestCounts {
            passed: 1,
            failed: 2
        })
    );
}

#[test]
fn a_run_without_a_summary_is_not_zero_tests() {
    // A killed or never-started run prints no summary; reading it as `0 passed`
    // would let the zero-tests defect back in through the parser.
    assert_eq!(parse_test_counts("error: could not compile `x`\n"), None);
    assert_eq!(parse_test_counts(""), None);
}

#[test]
fn the_declared_count_reads_registrations_only() {
    let source = r#"
import { describe, expect, it, test } from 'vitest';
// test("commented out", () => {});
/* it("block comment", () => {}) */
function check(re: RegExp, s: string): boolean { return re.test(s); }
const label = "test(not a call)";
const tpl = `it(${label})`;
test("one", () => { expect(check(/a/, "a")).toBe(true); });
describe("group", () => {
  it ("two", () => {});
  test('three test(', () => {});
});
"#;
    assert_eq!(declared_test_count(source), 3);
    assert_eq!(declared_test_count("export const x = 1;"), 0);
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-runtime-harness-contract-{}-{seq}",
        std::process::id()
    ))
}

/// Deletes every `#[test]` attribute in the emitted crate's sources: the crate
/// still compiles and `cargo test` exits 0, but runs no test.
fn strip_test_attributes(dir: &Path) {
    for entry in std::fs::read_dir(dir).expect("read crate dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            strip_test_attributes(&path);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            let text = std::fs::read_to_string(&path).expect("read emitted source");
            if text.contains("#[test]") {
                std::fs::write(&path, text.replace("#[test]", "")).expect("rewrite source");
            }
        }
    }
}

/// Emits `source`, applies `plant` to the crate, and returns whether `judge`
/// panicked (rejected the crate).
fn judge_rejects(source: &str, crate_name: &str, plant: fn(&Path), judge: harness::Judge) -> bool {
    let root = scratch_root();
    let crate_dir = root.join("crate");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crate_dir).expect("create crate dir");
    std::fs::create_dir_all(&target_dir).expect("create target dir");
    emit_program(source, crate_name, &crate_dir);
    plant(&crate_dir.join("src"));
    let rejected = std::panic::catch_unwind(AssertUnwindSafe(|| {
        judge(source, &crate_dir, &target_dir);
    }))
    .is_err();
    if std::env::var_os("SMELT_KEEP_RUNTIME_SCRATCH").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
    rejected
}

const TWO_PASSING_TESTS: &str = r#"
import { expect, test } from 'vitest';

test("adds", () => { expect(1 + 1).toBe(2); });
test("concatenates", () => { expect("a" + "b").toBe("ab"); });
"#;

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_untouched_crate_meets_the_contract() {
    // The control for the planted defect below: the same fixture, unmodified,
    // passes, so a rejection there is caused by the plant.
    assert!(!judge_rejects(
        TWO_PASSING_TESTS,
        "smelt_harness_contract_control",
        |_| {},
        run_generated_tests,
    ));
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_crate_that_runs_zero_tests_fails_the_contract() {
    assert!(
        judge_rejects(
            TWO_PASSING_TESTS,
            "smelt_harness_contract_zero_tests",
            strip_test_attributes,
            run_generated_tests,
        ),
        "a generated crate that compiles, exits 0 and runs no test was accepted"
    );
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_should_fail_control_does_not_accept_zero_tests() {
    // Zero tests exit 0, so the should-fail direction must reject them too.
    assert!(judge_rejects(
        TWO_PASSING_TESTS,
        "smelt_harness_contract_zero_tests_fail_dir",
        strip_test_attributes,
        run_generated_tests_expecting_failure,
    ));
}
