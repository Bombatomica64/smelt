//! Provenance of the source names a module did not declare itself.
//!
//! [`ImportScope`] owns the name sets that record where a binding came from:
//! value imports, type-only imports, namespace imports (`import * as ns`), the
//! Vitest-compatible test builtins, the `tz` factories from `@date-fns/tz`, and
//! the locals statically known to alias the ambient global object
//! (`const g = globalThis`).
//!
//! # What this struct owns
//!
//! Each set answers one question about a *source name*, and every membership
//! test now runs through a named predicate, so a lowering path cannot start
//! treating one provenance set as another. In particular
//! [`ImportScope::is_imported_binding`] is the single answer to "did this module
//! import this name at all?", replacing an open-coded three-set chain that a new
//! import form could be forgotten from.
//!
//! Note what is deliberately *not* claimed: value and type-only membership are
//! not mutually exclusive. An inline `import { type A, b }` marks `A` type-only
//! at the specifier and the statement still classifies its locals as value
//! imports, so a name can be in both sets. That is the existing behaviour and
//! this struct preserves it rather than tightening it.

use std::collections::HashSet;

/// Source-name provenance for one module being lowered.
#[derive(Debug, Default)]
pub(in crate::lowering) struct ImportScope {
    /// Local names imported as runtime values.
    values: HashSet<String>,
    /// Local names imported only for TypeScript type positions.
    type_only: HashSet<String>,
    /// Local names bound by namespace imports such as
    /// `import * as MathApi from "./math"`.
    namespaces: HashSet<String>,
    /// Test-framework API names imported from Vitest-compatible modules.
    test_builtins: HashSet<String>,
    /// Local names bound to `tz` from the `@date-fns/tz` package.
    date_fns_timezone_factories: HashSet<String>,
    /// Local names statically known to alias the ambient global object.
    ///
    /// Populated for `const g = globalThis;` style bindings so that global-path
    /// normalization and feature-probe erasure recognize `g.Object.keys(x)` and
    /// `"Map" in g` as global references. Used only for preserving known member
    /// types and stdlib dispatch.
    global_object_aliases: HashSet<String>,
}

impl ImportScope {
    /// Record a local name imported as a runtime value.
    pub(in crate::lowering) fn mark_value(&mut self, local: String) {
        self.values.insert(local);
    }

    /// Return whether a local name was imported as a runtime value.
    pub(in crate::lowering) fn is_value(&self, name: &str) -> bool {
        self.values.contains(name)
    }

    /// Record a local name imported only for type positions.
    pub(in crate::lowering) fn mark_type_only(&mut self, local: String) {
        self.type_only.insert(local);
    }

    /// Return whether a local name was imported only for type positions.
    pub(in crate::lowering) fn is_type_only(&self, name: &str) -> bool {
        self.type_only.contains(name)
    }

    /// Record a local name bound by a namespace import.
    pub(in crate::lowering) fn mark_namespace(&mut self, local: String) {
        self.namespaces.insert(local);
    }

    /// Return whether a local name is a namespace import binding.
    pub(in crate::lowering) fn is_namespace(&self, name: &str) -> bool {
        self.namespaces.contains(name)
    }

    /// Return whether this module imported `name` in any form.
    pub(in crate::lowering) fn is_imported_binding(&self, name: &str) -> bool {
        self.is_value(name) || self.is_type_only(name) || self.is_namespace(name)
    }

    /// Record a test-framework API name imported from a Vitest-compatible module.
    pub(in crate::lowering) fn mark_test_builtin(&mut self, local: String) {
        self.test_builtins.insert(local);
    }

    /// Return whether a name is an imported test-framework API.
    pub(in crate::lowering) fn is_test_builtin(&self, name: &str) -> bool {
        self.test_builtins.contains(name)
    }

    /// Record a local bound to `tz` from `@date-fns/tz`.
    pub(in crate::lowering) fn mark_date_fns_timezone_factory(&mut self, local: String) {
        self.date_fns_timezone_factories.insert(local);
    }

    /// Return whether a name is a `@date-fns/tz` timezone factory.
    pub(in crate::lowering) fn is_date_fns_timezone_factory(&self, name: &str) -> bool {
        self.date_fns_timezone_factories.contains(name)
    }

    /// Record a local statically known to alias the ambient global object.
    pub(in crate::lowering) fn mark_global_object_alias(&mut self, local: String) {
        self.global_object_aliases.insert(local);
    }

    /// Return whether a name aliases the ambient global object.
    pub(in crate::lowering) fn is_global_object_alias(&self, name: &str) -> bool {
        self.global_object_aliases.contains(name)
    }

    /// Return every name this module knows to alias the ambient global object.
    ///
    /// Used when recording module exports so an importing module can recognize
    /// an imported global-object alias (an es-toolkit style `globalThis` shim).
    pub(in crate::lowering) fn global_object_alias_names(&self) -> &HashSet<String> {
        &self.global_object_aliases
    }
}

#[cfg(test)]
mod tests {
    use super::ImportScope;

    /// Every import form answers the one "did this module import it?" question.
    #[test]
    fn is_imported_binding_covers_every_import_form() {
        let mut imports = ImportScope::default();
        assert!(!imports.is_imported_binding("value"));
        imports.mark_value("value".to_owned());
        imports.mark_type_only("ty".to_owned());
        imports.mark_namespace("ns".to_owned());

        assert!(imports.is_imported_binding("value"));
        assert!(imports.is_imported_binding("ty"));
        assert!(imports.is_imported_binding("ns"));
        assert!(!imports.is_imported_binding("local"));
    }

    /// The narrow provenance sets stay independent of the import-form sets: a
    /// `globalThis` alias is not an import, and a test builtin is not a
    /// namespace.
    #[test]
    fn narrow_provenance_sets_stay_separate() {
        let mut imports = ImportScope::default();
        imports.mark_global_object_alias("g".to_owned());
        imports.mark_test_builtin("expect".to_owned());
        imports.mark_date_fns_timezone_factory("tz".to_owned());

        assert!(imports.is_global_object_alias("g"));
        assert!(!imports.is_imported_binding("g"));
        assert!(imports.is_test_builtin("expect"));
        assert!(!imports.is_namespace("expect"));
        assert!(imports.is_date_fns_timezone_factory("tz"));
    }
}
