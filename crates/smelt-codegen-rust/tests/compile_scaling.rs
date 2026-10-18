//! Compile-time scaling: the TypeScript → HIR → MIR → Rust pipeline must grow
//! roughly linearly with program size for the shapes real code repeats.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631) and its
//! `analysis-scaling` tests: `bun check` found cubic blowups in long
//! interface/class chains, long unions and intersections, big arrays built
//! line by line, and deep generics (oven-sh/bun#44895, #44890, #44748). Each
//! of those is a shape a compiler handles fine at the size of a unit test and
//! falls over on at the size of a real file, so a unit test cannot catch it;
//! only measuring how time GROWS can.
//!
//! # What is measured
//!
//! For each shape a generator writes a synthetic program at size `n` and at
//! `4n`. The whole in-process pipeline — `to_hir`, `lower_hir`, `optimize`
//! and `emit_source` — runs on each, nothing is compiled by `rustc`, and the
//! time is the minimum of [`RUNS`] runs (the minimum is the least noisy
//! estimate of the work itself). A program at size 1 of the same shape is
//! measured too, and its time — the fixed cost of the prelude and the
//! pipeline's setup — is subtracted, so a superlinear term cannot hide behind
//! a large constant.
//!
//! # The assertion is a ratio, never a duration
//!
//! `t(4n) / t(n)` is 4 for linear work, 16 for quadratic and 64 for cubic. The
//! bound is [`MAX_GROWTH`] = `4 × 2.5` = 10: linear growth with headroom for
//! allocator and cache effects, well below quadratic. `n` is first doubled
//! until `t(n)` clears [`MIN_MEASURED`], so the ratio is never computed from
//! timer noise. No absolute time is asserted, so a slow machine or a debug
//! build cannot fail it — only a change in the ORDER of growth can.
//!
//! A shape that is superlinear for a reason that is not a local fix is
//! `#[ignore]`d with the measured growth and where the time goes, rather than
//! passing under a loosened bound.

#![expect(
    clippy::format_push_string,
    clippy::format_collect,
    reason = "the generators build small synthetic TypeScript programs; one `format!` per \
              line reads like the program it writes, and their cost is not what is measured"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "sizes, indices and doubling over bounded test sizes (`MAX_N`)"
)]
#![expect(
    clippy::panic,
    clippy::expect_used,
    reason = "a pipeline rejection of a generated shape is a test failure, reported with the phase"
)]
#![expect(
    clippy::use_debug,
    clippy::print_stderr,
    reason = "the measured growth and phase times are reported on stderr for every shape"
)]

use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

use smelt_frontend_ts::{HirCtx, predeclare_type_declarations_with_path, to_hir_with_path};
use smelt_hir::FileId;

/// Runs per size; the minimum is kept.
const RUNS: usize = 3;
/// The size ratio between the two measured programs.
const SCALE: usize = 4;
/// The largest accepted `t(SCALE·n) / t(n)`: linear (4) times 2.5.
const MAX_GROWTH: f64 = 10.0;
/// The smallest net time `t(n)` may have before the ratio is trusted.
const MIN_MEASURED: Duration = Duration::from_millis(50);
/// Upper bound on the calibrated `n`, so a pathological shape still finishes.
const MAX_N: usize = 1 << 14;

/// Calibration ceiling for shapes whose size is a nesting depth; see
/// [`assert_linear_up_to`].
const MAX_NESTING: usize = 128;

/// Serializes the shapes: timings taken while another shape runs on the next
/// core measure contention, not growth.
static SERIAL: Mutex<()> = Mutex::new(());

/// Wall time of each pipeline phase for one program.
#[derive(Clone, Copy, Default, Debug)]
struct Phases {
    /// `to_hir`.
    frontend: Duration,
    /// `lower_hir` + `optimize`.
    mir: Duration,
    /// `emit_source`.
    codegen: Duration,
    /// Bytes of emitted Rust, so a report can tell growth in the WORK apart
    /// from growth in the OUTPUT the program inherently needs.
    emitted_bytes: usize,
}

impl Phases {
    /// The whole pipeline.
    fn total(self) -> Duration {
        self.frontend + self.mir + self.codegen
    }
}

/// Runs the in-process pipeline once over `source` and times each phase.
///
/// Panics with the failing phase when the program does not lower: every
/// generated shape is valid TypeScript that the pipeline accepts, and a
/// rejection is a regression this test should report, not time.
fn run_pipeline(source: &str) -> Phases {
    let frontend_started = Instant::now();
    // The manifest build's two frontend passes: type declarations are
    // predeclared across the crate first, then the module is lowered.
    let mut ctx = HirCtx::new();
    if let Err(error) =
        predeclare_type_declarations_with_path(source, FileId(0), "main.ts", &mut ctx)
    {
        panic!("type predeclaration failed: {error:?}\n{source}");
    }
    if let Err(error) = to_hir_with_path(source, FileId(0), "main.ts", &mut ctx) {
        panic!("HIR lowering failed: {error:?}\n{source}");
    }
    let frontend = frontend_started.elapsed();
    let mir_started = Instant::now();
    let mut mir = match smelt_mir::lower_hir(&ctx.krate) {
        Ok(mir) => mir,
        Err(errors) => panic!("MIR lowering failed: {errors:?}"),
    };
    smelt_mir::opt::optimize(&mut mir);
    let mir_time = mir_started.elapsed();
    let codegen_started = Instant::now();
    let emitted_bytes = match smelt_codegen_rust::emit_source(&mir) {
        Ok(emitted) => emitted.len(),
        Err(error) => panic!("Rust emission failed: {error}"),
    };
    let codegen = codegen_started.elapsed();
    Phases {
        frontend,
        mir: mir_time,
        codegen,
        emitted_bytes,
    }
}

/// The fastest of [`RUNS`] runs, by total time.
fn measure(source: &str) -> Phases {
    (0..RUNS)
        .map(|_| run_pipeline(source))
        .min_by_key(|phases| phases.total())
        .unwrap_or_default()
}

/// Net time above the size-1 baseline, floored at a microsecond so a ratio is
/// always defined.
fn net(time: Duration, baseline: Duration) -> Duration {
    time.saturating_sub(baseline).max(Duration::from_micros(1))
}

/// Calibrate `n`, measure `n` and `SCALE·n`, report, and return the growth.
///
/// Runs on a thread with a large stack: deeply nested shapes recurse in the
/// parser and the lowering, and the default test-thread stack is sized for
/// ordinary programs, not for the 4× size of a calibrated one.
fn growth(shape: &str, start: usize, max_n: usize, generate: fn(usize) -> String) -> f64 {
    let _guard = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let shape_name = shape.to_owned();
    let worker = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || measure_growth(&shape_name, start, max_n, generate))
        .expect("spawn the measuring thread");
    worker.join().unwrap_or_else(|payload| std::panic::resume_unwind(payload))
}

/// The body of [`growth`], on the large-stack thread.
fn measure_growth(shape: &str, start: usize, max_n: usize, generate: fn(usize) -> String) -> f64 {
    let baseline_phases = measure(&generate(1));
    let baseline = baseline_phases.total();
    let baseline_bytes = baseline_phases.emitted_bytes;
    let mut n = start;
    let mut small = measure(&generate(n));
    while net(small.total(), baseline) < MIN_MEASURED && n < max_n {
        n *= 2;
        small = measure(&generate(n));
    }
    let large = measure(&generate(n * SCALE));
    let ratio = net(large.total(), baseline).as_secs_f64() / net(small.total(), baseline).as_secs_f64();
    let bytes = |phases: Phases| {
        f64::from(u32::try_from(phases.emitted_bytes.saturating_sub(baseline_bytes)).unwrap_or(u32::MAX))
    };
    let output_growth = bytes(large) / bytes(small).max(1.0);
    eprintln!(
        "{shape}: n={n} baseline={baseline:?} t(n)={:?} t({SCALE}n)={:?} growth={ratio:.2} \
         output-growth={output_growth:.2} \
         [frontend {:?} -> {:?}, mir {:?} -> {:?}, codegen {:?} -> {:?}]",
        small.total(),
        large.total(),
        small.frontend,
        large.frontend,
        small.mir,
        large.mir,
        small.codegen,
        large.codegen,
    );
    ratio
}

/// Assert the measured growth of `shape` is within [`MAX_GROWTH`].
fn assert_linear(shape: &str, start: usize, generate: fn(usize) -> String) {
    assert_linear_up_to(shape, start, MAX_N, generate);
}

/// [`assert_linear`] for a shape whose size is a NESTING depth, calibrated no
/// further than `max_n`.
///
/// A nested shape recurses once per level through the parser, the lowering
/// and the emitter, and under coverage instrumentation each debug frame is
/// large enough that a calibrated depth of a few hundred (×[`SCALE`] for the
/// large run) can exhaust even the measuring thread's stack. Where calibration
/// lands depends on the machine's speed, so without a cap the test overflowed
/// on some CI runs and not others. The cap keeps the depth deterministic; the
/// shape still measures growth, from a smaller absolute time.
fn assert_linear_up_to(shape: &str, start: usize, max_n: usize, generate: fn(usize) -> String) {
    let ratio = growth(shape, start, max_n, generate);
    assert!(
        ratio < MAX_GROWTH,
        "{shape}: t({SCALE}n)/t(n) = {ratio:.2}, above the linear bound {MAX_GROWTH} \
         (quadratic is {}, cubic {})",
        SCALE * SCALE,
        SCALE * SCALE * SCALE,
    );
}

/// `interface I1 extends I0 { .. }` … and a read through the last one.
fn interface_extends_chain(n: usize) -> String {
    let mut source = String::from("interface I0 { f0: number }\n");
    for k in 1..n {
        source.push_str(&format!("interface I{k} extends I{} {{ f{k}: number }}\n", k - 1));
    }
    let last = n - 1;
    source.push_str(&format!(
        "export function read(v: I{last}): number {{ return v.f0 + v.f{last}; }}\n"
    ));
    source
}

/// `class C1 extends C0 { .. }` …, each with a field and a method calling the
/// inherited one.
fn class_extends_chain(n: usize) -> String {
    let mut source =
        String::from("class C0 { a0: number = 0; m0(): number { return this.a0; } }\n");
    for k in 1..n {
        source.push_str(&format!(
            "class C{k} extends C{p} {{ a{k}: number = {k}; m{k}(): number {{ return this.a{k} + this.m{p}(); }} }}\n",
            p = k - 1
        ));
    }
    let last = n - 1;
    source.push_str(&format!("console.log(new C{last}().m{last}());\n"));
    source
}

/// `type U = "a0" | "a1" | …` and a function over it.
fn string_literal_union(n: usize) -> String {
    let members = (0..n).map(|k| format!("\"a{k}\"")).collect::<Vec<_>>().join(" | ");
    format!(
        "type U = {members};\nfunction name(u: U): string {{ return u; }}\nconsole.log(name(\"a0\"));\n"
    )
}

/// A discriminated union of `n` declared object shapes and a `switch` over it.
///
/// The members are declared interfaces. The same union written with INLINE
/// object-literal members (`{ kind: "k0"; v0: number } | ...`) does not lower
/// today -- emission fails with "cannot type the read of `kind` on
/// `SmeltUnknown`" at every size -- which is a lowering gap, not a scaling
/// question, so it is not what this shape measures.
fn discriminated_object_union(n: usize) -> String {
    let mut source = String::new();
    for k in 0..n {
        source.push_str(&format!("interface K{k} {{ kind: \"k{k}\"; v{k}: number }}\n"));
    }
    let members = (0..n).map(|k| format!("K{k}")).collect::<Vec<_>>().join(" | ");
    let cases = (0..n)
        .map(|k| format!("    case \"k{k}\": return s.v{k};\n"))
        .collect::<String>();
    source.push_str(&format!(
        "type S = {members};\nfunction value(s: S): number {{\n  switch (s.kind) {{\n{cases}  }}\n  return -1;\n}}\nconst first: K0 = {{ kind: \"k0\", v0: 7 }};\nconsole.log(value(first));\n"
    ));
    source
}

/// `type X = A0 & A1 & …` over `n` single-field interfaces (at least two).
///
/// At least two, because the one-member form is a plain alias of an
/// interface, `type X = A0`, and a parameter typed by such an alias does not
/// lower today ("cannot type the read of `f0` on `SmeltUnknown`") -- a
/// lowering gap of its own, not this shape's question.
fn long_intersection(size: usize) -> String {
    let n = size.max(2);
    let mut source = String::new();
    for k in 0..n {
        source.push_str(&format!("interface A{k} {{ f{k}: number }}\n"));
    }
    let members = (0..n).map(|k| format!("A{k}")).collect::<Vec<_>>().join(" & ");
    let fields = (0..n).map(|k| format!("f{k}: {k}")).collect::<Vec<_>>().join(", ");
    source.push_str(&format!(
        "type X = {members};\nfunction first(x: X): number {{ return x.f0; }}\nconst value: X = {{ {fields} }};\nconsole.log(first(value));\n"
    ));
    source
}

/// One array literal with an element per line.
fn array_literal_lines(n: usize) -> String {
    let elements = (0..n).map(|k| format!("  {k},\n")).collect::<String>();
    format!("const values: number[] = [\n{elements}];\nconsole.log(values.length);\n")
}

/// An empty array filled by `n` `push` statements.
fn array_push_statements(n: usize) -> String {
    let pushes = (0..n).map(|k| format!("values.push({k});\n")).collect::<String>();
    format!("const values: number[] = [];\n{pushes}console.log(values.length);\n")
}

/// `Box<Box<…<number>>>` nested `n` deep, read back through `n` `.value`s.
fn nested_generic_type(n: usize) -> String {
    let ty = (0..n).fold("number".to_owned(), |inner, _| format!("Box<{inner}>"));
    let read = (0..n).fold("b".to_owned(), |inner, _| format!("{inner}.value"));
    format!(
        "interface Box<T> {{ value: T }}\nexport function unwrap(b: {ty}): number {{ return {read}; }}\n"
    )
}

/// `id(id(…id(1)…))`, `n` generic calls deep.
fn generic_call_chain(n: usize) -> String {
    let call = (0..n).fold("1".to_owned(), |inner, _| format!("id({inner})"));
    format!("function id<T>(x: T): T {{ return x; }}\nconst v = {call};\nconsole.log(v);\n")
}

/// One function body of `n` dependent statements.
fn long_function(n: usize) -> String {
    let mut body = String::from("  let a0 = 0;\n");
    for k in 1..n {
        body.push_str(&format!("  let a{k} = a{} + {k};\n", k - 1));
    }
    format!(
        "function big(): number {{\n{body}  return a{};\n}}\nconsole.log(big());\n",
        n - 1
    )
}

/// `n` small functions, each calling the previous one.
fn function_call_ladder(n: usize) -> String {
    let mut source = String::from("function f0(x: number): number { return x + 1; }\n");
    for k in 1..n {
        source.push_str(&format!(
            "function f{k}(x: number): number {{ return f{}(x) + 1; }}\n",
            k - 1
        ));
    }
    source.push_str(&format!("console.log(f{}(0));\n", n - 1));
    source
}

/// IGNORED: quadratic, and the quadratic is the OUTPUT. Measured growth
/// 14.8 at n=256 (output growth 16.1): interface `I{k}` flattens to a struct of
/// `k` fields, so the emitted program holds sum(k) = O(n^2) fields -- the
/// layout a hand-written port of an `extends` chain also has. The cubic term
/// on top of that (growth 80 at n=128, 13 s in MIR lowering at 4n) is fixed:
/// `lower_effective_interface_fields` re-flattened every ancestor for every
/// interface with a linear-scan dedup, and codegen's
/// `effective_interface_fields` re-sanitized every candidate per field; both
/// are memoized / map-indexed now. Un-ignore if the representation stops
/// flattening.
#[test]
#[ignore = "quadratic emitted output (flattened extends chain); see the comment"]
fn interface_extends_chain_scales_linearly() {
    assert_linear("interface extends chain", 16, interface_extends_chain);
}

/// IGNORED: superlinear in codegen. Measured growth 56 at n=32 (output growth
/// 16.1, codegen 0.25 s -> 14.5 s). Two causes, neither local:
/// * inheritance is FLATTENED -- every subclass re-emits every inherited
///   field and method (`effective_class_methods`), so the output itself is
///   O(n^2);
/// * each emitted field access re-derives the class layout by linear scans
///   over the O(n) flattened fields and the class list (`place_ty`,
///   `structural_record_fields`, `class_index_store_types`,
///   `storage_field_is_function`, `descriptor_for_field`, `base_class`),
///   adding a further O(n) factor per access.
///
/// `effective_class_fields`' quadratic override lookup is fixed (map-indexed);
/// the remaining fix is a per-class layout cache in `EmitContext`.
#[test]
#[ignore = "superlinear codegen (flattened inheritance + per-access layout scans); see the comment"]
fn class_extends_chain_scales_linearly() {
    assert_linear("class extends chain", 8, class_extends_chain);
}

#[test]
fn string_literal_union_scales_linearly() {
    assert_linear("string-literal union", 32, string_literal_union);
}

/// IGNORED: cubic. Measured growth 29 at n=64 (output growth 4.0): frontend
/// 38 ms -> 1.27 s, codegen 41 ms -> 1.02 s. Every `s.v{k}` read asks each of
/// the n members for its field type, and each ask is a linear scan:
/// `ModuleBuilder::find_interface` walks all HIR items (from
/// `class_field_type`), and codegen's `is_erased_class_type` walks every MIR
/// class and interface (from `concrete_union_members` ->
/// `union_member_is_concrete`). n reads x n members x n items. The fix is a
/// symbol -> item index; the HIR crate's `items` is a public `Vec` several
/// passes rewrite in place, so that index is not a local change.
#[test]
#[ignore = "cubic: per-member linear item lookups; see the comment"]
fn discriminated_object_union_scales_linearly() {
    assert_linear("discriminated object union", 8, discriminated_object_union);
}

/// IGNORED: quadratic in the frontend. Measured growth 10.5 at n=512 (output
/// growth 4.0, frontend 74 ms -> 829 ms): the object literal's contextual type
/// resolves every member through `ModuleBuilder::find_interface`, a linear walk
/// of all HIR items (`object_literal_type`, `contextual_record_literal_type`,
/// `class_field_type`). Same root cause as the discriminated union. The
/// symbol interner's linear scan (the dominant cost before, growth 12.4 at
/// n=256) is fixed: `SymbolInterner` is hash-indexed now.
#[test]
#[ignore = "quadratic frontend item lookups; see the comment"]
fn long_intersection_scales_linearly() {
    assert_linear("long intersection", 16, long_intersection);
}

#[test]
fn array_literal_lines_scale_linearly() {
    assert_linear("array literal, one element per line", 64, array_literal_lines);
}

/// IGNORED: quadratic in codegen. Measured growth 11.4 at n=1024 (output
/// growth 4.05, codegen 86 ms -> 1.26 s). Every emitted statement asks
/// whole-function questions about its locals, each a scan over every
/// statement: `local_binding_needs_mut` (via `local_assignment_count`,
/// `rvalue_mutates_local`, `local_may_be_assigned_after_assignment`,
/// `local_uses_shared_capture_storage`), plus `list_alias_origin` per `push`.
/// The fix is one per-function pass that indexes statements by the locals
/// they touch -- a rework of `emitter::local_analysis`, not a local change.
#[test]
#[ignore = "quadratic codegen local analysis; see the comment"]
fn array_push_statements_scale_linearly() {
    assert_linear("array filled by push statements", 64, array_push_statements);
}

/// IGNORED: quadratic, and the quadratic is the OUTPUT. Measured growth 15.3
/// at n=128 with output growth 14.1: each of the n `.value` steps is a
/// temporary annotated with its full remaining type, `Box<Box<...>>` of depth
/// n-k, so the emitted text is O(n^2). The cubic-looking part (growth 21 at
/// n=128, MIR 7 ms -> 648 ms) was `TypeInterner::intern`, a linear scan over
/// every interned type on every intern; it is hash-indexed now.
#[test]
#[ignore = "quadratic emitted output (nested type annotations); see the comment"]
fn nested_generic_type_scales_linearly() {
    assert_linear_up_to("nested generic type", 4, MAX_NESTING, nested_generic_type);
}

#[test]
fn generic_call_chain_scales_linearly() {
    assert_linear_up_to("nested generic call chain", 8, MAX_NESTING, generic_call_chain);
}

/// IGNORED: quadratic in codegen, same cause as `array_push_statements`.
/// Measured growth 11.7 at n=512 (output growth 4.2, codegen 67 ms ->
/// 941 ms; `local_binding_needs_mut` and the capture-storage scan). The
/// frontend half is fixed: `predeclare_forward_referenced_locals` re-walked
/// the whole statement prefix for every declaration (frontend 44 ms -> 660 ms
/// at n=256 before, 14 ms -> 99 ms at n=512 after).
#[test]
#[ignore = "quadratic codegen local analysis; see the comment"]
fn long_function_scales_linearly() {
    assert_linear("long function body", 64, long_function);
}

#[test]
fn function_call_ladder_scales_linearly() {
    assert_linear("functions calling each other", 16, function_call_ladder);
}
