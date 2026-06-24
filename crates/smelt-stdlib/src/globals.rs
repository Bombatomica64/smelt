//! Recognition of well-known language builtins that Smelt does not model.
//!
//! When a reference fails to resolve, the frontend asks this module whether the
//! name is a language/runtime builtin. A builtin that fails to resolve is a
//! [`MissingStdlib`](crate::DiagnosticCategory::MissingStdlib) gap (Smelt should
//! grow support for it); any other unresolved name is an
//! [`UnresolvedReference`](crate::DiagnosticCategory::UnresolvedReference).
//!
//! The set is intentionally a recognition list, not a support list — names here
//! may or may not be lowered yet. It only answers "is this a builtin?".

/// Returns whether `name` is a well-known JavaScript/TypeScript global builtin
/// (ECMAScript intrinsics plus widely available Web/Node runtime globals).
#[must_use]
pub fn is_javascript_global_builtin(name: &str) -> bool {
    matches!(
        name,
        // Core constructors and namespaces.
        "Array" | "Object" | "Number" | "String" | "Boolean" | "BigInt"
        | "Symbol" | "Function" | "Math" | "JSON" | "Reflect" | "Proxy"
        | "Date" | "RegExp" | "Promise" | "Map" | "Set" | "WeakMap"
        | "WeakSet" | "WeakRef" | "Iterator" | "Generator" | "Intl"
        // Error constructors.
        | "Error" | "TypeError" | "RangeError" | "SyntaxError"
        | "ReferenceError" | "EvalError" | "URIError" | "AggregateError"
        // Binary data / typed arrays.
        | "ArrayBuffer" | "SharedArrayBuffer" | "DataView" | "Int8Array"
        | "Uint8Array" | "Uint8ClampedArray" | "Int16Array" | "Uint16Array"
        | "Int32Array" | "Uint32Array" | "Float32Array" | "Float64Array"
        | "BigInt64Array" | "BigUint64Array"
        // Global functions.
        | "parseInt" | "parseFloat" | "isNaN" | "isFinite" | "structuredClone"
        | "encodeURIComponent" | "decodeURIComponent" | "encodeURI" | "decodeURI"
        | "queueMicrotask" | "setTimeout" | "clearTimeout" | "setInterval"
        | "clearInterval"
        // Ambient globals and environment objects.
        | "globalThis" | "global" | "self" | "window" | "console" | "process"
        // Common Web / Node runtime classes.
        | "TextEncoder" | "TextDecoder" | "URL" | "URLSearchParams" | "Blob"
        | "File" | "FormData" | "Headers" | "Request" | "Response" | "Buffer"
        | "AbortController" | "AbortSignal" | "Event" | "EventTarget"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Recognized intrinsics and runtime globals report as builtins.
    #[test]
    fn recognizes_known_globals() {
        for name in ["Array", "Number", "Reflect", "Proxy", "TextEncoder", "globalThis"] {
            assert!(is_javascript_global_builtin(name), "{name} should be a builtin");
        }
    }

    /// Ordinary user identifiers are not builtins.
    #[test]
    fn rejects_user_identifiers() {
        for name in ["Foo", "curried", "myHelper", "Circle"] {
            assert!(!is_javascript_global_builtin(name), "{name} should not be a builtin");
        }
    }
}
