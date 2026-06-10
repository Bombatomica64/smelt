//! Exact source-call recognition metadata shared by frontends.

use crate::RuleId;

/// Exact syntactic call name that can be recognized without semantic inspection.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct CallRecognition {
    /// Static namespace receiver, or `None` for a global free function.
    pub receiver: Option<&'static str>,
    /// Function or static method name.
    pub member: &'static str,
    /// Shared rule selected by this exact source spelling.
    pub rule: RuleId,
}

/// TypeScript receiver category used when recognition depends on lowered type shape.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum TypeScriptReceiverKind {
    /// A source `Map<K, V>` value, represented internally as a dictionary.
    Map,
    /// A source `Set<T>` value.
    Set,
}

/// Receiver-method call shape recognized after a frontend knows the receiver type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct MethodRecognition {
    /// Receiver category selected by frontend type lowering.
    pub receiver: TypeScriptReceiverKind,
    /// Source method name.
    pub member: &'static str,
    /// Shared rule selected by this receiver/member pair.
    pub rule: RuleId,
}

/// Exact TypeScript call spellings that are safe to recognize before type lowering.
///
/// Receiver methods are deliberately absent because names such as `map`, `test`,
/// and `toString` require receiver types or source shapes to select a lowering.
pub const TYPESCRIPT_CALLS: &[CallRecognition] = &[
    free("fetch", RuleId::TsFetch),
    free("structuredClone", RuleId::TsStructuredClone),
    free("String", RuleId::TsPrimitiveCast),
    free("Number", RuleId::TsPrimitiveCast),
    free("Boolean", RuleId::TsPrimitiveCast),
    free("BigInt", RuleId::TsPrimitiveCast),
    free("parseFloat", RuleId::TsPrimitiveCast),
    free("parseInt", RuleId::TsPrimitiveCast),
    free("isNaN", RuleId::TsNumberPredicate),
    free("Symbol", RuleId::TsSymbol),
    static_call("JSON", "stringify", RuleId::TsJsonStringify),
    static_call("JSON", "parse", RuleId::TsJsonParse),
    static_call("Date", "now", RuleId::TsDateNow),
    static_call("Math", "random", RuleId::TsMathRandom),
    static_call("Math", "abs", RuleId::TsMathNumeric),
    static_call("Math", "floor", RuleId::TsMathNumeric),
    static_call("Math", "ceil", RuleId::TsMathNumeric),
    static_call("Math", "round", RuleId::TsMathNumeric),
    static_call("Math", "trunc", RuleId::TsMathNumeric),
    static_call("Math", "max", RuleId::TsMathNumeric),
    static_call("Math", "min", RuleId::TsMathNumeric),
    static_call("Math", "hypot", RuleId::TsMathNumeric),
    static_call("Math", "sqrt", RuleId::TsMathNumeric),
    static_call("Math", "cbrt", RuleId::TsMathNumeric),
    static_call("Math", "sign", RuleId::TsMathNumeric),
    static_call("Math", "sin", RuleId::TsMathNumeric),
    static_call("Math", "cos", RuleId::TsMathNumeric),
    static_call("Math", "tan", RuleId::TsMathNumeric),
    static_call("Math", "asin", RuleId::TsMathNumeric),
    static_call("Math", "acos", RuleId::TsMathNumeric),
    static_call("Math", "atan", RuleId::TsMathNumeric),
    static_call("Math", "log", RuleId::TsMathNumeric),
    static_call("Math", "log10", RuleId::TsMathNumeric),
    static_call("Math", "log2", RuleId::TsMathNumeric),
    static_call("Math", "exp", RuleId::TsMathNumeric),
    static_call("Math", "pow", RuleId::TsMathNumeric),
    static_call("Math", "atan2", RuleId::TsMathNumeric),
    static_call("Number", "isFinite", RuleId::TsNumberPredicate),
    static_call("Number", "isInteger", RuleId::TsNumberPredicate),
    static_call("Number", "isNaN", RuleId::TsNumberPredicate),
    static_call("Number", "parseFloat", RuleId::TsNumberParseFloat),
    static_call("Number", "parseInt", RuleId::TsNumberParseInt),
    static_call("Promise", "resolve", RuleId::TsPromiseStatic),
    static_call("Promise", "all", RuleId::TsPromiseStatic),
    static_call("Promise", "race", RuleId::TsPromiseStatic),
    static_call("Promise", "allSettled", RuleId::TsPromiseStatic),
    static_call("Symbol", "for", RuleId::TsSymbol),
    static_call("Object", "is", RuleId::TsObjectStatic),
    static_call("Object", "fromEntries", RuleId::TsObjectStatic),
    static_call("Object", "create", RuleId::TsObjectStatic),
    static_call("Object", "getPrototypeOf", RuleId::TsObjectStatic),
    static_call("Array", "isArray", RuleId::TsArrayStatic),
    static_call("Array", "from", RuleId::TsArrayStatic),
];

/// TypeScript receiver-method spellings keyed by known receiver type.
pub const TYPESCRIPT_METHODS: &[MethodRecognition] = &[
    method(TypeScriptReceiverKind::Map, "has", RuleId::TsMapHas),
    method(TypeScriptReceiverKind::Map, "get", RuleId::TsMapGet),
    method(TypeScriptReceiverKind::Map, "set", RuleId::TsMapMutation),
    method(TypeScriptReceiverKind::Map, "delete", RuleId::TsMapMutation),
    method(TypeScriptReceiverKind::Map, "clear", RuleId::TsMapMutation),
    method(TypeScriptReceiverKind::Map, "keys", RuleId::TsMapProjection),
    method(
        TypeScriptReceiverKind::Map,
        "values",
        RuleId::TsMapProjection,
    ),
    method(
        TypeScriptReceiverKind::Map,
        "entries",
        RuleId::TsMapProjection,
    ),
    method(TypeScriptReceiverKind::Set, "has", RuleId::TsSetHas),
    method(TypeScriptReceiverKind::Set, "add", RuleId::TsSetMutation),
    method(TypeScriptReceiverKind::Set, "delete", RuleId::TsSetMutation),
    method(TypeScriptReceiverKind::Set, "clear", RuleId::TsSetMutation),
    method(TypeScriptReceiverKind::Set, "keys", RuleId::TsSetProjection),
    method(
        TypeScriptReceiverKind::Set,
        "values",
        RuleId::TsSetProjection,
    ),
    method(
        TypeScriptReceiverKind::Set,
        "entries",
        RuleId::TsSetProjection,
    ),
];

/// Return the shared rule for an exact TypeScript global or static namespace call.
#[must_use]
pub fn typescript_call_rule(receiver: Option<&str>, member: &str) -> Option<RuleId> {
    TYPESCRIPT_CALLS
        .iter()
        .find(|entry| entry.receiver == receiver && entry.member == member)
        .map(|entry| entry.rule)
}

/// Return the shared rule for a TypeScript receiver-method call.
#[must_use]
pub fn typescript_method_rule(receiver: TypeScriptReceiverKind, member: &str) -> Option<RuleId> {
    TYPESCRIPT_METHODS
        .iter()
        .find(|entry| entry.receiver == receiver && entry.member == member)
        .map(|entry| entry.rule)
}

/// Build free-function recognition metadata.
const fn free(member: &'static str, rule: RuleId) -> CallRecognition {
    CallRecognition {
        receiver: None,
        member,
        rule,
    }
}

/// Build static namespace-call recognition metadata.
const fn static_call(
    receiver: &'static str,
    member: &'static str,
    rule: RuleId,
) -> CallRecognition {
    CallRecognition {
        receiver: Some(receiver),
        member,
        rule,
    }
}

/// Build receiver-method recognition metadata.
const fn method(
    receiver: TypeScriptReceiverKind,
    member: &'static str,
    rule: RuleId,
) -> MethodRecognition {
    MethodRecognition {
        receiver,
        member,
        rule,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact supported spellings map to their intended shared rule.
    #[test]
    fn recognizes_exact_typescript_calls() {
        let cases = [
            ((None, "structuredClone"), RuleId::TsStructuredClone),
            ((Some("JSON"), "parse"), RuleId::TsJsonParse),
            ((Some("Math"), "floor"), RuleId::TsMathNumeric),
            ((Some("Number"), "isNaN"), RuleId::TsNumberPredicate),
            ((Some("Promise"), "allSettled"), RuleId::TsPromiseStatic),
            ((Some("Object"), "fromEntries"), RuleId::TsObjectStatic),
            ((Some("Object"), "create"), RuleId::TsObjectStatic),
            ((Some("Array"), "isArray"), RuleId::TsArrayStatic),
        ];

        for ((receiver, member), expected) in cases {
            assert_eq!(typescript_call_rule(receiver, member), Some(expected));
        }
    }

    /// Similar unsupported spellings do not accidentally select a rule.
    #[test]
    fn rejects_similar_unsupported_typescript_calls() {
        let cases = [
            (None, "fetcher"),
            (Some("json"), "parse"),
            (Some("Math"), "randomBytes"),
            (Some("Number"), "parseDouble"),
            (Some("Promise"), "any"),
            (Some("Object"), "entries"),
            (Some("Array"), "of"),
            (Some("value"), "map"),
        ];

        for (receiver, member) in cases {
            assert_eq!(typescript_call_rule(receiver, member), None);
        }
    }

    /// Receiver-method rules are selected by frontend-provided receiver kind.
    #[test]
    fn recognizes_typescript_receiver_methods() {
        let cases = [
            (TypeScriptReceiverKind::Map, "has", Some(RuleId::TsMapHas)),
            (TypeScriptReceiverKind::Map, "get", Some(RuleId::TsMapGet)),
            (
                TypeScriptReceiverKind::Map,
                "set",
                Some(RuleId::TsMapMutation),
            ),
            (
                TypeScriptReceiverKind::Map,
                "entries",
                Some(RuleId::TsMapProjection),
            ),
            (TypeScriptReceiverKind::Set, "has", Some(RuleId::TsSetHas)),
            (
                TypeScriptReceiverKind::Set,
                "add",
                Some(RuleId::TsSetMutation),
            ),
            (
                TypeScriptReceiverKind::Set,
                "values",
                Some(RuleId::TsSetProjection),
            ),
            (TypeScriptReceiverKind::Set, "get", None),
            (TypeScriptReceiverKind::Map, "add", None),
        ];

        for (receiver, member, expected) in cases {
            assert_eq!(typescript_method_rule(receiver, member), expected);
        }
    }
}
