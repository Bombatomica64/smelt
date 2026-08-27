//! Per-body local state for the module being lowered.
//!
//! [`LocalScope`] owns everything keyed by a source local *name* or a
//! [`smelt_hir::LocalId`] and therefore valid only inside the body currently
//! being lowered: the name-to-local bindings, the two per-local fact sets
//! (`Date` identity and explicit `any`), the collected static-member writes onto
//! callable locals, the non-escaping local callback values, the hoisted local
//! function items, and the stack of flow narrowings.
//!
//! # The invariants this struct owns
//!
//! * **Local ids are body-scoped.** `LocalId`s are indices into one
//!   [`smelt_hir::Body`], so every entry keyed by one is only meaningful while
//!   that body is being lowered. Entering a nested body takes the relevant
//!   groups out ([`LocalScope::take_bindings`] and friends) and restores them on
//!   the way out, which is why each group hands back an opaque frame value
//!   rather than a bare collection: a frame can only be given back to the group
//!   it came from.
//! * **Last write wins for callable-local props.** A repeated
//!   `fn.prop = value` write must replace the earlier value while the surviving
//!   properties keep source order, and no property may be recorded onto a local
//!   that already escaped. [`LocalScope::record_callable_prop`] is the only way
//!   to add one, so the ordering rule cannot be reimplemented differently by a
//!   second caller.
//! * **Narrowings are a stack.** A fact applies until its scope is popped;
//!   lookups run innermost-first. The stack is only reachable through the
//!   named push/pop/lookup operations here.

use std::collections::{HashMap, HashSet};

use smelt_hir::{ExprId, ItemId, LocalId, Span, Symbol, TypeId};

use crate::lowering::LocalCallback;

/// Collected static-member property writes onto a function-typed local.
///
/// Populated by `ModuleBuilder::try_collect_callable_local_prop` as it claims
/// straight-line `fn.prop = value` writes, and consumed by the
/// callable-interface coercion that synthesizes a
/// [`smelt_hir::ExprKind::CallableObjectAssign`]. `props` keeps the writes in
/// source order with last-write-wins de-duplication; `escaped` records whether
/// the local has already been read for a purpose other than that consumption,
/// which makes a later property write a documented (blocked) write-after-escape.
#[derive(Debug, Clone, Default)]
struct CallableLocalProps {
    /// Property writes in source order (last write wins), as `(name, value)`.
    props: Vec<(Symbol, ExprId)>,
    /// Set once the local escapes via a non-consuming, non-self-call read.
    escaped: bool,
    /// Span of the first claimed write, used to point the
    /// collected-but-never-consumed diagnostic at the source the collection ate.
    first_write_span: Option<Span>,
    /// Callable-interface type the local was *declared* at, when its source
    /// annotation named one.
    ///
    /// The declaration is what says which struct the collected writes belong
    /// to, so a value that flows out at a position carrying no type hint (a
    /// `return` from a function with an inferred return type) can still be
    /// constructed instead of dropping its properties.
    declared_interface: Option<TypeId>,
}

/// Source-name to local-id bindings saved while a nested body is lowered.
#[derive(Debug, Clone, Default)]
pub(in crate::lowering) struct LocalBindings(HashMap<String, LocalId>);

impl LocalBindings {
    /// Return the local a saved scope bound to `name`.
    ///
    /// Capture collection needs to read the *enclosing* body's bindings while
    /// the nested body's are installed, which is the one place a saved frame is
    /// inspected rather than just handed back.
    pub(in crate::lowering) fn lookup(&self, name: &str) -> Option<LocalId> {
        self.0.get(name).copied()
    }
}

/// `Date`-identity local facts saved while a nested body is lowered.
#[derive(Debug, Default)]
pub(in crate::lowering) struct DateValueLocals(HashSet<LocalId>);

/// Explicit-`any` local facts saved while a nested body is lowered.
#[derive(Debug, Default)]
pub(in crate::lowering) struct ExplicitAnyLocals(HashSet<LocalId>);

/// Callable-local property writes saved while a nested body is lowered.
#[derive(Debug, Default)]
pub(in crate::lowering) struct CallableLocalPropWrites(HashMap<LocalId, CallableLocalProps>);

/// Flow-narrowing scope stack saved while a nested body is lowered.
#[derive(Debug, Default)]
pub(in crate::lowering) struct Narrowings(Vec<HashMap<String, TypeId>>);

/// Everything the lowering pass knows about the locals of the body in progress.
///
/// See the module docs for the invariants this owns.
#[derive(Debug, Default)]
pub(in crate::lowering) struct LocalScope {
    /// Local variable bindings in the current scope.
    bindings: LocalBindings,
    /// Local values statically known to retain JavaScript `Date` identity.
    date_values: DateValueLocals,
    /// Locals declared with an explicit `any` annotation. Their storage type is
    /// the erased `Unknown` boundary by source spelling, so a later concrete
    /// assignment must not flow-narrow them to that value's type; the source
    /// deliberately opted out of static shape tracking.
    explicit_any: ExplicitAnyLocals,
    /// Static-member property writes collected onto function-typed locals.
    callable_props: CallableLocalPropWrites,
    /// Local closure values available to non-escaping callback consumers.
    callbacks: HashMap<String, LocalCallback>,
    /// Function item slots reserved for local hoisted declarations.
    function_items: HashMap<String, ItemId>,
    /// Active local narrowings from guards and assertion calls.
    narrowings: Narrowings,
}

impl LocalScope {
    /// Bind a source name to a lowered local, returning the binding it replaced
    /// so a caller restoring a lexical scope can put the previous one back.
    pub(in crate::lowering) fn bind(&mut self, name: String, local: LocalId) -> Option<LocalId> {
        self.bindings.0.insert(name, local)
    }

    /// Return the local bound to `name` in the current scope.
    pub(in crate::lowering) fn lookup(&self, name: &str) -> Option<LocalId> {
        self.bindings.0.get(name).copied()
    }

    /// Return whether `name` is bound in the current scope.
    pub(in crate::lowering) fn is_bound(&self, name: &str) -> bool {
        self.bindings.0.contains_key(name)
    }

    /// Drop the binding for `name`, if any.
    pub(in crate::lowering) fn unbind(&mut self, name: &str) {
        self.bindings.0.remove(name);
    }

    /// Copy the current bindings so a temporary scope can be rolled back.
    pub(in crate::lowering) fn snapshot_bindings(&self) -> LocalBindings {
        self.bindings.clone()
    }

    /// Take the bindings, leaving an empty scope for a nested body.
    pub(in crate::lowering) fn take_bindings(&mut self) -> LocalBindings {
        std::mem::take(&mut self.bindings)
    }

    /// Restore bindings captured by [`Self::take_bindings`] or
    /// [`Self::snapshot_bindings`].
    pub(in crate::lowering) fn restore_bindings(&mut self, saved: LocalBindings) {
        self.bindings = saved;
    }

    /// Record that a local holds a value with JavaScript `Date` identity.
    pub(in crate::lowering) fn mark_date_value(&mut self, local: LocalId) {
        self.date_values.0.insert(local);
    }

    /// Return whether a local is known to hold a `Date` value.
    pub(in crate::lowering) fn is_date_value(&self, local: LocalId) -> bool {
        self.date_values.0.contains(&local)
    }

    /// Take the `Date`-identity facts, leaving an empty set for a nested body.
    pub(in crate::lowering) fn take_date_values(&mut self) -> DateValueLocals {
        std::mem::take(&mut self.date_values)
    }

    /// Restore facts captured by [`Self::take_date_values`].
    pub(in crate::lowering) fn restore_date_values(&mut self, saved: DateValueLocals) {
        self.date_values = saved;
    }

    /// Record that a local was declared with an explicit `any` annotation.
    pub(in crate::lowering) fn mark_explicit_any(&mut self, local: LocalId) {
        self.explicit_any.0.insert(local);
    }

    /// Return whether a local was declared with an explicit `any` annotation.
    pub(in crate::lowering) fn is_explicit_any(&self, local: LocalId) -> bool {
        self.explicit_any.0.contains(&local)
    }

    /// Take the explicit-`any` facts, leaving an empty set for a nested body.
    pub(in crate::lowering) fn take_explicit_any(&mut self) -> ExplicitAnyLocals {
        std::mem::take(&mut self.explicit_any)
    }

    /// Restore facts captured by [`Self::take_explicit_any`].
    pub(in crate::lowering) fn restore_explicit_any(&mut self, saved: ExplicitAnyLocals) {
        self.explicit_any = saved;
    }

    /// Record a `fn.prop = value` write onto a callable local.
    ///
    /// Last write wins: a repeated write to the same property replaces the
    /// earlier value while the surviving properties keep source order.
    pub(in crate::lowering) fn record_callable_prop(
        &mut self,
        local: LocalId,
        prop: Symbol,
        value: ExprId,
        span: Span,
    ) {
        let entry = self.callable_props.0.entry(local).or_default();
        entry.props.retain(|(name, _)| *name != prop);
        entry.props.push((prop, value));
        entry.first_write_span.get_or_insert(span);
    }

    /// Report the property writes this body collected and never consumed.
    ///
    /// Collection removes the write from the statement stream on the promise
    /// that a later callable-interface coercion will rebuild it as a struct
    /// field. When that coercion never fires — the value flows out at a bare
    /// function type, or through an intersection the interface lookup does not
    /// recognize — the promise is broken and the write is simply gone. Returning
    /// the leftovers lets the body-lowering exit turn that into a diagnostic
    /// instead of a silent drop.
    ///
    /// Each entry is `(span of the first claimed write, property names still
    /// pending)`, ordered by span so the diagnostics are deterministic.
    pub(in crate::lowering) fn unconsumed_callable_props(&self) -> Vec<(Span, Vec<Symbol>)> {
        let mut pending: Vec<(Span, Vec<Symbol>)> = self
            .callable_props
            .0
            .values()
            .filter(|state| !state.props.is_empty())
            .filter_map(|state| {
                state
                    .first_write_span
                    .map(|span| (span, state.props.iter().map(|(name, _)| *name).collect()))
            })
            .collect();
        pending.sort_by_key(|(span, _)| (span.start, span.end));
        pending
    }

    /// Record the callable-interface type a local was declared at.
    ///
    /// Called when a local's source annotation names a callable interface, so a
    /// later consumption with no contextual type hint knows the struct to build.
    pub(in crate::lowering) fn record_callable_local_interface(
        &mut self,
        local: LocalId,
        interface_ty: TypeId,
    ) {
        self.callable_props.0.entry(local).or_default().declared_interface = Some(interface_ty);
    }

    /// Return the callable-interface type a local was declared at, if any.
    pub(in crate::lowering) fn callable_local_declared_interface(
        &self,
        local: LocalId,
    ) -> Option<TypeId> {
        self.callable_props
            .0
            .get(&local)
            .and_then(|state| state.declared_interface)
    }

    /// Return whether a callable local has already escaped its collection.
    pub(in crate::lowering) fn callable_local_escaped(&self, local: LocalId) -> bool {
        self.callable_props
            .0
            .get(&local)
            .is_some_and(|state| state.escaped)
    }

    /// Record that a callable local escaped through a non-consuming read.
    pub(in crate::lowering) fn mark_callable_local_escaped(&mut self, local: LocalId) {
        if let Some(state) = self.callable_props.0.get_mut(&local) {
            state.escaped = true;
        }
    }

    /// Consume the collected property writes for a callable local.
    ///
    /// Returns `None` when the local collected no writes, which leaves the
    /// ordinary identifier-read path in charge.
    pub(in crate::lowering) fn take_callable_props(
        &mut self,
        local: LocalId,
    ) -> Option<Vec<(Symbol, ExprId)>> {
        self.callable_props
            .0
            .remove(&local)
            .map(|state| state.props)
    }

    /// Take the callable-local writes, leaving none for a nested body.
    pub(in crate::lowering) fn take_callable_prop_writes(&mut self) -> CallableLocalPropWrites {
        std::mem::take(&mut self.callable_props)
    }

    /// Restore writes captured by [`Self::take_callable_prop_writes`].
    pub(in crate::lowering) fn restore_callable_prop_writes(
        &mut self,
        saved: CallableLocalPropWrites,
    ) {
        self.callable_props = saved;
    }

    /// Register a non-escaping local callback value under its source name.
    pub(in crate::lowering) fn register_callback(&mut self, name: String, callback: LocalCallback) {
        self.callbacks.insert(name, callback);
    }

    /// Return the local callback value bound to `name`.
    pub(in crate::lowering) fn callback(&self, name: &str) -> Option<&LocalCallback> {
        self.callbacks.get(name)
    }

    /// Return whether `name` is bound to a local callback value.
    pub(in crate::lowering) fn has_callback(&self, name: &str) -> bool {
        self.callbacks.contains_key(name)
    }

    /// Reserve the function item slot of a hoisted local declaration.
    pub(in crate::lowering) fn register_function_item(&mut self, name: String, item: ItemId) {
        self.function_items.insert(name, item);
    }

    /// Return the function item reserved for the hoisted local named `name`.
    pub(in crate::lowering) fn function_item(&self, name: &str) -> Option<ItemId> {
        self.function_items.get(name).copied()
    }

    /// Return whether a function item slot is reserved for `name`.
    pub(in crate::lowering) fn has_function_item(&self, name: &str) -> bool {
        self.function_items.contains_key(name)
    }

    /// Apply a narrowing fact in the innermost active scope, opening one if the
    /// stack is empty.
    pub(in crate::lowering) fn apply_narrowing(&mut self, name: String, target: TypeId) {
        if let Some(scope) = self.narrowings.0.last_mut() {
            scope.insert(name, target);
        } else {
            self.push_narrowing_fact(name, target);
        }
    }

    /// Push a fresh narrowing scope carrying a single fact, so the caller can
    /// pop exactly the fact it added.
    pub(in crate::lowering) fn push_narrowing_fact(&mut self, name: String, target: TypeId) {
        let mut scope = HashMap::new();
        scope.insert(name, target);
        self.push_narrowing_scope(scope);
    }

    /// Push a whole narrowing scope, as produced by guard analysis.
    pub(in crate::lowering) fn push_narrowing_scope(&mut self, facts: HashMap<String, TypeId>) {
        self.narrowings.0.push(facts);
    }

    /// Pop the innermost narrowing scope.
    pub(in crate::lowering) fn pop_narrowing_scope(&mut self) {
        self.narrowings.0.pop();
    }

    /// Return the active narrowed type for a source local, innermost first.
    pub(in crate::lowering) fn narrowed_type(&self, name: &str) -> Option<TypeId> {
        self.narrowings
            .0
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    /// Take the narrowing stack, leaving an empty one for a nested body.
    ///
    /// Narrowing facts are keyed by *name*, so a fact recorded in one body must
    /// not leak into a sibling body that declares its own same-named binding.
    pub(in crate::lowering) fn take_narrowings(&mut self) -> Narrowings {
        std::mem::take(&mut self.narrowings)
    }

    /// Restore a stack captured by [`Self::take_narrowings`].
    pub(in crate::lowering) fn restore_narrowings(&mut self, saved: Narrowings) {
        self.narrowings = saved;
    }
}

#[cfg(test)]
mod tests {
    use super::LocalScope;
    use smelt_hir::{ExprId, FileId, LocalId, Span, Symbol, TypeId};

    /// A repeated write to the same property replaces the earlier value and the
    /// surviving properties keep source order.
    #[test]
    fn callable_prop_writes_are_last_write_wins_in_source_order() {
        let mut scope = LocalScope::default();
        let local = LocalId(0);
        scope.record_callable_prop(local, Symbol(1), ExprId(10), Span::new(FileId(0), 0, 1));
        scope.record_callable_prop(local, Symbol(2), ExprId(11), Span::new(FileId(0), 2, 3));
        scope.record_callable_prop(local, Symbol(1), ExprId(12), Span::new(FileId(0), 4, 5));

        assert_eq!(
            scope.take_callable_props(local),
            Some(vec![(Symbol(2), ExprId(11)), (Symbol(1), ExprId(12))])
        );
        // Consumption is destructive: a second read finds nothing.
        assert_eq!(scope.take_callable_props(local), None);
    }

    /// Pending writes are reported against the FIRST claimed write's span, and a
    /// consumed local leaves nothing behind to report.
    #[test]
    fn unconsumed_callable_props_report_the_first_write_span() {
        let mut scope = LocalScope::default();
        let local = LocalId(0);
        scope.record_callable_prop(local, Symbol(1), ExprId(10), Span::new(FileId(0), 7, 8));
        scope.record_callable_prop(local, Symbol(2), ExprId(11), Span::new(FileId(0), 9, 10));

        assert_eq!(
            scope.unconsumed_callable_props(),
            vec![(Span::new(FileId(0), 7, 8), vec![Symbol(1), Symbol(2)])]
        );
        drop(scope.take_callable_props(local));
        assert!(scope.unconsumed_callable_props().is_empty());
    }

    /// Narrowing lookups resolve innermost-first and a popped scope stops
    /// applying.
    #[test]
    fn narrowings_resolve_innermost_first() {
        let mut scope = LocalScope::default();
        scope.apply_narrowing("x".to_owned(), TypeId(1));
        scope.push_narrowing_fact("x".to_owned(), TypeId(2));

        assert_eq!(scope.narrowed_type("x"), Some(TypeId(2)));
        scope.pop_narrowing_scope();
        assert_eq!(scope.narrowed_type("x"), Some(TypeId(1)));
    }

    /// A nested body starts with no bindings and no name-keyed narrowings, and
    /// the enclosing ones come back intact.
    #[test]
    fn taking_a_body_frame_isolates_then_restores_it() {
        let mut scope = LocalScope::default();
        scope.bind("outer".to_owned(), LocalId(0));
        scope.apply_narrowing("outer".to_owned(), TypeId(1));

        let bindings = scope.take_bindings();
        let narrowings = scope.take_narrowings();
        assert!(!scope.is_bound("outer"));
        assert_eq!(scope.narrowed_type("outer"), None);

        scope.restore_bindings(bindings);
        scope.restore_narrowings(narrowings);
        assert_eq!(scope.lookup("outer"), Some(LocalId(0)));
        assert_eq!(scope.narrowed_type("outer"), Some(TypeId(1)));
    }
}
