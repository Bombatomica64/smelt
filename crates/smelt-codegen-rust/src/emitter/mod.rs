//! Internal Rust source emission helpers split by concept.
#![allow(
    clippy::wildcard_imports,
    reason = "emitter shards share a common private helper surface through the parent module"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "the private emitter module still exposes helpers to its parent codegen module"
)]

use crate::classes::{NominalStorage, type_holds_function};
use crate::generic_bindings::CalleeTypeParamBindings;
use crate::runtime_demand::{Need, RuntimeDemand};
use smelt_stdlib::runtime_symbols::markers::*;
use crate::rust::RustType;
use crate::thrown::ERROR_BOX;
use crate::type_substitution::{Resolved, TypeSubstitution};
use render_scope::RenderScope;
use crate::{EmitError, compact_index, id_index, sanitize_ident};
use literals::operand_mutation_root;
use smelt_hir::{FileId, PropertyLookup, Span, Symbol, Type, TypeId};
use smelt_mir::{
    AbsentSpelling, BasicBlock, BuiltinFn, Callee, Constant, FuncId, HirOrigin, LocalDecl,
    LocalId, LocalKind, Mir,
    MirClass, MirClosure, MirDescriptor, MirField, MirFunction, MirListSpliceItem, NegativeIndex,
    Operand, Place, Read, Rvalue, Statement, Terminator,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

/// Pick the environment a *callee-owned* type renders in at one call site.
///
/// A callee's `T` has no meaning in the caller's world: [`smelt_hir::Symbol`] is
/// name-interned, so rendering a callee type under the caller's lexical scope
/// captures a same-named caller parameter and erases every other one. That is
/// the historical `E0631` described in §1.2 of
/// `blocker-logs/estk-callback-generics-plan.md`.
///
/// There are exactly two sound answers, and which one applies is decided once
/// per call site rather than once per argument:
///
/// * the site **monomorphized** the callee — every one of its type parameters is
///   pinned to a concrete type, and the sibling value arguments were passed
///   through at those types. The callee's names are then spelled at their
///   bindings and nothing is left to the lexical scope, which is why the base is
///   [`TypeSubstitution::erased`] rather than the caller's scope: a caller `T`
///   must not shadow the binding for a callee `T`;
/// * the site did **not** monomorphize — every argument is instead coerced to
///   the callee's declared parameter type as rendered in the caller's lexical
///   scope, so the callee is instantiated at whatever that rendering produced.
///   Rendering the callback in the same scope reproduces exactly that
///   instantiation, which is why this arm keeps the caller's scope instead of
///   erasing.
///
/// Mixing the two is precisely what fails to compile.
fn callee_substitution<'a>(
    caller_scope: &'a HashSet<Symbol>,
    callee_bindings: Option<&'a CalleeTypeParamBindings>,
) -> TypeSubstitution<'a> {
    match callee_bindings {
        Some(bindings) => TypeSubstitution::erased().with_bindings(bindings),
        None => TypeSubstitution::lexical(caller_scope),
    }
}

/// Scope guard that renders text for a hoisted module-level item.
///
/// Sets [`FunctionEmitter::hoisted_module_item`] for as long as it lives and
/// restores the previous value on drop, so an early `?` inside the rendered
/// region cannot leave the emitter stuck in the module-level scope.
struct HoistedModuleItemScope<'e, 'm> {
    /// The emitter whose render scope this guard switched.
    emitter: &'e FunctionEmitter<'m>,
    /// The scope flag to restore on drop.
    previous: bool,
}

impl<'e, 'm> HoistedModuleItemScope<'e, 'm> {
    /// Enter the module-level render scope on `emitter`.
    fn enter(emitter: &'e FunctionEmitter<'m>) -> Self {
        let previous = emitter.hoisted_module_item.replace(true);
        Self { emitter, previous }
    }
}

impl Drop for HoistedModuleItemScope<'_, '_> {
    /// Restore whatever render scope was active before this guard.
    fn drop(&mut self) {
        self.emitter.hoisted_module_item.set(self.previous);
    }
}

/// Hash set of MIR block ids using a fast, non-DoS-resistant hasher.
///
/// The control-flow reachability queries (`block_reaches_target*`,
/// `block_can_reach`, …) build short-lived visited sets keyed by `BlockId`, a
/// dense `u32`. Profiling `emit_crate` on es-toolkit showed the default
/// SipHash's per-key cost dominating those queries; `FxHashSet` hashes a `u32`
/// in a couple of instructions and removes that cost without changing behavior.
pub(crate) type BlockIdSet = rustc_hash::FxHashSet<smelt_mir::BlockId>;

mod backend_erasure;
pub(crate) mod bound_dispatch;
mod bigint;
pub(crate) mod typedness;
mod binary_ops;
mod call;
mod call_runtime;
mod class_keyed_write;
mod capture_analysis;
mod cfg_queries;
mod closures;
mod coercion;
pub(crate) use coercion::record as record_coercion;
mod control_flow;
mod console_inspect;
mod construct;
mod control_flow_match;
mod core;
mod dict_entry_update;
pub(crate) mod emit_role;
mod flow_state;
mod local_binding;
mod helper_reservation;
mod blob;
mod fetch_types;
/// Emission of the operations that can throw.
mod fallible_op;
mod url;
mod fixed_int;
mod abort_signal;
mod crypto;
mod host_module;
mod form_data;
mod text_codec;
mod typed_array;
mod host_member_read;
mod host_interop;
mod json_decode;
mod http_server;
mod list;
mod list_mutation;
mod list_ordering;
mod list_query;
pub(crate) mod literals;
mod local_analysis;
mod map;
mod numeric;
mod optional_access;
mod place;
mod post_dominance;
mod receiver_bound_slot;
mod record_slot_abi;
mod render_scope;
mod rendered_value;
/// Debug-only emitter self-consistency checks; the module does not exist in a
/// release build, so neither do its assertions or their call sites.
#[cfg(debug_assertions)]
mod seam_assertions;
mod static_call_args;
mod set;
mod sqlite;
mod strings;
mod strings_io;
mod throw;
mod throwing_join;
mod tuple;
mod typeof_str_locals;
mod types;
mod union;
/// Property writes through an erased view of a reference-class instance.
pub(crate) mod view_write_through;
pub(crate) mod weak_receiver;

pub(crate) use literals::method_mutates_this;
use literals::{assigned_locals, constant_text};
use rendered_value::{Precedence, RenderedValue};
use types::MutablePrefix;
pub(crate) use union::emit_union_definitions;
pub(crate) use json_decode::emit_json_decoders;
pub(crate) use console_inspect::emit_console_inspect_support;
pub(crate) use core::is_erased_unknown_rest_function_in;

/// Process-wide runtime state an emitted native test resets before its body,
/// so mock state one test installs does not leak into the next.
///
/// Each flag is set only when the program uses the matching runtime, so a
/// crate that never touches it emits no reset.
#[derive(Clone, Copy)]
struct TestRuntimeResets {
    /// Mutable `Date.now()` mock state.
    date_now: bool,
    /// Virtual timer runtime state.
    timers: bool,
}

/// Precomputed crate-level codegen facts shared by all function emitters.
/// The visibility a free function's definition is written with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FreeFunctionVisibility {
    /// Private to its module: the crate root, where every call can see it.
    Private,
    /// Visible to the crate root, which re-exports a sibling module's functions.
    Crate,
}

impl FreeFunctionVisibility {
    /// The text in front of `fn`.
    const fn text(self) -> &'static str {
        match self {
            Self::Private => "",
            Self::Crate => "pub(crate) ",
        }
    }
}

/// Proof that [`EmitContext::populate_generic_functions`] has run.
///
/// Only that method constructs one, and the crate builder cannot start without
/// it, so no body renders before the generic decision is made.
pub(crate) struct GenericsDecided(());

pub(crate) struct EmitContext {
    /// The type ID of the None type.
    pub(crate) none_ty: TypeId,
    /// What this program demands of the runtime, computed once.
    demand: RuntimeDemand,
    /// Whether the render under trial touched the erased carrier; see
    /// [`Self::note_carrier_use`].
    carrier_touched: std::cell::Cell<bool>,
    /// Nesting of real dynamic boundaries, inside which a carrier use does not
    /// count; see [`Self::at_carrier_boundary`].
    carrier_boundaries: std::cell::Cell<usize>,
    /// The generated traits of class- and interface-bounded type parameters
    /// (see [`crate::bound_traits`]).
    pub(crate) bound_traits: crate::bound_traits::BoundTraitPlan,
    /// Runtime state every emitted native test resets before its body.
    test_runtime_resets: TestRuntimeResets,
    /// Rust function names keyed by MIR function ID.
    pub(crate) function_names: HashMap<FuncId, String>,
    /// Emitted parameter types keyed by Rust function name.
    function_param_types: HashMap<String, Vec<TypeId>>,
    /// Emitted return types keyed by Rust function name.
    function_return_types: HashMap<String, TypeId>,
    /// Function parameters that must use an owned callback handle.
    owned_callback_params: HashSet<(FuncId, LocalId)>,
    /// Per-function-item value accessors collected while erasing
    /// function-item-as-value wrappers to `SmeltUnknown`.
    ///
    /// Maps a crate-unique function item key to the self-contained accessor body
    /// expression that builds its erased `SmeltUnknown::Function`. After the
    /// function loop the crate emitter flushes one `__smelt_fn_value_<key>()`
    /// accessor per entry, each caching a single shared erased value so repeated
    /// references to the same named function keep JavaScript reference identity.
    /// A `BTreeMap` keeps the emitted order deterministic for golden tests.
    pub(crate) function_item_accessors:
        ::std::cell::RefCell<::std::collections::BTreeMap<usize, String>>,
    /// Per-function-item accessors for the CONCRETE `SmeltErasedFunction` type
    /// (as opposed to the `SmeltUnknown` value held in `function_item_accessors`).
    ///
    /// A nullary function-item constant lowered to a typed `SmeltErasedFunction`
    /// context (e.g. Remeda's `doNothing`/`constant`) would otherwise build a
    /// FRESH `SmeltErasedFunction` — and thus a fresh callback `Rc` — on every
    /// call, so two calls never satisfy `Rc::ptr_eq`. JavaScript instead returns
    /// the same function singleton. Routing the build through a per-item
    /// `__smelt_fn_erased_<key>()` accessor (caching one `SmeltErasedFunction`)
    /// makes every call return clones that share one inner callback `Rc`. Maps a
    /// crate-unique function item key to the self-contained `SmeltErasedFunction
    /// { .. }` factory expression. A `BTreeMap` keeps emitted order deterministic.
    pub(crate) function_item_erased_fn_accessors:
        ::std::cell::RefCell<::std::collections::BTreeMap<usize, String>>,
    /// Per-function-item accessors for a TYPED callable value (`Rc<dyn Fn..>`).
    ///
    /// A named function referenced as a value at a typed function position
    /// (`errorHandler: ErrorHandler = errorHandler`, `this.errorHandler ===
    /// errorHandler`) was emitted as a fresh `Rc::new(..)` wrapper per
    /// reference, so two references were never `===`: Hono's `route()` asks
    /// `app.errorHandler === errorHandler` to decide whether a sub-app has its
    /// own error handler, always answered `false`, and wrapped every mounted
    /// handler in an error-composing adapter. Each accessor caches ONE typed
    /// wrapper, so every reference shares one `Rc` — the JavaScript identity of
    /// the one function object.
    ///
    /// Keyed by `(item key, rendered Rust type)`, because one item can be
    /// referenced at differently rendered callable types (a callback parameter
    /// passed by shared reference renders `&T` where an owned slot renders `T`);
    /// the value is `(ordinal, wrapper expression)`, the ordinal naming the
    /// accessor `__smelt_fn_typed_<key>_<ordinal>`.
    pub(crate) function_item_typed_accessors: ::std::cell::RefCell<
        ::std::collections::BTreeMap<(usize, String), (usize, String)>,
    >,
    /// One generated conversion function per class type recovered from an
    /// erased value: `__smelt_from_unknown_record_<ordinal>(SmeltUnknown) -> T`.
    ///
    /// The conversion rebuilds the class from the erased object's record,
    /// field by field and with each field's own conversion nested inside, so
    /// spelled inline it is tens of kilobytes for a large class. It used to be
    /// spelled at EVERY site that recovered the class: Hono's CORS test module
    /// repeated `Hono`'s conversion about a thousand times and reached 28 MB,
    /// past what one rustc can compile in a capped build. Only outermost
    /// conversions are hoisted (see `extract_value_text`), so a helper's body
    /// is exactly the text that was inlined before.
    ///
    /// Keyed by the module-level Rust spelling of the type; the value is
    /// `(ordinal, body)`.
    ///
    /// Helper names start with [`crate::RECORD_CONVERSION_HELPER_PREFIX`].
    pub(crate) record_conversion_helpers: ::std::cell::RefCell<
        ::std::collections::BTreeMap<String, (usize, String)>,
    >,
    /// Hoisted record-to-record adapters for a pair that reaches ITSELF: a
    /// field whose own adaptation needs the same `(source, target)` pair
    /// (`swap(): Pair<T>` inside `Pair<T>`, `Pair<SmeltUnknown>` to
    /// `Pair<f64>`). Inlining it again would never end, so the cycle calls a
    /// module-level helper instead (see `hoisted_record_adapter_call`).
    ///
    /// Keyed by the module-level Rust spellings `(source, target)`; the value
    /// is `(ordinal, body)`. Helper names start with
    /// [`crate::RECORD_ADAPTER_HELPER_PREFIX`].
    pub(crate) record_adapter_helpers: ::std::cell::RefCell<
        ::std::collections::BTreeMap<(String, String), (usize, String)>,
    >,
    /// MIR ids of free functions that emit real Rust generics.
    ///
    /// A generic free function only keeps real generics when its signature is
    /// generic-safe AND its body keeps every type parameter opaque (the body
    /// trial in [`FunctionEmitter::emit`]). This decision must be identical at
    /// the definition and at every call site — otherwise a call would pass an
    /// argument through concretely to a parameter that was actually emitted as
    /// erased `SmeltUnknown`. The set is computed once by
    /// [`EmitContext::populate_generic_functions`] and read through
    /// [`EmitContext::is_generic_function`].
    generic_functions: RefCell<HashSet<FuncId>>,
    /// The functions whose generic status the running trial asked about.
    generic_reads: RefCell<HashSet<FuncId>>,
    /// Class symbols emitted as reference classes (handle newtype over
    /// `Rc<RefCell<Inner>>`), computed once by [`crate::classify`]. Every class
    /// not in this set stays a by-value value class with its current emission.
    reference_classes: HashSet<Symbol>,
    /// Closures stored on the instance they capture, which capture it weakly
    /// (see [`weak_receiver`]).
    weak_receivers: weak_receiver::WeakReceiverPlan,
    /// Which declared type-parameter positions each generated class and
    /// interface keeps in its emitted Rust, computed once by
    /// [`crate::generic_elision::compute`].
    ///
    /// A parameter the emitted Rust never spells is dropped from the struct,
    /// from every impl header and from every reference, so this map must be
    /// read by BOTH the declaration and the reference side or the two disagree
    /// on arity.
    type_param_elision: crate::generic_elision::TypeParamElision,
    /// Whether property writes through an erased reference-class view are
    /// forwarded to the instance (see [`view_write_through`]).
    erased_view_write_through: bool,
    /// Whether any body reads the dynamic `this` receiver channel (see
    /// [`receiver_bound_slot`]).
    program_reads_this: bool,
    /// Whether the crate carries a JavaScript `bigint` (see
    /// `Need::BigInt`). The `SmeltUnknown::BigInt` tag only
    /// exists in such a crate, so an exhaustive match over the erased carrier
    /// names that arm only when this is set.
    needs_bigint: bool,
    /// Whether some `console.log` argument took the `util.inspect` route, so
    /// the crate root must carry the `SmeltInspect` runtime (see
    /// [`console_inspect`]). Set while bodies are emitted, read after.
    console_inspect_used: ::std::cell::Cell<bool>,
    /// Erasures `rust_type` decided in the backend (erased generics and
    /// unions, unresolved classes, `never`); see `emitter::backend_erasure`.
    pub(crate) backend_erasures: RefCell<Vec<smelt_hir::ErasureRecord>>,
}

/// The state of an [`EmitContext`]'s body-render outputs at one point, restored
/// when the checkpoint is dropped.
///
/// A body render does not only produce text: it MINTS module-level items into
/// the shared context as it goes — function-item value accessors (erased,
/// erased-`SmeltErasedFunction` and typed), hoisted record conversion and
/// adapter helpers — flags the `util.inspect` runtime as used, and records the
/// backend's own erasure decisions. The generic body-cleanliness trial
/// (`EmitContext::populate_generic_functions`) renders bodies whose text is
/// thrown away; without a rollback everything those renders minted was still
/// flushed into the crate (helpers no emitted code calls, ordinals shifted by
/// discarded renders, erasure records for decisions the real emission never
/// made). Holding a checkpoint across the trial makes a discarded render leave
/// nothing behind.
///
/// The maps are cloned rather than journaled: trials run before any real
/// emission, when they are empty or hold only what earlier trials rolled
/// back, so the clones are of (near-)empty maps. `backend_erasures` is
/// append-only and is truncated back to its length.
pub(crate) struct ContextCheckpoint<'c> {
    /// The context restored on drop.
    context: &'c EmitContext,
    /// See [`EmitContext::function_item_accessors`].
    function_item_accessors: ::std::collections::BTreeMap<usize, String>,
    /// See [`EmitContext::function_item_erased_fn_accessors`].
    function_item_erased_fn_accessors: ::std::collections::BTreeMap<usize, String>,
    /// See [`EmitContext::function_item_typed_accessors`].
    function_item_typed_accessors:
        ::std::collections::BTreeMap<(usize, String), (usize, String)>,
    /// See [`EmitContext::record_conversion_helpers`].
    record_conversion_helpers: ::std::collections::BTreeMap<String, (usize, String)>,
    /// See [`EmitContext::record_adapter_helpers`].
    record_adapter_helpers: ::std::collections::BTreeMap<(String, String), (usize, String)>,
    /// See [`EmitContext::console_inspect_used`].
    console_inspect_used: bool,
    /// The number of backend erasures recorded at the checkpoint.
    backend_erasures_len: usize,
}

impl Drop for ContextCheckpoint<'_> {
    /// Restore every body-render output of the context to the checkpoint.
    fn drop(&mut self) {
        let context = self.context;
        *context.function_item_accessors.borrow_mut() =
            std::mem::take(&mut self.function_item_accessors);
        *context.function_item_erased_fn_accessors.borrow_mut() =
            std::mem::take(&mut self.function_item_erased_fn_accessors);
        *context.function_item_typed_accessors.borrow_mut() =
            std::mem::take(&mut self.function_item_typed_accessors);
        *context.record_conversion_helpers.borrow_mut() =
            std::mem::take(&mut self.record_conversion_helpers);
        *context.record_adapter_helpers.borrow_mut() =
            std::mem::take(&mut self.record_adapter_helpers);
        context.console_inspect_used.set(self.console_inspect_used);
        context
            .backend_erasures
            .borrow_mut()
            .truncate(self.backend_erasures_len);
    }
}

impl EmitContext {
    /// Builds crate-level name and signature indexes for Rust emission.
    pub(crate) fn new(mir: &Mir) -> Result<Self, EmitError> {
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
            .ok_or_else(|| EmitError::new("MIR is missing the None type"))?;
        let mut duplicate_counts = HashMap::<Symbol, usize>::new();
        for function in &mir.functions {
            if !matches!(
                function.origin,
                HirOrigin::ClassConstructor { .. } | HirOrigin::ClassMethod { .. }
            ) {
                let count = duplicate_counts.entry(function.name).or_insert(0usize);
                *count = count.saturating_add(1);
            }
        }

        let mut function_names = HashMap::new();
        let mut function_param_types = HashMap::new();
        let mut function_return_types = HashMap::new();
        let mut function_param_type_priorities = HashMap::<String, u8>::new();
        for function in &mir.functions {
            let source_name = mir
                .symbols
                .get(function.name)
                .ok_or_else(|| EmitError::new("function has unknown symbol"))?;
            let base = sanitize_ident(source_name);
            let rust_name =
                if !function.is_test && source_name == "main" && function.return_ty == none_ty {
                    // The binary's entry point keeps its Rust name; a SOURCE
                    // item named `main` renames (see `rust::sanitize_ident`).
                    "main".to_owned()
                } else if duplicate_counts.get(&function.name).copied().unwrap_or(0) > 1
                    || source_name.starts_with("__smelt_module_")
                {
                    format!("{}_{}", base, function.id.0)
                } else {
                    base
                };
            let params = function
                .params
                .iter()
                .map(|param| {
                    function
                        .locals
                        .get(id_index(param.0, "local index does not fit usize")?)
                        .map(|local| local.ty)
                        .ok_or_else(|| {
                            EmitError::new("function parameter references unknown local")
                        })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let priority = emitted_signature_priority(function);
            // Keyed by the emitted-signature key, not the bare Rust name: a
            // method's name is unique only inside its own `impl` block (see
            // `FunctionEmitter::emitted_signature_key_in`).
            let signature_key = core::emitted_signature_key_in(
                function,
                &rust_name,
                |symbol| mir.symbols.get(symbol).map(str::to_owned),
            );
            if function_param_type_priorities
                .get(&signature_key)
                .copied()
                .is_none_or(|existing| priority > existing)
            {
                function_param_types.insert(signature_key.clone(), params);
                function_return_types.insert(signature_key.clone(), function.return_ty);
                function_param_type_priorities.insert(signature_key, priority);
            }
            function_names.insert(function.id, rust_name);
        }
        let owned_callback_params = compute_owned_callback_params(mir)?;
        let demand = RuntimeDemand::of(mir);

        Ok(Self {
            none_ty,
            bound_traits: crate::bound_traits::BoundTraitPlan::compute(mir),
            test_runtime_resets: TestRuntimeResets {
                date_now: demand.has(Need::DateNow),
                timers: demand.has(Need::Timers),
            },
            function_names,
            function_param_types,
            function_return_types,
            owned_callback_params,
            function_item_accessors: ::std::cell::RefCell::new(
                ::std::collections::BTreeMap::new(),
            ),
            function_item_erased_fn_accessors: ::std::cell::RefCell::new(
                ::std::collections::BTreeMap::new(),
            ),
            function_item_typed_accessors: ::std::cell::RefCell::new(
                ::std::collections::BTreeMap::new(),
            ),
            record_conversion_helpers: ::std::cell::RefCell::new(::std::collections::BTreeMap::new()),
            record_adapter_helpers: ::std::cell::RefCell::new(::std::collections::BTreeMap::new()),
            generic_functions: RefCell::new(HashSet::new()),
            generic_reads: RefCell::new(HashSet::new()),
            weak_receivers: weak_receiver::WeakReceiverPlan::of(mir, &crate::classify::reference_classes(mir)),
            reference_classes: crate::classify::reference_classes(mir),
            type_param_elision: crate::generic_elision::compute(mir),
            erased_view_write_through: view_write_through::program_writes_erased_fields(mir),
            program_reads_this: demand.has(Need::ThisRead),
            needs_bigint: demand.has(Need::BigInt),
            demand,
            carrier_touched: std::cell::Cell::new(false),
            carrier_boundaries: std::cell::Cell::new(0),
            console_inspect_used: ::std::cell::Cell::new(false),
            backend_erasures: RefCell::new(Vec::new()),
        })
    }

    /// What this program demands of the runtime.
    pub(crate) const fn demand(&self) -> &RuntimeDemand {
        &self.demand
    }

    /// Capture everything a body render writes into this shared context, so a
    /// TRIAL render can be rolled back (see [`ContextCheckpoint`]).
    pub(crate) fn checkpoint(&self) -> ContextCheckpoint<'_> {
        ContextCheckpoint {
            context: self,
            function_item_accessors: self.function_item_accessors.borrow().clone(),
            function_item_erased_fn_accessors: self
                .function_item_erased_fn_accessors
                .borrow()
                .clone(),
            function_item_typed_accessors: self.function_item_typed_accessors.borrow().clone(),
            record_conversion_helpers: self.record_conversion_helpers.borrow().clone(),
            record_adapter_helpers: self.record_adapter_helpers.borrow().clone(),
            console_inspect_used: self.console_inspect_used.get(),
            backend_erasures_len: self.backend_erasures.borrow().len(),
        }
    }

    /// Records that the output touched the erased runtime carrier.
    ///
    /// The generic trial reads this: a body that erases or inspects a value
    /// cannot keep its type parameters opaque.
    pub(crate) fn note_carrier_use(&self) {
        if self.carrier_boundaries.get() == 0 {
            self.carrier_touched.set(true);
        }
    }

    /// Opens a real dynamic boundary, whose carrier uses say nothing about the
    /// surrounding body: a thrown payload, or a mutable-list adapter that erases
    /// and un-erases at the call it wraps. It closes when the guard drops.
    pub(crate) fn carrier_boundary(&self) -> CarrierBoundary<'_> {
        self.carrier_boundaries.set(self.carrier_boundaries.get().saturating_add(1));
        CarrierBoundary { context: self }
    }

    /// Answers whether the carrier was touched since the last call, and clears it.
    pub(crate) fn take_carrier_touched(&self) -> bool {
        self.carrier_touched.replace(false)
    }

    /// Record that a `console.log` argument renders through `SmeltInspect`.
    pub(crate) fn mark_console_inspect_used(&self) {
        self.console_inspect_used.set(true);
    }

    /// Whether any `console.log` argument rendered through `SmeltInspect`.
    pub(crate) fn console_inspect_used(&self) -> bool {
        self.console_inspect_used.get()
    }

    /// Whether the crate carries a JavaScript `bigint`, and therefore the
    /// `SmeltUnknown::BigInt` tag.
    pub(crate) const fn needs_bigint(&self) -> bool {
        self.needs_bigint
    }

    /// Return whether property writes through an erased view reach the
    /// reference-class instance it stands for (see [`view_write_through`]).
    pub(crate) fn erased_view_write_through(&self) -> bool {
        self.erased_view_write_through && !self.reference_classes.is_empty()
    }

    /// Return whether any body reads the dynamic `this` receiver channel, so a
    /// receiver installed by a call is observable (see [`receiver_bound_slot`]).
    pub(crate) fn program_reads_this(&self) -> bool {
        self.program_reads_this
    }

    /// Return whether any record type is emitted as a reference class.
    ///
    /// Gates the erased-view origin slot (see [`crate::reference_origin`]):
    /// only an erasure of a reference record ever stamps one.
    pub(crate) fn has_reference_classes(&self) -> bool {
        !self.reference_classes.is_empty()
    }

    /// The closures that capture their receiver weakly (see [`weak_receiver`]).
    pub(crate) fn weak_receivers(&self) -> &weak_receiver::WeakReceiverPlan {
        &self.weak_receivers
    }

    /// Return whether the class named `symbol` is emitted as a reference class.
    ///
    /// Reference classes use the handle-newtype representation with interior
    /// mutability; value classes keep the current by-value struct emission.
    pub(crate) fn is_reference_class(&self, symbol: Symbol) -> bool {
        self.reference_classes.contains(&symbol)
    }

    /// Return which type-parameter positions the emitted Rust declares.
    pub(crate) fn type_param_elision(&self) -> &crate::generic_elision::TypeParamElision {
        &self.type_param_elision
    }

    /// Compute, once, which free functions emit real Rust generics.
    ///
    /// Must be called after [`EmitContext::new`] and before any function is
    /// emitted. For each free function whose signature is generic-safe, this
    /// trial-renders the body with the type parameters in scope; the function
    /// keeps real generics only when the rendered body does not need the erased
    /// carrier (see `FunctionEmitter::renders_real_generics`). The result is
    /// shared by the definition and all call sites through
    /// [`EmitContext::is_generic_function`].
    ///
    /// The decision is a **fixed point**, not a single pass, because a trial
    /// body is not independent of the answer: a call site renders its arguments
    /// differently depending on whether the callee emits generics
    /// (`call::callee_free_function_type_params`), and since Increment 3 a
    /// lifted caller statically calling a demoted callee renders a demotion
    /// signal that dirties the caller's own trial body. A single pass answers
    /// that question with "no callee is generic yet", which makes every such
    /// caller demote for a reason the final emission does not reproduce.
    ///
    /// The iteration starts from the empty set — so the FIRST round reproduces
    /// the historical single-pass answer exactly — and recomputes the whole set
    /// each round until it stops changing. Recomputing rather than only removing
    /// is load-bearing: a caller demoted in one round because its callee looked
    /// generic must be reconsidered once that callee demotes, and vice versa.
    /// The result is self-consistent: every trial that produced it saw the same
    /// answer the emission will use.
    ///
    /// Convergence is not guaranteed in theory (the trial is not a monotone
    /// predicate: a callee's answer changes how its callers render), so the loop is capped and falls back to
    /// the first round's answer — the historical one — if it oscillates. In
    /// practice both compat corpora settle in two rounds.
    pub(crate) fn populate_generic_functions(&self, mir: &Mir) -> Result<GenericsDecided, EmitError> {
        /// Bound on the demotion rounds; each round holds out at least one more
        /// function, so the bound is never reached by a finite crate in
        /// practice and only caps a pathological one.
        const MAX_DEMOTION_ROUNDS: usize = 16;

        let signature_safe: Vec<&MirFunction> = mir
            .functions
            .iter()
            .filter(|function| matches!(function.origin, HirOrigin::Body(_)))
            .filter(|function| {
                crate::generic_lifting::function_emits_rust_generics(
                    mir,
                    function,
                    &self.owned_callback_params,
                    &self.bound_traits,
                )
            })
            .collect();

        // Functions demoted because a caller forwarding them its own bounded
        // type parameter did not lift it. Empty for every program without a
        // class- or interface-bounded generic, so the loop below then runs the
        // trial fixed point exactly once, as it always did.
        let mut forced_out: HashSet<FuncId> = HashSet::new();
        for _ in 0..MAX_DEMOTION_ROUNDS {
            let mut settled = self.trial_fixed_point(mir, &signature_safe, &forced_out)?;
            let before = settled.len();
            self.demote_unforwardable_bounded_callees(mir, &mut settled);
            if settled.len() == before {
                *self.generic_functions.borrow_mut() = settled;
                return Ok(GenericsDecided(()));
            }
            // A demoted callee changes how its lifted callers render their
            // calls, so the trials are rerun with it held out.
            forced_out.extend(
                self.generic_functions
                    .borrow()
                    .iter()
                    .copied()
                    .filter(|id| !settled.contains(id)),
            );
            *self.generic_functions.borrow_mut() = settled;
        }
        Ok(GenericsDecided(()))
    }

    /// The body-cleanliness trial iterated to a fixed point (see
    /// [`Self::populate_generic_functions`]), never admitting a function in
    /// `forced_out`. Leaves the answer in `generic_functions` and returns it.
    fn trial_fixed_point(
        &self,
        mir: &Mir,
        signature_safe: &[&MirFunction],
        forced_out: &HashSet<FuncId>,
    ) -> Result<HashSet<FuncId>, EmitError> {
        /// Rounds attempted before declaring the iteration non-convergent.
        const MAX_ROUNDS: usize = 16;

        self.generic_functions.borrow_mut().clear();
        let mut first_round: Option<HashSet<FuncId>> = None;
        // A trial's answer depends on the crate only through the callees whose
        // status it read, so a round re-runs just the trials that read a status
        // the previous round changed.
        let mut trials: HashMap<FuncId, TrialOutcome> = HashMap::new();
        let mut changed: HashSet<FuncId> = HashSet::new();
        for _ in 0..MAX_ROUNDS {
            let mut next = HashSet::new();
            for function in signature_safe {
                if forced_out.contains(&function.id) {
                    continue;
                }
                let outcome = match trials.get(&function.id) {
                    Some(outcome) if outcome.reads.is_disjoint(&changed) => outcome,
                    _ => {
                        self.generic_reads.borrow_mut().clear();
                        let emitter = FunctionEmitter::for_generic_trial(mir, self, function)?;
                        let generic = emitter.renders_real_generics()?;
                        let reads = self.generic_reads.take();
                        trials.insert(function.id, TrialOutcome { generic, reads });
                        trials
                            .get(&function.id)
                            .ok_or_else(|| EmitError::new("trial outcome was not recorded"))?
                    }
                };
                if outcome.generic {
                    next.insert(function.id);
                }
            }
            if first_round.is_none() {
                first_round = Some(next.clone());
            }
            changed = self.generic_functions.borrow().symmetric_difference(&next).copied().collect();
            if changed.is_empty() {
                return Ok(next);
            }
            *self.generic_functions.borrow_mut() = next;
        }
        // Non-convergent: keep the historical single-pass answer rather than an
        // arbitrary round of an oscillation.
        let historical = first_round.unwrap_or_default();
        self.generic_functions.borrow_mut().clone_from(&historical);
        Ok(historical)
    }

    /// Remove from `generic` every function that receives a caller's own
    /// bounded type parameter (see `classes::bound_forwarding_callers`) from a
    /// caller that does not lift that parameter, to a fixed point.
    ///
    /// `liftable_type_params` accepts `loud(a)` inside
    /// `twice<A extends Animal>(a: A)` because the caller's `A` will carry
    /// `AnimalLike` too — but only if `twice` itself ends up generic. When it
    /// does not, `a` is a `SmeltUnknown` there, which implements no bound
    /// trait, so the callee must take the erased signature as well. Removing a
    /// callee can in turn strand its own forwarding callees, hence the loop.
    fn demote_unforwardable_bounded_callees(&self, mir: &Mir, generic: &mut HashSet<FuncId>) {
        loop {
            let stranded: Vec<FuncId> = generic
                .iter()
                .copied()
                .filter(|id| {
                    let Some(function) = usize::try_from(id.0).ok().and_then(|index| mir.functions.get(index)) else {
                        return false;
                    };
                    crate::generic_lifting::bound_forwarding_callers(mir, function)
                        .into_iter()
                        .any(|(caller_id, caller_param)| {
                            !generic.contains(&caller_id)
                                || usize::try_from(caller_id.0).ok().and_then(|index| mir.functions.get(index)).is_none_or(|caller| {
                                    !crate::generic_lifting::liftable_type_params(
                                        mir,
                                        caller,
                                        &self.owned_callback_params,
                                        &self.bound_traits,
                                    )
                                    .contains(&caller_param)
                                })
                        })
                })
                .collect();
            if stranded.is_empty() {
                return;
            }
            for id in stranded {
                generic.remove(&id);
            }
        }
    }

    /// Return whether the free function with `id` emits real Rust generics.
    pub(crate) fn is_generic_function(&self, id: FuncId) -> bool {
        self.generic_reads.borrow_mut().insert(id);
        self.generic_functions.borrow().contains(&id)
    }
}

/// Return precedence for cross-module emitted ABI signatures.
///
/// Manifest builds can contain imported or test-local function entries that
/// resolve to the same Rust symbol as the real source function. Calls must be
/// adapted to the concrete Rust function definition, so body-backed non-test
/// functions win over test/import-like entries when the registry sees the same
/// emitted name more than once.
fn emitted_signature_priority(function: &MirFunction) -> u8 {
    match (function.is_test, function.origin) {
        (false, HirOrigin::Body(_)) => 2,
        (false, _) => 1,
        (true, _) => 0,
    }
}

/// Computes which callback parameters need owned reentrant `Rc<dyn Fn...>` handles.
///
/// A callback parameter needs ownership if it escapes its defining function, or
/// if it is forwarded to another function parameter that itself needs
/// ownership. The latter is a fixpoint because helper functions often pass
/// callbacks through multiple layers before one layer returns or stores them.
fn compute_owned_callback_params(mir: &Mir) -> Result<HashSet<(FuncId, LocalId)>, EmitError> {
    let mut owned = HashSet::new();
    for function in &mir.functions {
        for param in &function.params {
            let local = function
                .locals
                .get(id_index(param.0, "local index does not fit usize")?)
                .ok_or_else(|| EmitError::new("function parameter references unknown local"))?;
            if !matches!(mir.types.get(local.ty), Some(Type::Function(_))) {
                continue;
            }
            if callback_param_escapes_locally(mir, function, *param)? {
                owned.insert((function.id, *param));
            }
        }
    }

    let mut changed = true;
    while changed {
        changed = false;
        for function in &mir.functions {
            for block in &function.blocks {
                let Some(Terminator::Call {
                    callee: Callee::Static(callee_id),
                    args,
                    ..
                }) = &block.terminator
                else {
                    continue;
                };
                let Some(callee) = mir
                    .functions
                    .get(id_index(callee_id.0, "function index does not fit usize")?)
                else {
                    continue;
                };
                let closure_defs = closure_definitions(function)?;
                for (arg, callee_param) in args.iter().zip(callee.params.iter()) {
                    if !owned.contains(&(callee.id, *callee_param))
                        && !callee_param_is_owned_callback_sink(mir, callee, *callee_param)
                    {
                        continue;
                    }
                    let Some(arg_local) = arg.local() else {
                        continue;
                    };
                    if function.params.contains(&arg_local)
                        && owned.insert((function.id, arg_local))
                    {
                        changed = true;
                    }
                    if let Some(closure_id) = closure_defs.get(&arg_local)
                        && let Some(closure) = mir
                            .closures
                            .get(id_index(closure_id.0, "closure index does not fit usize")?)
                    {
                        for capture in &closure.captures {
                            if function.params.contains(&capture.source_local)
                                && owned.insert((function.id, capture.source_local))
                            {
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(owned)
}

/// Return whether a callee parameter is a position that always stores an owned
/// callback handle, so any callback argument passed into it must itself be owned.
///
/// An `Optional<fn(..)>` (and any container-wrapped function) parameter cannot be
/// emitted as a borrowed `&dyn Fn`: it lowers to `Option<Rc<dyn Fn>>` (an owned,
/// `'static` handle). Passing a borrowed callback parameter straight into such a
/// sink forces codegen to wrap it in an escaping `Rc::new(move || borrowed(..))`
/// adapter, and a non-`'static` borrow cannot satisfy the `Rc`'s `'static` bound
/// (borrowck "lifetime may not live long enough"). Treating these sinks as
/// ownership requirements propagates ownership back to the caller's parameter so
/// it is emitted as an owned `Rc<dyn Fn>` in the first place. A bare `Type::Function`
/// sink is handled separately through the `owned` set (it may still be borrowed).
fn callee_param_is_owned_callback_sink(
    mir: &Mir,
    callee: &MirFunction,
    param: LocalId,
) -> bool {
    let Ok(index) = id_index(param.0, "local index does not fit usize") else {
        return false;
    };
    let Some(local) = callee.locals.get(index) else {
        return false;
    };
    match mir.types.get(local.ty) {
        Some(Type::Optional(inner)) => {
            matches!(mir.types.get(*inner), Some(Type::Function(_)))
        }
        _ => false,
    }
}

/// Returns whether a callback parameter escapes inside its own function body.
fn callback_param_escapes_locally(
    mir: &Mir,
    function: &MirFunction,
    local: LocalId,
) -> Result<bool, EmitError> {
    let directly_returned = function.blocks.iter().any(|block| {
        matches!(
            &block.terminator,
            Some(Terminator::Return(Operand::Copy(place) | Operand::Move(place)))
                if *place == Place::Local(local)
        )
    });
    let closure_ids = function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match statement {
            Statement::Assign {
                value: Rvalue::Closure { id, .. },
                ..
            }
            | Statement::AssignPlace {
                value: Rvalue::Closure { id, .. },
                ..
            } => Some(*id),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let captured_by_any_closure = mir
        .closures
        .iter()
        .filter(|closure| closure_ids.contains(&closure.id))
        .any(|closure| {
            closure
                .captures
                .iter()
                .any(|capture| capture.source_local == local)
        });
    let closure_defs = closure_definitions(function)?;
    let captured_by_erased_closure_value = function.blocks.iter().any(|block| {
        block.statements.iter().any(|statement| {
            let Statement::Assign { dest, value } = statement else {
                return false;
            };
            let Some(dest_ty) = id_index(dest.0, "local index does not fit usize")
                .ok()
                .and_then(|index| function.locals.get(index))
                .map(|decl| decl.ty)
            else {
                return false;
            };
            if !type_erases_values(mir, dest_ty) {
                return false;
            }
            let maybe_source_local = match value {
                Rvalue::Use(
                    Operand::Copy(Place::Local(source) | Place::Field { base: source, .. })
                    | Operand::Move(Place::Local(source) | Place::Field { base: source, .. }),
                ) => Some(*source),
                _ => None,
            };
            let Some(source_local) = maybe_source_local else {
                return false;
            };
            let Some(closure_id) = closure_defs.get(&source_local) else {
                return false;
            };
            mir.closures
                .get(
                    id_index(closure_id.0, "closure index does not fit usize")
                        .unwrap_or(usize::MAX),
                )
                .is_some_and(|closure| {
                    closure
                        .captures
                        .iter()
                        .any(|capture| capture.source_local == local)
                })
        })
    });
    let captured_by_erased_return = type_erases_values(mir, function.return_ty)
        && function.blocks.iter().any(|block| {
            let Some(Terminator::Return(
                Operand::Copy(Place::Local(returned)) | Operand::Move(Place::Local(returned)),
            )) = &block.terminator
            else {
                return false;
            };
            closure_source_for_local(function, *returned, Some(&closure_defs))
                .and_then(|closure_id| {
                    mir.closures.get(
                        id_index(closure_id.0, "closure index does not fit usize")
                            .unwrap_or(usize::MAX),
                    )
                })
                .is_some_and(|closure| {
                    closure
                        .captures
                        .iter()
                        .any(|capture| capture.source_local == local)
                })
        });
    let erased_or_dynamic_escape = function.blocks.iter().any(|block| {
        block
            .statements
            .iter()
            .any(|statement| statement_erases_callback_param(mir, function, statement, local))
    });
    // A callback parameter PACKED INTO A CONTAINER (a list/set/tuple/dict
    // literal, including the one a call site builds for a container-shaped or
    // rest callback parameter) escapes. Every emitted container stores a
    // function-typed element as an owned `::std::rc::Rc<dyn Fn(..)>` handle —
    // `SmeltList<Rc<dyn Fn(..)>>` — and no container type carries a lifetime
    // parameter, so a borrowed `&dyn Fn` element has to be wrapped in an
    // `Rc::new(move |..| (&*borrowed)(..))` adapter whose `'static` bound the
    // borrow cannot meet (E0521 "borrowed data escapes outside of function").
    // The rule is about the DESTINATION's representation, not about who is
    // called with it, so it holds for a local container as much as for the
    // temporary a call site builds for `cbs: ((v: T) => T)[]` / `...cbs`.
    let packed_into_container = function.blocks.iter().any(|block| {
        block.statements.iter().any(|statement| {
            statement_packs_callback_param_into_container(mir, function, statement, local)
        })
    });
    // A callback parameter that is rebound in the body (`callback = …`, e.g. the
    // `mapAsync`/`filterAsync` concurrency wrappers reassign their callback to a
    // `limitAsync`-wrapped handle) receives an owned `Rc<dyn Fn…>` on the right.
    // A borrowed `&dyn Fn` binding cannot hold that owned value, so the parameter
    // must enter the function as an owned handle.
    let rebound_locally = function.blocks.iter().any(|block| {
        block.statements.iter().any(|statement| match statement {
            Statement::Assign { dest, .. } => *dest == local,
            Statement::AssignPlace {
                place: Place::Local(candidate),
                ..
            } => *candidate == local,
            _ => false,
        })
    });
    // A callback passed as the function argument of a timer async op
    // (`setTimeout`/`setInterval`) is wrapped by the backend into a `'static`
    // timer closure (see the emitter's timer callback lowering). A borrowed
    // `&dyn Fn` cannot escape into that `'static` closure, so the parameter must
    // enter the function as an owned handle. Mirrors es-toolkit `delay`.
    let escapes_into_timer = function.blocks.iter().any(|block| {
        block.statements.iter().any(|statement| {
            let value = match statement {
                Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } => value,
                Statement::DictEntryUpdate { .. } => return false,
            };
            matches!(
                value,
                Rvalue::AsyncOp {
                    op: smelt_hir::AsyncOp::SetTimeout | smelt_hir::AsyncOp::SetInterval,
                    args,
                } if matches!(
                    args.first(),
                    Some(Operand::Copy(Place::Local(callback)) | Operand::Move(Place::Local(callback)))
                        if *callback == local
                )
            )
        })
    });
    // A borrowed callback parameter bound to a LOCAL whose own Rust type is the
    // owned `SmeltErasedFunction` struct (the erased-unknown-rest shape) has to
    // enter the function owned. `SmeltErasedFunction` stores its callback as an
    // `Rc<dyn Fn(Vec<SmeltUnknown>) -> SmeltUnknown>`, i.e. a `'static` trait
    // object: a borrowed `&dyn Fn` cannot be wrapped into one (the borrow would
    // escape). Emitting a bare `Rc::new(move |..| borrowed(..))` closure instead
    // — which is what the borrowed-handle path used to do — types the local as
    // something that is NOT `SmeltErasedFunction`, so the call site's erased
    // `.call(..)` ABI disagreed with the value and produced E0658 + E0308.
    // Requiring ownership here keeps the value and its call ABI in agreement
    // with the type the renderer already assigned to that local.
    let bound_to_erased_handle = function.blocks.iter().any(|block| {
        block.statements.iter().any(|statement| {
            let Statement::Assign { dest, value } = statement else {
                return false;
            };
            let reads_param = matches!(
                value,
                Rvalue::Use(
                    Operand::Copy(Place::Local(source)) | Operand::Move(Place::Local(source)),
                ) if *source == local
            );
            if !reads_param {
                return false;
            }
            let Some(dest_ty) = id_index(dest.0, "local index does not fit usize")
                .ok()
                .and_then(|index| function.locals.get(index))
                .map(|decl| decl.ty)
            else {
                return false;
            };
            matches!(
                mir.types.get(dest_ty),
                Some(Type::Function(dest_function))
                    if !dest_function.may_throw
                        && is_erased_unknown_rest_function_in(
                            &mir.types,
                            dest_function,
                        )
            )
        })
    });
    // An async function runs its entire body inside the returned future, so any
    // callback parameter it references is retained past the caller's statement
    // (the future outlives the call expression). A borrowed `&dyn Fn` parameter
    // cannot survive that, and callers passing `&*(temporary)` would drop the
    // temporary at the end of the call statement (E0716/E0515). Async
    // function-typed params must therefore enter as owned `Rc<dyn Fn…>` handles.
    let retained_by_async_body = function.is_async;
    Ok(directly_returned
        || packed_into_container
        || bound_to_erased_handle
        || retained_by_async_body
        || rebound_locally
        || escapes_into_timer
        || erased_or_dynamic_escape
        || captured_by_erased_closure_value
        || captured_by_erased_return
        || type_holds_function(mir, function.return_ty, NominalStorage::Followed)
        || (matches!(mir.types.get(function.return_ty), Some(Type::Function(_)))
            && captured_by_any_closure)
        || mir.closures.iter().any(|closure| {
            closure_ids.contains(&closure.id)
                && closure.escapes
                && closure
                    .captures
                    .iter()
                    .any(|capture| capture.source_local == local)
        }))
}

/// Return whether a statement may put a callback parameter behind erased state.
///
/// `SmeltUnknown::Function` is stored as a `'static` callable handle in the
/// generated runtime. If a source callback parameter is wrapped into unknown
/// state, or passed to an erased closure-call result, the parameter cannot stay
/// as `&dyn Fn`; it must enter the function as an owned handle.
fn statement_erases_callback_param(
    mir: &Mir,
    function: &MirFunction,
    statement: &Statement,
    local: LocalId,
) -> bool {
    let Statement::Assign {
        dest,
        value: statement_value,
    } = statement
    else {
        return false;
    };
    let dest_ty = id_index(dest.0, "local index does not fit usize")
        .ok()
        .and_then(|dest_index| function.locals.get(dest_index))
        .map(|decl| decl.ty);
    let function_erases_return = type_erases_values(mir, function.return_ty);
    let closure_defs = closure_definitions(function).ok();
    match statement_value {
        Rvalue::Dict(entries) => {
            (function_erases_return || dest_ty.is_some_and(|ty| type_erases_values(mir, ty)))
                && entries.iter().any(|(_, entry_value)| {
                    operand_refs_callback_param_or_capturing_closure(
                        mir,
                        function,
                        entry_value,
                        local,
                        closure_defs.as_ref(),
                    )
                })
        }
        Rvalue::List(items) | Rvalue::Set(items) | Rvalue::Tuple(items) => {
            (function_erases_return || dest_ty.is_some_and(|ty| type_erases_values(mir, ty)))
                && items.iter().any(|item| {
                    operand_refs_callback_param_or_capturing_closure(
                        mir,
                        function,
                        item,
                        local,
                        closure_defs.as_ref(),
                    )
                })
        }
        Rvalue::ClosureCall { args, .. } => {
            dest_ty.is_some_and(|ty| type_erases_values(mir, ty))
                && args.iter().any(|arg| {
                    operand_refs_callback_param_or_capturing_closure(
                        mir,
                        function,
                        arg,
                        local,
                        closure_defs.as_ref(),
                    )
                })
        }
        Rvalue::CallableObjectAssign {
            callable,
            props,
            spreads,
        } => {
            dest_ty.is_some_and(|ty| type_erases_values(mir, ty))
                && (callable.local() == Some(local)
                    || callable.local()
                        .and_then(|callable_local| {
                            closure_source_for_local(
                                function,
                                callable_local,
                                closure_defs.as_ref(),
                            )
                        })
                        .and_then(|closure_id| {
                            mir.closures.get(
                                id_index(closure_id.0, "closure index does not fit usize").ok()?,
                            )
                        })
                        .is_some_and(|closure| {
                            closure
                                .captures
                                .iter()
                                .any(|capture| capture.source_local == local)
                        })
                    || props
                        .iter()
                        .any(|(_, prop_value)| prop_value.local() == Some(local))
                    || spreads
                        .iter()
                        .any(|spread_value| spread_value.local() == Some(local)))
        }
        Rvalue::Use(operand) => {
            dest_ty.is_some_and(|ty| type_erases_values(mir, ty))
                && operand.local() == Some(local)
        }
        _ => false,
    }
}

/// Return whether a statement packs a callback parameter into a container value.
///
/// A container literal (`Rvalue::List`/`Set`/`Tuple`/`Dict`) whose destination
/// type stores function values renders those elements as owned
/// `::std::rc::Rc<dyn Fn(..)>` handles: `SmeltList<Rc<dyn Fn(..)>>`,
/// `SmeltRecord`, and the tuple/set equivalents all own their elements and none
/// of them carries a lifetime parameter. A borrowed `&dyn Fn` parameter placed
/// into one is therefore wrapped in an `Rc::new(move |..| (&*borrowed)(..))`
/// adapter that outlives the borrow, which borrowck rejects with E0521
/// ("borrowed data escapes outside of function"; `'1` must outlive `'static`).
/// Counting this as an escape makes the parameter enter its function as an owned
/// handle, which the adapter can then move into the container.
///
/// This is deliberately independent of `type_erases_values`: the erasure gate in
/// [`statement_erases_callback_param`] only catches a container that *also*
/// erases (`SmeltList<SmeltUnknown>`), while a concretely typed
/// `SmeltList<Rc<dyn Fn(f64) -> f64>>` owns its elements just as hard. It is
/// also independent of the callee: the container may be a local (`const kept =
/// [cb]`) or the temporary a call site builds for a container-shaped or rest
/// callback parameter (`run(items, [cb])`, `run(items, cb)`).
///
/// A container whose element type is *not* function-shaped cannot be holding the
/// callback at all, so the destination type is checked with
/// [`type_holds_function`] rather than assuming every container is a sink.
fn statement_packs_callback_param_into_container(
    mir: &Mir,
    function: &MirFunction,
    statement: &Statement,
    local: LocalId,
) -> bool {
    let (dest, value) = match statement {
        Statement::Assign { dest, value } => (*dest, value),
        Statement::AssignPlace {
            place: Place::Local(dest) | Place::Field { base: dest, .. },
            value,
        } => (*dest, value),
        _ => return false,
    };
    let Some(dest_ty) = id_index(dest.0, "local index does not fit usize")
        .ok()
        .and_then(|index| function.locals.get(index))
        .map(|decl| decl.ty)
    else {
        return false;
    };
    if !type_holds_function(mir, dest_ty, NominalStorage::Followed) {
        return false;
    }
    let closure_defs = closure_definitions(function).ok();
    let mut refs = |operand: &Operand| {
        operand_refs_callback_param_or_capturing_closure(
            mir,
            function,
            operand,
            local,
            closure_defs.as_ref(),
        )
    };
    match value {
        Rvalue::List(items) | Rvalue::Set(items) | Rvalue::Tuple(items) => {
            items.iter().any(&mut refs)
        }
        Rvalue::Dict(entries) => entries.iter().any(|(_, entry_value)| refs(entry_value)),
        _ => false,
    }
}

/// Return whether an erased operand is a callback parameter or a closure capturing it.
fn operand_refs_callback_param_or_capturing_closure(
    mir: &Mir,
    function: &MirFunction,
    operand: &Operand,
    local: LocalId,
    closure_defs: Option<&HashMap<LocalId, smelt_mir::ClosureId>>,
) -> bool {
    if operand.local() == Some(local) {
        return true;
    }
    operand.local()
        .and_then(|operand_local| closure_source_for_local(function, operand_local, closure_defs))
        .and_then(|closure_id| {
            mir.closures
                .get(id_index(closure_id.0, "closure index does not fit usize").ok()?)
        })
        .is_some_and(|closure| {
            closure
                .captures
                .iter()
                .any(|capture| capture.source_local == local)
        })
}

/// Resolve a local through simple copy aliases to the closure assigned to it.
fn closure_source_for_local(
    function: &MirFunction,
    local: LocalId,
    closure_defs: Option<&HashMap<LocalId, smelt_mir::ClosureId>>,
) -> Option<smelt_mir::ClosureId> {
    let closure_definitions = closure_defs?;
    if let Some(closure_id) = closure_definitions.get(&local).copied() {
        return Some(closure_id);
    }
    let mut current = local;
    let mut seen = HashSet::new();
    while seen.insert(current) {
        let next = function.blocks.iter().find_map(|block| {
            block.statements.iter().find_map(|statement| {
                let Statement::Assign { dest, value } = statement else {
                    return None;
                };
                if *dest != current {
                    return None;
                }
                match value {
                    Rvalue::Use(
                        Operand::Copy(Place::Local(source) | Place::Field { base: source, .. })
                        | Operand::Move(Place::Local(source) | Place::Field { base: source, .. }),
                    ) => Some(*source),
                    _ => None,
                }
            })
        })?;
        if let Some(closure_id) = closure_definitions.get(&next).copied() {
            return Some(closure_id);
        }
        current = next;
    }
    None
}

/// Return whether a Rust value of `ty` erases nested values into unknown state.
fn type_erases_values(mir: &Mir, ty: TypeId) -> bool {
    match mir.types.get(ty) {
        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => true,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
            type_erases_values(mir, *item)
        }
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            type_erases_values(mir, *key) || type_erases_values(mir, *value)
        }
        Some(Type::Tuple(items)) => items.iter().any(|item| type_erases_values(mir, *item)),
        _ => false,
    }
}

/// Return closure definitions in a function keyed by destination local.
fn closure_definitions(
    function: &MirFunction,
) -> Result<HashMap<LocalId, smelt_mir::ClosureId>, EmitError> {
    let mut definitions = HashMap::new();
    for block in &function.blocks {
        for statement in &block.statements {
            match statement {
                Statement::Assign {
                    dest,
                    value: Rvalue::Closure { id, .. },
                } => {
                    definitions.insert(*dest, *id);
                }
                Statement::Assign {
                    dest,
                    value: Rvalue::Use(Operand::Copy(place) | Operand::Move(place)),
                } => {
                    if let Place::Local(source) = place
                        && let Some(id) = definitions.get(source).copied()
                    {
                        definitions.insert(*dest, id);
                    }
                }
                _ => {}
            }
        }
    }
    Ok(definitions)
}

/// What one body-cleanliness trial answered and what its answer rests on.
struct TrialOutcome {
    /// Whether the function keeps its generics.
    generic: bool,
    /// The functions whose generic status the trial read.
    reads: HashSet<FuncId>,
}

/// An open [`EmitContext::carrier_boundary`].
pub(crate) struct CarrierBoundary<'a> {
    /// The context whose boundary this holds open.
    context: &'a EmitContext,
}

impl Drop for CarrierBoundary<'_> {
    fn drop(&mut self) {
        let open = self.context.carrier_boundaries.get();
        self.context.carrier_boundaries.set(open.saturating_sub(1));
    }
}

/// Emits Rust source for one MIR function.
pub(crate) struct FunctionEmitter<'mir> {
    /// Reference to the MIR.
    mir: &'mir Mir,
    /// Shared crate-level emission indexes.
    context: &'mir EmitContext,
    /// The function being emitted.
    function: &'mir MirFunction,
    /// How each local is bound in the generated Rust scope.
    bindings: HashMap<LocalId, local_binding::LocalBinding>,
    /// Binding uses recorded while this body and its nested closures render.
    binding_uses: local_binding::SharedBindingUses,
    /// Set of locals that are mutated.
    mutable_locals: HashSet<LocalId>,
    /// Locals that have already been introduced in the generated Rust scope.
    declared_locals: RefCell<HashSet<LocalId>>,
    /// Locals that must be declared before structured block emission.
    predeclared_locals: HashSet<LocalId>,
    /// Predeclared shared-capture locals whose `Rc<RefCell<T>>` cell is
    /// declared uninitialized and created at the local's single assignment
    /// (see `capture_cell_created_at_assignment`), instead of being seeded
    /// with `T`'s default at function entry.
    deferred_capture_cells: RefCell<HashSet<LocalId>>,
    /// Locals whose emitted Rust type is `&'static str` rather than `String`.
    ///
    /// A `typeof` produces one of seven fixed spellings, so its destination
    /// never needs an owned copy. See [`typeof_str_locals`] for exactly which
    /// locals qualify and why the analysis is a whitelist.
    static_typeof_str_locals: HashSet<LocalId>,
    /// Compiler temporaries folded into the `throw` expression that consumes
    /// them, so neither their declaration nor their assignment is emitted.
    folded_throw_payloads: HashSet<LocalId>,
    /// Cached termination queries for this function CFG.
    termination_cache: RefCell<HashMap<smelt_mir::BlockId, bool>>,
    /// Per-local write facts for this function, built in one pass on first use
    /// (see `local_analysis::LocalWriteIndex`), so per-local mutability queries
    /// do not each rescan every block.
    local_write_index: std::cell::OnceCell<local_analysis::LocalWriteIndex>,
    /// Immediate post-dominator of every block of this function CFG, computed
    /// on first use (see `post_dominance`).
    post_dominators: RefCell<Option<Vec<Option<smelt_mir::BlockId>>>>,
    /// Cached loop-exit shape queries keyed by block, continue target, and break target.
    loop_exit_cache:
        RefCell<HashMap<(smelt_mir::BlockId, smelt_mir::BlockId, smelt_mir::BlockId), bool>>,
    /// Captured callback names that are emitted as borrowed `Fn` values.
    borrowed_callback_names: HashSet<String>,
    /// Parameter locals whose emitted Rust binding is already a shared
    /// reference (`name: &T`) rather than an owned `T`.
    ///
    /// A closure assigned to a `dyn Fn(&T, ..)` spells its parameters with the
    /// `&` that ABI owes them (see `callback_param_is_shared_reference`), but
    /// MIR still types the local as the bare `T`. A call site that wants a `&T`
    /// argument would then borrow the place again and hand the callee a `&&T`,
    /// which only compiles because Rust deref-coerces it away. Recording the
    /// borrowed parameters lets such a call pass the binding straight through,
    /// which is what a hand-writing Rust team would spell.
    by_reference_param_locals: HashSet<LocalId>,
    /// Record types currently being wrapped or extracted through erased objects.
    ///
    /// Callback-bearing records can contain their own option type again, so
    /// this bounds structural expansion of cyclic TypeScript object shapes.
    record_conversion_stack: RefCell<Vec<TypeId>>,
    /// Source/destination type pairs whose *structural* coercion is currently
    /// being expanded.
    ///
    /// Two coercions rebuild a value out of its parts and can therefore ask for
    /// a nested coercion of a component type: the field-wise record adapter
    /// ([`FunctionEmitter::structural_record_adapter_text`]) and the
    /// callable-object `__smelt_call` read
    /// ([`FunctionEmitter::callable_object_call_slot_text`], whose slot coercion
    /// goes on to adapt the call signature's return value). TypeScript callable
    /// interfaces routinely make those two mutually recursive — es-toolkit's
    /// `CurriedFunction1<T1, R>` has a call signature returning
    /// `CurriedFunction1<T1, R>`, so adapting the record adapts a function whose
    /// return value is the record again. Recording the pair being expanded lets
    /// the inner request decline (fall back to the caller's remaining rules)
    /// instead of recursing until the stack is gone.
    type_expansion_stack: RefCell<Vec<(TypeId, TypeId)>>,
    /// The type ID of the None type.
    none_ty: TypeId,
    /// Synthetic unknown local used when malformed MIR references a missing local.
    unknown_local: LocalDecl,
    /// What this emitter renders (own item, inherited copy, base initializer
    /// copy, closure body) and the scope that role carries; see
    /// [`emit_role`]. Fixed by the constructor, except an own item's
    /// [`emit_role::OwnGenerics`] decision.
    role: emit_role::EmitRole,
    /// Set while rendering text that is HOISTED to module level.
    ///
    /// `function_item_erased_accessor` builds the body of a module-level
    /// `fn __smelt_fn_value_<key>() -> SmeltUnknown` accessor, but it builds it
    /// from inside whatever function first referenced the function item. That
    /// accessor declares no type parameters, so any `T` the enclosing function
    /// has in scope must NOT be spelled in it — the hoisted item would reference
    /// an undeclared type (`E0412`). While this is set,
    /// [`FunctionEmitter::current_function_type_params`] answers with the empty
    /// scope, which is the truthful answer for a module-level item: a generic
    /// function's erased *value* is its `T = SmeltUnknown` instantiation.
    hoisted_module_item: std::cell::Cell<bool>,
    /// Set while a bound from the inherited type-argument map is rendered, so
    /// the map does not apply inside it; read as `types::InheritedScope` and
    /// held through the `InheritedBoundSuspension` guard.
    inherited_bound_suspended: std::cell::Cell<bool>,
    /// Structured control-flow emission state (recursion depths, the flow of
    /// the last emitted region tail, pending loop joins); see [`flow_state`].
    flow: flow_state::ControlFlowState,
}

/// Wraps a list-valued expression so it reads as its backing `Vec`.
///
/// `SmeltList<T>` keeps its elements in a shared `Rc<RefCell<Vec<T>>>` (a JS
/// array is a reference value), so there is no `Deref` to a `&Vec<T>`: every
/// read receiver has to take its own short-lived borrow. The returned `Ref`
/// guard lives to the end of the enclosing statement, which is safe because
/// emitted code is three-address — see the invariant on
/// [`FunctionEmitter::local_value_text`].
///
/// Use this, never [`list_write_text`], for a *read*: an index expression such
/// as `arr.get({ .. arr.len() .. })` takes two simultaneous borrows of one cell,
/// and two shared borrows are allowed where a second mutable one panics with
/// "already borrowed".
pub(super) fn list_read_text(list_text: &str) -> String {
    format!("{list_text}.borrow()")
}

/// Wraps a list-valued expression so it reads as its backing `Vec`, mutably.
///
/// The `RefMut` guard reaches `Vec`'s `&mut self` surface — `push`, `resize`,
/// `sort_by`, `IndexMut` — and the write lands in the buffer every other handle
/// on the array shares, which is what makes `b.push(x)` visible through `a`.
/// The receiver itself is only borrowed immutably (`RefCell` is interior
/// mutability), so this composes with a receiver held behind `&`.
///
/// Never combine this with [`list_read_text`] on the same list inside one
/// expression: the two guards would coexist and panic "already borrowed".
pub(super) fn list_write_text(list_text: &str) -> String {
    format!("{list_text}.borrow_mut()")
}

/// How to compute the default end bound for a slice.
#[derive(Clone, Copy)]
enum SliceLenKind {
    /// Use `.len()`.
    Len,
    /// Use `.chars().count()`.
    Chars,
}

/// Which ABI the callee behind an erased `smelt_args` adapter was rendered with.
///
/// See [`FunctionEmitter::function_args_from_smelt_args_text`]: the two callers
/// of that ladder disagree about the callee, not about the by-shared-reference
/// rule itself, so each states which callee it holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum ErasedCallTargetAbi {
    /// The callee is reached through a declared function type, so its parameters
    /// follow [`FunctionEmitter::callback_param_is_shared_reference`].
    Declared,
    /// The callee is a closure this emitter rendered against some other
    /// contextual type, so every parameter is owned whatever the synthesized
    /// `FunctionType` used to describe it would say.
    ByValueClosure,
}
