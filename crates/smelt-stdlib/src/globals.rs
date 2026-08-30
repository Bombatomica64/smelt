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

/// The ECMAScript `Error` constructors Smelt models as marker records.
///
/// One list, used both by the frontend (which decides that `class X extends
/// TypeError` inherits the error instance slots) and by the generated runtime's
/// reflection prelude (which must be able to rebuild any of them through
/// `Object.getPrototypeOf(e).constructor`). `AggregateError` is last only
/// because it is the one with a different constructor signature.
pub const ERROR_CLASS_NAMES: &[&str] = &[
    "Error",
    "EvalError",
    "RangeError",
    "ReferenceError",
    "SyntaxError",
    "TypeError",
    "URIError",
    "AggregateError",
];

/// The Node.js release the deterministic non-DOM, Node-compatible target
/// profile reports.
///
/// Single source of truth for every place the profile has to answer "which Node
/// is this?": the `process.versions.node` member of the modeled `process`
/// object and the `process.version` string. Keeping both derived from one
/// constant is what stops a value model and a member model from disagreeing
/// about the same host global.
pub const NODE_PROFILE_VERSION: &str = "20.0.0";

/// The `process.version` spelling of [`NODE_PROFILE_VERSION`] (`v`-prefixed).
pub const NODE_PROFILE_VERSION_STRING: &str = "v20.0.0";

/// Returns whether `name` is one of the modeled ECMAScript `Error` constructors.
#[must_use]
pub fn is_error_class_name(name: &str) -> bool {
    ERROR_CLASS_NAMES.contains(&name)
}

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

/// Compile-time availability of a global object member in the active target profile.
///
/// The current target is a deterministic non-DOM, Node-compatible environment
/// (the default generated Rust test profile from the global-objects plan). A
/// feature probe such as `"X" in globalThis` may only fold to a literal when the
/// answer is *known* for that profile; an [`Unknown`](GlobalPresence::Unknown)
/// member must keep its runtime check so erased and runtime answers never
/// disagree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum GlobalPresence {
    /// The member is a recognized builtin that exists in the non-DOM profile, so
    /// `"X" in globalThis` is statically `true`.
    Present,
    /// The member is a recognized builtin that is intentionally absent from the
    /// non-DOM profile (browser/DOM-only surfaces), so `"X" in globalThis` is
    /// statically `false`.
    Absent,
    /// The member is not in any recognition registry, so its presence cannot be
    /// decided at compile time and the probe must stay a runtime check.
    Unknown,
}

/// Global members that the non-DOM Node-compatible profile treats as absent.
///
/// These names are still recognized JavaScript globals (see
/// [`is_javascript_global_builtin`]) but only exist in a browser/DOM host, so the
/// non-DOM profile answers `"X" in globalThis` with `false`. Keeping the absent
/// set explicit means everything else recognized is derived as present, instead
/// of maintaining a parallel hand-written "present" list that could drift from
/// the recognition registry.
const NON_DOM_ABSENT_GLOBALS: &[&str] = &["window", "self", "document"];

/// Classify a candidate global member name for the non-DOM Node-compatible profile.
///
/// The result is derived from the recognition registries that codegen actually
/// lowers — [`is_javascript_global_builtin`] plus the absent-in-non-DOM denylist —
/// rather than a separate literal "present" list, so the compile-time answer to
/// `"X" in globalThis` cannot drift from what Smelt can lower. Per the plan,
/// `structuredClone` and `crypto` are *not* answered `Present` here: they are
/// runtime functions whose probes may only fold once a deterministic runtime
/// implementation exists, so they stay [`Unknown`](GlobalPresence::Unknown).
#[must_use]
pub fn global_member_presence(name: &str) -> GlobalPresence {
    if NON_DOM_ABSENT_GLOBALS.contains(&name) {
        return GlobalPresence::Absent;
    }
    // Runtime-capability functions are gated on real deterministic runtime
    // support landing (plan section 7); until then their probe must not fold.
    if matches!(name, "structuredClone" | "crypto" | "fetch") {
        return GlobalPresence::Unknown;
    }
    if is_javascript_global_builtin(name) {
        GlobalPresence::Present
    } else {
        GlobalPresence::Unknown
    }
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

    /// Recognized non-DOM globals are present so `"X" in globalThis` folds true.
    #[test]
    fn recognized_node_globals_are_present() {
        for name in ["Map", "Set", "ArrayBuffer", "Reflect", "Promise", "process"] {
            assert_eq!(global_member_presence(name), GlobalPresence::Present, "{name}");
        }
    }

    /// DOM-only globals are absent so `"X" in globalThis` folds false.
    #[test]
    fn dom_only_globals_are_absent() {
        for name in ["window", "self", "document"] {
            assert_eq!(global_member_presence(name), GlobalPresence::Absent, "{name}");
        }
    }

    /// Unrecognized names and runtime-gated capabilities stay unknown.
    #[test]
    fn unmodeled_members_are_unknown() {
        for name in ["DocumentFragment", "__feature", "structuredClone", "crypto"] {
            assert_eq!(global_member_presence(name), GlobalPresence::Unknown, "{name}");
        }
    }

    /// The present set is derived from the recognition registry, not a copy of it.
    #[test]
    fn presence_tracks_recognition_registry() {
        // Every recognized builtin is either present or explicitly absent; none
        // silently fall through to Unknown except the runtime-gated capabilities.
        for name in ["Array", "JSON", "Math", "Buffer", "URL", "TextEncoder"] {
            assert_ne!(
                global_member_presence(name),
                GlobalPresence::Unknown,
                "recognized builtin {name} should have a decided presence"
            );
        }
    }
}
