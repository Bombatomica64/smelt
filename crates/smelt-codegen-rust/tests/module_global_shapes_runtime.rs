//! Runtime execution tests for mutable module globals beyond primitives.
//!
//! A module-level `let` mutated from a hoisted item body lifts to a
//! "mutable global" backed by a per-thread cell. V1 accepted only a **literal**
//! initializer of a **primitive** type, which rejected the ordinary shape of a
//! module cache:
//!
//! ```ts
//! let cache: Record<string, RegExp> = createNullObject()   // both restrictions
//! ```
//!
//! Two things changed. The initializer may now be any expression: it becomes a
//! synthesized nullary function the cell's `thread_local!` initializer calls,
//! which runs once per thread on first access — the same "module state is
//! initialized before any consumer runs" guarantee JavaScript gives, per
//! generated test. And the type may be anything: a `Copy` primitive keeps its
//! `Cell`, everything else gets a `RefCell` and is read by cloning the borrow.
//!
//! What these fixtures are really checking is the *pairing* of three decisions
//! that used to be made in separate places — the cell type, the read spelling,
//! and the write spelling. A `Cell::get` on a non-`Copy` value does not
//! compile, and a `RefCell` read that forgot to clone does not either, so the
//! interesting failures are compile failures; the value assertions then confirm
//! the initializer actually ran and the writes actually landed.
//!
//! **Not covered, because it is not lowered:** a write *through* a non-`Copy`
//! global (`cache[key] = value`). A `GlobalGet` yields a copy, so such a write
//! would be silently lost; it is a named blocker instead. See
//! `blocker-logs/hono-h6-module-mutable-globals.md`.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test module_global_shapes_runtime -- --ignored
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
        "generated module-global test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-module-global-shapes-runtime-{}-{seq}",
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
fn a_call_initializes_a_record_typed_global() {
    // The Hono `router/reg-exp-router/router.ts` shape, minus the write-through
    // it also does: `let cache: Record<string, X> = createNullObject()`, read
    // from one hoisted function and wholly reassigned from another.
    let source = r"
import { test, expect } from 'vitest';

const seedCache = (): Record<string, string> => ({ seeded: 'yes' });

let cache: Record<string, string> = seedCache();

export function read(key: string): string {
  return cache[key];
}

export function reset(): void {
  cache = seedCache();
}

export function replaceWith(value: string): void {
  cache = { other: value };
}

test('the initializer expression actually ran', () => {
  expect(read('seeded')).toBe('yes');
});
test('a whole-value reassignment is observed by later reads', () => {
  replaceWith('here');
  expect(read('other')).toBe('here');
});
test('reassigning back to the initializer restores it', () => {
  replaceWith('here');
  reset();
  expect(read('seeded')).toBe('yes');
});
";
    run_fixture(source, "module_global_record");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_call_initializes_a_list_typed_global() {
    let source = r"
import { test, expect } from 'vitest';

const seedNames = (): string[] => ['a', 'b'];

let names: string[] = seedNames();

export function first(): string {
  return names[0];
}

export function count(): number {
  return names.length;
}

export function replaceNames(next: string[]): void {
  names = next;
}

test('the list initializer ran', () => {
  expect(first()).toBe('a');
  expect(count()).toBe(2);
});
test('a whole-value reassignment replaces the list', () => {
  replaceNames(['z']);
  expect(first()).toBe('z');
  expect(count()).toBe(1);
});
";
    run_fixture(source, "module_global_list");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn primitive_globals_keep_working_alongside_the_new_shapes() {
    // A guard: the `Cell` path for `Copy` primitives and the `RefCell` path for
    // `String` are decided by the same predicate as the read and write
    // spellings, so a change to one must not desync the others. All four kinds
    // live in ONE program here so a mismatch is a compile failure.
    let source = r"
import { test, expect } from 'vitest';

const seed = (): string => 'seeded';

let counter: number = 0;
let flag: boolean = false;
let label: string = 'x';
let computed: string = seed();

export function bump(): number {
  return ++counter;
}
export function toggle(): boolean {
  flag = !flag;
  return flag;
}
export function relabel(next: string): string {
  label = next;
  return label;
}
export function computedLabel(): string {
  return computed;
}
export function recompute(): string {
  computed = seed();
  return computed;
}

test('a numeric cell still increments', () => {
  expect(bump()).toBe(1);
  expect(bump()).toBe(2);
});
test('a boolean cell still toggles', () => {
  expect(toggle()).toBe(true);
  expect(toggle()).toBe(false);
});
test('a literal-initialized string still stores', () => {
  expect(relabel('y')).toBe('y');
});
test('an expression-initialized string ran its initializer', () => {
  expect(computedLabel()).toBe('seeded');
  expect(recompute()).toBe('seeded');
});
";
    run_fixture(source, "module_global_primitives");
}
