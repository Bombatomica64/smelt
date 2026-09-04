//! Runtime execution tests for an out-of-range element read that flows into an
//! ERASED slot.
//!
//! `array_index_undefined_runtime.rs` covers the `Option<..>` destination: a
//! miss stays `None`. This file covers the destination it must agree with. An
//! out-of-range element read is `undefined` in JavaScript, so erasing it has to
//! produce `SmeltUnknown::Undefined`.
//!
//! The emitter used to make the read TOTAL first and erase the result
//! afterwards, so the miss became the element type's own missing value and was
//! then erased as that value:
//!
//! * `row[i] = b[i]` for `b: string[]` stored `''` (the `String` default), and
//! * for `n: number[]` it stored `0`,
//! * and for `nested: string[][]` it stored an empty array,
//!
//! none of which JavaScript ever produces for a missing element. The two
//! coercion targets disagreed about the same read.
//!
//! The same rule applies when the RECEIVER is erased instead of the
//! destination: indexing a `SmeltUnknown` that holds an array or a string
//! answered `SmeltUnknown::Null` for an out-of-range index, which is what made
//! a `zipWith`-shaped combiner over ragged inputs render `"3null"` where
//! JavaScript renders `"3undefined"`.
//!
//! Golden string assertions live in `part_7_tests.rs`; they prove the shape is
//! emitted. Only running the program proves the stored value really is
//! `undefined`, that an in-range read is untouched, and that a CONCRETE
//! destination still stores the element default (there is no `undefined` to put
//! in a `Vec<f64>`).
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test erased_element_read_runtime -- --ignored
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
        "generated array-index test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-erased-element-read-runtime-{}-{seq}",
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
fn an_out_of_range_element_read_erases_as_undefined() {
    let source = r"
import { test, expect } from 'vitest';

function fill(b: string[], n: number[], nested: string[][], i: number): unknown[] {
  const row: unknown[] = [0, 0, 0];
  row[0] = b[i];
  row[1] = n[i];
  row[2] = nested[i];
  return row;
}

test('a missing element erases as undefined, whatever the element type', () => {
  expect(fill(['a'], [1], [['x']], 5)).toEqual([undefined, undefined, undefined]);
});
test('an in-range read is unaffected', () => {
  expect(fill(['a'], [1], [['x']], 0)).toEqual(['a', 1, ['x']]);
});
";
    run_fixture(source, "smelt_erased_element_read");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_concrete_destination_still_stores_the_element_default() {
    // The erased-target rule must not leak into a concrete slot: a `Vec<f64>`
    // has nowhere to put `undefined`, so widening it is a STORAGE question and
    // is deliberately out of scope here. This test pins the behaviour that was
    // left alone, so a later storage change is a deliberate edit rather than
    // accidental fallout.
    let source = r"
import { test, expect } from 'vitest';

function copy(b: string[], n: number[], i: number): [string, number] {
  const s: string[] = [''];
  const m: number[] = [0];
  s[0] = b[i];
  m[0] = n[i];
  return [s[0], m[0]];
}

test('a concrete slot keeps the element default for a miss', () => {
  expect(copy(['a'], [1], 5)).toEqual(['', 0]);
  expect(copy(['a'], [1], 0)).toEqual(['a', 1]);
});
";
    run_fixture(source, "smelt_concrete_element_default");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_out_of_range_read_on_an_erased_receiver_is_undefined() {
    // The `zipWith` shape: the receiver is erased, so the read goes through the
    // runtime `SmeltUnknown` index dispatch, which answered `Null` for a miss.
    let source = r"
import { test, expect } from 'vitest';

function pick(value: unknown, index: number): string {
  return `${(value as any)[index]}`;
}

test('a missing array element on an erased receiver is undefined', () => {
  expect(pick(['a', 'b'], 5)).toBe('undefined');
  expect(pick(['a', 'b'], 1)).toBe('b');
});
test('a missing string character on an erased receiver is undefined', () => {
  expect(pick('hi', 5)).toBe('undefined');
  expect(pick('hi', 0)).toBe('h');
});
";
    run_fixture(source, "smelt_erased_receiver_element_read");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_object_literal_holding_undefined_erases_as_undefined() {
    // `null` and `undefined` both collapse to MIR `Type::None`, so a
    // `Dict<_, None>` cannot say which JS singleton it holds; only the defining
    // constants can. The list arm recovered that from its defining
    // `Rvalue::List`; the dict arm did not, and `{ k: undefined }` crossing into
    // `unknown` emitted a per-entry `SmeltUnknown::Null` constant. es-toolkit's
    // `isJSONObject({ undefinedProperty: undefined })` then answered `true`,
    // because `null` IS valid JSON and `undefined` is not.
    let source = r"
import { test, expect } from 'vitest';

function typeOfProperty(value: unknown): string {
  return typeof (value as Record<string, unknown>)['a'];
}
function propertyIsNull(value: unknown): boolean {
  return (value as Record<string, unknown>)['a'] === null;
}

test('an undefined property stays undefined across erasure', () => {
  const holder = { a: undefined };
  const erased: unknown = holder;
  expect(typeOfProperty(erased)).toBe('undefined');
  expect(propertyIsNull(erased)).toBe(false);
});
test('a null property stays null across erasure', () => {
  const holder = { a: null };
  const erased: unknown = holder;
  expect(typeOfProperty(erased)).toBe('object');
  expect(propertyIsNull(erased)).toBe(true);
});
";
    run_fixture(source, "smelt_dict_undefined_erasure");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_absent_property_on_a_class_receiver_reads_as_undefined() {
    // A keyed read that resolves to "this receiver has no such property" is
    // JavaScript `undefined`, never `null` -- and `===` sees the difference, so
    // the two are not interchangeable. The emitter's index fallback handed back
    // the `null` tag, which made es-toolkit's `cloneDeep` spec assertion
    // `expect(b['#b']).toBe(undefined)` fail on a value that was otherwise
    // right (the private field correctly did not leak as a string key).
    let source = r"
import { test, expect } from 'vitest';

class Holder {
  #hidden = 1;
  value = 2;
  reveal(): number {
    return this.#hidden;
  }
}

test('an unmodelled key on a class instance is undefined, not null', () => {
  const holder = new Holder() as any;
  expect(typeof holder['#hidden']).toBe('undefined');
  expect(holder['#hidden']).toBe(undefined);
  expect(holder['#hidden'] === null).toBe(false);
  expect(holder['nope']).toBe(undefined);
});
test('the declared members are unaffected', () => {
  const holder = new Holder();
  expect(holder.value).toBe(2);
  expect(holder.reveal()).toBe(1);
});
";
    run_fixture(source, "smelt_absent_class_property_read");
}
