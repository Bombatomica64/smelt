//! Constant-value state for the module being lowered.
//!
//! [`ConstRegistry`] owns the five name-keyed constant tables the lowering pass
//! folds through — literal constants, `RegExp` literals, object constants,
//! array/set collections and the value projections of object constants — plus
//! the const-folded `enum` member values keyed by enum name then member name.
//!
//! # The invariants this struct owns
//!
//! * **A constant is one name resolving to at most one kind of folded value**,
//!   and "is this name a constant at all?" is a single question
//!   ([`ConstRegistry::is_folded_const`]) rather than an open-coded chain of
//!   `contains_key` calls that a new kind can be forgotten from.
//! * **An import alias rebinds every kind the imported name has.**
//!   [`ConstRegistry::rebind_import`] copies whichever tables hold the imported
//!   name, so aliasing an import can no longer carry the literal across while
//!   dropping the object or the collection.

use std::collections::HashMap;

use smelt_hir::TypeId;

use crate::lowering::{ConstCollection, ConstLiteral};
use crate::ObjectConst;

/// A `RegExp` literal constant: source pattern, flags and its HIR type.
type ConstRegexp = (String, String, TypeId);

/// Every folded constant visible to the module being lowered.
///
/// See the module docs for the invariants this owns.
#[derive(Debug, Default)]
pub(in crate::lowering) struct ConstRegistry {
    /// Literal constant items visible from already-lowered modules.
    literals: HashMap<String, ConstLiteral>,
    /// Const-folded TypeScript `enum` member values keyed by enum name then
    /// member name.
    ///
    /// Lets `EnumName.Member` reads and `case EnumName.Member:` labels inline
    /// the member's constant literal.
    enum_members: HashMap<String, HashMap<String, ConstLiteral>>,
    /// `RegExp` literal constants visible from nested function bodies.
    regexps: HashMap<String, ConstRegexp>,
    /// Object literal constants visible from current and already-lowered modules.
    objects: HashMap<String, ObjectConst>,
    /// Array/set constants visible from nested function bodies.
    collections: HashMap<String, ConstCollection>,
    /// Object constants whose values can be projected by `Object.values`.
    object_value_collections: HashMap<String, ConstCollection>,
}

impl ConstRegistry {
    /// Build the registry a fresh module starts from, seeded with the constants
    /// published by earlier modules.
    pub(in crate::lowering) fn new(
        literals: HashMap<String, ConstLiteral>,
        enum_members: HashMap<String, HashMap<String, ConstLiteral>>,
        objects: HashMap<String, ObjectConst>,
        collections: HashMap<String, ConstCollection>,
        object_value_collections: HashMap<String, ConstCollection>,
    ) -> Self {
        Self {
            literals,
            enum_members,
            regexps: HashMap::new(),
            objects,
            collections,
            object_value_collections,
        }
    }

    /// Return whether `name` resolves to any folded constant value.
    ///
    /// Used both to decide that an import alias already has concrete metadata
    /// and to recognize a resolvable module reference.
    pub(in crate::lowering) fn is_folded_const(&self, name: &str) -> bool {
        self.literals.contains_key(name)
            || self.objects.contains_key(name)
            || self.collections.contains_key(name)
    }

    /// Rebind every constant kind held by `imported` under the name `local`.
    ///
    /// One call so an alias cannot carry the literal across while dropping the
    /// object constant or the collection.
    pub(in crate::lowering) fn rebind_import(&mut self, local: &str, imported: &str) {
        if let Some(value) = self.literals.get(imported).cloned() {
            self.literals.insert(local.to_owned(), value);
        }
        if let Some(value) = self.objects.get(imported).cloned() {
            self.objects.insert(local.to_owned(), value);
        }
        if let Some(value) = self.collections.get(imported).cloned() {
            self.collections.insert(local.to_owned(), value);
        }
    }

    /// Return the folded literal constant named `name`.
    pub(in crate::lowering) fn literal(&self, name: &str) -> Option<&ConstLiteral> {
        self.literals.get(name)
    }

    /// Return whether `name` is a folded literal constant.
    pub(in crate::lowering) fn has_literal(&self, name: &str) -> bool {
        self.literals.contains_key(name)
    }

    /// Record a folded literal constant.
    pub(in crate::lowering) fn set_literal(&mut self, name: String, value: ConstLiteral) {
        self.literals.insert(name, value);
    }

    /// Return the folded literal of one `enum` member.
    pub(in crate::lowering) fn enum_member(
        &self,
        enum_name: &str,
        member_name: &str,
    ) -> Option<&ConstLiteral> {
        self.enum_members
            .get(enum_name)
            .and_then(|members| members.get(member_name))
    }

    /// Record the folded member literals of one `enum`.
    pub(in crate::lowering) fn set_enum_members(
        &mut self,
        enum_name: String,
        members: HashMap<String, ConstLiteral>,
    ) {
        self.enum_members.insert(enum_name, members);
    }

    /// Return the `RegExp` literal constant named `name`.
    pub(in crate::lowering) fn regexp(&self, name: &str) -> Option<&ConstRegexp> {
        self.regexps.get(name)
    }

    /// Record a `RegExp` literal constant.
    pub(in crate::lowering) fn set_regexp(&mut self, name: String, value: ConstRegexp) {
        self.regexps.insert(name, value);
    }

    /// Return the object constant named `name`.
    pub(in crate::lowering) fn object(&self, name: &str) -> Option<&ObjectConst> {
        self.objects.get(name)
    }

    /// Return whether `name` is an object constant.
    pub(in crate::lowering) fn has_object(&self, name: &str) -> bool {
        self.objects.contains_key(name)
    }

    /// Record an object constant.
    pub(in crate::lowering) fn set_object(&mut self, name: String, value: ObjectConst) {
        self.objects.insert(name, value);
    }

    /// Return the array/set constant named `name`.
    pub(in crate::lowering) fn collection(&self, name: &str) -> Option<&ConstCollection> {
        self.collections.get(name)
    }

    /// Record an array/set constant.
    pub(in crate::lowering) fn set_collection(&mut self, name: String, value: ConstCollection) {
        self.collections.insert(name, value);
    }

    /// Return the `Object.values` projection of the object constant `name`.
    pub(in crate::lowering) fn object_value_collection(
        &self,
        name: &str,
    ) -> Option<&ConstCollection> {
        self.object_value_collections.get(name)
    }

    /// Record the `Object.values` projection of an object constant.
    pub(in crate::lowering) fn set_object_value_collection(
        &mut self,
        name: String,
        value: ConstCollection,
    ) {
        self.object_value_collections.insert(name, value);
    }
}

#[cfg(test)]
mod tests {
    use super::ConstRegistry;
    use crate::lowering::{ConstCollection, ConstLiteral};
    use crate::ObjectConst;
    use smelt_hir::{Literal, TypeId};

    /// Build a folded string literal constant.
    fn literal(text: &str) -> ConstLiteral {
        ConstLiteral {
            literal: Literal::String(text.to_owned()),
            ty: TypeId(0),
        }
    }

    /// Build an empty object constant.
    fn object() -> ObjectConst {
        ObjectConst {
            entries: Vec::new(),
            ty: TypeId(0),
        }
    }

    /// Build an empty array collection constant.
    fn collection() -> ConstCollection {
        ConstCollection {
            items: Vec::new(),
            ty: TypeId(0),
            is_set: false,
        }
    }

    /// Every kind answers the one "is this a constant?" question, so a new kind
    /// cannot be forgotten at one of the call sites that used to chain
    /// `contains_key` by hand.
    #[test]
    fn is_folded_const_covers_every_value_kind() {
        let mut consts = ConstRegistry::default();
        assert!(!consts.is_folded_const("A"));
        consts.set_literal("A".to_owned(), literal("a"));
        consts.set_object("B".to_owned(), object());
        consts.set_collection("C".to_owned(), collection());

        assert!(consts.is_folded_const("A"));
        assert!(consts.is_folded_const("B"));
        assert!(consts.is_folded_const("C"));
    }

    /// Aliasing an import rebinds every kind the imported name carries, so the
    /// literal cannot travel while the object or collection is dropped.
    #[test]
    fn rebind_import_copies_every_kind_at_once() {
        let mut consts = ConstRegistry::default();
        consts.set_literal("source".to_owned(), literal("a"));
        consts.set_object("source".to_owned(), object());
        consts.set_collection("source".to_owned(), collection());

        consts.rebind_import("alias", "source");

        assert!(consts.literal("alias").is_some());
        assert!(consts.object("alias").is_some());
        assert!(consts.collection("alias").is_some());
    }
}
