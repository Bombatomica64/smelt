//! Lowering modes and lexical stacks scoped to one region of the source.
//!
//! Each is entered through a `with_*` helper that restores the enclosing
//! state on every exit path of the closure, so an early `?` inside the region
//! cannot leave the mode switched or a stack entry behind.
//!
//! [`UnknownIndexAccess`] says whether lowering may index erased (`unknown`)
//! metadata. Ordinary code may not: indexing a value with no static shape is a
//! dynamic boundary Smelt rejects. Type-test code may, because it is checked by
//! TypeScript and erased before runtime. The mode is set once for a whole
//! declaration type-test file and widened only for the argument of a type-test
//! assertion, through [`ModuleBuilder::with_type_test_argument_mode`], which
//! restores the enclosing mode on every exit path.

use crate::lowering::ModuleBuilder;

/// Whether lowering may index erased unknown metadata, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::lowering) enum UnknownIndexAccess {
    /// Ordinary code: indexing erased metadata is rejected.
    #[default]
    Rejected,
    /// A declaration type-test file: the whole module is type-level only.
    TypeTestFile,
    /// The argument of a type-test assertion (`expectTypeOf(x)`) is being
    /// lowered; the assertion itself is erased.
    TypeTestArgument,
}

impl UnknownIndexAccess {
    /// The file-level mode of a module.
    pub(in crate::lowering) const fn for_module(is_declaration_type_test: bool) -> Self {
        if is_declaration_type_test {
            Self::TypeTestFile
        } else {
            Self::Rejected
        }
    }
}

impl ModuleBuilder<'_> {
    /// Whether type-test-only lowering may index erased unknown metadata here.
    pub(in crate::lowering) const fn allows_unknown_index_access(&self) -> bool {
        !matches!(self.unknown_index_access, UnknownIndexAccess::Rejected)
    }

    /// Lower a type-test assertion's argument with unknown index access
    /// allowed, restoring the enclosing mode on every exit path of `lower`.
    pub(in crate::lowering) fn with_type_test_argument_mode<R>(
        &mut self,
        lower: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let previous = std::mem::replace(
            &mut self.unknown_index_access,
            UnknownIndexAccess::TypeTestArgument,
        );
        let result = lower(self);
        self.unknown_index_access = previous;
        result
    }

    /// Run `lower` with `receiver` (an AST node address) listed as an
    /// array-method receiver, popping it on every exit path; see
    /// `ModuleBuilder::array_view_receivers`.
    pub(in crate::lowering) fn with_array_view_receiver<R>(
        &mut self,
        receiver: usize,
        lower: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.array_view_receivers.push(receiver);
        let result = lower(self);
        self.array_view_receivers.pop();
        result
    }

    /// Run `lower` while the local `function` named `name` lowers its own
    /// body, popping it on every exit path; see
    /// `ModuleBuilder::defining_local_functions`.
    pub(in crate::lowering) fn with_defining_local_function<R>(
        &mut self,
        name: String,
        lower: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.defining_local_functions.push(name);
        let result = lower(self);
        self.defining_local_functions.pop();
        result
    }
}
