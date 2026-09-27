//! `Array.prototype` / `String.prototype` methods the erased runtime dispatches.
//!
//! A value whose static type is erased (`any`, or a read off one) has no
//! compile-time shape, so a method call on it (`value.reduceRight(f, 0)`)
//! cannot always be lowered to one typed list or string operation: which
//! prototype answers is decided by the RUNTIME tag of the value, exactly as
//! JavaScript's property lookup decides it. The generated runtime therefore
//! resolves the member at the property-read seam (`smelt_get_unknown_field`):
//! an erased array answers the `Array.prototype` methods below, an erased
//! string the `String.prototype` methods, and anything else answers
//! `undefined`, which the erased call ABI turns into JavaScript's
//! `TypeError: x.m is not a function`.
//!
//! These tables are the single answer to "which prototype methods does the
//! erased runtime implement?". The codegen prelude emits its member lookup from
//! them, and the TypeScript frontend asks [`is_erased_prototype_method`]
//! when a name-keyed builtin lowering must not claim an erased receiver (see
//! its docs). A method absent here reads as `undefined` on an erased receiver —
//! honest, rather than a callable that does something else.

/// Every `Array.prototype` method an erased array answers.
///
/// Ordered roughly by how often real code calls them on an erased receiver;
/// the order is documentation only, lookups are by name.
pub const ERASED_ARRAY_PROTOTYPE_METHODS: &[&str] = &[
    "map",
    "filter",
    "forEach",
    "reduce",
    "reduceRight",
    "some",
    "every",
    "find",
    "findIndex",
    "findLast",
    "findLastIndex",
    "includes",
    "indexOf",
    "lastIndexOf",
    "join",
    "slice",
    "concat",
    "at",
    "flat",
    "flatMap",
    "push",
    "pop",
    "shift",
    "unshift",
    "splice",
    "reverse",
    "sort",
    "fill",
    "keys",
    "values",
    "entries",
    "toString",
];

/// Every `String.prototype` method an erased string answers.
///
/// The pattern-taking methods (`split`, `replace`, `match`, `search`) are
/// absent on purpose: their argument may be a `RegExp`, which the erased
/// runtime has no matcher for, so they keep their static string lowering
/// rather than a runtime arm that would silently stringify the pattern.
pub const ERASED_STRING_PROTOTYPE_METHODS: &[&str] = &[
    "at",
    "charAt",
    "charCodeAt",
    "codePointAt",
    "concat",
    "endsWith",
    "includes",
    "indexOf",
    "lastIndexOf",
    "padEnd",
    "padStart",
    "repeat",
    "slice",
    "startsWith",
    "substring",
    "toLowerCase",
    "toUpperCase",
    "trim",
    "trimEnd",
    "trimStart",
    "toString",
];

/// The most arguments a call of the prototype method `name` can take.
///
/// The limits come from the ECMAScript parameter lists (optional parameters
/// included); `None` for a variadic method (`push`, `concat`, ...) or a name
/// neither table models.
///
/// Where a name is on both prototypes the larger limit applies. A call that
/// passes MORE than this is not a call of the prototype method at all: it is
/// the utility-function spelling of the same name (`utils.reduce(xs, f, 0)`,
/// `utils.keys(record)`), whose first argument is the collection, and it keeps
/// its dedicated lowering (see [`is_erased_prototype_call`]).
#[must_use]
pub fn prototype_method_max_arguments(name: &str) -> Option<usize> {
    match name {
        "pop" | "shift" | "reverse" | "keys" | "values" | "entries" | "toString" | "toLowerCase"
        | "toUpperCase" | "trim" | "trimEnd" | "trimStart" => Some(0),
        "join" | "at" | "flat" | "sort" | "charAt" | "charCodeAt" | "codePointAt" | "repeat" => {
            Some(1)
        }
        "map" | "filter" | "forEach" | "reduce" | "reduceRight" | "some" | "every" | "find"
        | "findIndex" | "findLast" | "findLastIndex" | "flatMap" | "includes" | "indexOf"
        | "lastIndexOf" | "slice" | "endsWith" | "startsWith" | "padEnd" | "padStart"
        | "substring" => Some(2),
        "fill" => Some(3),
        _ => None,
    }
}

/// Whether a call `receiver.name(..)` on an ERASED receiver resolves at run time.
///
/// It does when `name` is an erased prototype method (see
/// [`is_erased_prototype_method`]) and the call's `argument_count` fits that
/// method's parameter list ([`prototype_method_max_arguments`]).
#[must_use]
pub fn is_erased_prototype_call(name: &str, argument_count: usize) -> bool {
    is_erased_prototype_method(name)
        && prototype_method_max_arguments(name).is_none_or(|max| argument_count <= max)
}

/// Look an erased `Array.prototype` method up by name.
#[must_use]
pub fn erased_array_prototype_method(name: &str) -> Option<&'static str> {
    ERASED_ARRAY_PROTOTYPE_METHODS
        .iter()
        .copied()
        .find(|method| *method == name)
}

/// Look an erased `String.prototype` method up by name.
#[must_use]
pub fn erased_string_prototype_method(name: &str) -> Option<&'static str> {
    ERASED_STRING_PROTOTYPE_METHODS
        .iter()
        .copied()
        .find(|method| *method == name)
}

/// Whether a call of `name` on an ERASED receiver resolves at run time.
///
/// On a receiver typed `any` the member name does not fix the operation: the
/// receiver's runtime tag does. A name-keyed builtin lowering that claims such
/// a receiver has to guess a prototype, and every guess is wrong for some
/// values — `anyString.slice(1)` lowered as a list slice yields an array of
/// characters, `anyArray.includes(2)` lowered as string containment does not
/// even type-check, `anyNumber.map(f)` lowered as a list map panics instead of
/// throwing JavaScript's `TypeError`, and `anyArray.unshift(x)` was rejected
/// outright. TypeScript itself types the result `any`, so the guess buys no
/// static precision the source had. The frontend therefore leaves an erased
/// receiver calling one of these names to the erased member call, which
/// resolves it on the value it meets through these tables.
///
/// Two names are excluded because their lowering is not a guess:
/// - `toString`: every value answers it (`Object.prototype`), so it is not a
///   prototype question, and it keeps its dedicated lowering.
/// - `slice`: its erased lowering already dispatches on the runtime tag
///   (codegen's tag-preserving erased slice). It covers arrays, strings AND
///   byte-backed host objects (`ArrayBuffer`/`Buffer`/typed arrays, which must
///   slice to a fresh record of the same host identity). That is a superset of
///   what these tables model for the name, so rerouting it would lose the
///   host-buffer arm.
///
/// These names still resolve through the tables when the method is READ as a
/// value (`const f = anyValue.slice`).
#[must_use]
pub fn is_erased_prototype_method(name: &str) -> bool {
    !matches!(name, "toString" | "slice")
        && (erased_array_prototype_method(name).is_some()
            || erased_string_prototype_method(name).is_some())
}

#[cfg(test)]
mod tests {
    use super::{
        ERASED_ARRAY_PROTOTYPE_METHODS, ERASED_STRING_PROTOTYPE_METHODS, erased_array_prototype_method,
        is_erased_prototype_call, is_erased_prototype_method, prototype_method_max_arguments,
    };

    /// No table lists a method twice, so a lookup has one answer.
    #[test]
    fn tables_have_unique_names() {
        for table in [ERASED_ARRAY_PROTOTYPE_METHODS, ERASED_STRING_PROTOTYPE_METHODS] {
            for (index, name) in table.iter().enumerate() {
                assert!(
                    table.iter().skip(index + 1).all(|other| other != name),
                    "`{name}` is listed twice"
                );
            }
        }
    }

    /// A lookup answers only the prototype's own methods.
    #[test]
    fn array_lookup_answers_array_methods_only() {
        assert_eq!(erased_array_prototype_method("reduceRight"), Some("reduceRight"));
        assert_eq!(erased_array_prototype_method("toUpperCase"), None);
    }

    /// Every modeled method of either prototype resolves at run time, except
    /// `toString`, `slice` (already tag-dispatched by its own lowering) and
    /// names neither table models.
    #[test]
    fn runtime_resolution_covers_both_tables_except_non_guessing_lowerings() {
        for name in ["includes", "at", "map", "reduceRight", "unshift", "toUpperCase"] {
            assert!(is_erased_prototype_method(name), "{name}");
        }
        for name in ["toString", "slice", "split", "replace", "then"] {
            assert!(!is_erased_prototype_method(name), "{name}");
        }
    }

    /// Every modeled method has an argument limit or is known variadic, and a
    /// call with more arguments than the prototype accepts is the utility
    /// spelling (`utils.reduce(xs, f, 0)`), not a prototype call.
    #[test]
    fn over_long_argument_lists_are_not_prototype_calls() {
        let variadic = ["concat", "push", "unshift", "splice"];
        for table in [ERASED_ARRAY_PROTOTYPE_METHODS, ERASED_STRING_PROTOTYPE_METHODS] {
            for &name in table {
                assert!(
                    prototype_method_max_arguments(name).is_some() || variadic.contains(&name),
                    "`{name}` has no argument limit"
                );
            }
        }
        assert!(is_erased_prototype_call("reduce", 2));
        assert!(!is_erased_prototype_call("reduce", 3));
        assert!(is_erased_prototype_call("keys", 0));
        assert!(!is_erased_prototype_call("keys", 1));
        assert!(is_erased_prototype_call("push", 7));
    }
}
