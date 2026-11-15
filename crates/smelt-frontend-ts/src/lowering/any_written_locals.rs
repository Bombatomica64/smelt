//! Locals whose elements the program writes with an `any` value.
//!
//! TypeScript does not check a value typed `any`: assigning one to a slot of
//! any type compiles, and the slot then holds whatever the value is at run
//! time. ts-pattern's matchers collect their selections this way:
//!
//! ```ts
//! let selections: Record<string, unknown[]> = {};
//! const selector = (key: string, value: any) => {
//!   selections[key] = value;
//! };
//! ```
//!
//! and pass `undefined` or a single selected value as `value`. Lowered at the
//! declared `Record<string, unknown[]>`, each write converted the value to a
//! list: `undefined` became `[]` and a scalar panicked ("not iterable"), so a
//! `P.optional` selection that should be `undefined` read `[]`, and an
//! intersection selection crashed. The declaration names a type the program
//! does not keep; a hand port declares the slot as the dynamic value it really
//! stores, and that is what these locals get: a local whose element is
//! written with an `any` value holds erased elements (`Record<string,
//! unknown>`, `unknown[]`). The `any` the source wrote is the dynamic boundary.
//!
//! A function that RETURNS such a local by name (`return selections`) returns
//! the same dynamic value, so its declared return type widens the same way
//! ([`AnyWritten::returning_functions`]); otherwise the value would be
//! converted back to the narrower element at the `return`, which is where it
//! failed. A value that leaves through any other declared type (a field
//! assignment, an argument) is still converted at that boundary.
//!
//! The scan is syntactic: an `any` value is an identifier whose innermost
//! declaration (parameter or variable) is annotated `any`, or an `as any`
//! assertion. Names resolve through the enclosing functions innermost first,
//! so a closure's write widens the outer function's local it captures.

use std::collections::{HashMap, HashSet};

use oxc::ast::ast::{
    AssignmentExpression, AssignmentOperator, AssignmentTarget, BindingPattern, Expression,
    Program, TSType, VariableDeclarator,
};
use oxc::ast_visit::Visit;

/// What the scan found (see the module docs).
#[derive(Default)]
pub(super) struct AnyWritten {
    /// Binding-identifier span starts of every local whose elements are
    /// written with an `any` value.
    pub(super) bindings: HashSet<u32>,
    /// Span starts of the functions that return one of those locals by name.
    pub(super) returning_functions: HashSet<u32>,
}

/// Scan `program` for any-written locals and the functions returning them.
pub(super) fn any_written_bindings(program: &Program<'_>) -> AnyWritten {
    let mut collector = AnyWrittenCollector {
        frames: vec![HashMap::new()],
        functions: vec![u32::MAX],
        widened: HashSet::new(),
        returns: Vec::new(),
    };
    collector.visit_program(program);
    let returning_functions = collector
        .returns
        .iter()
        .filter(|(_, binding)| collector.widened.contains(binding))
        .map(|(function, _)| *function)
        .collect();
    AnyWritten {
        bindings: collector.widened,
        returning_functions,
    }
}

/// One declared name: its binding span start and whether it is annotated `any`.
#[derive(Clone, Copy)]
struct Declared {
    /// Binding-identifier span start.
    span: u32,
    /// Whether the declaration's annotation is `any`.
    is_any: bool,
}

/// AST visitor collecting [`any_written_bindings`].
struct AnyWrittenCollector {
    /// Declarations per enclosing function, innermost last.
    frames: Vec<HashMap<String, Declared>>,
    /// Span start of each enclosing function, innermost last (`u32::MAX` for
    /// the program body).
    functions: Vec<u32>,
    /// Accumulated widened bindings.
    widened: HashSet<u32>,
    /// `(function span start, binding span start)` of each `return local`.
    returns: Vec<(u32, u32)>,
}

impl AnyWrittenCollector {
    /// The innermost declaration of `name`.
    fn resolve(&self, name: &str) -> Option<Declared> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame.get(name).copied())
    }

    /// Record a declaration in the innermost function.
    fn declare(&mut self, pattern: &BindingPattern<'_>, annotation: Option<&TSType<'_>>) {
        let BindingPattern::BindingIdentifier(binding) = pattern else {
            return;
        };
        if let Some(frame) = self.frames.last_mut() {
            frame.insert(
                binding.name.to_string(),
                Declared {
                    span: binding.span.start,
                    is_any: matches!(annotation, Some(TSType::TSAnyKeyword(_))),
                },
            );
        }
    }

    /// Record the parameters of the function just entered.
    fn declare_params(&mut self, params: &oxc::ast::ast::FormalParameters<'_>) {
        for param in &params.items {
            let annotation = param
                .type_annotation
                .as_ref()
                .map(|annotation| &annotation.type_annotation);
            self.declare(&param.pattern, annotation);
        }
    }

    /// Whether `expression` is a value typed `any` (see the module docs).
    fn is_any_value(&self, expression: &Expression<'_>) -> bool {
        match expression.without_parentheses() {
            Expression::Identifier(identifier) => self
                .resolve(identifier.name.as_str())
                .is_some_and(|declared| declared.is_any),
            Expression::TSAsExpression(assertion) => {
                matches!(assertion.type_annotation, TSType::TSAnyKeyword(_))
            }
            _ => false,
        }
    }
}

impl<'a> Visit<'a> for AnyWrittenCollector {
    /// Visits a function with its own parameter scope.
    fn visit_function(
        &mut self,
        function: &oxc::ast::ast::Function<'a>,
        flags: oxc::syntax::scope::ScopeFlags,
    ) {
        self.frames.push(HashMap::new());
        self.functions.push(function.span.start);
        self.declare_params(&function.params);
        oxc::ast_visit::walk::walk_function(self, function, flags);
        self.functions.pop();
        self.frames.pop();
    }

    /// Visits an arrow function with its own parameter scope.
    fn visit_arrow_function_expression(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'a>,
    ) {
        self.frames.push(HashMap::new());
        self.functions.push(arrow.span.start);
        self.declare_params(&arrow.params);
        oxc::ast_visit::walk::walk_arrow_function_expression(self, arrow);
        self.functions.pop();
        self.frames.pop();
    }

    /// Visits a variable declarator to record its name and annotation.
    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        let annotation = declarator
            .type_annotation
            .as_ref()
            .map(|annotation| &annotation.type_annotation);
        self.declare(&declarator.id, annotation);
        oxc::ast_visit::walk::walk_variable_declarator(self, declarator);
    }

    /// Visits a `return local` to remember which function returns which local.
    fn visit_return_statement(&mut self, statement: &oxc::ast::ast::ReturnStatement<'a>) {
        if let Some(Expression::Identifier(identifier)) = statement
            .argument
            .as_ref()
            .map(Expression::without_parentheses)
            && let Some(declared) = self.resolve(identifier.name.as_str())
            && let Some(function) = self.functions.last().copied()
        {
            self.returns.push((function, declared.span));
        }
        oxc::ast_visit::walk::walk_return_statement(self, statement);
    }

    /// Visits an assignment to record `local[key] = anyValue`.
    fn visit_assignment_expression(&mut self, assign: &AssignmentExpression<'a>) {
        if assign.operator == AssignmentOperator::Assign
            && let AssignmentTarget::ComputedMemberExpression(member) = &assign.left
            && let Expression::Identifier(target) = member.object.without_parentheses()
            && self.is_any_value(&assign.right)
            && let Some(declared) = self.resolve(target.name.as_str())
        {
            self.widened.insert(declared.span);
        }
        oxc::ast_visit::walk::walk_assignment_expression(self, assign);
    }
}
