//! Runtime execution tests for H42: one render position, ONE answer about
//! whether a type parameter is spellable as a Rust generic.
//!
//! A value rendered at a destination that mentions a type parameter has more
//! than one side. A map-and-collect renders the entries AND the
//! `collect::<..>()` turbofish; a list rebuild renders the elements AND the
//! destination's annotation. Those sides used to derive the erasure rule
//! independently, at whatever depth they were reached, and they derived it
//! differently:
//!
//! * the coercion path (`value_at_type_text`) erased a bare `Type::TypeParam`
//!   target unconditionally, at any depth;
//! * the recovery path (`extract_value_text`, reached when the entry's source is
//!   already `SmeltUnknown`) kept the parameter wherever the emitted Rust item
//!   declares it.
//!
//! So a container whose entries took one path and whose annotation took the
//! other disagreed with itself, and no choice of rule for either side alone
//! could fix it — narrowing the annotation while leaving the coercion
//! unconditional was measured and made things strictly worse. The fix threads a
//! `RenderScope` through value rendering so the position decides once; see
//! `crates/smelt-codegen-rust/src/emitter/render_scope.rs` and
//! `blocker-logs/hono-h42-erasure-substitution.md`.
//!
//! # Why this is a runtime tier and not an end-to-end corpus fixture
//!
//! Every shape here needs a value that has ALREADY crossed into `unknown`: that
//! is what makes an entry take the recovery path while the container around it
//! takes the coercion path, which is the pairing the two rules disagreed about.
//! Source `unknown` is a genuine dynamic boundary under CLAUDE.md, but the
//! line-based classifier in `smelt-transpiler`'s `unknown_report` counts a local
//! declared `SmeltRecord<String, SmeltUnknown>` as avoidable erasure, and the
//! examples corpus is a hard `avoidable == 0` invariant. Measured: this fixture
//! contributes 12 such lines, all of them its own seeds. Same call as round 28
//! made for fixtures 80 and 85
//! (`blocker-logs/hono-round28-examples-invariant.md`): the erased subject moves
//! to an executing tier, where the assertion is the VALUE rather than the
//! spelling.
//!
//! The tier is `#[ignore]`d because it compiles and executes real crates:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test type_param_render_scope_runtime -- --ignored
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
        "generated type-parameter render-scope test failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Returns a unique scratch directory root for this test run.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "smelt-render-scope-runtime-{}-{seq}",
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
    // removed on the FAILURE path too. `SMELT_KEEP_RUNTIME_SCRATCH=1` keeps it
    // for a debugging session.
    if std::env::var_os("SMELT_KEEP_RUNTIME_SCRATCH").is_none() {
        drop(std::fs::remove_dir_all(&root));
    }
    if let Err(payload) = outcome {
        std::panic::resume_unwind(payload);
    }
}

/// The fixture program.
///
/// Lifted out of the test body so the assertion the tier makes stays
/// readable next to the `run_fixture` call rather than a hundred lines
/// below it.
const RENDER_SCOPE_FIXTURE: &str = r#"
import { test, expect } from "vitest";

type ParamMap = Record<string, number>;

function pick<T>(table: Record<string, T[]>, key: string): T[] {
  const found = table[key];
  return found === undefined ? [] : found;
}

class Registry<T> {
  flat: Record<string, T[]>;
  nested: Record<string, Record<string, [T, ParamMap][]>>;
  rows: [T, ParamMap][];

  constructor(seed: T) {
    const flatSeed: Record<string, unknown> = { seeded: [seed] };
    const nestedSeed: Record<string, unknown> = {
      seeded: { '/seed': [[seed, { index: 0 }]] },
    };
    const rowSeed: unknown[][] = [[seed, { index: 0 }]];
    this.flat = flatSeed as Record<string, T[]>;
    this.nested = nestedSeed as Record<string, Record<string, [T, ParamMap][]>>;
    this.rows = rowSeed as [T, ParamMap][];
  }

  addRow(handler: T, index: number): void {
    const params: ParamMap = { index: index };
    this.rows.push([handler, params]);
  }

  addFlat(key: string, handler: T): void {
    const list = this.flat[key];
    if (list === undefined) {
      this.flat[key] = [handler];
    } else {
      list.push(handler);
    }
  }

  addNested(group: string, path: string, handler: T, index: number): void {
    const params: ParamMap = { index: index };
    const existingTable = this.nested[group];
    const table: Record<string, [T, ParamMap][]> =
      existingTable === undefined ? {} : existingTable;
    const existingList = table[path];
    const list: [T, ParamMap][] = existingList === undefined ? [] : existingList;
    list.push([handler, params]);
    table[path] = list;
    this.nested[group] = table;
  }

  flatFor(key: string): T[] {
    return pick(this.flat, key);
  }

  rowIndexes(): number[] {
    return this.rows.map((row) => row[1]['index']);
  }

  nestedPaths(group: string): string[] {
    const table = this.nested[group];
    if (table === undefined) {
      return [];
    }
    return Object.keys(table).sort();
  }

  nestedSize(group: string, path: string): number {
    const table = this.nested[group];
    if (table === undefined) {
      return 0;
    }
    const list = table[path];
    return list === undefined ? 0 : list.length;
  }
}

test("a generic class rebuilds the same type parameter on every side", () => {
  const registry = new Registry<string>('root');
  registry.addFlat('get', 'a');
  registry.addFlat('get', 'b');
  registry.addFlat('post', 'c');
  registry.addRow('x', 1);
  registry.addRow('y', 2);
  registry.addNested('all', '/users', 'u', 3);
  registry.addNested('all', '/users', 'v', 4);
  registry.addNested('all', '/posts', 'p', 5);

  // (1) the seeded entry survives the recovery as a real `string`, and the
  //     table the generic `pick` reads is the same one the coercion built.
  expect(registry.flatFor('seeded').join(',')).toBe('root');
  expect(registry.flatFor('get').join(',')).toBe('a,b');
  expect(registry.flatFor('post').join(',')).toBe('c');
  expect(registry.flatFor('put').join(',')).toBe('');

  // (3) the tuple's first slot recovered as `T`, not as an erased tag, so the
  //     seeded row sits beside the two concretely built ones.
  expect(registry.rowIndexes().join(',')).toBe('0,1,2');

  // (2) the doubly nested rebuild kept both levels AND the tuple inside them.
  expect(registry.nestedPaths('seeded').join(',')).toBe('/seed');
  expect(registry.nestedPaths('all').join(',')).toBe('/posts,/users');
  expect(registry.nestedSize('all', '/users')).toBe(2);
  expect(registry.nestedSize('all', '/posts')).toBe(1);
  expect(registry.nestedSize('seeded', '/seed')).toBe(1);
});
"#;

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_generic_class_rebuilds_the_same_type_parameter_on_every_side() {
    // One generic class stores the same `T` three ways, and each way pairs a
    // container render with an entry render:
    //
    //   1. `Record<string, T[]>` from a table of erased values — a
    //      map-and-collect whose `collect::<..>()` turbofish must name what the
    //      recovered entries actually hold (was `E0277`: a
    //      `SmeltRecord<String, SmeltList<SmeltUnknown>>` built from an iterator
    //      over `(String, SmeltList<T>)`);
    //   2. `Record<string, Record<string, [T, ParamMap][]>>` from the same
    //      shape, so the rebuild recurses twice before it reaches `T` — the
    //      round-15 nested record-of-record case;
    //   3. `[T, ParamMap][]` from a list of LISTS, so the list-to-tuple element
    //      rebuild has to spell `T` in the tuple's first slot (was `E0308`:
    //      `SmeltList<(T, ..)>` against `SmeltList<(SmeltUnknown, ..)>`).
    //
    // `pick` is the fourth side: a generic free function taking
    // `Record<string, T[]>`. Whichever signature the crate-wide
    // generic-emission gate decides to emit for it, the argument has to be
    // rendered to match — the caller's own `T` must not leak across the seam
    // into a callee that erased its parameter (8 `E0308`s in Hono's router when
    // the caller's lexical scope was used at that position).
    //
    // The expected values are Node 22's, byte for byte.
    run_fixture(RENDER_SCOPE_FIXTURE, "type_param_render_scope");
}

/// The tuple-instantiation fixture.
///
/// Kept beside `RENDER_SCOPE_FIXTURE` for the same reason: the assertion the
/// tier makes should read next to its `run_fixture` call.
const TUPLE_RECOVERY_FIXTURE: &str = r#"
import { test, expect } from "vitest";

type Route = [string, number];

function pick<T>(table: Record<string, T[]>, key: string): T[] {
  const found = table[key];
  return found === undefined ? [] : found;
}

class TupleRegistry<T> {
  flat: Record<string, T[]>;

  constructor(seed: T) {
    const flatSeed: Record<string, unknown> = { seeded: [seed] };
    this.flat = flatSeed as Record<string, T[]>;
  }

  add(key: string, value: T): void {
    const list = this.flat[key];
    if (list === undefined) {
      this.flat[key] = [value];
    } else {
      list.push(value);
    }
  }

  entriesFor(key: string): T[] {
    return pick(this.flat, key);
  }
}

function render(entries: Route[]): string {
  return entries.map((entry) => entry[0] + ':' + entry[1]).join(',');
}

test("a type parameter instantiated at a tuple round-trips through the carrier", () => {
  const root: Route = ['root', 0];
  const first: Route = ['a', 1];
  const second: Route = ['b', 2];
  const third: Route = ['c', 3];
  const registry = new TupleRegistry<Route>(root);
  registry.add('get', first);
  registry.add('get', second);
  registry.add('post', third);

  // The seeded entry is the one that crossed into `unknown` and back: it can
  // only come out as a real `[string, number]` if the recovery names a tuple.
  expect(render(registry.entriesFor('seeded'))).toBe('root:0');
  expect(render(registry.entriesFor('get'))).toBe('a:1,b:2');
  expect(render(registry.entriesFor('post'))).toBe('c:3');
  expect(registry.entriesFor('delete').length).toBe(0);
});
"#;

#[test]
#[ignore = "slow: emits and runs a generated test crate; run in CI via --ignored"]
fn a_type_parameter_instantiated_at_a_tuple_round_trips_through_the_carrier() {
    // The same render-scope recovery as the test above, with `T` instantiated at
    // a TUPLE. The recovery arm the scope selects renders
    // `<T as SmeltFromUnknown>::smelt_from_unknown(..)`, so it can only compile
    // where the type `T` is instantiated at implements that trait — and a tuple
    // did not. `IntoSmeltUnknown for (A, B)` had existed since tuples were first
    // erased; the inverse had not, so the round trip was one-way and a generic
    // whose argument is a tuple failed with
    // `the trait bound `(SmeltUnknown, RouterRoute): SmeltFromUnknown` is not
    // satisfied` (3 of them in Hono, plus an `E0599` that fell out with them).
    //
    // A missing impl on one side of a round trip is not a reason to erase the
    // value on the other, which is why the fix is the impl rather than a
    // narrower recovery rule. The assertion is the VALUE — a seeded
    // `['root', 0]` that crossed into `unknown` and came back — so a recovery
    // that merely type-checked while losing the payload would still fail here.
    //
    // The expected values are Node 22's, byte for byte.
    run_fixture(TUPLE_RECOVERY_FIXTURE, "type_param_tuple_recovery");
}
