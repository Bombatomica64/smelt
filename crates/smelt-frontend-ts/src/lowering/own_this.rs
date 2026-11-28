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
use oxc::ast::ast::Function;
use oxc::ast_visit::{Visit, walk};

/// Whether `function` (a `function` expression or an object literal's method)
/// reads its own `this`, or its home object through `super`: a `this` or
/// `super` outside any nested non-arrow function or class, whose own it would
/// be. Arrows nested in the body see the function's `this`, so theirs count.
pub(in crate::lowering) fn function_reads_own_this(function: &Function<'_>) -> bool {
    let mut finder = OwnThisFinder::default();
    if let Some(body) = &function.body {
        walk::walk_function_body(&mut finder, body);
    }
    finder.found
}

/// Finds a `this` (or `super`) that belongs to the function being visited:
/// one outside any nested non-arrow function, whose own `this` it would be.
#[derive(Default)]
struct OwnThisFinder {
    /// Whether such a `this` was seen.
    found: bool,
}

impl<'a> Visit<'a> for OwnThisFinder {
    fn visit_this_expression(&mut self, _it: &oxc::ast::ast::ThisExpression) {
        self.found = true;
    }

    fn visit_super(&mut self, _it: &oxc::ast::ast::Super) {
        self.found = true;
    }

    fn visit_function(&mut self, _it: &Function<'a>, _flags: oxc::syntax::scope::ScopeFlags) {
        // A nested `function` has its own `this`.
    }

    fn visit_class(&mut self, _it: &oxc::ast::ast::Class<'a>) {
        // So do a nested class's members.
    }
}

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
