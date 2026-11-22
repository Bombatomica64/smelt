//! The `name` and `length` JavaScript builtin functions report as values.
//!
//! A builtin function is an ordinary function object: `const f = parseInt;
//! f.name` is `"parseInt"` and `f.length` is `2`. ECMA-262 fixes both for
//! every builtin. The name is the property key the function is installed
//! under (the global binding `parseInt`, the member `floor` of `Math`), so it
//! needs no table: [`builtin_function`] answers it from the key. The length is
//! the specification's own per-function number (`parseInt` counts its optional
//! `radix`, `Array.prototype.indexOf` does not count its `fromIndex`), which no
//! rule over the TypeScript lib signatures reproduces, so it is tabulated here.
//!
//! The table holds the global functions and constructors and the static
//! functions of the global namespace objects, with the lengths Node reports
//! (the generated program is judged against Node; host functions such as
//! `setTimeout` are Node's). It was produced from Node v24 by enumerating
//! each owner's own function-valued properties.

/// One builtin function: where it is installed and the `length` it reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct BuiltinFunction {
    /// The global namespace object owning the function (`"Math"`), or `None`
    /// for a global binding (`parseInt`, `String`).
    pub owner: Option<&'static str>,
    /// The property key it is installed under, which is also its
    /// `Function.prototype.name`.
    pub name: &'static str,
    /// Its `Function.prototype.length`.
    pub length: usize,
}

/// A global binding's function.
const fn global(name: &'static str, length: usize) -> BuiltinFunction {
    BuiltinFunction {
        owner: None,
        name,
        length,
    }
}

/// A static function of a global namespace object.
const fn member(owner: &'static str, name: &'static str, length: usize) -> BuiltinFunction {
    BuiltinFunction {
        owner: Some(owner),
        name,
        length,
    }
}

/// Every tabulated builtin function.
pub const BUILTIN_FUNCTIONS: &[BuiltinFunction] = &[
    global("parseInt", 2),
    global("parseFloat", 1),
    global("isNaN", 1),
    global("isFinite", 1),
    global("encodeURI", 1),
    global("encodeURIComponent", 1),
    global("decodeURI", 1),
    global("decodeURIComponent", 1),
    global("escape", 1),
    global("unescape", 1),
    global("atob", 1),
    global("btoa", 1),
    global("structuredClone", 2),
    global("queueMicrotask", 1),
    global("setTimeout", 2),
    global("setInterval", 2),
    global("clearTimeout", 1),
    global("clearInterval", 1),
    global("String", 1),
    global("Number", 1),
    global("Boolean", 1),
    global("Object", 1),
    global("Array", 1),
    global("Function", 1),
    global("Symbol", 0),
    global("BigInt", 1),
    global("Date", 7),
    global("RegExp", 2),
    global("Error", 1),
    global("TypeError", 1),
    global("RangeError", 1),
    global("SyntaxError", 1),
    global("ReferenceError", 1),
    global("EvalError", 1),
    global("URIError", 1),
    global("AggregateError", 2),
    global("Map", 0),
    global("Set", 0),
    global("WeakMap", 0),
    global("WeakSet", 0),
    global("WeakRef", 1),
    global("Promise", 1),
    global("Proxy", 2),
    global("ArrayBuffer", 1),
    global("DataView", 1),
    global("Int8Array", 3),
    global("Uint8Array", 3),
    global("Uint8ClampedArray", 3),
    global("Int16Array", 3),
    global("Uint16Array", 3),
    global("Int32Array", 3),
    global("Uint32Array", 3),
    global("Float32Array", 3),
    global("Float64Array", 3),
    global("BigInt64Array", 3),
    global("BigUint64Array", 3),
    global("URL", 1),
    global("URLSearchParams", 0),
    global("TextEncoder", 0),
    global("TextDecoder", 0),
    global("Headers", 0),
    global("Request", 1),
    global("Response", 0),
    global("AbortController", 0),
    global("Blob", 0),
    global("FormData", 0),
    member("Math", "abs", 1),
    member("Math", "acos", 1),
    member("Math", "acosh", 1),
    member("Math", "asin", 1),
    member("Math", "asinh", 1),
    member("Math", "atan", 1),
    member("Math", "atan2", 2),
    member("Math", "atanh", 1),
    member("Math", "cbrt", 1),
    member("Math", "ceil", 1),
    member("Math", "clz32", 1),
    member("Math", "cos", 1),
    member("Math", "cosh", 1),
    member("Math", "exp", 1),
    member("Math", "expm1", 1),
    member("Math", "f16round", 1),
    member("Math", "floor", 1),
    member("Math", "fround", 1),
    member("Math", "hypot", 2),
    member("Math", "imul", 2),
    member("Math", "log", 1),
    member("Math", "log10", 1),
    member("Math", "log1p", 1),
    member("Math", "log2", 1),
    member("Math", "max", 2),
    member("Math", "min", 2),
    member("Math", "pow", 2),
    member("Math", "random", 0),
    member("Math", "round", 1),
    member("Math", "sign", 1),
    member("Math", "sin", 1),
    member("Math", "sinh", 1),
    member("Math", "sqrt", 1),
    member("Math", "tan", 1),
    member("Math", "tanh", 1),
    member("Math", "trunc", 1),
    member("JSON", "isRawJSON", 1),
    member("JSON", "parse", 2),
    member("JSON", "rawJSON", 1),
    member("JSON", "stringify", 3),
    member("Object", "assign", 2),
    member("Object", "create", 2),
    member("Object", "defineProperties", 2),
    member("Object", "defineProperty", 3),
    member("Object", "entries", 1),
    member("Object", "freeze", 1),
    member("Object", "fromEntries", 1),
    member("Object", "getOwnPropertyDescriptor", 2),
    member("Object", "getOwnPropertyDescriptors", 1),
    member("Object", "getOwnPropertyNames", 1),
    member("Object", "getOwnPropertySymbols", 1),
    member("Object", "getPrototypeOf", 1),
    member("Object", "groupBy", 2),
    member("Object", "hasOwn", 2),
    member("Object", "is", 2),
    member("Object", "isExtensible", 1),
    member("Object", "isFrozen", 1),
    member("Object", "isSealed", 1),
    member("Object", "keys", 1),
    member("Object", "preventExtensions", 1),
    member("Object", "seal", 1),
    member("Object", "setPrototypeOf", 2),
    member("Object", "values", 1),
    member("Array", "from", 1),
    member("Array", "fromAsync", 1),
    member("Array", "isArray", 1),
    member("Array", "of", 0),
    member("Number", "isFinite", 1),
    member("Number", "isInteger", 1),
    member("Number", "isNaN", 1),
    member("Number", "isSafeInteger", 1),
    member("Number", "parseFloat", 1),
    member("Number", "parseInt", 2),
    member("String", "fromCharCode", 1),
    member("String", "fromCodePoint", 1),
    member("String", "raw", 1),
    member("Promise", "all", 1),
    member("Promise", "allSettled", 1),
    member("Promise", "any", 1),
    member("Promise", "race", 1),
    member("Promise", "reject", 1),
    member("Promise", "resolve", 1),
    member("Promise", "try", 1),
    member("Promise", "withResolvers", 0),
    member("Reflect", "apply", 3),
    member("Reflect", "construct", 2),
    member("Reflect", "defineProperty", 3),
    member("Reflect", "deleteProperty", 2),
    member("Reflect", "get", 2),
    member("Reflect", "getOwnPropertyDescriptor", 2),
    member("Reflect", "getPrototypeOf", 1),
    member("Reflect", "has", 2),
    member("Reflect", "isExtensible", 1),
    member("Reflect", "ownKeys", 1),
    member("Reflect", "preventExtensions", 1),
    member("Reflect", "set", 3),
    member("Reflect", "setPrototypeOf", 2),
    member("Date", "UTC", 7),
    member("Date", "now", 0),
    member("Date", "parse", 1),
    member("Symbol", "for", 1),
    member("Symbol", "keyFor", 1),
];

/// The builtin function installed as `name` on `owner` (`None`: a global
/// binding), when it is tabulated.
#[must_use]
pub fn builtin_function(owner: Option<&str>, name: &str) -> Option<&'static BuiltinFunction> {
    BUILTIN_FUNCTIONS
        .iter()
        .find(|entry| entry.owner == owner && entry.name == name)
}

#[cfg(test)]
mod tests {
    use super::builtin_function;

    /// A global's length counts what the specification counts, optional
    /// parameters included where it says so.
    #[test]
    fn global_lengths_follow_the_specification() {
        assert_eq!(
            builtin_function(None, "parseInt").map(|f| f.length),
            Some(2)
        );
        assert_eq!(builtin_function(None, "String").map(|f| f.length), Some(1));
        assert_eq!(builtin_function(None, "Date").map(|f| f.length), Some(7));
    }

    /// A namespace member is keyed by its owner: `Number.parseFloat` and the
    /// global `parseFloat` are two entries, and `Math` is not a function.
    #[test]
    fn members_are_keyed_by_owner() {
        assert_eq!(
            builtin_function(Some("Math"), "max").map(|f| f.length),
            Some(2)
        );
        assert_eq!(
            builtin_function(Some("JSON"), "stringify").map(|f| f.length),
            Some(3)
        );
        assert!(builtin_function(None, "max").is_none());
        assert!(builtin_function(None, "Math").is_none());
    }
}
