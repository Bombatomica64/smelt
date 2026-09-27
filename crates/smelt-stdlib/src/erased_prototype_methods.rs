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
/// `toString` is excluded: every value answers it (`Object.prototype`), so it
/// is not a prototype question, and it keeps its dedicated lowering.
#[must_use]
pub fn is_erased_prototype_method(name: &str) -> bool {
    name != "toString"
        && (erased_array_prototype_method(name).is_some()
            || erased_string_prototype_method(name).is_some())
}

#[cfg(test)]
mod tests {
    use super::{
        ERASED_ARRAY_PROTOTYPE_METHODS, ERASED_STRING_PROTOTYPE_METHODS,
        erased_array_prototype_method, is_erased_prototype_method,
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
    /// the `Object.prototype`-wide `toString` and names neither table models.
    #[test]
    fn runtime_resolution_covers_both_tables_but_not_to_string() {
        for name in ["slice", "includes", "map", "reduceRight", "unshift", "toUpperCase"] {
            assert!(is_erased_prototype_method(name), "{name}");
        }
        for name in ["toString", "split", "replace", "then"] {
            assert!(!is_erased_prototype_method(name), "{name}");
        }
    }
}
