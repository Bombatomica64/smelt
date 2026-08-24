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
//! ## Two corpora
//!
//! There are two tiers in this file, one `#[ignore]`d test each (a third,
//! `tests/shape_grid.rs`, *generates* its corpus instead of storing it, and
//! shares this file's harness through `tests/corpus_support/`):
//!
//! * [`corpus_emitted_rust_compiles`] — the inline [`Case`] corpus below, whose
//!   sources are `&'static str` literals.
//! * [`callback_generics_fixtures_compile`] — the rescued callback-generics
//!   fixtures, read from `tests/fixtures/callback_generics/*.ts` at run time.
//!   See that directory's `README.md`. Unlike the inline corpus, that tier
//!   records its known failures with error counts and causes and fails in both
//!   directions (a recorded failure that starts compiling is a failure too).
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
    fmt::Write as _,
    path::{Path, PathBuf},
};

#[cfg(feature = "ty")]
use smelt_codegen_rust::{CrateKind, EmitOptions, emit_crate};
#[cfg(feature = "ty")]
use smelt_hir::FileId;

mod corpus_support;

use corpus_support::{cargo_check, emit_typescript_crate, rustc_error_count, scratch_root};

// The Python corpus is only built with the `ty` feature, where annotation-free
// Python resolves its types via `ty` (issue #93).
#[cfg(feature = "ty")]
use smelt_frontend_py::{HirCtx as PyHirCtx, to_hir_with_path as py_to_hir_with_path};

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
        Case {
            // Regression: a `switch` whose case bodies fall through and use a
            // bare `break` to rejoin shared post-switch code, inside a fallible
            // (`throw`-capable) function. The structured-control-flow emitter
            // reconstructs the fallthrough as a labeled block; the `break` path
            // must still reach the shared continuation so the reconstructed
            // region diverges (every path returns) instead of leaving a `()`
            // value in the function's tail position (E0308). Mirrors
            // es-toolkit `compat/math/random.ts`.
            name: "switch_fallthrough_break_shared_continuation",
            area: "baseline",
            source: r"
function pick(...args: any[]): number {
  let lo = 0;
  let hi = 1;
  let flag = false;
  switch (args.length) {
    case 1: {
      if (typeof args[0] === 'boolean') {
        flag = args[0];
      } else {
        hi = args[0];
      }
      break;
    }
    case 2: {
      if (typeof args[1] === 'boolean') {
        hi = args[0];
        flag = args[1];
        break;
      } else {
        lo = args[0];
        hi = args[1];
      }
    }
    // eslint-disable-next-line no-fallthrough
    case 3: {
      lo = args[0];
      hi = args[1];
      flag = args[2];
    }
  }
  if (lo > hi) {
    throw new Error('bad range');
  }
  if (flag) {
    return lo + hi;
  }
  return hi - lo;
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
            name: "math_extrema_spread",
            area: "numeric",
            // `Math.max`/`Math.min` over a spread list reduce the list rather
            // than treating it as a single scalar operand. The reduction must
            // coerce each element (here an erased length result) to `f64`.
            // Mirrors es-toolkit `zipWith`/`unzipWith`.
            source: r"
function widestRow(rows: number[][]): number {
  return Math.max(...rows.map((row) => row.length));
}

function boundedMin(values: number[]): number {
  return Math.min(0, ...values);
}
",
        },
        Case {
            name: "timer_typed_callback",
            area: "closures",
            // A `setTimeout` whose callback has a statically-known function type
            // and forwarded arguments must erase the callback to the
            // `SmeltUnknown` callable boundary before the dispatch probe rather
            // than assuming it is already erased. Mirrors es-toolkit `delay`.
            source: r"
function schedule(func: (...args: unknown[]) => unknown, wait: number, ...args: unknown[]): number {
  return setTimeout(func, wait, ...args);
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
        Case {
            name: "non_arrow_array_callbacks",
            area: "closures",
            // Array methods must accept non-arrow callback forms (issue #86):
            // a `function` expression callback (`mapped`), a `function`
            // expression whose body needs full closure-body lowering because it
            // uses a statement form the compact callback IR cannot model
            // (`fallback`, a `try/catch`), a named function-item reference
            // (`byRef` calling `square`), and a local function-typed variable
            // handed to the method (`byLocal`). Each must lower into the callback
            // closure with its typed signature preserved, and the emitted Rust
            // must compile.
            source: r"
function square(value: number): number {
  return value * 2;
}

function mapped(values: number[]): number[] {
  return values.map(function (value) {
    return value + 1;
  });
}

function fallback(values: string[]): Array<string | undefined> {
  return values.map(function (value) {
    try {
      return value;
    } catch (error) {
      return undefined;
    }
  });
}

function byRef(values: number[]): number[] {
  return values.map(square);
}

function byLocal(values: number[]): number[] {
  const transform = (value: number): number => value * 3;
  return values.map(transform);
}
",
        },
        Case {
            name: "reduce_callback_return_reconciliation",
            area: "closures",
            // A named/opaque `reduce` callback whose declared return type is not
            // identical to the initial value's type but statically reconciles
            // with it must not be rejected with "array reduce callback returns an
            // unsupported type" (issue #113). TypeScript threads a single
            // accumulator type `U` through `reduce<U>`, so the accumulator widens
            // to the callback's return type and the seed is coerced into it. Each
            // form must emit a `fold` that compiles: `intoUnion` seeds a `0`
            // number into a `string | number` concrete-union accumulator;
            // `intoRecord`/`intoList` seed an empty object/array into a wider
            // container; `intoOptional` seeds `undefined` into an optional; and
            // `intoUnknown` widens a concrete seed into an erased accumulator.
            source: r#"
function unionStep(acc: string | number, value: number): string | number {
  return acc;
}
function intoUnion(values: number[]): string | number {
  return values.reduce(unionStep, 0);
}

function recordStep(acc: Record<string, number>, value: string): Record<string, number> {
  acc[value] = 1;
  return acc;
}
function intoRecord(values: string[]): Record<string, number> {
  return values.reduce(recordStep, {});
}

function listStep(acc: (string | number)[], value: number): (string | number)[] {
  acc.push(value);
  return acc;
}
function intoList(values: number[]): (string | number)[] {
  return values.reduce(listStep, []);
}

function optionalStep(acc: number | undefined, value: number): number | undefined {
  return acc;
}
function intoOptional(values: number[]): number | undefined {
  return values.reduce(optionalStep, undefined);
}

function unknownStep(acc: number, value: number): unknown {
  return acc + value;
}
function intoUnknown(values: number[]): unknown {
  return values.reduce(unknownStep, 0);
}

function narrowReturnStep(acc: string | number, value: number): number {
  return value;
}
function narrowReturn(values: number[]): string | number {
  return values.reduce(narrowReturnStep, "seed");
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
            // A class field whose declared type is a concrete union
            // (`string | number`) lowers the field to a tagged `SmeltUnion*`
            // enum. The struct derives `Clone, Debug, Default`, so the union
            // enum must provide `Debug` and `Default` too (an enum with
            // data-carrying variants can derive neither). Regression for the
            // union-field derive gap: the emitted crate must compile. The same
            // gap blocks union-valued class index signatures (issue #84).
            name: "class_union_field",
            area: "concrete_unions",
            source: r"
class Holder {
  value: string | number = 0;
}

export function getVal(h: Holder): string | number {
  return h.value;
}

export function makeHolder(): Holder {
  return new Holder();
}
",
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
        Case {
            // Issue #87: `.at(index)` accepts statically numeric-compatible index
            // types beyond an exact `number`. An optional-numeric index
            // (`number | undefined`) is coerced to the runtime `Float` the
            // optional-index path expects, and the emitted normalized-index
            // arithmetic must compile for both array and string receivers.
            name: "at_index_coercion",
            area: "collections",
            source: r"
export function pickOptional(values: number[], index: number | undefined): number | undefined {
  return values.at(index);
}
export function pickChar(text: string, index: number | undefined): string | undefined {
  return text.at(index);
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
            // Issue #84: class string index signatures `[key: string]: T`. A
            // class that is purely an index signature (`StringBag`) carries a
            // real runtime keyed store whose statically known value type drives
            // dynamic reads AND writes; a mixed class (`MixedBag`) keeps its
            // declared named field concretely typed alongside the index
            // signature. Keyed reads return the honest `Option<T>` (missing key
            // -> undefined), keyed writes insert into the store, and named access
            // stays concrete. The emitted Rust must compile. (The runtime
            // round-trip is asserted end-to-end by the CLI test
            // `build_round_trips_class_index_signature_keyed_store`.) Value types
            // here are concrete (`string`/`number`); a union index value type is
            // subject to the pre-existing union-field `Debug`/`Default` derive
            // gap that also affects plain union class fields.
            name: "class_index_signature",
            area: "classes",
            source: r"
class StringBag {
  [key: string]: string;
}

class MixedBag {
  size: number = 0;
  [key: string]: number;
}

export function readBag(bag: StringBag, key: string): string | undefined {
  return bag[key];
}

export function writeBag(bag: StringBag, key: string, value: string): void {
  bag[key] = value;
}

export function mixedSize(bag: MixedBag): number {
  return bag.size;
}

export function readMixed(bag: MixedBag, key: string): number | undefined {
  return bag[key];
}
",
        },
        Case {
            // Issue #96: statically-resolvable computed property names. A
            // `const`-keyed class field (`[TAG]`) folds to the const's string
            // value as a named field; an enum-member-keyed interface field
            // (`[Kind.First]`) folds to the enum member's value; a well-known
            // `[Symbol.iterator]` interface method resolves to the stable
            // synthetic member spelling. All three become ordinary named
            // members so the emitted Rust compiles, and reading the folded
            // class field back through its concrete member type stays concrete.
            //
            // Issue #115 extends the folding to more symbol-backed keys, all of
            // which must likewise become ordinary named members: a well-known
            // `[Symbol.asyncIterator]` method, an inline `[Symbol.for("k")]`
            // field, and a `Symbol.for("k")`-aliased-const key (`[matcher]`).
            // Registry symbols fold to the same stable synthetic spelling
            // regardless of how they are spelled, so `matcher` and the inline
            // `Symbol.for` key naming the same description name the same member.
            name: "computed_property_names",
            area: "interfaces",
            source: r#"
const TAG = "tag";
const matcher = Symbol.for("@ts-pattern/matcher");

enum Kind {
  First = "first",
}

class Tagged {
  [TAG]: string = "leaf";
  value: number = 0;
}

interface ByKind {
  [Kind.First]: number;
}

interface Seq {
  [Symbol.iterator](): number;
  [Symbol.asyncIterator](): number;
}

interface Matcher {
  [matcher](): number;
}

interface Override {
  [Symbol.for("@ts-pattern/override")]: number;
}

export function makeTagged(): Tagged {
  return new Tagged();
}

export function taggedValue(node: Tagged): number {
  return node.value;
}

export function firstOf(record: ByKind): number {
  return record.first;
}
"#,
        },
        Case {
            // Issue #98: static methods lower to receiver-free associated
            // functions (`Class::method(..)`) and static class constants lower
            // to materialized static fields resolvable via `Class.CONST`. Both
            // must round-trip through the pipeline and compile as Rust. The
            // static method is called qualified, and the static constants are
            // read qualified in a free function, exercising both the emitted
            // associated function and the concrete literal read paths.
            name: "class_static_members",
            area: "classes",
            source: r#"
class MathUtils {
  static readonly PI: number = 3.14;
  static readonly NAME: string = "math";

  static square(value: number): number {
    return value * value;
  }

  static clamp(value: number, low: number, high: number): number {
    if (value < low) return low;
    if (value > high) return high;
    return value;
  }
}

export function circleArea(radius: number): number {
  return MathUtils.square(radius) * MathUtils.PI;
}

export function boundedSquare(value: number): number {
  return MathUtils.clamp(MathUtils.square(value), 0, 100);
}

export function utilName(): string {
  return MathUtils.NAME;
}
"#,
        },
        Case {
            // Issue #97 / #18: optional class/data fields lower to `Option<T>`
            // with explicit construction. The optional field (`y?: number`)
            // becomes an `Option<f64>` struct slot; construction that supplies
            // the field passes `Some(..)` and construction that omits it passes
            // `None`, while the required field (`x`) stays concrete. Reading the
            // optional field through `??` consumes the `Option` directly. The
            // emitted Rust must compile. (The runtime round-trip is asserted
            // end-to-end by the CLI test `build_round_trips_optional_class_field`
            // and by the frontend/codegen regression tests.)
            name: "optional_class_field",
            area: "classes",
            source: r"
class Point {
  x: number;
  y?: number;
  constructor(x: number, y?: number) {
    this.x = x;
    this.y = y;
  }
  readY(): number {
    return this.y ?? -1;
  }
}

export function makePresent(): number {
  return new Point(1, 2).readY();
}

export function makeAbsent(): number {
  return new Point(3).readY();
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
        Case {
            // Issue #99: generic classes and interfaces lower to real Rust
            // generics rather than erasing their instantiations to
            // `SmeltUnknown`. `Container<T>` emits `struct Container<T>` with an
            // `impl<T: ..> Container<T>` whose methods return the parameter;
            // `Pair<A, B>` exercises multi-parameter generics; `Outcome<T, E>`
            // is a generic interface used at a concrete return position. The use
            // sites (`new Container<number>(3)`, `b.get()`, `p.getSecond()`)
            // must instantiate the class concretely (`Container<f64>`) and pass
            // arguments through so Rust monomorphizes, all of which must compile.
            name: "generic_classes_and_interfaces",
            area: "generics",
            source: r#"
class Container<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  get(): T { return this.value; }
  set(value: T): void { this.value = value; }
}

class Pair<A, B> {
  first: A;
  second: B;
  constructor(first: A, second: B) { this.first = first; this.second = second; }
  getFirst(): A { return this.first; }
  getSecond(): B { return this.second; }
}

interface Outcome<T, E> {
  ok: boolean;
  value: T;
  error: E;
}

export function useContainer(): number {
  const b = new Container<number>(3);
  b.set(5);
  return b.get();
}

export function usePair(): string {
  const p = new Pair<number, string>(1, "x");
  return p.getSecond();
}

export function makeOk(v: number): Outcome<number, string> {
  return { ok: true, value: v, error: "" };
}
"#,
        },
        Case {
            // Issue #99 (deferred piece of #102): generic FREE functions lower to
            // real Rust generics rather than erasing `T` to `SmeltUnknown`.
            // `identity<T>` emits `fn identity<T: ..>(x: T) -> T`; `first<T>`
            // exercises a type parameter nested in a `T[]` parameter; `pair<A, B>`
            // exercises multiple type parameters. The call sites (`identity(3)`,
            // `first([...])`, `pair(1, "x")`) pass concrete arguments through so
            // Rust monomorphizes each call, and the concrete results bind to the
            // callers' concrete return types — all of which must compile.
            name: "generic_free_functions",
            area: "generics",
            source: r#"
export function identity<T>(x: T): T {
  return x;
}

export function first<T>(xs: T[]): T {
  return xs[0];
}

export function pair<A, B>(first: A, second: B): B {
  return second;
}

export function useIdentity(): number {
  return identity(3);
}

export function useFirst(): number {
  return first([1, 2, 3]);
}

export function usePair(): string {
  return pair(1, "x");
}
"#,
        },
        Case {
            // Plan 197 Increment 0b: generic free functions whose return is a
            // *composite* built from their own type parameter (`T[]`, `T[][]`,
            // `T | undefined` lowering to `Option<T>`), plus a generic class
            // method with the same shape. Each call site pins `T` concretely, so
            // the emitted call passes its argument through unerased and takes the
            // result at the substituted return type. That only compiles if the
            // argument side and the return side agree: a monomorphized argument
            // with an erased return claim (or the reverse) is E0308, and a call
            // site that cannot pin every type parameter must demote wholesale —
            // `useDemoted` is that case, sharing one callee with `useTail`.
            name: "generic_composite_returns",
            area: "generics",
            source: r#"
class Holder<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  all(): T[] { return [this.value]; }
}

export function tail<T>(xs: T[]): T[] {
  return xs.slice(1);
}

export function nest<T>(xs: T[]): T[][] {
  return [xs];
}

export function last<T>(xs: T[]): T | undefined {
  return xs[xs.length - 1];
}

export function isShorter<T>(a: T[], b: T[]): boolean {
  return a.length < b.length;
}

export function useTail(): number[] {
  const data = [1, 2, 3];
  return tail(data);
}

export function useNest(): string[][] {
  const data = ["a", "b"];
  return nest(data);
}

export function useNestedElements(): number[][] {
  const data = [[1, 2], [3]];
  return tail(data);
}

export function useLast(): number | undefined {
  const data = [1, 2, 3];
  return last(data);
}

export function useIsShorter(): boolean {
  return isShorter([1], [2, 3]);
}

export function useDemoted(us: unknown[]): unknown[] {
  return tail(us);
}

export function useHolder(): number[] {
  const holder = new Holder<number>(7);
  return holder.all();
}
"#,
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
        Case {
            // Issue #83: field access beyond Record/class/interface. Driving an
            // erased `Iterable<unknown>` through the manual iterator protocol
            // reads `.done`/`.value` off the dynamic iterator result
            // (`iterator.next()`), whose element type is never statically
            // resolved. These reads route through the erased object-field
            // boundary rather than the field-access gate, and the emitted Rust
            // must compile (es-toolkit `fp/pipe.ts` shape).
            name: "erased_iterator_field_access",
            area: "field_access",
            source: r"
export function drive(data: Iterable<unknown>): unknown[] {
  const result: unknown[] = [];
  const iterator = data[Symbol.iterator]();
  let step = iterator.next();
  while (!step.done) {
    result.push(step.value);
    step = iterator.next();
  }
  return result;
}
",
        },
        Case {
            // Issue #114 (follow-up to #83/#84): dotted access to an *undeclared*
            // member of a class with an index signature used to hit the
            // `unknown class or interface field` gate. It now resolves through
            // the index signature's value type: a dotted read is a keyed lookup
            // into the runtime store (issue #84), typed as the index value `T`
            // rather than an erased `Unknown`, and the emitted Rust must
            // compile. Declared named members (`bag.size`) still use the
            // concrete struct access; only undeclared names route to the store.
            name: "undeclared_index_signature_field_access",
            area: "field_access",
            source: r"
class StringBag {
  size: number = 0;
  [key: string]: string | number;
}

export function readNamed(bag: StringBag): number {
  return bag.size;
}

export function readDynamic(bag: StringBag): string | number {
  return bag.anything;
}
",
        },
        // --- array literals with function / this / class elements ------------
        Case {
            name: "array_literal_expr_elements",
            area: "array_elements",
            // Array literal elements that are function expressions, `this`, and
            // class expressions (named and anonymous) route through the shared
            // expression lowering path. Function expressions become closure
            // values, `this` resolves to the method receiver, and class
            // expressions register a class and yield a class value.
            source: r"
export function funcElements(): unknown[] {
  return [function () { return 1; }, function () { return 2; }];
}

export function classElements(): unknown[] {
  return [class Named { value: number = 0; }, class { flag: boolean = false; }];
}

class Registry {
  id: number = 0;
  entries(): unknown[] {
    return [function () { return 0; }, this, class Entry { key: number = 0; }];
  }
}
",
        },
        // --- MIR-lowering gates cleared for the es-toolkit transpile ----------
        Case {
            // Assigning to an array's `length` resizes it. The shrink case
            // (`arr.length = n` with `n <= arr.length`) lowers to an in-place
            // truncating splice; before that it aborted MIR lowering with
            // "only local, field, and index expressions can be assigned".
            name: "array_length_assignment_truncates",
            area: "assignment_targets",
            source: r"
export function pull<T>(arr: T[], values: readonly T[]): T[] {
  const valuesSet = new Set(values);
  let resultIndex = 0;
  for (let i = 0; i < arr.length; i++) {
    if (valuesSet.has(arr[i])) {
      continue;
    }
    arr[resultIndex++] = arr[i];
  }
  arr.length = resultIndex;
  return arr;
}
",
        },
        Case {
            // A bare `array[i] = ...` write on an optional array (`T[] |
            // undefined`) after an `if (array == null) { array = ... }` default
            // initialization. Post-`if` null narrowing makes `array` a concrete
            // list so the index write is an assignable place.
            name: "optional_array_index_write_after_default_init",
            area: "assignment_targets",
            source: r"
export function copyArray<T>(source: T[], array?: T[]): T[] {
  const length = source.length;
  if (array == null) {
    array = new Array(length);
  }
  for (let i = 0; i < length; i++) {
    array[i] = source[i];
  }
  return array;
}
",
        },
        Case {
            // A `let` variable initialized to an arrow and later reassigned. The
            // arrow must bind a mutable closure-valued local (not lift to an
            // immutable function item), so the reassignment target is a place.
            name: "let_arrow_reassignment",
            area: "assignment_targets",
            source: r"
export function pick<T>(values: Array<(a: T, b: T) => boolean>): (a: T, b: T) => boolean {
  let comparator = (a: T, b: T) => a === b;
  const last = values[0];
  if (typeof last === 'function') {
    comparator = last;
  }
  return comparator;
}
",
        },
        Case {
            // A postfix update (`k++`) inside a function-expression body that is
            // itself a variable-declaration initializer. The update must emit
            // into the closure body, not defer into the outer declaration's
            // pending list (which produced a cross-body dangling expr ref).
            name: "postfix_update_in_nested_function_initializer",
            area: "closures",
            source: r"
export function bindArgs(func: (...args: number[]) => number, partial: number[]): (...args: number[]) => number {
  const bound = function (...provided: number[]): number {
    const args: number[] = [];
    let startIndex = 0;
    for (let i = 0; i < partial.length; i++) {
      args.push(provided[startIndex++]);
    }
    return func(...args);
  };
  return bound;
}
",
        },
        Case {
            // `String.raw` tagged template lowers to raw-quasi / substitution
            // concatenation; tagged templates were previously unsupported.
            name: "string_raw_tagged_template",
            area: "baseline",
            source: r"
export function raw(name: string): string {
  return String.raw`a\n${name}\t`;
}
",
        },
        Case {
            // Calling a value typed as a union of function types with *different*
            // arities (es-toolkit `once`). The arms must unify into one variadic
            // erased-rest signature so the `func` parameter, the packed-argument
            // wrapper, and the `SmeltUnknown::Function` adapter all agree on
            // arity; selecting arms inconsistently previously emitted a
            // 0-argument callee called with one argument and vice versa (E0057).
            // Exercised for both a zero-arg and a multi-arg call of the wrapper.
            name: "union_function_arity_once",
            area: "closures",
            source: r"
function once<F extends (() => any) | ((...args: any[]) => void)>(func: F): F {
  let called = false;
  let cache: ReturnType<F>;
  return function (...args: Parameters<F>): ReturnType<F> {
    if (!called) {
      called = true;
      cache = func(...args);
    }
    return cache;
  } as F;
}

export function useOnce(): void {
  const nullary = once(() => 1);
  nullary();
  const variadic = once((...values: number[]): void => {});
  variadic(1, 2, 3);
}
",
        },
        Case {
            // A `for` loop nested inside a branch that always diverges (every
            // path `return`s) must keep its loop body when emitted. es-toolkit
            // `some` has one such loop per branch of a conditional; the diverging
            // non-array branch's loop was previously dropped down to `i = 0` plus
            // a `break`, so control fell through into the sibling branch's loop
            // and read its counter uninitialized (E0381).
            name: "loop_in_diverging_branch",
            area: "baseline",
            source: r"
export function anyMatch(useLeft: boolean, left: number[], right: number[]): boolean {
  if (useLeft) {
    for (let i = 0; i < left.length; i++) {
      if (left[i] > 0) {
        return true;
      }
    }
    return false;
  }
  for (let i = 0; i < right.length; i++) {
    if (right[i] > 0) {
      return true;
    }
  }
  return false;
}
",
        },
        Case {
            // A JavaScript relational comparison (`<`) between a number and a
            // value typed as a numeric-bearing union (`string | number`, as in
            // es-toolkit `rangeRight`) must coerce the union side with `ToNumber`
            // and compare as `f64`; a bare `f64 < SmeltUnion` does not type-check
            // (E0277).
            name: "relational_number_vs_union",
            area: "numeric",
            source: r"
export function ascending(start: number, end: string | number): boolean {
  return start < end;
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
    emit_typescript_crate(case.name, case.source, crate_dir)
}

/// Python corpus: annotation-free source that only lowers because `ty`
/// resolves the return types (issue #93).
///
/// Each function omits its `-> T` return annotation; without the `ty` feature
/// the Python frontend would reject these with "must have an explicit return
/// type annotation". With `ty`, the return type is inferred from the body and
/// the program lowers and compiles.
#[cfg(feature = "ty")]
fn python_corpus() -> Vec<Case> {
    vec![
        Case {
            name: "py_inferred_returns",
            area: "py_ty_return_inference",
            // No function carries a `-> T`; `ty` infers int/str/bool from the
            // bodies. Parameters keep annotations (unannotated params stay an
            // explicit boundary — a documented deferral).
            source: r"
def inc(x: int):
    return x + 1

def label(name: str):
    return 'hi ' + name

def at_least_ten(n: int):
    return n >= 10

def total(values: list[int]):
    result = 0
    for value in values:
        result = result + value
    return result
",
        },
        Case {
            // Issue #94: method and non-top-level calls. `Counter.total`
            // (instance method) calls `self.doubled()`, a sibling declared
            // *later* in the class body — resolvable only because method items
            // are pre-registered before any body is lowered. `Counter.make` is a
            // `@classmethod` that constructs the class through the implicit `cls`
            // receiver (`cls(start)`) and calls the sibling `@staticmethod`
            // `origin()` via `cls.origin()`. `use_counter` drives an instance
            // method on a local, and `via_factory` calls the classmethod
            // qualified (`Counter.make(..)`). Every dispatch must lower to the
            // right shape (receiver method vs receiver-free associated call) and
            // the emitted Rust must compile.
            name: "py_method_calls",
            area: "py_method_dispatch",
            source: r#"
class Counter:
    value: int

    def __init__(self, value: int) -> None:
        self.value = value

    def total(self) -> int:
        return self.doubled() + Counter.origin()

    def doubled(self) -> int:
        return self.value * 2

    @classmethod
    def make(cls, start: int) -> "Counter":
        base = cls(start)
        return cls(base.value + cls.origin())

    @staticmethod
    def origin() -> int:
        return 0

def use_counter(start: int) -> int:
    counter = Counter(start)
    return counter.total()

def via_factory(start: int) -> int:
    return Counter.make(start).total()
"#,
        },
        Case {
            name: "py_lang_features",
            area: "py_try_lambda_classbody",
            // Issue #95: try/except/finally lowers to TryCatch, a lambda call
            // argument recovers its `Callable[...]` type and lowers to a
            // closure, and a class-body `...` placeholder is a no-op.
            source: r#"
from typing import Callable

class Marker:
    """A placeholder class."""
    ...

def apply(f: Callable[[int], int], value: int) -> int:
    return f(value)

def guarded(value: int) -> int:
    try:
        return apply(lambda x: x + 1, value)
    except ValueError as error:
        print(error)
        return 0
    finally:
        print("done")
"#,
        },
    ]
}

/// Lowers a Python corpus `case` through the real Python pipeline (with `ty`
/// type resolution) and emits a full program crate into `crate_dir`.
#[cfg(feature = "ty")]
fn emit_python_case_crate(case: &Case, crate_dir: &Path) -> Result<(), String> {
    let mut ctx = PyHirCtx::new();
    py_to_hir_with_path(case.source, FileId(0), "corpus/main.py", &mut ctx)
        .map_err(|err| format!("Python HIR lowering failed: {err:?}"))?;
    let mut mir =
        smelt_mir::lower_hir(&ctx.krate).map_err(|err| format!("MIR lowering failed: {err:?}"))?;
    smelt_mir::opt::optimize(&mut mir);
    let options = EmitOptions::new(format!("smelt_corpus_{}", case.name))
        .with_crate_kind(CrateKind::Program);
    emit_crate(&mir, crate_dir, &options).map_err(|err| format!("crate emission failed: {err}"))
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
    let root = scratch_root("smelt-compile-corpus");
    let crates_dir = root.join("crates");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crates_dir).expect("create scratch crates dir");
    std::fs::create_dir_all(&target_dir).expect("create scratch target dir");

    let mut failures: Vec<CorpusFailure> = Vec::new();

    // Optional single-case filter for local verification of one corpus entry
    // without checking the whole corpus. Unset in CI, which runs everything.
    let only = std::env::var("SMELT_CORPUS_ONLY").ok();

    for case in corpus() {
        if let Some(only_name) = &only
            && case.name != only_name
        {
            continue;
        }
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

    // Python corpus (only when built with `--features ty`): annotation-free
    // Python whose return types are resolved by `ty` (issue #93).
    #[cfg(feature = "ty")]
    for case in python_corpus() {
        if let Some(only_name) = &only
            && case.name != only_name
        {
            continue;
        }
        let crate_dir = crates_dir.join(case.name);
        if let Err(err) = emit_python_case_crate(&case, &crate_dir) {
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

// ---------------------------------------------------------------------------
// Rescued callback-generics fixture corpus
// ---------------------------------------------------------------------------
//
// The callback-generics campaign (PRs #202/#203) shipped six defects that the
// three compat corpora (es-toolkit, remeda, radash) were green through. Every
// one of them was found by *constructing a source shape the corpora do not
// contain* and compiling the generated Rust. Those hand-built repro projects
// lived in a session-scoped scratch directory; this corpus is their curated,
// in-repository home.
//
// Each fixture is a standalone TypeScript program in
// `tests/fixtures/callback_generics/<name>.ts` whose header comment records the
// defect class it guards. Fixtures are named for the shape they exercise, so a
// failure names a shape rather than an agent's scratch label.

/// Directory (relative to the crate root) holding the rescued fixtures.
const FIXTURE_DIR: &str = "tests/fixtures/callback_generics";

/// A rescued fixture: one TypeScript program read from [`FIXTURE_DIR`].
///
/// Unlike [`Case`], whose sources are `&'static str` literals compiled into the
/// test binary, fixtures are read from disk at run time so they can be edited,
/// diffed and reviewed as ordinary TypeScript files.
struct Fixture {
    /// File stem; also the emitted crate directory name.
    name: String,
    /// Group from the fixture's `// Area:` header, used to group failures.
    area: String,
    /// The `// Guards:` header line, quoted back when the fixture regresses.
    guard: String,
    /// Full TypeScript source, header comments included.
    source: String,
}

/// A fixture that does not compile at HEAD, recorded rather than deleted.
///
/// These are **pre-existing** defects: each one also reproduces at the
/// pre-campaign commit, so none of them is a callback-generics regression.
/// Recording them keeps the shape in the corpus and makes the day someone fixes
/// it visible — [`callback_generics_fixtures_compile`] fails in *both*
/// directions, when an expected failure starts passing and when a passing
/// fixture starts failing.
struct ExpectedFailure {
    /// Fixture file stem.
    name: &'static str,
    /// `cargo check` error count observed when the record was taken. Drift in
    /// this number is reported but does not fail the tier: rustc wording and
    /// error grouping change between toolchains, whereas pass/fail does not.
    errors: usize,
    /// Why it fails, in one line.
    cause: &'static str,
}

/// Fixtures known not to compile at HEAD, with their error counts and causes.
///
/// A record whose fixture no longer exists also fails the tier, so this table
/// cannot rot silently.
const EXPECTED_FIXTURE_FAILURES: &[ExpectedFailure] = &[
    // -- Confirmed pre-existing: each of these was re-run at the pre-campaign
    // commit during the callback-generics campaign and fails there too, so none
    // is a regression from PRs #202/#203.
    ExpectedFailure {
        name: "generic_class_method_callback",
        errors: 208,
        cause: "generic class construction with a composite `T[]` constructor parameter: the \
                emitted struct is used unparameterized, so nearly every use is E0277",
    },
    ExpectedFailure {
        name: "generic_class_two_methods_callback",
        errors: 4,
        cause: "a generic class whose two methods pin `T` differently; the method receivers \
                disagree with the constructed type (E0308)",
    },
    ExpectedFailure {
        name: "static_generic_method_callback",
        errors: 1,
        cause: "a static generic method's callback parameter is emitted at the declared, not \
                the substituted, type (E0308)",
    },
    ExpectedFailure {
        name: "string_length_in_callback_only",
        errors: 20,
        cause: "`.length` read off a string inside a non-generic `.map` callback (E0308)",
    },
    ExpectedFailure {
        name: "two_call_sites_pin_differently",
        errors: 10,
        cause: "two call sites pinning one callee differently; the second site reuses the \
                first site's substitution (E0308)",
    },
    ExpectedFailure {
        name: "source_class_named_box_with_callback_sink",
        errors: 27,
        cause: "a source class named `Box` collides with the generated/prelude `Box`, so its \
                uses take the wrong arity (E0107)",
    },
    ExpectedFailure {
        name: "concrete_callback_sunk_into_method",
        errors: 1,
        cause: "a non-generic callback sunk into a method call from a generic caller is \
                passed at the caller's borrowed type (E0308)",
    },
    ExpectedFailure {
        name: "concrete_and_generic_callbacks_two_sinks",
        errors: 10,
        cause: "one generic and one concrete callback in one signature: the concrete one is \
                still monomorphized with the generic one (E0308)",
    },
    // -- Also failing at HEAD. These come from the same rescued suite but were
    // not part of the campaign's re-verified ten, so they are recorded as
    // observed rather than asserted to be pre-existing. Anyone fixing one
    // should confirm which it is and update this note.
    ExpectedFailure {
        name: "generic_class_method_and_free_maker",
        errors: 208,
        cause: "same generic-class family as generic_class_method_callback: the class's own \
                `T` never reaches the emitted struct (E0277)",
    },
    ExpectedFailure {
        name: "map_valued_callback",
        errors: 0,
        cause: "rejected before emission: the frontend gates `Map.forEach` with \"array \
                forEach statement receiver must be an array\"",
    },
    ExpectedFailure {
        name: "optional_callback_parameter",
        errors: 0,
        cause: "rejected during emission: \"indirect call has too many arguments\" for a \
                `cb?:` parameter called at both arities",
    },
    ExpectedFailure {
        name: "second_type_param_pinned_by_key_callback",
        errors: 0,
        cause: "rejected during emission: \"type table does not contain literal operand type \
                Int\" when `K` is pinned only through the callback return",
    },
    ExpectedFailure {
        name: "variadic_type_param_callback",
        errors: 0,
        cause: "rejected during emission: \"type table does not contain literal operand type \
                Unknown\" for a variadic callback over `T`",
    },
    ExpectedFailure {
        name: "source_function_named_gen_is_rust_keyword",
        errors: 2,
        cause: "a source function named `gen` is emitted verbatim; `gen` is a Rust 2024 \
                reserved keyword, so the crate does not parse",
    },
    ExpectedFailure {
        name: "callback_sunk_into_erased_parameter",
        errors: 1,
        cause: "a borrowed callback also passed to an `unknown` parameter loses its lifetime \
                relation (\"lifetime may not live long enough\")",
    },
    ExpectedFailure {
        name: "fewer_param_callback_forwarded_to_wider_sink",
        errors: 1,
        cause: "a 1-parameter callback forwarded where 2 parameters are declared: the adapter \
                re-pins instead of widening (E0308)",
    },
    ExpectedFailure {
        name: "generic_maker_forwarded_into_sink",
        errors: 2,
        cause: "a caller-generic maker forwarded into a generic sink: the sink's bound is \
                stated over the caller's `T` (E0271/E0277)",
    },
    ExpectedFailure {
        name: "omitted_optional_callback_via_overload",
        errors: 1,
        cause: "an overload that omits the callback entirely: the passthrough branch renders \
                the remaining argument at the borrowed branch's type (E0308)",
    },
    ExpectedFailure {
        name: "variadic_spy_as_nullary_maker",
        errors: 2,
        cause: "a variadic erased function supplied where `() => T` is declared emits an \
                unstable feature use plus a mismatched adapter (E0658/E0308)",
    },
];

/// Reads the fixture corpus from [`FIXTURE_DIR`], sorted by name.
///
/// The `// Area:` and `// Guards:` header lines are parsed out for reporting;
/// the whole file (headers included, since they are comments) is what gets
/// lowered.
fn fixture_corpus() -> Vec<Fixture> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(FIXTURE_DIR);
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("read fixture directory")
        .map(|entry| entry.expect("read fixture dir entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ts"))
        .collect();
    entries.sort();
    entries
        .into_iter()
        .map(|path| {
            let source = std::fs::read_to_string(&path).expect("read fixture source");
            let name = path
                .file_stem()
                .expect("fixture file stem")
                .to_string_lossy()
                .into_owned();
            let header = |prefix: &str| {
                source
                    .lines()
                    .find_map(|line| line.strip_prefix(prefix))
                    .unwrap_or_default()
                    .trim()
                    .to_owned()
            };
            Fixture {
                area: header("// Area:"),
                guard: header("// Guards:"),
                name,
                source,
            }
        })
        .collect()
}

/// Compiles every rescued callback-generics fixture and asserts that each one
/// still lands on the side of the ledger it is recorded on.
///
/// Fails when a fixture that is expected to compile stops compiling (a
/// regression) **and** when a fixture recorded in [`EXPECTED_FIXTURE_FAILURES`]
/// starts compiling (a fix that must be recorded) or has gone missing (a rotted
/// record). Like [`corpus_emitted_rust_compiles`] it is `#[ignore]`d and shares
/// one `CARGO_TARGET_DIR` across the corpus.
#[test]
#[ignore = "slow: emits one crate per fixture and runs cargo check; run in CI via --ignored"]
fn callback_generics_fixtures_compile() {
    let root = scratch_root("smelt-compile-corpus");
    let crates_dir = root.join("crates");
    let target_dir = root.join("target");
    std::fs::create_dir_all(&crates_dir).expect("create scratch crates dir");
    std::fs::create_dir_all(&target_dir).expect("create scratch target dir");

    // Same single-fixture filter as the inline corpus, for local iteration.
    let only = std::env::var("SMELT_CORPUS_ONLY").ok();

    let fixtures = fixture_corpus();
    assert!(!fixtures.is_empty(), "fixture corpus is empty: {FIXTURE_DIR}");

    let mut regressions: Vec<String> = Vec::new();
    let mut unexpected_passes: Vec<String> = Vec::new();
    let mut drift: Vec<String> = Vec::new();

    for fixture in &fixtures {
        if let Some(only_name) = &only
            && &fixture.name != only_name
        {
            continue;
        }
        let expected = EXPECTED_FIXTURE_FAILURES
            .iter()
            .find(|entry| entry.name == fixture.name);

        let crate_dir = crates_dir.join(&fixture.name);
        let outcome = emit_typescript_crate(&fixture.name, &fixture.source, &crate_dir)
            .and_then(|()| cargo_check(&crate_dir, &target_dir));

        match (outcome, expected) {
            (Ok(()), None) => {}
            (Ok(()), Some(entry)) => unexpected_passes.push(format!(
                "[{}] {}: recorded as expected-failing ({} error(s): {}) but now COMPILES. \
                 Remove it from EXPECTED_FIXTURE_FAILURES.",
                fixture.area, fixture.name, entry.errors, entry.cause
            )),
            (Err(err), None) => regressions.push(format!(
                "[{}] {} (guards: {}):\n{err}",
                fixture.area, fixture.name, fixture.guard
            )),
            (Err(err), Some(entry)) => {
                let observed = rustc_error_count(&err);
                if observed != entry.errors {
                    drift.push(format!(
                        "[{}] {}: recorded {} error(s), observed {observed}",
                        fixture.area, fixture.name, entry.errors
                    ));
                }
            }
        }
    }

    // A record for a fixture that no longer exists is itself a failure: the
    // table must describe the corpus as it stands.
    let stale: Vec<String> = EXPECTED_FIXTURE_FAILURES
        .iter()
        .filter(|entry| !fixtures.iter().any(|fixture| fixture.name == entry.name))
        .map(|entry| format!("{}: no such fixture in {FIXTURE_DIR}", entry.name))
        .collect();

    // Best-effort cleanup; ignore errors so a leftover temp dir never fails CI.
    drop(std::fs::remove_dir_all(&root));

    if !drift.is_empty() {
        // Surfaced, not asserted: see [`ExpectedFailure::errors`].
        #[expect(
            clippy::print_stdout,
            reason = "the drift note is only useful in the tier's own output"
        )]
        {
            println!(
                "callback-generics fixtures: expected-failure error counts drifted:\n{}",
                drift.join("\n")
            );
        }
    }

    let mut report = String::new();
    if !regressions.is_empty() {
        write!(
            report,
            "{} fixture(s) that must compile no longer do:\n{}\n",
            regressions.len(),
            regressions.join("\n\n")
        )
        .expect("write to String");
    }
    if !unexpected_passes.is_empty() {
        write!(
            report,
            "{} recorded failure(s) now compile:\n{}\n",
            unexpected_passes.len(),
            unexpected_passes.join("\n")
        )
        .expect("write to String");
    }
    if !stale.is_empty() {
        write!(
            report,
            "{} stale expected-failure record(s):\n{}\n",
            stale.len(),
            stale.join("\n")
        )
        .expect("write to String");
    }
    assert!(report.is_empty(), "{report}");
}
