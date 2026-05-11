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
