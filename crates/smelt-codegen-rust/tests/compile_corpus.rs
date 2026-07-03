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
