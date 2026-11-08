//! Scope-aware questions about how a statement uses one outer binding.
//!
//! Several pre-passes ask "does this statement refer to the sibling
//! declaration `name`?" before the statement is lowered, so they cannot ask the
//! lowering's own scope. Matching identifier text alone is wrong whenever a
//! nested scope declares the same name: in
//!
//! ```ignore
//! const apply = (g: number) => g * 2
//! const scale = 3
//! function g() { return scale }
//! ```
//!
//! the arrow's `g` is its parameter, not the sibling function, and hoisting the
//! function above `const apply` (which a text match does) moves it above
//! `scale`, the binding it closes over.
//!
//! [`ScopedUses`] walks a statement with oxc's visitor and stops at every
//! nested scope that declares `name` itself: a function or arrow (its
//! parameters and its var-scoped declarations), a block (its lexical
//! declarations), a `catch` parameter, a `for` head and a named class
//! expression. Everything it reports therefore resolves to the binding that
//! is visible where the statement sits.

use oxc::ast::ast::{
    ArrowFunctionExpression, AssignmentExpression, AssignmentTarget, BinaryExpression,
    BindingPattern, BlockStatement, CatchClause, Class, Expression,
    ForInStatement, ForOfStatement, ForStatement, ForStatementInit, ForStatementLeft,
    FormalParameters, Function, IdentifierReference, NewExpression, Statement,
    StaticBlock, SwitchStatement, VariableDeclaration, VariableDeclarationKind,
};
use oxc::ast_visit::{Visit, walk};
use oxc::syntax::operator::BinaryOperator;
use oxc::syntax::scope::ScopeFlags;

/// How one statement uses the outer binding `name`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) struct BindingUses {
    /// Some identifier reference resolves to the binding.
    pub referenced: bool,
    /// The binding is used as a constructor: `new name(..)`, `x instanceof
    /// name`, or `name.prototype.member = ..`.
    pub constructed: bool,
}

/// Scope-aware uses of `name` in `statement`; see the module docs.
pub(in crate::lowering) fn statement_uses(statement: &Statement<'_>, name: &str) -> BindingUses {
    let mut visitor = ScopedUses { name, uses: BindingUses::default() };
    visitor.visit_statement(statement);
    visitor.uses
}

/// Scope-aware uses of `name` across a statement list (one scope's body).
pub(in crate::lowering) fn statements_use(statements: &[Statement<'_>], name: &str) -> BindingUses {
    let mut uses = BindingUses::default();
    for statement in statements {
        let one = statement_uses(statement, name);
        uses.referenced |= one.referenced;
        uses.constructed |= one.constructed;
    }
    uses
}

/// The visitor behind [`statement_uses`]: it records uses of `name` and does
/// not descend into a nested scope that rebinds it.
struct ScopedUses<'name> {
    /// The outer binding being looked for.
    name: &'name str,
    /// What has been found so far.
    uses: BindingUses,
}

impl ScopedUses<'_> {
    /// Whether `expression` (parentheses peeled) is a reference to `name`.
    fn is_name(&self, expression: &Expression<'_>) -> bool {
        matches!(expression.without_parentheses(), Expression::Identifier(id) if id.name == self.name)
    }

    /// Whether `pattern` binds `name`.
    fn pattern_binds(&self, pattern: &BindingPattern<'_>) -> bool {
        pattern.get_binding_identifiers().iter().any(|id| id.name == self.name)
    }

    /// Whether a parameter list binds `name`.
    fn params_bind(&self, params: &FormalParameters<'_>) -> bool {
        params.items.iter().any(|param| self.pattern_binds(&param.pattern))
            || params.rest.as_ref().is_some_and(|rest| self.pattern_binds(&rest.rest.argument))
    }

    /// Whether a variable declaration binds `name`.
    fn declaration_binds(&self, declaration: &VariableDeclaration<'_>) -> bool {
        declaration.declarations.iter().any(|declarator| self.pattern_binds(&declarator.id))
    }

    /// Whether a declaration statement directly in a block binds `name` in
    /// that block: `let`/`const`/`using`, a class, or (block-scoped in strict
    /// code) a function declaration.
    fn lexically_declares(&self, statement: &Statement<'_>) -> bool {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                declaration.kind != VariableDeclarationKind::Var && self.declaration_binds(declaration)
            }
            Statement::FunctionDeclaration(function) => {
                function.id.as_ref().is_some_and(|id| id.name == self.name)
            }
            Statement::ClassDeclaration(class) => class.id.as_ref().is_some_and(|id| id.name == self.name),
            _ => false,
        }
    }

    /// Whether a function body binds `name` for the whole function: any
    /// declaration directly in it, or a `var` anywhere in its blocks (a `var`
    /// is function-scoped, so one inside an `if` still shadows).
    fn function_scope_declares(&self, statements: &[Statement<'_>]) -> bool {
        statements
            .iter()
            .any(|statement| self.lexically_declares(statement) || self.var_declares(statement))
    }

    /// Whether `statement` holds a `var` binding `name`, looking through nested
    /// blocks but not into nested functions.
    fn var_declares(&self, statement: &Statement<'_>) -> bool {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                declaration.kind == VariableDeclarationKind::Var && self.declaration_binds(declaration)
            }
            Statement::BlockStatement(block) => block.body.iter().any(|inner| self.var_declares(inner)),
            Statement::IfStatement(statement) => {
                self.var_declares(&statement.consequent)
                    || statement.alternate.as_ref().is_some_and(|alternate| self.var_declares(alternate))
            }
            Statement::ForStatement(statement) => {
                matches!(&statement.init, Some(ForStatementInit::VariableDeclaration(declaration))
                    if declaration.kind == VariableDeclarationKind::Var && self.declaration_binds(declaration))
                    || self.var_declares(&statement.body)
            }
            Statement::ForInStatement(statement) => {
                self.for_left_var_binds(&statement.left) || self.var_declares(&statement.body)
            }
            Statement::ForOfStatement(statement) => {
                self.for_left_var_binds(&statement.left) || self.var_declares(&statement.body)
            }
            Statement::WhileStatement(statement) => self.var_declares(&statement.body),
            Statement::DoWhileStatement(statement) => self.var_declares(&statement.body),
            Statement::LabeledStatement(statement) => self.var_declares(&statement.body),
            Statement::TryStatement(statement) => {
                statement.block.body.iter().any(|inner| self.var_declares(inner))
                    || statement
                        .handler
                        .as_ref()
                        .is_some_and(|handler| handler.body.body.iter().any(|inner| self.var_declares(inner)))
                    || statement
                        .finalizer
                        .as_ref()
                        .is_some_and(|finalizer| finalizer.body.iter().any(|inner| self.var_declares(inner)))
            }
            Statement::SwitchStatement(statement) => statement
                .cases
                .iter()
                .any(|case| case.consequent.iter().any(|inner| self.var_declares(inner))),
            _ => false,
        }
    }

    /// Whether a `for (var x in/of ..)` head binds `name`.
    fn for_left_var_binds(&self, left: &ForStatementLeft<'_>) -> bool {
        matches!(left, ForStatementLeft::VariableDeclaration(declaration)
            if declaration.kind == VariableDeclarationKind::Var && self.declaration_binds(declaration))
    }

    /// Whether a `for (let x in/of ..)` head binds `name` for the loop.
    fn for_left_lexical_binds(&self, left: &ForStatementLeft<'_>) -> bool {
        matches!(left, ForStatementLeft::VariableDeclaration(declaration)
            if declaration.kind != VariableDeclarationKind::Var && self.declaration_binds(declaration))
    }
}

impl<'a> Visit<'a> for ScopedUses<'_> {
    /// A reference to `name` that reached here is not shadowed.
    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        if identifier.name == self.name {
            self.uses.referenced = true;
        }
    }

    /// `new name(..)` constructs the binding.
    fn visit_new_expression(&mut self, expression: &NewExpression<'a>) {
        if self.is_name(&expression.callee) {
            self.uses.constructed = true;
        }
        walk::walk_new_expression(self, expression);
    }

    /// `value instanceof name` tests against the binding as a constructor.
    fn visit_binary_expression(&mut self, expression: &BinaryExpression<'a>) {
        if expression.operator == BinaryOperator::Instanceof && self.is_name(&expression.right) {
            self.uses.constructed = true;
        }
        walk::walk_binary_expression(self, expression);
    }

    /// `name.prototype.member = ..` extends the binding as a constructor.
    fn visit_assignment_expression(&mut self, expression: &AssignmentExpression<'a>) {
        if let AssignmentTarget::StaticMemberExpression(member) = &expression.left
            && let Expression::StaticMemberExpression(prototype) = &member.object
            && prototype.property.name == "prototype"
            && self.is_name(&prototype.object)
        {
            self.uses.constructed = true;
        }
        walk::walk_assignment_expression(self, expression);
    }

    /// A function scope that rebinds `name` (its own name for an expression,
    /// a parameter, or a body declaration) hides every use inside it.
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        let own_name = function.is_expression() && function.id.as_ref().is_some_and(|id| id.name == self.name);
        let body_declares =
            function.body.as_ref().is_some_and(|body| self.function_scope_declares(&body.statements));
        if own_name || self.params_bind(&function.params) || body_declares {
            return;
        }
        walk::walk_function(self, function, flags);
    }

    /// An arrow that rebinds `name` hides every use inside it.
    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        if self.params_bind(&arrow.params) || self.function_scope_declares(crate::lowering::support::arrow_block_statements(arrow)) {
            return;
        }
        walk::walk_arrow_function_expression(self, arrow);
    }

    /// A block whose own declarations rebind `name` hides every use inside it.
    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        if block.body.iter().any(|statement| self.lexically_declares(statement)) {
            return;
        }
        walk::walk_block_statement(self, block);
    }

    /// A static block is a function scope of its own.
    fn visit_static_block(&mut self, block: &StaticBlock<'a>) {
        if self.function_scope_declares(&block.body) {
            return;
        }
        walk::walk_static_block(self, block);
    }

    /// The cases of a `switch` share one block scope.
    fn visit_switch_statement(&mut self, statement: &SwitchStatement<'a>) {
        if statement
            .cases
            .iter()
            .any(|case| case.consequent.iter().any(|inner| self.lexically_declares(inner)))
        {
            self.visit_expression(&statement.discriminant);
            return;
        }
        walk::walk_switch_statement(self, statement);
    }

    /// A `catch (name)` parameter rebinds `name` for the handler.
    fn visit_catch_clause(&mut self, clause: &CatchClause<'a>) {
        if clause.param.as_ref().is_some_and(|param| self.pattern_binds(&param.pattern)) {
            return;
        }
        walk::walk_catch_clause(self, clause);
    }

    /// A `for (let name ..)` head rebinds `name` for the whole loop.
    fn visit_for_statement(&mut self, statement: &ForStatement<'a>) {
        if matches!(&statement.init, Some(ForStatementInit::VariableDeclaration(declaration))
            if declaration.kind != VariableDeclarationKind::Var && self.declaration_binds(declaration))
        {
            return;
        }
        walk::walk_for_statement(self, statement);
    }

    /// A `for (let name in ..)` head rebinds `name` for the loop body; the
    /// iterated object is still evaluated outside it.
    fn visit_for_in_statement(&mut self, statement: &ForInStatement<'a>) {
        if self.for_left_lexical_binds(&statement.left) {
            self.visit_expression(&statement.right);
            return;
        }
        walk::walk_for_in_statement(self, statement);
    }

    /// A `for (let name of ..)` head rebinds `name` for the loop body; the
    /// iterated value is still evaluated outside it.
    fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'a>) {
        if self.for_left_lexical_binds(&statement.left) {
            self.visit_expression(&statement.right);
            return;
        }
        walk::walk_for_of_statement(self, statement);
    }

    /// A class expression named `name` binds it inside its own body.
    fn visit_class(&mut self, class: &Class<'a>) {
        if class.is_expression() && class.id.as_ref().is_some_and(|id| id.name == self.name) {
            return;
        }
        walk::walk_class(self, class);
    }
}

#[cfg(test)]
mod tests {
    use super::{BindingUses, statements_use};
    use oxc::allocator::Allocator;
    use oxc::parser::Parser;
    use oxc::span::SourceType;

    /// The uses of `name` across a whole parsed program.
    fn uses(source: &str, name: &str) -> BindingUses {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        statements_use(&parsed.program.body, name)
    }

    #[test]
    fn a_parameter_of_the_same_name_hides_the_outer_binding() {
        assert!(!uses("const apply = (g: number) => g * 2", "g").referenced);
        assert!(!uses("function h(g: number) { return g }", "g").referenced);
        assert!(uses("const apply = (x: number) => g(x)", "g").referenced);
    }

    #[test]
    fn a_nested_declaration_hides_the_outer_binding_for_its_whole_scope() {
        assert!(!uses("function h() { if (1) { var g = 1 } return g }", "g").referenced);
        assert!(!uses("{ g(); function g() {} }", "g").referenced);
        assert!(!uses("try {} catch (g) { g }", "g").referenced);
        assert!(!uses("for (const g of xs) { g }", "g").referenced);
        assert!(uses("for (const x of g) { x }", "g").referenced);
    }

    #[test]
    fn constructor_uses_are_found_at_any_depth() {
        assert!(uses("return new Foo().a", "Foo").constructed);
        assert!(uses("while (x) { y = [new Foo()][0] }", "Foo").constructed);
        assert!(uses("Foo.prototype.m = function () {}", "Foo").constructed);
        assert!(uses("const ok = v instanceof Foo", "Foo").constructed);
        assert!(!uses("const f = (Foo: any) => new Foo()", "Foo").constructed);
        assert!(!uses("Foo()", "Foo").constructed);
    }
}
