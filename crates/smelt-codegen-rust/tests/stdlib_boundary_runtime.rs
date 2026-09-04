//! Runtime execution tests for four stdlib seams whose defects produced Rust
//! that compiled cleanly and then did the wrong thing at run time.
//!
//! 1. **A fallible stdlib operation must be catchable.** `JSON.parse` was
//!    emitted as an infallible `serde_json::from_str(..).expect(..)`. A
//!    statement carries no unwind edge, so the parse could never reach an
//!    enclosing `try`: MIR dropped the `catch` block for want of a predecessor,
//!    and a JavaScript-catchable `SyntaxError` became a process abort.
//!
//! 2. **`Reflect.ownKeys` reports symbol keys.** It was lowered as
//!    `Object.keys`, whose projection deliberately filters the `__smelt_symbol:`
//!    storage keys out, and typed `List<string>` — which also const-folded every
//!    consumer's `typeof key === 'string'` guard.
//!
//! 3. **JavaScript and Rust regex syntax differ.** A JavaScript character class
//!    containing a bare `[` does not compile as Rust, and an uncompilable
//!    pattern used to make `replace`/`split`/`matchAll` return their input
//!    unchanged — a silent no-op that looked like a passing program.
//!
//! 4. **A replacement string is a pattern.** `$&`, `` $` ``, `$'`, `$$`, `$n`
//!    and `$<name>` stand for parts of the match; the runtime pushed the
//!    replacement text verbatim, so `'\\$&'` inserted the two characters `$&`.
//!
//! Each case is a TypeScript Vitest test: lowering emits a `#[test]`, and a
//! green `cargo test` on the generated crate means every `expect(...)` held.
//! None of these can be pinned by a string golden — cases 1 and 3 need the
//! program to actually run to show the wrong control flow, and cases 2 and 4
//! need the real runtime values.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates. Run it
//! explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test stdlib_boundary_runtime -- --ignored
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
        "generated stdlib-boundary test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-stdlib-boundary-runtime-{}-{seq}",
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
fn a_malformed_json_document_is_caught_not_aborted() {
    // The `catch` arm is the assertion: if the parse cannot throw, MIR has no
    // edge into the arm and the arm is gone, so `isJson('nope')` answers `true`
    // (or the process aborts on the `.expect`). Nothing about this is visible in
    // the emitted text alone -- the program has to run.
    let source = r#"
import { test, expect } from "vitest";

function isJson(value: string): boolean {
  try {
    JSON.parse(value);
    return true;
  } catch {
    return false;
  }
}

function parseKind(value: string): string {
  try {
    const parsed = JSON.parse(value);
    return typeof parsed;
  } catch (error) {
    return "threw";
  }
}

test("a malformed JSON document is caught, not aborted", () => {
  expect(isJson("{}")).toBe(true);
  expect(isJson("[1,2]")).toBe(true);
  expect(isJson("invalid json")).toBe(false);
  expect(isJson("")).toBe(false);
  expect(parseKind("{}")).toBe("object");
  expect(parseKind("oops")).toBe("threw");
});
"#;
    run_fixture(source, "smelt_json_parse_catchable");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn own_keys_reports_string_and_symbol_keys() {
    // A symbol-keyed property IS stored, so `Reflect.ownKeys` must report it and
    // its element type must stay dynamic enough for the caller's `typeof` test
    // to be a real test. The string/symbol counts are what a `List<string>`
    // element type cannot produce.
    let source = r#"
import { test, expect } from "vitest";

function keyKinds(value: object): string {
  let strings = 0;
  let symbols = 0;
  for (const key of Reflect.ownKeys(value)) {
    if (typeof key === "string") {
      strings += 1;
    } else {
      symbols += 1;
    }
  }
  return `${strings}/${symbols}`;
}

test("Reflect.ownKeys reports string and symbol keys", () => {
  const marker = Symbol("marker");
  expect(keyKinds({ a: 1, b: 2 })).toBe("2/0");
  expect(keyKinds({ [marker]: 1 })).toBe("0/1");
  expect(keyKinds({ a: 1, [marker]: 2 })).toBe("1/1");
  expect(keyKinds({})).toBe("0/0");
  // `Object.keys` is the string half only -- the two are not interchangeable.
  expect(Object.keys({ a: 1, [marker]: 2 }).length).toBe(1);
});
"#;
    run_fixture(source, "smelt_reflect_own_keys");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_javascript_character_class_compiles_as_rust() {
    // `[\\^$.*+?()[\]{}|]` is a valid JavaScript class whose bare `[` opens a
    // NESTED class in Rust, leaving the outer one unterminated. Because an
    // uncompilable pattern used to make `replace` return its input, the wrong
    // answer was "the input", which reads as a plausible result.
    let source = r#"
import { test, expect } from "vitest";

function escapeRegExp(value: string): string {
  return value.replace(/[\\^$.*+?()[\]{}|]/g, "\\$&");
}

function bracketsOnly(value: string): string {
  return value.replace(/[\[\]]/g, "-");
}

function literalBracket(value: string): string {
  return value.replace(/[a[b]/g, "*");
}

test("a JavaScript character class compiles as Rust", () => {
  expect(escapeRegExp("^$.*+?()[]{}|\\")).toBe("\\^\\$\\.\\*\\+\\?\\(\\)\\[\\]\\{\\}\\|\\\\");
  expect(escapeRegExp("abc")).toBe("abc");
  expect(bracketsOnly("a[b]c")).toBe("a-b-c");
  expect(literalBracket("a[bc")).toBe("***c");
});
"#;
    run_fixture(source, "smelt_regex_character_class");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_replacement_string_expands_its_dollar_patterns() {
    // ECMA-262 `GetSubstitution`. Pushing the replacement verbatim inserted the
    // literal characters `$&` / `$1`, which no static check can see.
    let source = r#"
import { test, expect } from "vitest";

test("a replacement string expands its dollar patterns", () => {
  expect("a-b".replace(/-/g, "$&$&")).toBe("a--b");
  expect("ab".replace(/(a)(b)/, "$2$1")).toBe("ba");
  expect("x".replace(/x/, "$$")).toBe("$");
  expect("abc".replace(/b/, "[$`|$']")).toBe("a[a|c]c");
  expect("2024".replace(/(?<year>\d{4})/, "y=$<year>")).toBe("y=2024");
  // A group number past the last group stays literal, as does any other `$x`.
  expect("ab".replace(/(a)/, "$2")).toBe("$2b");
  expect("ab".replace(/a/, "$z")).toBe("$zb");
});
"#;
    run_fixture(source, "smelt_regex_replacement_patterns");
}
