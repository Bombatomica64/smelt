//! One resolver for unqualified names.
//!
//! JavaScript and TypeScript resolve a bare name lexically: the innermost
//! enclosing scope that binds it wins, then the module scope (declarations
//! and imports), and only a name nothing in the source binds is the host's
//! (`Date`, `RegExp`, `NaN`, the lib type `Map`). The lowering kept that order
//! by convention, consulting its tables (classes, enums, lib spellings) at
//! each use site, and every site that asked a table before the scope got it
//! wrong: `const Item = xs[0]` beside `class Item` read the class (audit A4),
//! `function f(Mode: { Fast: number })` folded `Mode.Fast` to an enum member
//! (A3), and `import type { Map } from './game'` became the builtin `Map` (A5).
//!
//! The order now lives here, once. [`ModuleBuilder::resolve_value`] and
//! [`ModuleBuilder::resolve_type_name`] return an exhaustive answer whose
//! FIRST question is the lexical scope, so a caller matches on what the name
//! is instead of choosing which table to ask. The facts that must only be
//! read after resolution are gated by type: enum members are read through an
//! [`EnumBinding`], which only [`ModuleBuilder::resolve_value`] can make, so a
//! fold that skipped the scope does not compile.

use smelt_hir::{LocalId, TypeId};

use super::ModuleBuilder;

/// Proof that a bare name resolved to an `enum` the module declares or
/// imports, at the point it was resolved.
///
/// The field is private to this module: the only way to obtain one is
/// [`ModuleBuilder::resolve_value`], which asks the lexical scope first.
#[derive(Debug, Clone, Copy)]
pub(in crate::lowering) struct EnumBinding<'name> {
    /// The enum's local name.
    name: &'name str,
}

impl EnumBinding<'_> {
    /// The enum's local name, the key of the module's enum-member table.
    pub(in crate::lowering) const fn name(&self) -> &str {
        self.name
    }
}

/// What an unqualified VALUE name refers to where it is read.
#[derive(Debug, Clone, Copy)]
#[expect(
    variant_size_differences,
    reason = "a short-lived answer of 24 bytes; boxing the borrowed name would cost an allocation per identifier"
)]
pub(in crate::lowering) enum ValueBinding<'name> {
    /// A lexical binding: a parameter, local, loop or `catch` variable, or a
    /// nested function. It shadows everything below.
    Local(LocalId),
    /// An `enum` the module declares or imports.
    Enum(EnumBinding<'name>),
    /// A class the module declares or imports.
    Class,
    /// Some other module-scope binding: a constant, function, module
    /// variable or import.
    Module,
    /// Nothing in the source binds the name: it is the host's global
    /// (`Date`, `RegExp`, `NaN`, a typed array constructor) or unresolved.
    Host,
}

/// What an unqualified TYPE name refers to where it is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum TypeBinding {
    /// A type parameter in scope, lowered to this type.
    Param(TypeId),
    /// A declaration of this module, or a name it imports from a source
    /// module: never the lib type of the same spelling.
    Source,
    /// No source binding: the lib type of that spelling (`Map`, `Promise`).
    Lib,
}

impl ModuleBuilder<'_> {
    /// Resolve the value name `name` at the current lowering point; see the
    /// module docs. The lexical scope is asked first, always.
    pub(in crate::lowering) fn resolve_value<'name>(&self, name: &'name str) -> ValueBinding<'name> {
        if let Some(local) = self.scope.lookup(name) {
            return ValueBinding::Local(local);
        }
        if self.consts.has_enum(name) {
            return ValueBinding::Enum(EnumBinding { name });
        }
        if self.classes.contains(name) {
            return ValueBinding::Class;
        }
        if self.source_facts.declares_module_value(name)
            || self.imports.is_imported_binding(name)
            || self.module_globals.contains_key(name)
            || self.mutable_global_items.contains_key(name)
            || self.consts.is_folded_const(name)
            || self.consts.regexp(name).is_some()
        {
            return ValueBinding::Module;
        }
        ValueBinding::Host
    }

    /// Resolve the unqualified type name `name`; see the module docs. A type
    /// parameter in scope is asked first, then the module's declarations and
    /// imports, and only then the lib.
    pub(in crate::lowering) fn resolve_type_name(&self, name: &str) -> TypeBinding {
        if let Some(param) = self.types.param_type(name) {
            return TypeBinding::Param(param);
        }
        if self.source_shadows_global(name) {
            return TypeBinding::Source;
        }
        TypeBinding::Lib
    }

    /// Whether `name` is bound by an import from anything but a modeled host
    /// module.
    ///
    /// Any such import, type-only or value, shadows the lib name of the same
    /// spelling: `import type { Map } from './game'` is the game's `Map`. An
    /// import from a host module (`import { URL } from 'node:url'`) IS the
    /// host's binding, so it does not.
    pub(in crate::lowering) fn import_shadows_lib(&self, name: &str) -> bool {
        self.imports.is_imported_binding(name)
            && !self
                .imports
                .import_source(name)
                .is_some_and(smelt_stdlib::is_host_module)
    }
}
