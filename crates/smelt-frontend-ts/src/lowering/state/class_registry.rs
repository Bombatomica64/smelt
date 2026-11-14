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
//! Every class name has ONE entry ([`ClassName`]) holding both what the module
//! declares it as and the item it currently resolves to, and its state is read
//! as one [`ClassState`]. A constructor function is a *kind* of module
//! declaration, so being one and being pending cannot come apart:
//! [`ClassRegistry::declare_module_scope`] is the only way to record either.
//! Fact (3) reads the same entry through
//! [`ClassRegistry::is_constructor_function`], so the predeclaration skip and
//! the synthesis dispatch cannot disagree about which names are constructor
//! functions.
//!
//! The declaration half of an entry is module-wide; only the item half follows
//! the lexical class scopes of test suites and test cases, so rolling a scope
//! back can never forget that the module declares a name.

use crate::context::Published;
use std::collections::{HashMap, HashSet};

use smelt_hir::{Field, ItemId, MethodSig, Symbol, TypeId};

/// Base-class metadata recorded for a class name: the base symbol and the type
/// arguments applied to it.
type BaseClass = (Symbol, Vec<TypeId>);

/// How the module being lowered declares a class name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClassDeclaration {
    /// A `class Foo {}` declaration (or `const Foo = class { … }`).
    Class,
    /// A top-level `function Foo(){}` used as a JavaScript constructor function.
    ConstructorFunction,
}

/// What the module knows about one class name: how it declares it (module
/// wide) and the class item the name resolves to in the current scope.
#[derive(Debug, Clone, Copy, Default)]
struct ClassName {
    /// How the current module declares the name, if it does.
    declared: Option<ClassDeclaration>,
    /// The class item visible under the name, once lowered or imported.
    item: Option<ItemId>,
}

impl ClassName {
    /// Whether the entry carries no fact at all and can be dropped.
    const fn is_empty(self) -> bool {
        self.declared.is_none() && self.item.is_none()
    }
}

/// The state of a class name, as one answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum ClassState {
    /// Declared by this module as a class and not lowered yet.
    Declared,
    /// A module constructor function not synthesized into a class yet.
    CtorFunction,
    /// Resolves to a class item (lowered here, or visible from elsewhere).
    Lowered(ItemId),
}

/// Everything the lowering pass knows about the classes visible to one module.
///
/// See the module docs for the constructor-function invariant this owns.
#[derive(Debug, Default)]
pub(in crate::lowering) struct ClassRegistry {
    /// Every class name the module knows: its declaration and its item.
    names: HashMap<String, ClassName>,
    /// Names that stand for other class names while one arm of a conditional
    /// class is lowered, innermost last; see `conditional_base`.
    aliases: Vec<(String, String)>,
    /// The static fields of the classes being lowered, innermost last: a method
    /// reads its own class's statics before the class has an item.
    statics_in_flight: Vec<(String, Vec<smelt_hir::StaticField>)>,
    /// The class scopes local declarations shadowed, innermost last.
    shadows: Vec<ClassScopeSnapshot>,
    /// How many function and closure bodies enclose the code being lowered.
    body_depth: usize,
    /// Interfaces declaring a numeric index signature.
    array_like: HashSet<Symbol>,
    /// Fields for each class, keyed by emitted class name.
    fields: HashMap<String, Vec<Field>>,
    /// Method signatures for classes that are visible before their item is
    /// fully emitted.
    methods: HashMap<String, Vec<MethodSig>>,
    /// Base-class metadata for classes lowered so far.
    bases: HashMap<String, BaseClass>,
    /// Base-class metadata keyed by the SYMBOL of the class the current module
    /// declares.
    ///
    /// The by-name map above cannot answer a base-chain question safely: a class
    /// renamed for a cross-module collision (`Store_1`) keeps its SOURCE
    /// spelling as its recorded name, so asking the by-name map about another
    /// module's `Store_1` hands back THIS module's `Store` — and for
    /// `import { Store as StoreBase }; class Store extends StoreBase` that is the
    /// class itself, which walks its own base chain forever. Keyed by symbol the
    /// map answers only for the classes the module being lowered declared, which
    /// is exactly the set whose bases it recorded.
    bases_by_symbol: HashMap<Symbol, BaseClass>,
    /// Value types declared by class string index signatures (`[k: string]: T`),
    /// keyed by the class name symbol.
    ///
    /// Member and computed access fall back to it when no declared named field
    /// or method matches.
    index_values: Published<HashMap<Symbol, TypeId>>,
    /// Lexically scoped class type symbols keyed by their source binding name.
    ///
    /// Test suites are flattened into one Rust module, so sibling suites may
    /// legally declare different block-local classes with the same source name.
    /// This map preserves source lookup while giving each declaration a
    /// distinct emitted type identity.
    scoped_type_names: HashMap<String, Symbol>,
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
        index_values: Published<HashMap<Symbol, TypeId>>,
    ) -> Self {
        Self {
            names: visible
                .into_iter()
                .map(|(name, item)| {
                    let entry = ClassName {
                        declared: None,
                        item: Some(item),
                    };
                    (name, entry)
                })
                .collect(),
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
        #[expect(clippy::iter_over_hash_type, reason = "resets a field on every entry")]
        for entry in self.names.values_mut() {
            entry.declared = None;
        }
        for (names, kind) in [
            (declared, ClassDeclaration::Class),
            (constructor_functions, ClassDeclaration::ConstructorFunction),
        ] {
            #[expect(clippy::iter_over_hash_type, reason = "keyed inserts of one value")]
            for name in names {
                self.names.entry(name).or_default().declared = Some(kind);
            }
        }
        self.names.retain(|_, entry| !entry.is_empty());
    }

    /// The state of the class name `name`, if the module knows it at all.
    pub(in crate::lowering) fn state(&self, name: &str) -> Option<ClassState> {
        let entry = self.names.get(self.alias_target(name))?;
        match (entry.item, entry.declared) {
            (Some(item), _) => Some(ClassState::Lowered(item)),
            (None, Some(ClassDeclaration::Class)) => Some(ClassState::Declared),
            (None, Some(ClassDeclaration::ConstructorFunction)) => Some(ClassState::CtorFunction),
            (None, None) => None,
        }
    }

    /// Whether the module being lowered declares a class called `name` itself,
    /// lexically scoped ones included.
    pub(in crate::lowering) fn declared_in_module(&self, name: &str) -> bool {
        self.scoped_type_names.contains_key(name)
            || self
                .names
                .get(name)
                .is_some_and(|entry| entry.declared.is_some())
    }

    /// Whether `name` names a class nominally: registered, or declared by
    /// this module and still to be lowered.
    pub(in crate::lowering) fn is_known(&self, name: &str) -> bool {
        self.state(name).is_some()
    }

    /// Return whether `name` is a module top-level constructor function.
    ///
    /// Both the synthesis dispatch and the function-item predeclaration skip
    /// read the set through this one method, so they cannot disagree.
    pub(in crate::lowering) fn is_constructor_function(&self, name: &str) -> bool {
        self.names
            .get(name)
            .is_some_and(|entry| entry.declared == Some(ClassDeclaration::ConstructorFunction))
    }

    /// Return whether `name` is a class the current module declares (lowered
    /// yet or not).
    pub(in crate::lowering) fn is_pending(&self, name: &str) -> bool {
        self.names
            .get(name)
            .is_some_and(|entry| entry.declared.is_some())
    }

    /// Publish the static fields of class `name` for its own method bodies.
    pub(in crate::lowering) fn push_statics_in_flight(
        &mut self,
        name: String,
        fields: Vec<smelt_hir::StaticField>,
    ) {
        self.statics_in_flight.push((name, fields));
    }

    /// How many classes are publishing their statics.
    pub(in crate::lowering) fn statics_in_flight_depth(&self) -> usize {
        self.statics_in_flight.len()
    }

    /// End every [`Self::push_statics_in_flight`] past `depth`.
    pub(in crate::lowering) fn truncate_statics_in_flight(&mut self, depth: usize) {
        self.statics_in_flight.truncate(depth);
    }

    /// The static fields of `name`, when it is a class still being lowered.
    pub(in crate::lowering) fn statics_in_flight(
        &self,
        name: &str,
    ) -> Option<&[smelt_hir::StaticField]> {
        let name = self.alias_target(name);
        self.statics_in_flight
            .iter()
            .rev()
            .find(|(class, _)| class == name)
            .map(|(_, fields)| fields.as_slice())
    }

    /// Return whether a class is registered under `name`.
    pub(in crate::lowering) fn contains(&self, name: &str) -> bool {
        self.item(name).is_some()
    }

    /// Return the HIR item registered for the class named `name`.
    pub(in crate::lowering) fn item(&self, name: &str) -> Option<ItemId> {
        self.names
            .get(self.alias_target(name))
            .and_then(|entry| entry.item)
    }

    /// Make `from` stand for the class `to` until [`Self::pop_alias`].
    pub(in crate::lowering) fn push_alias(&mut self, from: String, to: String) {
        self.aliases.push((from, to));
    }

    /// End the innermost [`Self::push_alias`].
    pub(in crate::lowering) fn pop_alias(&mut self) {
        self.aliases.pop();
    }

    /// Whether `name` currently stands for another class.
    pub(in crate::lowering) fn is_aliased(&self, name: &str) -> bool {
        self.aliases.iter().any(|(from, _)| from == name)
    }

    /// The class name `name` stands for: itself unless an alias is in force.
    pub(in crate::lowering) fn alias_target<'a>(&'a self, name: &'a str) -> &'a str {
        self.aliases
            .iter()
            .rev()
            .find(|(from, _)| from == name)
            .map_or(name, |(_, to)| to.as_str())
    }

    /// Return whether `item` is registered under any class name.
    pub(in crate::lowering) fn has_item(&self, item: ItemId) -> bool {
        self.names.values().any(|entry| entry.item == Some(item))
    }

    /// Iterate the registered `(name, item)` pairs.
    pub(in crate::lowering) fn entries(&self) -> impl Iterator<Item = (&String, ItemId)> {
        self.names
            .iter()
            .filter_map(|(name, entry)| entry.item.map(|item| (name, item)))
    }

    /// Register (or re-alias) the class item reachable under `name`.
    pub(in crate::lowering) fn register(&mut self, name: String, item: ItemId) {
        self.names.entry(name).or_default().item = Some(item);
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
    ///
    /// `symbol` is the declaring class's own type symbol, so the same base is
    /// reachable by symbol through [`Self::base_of_symbol`] without going
    /// through the ambiguous source spelling.
    pub(in crate::lowering) fn set_base(
        &mut self,
        name: String,
        symbol: Symbol,
        base: Symbol,
        args: Vec<TypeId>,
    ) {
        self.bases.insert(name, (base, args.clone()));
        self.bases_by_symbol.insert(symbol, (base, args));
    }

    /// Return the base class recorded for the class SYMBOL `symbol`.
    ///
    /// Answers `None` for every symbol the current module did not declare, which
    /// is what keeps an imported class's base chain out of this module's
    /// in-progress registry.
    pub(in crate::lowering) fn base_of_symbol(&self, symbol: Symbol) -> Option<&BaseClass> {
        self.bases_by_symbol.get(&symbol)
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
        self.index_values.write().insert(name, value_ty);
    }

    /// Return the lexically scoped type symbol bound to the source name `name`.
    pub(in crate::lowering) fn scoped_type_name(&self, name: &str) -> Option<Symbol> {
        self.scoped_type_names.get(self.alias_target(name)).copied()
    }

    /// Bind a lexically scoped type symbol to the source name `name`.
    pub(in crate::lowering) fn bind_scoped_type_name(&mut self, name: String, symbol: Symbol) {
        self.scoped_type_names.insert(name, symbol);
    }

    /// The registered items by name.
    fn items_by_name(&self) -> HashMap<String, ItemId> {
        self.entries()
            .map(|(name, item)| (name.clone(), item))
            .collect()
    }

    /// Record that the interface `name` declares a numeric index signature.
    pub(in crate::lowering) fn mark_array_like(&mut self, name: Symbol) {
        self.array_like.insert(name);
    }

    /// Whether `name` is an interface with a numeric index signature.
    pub(in crate::lowering) fn is_array_like(&self, name: Symbol) -> bool {
        self.array_like.contains(&name)
    }

    /// Whether lowering is inside a function or closure body.
    pub(in crate::lowering) const fn in_body(&self) -> bool {
        self.body_depth > 0
    }

    /// Note entering (`true`) or leaving (`false`) a function or closure body.
    pub(in crate::lowering) const fn note_body(&mut self, entering: bool) {
        if entering {
            self.body_depth += 1;
        } else {
            self.body_depth -= 1;
        }
    }

    /// Open a shadow of `name` for the current lexical scope: the outer class of
    /// that name is out of sight.
    ///
    /// [`Self::close_shadows`] undoes it when that scope ends.
    pub(in crate::lowering) fn open_shadow(&mut self, name: &str) {
        let scope = self.snapshot_scope();
        self.shadows.push(scope);
        if let Some(entry) = self.names.get_mut(name) {
            entry.item = None;
        }
    }

    /// How many shadows are open, the mark [`Self::close_shadows`] returns to.
    pub(in crate::lowering) fn shadow_depth(&self) -> usize {
        self.shadows.len()
    }

    /// Undo every shadow opened since `depth`.
    pub(in crate::lowering) fn close_shadows(&mut self, depth: usize) {
        while self.shadows.len() > depth {
            if let Some(scope) = self.shadows.pop() {
                self.restore_scope(scope);
            }
        }
    }

    /// Capture the whole class scope so a nested lexical scope can be undone.
    ///
    /// Used when entering a test `describe` callback: sibling suites may declare
    /// same-named block-local classes, so the enclosing lookup is restored
    /// wholesale on the way out via [`Self::restore_scope`].
    pub(in crate::lowering) fn snapshot_scope(&self) -> ClassScopeSnapshot {
        ClassScopeSnapshot {
            items: self.items_by_name(),
            fields: self.fields.clone(),
            methods: self.methods.clone(),
            bases: self.bases.clone(),
            bases_by_symbol: self.bases_by_symbol.clone(),
            scoped_type_names: self.scoped_type_names.clone(),
            index_values: self.index_values.clone(),
        }
    }

    /// Restore a class scope captured by [`Self::snapshot_scope`].
    ///
    /// Only the items are restored: how the module declares a name is
    /// module-wide, so it is deliberately not part of the snapshot.
    pub(in crate::lowering) fn restore_scope(&mut self, scope: ClassScopeSnapshot) {
        let mut items = scope.items;
        #[expect(
            clippy::iter_over_hash_type,
            reason = "keyed writes, each entry independent"
        )]
        for (name, entry) in &mut self.names {
            entry.item = items.remove(name);
        }
        #[expect(clippy::iter_over_hash_type, reason = "keyed inserts of distinct keys")]
        for (name, item) in items {
            self.names.entry(name).or_default().item = Some(item);
        }
        self.names.retain(|_, entry| !entry.is_empty());
        self.fields = scope.fields;
        self.methods = scope.methods;
        self.bases = scope.bases;
        self.bases_by_symbol = scope.bases_by_symbol;
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
            by_name: self.entries().map(|(name, _)| name.clone()).collect(),
            fields: self.fields.keys().cloned().collect(),
            methods: self.methods.keys().cloned().collect(),
        }
    }

    /// Drop registry entries added since [`Self::snapshot_names`], preserving
    /// module-level and imported classes.
    pub(in crate::lowering) fn retain_names(&mut self, scope: &ClassNameSnapshot) {
        #[expect(
            clippy::iter_over_hash_type,
            reason = "keyed writes, each entry independent"
        )]
        for (name, entry) in &mut self.names {
            if !scope.by_name.contains(name) {
                entry.item = None;
            }
        }
        self.names.retain(|_, entry| !entry.is_empty());
        self.fields.retain(|name, _| scope.fields.contains(name));
        self.methods.retain(|name, _| scope.methods.contains(name));
    }
}

/// Whole-scope class-registry capture taken when entering a test suite body.
#[derive(Debug)]
pub(in crate::lowering) struct ClassScopeSnapshot {
    /// Class items visible before the suite body.
    items: HashMap<String, ItemId>,
    /// Class fields visible before the suite body.
    fields: HashMap<String, Vec<Field>>,
    /// Class method signatures visible before the suite body.
    methods: HashMap<String, Vec<MethodSig>>,
    /// Base classes visible before the suite body.
    bases: HashMap<String, BaseClass>,
    /// Symbol-keyed base classes visible before the suite body.
    bases_by_symbol: HashMap<Symbol, BaseClass>,
    /// Lexically scoped class type symbols visible before the suite body.
    scoped_type_names: HashMap<String, Symbol>,
    /// Class index-signature value types visible before the suite body.
    index_values: Published<HashMap<Symbol, TypeId>>,
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
    use super::{ClassRegistry, ClassState};
    use smelt_hir::ItemId;
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

    /// A declared class name lowered inside a test case keeps its declaration
    /// when the case's scope is rolled back: it reverts to `Declared` instead
    /// of being forgotten, and a name the case introduced disappears.
    #[test]
    fn rolling_back_names_keeps_module_declarations() {
        let mut registry = ClassRegistry::default();
        registry.declare_module_scope(names(&["Declared"]), names(&["Ctor"]));
        assert_eq!(registry.state("Declared"), Some(ClassState::Declared));
        assert_eq!(registry.state("Ctor"), Some(ClassState::CtorFunction));
        let before = registry.snapshot_names();
        registry.register("Declared".to_owned(), ItemId(1));
        registry.register("Ctor".to_owned(), ItemId(2));
        registry.register("Local".to_owned(), ItemId(3));
        assert_eq!(registry.state("Ctor"), Some(ClassState::Lowered(ItemId(2))));
        assert!(registry.is_constructor_function("Ctor"));

        registry.retain_names(&before);

        assert_eq!(registry.state("Declared"), Some(ClassState::Declared));
        assert_eq!(registry.state("Ctor"), Some(ClassState::CtorFunction));
        assert_eq!(registry.state("Local"), None);
        assert!(!registry.is_known("Local"));
    }

    /// Restoring a suite scope restores items only: an item registered inside
    /// the suite is dropped, one visible before it comes back.
    #[test]
    fn restoring_a_scope_restores_items_only() {
        let mut registry = ClassRegistry::new(
            std::collections::HashMap::from([("Imported".to_owned(), ItemId(5))]),
            crate::context::Published::default(),
        );
        registry.declare_module_scope(names(&["Declared"]), HashSet::new());
        let scope = registry.snapshot_scope();
        registry.register("Declared".to_owned(), ItemId(6));
        registry.register("Imported".to_owned(), ItemId(7));
        registry.restore_scope(scope);

        assert_eq!(registry.state("Declared"), Some(ClassState::Declared));
        assert_eq!(registry.item("Imported"), Some(ItemId(5)));
    }
}
