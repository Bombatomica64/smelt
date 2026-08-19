//! Class-shape state for one module being lowered.
//!
//! [`ClassRegistry`] owns every map that answers "what does the lowering pass
//! know about a class named `Foo`?": its HIR item, its fields, its method
//! signatures, its base class, its index-signature value type, the lexically
//! scoped type symbol its source name resolves to, and the two name sets that
//! let a `new Foo()` resolve before `Foo` itself has been lowered.
//!
//! # The invariant this struct owns
//!
//! A module top-level `function Foo() { this.a = 1 }` used as `new Foo()`,
//! `x instanceof Foo`, or `Foo.prototype.m = …` is a JavaScript *constructor
//! function*: it lowers to a synthesized class, not to a callable function
//! item. Three facts must agree for that to work, and before this struct they
//! were three loose fields any of 944 methods could set independently:
//!
//! 1. the name is recorded as a constructor function, so the declaration is
//!    synthesized into a class instead of lowered as a plain function;
//! 2. the name is *also* a pending class name, so a `new Foo()` lowered before
//!    the synthesis still resolves nominally rather than erasing;
//! 3. the name is *not* predeclared as a callable function item, which would
//!    shadow the class binding.
//!
//! [`ClassRegistry::declare_module_scope`] is the only way to record (1), and it
//! establishes (2) in the same statement, so no caller can do half of it. Fact
//! (3) reads the same private set through
//! [`ClassRegistry::is_constructor_function`] — the set is unreachable any other
//! way, so the predeclaration skip and the synthesis dispatch cannot disagree
//! about which names are constructor functions.

use std::collections::{HashMap, HashSet};

use smelt_hir::{Field, ItemId, MethodSig, Symbol, TypeId};

/// Base-class metadata recorded for a class name: the base symbol and the type
/// arguments applied to it.
type BaseClass = (Symbol, Vec<TypeId>);

/// Everything the lowering pass knows about the classes visible to one module.
///
/// See the module docs for the constructor-function invariant this owns.
#[derive(Debug, Default)]
pub(in crate::lowering) struct ClassRegistry {
    /// Class definitions by source-visible name.
    by_name: HashMap<String, ItemId>,
    /// Fields for each class, keyed by emitted class name.
    fields: HashMap<String, Vec<Field>>,
    /// Method signatures for classes that are visible before their item is
    /// fully emitted.
    methods: HashMap<String, Vec<MethodSig>>,
    /// Base-class metadata for classes lowered so far.
    bases: HashMap<String, BaseClass>,
    /// Value types declared by class string index signatures (`[k: string]: T`),
    /// keyed by the class name symbol.
    ///
    /// Member and computed access fall back to it when no declared named field
    /// or method matches.
    index_values: HashMap<Symbol, TypeId>,
    /// Lexically scoped class type symbols keyed by their source binding name.
    ///
    /// Test suites are flattened into one Rust module, so sibling suites may
    /// legally declare different block-local classes with the same source name.
    /// This map preserves source lookup while giving each declaration a
    /// distinct emitted type identity.
    scoped_type_names: HashMap<String, Symbol>,
    /// Class names declared later in the current module, including the
    /// constructor-function names below.
    pending_names: HashSet<String>,
    /// Module top-level `function Foo(){}` declarations this module uses as
    /// JavaScript constructor functions.
    constructor_functions: HashSet<String>,
}

impl ClassRegistry {
    /// Build the registry a fresh module starts from.
    ///
    /// `visible` are the class items already present in the shared crate
    /// (cross-module references and export aliases) and `index_values` are the
    /// class index-signature value types published by earlier modules. The name
    /// sets start empty: they describe the module about to be lowered and are
    /// filled by [`Self::declare_module_scope`].
    pub(in crate::lowering) fn new(
        visible: HashMap<String, ItemId>,
        index_values: HashMap<Symbol, TypeId>,
    ) -> Self {
        Self {
            by_name: visible,
            index_values,
            ..Self::default()
        }
    }

    /// Record the module-scope class names discovered by the prepass.
    ///
    /// `declared` are the module's `class Foo {}` declarations and
    /// `constructor_functions` are its `function Foo(){}` declarations used as
    /// JavaScript constructor functions. Both become pending class names in one
    /// step: a constructor-function name is always nominally resolvable, so a
    /// `new Foo()` lowered before the synthesis cannot erase. This is the only
    /// entry point for either set, which is what makes that pairing an
    /// invariant rather than a convention (see the module docs).
    pub(in crate::lowering) fn declare_module_scope(
        &mut self,
        declared: HashSet<String>,
        constructor_functions: HashSet<String>,
    ) {
        self.pending_names = declared;
        self.pending_names
            .extend(constructor_functions.iter().cloned());
        self.constructor_functions = constructor_functions;
    }

    /// Return whether `name` is a module top-level constructor function.
    ///
    /// Both the synthesis dispatch and the function-item predeclaration skip
    /// read the set through this one method, so they cannot disagree.
    pub(in crate::lowering) fn is_constructor_function(&self, name: &str) -> bool {
        self.constructor_functions.contains(name)
    }

    /// Return whether `name` is a class declared later in the current module.
    pub(in crate::lowering) fn is_pending(&self, name: &str) -> bool {
        self.pending_names.contains(name)
    }

    /// Return whether a class is registered under `name`.
    pub(in crate::lowering) fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// Return the HIR item registered for the class named `name`.
    pub(in crate::lowering) fn item(&self, name: &str) -> Option<ItemId> {
        self.by_name.get(name).copied()
    }

    /// Return whether `item` is registered under any class name.
    pub(in crate::lowering) fn has_item(&self, item: ItemId) -> bool {
        self.by_name.values().any(|registered| *registered == item)
    }

    /// Iterate the registered `(name, item)` pairs.
    pub(in crate::lowering) fn entries(&self) -> impl Iterator<Item = (&String, ItemId)> {
        self.by_name.iter().map(|(name, item)| (name, *item))
    }

    /// Register (or re-alias) the class item reachable under `name`.
    pub(in crate::lowering) fn register(&mut self, name: String, item: ItemId) {
        self.by_name.insert(name, item);
    }

    /// Return the recorded fields of the class named `name`.
    pub(in crate::lowering) fn fields(&self, name: &str) -> Option<&Vec<Field>> {
        self.fields.get(name)
    }

    /// Replace the recorded fields of the class named `name`.
    pub(in crate::lowering) fn set_fields(&mut self, name: String, fields: Vec<Field>) {
        self.fields.insert(name, fields);
    }

    /// Append fields to the class named `name`, creating the entry if needed.
    pub(in crate::lowering) fn extend_fields(
        &mut self,
        name: String,
        extra: impl IntoIterator<Item = Field>,
    ) {
        self.fields.entry(name).or_default().extend(extra);
    }

    /// Return the recorded method signatures of the class named `name`.
    pub(in crate::lowering) fn methods(&self, name: &str) -> Option<&Vec<MethodSig>> {
        self.methods.get(name)
    }

    /// Replace the recorded method signatures of the class named `name`.
    pub(in crate::lowering) fn set_methods(&mut self, name: String, methods: Vec<MethodSig>) {
        self.methods.insert(name, methods);
    }

    /// Return the base class recorded for the class named `name`.
    pub(in crate::lowering) fn base(&self, name: &str) -> Option<&BaseClass> {
        self.bases.get(name)
    }

    /// Record the base class of the class named `name`.
    pub(in crate::lowering) fn set_base(&mut self, name: String, base: Symbol, args: Vec<TypeId>) {
        self.bases.insert(name, (base, args));
    }

    /// Return the index-signature value type of the class symbol `name`.
    pub(in crate::lowering) fn index_value(&self, name: Symbol) -> Option<TypeId> {
        self.index_values.get(&name).copied()
    }

    /// Return whether the class symbol `name` declares an index signature.
    pub(in crate::lowering) fn has_index_value(&self, name: Symbol) -> bool {
        self.index_values.contains_key(&name)
    }

    /// Record the index-signature value type of the class symbol `name`.
    pub(in crate::lowering) fn set_index_value(&mut self, name: Symbol, value_ty: TypeId) {
        self.index_values.insert(name, value_ty);
    }

    /// Return the lexically scoped type symbol bound to the source name `name`.
    pub(in crate::lowering) fn scoped_type_name(&self, name: &str) -> Option<Symbol> {
        self.scoped_type_names.get(name).copied()
    }

    /// Bind a lexically scoped type symbol to the source name `name`.
    pub(in crate::lowering) fn bind_scoped_type_name(&mut self, name: String, symbol: Symbol) {
        self.scoped_type_names.insert(name, symbol);
    }

    /// Capture the whole class scope so a nested lexical scope can be undone.
    ///
    /// Used when entering a test `describe` callback: sibling suites may declare
    /// same-named block-local classes, so the enclosing lookup is restored
    /// wholesale on the way out via [`Self::restore_scope`].
    pub(in crate::lowering) fn snapshot_scope(&self) -> ClassScopeSnapshot {
        ClassScopeSnapshot {
            by_name: self.by_name.clone(),
            fields: self.fields.clone(),
            methods: self.methods.clone(),
            bases: self.bases.clone(),
            scoped_type_names: self.scoped_type_names.clone(),
            index_values: self.index_values.clone(),
        }
    }

    /// Restore a class scope captured by [`Self::snapshot_scope`].
    ///
    /// The pending and constructor-function name sets are module-wide, so they
    /// are deliberately not part of the snapshot.
    pub(in crate::lowering) fn restore_scope(&mut self, scope: ClassScopeSnapshot) {
        self.by_name = scope.by_name;
        self.fields = scope.fields;
        self.methods = scope.methods;
        self.bases = scope.bases;
        self.scoped_type_names = scope.scoped_type_names;
        self.index_values = scope.index_values;
    }

    /// Capture just the registered class *names*, for per-test-case scoping.
    ///
    /// Constructor functions declared inside a test case are synthesized into
    /// the registry during that case's predeclaration and setup replay, but they
    /// are block-local: a differently shaped `function Foo` in a sibling `it`
    /// must synthesize its own class rather than reuse a stale one.
    pub(in crate::lowering) fn snapshot_names(&self) -> ClassNameSnapshot {
        ClassNameSnapshot {
            by_name: self.by_name.keys().cloned().collect(),
            fields: self.fields.keys().cloned().collect(),
            methods: self.methods.keys().cloned().collect(),
        }
    }

    /// Drop registry entries added since [`Self::snapshot_names`], preserving
    /// module-level and imported classes.
    pub(in crate::lowering) fn retain_names(&mut self, scope: &ClassNameSnapshot) {
        self.by_name.retain(|name, _| scope.by_name.contains(name));
        self.fields.retain(|name, _| scope.fields.contains(name));
        self.methods.retain(|name, _| scope.methods.contains(name));
    }
}

/// Whole-scope class-registry capture taken when entering a test suite body.
#[derive(Debug)]
pub(in crate::lowering) struct ClassScopeSnapshot {
    /// Class items visible before the suite body.
    by_name: HashMap<String, ItemId>,
    /// Class fields visible before the suite body.
    fields: HashMap<String, Vec<Field>>,
    /// Class method signatures visible before the suite body.
    methods: HashMap<String, Vec<MethodSig>>,
    /// Base classes visible before the suite body.
    bases: HashMap<String, BaseClass>,
    /// Lexically scoped class type symbols visible before the suite body.
    scoped_type_names: HashMap<String, Symbol>,
    /// Class index-signature value types visible before the suite body.
    index_values: HashMap<Symbol, TypeId>,
}

/// Registered class names captured before lowering one test case.
#[derive(Debug)]
pub(in crate::lowering) struct ClassNameSnapshot {
    /// Class names registered before the case.
    by_name: HashSet<String>,
    /// Class names with recorded fields before the case.
    fields: HashSet<String>,
    /// Class names with recorded method signatures before the case.
    methods: HashSet<String>,
}

#[cfg(test)]
mod tests {
    use super::ClassRegistry;
    use std::collections::HashSet;

    /// Build a name set from string literals.
    fn names(values: &[&str]) -> HashSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    /// A constructor-function name is pending and reported as a constructor
    /// function in one step, so `new Foo()` resolves nominally and the function
    /// item is skipped. Recording one without the other is unrepresentable.
    #[test]
    fn constructor_functions_are_always_pending_class_names() {
        let mut registry = ClassRegistry::default();
        registry.declare_module_scope(names(&["Declared"]), names(&["Ctor"]));

        assert!(registry.is_pending("Declared"));
        assert!(registry.is_pending("Ctor"));
        assert!(registry.is_constructor_function("Ctor"));
        assert!(!registry.is_constructor_function("Declared"));
    }

    /// Re-declaring module scope replaces both sets together, so a stale
    /// constructor-function name cannot survive as a pending class name.
    #[test]
    fn redeclaring_module_scope_replaces_both_name_sets() {
        let mut registry = ClassRegistry::default();
        registry.declare_module_scope(HashSet::new(), names(&["Ctor"]));
        registry.declare_module_scope(names(&["Other"]), HashSet::new());

        assert!(!registry.is_pending("Ctor"));
        assert!(!registry.is_constructor_function("Ctor"));
        assert!(registry.is_pending("Other"));
    }

    /// A suite-scope snapshot restores class lookups without disturbing the
    /// module-wide pending / constructor-function sets.
    #[test]
    fn scope_snapshot_leaves_module_name_sets_alone() {
        let mut registry = ClassRegistry::default();
        registry.declare_module_scope(names(&["Declared"]), names(&["Ctor"]));
        let scope = registry.snapshot_scope();
        registry.bind_scoped_type_name("Declared".to_owned(), smelt_hir::Symbol(7));
        registry.restore_scope(scope);

        assert!(registry.scoped_type_name("Declared").is_none());
        assert!(registry.is_pending("Ctor"));
        assert!(registry.is_constructor_function("Ctor"));
    }
}
