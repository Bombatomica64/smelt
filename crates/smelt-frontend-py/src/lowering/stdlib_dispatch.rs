//! Python standard-library dispatch against shared rule metadata.

use ruff_python_ast::{Expr, ExprCall};
use smelt_stdlib::RuleId;

/// Return the shared stdlib rule of a function, by its fully qualified name.
#[must_use]
pub(super) fn call_rule(qualified: &str) -> Option<RuleId> {
    match qualified {
        "json.dumps" => Some(RuleId::PyJsonDumps),
        "json.loads" => Some(RuleId::PyJsonLoads),
        "re.search" => Some(RuleId::PyReSearch),
        "re.match" => Some(RuleId::PyReMatch),
        "re.fullmatch" => Some(RuleId::PyReFullMatch),
        "random.random" => Some(RuleId::PyRandomRandom),
        "random.randint" => Some(RuleId::PyRandomRandInt),
        "random.choice" => Some(RuleId::PyRandomChoice),
        "requests.get" => Some(RuleId::PyRequestsGet),
        "datetime.datetime.now" | "datetime.datetime.utcnow" => Some(RuleId::PyDateTimeNow),
        "datetime.datetime.fromtimestamp" => Some(RuleId::PyDateTimeFromTimestamp),
        "urllib.parse.urlparse" => Some(RuleId::PyUrlparseField),
        _ => None,
    }
}

/// Return the callee name of a direct `NAME(...)` call for a module-level constructed constant.
///
/// Callers check the returned name against the module's class items.
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
