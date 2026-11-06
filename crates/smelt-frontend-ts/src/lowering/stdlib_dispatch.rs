//! TypeScript standard-library dispatch against shared rule metadata.

use oxc::ast::ast::{CallExpression, Expression};
use smelt_hir::{NumericRoundOp, NumericUnaryFuncOp};
use smelt_stdlib::{MathFunction, RuleId};

/// The `NumericRound` operation of a rounding `Math` function.
#[must_use]
pub(super) const fn math_round_op(function: MathFunction) -> Option<NumericRoundOp> {
    match function {
        MathFunction::Floor => Some(NumericRoundOp::Floor),
        MathFunction::Ceil => Some(NumericRoundOp::Ceil),
        MathFunction::Round => Some(NumericRoundOp::Round),
        MathFunction::Trunc => Some(NumericRoundOp::Trunc),
        _ => None,
    }
}

/// The `NumericUnaryFunc` operation of a single-argument `Math` function.
#[must_use]
pub(super) const fn math_unary_op(function: MathFunction) -> Option<NumericUnaryFuncOp> {
    match function {
        MathFunction::Sqrt => Some(NumericUnaryFuncOp::Sqrt),
        MathFunction::Cbrt => Some(NumericUnaryFuncOp::Cbrt),
        MathFunction::Sign => Some(NumericUnaryFuncOp::Sign),
        MathFunction::Sin => Some(NumericUnaryFuncOp::Sin),
        MathFunction::Cos => Some(NumericUnaryFuncOp::Cos),
        MathFunction::Tan => Some(NumericUnaryFuncOp::Tan),
        MathFunction::Asin => Some(NumericUnaryFuncOp::Asin),
        MathFunction::Acos => Some(NumericUnaryFuncOp::Acos),
        MathFunction::Atan => Some(NumericUnaryFuncOp::Atan),
        MathFunction::Log => Some(NumericUnaryFuncOp::Log),
        MathFunction::Log10 => Some(NumericUnaryFuncOp::Log10),
        MathFunction::Log2 => Some(NumericUnaryFuncOp::Log2),
        MathFunction::Exp => Some(NumericUnaryFuncOp::Exp),
        MathFunction::Clz32 => Some(NumericUnaryFuncOp::Clz32),
        _ => None,
    }
}

/// Return the root identifier of a callee, when it has one.
///
/// `randomUUID` for a bare call, `crypto` for `crypto.randomUUID` and for
/// `crypto.subtle.digest` alike: shadowing a namespace takes its whole surface.
#[must_use]
pub(super) fn callee_root<'source>(callee: &'source Expression<'_>) -> Option<&'source str> {
    match callee {
        Expression::Identifier(identifier) => Some(identifier.name.as_str()),
        Expression::StaticMemberExpression(member) => callee_root(&member.object),
        _ => None,
    }
}

/// Return the shared stdlib rule matching a TypeScript call expression.
#[must_use]
pub(super) fn call_rule(call: &CallExpression<'_>) -> Option<RuleId> {
    match &call.callee {
        Expression::Identifier(callee) => smelt_stdlib::typescript_call_rule(None, &callee.name),
        Expression::StaticMemberExpression(member) => static_member_rule(member),
        _ => None,
    }
}

/// Return the dotted namespace path an expression spells, when it is one.
///
/// A namespace path is an identifier (`Math`), or a chain of static member
/// reads rooted at one (`crypto.subtle`). Anything else — a call, an index, an
/// optional chain, `this` — is not a namespace and answers `None`, so a
/// receiver that has to be *evaluated* can never be mistaken for a name in the
/// recognition table.
///
/// Computed and private members are excluded by construction:
/// [`Expression::StaticMemberExpression`] is only the `a.b` spelling, and
/// `a["b"]` and `a.#b` are different AST nodes.
fn namespace_path(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Identifier(identifier) => Some(identifier.name.as_str().to_owned()),
        Expression::StaticMemberExpression(member) => {
            let object = namespace_path(&member.object)?;
            Some(format!("{object}.{}", member.property.name.as_str()))
        }
        _ => None,
    }
}

/// Return the pure Math operation for a static `Math.*` call, excluding non-foldable APIs.
#[must_use]
pub(super) fn pure_math_call(call: &CallExpression<'_>) -> Option<MathFunction> {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    let Expression::Identifier(object) = &member.object else {
        return None;
    };
    if object.name != "Math" {
        return None;
    }
    smelt_stdlib::math_function(member.property.name.as_str()).filter(|function| {
        !matches!(function, MathFunction::Imul | MathFunction::Clz32 | MathFunction::Random)
    })
}

/// Return the rule matching a static member call.
fn static_member_rule(member: &oxc::ast::ast::StaticMemberExpression<'_>) -> Option<RuleId> {
    let property = member.property.name.as_str();
    // The receiver is the whole DOTTED namespace path, not just an identifier.
    // `Math.random()` and `crypto.subtle.digest(..)` are the same shape at
    // different depths, so the path is spelled once here and the recognition
    // table names the depth it wants (`"crypto.subtle"`). Resolving it here
    // rather than adding a nested-namespace shape to the table is what keeps a
    // second level from being a new kind of entry.
    if let Some(path) = namespace_path(&member.object)
        && let Some(rule) = smelt_stdlib::typescript_call_rule(Some(&path), property)
    {
        return Some(rule);
    }
    match &member.object {
        Expression::NewExpression(new_expr) if property == "test" => {
            let Expression::Identifier(callee) = &new_expr.callee else {
                return None;
            };
            (callee.name == "RegExp").then_some(RuleId::TsRegExpTest)
        }
        Expression::CallExpression(call_expr) if property == "test" => {
            let Expression::Identifier(callee) = &call_expr.callee else {
                return None;
            };
            (callee.name == "RegExp").then_some(RuleId::TsRegExpTest)
        }
        Expression::RegExpLiteral(_) if property == "test" => Some(RuleId::TsRegExpTest),
        Expression::NewExpression(new_expr) if property == "toISOString" => {
            let Expression::Identifier(callee) = &new_expr.callee else {
                return None;
            };
            (callee.name == "Date").then_some(RuleId::TsDateToIsoString)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use oxc::{allocator::Allocator, parser::Parser, span::SourceType};

    use super::*;

    /// Parse a single expression statement and return its recognized rule.
    fn parse_call_rule(source: &str) -> Option<RuleId> {
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, source, SourceType::ts()).parse();
        let Some(statement) = parsed.program.body.first() else {
            panic!("expected expression statement");
        };
        let oxc::ast::ast::Statement::ExpressionStatement(statement) = statement else {
            panic!("expected expression statement");
        };
        let Expression::CallExpression(call) = &statement.expression else {
            panic!("expected call expression");
        };
        call_rule(call)
    }

    /// Frontend AST recognition delegates exact calls to shared metadata.
    #[test]
    fn exact_call_rules_follow_shared_metadata() {
        let cases = [
            ("structuredClone(value);", RuleId::TsStructuredClone),
            ("Promise.all(values);", RuleId::TsPromiseStatic),
            ("Math.floor(value);", RuleId::TsMathNumeric),
            ("Number.parseInt(text);", RuleId::TsNumberParseInt),
            ("BigInt.asIntN(64, value);", RuleId::TsBigIntStatic),
            ("BigInt.asUintN(64, value);", RuleId::TsBigIntStatic),
            ("Object.fromEntries(entries);", RuleId::TsObjectStatic),
            ("Object.create(proto);", RuleId::TsObjectStatic),
            ("Array.isArray(value);", RuleId::TsArrayStatic),
        ];

        for (source, expected) in cases {
            assert_eq!(parse_call_rule(source), Some(expected));
        }
    }

    /// Similar unsupported AST call shapes remain outside exact metadata.
    #[test]
    fn similar_unsupported_calls_do_not_match() {
        for source in [
            "Promise.any(values);",
            "Math.randomBytes();",
            "Number.parseDouble(text);",
            "Object.entries(value);",
            "Array.of(value);",
            "value.map(callback);",
        ] {
            assert_eq!(parse_call_rule(source), None, "{source}");
        }
    }
}
