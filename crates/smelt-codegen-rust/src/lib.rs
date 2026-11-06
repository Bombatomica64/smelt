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

use crate::classes::{NominalStorage, type_holds_function};
use crate::{rust::erased_string, type_substitution::TypeSubstitution};
use smelt_hir::{BodyId, Type, TypeId};
use smelt_mir::{HirOrigin, Mir, MirClassProtocol, MirFunction, MirGlobalInit};

mod asymmetric_matcher_prelude;
mod builtin_member_prelude;
mod callable_registry;
mod erased_method_prelude;
mod erased_list_alias;
mod accessor_prelude;
mod bigint_prelude;
mod byte_buffer_prelude;
mod event_emitter_prelude;
mod host_value_erasure;
mod number_format_prelude;
mod console_inspect_prelude;
mod string_to_number_prelude;
mod blob_prelude;
mod fetch_types_prelude;
mod crypto_prelude;
mod host_module_prelude;
mod form_data_prelude;
mod text_codec_prelude;
mod typed_array_prelude;
mod url_prelude;
pub(crate) mod base_init;
pub(crate) mod class_proto;
pub(crate) mod bound_traits;
mod date_prelude;
pub(crate) mod classes;
pub(crate) mod classify;
pub(crate) mod lazy_view;
pub(crate) mod reference_origin;
pub(crate) mod deps;
mod function_object_prelude;
mod http_server_prelude;
mod sqlite_prelude;
// Increment 3 of the callback-generics plan made the last dormant entry point
// live: the safety valve consults `collect_bindings` and `TypeParamBinding`
// directly, so the module no longer needs a `dead_code` expectation.
pub(crate) mod generic_bindings;
pub(crate) mod generic_elision;
mod reflection_prelude;
pub(crate) mod runtime_demand;
pub(crate) mod runtime_prelude;
#[cfg(any(debug_assertions, test))]
mod runtime_symbols_check;
pub mod rust;
pub(crate) mod stdlib;
pub(crate) mod thrown;
pub(crate) mod type_substitution;

use deps::GeneratedDep;
use runtime_demand::{Need, RuntimeDemand};
mod emitter;
use classes::{
    class_impl_generics_text, class_name_text, class_type_args_text, class_type_params_text,
    effective_class_fields, effective_class_methods, effective_interface_fields,
    interface_impl_generics_text, interface_type_params_text, materialized_static_value_text,
};
use emitter::{EmitContext, FunctionEmitter};
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

/// Prelude helpers that alias an erased generic list into another erased slot.
///
/// The SmeltUnknown report classifies their call sites as a boundary; see
/// `erased_list_alias` for why.
pub use erased_list_alias::{ERASED_GENERIC_ARRAY_ALIAS_FN, ERASED_GENERIC_LIST_ALIAS_FN};
/// Name prefix of the generated erased-record-to-class conversion helpers,
/// `fn <prefix><n>(smelt_value: SmeltUnknown) -> T`.
///
/// Each helper is the inverse adapter at an erased-to-typed boundary: it
/// takes the erased value and rebuilds (or recovers) the typed class. Its
/// body used to be inlined at every call site, where it was classified as a
/// boundary through its `into_smelt_unknown` call; hoisted, the signature is
/// the one line that newly names `SmeltUnknown`, and the SmeltUnknown report
/// keys its boundary classification on this prefix so the same conversion
/// keeps the classification it had inline.
pub const RECORD_CONVERSION_HELPER_PREFIX: &str = "__smelt_from_unknown_record_";

/// Name prefix of the hoisted record-to-record adapters
/// (`EmitContext::record_adapter_helpers`).
pub const RECORD_ADAPTER_HELPER_PREFIX: &str = "__smelt_adapt_record_";
pub use emitter::typedness::{KindCounts, Typedness, TypednessCounts, TypednessMeasurement};

/// Measure how much of optimized MIR reaches Rust typed.
///
/// Counts values, property reads and calls, each concrete, generic or erased,
/// per source file. Uses the emitter's own type rendering so it matches the
/// generated crate; see `emitter::typedness`.
pub fn measure_typedness(mir: &Mir) -> Result<TypednessMeasurement, EmitError> {
    emitter::typedness::measure(mir)
}

/// Name of the prelude helper that reads an optional erased value at the
/// erased carrier: `Option<SmeltUnknown>` to `SmeltUnknown`, absence becoming
/// `undefined`.
///
/// It converts between two erased spellings of one JavaScript value and adds
/// no erasure: the value was a `SmeltUnknown` on both sides. The emitter used
/// to assert presence here (`.expect(..)`), which panicked on an absent value;
/// the helper's name is what lets the SmeltUnknown report classify the call
/// site as the boundary it is (same reasoning as `erased_list_alias`).
pub const OPTIONAL_UNKNOWN_OR_UNDEFINED_FN: &str = "smelt_optional_unknown_or_undefined";

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
    /// Fat (whole-program) LTO and one codegen unit.
    ///
    /// Thin LTO already inlines the prelude into its callers; fat LTO also
    /// optimizes across the dependency crates (hyper, tokio, serde_json) as one
    /// module. On the generated Hono server that was +3.4% requests/s on a
    /// router-and-handler-bound route, for about 25% more release build time
    /// (`blocker-logs/hono-crud-perf-round2.md`). That trade belongs to the
    /// application: a long-running server pays the build once, a library or a
    /// test corpus mostly pays it on every build. So it is opt-in, like the
    /// allocator.
    WholeProgram,
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
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EmitError {
    /// The error message.
    pub message: String,
    /// The generated function the blocker fired in, and where it came from in
    /// source: `` `buildRegExpStr` at src/router/reg-exp-router/node.ts:1234..1560 ``.
    ///
    /// Emitter blockers are raised deep in emission — a list mutation, a tuple
    /// index, a coercion — and carried a message about the SHAPE only, with
    /// nothing about where it was. A whole-crate build of Hono printed
    /// `list unshift item must match the list element type` and stopped, and
    /// pinning that meant bisecting the manifest's `exclude` list one directory
    /// at a time (two rounds' worth of notes say so: H48's closing section and
    /// H54's opening one).
    ///
    /// It is set ONCE, by the function-emission entry points, on the way out. A
    /// blocker cannot know its site (the deep helpers have no reason to hold
    /// one) and the driver cannot know the message, so the two meet here.
    /// `Option` rather than a `String` because a synthesized function has no
    /// source of its own, and because a blocker raised outside function
    /// emission (a class shape, a prelude gate) has no function to name.
    pub site: Option<String>,
}

impl std::fmt::Display for EmitError {
    /// Formats the error message for display, naming the site when known.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.site {
            Some(site) => write!(formatter, "{} (in {site})", self.message),
            None => formatter.write_str(&self.message),
        }
    }
}

impl std::fmt::Debug for EmitError {
    /// Formats the same text as [`Display`](std::fmt::Display).
    ///
    /// A failed `smelt build` reaches the terminal through `Box<dyn Error>`,
    /// which Rust's runtime prints with `Debug`. The derived form printed
    /// `EmitError { message: "list unshift item must match the list element
    /// type", site: Some("`build_reg_exp_str` at .../node.ts:4178..5003") }` --
    /// the site was there and buried in struct syntax. Delegating makes the one
    /// line a reader sees the readable one.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

impl std::error::Error for EmitError {}

impl EmitError {
    /// Creates a new emit error with the given message.
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            site: None,
        }
    }

    /// Record the generated function a blocker fired in, if it has none yet.
    ///
    /// Idempotent on purpose: emission nests (a method emits a closure emits a
    /// call), so the innermost frame that knows a site wins and the outer ones
    /// leave it alone. That is why this is a field rather than text appended to
    /// the message — appending could not tell whether a site was already there
    /// without matching on the message.
    fn with_site(mut self, site: impl FnOnce() -> String) -> Self {
        if self.site.is_none() {
            self.site = Some(site());
        }
        self
    }
}

/// Converts a compact MIR identifier into a `usize` for indexing.
fn id_index(index: u32, context: &'static str) -> Result<usize, EmitError> {
    usize::try_from(index).map_err(|_err| EmitError::new(context))
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
    let root = emit_root(mir, allocator, |_function, _context, source| Ok(Some(source)))?;
    assert_runtime_symbols_defined([root.source.as_str()])?;
    write_if_changed(
        output_dir.join("Cargo.toml"),
        &deps::cargo_toml(
            &options.crate_name,
            &root.dependencies,
            allocator,
            options.release_profile,
        ),
    )?;
    write_crate_root(&src_dir, options.crate_kind, &root.source)?;
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
    emit_crate_with_modules_reporting(mir, krate, modules, output_path, options).map(|_| ())
}

/// [`emit_crate_with_modules`], also returning the backend's erasures.
///
/// These are the erasures decided while rendering types (erased generics and
/// unions, unresolved classes, `never`). See `emitter::backend_erasure` for the
/// span each record carries. The records are unsorted and may repeat;
/// normalize them with [`smelt_hir::normalize_erasures`].
pub fn emit_crate_with_modules_reporting(
    mir: &Mir,
    krate: &smelt_hir::Crate,
    modules: &[(String, smelt_hir::ModuleId)],
    output_path: impl AsRef<Path>,
    options: &EmitOptions,
) -> Result<Vec<smelt_hir::ErasureRecord>, Box<dyn std::error::Error>> {
    let output_dir = output_path.as_ref();
    let src_dir = output_dir.join("src");
    fs::create_dir_all(&src_dir)?;
    let allocator = options.effective_allocator();
    let mapped = emit_mapped_sources(mir, krate, modules, allocator)?;
    assert_runtime_symbols_defined(
        std::iter::once(mapped.root.as_str()).chain(mapped.modules.iter().map(|module| module.source.as_str())),
    )?;
    write_if_changed(
        output_dir.join("Cargo.toml"),
        &deps::cargo_toml(
            &options.crate_name,
            &mapped.dependencies,
            allocator,
            options.release_profile,
        ),
    )?;
    write_crate_root(&src_dir, options.crate_kind, &mapped.root)?;
    for module in mapped.modules {
        let module_path = src_dir.join(format!("{}.rs", module.name));
        write_if_changed(module_path, &module.source)?;
    }
    Ok(mapped.erasures)
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
    let root = emit_root(mir, allocator, |_function, _context, source| Ok(Some(source)))?;
    assert_runtime_symbols_defined([root.source.as_str()])?;
    Ok(root.source)
}

/// Emits Rust source code while allowing callers to route free functions.
///
/// The router receives each already-emitted free function exactly once. Returning
/// `Some(source)` keeps the function in the crate root; returning `None` lets the
/// caller store it elsewhere, such as in source-shaped sibling modules.
fn emit_root(
    mir: &Mir,
    allocator: GeneratedAllocator,
    mut route_free_function: impl FnMut(
        &MirFunction,
        &EmitContext,
        String,
    ) -> Result<Option<String>, EmitError>,
) -> Result<EmittedRoot, EmitError> {
    let context = EmitContext::new(mir)?;
    // Decide, once, which free functions emit real Rust generics (signature
    // safety plus a body-cleanliness trial) so the definition and every call
    // site agree on the generic-vs-erased shape.
    context.populate_generic_functions(mir)?;
    // The remaining phases, in the order they write the crate root. Each is
    // one `CrateBuilder` method; see there for why a phase must come where it
    // does.
    let mut krate = CrateBuilder::with_prelude(mir, &context, allocator)?;
    krate.emit_mutable_globals()?;
    krate.emit_free_functions(&mut route_free_function)?;
    krate.emit_class_impls()?;
    krate.emit_bound_traits()?;
    krate.flush_hoisted_items();
    krate.splice_console_inspect()?;
    krate.emit_main_fallback()?;
    let (source, erasures) = krate.finish();
    Ok(EmittedRoot {
        source,
        erasures,
        dependencies: generated_deps(context.demand()),
    })
}

/// A finished crate root and what its manifest and report need from it.
struct EmittedRoot {
    /// The crate root's text.
    source: String,
    /// The erasures the backend decided while rendering types.
    erasures: Vec<smelt_hir::ErasureRecord>,
    /// The crates the emitted runtime links.
    dependencies: Vec<GeneratedDep>,
}

/// Fails the build, in debug builds only, when a crate's sources name a runtime
/// symbol nothing defines.
///
/// That is a missing edge in the `requires` graph of [`runtime_demand`], and
/// failing here names it where it was made rather than as an E0425 in the
/// generated crate.
#[cfg(debug_assertions)]
fn assert_runtime_symbols_defined<'a>(sources: impl IntoIterator<Item = &'a str>) -> Result<(), EmitError> {
    let undefined = runtime_symbols_check::undefined_runtime_symbols(sources);
    if undefined.is_empty() {
        return Ok(());
    }
    Err(EmitError::new(format!(
        "generated crate names runtime symbols with no definition: {}",
        undefined.join(", ")
    )))
}

/// Release builds trust the `requires` graph.
#[cfg(not(debug_assertions))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "shares its signature with the debug build's check"
)]
fn assert_runtime_symbols_defined<'a>(_sources: impl IntoIterator<Item = &'a str>) -> Result<(), EmitError> {
    Ok(())
}

/// The crate root under construction, phase by phase.
///
/// [`emit_root`] runs the phases in order:
/// generic decision (before any body is rendered), runtime prelude ending in
/// [`PRELUDE_END_MARKER`], module globals, free functions, class `impl`s,
/// the hoisted items those bodies minted, the `util.inspect` runtime spliced in
/// ahead of the marker, and the `fn main` fallback. The builder owns the
/// output text and the marker position, so a later phase cannot lose the
/// prelude boundary it splices at.
struct CrateBuilder<'a> {
    /// The program being emitted.
    mir: &'a Mir,
    /// The shared crate-level emission context.
    context: &'a EmitContext,
    /// The crate root text so far.
    out: String,
    /// Byte offset of the prelude end marker in `out` (where the
    /// `util.inspect` runtime is spliced).
    prelude_end: usize,
    /// Whether the crate carries the erased `SmeltUnknown` carrier (decided by
    /// the prelude, read by the class phase).
    needs_unknown: bool,
}

impl<'a> CrateBuilder<'a> {
    /// Emit the runtime prelude and the prelude end marker.
    fn with_prelude(
        mir: &'a Mir,
        context: &'a EmitContext,
        allocator: GeneratedAllocator,
    ) -> Result<Self, EmitError> {
        let (mut out, needs_unknown) = emit_runtime_prelude(mir, context, allocator)?;

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
        let prelude_end = out.len();
        out.push_str(&format!("\n\n{PRELUDE_END_MARKER}\n"));
        Ok(Self {
            mir,
            context,
            out,
            prelude_end,
            needs_unknown,
        })
    }

    /// Emit the module-level mutable globals.
    fn emit_mutable_globals(&mut self) -> Result<(), EmitError> {
        let (mir, context, out) = (self.mir, self.context, &mut self.out);
        // Emit one thread-local cell per module-level mutable global. These are
        // program-specific mangled names (not fixed runtime symbols): each `#[test]`
        // thread observes fresh module state initialized from the source literal,
        // deterministic and mirroring vitest's per-file module isolation. Copy
        // primitives use a `const`-initialized `Cell`; strings use the non-const
        // `RefCell` form because a `.to_owned()` initializer cannot be `const`.
        if !mir.globals.is_empty() {
            out.push('\n');
            out.push_str(&emit_mutable_globals(mir, context)?);
        }
        Ok(())
    }

    /// Emit every free function, handing each to `route_free_function`.
    fn emit_free_functions(&mut self,
        route_free_function: &mut impl FnMut(&MirFunction, &EmitContext, String) -> Result<Option<String>, EmitError>,
    ) -> Result<(), EmitError> {
        let (mir, context, out) = (self.mir, self.context, &mut self.out);
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
            let mut emitter = FunctionEmitter::new(mir, context, function)?;
            let mut emitted_function_source = String::new();
            emitter.emit(&mut emitted_function_source)?;
            let routed_function_source =
                route_free_function(function, context, emitted_function_source)?;
            let Some(root_function_source) = routed_function_source else {
                continue;
            };
            if has_emitted_root_function || !mir.classes.is_empty() {
                out.push('\n');
            }
            out.push_str(&root_function_source);
            has_emitted_root_function = true;
        }
        Ok(())
    }

    /// Emit every class `impl` block (constructors, base initializers,
    /// inherited and own methods, erased-view members, protocol impls).
    fn emit_class_impls(&mut self) -> Result<(), EmitError> {
        let needs_unknown = self.needs_unknown;
        let (mir, context, out) = (self.mir, self.context, &mut self.out);
        let mut emitted_impl_names = HashSet::new();
        for class in &mir.classes {
            let name = class_name_text(mir, class)?;
            if !emitted_impl_names.insert(name.clone()) {
                continue;
            }
            let impl_generics = class_impl_generics_text(mir, context, class)?;
            let type_args = class_type_args_text(mir, context, class)?;
            out.push_str(&format!("\nimpl{impl_generics} {name}{type_args} {{\n"));
            if !class.is_abstract
                && let Some(constructor) = class.constructor
                && let Some(function) = mir.functions.get(id_index(
                    constructor.0,
                    "constructor index does not fit usize",
                )?)
            {
                let mut emitter = FunctionEmitter::new(mir, context, function)?;
                emitter.emit_method(out)?;
            }
            // Each ancestor constructor this class's `super(..)` chain runs is
            // re-emitted here as an initializer over `Self`, exactly as inherited
            // methods are below: see `base_init` for why the base body must run on
            // the derived instance rather than on a separately constructed base.
            for base_constructor in base_init::base_initializer_chain(mir, class)? {
                if let Some(function) = mir.functions.get(id_index(
                    base_constructor.0,
                    "base constructor index does not fit usize",
                )?) {
                    let substituted = classes::substituted_inherited_member(mir, function, class.name);
                    let copied_body = substituted.as_ref().unwrap_or(function);
                    let mut emitter =
                        FunctionEmitter::base_initializer_copy(mir, context, copied_body, class.name)?;
                    emitter.emit_method(out)?;
                }
            }
            // Inherited methods are emitted into the subclass's own impl block:
            // Smelt flattens inheritance and Rust has no method inheritance, so a
            // base method is otherwise not callable on a subclass receiver.
            for method in &effective_class_methods(mir, class) {
                if let Some(function) = mir
                    .functions
                    .get(id_index(method.0, "method index does not fit usize")?)
                {
                    let substituted = classes::substituted_inherited_member(mir, function, class.name);
                    let copied_body = substituted.as_ref().unwrap_or(function);
                    // A method whose declaring class is not this impl's class is an
                    // INHERITED copy: its polymorphic-`this` return type belongs to
                    // the impl, not to the class that declared it.
                    let mut emitter = if matches!(
                        function.origin,
                        HirOrigin::ClassMethod { class: owner, .. } if owner == class.name
                    ) {
                        FunctionEmitter::new(mir, context, copied_body)?
                    } else {
                        FunctionEmitter::inherited_copy(mir, context, copied_body, class.name)?
                    };
                    emitter.emit_method(out)?;
                }
            }
            for static_method in &class.static_methods {
                if let Some(function) = mir.functions.get(id_index(
                    static_method.0,
                    "static method index does not fit usize",
                )?) {
                    let mut emitter = FunctionEmitter::new(mir, context, function)?;
                    emitter.emit_method(out)?;
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
                    class_proto::class_proto_entries_method(mir, context, class)?
            {
                out.push_str(&proto_entries);
            }
            if needs_unknown && let Some(host) = field_setter_host(mir, context, class) {
                let emitter = FunctionEmitter::new(mir, context, host)?;
                out.push_str(
                    &emitter.reference_field_setter_method_text(&effective_class_fields(mir, class))?,
                );
                let fields = effective_class_fields(mir, class);
                if emitter::view_write_through::has_live_callable_fields(
                    mir,
                    context.program_reads_this(),
                    &fields,
                ) {
                    out.push_str(&emitter.reference_callable_field_getter_method_text(&fields)?);
                }
            }
            out.push_str("}\n");
            for protocol in &class.protocols {
                match protocol {
                    MirClassProtocol::Add { method } => {
                        let function = mir
                            .functions
                            .get(id_index(method.0, "add protocol method index does not fit usize")?)
                            .ok_or_else(|| EmitError::new("add protocol method is missing"))?;
                        let mut emitter = FunctionEmitter::new(mir, context, function)?;
                        emitter.emit_python_add_impl(
                            out,
                            &name,
                            &impl_generics,
                            &type_args,
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Emit the generated traits of class- and interface-bounded type
    /// parameters (`trait AnimalLike` for `A extends Animal`).
    ///
    /// Must run after [`Self::emit_class_impls`]: each trait impl delegates to
    /// the inherent methods, virtual slots, or stored fields those impls define.
    fn emit_bound_traits(&mut self) -> Result<(), EmitError> {
        let (mir, context, out) = (self.mir, self.context, &mut self.out);
        out.push_str(&emitter::bound_dispatch::emit_bound_traits(mir, context)?);
        Ok(())
    }

    /// Flush the module-level items the bodies above minted into the context:
    /// function-item accessors and hoisted record conversion/adapter helpers.
    fn flush_hoisted_items(&mut self) {
        let (context, out) = (self.context, &mut self.out);
        // Flushed HERE, after every free function AND every class impl body has been
        // emitted: a function-item value can first be erased inside a constructor or
        // method body, which the free-function loop above skips and the class loop
        // below emits. Minting the accessor's name and emitting its definition are
        // one decision, so the flush has to sit after the last place that can mint
        // one (was `E0425: __smelt_fn_value_<key> not in scope` for a named function
        // used as a value only from a method body).
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

        // Flush the per-function-item accessors for TYPED callable values (see
        // `EmitContext::function_item_typed_accessors`): one cached `Rc` per item
        // and rendered callable type, so every typed reference to a named function
        // shares one identity. The wrapper is bound at its declared type first so
        // the unsizing coercion to `Rc<dyn Fn..>` happens at a `let`; the type is
        // spelled once, as an alias, rather than restated in each of the three
        // positions that name it.
        for ((key, type_text), (ordinal, body)) in context.function_item_typed_accessors.borrow().iter() {
            out.push_str(&format!(
                "\ntype SmeltFnTyped{key}_{ordinal} = {type_text};\nfn __smelt_fn_typed_{key}_{ordinal}() -> SmeltFnTyped{key}_{ordinal} {{\n    thread_local! {{ static SMELT_FN_TYPED: ::std::cell::OnceCell<SmeltFnTyped{key}_{ordinal}> = ::std::cell::OnceCell::new(); }}\n    SMELT_FN_TYPED.with(|cell| cell.get_or_init(|| {{ let smelt_function: SmeltFnTyped{key}_{ordinal} = {body}; smelt_function }}).clone())\n}}\n"
            ));
        }

        // Flush the hoisted erased-record-to-class conversions (see
        // `EmitContext::record_conversion_helpers`): each recovered class type's
        // conversion is spelled once here and called from every site.
        for (type_text, (ordinal, body)) in context.record_conversion_helpers.borrow().iter() {
            out.push_str(&format!(
                "\nfn {RECORD_CONVERSION_HELPER_PREFIX}{ordinal}(smelt_value: SmeltUnknown) -> {type_text} {{\n    {body}\n}}\n"
            ));
        }
        // The self-reaching record-to-record adapters, likewise spelled once.
        for ((source_text, target_text), (ordinal, body)) in context.record_adapter_helpers.borrow().iter() {
            out.push_str(&format!(
                "\nfn {RECORD_ADAPTER_HELPER_PREFIX}{ordinal}(smelt_value: {source_text}) -> {target_text} {{\n    {body}\n}}\n"
            ));
        }
    }

    /// Splice the `util.inspect` runtime in ahead of the prelude end marker,
    /// when a body used it.
    fn splice_console_inspect(&mut self) -> Result<(), EmitError> {
        // The `util.inspect` formatter is pay-for-use like the rest of the
        // prelude, but whether a `console.log` argument needs it is decided while
        // the bodies above are emitted, so it is spliced in ahead of the prelude
        // marker only now. See `emitter::console_inspect`.
        let console_inspect = emitter::emit_console_inspect_support(self.mir, self.context)?;
        if !console_inspect.is_empty() {
            // The marker position was recorded when the prelude phase wrote it, so
            // the splice point cannot be lost (it used to be searched for, and a
            // missing marker silently dropped the runtime).
            self.out
                .insert_str(self.prelude_end, &format!("\n\n{}", console_inspect.trim_end()));
        }
        Ok(())
    }

    /// Give a crate whose program has no `main` an empty one.
    fn emit_main_fallback(&mut self) -> Result<(), EmitError> {
        let (mir, out) = (self.mir, &mut self.out);
        if !has_main_function(mir)? {
            out.push_str("\nfn main() {}\n");
        }
        Ok(())
    }

    /// The finished crate root and the backend's erasure records.
    fn finish(self) -> (String, Vec<smelt_hir::ErasureRecord>) {
        (self.out, self.context.take_backend_erasures())
    }
}

/// Emit the runtime prelude: every runtime type and helper the program needs,
/// up to (not including) the prelude end marker. Also answers whether the
/// crate carries the erased `SmeltUnknown` carrier.
fn emit_runtime_prelude(
    mir: &Mir,
    context: &EmitContext,
    allocator: GeneratedAllocator,
) -> Result<(String, bool), EmitError> {
    let mut writer = CodeWriter::new();
    let demand = context.demand();
    let needs_serde_json = demand.has(Need::DepSerdeJson);
    let needs_unknown = demand.has(Need::Unknown);
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
    let (before_interfaces, after_interfaces) = runtime_prelude::split_at_interfaces();
    runtime_prelude::emit_entries(&mut writer, before_interfaces, mir, context)?;
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
        let type_params = interface_type_params_text(mir, context, interface)?;
        let impl_generics = interface_impl_generics_text(mir, context, interface)?;
        let fields = effective_interface_fields(mir, interface);
        // A shape whose fields are written after construction is a reference
        // record: it needs the shared-cell handle so aliases observe the write,
        // exactly as a mutated class does. See `classify::reference_classes`.
        if context.is_reference_class(interface.name) {
            emit_reference_record_storage(
                &mut writer,
                mir,
                context,
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
                    declared_name: interface.name,
                    // An object shape has no method bodies to bind.
                    has_proto_entries: false,
                    has_field_setter: false,
                },
                needs_unknown,
            )?;
            continue;
        }
        let has_function_field = fields
            .iter()
            .any(|field| type_holds_function(mir, field.ty, NominalStorage::Opaque));
        // An interface-backed record is a by-value struct, exactly like a value
        // class, so it supports structural equality (JS `==`/`===`/`toBe`, and
        // derived comparisons in generated specs) under the same rule: every
        // stored field must itself be `PartialEq`. Before shape structs existed
        // this rarely mattered, because a comparable record was usually spelled
        // as a dict; a statically-shaped object literal now lands here instead,
        // and comparing two of them must keep working.
        let interface_supports_partial_eq = fields
            .iter()
            .all(|field| type_supports_partial_eq(mir, context, field.ty, &mut Vec::new()));
        let interface_partial_eq_derive = if interface_supports_partial_eq {
            ", PartialEq"
        } else {
            ""
        };
        if has_function_field {
            writer.line("#[derive(Clone)]");
            writer.line("#[allow(dead_code)]");
        } else if needs_serde_json
            && interface_is_json_serializable(mir, interface)
            && !fields
                .iter()
                .any(|field| accessor_prelude::is_accessor_field(mir, interface.name, field.name))
        {
            writer.line(format!(
                "#[derive(Clone, Debug, Default{interface_partial_eq_derive}, serde::Serialize, serde::Deserialize)]"
            ));
        } else {
            writer.line(format!(
                "#[derive(Clone, Debug, Default{interface_partial_eq_derive})]"
            ));
        }
        // Only the parameters the emitted Rust declares: a position
        // `generic_elision` dropped is spelled nowhere, so it must not appear in
        // the `PhantomData` filler or in any hand-written impl's generic list
        // either (see `crate::generic_elision`).
        let interface_param_idents = context.type_param_elision().retain_carried(
            interface.name,
            interface
                .type_params
                .iter()
                .map(|param| {
                    mir.symbols
                        .get(param.name)
                        .map(|param_name| RustIdent::new(param_name).into_string())
                        .ok_or_else(|| {
                            EmitError::new("interface type parameter has unknown symbol")
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
        let phantom_args = interface_param_idents.join(", ");
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
                    context,
                    field.ty,
                    &TypeSubstitution::lexical(&scoped_type_params),
                )
                .unwrap_or_else(|_| "SmeltUnknown".to_owned());
                let field_ty = if accessor_prelude::is_accessor_field(mir, interface.name, field.name) {
                    accessor_prelude::storage_type_text(&field_ty)
                } else {
                    field_ty
                };
                block_writer.line(format!("{field_name}: {field_ty},"));
            }
            if !interface_param_idents.is_empty() {
                block_writer.line(format!(
                    "_smelt_phantom: ::std::marker::PhantomData<({phantom_args})>,"
                ));
            }
        });
        if has_function_field {
            emit_default_impl_for_storage_type(
                &mut writer,
                mir,
                context,
                Some(interface.name),
                &name,
                &impl_generics,
                &type_params,
                &fields,
                &phantom_args,
                &scoped_type_params,
                &interface_param_idents,
            )?;
            emit_debug_impl_for_storage_type(&mut writer, &name, &impl_generics, &type_params);
        }
        if needs_unknown {
            emit_record_into_smelt_unknown_impl(
                &mut writer,
                mir,
                context,
                Some(interface.name),
                &scoped_type_params,
                &name,
                &impl_generics,
                &type_params,
                &fields,
                &interface.decode_facts,
            )?;
            emit_record_from_smelt_unknown_impl(
                &mut writer,
                mir,
                context,
                &name,
                &impl_generics,
                &type_params,
                &fields,
                &scoped_type_params,
            )?;
        }
        writer.blank_line();
    }
    runtime_prelude::emit_entries(&mut writer, after_interfaces, mir, context)?;
    if !context.weak_receivers().is_empty() {
        emitter::weak_receiver::emit_prelude(&mut writer, needs_unknown, context.demand().has(Need::ErasedFunction));
    }
    for class in &mir.classes {
        let name = class_name_text(mir, class)?;
        if !emitted_class_names.insert(name.clone()) {
            continue;
        }
        if context.is_reference_class(class.name) {
            emit_reference_class_storage(&mut writer, mir, context, class, needs_unknown)?;
            continue;
        }
        let type_params = class_type_params_text(mir, context, class)?;
        let impl_generics = class_impl_generics_text(mir, context, class)?;
        let mut field_lines = Vec::new();
        let fields = effective_class_fields(mir, class);
        let scoped_type_params = class
            .type_params
            .iter()
            .map(|param| param.name)
            .collect::<HashSet<_>>();
        // Only the parameters the emitted Rust declares; see the sibling
        // comment on the interface path and `crate::generic_elision`.
        let class_param_idents = context.type_param_elision().retain_carried(
            class.name,
            class
                .type_params
                .iter()
                .map(|param| {
                    mir.symbols
                        .get(param.name)
                        .map(|param_name| RustIdent::new(param_name).into_string())
                        .ok_or_else(|| EmitError::new("class type parameter has unknown symbol"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        );
        let has_function_field = fields
            .iter()
            .any(|field| type_holds_function(mir, field.ty, NominalStorage::Opaque));
        // A value class supports structural equality (JS `==`/`===`/`toBe` on the
        // by-value representation, and derived comparisons in generated specs)
        // only when every stored field is itself `PartialEq`. Function fields
        // (`dyn Fn`) and promise fields (`SmeltPromise`, a `Clone`-only shared
        // future handle) are the two prelude shapes without `PartialEq`, so the
        // derive is gated on their absence.
        let supports_partial_eq = fields.iter().all(|field| {
            type_supports_partial_eq(mir, context, field.ty, &mut Vec::new())
        });
        let partial_eq_derive = if supports_partial_eq { ", PartialEq" } else { "" };
        // A GENERIC value class cannot use these derives at all. Every one of
        // them generates an impl bounded by its own trait and nothing more
        // (`impl<T: Clone> Clone`, `impl<T: Default> Default`, …), which is not
        // enough as soon as a field's type is another generated class: those
        // carry the full generated bound set
        // (`Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static`,
        // see `class_impl_generics_text`). Hono's
        // `struct TrieRouter<T> { root: Node<T> }` reported "the trait
        // `Clone`/`Default`/`IntoSmeltUnknown`/`SmeltFromUnknown` is not
        // implemented for `T`" once per derive (H29).
        //
        // The bounds are NOT moved onto the struct declaration: a bounded
        // declaration propagates to every generated type that mentions this one,
        // and that cascade is why the concrete-union emitter keeps its `pub enum`
        // bare and spells out each impl with the bound set it needs. Same choice
        // here — the impls are emitted by hand just below with `impl_generics`.
        // A class every one of whose parameters was elided is NOT generic in
        // Rust: it declares no parameters, so the ordinary derives apply and no
        // hand-written bounded impl is needed.
        let is_generic = !class_param_idents.is_empty();
        if has_function_field {
            writer.line("#[derive(Clone)]");
            writer.line("#[allow(dead_code)]");
        } else if is_generic {
            writer.line("#[allow(dead_code)]");
        } else if needs_serde_json && class_is_json_serializable(mir, class) {
            writer.line(format!(
                "#[derive(Clone, Debug, Default{partial_eq_derive}, serde::Serialize, serde::Deserialize)]"
            ));
        } else {
            writer.line(format!("#[derive(Clone, Debug, Default{partial_eq_derive})]"));
        }
        // The field types are kept as text as well: a hand-written `Clone` or
        // `PartialEq` bounds them in a `where` clause, which is the exact
        // requirement its body has (see the derive-gating comment above).
        let mut field_type_texts = Vec::with_capacity(fields.len());
        for field in &fields {
            let field_type_text = FunctionEmitter::type_text_for_with_scoped_type_params(
                mir,
                context,
                field.ty,
                &TypeSubstitution::lexical(&scoped_type_params),
            )?;
            field_lines.push(format!(
                "{}: {field_type_text},",
                RustIdent::new(
                    mir.symbols
                        .get(field.name)
                        .ok_or_else(|| EmitError::new("field has unknown symbol"))?
                ),
            ));
            field_type_texts.push(field_type_text);
        }
        if !class_param_idents.is_empty() {
            let phantom_args = class_param_idents.join(", ");
            field_lines.push(format!(
                "_smelt_phantom: ::std::marker::PhantomData<({phantom_args})>,"
            ));
        }
        writer.block(format!("struct {name}{type_params}"), |block_writer| {
            for field_line in field_lines {
                block_writer.line(field_line);
            }
        });
        if has_function_field || is_generic {
            let phantom_args = class_param_idents.join(", ");
            // The generated bound set, plus the field-type `where` clause the
            // emitter adds. See `derive_generics_text` for why both are needed,
            // and `classes::GENERATED_TYPE_PARAM_BOUNDS` for why the set is the
            // full one rather than the derive-equivalent `T: Default`.
            let default_generics = if is_generic {
                derive_generics_text(
                    &class_param_idents,
                    classes::GENERATED_TYPE_PARAM_BOUNDS,
                )
            } else {
                impl_generics.clone()
            };
            emit_default_impl_for_storage_type(
                &mut writer,
                mir,
                context,
                None,
                &name,
                &default_generics,
                &type_params,
                &fields,
                &phantom_args,
                &scoped_type_params,
                &class_param_idents,
            )?;
            // `Debug` names the struct and stops, so it needs no bound at all.
            let debug_generics = if is_generic {
                type_params.clone()
            } else {
                impl_generics.clone()
            };
            emit_debug_impl_for_storage_type(&mut writer, &name, &debug_generics, &type_params);
        }
        // A generic value class also loses its `Clone` and `PartialEq` derives
        // for the same reason, so those are spelled out too. The callback case
        // keeps `#[derive(Clone)]` (its `dyn Fn` field is an `Rc`, and it has no
        // `PartialEq` to begin with), so this is the generic case only.
        //
        // Both bound their FIELD TYPES in a `where` clause rather than bounding
        // `T`. That is the exact requirement of a field-by-field body, and it is
        // the only formulation that is right for both shapes at once:
        // `Ok<T> { value: T }` needs `T: Clone` (so a union deriving `Clone`
        // over an `Ok<T>` arm can still satisfy it), while
        // `TrieRouter<T> { root: Node<T> }` needs `Node<T>: Clone`, which pulls
        // the whole generated bound set — a requirement of `Node`, not of
        // `TrieRouter`, and one only its own use sites can discharge.
        if is_generic && !has_function_field {
            let field_names = fields
                .iter()
                .map(|field| {
                    mir.symbols
                        .get(field.name)
                        .map(|field_name| RustIdent::new(field_name).into_string())
                        .ok_or_else(|| EmitError::new("field has unknown symbol"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            emit_clone_impl_for_value_class(
                &mut writer,
                &name,
                &type_params,
                &type_params,
                &field_names,
                &field_type_texts,
                &class_param_idents,
                true,
            );
            if supports_partial_eq {
                emit_partial_eq_impl_for_value_class(
                    &mut writer,
                    &name,
                    &type_params,
                    &type_params,
                    &field_names,
                    &field_type_texts,
                    &class_param_idents,
                );
            }
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
            emit_record_into_smelt_unknown_impl(
                &mut writer,
                mir,
                context,
                None,
                &scoped_type_params,
                &name,
                &impl_generics,
                &type_params,
                &fields,
                &[],
            )?;
            emit_record_from_smelt_unknown_impl(
                &mut writer,
                mir,
                context,
                &name,
                &impl_generics,
                &type_params,
                &fields,
                &scoped_type_params,
            )?;
        }
        writer.blank_line();
    }

    let union_definitions = emitter::emit_union_definitions(mir, context)?;
    if !union_definitions.is_empty() {
        for line in union_definitions.lines() {
            writer.line(line);
        }
    }

    // Strict JSON entry decoders (`emitter::json_decode`): boundary adapters
    // from the parsed JSON value into each source-declared target's concrete
    // type. Emitted beside the union definitions they construct.
    let json_decoders = emitter::emit_json_decoders(mir, context)?;
    if !json_decoders.is_empty() {
        for line in json_decoders.lines() {
            writer.line(line);
        }
    }

    Ok((writer.finish(), needs_unknown))
}

/// Rust sources split by original source module.
struct MappedSources {
    /// The crates the emitted runtime links.
    dependencies: Vec<GeneratedDep>,
    /// Crate root source.
    root: String,
    /// Erasures the backend decided while emitting; see
    /// `emitter::backend_erasure`.
    erasures: Vec<smelt_hir::ErasureRecord>,
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

    let EmittedRoot {
        source: mut root,
        erasures,
        dependencies,
    } = emit_root(mir, allocator, |function, context, function_source| {
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
        erasures,
        dependencies,
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
    // Decided on the SOURCE stem: `sanitize_ident` renames an identifier
    // spelled `main` (the binary's entry point), which would otherwise turn
    // `main.ts` into `main_.rs` instead of the established `source_main.rs`.
    if name == "main" || name == "lib" {
        return format!("source_{name}");
    }
    sanitize_ident(name)
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
            type_params: class_type_params_text(mir, context, class)?,
            type_args: class_type_args_text(mir, context, class)?,
            impl_generics: class_impl_generics_text(mir, context, class)?,
            fields: effective_class_fields(mir, class),
            static_fields: &class.static_fields,
            type_param_names: class.type_params.iter().map(|param| param.name).collect(),
            declared_name: class.name,
            has_proto_entries: class_proto::class_has_proto_entries(mir, context, class),
            has_field_setter: field_setter_host(mir, context, class).is_some(),
        },
        needs_unknown,
    )
}

/// Everything the reference (handle) representation needs about one record type.
///
/// A reference record is emitted identically whether the source spelled it as a
/// `class` or as an object *shape* (an `interface`, or an inline object type
/// literal lowered to a synthetic one). Both are JavaScript objects, so both need the same handle newtype
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
    ///
    /// This is the FULL declared list: an elided parameter stays in lexical
    /// scope so a stray occurrence renders its name and fails loudly with
    /// `cannot find type`, rather than silently erasing to `SmeltUnknown`.
    /// What the emitted Rust declares is [`Self::declared_name`]'s carried
    /// subset (see `crate::generic_elision`).
    type_param_names: Vec<smelt_hir::Symbol>,
    /// Name symbol of the declaring class or interface, for the elision map.
    declared_name: smelt_hir::Symbol,
    /// Whether the type emits `__smelt_proto_entries` (see [`crate::class_proto`]).
    ///
    /// Only a `class` has method bodies to bind; an object *shape* has none, so
    /// it is always `false` there.
    has_proto_entries: bool,
    /// Whether the type emits the erased-view write-through setter (see
    /// `emitter::view_write_through`); always `false` for a shape.
    has_field_setter: bool,
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
        declared_name,
        has_proto_entries,
        has_field_setter,
    } = shape;
    let inner_name = format!("{name}Inner");
    // Fields holding a closure that captures this instance weakly
    // (`emitter::weak_receiver`): the handle exposes its cell to that capture,
    // and the erasure keeps the instance alive through those fields' values.
    let weak_fields: HashSet<smelt_hir::Symbol> = fields
        .iter()
        .filter(|field| context.weak_receivers().is_weak_field(*declared_name, field.name))
        .map(|field| field.name)
        .collect();
    if !weak_fields.is_empty() {
        writer.line(emitter::weak_receiver::handle_impl_text(name, impl_generics, type_args));
        let checked = fields
            .iter()
            .filter(|field| weak_fields.contains(&field.name))
            .map(|field| {
                (
                    RustIdent::new(mir.symbols.get(field.name).unwrap_or("field")).into_string(),
                    emitter::weak_receiver::closure_slot(mir, field.ty).unwrap_or(""),
                )
            })
            .collect::<Vec<_>>();
        writer.line(emitter::weak_receiver::drop_impl_text(name, type_params, type_args, &checked));
        writer.blank_line();
    }
    let scoped_type_params = type_param_names.iter().copied().collect::<HashSet<_>>();
    let has_function_field = fields
        .iter()
        .any(|field| type_holds_function(mir, field.ty, NominalStorage::Opaque));
    let record_param_idents = context.type_param_elision().retain_carried(
        *declared_name,
        type_param_names
            .iter()
            .map(|param| {
                mir.symbols
                    .get(*param)
                    .map(|param_name| RustIdent::new(param_name).into_string())
                    .ok_or_else(|| EmitError::new("record type parameter has unknown symbol"))
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let phantom_args = record_param_idents.join(", ");

    // The handle newtype. `#[derive(Clone)]` is intentionally NOT used: a derived
    // clone would require `Inner: Clone` and clone the cell contents. We hand-
    // write `Rc::clone` below so a clone shares identity.
    writer.line("#[allow(dead_code)]");
    writer.line(format!(
        "struct {name}{type_params}(::std::rc::Rc<::std::cell::RefCell<{inner_name}{type_args}>>);"
    ));

    // The inner record. `Debug`/`Default` are derived only when the record is
    // NON-GENERIC and no callback field blocks the derives.
    //
    // A derive on a generic struct generates `impl<T: Debug> Debug` /
    // `impl<T: Default> Default` — its own bound and nothing more. That is not
    // enough whenever a field's type is another generated class, because those
    // carry the full generated bound set
    // (`Clone + Default + IntoSmeltUnknown + SmeltFromUnknown + 'static`, see
    // `class_impl_generics_text`). Hono's `SmartRouterInner<T>` holds a
    // `SmeltList<Router<T>>` and `NodeInner<T>` a `SmeltRecord<String, Node<T>>`,
    // so `derive(Default)` demanded `Router<T>: Default` with only `T: Default`
    // in scope: "the trait `Clone`/`Default`/`IntoSmeltUnknown`/
    // `SmeltFromUnknown` is not implemented for `T`" (H29).
    //
    // The bounds do NOT go on the struct declaration: a bounded declaration
    // propagates to every type that mentions it — starting with the handle
    // newtype whose field is `Rc<RefCell<Inner<T>>>` — and that cascade is why
    // the concrete-union emitter keeps its `pub enum` declaration bare and
    // hand-writes each impl with the bound set it needs. Same choice here, and
    // both hand-written impls already exist for the callback case.
    if has_function_field || !type_params.is_empty() {
        writer.line("#[allow(dead_code)]");
        writer.block(
            format!("struct {inner_name}{type_params}"),
            |block_writer| {
                emit_reference_inner_fields(block_writer, mir, context, fields, &scoped_type_params, &phantom_args);
            },
        );
        // Per-trait bounds. `Debug` names the struct and stops, so it needs no
        // bound at all. `Default` DOES take the full generated set
        // (`classes::GENERATED_TYPE_PARAM_BOUNDS`): its body constructs every
        // field's default explicitly, and a field default can reach another
        // generated class — Hono's `Hono_1Inner<E, S, BasePath, CurrentPath>`
        // defaults its `onError`/`notFound` slots to closures RETURNING
        // `Hono_1<E, S, BasePath, CurrentPath>`, so the body needs
        // `Hono_1: Default`, which is emitted with the full set. The
        // derive-equivalent `T: Default` cannot prove it, and no `where` clause
        // on a field's TYPE reaches it either, because the requirement comes
        // from the default EXPRESSION and not from the slot's `Rc<dyn Fn(..)>`
        // type. Since every generic item the crate emits carries the same set,
        // asking for it here cannot fail for an in-crate consumer, and a
        // parameter instantiated at a concrete generated type satisfies it by
        // construction.
        let (default_generics, debug_generics) = if type_params.is_empty() {
            (impl_generics.clone(), impl_generics.clone())
        } else {
            (
                derive_generics_text(
                    &record_param_idents,
                    classes::GENERATED_TYPE_PARAM_BOUNDS,
                ),
                type_params.clone(),
            )
        };
        emit_default_impl_for_storage_type(
            writer,
            mir,
            context,
            None,
            &inner_name,
            &default_generics,
            type_args,
            fields,
            &phantom_args,
            &scoped_type_params,
            &record_param_idents,
        )?;
        emit_debug_impl_for_storage_type(writer, &inner_name, &debug_generics, type_args);
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
            &weak_fields,
            *has_proto_entries,
            *has_field_setter,
            *has_field_setter
                && emitter::view_write_through::has_live_callable_fields(
                    mir,
                    context.program_reads_this(),
                    fields,
                ),
        )?;
        // The inbound half restores the live handle (see `reference_origin`).
        reference_origin::emit_reference_from_smelt_unknown_impl(
            writer,
            mir,
            context,
            name,
            impl_generics,
            type_args,
            fields,
        );
    }
    writer.blank_line();
    Ok(())
}

/// The function whose emitter renders a reference class's erased-view field
/// setter (see `emitter::view_write_through`), when the class gets one.
///
/// Any function of the class works — the setter only needs the class's type
/// parameters in its render scope — so the constructor is used, falling back
/// to the first method. `None` when the program has no erased property write
/// (the machinery is pay-for-use), when the class is not a reference class
/// (a by-value instance has no identity for a view to stand for), or when the
/// class has no function at all.
pub(crate) fn field_setter_host<'mir>(
    mir: &'mir Mir,
    context: &EmitContext,
    class: &smelt_mir::MirClass,
) -> Option<&'mir MirFunction> {
    if !context.erased_view_write_through() || !context.is_reference_class(class.name) {
        return None;
    }
    class
        .constructor
        .into_iter()
        .chain(effective_class_methods(mir, class))
        .find_map(|function| mir.functions.get(usize::try_from(function.0).ok()?))
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
    weak_fields: &HashSet<smelt_hir::Symbol>,
    has_proto_entries: bool,
    has_field_setter: bool,
    has_live_reads: bool,
) -> Result<(), EmitError> {
    writer.block(
        format!("impl{impl_generics} IntoSmeltUnknown for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                // One shared cell is one JavaScript object, so the erased id is
                // derived from the cell address rather than minted fresh; see
                // `smelt_reference_object_identity` in the prelude.
                fn_writer.line(
                    "let __smelt_id = smelt_reference_object_identity(&self.0);",
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
                    if !field.visibility.is_own_property() {
                        continue;
                    }
                    // The erased key is the SOURCE spelling (`mir.names`), the
                    // Rust field is the sanitized one: `onRead` must read back
                    // as `"onRead"` from an erased view, not as `"on_read"`.
                    let field_name = RustIdent::new(mir.symbols.get(field.name).unwrap_or("field"))
                        .into_string();
                    let key = mir
                        .names
                        .get(field.name)
                        .or_else(|| mir.symbols.get(field.name))
                        .unwrap_or("field");
                    let value = record_field_unknown_text(
                        mir,
                        &format!("__smelt_inner.{field_name}.clone()"),
                        field.ty,
                    )
                    .unwrap_or_else(|_| "SmeltUnknown::Null".to_owned());
                    // A closure capturing this instance weakly must hold it
                    // strongly once it is reachable from an erased view.
                    let value = if weak_fields.contains(&field.name) {
                        format!("smelt_keep_receiver_alive({value}, &self)")
                    } else {
                        value
                    };
                    fn_writer.line(format!("({key:?}.to_owned(), {value}),"));
                }
                fn_writer.line("]);");
                if has_proto_entries {
                    fn_writer.line("__smelt_entries.extend(__smelt_proto);");
                }
                // A property write through this view reaches the instance
                // (see `emitter::view_write_through`).
                if has_field_setter {
                    fn_writer.line(emitter::view_write_through::erased_view_entry_text("self", "__smelt_entries", has_live_reads));
                }
                // The erased view holds the live handle, so narrowing it back
                // yields this instance (see `reference_origin`).
                fn_writer.line(format!(
                    "SmeltUnknown::Object({})",
                    reference_origin::with_origin_text("SmeltObject::with_id(__smelt_id, __smelt_entries)", "self")
                ));
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
    accessor_owner: Option<smelt_hir::Symbol>,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
    phantom_args: &str,
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
    type_param_idents: &[String],
) -> Result<(), EmitError> {
    // Each field's default expression is computed ONCE and used twice: for the
    // body, and to decide the impl's `where` clause.
    //
    // The clause has to follow the body rather than the field list. Most field
    // defaults are spelled out and need nothing of their type — a callback field
    // gets a constructed no-op closure, and `Rc<dyn Fn(..)>` is not `Default`, so
    // naming every field type demanded something false. The defaults that DELEGATE
    // (`Default::default()` for a bare `T`, `Node::default()` for a
    // reference-class field) are exactly the ones whose type must be `Default`,
    // and those cannot be covered by bounding `T` either:
    // `impl<T: Default> Default for TrieRouter<T>` could not prove
    // `Node<T>: Default`, whose own impl carries the full generated bound set.
    let mut field_defaults = Vec::with_capacity(fields.len());
    for field in fields {
        let field_name =
            RustIdent::new(mir.symbols.get(field.name).unwrap_or("field")).into_string();
        let default_value = FunctionEmitter::default_value_for_with_scoped_type_params(
            mir,
            context,
            field.ty,
            &TypeSubstitution::lexical(scoped_type_params),
        )
        .unwrap_or_else(|_| "Default::default()".to_owned());
        let field_type_text = FunctionEmitter::type_text_for_with_scoped_type_params(
            mir,
            context,
            field.ty,
            &TypeSubstitution::lexical(scoped_type_params),
        )?;
        // A CALLABLE slot is never a delegation, whatever its default text
        // looks like. The emitter always spells such a field's default out as a
        // constructed no-op closure, and `Rc<dyn Fn(..)>` implements no
        // `Default` for the clause to name — but the closure BODY may itself
        // delegate, because the slot's return type has to be produced from
        // somewhere. Hono's `Hono_1Inner` defaults `onError` to
        // `Rc::new(move |..| -> Hono_1<..> { Default::default() })`, whose
        // `default()` belongs to the RETURN type and not to the field type, so
        // a purely textual search sees a delegation and asks for
        // `Rc<dyn Fn(..)>: Default` — an unsatisfiable clause that makes the
        // whole `Default` impl unusable (E0599 at every `Inner::default()`).
        // The nested requirement is carried by the impl's own type-parameter
        // bounds instead (`classes::GENERATED_TYPE_PARAM_BOUNDS`), which is what
        // the return type's `Default` impl asks for.
        let is_callable_slot = type_holds_function(mir, field.ty, NominalStorage::Opaque);
        // An accessor field answers its type's default through a constant
        // getter; its storage is never a delegation.
        if accessor_owner.is_some_and(|owner| accessor_prelude::is_accessor_field(mir, owner, field.name)) {
            field_defaults.push((
                field_name,
                accessor_prelude::storage_type_text(&field_type_text),
                accessor_prelude::constant_text(&default_value),
                true,
            ));
            continue;
        }
        field_defaults.push((field_name, field_type_text, default_value, is_callable_slot));
    }
    let delegating_field_types = field_defaults
        .iter()
        .filter(|(_, _, default_value, is_callable_slot)| {
            !is_callable_slot && default_expression_delegates(default_value)
        })
        .map(|(_, field_type_text, _, _)| field_type_text.clone())
        .collect::<Vec<_>>();
    let where_clause = if impl_generics.is_empty() {
        String::new()
    } else {
        trait_where_clause_for_field_types(&delegating_field_types, "Default", type_param_idents)
    };
    writer.block(
        format!("impl{impl_generics} Default for {name}{type_args}{where_clause}"),
        |impl_writer| {
            impl_writer.block("fn default() -> Self", |fn_writer| {
                fn_writer.block("Self", |self_writer| {
                    for (field_name, _, default_value, _) in &field_defaults {
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

/// Whether a rendered field default resolves through the field type's `Default`.
///
/// `Default::default()` (a bare type parameter) and `Type::default()` (a
/// generated class) both do; a spelled-out constructor such as
/// `SmeltList::new(Vec::<T>::new())` or a no-op closure does not. Only the
/// former need the field type bounded on the impl.
///
/// The call can be NESTED — a promise field defaults to
/// `SmeltFuture::resolved(SmeltUnion9::M0(Default::default()))`, whose `T:
/// Default` requirement reaches the impl through the field type all the same —
/// so the whole expression is searched rather than only its tail.
fn default_expression_delegates(default_value: &str) -> bool {
    default_value.contains("default()")
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

/// Build ` where <field type>: Trait, …` for a field-by-field impl.
///
/// A field-by-field body needs exactly one thing: every field's type implements
/// the trait. Bounding the FIELD TYPES says that and nothing else, where
/// bounding `T` says either too little (a `T`-typed field) or too much (a field
/// whose type is another generated class, which needs the whole generated bound
/// set). Duplicate field types are emitted once; an empty list yields no clause.
fn trait_where_clause_for_field_types(
    field_type_texts: &[String],
    bound: &str,
    type_param_idents: &[String],
) -> String {
    let mut seen = Vec::new();
    for field_type_text in field_type_texts {
        // A field type that mentions no type parameter is decided already:
        // `where String: Clone` is noise, and if such a bound were unsatisfied
        // the impl body would report it directly. Only a type whose
        // satisfiability depends on `T` needs saying.
        if !mentions_type_param(field_type_text, type_param_idents) {
            continue;
        }
        if !seen.contains(field_type_text) {
            seen.push(field_type_text.clone());
        }
    }
    if seen.is_empty() {
        return String::new();
    }
    let clauses = seen
        .iter()
        .map(|field_type_text| format!("{field_type_text}: {bound}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(" where {clauses}")
}

/// Render `<T: Bound, …>` — the bound `#[derive(Bound)]` would have imposed.
///
/// Used together with a field-type `where` clause, never instead of it. The
/// clause alone is not enough: a field default can require the bound through an
/// expression the field's TYPE does not mention, as a promise field defaulting
/// to `SmeltFuture::resolved(SmeltUnion9::M0(Default::default()))` does when the
/// union renders without its argument. Since a derive would have imposed exactly
/// this bound, adding it can never leave a consumer worse off than the derive it
/// replaces.
fn derive_generics_text(type_param_idents: &[String], bound: &str) -> String {
    if type_param_idents.is_empty() {
        return String::new();
    }
    let params = type_param_idents
        .iter()
        .map(|ident| format!("{ident}: {bound}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!("<{params}>")
}

/// Whether a rendered type mentions one of these type parameters as a whole
/// identifier, so `T` matches in `Node<T>` but not inside `Trie` or `TABLE`.
fn mentions_type_param(type_text: &str, type_param_idents: &[String]) -> bool {
    type_param_idents.iter().any(|ident| {
        type_text
            .match_indices(ident.as_str())
            .any(|(at, matched)| {
                let before = type_text[..at].chars().next_back();
                let after = type_text[at + matched.len()..].chars().next();
                let boundary = |ch: Option<char>| {
                    ch.is_none_or(|ch| !ch.is_alphanumeric() && ch != '_')
                };
                boundary(before) && boundary(after)
            })
    })
}

/// Emit a field-by-field `Clone` for a generic value class.
///
/// `#[derive(Clone)]` would generate `impl<T: Clone> Clone`, whose single bound
/// cannot satisfy a field whose type is another generated class — those carry
/// the full generated bound set (see `class_impl_generics_text`). The impl is
/// spelled out with bare parameters plus a field-type `where` clause instead.
fn emit_clone_impl_for_value_class(
    writer: &mut CodeWriter,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    field_names: &[String],
    field_type_texts: &[String],
    type_param_idents: &[String],
    has_phantom: bool,
) {
    let where_clause = trait_where_clause_for_field_types(field_type_texts, "Clone", type_param_idents);
    writer.block(
        format!("impl{impl_generics} Clone for {name}{type_args}{where_clause}"),
        |impl_writer| {
            impl_writer.block("fn clone(&self) -> Self", |fn_writer| {
                fn_writer.block(name, |init_writer| {
                    for field_name in field_names {
                        init_writer.line(format!("{field_name}: self.{field_name}.clone(),"));
                    }
                    if has_phantom {
                        init_writer.line("_smelt_phantom: ::std::marker::PhantomData,");
                    }
                });
            });
        },
    );
}

/// Emit a field-by-field `PartialEq` for a generic value class.
///
/// The `Clone` reasoning above applies unchanged: a derived `PartialEq` carries
/// only `T: PartialEq`, which a generated-class field does not satisfy. The
/// phantom field is skipped — `PhantomData` compares equal to itself and says
/// nothing about the value.
fn emit_partial_eq_impl_for_value_class(
    writer: &mut CodeWriter,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    field_names: &[String],
    field_type_texts: &[String],
    type_param_idents: &[String],
) {
    let where_clause = trait_where_clause_for_field_types(field_type_texts, "PartialEq", type_param_idents);
    writer.block(
        format!("impl{impl_generics} PartialEq for {name}{type_args}{where_clause}"),
        |impl_writer| {
            impl_writer.block("fn eq(&self, other: &Self) -> bool", |fn_writer| {
                if field_names.is_empty() {
                    fn_writer.line("true");
                    return;
                }
                let comparisons = field_names
                    .iter()
                    .map(|field_name| format!("self.{field_name} == other.{field_name}"))
                    .collect::<Vec<_>>()
                    .join(" && ");
                fn_writer.line(comparisons);
            });
        },
    );
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
        // A map keyed by a TYPE PARAMETER is a `SmeltJsMap<P, _>`, whose
        // `PartialEq` needs `P: SmeltJsKeyEq`. A `#[derive(PartialEq)]` only
        // adds `P: PartialEq`, so the derive does not compile (E0369); the
        // record is not derivably comparable.
        Some(Type::Dict(key, _) | Type::JsMap(key, _))
            if matches!(mir.types.get(*key), Some(Type::TypeParam { .. })) =>
        {
            false
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
            | Type::FixedInt(_)
            | Type::BigInt
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
#[expect(clippy::too_many_arguments, reason = "the record impl needs its emission context and lexical type parameters beside the existing record facts")]
fn emit_record_into_smelt_unknown_impl(
    writer: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    accessor_owner: Option<smelt_hir::Symbol>,
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
    decode_facts: &[smelt_hir::FieldDecodeFacts],
) -> Result<(), EmitError> {
    writer.block(
        format!("impl{impl_generics} IntoSmeltUnknown for {name}{type_args}"),
        |impl_writer| {
            impl_writer.block("fn into_smelt_unknown(self) -> SmeltUnknown", |fn_writer| {
                // An interface field that may be absent (`p?: T`) and holds
                // `None` IS absent: the erased object has no such key, which
                // is what an object spread, `in` and `Object.keys` observe. A
                // spread of `{ ...defaults, ...options }` used to copy an
                // `undefined` over every default the caller left out.
                let absent_keys = fields
                    .iter()
                    .filter(|field| {
                        field.visibility.is_own_property()
                            && matches!(mir.types.get(field.ty), Some(Type::Optional(_)))
                            && decode_facts
                                .iter()
                                .any(|facts| facts.name == field.name && facts.may_be_absent)
                    })
                    .map(|field| {
                        mir.names
                            .get(field.name)
                            .or_else(|| mir.symbols.get(field.name))
                            .unwrap_or("field")
                            .to_owned()
                    })
                    .collect::<Vec<_>>();
                if !absent_keys.is_empty() {
                    fn_writer.line(format!(
                        "let smelt_absent_keys: [&str; {}] = {:?};",
                        absent_keys.len(),
                        absent_keys
                    ));
                }
                fn_writer.line(
                    "SmeltUnknown::Object(SmeltObject::new(Vec::from([",
                );
                for field in fields {
                    if !field.visibility.is_own_property() {
                        continue;
                    }
                    // The erased key is the SOURCE spelling (`mir.names`), the
                    // Rust field is the sanitized one: `onRead` must read back
                    // as `"onRead"` from an erased view, not as `"on_read"`.
                    let field_name = RustIdent::new(mir.symbols.get(field.name).unwrap_or("field"))
                        .into_string();
                    let key = mir
                        .names
                        .get(field.name)
                        .or_else(|| mir.symbols.get(field.name))
                        .unwrap_or("field");
                    // A field declared `p: T | null` (it admits `null` and can
                    // never be absent) holds `null` when its `Option` is empty,
                    // and its erased view says so: `JSON.stringify` prints
                    // `"p":null` rather than omitting the key.
                    let absent_is_null = decode_facts.iter().any(|facts| {
                        facts.name == field.name && facts.admits_null && !facts.may_be_absent
                    });
                    // An accessor is read by running its getter now: the erased
                    // view holds the value the property answers at erase time.
                    let field_source = if accessor_owner
                        .is_some_and(|owner| accessor_prelude::is_accessor_field(mir, owner, field.name))
                    {
                        format!("({})", accessor_prelude::read_text(&format!("self.{field_name}")))
                    } else {
                        format!("self.{field_name}")
                    };
                    let value = match mir.types.get(field.ty) {
                        Some(Type::Optional(inner)) if absent_is_null => {
                            record_field_unknown_text(mir, "smelt_present", *inner).map(|present| {
                                format!(
                                    "{field_source}.map_or(SmeltUnknown::Null, |smelt_present| {present})"
                                )
                            })
                        }
                        _ => erased_function_field_text(
                            mir,
                            context,
                            scoped_type_params,
                            &field_source,
                            field.ty,
                        )
                        .map_or_else(
                            || record_field_unknown_text(mir, &field_source, field.ty),
                            Ok,
                        ),
                    }
                    .unwrap_or_else(|_| "SmeltUnknown::Null".to_owned());
                    fn_writer.line(format!("({key:?}.to_owned(), {value}),"));
                }
                if absent_keys.is_empty() {
                    fn_writer.line("])))");
                } else {
                    fn_writer.line("]).into_iter().filter(|(smelt_key, smelt_value)| !(matches!(smelt_value, SmeltUnknown::Undefined) && smelt_absent_keys.contains(&smelt_key.as_str()))).collect::<Vec<_>>()))");
                }
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
/// `SmeltFromUnknown`). Everything else keeps the record's `Default`: callbacks,
/// futures and generators have no inbound impl in the prelude, and runtime
/// builtin classes (a compiled `RegExp` among them, even though the prelude does
/// emit `SmeltFromUnknown for SmeltRegExp`) are left out of this whitelist
/// because the program does not generate them.
pub(crate) fn type_supports_from_unknown(
    mir: &Mir,
    context: &EmitContext,
    ty: TypeId,
) -> bool {
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
        // that is what now emits `SmeltFromUnknown`. `Type::Class` also names
        // runtime builtins, which this program does not generate, so they are
        // excluded.
        //
        // A *reference* class is generated too, as an `Rc<RefCell<Inner>>`
        // newtype by `emit_reference_class_storage`. It now carries an inbound
        // impl as well (`reference_origin::emit_reference_from_smelt_unknown_impl`,
        // which restores the live handle an erasure stamped), so it satisfies
        // the generated `SmeltFromUnknown` type-parameter bound. This whitelist
        // still leaves it out: it decides where the EMITTER reaches for the
        // impl (record fields, prototype-method parameters), and widening those
        // sites is a separate change from making identity round-trip.
        Some(Type::Class { name, .. }) => {
            let name = *name;
            !context.is_reference_class(name)
                && (mir.classes.iter().any(|class| class.name == name)
                    || mir.interfaces.iter().any(|item| item.name == name))
        }
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item)) => {
            type_supports_from_unknown(mir, context, *item)
        }
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            type_supports_from_unknown(mir, context, *key)
                && type_supports_from_unknown(mir, context, *value)
        }
        Some(Type::Tuple(items) | Type::Union(items)) => items
            .iter()
            .all(|item| type_supports_from_unknown(mir, context, *item)),
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
    context: &EmitContext,
    name: &str,
    impl_generics: &str,
    type_args: &str,
    fields: &[smelt_mir::MirField],
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
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
                            if !field.visibility.is_own_property() {
                                continue;
                            }
                            // Only rebuild fields whose type can actually be
                            // recovered. This is a whitelist rather than a
                            // blacklist: a field the erased view cannot restore
                            // (a callback handle, a compiled regex) keeps its
                            // `Default`, and a type added later has to opt in
                            // rather than silently emit a call to a
                            // `SmeltFromUnknown` impl that does not exist.
                            let field_name =
                                RustIdent::new(mir.symbols.get(field.name).unwrap_or("field"))
                                    .into_string();
                            let key = mir
                                .names
                                .get(field.name)
                                .or_else(|| mir.symbols.get(field.name))
                                .unwrap_or("field");
                            if matches!(mir.types.get(field.ty), Some(Type::Function(_))) {
                                emit_function_field_origin_restore(
                                    body,
                                    mir,
                                    context,
                                    scoped_type_params,
                                    &field_name,
                                    key,
                                    field.ty,
                                );
                                continue;
                            }
                            if !type_supports_from_unknown(mir, context, field.ty) {
                                continue;
                            }
                            // Read the SOURCE spelling out of the erased object;
                            // write the sanitized Rust field. See the outbound
                            // impl above.
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

/// Restores a function-typed record field from an erased object, by identity.
///
/// A record that crosses an erased boundary and comes back (an options bag
/// passed as a generic `T`, then `Object.assign(this, options)`) carries its
/// callable members as erased functions. Leaving the field at its `Default`
/// replaces a live method with a no-op: Hono's `basePath()`/`route()` build a
/// clone whose `router` came back through here, and every route added to the
/// clone went into a router whose `add` did nothing, so mounted routes 404'd.
///
/// The erasure of a typed callback registers that callback as the erased
/// function's origin (`smelt_register_function_origin`), so the field is
/// restored to the SAME typed `Rc` — the identity JavaScript preserves — when
/// the origin has the field's exact type. A function that never had a typed
/// origin of this type keeps the `Default` it had before; adapting an arbitrary
/// erased function to a typed signature needs the per-call-site coercion the
/// function emitter owns.
fn emit_function_field_origin_restore(
    body: &mut CodeWriter,
    mir: &Mir,
    context: &EmitContext,
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
    field_name: &str,
    key: &str,
    field_ty: TypeId,
) {
    let Ok(field_ty_text) = FunctionEmitter::type_text_for_with_scoped_type_params(
        mir,
        context,
        field_ty,
        &TypeSubstitution::lexical(scoped_type_params),
    ) else {
        return;
    };
    // Origins are recorded only for typed `Rc` callables (the erasure in
    // `emitter/coercion.rs`), so a field spelled any other way (an erased
    // `SmeltErasedFunction`) has no origin to find and keeps its `Default`.
    if !field_ty_text.starts_with("::std::rc::Rc<") {
        return;
    }
    body.block(format!("if let Some(field) = object.get({key:?})"), |assign| {
        assign.line(
            "let smelt_function = match field { SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") { Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }, _ => None };",
        );
        assign.line(format!(
            "if let Some(smelt_origin) = smelt_function.and_then(|smelt_function| smelt_restore_function_origin::<{field_ty_text}>(&smelt_function)) {{ result.{field_name} = smelt_origin; }}"
        ));
    });
}

/// Recovers a value of type `ty` from a `SmeltUnknown` expression.
///
/// The mirror of [`record_field_unknown_text`], and it exists for the same
/// reason: a Rust TUPLE of an arity without an emitted `SmeltFromUnknown` impl has none, so the blanket
/// `SmeltFromUnknown::smelt_from_unknown(…)` an erased adapter reaches for does
/// not compile against a tuple parameter. `RegExpRouter#add`'s
/// `HandlerWithMetadata<T> = [T, Record<string, number>]` parameter reported
/// "the trait `SmeltFromUnknown` is not implemented for
/// `(T, SmeltRecord<String, f64>)`".
///
/// A tuple is recovered ELEMENT-WISE from the erased array it was written as,
/// recursing so a tuple of tuples works too. Everything else keeps the blanket
/// impl, which is what its type provides.
///
/// The `panic!` on a non-array matches the spelling the emitter's other
/// tuple-extraction sites already use, rather than inventing a second answer for
/// the same impossible input.
pub(crate) fn record_field_from_unknown_text(
    mir: &Mir,
    value_text: &str,
    ty: TypeId,
) -> Result<String, EmitError> {
    let Some(Type::Tuple(items)) = mir.types.get(ty) else {
        return Ok(format!(
            "SmeltFromUnknown::smelt_from_unknown({value_text})"
        ));
    };
    let elements = items
        .iter()
        .enumerate()
        .map(|(index, item)| {
            record_field_from_unknown_text(
                mir,
                &format!(
                    "smelt_tuple_values.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined)"
                ),
                *item,
            )
        })
        .collect::<Result<Vec<_>, _>>()?
        .join(", ");
    Ok(format!(
        "(if let SmeltUnknown::Array(smelt_tuple_values) = {value_text} {{ ({elements}) }} else {{ panic!(\"unknown is not tuple\") }})"
    ))
}

/// Erases a FUNCTION-typed record field (`p: (a: A) => R` or `p?: ..`) to a
/// callable `SmeltUnknown`, or `None` for a signature this adapter declines.
///
/// [`record_field_unknown_text`] spells a function field `SmeltUnknown::Null`
/// because it has no emitter to render the adapter. The erased view of an
/// interface struct then lost its methods: Hono's `Object.assign(this, rest)`
/// copied a `Router` through such a view, and the router it read back had a
/// `match` that answered `null` for every request. The adapter converts each
/// argument with `SmeltFromUnknown` (its target type is inferred from the
/// callable itself, so the interface's own type parameters need no
/// rendering), erases the result with `IntoSmeltUnknown`, and registers the
/// typed callable as its origin so converting back yields that same callable.
///
/// A parameter the rendered signature borrows (`&T`) gets a reference to the
/// converted value. Declined, keeping the old spelling: rest, async and
/// mutable-parameter signatures, an `impl` or `&mut` in the rendered type, and
/// a parameter type that cannot be produced from an erased value.
fn erased_function_field_text(
    mir: &Mir,
    context: &EmitContext,
    scoped_type_params: &HashSet<smelt_hir::Symbol>,
    value_text: &str,
    ty: TypeId,
) -> Option<String> {
    let (optional, function_ty) = match mir.types.get(ty)? {
        Type::Optional(inner) => (true, *inner),
        _ => (false, ty),
    };
    let Type::Function(function) = mir.types.get(function_ty)? else {
        return None;
    };
    if function.rest.is_some() || function.is_async || !function.mutable_params.is_empty() {
        return None;
    }
    let rendered = FunctionEmitter::type_text_for_with_scoped_type_params(
        mir,
        context,
        function_ty,
        &TypeSubstitution::lexical(scoped_type_params),
    )
    .ok()?;
    if rendered.contains("impl ") || rendered.contains("&mut") {
        return None;
    }
    let borrowed = rendered_fn_params(&rendered)?
        .iter()
        .map(|param| param.starts_with('&'))
        .collect::<Vec<_>>();
    if borrowed.len() != function.params.len()
        || !function
            .params
            .iter()
            .all(|param| type_supports_from_unknown(mir, context, *param))
    {
        return None;
    }
    let args = borrowed
        .iter()
        .enumerate()
        .map(|(index, borrowed)| {
            let reference = if *borrowed { "&" } else { "" };
            format!(
                "{reference}SmeltFromUnknown::smelt_from_unknown(smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined))"
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let call = if function.may_throw {
        format!("(smelt_function_value)({args})?")
    } else {
        format!("(smelt_function_value)({args})")
    };
    let adapter = |callable: &str| {
        format!(
            "{{ let smelt_function_value: {rendered} = {callable}; let smelt_function_origin = smelt_function_value.clone(); let smelt_erased_function: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| Ok::<SmeltUnknown, Box<dyn std::error::Error>>(({call}).into_smelt_unknown())); smelt_register_function_origin(&smelt_erased_function, smelt_function_origin); SmeltUnknown::Function(smelt_erased_function) }}"
        )
    };
    Some(if optional {
        format!(
            "{value_text}.map_or(SmeltUnknown::Undefined, |smelt_present| {})",
            adapter("smelt_present")
        )
    } else {
        adapter(&format!("{value_text}.clone()"))
    })
}

/// The top-level parameter types of a rendered `Rc<dyn Fn(..) -> R>`, or
/// `None` when the text is not that shape.
fn rendered_fn_params(rendered: &str) -> Option<Vec<String>> {
    let start = rendered.find("Fn(")? + 3;
    let mut depth = 0usize;
    let mut params = Vec::new();
    let mut current = String::new();
    for ch in rendered[start..].chars() {
        match ch {
            '(' | '<' | '[' => {
                depth += 1;
                current.push(ch);
            }
            ')' if depth == 0 => {
                if !current.trim().is_empty() {
                    params.push(current.trim().to_owned());
                }
                return Some(params);
            }
            ')' | '>' | ']' => {
                depth = depth.checked_sub(1)?;
                current.push(ch);
            }
            ',' if depth == 0 => {
                params.push(current.trim().to_owned());
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    None
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
        Some(Type::Int | Type::FixedInt(_) | Type::Float) => format!("SmeltUnknown::Number({value_text} as f64)"),
        Some(Type::BigInt) => format!("SmeltUnknown::BigInt(({value_text}).clone())"),
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
        // A tuple erases ELEMENT-WISE to a JS array, like every other structural
        // shape above it. It was grouped with the shapes that have an
        // `IntoSmeltUnknown` impl, but a Rust tuple has none — the prototype
        // carrier for `RegExpRouter#buildAllMatchers`, whose return type is
        // `[RegExp, HandlerParamsSet<T>[][], …]`, asked for
        // `(value).into_smelt_unknown()` and got "no method named
        // `into_smelt_unknown` found for tuple" (H29, second half).
        //
        // The value is bound once: `value_text` is an arbitrary expression here,
        // and indexing it per element would evaluate it once per element.
        Some(Type::Tuple(items)) => {
            let elements = items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    record_field_unknown_text(mir, &format!("smelt_tuple.{index}"), *item)
                })
                .collect::<Result<Vec<_>, _>>()?
                .join(", ");
            format!(
                "{{ let smelt_tuple = {value_text}; SmeltUnknown::Array(vec![{elements}].into()) }}"
            )
        }
        Some(Type::Dict(_, _) | Type::JsMap(_, _) | Type::Class { .. }) => {
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

/// Collects the dependency list required by generated Rust code.
fn generated_deps(demand: &RuntimeDemand) -> Vec<GeneratedDep> {
    let mut deps = Vec::new();
    if demand.has(Need::DepTokio) {
        deps.push(GeneratedDep::Tokio);
    }
    if demand.has(Need::DepGenawaiter) {
        deps.push(GeneratedDep::Genawaiter);
    }
    deps.extend(
        runtime_demand::backend_dependencies(demand)
            .into_iter()
            .map(GeneratedDep::Stdlib),
    );
    deps
}

/// Sanitizes an identifier into a Rust-safe spelling.
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
fn emit_mutable_globals(mir: &Mir, context: &EmitContext) -> Result<String, EmitError> {
    let mut writer = CodeWriter::new();
    let mut any_fallible_init = false;
    writer.line("thread_local! {");
    for (index, global) in mir.globals.iter().enumerate() {
        let name = global_static_name(mir, compact_index(index, "global index")?);
        // The cell's initial value. A constant is emitted inline; an expression
        // initializer is a CALL to the nullary function the frontend
        // synthesized from it, which is emitted like any other function. A
        // `thread_local!` initializer is an arbitrary expression evaluated once
        // per thread on first access, so a call is as legal there as a literal
        // — and that timing is exactly JavaScript's "module state is
        // initialized before any consumer runs", per generated test thread.
        let init = match &global.init {
            MirGlobalInit::Constant(constant) => emitter::literals::constant_text(constant),
            MirGlobalInit::Call(func_id) => {
                let function = mir
                    .functions
                    .get(usize::try_from(func_id.0).unwrap_or(usize::MAX))
                    .ok_or_else(|| {
                        EmitError::new(
                            "mutable global initializer references an unknown function",
                        )
                    })?;
                let function_name = context
                    .function_names
                    .get(&function.id)
                    .cloned()
                    .map_or_else(
                        || {
                            Ok::<String, EmitError>(sanitize_ident(
                                mir.symbols.get(function.name).ok_or_else(|| {
                                    EmitError::new("mutable global initializer has no name")
                                })?,
                            ))
                        },
                        Ok,
                    )?;
                // An initializer that can throw (`const db = new
                // DatabaseSync(path)`, a call to a throwing function) returns a
                // `Result`, and a `thread_local!` initializer has nowhere to
                // propagate it. The cell is touched at the binding's own
                // declaration (see `module_init`), so a throw surfaces there,
                // after the output of the statements before it — and a throw
                // during module evaluation is always uncaught in JavaScript.
                if function.can_throw {
                    any_fallible_init = true;
                    format!("{function_name}().unwrap_or_else(|error| smelt_module_init_failed(error))")
                } else {
                    format!("{function_name}()")
                }
            }
        };
        // A Copy primitive uses `Cell`, which needs no borrow bookkeeping and
        // can be `const`-initialized from a literal. Everything else — a
        // `String`, a record, a list, a map — is not `Copy`, so its cell is a
        // `RefCell`; an owned or computed initializer cannot be `const` either.
        let is_constant_init = matches!(global.init, MirGlobalInit::Constant(_));
        match mir.types.get(global.ty) {
            Some(Type::Float | Type::Int | Type::Bool) if is_constant_init => {
                let rust_ty = match mir.types.get(global.ty) {
                    Some(Type::Float) => "f64",
                    Some(Type::Int) => "i64",
                    _ => "bool",
                };
                writer.line(format!(
                    "    static {name}: ::std::cell::Cell<{rust_ty}> = const {{ ::std::cell::Cell::new({init}) }};"
                ));
            }
            Some(Type::Float | Type::Int | Type::Bool) => {
                let rust_ty = match mir.types.get(global.ty) {
                    Some(Type::Float) => "f64",
                    Some(Type::Int) => "i64",
                    _ => "bool",
                };
                writer.line(format!(
                    "    static {name}: ::std::cell::Cell<{rust_ty}> = ::std::cell::Cell::new({init});"
                ));
            }
            _ => {
                let value_ty = FunctionEmitter::type_text_for_with_context(mir, context, global.ty)?;
                writer.line(format!(
                    "    static {name}: ::std::cell::RefCell<{value_ty}> = ::std::cell::RefCell::new({init});"
                ));
            }
        }
    }
    writer.line("}");
    if any_fallible_init {
        emit_module_init_failed(&mut writer);
    }
    Ok(writer.finish())
}

/// Emit `smelt_module_init_failed`, the report for a module global whose
/// initializer threw.
///
/// It does what a throwing top-level statement does through `main`'s
/// `Result` return — prints `Error: {error:?}` to stderr and exits with status
/// 1 — because the initializer runs at the same point of module evaluation and
/// no `catch` can enclose a module-level declaration. Not a panic: a panic
/// prints a different message and exits 101, so the two spellings of the same
/// source failure would disagree. Stdout is flushed first, since `exit` runs
/// no destructors and a partial line would otherwise be lost.
fn emit_module_init_failed(writer: &mut CodeWriter) {
    writer.line("/// A module global's initializer threw during module evaluation: report it as `main` does and exit 1.");
    writer.block(
        "fn smelt_module_init_failed(error: Box<dyn std::error::Error>) -> !",
        |fn_writer| {
            fn_writer.line("let _ = ::std::io::Write::flush(&mut ::std::io::stdout());");
            fn_writer.line("eprintln!(\"Error: {error:?}\");");
            fn_writer.line("::std::process::exit(1)");
        },
    );
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
