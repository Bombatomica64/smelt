//! Python standard-library dispatch against shared rule metadata.

use ruff_python_ast::{Expr, ExprCall};
use smelt_stdlib::RuleId;

/// Return the shared stdlib rule matching a Python call expression.
#[must_use]
pub(super) fn call_rule(call: &ExprCall) -> Option<RuleId> {
    let Expr::Attribute(attr) = call.func.as_ref() else {
        return None;
    };
    let Expr::Name(module) = attr.value.as_ref() else {
        return None;
    };
    match (module.id.as_str(), attr.attr.as_str()) {
        ("json", "dumps") => Some(RuleId::PyJsonDumps),
        ("json", "loads") => Some(RuleId::PyJsonLoads),
        ("re", "search") => Some(RuleId::PyReSearch),
        ("re", "match") => Some(RuleId::PyReMatch),
        ("re", "fullmatch") => Some(RuleId::PyReFullMatch),
        ("random", "random") => Some(RuleId::PyRandomRandom),
        ("random", "randint") => Some(RuleId::PyRandomRandInt),
        ("random", "choice") => Some(RuleId::PyRandomChoice),
        ("requests", "get") => Some(RuleId::PyRequestsGet),
        _ => None,
    }
}

/// Return the shared rule for `datetime.datetime.*` calls.
#[must_use]
pub(super) fn datetime_rule(call: &ExprCall) -> Option<RuleId> {
    let Expr::Attribute(attr) = call.func.as_ref() else {
        return None;
    };
    let Expr::Attribute(class_attr) = attr.value.as_ref() else {
        return None;
    };
    if !matches!(class_attr.value.as_ref(), Expr::Name(module) if module.id.as_str() == "datetime")
        || class_attr.attr.as_str() != "datetime"
    {
        return None;
    }
    match attr.attr.as_str() {
        "now" | "utcnow" => Some(RuleId::PyDateTimeNow),
        "fromtimestamp" => Some(RuleId::PyDateTimeFromTimestamp),
        _ => None,
    }
}

/// Return the shared rule for `urllib.parse.urlparse(...)` field projections.
#[must_use]
pub(super) fn urlparse_field_rule(parse_call: &ExprCall) -> Option<RuleId> {
    let Expr::Attribute(parse_attr) = parse_call.func.as_ref() else {
        return None;
    };
    if parse_attr.attr.as_str() != "urlparse" {
        return None;
    }
    let Expr::Attribute(parse_module) = parse_attr.value.as_ref() else {
        return None;
    };
    if parse_module.attr.as_str() != "parse"
        || !matches!(parse_module.value.as_ref(), Expr::Name(module) if module.id.as_str() == "urllib")
    {
        return None;
    }
    Some(RuleId::PyUrlparseField)
}

/// Return the known class constructor name for a module-level constructed constant.
///
/// This intentionally accepts only direct `NAME = ClassName(...)` shapes.  The
/// frontend uses the result as importable package metadata, not as general
/// Python constant evaluation.
#[must_use]
pub(super) fn constructed_constant_constructor(call: &ExprCall) -> Option<&str> {
    let Expr::Name(name) = call.func.as_ref() else {
        return None;
    };
    Some(name.id.as_str())
}
