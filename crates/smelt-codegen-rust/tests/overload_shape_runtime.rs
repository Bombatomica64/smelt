//! Runtime execution tests for four rules about what a *source annotation*
//! states, and what the call site must prove before a lowering may act on it.
//!
//! Each case here was a program that type-checked, compiled, and answered the
//! wrong thing -- the defect class no other gate catches:
//!
//! * A tuple parameter (`readonly [T]`) or a non-empty-array parameter
//!   (`readonly [T, ...T[]]`) states a LENGTH. Only a call-site array literal
//!   proves one; a `T[]` variable proves nothing. Matching a plain array
//!   against the tuple overload picked a signature TypeScript would not, and
//!   the tuple overloads in this family return `[]` -- Rust `()` -- so the call
//!   was computed and thrown away, and the assertion over it compared `()` with
//!   `()` and passed.
//! * A callee whose lowered return is `Option<T>` keeps that at the call site.
//!   Collapsing it with `map_or(Default::default(), ..)` manufactures a value
//!   for absence, so `undefined` came back as `0` / `""` and the
//!   `toBeUndefined()` check const-folded to a passing constant.
//! * An array literal's item-type hint taken from a callee's own type parameter
//!   is not in scope at the call site, and typing the concat with it left the
//!   operands unrelatable -- the emitter substituted an empty list, so
//!   `[...a, ...b]` silently became `[]`.
//! * A JavaScript property key is case-sensitive. Interning a symbol on its
//!   `snake_case` Rust rendering aliased a declaration `Foo` with a property
//!   `foo`, and whichever lowered last owned the spelling BOTH were read with
//!   -- so every erased `.foo` read in a crate declaring `function Foo`
//!   answered `undefined`.
//!
//! Each case is a TypeScript Vitest test; lowering it emits a `#[test]`, so
//! this tier lowers the program to a crate and runs `cargo test` on it -- a
//! green run means every `expect(...)` held at runtime. The tier is `#[ignore]`d
//! because it compiles and executes real crates. Run it explicitly:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test overload_shape_runtime -- --ignored
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
        "generated overload/shape test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-overload-shape-runtime-{}-{seq}",
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
fn a_tuple_overload_is_selected_only_by_an_argument_that_proves_its_length() {
    // The three overloads are `initial`'s real shape. Which one applies is
    // decided purely by the argument's LENGTH, and only the literal calls can
    // settle that: `values` is a `number[]` of unknown length, so the
    // one-element tuple overload (return `[]`) is inapplicable to it however
    // early it is declared.
    let source = r#"
import { test, expect } from "vitest";

function initial<T>(arr: readonly [T]): [];
function initial<T>(arr: readonly [...T[], T]): T[];
function initial<T>(arr: readonly T[]): T[];
function initial<T>(arr: readonly T[]): T[] {
  return arr.slice(0, -1);
}

test("a plain array argument reaches the array overload, not the tuple one", () => {
  const values: number[] = [1, 2, 3];
  const dropped = initial(values);
  expect(dropped.length).toBe(2);
  expect(dropped.join(",")).toBe("1,2");
});

test("an array literal that does prove the length keeps the tuple overload", () => {
  expect(initial([1, 2, 3]).join(",")).toBe("1,2");
  expect(initial(["a"]).length).toBe(0);
});

test("a thousand-element array is not treated as a one-element tuple", () => {
  const large = Array(1000).fill(0).map((_, i) => i);
  expect(initial(large).length).toBe(999);
});
"#;
    run_fixture(source, "smelt_tuple_overload_length_evidence");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_non_empty_array_overload_leaves_absence_observable() {
    // `readonly [T, ...T[]]` guarantees an element, so its overload can promise
    // `T`. A plain `Person[]` cannot prove that and must select the
    // `T | undefined` overload -- and the `Option` the callee returns has to
    // survive to the call site. Collapsing it to `Person::default()` makes the
    // empty-array answer a real object with an empty name, so both the
    // `toBeUndefined()` and the present-value assertions matter.
    let source = r#"
import { test, expect } from "vitest";

interface Person {
  name: string;
  age: number;
}

function maxBy<T>(items: readonly [T, ...T[]], getValue: (item: T) => number): T;
function maxBy<T>(items: readonly T[], getValue: (item: T) => number): T | undefined;
function maxBy<T>(items: readonly T[], getValue: (item: T) => number): T | undefined {
  let best: T | undefined = undefined;
  let bestValue = -Infinity;
  for (const item of items) {
    const value = getValue(item);
    if (value > bestValue) {
      bestValue = value;
      best = item;
    }
  }
  return best;
}

test("an empty array answers undefined instead of a manufactured default", () => {
  const people: Person[] = [];
  const result = maxBy(people, p => p.age);
  expect(result).toBeUndefined();
});

test("a non-empty array still answers the element", () => {
  const people: Person[] = [
    { name: "ada", age: 36 },
    { name: "grace", age: 45 },
  ];
  const result = maxBy(people, p => p.age);
  expect(result?.name).toBe("grace");
});
"#;
    run_fixture(source, "smelt_non_empty_overload_absence");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn an_array_spread_in_an_argument_position_concatenates_its_operands() {
    // The literal's contextual hint is `readonly T[]` with `T` the *callee's*
    // type parameter, which names nothing at the call site. Adopting it typed
    // the concat at `List<T>`, which nothing downstream could relate to the
    // operands' real types -- and the emitter answered with an empty list, so
    // the spread silently contributed no elements at all.
    let source = r#"
import { test, expect } from "vitest";

function sumBy<T>(items: readonly T[], getValue: (item: T) => number): number {
  let total = 0;
  for (const item of items) {
    total += getValue(item);
  }
  return total;
}

test("summing a spread concatenation equals the sum of the parts", () => {
  const first: Array<{ a: number }> = [];
  const second = [{ a: 1 }, { a: 2 }, { a: 3 }];
  expect(sumBy(first, x => x.a) + sumBy(second, x => x.a)).toBe(sumBy([...first, ...second], x => x.a));
  expect(sumBy([...first, ...second], x => x.a)).toBe(6);
});

test("both operands of the spread contribute their elements", () => {
  const left = [{ a: 10 }];
  const right = [{ a: 5 }, { a: 1 }];
  expect([...left, ...right].length).toBe(3);
  expect(sumBy([...left, ...right], x => x.a)).toBe(16);
});
"#;
    run_fixture(source, "smelt_argument_spread_concat");
}

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_property_read_uses_the_source_spelling_even_when_a_declaration_shares_it() {
    // `function Foo` renders as the Rust identifier `foo`, which is also the
    // rendering of the property `foo`. Interning on the rendering made them one
    // symbol, and the declaration -- lowered last -- donated its spelling, so
    // the erased `.foo` reads looked up the key `"Foo"`, found nothing, and
    // compared `undefined === undefined`: the comparator accepted every pair.
    let source = r#"
import { test, expect } from "vitest";

function Foo(this: any, value: unknown) {
  this.value = value;
}

function sameFoo(x: unknown, y: unknown): boolean {
  return (x as any).foo === (y as any).foo;
}

function intersectionWith<T>(first: readonly T[], second: readonly T[], matches: (a: T, b: T) => boolean): T[] {
  return first.filter(a => second.some(b => matches(a, b)));
}

test("an erased property read finds the key the source wrote", () => {
  expect(sameFoo({ foo: 1 }, { foo: 1 })).toBe(true);
  expect(sameFoo({ foo: 1 }, { foo: 2 })).toBe(false);
  // A declaration named `Foo` exists in this module and must not rename `foo`.
  expect(typeof Foo).toBe("function");
});

test("a comparator reading a property discriminates instead of accepting everything", () => {
  const kept = intersectionWith([{ foo: 1 }, { foo: 2 }], [{ foo: 1 }, { foo: 3 }], (a, b) => a.foo === b.foo);
  expect(kept.length).toBe(1);
  expect(kept[0].foo).toBe(1);
});
"#;
    run_fixture(source, "smelt_property_spelling_identity");
}
