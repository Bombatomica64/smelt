//! Grouped mutable state of the emitter.
//!
//! [`MintedItems`] is everything a body render writes into the shared context,
//! so the generic trial's rollback ([`super::ContextCheckpoint`]) is one clone
//! and one [`MintedItems::restore`], and a field added here cannot escape it.
//! Once the last body is written, the crate builder asks the context for the
//! text of every minted item at once ([`EmitContext::hoisted_items_text`]);
//! which tables exist and how each entry is spelled stays here.
//! [`AnalysisCaches`] and [`ConversionGuards`] are a function emitter's lazily
//! computed analyses and its coercion recursion stacks.

use super::{EmitContext, local_analysis};
use crate::runtime_demand::Need;
use crate::{RECORD_ADAPTER_HELPER_PREFIX, RECORD_CONVERSION_HELPER_PREFIX};
use smelt_hir::TypeId;
use smelt_mir::Mir;
use std::cell::RefCell;
use std::collections::HashMap;

/// The body-render outputs of an [`super::EmitContext`].
#[derive(Clone, Default)]
pub(crate) struct MintedItems {
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
    pub(crate) function_item_typed_accessors:
        ::std::cell::RefCell<::std::collections::BTreeMap<(usize, String), (usize, String)>>,
    /// One generated conversion function per class type recovered from an
    /// erased value: `__smelt_from_unknown_record_<ordinal>(SmeltUnknown) -> T`.
    ///
    /// The conversion rebuilds the class from the erased object's record,
    /// field by field and with each field's own conversion nested inside, so
    /// spelled inline it is tens of kilobytes for a large class: repeated at
    /// every site that recovers the class, Hono's CORS test module would reach
    /// 28 MB, past what one rustc can compile in a capped build. Only outermost
    /// conversions are hoisted (see `extract_rendered`).
    ///
    /// Keyed by the module-level Rust spelling of the type; the value is
    /// `(ordinal, body)`.
    ///
    /// Helper names start with [`crate::RECORD_CONVERSION_HELPER_PREFIX`].
    pub(crate) record_conversion_helpers:
        ::std::cell::RefCell<::std::collections::BTreeMap<String, (usize, String)>>,
    /// Hoisted record-to-record adapters for a pair that reaches ITSELF: a
    /// field whose own adaptation needs the same `(source, target)` pair
    /// (`swap(): Pair<T>` inside `Pair<T>`, `Pair<SmeltUnknown>` to
    /// `Pair<f64>`). Inlining it again would never end, so the cycle calls a
    /// module-level helper instead (see `hoisted_record_adapter_call`).
    ///
    /// Keyed by the module-level Rust spellings `(source, target)`; the value
    /// is `(ordinal, body)`. Helper names start with
    /// [`crate::RECORD_ADAPTER_HELPER_PREFIX`].
    pub(crate) record_adapter_helpers:
        ::std::cell::RefCell<::std::collections::BTreeMap<(String, String), (usize, String)>>,
    /// Whether some `console.log` argument took the `util.inspect` route, so
    /// the crate root must carry the `SmeltInspect` runtime (see
    /// [`console_inspect`]). Set while bodies are emitted, read after.
    pub(crate) console_inspect_used: ::std::cell::Cell<bool>,
    /// Erasures `rust_type` decided in the backend (erased generics and
    /// unions, unresolved classes, `never`); see `emitter::backend_erasure`.
    pub(crate) backend_erasures: RefCell<Vec<smelt_hir::ErasureRecord>>,
}

/// What [`MintedItems`] held at one point, cheaply: the keys of each table,
/// the length of each log, the value of each flag. See
/// [`MintedItems::rollback_to`].
pub(super) struct MintedMark {
    /// Keys of `function_item_accessors`.
    function_item_accessors: Vec<usize>,
    /// Keys of `function_item_erased_fn_accessors`.
    function_item_erased_fn_accessors: Vec<usize>,
    /// Keys of `function_item_typed_accessors`.
    function_item_typed_accessors: Vec<(usize, String)>,
    /// Keys of `record_conversion_helpers`.
    record_conversion_helpers: Vec<String>,
    /// Keys of `record_adapter_helpers`.
    record_adapter_helpers: Vec<(String, String)>,
    /// `console_inspect_used`.
    console_inspect_used: bool,
    /// Length of `backend_erasures`.
    backend_erasures: usize,
}

/// Removes every entry of `table` whose key `kept` does not hold.
fn retain_keys<K: Ord, V>(table: &RefCell<std::collections::BTreeMap<K, V>>, kept: &[K]) {
    table
        .borrow_mut()
        .retain(|key, _| kept.binary_search(key).is_ok());
}

impl MintedItems {
    /// Marks what is minted now, for [`Self::rollback_to`].
    ///
    /// Destructures `self` with no `..`, so a new field fails to compile until
    /// it is marked here.
    pub(super) fn mark(&self) -> MintedMark {
        let Self {
            function_item_accessors,
            function_item_erased_fn_accessors,
            function_item_typed_accessors,
            record_conversion_helpers,
            record_adapter_helpers,
            console_inspect_used,
            backend_erasures,
        } = self;
        MintedMark {
            function_item_accessors: function_item_accessors.borrow().keys().copied().collect(),
            function_item_erased_fn_accessors: function_item_erased_fn_accessors
                .borrow()
                .keys()
                .copied()
                .collect(),
            function_item_typed_accessors: function_item_typed_accessors
                .borrow()
                .keys()
                .cloned()
                .collect(),
            record_conversion_helpers: record_conversion_helpers.borrow().keys().cloned().collect(),
            record_adapter_helpers: record_adapter_helpers.borrow().keys().cloned().collect(),
            console_inspect_used: console_inspect_used.get(),
            backend_erasures: backend_erasures.borrow().len(),
        }
    }

    /// Forgets everything minted since `mark`: entries added to a table, log
    /// records appended, flags raised.
    ///
    /// Entries present at the mark keep their current value. Items are only
    /// ever added while a body renders (a helper's body is stored into the
    /// entry its reservation added), so this is exactly the state at the mark
    /// for every entry a render created after it.
    pub(super) fn rollback_to(&self, mark: &MintedMark) {
        let Self {
            function_item_accessors,
            function_item_erased_fn_accessors,
            function_item_typed_accessors,
            record_conversion_helpers,
            record_adapter_helpers,
            console_inspect_used,
            backend_erasures,
        } = self;
        retain_keys(function_item_accessors, &mark.function_item_accessors);
        retain_keys(
            function_item_erased_fn_accessors,
            &mark.function_item_erased_fn_accessors,
        );
        retain_keys(
            function_item_typed_accessors,
            &mark.function_item_typed_accessors,
        );
        retain_keys(record_conversion_helpers, &mark.record_conversion_helpers);
        retain_keys(record_adapter_helpers, &mark.record_adapter_helpers);
        console_inspect_used.set(mark.console_inspect_used);
        backend_erasures
            .borrow_mut()
            .truncate(mark.backend_erasures);
    }

    /// Put back the state in `saved`.
    ///
    /// Destructures `saved` with no `..`, so a new field fails to compile until
    /// it is restored here.
    pub(super) fn restore(&self, saved: Self) {
        let Self {
            function_item_accessors,
            function_item_erased_fn_accessors,
            function_item_typed_accessors,
            record_conversion_helpers,
            record_adapter_helpers,
            console_inspect_used,
            backend_erasures,
        } = saved;
        self.function_item_accessors
            .replace(function_item_accessors.into_inner());
        self.function_item_erased_fn_accessors
            .replace(function_item_erased_fn_accessors.into_inner());
        self.function_item_typed_accessors
            .replace(function_item_typed_accessors.into_inner());
        self.record_conversion_helpers
            .replace(record_conversion_helpers.into_inner());
        self.record_adapter_helpers
            .replace(record_adapter_helpers.into_inner());
        self.console_inspect_used
            .set(console_inspect_used.into_inner());
        self.backend_erasures.replace(backend_erasures.into_inner());
    }
}

/// Analyses of one body, computed on first ask.
#[derive(Default)]
pub(super) struct AnalysisCaches {
    /// Cached termination queries for this function CFG.
    pub(super) termination_cache: RefCell<HashMap<smelt_mir::BlockId, bool>>,
    /// Per-local write facts for this function, built in one pass on first use
    /// (see `local_analysis::LocalWriteIndex`), so per-local mutability queries
    /// do not each rescan every block.
    pub(super) local_write_index: std::cell::OnceCell<local_analysis::LocalWriteIndex>,
    /// Immediate post-dominator of every block of this function CFG, computed
    /// on first use (see `post_dominance`).
    pub(super) post_dominators: RefCell<Option<Vec<Option<smelt_mir::BlockId>>>>,
    /// The same over the paths that do not `throw` (see `post_dominance`).
    pub(super) post_dominators_past_throws: RefCell<Option<Vec<Option<smelt_mir::BlockId>>>>,
    /// Cached loop-exit shape queries keyed by block, continue target, and break target.
    pub(super) loop_exit_cache:
        RefCell<HashMap<(smelt_mir::BlockId, smelt_mir::BlockId, smelt_mir::BlockId), bool>>,
}

/// The conversions in progress in one emitter.
#[derive(Default)]
pub(super) struct ConversionGuards {
    /// Record types currently being wrapped or extracted through erased objects.
    ///
    /// Callback-bearing records can contain their own option type again, so
    /// this bounds structural expansion of cyclic TypeScript object shapes.
    pub(super) record_conversion_stack: RefCell<Vec<TypeId>>,
    /// Source/destination type pairs whose *structural* coercion is currently
    /// being expanded.
    ///
    /// Two coercions rebuild a value out of its parts and can therefore ask for
    /// a nested coercion of a component type: the field-wise record adapter
    /// ([`FunctionEmitter::structural_record_adapter`]) and the
    /// callable-object `__smelt_call` read
    /// ([`FunctionEmitter::callable_object_call_slot_expr`], whose slot coercion
    /// goes on to adapt the call signature's return value). TypeScript callable
    /// interfaces routinely make those two mutually recursive — es-toolkit's
    /// `CurriedFunction1<T1, R>` has a call signature returning
    /// `CurriedFunction1<T1, R>`, so adapting the record adapts a function whose
    /// return value is the record again. Recording the pair being expanded lets
    /// the inner request decline (fall back to the caller's remaining rules)
    /// instead of recursing until the stack is gone.
    pub(super) type_expansion_stack: RefCell<Vec<(TypeId, TypeId)>>,
}

impl EmitContext {
    /// The text of every crate-level item the rendered bodies minted:
    /// function-item identity tokens and value accessors (erased, concrete
    /// erased-function and typed), then the hoisted record conversion and
    /// adapter helpers, each table in its deterministic key order.
    pub(crate) fn hoisted_items_text(&self, mir: &Mir) -> String {
        let mut out = String::new();
        // Flush the per-function-item value accessors collected while erasing
        // function-item-as-value wrappers to `SmeltUnknown`. Each accessor lazily
        // builds and caches ONE erased `SmeltUnknown::Function` so that every
        // reference to the same named function value shares one inner `Rc`, keeping
        // JavaScript reference identity (`===`) stable across references. Sorted by
        // item key via the `BTreeMap` for deterministic golden output.
        // One JavaScript function has several representations: its erased
        // value and a typed `Rc` per callable type it is used at. Each accessor
        // links its allocation to the item's one identity token, so a value
        // erased from ANY of them compares equal (`fs.indexOf(f)` with `fs`
        // holding the erased value and `f` read at its typed accessor answered
        // -1). The token lives in the erased runtime, so it is only minted when
        // that runtime is emitted; without it nothing compares across forms.
        let links_identity = self.demand.has(Need::Unknown);
        let mut identity_keys = std::collections::BTreeSet::new();
        if links_identity {
            identity_keys.extend(self.minted.function_item_accessors.borrow().keys().copied());
            identity_keys.extend(
                self.minted
                    .function_item_typed_accessors
                    .borrow()
                    .keys()
                    .map(|(key, _)| *key),
            );
        }
        // The item's `Function.prototype.name`, carried on its value wrappers.
        let item_names: HashMap<usize, &str> = mir
            .closures
            .iter()
            .filter_map(|closure| {
                let name = mir
                    .names
                    .get(closure.name?)
                    .or_else(|| mir.symbols.get(closure.name?))?;
                Some((closure.function_item_key?, name))
            })
            .collect();
        for key in &identity_keys {
            // A `function` declaration is a JavaScript constructor and owns a
            // `prototype`; an arrow or method item does not (see
            // `smelt_hir::Crate::constructible_functions`).
            let fresh = if mir.constructible_function_items.contains(key) {
                "SmeltFunctionIdentity::constructor()"
            } else {
                "SmeltFunctionIdentity::default()"
            };
            let make = match item_names.get(key) {
                Some(name) => format!("{fresh}.named({name:?})"),
                None => fresh.to_owned(),
            };
            out.push_str(&format!(
                "\nfn __smelt_fn_identity_{key}() -> SmeltFunctionIdentity {{\n    thread_local! {{ static SMELT_FN_IDENTITY: SmeltFunctionIdentity = {make}; }}\n    SMELT_FN_IDENTITY.with(Clone::clone)\n}}\n"
            ));
        }
        for (key, body) in self.minted.function_item_accessors.borrow().iter() {
            let linked_body = if links_identity {
                format!(
                    "{{ let smelt_value = {body}; if let SmeltUnknown::Function(smelt_function) = &smelt_value {{ smelt_join_function_identity(smelt_function, &__smelt_fn_identity_{key}()); }} smelt_value }}"
                )
            } else {
                body.clone()
            };
            out.push_str(&format!(
                "\nfn __smelt_fn_value_{key}() -> SmeltUnknown {{\n    thread_local! {{ static SMELT_FN_VALUE: ::std::cell::OnceCell<SmeltUnknown> = ::std::cell::OnceCell::new(); }}\n    SMELT_FN_VALUE.with(|smelt_cell| smelt_cell.get_or_init(|| {linked_body}).clone())\n}}\n"
            ));
        }

        // Flush the per-function-item accessors for the CONCRETE `SmeltErasedFunction`
        // type. Each caches ONE `SmeltErasedFunction` so repeated calls of a nullary
        // function-item constant (`doNothing()`/`constant()`) return clones sharing
        // one inner callback `Rc`, keeping JavaScript function-singleton identity.
        // A non-empty collector means a closure was lowered to `SmeltErasedFunction`,
        // so `needs_erased_function` is necessarily true and the struct is emitted.
        for (key, body) in self
            .minted
            .function_item_erased_fn_accessors
            .borrow()
            .iter()
        {
            out.push_str(&format!(
                "\nfn __smelt_fn_erased_{key}() -> SmeltErasedFunction {{\n    thread_local! {{ static SMELT_FN_ERASED: ::std::cell::OnceCell<SmeltErasedFunction> = ::std::cell::OnceCell::new(); }}\n    SMELT_FN_ERASED.with(|smelt_cell| smelt_cell.get_or_init(|| {body}).clone())\n}}\n"
            ));
        }

        // Flush the per-function-item accessors for TYPED callable values (see
        // `EmitContext::function_item_typed_accessors`): one cached `Rc` per item
        // and rendered callable type, so every typed reference to a named function
        // shares one identity. The wrapper is bound at its declared type first so
        // the unsizing coercion to `Rc<dyn Fn..>` happens at a `let`; the type is
        // spelled once, as an alias, rather than restated in each of the three
        // positions that name it.
        for ((key, rendered_type), (ordinal, body)) in
            self.minted.function_item_typed_accessors.borrow().iter()
        {
            let link = if links_identity {
                format!(
                    " smelt_join_function_identity(&smelt_function, &__smelt_fn_identity_{key}());"
                )
            } else {
                String::new()
            };
            out.push_str(&format!(
                "\ntype SmeltFnTyped{key}_{ordinal} = {rendered_type};\nfn __smelt_fn_typed_{key}_{ordinal}() -> SmeltFnTyped{key}_{ordinal} {{\n    thread_local! {{ static SMELT_FN_TYPED: ::std::cell::OnceCell<SmeltFnTyped{key}_{ordinal}> = ::std::cell::OnceCell::new(); }}\n    SMELT_FN_TYPED.with(|smelt_cell| smelt_cell.get_or_init(|| {{ let smelt_function: SmeltFnTyped{key}_{ordinal} = {body};{link} smelt_function }}).clone())\n}}\n"
            ));
        }

        // Flush the hoisted erased-record-to-class conversions (see
        // `EmitContext::record_conversion_helpers`): each recovered class type's
        // conversion is spelled once here and called from every site.
        for (rendered_type, (ordinal, body)) in
            self.minted.record_conversion_helpers.borrow().iter()
        {
            out.push_str(&format!(
                "\nfn {RECORD_CONVERSION_HELPER_PREFIX}{ordinal}(smelt_value: SmeltUnknown) -> {rendered_type} {{\n    {body}\n}}\n"
            ));
        }
        // The self-reaching record-to-record adapters, likewise spelled once.
        for ((source_text, target_text), (ordinal, body)) in
            self.minted.record_adapter_helpers.borrow().iter()
        {
            out.push_str(&format!(
                "\nfn {RECORD_ADAPTER_HELPER_PREFIX}{ordinal}(smelt_value: {source_text}) -> {target_text} {{\n    {body}\n}}\n"
            ));
        }
        out
    }
}
