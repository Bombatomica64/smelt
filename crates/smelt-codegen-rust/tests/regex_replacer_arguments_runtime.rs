//! Runtime execution tests for the ECMA-262 regex replacer argument list.
//!
//! `RegExp.prototype[@@replace]` calls a replacement callback with a fixed
//! positional list:
//!
//! ```text
//! (matched, p1, …, pN, position, string)
//! ```
//!
//! where `N` is the PATTERN's capture-group count. A callback declares a prefix
//! of that list, so what its second parameter means is a property of the
//! pattern, not of the callback:
//!
//! ```js
//! '{a}'.replace(/\{[^}]+\}/g, (m, i) => `${i}`)   // no groups: i is the POSITION (0)
//! '"##x##"'.replace(/"##(.+?)##"/g, (m, p) => p)   // one group: p is the CAPTURE ('x')
//! ```
//!
//! Smelt used to model exactly one argument, so every multi-parameter replacer
//! was rejected outright ("regex replacement callback must accept a match
//! string and return a string"). The two spellings above are indistinguishable
//! by callback shape alone, which is why the capture count is now counted from
//! the pattern (`smelt_stdlib::js_regex::capture_group_count`) and each
//! parameter's ROLE resolved in the frontend.
//!
//! Compiling proves nothing here: the failure mode is an argument that arrives
//! with the wrong VALUE — a capture where the position belongs, a byte offset
//! where a character index belongs, or a group that did not participate
//! arriving as `''` instead of `undefined`. Every fixture therefore asserts the
//! substituted string.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test regex_replacer_arguments_runtime -- --ignored
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
        "generated regex replacer test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-regex-replacer-runtime-{}-{seq}",
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
    emit_program(source, crate_name, &crate_dir);
    run_generated_tests(&crate_dir, &target_dir);
    drop(std::fs::remove_dir_all(&root));
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_second_parameter_is_the_position_when_the_pattern_has_no_groups() {
    // Hono's `utils/url.ts` shape: `(match, index) => …` over `/\{[^}]+\}/g`.
    // The pattern has no capture groups, so `index` is the match POSITION.
    // Two matches at different offsets are what tells a real position from a
    // constant 0.
    let source = r"
import { test, expect } from 'vitest';

export const mark = (path: string): string =>
  path.replace(/\{[^}]+\}/g, (match, index) => `@${index}${match}`);

test('the position of the first match is its offset', () => {
  expect(mark('{a}/b')).toBe('@0{a}/b');
});
test('every match reports its own offset', () => {
  expect(mark('x/{ab}/y/{c}')).toBe('x/@2{ab}/y/@9{c}');
});
test('a subject with no match is returned unchanged', () => {
  expect(mark('plain/path')).toBe('plain/path');
});
";
    run_fixture(source, "regex_replacer_position");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_second_parameter_is_capture_one_when_the_pattern_has_a_group() {
    // Hono's `reg-exp-router/router.ts` shape: `(match, metaChar) => …` over a
    // one-group pattern. Same callback arity as the fixture above, opposite
    // meaning — which is precisely why the count comes from the pattern.
    let source = r"
import { test, expect } from 'vitest';

export const escapeMeta = (path: string): string =>
  path.replace(/([.\\+*[^\]$()])/g, (match, metaChar) => (metaChar ? `\\${metaChar}` : match));

test('the second parameter is the captured group', () => {
  expect(escapeMeta('a.b')).toBe('a\\.b');
});
test('every metacharacter is escaped through its own capture', () => {
  expect(escapeMeta('a+b*c')).toBe('a\\+b\\*c');
});
test('a subject with nothing to escape is unchanged', () => {
  expect(escapeMeta('abc')).toBe('abc');
});
";
    run_fixture(source, "regex_replacer_capture");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_unmatched_capture_group_arrives_as_undefined() {
    // A group inside an alternation that did not participate is passed
    // `undefined`, not the empty string — which is why a capture parameter's
    // resolved type is `string | undefined` rather than `string`. Collapsing
    // the two would make the `??` below unreachable and silently change the
    // result.
    let source = r"
import { test, expect } from 'vitest';

export const label = (input: string): string =>
  input.replace(/(?:a(x)|b(y))/g, (match, first, second) => `${first ?? '-'}${second ?? '-'}`);

test('a participating group arrives as its text', () => {
  expect(label('ax')).toBe('x-');
});
test('a group that did not participate arrives as undefined', () => {
  expect(label('by')).toBe('-y');
});
test('each match resolves its own groups', () => {
  expect(label('axby')).toBe('x--y');
});
";
    run_fixture(source, "regex_replacer_unmatched_group");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn the_full_argument_list_ends_with_position_and_subject() {
    // The complete spec list for a one-group pattern:
    // `(matched, p1, position, string)`. The trailing `string` is the WHOLE
    // subject, not the match, and `position` follows the captures rather than
    // preceding them.
    let source = r"
import { test, expect } from 'vitest';

export const describeAll = (input: string): string =>
  input.replace(/<(\w+)>/g, (matched, group, position, subject) =>
    `[${matched}|${group}|${position}|${subject}]`
  );

test('the full replacer argument list is delivered in spec order', () => {
  expect(describeAll('a<bc>d')).toBe('a[<bc>|bc|1|a<bc>d]d');
});
test('the subject argument is the whole input for every match', () => {
  expect(describeAll('<x><y>')).toBe('[<x>|x|0|<x><y>][<y>|y|3|<x><y>]');
});
";
    run_fixture(source, "regex_replacer_full_list");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_single_parameter_callback_still_receives_only_the_match() {
    // The shape that already worked, kept as a guard: adding the wider
    // signature must not start passing extra arguments to a one-parameter
    // callback, and `replace` without the `g` flag must still stop after the
    // first match.
    let source = r"
import { test, expect } from 'vitest';

export const upperAll = (input: string): string =>
  input.replace(/[a-z]+/g, (match) => match.toUpperCase());

export const countAll = (input: string): string =>
  input.replace(/[a-z]+/g, (match) => `${match.length}`);

export const upperFirst = (input: string): string =>
  input.replace(/[a-z]+/, (match) => match.toUpperCase());

test('a one-parameter callback receives the matched text', () => {
  expect(upperAll('ab cd')).toBe('AB CD');
});
test('a non-global replace stops after the first match', () => {
  expect(upperFirst('ab cd')).toBe('AB cd');
});
test('a number-valued expression on the match is stringified, not tagged', () => {
  // `${match.length}` inside a replacer body: the closure-body lowering used
  // to type `.length` as an integer and then run the SmeltUnknown ToString
  // match over it, which does not compile. The shape is independent of the
  // argument list -- it fails for a one-parameter callback too -- so it is
  // guarded here rather than in the multi-argument fixtures.
  expect(countAll('ab cde')).toBe('2 3');
});
";
    run_fixture(source, "regex_replacer_single_argument");
}
