//! Type-level declaration state for the module being lowered.
//!
//! [`TypeScope`] owns the type surface that is not an interface or a class: the
//! fields carried by structural type aliases, the fields attached to callable
//! intersection types, which aliases spell a callable-object surface, the
//! namespace path currently qualifying type-only declarations, and the two
//! parallel stacks of active generic type parameters.
//!
//! # The invariants this struct owns
//!
//! * **Type-parameter scopes are a paired stack.** A generic declaration
//!   contributes both a name-to-type scope and a symbol-to-constraint scope, and
//!   the two must be popped together — [`TypeScope::pop_param_scope`] pops both,
//!   so a caller cannot leave one stack deeper than the other and silently
//!   resolve a stale constraint. The two pushes stay separate because a
//!   constraint may reference the parameters of its own declaration
//!   (`<T, U extends T>`), so the name scope has to be visible while the
//!   constraints are being lowered.
//! * **A constraint-only scope is its own operation.** Overload selection
//!   deliberately pushes constraints with no matching name scope, so that path
//!   uses [`TypeScope::push_constraint_scope`] /
//!   [`TypeScope::pop_constraint_scope`] and is visible as an exception rather
//!   than looking like an unbalanced paired push.
//! * **Namespace qualification is a stack, not a string.** `qualify` is the only
//!   way to read it, so a type declared inside `namespace A.B` cannot be
//!   registered under an unqualified name by accident.

use std::collections::{HashMap, HashSet};

use smelt_hir::{Field, Symbol, TypeId};

/// Type-level lookups and active generic scopes for one module.
///
/// See the module docs for the invariants this owns.
#[derive(Debug, Default)]
pub(in crate::lowering) struct TypeScope {
    /// Fields carried by structural type aliases.
    alias_fields: HashMap<Symbol, Vec<Field>>,
    /// Fields attached to callable intersection types.
    callable_fields: HashMap<TypeId, Vec<Field>>,
    /// Type aliases whose source surface is a callable object intersection.
    callable_object_aliases: HashSet<Symbol>,
    /// Namespace path currently qualifying type-only declarations.
    namespace_prefix: Vec<String>,
    /// Active generic type parameter scopes, innermost last.
    param_scopes: Vec<HashMap<String, TypeId>>,
    /// Constraints for active generic type parameters, innermost last.
    param_constraint_scopes: Vec<HashMap<Symbol, TypeId>>,
}

impl TypeScope {
    /// Build the scope a fresh module starts from.
    ///
    /// `alias_fields`, `callable_fields` and `callable_object_aliases` carry the
    /// type surface published by earlier modules; the stacks start empty.
    pub(in crate::lowering) fn new(
        alias_fields: HashMap<Symbol, Vec<Field>>,
        callable_fields: HashMap<TypeId, Vec<Field>>,
        callable_object_aliases: HashSet<Symbol>,
    ) -> Self {
        Self {
            alias_fields,
            callable_fields,
            callable_object_aliases,
            ..Self::default()
        }
    }

    /// Return the structural fields recorded for the type alias `name`.
    pub(in crate::lowering) fn alias_fields(&self, name: Symbol) -> Option<&Vec<Field>> {
        self.alias_fields.get(&name)
    }

    /// Record the structural fields of the type alias `name`.
    pub(in crate::lowering) fn set_alias_fields(&mut self, name: Symbol, fields: Vec<Field>) {
        self.alias_fields.insert(name, fields);
    }

    /// Return the fields attached to a callable intersection type.
    pub(in crate::lowering) fn callable_fields(&self, ty: TypeId) -> Option<&Vec<Field>> {
        self.callable_fields.get(&ty)
    }

    /// Record the fields attached to a callable intersection type.
    pub(in crate::lowering) fn set_callable_fields(&mut self, ty: TypeId, fields: Vec<Field>) {
        self.callable_fields.insert(ty, fields);
    }

    /// Return whether an alias spells a callable-object intersection surface.
    pub(in crate::lowering) fn is_callable_object_alias(&self, name: Symbol) -> bool {
        self.callable_object_aliases.contains(&name)
    }

    /// Record that an alias spells a callable-object intersection surface.
    pub(in crate::lowering) fn mark_callable_object_alias(&mut self, name: Symbol) {
        self.callable_object_aliases.insert(name);
    }

    /// Enter a TypeScript namespace for type-only declarations.
    pub(in crate::lowering) fn push_namespace(&mut self, name: String) {
        self.namespace_prefix.push(name);
    }

    /// Leave the innermost TypeScript namespace.
    pub(in crate::lowering) fn pop_namespace(&mut self) {
        self.namespace_prefix.pop();
    }

    /// Prefix a local type declaration name with the active namespace path.
    pub(in crate::lowering) fn qualify(&self, name: &str) -> String {
        if self.namespace_prefix.is_empty() {
            return name.to_owned();
        }
        format!("{}.{}", self.namespace_prefix.join("."), name)
    }

    /// Push the name-to-type scope of a generic declaration.
    ///
    /// Pair it with [`Self::push_param_constraints`] and close both with
    /// [`Self::pop_param_scope`].
    pub(in crate::lowering) fn push_param_types(&mut self, scope: HashMap<String, TypeId>) {
        self.param_scopes.push(scope);
    }

    /// Push the constraint scope of the generic declaration whose names were
    /// pushed by [`Self::push_param_types`].
    pub(in crate::lowering) fn push_param_constraints(
        &mut self,
        constraints: HashMap<Symbol, TypeId>,
    ) {
        self.param_constraint_scopes.push(constraints);
    }

    /// Pop a generic declaration's name *and* constraint scopes together.
    pub(in crate::lowering) fn pop_param_scope(&mut self) {
        self.param_scopes.pop();
        self.param_constraint_scopes.pop();
    }

    /// Push a constraint scope with no matching name scope.
    ///
    /// Overload selection tests one candidate signature at a time and only needs
    /// its constraints; this is the documented exception to the paired stack.
    pub(in crate::lowering) fn push_constraint_scope(
        &mut self,
        constraints: HashMap<Symbol, TypeId>,
    ) {
        self.param_constraint_scopes.push(constraints);
    }

    /// Pop a scope pushed by [`Self::push_constraint_scope`].
    pub(in crate::lowering) fn pop_constraint_scope(&mut self) {
        self.param_constraint_scopes.pop();
    }

    /// Resolve a type parameter by source name, innermost scope first.
    pub(in crate::lowering) fn param_type(&self, name: &str) -> Option<TypeId> {
        self.param_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
    }

    /// Resolve an active type parameter's constraint, innermost scope first.
    pub(in crate::lowering) fn param_constraint(&self, name: Symbol) -> Option<TypeId> {
        self.param_constraint_scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(&name).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::TypeScope;
    use smelt_hir::{Symbol, TypeId};
    use std::collections::HashMap;

    /// Popping a generic scope closes the name and constraint stacks together,
    /// so an outer declaration's constraint cannot outlive its parameters.
    #[test]
    fn popping_a_param_scope_closes_both_stacks() {
        let mut scope = TypeScope::default();
        scope.push_param_types(HashMap::from([("T".to_owned(), TypeId(1))]));
        scope.push_param_constraints(HashMap::from([(Symbol(1), TypeId(2))]));

        assert_eq!(scope.param_type("T"), Some(TypeId(1)));
        assert_eq!(scope.param_constraint(Symbol(1)), Some(TypeId(2)));

        scope.pop_param_scope();
        assert_eq!(scope.param_type("T"), None);
        assert_eq!(scope.param_constraint(Symbol(1)), None);
    }

    /// A constraint-only scope is independent of the paired stack.
    #[test]
    fn constraint_only_scope_does_not_touch_param_names() {
        let mut scope = TypeScope::default();
        scope.push_param_types(HashMap::from([("T".to_owned(), TypeId(1))]));
        scope.push_param_constraints(HashMap::new());
        scope.push_constraint_scope(HashMap::from([(Symbol(9), TypeId(3))]));

        assert_eq!(scope.param_constraint(Symbol(9)), Some(TypeId(3)));
        scope.pop_constraint_scope();
        assert_eq!(scope.param_constraint(Symbol(9)), None);
        assert_eq!(scope.param_type("T"), Some(TypeId(1)));
    }

    /// Namespace qualification composes the active path.
    #[test]
    fn qualify_uses_the_active_namespace_path() {
        let mut scope = TypeScope::default();
        assert_eq!(scope.qualify("Shape"), "Shape");
        scope.push_namespace("A".to_owned());
        scope.push_namespace("B".to_owned());
        assert_eq!(scope.qualify("Shape"), "A.B.Shape");
        scope.pop_namespace();
        assert_eq!(scope.qualify("Shape"), "A.Shape");
    }
}
