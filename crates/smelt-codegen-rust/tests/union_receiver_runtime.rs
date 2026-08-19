//! Runtime execution tests for containment against a narrowed union receiver.
//!
//! `chars?: string | string[]` interns as `Optional(Union([String, List(String)]))`.
//! Source narrows it with `switch (typeof chars) { case 'object': … }`, and the
//! frontend only emits `Rvalue::ListContains` once that narrowing holds — but MIR
//! reads the value through its DECLARING local, so the operand type at emission is
//! still the wide one. `list_contains_text` matched `Type::List` alone and returned
//! a constant `false` for everything else, so every `chars.includes(...)` loop over
//! an array of characters exited immediately.
//!
//! es-toolkit's `trim`, `trimStart` and `trimEnd` are exactly that loop: ten specs
//! answered the untrimmed string instead of failing. A silent wrong answer is why
//! this needs an executing tier and not only a string golden
//! (`array_containment_projects_an_optional_union_receiver`), which can prove the
//! projection is emitted but not that it selects the right arm.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test union_receiver_runtime -- --ignored
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
        "generated union-receiver test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-union-receiver-runtime-{}-{seq}",
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
fn containment_against_a_narrowed_optional_union_receiver_holds() {
    // Both arms of the `typeof` switch must work: the string arm was already right,
    // and the array arm is the one that used to fold to `false`. The third case pins
    // the non-match path so a projection that always answered `true` would fail too.
    let source = r"
import { test, expect } from 'vitest';

function trimIt(str: string, chars?: string | string[]): string {
  if (chars === undefined) {
    return str;
  }
  let startIndex = 0;
  switch (typeof chars) {
    case 'string': {
      while (startIndex < str.length && str[startIndex] === chars) {
        startIndex++;
      }
      break;
    }
    case 'object': {
      while (startIndex < str.length && chars.includes(str[startIndex])) {
        startIndex++;
      }
    }
  }
  return str.substring(startIndex);
}

test('an array of chars trims every leading match', () => {
  expect(trimIt('---hello', ['-', 'h'])).toBe('ello');
  expect(trimIt('000123', ['0', '1'])).toBe('23');
  expect(trimIt('abcabcabc', ['a', 'b'])).toBe('cabcabc');
});
test('a single-character string still trims', () => {
  expect(trimIt('---hello', '-')).toBe('hello');
});
test('a non-matching array leaves the string alone', () => {
  expect(trimIt('hello', ['x', 'y', 'z'])).toBe('hello');
});
";
    run_fixture(source, "smelt_narrowed_union_containment");
}
