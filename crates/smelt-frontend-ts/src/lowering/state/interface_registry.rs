//! Interface-shape state for one module being lowered.
//!
//! [`InterfaceRegistry`] owns every map keyed by an interface: the item visible
//! under a source name, the heritage clauses, the call and construct signatures,
//! the string index-signature value type, plus the two name sets that decide
//! whether an `implements` clause names a *local* interface (one this module
//! declares, whether or not it has been lowered yet) or an opaque imported /
//! ambient one.
//!
//! # The invariant this struct owns
//!
//! A locally lowered interface must be registered under its source name *and*
//! marked as locally lowered in the same step, because `implements` resolution
//! and `validate_implements` both key off the "lowered locally" set: an
//! interface registered without that mark would be silently skipped by
//! validation, and a mark without the sidecars (heritage, signatures, index
//! value) would make field lookup fall through to an erased shape.
//! [`InterfaceRegistry::register_lowered`] takes all of it at once, so no caller
//! can record half an interface.

use std::collections::{HashMap, HashSet};

use smelt_hir::{FunctionType, ItemId, Symbol, TypeId};

use crate::lowering::InterfaceHeritageRef;

/// Everything one lowered interface contributes to the registry.
///
/// Grouped into one value so [`InterfaceRegistry::register_lowered`] can take
/// the whole set of facts at once instead of exposing a setter per sidecar.
pub(in crate::lowering) struct LoweredInterface {
    /// Interned (possibly namespace-qualified) interface type name.
    pub(in crate::lowering) name: Symbol,
    /// Source-visible name the interface is looked up under.
    pub(in crate::lowering) name_text: String,
    /// HIR item holding the lowered interface.
    pub(in crate::lowering) item: ItemId,
    /// Resolved heritage clauses (`extends`) in declaration order.
    pub(in crate::lowering) extends: Vec<InterfaceHeritageRef>,
    /// Call signatures making the interface callable.
    pub(in crate::lowering) call_signatures: Vec<FunctionType>,
    /// Construct signatures (`new (): T`) for constructor-slot types.
    pub(in crate::lowering) construct_signatures: Vec<FunctionType>,
    /// Value type of a string index signature, when declared.
    pub(in crate::lowering) index_value_ty: Option<TypeId>,
}

/// Everything the lowering pass knows about the interfaces visible to a module.
///
/// See the module docs for the registration invariant this owns.
#[derive(Debug, Default)]
pub(in crate::lowering) struct InterfaceRegistry {
    /// Interface definitions by source-visible name.
    by_name: HashMap<String, ItemId>,
    /// Interface names declared in the current module, including declarations
    /// that appear after a class which implements them.
    pending_names: HashSet<String>,
    /// Local interface symbols whose declarations have finished lowering.
    lowered_local: HashSet<Symbol>,
    /// Heritage clauses, kept for resolving fields after cyclic type imports
    /// settle.
    extends: HashMap<Symbol, Vec<InterfaceHeritageRef>>,
    /// Value types declared by interface string index signatures.
    index_values: HashMap<Symbol, TypeId>,
    /// Call signatures for callable interface types.
    call_signatures: HashMap<Symbol, Vec<FunctionType>>,
    /// Construct signatures (`new (): T`) for constructor-slot types.
    ///
    /// A constructor interface such as `interface MapCacheConstructor { new ():
    /// MapCache }` is, at runtime, an ordinary callable value: `new value()`
    /// invokes it to produce the constructed type. Each construct signature is
    /// stored as the equivalent [`FunctionType`] so a reference to the interface
    /// can lower to a typed constructor slot instead of an erased dictionary.
    construct_signatures: HashMap<Symbol, Vec<FunctionType>>,
}

impl InterfaceRegistry {
    /// Build the registry a fresh module starts from.
    ///
    /// `visible` are the interface items already present in the shared crate;
    /// the sidecar maps are the ones published by earlier modules. The name sets
    /// start empty: they describe the module about to be lowered.
    pub(in crate::lowering) fn new(
        visible: HashMap<String, ItemId>,
        extends: HashMap<Symbol, Vec<InterfaceHeritageRef>>,
        index_values: HashMap<Symbol, TypeId>,
        call_signatures: HashMap<Symbol, Vec<FunctionType>>,
        construct_signatures: HashMap<Symbol, Vec<FunctionType>>,
    ) -> Self {
        Self {
            by_name: visible,
            pending_names: HashSet::new(),
            lowered_local: HashSet::new(),
            extends,
            index_values,
            call_signatures,
            construct_signatures,
        }
    }

    /// Record the interface names the current module declares.
    pub(in crate::lowering) fn declare_module_scope(&mut self, declared: HashSet<String>) {
        self.pending_names = declared;
    }

    /// Register a freshly lowered local interface with all of its sidecars.
    ///
    /// This is the only way to mark an interface as locally lowered, which is
    /// what keeps `implements` resolution, `validate_implements` and structural
    /// field lookup in agreement (see the module docs). Empty signature lists
    /// and an absent index value are simply not recorded, matching the previous
    /// per-map conditionals.
    pub(in crate::lowering) fn register_lowered(&mut self, lowered: LoweredInterface) {
        let LoweredInterface {
            name,
            name_text,
            item,
            extends,
            call_signatures,
            construct_signatures,
            index_value_ty,
        } = lowered;
        self.extends.insert(name, extends);
        self.call_signatures.insert(name, call_signatures);
        if !construct_signatures.is_empty() {
            self.construct_signatures.insert(name, construct_signatures);
        }
        if let Some(index_value_ty) = index_value_ty {
            self.index_values.insert(name, index_value_ty);
        }
        self.by_name.insert(name_text, item);
        self.lowered_local.insert(name);
    }

    /// Return whether an interface is registered under `name`.
    pub(in crate::lowering) fn contains(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    /// Return the HIR item registered for the interface named `name`.
    pub(in crate::lowering) fn item(&self, name: &str) -> Option<ItemId> {
        self.by_name.get(name).copied()
    }

    /// Return whether `item` is registered under any interface name.
    pub(in crate::lowering) fn has_item(&self, item: ItemId) -> bool {
        self.by_name.values().any(|registered| *registered == item)
    }

    /// Iterate the registered `(name, item)` pairs.
    pub(in crate::lowering) fn entries(&self) -> impl Iterator<Item = (&String, ItemId)> {
        self.by_name.iter().map(|(name, item)| (name, *item))
    }

    /// Register an import or export alias for an existing interface item.
    pub(in crate::lowering) fn register_alias(&mut self, name: String, item: ItemId) {
        self.by_name.insert(name, item);
    }

    /// Return whether an `implements` entry names an interface of this module.
    ///
    /// True when the source name is declared in the current module (possibly
    /// later in the file) or the qualified symbol has already been lowered
    /// locally. A qualified external reference, or an imported / ambient
    /// interface with no local structural definition, is opaque to the local
    /// interface validator and answers false.
    pub(in crate::lowering) fn resolves_locally(&self, local_name: &str, parent: Symbol) -> bool {
        self.pending_names.contains(local_name) || self.lowered_local.contains(&parent)
    }

    /// Return whether an interface symbol was lowered by the current module.
    pub(in crate::lowering) fn is_lowered_locally(&self, name: Symbol) -> bool {
        self.lowered_local.contains(&name)
    }

    /// Return the heritage clauses recorded for an interface symbol.
    pub(in crate::lowering) fn extends(&self, name: Symbol) -> Option<&Vec<InterfaceHeritageRef>> {
        self.extends.get(&name)
    }

    /// Return the index-signature value type of an interface symbol.
    pub(in crate::lowering) fn index_value(&self, name: Symbol) -> Option<TypeId> {
        self.index_values.get(&name).copied()
    }

    /// Return the call signatures of an interface symbol.
    pub(in crate::lowering) fn call_signatures(&self, name: Symbol) -> Option<&Vec<FunctionType>> {
        self.call_signatures.get(&name)
    }

    /// Return the construct signatures of an interface symbol.
    pub(in crate::lowering) fn construct_signatures(
        &self,
        name: Symbol,
    ) -> Option<&Vec<FunctionType>> {
        self.construct_signatures.get(&name)
    }
}

#[cfg(test)]
mod tests {
    use super::{InterfaceRegistry, LoweredInterface};
    use smelt_hir::{ItemId, Symbol};
    use std::collections::{HashMap, HashSet};

    /// Build an empty registry with no cross-module state.
    fn registry() -> InterfaceRegistry {
        InterfaceRegistry::new(
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
        )
    }

    /// Registering a lowered interface makes it visible by name *and* marks it
    /// locally lowered, so `implements` validation cannot silently skip it.
    #[test]
    fn registering_a_lowered_interface_marks_it_local() {
        let mut interfaces = registry();
        interfaces.register_lowered(LoweredInterface {
            name: Symbol(3),
            name_text: "Shape".to_owned(),
            item: ItemId(9),
            extends: Vec::new(),
            call_signatures: Vec::new(),
            construct_signatures: Vec::new(),
            index_value_ty: None,
        });

        assert_eq!(interfaces.item("Shape"), Some(ItemId(9)));
        assert!(interfaces.is_lowered_locally(Symbol(3)));
        assert!(interfaces.resolves_locally("Shape", Symbol(3)));
        assert!(interfaces.extends(Symbol(3)).is_some());
    }

    /// A forward-declared interface resolves locally before it is lowered, and
    /// an imported name with no local declaration does not.
    #[test]
    fn pending_names_resolve_locally_but_imports_do_not() {
        let mut interfaces = registry();
        interfaces.declare_module_scope(HashSet::from(["Later".to_owned()]));

        assert!(interfaces.resolves_locally("Later", Symbol(1)));
        assert!(!interfaces.is_lowered_locally(Symbol(1)));

        interfaces.register_alias("Imported".to_owned(), ItemId(4));
        assert!(interfaces.contains("Imported"));
        assert!(!interfaces.resolves_locally("Imported", Symbol(2)));
    }
}
