//! Seeing through source wrappers that carry no runtime meaning.
//!
//! Callers differ in which wrappers they may see through, and those differences
//! are load-bearing, so each caller names its set rather than writing its own loop.

use oxc::ast::ast::Expression;

/// The wrappers a peel sees through. Each set contains the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::lowering) enum Wrappers {
    /// Parentheses only.
    Parens,
    /// Parentheses, `as` and `satisfies`. A non-null assertion stays: it asserts
    /// something of the value rather than of the expression's type alone.
    Casts,
    /// Parentheses, `as`, `satisfies` and `!`.
    Transparent,
}

/// The expression under `wrappers`, or `expression` itself.
pub(in crate::lowering) fn peel<'a>(
    mut expression: &'a Expression<'a>,
    wrappers: Wrappers,
) -> &'a Expression<'a> {
    loop {
        expression = match expression {
            Expression::ParenthesizedExpression(parenthesized) => &parenthesized.expression,
            Expression::TSAsExpression(as_expr) if wrappers != Wrappers::Parens => {
                &as_expr.expression
            }
            Expression::TSSatisfiesExpression(satisfies) if wrappers != Wrappers::Parens => {
                &satisfies.expression
            }
            Expression::TSNonNullExpression(non_null) if wrappers == Wrappers::Transparent => {
                &non_null.expression
            }
            _ => return expression,
        };
    }
}
