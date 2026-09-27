//! Locals that hold a keyed read the program itself tests for presence.
//!
//! Without `noUncheckedIndexedAccess`, TypeScript types `record[key]` as the
//! record's value type even though a missing key reads `undefined`. Code that
//! knows better says so with a guard on the local it stored the read in:
//!
//! ```ts
//! let nextNode: Node
//! nextNode = node.children[token]
//! if (!nextNode) {
//!   nextNode = node.children[token] = new Node()
//! }
//! ```
//!
//! (Hono's reg-exp router `Node#insert`.) Lowered at its declared type, the
//! local cannot hold the absent value: the read was unwrapped on assignment
//! (a panic for a missing key) and `!nextNode` folded to `false` because a
//! class value is always truthy. A hand-written Rust port would hold an
//! `Option<Node>` there, and that is what these locals get: a binding that is
//! both ASSIGNED a keyed read and TRUTHINESS-TESTED in the same function is
//! widened to `T | undefined`, so the read keeps its absence and the guard is
//! a real presence test. Every later use at the declared type goes through the
//! ordinary optional-to-required coercion, exactly as for a source-spelled
//! `let nextNode: Node | undefined`.
//!
//! The scan is purely syntactic and conservative: it attributes a use to the
//! innermost enclosing function, so a guard inside a nested closure does not
//! widen the outer binding (that binding keeps its declared type, as before).

use std::collections::{HashMap, HashSet};

use oxc::ast::ast::{
    AssignmentExpression, AssignmentTarget, BinaryExpression, BindingPattern, ConditionalExpression, Expression,
    IfStatement, LogicalExpression, Program, UnaryExpression, UnaryOperator,
    VariableDeclarator, WhileStatement,
};
use oxc::ast_visit::Visit;

/// Binding-identifier span starts of every local that should be widened to an
/// optional type (see the module docs).
pub(super) fn presence_tested_keyed_bindings(program: &Program<'_>) -> HashSet<u32> {
    let mut collector = PresenceTestedLocalCollector {
        frames: vec![Frame::default()],
        widened: HashSet::new(),
    };
    collector.visit_program(program);
    collector.pop_frame();
    collector.widened
}

/// Per-function facts gathered while walking one function body.
#[derive(Default)]
struct Frame {
    /// Binding span starts of each name declared in this function.
    declarations: HashMap<String, Vec<u32>>,
    /// Names assigned (or initialized) from a keyed read in this function.
    keyed: HashSet<String>,
    /// Names whose truthiness this function tests.
    tested: HashSet<String>,
}

/// AST visitor collecting [`presence_tested_keyed_bindings`].
struct PresenceTestedLocalCollector {
    /// Innermost function last; the program body is the bottom frame.
    frames: Vec<Frame>,
    /// Accumulated result.
    widened: HashSet<u32>,
}

impl PresenceTestedLocalCollector {
    /// Close the innermost function and record its widened bindings.
    fn pop_frame(&mut self) {
        let Some(frame) = self.frames.pop() else {
            return;
        };
        for name in frame.keyed.intersection(&frame.tested) {
            if let Some(spans) = frame.declarations.get(name) {
                self.widened.extend(spans.iter().copied());
            }
        }
    }

    /// The innermost function's facts (the program frame is always present
    /// while walking; a missing one records nothing).
    fn frame(&mut self) -> Option<&mut Frame> {
        self.frames.last_mut()
    }

    /// Record `expression` as a truthiness test when it is a bare identifier.
    fn record_test(&mut self, expression: &Expression<'_>) {
        if let Expression::Identifier(identifier) = expression.without_parentheses() {
            if let Some(frame) = self.frame() {
                frame.tested.insert(identifier.name.to_string());
            }
        }
    }
}

/// Return whether `expression` spells `null` or `undefined`.
fn is_nullish_literal(expression: &Expression<'_>) -> bool {
    match expression.without_parentheses() {
        Expression::NullLiteral(_) => true,
        Expression::Identifier(identifier) => identifier.name == "undefined",
        _ => false,
    }
}

/// Return whether `expression` reads a value by computed key (`a[k]`).
fn is_keyed_read(expression: &Expression<'_>) -> bool {
    matches!(
        expression.without_parentheses(),
        Expression::ComputedMemberExpression(_)
    )
}

impl<'a> Visit<'a> for PresenceTestedLocalCollector {
    fn visit_function(
        &mut self,
        function: &oxc::ast::ast::Function<'a>,
        flags: oxc::syntax::scope::ScopeFlags,
    ) {
        self.frames.push(Frame::default());
        oxc::ast_visit::walk::walk_function(self, function, flags);
        self.pop_frame();
    }

    fn visit_arrow_function_expression(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'a>,
    ) {
        self.frames.push(Frame::default());
        oxc::ast_visit::walk::walk_arrow_function_expression(self, arrow);
        self.pop_frame();
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        if let BindingPattern::BindingIdentifier(binding) = &declarator.id {
            let name = binding.name.to_string();
            let keyed = declarator.init.as_ref().is_some_and(is_keyed_read);
            if let Some(frame) = self.frame() {
                frame
                    .declarations
                    .entry(name.clone())
                    .or_default()
                    .push(binding.span.start);
                if keyed {
                    frame.keyed.insert(name);
                }
            }
        }
        oxc::ast_visit::walk::walk_variable_declarator(self, declarator);
    }

    fn visit_assignment_expression(&mut self, assign: &AssignmentExpression<'a>) {
        if let AssignmentTarget::AssignmentTargetIdentifier(identifier) = &assign.left
            && is_keyed_read(&assign.right)
        {
            if let Some(frame) = self.frame() {
                frame.keyed.insert(identifier.name.to_string());
            }
        }
        oxc::ast_visit::walk::walk_assignment_expression(self, assign);
    }

    fn visit_unary_expression(&mut self, unary: &UnaryExpression<'a>) {
        if unary.operator == UnaryOperator::LogicalNot {
            self.record_test(&unary.argument);
        }
        oxc::ast_visit::walk::walk_unary_expression(self, unary);
    }

    fn visit_if_statement(&mut self, statement: &IfStatement<'a>) {
        self.record_test(&statement.test);
        oxc::ast_visit::walk::walk_if_statement(self, statement);
    }

    fn visit_while_statement(&mut self, statement: &WhileStatement<'a>) {
        self.record_test(&statement.test);
        oxc::ast_visit::walk::walk_while_statement(self, statement);
    }

    fn visit_conditional_expression(&mut self, conditional: &ConditionalExpression<'a>) {
        self.record_test(&conditional.test);
        oxc::ast_visit::walk::walk_conditional_expression(self, conditional);
    }

    fn visit_logical_expression(&mut self, logical: &LogicalExpression<'a>) {
        self.record_test(&logical.left);
        oxc::ast_visit::walk::walk_logical_expression(self, logical);
    }

    fn visit_binary_expression(&mut self, binary: &BinaryExpression<'a>) {
        // `x === undefined`, `x == null`, and their negations test presence
        // just as `!x` does.
        if binary.operator.is_equality() {
            if is_nullish_literal(&binary.right) {
                self.record_test(&binary.left);
            } else if is_nullish_literal(&binary.left) {
                self.record_test(&binary.right);
            }
        }
        oxc::ast_visit::walk::walk_binary_expression(self, binary);
    }
}
