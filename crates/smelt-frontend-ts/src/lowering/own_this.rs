//! A non-arrow function's `this` is its own.
//!
//! JavaScript supplies a `function`'s `this` from its CALL; an arrow has none
//! of its own and sees the enclosing one. Inside a class method `this` is bound
//! in scope to the method's receiver local, so a `function` expression nested
//! there must not resolve `this` to that local (it belongs to another body):
//! the binding is shadowed for the function's body, and its `this` lowers to
//! `ExprKind::ThisRead`, the receiver its frame took at entry (codegen's
//! `this_frame`). An arrow keeps the enclosing binding, and an arrow inside a
//! plain function reads the enclosing frame's receiver (`ClosureThis::Lexical`).

use crate::lowering::ModuleBuilder;

impl ModuleBuilder<'_> {
    /// Shadow an enclosing `this` binding for the body of a non-arrow function
    /// about to be lowered; answers the binding the caller restores afterwards.
    pub(in crate::lowering) fn shadow_enclosing_this(&mut self) -> Option<smelt_hir::LocalId> {
        let prior = self.scope.lookup("this");
        if prior.is_some() {
            self.scope.unbind("this");
        }
        prior
    }
}
