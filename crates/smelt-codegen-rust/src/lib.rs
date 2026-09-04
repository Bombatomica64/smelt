//! Code generator for emitting Rust source code from the MIR.
//!
//! This crate provides functionality to transform a MIR (Middle Intermediate Representation)
//! into valid Rust source code, including struct definitions and function implementations.

#![expect(
    clippy::doc_markdown,
    reason = "codegen docs mention generated language tokens that are not fully marked up yet"
)]
#![expect(
    clippy::missing_errors_doc,
    reason = "public codegen entrypoint docs need a focused polish pass"
)]
#![expect(
    clippy::type_complexity,
    reason = "control-flow pattern recognizers currently return structured tuples"
)]
#![expect(
    clippy::format_push_string,
    reason = "Rust code emission is still written as direct string assembly"
)]
#![expect(
    clippy::too_many_arguments,
    reason = "structured codegen emitters pass explicit control-flow context"
)]
#![expect(
    clippy::too_many_lines,
    reason = "large emitters will be split after behavior stabilizes"
)]
#![expect(
    clippy::literal_string_with_formatting_args,
    reason = "generated Rust format strings intentionally contain braces"
)]
#![expect(
    clippy::wildcard_enum_match_arm,
    reason = "operand formatting groups future constants with existing constants"
)]
#![expect(
    clippy::unused_self,
    reason = "helper methods stay on the emitter for future shared state"
)]
#![expect(
    clippy::needless_pass_by_value,
    reason = "type lookup helpers accept owned HIR type values for call-site readability"
)]
#![expect(
    clippy::missing_const_for_fn,
    reason = "utility const qualification will be handled after behavior cleanup"
)]
#![expect(
    clippy::needless_pass_by_ref_mut,
    reason = "emitter entrypoints reserve mutability for incremental generation state"
)]
#![allow(
    clippy::manual_let_else,
    clippy::match_same_arms,
    clippy::needless_question_mark,
    clippy::option_if_let_else,
    clippy::unnested_or_patterns,
    clippy::unnecessary_wraps,
    reason = "Remeda fallback emission currently favors explicit conservative branches over stylistic rewrites"
)]
#![cfg_attr(
    test,
    expect(
        clippy::manual_let_else,
        clippy::match_wild_err_arm,
        clippy::option_if_let_else,
        clippy::panic,
        reason = "codegen tests keep fixture setup compact and fail fast on invalid test inputs"
    )
)]

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use crate::{rust::erased_string, type_substitution::TypeSubstitution};
use smelt_hir::{AsyncOp, BodyId, Type, TypeId};
use smelt_mir::{HirOrigin, Mir, MirClassProtocol, MirFunction, Rvalue};

mod builtin_member_prelude;
mod byte_buffer_prelude;
pub(crate) mod class_proto;
pub(crate) mod classes;
pub(crate) mod classify;
pub(crate) mod deps;
// Increment 3 of the callback-generics plan made the last dormant entry point
// live: the safety valve consults `collect_bindings` and `TypeParamBinding`
// directly, so the module no longer needs a `dead_code` expectation.
pub(crate) mod generic_bindings;
mod reflection_prelude;
pub(crate) mod runtime_prelude;
pub mod rust;
pub(crate) mod stdlib;
pub(crate) mod thrown;
pub(crate) mod type_substitution;

use deps::GeneratedDep;
mod emitter;
use classes::{
    class_impl_generics_text, class_name_text, class_type_args_text, class_type_params_text,
    effective_class_fields, effective_class_methods, effective_interface_fields,
    inherited_trait_methods,
    interface_impl_generics_text, interface_type_params_text, materialized_static_value_text,
};
use emitter::{EmitContext, FunctionEmitter};
use runtime_prelude::{PreludeGate, emit_gate as emit_runtime_gate};
use rust::{CodeWriter, RustIdent};

/// Sentinel comment emitted between the fixed runtime prelude and the generated
/// program body.
///
/// Every emitted crate root contains this line exactly once. It is an inert Rust
/// comment (so it never affects compilation) and the single source of truth for
/// where the shared runtime scaffolding ends. Tooling that measures
/// `SmeltUnknown` usage reads it to attribute occurrences above it to the
/// prelude and occurrences below it to program code. Because it is exported,
/// that tooling references this constant instead of hard-coding the string, so
/// the marker and its consumers can never drift apart.
pub const PRELUDE_END_MARKER: &str = "// @smelt:prelude-end — generated program below";

/// Options for controlling code emission behavior.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct EmitOptions {
    /// The name of the Rust crate to generate.
    pub crate_name: String,
    /// The Rust crate target kind to generate.
    pub crate_kind: CrateKind,
    /// The global allocator the generated program installs.
    pub allocator: GeneratedAllocator,
    /// The `[profile.release]` the generated crate carries.
    pub release_profile: ReleaseProfile,
}

impl Default for EmitOptions {
    /// Returns the default emission options with crate name "smelt_app".
    fn default() -> Self {
        Self {
            crate_name: "smelt_app".to_owned(),
            crate_kind: CrateKind::Program,
            allocator: GeneratedAllocator::default(),
            release_profile: ReleaseProfile::default(),
        }
    }
}

impl EmitOptions {
    /// Creates program emission options for the given Rust crate name.
    pub fn new(crate_name: impl Into<String>) -> Self {
        Self {
            crate_name: crate_name.into(),
            ..Self::default()
        }
    }

    /// Sets whether Smelt emits a program or library crate root.
    #[must_use]
    pub fn with_crate_kind(mut self, crate_kind: CrateKind) -> Self {
        self.crate_kind = crate_kind;
        self
    }

    /// Sets the global allocator the generated program installs.
    #[must_use]
    pub fn with_allocator(mut self, allocator: GeneratedAllocator) -> Self {
        self.allocator = allocator;
        self
    }

    /// Sets the `[profile.release]` the generated crate carries.
    #[must_use]
    pub fn with_release_profile(mut self, release_profile: ReleaseProfile) -> Self {
        self.release_profile = release_profile;
        self
    }

    /// The allocator actually emitted for this crate.
    ///
    /// A `#[global_allocator]` is a whole-program choice, so only a generated
    /// PROGRAM may make it. A generated library is linked into someone else's
    /// binary, and that binary's author owns the decision; emitting one there
    /// would silently override it.
    fn effective_allocator(&self) -> GeneratedAllocator {
        match self.crate_kind {
            CrateKind::Program => self.allocator,
            CrateKind::Library => GeneratedAllocator::System,
        }
    }
}

/// The `[profile.release]` a generated crate carries.
///
/// Cargo's stock release profile builds with 16 codegen units and no LTO, so a
/// call that crosses a unit boundary is never inlined. That is the common case in
/// generated code: the runtime prelude lives in the crate root and every module
/// calls into it, so `SmeltUnknown::clone`, a list index read and a field lookup
/// are all cross-unit calls in the hot loop. `callgrind` badly under-reports this
/// — instruction counts for the es-toolkit bench crate moved less than 1% — but
/// wall clock on `sumBy`, which is a loop and nothing else, moved **1.78x**, and
/// the binary got 19% SMALLER. The cost is build time (+28% on that crate) and it
/// falls only on `--release`.
///
/// A team shipping this library by hand would set it, so `Optimized` is the
/// default; `Default` leaves Cargo's profile alone for a project that would rather
/// have the build time back.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum ReleaseProfile {
    /// Thin LTO and one codegen unit.
    #[default]
    Optimized,
    /// Cargo's stock release profile.
    Default,
}

/// The global allocator a generated program installs.
///
/// Generated code allocates far more than hand-written Rust does, because every
/// JavaScript array, object and string is a separate heap value with JavaScript's
/// reference semantics. Profiling the es-toolkit corpus put the malloc family at
/// roughly 30% of `groupBy` and 43% of `partition`, which is a bigger share than
/// any single thing the emitter does. glibc's allocator is tuned for a general
/// mix; this workload is a stream of small, short-lived allocations, which is
/// exactly what a modern thread-caching allocator is built for.
///
/// A team hand-writing this library in Rust would reach for one, and generated
/// programs that care about throughput should opt in with
/// `[rust] allocator = "mimalloc"`.
///
/// The DEFAULT is nonetheless `System`, deliberately. `mimalloc` builds C, which
/// means a network fetch and a compiler at build time; making that the default
/// would silently impose it on every program Smelt emits, including the ones in
/// `examples/`, which are meant to be readable and to build offline. Choosing an
/// allocator is the application author's call, so Smelt offers it rather than
/// making it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[non_exhaustive]
pub enum GeneratedAllocator {
    /// Leave the platform allocator in place.
    #[default]
    System,
    /// Install `mimalloc` as the program's global allocator.
    Mimalloc,
}

/// Rust crate target kind emitted by Smelt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum CrateKind {
    /// Generate an executable Rust program rooted at `src/main.rs`.
    Program,
    /// Generate a Rust library crate rooted at `src/lib.rs`.
    Library,
}

impl CrateKind {
    /// Returns the Rust source root file name for this crate kind.
    #[must_use]
    fn root_file_name(self) -> &'static str {
        match self {
            Self::Program => "main.rs",
            Self::Library => "lib.rs",
        }
    }

    /// Returns the Rust source root file name for the other crate kind.
    #[must_use]
    fn stale_root_file_name(self) -> &'static str {
        match self {
            Self::Program => "lib.rs",
            Self::Library => "main.rs",
        }
    }
}

/// An error encountered during code emission.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmitError {
    /// The error message.
    pub message: String,
}

impl std::fmt::Display for EmitError {
    /// Formats the error message for display.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for EmitError {}

impl EmitError {
    /// Creates a new emit error with the given message.
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// Converts a compact MIR identifier into a `usize` for indexing.
fn id_index(index: u32, context: &'static str) -> Result<usize, EmitError> {
    usize::try_from(index).map_err(|_err| EmitError::new(context))
}

/// Render the Rust array literal of host-object identity markers hidden from
/// JavaScript `for...in` / `Object.keys` enumeration.
///
/// The host-object identity markers (`__smelt_arraybuffer`, `__smelt_weakmap`,
/// the boxed-primitive markers, ...) come from the shared
/// `smelt_stdlib::host_object` registry so this runtime filter, the frontend
/// construction path, and the `instanceof` codegen path share one source of
/// truth. Appended to that are the markers owned by other runtime subsystems
/// (abort controllers/signals, builtin namespaces, the global object) whose
/// records must equally hide their internal keys but which are not part of the
/// host-object registry proper.
fn host_marker_registry_array() -> String {
    let markers = smelt_stdlib::host_object_markers()
        .chain([
            "__smelt_abortcontroller",
            "__smelt_abortsignal",
            "__smelt_builtin_namespace",
            "__smelt_global_object",
        ])
        .map(|marker| format!("\"{marker}\""))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{markers}]")
}

/// Converts a `usize` into a compact MIR identifier.
fn compact_index(index: usize, context: &'static str) -> Result<u32, EmitError> {
    u32::try_from(index).map_err(|_err| EmitError::new(context))
}

/// Emits a complete Rust crate from the given MIR.
///
/// Creates the crate structure with Cargo.toml and the configured crate root.
pub fn emit_crate(
    mir: &Mir,
    output_path: impl AsRef<Path>,
    options: &EmitOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = output_path.as_ref();
    let src_dir = output_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    let allocator = options.effective_allocator();
    write_if_changed(
        output_dir.join("Cargo.toml"),
        &deps::cargo_toml(
            &options.crate_name,
            &generated_deps(mir),
            allocator,
            options.release_profile,
        ),
    )?;
    write_crate_root(
        &src_dir,
        options.crate_kind,
        &emit_source_with_allocator(mir, allocator)?,
    )?;
    Ok(())
}

/// Emits a complete Rust crate while preserving the source module layout.
///
/// Shared generated runtime/types stay in the configured crate root. Non-entry
/// module-level functions are moved into source-shaped Rust modules and
/// re-exported from the crate root so existing flat-name call emission
/// continues to resolve.
pub fn emit_crate_with_modules(
    mir: &Mir,
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
    output_path: impl AsRef<Path>,
    options: &EmitOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = output_path.as_ref();
    let src_dir = output_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    let allocator = options.effective_allocator();
    write_if_changed(
        output_dir.join("Cargo.toml"),
        &deps::cargo_toml(
            &options.crate_name,
            &generated_deps(mir),
            allocator,
            options.release_profile,
        ),
    )?;

    let mapped = emit_mapped_sources(mir, krate, modules, allocator)?;
    write_crate_root(&src_dir, options.crate_kind, &mapped.root)?;
    for module in mapped.modules {
        let module_path = src_dir.join(format!("{}.rs", module.name));
        write_if_changed(module_path, &module.source)?;
    }
    Ok(())
}

/// Writes the configured crate root and removes the stale opposite root.
///
/// Cargo infers program and library targets from `src/main.rs` and `src/lib.rs`.
/// Removing the other generated root keeps manifest kind changes from silently
/// producing both targets in the same output directory.
fn write_crate_root(
    src_dir: &Path,
    crate_kind: CrateKind,
    contents: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    write_if_changed(src_dir.join(crate_kind.root_file_name()), contents)?;
    let stale_root = src_dir.join(crate_kind.stale_root_file_name());
    if stale_root.exists() {
        fs::remove_file(stale_root)?;
    }
    Ok(())
}

/// Writes generated text only when the destination bytes actually change.
///
/// Cargo uses file mtimes as part of its freshness checks. Rewriting every
/// generated Rust module on each Smelt build forces Cargo to recompile modules
/// whose source is byte-for-byte identical, which dominates large projects such
/// as Remeda. Keeping identical files untouched preserves incremental build
/// artifacts without changing Smelt's lowering or codegen semantics.
fn write_if_changed(
    path: impl AsRef<Path>,
    contents: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let destination = path.as_ref();
    if fs::read_to_string(destination).is_ok_and(|existing| existing == contents) {
        return Ok(());
    }
    fs::write(destination, contents)?;
    Ok(())
}

/// Returns whether generated Rust rounds a float with JavaScript `Math.round`.
///
/// Scans function *and* closure rvalues, like `needs_timer_helpers`: the op can
/// sit inside a synthesized closure body and the prelude must still define the
/// helper for it. The other three rounding ops (`floor`, `ceil`, `trunc`) agree
/// between the two languages and map straight to their `f64` methods, so only
/// `Round` pulls the helper in.
fn needs_math_round(mir: &Mir) -> bool {
    stdlib::rvalues(mir).any(|value| {
        matches!(
            value,
            Rvalue::NumericRound {
                op: smelt_hir::NumericRoundOp::Round,
                ..
            }
        )
    })
}

/// Returns whether generated Rust uses JavaScript timer helpers.
///
/// Scans function *and* closure rvalues: a timer op can live inside a
/// synthesized closure body (e.g. the first-class `setTimeout` value form),
/// and the prelude must still define the timer queue for it.
fn needs_timer_helpers(mir: &Mir) -> bool {
    stdlib::rvalues(mir).any(|value| {
        matches!(
            value,
            Rvalue::AsyncOp {
                op: AsyncOp::Sleep
                    | AsyncOp::Resolve
                    | AsyncOp::Reject
                    | AsyncOp::SetTimeout
                    | AsyncOp::ClearTimeout
                    | AsyncOp::SetInterval
                    | AsyncOp::ClearInterval
                    | AsyncOp::Promise
                    | AsyncOp::Then
                    | AsyncOp::Catch
                    // `Promise.race` is backed by `smelt_promise_race`, which
                    // lives with the timer helpers because it drives the same
                    // cooperative promise-task queue.
                    | AsyncOp::Race
                    | AsyncOp::SpawnLocal,
                ..
            }
        )
    })
}

/// Emits Rust source code from the given MIR.
///
/// Returns a string containing the complete source code for the MIR, including
/// struct definitions, free functions, and impl blocks for methods.
pub fn emit_source(mir: &Mir) -> Result<String, EmitError> {
    emit_source_with_allocator(mir, GeneratedAllocator::System)
}

/// Emits Rust source code, installing `allocator` as the program's global allocator.
///
/// [`emit_source`] keeps the platform allocator so that callers rendering a
/// snippet — unit tests, snapshot fixtures, the diagnostics tooling — get source
/// with no external dependency. Whole-crate emission goes through here.
pub fn emit_source_with_allocator(
    mir: &Mir,
    allocator: GeneratedAllocator,
) -> Result<String, EmitError> {
    emit_source_with_free_function_router(mir, allocator, |_function, _context, source| {
        Ok(Some(source))
    })
}

/// Emits Rust source code while allowing callers to route free functions.
///
/// The router receives each already-emitted free function exactly once. Returning
/// `Some(source)` keeps the function in the crate root; returning `None` lets the
/// caller store it elsewhere, such as in source-shaped sibling modules.
fn emit_source_with_free_function_router(
    mir: &Mir,
    allocator: GeneratedAllocator,
    mut route_free_function: impl FnMut(
        &MirFunction,
        &EmitContext,
        String,
    ) -> Result<Option<String>, EmitError>,
) -> Result<String, EmitError> {
    let mut writer = CodeWriter::new();
    let context = EmitContext::new(mir)?;
    // Decide, once, which free functions emit real Rust generics (signature
    // safety plus a body-cleanliness trial) so the definition and every call
    // site agree on the generic-vs-erased shape.
    context.populate_generic_functions(mir)?;
    let needs_serde_json =
        stdlib::backend_dependencies(mir).contains(&smelt_stdlib::BackendDependency::SerdeJson);
    let needs_regex =
        stdlib::backend_dependencies(mir).contains(&smelt_stdlib::BackendDependency::Regex);
    let needs_unknown = stdlib::needs_unknown_type(mir);
    let needs_smelt_list = stdlib::needs_smelt_list(mir);
    let needs_erased_function = needs_erased_function_runtime(mir);
    let needs_date_now = stdlib::needs_date_now_runtime(mir);
    let needs_date_timezone_offset = stdlib::needs_date_timezone_offset_runtime(mir);
    let needs_vitest_mock = stdlib::needs_vitest_mock_runtime(mir);
    let needs_structured_clone = stdlib::rvalues(mir)
        .any(|rvalue| matches!(rvalue, Rvalue::StructuredClone { .. }));
    // Only crates that actually concatenate an erased argument need the
    // `IsConcatSpreadable` helper, so it stays out of every other prelude.
    let needs_concat_spread =
        stdlib::rvalues(mir).any(|rvalue| matches!(rvalue, Rvalue::ConcatSpread { .. }));
    // Only crates that actually define properties through the `Object` statics
    // carry the descriptor-installation helper.
    let needs_define_properties =
        stdlib::rvalues(mir).any(|rvalue| matches!(rvalue, Rvalue::DefineProperties { .. }));
    // The dynamically scoped `this` channel is only reachable from the two
    // rvalues that read and install it, so a program that never mentions `this`
    // (and never binds a receiver) carries none of it.
    let needs_this_channel = stdlib::rvalues(mir)
        .any(|rvalue| matches!(rvalue, Rvalue::ThisRead | Rvalue::BindThis { .. }));
    let needs_host_override = stdlib::needs_host_override_runtime(mir);
    let needs_shared_captures = mir
        .closures
        .iter()
        .any(|closure| !closure.captures.is_empty());
    let needs_generator = mir_uses_generators(mir);
    let needs_timer_helpers = needs_timer_helpers(mir);
    let needs_math_round = needs_math_round(mir);
    writer.line("// @generated by smelt. Do not edit by hand.");
    writer.line("#![allow(dead_code, non_snake_case, unused_imports, unused_variables)]");
    writer.blank_line();
    // A `#[global_allocator]` is an ordinary item, not an inner attribute, so it
    // does not have to lead the file — the module-mapped root emits the `#[path]`
    // declarations ahead of everything here, and this lands below them. See
    // `GeneratedAllocator` for why a generated program installs one at all.
    if allocator == GeneratedAllocator::Mimalloc {
        writer.line("#[global_allocator]");
        writer.line("static SMELT_GLOBAL_ALLOCATOR: ::mimalloc::MiMalloc = ::mimalloc::MiMalloc;");
        writer.blank_line();
    }
    if needs_date_now {
        emit_runtime_gate(&mut writer, PreludeGate::DateNow)?;
    }
    if needs_date_timezone_offset {
        emit_runtime_gate(&mut writer, PreludeGate::DateTimezoneOffset)?;
    }
    if needs_timer_helpers || needs_date_now {
        // The clock is one item family in `smelt-runtime` (`clock.rs`), which
        // documents why timers and `Date.now()` must share a timeline.
        emit_runtime_gate(&mut writer, PreludeGate::VirtualClock)?;
    }
    if needs_math_round {
        // JavaScript rounds a tie toward +∞; Rust's `f64::round` rounds a tie away
        // from zero. They disagree for every negative value whose fraction is
        // exactly 0.5 — `Math.round(-1.5)` is `-1` in JavaScript and `-2.0` in
        // Rust — which is what made es-toolkit's `round` specs disagree.
        //
        // Not `(x + 0.5).floor()`: the ECMA-262 note on `Math.round` calls out that
        // the naive form is wrong for very large `x`, where adding `0.5` is not
        // representable. `floor` is exact at those magnitudes and the fraction is
        // then `0`, so the value passes through unchanged.
        //
        // `-0` is preserved: JavaScript's `Math.round(-0.5)` is `-0`, and
        // `Object.is(-0, 0)` is `false`, so a caller that inspects the sign of a
        // zero must see the JavaScript answer. NaN and the infinities pass through
        // because `floor` already leaves them alone.
        writer.line("/// JavaScript `Math.round`: a tie rounds toward +∞, not away from zero.");
        writer.line(format!(
            "fn {helper}(value: f64) -> f64 {{ let floor = value.floor(); let rounded = if value - floor >= 0.5 {{ floor + 1.0 }} else {{ floor }}; if rounded == 0.0 && value.is_sign_negative() {{ -0.0 }} else {{ rounded }} }}",
            helper = smelt_stdlib::runtime_symbols::math::ROUND,
        ));
        writer.blank_line();
    }
    if needs_host_override {
        // Bounded host-global override support: a fixed override-state enum, one
        // `thread_local!` slot per host name the crate reassigns (fresh `Native`
        // per test thread), and the three fixed helpers. Gated on any
        // `globalThis.<Name> =` write, so crates that never reassign a host
        // global emit byte-identical output.
        writer.line("/// Override state of a modeled host constructor's global slot.");
        writer.line("///");
        writer.line("/// `Native` is the unmodified builtin; `Absent` is an explicit");
        writer.line("/// `globalThis.X = undefined`; `Ctor` holds a reassigned constructor value.");
        writer.line(format!(
            "#[derive(Clone)] enum {enum_name} {{ Native, Absent, Ctor(SmeltUnknown) }}",
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
        ));
        writer.blank_line();
        writer.line("thread_local! {");
        for (suffix, _name) in stdlib::host_override_slot_names(mir) {
            writer.line(format!(
                "    static {prefix}{suffix}: ::std::cell::RefCell<{enum_name}> = const {{ ::std::cell::RefCell::new({enum_name}::Native) }};",
                prefix = smelt_stdlib::runtime_symbols::host_override::SLOT_PREFIX,
                enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
            ));
        }
        writer.line("}");
        writer.blank_line();
        writer.line("/// Read a host override slot: native-handle marker for `Native`, the");
        writer.line("/// stored constructor for `Ctor`, JS `undefined` for `Absent`.");
        writer.line(format!(
            "fn {read}(slot: &::std::cell::RefCell<{enum_name}>, name: &str) -> SmeltUnknown {{",
            read = smelt_stdlib::runtime_symbols::host_override::READ,
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
        ));
        writer.line(format!(
            "    match &*slot.borrow() {{ {enum_name}::Native => {{ let entries = Vec::from([({marker:?}.to_owned(), SmeltUnknown::Bool(true)), (\"name\".to_owned(), SmeltUnknown::String(name.into()))]); SmeltUnknown::Object(SmeltObject::new(entries)) }}, {enum_name}::Absent => SmeltUnknown::Undefined, {enum_name}::Ctor(value) => value.clone() }}",
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
            marker = smelt_stdlib::runtime_symbols::host_override::NATIVE_CTOR_MARKER,
        ));
        writer.line("}");
        writer.blank_line();
        writer.line("/// Classify and store a written host-global value; returns the stored value.");
        writer.line("///");
        writer.line("/// JS `undefined` -> `Absent`; a native-handle marker record -> `Native`");
        writer.line("/// (the save/restore round trip); any function/class value -> `Ctor`.");
        writer.line(format!(
            "fn {write}(slot: &::std::cell::RefCell<{enum_name}>, value: SmeltUnknown) -> SmeltUnknown {{",
            write = smelt_stdlib::runtime_symbols::host_override::WRITE,
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
        ));
        writer.line(format!(
            "    let state = match &value {{ SmeltUnknown::Undefined => {enum_name}::Absent, SmeltUnknown::Object(entries) if entries.contains_key({marker:?}) => {enum_name}::Native, _ => {enum_name}::Ctor(value.clone()) }}; *slot.borrow_mut() = state; value",
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
            marker = smelt_stdlib::runtime_symbols::host_override::NATIVE_CTOR_MARKER,
        ));
        writer.line("}");
        writer.blank_line();
        // A class erased to a plain callable loses its constructor identity: an
        // `Rc<dyn Fn>` has no `prototype` and no name, so nothing relates the
        // stored override to the `__smelt_class` its instances carry. The
        // erasure site knows both, and records the class names whose instances
        // are instances of this constructor — the class itself plus every
        // subclass of it in this crate, computed where the hierarchy is known so
        // the runtime needs no class graph.
        writer.line("thread_local! { static SMELT_FUNCTION_CLASSES: ::std::cell::RefCell<::std::collections::HashMap<usize, Vec<&'static str>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }");
        writer.line("/// Record which classes a callable constructs (the class and its subclasses).");
        writer.line("fn smelt_register_function_classes<T: ?Sized + 'static>(function: &::std::rc::Rc<T>, classes: &[&'static str]) { let key = smelt_retain_callable_key(function); SMELT_FUNCTION_CLASSES.with(|registry| { registry.borrow_mut().insert(key, classes.to_vec()); }); }");
        writer.line("/// The classes a stored constructor value constructs, if it is a known one.");
        writer.line("fn smelt_function_classes(value: &SmeltUnknown) -> Option<Vec<&'static str>> { let SmeltUnknown::Function(function) = value else { return None; }; let key = smelt_retain_callable_key(function); SMELT_FUNCTION_CLASSES.with(|registry| registry.borrow().get(&key).cloned()) }");
        writer.blank_line();
        writer.line("/// `value instanceof <HostName>` where the name lives in an override slot.");
        writer.line("///");
        writer.line("/// `instanceof` reads the binding, and an overridable host constructor's");
        writer.line("/// binding is its slot, so the answer follows the slot's state: the native");
        writer.line("/// builtin is recognized by its identity marker(s); a reassigned constructor");
        writer.line("/// is recognized by the class its instances record (which is why a native");
        writer.line("/// record is *not* an instance of a replacement class); and a deleted global");
        writer.line("/// has no instances at all. A stored constructor with no registered class");
        writer.line("/// (an ordinary function assigned into the slot) falls back to the marker");
        writer.line("/// probe rather than answering `false` for the native records still around.");
        writer.line(format!(
            "fn smelt_host_override_instance_of(slot: &::std::cell::RefCell<{enum_name}>, value: &SmeltUnknown, markers: &[&str]) -> bool {{",
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
        ));
        writer.line("    let marker_probe = || matches!(value, SmeltUnknown::Object(map) if markers.iter().any(|marker| map.contains_key(*marker)));");
        writer.line(format!(
            "    match &*slot.borrow() {{ {enum_name}::Absent => false, {enum_name}::Native => marker_probe(), {enum_name}::Ctor(ctor) => match smelt_function_classes(ctor) {{ Some(classes) => matches!(value, SmeltUnknown::Object(map) if matches!(map.get(\"__smelt_class\"), Some(SmeltUnknown::String(class)) if classes.iter().any(|entry| *entry == &*class))), None => marker_probe() }} }}",
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
        ));
        writer.line("}");
        writer.blank_line();
        writer.line("/// Whether a host override slot is present (`false` only when `Absent`).");
        writer.line(format!(
            "fn {present}(slot: &::std::cell::RefCell<{enum_name}>) -> bool {{ !matches!(&*slot.borrow(), {enum_name}::Absent) }}",
            present = smelt_stdlib::runtime_symbols::host_override::PRESENT,
            enum_name = smelt_stdlib::runtime_symbols::host_override::OVERRIDE_ENUM,
        ));
        writer.blank_line();
    }
    if needs_shared_captures {
        emit_runtime_gate(&mut writer, PreludeGate::SharedCaptures)?;
    }
    if stdlib::needs_uri_encode_runtime(mir) {
        emit_runtime_gate(&mut writer, PreludeGate::UriEncode)?;
    }
    if stdlib::needs_locale_compare_runtime(mir) {
        emit_runtime_gate(&mut writer, PreludeGate::LocaleCompare)?;
    }
    // `smelt_next_object_id` mints fresh JavaScript object reference ids. It is
    // emitted in the `needs_smelt_list` block below (a list mints ids), but a
    // regex/match program without any list still needs it for
    // `SmeltMatch::from_captures` (a match carries an id so it keeps a stable
    // identity when later erased to `SmeltUnknown`). Emit it standalone only in
    // that regex-without-list case so list-using programs keep byte-identical
    // output. `needs_smelt_list` already subsumes `needs_unknown`.
    if needs_regex && !needs_smelt_list {
        emit_runtime_gate(&mut writer, PreludeGate::ObjectIdentity)?;
    }
    if needs_smelt_list {
        // `SmeltList` plus the id counter it mints from: see `smelt-runtime`'s
        // `value` and `value::list` modules for the identity semantics. The
        // `SmeltUnknown`-dependent impls (erase / `From<SmeltArray>` / serde) are
        // still emitted by the `needs_unknown` block below.
        emit_runtime_gate(&mut writer, PreludeGate::SmeltList)?;
    }

    if needs_unknown {
        writer.line("use ::std::hash::Hash;");
        writer.blank_line();
        // JavaScript object property lookup goes through these maps on EVERY field
        // read, and `std`'s default `RandomState` is SipHash-1-3: DoS-resistant, and
        // priced accordingly. Profiling es-toolkit's `partition` under callgrind put
        // 16.2% of the whole benchmark in `BuildHasher::hash_one` plus
        // `sip::Hasher::write` — more than six times the 2.7% spent in the transpiled
        // function itself. Property keys are short strings from the program's own
        // source, not attacker-controlled input reaching a server, so the collision
        // resistance buys nothing here.
        //
        // This is the FxHash construction rustc uses on its own symbol tables: one
        // multiply-rotate per 8 bytes, no keying. It is also DETERMINISTIC, which
        // matters beyond speed — `RandomState` seeds per process, so anything that
        // observes map iteration order (and some erased-value paths do) varies run to
        // run. That is the suspected mechanism behind the intermittent remeda
        // `pipe` failure recorded in blocker-logs/smeltlist-shared-buffer.md; a fixed
        // hasher makes such a failure reproduce every run instead of one in six.
        writer.line("#[derive(Default, Clone, Copy)]");
        writer.line("pub struct SmeltFieldHasher(u64);");
        writer.blank_line();
        writer.line("impl ::std::hash::Hasher for SmeltFieldHasher {");
        // FxHash's accumulator has weak avalanche in its high bits, and hashbrown
        // takes its control byte from the TOP 7 bits — so a raw `self.0` collides
        // badly on structured keys. That is not hypothetical: routing the
        // Set/Map member index (`SmeltFieldMap<u64, ..>`) through the unfinalized
        // form cost es-toolkit `unique` 25.4M -> 30.9M instructions, because every
        // extra bucket collision pays a `same_member` comparison and each of those
        // erases a value. The splitmix64 finalizer is six cheap ALU ops and
        // restores full avalanche, which brought that case back and past its
        // starting point.
        writer.line("    fn finish(&self) -> u64 {");
        writer.line("        let mut mixed = self.0;");
        writer.line("        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);");
        writer.line("        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);");
        writer.line("        mixed ^ (mixed >> 31)");
        writer.line("    }");
        writer.line("    fn write(&mut self, bytes: &[u8]) {");
        writer.line("        const SMELT_FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;");
        writer.line("        let mut hash = self.0;");
        writer.line("        let mut rest = bytes;");
        writer.line("        while rest.len() >= 8 {");
        writer.line("            let (head, tail) = rest.split_at(8);");
        writer.line("            let word = u64::from_le_bytes(head.try_into().unwrap_or([0; 8]));");
        writer.line("            hash = (hash.rotate_left(5) ^ word).wrapping_mul(SMELT_FX_SEED);");
        writer.line("            rest = tail;");
        writer.line("        }");
        writer.line("        if !rest.is_empty() {");
        writer.line("            let mut buf = [0_u8; 8];");
        writer.line("            buf[..rest.len()].copy_from_slice(rest);");
        writer.line("            let word = u64::from_le_bytes(buf);");
        writer.line("            hash = (hash.rotate_left(5) ^ word).wrapping_mul(SMELT_FX_SEED);");
        writer.line("        }");
        writer.line("        self.0 = hash;");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("/// The map behind a JavaScript object/record, keyed by property name.");
        writer.line("pub type SmeltFieldMap<K, V> = ::std::collections::HashMap<K, V, ::std::hash::BuildHasherDefault<SmeltFieldHasher>>;");
        writer.blank_line();
        // The property store shared by `SmeltRecord` and `SmeltObject`.
        //
        // The representation it replaces was a `HashMap<K, V>` plus a separate
        // `Vec<K>` holding JavaScript own-key order, each behind its own
        // `Rc<RefCell<..>>`: four heap allocations per object, two stored copies of
        // every key, and a full hash-table probe (hash, control-byte scan, bucket
        // compare) on every property read. Callgrind put `SmeltObject::get` at
        // 17.4% of es-toolkit's `partition` on that shape -- the largest single
        // cost centre in the case -- and object CONSTRUCTION at another large
        // slice on top.
        //
        // JavaScript objects are overwhelmingly SMALL: a handful of named fields.
        // For those a hash table is the wrong structure, and so is hashing at all.
        // This keeps the key/value pairs in ONE `Vec` in own-key order and, below
        // `SMELT_FIELD_SCAN_LIMIT` entries, resolves a property by scanning it and
        // comparing keys directly. `str` equality compares lengths first, so the
        // handful of differently sized names in a record are rejected without
        // touching their bytes -- cheaper than the ~40 instructions it takes to
        // hash even a four-byte key. Objects used as dictionaries do need O(1)
        // lookup, so a `hash -> first position` index is built and maintained once
        // a store grows past that limit; only then is a key ever hashed.
        //
        // Nothing here keys off a field name, a library, or a source spelling: it
        // is the representation every erased object and every record uses.
        writer.line("/// Hash a property key's bytes into a property store's key space.");
        writer.line("///");
        writer.line("/// Only the dictionary-sized path uses this; a record-sized store scans.");
        writer.line("/// Deliberately not routed through `Hash`/`SmeltFieldHasher`: `<str as Hash>::hash`");
        writer.line("/// makes TWO `Hasher::write` calls -- the bytes, then a `0xff` terminator that");
        writer.line("/// disambiguates concatenations -- and each rebuilds an 8-byte staging buffer.");
        writer.line("/// Property names are short, so this folds the length in directly (which serves");
        writer.line("/// the same disambiguating purpose) and takes the <=8-byte case, nearly every");
        writer.line("/// property name, as a single load and mix. No avalanche finalizer: the value");
        writer.line("/// only ever enters the `index` map, which re-hashes it through");
        writer.line("/// `SmeltFieldHasher`, and that finalizes.");
        writer.line("#[inline]");
        writer.line("fn smelt_field_hash_bytes(bytes: &[u8]) -> u64 {");
        writer.line("    const SMELT_FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;");
        writer.line("    let mut hash = bytes.len() as u64;");
        writer.line("    let mut rest = bytes;");
        writer.line("    while rest.len() >= 8 {");
        writer.line("        let (head, tail) = rest.split_at(8);");
        writer.line("        let word = u64::from_le_bytes(head.try_into().unwrap_or([0; 8]));");
        writer.line("        hash = (hash.rotate_left(5) ^ word).wrapping_mul(SMELT_FX_SEED);");
        writer.line("        rest = tail;");
        writer.line("    }");
        writer.line("    if !rest.is_empty() {");
        writer.line("        let mut buf = [0_u8; 8];");
        writer.line("        buf[..rest.len()].copy_from_slice(rest);");
        writer.line("        let word = u64::from_le_bytes(buf);");
        writer.line("        hash = (hash.rotate_left(5) ^ word).wrapping_mul(SMELT_FX_SEED);");
        writer.line("    }");
        writer.line("    hash");
        writer.line("}");
        writer.blank_line();
        // The same-length tie-break the scan was missing.
        //
        // A record-sized store is resolved by scanning `entries` and comparing
        // keys. `str` equality rejects a DIFFERENT-length key for free, but two
        // keys of the SAME length go all the way into `memcmp` -- and a real
        // record is full of those. On the es-toolkit benchmark records
        // (`id`/`group`/`value`/`flag`) a read of `"value"` paid a full failing
        // `memcmp` against `"group"` before reaching its own entry, which is why
        // callgrind put `__memcmp_avx2_movbe` at 15.3% of `sum_by` next to
        // `position`'s 21.8%.
        //
        // This is NOT the `smelt_key_hash` the docstring below rules out: it is
        // not a hash function, it is three loads and two shifts, and it is stored
        // ALONGSIDE the key so a mismatching entry is rejected from the entries
        // vector itself, without dereferencing the key's heap buffer at all.
        // Length goes in the high half so it still subsumes the free
        // length-rejection the scan already had.
        writer.line("/// A cheap same-length tie-break for a property key: length, first byte, last byte.");
        writer.line("///");
        writer.line("/// Equal keys MUST produce equal fingerprints, so every `SmeltPropertyKey`");
        writer.line("/// impl delegates here rather than computing its own -- a `String` entry and");
        writer.line("/// the `&str` a lookup borrows against it must agree or the lookup would");
        writer.line("/// report a present key absent. Unequal keys may collide; a fingerprint hit");
        writer.line("/// is always confirmed with full key equality.");
        writer.line("#[inline]");
        writer.line("fn smelt_field_fingerprint(bytes: &[u8]) -> u32 {");
        writer.line("    let first = bytes.first().copied().unwrap_or(0) as u32;");
        writer.line("    let last = bytes.last().copied().unwrap_or(0) as u32;");
        writer.line("    ((bytes.len() as u32) << 16) | (first << 8) | last");
        writer.line("}");
        writer.blank_line();
        writer.line("/// One property of a store: its key, its value, and the key's fingerprint.");
        writer.line("///");
        writer.line("/// `fingerprint` is derived from `key` at insert time and is never updated");
        writer.line("/// independently -- the key of an existing entry is never rewritten, only its");
        writer.line("/// value is -- so the two cannot drift apart.");
        writer.line("#[derive(Debug)]");
        writer.line("pub struct SmeltFieldEntry<K, V> {");
        writer.line("    fingerprint: u32,");
        writer.line("    key: K,");
        writer.line("    value: V,");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Entries a property store scans linearly before it builds a hash index.");
        writer.line("///");
        writer.line("/// Set where an object stops looking like a record and starts looking like a");
        writer.line("/// dictionary: below it a length-first key scan beats hashing plus a probe,");
        writer.line("/// above it dictionary-shaped objects need their O(1) lookup back.");
        writer.line("const SMELT_FIELD_SCAN_LIMIT: usize = 12;");
        writer.blank_line();
        writer.line("/// The ordered property store behind a JavaScript object or record.");
        writer.line("///");
        writer.line("/// `entries` is held in JavaScript own-key order (array-index keys first, in");
        writer.line("/// ascending numeric order, then the remaining string keys in insertion");
        writer.line("/// order), so `keys()`/`iter()` read the order as a fact rather than");
        writer.line("/// re-deriving it. `index` maps a key hash to the FIRST entry position");
        writer.line("/// carrying it and exists ONLY while the store is larger than");
        writer.line("/// `SMELT_FIELD_SCAN_LIMIT` — a small store never hashes a key at all.");
        writer.line("#[derive(Debug)]");
        writer.line("pub struct SmeltFieldStore<K, V> {");
        writer.line("    entries: Vec<SmeltFieldEntry<K, V>>,");
        writer.line("    index: Option<SmeltFieldMap<u64, usize>>,");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K, V> SmeltFieldStore<K, V> {");
        writer.line("    /// Build an empty store sized for `capacity` entries.");
        writer.line("    fn with_capacity(capacity: usize) -> Self { Self { entries: Vec::with_capacity(capacity), index: None } }");
        writer.line("    #[inline]");
        writer.line("    fn len(&self) -> usize { self.entries.len() }");
        writer.line("    /// Borrow the key/value pairs in JavaScript own-key order.");
        writer.line("    #[inline]");
        writer.line("    fn entries(&self) -> &[SmeltFieldEntry<K, V>] { &self.entries }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K, V> Default for SmeltFieldStore<K, V> {");
        writer.line("    fn default() -> Self { Self { entries: Vec::new(), index: None } }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + SmeltPropertyKey, V> SmeltFieldStore<K, V> {");
        writer.line("    /// Rebuild the hash index, or drop it when the store is small enough to scan.");
        writer.line("    fn reindex(&mut self) {");
        writer.line("        if self.entries.len() <= SMELT_FIELD_SCAN_LIMIT { self.index = None; return; }");
        writer.line("        let mut index: SmeltFieldMap<u64, usize> = SmeltFieldMap::with_capacity_and_hasher(self.entries.len(), ::std::default::Default::default());");
        writer.line("        for (position, entry) in self.entries.iter().enumerate() { index.entry(entry.key.smelt_key_hash()).or_insert(position); }");
        writer.line("        self.index = Some(index);");
        writer.line("    }");
        writer.line("    /// Return the entry position holding `key`, or `None`.");
        writer.line("    ///");
        writer.line("    /// A small store is SCANNED, comparing keys directly: `str` equality checks");
        writer.line("    /// the length first, so a handful of differently sized property names are");
        writer.line("    /// rejected without touching their bytes, and the key is never hashed. That");
        writer.line("    /// is the whole point — hashing a short key costs more than the scan it");
        writer.line("    /// would save. Two keys of the SAME length are the case that costs, so");
        writer.line("    /// each entry carries a `smelt_field_fingerprint` — length, first byte,");
        writer.line("    /// last byte — that is compared first and rejects such a key out of the");
        writer.line("    /// entries vector, without dereferencing its bytes. A fingerprint hit is");
        writer.line("    /// still confirmed with full key equality, so the answer is unchanged.");
        writer.line("    /// Past `SMELT_FIELD_SCAN_LIMIT` entries the store is a");
        writer.line("    /// dictionary rather than a record, and the hash index resolves the");
        writer.line("    /// position directly; a hash collision between two DISTINCT keys is");
        writer.line("    /// vanishingly unlikely but not impossible, so a failed key confirmation");
        writer.line("    /// falls back to the scan rather than reporting the key absent.");
        writer.line("    #[inline]");
        writer.line("    fn position<Q>(&self, key: &Q) -> Option<usize> where K: ::std::borrow::Borrow<Q>, Q: Eq + SmeltPropertyKey + ?Sized {");
        writer.line("        let fingerprint = key.smelt_key_fingerprint();");
        writer.line("        if let Some(index) = self.index.as_ref() {");
        writer.line("            let start = *index.get(&key.smelt_key_hash())?;");
        writer.line("            let entry = &self.entries[start];");
        writer.line("            if entry.fingerprint == fingerprint && entry.key.borrow() == key { return Some(start); }");
        writer.line("        }");
        writer.line("        self.entries.iter().position(|entry| entry.fingerprint == fingerprint && entry.key.borrow() == key)");
        writer.line("    }");
        writer.line("    #[inline]");
        writer.line("    fn get<Q>(&self, key: &Q) -> Option<&V> where K: ::std::borrow::Borrow<Q>, Q: Eq + SmeltPropertyKey + ?Sized {");
        writer.line("        self.position(key).map(|position| &self.entries[position].value)");
        writer.line("    }");
        writer.line("    fn get_mut<Q>(&mut self, key: &Q) -> Option<&mut V> where K: ::std::borrow::Borrow<Q>, Q: Eq + SmeltPropertyKey + ?Sized {");
        writer.line("        let position = self.position(key)?;");
        writer.line("        Some(&mut self.entries[position].value)");
        writer.line("    }");
        writer.line("    #[inline]");
        writer.line("    fn contains_key<Q>(&self, key: &Q) -> bool where K: ::std::borrow::Borrow<Q>, Q: Eq + SmeltPropertyKey + ?Sized {");
        writer.line("        self.position(key).is_some()");
        writer.line("    }");
        writer.line("    /// Remove `key`, returning its value; later positions shift, so reindex.");
        writer.line("    fn remove<Q>(&mut self, key: &Q) -> Option<V> where K: ::std::borrow::Borrow<Q>, Q: Eq + SmeltPropertyKey + ?Sized {");
        writer.line("        let position = self.position(key)?;");
        writer.line("        let value = self.entries.remove(position).value;");
        writer.line("        self.reindex();");
        writer.line("        Some(value)");
        writer.line("    }");
        writer.line("    /// Insert or overwrite `key`, keeping JavaScript own-key order.");
        writer.line("    ///");
        writer.line("    /// A new key almost always appends, which keeps the index valid with a");
        writer.line("    /// single added mapping; only an array-index key landing before an existing");
        writer.line("    /// entry shifts later positions and forces a rebuild.");
        writer.line("    fn insert(&mut self, key: K, value: V) -> Option<V> {");
        writer.line("        if let Some(position) = self.position(&key) { return Some(::std::mem::replace(&mut self.entries[position].value, value)); }");
        writer.line("        let position = smelt_js_key_order_position(&self.entries, &key);");
        writer.line("        if position == self.entries.len() {");
        writer.line("            let hash = if self.index.is_some() { key.smelt_key_hash() } else { 0 };");
        writer.line("            let fingerprint = key.smelt_key_fingerprint();");
        writer.line("            self.entries.push(SmeltFieldEntry { fingerprint, key, value });");
        writer.line("            match self.index.as_mut() {");
        writer.line("                Some(index) => { index.entry(hash).or_insert(position); }");
        writer.line("                None => self.reindex(),");
        writer.line("            }");
        writer.line("        } else {");
        writer.line("            let fingerprint = key.smelt_key_fingerprint();");
        writer.line("            self.entries.insert(position, SmeltFieldEntry { fingerprint, key, value });");
        writer.line("            self.reindex();");
        writer.line("        }");
        writer.line("        None");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("#[derive(Debug)]");
        writer.line("pub struct SmeltRecord<K, V> {");
        writer.line("    id: usize,");
        writer.line(
            "    store: ::std::rc::Rc<::std::cell::RefCell<SmeltFieldStore<K, V>>>,",
        );
        writer.line("}");
        writer.blank_line();
        // `smelt_next_object_id` is emitted in the `needs_smelt_list` block above
        // (which `needs_unknown` always implies), so it is in scope here.
        writer.line("thread_local! {");
        writer.line("    static SMELT_PROMISE_IDENTITIES: ::std::cell::RefCell<::std::collections::HashMap<usize, usize>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("thread_local! {");
        writer.line(
            "    /// Map a reference record's shared cell address to a stable erased id.",
        );
        writer.line("    static SMELT_REFERENCE_OBJECT_IDENTITIES: ::std::cell::RefCell<::std::collections::HashMap<usize, usize>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Return a stable erased-object id for a reference record's shared cell.");
        writer.line("///");
        writer.line("/// A reference record (a class or object shape whose fields are written");
        writer.line("/// after construction) is a handle over one `Rc<RefCell<..>>`, and every");
        writer.line("/// handle on that cell is the SAME JavaScript object. Erasing any of them");
        writer.line("/// must therefore produce an object that compares `===` equal to the");
        writer.line("/// others, which means one id per cell rather than a fresh id per erasure.");
        writer.line("/// Keying on the live cell's address gives exactly that. The handle is");
        writer.line("/// alive at the call site, so the address is valid; a freed cell can see");
        writer.line("/// its address reused, which at worst hands a stale id to a NEW object no");
        writer.line("/// live erased alias of the old one can still be compared against.");
        writer.line("#[allow(dead_code)]");
        writer.line("fn smelt_reference_object_identity(cell_address: usize) -> usize {");
        writer.line("    SMELT_REFERENCE_OBJECT_IDENTITIES.with(|identities| {");
        writer.line("        let mut identities = identities.borrow_mut();");
        writer.line("        if let Some(id) = identities.get(&cell_address) { return *id; }");
        writer.line("        let id = smelt_next_object_id();");
        writer.line("        identities.insert(cell_address, id);");
        writer.line("        id");
        writer.line("    })");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Return a stable erased promise id for a source future local.");
        writer.line("fn smelt_promise_identity(source_key: usize) -> usize {");
        writer.line("    SMELT_PROMISE_IDENTITIES.with(|identities| {");
        writer.line("        let mut identities = identities.borrow_mut();");
        writer.line("        if let Some(id) = identities.get(&source_key) { return *id; }");
        writer.line("        let id = smelt_next_object_id();");
        writer.line("        identities.insert(source_key, id);");
        writer.line("        id");
        writer.line("    })");
        writer.line("}");
        writer.blank_line();
        writer.line("thread_local! {");
        writer
            .line("    /// Map a source list's storage address to a stable erased-array identity.");
        writer.line("    static SMELT_LIST_IDENTITIES: ::std::cell::RefCell<::std::collections::HashMap<usize, usize>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line(
            "/// Return a stable erased-array id for a source list keyed on its `Vec` address.",
        );
        writer.line("///");
        writer.line("/// Erasing the SAME source list local twice must yield arrays that compare");
        writer.line("/// `===` equal (arrays compare by id). Keying on the live `Vec`'s storage");
        writer.line("/// address lets every erasure of one binding reuse a single id, while a");
        writer.line("/// fresh list (literal or transform result) still gets a fresh id from");
        writer.line("/// `SmeltArray::new`. KNOWN LIMITATION: `Vec::as_ptr` on an EMPTY `Vec`");
        writer.line("/// returns a shared dangling sentinel, so distinct empty list bindings can");
        writer.line("/// collide on one id; acceptable here because the targeted cases are");
        writer.line("/// non-empty.");
        writer.line("fn smelt_list_identity(source_key: usize) -> usize {");
        writer.line("    SMELT_LIST_IDENTITIES.with(|identities| {");
        writer.line("        let mut identities = identities.borrow_mut();");
        writer.line("        if let Some(id) = identities.get(&source_key) { return *id; }");
        writer.line("        let id = smelt_next_object_id();");
        writer.line("        identities.insert(source_key, id);");
        writer.line("        id");
        writer.line("    })");
        writer.line("}");
        writer.blank_line();
        // Address reuse in the identity registries.
        //
        // Every registry below (`SMELT_FUNCTION_ORIGINS`,
        // `SMELT_FUNCTION_IDENTITIES`, `SMELT_FUNCTION_LENGTHS`,
        // `SMELT_CALLABLE_OBJECTS`) keys on the ADDRESS of an `Rc` allocation and
        // never removes an entry, because there is no drop hook to remove it
        // from. The allocator, meanwhile, happily hands a freed address to the
        // next allocation of the same size — so a fresh callable can land on a
        // dead callable's address and inherit its registry entries. That is not
        // theoretical: it is what made remeda's lazy `pipe` fail intermittently
        // under `cargo test`'s thread-per-test scheduling. A `map(cb)` lazy
        // evaluator allocated at a recycled address hit a stale
        // `SMELT_CALLABLE_OBJECTS` entry, so `prepareLazyFunction` received the
        // PREVIOUS operation's `{ __smelt_call: dataLast, lazy, lazyArgs }`
        // callable object instead of the evaluator, and `pipe` then invoked
        // `dataLast(item)` — routing one ITEM into the ARRAY parameter of
        // `map`'s data-first implementation.
        //
        // The fix reserves the address for as long as a registry can name it.
        // Holding a `Weak` keeps the `RcBox` block allocated even after the last
        // strong handle is gone (the value itself is still dropped, so captured
        // state is released), which makes the address unreusable and therefore
        // makes every key unique to one allocation for the life of the thread.
        // Growth matches the registries this guards, which are already unbounded.
        writer.line("thread_local! {");
        writer.line("    /// Weak handles reserving every address used as an identity-registry key.");
        writer.line("    static SMELT_CALLABLE_KEY_GUARDS: ::std::cell::RefCell<::std::collections::HashMap<usize, Box<dyn ::std::any::Any>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Reserve a callable allocation's address so no later allocation can reuse it.");
        writer.line("///");
        writer.line("/// Call this before storing that address in ANY identity registry, as a key or");
        writer.line("/// as a canonical-identity value. Returns the reserved key for convenience.");
        writer.line("fn smelt_retain_callable_key<F: ?Sized + 'static>(function: &::std::rc::Rc<F>) -> usize {");
        writer.line("    let key = smelt_callable_object_key(function);");
        writer.line("    SMELT_CALLABLE_KEY_GUARDS.with(|guards| {");
        writer.line("        guards.borrow_mut().entry(key).or_insert_with(|| Box::new(::std::rc::Rc::downgrade(function)) as Box<dyn ::std::any::Any>);");
        writer.line("    });");
        writer.line("    key");
        writer.line("}");
        writer.blank_line();
        writer.line("/// The canonical JavaScript identity of a callable, reserving its address first.");
        writer.line("///");
        writer.line("/// Use this instead of `smelt_function_identity_of(smelt_callable_object_key(..))`");
        writer.line("/// wherever the result is STORED, so an unlinked callable's own address cannot be");
        writer.line("/// recycled underneath the registry entry that names it.");
        writer.line("fn smelt_canonical_function_identity<F: ?Sized + 'static>(function: &::std::rc::Rc<F>) -> usize {");
        writer.line("    smelt_function_identity_of(smelt_retain_callable_key(function))");
        writer.line("}");
        writer.blank_line();
        writer.line("thread_local! {");
        writer.line("    static SMELT_FUNCTION_ORIGINS: ::std::cell::RefCell<::std::collections::HashMap<usize, Box<dyn ::std::any::Any>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Return the stable key for an erased callback wrapper.");
        writer.line("fn smelt_erased_function_key(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>) -> usize {");
        writer.line("    ::std::rc::Rc::as_ptr(function) as *const () as usize");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Retain typed callback identity while it crosses an erased ABI.");
        writer.line("fn smelt_register_function_origin<T: Clone + 'static>(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>, origin: T) {");
        writer.line("    smelt_retain_callable_key(function);");
        writer.line("    SMELT_FUNCTION_ORIGINS.with(|origins| { origins.borrow_mut().insert(smelt_erased_function_key(function), Box::new(origin)); });");
        writer.line("}");
        writer.blank_line();
        // JavaScript reference identity for an erased callable.
        //
        // Erasing a typed callback builds a fresh forwarding adapter, so two
        // erasures of the SAME source callable produced two distinct `Rc`s and
        // `js_strict_eq`'s `Rc::ptr_eq` called them unequal — `f === f` read
        // `false`. Named function *items* already dodge this through the
        // per-item `__smelt_fn_value_<key>()` accessor, which caches one erased
        // value, but a closure bound to a local (`const f = () => {}`) has no
        // compile-time key: its identity exists only at runtime.
        //
        // So each adapter records the address of the callable it wraps, and
        // `===` compares those. This is sound despite address reuse: an adapter
        // owns a clone of its source, so a live adapter's recorded address
        // cannot have been recycled, and the entry for a reused adapter address
        // is overwritten when the new adapter is built. Growth matches the
        // sibling `SMELT_FUNCTION_ORIGINS` / `SMELT_CALLABLE_OBJECTS` registries.
        writer.line("thread_local! {");
        writer.line("    static SMELT_FUNCTION_IDENTITIES: ::std::cell::RefCell<::std::collections::HashMap<usize, usize>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("/// The JavaScript function identity of a callable allocation.");
        writer.line("///");
        writer.line("/// Defaults to the allocation's own address, so two callables that were never");
        writer.line("/// linked stay distinct.");
        writer.line("fn smelt_function_identity_of(key: usize) -> usize { SMELT_FUNCTION_IDENTITIES.with(|identities| identities.borrow().get(&key).copied()).unwrap_or(key) }");
        writer.blank_line();
        writer.line("/// Record that `derived` wraps `origin`, so both denote one JavaScript function.");
        writer.line("///");
        writer.line("/// Stores `origin`'s CANONICAL identity rather than its address, so a chain of");
        writer.line("/// wrappers (erase, extract, erase again) collapses to one id without a walk.");
        writer.line("fn smelt_link_function_identity<D: ?Sized + 'static, O: ?Sized + 'static>(derived: &::std::rc::Rc<D>, origin: &::std::rc::Rc<O>) { smelt_link_function_identity_key(derived, smelt_canonical_function_identity(origin)); }");
        writer.blank_line();
        writer.line("/// Link `derived` to an already-resolved canonical identity.");
        writer.line("///");
        writer.line("/// Needed where the origin callable is moved into the wrapper being built, so");
        writer.line("/// its identity has to be read before the move.");
        writer.line("fn smelt_link_function_identity_key<D: ?Sized + 'static>(derived: &::std::rc::Rc<D>, canonical: usize) { let key = smelt_retain_callable_key(derived); SMELT_FUNCTION_IDENTITIES.with(|identities| { identities.borrow_mut().insert(key, canonical); }); }");
        writer.blank_line();
        // One canonical JavaScript identity per (defining class, method).
        //
        // In JavaScript a method lives ONCE, on the prototype, so every read of
        // it denotes the same function value: `a.m === b.m === C.prototype.m`.
        // A generated method reference cannot be that single allocation — it has
        // to capture its receiver to stay callable — so each read builds a fresh
        // `Rc` and a bare address comparison answered `false`.
        //
        // The identity registry above already exists for exactly this shape
        // (erase/extract wrappers of one source callable); it needs a canonical
        // key that no allocation owns. `smelt_method_identity` mints one leaked
        // byte per key: a unique address that stays live for the life of the
        // process, so it can never be recycled underneath the registry, and one
        // that every read of the same (class, method) pair resolves to.
        writer.line("thread_local! {");
        writer.line("    /// Canonical identity address of each class method, keyed by `Class::method`.");
        writer.line("    static SMELT_METHOD_IDENTITIES: ::std::cell::RefCell<::std::collections::HashMap<&'static str, usize>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("/// The canonical JavaScript identity of one class method.");
        writer.line("///");
        writer.line("/// Allocated once per key and never freed, so it is distinct from every");
        writer.line("/// callable allocation and stable across every read of the method.");
        writer.line("fn smelt_method_identity(key: &'static str) -> usize {");
        writer.line("    SMELT_METHOD_IDENTITIES.with(|identities| *identities.borrow_mut().entry(key).or_insert_with(|| ::std::boxed::Box::leak(::std::boxed::Box::new(0u8)) as *const u8 as usize))");
        writer.line("}");
        writer.blank_line();
        // `Function.prototype.length` across the erasure boundary.
        //
        // A typed callable knows its own arity, and `SmeltErasedFunction` carries it
        // in a `length` field. Erasing that value to `SmeltUnknown::Function(Rc<…>)`
        // throws the field away — an `Rc<dyn Fn>` has nowhere to put it — so a
        // `.length` read on an erased callable answered `0`. Real code branches on
        // it: es-toolkit `rest(func)` defaults its split point to `func.length - 1`,
        // so an answer of `0` made the default `-1` and every rest-parameter spec
        // reshaped its arguments wrongly.
        //
        // Keyed and read through the CANONICAL identity, so a chain of erasure
        // wrappers resolves to the arity of the function the chain started from,
        // exactly as `smelt_same_function_identity` resolves equality.
        writer.line("thread_local! {");
        writer.line("    /// Source arity of each erased callable, keyed by canonical identity.");
        writer.line("    static SMELT_FUNCTION_LENGTHS: ::std::cell::RefCell<::std::collections::HashMap<usize, f64>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Record an erased callable's `Function.prototype.length`.");
        writer.line(format!(
            "fn {register}<T: ?Sized + 'static>(function: &::std::rc::Rc<T>, length: f64) {{ let key = smelt_canonical_function_identity(function); SMELT_FUNCTION_LENGTHS.with(|lengths| {{ lengths.borrow_mut().insert(key, length); }}); }}",
            register = smelt_stdlib::runtime_symbols::function_length::REGISTER,
        ));
        writer.blank_line();
        writer.line("/// Read `Function.prototype.length` off an erased value.");
        writer.line("///");
        writer.line("/// A callable object (`{ __smelt_call }`) reports the arity of the callable it");
        writer.line("/// carries; anything else reports `0`, which is what JavaScript answers for a");
        writer.line("/// value with no `length` property.");
        writer.line(format!(
            "fn {read}(value: &SmeltUnknown) -> f64 {{ let function = match value {{ SmeltUnknown::Function(function) => Some(function.clone()), SmeltUnknown::Object(object) => match object.get(\"__smelt_call\") {{ Some(SmeltUnknown::Function(function)) => Some(function), _ => None }}, _ => None }}; let Some(function) = function else {{ return 0.0; }}; let key = smelt_function_identity_of(smelt_callable_object_key(&function)); SMELT_FUNCTION_LENGTHS.with(|lengths| lengths.borrow().get(&key).copied()).unwrap_or(0.0) }}",
            read = smelt_stdlib::runtime_symbols::function_length::READ,
        ));
        writer.blank_line();
        writer.line("/// Whether two callables denote the same JavaScript function value.");
        writer.line("///");
        writer.line("/// Two allocations can denote one source function once a wrapper sits between");
        writer.line("/// them (an erasure adapter, or the typed callback recovered back out of one),");
        writer.line("/// so a bare `Rc::ptr_eq` is not sufficient. Accepts differently-typed handles");
        writer.line("/// because `===` can compare a typed callback against an erased one.");
        writer.line("fn smelt_same_function_identity<L: ?Sized, R: ?Sized>(left: &::std::rc::Rc<L>, right: &::std::rc::Rc<R>) -> bool { let left_key = smelt_callable_object_key(left); let right_key = smelt_callable_object_key(right); left_key == right_key || smelt_function_identity_of(left_key) == smelt_function_identity_of(right_key) }");
        writer.blank_line();
        writer.line("/// Whether two erased callables are the same JavaScript function value.");
        writer.line("fn smelt_same_erased_function(left: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>, right: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>) -> bool { smelt_same_function_identity(left, right) }");
        writer.blank_line();
        writer.line("/// Recover a typed callback previously passed through an erased ABI.");
        writer.line("fn smelt_restore_function_origin<T: Clone + 'static>(function: &::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>) -> Option<T> {");
        writer.line("    SMELT_FUNCTION_ORIGINS.with(|origins| origins.borrow().get(&smelt_erased_function_key(function)).and_then(|origin| origin.downcast_ref::<T>()).cloned())");
        writer.line("}");
        writer.blank_line();
        // A JavaScript "callable object" (a function with attached own
        // properties, e.g. remeda's `map(cb)` carrying `.lazy`/`.lazyArgs`)
        // erases to `SmeltUnknown::Object { __smelt_call, ...props }`. When such
        // a value is narrowed to a CONCRETE typed callback (`Rc<dyn Fn(..)>`),
        // the sibling properties have nowhere to live on the bare `Rc`, so a
        // naive round-trip back to `SmeltUnknown` would forget them and yield a
        // plain `SmeltUnknown::Function`. `SmeltErasedFunction` solves the same
        // problem with its `object` field; typed `Rc` callbacks instead stash
        // the originating object here, keyed by the callback allocation address
        // (stable across `Rc::clone`). This is a genuine dynamic boundary
        // (erased callable identity), not avoidable erasure: it PRESERVES a
        // concrete object shape that would otherwise be lost.
        writer.line("thread_local! {");
        writer.line("    static SMELT_CALLABLE_OBJECTS: ::std::cell::RefCell<::std::collections::HashMap<usize, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Stable key for a typed callback allocation (address of its `Rc`).");
        writer.line("fn smelt_callable_object_key<F: ?Sized>(function: &::std::rc::Rc<F>) -> usize {");
        writer.line("    ::std::rc::Rc::as_ptr(function) as *const () as usize");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Remember the callable object a typed callback was narrowed from.");
        writer.line("fn smelt_register_callable_object<F: ?Sized + 'static>(function: &::std::rc::Rc<F>, object: SmeltUnknown) {");
        writer.line("    if let SmeltUnknown::Object(_) = &object {");
        writer.line("        let key = smelt_retain_callable_key(function);");
        writer.line("        SMELT_CALLABLE_OBJECTS.with(|objects| { objects.borrow_mut().insert(key, object); });");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Recover the callable object a typed callback was narrowed from.");
        writer.line("fn smelt_lookup_callable_object<F: ?Sized>(function: &::std::rc::Rc<F>) -> Option<SmeltUnknown> {");
        writer.line("    SMELT_CALLABLE_OBJECTS.with(|objects| objects.borrow().get(&smelt_callable_object_key(function)).cloned())");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K, V> Clone for SmeltRecord<K, V> {");
        writer.line(
            "    fn clone(&self) -> Self { Self { id: self.id, store: self.store.clone() } }",
        );
        writer.line("}");
        writer.blank_line();
        if needs_serde_json {
            writer.line("impl<K, V> serde::Serialize for SmeltRecord<K, V> where K: Eq + ::std::hash::Hash + Clone + serde::Serialize, V: serde::Serialize {");
            writer.line("    /// Serialize record entries in JavaScript insertion order.");
            writer.line("    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {");
            writer.line("        let store = self.store.borrow();");
            writer.line("        let mut map = serde::Serializer::serialize_map(serializer, Some(store.len()))?;");
            writer.line("        for entry in store.entries() { serde::ser::SerializeMap::serialize_entry(&mut map, &entry.key, &entry.value)?; }");
            writer.line("        serde::ser::SerializeMap::end(map)");
            writer.line("    }");
            writer.line("}");
            writer.blank_line();
        }
        writer.line("trait SmeltOwnedOptionCloned<T> {");
        writer.line("    /// Return owned optional values unchanged when generated shared lookup code calls `.cloned()`.");
        writer.line("    fn cloned(self) -> Option<T>;");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<T> SmeltOwnedOptionCloned<T> for Option<T> {");
        writer.line("    fn cloned(self) -> Option<T> { self }");
        writer.line("}");
        writer.blank_line();
        // JavaScript own-property key order (`OrdinaryOwnPropertyKeys`) is NOT
        // plain insertion order: keys that spell a canonical array index come
        // first in ascending numeric order, and only then do the remaining
        // string keys follow in insertion order. `SmeltRecord`/`SmeltObject`
        // therefore keep `order` in that canonical order at INSERT time — one
        // ordered structure, so `keys()`/`iter()`/`serialize` can read the order
        // as a fact instead of re-deriving (or, as they once did, re-sorting) it.
        writer.line("/// Return the canonical array index a JavaScript property key spells.");
        writer.line("///");
        writer.line("/// Only the canonical decimal spelling of a `u32` below `u32::MAX` is an");
        writer.line("/// array index, so `\"01\"`, `\"+1\"`, `\"1.0\"` and `\"-1\"` stay string keys.");
        writer.line("fn smelt_canonical_array_index(key: &str) -> Option<u32> {");
        writer.line("    if key.is_empty() || key.len() > 10 { return None; }");
        writer.line("    if key.len() > 1 && key.starts_with('0') { return None; }");
        writer.line("    if !key.bytes().all(|byte| byte.is_ascii_digit()) { return None; }");
        writer.line("    key.parse::<u32>().ok().filter(|index| *index != u32::MAX)");
        writer.line("}");
        writer.blank_line();
        writer.line("/// A key of an erased JavaScript object or record.");
        writer.line("///");
        writer.line("/// Generated code only ever instantiates records with `String` keys; the");
        writer.line("/// trait exists so the shared ordering logic can ask a generic `K` whether");
        writer.line("/// it spells an array index without the containers hard-coding `String`,");
        writer.line("/// and so a lookup can hash a borrowed `str` key the same way the stored");
        writer.line("/// `String` was hashed. Both impls must agree on `smelt_key_hash` and on");
        writer.line("/// `smelt_key_fingerprint`, which is why both delegate to the shared");
        writer.line("/// `smelt_field_hash_bytes`/`smelt_field_fingerprint` functions.");
        writer.line("pub trait SmeltPropertyKey {");
        writer.line("    fn smelt_array_index(&self) -> Option<u32>;");
        writer.line("    fn smelt_key_hash(&self) -> u64;");
        writer.line("    /// The stored-entry tie-break for this key; see `smelt_field_fingerprint`.");
        writer.line("    fn smelt_key_fingerprint(&self) -> u32;");
        writer.line("}");
        writer.blank_line();
        writer.line("impl SmeltPropertyKey for str { fn smelt_array_index(&self) -> Option<u32> { smelt_canonical_array_index(self) } #[inline] fn smelt_key_hash(&self) -> u64 { smelt_field_hash_bytes(self.as_bytes()) } #[inline] fn smelt_key_fingerprint(&self) -> u32 { smelt_field_fingerprint(self.as_bytes()) } }");
        writer.line("impl SmeltPropertyKey for String { fn smelt_array_index(&self) -> Option<u32> { smelt_canonical_array_index(self) } #[inline] fn smelt_key_hash(&self) -> u64 { smelt_field_hash_bytes(self.as_bytes()) } #[inline] fn smelt_key_fingerprint(&self) -> u32 { smelt_field_fingerprint(self.as_bytes()) } }");
        writer.blank_line();
        writer.line("/// Return the position a newly inserted key takes in a JavaScript own-key order.");
        writer.line("///");
        writer.line("/// `entries` keeps its array-index keys as a sorted leading run, so the slot");
        writer.line("/// is found with two binary searches: one for the end of that run, one for");
        writer.line("/// the ascending slot inside it. A non-index key appends. Appending keys in");
        writer.line("/// ascending index order therefore stays linear overall, never quadratic.");
        writer.line("fn smelt_js_key_order_position<K: SmeltPropertyKey, V>(entries: &[SmeltFieldEntry<K, V>], key: &K) -> usize {");
        writer.line("    let Some(index) = key.smelt_array_index() else { return entries.len() };");
        writer.line("    let indexed = entries.partition_point(|entry| entry.key.smelt_array_index().is_some());");
        writer.line("    entries[..indexed].partition_point(|entry| entry.key.smelt_array_index().is_some_and(|existing| existing < index))");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V> SmeltRecord<K, V> {");
        writer.line("    fn new() -> Self { Self { id: smelt_next_object_id(), store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltFieldStore::default())) } }");
        writer.line("    fn with_id_from_entries<I: IntoIterator<Item = (K, V)>>(id: usize, iter: I) -> Self { let record = Self { id, store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltFieldStore::default())) }; record.extend(iter); record }");
        writer.line("    fn len(&self) -> usize { self.store.borrow().len() }");
        writer.line("    fn contains_key<Q>(&self, key: &Q) -> bool where K: ::std::borrow::Borrow<Q>, Q: Eq + ::std::hash::Hash + SmeltPropertyKey + ?Sized { self.store.borrow().contains_key(key) }");
        writer.line("    fn insert(&self, key: K, value: V) -> Option<V> { self.store.borrow_mut().insert(key, value) }");
        writer.line("    fn remove<Q>(&self, key: &Q) -> Option<V> where K: ::std::borrow::Borrow<Q>, Q: Eq + ::std::hash::Hash + SmeltPropertyKey + ?Sized { self.store.borrow_mut().remove(key) }");
        writer.line("    fn get<Q>(&self, key: &Q) -> Option<V> where K: ::std::borrow::Borrow<Q>, Q: Eq + ::std::hash::Hash + SmeltPropertyKey + ?Sized, V: Clone { self.store.borrow().get(key).cloned() }");
        writer.line("    fn iter(&self) -> ::std::vec::IntoIter<(K, V)> where V: Clone { self.store.borrow().entries().iter().map(|entry| (entry.key.clone(), entry.value.clone())).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn keys(&self) -> ::std::vec::IntoIter<K> { self.store.borrow().entries().iter().map(|entry| entry.key.clone()).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn values(&self) -> ::std::vec::IntoIter<V> where V: Clone { self.store.borrow().entries().iter().map(|entry| entry.value.clone()).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn extend<I: IntoIterator<Item = (K, V)>>(&self, iter: I) { for (key, value) in iter { self.insert(key, value); } }");
        // The default is a CLOSURE, not a value. A JavaScript accumulator loop
        // (`groupBy`, `countBy`, `uniqBy`) calls this once per element and the key
        // is already present for all but the first, so a by-value default built an
        // empty `SmeltList` — two heap allocations — for every element and threw it
        // away. Building it only on the absent path costs nothing.
        //
        // The `SmeltRecord` twin of `SmeltJsMap::entry_or_insert`: borrow the
        // stored value for in-place mutation instead of copying it out and back.
        // `insert` already maintains the JavaScript own-key order, so routing the
        // absent-key case through it keeps `keys()`/`iter()` ordering identical to
        // the copy-back form this replaces. Takes `&self` because a record is a
        // reference value with interior mutability, exactly like `insert`.
        writer.line("    fn entry_or_insert(&self, key: K, default: impl FnOnce() -> V) -> ::std::cell::RefMut<'_, V> {");
        writer.line("        let missing = !self.store.borrow().contains_key(&key);");
        writer.line("        if missing { self.insert(key.clone(), default()); }");
        writer.line("        ::std::cell::RefMut::map(self.store.borrow_mut(), move |store| store.get_mut(&key).expect(\"record entry just inserted\"))");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V> Default for SmeltRecord<K, V> {");
        writer.line("    fn default() -> Self { Self::new() }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V, const N: usize> From<[(K, V); N]> for SmeltRecord<K, V> {");
        writer.line("    fn from(values: [(K, V); N]) -> Self { values.into_iter().collect() }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V> ::std::iter::FromIterator<(K, V)> for SmeltRecord<K, V> {");
        writer.line("    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self { let record = Self::new(); record.extend(iter); record }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V: Clone> IntoIterator for SmeltRecord<K, V> {");
        writer.line("    type Item = (K, V);");
        writer.line("    type IntoIter = ::std::vec::IntoIter<(K, V)>;");
        writer.line("    fn into_iter(self) -> Self::IntoIter { self.iter() }");
        writer.line("}");
        writer.blank_line();
        writer.line(
            "impl<K: Eq + ::std::hash::Hash + SmeltPropertyKey, V: PartialEq> PartialEq for SmeltRecord<K, V> {",
        );
        // Compared entry-by-entry and order-INSENSITIVELY, matching the `HashMap`
        // equality this replaces: JavaScript own-key order is observable through
        // enumeration, not through deep equality.
        writer.line("    fn eq(&self, other: &Self) -> bool { let left = self.store.borrow(); let right = other.store.borrow(); left.len() == right.len() && left.entries().iter().all(|entry| right.get(&entry.key).is_some_and(|found| *found == entry.value)) }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Eq + ::std::hash::Hash + SmeltPropertyKey, V: Eq> Eq for SmeltRecord<K, V> {}");
        writer.blank_line();
        writer.line("impl<K, V> PartialEq<::std::collections::HashMap<K, V>> for SmeltRecord<K, V> where K: Eq + ::std::hash::Hash, V: PartialEq {");
        // Compared entry-by-entry rather than with `HashMap::eq`: the record's own
        // store is keyed by `SmeltFieldHasher` while the operand here is a stock
        // `HashMap` (`RandomState`), and `eq` requires both sides to share a hasher
        // type. Equality does not depend on the hasher, so this compares the
        // contents directly.
        writer.line("    fn eq(&self, other: &::std::collections::HashMap<K, V>) -> bool { let store = self.store.borrow(); store.len() == other.len() && store.entries().iter().all(|entry| other.get(&entry.key).is_some_and(|found| *found == entry.value)) }");
        writer.line("}");
        writer.blank_line();
        // JS `Map` container. Carries a stable object `id` so that the identity a
        // `Map` value has in JavaScript survives the erasure round-trip: when the
        // map crosses a dynamic boundary (`IntoSmeltUnknown`) it becomes a
        // marker-bearing `SmeltUnknown::Object` stamped with `__smelt_map` and its
        // `id`, and un-erasing (`SmeltFromUnknown`) restores that same `id`. The
        // marker is what lets `smelt_object_to_string_tag` report `[object Map]`
        // and the `isMap`/`isEqualWith` runtime probes recognize the value once it
        // is erased — a plain `SmeltUnknown::Object` (a JS object literal) cannot
        // carry that identity, which is why the marker boundary exists.
        // A JavaScript `Map` is a REFERENCE value: every binding that names one
        // Map object shares a single backing store, so `otherName.set(k, v)` is
        // observable through every alias, and passing a Map into a function hands
        // over the same object (not a copy). The backing `entries` therefore live
        // behind a shared `Rc<RefCell<..>>`: `#[derive(Clone)]` produces an ALIAS
        // (bumps the refcount, copies the stable `id`) rather than a deep copy, so
        // a `.clone()` inserted by codegen when a Map flows through an expression
        // or a recursive call still mutates the one shared store. Value-copy
        // semantics here silently drops writes — e.g. `isEqualWith`'s cycle guard
        // (`stack.set(a, b)` before recursing) never persists, so circular inputs
        // recurse forever and abort. A genuinely independent Map is only produced
        // by an explicit JS construction (`new Map(other)`), which lowers through
        // `From`/`from_iter`/`new` and allocates a fresh store. Keys compare by
        // SameValueZero via `SmeltJsKeyEq::same_js_key` (objects/arrays/functions
        // by reference identity, primitives by value, `NaN` matches `NaN`).
        // `Clone` is the alias-producing derive described above (it bumps the `Rc`
        // refcount and copies the stable `id`); `Debug` is hand-written below so the
        // store and its index stay invisible and the rendering keeps reading
        // `SmeltJsMap { id: 1, entries: [..] }`.
        writer.line("#[derive(Clone)]");
        writer.line("pub struct SmeltJsMap<K, V> {");
        writer.line("    id: usize,");
        writer.line("    store: ::std::rc::Rc<::std::cell::RefCell<SmeltJsMapStore<K, V>>>,");
        writer.line("}");
        writer.blank_line();
        // The entries of a `SmeltJsMap` plus the hash index over their keys.
        //
        // A lookup does NOT scan `entries`: every key is also recorded in a
        // `SmeltJsSlotIndex` under `SmeltJsKeyEq::js_key_hash`, whose contract is that
        // two `same_js_key`-equal keys always hash the same, so the index only narrows
        // which slots a comparison has to look at and the surviving candidates are
        // still confirmed with `same_js_key`. A hash collision therefore costs a
        // comparison and never changes an answer. Without it `contains_key`, `get`,
        // `insert` and `remove` were all linear, so building an n-entry Map — what
        // `groupBy`/`countBy`/`uniqBy`/memoization caches do on every call — cost
        // O(n^2) key comparisons.
        //
        // The index lives INSIDE the `RefCell` alongside the entries because a JS Map
        // is a reference value: a write through any alias must be seen by every other
        // alias, and an index kept outside the shared store would go stale the moment
        // another handle inserted. `entries` stays a plain insertion-ordered `Vec`, so
        // iteration order, `IntoIterator`, and the erasure adapters are unchanged.
        writer.line("#[derive(Clone, Debug)]");
        writer.line("struct SmeltJsMapStore<K, V> {");
        writer.line("    entries: Vec<(K, V)>,");
        writer.line("    index: SmeltJsSlotIndex,");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K, V> SmeltJsMapStore<K, V> {");
        writer.line("    fn new() -> Self { Self { entries: Vec::new(), index: SmeltJsSlotIndex::new() } }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: SmeltJsKeyEq, V> SmeltJsMapStore<K, V> {");
        writer.line("    /// Index every slot of `entries`, preserving their order and any duplicate");
        writer.line("    /// keys, so a bulk construction pays one hash per entry instead of a scan.");
        writer.line("    fn from_entries(entries: Vec<(K, V)>) -> Self { let mut index = SmeltJsSlotIndex::new(); for (slot, (key, _)) in entries.iter().enumerate() { index.remember(slot, key.js_key_hash()); } Self { entries, index } }");
        writer.line("    /// Slot of `key`, comparing only the slots that share `hash` — every");
        writer.line("    /// `same_js_key`-equal key is guaranteed to be one of them.");
        writer.line("    fn position(&self, key: &K, hash: Option<u64>) -> Option<usize> { self.index.slots(hash).iter().copied().find(|slot| self.entries[*slot].0.same_js_key(key)) }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: ::std::fmt::Debug, V: ::std::fmt::Debug> ::std::fmt::Debug for SmeltJsMap<K, V> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct(\"SmeltJsMap\").field(\"id\", &self.id).field(\"entries\", &self.store.borrow().entries).finish() } }");
        writer.blank_line();
        writer.line("impl<K, V> SmeltJsMap<K, V> {");
        writer.line("    fn new() -> Self { Self { id: smelt_next_object_id(), store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltJsMapStore::new())) } }");
        // `clear` removes every entry without comparing keys, so it needs no
        // `SmeltJsKeyEq`/`Clone` bounds and lives on the unbounded impl block. It
        // replaces the store in place (through the shared `RefCell`, so every alias
        // sees the clear) rather than reallocating the handle, which would detach
        // the other aliases.
        writer.line("    fn clear(&mut self) { *self.store.borrow_mut() = SmeltJsMapStore::new(); }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: SmeltJsKeyEq + Clone, V: Clone> SmeltJsMap<K, V> {");
        writer.line("    fn len(&self) -> usize { self.store.borrow().entries.len() }");
        writer.line("    fn contains_key(&self, key: &K) -> bool { let hash = key.js_key_hash(); self.store.borrow().position(key, hash).is_some() }");
        writer.line("    fn get(&self, key: &K) -> Option<V> { let hash = key.js_key_hash(); let store = self.store.borrow(); store.position(key, hash).map(|slot| store.entries[slot].1.clone()) }");
        writer.line("    fn insert(&mut self, key: K, value: V) -> Option<V> { let hash = key.js_key_hash(); let mut store = self.store.borrow_mut(); let existing = store.position(&key, hash); if let Some(slot) = existing { Some(::std::mem::replace(&mut store.entries[slot].1, value)) } else { let slot = store.entries.len(); store.entries.push((key, value)); store.index.remember(slot, hash); None } }");
        writer.line("    fn remove(&mut self, key: &K) -> Option<V> { let hash = key.js_key_hash(); let mut store = self.store.borrow_mut(); let existing = store.position(key, hash); match existing { Some(slot) => { let removed = store.entries.remove(slot).1; store.index.forget(slot); Some(removed) }, None => None } }");
        writer.line("    fn iter(&self) -> ::std::vec::IntoIter<(K, V)> { self.store.borrow().entries.clone().into_iter() }");
        writer.line("    fn keys(&self) -> ::std::vec::IntoIter<K> { self.store.borrow().entries.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn values(&self) -> ::std::vec::IntoIter<V> { self.store.borrow().entries.iter().map(|(_, value)| value.clone()).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) { for (key, value) in iter { self.insert(key, value); } }");
        // Borrow the value stored under `key` for mutation, inserting `default`
        // first when the key is absent. This is the accessor a source
        // `dict[key].push(item)` lowers to (see
        // `smelt_mir::opt::DictEntryInPlaceMutation`): the entry is mutated
        // THROUGH the map, so neither the old value nor the updated one is ever
        // copied. Without it, growing n grouped entries cost O(n^2) element
        // clones because every mutation copied the whole entry out and back.
        //
        // The returned `RefMut` is a live borrow of the shared store, so the
        // caller must not touch the same map while holding it; codegen therefore
        // evaluates the key, the default, and the pushed item BEFORE the call.
        // The slot is resolved (or appended) under one `borrow_mut`, which is
        // released before the guard is taken, so the guard projects a settled
        // slot index. Insertion order and `SameValueZero` key identity come from
        // the same `position`/`remember` pair `insert` uses, so a fused mutation
        // is indistinguishable from the copy-out/copy-back form it replaces —
        // and, the store being shared, it is visible through every alias.
        writer.line("    fn entry_or_insert(&mut self, key: K, default: impl FnOnce() -> V) -> ::std::cell::RefMut<'_, V> {");
        writer.line("        let hash = key.js_key_hash();");
        writer.line("        let slot = { let mut store = self.store.borrow_mut(); match store.position(&key, hash) { Some(slot) => slot, None => { let slot = store.entries.len(); store.entries.push((key, default())); store.index.remember(slot, hash); slot } } };");
        writer.line("        ::std::cell::RefMut::map(self.store.borrow_mut(), move |store| &mut store.entries[slot].1)");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K, V> Default for SmeltJsMap<K, V> {");
        writer.line("    fn default() -> Self { Self::new() }");
        writer.line("}");
        writer.blank_line();
        // The bulk constructors carry `K: SmeltJsKeyEq` because they build the key
        // index up front. Every key type a Map can be built with already satisfies it
        // (the map's own accessors demand it), so this widens no call site.
        writer.line("impl<K: SmeltJsKeyEq, V, const N: usize> From<[(K, V); N]> for SmeltJsMap<K, V> {");
        writer.line("    fn from(entries: [(K, V); N]) -> Self { Self { id: smelt_next_object_id(), store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltJsMapStore::from_entries(Vec::from(entries)))) } }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: SmeltJsKeyEq, V> ::std::iter::FromIterator<(K, V)> for SmeltJsMap<K, V> {");
        writer.line("    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self { Self { id: smelt_next_object_id(), store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltJsMapStore::from_entries(iter.into_iter().collect()))) } }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: Clone, V: Clone> IntoIterator for SmeltJsMap<K, V> {");
        writer.line("    type Item = (K, V);");
        writer.line("    type IntoIter = ::std::vec::IntoIter<(K, V)>;");
        writer.line("    fn into_iter(self) -> Self::IntoIter { ::std::rc::Rc::try_unwrap(self.store).map(|cell| cell.into_inner()).unwrap_or_else(|shared| shared.borrow().clone()).entries.into_iter() }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<K: SmeltJsKeyEq + Clone, V: PartialEq + Clone> PartialEq for SmeltJsMap<K, V> {");
        writer.line("    fn eq(&self, other: &Self) -> bool { let store = self.store.borrow(); store.entries.len() == other.store.borrow().entries.len() && store.entries.iter().all(|(key, value)| other.get(key).is_some_and(|other_value| other_value == *value)) }");
        writer.line("}");
        writer.line("impl<K: SmeltJsKeyEq + Clone, V: Eq + Clone> Eq for SmeltJsMap<K, V> {}");
        // Erase a `Map` to a marker-bearing object: `{ __smelt_map: [[k, v], ...] }`
        // stamped with the map's stable `id`. This is the dynamic boundary adapter
        // — the only place a typed `SmeltJsMap` becomes a shapeless `SmeltUnknown`
        // — and it preserves both the entries (as an array of `[key, value]` pairs)
        // and the object identity so `isMap`/`isEqualWith`/`Object.prototype.toString`
        // work on the erased value and `SmeltFromUnknown` can restore it losslessly.
        writer.line("impl<K: IntoSmeltUnknown + Clone, V: IntoSmeltUnknown + Clone> IntoSmeltUnknown for SmeltJsMap<K, V> { fn into_smelt_unknown(self) -> SmeltUnknown { let id = self.id; let pairs = self.store.borrow().entries.clone().into_iter().map(|(key, value)| SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), vec![key.into_smelt_unknown(), value.into_smelt_unknown()]))).collect::<Vec<_>>(); let object = Vec::from([(\"__smelt_map\".to_owned(), SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), pairs)))]); SmeltUnknown::Object(SmeltObject::with_id(id, object)) } }");
        writer.blank_line();
        // JS `Set` container with SameValueZero membership and insertion order.
        //
        // Rust `HashSet` demands `Eq + Hash`, which is impossible for `f64`,
        // generated unions, and generic type parameters, and is the wrong
        // equality besides: JavaScript `Set` compares objects/functions by
        // reference identity and treats `NaN` as equal to itself. Rather than
        // require a per-element `Eq + Hash`, membership projects each element
        // through its `IntoSmeltUnknown` erasure and compares the resulting
        // runtime values with `SmeltJsKeyEq::same_js_key` (the same erased-key
        // projection `SmeltJsMap` uses for keys). This makes one uniform,
        // JS-correct container work for every element type that can be erased.
        // Elements are stored in a `Vec` so iteration preserves insertion order
        // like a real JS `Set`.
        // Carries a stable object `id` — exactly like `SmeltJsMap` — so the
        // identity a `Set` value has in JavaScript survives the erasure
        // round-trip: when the set crosses a dynamic boundary
        // (`IntoSmeltUnknown`) it becomes a marker-bearing `SmeltUnknown::Object`
        // stamped with `__smelt_set` and its `id`, and un-erasing
        // (`SmeltFromUnknown`) restores that same `id`. The marker is what lets
        // `smelt_object_to_string_tag` report `[object Set]` and the
        // `isSet`/`isEqualWith` runtime probes recognize the value once it is
        // erased — a plain `SmeltUnknown::Array` cannot carry Set identity, which
        // is why the marker boundary exists.
        // Membership does NOT scan `entries`: every member is also recorded in a
        // hash index (`hashed`) keyed by `smelt_js_member_hash_key` — a hash of
        // the member's erased runtime value that is consistent with
        // `same_js_key`, i.e. two members that compare SameValueZero-equal always
        // land in the same bucket (see that function for the per-variant argument).
        // Without it `insert` was `contains` + `push`, so building an n-member set
        // — what `uniq`/`difference`/`intersection` do on every call — cost
        // O(n^2) erasures. `entries` stays a plain insertion-ordered `Vec`, so
        // iteration order, `IntoIterator`, and the erasure adapters are unchanged;
        // the index only narrows which slots a comparison has to look at, and the
        // surviving candidates are still confirmed with `same_member`, so a hash
        // collision costs a comparison and never changes an answer.
        // `unhashed` holds the slots of members that have no hashable identity —
        // erased functions, whose `same_js_key` arm resolves through a mutable
        // function-identity registry (`smelt_register_function_identity`) that can
        // be rewritten after a member is inserted, which would leave a stored hash
        // stale. Those members keep the linear scan.
        //
        // Members and index live in one store behind an `Rc`, and `.clone()` is a
        // refcount bump rather than a copy of every member. That is not an
        // optimization of an unused path: codegen clones a Set at every use of a
        // captured one, so `difference`/`intersection` — which build a Set from one
        // array and then probe it once per element of the other — ran
        // `second_set.clone().contains(&item)` per element and paid a full copy of
        // the set on every probe. The store is copy-on-write, NOT shared mutable
        // state: `Rc::make_mut` copies it before any write that still has another
        // handle alive, so a cloned Set remains an independent value exactly as it
        // was when the members were a bare `Vec`. (This is deliberately weaker than
        // `SmeltJsMap`, which is a genuinely shared `Rc<RefCell<..>>` because JS
        // Maps are reference values; giving `Set` the same treatment would change
        // what `iter`/`union`/`difference` can hand back — they borrow the members
        // out of the store, which a `RefCell` guard cannot outlive — so it is a
        // separate change.)
        writer.line("pub struct SmeltJsSet<T> {");
        writer.line("    id: usize,");
        writer.line("    store: ::std::rc::Rc<SmeltJsSetStore<T>>,");
        writer.line("}");
        writer.blank_line();
        // A hash index over the slots of an insertion-ordered container.
        //
        // Shared by `SmeltJsSet` (its members) and `SmeltJsMap` (its keys): both keep
        // their entries in a plain `Vec` so iteration stays insertion-ordered, and both
        // use this to narrow a lookup to the slots whose entry hashes the same instead
        // of scanning every one. The bookkeeping is identical for either container
        // because it only ever handles slot numbers, never the entries themselves.
        // `unhashed` holds the slots of entries with no stable hashable identity —
        // erased functions, whose identity resolves through a registry a later
        // `smelt_register_function_identity` call can rewrite, so a hash taken at
        // insert time is not guaranteed to stay valid. Those keep the linear scan,
        // which cannot go stale.
        // The index is keyed by a value that has ALREADY been hashed:
        // `SmeltJsKeyEq::js_key_hash` runs the key through `SmeltFieldHasher`,
        // whose `finish` applies a splitmix64 finalizer, so the `u64` handed to
        // the index has full avalanche in every bit. Hashing it a second time
        // inside the index map would buy no distribution and cost a whole extra
        // hash round on the hottest path a `groupBy`/`countBy`/`Set` has --
        // `SmeltJsMapStore::position` and `smelt_js_member_hash_key` together were
        // 31% of es-toolkit's `group_by`. `SmeltPreHashedHasher` therefore passes
        // the key straight through.
        writer.line("#[derive(Default, Clone, Copy)]");
        writer.line("pub struct SmeltPreHashedHasher(u64);");
        writer.blank_line();
        writer.line("impl ::std::hash::Hasher for SmeltPreHashedHasher {");
        writer.line("    fn finish(&self) -> u64 { self.0 }");
        writer.line("    fn write_u64(&mut self, value: u64) { self.0 = value; }");
        // Only `write_u64` is ever called: the map below is keyed by `u64` and
        // `<u64 as Hash>::hash` calls exactly that. The byte path is unreachable
        // for this map, but a `Hasher` impl must supply it, so fold the bytes the
        // same way `SmeltFieldHasher` does rather than leaving a silent no-op that
        // would collide everything if the key type ever changed.
        writer.line("    fn write(&mut self, bytes: &[u8]) {");
        writer.line("        let mut hasher = SmeltFieldHasher(self.0);");
        writer.line("        ::std::hash::Hasher::write(&mut hasher, bytes);");
        writer.line("        self.0 = ::std::hash::Hasher::finish(&hasher);");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("/// A map keyed by an already-hashed `u64`, which is passed through unmixed.");
        writer.line("pub type SmeltPreHashedMap<V> = ::std::collections::HashMap<u64, V, ::std::hash::BuildHasherDefault<SmeltPreHashedHasher>>;");
        writer.blank_line();
        // The overwhelming majority of hash keys index exactly ONE slot: a
        // `groupBy` over n distinct keys has n buckets of one entry each until a
        // genuine hash collision, and duplicate JavaScript keys cannot coexist in
        // a Map at all. A `Vec<usize>` per bucket therefore charged one heap
        // allocation per distinct key for a single `usize`, on the very path this
        // container exists to make fast. `SmeltSlotList` stores that lone slot
        // inline and only allocates once a bucket actually holds two.
        writer.line("#[derive(Clone, Debug)]");
        writer.line("enum SmeltSlotList {");
        writer.line("    One(usize),");
        writer.line("    Many(Vec<usize>),");
        writer.line("}");
        writer.blank_line();
        writer.line("impl SmeltSlotList {");
        writer.line("    /// The slots in this bucket, in insertion order.");
        writer.line("    fn slots(&self) -> &[usize] { match self { Self::One(slot) => ::std::slice::from_ref(slot), Self::Many(slots) => slots.as_slice() } }");
        writer.line("    /// Append `slot`, promoting a one-slot bucket to an allocated one.");
        writer.line("    fn push(&mut self, slot: usize) { match self { Self::One(first) => *self = Self::Many(::std::vec![*first, slot]), Self::Many(slots) => slots.push(slot) } }");
        writer.line("    /// Drop `removed` and shift every later slot down by one, mirroring what");
        writer.line("    /// `entries.remove(removed)` did to the positions this bucket stores.");
        writer.line("    /// Returns whether the bucket is now empty and should be dropped.");
        writer.line("    fn forget(&mut self, removed: usize) -> bool {");
        writer.line("        match self {");
        writer.line("            Self::One(slot) => { if *slot == removed { return true; } if *slot > removed { *slot -= 1; } false }");
        writer.line("            Self::Many(slots) => {");
        writer.line("                slots.retain(|existing| *existing != removed);");
        writer.line("                for existing in slots.iter_mut() { if *existing > removed { *existing -= 1; } }");
        // Demote back to the inline form so a bucket that collided once does not
        // keep its allocation for the rest of the container's life.
        writer.line("                match slots.len() { 0 => true, 1 => { *self = Self::One(slots[0]); false }, _ => false }");
        writer.line("            }");
        writer.line("        }");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("#[derive(Clone, Debug)]");
        writer.line("struct SmeltJsSlotIndex {");
        writer.line("    hashed: SmeltPreHashedMap<SmeltSlotList>,");
        writer.line("    unhashed: Vec<usize>,");
        writer.line("}");
        writer.blank_line();
        writer.line("impl SmeltJsSlotIndex {");
        writer.line("    fn new() -> Self { Self { hashed: SmeltPreHashedMap::default(), unhashed: Vec::new() } }");
        writer.line("    /// Index the freshly pushed slot `slot` under its entry's hash key.");
        writer.line("    fn remember(&mut self, slot: usize, key: Option<u64>) { match key { Some(key) => match self.hashed.entry(key) { ::std::collections::hash_map::Entry::Occupied(mut bucket) => bucket.get_mut().push(slot), ::std::collections::hash_map::Entry::Vacant(bucket) => { bucket.insert(SmeltSlotList::One(slot)); } }, None => self.unhashed.push(slot) } }");
        writer.line("    /// Drop `slot` from the index and shift every later slot down by one, which");
        writer.line("    /// is what `entries.remove(slot)` did to the positions the index stores.");
        writer.line("    fn forget(&mut self, slot: usize) { self.hashed.retain(|_, slots| !slots.forget(slot)); self.unhashed.retain(|existing| *existing != slot); for existing in self.unhashed.iter_mut() { if *existing > slot { *existing -= 1; } } }");
        writer.line("    /// The slots that may hold an entry with hash key `key`.");
        writer.line("    fn slots(&self, key: Option<u64>) -> &[usize] { match key { Some(key) => self.hashed.get(&key).map_or(&[], SmeltSlotList::slots), None => self.unhashed.as_slice() } }");
        writer.line("}");
        writer.blank_line();
        writer.line("/// The members of a `SmeltJsSet` plus the hash index over them.");
        writer.line("#[derive(Clone, Debug)]");
        writer.line("struct SmeltJsSetStore<T> {");
        writer.line("    entries: Vec<T>,");
        writer.line("    index: SmeltJsSlotIndex,");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<T> SmeltJsSetStore<T> {");
        writer.line("    fn new() -> Self { Self { entries: Vec::new(), index: SmeltJsSlotIndex::new() } }");
        writer.line("}");
        writer.blank_line();
        // Hand-written `Clone`: the derive would demand `T: Clone` (the old `Vec<T>`
        // field forced that) even though sharing the store needs nothing of `T`.
        writer.line("impl<T> Clone for SmeltJsSet<T> { fn clone(&self) -> Self { Self { id: self.id, store: self.store.clone() } } }");
        // Hand-written `Debug` so the store stays invisible and the rendering keeps
        // reading `SmeltJsSet { id: 1, entries: [..] }`, as it did when the members
        // were an inline field.
        writer.line("impl<T: ::std::fmt::Debug> ::std::fmt::Debug for SmeltJsSet<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct(\"SmeltJsSet\").field(\"id\", &self.id).field(\"entries\", &self.store.entries).finish() } }");
        writer.blank_line();
        writer.line("impl<T> SmeltJsSet<T> {");
        writer.line("    fn new() -> Self { Self::with_id(smelt_next_object_id()) }");
        writer.line("    /// Build an empty set that keeps a caller-supplied JavaScript object identity.");
        writer.line("    fn with_id(id: usize) -> Self { Self { id, store: ::std::rc::Rc::new(SmeltJsSetStore::new()) } }");
        writer.line("    /// Drop every member. Replaces the store instead of emptying it in place, so");
        writer.line("    /// this needs no `T: Clone` and never copies the members it is about to drop.");
        writer.line("    fn clear(&mut self) { self.store = ::std::rc::Rc::new(SmeltJsSetStore::new()); }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<T: Clone + IntoSmeltUnknown> SmeltJsSet<T> {");
        writer.line("    /// SameValueZero equality via each element's erased runtime value.");
        writer.line("    fn same_member(left: &T, right: &T) -> bool { left.clone().into_smelt_unknown().same_js_key(&right.clone().into_smelt_unknown()) }");
        writer.line("    fn len(&self) -> usize { self.store.entries.len() }");
        writer.line("    fn is_empty(&self) -> bool { self.store.entries.is_empty() }");
        writer.line("    /// The hash-index key of `value`, or `None` when it has no hashable identity.");
        writer.line("    fn member_key(value: &T) -> Option<u64> { smelt_js_member_hash_key(&value.clone().into_smelt_unknown()) }");
        writer.line("    /// Copy-on-write access to the store for a mutation.");
        writer.line("    fn store_mut(&mut self) -> &mut SmeltJsSetStore<T> { ::std::rc::Rc::make_mut(&mut self.store) }");
        writer.line("    /// Slot of `value` in the members, comparing only the ones that share `key`");
        writer.line("    /// — every SameValueZero-equal member is guaranteed to be one of them.");
        writer.line("    fn position_with_key(&self, value: &T, key: Option<u64>) -> Option<usize> { let store = &*self.store; store.index.slots(key).iter().copied().find(|slot| Self::same_member(&store.entries[*slot], value)) }");
        writer.line("    fn position(&self, value: &T) -> Option<usize> { self.position_with_key(value, Self::member_key(value)) }");
        writer.line("    fn contains(&self, value: &T) -> bool { self.position(value).is_some() }");
        writer.line("    fn insert(&mut self, value: T) -> bool { let key = Self::member_key(&value); if self.position_with_key(&value, key).is_some() { return false; } let store = self.store_mut(); let slot = store.entries.len(); store.entries.push(value); store.index.remember(slot, key); true }");
        writer.line("    fn remove(&mut self, value: &T) -> bool { match self.position(value) { Some(slot) => { let store = self.store_mut(); store.entries.remove(slot); store.index.forget(slot); true }, None => false } }");
        writer.line("    fn iter(&self) -> ::std::slice::Iter<'_, T> { self.store.entries.iter() }");
        writer.line("    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) { for value in iter { self.insert(value); } }");
        writer.line("    fn is_disjoint(&self, other: &Self) -> bool { self.store.entries.iter().all(|value| !other.contains(value)) }");
        writer.line("    fn is_subset(&self, other: &Self) -> bool { self.store.entries.iter().all(|value| other.contains(value)) }");
        writer.line("    fn is_superset(&self, other: &Self) -> bool { other.is_subset(self) }");
        writer.line("    fn union<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { let mut out: Vec<&T> = self.store.entries.iter().collect(); out.extend(other.store.entries.iter().filter(|value| !self.contains(value))); out.into_iter() }");
        writer.line("    fn intersection<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { self.store.entries.iter().filter(|value| other.contains(value)).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn difference<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { self.store.entries.iter().filter(|value| !other.contains(value)).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn symmetric_difference<'smelt_set>(&'smelt_set self, other: &'smelt_set Self) -> ::std::vec::IntoIter<&'smelt_set T> { let mut out: Vec<&T> = self.store.entries.iter().filter(|value| !other.contains(value)).collect(); for value in other.store.entries.iter() { if !self.contains(value) { out.push(value); } } out.into_iter() }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl<T> Default for SmeltJsSet<T> { fn default() -> Self { Self::new() } }");
        writer.line("impl<T: Clone + IntoSmeltUnknown, const N: usize> From<[T; N]> for SmeltJsSet<T> { fn from(values: [T; N]) -> Self { values.into_iter().collect() } }");
        writer.line("impl<T: Clone + IntoSmeltUnknown> ::std::iter::FromIterator<T> for SmeltJsSet<T> { fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self { let mut set = Self::new(); set.extend(iter); set } }");
        // Taking the members by value reclaims the store when this handle is the
        // last one and copies them only when it is not, so the `T: Clone` bound is
        // the price of the shared store (every generated element type is `Clone`).
        writer.line("impl<T: Clone> IntoIterator for SmeltJsSet<T> { type Item = T; type IntoIter = ::std::vec::IntoIter<T>; fn into_iter(self) -> Self::IntoIter { smelt_js_set_into_members(self.store).into_iter() } }");
        writer.line("impl<'smelt_set, T> IntoIterator for &'smelt_set SmeltJsSet<T> { type Item = &'smelt_set T; type IntoIter = ::std::slice::Iter<'smelt_set, T>; fn into_iter(self) -> Self::IntoIter { self.store.entries.iter() } }");
        writer.line("impl<T: Clone + IntoSmeltUnknown> PartialEq for SmeltJsSet<T> { fn eq(&self, other: &Self) -> bool { self.store.entries.len() == other.store.entries.len() && self.store.entries.iter().all(|value| other.contains(value)) } }");
        writer.blank_line();
        writer.line("/// Take the members out of a set store, reusing them when this is the last handle.");
        writer.line("fn smelt_js_set_into_members<T: Clone>(store: ::std::rc::Rc<SmeltJsSetStore<T>>) -> Vec<T> { match ::std::rc::Rc::try_unwrap(store) { Ok(store) => store.entries, Err(shared) => shared.entries.clone() } }");
        // Erase a `Set` to a marker-bearing object: `{ __smelt_set: [members...] }`
        // stamped with the set's stable `id`. This is the dynamic boundary adapter
        // — the only place a typed `SmeltJsSet` becomes a shapeless `SmeltUnknown`
        // — and it preserves both the members and the object identity so
        // `isSet`/`isEqualWith`/`Object.prototype.toString` work on the erased
        // value and `SmeltFromUnknown` can restore it losslessly. Mirrors the
        // `SmeltJsMap` `__smelt_map` adapter. Members are sorted by their stable
        // erased-hash key — as the pre-marker bare-array erasure did — so that
        // spreading / iterating an erased Set yields a deterministic order and
        // structural equality over two sets with the same members (but different
        // insertion order) still compares equal.
        writer.line("impl<T: IntoSmeltUnknown + Clone> IntoSmeltUnknown for SmeltJsSet<T> { fn into_smelt_unknown(self) -> SmeltUnknown { let id = self.id; let mut members = smelt_js_set_into_members(self.store).into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<Vec<_>>(); members.sort_by_key(smelt_unknown_stable_hash_key); let object = Vec::from([(\"__smelt_set\".to_owned(), SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), members)))]); SmeltUnknown::Object(SmeltObject::with_id(id, object)) } }");
        writer.blank_line();
        // The hash side of `SmeltJsKeyEq::same_js_key`, used to bucket `SmeltJsSet`
        // members. The contract a hash index needs is one-directional: whenever two
        // erased values are `same_js_key`-equal they MUST hash the same. Per variant:
        //
        // * `Null`/`Undefined`/`Bool`/`String`/`Symbol` compare by value and hash
        //   that same value, tagged by variant (`same_js_key` never equates values
        //   of two different variants: its fallthrough is structural equality, whose
        //   cross-variant arm is `false`).
        // * `Number` is SameValueZero, which differs from `f64` bit equality in
        //   exactly two places, and both are normalized here: every `NaN` hashes as
        //   one canonical `NaN` (`same_js_key` equates them), and `-0.0` hashes as
        //   `+0.0` (`f64 ==` equates them, but their bit patterns differ). Any other
        //   pair that is `==` and not `NaN` already has identical bits.
        // * `Array`/`Object`/`Promise` compare by REFERENCE identity — `same_js_key`
        //   looks only at the stable `id` — so the hash is that `id` and nothing
        //   else. Deliberately NOT the structural hash
        //   (`smelt_unknown_stable_hash_key`): that one hashes contents, so mutating
        //   a member already in a set would change its hash and lose it, and two
        //   structurally equal but distinct objects (two distinct JS Set members)
        //   would collide needlessly.
        // * `Function` returns `None`: `same_js_key` resolves callables through
        //   `smelt_function_identity_of`, a registry a later
        //   `smelt_register_function_identity` call can still rewrite, so no hash
        //   taken at insert time is guaranteed to stay valid. `None` members keep
        //   the linear scan, which cannot go stale.
        // Hash one already-normalized value into the same key space as
        // `smelt_js_member_hash_key`. Used by the primitive `SmeltJsKeyEq::js_key_hash`
        // impls, which compare a key only against another key of its own type and so
        // need self-consistency, not the cross-variant tagging the erased hash does.
        writer.line("fn smelt_js_hash_one<H: ::std::hash::Hash>(value: &H) -> u64 { let mut hasher = SmeltFieldHasher::default(); value.hash(&mut hasher); ::std::hash::Hasher::finish(&hasher) }");
        writer.blank_line();
        writer.line("fn smelt_js_member_hash_key(value: &SmeltUnknown) -> Option<u64> {");
        writer.line("    let mut hasher = SmeltFieldHasher::default();");
        writer.line("    match value {");
        writer.line("        SmeltUnknown::Null => 0_u8.hash(&mut hasher),");
        writer.line("        SmeltUnknown::Undefined => 1_u8.hash(&mut hasher),");
        writer.line("        SmeltUnknown::Bool(value) => { 2_u8.hash(&mut hasher); value.hash(&mut hasher); },");
        writer.line("        SmeltUnknown::Number(value) => { 3_u8.hash(&mut hasher); let bits = if value.is_nan() { f64::NAN.to_bits() } else if *value == 0.0 { 0_f64.to_bits() } else { value.to_bits() }; bits.hash(&mut hasher); },");
        writer.line("        SmeltUnknown::String(value) => { 4_u8.hash(&mut hasher); value.hash(&mut hasher); },");
        writer.line("        SmeltUnknown::Symbol(value) => { 5_u8.hash(&mut hasher); value.hash(&mut hasher); },");
        writer.line("        SmeltUnknown::Array(value) => { 6_u8.hash(&mut hasher); value.id.hash(&mut hasher); },");
        writer.line("        SmeltUnknown::Object(value) => { 7_u8.hash(&mut hasher); value.id.hash(&mut hasher); },");
        writer.line("        SmeltUnknown::Promise(value) => { 8_u8.hash(&mut hasher); value.id.hash(&mut hasher); },");
        writer.line("        SmeltUnknown::Function(_) => return None,");
        writer.line("    }");
        writer.line("    Some(::std::hash::Hasher::finish(&hasher))");
        writer.line("}");
        writer.blank_line();
        // The hash side of the key-equality trait is a DEFAULTED method rather than a
        // separate `SmeltJsKeyHash` trait: a second trait would have to be threaded
        // through every `+ SmeltJsKeyEq` bound codegen appends (see
        // `classes::class_impl_generics_text`), and any bound it was missed on would
        // fail to type-check. As one defaulted method, every existing bound already
        // carries it and an impl that only defines equality stays correct — just
        // unindexed, falling back to the linear scan.
        writer.line("pub trait SmeltJsKeyEq {");
        writer.line("    fn same_js_key(&self, other: &Self) -> bool;");
        writer.line("    /// A hash consistent with `same_js_key`: whenever two keys compare equal");
        writer.line("    /// they MUST hash the same, so a hash index can narrow a lookup without");
        writer.line("    /// changing its answer. The converse is not required — a collision only");
        writer.line("    /// costs a `same_js_key` comparison. `None` means the key has no stable");
        writer.line("    /// hashable identity and must keep the linear scan.");
        writer.line("    fn js_key_hash(&self) -> Option<u64> { None }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl SmeltJsKeyEq for SmeltUnknown {");
        writer.line("    fn js_key_hash(&self) -> Option<u64> { smelt_js_member_hash_key(self) }");
        writer.line("    fn same_js_key(&self, other: &Self) -> bool { match (self, other) { (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) if left.is_nan() && right.is_nan() => true, (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.id == right.id, (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => left.id == right.id, (SmeltUnknown::Function(left), SmeltUnknown::Function(right)) => smelt_same_erased_function(left, right), (SmeltUnknown::Promise(left), SmeltUnknown::Promise(right)) => left.id == right.id, (SmeltUnknown::String(left), SmeltUnknown::String(right)) => ::std::rc::Rc::ptr_eq(left, right) || left == right, _ => self == other } }");
        writer.line("}");
        writer.blank_line();
        writer.line("impl SmeltJsKeyEq for String { fn same_js_key(&self, other: &Self) -> bool { self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(self)) } }");
        writer.line("impl SmeltJsKeyEq for bool { fn same_js_key(&self, other: &Self) -> bool { self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(self)) } }");
        writer.line("impl SmeltJsKeyEq for i64 { fn same_js_key(&self, other: &Self) -> bool { self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(self)) } }");
        writer.line("impl SmeltJsKeyEq for f64 { fn same_js_key(&self, other: &Self) -> bool { (self.is_nan() && other.is_nan()) || self == other } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(&(if self.is_nan() { f64::NAN.to_bits() } else if *self == 0.0 { 0_f64.to_bits() } else { self.to_bits() }))) } }");
        // A record/object used as a collection key compares by JavaScript
        // reference identity (its stable object `id`), matching `same_js_key`'s
        // object arm on the erased value. This lets a `Set`/`Map`/cache keyed by
        // a concrete `SmeltRecord` resolve `SmeltJsKeyEq` without erasing to
        // `SmeltUnknown` (was E0599: unsatisfied `SmeltJsKeyEq` bound).
        writer.line("impl<K, V> SmeltJsKeyEq for SmeltRecord<K, V> { fn same_js_key(&self, other: &Self) -> bool { self.id == other.id } fn js_key_hash(&self) -> Option<u64> { Some(smelt_js_hash_one(&self.id)) } }");
        writer.blank_line();
        // The fourth equality kind: JavaScript strict equality (`===`/`!==`).
        // Distinct from the other three — objects/arrays/functions compare by
        // REFERENCE identity (id/ptr), primitives by value, and `NaN !== NaN`
        // (unlike `same_js_key`'s SameValueZero and `Object.is`'s SameValue, which
        // both treat NaN as equal). `+0 === -0` holds because `f64 ==` is true for
        // them. Unlike `PartialEq` (`smelt_unknown_structural_eq`, intentionally
        // deep for `isDeepEqual`), this never recurses into object/array contents.
        // A trait (not an inherent method) with primitive impls so erased operands
        // that lower to a concrete `String`/`bool`/number still resolve the method.
        writer.line("pub trait SmeltJsStrictEq {");
        writer.line("    fn js_strict_eq(&self, other: &Self) -> bool;");
        writer.line("}");
        writer.line("impl SmeltJsStrictEq for SmeltUnknown {");
        writer.line("    fn js_strict_eq(&self, other: &Self) -> bool { match (self, other) { (SmeltUnknown::Null, SmeltUnknown::Null) => true, (SmeltUnknown::Undefined, SmeltUnknown::Undefined) => true, (SmeltUnknown::Bool(left), SmeltUnknown::Bool(right)) => left == right, (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) => left == right, (SmeltUnknown::String(left), SmeltUnknown::String(right)) => left == right, (SmeltUnknown::Symbol(left), SmeltUnknown::Symbol(right)) => left == right, (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.id == right.id, (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => left.id == right.id, (SmeltUnknown::Function(left), SmeltUnknown::Function(right)) => smelt_same_erased_function(left, right), (SmeltUnknown::Promise(left), SmeltUnknown::Promise(right)) => left.id == right.id, _ => false } }");
        writer.line("}");
        writer.line("impl SmeltJsStrictEq for String { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsStrictEq for bool { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsStrictEq for i64 { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }");
        writer.line("impl SmeltJsStrictEq for f64 { fn js_strict_eq(&self, other: &Self) -> bool { self == other } }");
        writer.blank_line();
        writer.line("#[derive(Debug)]");
        writer.line("pub struct SmeltObject {");
        writer.line("    id: usize,");
        writer.line("    store: ::std::rc::Rc<::std::cell::RefCell<SmeltFieldStore<String, SmeltUnknown>>>,");
        writer.line("}");
        writer.blank_line();
        writer.line("impl Clone for SmeltObject { fn clone(&self) -> Self { Self { id: self.id, store: self.store.clone() } } }");
        writer.line("impl SmeltObject {");
        writer.line("    /// Build an erased object from entries in source order.");
        writer.line("    ///");
        writer.line("    /// The entry sequence is ordered on purpose: JavaScript own-key order is a");
        writer.line("    /// property of how the object was written, and a `HashMap` argument would");
        writer.line("    /// have thrown that away before the constructor could see it. Duplicate");
        writer.line("    /// keys keep the first key\'s position and take the last value, as in JS.");
        writer.line("    fn new(entries: Vec<(String, SmeltUnknown)>) -> Self { Self::with_id(smelt_next_object_id(), entries) }");
        writer.line("    /// Build an erased object that keeps a source value\'s reference identity.");
        writer.line("    fn with_id(id: usize, entries: Vec<(String, SmeltUnknown)>) -> Self { let object = Self { id, store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltFieldStore::with_capacity(entries.len()))) }; for (key, value) in entries { object.insert(key, value); } object }");
        writer.line("    fn from_unknown_record(record: SmeltRecord<String, SmeltUnknown>) -> Self { Self { id: record.id, store: record.store } }");
        writer.line("    fn len(&self) -> usize { self.store.borrow().len() }");
        writer.line("    #[inline]");
        writer.line("    fn contains_key(&self, key: &str) -> bool { self.store.borrow().contains_key(key) }");
        writer.line("    #[inline]");
        writer.line("    fn get(&self, key: &str) -> Option<SmeltUnknown> { self.store.borrow().get(key).cloned() }");
        writer.line("    fn insert(&self, key: String, value: SmeltUnknown) -> Option<SmeltUnknown> { self.store.borrow_mut().insert(key, value) }");
        writer.line("    fn remove(&self, key: &str) -> Option<SmeltUnknown> { self.store.borrow_mut().remove(key) }");
        writer.line("    fn iter(&self) -> ::std::vec::IntoIter<(String, SmeltUnknown)> { self.store.borrow().entries().iter().map(|entry| (entry.key.clone(), entry.value.clone())).collect::<Vec<_>>().into_iter() }");
        writer.line("    fn keys(&self) -> Vec<String> { self.store.borrow().entries().iter().map(|entry| entry.key.clone()).collect() }");
        writer.line("    fn values(&self) -> Vec<SmeltUnknown> { self.store.borrow().entries().iter().map(|entry| entry.value.clone()).collect() }");
        writer.line("}");
        writer.blank_line();
        writer.line(
            "/// Return whether an erased object key is visible to JavaScript `for...in` iteration.",
        );
        let host_marker_array = host_marker_registry_array();
        writer.line(format!("fn smelt_object_has_host_marker(object: &SmeltObject) -> bool {{ {host_marker_array}.iter().any(|marker| object.contains_key(marker)) }}"));
        writer.line(format!("fn smelt_record_has_host_marker<V>(record: &SmeltRecord<String, V>) -> bool {{ {host_marker_array}.iter().any(|marker| record.contains_key(*marker)) }}"));
        byte_buffer_prelude::emit(&mut writer);
        // The `arguments` exotic object. Its indexed elements are enumerable own
        // properties but its `length` is not, which is what makes
        // `isEqual(toArgs([1, 2, 3]), { 0: 1, 1: 2, 2: 3 })` hold: both sides
        // enumerate exactly `["0", "1", "2"]`. The enumeration filters below carry
        // the `length` exception, in the same shape as the `__smelt_regexp` and
        // `__smelt_error` field exceptions.
        writer.line("/// Build the array-like `arguments` object from a function's parameters.");
        writer.line("///");
        writer.line("/// Positional parameters lead; the rest parameter's list is flattened onto");
        writer.line("/// the end, recovering the original call's argument vector. Elements are");
        writer.line("/// stored under index keys and `length` is stored but hidden from own-key");
        writer.line("/// enumeration, matching the exotic object's property attributes.");
        writer.line(format!(
            "fn {helper}(fixed: Vec<SmeltUnknown>, rest: Option<SmeltUnknown>) -> SmeltUnknown {{ let mut smelt_elements = fixed; if let Some(SmeltUnknown::Array(items)) = rest {{ smelt_elements.extend(items.into_vec()); }} let mut fields = Vec::from([(\"{marker}\".to_owned(), SmeltUnknown::Bool(true))]); for (index, value) in smelt_elements.iter().enumerate() {{ fields.push((index.to_string(), value.clone())); }} fields.push((\"length\".to_owned(), SmeltUnknown::Number(smelt_elements.len() as f64))); SmeltUnknown::Object(SmeltObject::new(fields)) }}",
            helper = smelt_stdlib::runtime_symbols::host::ARGUMENTS_OBJECT,
            marker = smelt_stdlib::runtime_symbols::host::ARGUMENTS_MARKER,
        ));
        // An `arguments` object is iterable in JavaScript (its `Symbol.iterator`
        // is `Array.prototype.values`), but the marker record above stores no
        // `__smelt_symbol_iterator` slot, so the erased iterable-to-list coercion
        // could not walk it: `Array.from(arguments)` and `[...arguments]` both
        // panicked with "unknown is not iterable". This helper is the iteration
        // door. It reads `length` and the index keys rather than the record's raw
        // key order, so a caller that assigned an extra named property to the
        // record cannot perturb the element sequence.
        writer.line("/// Extract an `arguments` object's elements, or `None` for any other value.");
        writer.line(format!(
            "fn {helper}(object: &SmeltObject) -> Option<Vec<SmeltUnknown>> {{ if !object.contains_key(\"{marker}\") {{ return None; }} let length = match object.get(\"length\") {{ Some(SmeltUnknown::Number(length)) if length >= 0.0 => length as usize, _ => 0 }}; Some((0..length).map(|index| object.get(&index.to_string()).unwrap_or(SmeltUnknown::Undefined)).collect()) }}",
            helper = smelt_stdlib::runtime_symbols::host::ARGUMENTS_ELEMENTS,
            marker = smelt_stdlib::runtime_symbols::host::ARGUMENTS_MARKER,
        ));
        // `__smelt_proto:`-prefixed entries hold members INHERITED from a
        // prototype (`Object.create(proto)`), so they are never own keys — JS
        // `Object.keys` / `for...in` own-key enumeration must skip them.
        writer.line("fn smelt_is_for_in_object_key(object: &SmeltObject, key: &str) -> bool { if smelt_object_has_host_marker(object) { return false; } !key.starts_with(\"__smelt_proto:\") && !key.starts_with(\"__smelt_method:\") && key != \"__smelt_date\" && key != \"__smelt_timezone\" && key != \"__smelt_class\" && key != \"__smelt_map\" && key != \"__smelt_set\" && !(object.contains_key(\"__smelt_regexp\") && matches!(key, \"__smelt_regexp\" | \"source\" | \"flags\" | \"lastIndex\")) && !(object.contains_key(\"__smelt_error\") && matches!(key, \"__smelt_error\" | \"message\" | \"cause\" | \"errors\" | \"stack\")) && !(object.contains_key(\"__smelt_arguments\") && matches!(key, \"__smelt_arguments\" | \"length\")) }");
        writer
            .line("/// Return whether a record key is visible to JavaScript `for...in` iteration.");
        writer.line("fn smelt_is_for_in_record_key<V>(record: &SmeltRecord<String, V>, key: &str) -> bool { if smelt_record_has_host_marker(record) { return false; } !key.starts_with(\"__smelt_proto:\") && !key.starts_with(\"__smelt_method:\") && key != \"__smelt_date\" && key != \"__smelt_timezone\" && key != \"__smelt_class\" && !(record.contains_key(\"__smelt_regexp\") && matches!(key, \"__smelt_regexp\" | \"source\" | \"flags\" | \"lastIndex\")) && !(record.contains_key(\"__smelt_error\") && matches!(key, \"__smelt_error\" | \"message\" | \"cause\" | \"errors\" | \"stack\")) && !(record.contains_key(\"__smelt_arguments\") && matches!(key, \"__smelt_arguments\" | \"length\")) }");
        // `for...in` walks the PROTOTYPE CHAIN; `Object.keys` does not. The two
        // therefore cannot share one key list. Inherited members live behind the
        // `__smelt_proto:` prefix, which the own-key filters above exclude — right
        // for `Object.keys`, wrong for `for...in`. remeda's `isEmptyish` reads
        // exactly that difference: it uses `for (const _ in data) return false`, so
        // `Object.create(Object.create({ a: 123 }))` must report a key even though
        // it has no OWN key.
        //
        // Own keys come first and shadow an inherited member of the same name,
        // matching JavaScript's enumeration order and its shadowing rule.
        writer.line("/// Every key JavaScript `for...in` yields for an erased object, prototype chain included.");
        writer.line("fn smelt_for_in_object_keys(map: &SmeltObject) -> Vec<String> { let mut keys = Vec::new(); let mut seen = ::std::collections::HashSet::new(); for key in map.keys() { if key.starts_with(\"__smelt_proto:\") { continue; } if smelt_is_for_in_object_key(map, &key) && seen.insert(key.clone()) { keys.push(key); } } for key in map.keys() { if let Some(inherited) = key.strip_prefix(\"__smelt_proto:\") { let inherited = inherited.to_owned(); if seen.insert(inherited.clone()) { keys.push(inherited); } } } keys }");
        writer.blank_line();
        writer.line("/// Every key JavaScript `for...in` yields for a typed record, prototype chain included.");
        writer.line("fn smelt_for_in_record_keys<V>(record: &SmeltRecord<String, V>) -> Vec<String> { let mut keys = Vec::new(); let mut seen = ::std::collections::HashSet::new(); for key in record.keys() { if key.starts_with(\"__smelt_proto:\") { continue; } if smelt_is_for_in_record_key(record, &key) && seen.insert(key.clone()) { keys.push(key); } } for key in record.keys() { if let Some(inherited) = key.strip_prefix(\"__smelt_proto:\") { let inherited = inherited.to_owned(); if seen.insert(inherited.clone()) { keys.push(inherited); } } } keys }");
        writer.blank_line();
        // Own-property enumeration over a `SmeltJsMap` backing.
        //
        // Smelt's marker convention (`__smelt_proto:` for an `Object.create`
        // prototype member, `__smelt_method:` for a class's prototype methods,
        // `__smelt_class` for provenance, `__smelt_symbol:` for a symbol key)
        // is a property of the *representation*, not of the source object, so
        // every own-key view has to honour it. `SmeltRecord` did, through
        // `smelt_is_for_in_record_key`; the `SmeltUnknown`-keyed `SmeltJsMap`
        // backing did not, even though the erased-object -> `SmeltJsMap`
        // coercion copies those keys in verbatim. `Object.keys` of an
        // `Object.create({a: 1})` therefore reported the inherited `a`.
        //
        // This is the `SmeltJsMap` twin of that filter: it drops the markers
        // and turns a stored `__smelt_symbol:x` key back into the
        // `SmeltUnknown::Symbol` value it denotes, so the callers' own
        // string-versus-symbol split (`Object.keys` excludes symbol keys,
        // `Object.getOwnPropertySymbols` keeps only those) works on real tags.
        writer.line("/// Own entries of a `SmeltJsMap` backing, with representation markers removed.");
        writer.line("///");
        writer.line("/// Drops `__smelt_proto:` / `__smelt_method:` / `__smelt_class` keys (inherited");
        writer.line("/// members, prototype methods and class provenance are not own properties) and");
        writer.line("/// restores a `__smelt_symbol:` key to its `SmeltUnknown::Symbol` tag.");
        writer.line("fn smelt_own_js_map_entries<V: Clone>(map: &SmeltJsMap<SmeltUnknown, V>) -> Vec<(SmeltUnknown, V)> { map.iter().filter_map(|(key, value)| { let SmeltUnknown::String(text) = &key else { return Some((key, value)); }; let text = text.to_string(); if text.starts_with(\"__smelt_proto:\") || text.starts_with(\"__smelt_method:\") || text == \"__smelt_class\" { return None; } if let Some(description) = text.strip_prefix(\"__smelt_symbol:\") { return Some((SmeltUnknown::Symbol(description.into()), value)); } Some((key, value)) }).collect() }");
        writer.blank_line();
        writer.line("/// Every key JavaScript `for...in` yields for a `SmeltJsMap` backing.");
        writer.line("///");
        writer.line("/// Own string keys first, then the enumerable `__smelt_proto:` members with");
        writer.line("/// their prefix stripped -- the same order and the same prototype-chain rule as");
        writer.line("/// `smelt_for_in_record_keys`. Symbol keys are never enumerated by `for...in`.");
        writer.line("fn smelt_for_in_js_map_keys<V: Clone>(map: &SmeltJsMap<SmeltUnknown, V>) -> Vec<SmeltUnknown> { let mut keys = Vec::new(); let mut seen = ::std::collections::HashSet::new(); for (key, _) in smelt_own_js_map_entries(map) { if matches!(key, SmeltUnknown::Symbol(_)) { continue; } if let SmeltUnknown::String(text) = &key { if !seen.insert(text.to_string()) { continue; } } keys.push(key); } for key in map.keys() { let SmeltUnknown::String(text) = &key else { continue; }; if let Some(inherited) = text.strip_prefix(\"__smelt_proto:\") { let inherited = inherited.to_owned(); if seen.insert(inherited.clone()) { keys.push(SmeltUnknown::String(inherited.as_str().into())); } } } keys }");
        writer.blank_line();
        writer.line("/// Stringify a marker-bearing erased RegExp as JavaScript does: `/source/flags`.");
        writer.line("fn smelt_regexp_literal(map: &SmeltObject) -> String { let source = match map.get(\"source\") { Some(SmeltUnknown::String(source)) => source.to_string(), _ => String::new() }; let flags = match map.get(\"flags\") { Some(SmeltUnknown::String(flags)) => flags.to_string(), _ => String::new() }; format!(\"/{source}/{flags}\") }");
        writer.blank_line();
        // JavaScript exposes `instance.constructor`, and code branches on it:
        // es-toolkit `isEqualWith` gates instance comparison on
        // `areObjectsEqual(a.constructor, b.constructor) || (isPlainObject(a) && isPlainObject(b))`.
        // The class marker now records the class name, so one constructor value is
        // interned per name and every instance of a class reads back the SAME one.
        //
        // It is a `Function` on purpose. `areObjectsEqual` tags it `[object Function]`
        // and that branch decides by identity (`a === b`), so two classes compare
        // unequal and two instances of one class compare equal — without recursing
        // into the constructor's own `.constructor`, which an object value would do.
        // Calling it yields a fresh instance carrying the same class marker, which is
        // what `new obj.constructor()` means.
        writer.line("thread_local! { static SMELT_CLASS_CONSTRUCTORS: ::std::cell::RefCell<::std::collections::HashMap<String, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }");
        writer.blank_line();
        writer.line("/// One interned constructor value per class name, for `instance.constructor`.");
        writer.line("fn smelt_class_constructor(class_name: String) -> SmeltUnknown { SMELT_CLASS_CONSTRUCTORS.with(|constructors| constructors.borrow_mut().entry(class_name.clone()).or_insert_with(|| { let marker = class_name.clone(); SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { let fields = Vec::from([(\"__smelt_class\".to_owned(), SmeltUnknown::String(marker.as_str().into()))]); Ok(SmeltUnknown::Object(SmeltObject::new(fields))) })) }).clone()) }");
        writer.blank_line();
        writer.line("/// Return the opaque `Object.getPrototypeOf` sentinel for an erased value.");
        writer.line(
            "/// Class instances carry a hidden `__smelt_class` marker and map to a distinct",
        );
        writer.line(
            "/// `\"__smelt_proto:class\"` sentinel so they are not treated as plain objects; arrays,",
        );
        writer.line("/// `null`, and plain objects keep their existing sentinels.");
        writer.line("///");
        writer.line("/// The sentinel strings are themselves valid inputs: walking the prototype");
        writer.line("/// chain (`while (Object.getPrototypeOf(proto) !== null)`) re-invokes this");
        writer.line("/// helper on a returned sentinel, so each sentinel must advance one link");
        writer.line("/// toward `null` (Array/Promise/class prototypes inherit from");
        writer.line("/// `Object.prototype`, whose prototype is `null`). Without this the walk");
        writer.line("/// would return `\"__smelt_proto:object\"` forever and never terminate.");
        // Constructor reflection for host-marker objects. es-toolkit `clone` reads
        // `Object.getPrototypeOf(obj).constructor` and calls `new Constructor(obj)`
        // to rebuild Dates, Maps, Sets, RegExps, DataViews, Errors, Files and boxed
        // primitives. Those live as marker-bearing `SmeltUnknown::Object`s, so their
        // prototype sentinel exposes a callable `constructor` that reconstructs a
        // fresh instance from the captured original, mirroring how `new Map(m)` /
        // `new Date(d)` copy their argument. Plain objects, arrays, promises and
        // class instances keep their string sentinels (their `.constructor` reads as
        // `undefined`, matching the existing Object-assign clone path).
        //
        // The marker→kind discriminator, the single host constructor that both the
        // direct and the reflected construction path call, the cached per-kind
        // prototype, and the interned per-name constructor value all live in
        // `reflection_prelude`, which derives them from the shared host registry.
        reflection_prelude::emit(&mut writer);
        // Rebuild a marker object/array with a FRESH identity while keeping its
        // fields (shallow, matching `new Ctor(obj)` which copies the top level and
        // shares nested references). `SmeltObject`/`SmeltArray` clones share the
        // underlying `Rc` (JS reference semantics), so a genuinely new instance must
        // allocate a new id over a copied entry map/vec.
        writer.line("fn smelt_fresh_identity(value: SmeltUnknown) -> SmeltUnknown { match value { SmeltUnknown::Object(map) => SmeltUnknown::Object(SmeltObject::with_id(smelt_next_object_id(), map.iter().collect())), SmeltUnknown::Array(array) => SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), array.into_vec())), other => other } }");
        // `structuredClone(value)` deep-copies an object graph with fresh
        // identities, preserving host markers (Date/Map/Set/RegExp/Error/...). Used
        // by es-toolkit `cloneDeep` (Error) and remeda `clone` (host objects it
        // delegates to the platform). Primitives/functions/promises pass through.
        if needs_structured_clone {
            writer.line("fn smelt_structured_clone(value: SmeltUnknown) -> SmeltUnknown { match value { SmeltUnknown::Object(map) => { let cloned: Vec<(String, SmeltUnknown)> = map.iter().map(|(key, field)| (key, smelt_structured_clone(field))).collect(); SmeltUnknown::Object(SmeltObject::with_id(smelt_next_object_id(), cloned)) }, SmeltUnknown::Array(array) => SmeltUnknown::Array(SmeltArray::with_id(smelt_next_object_id(), array.into_vec().into_iter().map(smelt_structured_clone).collect())), other => other } }");
        }
        // `Object.create(proto)` must mint a FRESH object. Two failure modes make
        // "just return the prototype" wrong: a concrete prototype object would be
        // aliased (so the `Object.assign(Object.create(p), o)` clone idiom mutates
        // `p`), and an opaque `"__smelt_proto:*"` sentinel from
        // `smelt_prototype_sentinel` is a *string*, so the caller would go on to
        // index and assign fields on a string. Inherited members are modeled the
        // same way the `Object.create({ ... })` literal lowering models them — the
        // prototype's own keys are stored behind a `__smelt_proto:` prefix, which
        // keeps them out of the created object's own-key enumeration. A class
        // prototype keeps the hidden `__smelt_class` marker so the fresh object is
        // still classified as a class instance rather than a plain object.
        writer.line("/// Create a fresh erased object from a runtime prototype value (`Object.create`).");
        writer.line("fn smelt_object_from_prototype(prototype: SmeltUnknown) -> SmeltUnknown { let mut fields: Vec<(String, SmeltUnknown)> = Vec::new(); match prototype { SmeltUnknown::String(sentinel) if &*sentinel == \"__smelt_proto:class\" => { fields.push((\"__smelt_class\".to_owned(), SmeltUnknown::Bool(true))); }, SmeltUnknown::Object(map) => { for (key, value) in map.iter() { if key == \"__smelt_class\" || key.starts_with(\"__smelt_proto:\") { fields.push((key, value)); } else { fields.push((format!(\"__smelt_proto:{key}\"), value)); } } }, _ => {} } SmeltUnknown::Object(SmeltObject::new(fields)) }");
        // `Object.defineProperty(o, k, d)` and `Object.defineProperties(o, ds)`
        // both install descriptors on an object, so the frontend normalizes the
        // singular form into the plural one and both land here. Previously both
        // lowered to an opaque `null` and the mutation was DROPPED outright.
        //
        // What is modeled is the descriptor's VALUE: a data descriptor's `value`,
        // or the one-shot result of an accessor descriptor's `get`. Object
        // literal getters that Smelt already collapses to their return
        // expression arrive as a plain value rather than a function, so both
        // spellings are accepted.
        //
        // ENUMERABILITY is the one attribute that must be honoured rather than
        // ignored, and it is honoured by NOT installing the property. An erased
        // object is a flat key/value store with no per-property attribute table,
        // so a key it holds is enumerable by construction: installing a
        // `enumerable: false` property would make it appear in `Object.keys`,
        // in spread, in `JSON.stringify` and in structural equality, all of
        // which JavaScript hides it from. Leaving it out keeps every one of
        // those answers right and only loses a direct `o.k` read.
        if needs_define_properties {
            writer.line("/// Install a property-descriptor table on an erased object (`Object.defineProperties`).");
            writer.line("fn smelt_define_properties(target: SmeltUnknown, descriptors: SmeltUnknown) -> SmeltUnknown { if let (SmeltUnknown::Object(map), SmeltUnknown::Object(table)) = (&target, &descriptors) { for (key, descriptor) in table.iter() { let SmeltUnknown::Object(entry) = descriptor else { continue }; let enumerable = match entry.get(\"enumerable\") { None | Some(SmeltUnknown::Null) | Some(SmeltUnknown::Undefined) => false, Some(SmeltUnknown::Bool(value)) => value, Some(SmeltUnknown::Number(value)) => value != 0.0 && !value.is_nan(), Some(SmeltUnknown::String(value)) => !value.is_empty(), Some(_) => true }; if !enumerable { continue; } let value = if let Some(value) = entry.get(\"value\") { value } else if let Some(getter) = entry.get(\"get\") { match getter { SmeltUnknown::Function(getter) => (getter)(Vec::new()).unwrap_or(SmeltUnknown::Undefined), other => other } } else { SmeltUnknown::Undefined }; map.insert(key, value); } } target }");
            writer.blank_line();
        }
        writer.line("fn smelt_prototype_sentinel(value: &SmeltUnknown) -> SmeltUnknown { match value { SmeltUnknown::Null => SmeltUnknown::Null, SmeltUnknown::Array(_) => SmeltUnknown::String(\"__smelt_proto:array\".into()), SmeltUnknown::Promise(_) => SmeltUnknown::String(\"__smelt_proto:promise\".into()), SmeltUnknown::Object(map) if map.contains_key(\"__smelt_class\") => SmeltUnknown::String(\"__smelt_proto:class\".into()), SmeltUnknown::Object(map) => match smelt_reflected_marker_class(map) { Some(class) => smelt_reflected_prototype(class), None => SmeltUnknown::String(\"__smelt_proto:object\".into()) }, SmeltUnknown::String(marker) if &**marker == \"__smelt_proto:object\" => SmeltUnknown::Null, SmeltUnknown::String(marker) if &**marker == \"__smelt_proto:array\" || &**marker == \"__smelt_proto:promise\" || &**marker == \"__smelt_proto:class\" => SmeltUnknown::String(\"__smelt_proto:object\".into()), _ => SmeltUnknown::String(\"__smelt_proto:object\".into()) } }");
        // `v.__proto__` is NOT `Object.getPrototypeOf(v)`. In JavaScript the
        // `__proto__` accessor is inherited from `Object.prototype`, so a value
        // whose prototype is `null` (`Object.create(null)`) does not inherit it
        // and a `__proto__` write on such a value stores an ordinary OWN
        // property that a later read has to answer. Smelt represents a
        // null-prototype object as a plain erased object, so the own slot is the
        // only observable trace of that case; everything else answers the same
        // sentinel `Object.getPrototypeOf` does. Keeping the two spellings on
        // separate helpers is what lets `Object.getPrototypeOf` stay blind to own
        // properties, which the spec requires.
        writer.line("/// Read the JavaScript `__proto__` accessor for an erased value.");
        writer.line("fn smelt_proto_accessor(value: &SmeltUnknown) -> SmeltUnknown { match value { SmeltUnknown::Object(map) if map.contains_key(\"__proto__\") => smelt_get_object_field(map, \"__proto__\"), other => smelt_prototype_sentinel(other) } }");
        writer.blank_line();
        writer.line("/// Resolve the JavaScript `Object.prototype.toString.call(x)` tag for an erased value.");
        writer.line("///");
        writer.line("/// Primitive and function variants map to their spec tags. Object records");
        writer.line("/// resolve through Smelt's host identity markers: dates, regexps, errors, the");
        writer.line("/// global object, abort records, registry host objects (WeakMap, Blob, boxed");
        writer.line("/// primitives, Intl instances, ...), and builtin namespace records (whose");
        writer.line("/// `@@toStringTag`-bearing `name` becomes the tag, matching `[object JSON]` /");
        writer.line("/// `[object Math]`). Class instances and unmarked records are plain");
        writer.line("/// `[object Object]`, exactly like JavaScript objects without a custom tag.");
        {
            // ES2024 §20.1.3.6 step 15: a string-valued `@@toStringTag` wins over
            // every builtin tag, which is how a plain object can report
            // `[object string-tagged]` and stop answering `isPlainObject`. The key
            // comes from the shared well-known-symbol table, so the tag a source
            // `{ [Symbol.toStringTag]: 'x' }` declares is the very key read here.
            let to_string_tag_key = smelt_stdlib::well_known_symbols::storage_key("toStringTag")
                .expect("toStringTag is a modeled well-known symbol");
            let host_tag_arms = smelt_stdlib::HOST_OBJECTS.iter().fold(
                String::new(),
                |mut arms, entry| {
                    use ::std::fmt::Write as _;
                    let _ = write!(
                        arms,
                        "if map.contains_key(\"{marker}\") {{ return \"[object {tag}]\".to_owned(); }} ",
                        marker = entry.marker,
                        tag = entry.to_string_tag,
                    );
                    arms
                },
            );
            writer.line(format!(
                "fn smelt_object_to_string_tag(value: &SmeltUnknown) -> String {{ match value {{ SmeltUnknown::Null => \"[object Null]\".to_owned(), SmeltUnknown::Undefined => \"[object Undefined]\".to_owned(), SmeltUnknown::Bool(_) => \"[object Boolean]\".to_owned(), SmeltUnknown::Number(_) => \"[object Number]\".to_owned(), SmeltUnknown::String(_) => \"[object String]\".to_owned(), SmeltUnknown::Symbol(_) => \"[object Symbol]\".to_owned(), SmeltUnknown::Array(_) => \"[object Array]\".to_owned(), SmeltUnknown::Function(_) => \"[object Function]\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned(), SmeltUnknown::Object(map) => {{ if let Some(SmeltUnknown::String(tag)) = map.get({to_string_tag_key:?}) {{ return format!(\"[object {{tag}}]\"); }} if map.contains_key(\"__smelt_date\") {{ return \"[object Date]\".to_owned(); }} if map.contains_key(\"__smelt_regexp\") {{ return \"[object RegExp]\".to_owned(); }} if map.contains_key(\"__smelt_error\") {{ return \"[object Error]\".to_owned(); }} if map.contains_key(\"__smelt_global_object\") {{ return \"[object global]\".to_owned(); }} if map.contains_key(\"__smelt_abortcontroller\") {{ return \"[object AbortController]\".to_owned(); }} if map.contains_key(\"__smelt_abortsignal\") {{ return \"[object AbortSignal]\".to_owned(); }} if map.contains_key(\"__smelt_map\") {{ return \"[object Map]\".to_owned(); }} if map.contains_key(\"__smelt_set\") {{ return \"[object Set]\".to_owned(); }} if map.contains_key(\"__smelt_arguments\") {{ return \"[object Arguments]\".to_owned(); }} {host_tag_arms}if map.contains_key(\"__smelt_builtin_namespace\") {{ if let Some(SmeltUnknown::String(name)) = map.get(\"name\") {{ return format!(\"[object {{name}}]\"); }} }} \"[object Object]\".to_owned() }} }} }}",
            ));
        }
        writer.blank_line();
        writer.line("impl PartialEq for SmeltObject { fn eq(&self, other: &Self) -> bool { let mut smelt_seen = ::std::collections::HashSet::new(); smelt_object_structural_eq(self, other, &mut smelt_seen) } }");
        writer.line("impl Eq for SmeltObject {}");
        writer.line("impl ::std::hash::Hash for SmeltObject { fn hash<H: ::std::hash::Hasher>(&self, state: &mut H) { let mut smelt_seen = ::std::collections::HashSet::new(); smelt_object_structural_hash(self, state, &mut smelt_seen); } }");
        writer.line("impl IntoIterator for SmeltObject { type Item = (String, SmeltUnknown); type IntoIter = ::std::vec::IntoIter<(String, SmeltUnknown)>; fn into_iter(self) -> Self::IntoIter { self.iter() } }");
        writer.blank_line();
        // An erased JavaScript array is a REFERENCE, exactly like `SmeltObject`:
        // the elements live in a shared `Rc<RefCell<Vec<_>>>` so every clone of the
        // handle observes the same buffer. Codegen `.clone()`s erased values freely
        // as they flow through expressions, and JS semantics require those copies to
        // stay aliases.
        //
        // This used to be a plain `Vec`, which silently dropped writes. `obj.a[2] = 4`
        // emits "read the field into a temporary, `smelt_index_assign` into the
        // temporary" with no write-back — correct for an object payload only because
        // `SmeltObject` already shared its `Rc`. With a copied `Vec` the array
        // mutation went to the temporary and the parent never saw it. It also broke
        // es-toolkit's cycle guards, which do `stack.set(a, b)` before recursing and
        // then rely on the stored handle aliasing the array being filled in.
        //
        // A JavaScript array is an *exotic object*: it has index elements AND
        // ordinary named properties (`const a = ['1']; a.x = 2` keeps `a` an
        // array, with `a.length === 1` and `a.x === 2`). `props` is where the
        // named half lives — lazily allocated, so an array that never takes a
        // named property costs one `Rc` and no `Vec`, and shared through its own
        // `Rc` so every alias of the array observes a named write exactly as it
        // observes an element write. Values are `SmeltUnknown` regardless of what
        // the elements are: a named write only reaches an array through an erased
        // receiver (TypeScript's `T[]` has no named members, so the source needs
        // an `as any` first), which is a genuine dynamic boundary.
        //
        // Both store seams used to REPLACE the array with a fresh one-property
        // object when the key was not an index, which lost the elements and made
        // `Array.isArray` answer `false`. `props` is deliberately invisible to
        // structural equality, `len()`, iteration and JSON — JavaScript compares
        // arrays index-wise (es-toolkit `isEqualWith`'s array arm compares only
        // `length` and the elements) and `JSON.stringify` serializes only the
        // elements — and visible to property reads and to key enumeration, where
        // it follows the index keys.
        writer.line("pub struct SmeltArray {");
        writer.line("    id: usize,");
        writer.line(
            "    values: ::std::rc::Rc<::std::cell::RefCell<Vec<SmeltUnknown>>>,",
        );
        writer.line(
            "    props: ::std::rc::Rc<::std::cell::RefCell<Option<Vec<(String, SmeltUnknown)>>>>,",
        );
        writer.line("}");
        writer.blank_line();
        // Hand-written `Debug` rather than a derive, so the shared cell does not
        // show up as `RefCell { value: [..] }` in output that used to read
        // `SmeltArray { id: 1, values: [..] }`. Keeping the rendering byte-identical
        // means the storage change is invisible to anything that formats a value.
        writer.line("impl ::std::fmt::Debug for SmeltArray { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct(\"SmeltArray\").field(\"id\", &self.id).field(\"values\", &*self.values.borrow()).finish() } }");
        writer.line("impl Clone for SmeltArray { fn clone(&self) -> Self { Self { id: self.id, values: self.values.clone(), props: self.props.clone() } } }");
        writer.line("impl SmeltArray {");
        writer.line("    /// Create an identity-bearing erased JavaScript array.");
        writer.line("    fn new(values: Vec<SmeltUnknown>) -> Self { Self { id: smelt_next_object_id(), values: ::std::rc::Rc::new(::std::cell::RefCell::new(values)), props: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }");
        writer.line(
            "    /// Reuse a caller-supplied identity so repeated erasures of one source list compare `===` equal.",
        );
        writer.line(
            "    fn with_id(id: usize, values: Vec<SmeltUnknown>) -> Self { Self { id, values: ::std::rc::Rc::new(::std::cell::RefCell::new(values)), props: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }",
        );
        writer.line(
            "    /// Reuse an existing shared buffer, so a re-wrap keeps aliasing the same array.",
        );
        writer.line("    fn with_storage(id: usize, values: ::std::rc::Rc<::std::cell::RefCell<Vec<SmeltUnknown>>>) -> Self { Self { id, values, props: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }");
        // The twin of `SmeltList::storage()`. An erased array re-wrapped as a
        // typed `SmeltList<SmeltUnknown>` must keep aliasing the SAME buffer, or
        // the round-trip produces a stale snapshot wearing the live array's id
        // (see the `From<SmeltList<SmeltUnknown>>` comment below for the same
        // argument in the other direction). Without this accessor the only way
        // back was `into_vec()`, which copies.
        writer.line("    /// Another handle on this array's shared buffer.");
        writer.line("    fn storage(&self) -> ::std::rc::Rc<::std::cell::RefCell<Vec<SmeltUnknown>>> { ::std::rc::Rc::clone(&self.values) }");
        writer.line(
            "    /// Snapshot the elements. This COPIES: mutating the result does not write back.",
        );
        writer.line("    fn into_vec(self) -> Vec<SmeltUnknown> { self.values.borrow().clone() }");
        writer.line("    /// Element count.");
        writer.line("    fn len(&self) -> usize { self.values.borrow().len() }");
        writer.line("    /// Whether the array holds no elements.");
        writer.line("    fn is_empty(&self) -> bool { self.values.borrow().is_empty() }");
        writer.line(
            "    /// Read one element by index, cloned out of the shared buffer (JS `arr[i]`).",
        );
        writer.line("    fn get(&self, index: usize) -> Option<SmeltUnknown> { self.values.borrow().get(index).cloned() }");
        writer.line("    /// Iterate a snapshot of the elements, so the buffer is not borrowed across the loop body.");
        writer.line("    fn iter(&self) -> ::std::vec::IntoIter<SmeltUnknown> { self.values.borrow().clone().into_iter() }");
        writer.line(
            "    /// Set the element at a numeric index, extending with `undefined` holes to match JS `arr[i] = v`.",
        );
        writer.line("    fn set_index(&self, index: usize, value: SmeltUnknown) { let mut values = self.values.borrow_mut(); if index >= values.len() { values.resize(index.saturating_add(1), SmeltUnknown::Undefined); } values[index] = value; }");
        writer.line("    /// Append one element (JS `arr.push(v)`), through the shared buffer.");
        writer.line("    fn push(&self, value: SmeltUnknown) { self.values.borrow_mut().push(value); }");
        writer.line("    /// Replace every element in place, so aliases observe the new contents.");
        writer.line("    fn replace_all(&self, values: Vec<SmeltUnknown>) { *self.values.borrow_mut() = values; }");
        // The named half of the array's property model. Kept as an insertion-ordered
        // `Vec` rather than a map because JavaScript enumerates an object's string
        // keys in insertion order and there are only ever a handful of them.
        writer.line("    /// Read a NON-INDEX named property (JS `arr.x`), or `None` when absent.");
        writer.line("    fn named_property(&self, key: &str) -> Option<SmeltUnknown> { self.props.borrow().as_ref().and_then(|props| props.iter().find(|(name, _)| name == key).map(|(_, value)| value.clone())) }");
        writer.line("    /// Write a NON-INDEX named property (JS `arr.x = v`), allocating the side table on first use.");
        writer.line("    fn set_named_property(&self, key: String, value: SmeltUnknown) { let mut props = self.props.borrow_mut(); let props = props.get_or_insert_with(Vec::new); match props.iter_mut().find(|(name, _)| *name == key) { Some(slot) => slot.1 = value, None => props.push((key, value)) } }");
        writer.line("    /// The named property keys, in insertion order (empty in the common case).");
        writer.line("    fn named_keys(&self) -> Vec<String> { self.props.borrow().as_ref().map_or_else(Vec::new, |props| props.iter().map(|(name, _)| name.clone()).collect()) }");
        writer.line("    /// Own enumerable keys: the element indices, then the named properties, exactly the order `Object.keys` reports.");
        writer.line("    fn own_keys(&self) -> Vec<String> { let mut keys = (0..self.len()).map(|index| index.to_string()).collect::<Vec<_>>(); keys.extend(self.named_keys()); keys }");
        writer.line("    /// Own enumerable entries, paired with the keys `own_keys` reports.");
        writer.line("    fn own_entries(&self) -> Vec<(String, SmeltUnknown)> { let mut entries = self.values.borrow().iter().enumerate().map(|(index, value)| (index.to_string(), value.clone())).collect::<Vec<_>>(); if let Some(props) = self.props.borrow().as_ref() { entries.extend(props.iter().cloned()); } entries }");
        writer.line("}");
        writer.line("impl From<Vec<SmeltUnknown>> for SmeltArray { fn from(values: Vec<SmeltUnknown>) -> Self { Self::new(values) } }");
        writer.line("impl ::std::iter::FromIterator<SmeltUnknown> for SmeltArray { fn from_iter<T: IntoIterator<Item = SmeltUnknown>>(iter: T) -> Self { Self::new(iter.into_iter().collect()) } }");
        // No `Deref` to `[SmeltUnknown]`: the elements live behind a `RefCell`, so
        // there is no `&[SmeltUnknown]` to hand out that could outlive the borrow.
        // `len`/`is_empty`/`get`/`iter` above cover what the old deref was used for,
        // each taking its own short-lived borrow.
        writer.line("impl IntoIterator for SmeltArray { type Item = SmeltUnknown; type IntoIter = ::std::vec::IntoIter<SmeltUnknown>; fn into_iter(self) -> Self::IntoIter { self.values.borrow().clone().into_iter() } }");
        writer.line("impl<'smelt_array> IntoIterator for &'smelt_array SmeltArray { type Item = SmeltUnknown; type IntoIter = ::std::vec::IntoIter<SmeltUnknown>; fn into_iter(self) -> Self::IntoIter { self.values.borrow().clone().into_iter() } }");
        writer.blank_line();
        // `SmeltList<T>` itself is defined in the `needs_smelt_list` block above.
        // These impls depend on `SmeltArray`/`SmeltUnknown`, so they live here.
        // Erasing a typed list to a `SmeltUnknown::Array` ALIASES its buffer.
        //
        // `SmeltList<SmeltUnknown>` and `SmeltArray` have the identical
        // `Rc<RefCell<Vec<SmeltUnknown>>>` representation, and in JavaScript passing
        // an array where `unknown` is expected hands over THE SAME object: the erased
        // value must observe every later write through the typed handle, and a write
        // through the erased value must be visible to the typed one. So the erasure
        // is a `with_storage` refcount bump that carries both halves of the array's
        // reference semantics — the stable `id` AND the shared buffer.
        //
        // This used to copy the elements (`with_id(list.id(), list.into_vec())`),
        // which kept the identity but detached the storage, producing a half
        // reference: a stale snapshot wearing the live array's `id`. That is wrong
        // for any array mutated after it is erased, and it is what made
        // `isEqualWith`'s cycle guard unable to see a circular array — the erased
        // element could never BE the array it was taken from, so `Object.is(a, b)`
        // compared a live array against a frozen copy of itself.
        //
        // Sharing was tried and reverted once, in #219, after remeda's
        // `uniqueBy > pipe get executed 3 times when take before uniqueBy` panicked
        // with `unknown is not array` INTERMITTENTLY. That intermittency was later
        // root-caused and it was NOT this copy: identity-registry keys are `Rc`
        // addresses, and a freed address handed to a later callback inherited the
        // dead callback's `SMELT_CALLABLE_OBJECTS` entry (see the address-reuse
        // comment on `SMELT_CALLABLE_KEY_GUARDS` above, and
        // `callable_object_identity_runtime`). Sharing the buffer only perturbed
        // allocation enough to change how often the aliasing was observed. With that
        // fix in place the remeda suite was re-measured over twenty consecutive runs
        // with the sharing below and stayed 1789/1789.
        writer.line("impl From<SmeltList<SmeltUnknown>> for SmeltArray { fn from(list: SmeltList<SmeltUnknown>) -> Self { SmeltArray::with_storage(list.id(), list.storage()) } }");
        // A callback that declares a list parameter receives it by shared reference
        // (see `callback_param_is_shared_reference`), so the erasure adapters need to
        // build an erased array from `&SmeltList` as well as from an owned one. The
        // reference form aliases the same buffer for the same reason the owned form
        // does; borrowing rather than owning the handle changes nothing about the
        // array's identity.
        writer.line("impl From<&SmeltList<SmeltUnknown>> for SmeltArray { fn from(list: &SmeltList<SmeltUnknown>) -> Self { SmeltArray::with_storage(list.id(), list.storage()) } }");
        writer.line("impl<T: Clone> From<&SmeltList<T>> for Vec<T> { fn from(list: &SmeltList<T>) -> Self { list.to_vec() } }");
        // serde impls only when the crate actually links serde (JSON contexts).
        if needs_serde_json {
            writer.line("impl<T: serde::Serialize> serde::Serialize for SmeltList<T> { fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> { serde::Serialize::serialize(&*self.borrow(), serializer) } }");
            writer.line("impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for SmeltList<T> { fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> { <Vec<T> as serde::Deserialize>::deserialize(deserializer).map(SmeltList::new) } }");
        }
        writer.line("type SmeltPromiseFuture = ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<SmeltUnknown, Box<dyn std::error::Error>>>>>;");
        writer.blank_line();
        // JS eager-async-prefix semantics. Calling an `async` function (or running
        // a `new Promise` executor) executes SYNCHRONOUSLY up to the first `await`;
        // only the continuation after that suspension point is deferred to a later
        // microtask. Smelt models a promise value as a lazily-stored future that is
        // not driven until `smelt_await`, which would instead defer the ENTIRE body
        // (including its synchronous prefix). That breaks any code whose observable
        // ordering depends on all prefixes running before the event loop turns —
        // e.g. `Promise.all([api.call(), api.call(), api.call()])` where each
        // `call` synchronously registers a request with a batch/funnel scheduler:
        // lazily, the first `await` inside `Promise.all` drains a timer after only
        // the first prefix ran, splitting one batch into three.
        //
        // `smelt_eager_poll_waker` is a no-op `Waker` used to poll a freshly
        // constructed future exactly once at construction time, advancing it
        // through its synchronous prefix up to (and not past) the first real
        // suspension. The poll's result is folded into the promise's shared settle
        // state (see `SmeltPromise::from_future` / `SmeltFuture::from_future`); a
        // still-`Pending` future is kept and later resumed by `smelt_await` under
        // the real executor waker. Because the no-op waker never schedules a wake,
        // it is only ever valid for this single priming poll.
        //
        // Error observability contract: a synchronous prefix that throws lowers to
        // a future that resolves `Poll::Ready(Err(_))` on its first poll. Per JS,
        // an exception thrown before the first `await` of an `async` function
        // becomes a REJECTED promise, not a synchronous throw, and its rejection is
        // observable only through the normal await/handler path in microtask order.
        // The priming poll therefore CAPTURES a `Ready(Err)` into the shared
        // rejected settle state and NEVER propagates it out of `from_future`;
        // `smelt_await` surfaces it exactly when a consumer awaits, preserving when
        // rejections become observable relative to other continuations and timers.
        writer.line("fn smelt_eager_poll_waker() -> ::std::task::Waker {");
        writer.line("    unsafe fn clone(_: *const ()) -> ::std::task::RawWaker { raw() }");
        writer.line("    unsafe fn wake(_: *const ()) {}");
        writer.line("    unsafe fn wake_by_ref(_: *const ()) {}");
        writer.line("    unsafe fn drop(_: *const ()) {}");
        writer.line("    fn raw() -> ::std::task::RawWaker { ::std::task::RawWaker::new(::std::ptr::null(), &::std::task::RawWakerVTable::new(clone, wake, wake_by_ref, drop)) }");
        writer.line("    unsafe { ::std::task::Waker::from_raw(raw()) }");
        writer.line("}");
        writer.blank_line();
        writer.line("#[derive(Clone)]");
        writer.line("pub struct SmeltPromise {");
        writer.line("    id: usize,");
        // The settled state's error slot is the *same* exception-payload ABI
        // `smelt_throw`/`smelt_thrown_value` define (see `thrown.rs`), not a new
        // erasure: a JavaScript rejection reason is any value at all, so it has
        // no static type to preserve here. It previously held a `String`, which
        // silently destroyed every rejection reason that was not exactly its own
        // `message` — `Promise.reject({ status: 400 })` settled as the string
        // "[object Object]" and was re-inflated on await as a synthetic
        // `{ __smelt_error: "Error", message }` record with `status` gone. Since
        // the payload arrives as a `SmeltUnknown` and leaves as one, storing it
        // as a `SmeltUnknown` keeps it whole across the settle boundary.
        writer.line(
            "    state: ::std::rc::Rc<::std::cell::RefCell<Option<Result<SmeltUnknown, SmeltUnknown>>>>,",
        );
        writer.line("    future: ::std::rc::Rc<::std::cell::RefCell<Option<SmeltPromiseFuture>>>,");
        writer.line("}");
        writer.blank_line();
        writer.line("impl SmeltPromise {");
        writer.line("    /// Create a pending erased promise identity with shared settle state.");
        writer.line("    fn pending() -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), future: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }");
        writer.line("    /// Create a pending promise with a preassigned identity.");
        writer.line("    fn pending_with_id(id: usize) -> Self { Self { id, state: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), future: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }");
        writer.line("    /// Create an already-fulfilled erased promise value.");
        writer.line("    fn resolved(value: SmeltUnknown) -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(Some(Ok(value)))), future: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }");
        if needs_vitest_mock {
            // `SmeltPromise::rejected` is only referenced by the Vitest mock
            // runtime (`mockRejectedValue*`), so it is gated on the same flag to
            // keep every non-mock crate's prelude byte-identical.
            writer.line("    /// Create an already-rejected erased promise value. The rejection");
            writer.line("    /// reason is kept whole in the shared settle state and re-enters the");
            writer.line("    /// error channel unchanged on await, so a non-`Error` reason keeps its");
            writer.line("    /// own properties (JavaScript rejects with any value, not a message).");
            writer.line("    fn rejected(value: SmeltUnknown) -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(Some(Err(value)))), future: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }");
        }
        writer.line("    /// Store a live future behind a cloneable erased promise handle. This");
        writer.line("    /// is the lazy constructor used by derived/adapter promises (await");
        writer.line("    /// flattening, `.then`/`.catch` chains, coercions): the future is not");
        writer.line("    /// driven until `smelt_await`, matching JS deferral of those.");
        writer.line("    fn from_future(future: SmeltPromiseFuture) -> Self { Self { id: smelt_next_object_id(), state: ::std::rc::Rc::new(::std::cell::RefCell::new(None)), future: ::std::rc::Rc::new(::std::cell::RefCell::new(Some(future))) } }");
        writer
            .line("    /// Await the stored future once and share its settled result with clones.");
        writer.line(
            "    async fn smelt_await(&self) -> Result<SmeltUnknown, Box<dyn std::error::Error>> {",
        );
        writer.line("        if self.state.borrow().is_none() {");
        // Bind the taken future to a local before awaiting so no `RefMut` from
        // `borrow_mut()` stays alive across the `.await` (pre-2024-edition
        // temporary-scope rules would otherwise keep it borrowed and panic on
        // re-entry through the shared cell).
        writer.line("            let taken = self.future.borrow_mut().take();");
        writer.line("            if let Some(future) = taken {");
        writer
            .line("                let settled = future.await.map_err(|error| smelt_thrown_value(&*error));");
        writer.line("                *self.state.borrow_mut() = Some(settled);");
        writer.line("            }");
        writer.line("        }");
        writer.line("        loop {");
        writer.line("            if let Some(result) = self.state.borrow().clone() {");
        writer.line("                return result.map_err(smelt_throw);");
        writer.line("            }");
        // The result cell is still empty: another task (or a timer callback) must
        // settle it. When timer helpers exist, drive the cooperative scheduler and
        // virtual clock the same way the spin-wait futures do (H1/H3) so a future
        // handed to the detached promise-task queue, or one whose resolution
        // depends on a pending timer, cannot spin forever with nothing advancing
        // time. Without timer helpers there is no timer queue or detached-task
        // queue to drive, so a bare cooperative yield is the only (and correct)
        // driver, and the helper symbols are not emitted to reference.
        if needs_timer_helpers {
            writer.line(format!(
                "            {sleep_ms}(0.0).await;",
                sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
            ));
        }
        writer.line("            tokio::task::yield_now().await;");
        writer.line("        }");
        writer.line("    }");
        writer.line("}");
        writer.line("impl ::std::fmt::Debug for SmeltPromise { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.debug_struct(\"SmeltPromise\").field(\"id\", &self.id).finish() } }");
        writer.blank_line();
        // JavaScript `await` flattens: awaiting a value that is itself a promise
        // resolves through the whole chain. An `async` function that returns a
        // `Promise` erases that inner promise to `SmeltUnknown::Promise`, so a
        // consumer that awaits the function's result must keep awaiting while the
        // settled value is still a promise. This helper drives the shared cell of
        // each erased promise (which in turn drives the cooperative scheduler),
        // and is a no-op pass-through for any non-promise value.
        writer.line("#[allow(dead_code)]");
        writer.line("async fn smelt_await_flatten(value: SmeltUnknown) -> Result<SmeltUnknown, Box<dyn std::error::Error>> {");
        writer.line("    let mut current = value;");
        writer.line("    while let SmeltUnknown::Promise(promise) = current {");
        writer.line("        current = promise.smelt_await().await?;");
        writer.line("    }");
        writer.line("    Ok(current)");
        writer.line("}");
        writer.blank_line();
        // Generic promise-value ABI. A source `Promise<T>` / `Type::Future(T)`
        // lowers to `SmeltFuture<T>` in *every* position (parameter, field,
        // return, local, async-op result), so the same MIR future type renders
        // one Rust type everywhere instead of a bare
        // `Pin<Box<dyn Future>>` in some positions and a value in others. The
        // handle is a shared `Rc<RefCell<..>>` so it is cheaply `Clone`
        // (matching JS promise-value copy semantics) and `Default` (a ready
        // default value). `smelt_await` drives the stored future once, caches the
        // resolved value, and serves later awaits from that cache — JS promises
        // may be awaited multiple times, and each await after the first resolves
        // from the cached value (single-consumer of the underlying future).
        writer.line("#[allow(dead_code)]");
        writer.line("enum SmeltFutureState<T> {");
        writer.line("    Pending(::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>),");
        writer.line("    Resolved(T),");
        // A synchronous prefix that threw during eager priming (see
        // `smelt_eager_poll_waker`) is stored as a rejection. JS promises may be
        // awaited more than once and each await re-observes the SAME rejection,
        // so the state has to hold something `smelt_await` can rebuild an error
        // from on every call — which is why it is a value and not the
        // `Box<dyn Error>` itself. It holds the thrown payload
        // `smelt_throw`/`smelt_thrown_value` already define (see `thrown.rs`),
        // not a new erasure: this slot previously held a `String`, which reduced
        // every rejection to its message text and re-inflated it as a synthetic
        // `{ __smelt_error, message }` record, so `async () => { throw "oops"; }`
        // handed its `catch` an object where JavaScript hands it the string.
        writer.line("    Rejected(SmeltUnknown),");
        writer.line("    Taken,");
        writer.line("}");
        // A priming poll must not own the virtual clock. `from_future_primed`
        // runs an async body's synchronous prefix at call time; in JavaScript
        // that prefix only *schedules* its timers, it does not make time pass.
        // The virtual-clock sleep helper advances the clock to its own deadline
        // when it is driven, so without this marker priming
        // `withTimeout(() => delay(1000), 50)`'s `run()` jumped the clock 1000ms
        // before the 50ms deadline was even armed and the timeout could never
        // win. This is the same rule `SMELT_RACE_DEPTH` already states for a
        // `Promise.race` driver, applied to the other place that polls a future
        // out of band.
        writer.line("thread_local! {");
        writer.line("    /// Non-zero while `SmeltFuture::from_future_primed` is running its");
        writer.line("    /// eager prefix poll; see `smelt_sleep_ms`.");
        writer.line("    static SMELT_PRIME_DEPTH: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(0) };");
        writer.line("}");
        writer.line("pub struct SmeltFuture<T> {");
        writer.line("    state: ::std::rc::Rc<::std::cell::RefCell<SmeltFutureState<T>>>,");
        writer.line("}");
        writer.line("impl<T> Clone for SmeltFuture<T> { fn clone(&self) -> Self { Self { state: self.state.clone() } } }");
        writer.line("impl<T: Default> Default for SmeltFuture<T> { fn default() -> Self { Self::resolved(T::default()) } }");
        writer.line("impl<T> ::std::fmt::Debug for SmeltFuture<T> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(\"SmeltFuture\") } }");
        writer.line("#[allow(dead_code)]");
        writer.line("impl<T> SmeltFuture<T> {");
        writer.line("    /// Wrap a live future behind a cloneable promise-value handle, lazily:");
        writer.line("    /// the future is not driven until `smelt_await`. Used by derived and");
        writer.line("    /// adapter promises (await flattening, `.then`/`.catch` chains, callback");
        writer.line("    /// coercions, `Promise.all` collection) whose bodies JS also defers,");
        writer.line("    /// so priming them would run continuations/handlers out of microtask");
        writer.line("    /// order — see `from_future_primed` for the eager-prefix constructor.");
        writer.line("    fn from_future(future: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>) -> Self { Self { state: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltFutureState::Pending(future))) } }");
        writer.line("    /// Wrap an ASYNC-FUNCTION-BODY future, priming it with a single no-op-");
        writer.line("    /// waker poll so its synchronous prefix runs at call time (JS");
        writer.line("    /// eager-async-prefix semantics; see `smelt_eager_poll_waker`). Only");
        writer.line("    /// genuine async function / async closure / async method bodies use this,");
        writer.line("    /// so a call like `api.call()` registers its request synchronously before");
        writer.line("    /// the event loop turns — the difference that keeps");
        writer.line("    /// `Promise.all([call(), call(), call()])` from splitting a funnel/batch.");
        writer.line("    /// A prefix that completes resolves now; one that throws is captured as a");
        writer.line("    /// rejection surfaced only via `smelt_await` (JS: a throw before the first");
        writer.line("    /// await becomes a rejected promise, observable only through the");
        writer.line("    /// await/handler path); one that suspends keeps the advanced future for");
        writer.line("    /// later resume. Derived/adapter promises deliberately do NOT use this,");
        writer.line("    /// preserving when their continuations and rejections become observable.");
        writer.line("    fn from_future_primed(mut future: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>) -> Self {");
        writer.line("        struct SmeltPrimeGuard;");
        writer.line("        impl Drop for SmeltPrimeGuard { fn drop(&mut self) { SMELT_PRIME_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1))); } }");
        writer.line("        SMELT_PRIME_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));");
        writer.line("        let _smelt_prime_guard = SmeltPrimeGuard;");
        writer.line("        let waker = smelt_eager_poll_waker();");
        writer.line("        let mut cx = ::std::task::Context::from_waker(&waker);");
        writer.line("        let state = match ::std::future::Future::poll(future.as_mut(), &mut cx) {");
        writer.line("            ::std::task::Poll::Ready(Ok(value)) => SmeltFutureState::Resolved(value),");
        writer.line("            ::std::task::Poll::Ready(Err(error)) => SmeltFutureState::Rejected(smelt_thrown_value(&*error)),");
        writer.line("            ::std::task::Poll::Pending => SmeltFutureState::Pending(future),");
        writer.line("        };");
        writer.line("        Self { state: ::std::rc::Rc::new(::std::cell::RefCell::new(state)) }");
        writer.line("    }");
        writer.line("    /// Build an already-resolved promise-value handle.");
        writer.line("    fn resolved(value: T) -> Self { Self { state: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltFutureState::Resolved(value))) } }");
        writer.line("    /// Drive the stored future once, cache its resolved value, and serve");
        writer.line("    /// later awaits from that cache (single-consumer of the future).");
        writer.line("    async fn smelt_await(&self) -> Result<T, Box<dyn std::error::Error>> where T: Clone {");
        writer.line("        let pending = {");
        writer.line("            let mut guard = self.state.borrow_mut();");
        writer.line("            if matches!(&*guard, SmeltFutureState::Pending(_)) {");
        writer.line("                match ::std::mem::replace(&mut *guard, SmeltFutureState::Taken) {");
        writer.line("                    SmeltFutureState::Pending(future) => Some(future),");
        writer.line("                    _ => None,");
        writer.line("                }");
        writer.line("            } else { None }");
        writer.line("        };");
        writer.line("        if let Some(future) = pending {");
        writer.line("            let value = future.await?;");
        writer.line("            *self.state.borrow_mut() = SmeltFutureState::Resolved(value.clone());");
        writer.line("            return Ok(value);");
        writer.line("        }");
        writer.line("        let guard = self.state.borrow();");
        writer.line("        match &*guard {");
        writer.line("            SmeltFutureState::Resolved(value) => Ok(value.clone()),");
        writer.line("            SmeltFutureState::Rejected(payload) => Err(smelt_throw(payload.clone())),");
        writer.line("            _ => Err(std::io::Error::new(std::io::ErrorKind::Other, \"future already consumed\").into()),");
        writer.line("        }");
        writer.line("    }");
        writer.line("}");
        // `SmeltFuture<T>` is `IntoFuture`, so an ordinary Rust `.await` on a
        // promise value drives it through `smelt_await` — every generated `.await`
        // site works on the wrapper without a bespoke call form. The underlying
        // futures are `'static` (the prior `Pin<Box<dyn Future>>` ABI was too), so
        // `T: 'static` here matches existing constraints.
        writer.line("impl<T: Clone + 'static> ::std::future::IntoFuture for SmeltFuture<T> {");
        writer.line("    type Output = Result<T, Box<dyn std::error::Error>>;");
        writer.line("    type IntoFuture = ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>;");
        writer.line("    fn into_future(self) -> Self::IntoFuture { Box::pin(async move { self.smelt_await().await }) }");
        writer.line("}");
        // Erasing a typed promise value (`SmeltFuture<T>`) to the dynamic carrier
        // yields a `SmeltUnknown::Promise`: a JS `Promise<T>` flowing into an
        // `unknown`/erased position is still a promise object, not its resolved
        // value. This is a genuine dynamic boundary — the erased consumer only
        // knows the promise/thenable protocol, so no concrete `T` survives. The
        // adapter defers exactly like JS: it wraps a fresh `SmeltPromise` whose
        // body awaits this future and erases the settled value through
        // `IntoSmeltUnknown`, matching the `SmeltUnknown::Promise(SmeltPromise::
        // from_future(..))` shape the future-recovery coercion emits. This impl
        // must exist whenever generated code can call `.into_smelt_unknown()` on a
        // future — e.g. the erased-callback promise adapter and the recover-erased-
        // promise-on-await coercion both do — and it lives in this same
        // `SmeltUnknown`/`SmeltPromise` prelude region so the gate is shared.
        writer.line("impl<T: IntoSmeltUnknown + Clone + 'static> IntoSmeltUnknown for SmeltFuture<T> {");
        writer.line("    fn into_smelt_unknown(self) -> SmeltUnknown { SmeltUnknown::Promise(SmeltPromise::from_future(Box::pin(async move { let smelt_resolved = self.smelt_await().await?; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_resolved.into_smelt_unknown()) }))) }");
        writer.line("}");
        writer.blank_line();
        // Synchronous TypeScript generators are true resumable computations.
        // The producer future itself has an anonymous type, so the public ABI
        // stores only a typed resume closure; yielded and returned values remain
        // concrete and never cross the dynamic `SmeltUnknown` boundary.
        if needs_generator {
        writer.line("#[derive(Clone, Debug)]");
        writer.line("pub enum SmeltGeneratorResult<Y, R> { Yielded(Y), Complete(R) }");
        // JavaScript permits throwing any runtime value, independently of the
        // generator's concrete Y/R/N parameters. This is a genuine dynamic
        // boundary: preserving a number, object, symbol, or string for a catch
        // binding requires the tagged runtime carrier rather than a concrete
        // Rust generic shared with the other protocol channels.
        writer.line("#[derive(Clone, Debug)]");
        writer.line("pub enum SmeltGeneratorCommand<N, R> { Next(N), Return(R), Throw(SmeltUnknown) }");
        writer.line("pub struct SmeltGenerator<Y, R, N> {");
        writer.line("    resume: ::std::rc::Rc<::std::cell::RefCell<Box<dyn FnMut(SmeltGeneratorCommand<N, R>) -> SmeltGeneratorResult<Y, R>>>>,");
        writer.line("    completed: ::std::rc::Rc<::std::cell::RefCell<Option<R>>>,");
        writer.line("}");
        writer.line("impl<Y, R, N> Clone for SmeltGenerator<Y, R, N> { fn clone(&self) -> Self { Self { resume: self.resume.clone(), completed: self.completed.clone() } } }");
        writer.line("impl<Y, R, N> ::std::fmt::Debug for SmeltGenerator<Y, R, N> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(\"SmeltGenerator\") } }");
        writer.line("impl<Y, R: Clone, N> SmeltGenerator<Y, R, N> {");
        writer.line("    /// Hide the producer future type while retaining concrete yield/return types.");
        writer.line("    fn new(resume: impl FnMut(SmeltGeneratorCommand<N, R>) -> SmeltGeneratorResult<Y, R> + 'static) -> Self { Self { resume: ::std::rc::Rc::new(::std::cell::RefCell::new(Box::new(resume))), completed: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }");
        writer.line("    /// Resume execution until the next suspension point or completion.");
        writer.line("    fn resume(&self, command: SmeltGeneratorCommand<N, R>) -> SmeltGeneratorResult<Y, R> { if let Some(value) = self.completed.borrow().clone() { return SmeltGeneratorResult::Complete(value); } let result = (self.resume.borrow_mut())(command); if let SmeltGeneratorResult::Complete(value) = &result { *self.completed.borrow_mut() = Some(value.clone()); } result }");
        writer.line("}");
        writer.line("pub struct SmeltAsyncGenerator<Y, R, N> {");
        writer.line("    resume: ::std::rc::Rc<dyn Fn(SmeltGeneratorCommand<N, R>) -> SmeltFuture<SmeltGeneratorResult<Y, R>>>,");
        writer.line("    completed: ::std::rc::Rc<::std::cell::RefCell<Option<R>>>,");
        writer.line("}");
        writer.line("impl<Y, R, N> Clone for SmeltAsyncGenerator<Y, R, N> { fn clone(&self) -> Self { Self { resume: self.resume.clone(), completed: self.completed.clone() } } }");
        writer.line("impl<Y, R, N> ::std::fmt::Debug for SmeltAsyncGenerator<Y, R, N> { fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result { formatter.write_str(\"SmeltAsyncGenerator\") } }");
        writer.line("impl<Y: Clone + 'static, R: Clone + 'static, N> SmeltAsyncGenerator<Y, R, N> {");
        writer.line("    fn new(resume: impl Fn(SmeltGeneratorCommand<N, R>) -> SmeltFuture<SmeltGeneratorResult<Y, R>> + 'static) -> Self { Self { resume: ::std::rc::Rc::new(resume), completed: ::std::rc::Rc::new(::std::cell::RefCell::new(None)) } }");
        writer.line("    fn resume(&self, command: SmeltGeneratorCommand<N, R>) -> SmeltFuture<SmeltGeneratorResult<Y, R>> { if let Some(value) = self.completed.borrow().clone() { return SmeltFuture::from_future(Box::pin(async move { Ok(SmeltGeneratorResult::Complete(value)) })); } let future = (self.resume)(command); let completed = self.completed.clone(); SmeltFuture::from_future(Box::pin(async move { let result = future.await?; if let SmeltGeneratorResult::Complete(value) = &result { *completed.borrow_mut() = Some(value.clone()); } Ok(result) })) }");
        writer.line("}");
        // Genuine dynamic boundary: a generator crossing into source `unknown`
        // (e.g. a `function*` value stored in an erased callable slot, or a
        // generator object handed to an `unknown`-typed parameter) is a live
        // resumable state machine. No concrete type, generated union, or
        // scoped generic can represent it on the erased side — the receiver
        // only knows the JavaScript iterator protocol. The adapter therefore
        // reproduces exactly that protocol: an object with a callable `next`
        // that resumes the same shared state machine and returns erased
        // `{ value, done }` steps, plus a `__smelt_generator` marker for tag
        // checks. Yielded/returned values erase through `IntoSmeltUnknown` at
        // the step boundary; resume inputs are dropped because the erased
        // protocol cannot type them (matching a bare `next()` call).
        writer.line("impl<Y: IntoSmeltUnknown + 'static, R: IntoSmeltUnknown + Clone + 'static, N: Default + 'static> IntoSmeltUnknown for SmeltGenerator<Y, R, N> {");
        writer.line("    fn into_smelt_unknown(self) -> SmeltUnknown { let generator = self; let mut object: Vec<(String, SmeltUnknown)> = Vec::new(); object.push((\"__smelt_generator\".to_owned(), SmeltUnknown::Bool(true))); object.push((\"next\".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { let mut step: Vec<(String, SmeltUnknown)> = Vec::new(); match generator.resume(SmeltGeneratorCommand::Next(Default::default())) { SmeltGeneratorResult::Yielded(value) => { step.push((\"value\".to_owned(), value.into_smelt_unknown())); step.push((\"done\".to_owned(), SmeltUnknown::Bool(false))); } SmeltGeneratorResult::Complete(value) => { step.push((\"value\".to_owned(), value.into_smelt_unknown())); step.push((\"done\".to_owned(), SmeltUnknown::Bool(true))); } } Ok(SmeltUnknown::Object(SmeltObject::new(step))) })))); SmeltUnknown::Object(SmeltObject::new(object)) }");
        writer.line("}");
        // Async flavor of the same boundary: `next` returns an erased promise
        // that resolves to the `{ value, done }` step, mirroring the async
        // iterator protocol an erased consumer would drive.
        writer.line("impl<Y: IntoSmeltUnknown + Clone + 'static, R: IntoSmeltUnknown + Clone + 'static, N: Default + 'static> IntoSmeltUnknown for SmeltAsyncGenerator<Y, R, N> {");
        writer.line("    fn into_smelt_unknown(self) -> SmeltUnknown { let generator = self; let mut object: Vec<(String, SmeltUnknown)> = Vec::new(); object.push((\"__smelt_generator\".to_owned(), SmeltUnknown::Bool(true))); object.push((\"next\".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { let future = generator.resume(SmeltGeneratorCommand::Next(Default::default())); Ok(SmeltUnknown::Promise(SmeltPromise::from_future(Box::pin(async move { let mut step: Vec<(String, SmeltUnknown)> = Vec::new(); match future.await? { SmeltGeneratorResult::Yielded(value) => { step.push((\"value\".to_owned(), value.into_smelt_unknown())); step.push((\"done\".to_owned(), SmeltUnknown::Bool(false))); } SmeltGeneratorResult::Complete(value) => { step.push((\"value\".to_owned(), value.into_smelt_unknown())); step.push((\"done\".to_owned(), SmeltUnknown::Bool(true))); } } Ok(SmeltUnknown::Object(SmeltObject::new(step))) })))) })))); SmeltUnknown::Object(SmeltObject::new(object)) }");
        writer.line("}");
        writer.blank_line();
        }
        // `[Symbol.iterator]()` on an erased iterable may return a plain array,
        // a string, nothing, or a live iterator object obeying the JavaScript
        // iterator protocol (an erased generator or hand-written `{ next }`
        // iterator). Only the protocol itself is observable across the erased
        // boundary, so list extraction drains `next()` until `done`.
        writer.line("/// Collect an erased `[Symbol.iterator]()` result into its item values.");
        writer.line("fn smelt_unknown_iterator_items(source: SmeltUnknown) -> Vec<SmeltUnknown> { match source { SmeltUnknown::Null | SmeltUnknown::Undefined => Vec::new(), SmeltUnknown::Array(values) => values.into_vec(), SmeltUnknown::String(value) => value.chars().map(|ch| SmeltUnknown::String(ch.to_string().into())).collect::<Vec<_>>(), SmeltUnknown::Object(object) => { let Some(SmeltUnknown::Function(next)) = object.get(\"next\") else { panic!(\"unknown iterator did not return an iterable\") }; let mut items = Vec::new(); loop { let step = next(vec![]).unwrap_or(SmeltUnknown::Undefined); let SmeltUnknown::Object(step) = step else { break }; if matches!(step.get(\"done\"), Some(SmeltUnknown::Bool(true))) { break; } items.push(step.get(\"value\").unwrap_or(SmeltUnknown::Undefined)); } items } _ => panic!(\"unknown iterator did not return an iterable\") } }");
        writer.blank_line();
        writer.line("/// Return an erased JavaScript `Array.prototype.sort` method bound to an erased array.");
        writer.line("fn smelt_array_sort_method(values: SmeltArray) -> SmeltUnknown { SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let mut sorted = values.clone().into_vec(); if let Some(SmeltUnknown::Function(compare)) = args.get(0).cloned() { sorted.sort_by(|left, right| { let result = compare(vec![left.clone(), right.clone()]).unwrap_or(SmeltUnknown::Number(0.0)); let ordering = match result { SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(0.0), SmeltUnknown::Bool(value) => if value { 1.0 } else { 0.0 }, _ => 0.0 }; if ordering < 0.0 { ::std::cmp::Ordering::Less } else if ordering > 0.0 { ::std::cmp::Ordering::Greater } else { ::std::cmp::Ordering::Equal } }); } else { sorted.sort_by(|left, right| left.to_string().cmp(&right.to_string())); } Ok(SmeltUnknown::Array(sorted.into())) })) }");
        writer.blank_line();
        // `Function.prototype.apply`/`call` on an erased receiver. A
        // `SmeltUnknown::Function` receiver is not an object, so the plain
        // erased-object field read (`smelt_get_object_field`) finds nothing and
        // the value falls through to a null callback. This helper binds the
        // callable directly: `apply` drops the `this` argument and spreads the
        // trailing array, `call` drops `this` and forwards the remaining
        // positional arguments. Object receivers keep the ordinary field read so
        // user-defined `.apply`/`.call` properties still resolve — but a
        // *callable object* (a JS function carrying properties, erased here to an
        // object with a `__smelt_call` slot: a vitest mock, `partial.placeholder`,
        // a debounced function) has no own `apply`/`call`/`bind` property, and in
        // JavaScript those names resolve on `Function.prototype` of the
        // underlying callable. Fall back to the `__smelt_call` slot when the own
        // field read finds nothing, so `mock.apply(this, args)` actually invokes
        // and records the call instead of yielding `undefined`.
        writer.line("/// Bind `Function.prototype.apply`/`call` on an erased receiver, or read the field of an object receiver.");
        writer.line("fn smelt_function_method(receiver: SmeltUnknown, method: &str) -> SmeltUnknown { match receiver { SmeltUnknown::Function(function) => { let method = method.to_owned(); SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let receiver_method = smelt_is_receiver_method(&function); let forwarded: Vec<SmeltUnknown> = if method == \"apply\" { let mut forwarded = if receiver_method { ::std::vec![args.first().map_or(SmeltUnknown::Undefined, Clone::clone)] } else { Vec::new() }; if let Some(SmeltUnknown::Array(values)) = args.get(1) { forwarded.extend(values.clone().into_vec()); } forwarded } else if receiver_method { args.into_iter().collect() } else { args.into_iter().skip(1).collect() }; function(forwarded) })) } SmeltUnknown::Object(map) => match smelt_get_object_field(&map, method) { SmeltUnknown::Undefined => match map.get(\"__smelt_call\") { Some(callable @ SmeltUnknown::Function(_)) => smelt_function_method(callable, method), _ => SmeltUnknown::Undefined }, value => value }, _ => SmeltUnknown::Undefined } }");
        writer.blank_line();
        // JavaScript `Object.prototype.valueOf` / boxed-primitive unwrapping.
        //
        // `Object(1)` / `new Number(1)` erase to a marker object
        // `{ __smelt_number: true, value: 1 }`, so `.valueOf()` used to read a
        // missing own field, fall through to the null callback, and answer `null`.
        // Deep-equality code branches on exactly this: es-toolkit `isEqualWith`
        // compares a boxed and an unboxed primitive through
        // `Object.is(a.valueOf(), b.valueOf())` after their
        // `Object.prototype.toString` tags matched, and `cloneDeepWith` rebuilds a
        // wrapper with `new Number(valueToClone.valueOf())`.
        //
        // Unboxing is defined for every erased value, not just the wrappers: a
        // primitive is its own `valueOf`, a Date unwraps to its epoch
        // milliseconds, and any other object is itself (JS
        // `Object.prototype.valueOf` returns the receiver).
        writer.line("/// Unwrap a boxed primitive wrapper (or a Date) to the primitive it holds.");
        writer.line("fn smelt_unbox_primitive(value: SmeltUnknown) -> SmeltUnknown { match value { SmeltUnknown::Object(map) => { if map.contains_key(\"__smelt_number\") || map.contains_key(\"__smelt_boolean\") || map.contains_key(\"__smelt_string\") || map.contains_key(\"__smelt_symbol\") { return map.get(\"value\").unwrap_or(SmeltUnknown::Undefined); } if let Some(millis @ SmeltUnknown::Number(_)) = map.get(\"__smelt_date\") { return millis; } SmeltUnknown::Object(map) }, other => other } }");
        writer.line("/// Bind `Object.prototype.valueOf` on an erased receiver.");
        writer.line("fn smelt_value_of_method(receiver: SmeltUnknown) -> SmeltUnknown { if let SmeltUnknown::Object(map) = &receiver && let own @ SmeltUnknown::Function(_) = smelt_get_object_field(map, \"valueOf\") { return own; } let unboxed = smelt_unbox_primitive(receiver); SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(unboxed.clone()))) }");
        // `Object(value)` called as a FUNCTION (not `new`) boxes a primitive and
        // passes objects through unchanged. `Object(null)` / `Object(undefined)`
        // yield a fresh empty object. The wrapper shapes match what
        // `new Number(..)` / `new Boolean(..)` / `new String(..)` build, so the
        // `Object.prototype.toString` tag arms and `smelt_unbox_primitive` treat
        // both spellings identically.
        // Strings are deliberately NOT boxed, mirroring `new String(x)`, which
        // lowers to the plain string (see `string_constructor_expression`): a
        // string wrapper would have to re-expose the whole `String.prototype`
        // surface — `length`, character indexing, and every string method — on a
        // marker object, and Smelt models a JS string as a Rust `String`. Keeping
        // both spellings unboxed keeps the two consistent. The visible cost is
        // that `new String(x) === x` reads as `true` here and `false` in JS.
        writer.line("/// Box a primitive the way `Object(value)` does; objects and strings pass through.");
        writer.line("fn smelt_box_value(value: SmeltUnknown) -> SmeltUnknown { let (marker, boxed) = match value { SmeltUnknown::Number(_) => (\"__smelt_number\", value), SmeltUnknown::Bool(_) => (\"__smelt_boolean\", value), SmeltUnknown::Symbol(_) => (\"__smelt_symbol\", value), SmeltUnknown::Null | SmeltUnknown::Undefined => return SmeltUnknown::Object(SmeltObject::new(Vec::new())), other => return other }; let fields = Vec::from([(marker.to_owned(), SmeltUnknown::Bool(true)), (\"value\".to_owned(), boxed)]); SmeltUnknown::Object(SmeltObject::new(fields)) }");
        writer.blank_line();
        // `AbortController`/`AbortSignal` cancellation model. Both erase to
        // marker-bearing `SmeltObject`s whose shared `Rc<RefCell<..>>` storage
        // makes `controller.abort()` observable through any binding that read
        // `controller.signal`. These helpers resolve the signal record behind a
        // controller or signal, flip the shared `aborted` flag, and fire (then
        // clear) registered `'abort'` listeners.
        writer.line("/// Resolve the AbortSignal record behind an abort controller or signal object.");
        writer.line("fn smelt_abort_signal_object(object: &SmeltObject) -> Option<SmeltObject> { if object.contains_key(\"__smelt_abortsignal\") { return Some(object.clone()); } match object.get(\"signal\") { Some(SmeltUnknown::Object(signal)) if signal.contains_key(\"__smelt_abortsignal\") => Some(signal), _ => None } }");
        writer.line("/// Mark an AbortSignal aborted and fire (then clear) its registered `'abort'` listeners.");
        writer.line("fn smelt_abort_signal_fire(signal: &SmeltObject) { if matches!(signal.get(\"aborted\"), Some(SmeltUnknown::Bool(true))) { return; } signal.insert(\"aborted\".to_owned(), SmeltUnknown::Bool(true)); let listeners = match signal.get(\"__smelt_abort_listeners\") { Some(SmeltUnknown::Array(values)) => values.clone().into_vec(), _ => Vec::new() }; signal.insert(\"__smelt_abort_listeners\".to_owned(), SmeltUnknown::Array(Vec::new().into())); for listener in listeners { if let SmeltUnknown::Function(callback) = listener { let event = SmeltObject::new(Vec::from([(\"type\".to_owned(), SmeltUnknown::String(\"abort\".into()))])); let _ = callback(vec![SmeltUnknown::Object(event)]); } } }");
        writer.line("/// Return an erased AbortController/AbortSignal method bound to its shared record.");
        writer.line("fn smelt_abort_method(object: SmeltObject, method: &str) -> SmeltUnknown { let method = method.to_owned(); SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let signal = smelt_abort_signal_object(&object); match method.as_str() { \"abort\" | \"dispatchEvent\" => { if let Some(signal) = signal { smelt_abort_signal_fire(&signal); } Ok(if method == \"dispatchEvent\" { SmeltUnknown::Bool(true) } else { SmeltUnknown::Undefined }) } \"addEventListener\" => { if let Some(signal) = signal { let event_type = match args.first() { Some(SmeltUnknown::String(value)) => value.to_string(), _ => String::new() }; if event_type == \"abort\" { if let Some(listener @ SmeltUnknown::Function(_)) = args.get(1).cloned() { let mut listeners = match signal.get(\"__smelt_abort_listeners\") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }; listeners.push(listener); signal.insert(\"__smelt_abort_listeners\".to_owned(), SmeltUnknown::Array(listeners.into())); } } } Ok(SmeltUnknown::Undefined) } \"removeEventListener\" => { if let Some(signal) = signal { if let Some(target @ SmeltUnknown::Function(_)) = args.get(1).cloned() { let listeners: Vec<SmeltUnknown> = match signal.get(\"__smelt_abort_listeners\") { Some(SmeltUnknown::Array(values)) => values.into_vec(), _ => Vec::new() }.into_iter().filter(|listener| !listener.js_strict_eq(&target)).collect(); signal.insert(\"__smelt_abort_listeners\".to_owned(), SmeltUnknown::Array(listeners.into())); } } Ok(SmeltUnknown::Undefined) } _ => Ok(SmeltUnknown::Undefined) } })) }");
        writer.blank_line();
        writer.block("pub enum SmeltUnknown", |unknown_writer| {
            unknown_writer.line("Null,");
            unknown_writer.line("Undefined,");
            unknown_writer.line("Bool(bool),");
            unknown_writer.line("Number(f64),");
            // JavaScript strings are immutable, so the erased tag shares one
            // heap buffer across clones instead of copying it: `Rc<str>` turns
            // `SmeltUnknown::clone` -- and therefore every `SmeltObject::get`
            // property read -- into a refcount bump rather than malloc+memcpy.
            unknown_writer.line("String(::std::rc::Rc<str>),");
            unknown_writer.line("Symbol(::std::rc::Rc<str>),");
            unknown_writer.line("Array(SmeltArray),");
            unknown_writer.line("Object(SmeltObject),");
            unknown_writer.line("Function(::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>),");
            unknown_writer.line("Promise(SmeltPromise),");
        });
        writer.blank_line();
        writer.block("impl Clone for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn clone(&self) -> Self", |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::Null => Self::Null,");
                    match_writer.line("Self::Undefined => Self::Undefined,");
                    match_writer.line("Self::Bool(value) => Self::Bool(*value),");
                    match_writer.line("Self::Number(value) => Self::Number(*value),");
                    match_writer.line("Self::String(value) => Self::String(value.clone()),");
                    match_writer.line("Self::Symbol(value) => Self::Symbol(value.clone()),");
                    match_writer.line("Self::Array(values) => Self::Array(values.clone()),");
                    match_writer.line("Self::Object(values) => Self::Object(values.clone()),");
                    match_writer.line("Self::Function(value) => Self::Function(value.clone()),");
                    match_writer.line("Self::Promise(value) => Self::Promise(value.clone()),");
                });
            });
        });
        if needs_erased_function {
            writer.blank_line();
            writer.line("#[derive(Clone)]");
            writer.line("pub struct SmeltErasedFunction {");
            writer.line("    callback: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> SmeltUnknown>,");
            writer.line("    length: f64,");
            writer.line("    object: Option<SmeltObject>,");
            writer.line("}");
            writer.blank_line();
            writer.line("impl SmeltErasedFunction {");
            writer.line("    /// Invoke an erased JavaScript callable through a reentrant handle.");
            writer.line("    fn call(&self, args: impl Into<Vec<SmeltUnknown>>) -> SmeltUnknown {");
            writer.line("        (self.callback)(args.into())");
            writer.line("    }");
            // A callable object narrowed to a callable interface that declares
            // fewer members keeps its dropped members as own properties of the
            // underlying function value, exactly as JavaScript does (es-toolkit's
            // `curry` assigns `wrapper.placeholder`, and its `flow` spec reads
            // that property back through a `CurriedFunction1`). They are parked
            // in a side registry keyed on the callback allocation rather than in
            // the `object` bag: `object` is what makes an erased callable erase
            // as an OBJECT carrying `__smelt_call`, and the value being narrowed
            // here is still a plain function everywhere else, so filling `object`
            // would double-wrap it at the next erasure.
            writer.line("    /// Record a callable object's own JavaScript properties.");
            writer.line("    fn smelt_with_properties(self, entries: Vec<(String, SmeltUnknown)>) -> Self {");
            writer.line("        let key = ::std::rc::Rc::as_ptr(&self.callback) as *const () as usize;");
            writer.line("        SMELT_CALLABLE_PROPERTIES.with(|registry| {");
            writer.line("            let mut registry = registry.borrow_mut();");
            writer.line("            let object = registry.entry(key).or_insert_with(|| SmeltObject::new(Vec::new()));");
            writer.line("            for (name, value) in entries { object.insert(name, value); }");
            writer.line("        });");
            writer.line("        self");
            writer.line("    }");
            // Binding a receiver keeps the callable TYPED: a bound method is
            // still a function everywhere else in the program, so degrading it
            // to `SmeltUnknown` at the bind would erase a shape the source
            // states. `object` rides along so a bound callable object keeps its
            // own properties.
            if needs_this_channel {
            writer.line("    /// Bind a receiver to this callable, as `Function.prototype.bind` does.");
            writer.line("    fn smelt_bind_this(&self, receiver: SmeltUnknown) -> SmeltErasedFunction {");
            writer.line("        let callback = self.callback.clone();");
            writer.line("        SmeltErasedFunction {");
            writer.line("            callback: ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| {");
            writer.line("                let _smelt_this_guard = smelt_push_this(receiver.clone());");
            writer.line("                (callback)(args)");
            writer.line("            }),");
            writer.line("            length: self.length,");
            writer.line("            object: self.object.clone(),");
            writer.line("        }");
            writer.line("    }");
            }
            writer.line("    /// Read one own property of a callable object, `undefined` when absent.");
            writer.line("    fn smelt_property(&self, name: &str) -> SmeltUnknown {");
            writer.line("        if let Some(value) = self.object.as_ref().and_then(|object| object.get(name)) { return value; }");
            writer.line("        let key = ::std::rc::Rc::as_ptr(&self.callback) as *const () as usize;");
            writer.line("        SMELT_CALLABLE_PROPERTIES.with(|registry| registry.borrow().get(&key).and_then(|object| object.get(name))).unwrap_or(SmeltUnknown::Undefined)");
            writer.line("    }");
            writer.line(
                "    /// Restore an erased callable value without dropping callable-object fields.",
            );
            writer.line("    fn into_smelt_unknown(self) -> SmeltUnknown {");
            writer.line(
                "        // Erasing the SAME callback twice must yield `SmeltUnknown::Function`",
            );
            writer.line("        // values that share one OUTER `Rc`, because reference identity");
            writer.line(
                "        // (`Rc::ptr_eq` in `same_js_key`/`smelt_unknown_structural_eq`) compares",
            );
            writer.line(
                "        // that outer `Rc`. A nullary function-item constant (`doNothing()`)",
            );
            writer.line(
                "        // routes through one cached `SmeltErasedFunction`, so two calls share",
            );
            writer.line(
                "        // the inner callback `Rc`; key a `Weak` outer cache on its address so",
            );
            writer.line(
                "        // both erasures resolve to one `SmeltUnknown::Function` while both are",
            );
            writer.line(
                "        // alive (e.g. inside one `toStrictEqual`). A `Weak` avoids pinning the",
            );
            writer.line(
                "        // callback alive; a successful upgrade proves the address is still the",
            );
            writer.line("        // same callback, so it is a true hit. Callable objects");
            writer.line("        // (`object: Some(_)`) are per-instance and skip the cache.");
            writer.line("        if self.object.is_none() {");
            writer.line("            let key = ::std::rc::Rc::as_ptr(&self.callback) as *const () as usize;");
            writer.line("            if let Some(callable) = SMELT_ERASED_FUNCTION_VALUES.with(|cache| cache.borrow().get(&key).and_then(::std::rc::Weak::upgrade)) {");
            writer.line("                return SmeltUnknown::Function(callable);");
            writer.line("            }");
            writer.line("            let callback = self.callback.clone();");
            writer.line("            let callable: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>((callback)(args)));");
            writer.line("            SMELT_ERASED_FUNCTION_VALUES.with(|cache| { cache.borrow_mut().insert(key, ::std::rc::Rc::downgrade(&callable)); });");
            // The `length` field dies with the struct, so hand it to the registry
            // before returning: this is the only point that still knows both the
            // arity and the erased allocation it belongs to.
            writer.line(format!(
                "            {register}(&callable, self.length);",
                register = smelt_stdlib::runtime_symbols::function_length::REGISTER,
            ));
            writer.line("            return SmeltUnknown::Function(callable);");
            writer.line("        }");
            writer.line("        let callback = self.callback.clone();");
            writer.line("        let length = self.length;");
            writer.line("        let callable = ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>((callback)(args)));");
            writer.line(format!(
                "        {register}(&callable, length);",
                register = smelt_stdlib::runtime_symbols::function_length::REGISTER,
            ));
            writer.line("        let callable = SmeltUnknown::Function(callable);");
            writer.line("        if let Some(object) = self.object { object.insert(\"__smelt_call\".to_owned(), callable); SmeltUnknown::Object(object) } else { callable }");
            writer.line("    }");
            writer.line("}");
            writer.blank_line();
            writer.line("thread_local! {");
            writer.line(
                "    /// Cache the OUTER `SmeltUnknown::Function` `Rc` derived from each erased",
            );
            writer
                .line("    /// callback, keyed on the inner callback `Rc` address, as a `Weak` so");
            writer.line(
                "    /// repeated erasures of one shared `SmeltErasedFunction` keep reference",
            );
            writer.line("    /// identity while alive without pinning transient callbacks.");
            writer.line("    static SMELT_ERASED_FUNCTION_VALUES: ::std::cell::RefCell<::std::collections::HashMap<usize, ::std::rc::Weak<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
            writer.line(
                "    /// Own JavaScript properties of a callable object, keyed on its callback",
            );
            writer.line(
                "    /// allocation, so a narrowing conversion to a callable interface that does",
            );
            writer.line("    /// not declare them can still answer a later property read.");
            writer.line("    static SMELT_CALLABLE_PROPERTIES: ::std::cell::RefCell<::std::collections::HashMap<usize, SmeltObject>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
            writer.line("}");
        }
        if needs_this_channel {
        writer.blank_line();
        // JavaScript `this` is supplied by the CALL, not by the definition site:
        // the same plain function sees a different receiver depending on whether
        // it was reached as `object.method()`, `fn.call(thisArg, ..)`, or a bare
        // `fn()`. That is a genuinely dynamic binding, so it is modeled as a
        // dynamically scoped channel rather than as a value threaded through the
        // erased call ABI -- threading it would change the signature of every
        // erased callable in the program for a feature only a handful of them
        // read. `smelt_bind_this` installs a receiver for exactly one call and
        // the guard restores the previous binding on scope exit (including on
        // unwind), so the channel is always balanced.
        writer.line("thread_local! {");
        writer.line("    /// Receiver installed by the innermost active call, `undefined` when none.");
        writer.line("    static SMELT_THIS: ::std::cell::RefCell<SmeltUnknown> = ::std::cell::RefCell::new(SmeltUnknown::Undefined);");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Restores the previously installed `this` binding when dropped.");
        writer.line("struct SmeltThisGuard { previous: SmeltUnknown }");
        writer.blank_line();
        writer.line("impl Drop for SmeltThisGuard {");
        writer.line("    fn drop(&mut self) {");
        writer.line("        let previous = ::std::mem::replace(&mut self.previous, SmeltUnknown::Undefined);");
        writer.line("        SMELT_THIS.with(|slot| { *slot.borrow_mut() = previous; });");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Install `receiver` as `this` until the returned guard is dropped.");
        writer.line("fn smelt_push_this(receiver: SmeltUnknown) -> SmeltThisGuard {");
        writer.line("    SmeltThisGuard { previous: SMELT_THIS.with(|slot| slot.replace(receiver)) }");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Read the `this` receiver installed by the innermost active call.");
        writer.line("fn smelt_this() -> SmeltUnknown {");
        writer.line("    SMELT_THIS.with(|slot| slot.borrow().clone())");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Bind a receiver to an erased callable, as `Function.prototype.bind` does.");
        writer.line("fn smelt_bind_this(callee: SmeltUnknown, receiver: SmeltUnknown) -> SmeltUnknown {");
        writer.line("    match callee {");
        writer.line("        SmeltUnknown::Function(function) => SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| {");
        writer.line("            let _smelt_this_guard = smelt_push_this(receiver.clone());");
        writer.line("            function(args)");
        writer.line("        })),");
        // A callable OBJECT (`__smelt_call`) keeps its property bag: rebuilding
        // the object with a bound `__smelt_call` preserves every other member,
        // which is what `throttle`/`debounce` return and then invoke as a method.
        writer.line("        SmeltUnknown::Object(object) => match object.get(\"__smelt_call\") {");
        writer.line("            Some(callable @ SmeltUnknown::Function(_)) => {");
        writer.line("                let bound = object.clone();");
        writer.line("                bound.insert(\"__smelt_call\".to_owned(), smelt_bind_this(callable, receiver));");
        writer.line("                SmeltUnknown::Object(bound)");
        writer.line("            }");
        writer.line("            _ => SmeltUnknown::Object(object),");
        writer.line("        },");
        writer.line("        other => other,");
        writer.line("    }");
        writer.line("}");
        }
        if needs_vitest_mock {
            writer.blank_line();
            // Stateful Vitest `vi.fn()` mock runtime. A mock is a genuine dynamic
            // boundary: the source spells no parameter or return shape at all, and
            // its behavior is reconfigured imperatively at runtime through the
            // chainable `mock*` methods, so the callable and its recorded calls
            // live behind `SmeltUnknown` by design (see `SmeltUnknown boundaries`
            // in CLAUDE.md). The mock erases to a callable object (`__smelt_call`)
            // that flows through every existing erased-call path; its shared state
            // is reachable from the erased object through the `__smelt_vitest_mock`
            // marker field (a registry id), so matchers can read call counts and
            // recorded arguments without a parallel value channel.
            writer.line("/// One configured outcome served by a Vitest mock invocation.");
            writer.line("#[derive(Clone)]");
            writer.line("enum SmeltVitestMockOutcome {");
            writer.line("    /// `mockReturnValue(Once)`: return the value directly.");
            writer.line("    Return(SmeltUnknown),");
            writer.line("    /// `mockResolvedValue(Once)`: return a resolved promise of the value.");
            writer.line("    Resolve(SmeltUnknown),");
            writer.line("    /// `mockRejectedValue(Once)`: return a rejected promise of the value.");
            writer.line("    Reject(SmeltUnknown),");
            writer.line("    /// `vi.fn(impl)` / `mockImplementation(Once)`: delegate to the callback.");
            writer.line("    Implementation(::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>),");
            writer.line("}");
            writer.blank_line();
            writer.line("/// Shared mutable state behind one `vi.fn()` mock instance.");
            writer.line("struct SmeltVitestMockState {");
            writer.line("    /// FIFO of one-shot outcomes (`mock*Once`), consumed before `default`.");
            writer.line("    once: ::std::collections::VecDeque<SmeltVitestMockOutcome>,");
            writer.line("    /// Sticky outcome served when `once` is empty; `None` yields `undefined`.");
            writer.line("    default: Option<SmeltVitestMockOutcome>,");
            writer.line("    /// Recorded argument vectors, one entry per invocation.");
            writer.line("    calls: Vec<Vec<SmeltUnknown>>,");
            writer.line("    /// Recorded return value, one entry per invocation. An async mock");
            writer.line("    /// records the returned `SmeltUnknown::Promise`; the result matchers");
            writer.line("    /// flatten it to its settled value at assertion time.");
            writer.line("    results: Vec<SmeltUnknown>,");
            writer.line("}");
            writer.blank_line();
            writer.line("thread_local! {");
            writer.line("    /// Registry mapping the `__smelt_vitest_mock` marker id stored on each");
            writer.line("    /// erased mock object to its shared state handle.");
            writer.line("    static SMELT_VITEST_MOCKS: ::std::cell::RefCell<::std::collections::HashMap<usize, ::std::rc::Rc<::std::cell::RefCell<SmeltVitestMockState>>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
            writer.line("}");
            writer.blank_line();
            writer.line("/// Resolve an erased value to a plain callable (function or callable object).");
            writer.line("fn smelt_vitest_mock_callable(value: &SmeltUnknown) -> Option<::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>>> { match value { SmeltUnknown::Function(function) => Some(function.clone()), SmeltUnknown::Object(object) => match object.get(\"__smelt_call\") { Some(SmeltUnknown::Function(function)) => Some(function), _ => None }, _ => None } }");
            writer.blank_line();
            writer.line("/// Build a stateful Vitest `vi.fn([impl])` mock as an erased callable object.");
            writer.line("///");
            writer.line("/// Invoking the object's `__smelt_call` records the argument vector, then");
            writer.line("/// serves the next one-shot outcome (FIFO) or the sticky default; with no");
            writer.line("/// configuration it returns `undefined` (or delegates to `impl` when given).");
            writer.line("/// The chainable `mock*` configuration methods are real fields on the object,");
            writer.line("/// so they flow through the ordinary erased method-call path and each returns");
            writer.line("/// the same mock instance for chaining, matching Vitest.");
            writer.line("fn smelt_vitest_mock_new(implementation: Option<SmeltUnknown>) -> SmeltUnknown {");
            writer.line("    let id = smelt_next_object_id();");
            writer.line("    let state = ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltVitestMockState { once: ::std::collections::VecDeque::new(), default: implementation.as_ref().and_then(smelt_vitest_mock_callable).map(SmeltVitestMockOutcome::Implementation), calls: Vec::new(), results: Vec::new() }));");
            writer.line("    SMELT_VITEST_MOCKS.with(|mocks| { mocks.borrow_mut().insert(id, state.clone()); });");
            writer.line("    let object = SmeltObject::new(Vec::new());");
            writer.line("    object.insert(\"__smelt_vitest_mock\".to_owned(), SmeltUnknown::Number(id as f64));");
            writer.line("    let call_state = state.clone();");
            writer.line("    object.insert(\"__smelt_call\".to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let outcome = { let mut state = call_state.borrow_mut(); state.calls.push(args.clone()); state.once.pop_front().or_else(|| state.default.clone()) }; let result = match outcome { None => Ok(SmeltUnknown::Undefined), Some(SmeltVitestMockOutcome::Return(value)) => Ok(value), Some(SmeltVitestMockOutcome::Resolve(value)) => Ok(SmeltUnknown::Promise(SmeltPromise::resolved(value))), Some(SmeltVitestMockOutcome::Reject(value)) => Ok(SmeltUnknown::Promise(SmeltPromise::rejected(value))), Some(SmeltVitestMockOutcome::Implementation(callback)) => (callback)(args) }; if let Ok(value) = &result { call_state.borrow_mut().results.push(value.clone()); } result })));");
            writer.line("    for (method, once) in [(\"mockReturnValue\", false), (\"mockReturnValueOnce\", true), (\"mockResolvedValue\", false), (\"mockResolvedValueOnce\", true), (\"mockRejectedValue\", false), (\"mockRejectedValueOnce\", true), (\"mockImplementation\", false), (\"mockImplementationOnce\", true)] {");
            writer.line("        let method_state = state.clone();");
            writer.line("        let method_object = object.clone();");
            writer.line("        object.insert(method.to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let value = args.into_iter().next().unwrap_or(SmeltUnknown::Undefined); let outcome = match method { \"mockReturnValue\" | \"mockReturnValueOnce\" => Some(SmeltVitestMockOutcome::Return(value)), \"mockResolvedValue\" | \"mockResolvedValueOnce\" => Some(SmeltVitestMockOutcome::Resolve(value)), \"mockRejectedValue\" | \"mockRejectedValueOnce\" => Some(SmeltVitestMockOutcome::Reject(value)), _ => smelt_vitest_mock_callable(&value).map(SmeltVitestMockOutcome::Implementation) }; if let Some(outcome) = outcome { let mut state = method_state.borrow_mut(); if once { state.once.push_back(outcome); } else { state.default = Some(outcome); } } Ok(SmeltUnknown::Object(method_object.clone())) })));");
            writer.line("    }");
            writer.line("    for method in [\"mockClear\", \"mockReset\", \"mockRestore\"] {");
            writer.line("        let method_state = state.clone();");
            writer.line("        let method_object = object.clone();");
            writer.line("        let reset = method != \"mockClear\";");
            writer.line("        object.insert(method.to_owned(), SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { { let mut state = method_state.borrow_mut(); state.calls.clear(); state.results.clear(); if reset { state.once.clear(); state.default = None; } } Ok(SmeltUnknown::Object(method_object.clone())) })));");
            writer.line("    }");
            writer.line("    SmeltUnknown::Object(object)");
            writer.line("}");
            writer.blank_line();
            writer.line("/// Resolve the shared mock state behind an erased value, if it is a mock.");
            writer.line("fn smelt_vitest_mock_state(value: &SmeltUnknown) -> Option<::std::rc::Rc<::std::cell::RefCell<SmeltVitestMockState>>> { let SmeltUnknown::Object(object) = value else { return None }; match object.get(\"__smelt_vitest_mock\") { Some(SmeltUnknown::Number(id)) => SMELT_VITEST_MOCKS.with(|mocks| mocks.borrow().get(&(id as usize)).cloned()), _ => None } }");
            writer.blank_line();
            writer.line("/// `expect(mock).toHaveBeenCalledTimes(expected)`: true when the assertion");
            writer.line("/// holds. Non-mock actuals pass vacuously — the pre-mock matcher was fully");
            writer.line("/// vacuous, and unmocked spy handles (`vi.spyOn`) still lower to plain");
            writer.line("/// placeholders, so failing them here would regress unrelated suites.");
            writer.line("fn smelt_vitest_mock_called_times(value: &SmeltUnknown, expected: f64) -> bool { match smelt_vitest_mock_state(value) { Some(state) => state.borrow().calls.len() as f64 == expected, None => true } }");
            writer.blank_line();
            writer.line("/// `expect(mock).toHaveBeenCalledWith(...)`: true when any recorded call's");
            writer.line("/// arguments deep-equal the expected arguments (same `toEqual` structural");
            writer.line("/// equality). When `last` is set, only the most recent recorded call is");
            writer.line("/// compared (`toHaveBeenLastCalledWith`). Non-mock actuals pass vacuously,");
            writer.line("/// mirroring `toHaveBeenCalledTimes`.");
            writer.line("/// Drop trailing `undefined`/`null` arguments. Generated Rust closures are");
            writer.line("/// fixed-arity, so a source `callback()` that omits a declared parameter is");
            writer.line("/// emitted as `callback(undefined)` and the mock records a trailing nullish");
            writer.line("/// slot, whereas JavaScript records only `arguments.length`. Normalizing both");
            writer.line("/// the recorded and expected argument vectors this way reconciles the two so");
            writer.line("/// `toHaveBeenLastCalledWith()` matches an omitted-argument call.");
            writer.line("fn smelt_vitest_mock_trim_trailing_nullish(args: &[SmeltUnknown]) -> &[SmeltUnknown] { let mut end = args.len(); while end > 0 && matches!(args[end - 1], SmeltUnknown::Undefined | SmeltUnknown::Null) { end -= 1; } &args[..end] }");
            writer.line("fn smelt_vitest_mock_called_with(value: &SmeltUnknown, expected: Vec<SmeltUnknown>, last: bool) -> bool { let expected = smelt_vitest_mock_trim_trailing_nullish(&expected); let call_matches = |call: &Vec<SmeltUnknown>| { let call = smelt_vitest_mock_trim_trailing_nullish(call); call.len() == expected.len() && call.iter().zip(expected.iter()).all(|(left, right)| smelt_unknown_structural_eq(left, right, &mut ::std::collections::HashSet::new())) }; match smelt_vitest_mock_state(value) { Some(state) => { let state = state.borrow(); if last { state.calls.last().is_some_and(call_matches) } else { state.calls.iter().any(call_matches) } }, None => true } }");
            writer.blank_line();
            writer.line("/// `expect(mock).toHaveLastResolvedWith(...)`: true when the mock's most");
            writer.line("/// recent recorded result deep-equals the expected value. An async mock");
            writer.line("/// records a `SmeltUnknown::Promise`, so a promise result is flattened to");
            writer.line("/// its settled `Ok` value before comparison (the caller has already awaited");
            writer.line("/// it, so the shared state cell is populated). Non-mock actuals pass");
            writer.line("/// vacuously, mirroring the other mock matchers.");
            writer.line("fn smelt_vitest_mock_last_resolved_with(value: &SmeltUnknown, expected: SmeltUnknown) -> bool { match smelt_vitest_mock_state(value) { Some(state) => { let last = state.borrow().results.last().cloned(); match last { Some(result) => { let resolved = match &result { SmeltUnknown::Promise(promise) => match &*promise.state.borrow() { Some(Ok(value)) => value.clone(), _ => return false }, other => other.clone() }; smelt_unknown_structural_eq(&resolved, &expected, &mut ::std::collections::HashSet::new()) }, None => false } }, None => true } }");
        }
        writer.blank_line();
        {
            // Unconditional: the shared host constructor
            // (`smelt_reflected_construct`, always emitted) builds `Blob`/`File`
            // through this helper, so it cannot be gated on the crate spelling a
            // `new Blob(...)` itself.
            writer.line("/// Build the modeled host `Blob`/`File` record for `new Blob(...)` / `new File(...)`.");
            writer.line("///");
            writer.line("/// Concatenates BlobPart contents (strings verbatim; nested Blob/File records");
            writer.line("/// contribute their stored `content`; other parts stringify like JavaScript)");
            writer.line("/// and stores the UTF-8 byte length as `size`. Passing a file name stamps the");
            writer.line("/// `__smelt_file` marker on top of `__smelt_blob`, so `file instanceof Blob`");
            writer.line("/// observes the host subtype relationship; `lastModified` defaults to `0.0`");
            writer.line("/// for determinism instead of the wall clock.");
            writer.line(format!(
                "fn {}(parts: SmeltUnknown, blob_type: String, file_name: Option<String>, last_modified: Option<f64>) -> SmeltUnknown {{",
                smelt_stdlib::runtime_symbols::host::BLOB_RECORD_FROM_PARTS,
            ));
            writer.line("    let mut content = String::new();");
            writer.line("    if let SmeltUnknown::Array(items) = parts {");
            writer.line("        for item in items.iter() {");
            writer.line("            match item {");
            writer.line("                SmeltUnknown::String(text) => content.push_str(&text),");
            writer.line("                SmeltUnknown::Object(map) if map.contains_key(\"__smelt_blob\") => {");
            writer.line("                    if let Some(SmeltUnknown::String(text)) = map.get(\"content\") { content.push_str(&text); }");
            writer.line("                }");
            writer.line("                other => content.push_str(&other.to_string()),");
            writer.line("            }");
            writer.line("        }");
            writer.line("    }");
            writer.line("    let record = Vec::from([");
            writer.line("        (\"__smelt_blob\".to_owned(), SmeltUnknown::Bool(true)),");
            writer.line("        (\"type\".to_owned(), SmeltUnknown::String(blob_type.into())),");
            writer.line("        (\"size\".to_owned(), SmeltUnknown::Number(content.len() as f64)),");
            writer.line("        (\"content\".to_owned(), SmeltUnknown::String(content.into())),");
            writer.line("    ]);");
            writer.line("    let record = SmeltObject::new(record);");
            writer.line("    if let Some(name) = file_name {");
            writer.line("        record.insert(\"__smelt_file\".to_owned(), SmeltUnknown::Bool(true));");
            writer.line("        record.insert(\"name\".to_owned(), SmeltUnknown::String(name.into()));");
            writer.line("        record.insert(\"lastModified\".to_owned(), SmeltUnknown::Number(last_modified.unwrap_or(0.0)));");
            writer.line("    }");
            writer.line("    SmeltUnknown::Object(record)");
            writer.line("}");
            writer.blank_line();
        }
        // Erased dynamic indexed assignment `target[key] = value`. Mirrors JS:
        // an object gets a string property; an array with a numeric index sets
        // (and extends) that element; any other value (or an array with a
        // non-index key) becomes a fresh object holding the single property.
        // Centralizing this keeps each call site a single call instead of an
        // inline `match` that repeats `SmeltUnknown` at every assignment.
        writer.line("fn smelt_index_assign(target: &mut SmeltUnknown, key: String, value: SmeltUnknown) {");
        writer.line("    match target {");
        // A byte-backed host object's indexed slots are its bytes, not ordinary
        // record properties: `view[i] = byte` has to land in `bytes` so a later
        // `view[i]` read and a `slice()` observe it. Writes to any other key, or to
        // a record without byte storage, fall through to the property insert.
        writer.line(format!(
            "        SmeltUnknown::Object(map) if {set_element}(map, &key, value.clone()) => {{}}",
            set_element = smelt_stdlib::runtime_symbols::byte_buffer::SET_ELEMENT,
        ));
        writer.line("        SmeltUnknown::Object(map) => { map.insert(key, value); }");
        // A JS array keeps being an array when a non-index key is written to it:
        // `a[0] = x` sets an element, `a.x = v` sets a named property in the
        // array's side table (`SmeltArray::props`). Replacing the array with a
        // one-property object — what this used to do — lost every element and made
        // `Array.isArray(a)` answer `false`.
        writer.line("        SmeltUnknown::Array(array) => {");
        writer.line("            if let Ok(index) = key.parse::<usize>() { array.set_index(index, value); }");
        writer.line("            else { array.set_named_property(key, value); }");
        writer.line("        }");
        writer.line("        other => { *other = SmeltUnknown::Object(SmeltObject::new(Vec::from([(key, value)]))); }");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        // The property key a SYMBOL value indexes.
        //
        // A well-known symbol (`Symbol.iterator`, `Symbol.toStringTag`, ...) is a
        // constant of the language, so `obj[Symbol.iterator]` and a declared
        // `[Symbol.iterator]` member must name ONE member. The frontend folds the
        // static key spelling through `smelt_stdlib::well_known_symbols`; this is
        // the runtime half of that same table, for the spelling that only exists
        // as a value at compile time (a `const s = Symbol.iterator` alias handed
        // through an erased slot, a symbol read out of `Object.getOwnPropertySymbols`).
        // Every other symbol — unique `Symbol('d')`, registry `Symbol.for('d')` —
        // keeps the generic `__smelt_symbol:<description>` storage form, which is
        // what makes it a key distinct from its own description string.
        writer.line("/// The property key a symbol value indexes.");
        {
            let well_known_arms = smelt_stdlib::well_known_symbols::spelling_key_pairs()
                .into_iter()
                .fold(String::new(), |mut arms, (spelling, key)| {
                    use ::std::fmt::Write as _;
                    let _ = write!(arms, "{spelling:?} => {key:?}.to_owned(), ");
                    arms
                });
            writer.line(format!(
                "fn smelt_symbol_property_key(description: &str) -> String {{ match description {{ {well_known_arms}other => format!(\"__smelt_symbol:{{other}}\") }} }}"
            ));
        }
        writer.blank_line();
        // JavaScript property-key coercion: `obj[key]` stringifies whatever `key`
        // is. Lives in the prelude because it was previously emitted as a full
        // nested `fn` definition at EVERY dynamic property-key site. es-toolkit's
        // `compat/object/mergeWith` key loop alone repeated it enough to make one
        // generated file 2.6 MB of the crate's 14 MB.
        //
        // A symbol key keeps its `__smelt_symbol:` prefix, which is the storage
        // form symbol-keyed properties use, so a symbol round-trips as a distinct
        // key rather than colliding with its own description string.
        writer.line("fn smelt_property_key(value: SmeltUnknown) -> String { match value { SmeltUnknown::String(value) => value.to_string(), SmeltUnknown::Symbol(value) => smelt_symbol_property_key(&value), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Array(values) => values.into_vec().into_iter().map(smelt_property_key).collect::<Vec<_>>().join(\",\"), SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() } }");
        writer.blank_line();
        // JavaScript `Array.prototype.concat` normalizes each argument with
        // `IsConcatSpreadable`: an array contributes its elements, and any other
        // value contributes itself as one element. When the argument's static
        // type is erased the frontend cannot pick a side, so it emits
        // `Rvalue::ConcatSpread` and the decision lands here, at runtime.
        if needs_concat_spread {
            writer.line("fn smelt_concat_spread(value: SmeltUnknown) -> Vec<SmeltUnknown> { match value { SmeltUnknown::Array(values) => values.into_vec(), other => ::std::vec![other] } }");
            writer.blank_line();
        }
        // One key test shared by every erased-Map accessor, so a read and a write
        // cannot disagree about which entry a key names. A `{ __smelt_map: [..] }`
        // entry is a two-element array, and Map key identity is SameValueZero.
        writer.line("/// Whether an erased Map entry (a `[key, value]` pair) is keyed by `key`.");
        writer.line("fn smelt_map_entry_key_is(entry: &SmeltUnknown, key: &SmeltUnknown) -> bool {");
        writer.line("    let SmeltUnknown::Array(pair) = entry else { return false };");
        writer.line("    pair.get(0).is_some_and(|entry_key| entry_key.same_js_key(key))");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_get_object_field(map: &SmeltObject, field: &str) -> SmeltUnknown {");
        if needs_vitest_mock {
            // Vitest exposes a `.mock` accessor on every mock function carrying
            // its recorded activity (`mockFn.mock.calls`, `mockFn.mock.results`).
            // The erased mock object only stores the `__smelt_vitest_mock` marker
            // and its configuration methods, so synthesize the `.mock` object
            // from the live registry state on read. `calls` is an array of the
            // recorded argument arrays; `results` mirrors the recorded return
            // values, so `mockFn.mock.calls.length` flows through the ordinary
            // array-length path.
            writer.line("    if field == \"mock\" && let Some(state) = smelt_vitest_mock_state(&SmeltUnknown::Object(map.clone())) { let state = state.borrow(); let calls = state.calls.iter().map(|call| SmeltUnknown::Array(call.clone().into())).collect::<Vec<_>>(); let results = state.results.clone(); let mock = SmeltObject::new(Vec::new()); mock.insert(\"calls\".to_owned(), SmeltUnknown::Array(calls.into())); mock.insert(\"results\".to_owned(), SmeltUnknown::Array(results.into())); return SmeltUnknown::Object(mock); }");
        }
        // An erased `Map` is a marker object `{ __smelt_map: [[k, v], ...] }`.
        // Real Maps expose `.size` through `Map.prototype`, which the marker
        // object does not store as an own field, so synthesize it from the entry
        // count when a `.size` read reaches an erased Map. This keeps generic
        // `unknown`-typed code that probes `value.size` (e.g. `isEmptyish`)
        // correct without materializing the typed `SmeltJsMap`.
        // Error markers do not store `name` as an own field (the base
        // `new Error(msg)` shape is just `{ __smelt_error, message }`). Real errors
        // inherit `name` from their prototype, so synthesize it when absent — from
        // the class name the marker records, which is what makes
        // `new TypeError('t').name` read `"TypeError"` rather than `"Error"`.
        writer.line("    if field == \"name\" && !map.contains_key(\"name\") && let Some(SmeltUnknown::String(class_name)) = map.get(\"__smelt_error\") { return SmeltUnknown::String(class_name); }");
        // A class instance answers `.constructor` with its interned per-class value.
        // An own `constructor` field still wins (source code can assign one), and a
        // plain object keeps `undefined` — which is what lets two plain objects pass
        // the `isPlainObject(a) && isPlainObject(b)` arm instead.
        writer.line("    if field == \"constructor\" && !map.contains_key(\"constructor\") && let Some(SmeltUnknown::String(class_name)) = map.get(\"__smelt_class\") { return smelt_class_constructor(class_name.to_string()); }");
        // Byte-backed host objects (`ArrayBuffer`, `Buffer`, `DataView`, ...) hold
        // their storage in a `bytes` list, so an indexed read (`buffer[1]`, and the
        // `a[i]` walk es-toolkit's `isEqualWith` runs over a typed-array-tagged
        // value) must resolve against those bytes rather than answering
        // `undefined`. Non-index fields and non-byte-backed records are untouched.
        writer.line(format!(
            "    if let Some(element) = {element}(map, field) {{ return element; }}",
            element = smelt_stdlib::runtime_symbols::byte_buffer::ELEMENT,
        ));
        // `blob.constructor === Blob` holds in JavaScript, and the clone specs
        // assert it on a cloned host object. A marker-bearing record therefore
        // answers `.constructor` with the interned constructor value for its
        // identity — the very object a bare `Blob` reference evaluates to. An own
        // `constructor` field still wins (source code can assign one), and an
        // unmarked record keeps `undefined`, which is what makes two plain objects
        // compare as equal instances.
        writer.line(format!(
            "    if field == \"constructor\" && !map.contains_key(\"constructor\") && let Some(class) = {class_of}(map) {{ return {namespace}(class); }}",
            class_of = smelt_stdlib::runtime_symbols::host::MARKER_CONSTRUCTOR_CLASS,
            namespace = smelt_stdlib::runtime_symbols::host::BUILTIN_NAMESPACE,
        ));
        // The ambient global object's properties ARE the modeled JavaScript
        // globals. `globalThis.Error` already resolved to the modeled
        // constructor through the static-member normalization in the frontend,
        // but the same read spelled with a runtime key (`globalThis[type]`, the
        // shape every lodash-derived typed-array/error spec loop uses) answered
        // `undefined`, so `new (globalThis[type])(...)` fabricated a
        // null-returning closure call and every value it produced compared
        // equal to every other. Resolving the name against the interned
        // builtin-constructor registry — the same table the static spelling
        // uses — makes the two spellings agree. A name this profile models no
        // constructor for is still genuinely absent, and an own field set on
        // the global object still wins.
        writer.line(format!(
            "    if map.contains_key(\"__smelt_global_object\") && !map.contains_key(field) && smelt_builtin_construct_kind(field).is_some() {{ return {namespace}(field); }}",
            namespace = smelt_stdlib::runtime_symbols::host::BUILTIN_NAMESPACE,
        ));
        // A boxed string (`new String('ab')`) is an exotic String object: its
        // payload's own properties — `length` and the indexed characters — are
        // properties of the WRAPPER too, so `new String('ab').length` is 2 and
        // `[0]` is `"a"`. They are not stored as own entries (that would make
        // them enumerable and would double them in a clone), so they are
        // synthesized on read, exactly like the erased `Map`'s `.size` below. An
        // own field still wins, and the payload's *methods* stay on the
        // prototype rather than becoming own members.
        writer.line(format!(
            "    if !map.contains_key(field) && let Some(SmeltUnknown::String(text)) = map.get({string_marker:?}).and(map.get(\"value\")) {{ if field == \"length\" {{ return SmeltUnknown::Number(text.chars().count() as f64); }} if let Ok(index) = field.parse::<usize>() {{ return text.chars().nth(index).map_or(SmeltUnknown::Undefined, |character| SmeltUnknown::String(character.to_string().into())); }} }}",
            string_marker = smelt_stdlib::host_object_marker("String").unwrap_or("__smelt_string"),
        ));
        // A builtin read as a value is a marker record (`smelt_builtin_namespace`),
        // and JavaScript answers property reads on it: `Array.prototype` is the
        // builtin's prototype object and `Array.isArray` is a function value. Both
        // resolve through the shared modeled-member registry, so the value spelling
        // of a member and its call spelling agree. An own field still wins, and an
        // unmodeled member stays `undefined` rather than becoming a callable that
        // cannot run.
        writer.line("    if !map.contains_key(field) && let Some(SmeltUnknown::String(class)) = map.get(\"__smelt_builtin_namespace\").and(map.get(\"name\")) { if field == \"prototype\" { return smelt_builtin_prototype_object(&class); } if let Some(member) = smelt_builtin_member_value(&class, \"static\", field) { return member; } }");
        writer.line("    if let Some(SmeltUnknown::String(class)) = map.get(\"__smelt_builtin_prototype\") && let Some(member) = smelt_builtin_member_value(&class, \"prototype\", field) { return member; }");
        writer.line("    if field == \"size\" && let Some(SmeltUnknown::Array(pairs)) = map.get(\"__smelt_map\") { return SmeltUnknown::Number(pairs.len() as f64); }");
        // Same synthesis for an erased `Set` (`{ __smelt_set: [members...] }`):
        // real Sets expose `.size` through `Set.prototype`, absent from the marker
        // object's own fields, so derive it from the member count.
        writer.line("    if field == \"size\" && let Some(SmeltUnknown::Array(members)) = map.get(\"__smelt_set\") { return SmeltUnknown::Number(members.len() as f64); }");
        // Erased `Map` prototype methods, the exact counterpart of the erased-Set
        // block below. Only `.size` used to be synthesized, so every other
        // `Map.prototype` read on an erased Map answered `undefined` — and a
        // *call* of that `undefined` collapsed to a null callback rather than
        // failing, which made the miss silent. es-toolkit `isEqualWith` walks a
        // Map with `for (const [key, value] of a.entries())` after checking
        // `a.size !== b.size`: with `entries()` yielding nothing, the loop body
        // never ran and two same-size Maps with completely different contents
        // compared EQUAL.
        //
        // Entries are read from the `{ __smelt_map: [[k, v], ...] }` marker array.
        // `get`/`has` apply SameValueZero (`same_js_key`) and take the FIRST
        // matching pair, which is the only one a Map can hold. `forEach` invokes
        // the callback with `(value, key, map)` per JS semantics — note the
        // argument order is the reverse of the stored pair. As in the erased-Set
        // block, the direct `m.forEach(cb)` spelling never reaches here: the
        // erased-iterable coercion claims it first and walks the marker array,
        // handing the callback `(entry, index)`. The arm covers the spellings that
        // do go through a field read (`m["forEach"]`, a detached method value).
        //
        // The mutators (`set`/`delete`/`clear`) are synthesized too, because a
        // Map does NOT always reach a mutation site at its typed `SmeltJsMap`:
        // a declared `Map<K, V> | SomeCacheInterface` erases the whole union to
        // `SmeltUnknown`, and es-toolkit's `memoize` mutates its cache through
        // exactly that spelling (`cache.set(key, result)`). With the mutators
        // missing, the read answered `undefined`, the call collapsed to a null
        // callback, and every write vanished silently — a memoizing function
        // never memoized. The marker array is the map's own shared storage
        // (`SmeltArray` is an `Rc<RefCell<..>>` handle), so an insert, a delete
        // and a clear all land in the same entries every later read walks. Key
        // identity is SameValueZero (`same_js_key`) and the return values are
        // JavaScript's: `set` answers the map, `delete` a boolean, `clear`
        // `undefined`.
        writer.line("    if let Some(SmeltUnknown::Array(pairs)) = map.get(\"__smelt_map\") {");
        // The live handle, kept alongside the read-only snapshot the accessors
        // below close over: a mutator must reach the shared storage, not a copy.
        writer.line("        let entry_store = pairs.clone();");
        writer.line("        let pairs = pairs.into_vec();");
        writer.line("        let entry_at = |pair: &SmeltUnknown| -> Option<(SmeltUnknown, SmeltUnknown)> { let SmeltUnknown::Array(entry) = pair else { return None }; let mut entry = entry.clone().into_vec().into_iter(); match (entry.next(), entry.next()) { (Some(key), Some(value)) => Some((key, value)), _ => None } };");
        writer.line("        let entries = pairs.iter().filter_map(entry_at).collect::<Vec<_>>();");
        writer.line("        match field {");
        writer.line("            \"keys\" => { let keys = entries.iter().map(|(key, _)| key.clone()).collect::<Vec<_>>(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Array(keys.clone().into())))); }");
        writer.line("            \"values\" => { let values = entries.iter().map(|(_, value)| value.clone()).collect::<Vec<_>>(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Array(values.clone().into())))); }");
        writer.line("            \"entries\" => { let pairs = pairs.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Array(pairs.clone().into())))); }");
        writer.line("            \"get\" => { let entries = entries.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let needle = args.into_iter().next().unwrap_or(SmeltUnknown::Undefined); Ok(entries.iter().find(|(key, _)| key.same_js_key(&needle)).map_or(SmeltUnknown::Undefined, |(_, value)| value.clone())) })); }");
        writer.line("            \"has\" => { let entries = entries.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let needle = args.into_iter().next().unwrap_or(SmeltUnknown::Undefined); Ok(SmeltUnknown::Bool(entries.iter().any(|(key, _)| key.same_js_key(&needle)))) })); }");
        writer.line("            \"forEach\" => { let entries = entries.clone(); let receiver = SmeltUnknown::Object(map.clone()); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { if let Some(SmeltUnknown::Function(callback)) = args.into_iter().next() { for (key, value) in entries.clone() { callback(vec![value, key, receiver.clone()])?; } } Ok(SmeltUnknown::Undefined) })); }");
        writer.line("            \"set\" => { let store = entry_store.clone(); let receiver = SmeltUnknown::Object(map.clone()); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let mut args = args.into_iter(); let key = args.next().unwrap_or(SmeltUnknown::Undefined); let value = args.next().unwrap_or(SmeltUnknown::Undefined); let existing = store.iter().position(|pair| smelt_map_entry_key_is(&pair, &key)); let entry = SmeltUnknown::Array(vec![key, value].into()); match existing { Some(index) => store.set_index(index, entry), None => store.push(entry) } Ok(receiver.clone()) })); }");
        writer.line("            \"delete\" => { let store = entry_store.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let needle = args.into_iter().next().unwrap_or(SmeltUnknown::Undefined); let kept = store.iter().filter(|pair| !smelt_map_entry_key_is(pair, &needle)).collect::<Vec<_>>(); let removed = kept.len() != store.len(); store.replace_all(kept); Ok(SmeltUnknown::Bool(removed)) })); }");
        writer.line("            \"clear\" => { let store = entry_store.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| { store.replace_all(Vec::new()); Ok(SmeltUnknown::Undefined) })); }");
        writer.line("            _ => {}");
        writer.line("        }");
        writer.line("    }");
        // Erased `Set` prototype methods. A real Set exposes
        // `values`/`keys`/`entries`/`has`/`forEach` through `Set.prototype`, which
        // the `{ __smelt_set: [...] }` marker object does not store as own fields.
        // Synthesize them so generic `unknown`-typed code that iterates or probes
        // an erased Set (e.g. `es-toolkit` `isEqualWith`'s `Array.from(a.values())`
        // membership walk) sees the members instead of `undefined`. Each returns a
        // fresh closure over a clone of the member array. `values`/`keys` yield the
        // member array (Set keys equal values); `entries` yields `[value, value]`
        // pairs; `has` applies SameValueZero (`same_js_key`); `forEach` invokes the
        // callback with `(value, value, set)` per JS semantics.
        // The mutators (`add`/`delete`/`clear`) stay absent: unlike a Map, a Set
        // whose element type is concrete erases to a plain `SmeltUnknown::Array`
        // rather than to the `{ __smelt_set: [..] }` marker, so a write through
        // an erased Set is a different (unmodeled) shape, not this one.
        writer.line("    if let Some(SmeltUnknown::Array(members)) = map.get(\"__smelt_set\") {");
        writer.line("        let members = members.into_vec();");
        writer.line("        match field {");
        writer.line("            \"values\" | \"keys\" => { let members = members.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Array(members.clone().into())))); }");
        writer.line("            \"entries\" => { let members = members.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |_args: Vec<SmeltUnknown>| Ok(SmeltUnknown::Array(members.clone().into_iter().map(|value| SmeltUnknown::Array(vec![value.clone(), value].into())).collect::<Vec<_>>().into())))); }");
        writer.line("            \"has\" => { let members = members.clone(); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { let needle = args.into_iter().next().unwrap_or(SmeltUnknown::Undefined); Ok(SmeltUnknown::Bool(members.iter().any(|member| member.same_js_key(&needle)))) })); }");
        writer.line("            \"forEach\" => { let members = members.clone(); let receiver = SmeltUnknown::Object(map.clone()); return SmeltUnknown::Function(::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| { if let Some(SmeltUnknown::Function(callback)) = args.into_iter().next() { for member in members.clone() { callback(vec![member.clone(), member.clone(), receiver.clone()])?; } } Ok(SmeltUnknown::Undefined) })); }");
        writer.line("            _ => {}");
        writer.line("        }");
        writer.line("    }");
        writer.line("    // A missing property reads as JS `undefined`, distinct from an");
        writer.line("    // explicit `null` value (`obj.missing === undefined`, `!== null`).");
        // Prototype-chain read. `Object.create(proto)` stores the prototype's
        // members behind a `__smelt_proto:` prefix so they stay out of own-key
        // enumeration; an own field shadows the inherited one, exactly as in JS.
        // A class instance answers a method read from its prototype slot, the
        // same way `Object.create(proto)` answers an inherited property from
        // `__smelt_proto:`. Both are consulted only after the own property
        // misses, exactly as JavaScript's prototype chain does.
        writer.line("    let smelt_field_value = match map.get(field) { Some(value) => Some(value), None => match map.get(&format!(\"__smelt_proto:{field}\")) { Some(value) => Some(value), None => map.get(&format!(\"__smelt_method:{field}\")) } };");
        writer.line("    match smelt_field_value.unwrap_or(SmeltUnknown::Undefined) {");
        writer.line("        SmeltUnknown::Object(getter) if getter.contains_key(\"__smelt_get\") => match getter.get(\"__smelt_get\") {");
        writer.line("            Some(SmeltUnknown::Function(smelt_getter)) => (smelt_getter)(Vec::new()).unwrap_or_else(|error| panic!(\"{}\", error)),");
        writer.line("            _ => SmeltUnknown::Null,");
        writer.line("        },");
        writer.line("        value => value,");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        // `Object.prototype`'s members, as a lookup FALLBACK rather than stored
        // entries.
        //
        // In JavaScript every plain object inherits them: `'toString' in {}` is
        // `true`, `({}).toString` is a function, and `({}).toString ===
        // Object.prototype.toString` — one function, on the prototype, so any two
        // reads are `===`. Smelt models an object as its own entries, so these
        // members have to come from somewhere else, and they must NOT be entries:
        // enumeration (`Object.keys`, `for...in`), structural equality and JSON
        // would then see them, making `{}` a two-key object unequal to `{}`.
        //
        // Each member is built once per key and cached with one canonical identity
        // (`smelt_method_identity`, the same registry class-method reads use),
        // which is what makes the `===` above hold. The receiver is taken from the
        // first argument, matching the explicit-`this` spelling
        // (`Object.prototype.toString.call(v)`); the frontend lowers that spelling
        // to a direct helper call, so this path serves value-position reads
        // (`const f = Object.prototype.toString`) and presence checks.
        //
        // `constructor` is deliberately NOT in the table: `smelt_get_object_field`
        // already answers it for a class instance and for a marker-bearing host
        // record, and a plain object answering `undefined` there is what lets
        // es-toolkit `isEqualWith` fall through to its `isPlainObject(a) &&
        // isPlainObject(b)` arm.
        writer.line("thread_local! {");
        writer.line("    /// One cached `Object.prototype` member value per key, so repeated reads are `===`.");
        writer.line("    static SMELT_OBJECT_PROTOTYPE_MEMBERS: ::std::cell::RefCell<::std::collections::HashMap<&'static str, SmeltUnknown>> = ::std::cell::RefCell::new(::std::collections::HashMap::new());");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Apply one `Object.prototype` member to an explicit receiver.");
        writer.line("fn smelt_object_prototype_apply(key: &str, args: Vec<SmeltUnknown>) -> SmeltUnknown {");
        writer.line("    let mut args = args.into_iter();");
        writer.line("    let receiver = args.next().unwrap_or(SmeltUnknown::Undefined);");
        writer.line("    match key {");
        writer.line("        \"Object.prototype.toString\" | \"Object.prototype.toLocaleString\" => SmeltUnknown::String(smelt_object_to_string_tag(&receiver).into()),");
        writer.line("        \"Object.prototype.valueOf\" => smelt_unbox_primitive(receiver),");
        writer.line("        \"Object.prototype.hasOwnProperty\" | \"Object.prototype.propertyIsEnumerable\" => { let key = smelt_property_key(args.next().unwrap_or(SmeltUnknown::Undefined)); SmeltUnknown::Bool(match receiver { SmeltUnknown::Object(map) => map.contains_key(&key), SmeltUnknown::Array(values) => values.own_keys().contains(&key), _ => false }) }");
        // `isPrototypeOf` asks whether the receiver appears on the argument's
        // prototype CHAIN. Smelt represents a prototype as an opaque sentinel
        // (`smelt_prototype_sentinel`) rather than as a linked object, so there is
        // no chain to walk: the honest answer for the only receivers that can
        // reach here — a plain object or the `Object.prototype` sentinel read as a
        // value — is `false`, which is also what JavaScript answers for every
        // plain object receiver.
        writer.line("        _ => SmeltUnknown::Bool(false),");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("/// Read an `Object.prototype` member, or `None` when the name is not one.");
        writer.line("fn smelt_object_prototype_member(field: &str) -> Option<SmeltUnknown> {");
        writer.line("    let key: &'static str = match field {");
        writer.line("        \"toString\" => \"Object.prototype.toString\",");
        writer.line("        \"toLocaleString\" => \"Object.prototype.toLocaleString\",");
        writer.line("        \"valueOf\" => \"Object.prototype.valueOf\",");
        writer.line("        \"hasOwnProperty\" => \"Object.prototype.hasOwnProperty\",");
        writer.line("        \"isPrototypeOf\" => \"Object.prototype.isPrototypeOf\",");
        writer.line("        \"propertyIsEnumerable\" => \"Object.prototype.propertyIsEnumerable\",");
        writer.line("        _ => return None,");
        writer.line("    };");
        writer.line("    Some(SMELT_OBJECT_PROTOTYPE_MEMBERS.with(|members| members.borrow_mut().entry(key).or_insert_with(|| { let function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |args: Vec<SmeltUnknown>| Ok(smelt_object_prototype_apply(key, args))); smelt_link_function_identity_key(&function, smelt_method_identity(key)); SmeltUnknown::Function(function) }).clone()))");
        writer.line("}");
        writer.blank_line();
        // Members of the other builtins (`Array.prototype.slice`,
        // `Array.isArray`) read as values through the shared registry, the same
        // way `Object.prototype`'s members do just above.
        builtin_member_prelude::emit(&mut writer);
        // Property reads on an erased ARRAY. A JS array answers `length`, its
        // element indices, and the named properties written through its side
        // table; `Array.prototype`'s own methods are lowered statically, so a miss
        // here is `undefined` rather than a prototype walk.
        writer.line("/// Read a property off an erased JavaScript array (`arr.k` / `arr[k]`).");
        writer.line("fn smelt_get_array_field(values: &SmeltArray, field: &str) -> SmeltUnknown {");
        writer.line("    if field == \"length\" { return SmeltUnknown::Number(values.len() as f64); }");
        writer.line("    if let Ok(index) = field.parse::<usize>() { return values.get(index).unwrap_or(SmeltUnknown::Undefined); }");
        writer.line("    values.named_property(field).unwrap_or(SmeltUnknown::Undefined)");
        writer.line("}");
        writer.blank_line();
        // One erased property read for every receiver shape, so `v.k` and `v[k]`
        // cannot diverge and each rule (marker records, array named properties,
        // the `Object.prototype` fallback) lands in one place.
        //
        // Last stop of the chain is `Object.prototype`, whose members every object
        // inherits — so a read that found no own entry (and no `__smelt_proto:` /
        // `__smelt_method:` level, both resolved inside `smelt_get_object_field`)
        // resolves there rather than answering `undefined` for `toString` or
        // `hasOwnProperty`. It sits HERE, at the property-read seam, and not
        // inside `smelt_get_object_field`, because helpers that ask that function
        // for a user-defined OWN override (`smelt_value_of_method`,
        // `smelt_function_method`) must not be handed the inherited member.
        //
        // The `"__smelt_proto:object"` arm is the prototype SENTINEL: Smelt
        // represents `Object.prototype` (and `Object.getPrototypeOf({})`) as that
        // opaque string rather than as a record, precisely so its members do not
        // become enumerable entries of every object created from it. A member read
        // on the sentinel resolves through the same fallback table an ordinary
        // object uses, which is what makes `Object.prototype.toString` a value.
        writer.line("/// Read a property off any erased value (JS `value.field`).");
        writer.line("fn smelt_get_unknown_field(value: &SmeltUnknown, field: &str) -> SmeltUnknown {");
        writer.line("    match value {");
        writer.line("        SmeltUnknown::Object(map) => match smelt_get_object_field(map, field) { SmeltUnknown::Undefined => smelt_object_prototype_member(field).unwrap_or(SmeltUnknown::Undefined), value => value },");
        writer.line("        SmeltUnknown::Array(values) => smelt_get_array_field(values, field),");
        writer.line("        SmeltUnknown::String(marker) if &**marker == \"__smelt_proto:object\" => smelt_object_prototype_member(field).unwrap_or(SmeltUnknown::Undefined),");
        writer.line("        _ => SmeltUnknown::Undefined,");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_unknown_is_nullish(value: &SmeltUnknown) -> bool { matches!(value, SmeltUnknown::Null | SmeltUnknown::Undefined) }");
        writer.line("fn smelt_unknown_is_undefined(value: &SmeltUnknown) -> bool { matches!(value, SmeltUnknown::Undefined) }");
        writer
            .line("fn smelt_missing_property_value() -> SmeltUnknown { SmeltUnknown::Undefined }");
        writer.line("fn smelt_missing_index_value() -> SmeltUnknown { SmeltUnknown::Undefined }");
        writer.blank_line();
        writer.line("fn smelt_unknown_structural_eq(left: &SmeltUnknown, right: &SmeltUnknown, seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool {");
        writer.line("    match (left, right) {");
        writer.line("        (SmeltUnknown::Null, SmeltUnknown::Null) => true,");
        writer.line("        (SmeltUnknown::Undefined, SmeltUnknown::Undefined) => true,");
        writer.line(
            "        (SmeltUnknown::Bool(left), SmeltUnknown::Bool(right)) => left == right,",
        );
        writer.line("        (SmeltUnknown::Number(left), SmeltUnknown::Number(right)) => left == right || (left.is_nan() && right.is_nan()),");
        writer.line(
            // JavaScript strings are immutable and `SmeltUnknown::String` shares
            // one `Rc<str>` between every copy of a value, so two strings that are
            // the SAME allocation are equal without touching their bytes. That is
            // the common case wherever a string is read out of a value and used as
            // a key: `groupBy` pulls the same `Rc` out of the same record field for
            // every element in a group, and this turns each of those comparisons
            // from a `memcmp` into a pointer compare. `Rc::ptr_eq` is only a fast
            // path — a differing pointer still falls through to content equality,
            // so two separately built strings with the same characters stay equal.
            "        (SmeltUnknown::String(left), SmeltUnknown::String(right)) => ::std::rc::Rc::ptr_eq(left, right) || left == right,",
        );
        writer.line(
            "        (SmeltUnknown::Symbol(left), SmeltUnknown::Symbol(right)) => left == right,",
        );
        writer.line("        (SmeltUnknown::Array(left), SmeltUnknown::Array(right)) => left.len() == right.len() && left.iter().zip(right.iter()).all(|(left, right)| smelt_unknown_structural_eq(&left, &right, seen)),");
        writer.line("        (SmeltUnknown::Object(left), SmeltUnknown::Object(right)) => smelt_object_structural_eq(left, right, seen),");
        writer.line("        (SmeltUnknown::Function(left), SmeltUnknown::Function(right)) => smelt_same_erased_function(left, right),");
        writer.line("        (SmeltUnknown::Promise(left), SmeltUnknown::Promise(right)) => left.id == right.id,");
        writer.line("        _ => false,");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_object_structural_eq(left: &SmeltObject, right: &SmeltObject, seen: &mut ::std::collections::HashSet<(usize, usize)>) -> bool {");
        writer.line("    if left.contains_key(\"__smelt_date\") || right.contains_key(\"__smelt_date\") { let left_date = smelt_unknown_date_value(&SmeltUnknown::Object(left.clone())); let right_date = smelt_unknown_date_value(&SmeltUnknown::Object(right.clone())); return left_date == right_date || (left_date.is_nan() && right_date.is_nan()); }");
        writer.line("    if left.id == right.id { return true; }");
        writer.line("    let key = (left.id, right.id);");
        writer.line("    if !seen.insert(key) { return true; }");
        // Prototype-carried members (`__smelt_proto:<name>`) are NOT own
        // properties in JavaScript, so structural equality must not see them.
        // Erasing a class instance now pushes one bound `SmeltUnknown::Function`
        // per method under that prefix, and function values compare by `Rc`
        // pointer — without this filter two structurally equal instances of the
        // same class would each carry distinct closures and compare unequal.
        // The same rule already governs `Object.create(proto)` results, whose
        // inherited keys live under the same prefix.
        writer.line("    let left_entries = left.iter().filter(|(key, _)| !key.starts_with(\"__smelt_proto:\") && !key.starts_with(\"__smelt_method:\")).collect::<Vec<_>>();");
        writer.line("    let right_own = right.iter().filter(|(key, _)| !key.starts_with(\"__smelt_proto:\") && !key.starts_with(\"__smelt_method:\")).count();");
        writer.line("    if left_entries.len() != right_own { return false; }");
        writer.line("    left_entries.into_iter().all(|(key, left_value)| right.get(&key).is_some_and(|right_value| smelt_unknown_structural_eq(&left_value, &right_value, seen)))");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_unknown_structural_hash<H: ::std::hash::Hasher>(value: &SmeltUnknown, state: &mut H, seen: &mut ::std::collections::HashSet<usize>) {");
        writer.line("    match value {");
        writer.line("        SmeltUnknown::Null => 0_u8.hash(state),");
        writer.line("        SmeltUnknown::Undefined => 8_u8.hash(state),");
        writer
            .line("        SmeltUnknown::Bool(value) => { 1_u8.hash(state); value.hash(state); }");
        writer.line("        SmeltUnknown::Number(value) => { 2_u8.hash(state); if value.is_nan() { f64::NAN.to_bits().hash(state); } else { value.to_bits().hash(state); } }");
        writer.line(
            "        SmeltUnknown::String(value) => { 3_u8.hash(state); value.hash(state); }",
        );
        writer.line(
            "        SmeltUnknown::Symbol(value) => { 4_u8.hash(state); value.hash(state); }",
        );
        writer.line("        SmeltUnknown::Array(values) => { 5_u8.hash(state); values.len().hash(state); for value in values.iter() { smelt_unknown_structural_hash(&value, state, seen); } }");
        writer.line("        SmeltUnknown::Object(values) => { 6_u8.hash(state); smelt_object_structural_hash(values, state, seen); }");
        writer.line("        SmeltUnknown::Function(function) => { 7_u8.hash(state); ::std::rc::Rc::as_ptr(function).hash(state); }");
        writer.line("        SmeltUnknown::Promise(promise) => { 9_u8.hash(state); promise.id.hash(state); }");
        writer.line("    }");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_unknown_stable_hash_key(value: &SmeltUnknown) -> u64 {");
        writer.line("    let mut hasher = SmeltFieldHasher::default();");
        writer.line("    let mut seen = ::std::collections::HashSet::new();");
        writer.line("    smelt_unknown_structural_hash(value, &mut hasher, &mut seen);");
        writer.line("    ::std::hash::Hasher::finish(&hasher)");
        writer.line("}");
        writer.blank_line();
        writer.line("fn smelt_object_structural_hash<H: ::std::hash::Hasher>(object: &SmeltObject, state: &mut H, seen: &mut ::std::collections::HashSet<usize>) {");
        writer.line("    if !seen.insert(object.id) { 255_u8.hash(state); return; }");
        // Mirror the `__smelt_proto:` filter in `smelt_object_structural_eq`:
        // `Hash` and `PartialEq` must agree, and prototype-carried members are
        // not part of an object's own structural identity.
        writer.line("    let mut entries = object.iter().filter(|(key, _)| !key.starts_with(\"__smelt_proto:\") && !key.starts_with(\"__smelt_method:\")).collect::<Vec<_>>();");
        writer.line("    entries.sort_by(|left, right| left.0.cmp(&right.0));");
        writer.line("    entries.len().hash(state);");
        writer.line("    for (key, value) in entries { key.hash(state); smelt_unknown_structural_hash(&value, state, seen); }");
        writer.line("}");
        writer.blank_line();
        writer.block("impl ::std::fmt::Debug for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result",
                |fn_writer| {
                    fn_writer.block("match self", |match_writer| {
                        match_writer.line("Self::Null => formatter.write_str(\"Null\"),");
                        match_writer.line("Self::Undefined => formatter.write_str(\"Undefined\"),");
                        match_writer.line("Self::Bool(value) => formatter.debug_tuple(\"Bool\").field(value).finish(),");
                        match_writer.line("Self::Number(value) => formatter.debug_tuple(\"Number\").field(value).finish(),");
                        match_writer.line("Self::String(value) => formatter.debug_tuple(\"String\").field(value).finish(),");
                        match_writer.line("Self::Symbol(value) => formatter.debug_tuple(\"Symbol\").field(value).finish(),");
                        match_writer.line("Self::Array(values) => formatter.debug_tuple(\"Array\").field(values).finish(),");
                        match_writer.line("Self::Object(values) => formatter.debug_tuple(\"Object\").field(values).finish(),");
                        match_writer.line("Self::Function(_) => formatter.write_str(\"Function(<closure>)\"),");
                        match_writer.line("Self::Promise(value) => formatter.debug_tuple(\"Promise\").field(value).finish(),");
                    });
                },
            );
        });
        writer.blank_line();
        writer.block("impl PartialEq for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn eq(&self, other: &Self) -> bool", |fn_writer| {
                fn_writer.line("let mut smelt_seen = ::std::collections::HashSet::new();");
                fn_writer.line("smelt_unknown_structural_eq(self, other, &mut smelt_seen)");
            });
        });
        writer.blank_line();
        // `Default` is what every ABSENT erased slot falls back to: an
        // out-of-range element read, a `resize` fill, a `new Array(n)` hole.
        // JavaScript answers `undefined` for every one of those -- reading a
        // missing index or a hole never produces `null`, which is a value a
        // program has to store deliberately. Defaulting to `Null` made
        // `[1, , 2]`-shaped holes and missing reads indistinguishable from a
        // stored `null`, so `at(['a','b','c'], [4])` compared unequal to
        // `[undefined]`.
        writer.block("impl Default for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.line("Self::Undefined");
            });
        });
        writer.blank_line();
        if needs_timer_helpers {
            writer.line("struct SmeltTimer {");
            writer.line("    id: u64,");
            writer.line("    due_ms: u64,");
            writer.line("    callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>,");
            writer.line("    // `Some(period)` marks a repeating `setInterval` timer that re-arms");
            writer.line("    // itself `period` ms after each fire; `None` is a one-shot `setTimeout`.");
            writer.line("    period_ms: Option<u64>,");
            writer.line("}");
            writer.blank_line();
            writer.line("thread_local! {");
            writer.line("    static SMELT_NEXT_TIMER_ID: ::std::cell::Cell<u64> = const { ::std::cell::Cell::new(1) };");
            writer.line("    static SMELT_TIMERS: ::std::cell::RefCell<Vec<SmeltTimer>> = const { ::std::cell::RefCell::new(Vec::new()) };");
            writer.line("    static SMELT_PROMISE_TASKS: ::std::cell::RefCell<Vec<::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()>>>>> = const { ::std::cell::RefCell::new(Vec::new()) };");
            writer.line("    // Non-zero while a `Promise.race` driver owns the event loop; see");
            writer.line("    // `smelt_promise_race`.");
            writer.line("    static SMELT_RACE_DEPTH: ::std::cell::Cell<usize> = const { ::std::cell::Cell::new(0) };");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {reset_timers}() {{",
                reset_timers = smelt_stdlib::runtime_symbols::timers::RESET_TIMERS,
            ));
            writer.line("    SMELT_NEXT_TIMER_ID.with(|next| next.set(1));");
            writer.line("    SMELT_VIRTUAL_MS.with(|virtual_ms| virtual_ms.set(0));");
            writer.line("    SMELT_TIMER_EPOCH.with(|epoch| epoch.set(None));");
            writer.line("    SMELT_TIMERS.with(|timers| timers.borrow_mut().clear());");
            writer.line("    SMELT_PROMISE_TASKS.with(|tasks| tasks.borrow_mut().clear());");
            writer.line("    SMELT_RACE_DEPTH.with(|depth| depth.set(0));");
            writer.line("    SMELT_PRIME_DEPTH.with(|depth| depth.set(0));");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {noop_waker}() -> ::std::task::Waker {{",
                noop_waker = smelt_stdlib::runtime_symbols::timers::NOOP_WAKER,
            ));
            writer.line(
                "    unsafe fn clone(_: *const ()) -> ::std::task::RawWaker { smelt_raw_waker() }",
            );
            writer.line("    unsafe fn wake(_: *const ()) {}");
            writer.line("    unsafe fn wake_by_ref(_: *const ()) {}");
            writer.line("    unsafe fn drop(_: *const ()) {}");
            writer.line("    fn smelt_raw_waker() -> ::std::task::RawWaker { ::std::task::RawWaker::new(::std::ptr::null(), &::std::task::RawWakerVTable::new(clone, wake, wake_by_ref, drop)) }");
            writer.line("    unsafe { ::std::task::Waker::from_raw(smelt_raw_waker()) }");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {spawn_promise_task}(task: ::std::pin::Pin<Box<dyn ::std::future::Future<Output = ()>>>) {{",
                spawn_promise_task = smelt_stdlib::runtime_symbols::timers::SPAWN_PROMISE_TASK,
            ));
            writer.line("    SMELT_PROMISE_TASKS.with(|tasks| tasks.borrow_mut().push(task));");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "async fn {drain_promise_tasks}() {{",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            writer.line("    for _ in 0..64 {");
            writer.line("        let mut tasks = SMELT_PROMISE_TASKS.with(|tasks| ::std::mem::take(&mut *tasks.borrow_mut()));");
            writer.line("        if tasks.is_empty() { break; }");
            writer.line(format!(
                "        let waker = {noop_waker}();",
                noop_waker = smelt_stdlib::runtime_symbols::timers::NOOP_WAKER,
            ));
            writer.line("        let mut cx = ::std::task::Context::from_waker(&waker);");
            writer.line("        let mut pending = Vec::new();");
            writer.line("        for mut task in tasks.drain(..) {");
            writer.line(
                "            if task.as_mut().poll(&mut cx).is_pending() { pending.push(task); }",
            );
            writer.line("        }");
            writer.line("        let had_pending = !pending.is_empty();");
            writer.line(
                "        SMELT_PROMISE_TASKS.with(|tasks| tasks.borrow_mut().extend(pending));",
            );
            writer.line("        if !had_pending { break; }");
            writer.line("        tokio::task::yield_now().await;");
            writer.line("    }");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {set_timeout}(callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>, delay_ms: f64) -> SmeltUnknown {{",
                set_timeout = smelt_stdlib::runtime_symbols::timers::SET_TIMEOUT,
            ));
            writer.line("    let id = SMELT_NEXT_TIMER_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id });");
            writer.line("    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 { delay_ms as u64 } else { 0 };");
            writer.line("    let due_ms = smelt_mono_ms().saturating_add(delay_ms);");
            writer.line("    SMELT_TIMERS.with(|timers| timers.borrow_mut().push(SmeltTimer { id, due_ms, callback, period_ms: None }));");
            writer.line("    SmeltUnknown::Number(id as f64)");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {set_interval}(callback: ::std::rc::Rc<::std::cell::RefCell<dyn FnMut() -> Result<(), Box<dyn std::error::Error>>>>, period_ms: f64) -> SmeltUnknown {{",
                set_interval = smelt_stdlib::runtime_symbols::timers::SET_INTERVAL,
            ));
            writer.line("    let id = SMELT_NEXT_TIMER_ID.with(|next| { let id = next.get(); next.set(id.saturating_add(1)); id });");
            writer.line("    // Clamp non-positive periods to 1 ms so an interval still advances virtual");
            writer.line("    // time and cannot busy-loop the drain at the current instant.");
            writer.line("    let period_ms = if period_ms.is_finite() && period_ms > 0.0 { period_ms as u64 } else { 1 };");
            writer.line("    let due_ms = smelt_mono_ms().saturating_add(period_ms);");
            writer.line("    SMELT_TIMERS.with(|timers| timers.borrow_mut().push(SmeltTimer { id, due_ms, callback, period_ms: Some(period_ms) }));");
            writer.line("    SmeltUnknown::Number(id as f64)");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "fn {clear_timeout}<T: IntoSmeltUnknown>(handle: T) {{",
                clear_timeout = smelt_stdlib::runtime_symbols::timers::CLEAR_TIMEOUT,
            ));
            writer.line(
                "    let SmeltUnknown::Number(id) = handle.into_smelt_unknown() else { return; };",
            );
            writer.line("    let id = id as u64;");
            writer.line(
            "    SMELT_TIMERS.with(|timers| timers.borrow_mut().retain(|timer| timer.id != id));",
        );
            writer.line("}");
            writer.blank_line();
            // `clearInterval` cancels by id exactly like `clearTimeout`; both share
            // the single timer queue. Emit it as a thin alias so the generated
            // `async_clear_interval` call site resolves to a real helper.
            writer.line(format!(
                "fn {clear_interval}<T: IntoSmeltUnknown>(handle: T) {{ {clear_timeout}(handle) }}",
                clear_interval = smelt_stdlib::runtime_symbols::timers::CLEAR_INTERVAL,
                clear_timeout = smelt_stdlib::runtime_symbols::timers::CLEAR_TIMEOUT,
            ));
            writer.blank_line();
            // `id_barrier` defers timers created *after* the current drain began:
            // a timer whose id is at or above the barrier was (re)scheduled during
            // this drain pass and, like a Node timer scheduled inside the timer
            // phase, must wait for a later tick rather than firing again now. This
            // stops a self-rearming `setTimeout(cb, 0)` (e.g. a funnel's 0 ms
            // interval) from firing repeatedly within one `sleep`.
            writer.line(format!(
                "fn {drain_due_timers}(id_barrier: u64) {{",
                drain_due_timers = smelt_stdlib::runtime_symbols::timers::DRAIN_DUE_TIMERS,
            ));
            writer.line("    loop {");
            writer.line("        let now = smelt_mono_ms();");
            writer.line("        let due = SMELT_TIMERS.with(|timers| {");
            writer.line("            let mut timers = timers.borrow_mut();");
            writer.line("            let mut due = Vec::new();");
            writer.line("            let mut pending = Vec::new();");
            writer.line("            for timer in timers.drain(..) {");
            writer.line("                if timer.due_ms <= now && timer.id < id_barrier { due.push(timer); } else { pending.push(timer); }");
            writer.line("            }");
            writer.line("            *timers = pending;");
            writer.line("            due");
            writer.line("        });");
            writer.line("        if due.is_empty() { break; }");
            writer.line("        for timer in due {");
            writer.line("            (&mut *timer.callback.borrow_mut())().unwrap_or_else(|error| panic!(\"{}\", error));");
            writer.line("            // Re-arm repeating `setInterval` timers for their next period. The");
            writer.line("            // next fire is scheduled `period` ms from the current virtual time, so");
            writer.line("            // it is strictly in the future and cannot re-fire within this drain pass.");
            writer.line("            if let Some(period_ms) = timer.period_ms {");
            writer.line("                let next_due = now.saturating_add(period_ms);");
            writer.line("                SMELT_TIMERS.with(|timers| timers.borrow_mut().push(SmeltTimer { id: timer.id, due_ms: next_due, callback: timer.callback.clone(), period_ms: Some(period_ms) }));");
            writer.line("            }");
            writer.line("        }");
            writer.line("    }");
            writer.line("}");
            writer.blank_line();
            writer.line(format!(
                "async fn {sleep_ms}(delay_ms: f64) {{",
                sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
            ));
            // Suspend immediately under an eager prefix poll, before touching the
            // clock. `from_future_primed` runs an async body's synchronous prefix
            // at call time; in JavaScript that prefix schedules its timers but
            // does not make time pass, whereas this helper advances virtual time
            // to its own deadline as soon as it is driven. Yielding here leaves
            // the prefix's effects in place and defers all timekeeping to the
            // first real poll, so a long sleep started inside a primed body
            // cannot outrun deadlines armed after it (see `SMELT_PRIME_DEPTH`).
            writer.line("    if SMELT_PRIME_DEPTH.with(::std::cell::Cell::get) > 0 { tokio::task::yield_now().await; }");
            writer.line(format!(
                "    {drain_promise_tasks}().await;",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            writer.line("    let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 { delay_ms as u64 } else { 0 };");
            writer.line("    let target_ms = smelt_mono_ms().saturating_add(delay_ms);");
            // For a zero-delay sleep (one event-loop tick), only timers that
            // already exist when it begins may fire; anything (re)scheduled while
            // draining is deferred to a later tick, exactly as Node runs a timer
            // scheduled inside the timer phase on the next turn. A positive-delay
            // sleep spans a real window and must fire every timer that becomes due
            // within it, including ones scheduled mid-window (e.g. a recursive
            // debounce re-arming itself), so it uses no barrier.
            writer.line("    let id_barrier = if delay_ms == 0 { SMELT_NEXT_TIMER_ID.with(::std::cell::Cell::get) } else { u64::MAX };");
            // Fire every timer due within the requested window, advancing virtual
            // time to each in turn and draining the microtask queue between fires.
            writer.line("    let mut fired_any = false;");
            writer.line("    loop {");
            writer.line("        let next_due = SMELT_TIMERS.with(|timers| timers.borrow().iter().filter(|timer| timer.due_ms <= target_ms && timer.id < id_barrier).map(|timer| timer.due_ms).min());");
            writer.line("        let Some(next_due) = next_due else { break; };");
            writer.line("        fired_any = true;");
            writer.line("        smelt_virtual_advance_to(next_due);");
            writer.line(format!(
                "        {drain_due_timers}(id_barrier);",
                drain_due_timers = smelt_stdlib::runtime_symbols::timers::DRAIN_DUE_TIMERS,
            ));
            writer.line(format!(
                "        {drain_promise_tasks}().await;",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            writer.line("    }");
            writer.line("    smelt_virtual_advance_to(target_ms);");
            // Node-style run-until-idle. A zero-delay sleep is what the generated
            // promise-executor spin loops await while waiting for a result cell to
            // be settled (e.g. by a `setTimeout(resolve, 100)` timer). With no
            // window to fire into, virtual time would never reach that timer and
            // the spin loop would starve forever. When the requested window has no
            // due timer AND no detached promise task is still runnable (the
            // microtask queue is drained, so nothing else can make progress), the
            // event loop is idle: advance virtual time to the earliest pending
            // timer and fire it, exactly as Node advances to its timer phase once
            // microtasks settle. Repeat until either the queue has runnable work
            // again, a timer settles the awaited state, or no timers remain.
            //
            // This applies only to a zero-delay sleep. A positive-delay sleep has
            // a real deadline: it must fire exactly the timers due within its
            // window (handled above) and must not jump the clock to a later timer,
            // or a bounded `await delay(35)` would over-fire a `setInterval`.
            //
            // It also applies only when the window fired nothing. If the window
            // already ran a due timer, progress was made this tick and control
            // must return so the caller's spin loop can re-check its awaited
            // state; jumping ahead to fire a timer that was (re)scheduled during
            // this drain — e.g. a `setInterval(_, 0)` re-arming itself — would
            // over-fire it a tick early and, for a funnel, collapse the burst to
            // idle before the next `call`.
            writer.line("    'idle: {");
            writer.line("        // A `Promise.race` driver owns the clock while it is running: if a");
            writer.line("        // racer advanced time here, polling one racer could fire ANOTHER");
            writer.line("        // racer's timer, so both settle in the same round and the winner");
            writer.line("        // stops being the one that finished first. Yield instead and let");
            writer.line("        // `smelt_promise_race` take exactly one timer step per round.");
            writer.line("        if delay_ms != 0 || fired_any || SMELT_RACE_DEPTH.with(::std::cell::Cell::get) > 0 { break 'idle; }");
            writer.line("        let tasks_pending = SMELT_PROMISE_TASKS.with(|tasks| !tasks.borrow().is_empty());");
            writer.line("        if tasks_pending { break 'idle; }");
            writer.line("        let earliest = SMELT_TIMERS.with(|timers| timers.borrow().iter().filter(|timer| timer.id < id_barrier).map(|timer| timer.due_ms).min());");
            writer.line("        let Some(earliest) = earliest else { break 'idle; };");
            writer.line("        smelt_virtual_advance_to(earliest);");
            writer.line(format!(
                "        {drain_due_timers}(id_barrier);",
                drain_due_timers = smelt_stdlib::runtime_symbols::timers::DRAIN_DUE_TIMERS,
            ));
            writer.line(format!(
                "        {drain_promise_tasks}().await;",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            writer.line("    }");
            writer.line(format!(
                "    {drain_promise_tasks}().await;",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            // The trailing cooperative yield keeps a spin-waiting caller from
            // monopolising the executor. Under a `Promise.race` driver it instead
            // costs a whole extra round to observe a settled racer: the spin loop
            // already yielded once this iteration, so this second suspension lands
            // BEFORE the re-check of the result cell, and the driver would take
            // another timer step in the meantime — settling a later racer and
            // handing it the win. The driver is the scheduler while it runs, so it
            // supplies the yield itself.
            writer.line("    if SMELT_RACE_DEPTH.with(::std::cell::Cell::get) == 0 { tokio::task::yield_now().await; }");
            writer.line("}");
            writer.blank_line();
            // `Promise.race` on the virtual clock. Every generated promise value
            // is a spin-loop future that, when polled, advances virtual time by
            // at most one timer step and then yields. So within a single poll
            // round each racer can fire its own timer, and two racers whose
            // timers are due at different virtual instants both become settled
            // before anyone observes either. `tokio::select!` picks a branch in
            // randomized order and returns the first one that reports `Ready`,
            // which made the winner a coin flip: `withTimeout(() => delay(1000),
            // 50)` resolved with the 1000 ms work about half the time instead of
            // rejecting with the 50 ms timeout.
            //
            // Polling the racers in source order fixes that without touching the
            // clock: a racer settles only on the poll AFTER the step that fired
            // its timer, so the racer whose timer was due earlier is always
            // `Ready` in an earlier round, and ties inside one timer instant fall
            // to the earlier-listed racer exactly as JS resolves same-tick ties by
            // registration order. Losers are dropped when the vector goes out of
            // scope, matching `select!`'s cancellation.
            writer.line(format!(
                "async fn {promise_race}<T>(mut racers: Vec<::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<T, Box<dyn std::error::Error>>>>>>) -> Result<T, Box<dyn std::error::Error>> {{",
                promise_race = smelt_stdlib::runtime_symbols::timers::PROMISE_RACE,
            ));
            // The depth guard is a `Drop` type so the count is restored on every
            // exit path, including the early `return` that a settled racer takes.
            writer.line("    struct SmeltRaceGuard;");
            writer.line("    impl Drop for SmeltRaceGuard { fn drop(&mut self) { SMELT_RACE_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1))); } }");
            writer.line("    SMELT_RACE_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));");
            writer.line("    let _smelt_race_guard = SmeltRaceGuard;");
            writer.line("    loop {");
            writer.line(format!(
                "        let waker = {noop_waker}();",
                noop_waker = smelt_stdlib::runtime_symbols::timers::NOOP_WAKER,
            ));
            writer.line("        let mut cx = ::std::task::Context::from_waker(&waker);");
            writer.line("        for racer in racers.iter_mut() {");
            writer.line("            if let ::std::task::Poll::Ready(result) = ::std::future::Future::poll(racer.as_mut(), &mut cx) { return result; }");
            writer.line("        }");
            writer.line(format!(
                "        {drain_promise_tasks}().await;",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            // Every racer is pending, so the event loop is idle: take exactly ONE
            // timer step — advance to the earliest pending due time and fire the
            // timers due at that instant — then re-poll everyone. Timers scheduled
            // by those callbacks are held back by the id barrier so they run on the
            // next round, the same deferral a zero-delay sleep applies.
            writer.line("        let id_barrier = SMELT_NEXT_TIMER_ID.with(::std::cell::Cell::get);");
            writer.line("        let earliest = SMELT_TIMERS.with(|timers| timers.borrow().iter().filter(|timer| timer.id < id_barrier).map(|timer| timer.due_ms).min());");
            writer.line("        if let Some(earliest) = earliest {");
            writer.line("            smelt_virtual_advance_to(earliest);");
            writer.line(format!(
                "            {drain_due_timers}(id_barrier);",
                drain_due_timers = smelt_stdlib::runtime_symbols::timers::DRAIN_DUE_TIMERS,
            ));
            writer.line(format!(
                "            {drain_promise_tasks}().await;",
                drain_promise_tasks = smelt_stdlib::runtime_symbols::timers::DRAIN_PROMISE_TASKS,
            ));
            writer.line("        }");
            writer.line("        tokio::task::yield_now().await;");
            writer.line("    }");
            writer.line("}");
            writer.blank_line();
        }
        writer.block("impl SmeltUnknown", |impl_writer| {
            impl_writer.line("/// Returns the JavaScript-style length for unknown string, array, and object values.");
            impl_writer.block("pub fn len(&self) -> usize", |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::String(value) => value.chars().count(),");
                    match_writer.line("Self::Array(value) => value.len(),");
                    match_writer.line("Self::Object(value) => value.len(),");
                    match_writer.line("Self::Null | Self::Undefined | Self::Bool(_) | Self::Number(_) | Self::Symbol(_) | Self::Function(_) | Self::Promise(_) => 0,");
                });
            });
            impl_writer.line("/// Return a JavaScript-like weekday for erased Date-compatible numeric timestamps.");
            impl_writer.block("pub fn get_day(&self) -> f64", |fn_writer| {
                fn_writer.line("let timestamp_ms = match self { Self::Number(value) => *value, Self::Object(value) => match value.get(\"__smelt_date\") { Some(Self::Number(value)) => value, _ => return f64::NAN }, _ => return f64::NAN };");
                fn_writer.line("let days_since_epoch = (timestamp_ms / 86_400_000.0).floor() as i64;");
                fn_writer.line("((days_since_epoch + 4).rem_euclid(7)) as f64");
            });
            impl_writer.line("/// Return JavaScript Date.toISOString output for erased Date-compatible values.");
            impl_writer.block("pub fn to_iso_string(&self) -> String", |fn_writer| {
                fn_writer.line("if let Self::Object(value) = self {");
                fn_writer.line("    if let (Some(Self::Number(timestamp_ms)), Some(Self::String(timezone_name))) = (value.get(\"__smelt_date\"), value.get(\"__smelt_timezone\")) {");
                fn_writer.line("        if let (Some(mut local), Ok(timezone)) = (chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map(|date| date.naive_utc()), timezone_name.parse::<chrono_tz::Tz>()) {");
                fn_writer.line("            loop {");
                fn_writer.line("                match chrono::TimeZone::from_local_datetime(&timezone, &local) {");
                fn_writer.line("                    chrono::LocalResult::Single(date) => return date.to_rfc3339_opts(chrono::SecondsFormat::Millis, false),");
                fn_writer.line("                    chrono::LocalResult::Ambiguous(first, _) => return first.to_rfc3339_opts(chrono::SecondsFormat::Millis, false),");
                fn_writer.line("                    chrono::LocalResult::None => local += chrono::Duration::minutes(1),");
                fn_writer.line("                }");
                fn_writer.line("            }");
                fn_writer.line("        }");
                fn_writer.line("    }");
                fn_writer.line("}");
                fn_writer.line("let timestamp_ms = match self {");
                fn_writer.line("    Self::Number(value) => *value,");
                fn_writer.line("    Self::Object(value) => match value.get(\"__smelt_date\") { Some(Self::Number(value)) => value, _ => f64::NAN },");
                fn_writer.line("    Self::String(value) => value.parse::<f64>().unwrap_or(f64::NAN),");
                fn_writer.line("    Self::Bool(value) => if *value { 1.0 } else { 0.0 },");
                fn_writer.line("    Self::Null | Self::Undefined | Self::Symbol(_) | Self::Array(_) | Self::Function(_) | Self::Promise(_) => f64::NAN,");
                fn_writer.line("};");
                fn_writer.line("chrono::DateTime::<chrono::Utc>::from_timestamp_millis(timestamp_ms as i64).map(|date| date.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_else(|| \"Invalid Date\".to_owned())");
            });
            impl_writer.line("/// Return JavaScript-like `includes` membership for erased strings and arrays.");
            impl_writer.block(
                "pub fn includes<T: IntoSmeltUnknown>(&self, needle: T) -> bool",
                |fn_writer| {
                    fn_writer.line("let needle = needle.into_smelt_unknown();");
                    fn_writer.block("match (self, needle)", |match_writer| {
                        match_writer.line("(Self::String(haystack), Self::String(needle)) => haystack.contains(&*needle),");
                        match_writer.line("(Self::Array(values), needle) => values.iter().any(|value| value == needle),");
                        match_writer.line("_ => false,");
                    });
                },
            );
            impl_writer.line("/// Return JavaScript-like RegExp.test behavior for erased regex-like values.");
            impl_writer.block(
                "pub fn test<T: IntoSmeltUnknown>(&self, haystack: T) -> bool",
                |fn_writer| {
                    fn_writer.line("let SmeltUnknown::String(haystack) = haystack.into_smelt_unknown() else { return false; };");
                    fn_writer.block("match self", |match_writer| {
                        match_writer.line("Self::String(pattern) => regex::Regex::new(pattern).is_ok_and(|regex| regex.is_match(&haystack)),");
                        match_writer.line("Self::Object(map) => {");
                        match_writer.line("    let Some(Self::String(source)) = map.get(\"source\") else { return false; };");
                        match_writer.line("    let flags = match map.get(\"flags\") { Some(Self::String(flags)) => flags.to_string(), _ => String::new() };");
                        match_writer.line("    SmeltRegExp::new(source.to_string(), flags).test(&haystack)");
                        match_writer.line("}");
                        match_writer.line("_ => false,");
                    });
                },
            );
            impl_writer.line("/// No-op compatibility hook for erased callable objects with a `flush` method.");
            impl_writer.block("pub fn flush(&self)", |_fn_writer| {});
        });
        writer.blank_line();
        writer.block("impl ::std::fmt::Display for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result",
                |fn_writer| {
                    fn_writer.block("match self", |match_writer| {
                        match_writer.line("Self::Null => formatter.write_str(\"null\"),");
                        match_writer.line("Self::Undefined => formatter.write_str(\"undefined\"),");
                        match_writer.line("Self::Bool(value) => write!(formatter, \"{value}\"),");
                        match_writer.line("Self::Number(value) => write!(formatter, \"{value}\"),");
                        match_writer.line("Self::String(value) => formatter.write_str(value),");
                        match_writer.line("Self::Symbol(value) => formatter.write_str(value),");
                        match_writer.line("Self::Array(_) | Self::Object(_) => formatter.write_str(\"[object Object]\"),");
                        match_writer.line("Self::Function(_) => formatter.write_str(\"function () { [native code] }\"),");
                        match_writer.line("Self::Promise(_) => formatter.write_str(\"[object Promise]\"),");
                    });
                },
            );
        });
        // The exception-payload ABI sits right after `Display for SmeltUnknown`
        // because `SmeltThrown`'s own `Display` falls back to it for non-error
        // payloads.
        thrown::emit_thrown_payload_support(&mut writer);
        writer.blank_line();
        writer.line("impl Eq for SmeltUnknown {}");
        writer.blank_line();
        writer.block("impl ::std::hash::Hash for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn hash<H: ::std::hash::Hasher>(&self, state: &mut H)",
                |fn_writer| {
                    fn_writer.line("let mut smelt_seen = ::std::collections::HashSet::new();");
                    fn_writer.line("smelt_unknown_structural_hash(self, state, &mut smelt_seen);");
                },
            );
        });
        writer.blank_line();
        writer.block("impl PartialOrd for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn partial_cmp(&self, other: &Self) -> Option<::std::cmp::Ordering>",
                |fn_writer| {
                    fn_writer.block("match (self, other)", |match_writer| {
                        match_writer.line("(Self::Number(left), Self::Number(right)) => left.partial_cmp(right),");
                        match_writer.line("(Self::String(left), Self::String(right)) => Some(left.cmp(right)),");
                        match_writer.line("(Self::Bool(left), Self::Bool(right)) => Some(left.cmp(right)),");
                        match_writer.line("(Self::Object(left), Self::Object(right)) if left.contains_key(\"__smelt_date\") || right.contains_key(\"__smelt_date\") => smelt_unknown_date_value(self).partial_cmp(&smelt_unknown_date_value(other)),");
                        match_writer.line("(Self::Null, Self::Null) => Some(::std::cmp::Ordering::Equal),");
                        match_writer.line("(left, right) => Some(smelt_unknown_rank(left).cmp(&smelt_unknown_rank(right))),");
                    });
                },
            );
        });
        writer.blank_line();
        writer.block("impl PartialEq<f64> for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn eq(&self, other: &f64) -> bool", |fn_writer| {
                fn_writer.line("matches!(self, Self::Number(value) if value == other)");
            });
        });
        writer.blank_line();
        writer.block("impl PartialEq<SmeltUnknown> for f64", |impl_writer| {
            impl_writer.block("fn eq(&self, other: &SmeltUnknown) -> bool", |fn_writer| {
                fn_writer.line("other == self");
            });
        });
        writer.blank_line();
        writer.block("impl PartialOrd<f64> for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn partial_cmp(&self, other: &f64) -> Option<::std::cmp::Ordering>",
                |fn_writer| {
                    fn_writer.block("match self", |match_writer| {
                        match_writer.line("Self::Number(value) => value.partial_cmp(other),");
                        match_writer.line("Self::String(value) => value.parse::<f64>().ok().and_then(|number| number.partial_cmp(other)),");
                        match_writer.line("Self::Bool(value) => (if *value { 1.0 } else { 0.0 }).partial_cmp(other),");
                        match_writer.line("Self::Null | Self::Undefined | Self::Symbol(_) | Self::Array(_) | Self::Object(_) | Self::Function(_) | Self::Promise(_) => None,");
                    });
                },
            );
        });
        writer.blank_line();
        writer.block("impl PartialOrd<SmeltUnknown> for f64", |impl_writer| {
            impl_writer.block(
                "fn partial_cmp(&self, other: &SmeltUnknown) -> Option<::std::cmp::Ordering>",
                |fn_writer| {
                    fn_writer.line("other.partial_cmp(self).map(::std::cmp::Ordering::reverse)");
                },
            );
        });
        writer.blank_line();
        writer.block(
            "impl PartialEq<SmeltUnknown> for Option<SmeltUnknown>",
            |impl_writer| {
                impl_writer.block("fn eq(&self, other: &SmeltUnknown) -> bool", |fn_writer| {
                    fn_writer.line("self.as_ref().is_some_and(|value| value == other)");
                });
            },
        );
        writer.blank_line();
        writer.block(
            "impl PartialOrd<SmeltUnknown> for Option<SmeltUnknown>",
            |impl_writer| {
                impl_writer.block(
                    "fn partial_cmp(&self, other: &SmeltUnknown) -> Option<::std::cmp::Ordering>",
                    |fn_writer| {
                        fn_writer.line("self.as_ref().and_then(|value| value.partial_cmp(other))");
                    },
                );
            },
        );
        writer.blank_line();
        writer.block(
            "fn smelt_unknown_date_value(value: &SmeltUnknown) -> f64",
            |fn_writer| {
                fn_writer.block("match value", |match_writer| {
                    match_writer.line("SmeltUnknown::Object(object) => match object.get(\"__smelt_date\") { Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN },");
                    match_writer.line("_ => f64::NAN,");
                });
            },
        );
        writer.blank_line();
        writer.block(
            "fn smelt_unknown_rank(value: &SmeltUnknown) -> u8",
            |fn_writer| {
                fn_writer.block("match value", |match_writer| {
                    match_writer.line("SmeltUnknown::Null => 0,");
                    match_writer.line("SmeltUnknown::Undefined => 8,");
                    match_writer.line("SmeltUnknown::Bool(_) => 1,");
                    match_writer.line("SmeltUnknown::Number(_) => 2,");
                    match_writer.line("SmeltUnknown::String(_) => 3,");
                    match_writer.line("SmeltUnknown::Symbol(_) => 4,");
                    match_writer.line("SmeltUnknown::Array(_) => 5,");
                    match_writer.line("SmeltUnknown::Object(_) => 6,");
                    match_writer.line("SmeltUnknown::Function(_) => 7,");
                    match_writer.line("SmeltUnknown::Promise(_) => 9,");
                });
            },
        );
        writer.blank_line();
        // JavaScript's Abstract Relational Comparison (`<`, `<=`, `>`, `>=`) for
        // two erased values: when *both* operands are strings it compares them
        // lexically (byte order over the UTF-8 encoding, matching JS's UTF-16
        // code-unit order for the BMP), otherwise it runs `ToNumber` on both and
        // compares as `f64`. A `NaN` on either side yields `None` (an unordered
        // result), so every relational operator reports `false` — exactly the JS
        // outcome. Codegen routes the both-erased relational arms here so a
        // runtime string comparison stays lexical instead of collapsing to
        // `NaN`-vs-`NaN` under blind numeric coercion.
        writer.block(
            "fn smelt_unknown_js_relational_ordering(left: &SmeltUnknown, right: &SmeltUnknown) -> Option<::std::cmp::Ordering>",
            |fn_writer| {
                fn_writer.block("match (left, right)", |match_writer| {
                    match_writer.line("(SmeltUnknown::String(left), SmeltUnknown::String(right)) => Some(left.cmp(right)),");
                    match_writer.line("_ => smelt_unknown_to_number(left).partial_cmp(&smelt_unknown_to_number(right)),");
                });
            },
        );
        writer.blank_line();
        // `ToNumber` for an erased value, mirroring the inline coercion codegen
        // emits when a `SmeltUnknown` flows into a numeric context: numeric
        // strings parse to their value, non-numeric strings become `NaN`, booleans
        // map to `0`/`1`, `__smelt_date` objects surface their timestamp, and
        // every remaining shape is `NaN`.
        writer.block(
            "fn smelt_unknown_to_number(value: &SmeltUnknown) -> f64",
            |fn_writer| {
                fn_writer.block("match value", |match_writer| {
                    match_writer.line("SmeltUnknown::Number(value) => *value,");
                    match_writer.line("SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") { Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN },");
                    match_writer.line("SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN),");
                    match_writer.line("SmeltUnknown::Bool(value) => if *value { 1.0 } else { 0.0 },");
                    match_writer.line("SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN,");
                });
            },
        );
        writer.blank_line();
        writer.block("pub trait IntoSmeltUnknown", |trait_writer| {
            trait_writer.line("fn into_smelt_unknown(self) -> SmeltUnknown;");
        });
        writer.blank_line();
        // A callback parameter passed by shared reference (see
        // `callback_param_is_shared_reference`) is still erased back into a JS array
        // by the erased-callback adapters, and that walk yields `&SmeltUnknown`
        // rather than an owned value. Erasing a borrowed value clones it, which the
        // erasure would have done anyway when it built the `SmeltArray`; the saving
        // is on the ARGUMENT, which no longer deep-copies the list per element.
        writer.line("impl IntoSmeltUnknown for &SmeltUnknown { fn into_smelt_unknown(self) -> SmeltUnknown { self.clone() } }");
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("self");
            });
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for bool", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::Bool(self)");
            });
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for f64", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::Number(self)");
            });
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for i64", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::Number(self as f64)");
            });
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for String", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::String(self.into())");
            });
        });
        writer.blank_line();
        writer.block("impl IntoSmeltUnknown for ()", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::Null");
            });
        });
        writer.blank_line();
        // Erasing a typed list yields an identity-bearing `SmeltUnknown::Array`,
        // carrying the list's reference id and erasing each element in turn.
        writer.line("impl<T: IntoSmeltUnknown + Clone> IntoSmeltUnknown for SmeltList<T> { fn into_smelt_unknown(self) -> SmeltUnknown { SmeltUnknown::Array(SmeltArray::with_id(self.id(), self.into_vec().into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect())) } }");
        writer.blank_line();
        writer.block("pub trait SmeltFromUnknown", |trait_writer| {
            trait_writer.line("fn smelt_from_unknown(value: SmeltUnknown) -> Self;");
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for SmeltUnknown", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("value");
                },
            );
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for bool", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("match value { SmeltUnknown::Null | SmeltUnknown::Undefined => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => true }");
                },
            );
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for f64", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("match value { SmeltUnknown::Number(value) => value, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") { Some(SmeltUnknown::Number(value)) => value, _ => f64::NAN }, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value { 1.0 } else { 0.0 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => f64::NAN }");
                },
            );
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for i64", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("match value { SmeltUnknown::Number(value) => value as i64, SmeltUnknown::Object(value) => match value.get(\"__smelt_date\") { Some(SmeltUnknown::Number(value)) => value as i64, _ => 0_i64 }, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN) as i64, SmeltUnknown::Bool(value) => if value { 1_i64 } else { 0_i64 }, SmeltUnknown::Null | SmeltUnknown::Undefined | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Function(_) | SmeltUnknown::Promise(_) => 0_i64 }");
                },
            );
        });
        writer.blank_line();
        writer.block("impl SmeltFromUnknown for String", |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("match value { SmeltUnknown::String(value) | SmeltUnknown::Symbol(value) => value.to_string(), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => String::new(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }");
                },
            );
        });
        writer.blank_line();
        // Un-erasing a `SmeltUnknown` back into statically typed collections is the
        // mirror of the `IntoSmeltUnknown` impls above: a returned generic value
        // (e.g. `identity<T>(x): T` inferred at `SmeltRecord<String, String>`) is
        // erased to `SmeltUnknown` inside the generic body and must be converted
        // back element-wise at the typed binding site. Each impl reuses the source
        // container's reference id so round-tripping preserves JS identity, and
        // converts each element/entry through the element type's own
        // `SmeltFromUnknown`. A non-matching `SmeltUnknown` variant yields an empty
        // container (the JS-coercion fallback, matching the scalar impls above).
        writer.line("impl<T: SmeltFromUnknown> SmeltFromUnknown for SmeltList<T> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Array(array) => SmeltList::with_id(array.id, array.into_vec().into_iter().map(T::smelt_from_unknown).collect()), _ => SmeltList::new(Vec::new()) } } }");
        writer.blank_line();
        writer.line("impl<K: SmeltFromUnknown + Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, V: SmeltFromUnknown + Clone> SmeltFromUnknown for SmeltRecord<K, V> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Object(object) => SmeltRecord::with_id_from_entries(object.id, object.iter().map(|(key, value)| (K::smelt_from_unknown(SmeltUnknown::String(key.into())), V::smelt_from_unknown(value)))), _ => SmeltRecord::with_id_from_entries(smelt_next_object_id(), ::std::iter::empty()) } } }");
        writer.blank_line();
        // Un-erase a `Map`. A `__smelt_map` marker object restores the original
        // entries (from the `[[k, v], ...]` pair array) and the source `id`, so the
        // erasure round-trip preserves JS identity. A plain object (no marker) still
        // decodes as string-keyed entries — the "Map and Record share Dict
        // internally" tolerance — and any other value yields an empty map.
        writer.line("impl<K: SmeltFromUnknown + SmeltJsKeyEq + Clone, V: SmeltFromUnknown + Clone> SmeltFromUnknown for SmeltJsMap<K, V> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Object(object) => { if let Some(SmeltUnknown::Array(pairs)) = object.get(\"__smelt_map\") { let mut map = SmeltJsMap { id: object.id, store: ::std::rc::Rc::new(::std::cell::RefCell::new(SmeltJsMapStore::new())) }; for pair in pairs.into_vec() { if let SmeltUnknown::Array(entry) = pair { let mut entry = entry.into_vec().into_iter(); if let (Some(key), Some(value)) = (entry.next(), entry.next()) { map.insert(K::smelt_from_unknown(key), V::smelt_from_unknown(value)); } } } map } else { object.iter().map(|(key, value)| (K::smelt_from_unknown(SmeltUnknown::String(key.into())), V::smelt_from_unknown(value))).collect() } }, _ => SmeltJsMap::default() } } }");
        writer.blank_line();
        // Un-erase a `Set`. A `__smelt_set` marker object restores the original
        // members (from the members array) and the source `id`, so the erasure
        // round-trip preserves JS identity — mirrors the `SmeltJsMap` decode. The
        // bare-`Array` arm is the tolerant back-compat boundary: an erased value
        // that is a plain array (e.g. produced outside this stage's marker path,
        // or a genuine dynamic-interop array coerced to a `Set`) still decodes as
        // set members via SameValueZero insert. Any other value yields an empty set.
        writer.line("impl<T: SmeltFromUnknown> SmeltFromUnknown for Option<T> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Null | SmeltUnknown::Undefined => None, other => Some(T::smelt_from_unknown(other)) } } }");
        writer.line("impl<T: SmeltFromUnknown + Clone + IntoSmeltUnknown> SmeltFromUnknown for SmeltJsSet<T> { fn smelt_from_unknown(value: SmeltUnknown) -> Self { match value { SmeltUnknown::Object(object) => { if let Some(SmeltUnknown::Array(members)) = object.get(\"__smelt_set\") { let mut set = SmeltJsSet::with_id(object.id); for member in members.into_vec() { set.insert(T::smelt_from_unknown(member)); } set } else { SmeltJsSet::default() } }, SmeltUnknown::Array(members) => { let mut set = SmeltJsSet::new(); for member in members.into_vec() { set.insert(T::smelt_from_unknown(member)); } set }, _ => SmeltJsSet::default() } } }");
        writer.blank_line();
        writer.block("trait SmeltIntoF64", |trait_writer| {
            trait_writer.line("fn smelt_into_f64(self) -> f64;");
        });
        writer.blank_line();
        writer.block("impl SmeltIntoF64 for f64", |impl_writer| {
            impl_writer.block("fn smelt_into_f64(self) -> f64", |fn_writer| {
                fn_writer.line("self");
            });
        });
        writer.blank_line();
        writer.block("impl SmeltIntoF64 for SmeltUnknown", |impl_writer| {
            impl_writer.block("fn smelt_into_f64(self) -> f64", |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("SmeltUnknown::Number(value) => value,");
                    match_writer.line("_ => 0.0,");
                });
            });
        });
        writer.blank_line();
        writer.block(
            "impl<T: IntoSmeltUnknown> IntoSmeltUnknown for Option<T>",
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line(
                        // An absent optional erases to JS `undefined` (missing
                        // element / optional / `?.`), distinct from explicit `null`.
                        "self.map_or(SmeltUnknown::Undefined, IntoSmeltUnknown::into_smelt_unknown)",
                    );
                });
            },
        );
        writer.blank_line();
        writer.block("impl<T: IntoSmeltUnknown> IntoSmeltUnknown for Vec<T>", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("SmeltUnknown::Array(self.into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect())");
            });
        });
        writer.blank_line();
        writer.block(
            "impl<T: IntoSmeltUnknown + Eq + ::std::hash::Hash> IntoSmeltUnknown for ::std::collections::HashSet<T>",
            |impl_writer| {
                impl_writer.line("/// Erase JavaScript Set values as iterable array-like values.");
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line("let mut values = self.into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<Vec<_>>();");
                    fn_writer.line("values.sort_by_key(smelt_unknown_stable_hash_key);");
                    fn_writer.line("SmeltUnknown::Array(values.into())");
                });
            },
        );
        writer.blank_line();
        writer.block("trait SmeltArrayExt<T>", |trait_writer| {
            trait_writer
                .line("/// Return the first JavaScript-style index of a value, or -1 when absent.");
            trait_writer.line("fn index_of(&self, needle: T) -> SmeltUnknown where T: PartialEq;");
        });
        writer.blank_line();
        writer.block("impl<T> SmeltArrayExt<T> for Vec<T>", |impl_writer| {
            impl_writer.line("/// Implement `Array.prototype.indexOf` for generated Rust vectors.");
            impl_writer.block("fn index_of(&self, needle: T) -> SmeltUnknown where T: PartialEq", |fn_writer| {
                fn_writer.line("self.iter().position(|value| value == &needle).map_or(SmeltUnknown::Number(-1.0), |index| SmeltUnknown::Number(index as f64))");
            });
        });
        writer.blank_line();
        writer.block("trait SmeltUnknownArrayExt", |trait_writer| {
            trait_writer
                .line("/// Concatenate erased array values with JavaScript-style spreading.");
            trait_writer.line("fn concat(self, other: Vec<SmeltUnknown>) -> Vec<SmeltUnknown>;");
        });
        writer.blank_line();
        writer.block(
            "impl SmeltUnknownArrayExt for Vec<SmeltUnknown>",
            |impl_writer| {
                impl_writer.line(
                    "/// Implement the generated rest-argument concat helper for erased arrays.",
                );
                impl_writer.block(
                    "fn concat(mut self, other: Vec<SmeltUnknown>) -> Vec<SmeltUnknown>",
                    |fn_writer| {
                        fn_writer.line("self.extend(other);");
                        fn_writer.line("self");
                    },
                );
            },
        );
        writer.blank_line();
        writer.block("trait SmeltUnitExt", |trait_writer| {
            trait_writer.line(
                "/// Return false for generated calls against unreachable unit placeholders.",
            );
            trait_writer.line("fn includes<T>(&self, _needle: T) -> bool;");
        });
        writer.blank_line();
        writer.block("impl SmeltUnitExt for ()", |impl_writer| {
            impl_writer.line("/// Keep unreachable JavaScript membership calls type-checkable.");
            impl_writer.block("fn includes<T>(&self, _needle: T) -> bool", |fn_writer| {
                fn_writer.line("false");
            });
        });
        writer.blank_line();
        writer.block(
            "impl<A: IntoSmeltUnknown, B: IntoSmeltUnknown> IntoSmeltUnknown for (A, B)",
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line(
                        "SmeltUnknown::Array(vec![self.0.into_smelt_unknown(), self.1.into_smelt_unknown()].into())",
                    );
                });
            },
        );
        writer.blank_line();
        writer.block(
            "impl<K, T> IntoSmeltUnknown for ::std::collections::HashMap<K, T> where K: IntoSmeltUnknown + Eq + ::std::hash::Hash, T: IntoSmeltUnknown",
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line("SmeltUnknown::Object(SmeltObject::new(self.into_iter().map(|(key, value)| { let key = match key.into_smelt_unknown() { SmeltUnknown::String(value) => value.to_string(), SmeltUnknown::Symbol(value) => smelt_symbol_property_key(&value), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => \"null\".to_owned(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }; (key, value.into_smelt_unknown()) }).collect()))");
                });
            },
        );
        writer.blank_line();
        writer.block(
            "impl<K, T> IntoSmeltUnknown for SmeltRecord<K, T> where K: IntoSmeltUnknown + Eq + ::std::hash::Hash + Clone + SmeltPropertyKey, T: IntoSmeltUnknown + Clone",
            |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line("SmeltUnknown::Object(SmeltObject::with_id(self.id, self.iter().into_iter().map(|(key, value)| { let key = match key.into_smelt_unknown() { SmeltUnknown::String(value) => value.to_string(), SmeltUnknown::Symbol(value) => smelt_symbol_property_key(&value), SmeltUnknown::Number(value) => value.to_string(), SmeltUnknown::Bool(value) => value.to_string(), SmeltUnknown::Null => \"null\".to_owned(), SmeltUnknown::Undefined => \"undefined\".to_owned(), SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"[object Object]\".to_owned(), SmeltUnknown::Function(_) => \"function () { [native code] }\".to_owned(), SmeltUnknown::Promise(_) => \"[object Promise]\".to_owned() }; (key, value.into_smelt_unknown()) }).collect()))");
                });
            },
        );
        writer.blank_line();
        if needs_serde_json {
            emit_unknown_serde_impls(&mut writer);
        }
    }

    let mut emitted_class_names = HashSet::new();
    for interface in &mir.interfaces {
        let name = RustIdent::new(
            mir.symbols
                .get(interface.name)
                .ok_or_else(|| EmitError::new("interface has unknown symbol"))?,
        )
        .into_string();
        if !emitted_class_names.insert(name.clone()) {
            continue;
        }
        let type_params = interface_type_params_text(mir, interface)?;
        let impl_generics = interface_impl_generics_text(mir, interface)?;
        let fields = effective_interface_fields(mir, interface);
        // A shape whose fields are written after construction is a reference
        // record: it needs the shared-cell handle so aliases observe the write,
        // exactly as a mutated class does. See `classify::reference_classes`.
        if context.is_reference_class(interface.name) {
            emit_reference_record_storage(
                &mut writer,
                mir,
                &context,
                &ReferenceRecordShape {
                    name,
                    type_params: type_params.clone(),
                    type_args: type_params,
                    impl_generics,
                    fields,
                    static_fields: &[],
                    type_param_names: interface
                        .type_params
                        .iter()
                        .map(|param| param.name)
                        .collect(),
                    // An object shape has no method bodies to bind.
                    has_proto_entries: false,
                },
                needs_unknown,
            )?;
            continue;
        }
        let has_function_field = fields
            .iter()
            .any(|field| type_contains_function(mir, field.ty));
        // An interface-backed record is a by-value struct, exactly like a value
        // class, so it supports structural equality (JS `==`/`===`/`toBe`, and
        // derived comparisons in generated specs) under the same rule: every
        // stored field must itself be `PartialEq`. Before shape structs existed
        // this rarely mattered, because a comparable record was usually spelled
        // as a dict; a statically-shaped object literal now lands here instead,
        // and comparing two of them must keep working.
        let interface_supports_partial_eq = fields
            .iter()
            .all(|field| type_supports_partial_eq(mir, &context, field.ty, &mut Vec::new()));
        let interface_partial_eq_derive = if interface_supports_partial_eq {
            ", PartialEq"
        } else {
            ""
        };
        if has_function_field {
            writer.line("#[derive(Clone)]");
            writer.line("#[allow(dead_code)]");
        } else if needs_serde_json && interface_is_json_serializable(mir, interface) {
            writer.line(format!(
                "#[derive(Clone, Debug, Default{interface_partial_eq_derive}, serde::Serialize, serde::Deserialize)]"
            ));
        } else {
            writer.line(format!(
                "#[derive(Clone, Debug, Default{interface_partial_eq_derive})]"
            ));
        }
        let phantom_args = interface
            .type_params
            .iter()
            .map(|param| {
                mir.symbols
                    .get(param.name)
                    .map(|param_name| RustIdent::new(param_name).into_string())
                    .ok_or_else(|| EmitError::new("interface type parameter has unknown symbol"))
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let scoped_type_params = interface
            .type_params
            .iter()
            .map(|param| param.name)
            .collect::<HashSet<_>>();
        writer.block(format!("struct {name}{type_params}"), |block_writer| {
            for field in &fields {
                let field_name = RustIdent::new(
                    mir.symbols
                        .get(field.name)
                        .ok_or_else(|| EmitError::new("field has unknown symbol"))
                        .unwrap_or("field"),
                )
                .into_string();
                let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
                    mir,
                    &context,
                    field.ty,
                    &TypeSubstitution::lexical(&scoped_type_params),
                )
                .unwrap_or_else(|_| "SmeltUnknown".to_owned());
                block_writer.line(format!("{field_name}: {field_ty},"));
            }
            if !interface.type_params.is_empty() {
                block_writer.line(format!(
                    "_smelt_phantom: ::std::marker::PhantomData<({phantom_args})>,"
                ));
            }
        });
        if has_function_field {
            emit_default_impl_for_storage_type(
                &mut writer,
                mir,
                &context,
                &name,
                &impl_generics,
                &type_params,
                &fields,
                &phantom_args,
                &scoped_type_params,
            )?;
            emit_debug_impl_for_storage_type(&mut writer, &name, &impl_generics, &type_params);
        }
        if needs_unknown {
            emit_record_into_smelt_unknown_impl(
                &mut writer,
                mir,
                &name,
                &impl_generics,
                &type_params,
                &fields,
            )?;
            emit_record_from_smelt_unknown_impl(
                &mut writer,
                mir,
                &name,
                &impl_generics,
                &type_params,
                &fields,
            )?;
        }
        writer.blank_line();
    }
    if needs_regex {
        // Compiled-automaton memo, keyed by the translated Rust pattern text.
        //
        // A JavaScript `RegExp` object is constructed once (typically at module
        // evaluation, from a literal) and reused; the generated equivalent
        // rebuilds a `SmeltRegExp` wrapper wherever the value is referenced,
        // because `SmeltRegExp` carries observable mutable state (`lastIndex`)
        // and a per-object identity that must NOT be shared between distinct
        // source objects. The expensive half — compiling the pattern into a
        // `fancy_regex::Regex` — is pure and depends only on the pattern text,
        // so it is memoized here instead. Each wrapper therefore stays a fresh,
        // independently-mutable value while the automaton for a given pattern is
        // built at most once per thread. `Rc` (not `Arc`) matches the rest of the
        // single-threaded runtime, and the per-thread cell keeps each `#[test]`
        // thread independent. `None` is cached too, so an invalid pattern is not
        // recompiled on every call either.
        //
        // The map is unbounded: it retains one compiled automaton per distinct
        // pattern the thread has ever built. That is exactly one entry per regex
        // literal in the program, which is what the source itself would have kept
        // alive at module scope. A program that builds regexes from *dynamic*
        // strings (`new RegExp(input)`) instead retains one entry per distinct
        // input, which JavaScript would not — an eviction policy belongs here if
        // that shape ever shows up in a corpus, but no arbitrary cap is imposed
        // before there is a case to size it against.
        writer.line("thread_local! { static SMELT_REGEX_CACHE: ::std::cell::RefCell<::std::collections::HashMap<String, ::std::option::Option<::std::rc::Rc<fancy_regex::Regex>>>> = ::std::cell::RefCell::new(::std::collections::HashMap::new()); }");
        writer.blank_line();
        writer.line("#[derive(Clone, Debug)]");
        writer.block("pub struct SmeltRegExp", |struct_writer| {
            struct_writer.line("id: usize,");
            struct_writer.line("source: String,");
            struct_writer.line("flags: String,");
            struct_writer.line("last_index: ::std::rc::Rc<::std::cell::RefCell<usize>>,");
        });
        writer.blank_line();
        writer.line("impl PartialEq for SmeltRegExp { fn eq(&self, other: &Self) -> bool { self.source == other.source && self.flags == other.flags && *self.last_index.borrow() == *other.last_index.borrow() } }");
        writer.blank_line();
        writer.block("impl SmeltRegExp", |impl_writer| {
            impl_writer.line("/// Construct a JavaScript-like RegExp value with shared lastIndex state.");
            impl_writer.block("pub fn new(source: String, flags: String) -> Self", |fn_writer| {
                fn_writer.line("Self { id: smelt_next_object_id(), source, flags, last_index: ::std::rc::Rc::new(::std::cell::RefCell::new(0)) }");
            });
            impl_writer.line("/// Return true when this RegExp has a flag.");
            impl_writer.block("pub fn has_flag(&self, flag: char) -> bool", |fn_writer| {
                fn_writer.line("self.flags.chars().any(|value| value == flag)");
            });
            impl_writer.line("/// Compile the Rust regex equivalent for this JavaScript RegExp.");
            impl_writer.block("fn compiled(&self) -> ::std::rc::Rc<fancy_regex::Regex>", |fn_writer| {
                fn_writer.line("self.try_compiled().expect(\"regex compile failed\")");
            });
            impl_writer.line("/// Try to compile the Rust regex equivalent for this JavaScript RegExp.");
            impl_writer.line("///");
            impl_writer.line("/// The compiled automaton is memoized per translated pattern in");
            impl_writer.line("/// `SMELT_REGEX_CACHE`, so repeatedly constructing the same RegExp value");
            impl_writer.line("/// (for example a module-level pattern referenced from a hot function)");
            impl_writer.line("/// compiles it at most once per thread. Compilation is a pure function of");
            impl_writer.line("/// the pattern text, so sharing it is unobservable; the mutable");
            impl_writer.line("/// `lastIndex` state stays per-`SmeltRegExp`.");
            impl_writer.block("fn try_compiled(&self) -> Option<::std::rc::Rc<fancy_regex::Regex>>", |fn_writer| {
                fn_writer.line("let mut prefix = String::new();");
                fn_writer.line("if self.has_flag('i') { prefix.push('i'); }");
                fn_writer.line("if self.has_flag('m') { prefix.push('m'); }");
                fn_writer.line("if self.has_flag('s') { prefix.push('s'); }");
                fn_writer.line("let translated_source = self.source.replace(\"[^]\", \"(?s:.)\");");
                fn_writer.line("let pattern = if prefix.is_empty() { translated_source } else { format!(\"(?{prefix}){translated_source}\") };");
                fn_writer.line("if let Some(cached) = SMELT_REGEX_CACHE.with(|cache| cache.borrow().get(&pattern).cloned()) { return cached; }");
                fn_writer.line("let compiled = fancy_regex::Regex::new(&pattern).ok().map(::std::rc::Rc::new);");
                fn_writer.line("SMELT_REGEX_CACHE.with(|cache| { cache.borrow_mut().insert(pattern, compiled.clone()); });");
                fn_writer.line("compiled");
            });
            impl_writer.line("/// Match a string with JavaScript String.prototype.match semantics.");
            impl_writer.block("pub fn match_string(&self, haystack: &str) -> Option<Vec<String>>", |fn_writer| {
                fn_writer.line("let regex = self.try_compiled()?;");
                fn_writer.block("if self.has_flag('g')", |if_writer| {
                    if_writer.line("let matches = regex.find_iter(haystack).filter_map(Result::ok).map(|value| value.as_str().to_owned()).collect::<Vec<_>>();");
                    if_writer.line("if matches.is_empty() { None } else { Some(matches) }");
                });
                fn_writer.block("else", |else_writer| {
                    else_writer.line("let captures = regex.captures(haystack).ok().flatten()?;");
                    else_writer.line("Some((0..captures.len()).map(|index| captures.get(index).map_or(String::new(), |value| value.as_str().to_owned())).collect::<Vec<_>>())");
                });
            });
            impl_writer.line("/// Split a string with JavaScript RegExp separator semantics.");
            impl_writer.block("pub fn split_string(&self, haystack: &str) -> Vec<String>", |fn_writer| {
                fn_writer.line("let Some(regex) = self.try_compiled() else { return vec![haystack.to_owned()]; };");
                fn_writer.line("regex.split(haystack).filter_map(Result::ok).map(str::to_owned).collect::<Vec<_>>()");
            });
            impl_writer.line("/// Replace matches with JavaScript RegExp-aware String.prototype.replace semantics.");
            impl_writer.block("pub fn replace_string(&self, haystack: &str, replacement: &str, force_all: bool) -> String", |fn_writer| {
                fn_writer.line("let Some(regex) = self.try_compiled() else { return haystack.to_owned(); };");
                fn_writer.line("let replace_all = force_all || self.has_flag('g');");
                fn_writer.block("if replace_all", |if_writer| {
                    if_writer.line("let mut output = String::new();");
                    if_writer.line("let mut last_end = 0usize;");
                    if_writer.block("for matched in regex.find_iter(haystack).filter_map(Result::ok)", |for_writer| {
                        for_writer.line("output.push_str(&haystack[last_end..matched.start()]);");
                        for_writer.line("output.push_str(replacement);");
                        for_writer.line("last_end = matched.end();");
                    });
                    if_writer.line("output.push_str(&haystack[last_end..]);");
                    if_writer.line("output");
                });
                fn_writer.line("else if let Ok(Some(matched)) = regex.find(haystack) {");
                fn_writer.line("    format!(\"{}{}{}\", &haystack[..matched.start()], replacement, &haystack[matched.end()..])");
                fn_writer.line("} else {");
                fn_writer.line("    haystack.to_owned()");
                fn_writer.line("}");
            });
            impl_writer.line("/// Execute this RegExp and return a concrete `SmeltMatch` result.");
            impl_writer.line("///");
            impl_writer.line("/// The match result is a typed `SmeltMatch` value (numbered groups,");
            impl_writer.line("/// named groups, `index`, `input`) instead of an erased `SmeltUnknown`");
            impl_writer.line("/// property bag; callers erase it explicitly at a dynamic boundary via");
            impl_writer.line("/// `SmeltMatch::into_smelt_unknown` when required.");
            impl_writer.block("pub fn exec(&self, haystack: &str) -> Option<SmeltMatch>", |fn_writer| {
                fn_writer.line("let regex = self.compiled();");
                fn_writer.line("let start = if self.has_flag('g') || self.has_flag('y') { *self.last_index.borrow() } else { 0 };");
                fn_writer.line("let suffix = haystack.get(start..).unwrap_or(\"\");");
                fn_writer.line("let captures = regex.captures(suffix).ok().flatten()?;");
                fn_writer.line("let matched = captures.get(0)?;");
                fn_writer.line("if self.has_flag('y') && matched.start() != 0 { *self.last_index.borrow_mut() = 0; return None; }");
                fn_writer.line("if self.has_flag('g') || self.has_flag('y') { *self.last_index.borrow_mut() = start + matched.end(); }");
                fn_writer.line("Some(SmeltMatch::from_captures(&regex, &captures, start + matched.start(), haystack))");
            });
            impl_writer.line("/// Return concrete `SmeltMatch` results for String.prototype.matchAll.");
            impl_writer.block("pub fn match_all_indices(&self, haystack: &str) -> Vec<SmeltMatch>", |fn_writer| {
                fn_writer.line("let Some(regex) = self.try_compiled() else { return Vec::new(); };");
                fn_writer.block("regex.captures_iter(haystack).filter_map(Result::ok).filter_map(|captures|", |map_writer| {
                    map_writer.line("let matched = captures.get(0)?;");
                    map_writer.line("Some(SmeltMatch::from_captures(&regex, &captures, matched.start(), haystack))");
                });
                fn_writer.line(").collect::<Vec<_>>()");
            });
            impl_writer.line("/// Test this RegExp against a string with JavaScript lastIndex updates.");
            impl_writer.block("pub fn test(&self, haystack: &str) -> bool", |fn_writer| {
                fn_writer.line("self.exec(haystack).is_some()");
            });
        });
        writer.blank_line();
        // The `IntoSmeltUnknown` erasure adapters reference the `SmeltUnknown`
        // carrier, which is only emitted when the crate genuinely crosses a
        // dynamic boundary. A regex/match program that keeps every value
        // statically typed does not emit `SmeltUnknown`, so the adapters are
        // gated on that need rather than on regex presence alone.
        if needs_unknown {
            writer.line("/// Erase a concrete RegExp into a `SmeltUnknown` at a dynamic boundary.");
            writer.line("///");
            writer.line("/// A JavaScript RegExp object owns three observable data properties:");
            writer.line("/// `source`, `flags` and the writable `lastIndex`. All three must cross");
            writer.line("/// the boundary, or a round trip through erased dataflow (`clone(re)`,");
            writer.line("/// `structuredClone`, a `Record<string, unknown>` bag) silently resets");
            writer.line("/// `lastIndex` to 0.");
            writer.block("impl IntoSmeltUnknown for SmeltRegExp", |impl_writer| {
                impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                    fn_writer.line("let last_index = *self.last_index.borrow() as f64;");
                    fn_writer.line("SmeltUnknown::Object(SmeltObject::with_id(self.id, Vec::from([");
                    fn_writer.line("(\"source\".to_owned(), SmeltUnknown::String(self.source.into())),");
                    fn_writer.line("(\"flags\".to_owned(), SmeltUnknown::String(self.flags.into())),");
                    fn_writer.line("(\"lastIndex\".to_owned(), SmeltUnknown::Number(last_index)),");
                    fn_writer.line("(\"__smelt_regexp\".to_owned(), SmeltUnknown::Bool(true)),");
                    fn_writer.line("])))");
                });
            });
            writer.blank_line();
            writer.line("/// Recover a concrete RegExp from an erased value.");
            writer.line("///");
            writer.line("/// The exact inverse of the adapter above: a marker record restores");
            writer.line("/// `source`, `flags` and `lastIndex`; a bare string is the `new");
            writer.line("/// RegExp(str)` spelling and yields a flagless pattern. Anything else");
            writer.line("/// answers the empty pattern, matching `new RegExp(undefined)`.");
            writer.block("impl SmeltFromUnknown for SmeltRegExp", |impl_writer| {
                impl_writer.block("fn smelt_from_unknown(value: SmeltUnknown) -> Self", |fn_writer| {
                    fn_writer.block("match value", |match_writer| {
                        match_writer.line("SmeltUnknown::String(source) => Self::new(source.to_string(), String::new()),");
                        match_writer.block("SmeltUnknown::Object(map) =>", |arm_writer| {
                            arm_writer.line("let source = match map.get(\"source\") { Some(SmeltUnknown::String(source)) => source.to_string(), _ => String::new() };");
                            arm_writer.line("let flags = match map.get(\"flags\") { Some(SmeltUnknown::String(flags)) => flags.to_string(), _ => String::new() };");
                            arm_writer.line("let regexp = Self::new(source, flags);");
                            arm_writer.line("if let Some(SmeltUnknown::Number(last_index)) = map.get(\"lastIndex\") { if last_index.is_finite() && last_index >= 0.0 { *regexp.last_index.borrow_mut() = last_index as usize; } }");
                            arm_writer.line("regexp");
                        });
                        match_writer.line("_ => Self::new(String::new(), String::new()),");
                    });
                });
            });
            writer.blank_line();
        }
        writer.block("impl Default for SmeltRegExp", |impl_writer| {
            impl_writer.line("/// Construct a RegExp that matches the empty string.");
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.line("Self::new(String::new(), String::new())");
            });
        });
        writer.blank_line();
        emit_smelt_match(&mut writer, needs_unknown);
    }
    for class in &mir.classes {
        let name = class_name_text(mir, class)?;
        if !emitted_class_names.insert(name.clone()) {
            continue;
        }
        if context.is_reference_class(class.name) {
            emit_reference_class_storage(&mut writer, mir, &context, class, needs_unknown)?;
            continue;
        }
        let type_params = class_type_params_text(mir, class)?;
        let impl_generics = class_impl_generics_text(mir, class)?;
        let _inherited_trait_methods = inherited_trait_methods(mir, class);
        let mut field_lines = Vec::new();
        let fields = effective_class_fields(mir, class);
        let scoped_type_params = class
            .type_params
            .iter()
            .map(|param| param.name)
            .collect::<HashSet<_>>();
        let has_function_field = fields
            .iter()
            .any(|field| type_contains_function(mir, field.ty));
        // A value class supports structural equality (JS `==`/`===`/`toBe` on the
        // by-value representation, and derived comparisons in generated specs)
        // only when every stored field is itself `PartialEq`. Function fields
        // (`dyn Fn`) and promise fields (`SmeltPromise`, a `Clone`-only shared
        // future handle) are the two prelude shapes without `PartialEq`, so the
        // derive is gated on their absence.
        let supports_partial_eq = fields.iter().all(|field| {
            type_supports_partial_eq(mir, &context, field.ty, &mut Vec::new())
        });
        let partial_eq_derive = if supports_partial_eq { ", PartialEq" } else { "" };
        if has_function_field {
            writer.line("#[derive(Clone)]");
            writer.line("#[allow(dead_code)]");
        } else if needs_serde_json && class_is_json_serializable(mir, class) {
            writer.line(format!(
                "#[derive(Clone, Debug, Default{partial_eq_derive}, serde::Serialize, serde::Deserialize)]"
            ));
        } else {
            writer.line(format!("#[derive(Clone, Debug, Default{partial_eq_derive})]"));
        }
        for field in &fields {
            field_lines.push(format!(
                "{}: {},",
                RustIdent::new(
                    mir.symbols
                        .get(field.name)
                        .ok_or_else(|| EmitError::new("field has unknown symbol"))?
                ),
                FunctionEmitter::type_text_for_with_scoped_type_params(
                    mir,
                    &context,
                    field.ty,
                    &TypeSubstitution::lexical(&scoped_type_params),
                )?
            ));
        }
        if !class.type_params.is_empty() {
            let phantom_args = class
                .type_params
                .iter()
                .map(|param| {
                    mir.symbols
                        .get(param.name)
                        .map(|param_name| RustIdent::new(param_name).into_string())
                        .ok_or_else(|| EmitError::new("class type parameter has unknown symbol"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            field_lines.push(format!(
                "_smelt_phantom: ::std::marker::PhantomData<({phantom_args})>,"
            ));
        }
        writer.block(format!("struct {name}{type_params}"), |block_writer| {
            for field_line in field_lines {
                block_writer.line(field_line);
            }
        });
        if has_function_field {
            let phantom_args = class
                .type_params
                .iter()
                .map(|param| {
                    mir.symbols
                        .get(param.name)
                        .map(|param_name| RustIdent::new(param_name).into_string())
                        .ok_or_else(|| EmitError::new("class type parameter has unknown symbol"))
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            emit_default_impl_for_storage_type(
                &mut writer,
                mir,
                &context,
                &name,
                &impl_generics,
                &type_params,
                &fields,
                &phantom_args,
                &scoped_type_params,
            )?;
            emit_debug_impl_for_storage_type(&mut writer, &name, &impl_generics, &type_params);
        }
        if !class.static_fields.is_empty() {
            writer.block(
                format!("impl{impl_generics} {name}{type_params}"),
                |impl_writer| {
                    for field in &class.static_fields {
                        let field_name = mir
                            .symbols
                            .get(field.name)
                            .map(RustIdent::new)
                            .map_or_else(|| "field".to_owned(), RustIdent::into_string);
                        let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
                            mir,
                            &context,
                            field.ty,
                            &TypeSubstitution::lexical(&scoped_type_params),
                        )
                        .unwrap_or_else(|_| "SmeltUnknown".to_owned());
                        let value = materialized_static_value_text(field.value.as_ref());
                        impl_writer.block(
                            format!("fn __smelt_static_{field_name}() -> {field_ty}"),
                            |function_writer| function_writer.line(value),
                        );
                    }
                },
            );
        }
        if needs_unknown {
            emit_record_into_smelt_unknown_impl(
                &mut writer,
                mir,
                &name,
                &impl_generics,
                &type_params,
                &fields,
            )?;
            emit_record_from_smelt_unknown_impl(
                &mut writer,
                mir,
                &name,
                &impl_generics,
                &type_params,
                &fields,
            )?;
        }
        writer.blank_line();
    }

    let union_definitions = emitter::emit_union_definitions(mir, &context)?;
    if !union_definitions.is_empty() {
        for line in union_definitions.lines() {
            writer.line(line);
        }
    }

    let mut out = writer.finish();

    // Stable sentinel marking the end of the fixed runtime prelude and the start
    // of program-specific code. Tooling (`smelt smelt-unknown-report`) keys on
    // this exact comment to separate shared runtime scaffolding — the
    // `SmeltUnknown` enum, its impls, and the `smelt_*` helpers above — from the
    // generated program below, so `SmeltUnknown` occurrences in the prelude are
    // never mistaken for program-level erasure. Keep the text byte-for-byte
    // stable; changing it silently reclassifies every prelude occurrence.
    //
    // Normalize the trailing whitespace first so the marker always sits after
    // exactly one blank line, regardless of how the last prelude block ended.
    while out.ends_with('\n') {
        out.pop();
    }
    out.push_str(&format!("\n\n{PRELUDE_END_MARKER}\n"));

    // Emit one thread-local cell per module-level mutable global. These are
    // program-specific mangled names (not fixed runtime symbols): each `#[test]`
    // thread observes fresh module state initialized from the source literal,
    // deterministic and mirroring vitest's per-file module isolation. Copy
    // primitives use a `const`-initialized `Cell`; strings use the non-const
    // `RefCell` form because a `.to_owned()` initializer cannot be `const`.
    if !mir.globals.is_empty() {
        out.push('\n');
        out.push_str(&emit_mutable_globals(mir)?);
    }

    let mut has_emitted_root_function = false;
    for function in &mir.functions {
        if matches!(
            function.origin,
            HirOrigin::ClassConstructor { .. }
                | HirOrigin::ClassMethod { .. }
                | HirOrigin::ClassStaticMethod { .. }
        ) {
            continue;
        }
        let mut emitter = FunctionEmitter::new(mir, &context, function)?;
        let mut emitted_function_source = String::new();
        emitter.emit(&mut emitted_function_source)?;
        let routed_function_source =
            route_free_function(function, &context, emitted_function_source)?;
        let Some(root_function_source) = routed_function_source else {
            continue;
        };
        if has_emitted_root_function || !mir.classes.is_empty() {
            out.push('\n');
        }
        out.push_str(&root_function_source);
        has_emitted_root_function = true;
    }

    // Flush the per-function-item value accessors collected while erasing
    // function-item-as-value wrappers to `SmeltUnknown`. Each accessor lazily
    // builds and caches ONE erased `SmeltUnknown::Function` so that every
    // reference to the same named function value shares one inner `Rc`, keeping
    // JavaScript reference identity (`===`) stable across references. Sorted by
    // item key via the `BTreeMap` for deterministic golden output.
    for (key, body) in context.function_item_accessors.borrow().iter() {
        out.push_str(&format!(
            "\nfn __smelt_fn_value_{key}() -> SmeltUnknown {{\n    thread_local! {{ static SMELT_FN_VALUE: ::std::cell::OnceCell<SmeltUnknown> = ::std::cell::OnceCell::new(); }}\n    SMELT_FN_VALUE.with(|cell| cell.get_or_init(|| {body}).clone())\n}}\n"
        ));
    }

    // Flush the per-function-item accessors for the CONCRETE `SmeltErasedFunction`
    // type. Each caches ONE `SmeltErasedFunction` so repeated calls of a nullary
    // function-item constant (`doNothing()`/`constant()`) return clones sharing
    // one inner callback `Rc`, keeping JavaScript function-singleton identity.
    // A non-empty collector means a closure was lowered to `SmeltErasedFunction`,
    // so `needs_erased_function` is necessarily true and the struct is emitted.
    for (key, body) in context.function_item_erased_fn_accessors.borrow().iter() {
        out.push_str(&format!(
            "\nfn __smelt_fn_erased_{key}() -> SmeltErasedFunction {{\n    thread_local! {{ static SMELT_FN_ERASED: ::std::cell::OnceCell<SmeltErasedFunction> = ::std::cell::OnceCell::new(); }}\n    SMELT_FN_ERASED.with(|cell| cell.get_or_init(|| {body}).clone())\n}}\n"
        ));
    }

    let mut emitted_impl_names = HashSet::new();
    for class in &mir.classes {
        let name = class_name_text(mir, class)?;
        if !emitted_impl_names.insert(name.clone()) {
            continue;
        }
        let impl_generics = class_impl_generics_text(mir, class)?;
        let type_args = class_type_args_text(mir, class)?;
        out.push_str(&format!("\nimpl{impl_generics} {name}{type_args} {{\n"));
        if !class.is_abstract
            && let Some(constructor) = class.constructor
            && let Some(function) = mir.functions.get(id_index(
                constructor.0,
                "constructor index does not fit usize",
            )?)
        {
            let mut emitter = FunctionEmitter::new(mir, &context, function)?;
            emitter.emit_method(&mut out)?;
        }
        // Inherited methods are emitted into the subclass's own impl block:
        // Smelt flattens inheritance and Rust has no method inheritance, so a
        // base method is otherwise not callable on a subclass receiver.
        for method in &effective_class_methods(mir, class) {
            if let Some(function) = mir
                .functions
                .get(id_index(method.0, "method index does not fit usize")?)
            {
                let mut emitter = FunctionEmitter::new(mir, &context, function)?;
                emitter.emit_method(&mut out)?;
            }
        }
        for static_method in &class.static_methods {
            if let Some(function) = mir.functions.get(id_index(
                static_method.0,
                "static method index does not fit usize",
            )?) {
                let mut emitter = FunctionEmitter::new(mir, &context, function)?;
                emitter.emit_method(&mut out)?;
            }
        }
        // A class instance that crosses into erased code keeps its methods, as
        // prototype-carried members. Without this the erased view held only
        // fields, and every method read off it answered `undefined` — which the
        // erased call sites silently replaced with a fabricated default.
        // Gated on `needs_unknown`: the adapters are written in terms of the
        // erased carrier, and a program that never erases anything does not
        // emit `SmeltUnknown` at all (the Python specialization fixtures are
        // exactly that shape, and unconditional emission made them E0425).
        if needs_unknown
            && let Some(proto_entries) =
                class_proto::class_proto_entries_method(mir, &context, class)?
        {
            out.push_str(&proto_entries);
        }
        out.push_str("}\n");
        for protocol in &class.protocols {
            match protocol {
                MirClassProtocol::Add { method } => {
                    let function = mir
                        .functions
                        .get(id_index(method.0, "add protocol method index does not fit usize")?)
                        .ok_or_else(|| EmitError::new("add protocol method is missing"))?;
                    let mut emitter = FunctionEmitter::new(mir, &context, function)?;
                    emitter.emit_python_add_impl(
                        &mut out,
                        &name,
                        &impl_generics,
                        &type_args,
                    )?;
                }
            }
        }
    }

    if !has_main_function(mir)? {
        out.push_str("\nfn main() {}\n");
    }

    Ok(out)
}

/// Rust sources split by original source module.
struct MappedSources {
    /// Crate root source.
    root: String,
    /// Generated sibling module sources.
    modules: Vec<MappedModuleSource>,
}

/// One generated Rust module file.
struct MappedModuleSource {
    /// Source-shaped Rust file stem.
    name: String,
    /// Module source text.
    source: String,
}

/// Generated module metadata for a HIR body.
#[derive(Clone)]
struct BodyModuleInfo {
    /// Source-shaped Rust file stem.
    name: String,
    /// Original source path that owns the body.
    source_path: String,
}

/// Emits source text split across Rust module files.
fn emit_mapped_sources(
    mir: &Mir,
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
    allocator: GeneratedAllocator,
) -> Result<MappedSources, EmitError> {
    let body_modules = body_module_names(krate, modules);
    let mut module_chunks = HashMap::<String, Vec<String>>::new();
    let mut module_paths = HashMap::<String, String>::new();

    let mut root =
        emit_source_with_free_function_router(mir, allocator, |function, context, function_source| {
            let HirOrigin::Body(body) = function.origin else {
                return Ok(Some(function_source));
            };
            if is_root_main_function(mir, function, context.none_ty) {
                return Ok(Some(function_source));
            }
            let Some(module_info) = body_modules.get(&body).cloned() else {
                return Ok(Some(function_source));
            };
            let module_name = module_info.name;
            module_paths
                .entry(module_name.clone())
                .or_insert(module_info.source_path);
            module_chunks
                .entry(module_name)
                .or_default()
                .push(publicize_free_function(function_source));
            Ok(None)
        })?;

    let mut module_names = module_chunks.keys().cloned().collect::<Vec<_>>();
    module_names.sort();
    let declarations = module_names
        .iter()
        .map(|name| {
            let module_ident = generated_source_module_ident(name);
            format!(
                "#[path = \"{name}.rs\"]\nmod {module_ident};\npub(crate) use {module_ident}::*;"
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    if !declarations.is_empty() {
        root = insert_after_crate_header(root, &format!("{declarations}\n\n\n"));
    }

    let mapped_modules = module_names
        .into_iter()
        .map(|name| {
            let chunks = module_chunks.remove(&name).unwrap_or_default();
            let source_path = module_paths.remove(&name).unwrap_or_else(|| name.clone());
            MappedModuleSource {
                name,
                source: format!(
                    "// @generated by smelt. Do not edit by hand.\n// source: {source_path}\n#![allow(dead_code, non_snake_case, unused_imports, unused_variables)]\n\nuse super::*;\n\n{}",
                    chunks.join("\n")
                ),
            }
        })
        .collect();

    Ok(MappedSources {
        root,
        modules: mapped_modules,
    })
}

/// Builds a body-to-Rust-module map from HIR module ownership metadata.
fn body_module_names(
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
) -> HashMap<BodyId, BodyModuleInfo> {
    let mut names = HashMap::new();
    for (path, module_id) in modules {
        let Some(module) = usize::try_from(module_id.0)
            .ok()
            .and_then(|index| krate.modules.get(index))
        else {
            continue;
        };
        let rust_module = source_module_name(&module.name);
        let module_info = BodyModuleInfo {
            name: rust_module.clone(),
            source_path: path.clone(),
        };
        if let Some(body) = module.body {
            names.insert(body, module_info.clone());
        }
        for item in &module.items {
            if let Some(smelt_hir::Item::Function(function)) = usize::try_from(item.0)
                .ok()
                .and_then(|index| krate.items.get(index))
                && let Some(body) = function.body
            {
                names.insert(body, module_info.clone());
            }
        }
    }
    names
}

/// Returns the source-shaped Rust file stem for one lowered source module.
fn source_module_name(name: &str) -> String {
    let sanitized = sanitize_ident(name);
    if sanitized == "main" || sanitized == "lib" {
        format!("source_{sanitized}")
    } else {
        sanitized
    }
}

/// Returns the private Rust module identifier used to include a source file.
///
/// The generated file stem intentionally tracks the source module name for
/// readable artifacts. The Rust module item needs a separate internal name
/// because Rust puts `mod Foo;` and `struct Foo` in the same type namespace.
/// Date-fns has source modules such as `Setter.ts` that also define `Setter`,
/// so declaring them through an internal identifier avoids double emission
/// collisions while keeping `Setter.rs` as the artifact file.
fn generated_source_module_ident(file_stem: &str) -> String {
    sanitize_ident(&format!("__smelt_module_{file_stem}"))
}

/// Returns whether this generated function must remain the Rust crate root.
fn is_root_main_function(mir: &Mir, function: &MirFunction, none_ty: TypeId) -> bool {
    !function.is_test
        && function.return_ty == none_ty
        && mir
            .symbols
            .get(function.name)
            .is_some_and(|name| name == "main")
}

/// Makes a top-level generated function visible to the crate root.
fn publicize_free_function(source: String) -> String {
    if source.starts_with("async fn ") {
        return source.replacen("async fn ", "pub(crate) async fn ", 1);
    }
    if source.starts_with("fn ") {
        return source.replacen("fn ", "pub(crate) fn ", 1);
    }
    if source.starts_with("#[")
        && let Some(index) = source.find("\nfn ")
    {
        let mut output = source;
        let start = index.checked_add(1).unwrap_or(index);
        let end = index.checked_add(4).unwrap_or(index);
        output.replace_range(start..end, "pub(crate) fn ");
        return output;
    }
    source
}

/// Inserts module declarations after the generated header and crate attributes.
fn insert_after_crate_header(mut root: String, text: &str) -> String {
    let marker = "#![allow(dead_code, non_snake_case, unused_imports, unused_variables)]\n\n";
    if let Some(index) = root.find(marker) {
        let insertion_index = index.checked_add(marker.len()).unwrap_or(index);
        root.insert_str(insertion_index, text);
        return root;
    }
    root.insert_str(0, text);
    root
}

/// Emit the concrete `SmeltMatch` runtime type for RegExp match results.
///
/// `RegExp.prototype.exec` and `String.prototype.matchAll` return a JavaScript
/// match result: an array-like value whose numbered entries are the capture
/// groups (entry `0` is the whole match) plus the `index`, `input`, and
/// `groups` properties. Historically Smelt built this shape inline as a
/// `SmeltUnknown::Object` property bag, which erased its statically known
/// structure. `SmeltMatch` models that structure with concrete Rust fields:
///
/// * `groups` — numbered capture groups as `Vec<Option<String>>` (an absent
///   optional group is `None`, matching JavaScript `undefined`).
/// * `named` — named capture groups keyed by their original name (and a
///   snake_case alias so generated snake_cased field reads still resolve).
/// * `match_index` — the zero-based match offset (`.index`).
/// * `input` — the full searched string (`.input`).
///
/// Typed accessors (`group`, `named_group`, `index`, `input`, `len`) let
/// callers read the shape without any dynamic tagging. When a match value has
/// to cross a genuinely dynamic boundary (the erased `unknown` result type the
/// frontend still assigns for `exec`/`matchAll` consumers), it is converted
/// with the explicit [`IntoSmeltUnknown`] adapter — the single place where the
/// concrete match is intentionally erased.
fn emit_smelt_match(writer: &mut CodeWriter, needs_unknown: bool) {
    writer.line("/// A concrete JavaScript RegExp match result (numbered groups, named");
    writer.line("/// groups, `index`, and `input`).");
    writer.line("#[derive(Clone, Debug, Default)]");
    writer.block("pub struct SmeltMatch", |struct_writer| {
        struct_writer.line("id: usize,");
        struct_writer.line("/// Numbered capture groups; entry 0 is the whole match. An absent");
        struct_writer.line("/// optional group is `None` (JavaScript `undefined`).");
        struct_writer.line("groups: Vec<Option<String>>,");
        struct_writer.line("/// Named capture groups keyed by name (plus a snake_case alias).");
        struct_writer.line("named: ::std::collections::HashMap<String, Option<String>>,");
        struct_writer.line("/// Zero-based offset of the match within `input` (`.index`).");
        struct_writer.line("match_index: usize,");
        struct_writer.line("/// The full string that was searched (`.input`).");
        struct_writer.line("input: String,");
    });
    writer.blank_line();
    // `id` is reference identity, not content: two matches produced by separate
    // `exec` calls (or an original and its clone) must still compare structurally
    // equal under `toEqual`/`isEqual`. This mirrors the hand-written
    // `SmeltRegExp` `PartialEq`, which excludes its `id` for the same reason;
    // the derived impl used to make every clone unequal to its source.
    writer.line("/// Structural equality over the match content, ignoring reference identity.");
    writer.line("impl PartialEq for SmeltMatch { fn eq(&self, other: &Self) -> bool { self.groups == other.groups && self.named == other.named && self.match_index == other.match_index && self.input == other.input } }");
    writer.blank_line();
    writer.line("#[allow(dead_code)]");
    writer.block("impl SmeltMatch", |impl_writer| {
        impl_writer.line("/// Build a `SmeltMatch` from a compiled regex and its captures.");
        impl_writer.block(
            "fn from_captures(regex: &fancy_regex::Regex, captures: &fancy_regex::Captures<'_>, match_index: usize, input: &str) -> Self",
            |fn_writer| {
                fn_writer.line("let groups = (0..captures.len()).map(|index| captures.get(index).map(|value| value.as_str().to_owned())).collect::<Vec<_>>();");
                fn_writer.line("let mut named = ::std::collections::HashMap::new();");
                fn_writer.block("for name in regex.capture_names().flatten()", |for_writer| {
                    for_writer.line("let value = captures.name(name).map(|value| value.as_str().to_owned());");
                    for_writer.line("named.insert(name.to_owned(), value.clone());");
                    for_writer.line("let mut snake = String::new();");
                    for_writer.block("for (index, ch) in name.chars().enumerate()", |snake_writer| {
                        snake_writer.line("if ch.is_ascii_uppercase() { if index > 0 { snake.push('_'); } snake.push(ch.to_ascii_lowercase()); } else { snake.push(ch); }");
                    });
                    for_writer.line("named.insert(snake, value);");
                });
                fn_writer.line("Self { id: smelt_next_object_id(), groups, named, match_index, input: input.to_owned() }");
            },
        );
        impl_writer.line("/// Read a numbered capture group (`match[n]`).");
        impl_writer.block("fn group(&self, index: usize) -> Option<&str>", |fn_writer| {
            fn_writer.line("self.groups.get(index).and_then(|value| value.as_deref())");
        });
        impl_writer.line("/// Read a numbered capture group as an owned optional string.");
        impl_writer.line("///");
        impl_writer.line("/// Consumer index reads (`match[n]`) are typed `Optional(String)`; a");
        impl_writer.line("/// group that did not participate in the match is `None` (JavaScript");
        impl_writer.line("/// `undefined`).");
        impl_writer.block("pub fn group_owned(&self, index: usize) -> Option<String>", |fn_writer| {
            fn_writer.line("self.groups.get(index).cloned().flatten()");
        });
        impl_writer.line("/// Read a named capture group (`match.groups.name`).");
        impl_writer.block("fn named_group(&self, name: &str) -> Option<&str>", |fn_writer| {
            fn_writer.line("self.named.get(name).and_then(|value| value.as_deref())");
        });
        impl_writer.line("/// Read a named capture group as an owned optional string.");
        impl_writer.line("///");
        impl_writer.line("/// Consumer named-group reads (`match.groups.name`) are typed");
        impl_writer.line("/// `Optional(String)`; a group that did not participate is `None`.");
        impl_writer.block("pub fn named_group_owned(&self, name: &str) -> Option<String>", |fn_writer| {
            fn_writer.line("self.named.get(name).cloned().flatten()");
        });
        impl_writer.line("/// The zero-based match offset (`match.index`).");
        impl_writer.block("pub fn index(&self) -> f64", |fn_writer| {
            fn_writer.line("self.match_index as f64");
        });
        impl_writer.line("/// The full searched string (`match.input`).");
        impl_writer.block("fn input(&self) -> &str", |fn_writer| {
            fn_writer.line("&self.input");
        });
        impl_writer.line("/// The full searched string as an owned value (`match.input`).");
        impl_writer.block("pub fn input_owned(&self) -> String", |fn_writer| {
            fn_writer.line("self.input.clone()");
        });
        impl_writer.line("/// Number of numbered capture groups, including the whole match.");
        impl_writer.block("fn len(&self) -> usize", |fn_writer| {
            fn_writer.line("self.groups.len()");
        });
        impl_writer.line("/// Number of numbered capture groups as a JavaScript number (`match.length`).");
        impl_writer.block("pub fn length(&self) -> f64", |fn_writer| {
            fn_writer.line("self.groups.len() as f64");
        });
        impl_writer.line("/// Whether there are no numbered groups (never true for a real match).");
        impl_writer.block("fn is_empty(&self) -> bool", |fn_writer| {
            fn_writer.line("self.groups.is_empty()");
        });
    });
    writer.blank_line();
    // The erasure adapter references the `SmeltUnknown` carrier and is only
    // emitted when the crate genuinely erases a match into dynamic dataflow.
    // Programs that keep every match read statically typed never emit
    // `SmeltUnknown`, so the adapter would otherwise fail to compile.
    if needs_unknown {
        writer.line("/// Erase a concrete match into a `SmeltUnknown` at a dynamic boundary.");
        writer.line("///");
        writer.line("/// This reproduces the JavaScript match-array-with-properties shape:");
        writer.line("/// numbered string keys for the groups plus `groups`, `index`, and");
        writer.line("/// `input`. It is the single explicit adapter used when a typed");
        writer.line("/// `SmeltMatch` must flow into erased `unknown` consumer dataflow.");
        writer.line("///");
        writer.line("/// A JavaScript match result is really an ARRAY with those extra named");
        writer.line("/// properties, and `SmeltArray` can now carry named properties (see its");
        writer.line("/// `props` side table), so this could report `[object Array]` instead --");
        writer.line("/// which is what makes a match compare equal to the plain array a spec");
        writer.line("/// matches it against. It does not yet, because the typed `SmeltList` view");
        writer.line("/// cannot reach those properties: a `T[]`-typed reader of the erased value");
        writer.line("/// (es-toolkit `cloneDeepWith` narrows with `Array.isArray` and then reads");
        writer.line("/// `.index`/`.input`) would take the array branch and copy neither, losing");
        writer.line("/// them from the clone. Flipping this adapter belongs with making the side");
        writer.line("/// table reachable through a typed list handle.");
        writer.block("impl IntoSmeltUnknown for SmeltMatch", |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line("let mut object: Vec<(String, SmeltUnknown)> = Vec::new();");
                fn_writer.line("for (index, value) in self.groups.iter().enumerate() { object.push((index.to_string(), value.clone().map_or(SmeltUnknown::Undefined, |value| SmeltUnknown::String(value.into())))); }");
                fn_writer.line("let groups = self.named.into_iter().map(|(name, value)| (name, value.map_or(SmeltUnknown::Undefined, |value| SmeltUnknown::String(value.into())))).collect::<Vec<_>>();");
                fn_writer.line("object.push((\"groups\".to_owned(), SmeltUnknown::Object(SmeltObject::new(groups))));");
                fn_writer.line("object.push((\"index\".to_owned(), SmeltUnknown::Number(self.match_index as f64)));");
                fn_writer.line("object.push((\"input\".to_owned(), SmeltUnknown::String(self.input.into())));");
                fn_writer.line("SmeltUnknown::Object(SmeltObject::with_id(self.id, object))");
            });
        });
        writer.blank_line();
        writer.line("/// Recover a concrete match from an erased value.");
        writer.line("///");
        writer.line("/// The inverse of the adapter above, so a match that round-trips through");
        writer.line("/// erased dataflow (`cloneDeep(/re/.exec(s))`, an `unknown` bag) comes back");
        writer.line("/// with its groups, named groups, `index` and `input` intact instead of the");
        writer.line("/// empty `Default::default()` the generic class fallback would produce.");
        writer.line("/// A bare array (the JavaScript match value IS an array) restores the");
        writer.line("/// numbered groups; the extra properties are then simply absent.");
        writer.block("impl SmeltFromUnknown for SmeltMatch", |impl_writer| {
            impl_writer.block("fn smelt_from_unknown(value: SmeltUnknown) -> Self", |fn_writer| {
                fn_writer.line("let group_of = |value: SmeltUnknown| match value { SmeltUnknown::String(text) => Some(text.to_string()), _ => None };");
                fn_writer.block("match value", |match_writer| {
                    match_writer.block("SmeltUnknown::Array(values) =>", |arm_writer| {
                        arm_writer.line("let mut named = ::std::collections::HashMap::new();");
                        arm_writer.line("if let Some(SmeltUnknown::Object(entries)) = values.named_property(\"groups\") { for (name, entry) in entries.iter() { named.insert(name, group_of(entry)); } }");
                        arm_writer.line("let match_index = match values.named_property(\"index\") { Some(SmeltUnknown::Number(index)) if index.is_finite() && index >= 0.0 => index as usize, _ => 0 };");
                        arm_writer.line("let input = match values.named_property(\"input\") { Some(SmeltUnknown::String(input)) => input.to_string(), _ => String::new() };");
                        arm_writer.line("let id = values.id;");
                        arm_writer.line("Self { id, groups: values.into_vec().into_iter().map(group_of).collect(), named, match_index, input }");
                    });
                    match_writer.block("SmeltUnknown::Object(map) =>", |arm_writer| {
                        arm_writer.line("let mut groups = Vec::new();");
                        arm_writer.line("while let Some(entry) = map.get(&groups.len().to_string()) { groups.push(group_of(entry)); }");
                        arm_writer.line("let mut named = ::std::collections::HashMap::new();");
                        arm_writer.line("if let Some(SmeltUnknown::Object(entries)) = map.get(\"groups\") { for (name, entry) in entries.iter() { named.insert(name, group_of(entry)); } }");
                        arm_writer.line("let match_index = match map.get(\"index\") { Some(SmeltUnknown::Number(index)) if index.is_finite() && index >= 0.0 => index as usize, _ => 0 };");
                        arm_writer.line("let input = match map.get(\"input\") { Some(SmeltUnknown::String(input)) => input.to_string(), _ => String::new() };");
                        arm_writer.line("Self { id: map.id, groups, named, match_index, input }");
                    });
                    match_writer.line("_ => Self::default(),");
                });
            });
        });
        writer.blank_line();
    }
}

/// Emit natural JSON serde support for `SmeltUnknown`.
fn emit_unknown_serde_impls(writer: &mut CodeWriter) {
    writer.block("impl serde::Serialize for SmeltUnknown", |impl_writer| {
        impl_writer.block(
            "fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where S: serde::Serializer",
            |fn_writer| {
                fn_writer.block("match self", |match_writer| {
                    match_writer.line("Self::Null => serializer.serialize_none(),");
                    match_writer.line("Self::Undefined => serializer.serialize_none(),");
                    match_writer.line("Self::Bool(value) => serializer.serialize_bool(*value),");
                    match_writer.line("Self::Number(value) => serializer.serialize_f64(*value),");
                    match_writer.line("Self::String(value) => serializer.serialize_str(value),");
                    match_writer.line("Self::Symbol(value) => serializer.serialize_str(value),");
                    match_writer.line("Self::Array(values) => serde::Serialize::serialize(&*values.values.borrow(), serializer),");
                    match_writer.line("Self::Object(values) => serde::Serialize::serialize(&values.iter().filter(|(key, _)| key != \"__smelt_class\" && !key.starts_with(\"__smelt_proto:\") && !key.starts_with(\"__smelt_method:\")).collect::<::std::collections::HashMap<_, _>>(), serializer),");
                    match_writer.line("Self::Function(_) => serializer.serialize_str(\"function () { [native code] }\"),");
                    match_writer.line("Self::Promise(_) => serializer.serialize_str(\"[object Promise]\"),");
                });
            },
        );
    });
    writer.blank_line();
    writer.block("impl<'de> serde::Deserialize<'de> for SmeltUnknown", |impl_writer| {
        impl_writer.block(
            "fn deserialize<D>(deserializer: D) -> Result<Self, D::Error> where D: serde::Deserializer<'de>",
            |fn_writer| {
                fn_writer.line("let value = <serde_json::Value as serde::Deserialize>::deserialize(deserializer)?;");
                fn_writer.line(format!(
                    "Ok({from_json}(value))",
                    from_json = smelt_stdlib::runtime_symbols::json::UNKNOWN_FROM_JSON_VALUE,
                ));
            },
        );
    });
    writer.blank_line();
    let from_json = smelt_stdlib::runtime_symbols::json::UNKNOWN_FROM_JSON_VALUE;
    writer.block(
        format!("fn {from_json}(value: serde_json::Value) -> SmeltUnknown"),
        |fn_writer| {
            fn_writer.block("match value", |match_writer| {
                match_writer.line("serde_json::Value::Null => SmeltUnknown::Null,");
                match_writer.line("serde_json::Value::Bool(value) => SmeltUnknown::Bool(value),");
                match_writer.line("serde_json::Value::Number(value) => SmeltUnknown::Number(value.as_f64().unwrap_or_default()),");
                match_writer.line("serde_json::Value::String(value) => SmeltUnknown::String(value.into()),");
                match_writer.line(format!(
                    "serde_json::Value::Array(values) => SmeltUnknown::Array(values.into_iter().map({from_json}).collect()),"
                ));
                match_writer.line(format!(
                    "serde_json::Value::Object(values) => SmeltUnknown::Object(SmeltObject::new(values.into_iter().map(|(key, value)| (key, {from_json}(value))).collect())),"
                ));
            });
        },
    );
    writer.blank_line();
}

/// Emit storage for a reference class as a handle newtype over `Rc<RefCell<_>>`.
///
/// A reference class `Name` becomes a thin handle `struct Name(Rc<RefCell<
/// NameInner>>)` whose fields live in `NameInner`. Identity lives only in the
/// wrapper, so:
/// - `Clone` is hand-written as `Rc::clone` — a clone shares the SAME cell, which
///   is JavaScript reference-identity and the fix for the silent "mutate a
///   throwaway clone" miscompile;
/// - `Default`/`Debug` delegate through the cell;
/// - `IntoSmeltUnknown` (only when the crate crosses a dynamic boundary) borrows
///   the cell and projects the public fields into an object.
///
/// The inner struct carries the same concrete field types the value struct would
/// have used, so field access stays strongly typed; only identity concentrates
/// in the outer `Rc`.
fn emit_reference_class_storage(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    class: &smelt_mir::MirClass,
    needs_unknown: bool,
) -> Result<(), EmitError> {
    emit_reference_record_storage(
        writer,
        mir,
        context,
        &ReferenceRecordShape {
            name: class_name_text(mir, class)?,
            type_params: class_type_params_text(mir, class)?,
            type_args: class_type_args_text(mir, class)?,
            impl_generics: class_impl_generics_text(mir, class)?,
            fields: effective_class_fields(mir, class),
            static_fields: &class.static_fields,
            type_param_names: class.type_params.iter().map(|param| param.name).collect(),
            has_proto_entries: class_proto::class_has_proto_entries(mir, context, class),
        },
        needs_unknown,
    )
}

/// Everything the reference (handle) representation needs about one record type.
///
/// A reference record is emitted identically whether the source spelled it as a
/// `class` or as an object *shape* (an `interface`, or an inline object type
/// literal lowered to a synthetic one — see `shape_object` in the TypeScript
/// frontend). Both are JavaScript objects, so both need the same handle newtype
/// when a field is written after construction; only where the pieces come from
/// differs, which is what this struct hides.
struct ReferenceRecordShape<'a> {
    /// Generated Rust type name.
    name: String,
    /// Declaration-site generic list, e.g. `<T>`; empty when non-generic.
    type_params: String,
    /// Use-site generic list, e.g. `<T>`; empty when non-generic.
    type_args: String,
    /// `impl` generic list including bounds.
    impl_generics: String,
    /// Stored fields, heritage included, in declaration order.
    fields: Vec<smelt_mir::MirField>,
    /// Materialized class-level fields; always empty for a shape.
    static_fields: &'a [smelt_mir::MirStaticField],
    /// Generic parameter symbols, for the lexical type-parameter scope.
    type_param_names: Vec<smelt_hir::Symbol>,
    /// Whether the type emits `__smelt_proto_entries` (see [`crate::class_proto`]).
    ///
    /// Only a `class` has method bodies to bind; an object *shape* has none, so
    /// it is always `false` there.
    has_proto_entries: bool,
}

/// Emit the handle newtype, inner record, and identity impls for one record type.
fn emit_reference_record_storage(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    shape: &ReferenceRecordShape<'_>,
    needs_unknown: bool,
) -> Result<(), EmitError> {
    let ReferenceRecordShape {
        name,
        type_params,
        type_args,
        impl_generics,
        fields,
        static_fields,
        type_param_names,
        has_proto_entries,
    } = shape;
    let inner_name = format!("{name}Inner");
    let scoped_type_params = type_param_names.iter().copied().collect::<HashSet<_>>();
    let has_function_field = fields
        .iter()
        .any(|field| type_contains_function(mir, field.ty));
    let phantom_args = type_param_names
        .iter()
        .map(|param| {
            mir.symbols
                .get(*param)
                .map(|param_name| RustIdent::new(param_name).into_string())
                .ok_or_else(|| EmitError::new("record type parameter has unknown symbol"))
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");

    // The handle newtype. `#[derive(Clone)]` is intentionally NOT used: a derived
    // clone would require `Inner: Clone` and clone the cell contents. We hand-
    // write `Rc::clone` below so a clone shares identity.
    writer.line("#[allow(dead_code)]");
    writer.line(format!(
        "struct {name}{type_params}(::std::rc::Rc<::std::cell::RefCell<{inner_name}{type_args}>>);"
    ));

    // The inner record. Debug/Default are derived unless a callback field blocks
    // the derives, matching the value-struct rules.
    if has_function_field {
        writer.line("#[allow(dead_code)]");
        writer.block(
            format!("struct {inner_name}{type_params}"),
            |block_writer| {
                emit_reference_inner_fields(block_writer, mir, context, fields, &scoped_type_params, &phantom_args);
            },
        );
        emit_default_impl_for_storage_type(
            writer,
            mir,
            context,
            &inner_name,
            impl_generics,
            type_args,
            fields,
            &phantom_args,
            &scoped_type_params,
        )?;
        emit_debug_impl_for_storage_type(writer, &inner_name, impl_generics, type_args);
    } else {
        writer.line("#[derive(Debug, Default)]");
        writer.line("#[allow(dead_code)]");
        writer.block(
            format!("struct {inner_name}{type_params}"),
            |block_writer| {
                emit_reference_inner_fields(block_writer, mir, context, fields, &scoped_type_params, &phantom_args);
            },
        );
    }

    // Hand-written identity `Clone`: share the SAME cell.
    writer.block(
        format!("impl{impl_generics} Clone for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn clone(&self) -> Self", |fn_writer| {
                fn_writer.line(format!("{name}(::std::rc::Rc::clone(&self.0))"));
            });
        },
    );

    // Hand-written identity `PartialEq`: a reference class has JavaScript object
    // identity, so `==`/`===`/`toBe` compare whether two handles share the same
    // cell (`Rc::ptr_eq`), never the cell contents. This also avoids requiring
    // `Inner: PartialEq`, which a callback-storing inner record could not satisfy.
    writer.block(
        format!("impl{impl_generics} PartialEq for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn eq(&self, other: &Self) -> bool", |fn_writer| {
                fn_writer.line("::std::rc::Rc::ptr_eq(&self.0, &other.0)");
            });
        },
    );

    // `Default` wraps a defaulted inner record in a fresh cell.
    writer.block(
        format!("impl{impl_generics} Default for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.line(format!(
                    "{name}(::std::rc::Rc::new(::std::cell::RefCell::new({inner_name}::default())))"
                ));
            });
        },
    );

    // `Debug` delegates through the cell to the inner record for a non-generic
    // class. A generic class's declared `T` bound does not include `Debug`, so a
    // delegating body would not satisfy `Inner<T>: Debug`; it falls back to a
    // non-exhaustive struct debug that needs no extra bound.
    writer.block(
        format!("impl{impl_generics} ::std::fmt::Debug for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block(
                "fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result",
                |fn_writer| {
                    if type_param_names.is_empty() {
                        fn_writer.line("::std::fmt::Debug::fmt(&*self.0.borrow(), formatter)");
                    } else {
                        fn_writer.line(format!(
                            "formatter.debug_struct({name:?}).finish_non_exhaustive()"
                        ));
                    }
                },
            );
        },
    );

    if !static_fields.is_empty() {
        writer.block(
            format!("impl{impl_generics} {name}{type_args}"),
            |impl_writer| {
                for field in *static_fields {
                    let field_name = mir
                        .symbols
                        .get(field.name)
                        .map(RustIdent::new)
                        .map_or_else(|| "field".to_owned(), RustIdent::into_string);
                    let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
                        mir,
                        context,
                        field.ty,
                        &TypeSubstitution::lexical(&scoped_type_params),
                    )
                    .unwrap_or_else(|_| "SmeltUnknown".to_owned());
                    let value = materialized_static_value_text(field.value.as_ref());
                    impl_writer.block(
                        format!("fn __smelt_static_{field_name}() -> {field_ty}"),
                        |function_writer| function_writer.line(value),
                    );
                }
            },
        );
    }

    if needs_unknown {
        emit_reference_class_into_smelt_unknown_impl(
            writer,
            mir,
            name,
            impl_generics,
            type_args,
            fields,
            *has_proto_entries,
        )?;
    }
    writer.blank_line();
    Ok(())
}

/// Emit the field lines of a reference class's inner record.
fn emit_reference_inner_fields(
    block_writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    fields: &[smelt_mir::MirField],
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
    phantom_args: &str,
) {
    for field in fields {
        let field_name = RustIdent::new(mir.symbols.get(field.name).unwrap_or("field")).into_string();
        let field_ty = FunctionEmitter::type_text_for_with_scoped_type_params(
            mir,
            context,
            field.ty,
            &TypeSubstitution::lexical(scoped_type_params),
        )
        .unwrap_or_else(|_| "SmeltUnknown".to_owned());
        block_writer.line(format!("{field_name}: {field_ty},"));
    }
    if !phantom_args.is_empty() {
        block_writer.line(format!(
            "_smelt_phantom: ::std::marker::PhantomData<({phantom_args})>,"
        ));
    }
}

/// Emit `IntoSmeltUnknown` for a reference class by borrowing the shared cell.
///
/// Mirrors [`emit_record_into_smelt_unknown_impl`] but projects the public
/// fields out of `self.0.borrow()` rather than owned struct fields.
fn emit_reference_class_into_smelt_unknown_impl(
    writer: &mut CodeWriter,
    mir: &Mir,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
    has_proto_entries: bool,
) -> Result<(), EmitError> {
    writer.block(
        format!("impl{impl_generics} IntoSmeltUnknown for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                // One shared cell is one JavaScript object, so the erased id is
                // derived from the cell address rather than minted fresh; see
                // `smelt_reference_object_identity` in the prelude.
                fn_writer.line(
                    "let __smelt_id = smelt_reference_object_identity(::std::rc::Rc::as_ptr(&self.0) as usize);",
                );
                // The prototype members are collected BEFORE the cell is
                // borrowed: each adapter clones the handle, and taking the
                // `Ref` first would not conflict but reads worse than keeping
                // the two phases separate.
                if has_proto_entries {
                    fn_writer.line(format!(
                        "let __smelt_proto = self.{method}();",
                        method = class_proto::PROTO_ENTRIES_METHOD,
                    ));
                }
                fn_writer.line("let __smelt_inner = self.0.borrow();");
                fn_writer.line(
                    "let mut __smelt_entries: Vec<(String, SmeltUnknown)> = Vec::from([",
                );
                for field in fields {
                    if matches!(field.visibility, smelt_hir::Visibility::Private) {
                        continue;
                    }
                    let key = mir.symbols.get(field.name).unwrap_or("field");
                    let field_name = RustIdent::new(key).into_string();
                    let value = record_field_unknown_text(
                        mir,
                        &format!("__smelt_inner.{field_name}.clone()"),
                        field.ty,
                    )
                    .unwrap_or_else(|_| "SmeltUnknown::Null".to_owned());
                    fn_writer.line(format!("({key:?}.to_owned(), {value}),"));
                }
                fn_writer.line("]);");
                if has_proto_entries {
                    fn_writer.line("__smelt_entries.extend(__smelt_proto);");
                }
                fn_writer.line("SmeltUnknown::Object(SmeltObject::with_id(__smelt_id, __smelt_entries))");
            });
        },
    );
    Ok(())
}

/// Emits a manual `Default` impl for generated storage structs with callbacks.
///
/// Rust cannot derive `Default` for fields that contain `dyn FnMut`, but Smelt
/// still needs defaultable generated option bags and locale structs. This
/// helper reuses the function emitter's canonical default expression for each
/// field so callback defaults remain callable and ordinary fields match local
/// initialization behavior.
fn emit_default_impl_for_storage_type(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
    phantom_args: &str,
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
) -> Result<(), EmitError> {
    writer.block(
        format!("impl{impl_generics} Default for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.block("Self", |self_writer| {
                    for field in fields {
                        let field_name =
                            RustIdent::new(mir.symbols.get(field.name).unwrap_or("field"))
                                .into_string();
                        let default_value =
                            FunctionEmitter::default_value_for_with_scoped_type_params(
                                mir,
                                context,
                                field.ty,
                                &TypeSubstitution::lexical(scoped_type_params),
                            )
                            .unwrap_or_else(|_| "Default::default()".to_owned());
                        self_writer.line(format!("{field_name}: {default_value},"));
                    }
                    if !phantom_args.is_empty() {
                        self_writer.line("_smelt_phantom: ::std::marker::PhantomData,");
                    }
                });
            });
        },
    );
    Ok(())
}

/// Emits a conservative `Debug` impl for storage structs that contain callbacks.
///
/// Callback trait objects cannot be formatted structurally, but parent option
/// bags still often derive `Debug` and therefore need nested storage records to
/// satisfy Rust bounds. The generated implementation names the storage type and
/// intentionally omits callable field internals.
fn emit_debug_impl_for_storage_type(
    writer: &mut CodeWriter,
    name: &str,
    impl_generics: &str,
    type_args: &str,
) {
    writer.block(
        format!("impl{impl_generics} ::std::fmt::Debug for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block(
                "fn fmt(&self, formatter: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result",
                |fn_writer| {
                    fn_writer.line(format!(
                        "formatter.debug_struct({name:?}).finish_non_exhaustive()"
                    ));
                },
            );
        },
    );
}

/// Return whether a type contains a callable value in a stored position.
///
/// Rust cannot derive `Clone` or `Debug` for `dyn FnMut` trait objects. Class
/// structs that store function fields therefore opt out of those derives until
/// callable storage grows an explicit cloneable wrapper.
fn type_contains_function(mir: &Mir, ty: TypeId) -> bool {
    match mir.types.get(ty) {
        Some(Type::Function(_)) => true,
        Some(Type::Generator { .. }) => false,
        Some(Type::GeneratorResult { .. }) => false,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
            type_contains_function(mir, *item)
        }
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            type_contains_function(mir, *key) || type_contains_function(mir, *value)
        }
        Some(Type::Tuple(items) | Type::Union(items)) => {
            items.iter().any(|item| type_contains_function(mir, *item))
        }
        Some(
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::None
            | Type::Unknown
            | Type::Never
            | Type::TypeParam { .. }
            | Type::Class { .. },
        )
        | None => false,
    }
}

/// Return whether a type supports Rust `PartialEq` in generated code.
///
/// A value class derives `PartialEq` for JavaScript structural comparison
/// (`==`/`===`/`toBe` on the by-value representation and derived comparisons in
/// generated specs) only when every stored field is itself comparable. The two
/// prelude shapes without `PartialEq` are `dyn Fn` callbacks and the
/// `Clone`-only `SmeltPromise` shared-future handle, so any type transitively
/// reaching one is rejected. Class fields are followed through their effective
/// field layout — a reference class always compares by identity (`Rc::ptr_eq`)
/// and is therefore always comparable, while a value class is comparable only
/// when its own fields are. `seen` breaks recursive class cycles by treating an
/// in-progress class as comparable (its fields are validated by the outer call).
fn type_supports_partial_eq(
    mir: &Mir,
    context: &EmitContext,
    ty: TypeId,
    seen: &mut Vec<smelt_hir::Symbol>,
) -> bool {
    match mir.types.get(ty) {
        Some(
            Type::Function(_)
            | Type::Future(_)
            | Type::Generator { .. }
            | Type::GeneratorResult { .. },
        ) => false,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
            type_supports_partial_eq(mir, context, *item, seen)
        }
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            type_supports_partial_eq(mir, context, *key, seen)
                && type_supports_partial_eq(mir, context, *value, seen)
        }
        // A concrete union lowers to a generated enum with a hand-written
        // `PartialEq` (and an erased union falls back to `SmeltUnknown`, which is
        // also `PartialEq`), so a union field is always comparable regardless of
        // its members.
        Some(Type::Union(_)) => true,
        Some(Type::Tuple(items)) => items
            .iter()
            .all(|item| type_supports_partial_eq(mir, context, *item, seen)),
        Some(Type::Class { name, .. }) => {
            let name = *name;
            // A reference class compares by identity (`Rc::ptr_eq`) with no
            // constraint on its inner fields, so it is always comparable.
            if context.is_reference_class(name) {
                return true;
            }
            let Some(class) = mir.classes.iter().find(|candidate| candidate.name == name) else {
                // A shape (interface / synthetic object-literal shape) is a
                // by-value struct that derives `PartialEq` under this same rule,
                // so a field holding one is comparable exactly when the shape's
                // own fields are. Recursing here is what keeps the two derives in
                // agreement: a shape storing a callback derives no `PartialEq`,
                // and a record storing THAT shape must not derive one either.
                if let Some(interface) = mir
                    .interfaces
                    .iter()
                    .find(|candidate| candidate.name == name)
                {
                    if seen.contains(&name) {
                        return true;
                    }
                    seen.push(name);
                    let comparable = effective_interface_fields(mir, interface)
                        .iter()
                        .all(|field| type_supports_partial_eq(mir, context, field.ty, seen));
                    seen.pop();
                    return comparable;
                }
                // An external/builtin class surface (e.g. a runtime prelude type)
                // is assumed comparable; only user classes gate the derive.
                return true;
            };
            if seen.contains(&name) {
                return true;
            }
            seen.push(name);
            let comparable = effective_class_fields(mir, class)
                .iter()
                .all(|field| type_supports_partial_eq(mir, context, field.ty, seen));
            seen.pop();
            comparable
        }
        Some(
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::None
            | Type::Unknown
            | Type::Never
            | Type::TypeParam { .. },
        )
        | None => true,
    }
}

/// Return whether all stored fields in a class are JSON-compatible.
fn class_is_json_serializable(mir: &Mir, class: &smelt_mir::MirClass) -> bool {
    let mut seen = Vec::new();
    effective_class_fields(mir, class)
        .iter()
        .all(|field| type_is_json_serializable(mir, field.ty, &mut seen))
}

/// Return whether all stored fields in an interface are JSON-compatible.
fn interface_is_json_serializable(mir: &Mir, interface: &smelt_mir::MirInterface) -> bool {
    let mut seen = vec![interface.name];
    effective_interface_fields(mir, interface)
        .iter()
        .all(|field| type_is_json_serializable(mir, field.ty, &mut seen))
}

/// Emits `IntoSmeltUnknown` for a generated record storage type.
///
/// Some lowered object operations temporarily recover string-keyed object
/// shapes from typed class/interface records. The implementation keeps that
/// conversion available without requiring every call site to know the concrete
/// record type that produced the object.
fn emit_record_into_smelt_unknown_impl(
    writer: &mut CodeWriter,
    mir: &Mir,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
) -> Result<(), EmitError> {
    writer.block(
        format!("impl{impl_generics} IntoSmeltUnknown for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                fn_writer.line(
                    "SmeltUnknown::Object(SmeltObject::new(Vec::from([",
                );
                for field in fields {
                    if matches!(field.visibility, smelt_hir::Visibility::Private) {
                        continue;
                    }
                    let key = mir.symbols.get(field.name).unwrap_or("field");
                    let field_name = RustIdent::new(key).into_string();
                    let value =
                        record_field_unknown_text(mir, &format!("self.{field_name}"), field.ty)
                            .unwrap_or_else(|_| "SmeltUnknown::Null".to_owned());
                    fn_writer.line(format!("({key:?}.to_owned(), {value}),"));
                }
                fn_writer.line("])))");
            });
        },
    );
    Ok(())
}

/// Whether a value of `ty` can be rebuilt from its erased `SmeltUnknown` view.
///
/// The whitelist behind [`emit_record_from_smelt_unknown_impl`]. Recoverable
/// shapes are the primitives, the erased carrier itself, the generated
/// containers, and generated records/unions (which now emit their own
/// `SmeltFromUnknown`). Everything else — callbacks, compiled regexes, futures,
/// generators — has no inbound impl in the prelude, so the field keeps the
/// record's `Default` rather than emitting a call that would not compile.
pub(crate) fn type_supports_from_unknown(mir: &Mir, ty: TypeId) -> bool {
    match mir.types.get(ty) {
        Some(
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::Unknown
            | Type::TypeParam { .. },
        ) => true,
        // A class is recoverable only when *this* program generates it, because
        // that is what now emits `SmeltFromUnknown`. `Type::Class` also spells
        // the runtime builtins — `TemplateOptions.escape` is a `RegExp`, and
        // `SmeltRegExp` has no inbound impl — so admitting every class emitted a
        // call that does not compile.
        Some(Type::Class { name, .. }) => {
            let name = *name;
            mir.classes.iter().any(|class| class.name == name)
                || mir.interfaces.iter().any(|item| item.name == name)
        }
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
            type_supports_from_unknown(mir, *item)
        }
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            type_supports_from_unknown(mir, *key) && type_supports_from_unknown(mir, *value)
        }
        Some(Type::Tuple(items) | Type::Union(items)) => items
            .iter()
            .all(|item| type_supports_from_unknown(mir, *item)),
        _ => false,
    }
}

/// Emits `SmeltFromUnknown` for a generated record storage type.
///
/// The inbound mirror of [`emit_record_into_smelt_unknown_impl`]. Every lifted
/// type parameter is bounded by `SmeltFromUnknown`, so a class that lacks the
/// impl cannot be used as a generic argument at all: es-toolkit's `meanBy`/
/// `medianBy` specs call generic helpers with `Person[]` and failed with "the
/// trait bound `Person: SmeltFromUnknown` is not satisfied". Only the outbound
/// half was ever emitted, which is why concrete class values could flow out to
/// erased code but never back.
///
/// The recovery is total rather than fallible: a non-object input, a missing
/// key, or a field the projection skips all fall back to `Default`, which every
/// generated record derives. Private fields are not part of the erased object
/// view, so they take that default too.
fn emit_record_from_smelt_unknown_impl(
    writer: &mut CodeWriter,
    mir: &Mir,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
) -> Result<(), EmitError> {
    writer.block(
        format!("impl{impl_generics} SmeltFromUnknown for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block(
                "fn smelt_from_unknown(value: SmeltUnknown) -> Self",
                |fn_writer| {
                    fn_writer.line("let mut result = Self::default();");
                    fn_writer.block("if let SmeltUnknown::Object(object) = value", |body| {
                        for field in fields {
                            if matches!(field.visibility, smelt_hir::Visibility::Private) {
                                continue;
                            }
                            // Only rebuild fields whose type can actually be
                            // recovered. This is a whitelist rather than a
                            // blacklist: a field the erased view cannot restore
                            // (a callback handle, a compiled regex) keeps its
                            // `Default`, and a type added later has to opt in
                            // rather than silently emit a call to a
                            // `SmeltFromUnknown` impl that does not exist.
                            if !type_supports_from_unknown(mir, field.ty) {
                                continue;
                            }
                            let key = mir.symbols.get(field.name).unwrap_or("field");
                            let field_name = RustIdent::new(key).into_string();
                            body.block(
                                format!("if let Some(field) = object.get({key:?})"),
                                |assign| {
                                    assign.line(format!(
                                        "result.{field_name} = SmeltFromUnknown::smelt_from_unknown(field);"
                                    ));
                                },
                            );
                        }
                    });
                    fn_writer.line("result");
                },
            );
        },
    );
    Ok(())
}

/// Renders a generated record field as a `SmeltUnknown` expression.
pub(crate) fn record_field_unknown_text(mir: &Mir, value_text: &str, ty: TypeId) -> Result<String, EmitError> {
    Ok(match mir.types.get(ty) {
        Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
            format!("({value_text}).into_smelt_unknown()")
        }
        Some(
            Type::None
            | Type::Never
            | Type::Future(_)
            | Type::Generator { .. }
            | Type::GeneratorResult { .. },
        )
        | None => {
            "SmeltUnknown::Null".to_owned()
        }
        Some(Type::Bool) => format!("SmeltUnknown::Bool({value_text})"),
        Some(Type::Int | Type::Float) => format!("SmeltUnknown::Number({value_text} as f64)"),
        Some(Type::String) => erased_string(value_text),
        Some(Type::Optional(inner)) => {
            let inner_text = record_field_unknown_text(mir, "value", *inner)?;
            format!("{value_text}.map_or(SmeltUnknown::Undefined, |value| {inner_text})")
        }
        Some(Type::List(item) | Type::Set(item)) => {
            let item_text = record_field_unknown_text(mir, "value", *item)?;
            format!(
                "SmeltUnknown::Array({value_text}.into_iter().map(|value| {item_text}).collect())"
            )
        }
        Some(Type::Dict(key, item)) if matches!(mir.types.get(*key), Some(Type::String)) => {
            let item_text = record_field_unknown_text(mir, "value", *item)?;
            format!(
                "SmeltUnknown::Object(SmeltObject::new({value_text}.into_iter().map(|(key, value)| (key, {item_text})).collect()))"
            )
        }
        Some(Type::Dict(_, _) | Type::JsMap(_, _) | Type::Tuple(_) | Type::Class { .. }) => {
            format!("({value_text}).into_smelt_unknown()")
        }
        Some(Type::Function(_)) => "SmeltUnknown::Null".to_owned(),
    })
}

/// Return whether generated serde can represent a type as natural JSON.
fn type_is_json_serializable(mir: &Mir, ty: TypeId, seen: &mut Vec<smelt_hir::Symbol>) -> bool {
    match mir.types.get(ty) {
        Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::Unknown) => true,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
            type_is_json_serializable(mir, *item, seen)
        }
        Some(Type::Tuple(items)) => items
            .iter()
            .all(|item| type_is_json_serializable(mir, *item, seen)),
        Some(Type::Dict(key, value)) => {
            matches!(mir.types.get(*key), Some(Type::String))
                && type_is_json_serializable(mir, *value, seen)
        }
        Some(Type::Class { name, .. }) => {
            if seen.contains(name) {
                return true;
            }
            seen.push(*name);
            let serializable =
                if let Some(class) = mir.classes.iter().find(|class| class.name == *name) {
                    effective_class_fields(mir, class)
                        .iter()
                        .all(|field| type_is_json_serializable(mir, field.ty, seen))
                } else if let Some(interface) = mir
                    .interfaces
                    .iter()
                    .find(|interface| interface.name == *name)
                {
                    effective_interface_fields(mir, interface)
                        .iter()
                        .all(|field| type_is_json_serializable(mir, field.ty, seen))
                } else {
                    false
                };
            seen.pop();
            serializable
        }
        _ => false,
    }
}

/// Return whether the MIR already contains a Rust entrypoint.
fn has_main_function(mir: &Mir) -> Result<bool, EmitError> {
    let none_ty = mir
        .types
        .all()
        .iter()
        .enumerate()
        .find_map(|(id, ty)| {
            (*ty == Type::None)
                .then(|| compact_index(id, "type index does not fit u32").map(TypeId))
        })
        .transpose()?
        .unwrap_or(TypeId(u32::MAX));
    Ok(mir.functions.iter().any(|function| {
        mir.symbols
            .get(function.name)
            .is_some_and(|name| name == "main")
            && function.return_ty == none_ty
    }))
}

/// Return whether generated code needs the first-class erased callback runtime.
fn needs_erased_function_runtime(mir: &Mir) -> bool {
    mir.types.all().iter().any(|ty| {
        let Type::Function(function) = ty else {
            return false;
        };
        if function.is_async || matches!(mir.types.get(function.return_ty), Some(Type::Future(_))) {
            return false;
        }
        let Some(0) = function.rest else {
            return false;
        };
        let [param] = function.params.as_slice() else {
            return false;
        };
        matches!(
            mir.types.get(*param),
            Some(Type::List(item))
                if matches!(
                    mir.types.get(*item),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Never)
                )
        )
    })
}

/// Reports whether the crate emits any generator state machine.
///
/// This gates both the genawaiter runtime prelude (`SmeltGenerator`,
/// `SmeltGeneratorResult`, `SmeltGeneratorCommand`) and the `genawaiter`
/// dependency. Emission is driven by a function's or closure's `is_generator`
/// flag, so a `Type::Generator` in the types table is not a reliable signal:
/// an erased or interop generator (e.g. es-toolkit's `isFunction` spec) still
/// emits `genawaiter::rc::Gen` bodies while its generator type is erased. A
/// mismatch here produced generated crates that referenced `genawaiter` and
/// `SmeltGeneratorResult` without declaring or defining them.
fn mir_uses_generators(mir: &Mir) -> bool {
    mir.functions.iter().any(|function| function.is_generator)
        || mir.closures.iter().any(|closure| closure.is_generator)
        || mir
            .types
            .all()
            .iter()
            .any(|ty| matches!(ty, Type::Generator { .. }))
}

/// Collects the dependency list required by generated Rust code.
fn generated_deps(mir: &Mir) -> Vec<GeneratedDep> {
    let mut deps = Vec::new();
    if stdlib::needs_tokio(mir) || stdlib::needs_unknown_type(mir) {
        deps.push(GeneratedDep::Tokio);
    }
    if mir_uses_generators(mir) {
        deps.push(GeneratedDep::Genawaiter);
    }
    deps.extend(
        stdlib::backend_dependencies(mir)
            .into_iter()
            .map(GeneratedDep::Stdlib),
    );
    deps
}

/// Helper struct for emitting Rust code from a MirFunction.
pub(crate) fn sanitize_ident(name: &str) -> String {
    RustIdent::new(name).into_string()
}

/// Emit the `thread_local!` block declaring every mutable global's cell.
///
/// Each `#[test]` runs on its own thread, so a `thread_local!` cell gives every
/// test a fresh copy of module state initialized from the source literal —
/// deterministic, mirroring vitest's per-file module isolation. Copy primitives
/// (`f64`/`i64`/`bool`) use a `const`-initialized [`std::cell::Cell`]; strings
/// use a [`std::cell::RefCell`] with the non-const initializer form because a
/// `.to_owned()` initializer cannot appear in a `const` block.
fn emit_mutable_globals(mir: &Mir) -> Result<String, EmitError> {
    let mut writer = CodeWriter::new();
    writer.line("thread_local! {");
    for (index, global) in mir.globals.iter().enumerate() {
        let name = global_static_name(mir, compact_index(index, "global index")?);
        let init = emitter::literals::constant_text(&global.init);
        match mir.types.get(global.ty) {
            Some(Type::String) => {
                // `init` is already the owned-string expression (`"…".to_owned()`),
                // which cannot appear in a `const` block, hence the non-const form.
                writer.line(format!(
                    "    static {name}: ::std::cell::RefCell<String> = ::std::cell::RefCell::new({init});"
                ));
            }
            Some(Type::Float) => {
                writer.line(format!(
                    "    static {name}: ::std::cell::Cell<f64> = const {{ ::std::cell::Cell::new({init}) }};"
                ));
            }
            Some(Type::Int) => {
                writer.line(format!(
                    "    static {name}: ::std::cell::Cell<i64> = const {{ ::std::cell::Cell::new({init}) }};"
                ));
            }
            Some(Type::Bool) => {
                writer.line(format!(
                    "    static {name}: ::std::cell::Cell<bool> = const {{ ::std::cell::Cell::new({init}) }};"
                ));
            }
            _ => {
                return Err(EmitError::new(
                    "mutable global has a non-primitive type; only Float/Int/Bool/String are supported",
                ));
            }
        }
    }
    writer.line("}");
    Ok(writer.finish())
}

/// Compute the thread-local static name for a mutable global by index.
///
/// The name is derived from the binding's source symbol (sanitized and
/// uppercased) with the global's dense `Mir::globals` index appended, prefixed
/// `SMELT_GLOBAL_`. Including the index disambiguates cross-module bindings
/// that share a source name so each mutable global gets a unique per-program
/// static. These names are program-specific and never enter the fixed
/// runtime-symbol registry.
pub(crate) fn global_static_name(mir: &Mir, index: u32) -> String {
    let base = usize::try_from(index)
        .ok()
        .and_then(|idx| mir.globals.get(idx))
        .and_then(|global| mir.symbols.get(global.name))
        .map_or_else(|| "global".to_owned(), sanitize_ident);
    format!("SMELT_GLOBAL_{}_{index}", base.to_uppercase())
}

#[cfg(test)]
/// Tests for the code generator.
#[cfg(test)]
mod tests;
