//! Parameters of functions defined in an object literal cast `as any`.
//!
//! Such an object's members reach their callers only through `any`: TypeScript
//! checks no call of them, so a caller may pass any value, and the function
//! receives it as JavaScript does. ts-pattern builds its string matcher this way:
//!
//! ```ts
//! const stringChainable = (pattern) =>
//!   Object.assign(chainable(pattern), {
//!     regex: (str: string) => stringChainable(intersection(pattern, regex(str))),
//!   }) as any;
//! ```
//!
//! and `P.string.regex(/[a-z]+/)` hands `str` a `RegExp`. Lowered at the
//! annotated `string`, the argument was converted with `ToString` ("[object
//! Object]" for the erased regex object), and the pattern never matched. The
//! annotation describes nothing a caller is held to: a hand port takes the
//! dynamic value such a parameter really receives, and that is what these
//! parameters get (`ErasureReason::ExplicitAny`, the author's `as any`).
//!
//! The scan is syntactic. From the operand of each `as any` it follows
//! parentheses, assertions, conditionals, call arguments (`Object.assign(t,
//! {..})` returns its arguments' members) and spreads to the object literals
//! whose members reach the `any` value, and records the parameters of the
//! functions they define (arrow, function expression and method values),
//! nested object literals included. A function body is not entered: an object
//! built and used inside it is typed as usual.

use std::collections::HashSet;

use oxc::ast::ast::{
    Argument, Expression, FormalParameters, ObjectPropertyKind, Program, TSAsExpression, TSType,
};
use oxc::ast_visit::Visit;

/// Span starts of every parameter the source leaves unchecked (see the module docs).
pub(super) fn unchecked_params(program: &Program<'_>) -> HashSet<u32> {
    let mut collector = UncheckedParamCollector {
        params: HashSet::new(),
    };
    collector.visit_program(program);
    collector.params
}

/// AST visitor collecting [`unchecked_params`].
struct UncheckedParamCollector {
    /// Accumulated parameter span starts.
    params: HashSet<u32>,
}

impl UncheckedParamCollector {
    /// Record the parameters of one function value.
    fn record(&mut self, params: &FormalParameters<'_>) {
        for param in &params.items {
            self.params.insert(param.span.start);
        }
        if let Some(rest) = &params.rest {
            self.params.insert(rest.span.start);
        }
    }

    /// Follow `expression`, a value whose members reach an `any`, to the
    /// object literals it is built from.
    fn reach(&mut self, expression: &Expression<'_>) {
        match expression.without_parentheses() {
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(property) => {
                            self.member_value(&property.value);
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            self.reach(&spread.argument);
                        }
                    }
                }
            }
            Expression::CallExpression(call) => {
                for argument in &call.arguments {
                    if let Some(argument) = argument_expression(argument) {
                        self.reach(argument);
                    }
                }
            }
            Expression::ConditionalExpression(conditional) => {
                self.reach(&conditional.consequent);
                self.reach(&conditional.alternate);
            }
            Expression::TSAsExpression(assertion) => self.reach(&assertion.expression),
            Expression::TSSatisfiesExpression(satisfies) => self.reach(&satisfies.expression),
            Expression::TSNonNullExpression(non_null) => self.reach(&non_null.expression),
            Expression::TSTypeAssertion(assertion) => self.reach(&assertion.expression),
            _ => {}
        }
    }

    /// A member value of an object reaching an `any`: a function's parameters
    /// are unchecked, and a nested object's members reach it too.
    fn member_value(&mut self, value: &Expression<'_>) {
        match value.without_parentheses() {
            Expression::ArrowFunctionExpression(arrow) => self.record(&arrow.params),
            Expression::FunctionExpression(function) => self.record(&function.params),
            other => self.reach(other),
        }
    }
}

/// The expression of a call argument (a spread's operand included).
fn argument_expression<'a, 'b>(argument: &'b Argument<'a>) -> Option<&'b Expression<'a>> {
    match argument {
        Argument::SpreadElement(spread) => Some(&spread.argument),
        other => other.as_expression(),
    }
}

impl<'a> Visit<'a> for UncheckedParamCollector {
    /// Visits an assertion to start a scan from an `as any` operand.
    fn visit_ts_as_expression(&mut self, assertion: &TSAsExpression<'a>) {
        if matches!(assertion.type_annotation, TSType::TSAnyKeyword(_)) {
            self.reach(&assertion.expression);
        }
        oxc::ast_visit::walk::walk_ts_as_expression(self, assertion);
    }
}
