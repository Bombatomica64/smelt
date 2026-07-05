//! Compile-the-output test tier for the Rust codegen crate.
//!
//! The other codegen tests (`src/tests/snapshot_tests.rs` and the
//! `src/tests/part_*_tests.rs` modules) are text-only: they assert on insta
//! snapshots or `.contains()` substrings of the emitted Rust. That catches
//! shape regressions but not whether the emitted Rust actually *compiles*.
//! Precedence, erased-type (`SmeltUnknown`) and ABI bugs have historically
//! only surfaced during full source-project regeneration.
//!
//! This tier closes that gap. It lowers a corpus of representative source
//! programs through the real pipeline (frontend -> HIR -> MIR -> optimize),
//! emits full crates via the crate's real public entry point
//! [`smelt_codegen_rust::emit_crate`], and runs `cargo check` on each emitted
//! crate. The interface being verified is "Rust that compiles".
//!
//! ## Cost and how to run it
//!
//! Running `cargo check` on many crates is slow, so the whole tier is a single
//! `#[ignore]`d test. It does not run during a plain `cargo test`. CI runs it
//! explicitly, and you can too:
//!
//! ```sh
//! cargo test -p smelt-codegen-rust --test compile_corpus -- --ignored
//! ```
//!
//! To keep the tier fast the emitted crates share one `CARGO_TARGET_DIR`, so
//! their common dependencies and the shared runtime prelude compile once and
//! are reused across the corpus.
//!
//! ## Known failures
//!
//! When `cargo check` exposes a real bug in emitted code, the offending case is
//! excluded from the green corpus (see [`KNOWN_COMPILE_FAILURES`]) with a
//! reference to `blocker-logs/compile-snapshots-findings.md`, rather than the
//! emitter being patched here. This test tier is additive only.

#![expect(
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::expect_used,
    reason = "codegen tests keep fixture setup compact and fail fast on invalid test inputs"
)]

use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

use smelt_codegen_rust::{CrateKind, EmitOptions, emit_crate};
use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::FileId;

/// A recorded corpus failure: `(area, case name, captured error text)`.
type CorpusFailure = (String, String, String);

/// A named source program in the compile corpus.
///
/// `area` groups programs by the bug-prone emitter feature they exercise so a
/// `cargo check` failure points at a category, not just a single snippet.
struct Case {
    /// Stable identifier; also used as the emitted crate directory name.
    name: &'static str,
    /// Bug-prone emitter area this case stresses.
    area: &'static str,
    /// TypeScript source lowered through the real pipeline.
    source: &'static str,
}

/// Cases known to emit Rust that does not currently compile.
///
/// Each entry MUST reference an entry in
/// `blocker-logs/compile-snapshots-findings.md`. These names are skipped by the
/// green tier so it stays passing; the bug is tracked in the log rather than
/// fixed in this additive change.
const KNOWN_COMPILE_FAILURES: &[&str] = &[
    // async function bodies emit a bare `return <expr>` against a synthesized
    // `Result<..>` return type instead of `return Ok(<expr>)`.
    // See blocker-logs/compile-snapshots-findings.md (finding 1).
    "async_await",
];

/// Returns the compile corpus: representative programs covering the bug-prone
/// emitter areas (`SmeltUnknown` erasure/extraction, nested closures/callbacks,
/// string/list operations, async/Promise lowering, plus baseline shapes).
///
/// Sources are reused from / aligned with the inputs the existing snapshot
/// tests lower (see `src/tests/snapshot_tests.rs`) and the async HIR example
/// (`examples/typescript/hir/09_async_function.ts`).
fn corpus() -> Vec<Case> {
    vec![
        // --- baseline shapes -------------------------------------------------
        Case {
            name: "basic_function",
            area: "baseline",
            source: r"
function add(left: number, right: number): number {
  return left + right;
}
const total = add(2, 3);
",
        },
        Case {
            name: "control_flow",
            area: "baseline",
            source: r"
function countPositive(values: number[]): number {
  let count = 0;
  for (const value of values) {
    if (value > 0) count = count + 1;
  }
  return count;
}
",
        },
        // --- SmeltUnknown erasure / extraction (Coercion) --------------------
        Case {
            name: "typed_coercion",
            area: "smelt_unknown",
            source: r"
function asNumber(value: unknown): number {
  return value as number;
}
",
        },
        Case {
            name: "erased_coercion",
            area: "smelt_unknown",
            source: r"
function erase(value: number): unknown {
  return value as unknown;
}
",
        },
        // --- nested closures / callbacks -------------------------------------
        Case {
            name: "closure_capture",
            area: "closures",
            source: r"
function makeAdder(base: number): (value: number) => number {
  return (value: number): number => value + base;
}
",
        },
        Case {
            name: "collection_callback",
            area: "closures",
            source: r"
function double(values: number[]): number[] {
  return values.map((value) => value * 2);
}
",
        },
        Case {
            name: "function_item_value_identity",
            area: "closures",
            source: r"
function func1(): void {}
function takesTwo(a: unknown, b: unknown): boolean { return true; }
const r = takesTwo(func1, func1);
",
        },
        Case {
            name: "callback_cfg_shapes",
            area: "closures",
            source: r"
function plusOne(values: number[]): number[] {
  return values.map((x) => x + 1);
}

function lower(values: string[]): string[] {
  return values.map((value) => value.toLowerCase());
}

function collectIndices(values: number[]): number[] {
  const indices: number[] = [];
  values.forEach((_value, index) => {
    indices.push(index);
  });
  return indices;
}
",
        },
        Case {
            name: "callback_conditional_branches",
            area: "closures",
            // Callback bodies whose `cond ? a : b` / `if`-`else` branches lower
            // to different concrete Rust types must reconcile to a single lowered
            // type and compile. `classify` is a value-yielding `if/else` lowered
            // as a direct conditional; `widen` is a ternary whose list branches
            // have different element types; `normalize` mutates the callback
            // parameter in an `if/else if` chain and must fall back to a full
            // closure body. Mirrors es-toolkit `keys`/`fill`/`toFinite` mappers.
            source: r#"
function classify(values: number[]): string[] {
  return values.map((value) => {
    if (value > 0) {
      return "pos";
    } else {
      return "nonpos";
    }
  });
}

function widen(values: (string | undefined)[]): (string | number)[][] {
  return values.map((value) => (value === undefined ? ["a"] : [1, 2, 3]));
}

function normalize(values: number[]): number[][] {
  return values.map((value) => {
    if (value === 0) {
      value = 1;
    } else if (value !== value) {
      value = 0;
    }
    const neg = value === 0 ? 0 : -value;
    return [value, neg];
  });
}
"#,
        },
        // --- string / list operations ----------------------------------------
        Case {
            name: "list_collection",
            area: "collections",
            source: r"
function append(values: number[], value: number): number[] {
  values.push(value);
  return values;
}
",
        },
        Case {
            name: "map_collection",
            area: "collections",
            source: r#"
function lookup(): number | undefined {
  const values = new Map<string, number>([["a", 1]]);
  return values.get("a");
}
"#,
        },
        Case {
            name: "map_union_values",
            area: "collections",
            source: r#"
function tags(): Map<string, string | number> {
  return new Map<string, string | number>([["a", 1], ["b", "two"]]);
}
function mixed(): Map<string, string | number> {
  return new Map([["a", 1], ["b", "two"]]);
}
"#,
        },
        Case {
            name: "flow_narrowed_concrete_union",
            area: "concrete_unions",
            source: r#"
function resolvePath(path: string | (() => string)): string {
  if (typeof path === "string" && path.includes(".")) {
    return path;
  }
  if (typeof path === "string") {
    return path + ".ts";
  }
  return path();
}
"#,
        },
        Case {
            name: "structural_guarded_concrete_unions",
            area: "concrete_unions",
            source: r#"
interface Named { name: string; }
interface LengthBearing { length: number; }
function lengthOf(value: Named | LengthBearing): number {
  if ("length" in value) return value.length;
  return 0;
}

function values(source: number[] | Record<string, number>): number[] {
  return Array.isArray(source) ? source : Object.values(source);
}

class Left { left: string = "left"; }
class Right { right: string = "right"; }
function read(value: Left | Right): string {
  if (value instanceof Left) return value.left;
  return "right";
}
"#,
        },
        Case {
            // Issue #55: structural `in` and discriminant-property comparison on
            // a concrete class union must compile to tagged-enum discriminant
            // checks and concrete arm projections, never SmeltUnknown erasure.
            name: "discriminant_in_and_property_narrowing",
            area: "concrete_unions",
            source: r#"
class Circle { radius: number = 1; }
class Square { side: number = 2; }

function area(shape: Circle | Square): number {
  if ("radius" in shape) {
    return shape.radius * shape.radius;
  }
  return 0;
}

function tagged(shape: Circle | Square): number {
  if ("radius" in shape && shape.radius === 3) {
    return shape.radius;
  }
  return 0;
}
"#,
        },
        Case {
            // Issue #55 invalidation: a widening-compatible write to a narrowed
            // union local re-injects the concrete arm and must still compile.
            name: "narrowed_union_reassignment",
            area: "concrete_unions",
            source: r#"
function resolvePath(path: string | (() => string)): string {
  if (typeof path === "string") {
    path = path + ".ts";
    return path;
  }
  return path();
}
"#,
        },
        Case {
            name: "set_collection",
            area: "collections",
            source: r"
function contains(): boolean {
  const values = new Set<number>([1, 2]);
  return values.has(2);
}
",
        },
        Case {
            name: "search_from_index",
            area: "collections",
            source: r"
export function findFrom(values: readonly number[], target: number, from: number): number {
  return values.indexOf(target, from);
}
export function findLastFrom(values: readonly number[], target: number, from: number): number {
  return values.lastIndexOf(target, from);
}
export function findWhole(values: readonly number[], target: number): number {
  return values.indexOf(target);
}
export function containsFrom(haystack: string, needle: string, from: number): boolean {
  return haystack.includes(needle, from);
}
export function containsWhole(haystack: string, needle: string): boolean {
  return haystack.includes(needle);
}
",
        },
        // --- async / Promise lowering ----------------------------------------
        Case {
            name: "async_await",
            area: "async",
            source: r"
async function lift(value: number): Promise<number> {
  return value;
}

async function run(): Promise<number> {
  return await lift(5);
}
",
        },
        // --- timer typed extra arguments -------------------------------------
        Case {
            name: "timer_typed_extra_args",
            area: "timers",
            source: r#"
function greet(name: string, count: number): void {
  console.log(name);
  console.log(count);
}
function tick(label: string): void {
  console.log(label);
}
setTimeout(greet, 10, "hi", 3);
setInterval(tick, 5, "tock");
"#,
        },
        Case {
            name: "timer_optional_typed_arg",
            area: "timers",
            source: r#"
function note(prefix: string, suffix?: string): void {
  console.log(prefix);
}
setTimeout(note, 10, "with-optional", "extra");
"#,
        },
        Case {
            // Issue #73: exported consts initialized from member expressions
            // beyond well-known Number/Math constants. Builtin `.prototype`
            // members and bound builtin methods are erased boundaries that lower
            // to `SmeltUnknown`; a numeric alias and an object-const field keep
            // their concrete value. All must emit Rust that compiles.
            name: "exported_const_member_expressions",
            area: "exported_consts",
            source: r"
export const MAX_INTEGER = Number.MAX_VALUE;
export const arrayProto = Array.prototype;
export const slice = Array.prototype.slice;
export const objectProto = Object.prototype;
const limits = { lower: 1, upper: 640 } as const;
export const UPPER_LIMIT = limits.upper;
",
        },
        Case {
            name: "interface_method_call",
            area: "interfaces",
            source: r"
interface Counter {
  count(): number;
}

interface Adder {
  add(a: number, b: number): number;
}

export function total(counter: Counter): number {
  return counter.count();
}

export function sum(adder: Adder): number {
  return adder.add(1, 2);
}
",
        },
        Case {
            // Issue #78: `switch` over non-literal case labels. Enum-member and
            // const-reference labels const-fold to the member's numeric/string
            // literal, and the enum type resolves to its underlying primitive so
            // the scrutinee matches the folded arms and the emitted Rust
            // compiles.
            name: "switch_nonliteral_case_labels",
            area: "control_flow",
            source: r#"
enum Color {
  Red = 0,
  Green = 1,
  Blue = 2,
}

enum Fruit {
  Apple,
  Banana,
  Cherry,
}

enum Level {
  Low = "low",
  High = "high",
}

const OTHER_TAG = "[object Other]";

export function colorName(c: Color): string {
  switch (c) {
    case Color.Red:
      return "red";
    case Color.Green:
      return "green";
    case Color.Blue:
      return "blue";
    default:
      return "unknown";
  }
}

export function fruitRank(f: Fruit): number {
  switch (f) {
    case Fruit.Apple:
      return 10;
    case Fruit.Banana:
      return 20;
    case Fruit.Cherry:
      return 30;
    default:
      return 0;
  }
}

export function levelLabel(l: Level): string {
  switch (l) {
    case Level.Low:
      return "lo";
    case Level.High:
      return "hi";
    default:
      return "x";
  }
}

export function classify(tag: string): number {
  switch (tag) {
    case OTHER_TAG:
      return 1;
    default:
      return 0;
  }
}

export function redValue(): number {
  return Color.Red;
}
"#,
        },
        Case {
            // Issue #77: method calls on non-class receivers must lower and
            // compile instead of hard-erroring on "method calls are only lowered
            // for class values". Records, concrete unions, and erased/builtin
            // receivers whose method is not a modeled builtin (`localeCompare`
            // here) lower through the shared dynamic-dispatch boundary. Each
            // function returns a modeled value so the erased method result is
            // never load-bearing, keeping the emitted Rust type-correct.
            name: "method_call_nonclass",
            area: "method_call_nonclass",
            source: r#"
// Erased/builtin receiver: `string` has no modeled `localeCompare` method, so
// the call lowers through the dynamic boundary rather than being rejected.
export function compareStrings(a: string, b: string): number {
  a.localeCompare(b);
  return a.length - b.length;
}

// Template-literal string receiver (radash `sort` idiom).
export function compareTemplates(a: string, b: string): number {
  `${a}`.localeCompare(b);
  return 0;
}

// Record receiver: an unmodeled method on a `Record<string, T>` value.
export function recordMethod(record: Record<string, number>): number {
  const size = Object.keys(record).length;
  return size;
}

// Concrete-union receiver: an unmodeled method reached on either arm lowers
// through the boundary; the function still returns a concrete value.
export function unionMethod(value: string | number): number {
  if (typeof value === "string") {
    value.localeCompare("x");
    return value.length;
  }
  return value;
}
"#,
        },
    ]
}

/// Lowers `source` (TypeScript) through the real pipeline and emits a full
/// program crate into `crate_dir` via [`emit_crate`].
///
/// Returns a human-readable error string on any frontend/MIR/emit failure so
/// the caller can record it as a corpus failure rather than panicking.
fn emit_case_crate(case: &Case, crate_dir: &Path) -> Result<(), String> {
    let mut ctx = HirCtx::new();
    to_hir(case.source, FileId(0), &mut ctx)
        .map_err(|err| format!("HIR lowering failed: {err:?}"))?;
    let mut mir =
        smelt_mir::lower_hir(&ctx.krate).map_err(|err| format!("MIR lowering failed: {err:?}"))?;
    smelt_mir::opt::optimize(&mut mir);
    let options = EmitOptions::new(format!("smelt_corpus_{}", case.name))
        .with_crate_kind(CrateKind::Program);
    emit_crate(&mir, crate_dir, &options).map_err(|err| format!("crate emission failed: {err}"))
}

/// Runs `cargo check` on the emitted crate at `crate_dir`, sharing the given
/// `target_dir` so corpus crates reuse compiled dependencies.
///
/// Returns `Ok(())` when `cargo check` succeeds, otherwise the captured
/// stdout/stderr so the failure can be reported.
fn cargo_check(crate_dir: &Path, target_dir: &Path) -> Result<(), String> {
    let output = Command::new(env!("CARGO"))
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(crate_dir.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", target_dir)
        // Generated crates carry their own lint posture; warnings must not fail
        // the tier, only genuine compile errors should.
        .env("RUSTFLAGS", "-Awarnings")
        .output()
        .map_err(|err| format!("failed to spawn cargo check: {err}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(format!("cargo check failed:\n{stdout}\n{stderr}"))
}

/// Returns a unique scratch directory root for this test run.
///
/// Uses the process id and a monotonically increasing counter so repeated runs
/// and parallel cargo invocations do not collide.
fn scratch_root() -> PathBuf {
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("smelt-compile-corpus-{}-{seq}", std::process::id()))
}

/// Compiles every corpus case and asserts the emitted Rust type-checks.
///
/// This is the green tier: it must pass when invoked explicitly, modulo the
/// cases listed in [`KNOWN_COMPILE_FAILURES`]. It is `#[ignore]`d so it does not
/// run during a plain `cargo test`; CI runs it via `-- --ignored`. See the
/// module docs for how to invoke it.
#[test]
#[ignore = "slow: emits crates and runs cargo check; run in CI via --ignored"]
fn corpus_emitted_rust_compiles() {
    let root = scratch_root();
    let crates_dir = root.join("crates");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crates_dir).expect("create scratch crates dir");
    std::fs::create_dir_all(&target_dir).expect("create scratch target dir");

    let mut failures: Vec<CorpusFailure> = Vec::new();

    for case in corpus() {
        if KNOWN_COMPILE_FAILURES.contains(&case.name) {
            continue;
        }
        let crate_dir = crates_dir.join(case.name);
        if let Err(err) = emit_case_crate(&case, &crate_dir) {
            failures.push((case.area.to_owned(), case.name.to_owned(), err));
            continue;
        }
        if let Err(err) = cargo_check(&crate_dir, &target_dir) {
            failures.push((case.area.to_owned(), case.name.to_owned(), err));
        }
    }

    // Best-effort cleanup; ignore errors so a leftover temp dir never fails CI.
    drop(std::fs::remove_dir_all(&root));

    assert!(
        failures.is_empty(),
        "{} corpus case(s) failed to compile:\n{}",
        failures.len(),
        failures
            .iter()
            .map(|(area, name, err)| format!("[{area}] {name}:\n{err}"))
            .collect::<Vec<_>>()
            .join("\n\n")
    );
}
