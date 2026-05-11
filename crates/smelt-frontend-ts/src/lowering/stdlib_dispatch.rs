//! TypeScript standard-library dispatch against shared rule metadata.

use oxc::ast::ast::{CallExpression, Expression};
use smelt_stdlib::RuleId;

/// Pure `Math.*` calls that can be folded while reading exported constants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PureMathCall {
    /// `Math.abs(value)`.
    Abs,
    /// `Math.floor(value)`.
    Floor,
    /// `Math.ceil(value)`.
    Ceil,
    /// `Math.round(value)`.
    Round,
    /// `Math.trunc(value)`.
    Trunc,
    /// `Math.max(...values)`.
    Max,
    /// `Math.min(...values)`.
    Min,
    /// `Math.hypot(...values)`.
    Hypot,
    /// `Math.sqrt(value)`.
    Sqrt,
    /// `Math.cbrt(value)`.
    Cbrt,
    /// `Math.sign(value)`.
    Sign,
    /// `Math.sin(value)`.
    Sin,
    /// `Math.cos(value)`.
    Cos,
    /// `Math.tan(value)`.
    Tan,
    /// `Math.asin(value)`.
    Asin,
    /// `Math.acos(value)`.
    Acos,
    /// `Math.atan(value)`.
    Atan,
    /// `Math.log(value)`.
    Log,
    /// `Math.log10(value)`.
    Log10,
    /// `Math.log2(value)`.
    Log2,
    /// `Math.exp(value)`.
    Exp,
    /// `Math.pow(base, exponent)`.
    Pow,
    /// `Math.atan2(y, x)`.
    Atan2,
}

/// Return the shared stdlib rule matching a TypeScript call expression.
#[must_use]
pub(super) fn call_rule(call: &CallExpression<'_>) -> Option<RuleId> {
    match &call.callee {
        Expression::Identifier(callee) if callee.name == "fetch" => Some(RuleId::TsFetch),
        Expression::StaticMemberExpression(member) => static_member_rule(member),
        _ => None,
    }
}

/// Return the pure Math operation for a static `Math.*` call, excluding non-foldable APIs.
#[must_use]
pub(super) fn pure_math_call(call: &CallExpression<'_>) -> Option<PureMathCall> {
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    let Expression::Identifier(object) = &member.object else {
        return None;
    };
    if object.name != "Math" {
        return None;
    }
    match member.property.name.as_str() {
        "abs" => Some(PureMathCall::Abs),
        "floor" => Some(PureMathCall::Floor),
        "ceil" => Some(PureMathCall::Ceil),
        "round" => Some(PureMathCall::Round),
        "trunc" => Some(PureMathCall::Trunc),
        "max" => Some(PureMathCall::Max),
        "min" => Some(PureMathCall::Min),
        "hypot" => Some(PureMathCall::Hypot),
        "sqrt" => Some(PureMathCall::Sqrt),
        "cbrt" => Some(PureMathCall::Cbrt),
        "sign" => Some(PureMathCall::Sign),
        "sin" => Some(PureMathCall::Sin),
        "cos" => Some(PureMathCall::Cos),
        "tan" => Some(PureMathCall::Tan),
        "asin" => Some(PureMathCall::Asin),
        "acos" => Some(PureMathCall::Acos),
        "atan" => Some(PureMathCall::Atan),
        "log" => Some(PureMathCall::Log),
        "log10" => Some(PureMathCall::Log10),
        "log2" => Some(PureMathCall::Log2),
        "exp" => Some(PureMathCall::Exp),
        "pow" => Some(PureMathCall::Pow),
        "atan2" => Some(PureMathCall::Atan2),
        _ => None,
    }
}

/// Return the rule matching a static member call.
fn static_member_rule(member: &oxc::ast::ast::StaticMemberExpression<'_>) -> Option<RuleId> {
    let property = member.property.name.as_str();
    match &member.object {
        Expression::Identifier(object) if object.name == "JSON" && property == "stringify" => {
            Some(RuleId::TsJsonStringify)
        }
        Expression::Identifier(object) if object.name == "JSON" && property == "parse" => {
            Some(RuleId::TsJsonParse)
        }
        Expression::Identifier(object) if object.name == "Math" && property == "random" => {
            Some(RuleId::TsMathRandom)
        }
        Expression::NewExpression(new_expr) if property == "test" => {
            let Expression::Identifier(callee) = &new_expr.callee else {
                return None;
            };
            (callee.name == "RegExp").then_some(RuleId::TsRegExpTest)
        }
        _ => None,
    }
}
