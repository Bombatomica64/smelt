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
//! [`ScopedUses`] walks a statement ONCE with oxc's visitor for a whole set of
//! names, and at every nested scope that declares some of them itself (a
//! function or arrow: its parameters and var-scoped declarations; a block:
//! its lexical declarations; a `catch` parameter; a `for` head; a named class
//! expression) it shadows those names for the scope's extent. Everything it
//! reports therefore resolves to the bindings visible where the statement
//! sits. One walk per statement, whatever the number of names, is what keeps
//! the callers (hoisting, constructor detection) linear in program size.

use std::collections::HashSet;

use oxc::ast::ast::{
    ArrowFunctionExpression, AssignmentExpression, AssignmentTarget,
    BindingPattern, BlockStatement, CatchClause, Class, Expression,
    ForInStatement, ForOfStatement, ForStatement, ForStatementInit, ForStatementLeft,
    FormalParameters, Function, IdentifierReference, NewExpression, Statement,
    StaticBlock, SwitchStatement, VariableDeclaration, VariableDeclarationKind,
};
use oxc::ast_visit::{Visit, walk};
use oxc::syntax::scope::ScopeFlags;

/// How one statement uses the outer binding `name`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) struct BindingUses {
    /// Some identifier reference resolves to the binding.
    pub referenced: bool,
    /// The binding is used as a NOMINAL constructor: `new name(..)` or
    /// `name.prototype.member = ..`.
    ///
    /// Not `x instanceof name`: a test makes no instance, so it is no reason
    /// to synthesize a class, and the instances it tests come from somewhere
    /// else. A function constructed only as an erased value (`new (C as
    /// any)()`) keeps the dynamic construction path, which models what a
    /// synthesized class cannot: an object `return` that overrides the
    /// allocation, and `C.prototype` as the instances' prototype.
    pub constructed: bool,
}

/// How a statement uses each of a set of outer bindings: the names, among
/// those asked about, it references and constructs (see [`BindingUses`]).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(in crate::lowering) struct NameUses {
    /// Names some identifier reference resolves to.
    pub referenced: HashSet<String>,
    /// Names used as a nominal constructor.
    pub constructed: HashSet<String>,
}

/// Scope-aware uses of every name of `names` in `statement`, in one walk.
pub(in crate::lowering) fn statement_name_uses(statement: &Statement<'_>, names: &HashSet<&str>) -> NameUses {
    let mut visitor = ScopedUses { names, shadowed: Vec::new(), uses: NameUses::default() };
    visitor.visit_statement(statement);
    visitor.uses
}

/// Scope-aware uses of `name` in `statement`; see the module docs.
pub(in crate::lowering) fn statement_uses(statement: &Statement<'_>, name: &str) -> BindingUses {
    let names = HashSet::from([name]);
    let uses = statement_name_uses(statement, &names);
    BindingUses { referenced: uses.referenced.contains(name), constructed: uses.constructed.contains(name) }
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

/// The visitor behind [`statement_name_uses`]: it records uses of `names` that
/// no enclosing scope inside the statement rebinds.
struct ScopedUses<'names, 'name> {
    /// The outer bindings being looked for.
    names: &'names HashSet<&'name str>,
    /// For each nested scope being walked, the asked-about names it rebinds.
    shadowed: Vec<Vec<String>>,
    /// What has been found so far.
    uses: NameUses,
}

impl ScopedUses<'_, '_> {
    /// The asked-about name `text` is when it is one and no scope shadows it.
    fn visible(&self, text: &str) -> bool {
        self.names.contains(text) && !self.shadowed.iter().any(|frame| frame.iter().any(|name| name == text))
    }

    /// The visible asked-about name `expression` (parentheses peeled) reads.
    fn visible_name(&self, expression: &Expression<'_>) -> Option<String> {
        match expression.without_parentheses() {
            Expression::Identifier(id) if self.visible(id.name.as_str()) => Some(id.name.to_string()),
            _ => None,
        }
    }

    /// Run `walk` with `bound` (filtered to the asked-about names) shadowed.
    fn within_scope(&mut self, bound: Vec<String>, walk: impl FnOnce(&mut Self)) {
        let bound: Vec<String> = bound.into_iter().filter(|name| self.names.contains(name.as_str())).collect();
        if bound.is_empty() {
            walk(self);
            return;
        }
        self.shadowed.push(bound);
        walk(self);
        self.shadowed.pop();
    }

    /// The names `pattern` binds.
    fn pattern_names(pattern: &BindingPattern<'_>, out: &mut Vec<String>) {
        out.extend(pattern.get_binding_identifiers().iter().map(|id| id.name.to_string()));
    }

    /// The names a parameter list binds.
    fn params_names(params: &FormalParameters<'_>, out: &mut Vec<String>) {
        for param in &params.items {
            Self::pattern_names(&param.pattern, out);
        }
        if let Some(rest) = &params.rest {
            Self::pattern_names(&rest.rest.argument, out);
        }
    }

    /// The names a variable declaration binds.
    fn declaration_names(declaration: &VariableDeclaration<'_>, out: &mut Vec<String>) {
        for declarator in &declaration.declarations {
            Self::pattern_names(&declarator.id, out);
        }
    }

    /// The names a declaration statement directly in a block binds in that
    /// block: `let`/`const`/`using`, a class, or (block-scoped in strict code)
    /// a function declaration.
    fn lexical_names(statement: &Statement<'_>, out: &mut Vec<String>) {
        match statement {
            Statement::VariableDeclaration(declaration) if declaration.kind != VariableDeclarationKind::Var => {
                Self::declaration_names(declaration, out);
            }
            Statement::FunctionDeclaration(function) => out.extend(function.id.as_ref().map(|id| id.name.to_string())),
            Statement::ClassDeclaration(class) => out.extend(class.id.as_ref().map(|id| id.name.to_string())),
            _ => {}
        }
    }

    /// The names a function body binds for the whole function: any
    /// declaration directly in it, or a `var` anywhere in its blocks (a `var`
    /// is function-scoped, so one inside an `if` still shadows).
    fn function_scope_names(statements: &[Statement<'_>], out: &mut Vec<String>) {
        for statement in statements {
            Self::lexical_names(statement, out);
            Self::var_names(statement, out);
        }
    }

    /// The `var` names `statement` holds, looking through nested blocks but
    /// not into nested functions.
    fn var_names(statement: &Statement<'_>, out: &mut Vec<String>) {
        match statement {
            Statement::VariableDeclaration(declaration) if declaration.kind == VariableDeclarationKind::Var => {
                Self::declaration_names(declaration, out);
            }
            Statement::BlockStatement(block) => block.body.iter().for_each(|inner| Self::var_names(inner, out)),
            Statement::IfStatement(statement) => {
                Self::var_names(&statement.consequent, out);
                if let Some(alternate) = &statement.alternate {
                    Self::var_names(alternate, out);
                }
            }
            Statement::ForStatement(statement) => {
                if let Some(ForStatementInit::VariableDeclaration(declaration)) = &statement.init
                    && declaration.kind == VariableDeclarationKind::Var
                {
                    Self::declaration_names(declaration, out);
                }
                Self::var_names(&statement.body, out);
            }
            Statement::ForInStatement(statement) => {
                Self::for_left_names(&statement.left, true, out);
                Self::var_names(&statement.body, out);
            }
            Statement::ForOfStatement(statement) => {
                Self::for_left_names(&statement.left, true, out);
                Self::var_names(&statement.body, out);
            }
            Statement::WhileStatement(statement) => Self::var_names(&statement.body, out),
            Statement::DoWhileStatement(statement) => Self::var_names(&statement.body, out),
            Statement::LabeledStatement(statement) => Self::var_names(&statement.body, out),
            Statement::TryStatement(statement) => {
                statement.block.body.iter().for_each(|inner| Self::var_names(inner, out));
                if let Some(handler) = &statement.handler {
                    handler.body.body.iter().for_each(|inner| Self::var_names(inner, out));
                }
                if let Some(finalizer) = &statement.finalizer {
                    finalizer.body.iter().for_each(|inner| Self::var_names(inner, out));
                }
            }
            Statement::SwitchStatement(statement) => {
                for case in &statement.cases {
                    case.consequent.iter().for_each(|inner| Self::var_names(inner, out));
                }
            }
            _ => {}
        }
    }

    /// The names a `for (.. in/of ..)` head binds: `var` ones when `var` is
    /// set, lexical ones otherwise.
    fn for_left_names(left: &ForStatementLeft<'_>, var: bool, out: &mut Vec<String>) {
        if let ForStatementLeft::VariableDeclaration(declaration) = left
            && (declaration.kind == VariableDeclarationKind::Var) == var
        {
            Self::declaration_names(declaration, out);
        }
    }
}

impl<'a> Visit<'a> for ScopedUses<'_, '_> {
    /// A reference to an asked-about name that reached here unshadowed.
    fn visit_identifier_reference(&mut self, identifier: &IdentifierReference<'a>) {
        if self.visible(identifier.name.as_str()) {
            self.uses.referenced.insert(identifier.name.to_string());
        }
    }

    /// `new name(..)` constructs the binding.
    fn visit_new_expression(&mut self, expression: &NewExpression<'a>) {
        if let Some(name) = self.visible_name(&expression.callee) {
            self.uses.constructed.insert(name);
        }
        walk::walk_new_expression(self, expression);
    }

    /// `name.prototype.member = ..` extends the binding as a constructor.
    fn visit_assignment_expression(&mut self, expression: &AssignmentExpression<'a>) {
        if let AssignmentTarget::StaticMemberExpression(member) = &expression.left
            && let Expression::StaticMemberExpression(prototype) = &member.object
            && prototype.property.name == "prototype"
            && let Some(name) = self.visible_name(&prototype.object)
        {
            self.uses.constructed.insert(name);
        }
        walk::walk_assignment_expression(self, expression);
    }

    /// A function scope shadows its own name (for an expression), its
    /// parameters and its body declarations.
    fn visit_function(&mut self, function: &Function<'a>, flags: ScopeFlags) {
        let mut bound = Vec::new();
        if function.is_expression() {
            bound.extend(function.id.as_ref().map(|id| id.name.to_string()));
        }
        Self::params_names(&function.params, &mut bound);
        if let Some(body) = &function.body {
            Self::function_scope_names(&body.statements, &mut bound);
        }
        self.within_scope(bound, |this| walk::walk_function(this, function, flags));
    }

    /// An arrow shadows its parameters and body declarations.
    fn visit_arrow_function_expression(&mut self, arrow: &ArrowFunctionExpression<'a>) {
        let mut bound = Vec::new();
        Self::params_names(&arrow.params, &mut bound);
        Self::function_scope_names(crate::lowering::support::arrow_block_statements(arrow), &mut bound);
        self.within_scope(bound, |this| walk::walk_arrow_function_expression(this, arrow));
    }

    /// A block shadows its own lexical declarations.
    fn visit_block_statement(&mut self, block: &BlockStatement<'a>) {
        let mut bound = Vec::new();
        block.body.iter().for_each(|statement| Self::lexical_names(statement, &mut bound));
        self.within_scope(bound, |this| walk::walk_block_statement(this, block));
    }

    /// A static block is a function scope of its own.
    fn visit_static_block(&mut self, block: &StaticBlock<'a>) {
        let mut bound = Vec::new();
        Self::function_scope_names(&block.body, &mut bound);
        self.within_scope(bound, |this| walk::walk_static_block(this, block));
    }

    /// The cases of a `switch` share one block scope; the discriminant is
    /// evaluated outside it.
    fn visit_switch_statement(&mut self, statement: &SwitchStatement<'a>) {
        self.visit_expression(&statement.discriminant);
        let mut bound = Vec::new();
        for case in &statement.cases {
            case.consequent.iter().for_each(|inner| Self::lexical_names(inner, &mut bound));
        }
        self.within_scope(bound, |this| {
            for case in &statement.cases {
                this.visit_switch_case(case);
            }
        });
    }

    /// A `catch (name)` parameter shadows `name` for the handler.
    fn visit_catch_clause(&mut self, clause: &CatchClause<'a>) {
        let mut bound = Vec::new();
        if let Some(param) = &clause.param {
            Self::pattern_names(&param.pattern, &mut bound);
        }
        self.within_scope(bound, |this| walk::walk_catch_clause(this, clause));
    }

    /// A `for (let name ..)` head shadows `name` for the whole loop.
    fn visit_for_statement(&mut self, statement: &ForStatement<'a>) {
        let mut bound = Vec::new();
        if let Some(ForStatementInit::VariableDeclaration(declaration)) = &statement.init
            && declaration.kind != VariableDeclarationKind::Var
        {
            Self::declaration_names(declaration, &mut bound);
        }
        self.within_scope(bound, |this| walk::walk_for_statement(this, statement));
    }

    /// A `for (let name in ..)` head shadows `name` for the loop; the
    /// iterated object is evaluated outside it.
    fn visit_for_in_statement(&mut self, statement: &ForInStatement<'a>) {
        self.visit_expression(&statement.right);
        let mut bound = Vec::new();
        Self::for_left_names(&statement.left, false, &mut bound);
        self.within_scope(bound, |this| {
            this.visit_for_statement_left(&statement.left);
            this.visit_statement(&statement.body);
        });
    }

    /// A `for (let name of ..)` head shadows `name` for the loop; the
    /// iterated value is evaluated outside it.
    fn visit_for_of_statement(&mut self, statement: &ForOfStatement<'a>) {
        self.visit_expression(&statement.right);
        let mut bound = Vec::new();
        Self::for_left_names(&statement.left, false, &mut bound);
        self.within_scope(bound, |this| {
            this.visit_for_statement_left(&statement.left);
            this.visit_statement(&statement.body);
        });
    }

    /// A class expression named `name` binds it inside its own body.
    fn visit_class(&mut self, class: &Class<'a>) {
        let mut bound = Vec::new();
        if class.is_expression() {
            bound.extend(class.id.as_ref().map(|id| id.name.to_string()));
        }
        self.within_scope(bound, |this| walk::walk_class(this, class));
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
        assert!(!uses("const ok = v instanceof Foo", "Foo").constructed);
        assert!(!uses("const made = new (Foo as any)()", "Foo").constructed);
        assert!(!uses("const f = (Foo: any) => new Foo()", "Foo").constructed);
        assert!(!uses("Foo()", "Foo").constructed);
    }
}
