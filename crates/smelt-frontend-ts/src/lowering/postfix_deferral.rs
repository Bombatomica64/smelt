//! Whether a variable initializer may defer its postfix updates.
//!
//! A postfix update evaluated inside a declaration's initializer
//! (`const v = i++`) can be lowered two ways. The immediate form snapshots the
//! old value into a temporary, stores the new value, and evaluates to the
//! snapshot. The deferred form evaluates to a bare read of `i` and emits the
//! store AFTER the binding statement, which still sees the old value: it is
//! the shape a hand-written port uses (`let v = i; i += 1;`).
//!
//! JavaScript stores the new value at the point the update is evaluated, so
//! the deferred form is only equivalent when nothing evaluated LATER in the
//! same initializer can observe the variable before the binding finishes:
//!
//! * a later read of the same name (`i++ + i`, `i++ + f(i)`) would see the old
//!   value -- and the deferred update's own bare read would see a value a
//!   later store already changed;
//! * any later call, construction, tagged template, `await`/`yield` or member
//!   read (which may run a getter) can run code that reads the variable
//!   through a closure;
//! * a closure created later captures by value where it is created (see
//!   `hoisting`), so one that mentions the name would capture the old value;
//! * a member target (`o.p++`) is observed by any later member access, which
//!   may reach the same object through another name.
//!
//! The check is a conservative, purely syntactic walk in source order, which
//! is evaluation order for every expression form that can contain an update
//! except the branches of a conditional -- and a branch only makes the answer
//! more conservative. When it answers `false` the declaration keeps every
//! update immediate.

use std::collections::HashSet;

use oxc::ast::ast::{
    ArrowFunctionExpression, AwaitExpression, CallExpression, ComputedMemberExpression, Expression,
    Function, IdentifierReference, NewExpression, PrivateFieldExpression, SimpleAssignmentTarget,
    StaticMemberExpression, TaggedTemplateExpression, UpdateExpression, YieldExpression,
};
use oxc::ast_visit::{Visit, walk};
use oxc::syntax::scope::ScopeFlags;

/// Return whether every postfix update in `initializer` may store after the
/// binding statement instead of where it is evaluated; see the module docs.
pub(in crate::lowering) fn initializer_can_defer_postfix_updates(
    initializer: &Expression<'_>,
) -> bool {
    let mut check = DeferralCheck::default();
    check.visit_expression(initializer);
    !check.observed
}

/// Source-order walk state of [`initializer_can_defer_postfix_updates`].
#[derive(Default)]
struct DeferralCheck {
    /// Names a postfix update evaluated earlier in the initializer stores to.
    pending_names: HashSet<String>,
    /// Whether an earlier postfix update stores to a member target.
    pending_member: bool,
    /// Nesting depth of function bodies being walked: code there does not run
    /// while the initializer is evaluated, so its updates are not this
    /// declaration's and its calls are not evaluated now.
    function_depth: usize,
    /// Whether some later evaluation may observe a deferred store.
    observed: bool,
}

impl DeferralCheck {
    /// Whether an update evaluated earlier in the initializer is still waiting
    /// for its deferred store.
    fn has_pending(&self) -> bool {
        !self.pending_names.is_empty() || self.pending_member
    }

    /// Record that the operation just evaluated may run arbitrary code, which
    /// observes any pending store when it runs during the initializer.
    fn opaque_operation(&mut self) {
        if self.function_depth == 0 && self.has_pending() {
            self.observed = true;
        }
    }
}

impl<'a> Visit<'a> for DeferralCheck {
    /// A read (or write) of a pending name observes its store; this holds
    /// inside a nested function too, because the closure captures the value
    /// where it is created.
    fn visit_identifier_reference(&mut self, ident: &IdentifierReference<'a>) {
        if self.pending_names.contains(ident.name.as_str()) {
            self.observed = true;
        }
    }

    /// Record a postfix update AFTER its operand is walked: the operand's own
    /// read is not a later observation.
    fn visit_update_expression(&mut self, update: &UpdateExpression<'a>) {
        walk::walk_update_expression(self, update);
        if update.prefix || self.function_depth > 0 {
            return;
        }
        match &update.argument {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => {
                self.pending_names.insert(ident.name.to_string());
            }
            _ => self.pending_member = true,
        }
    }

    /// A call runs after its callee and arguments are evaluated, so it is
    /// checked once they are walked.
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        walk::walk_call_expression(self, call);
        self.opaque_operation();
    }

    /// A construction runs the constructor after its arguments.
    fn visit_new_expression(&mut self, new: &NewExpression<'a>) {
        walk::walk_new_expression(self, new);
        self.opaque_operation();
    }

    /// A tagged template calls its tag after the substitutions.
    fn visit_tagged_template_expression(&mut self, tagged: &TaggedTemplateExpression<'a>) {
        walk::walk_tagged_template_expression(self, tagged);
        self.opaque_operation();
    }

    /// `await` lets other code run before the binding completes.
    fn visit_await_expression(&mut self, await_expr: &AwaitExpression<'a>) {
        walk::walk_await_expression(self, await_expr);
        self.opaque_operation();
    }

    /// `yield` hands control to the consumer before the binding completes.
    fn visit_yield_expression(&mut self, yield_expr: &YieldExpression<'a>) {
        walk::walk_yield_expression(self, yield_expr);
        self.opaque_operation();
    }

    /// A member read may run a getter after its object (and key) is evaluated.
    fn visit_static_member_expression(&mut self, member: &StaticMemberExpression<'a>) {
        walk::walk_static_member_expression(self, member);
        self.opaque_operation();
    }

    /// See [`Self::visit_static_member_expression`].
    fn visit_computed_member_expression(&mut self, member: &ComputedMemberExpression<'a>) {
        walk::walk_computed_member_expression(self, member);
        self.opaque_operation();
    }

    /// See [`Self::visit_static_member_expression`].
    fn visit_private_field_expression(&mut self, member: &PrivateFieldExpression<'a>) {
        walk::walk_private_field_expression(self, member);
        self.opaque_operation();
    }

    /// A function body does not run during the initializer; see
    /// [`DeferralCheck::function_depth`].
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        self.function_depth += 1;
        walk::walk_function(self, function, flags);
        self.function_depth -= 1;
    }

    /// See [`Self::visit_function`].
    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        self.function_depth += 1;
        walk::walk_arrow_function_expression(self, arrow);
        self.function_depth -= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::initializer_can_defer_postfix_updates;
    use oxc::allocator::Allocator;
    use oxc::ast::ast::{Expression, Statement};
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    /// Parse `const v = <initializer>;` and run the check on its initializer.
    fn can_defer(initializer: &str) -> bool {
        let allocator = Allocator::default();
        let source = format!("const v = {initializer};");
        let parsed = Parser::new(&allocator, &source, SourceType::ts()).parse();
        let Some(Statement::VariableDeclaration(decl)) = parsed.program.body.first() else {
            return false;
        };
        let Some(init): Option<&Expression<'_>> =
            decl.declarations.first().and_then(|d| d.init.as_ref())
        else {
            return false;
        };
        initializer_can_defer_postfix_updates(init)
    }

    /// Nothing evaluated after the update can observe it.
    #[test]
    fn a_lone_or_trailing_update_defers() {
        assert!(can_defer("i++"));
        assert!(can_defer("i++ + 1"));
        assert!(can_defer("i + i++"));
        assert!(can_defer("f(i) + i++"));
        assert!(can_defer("--i + j++"));
    }

    /// A later read, call, construction or member read may observe the store.
    #[test]
    fn a_later_observation_keeps_the_update_immediate() {
        assert!(!can_defer("i++ + i"));
        assert!(!can_defer("i++ + i++"));
        assert!(!can_defer("i++ + new C().m(i)"));
        assert!(!can_defer("i++ + g()"));
        assert!(!can_defer("f(i++)"));
        assert!(!can_defer("arr[i++]"));
        assert!(!can_defer("[i++, () => i]"));
        assert!(!can_defer("o.p++ + q.p"));
    }

    /// Code inside a nested body does not run during the initializer.
    #[test]
    fn nested_bodies_do_not_count_as_evaluation() {
        assert!(can_defer("[i++, () => g()]"));
        assert!(can_defer("[() => j++, i]"));
    }
}
