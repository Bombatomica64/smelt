//! Synthetic standard-library class recognition shared by frontends and codegen.

/// Standard-library class modeled with dedicated runtime support instead of
/// user-defined struct emission.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum StdlibClass {
    /// JavaScript `Date`, represented by timestamp and date helper operations.
    Date,
    /// WHATWG `Headers`, backed by the generated concrete `SmeltHeaders`
    /// runtime type (a case-insensitive, insertion-ordered header multi-map).
    /// Reads keep their exact types: `get` is `string | null`, `has` is a
    /// boolean, `getSetCookie` is a string list.
    Headers,
    /// JavaScript `Map`, represented by dictionary HIR values.
    Map,
    /// Synthetic `RegExp` match result (`RegExp.exec` / `String.matchAll`),
    /// backed by the generated concrete `SmeltMatch` runtime type. Consumer
    /// reads (`m[0]`, `m.index`, `m.input`, `m.groups.name`) resolve to typed
    /// accessors on `SmeltMatch` instead of the erased `SmeltUnknown` path.
    Match,
    /// Synthetic named-capture-group accessor for `matchResult.groups`. It is
    /// the same underlying `SmeltMatch` value; a `.name` read on it resolves to
    /// the typed named-group accessor.
    MatchGroups,
    /// Remeda parser helper result class synthesized during TypeScript lowering.
    MatchFnResult,
    /// JavaScript `RegExp`, backed by the generated regex runtime shim.
    RegExp,
    /// JavaScript `Set`, represented by set HIR values.
    Set,
    /// WHATWG `URLSearchParams`, backed by the generated concrete
    /// `SmeltUrlSearchParams` runtime type (an ordered, case-sensitive
    /// name/value pair list with `application/x-www-form-urlencoded`
    /// serialization).
    UrlSearchParams,
    /// WHATWG `Response`, backed by the generated concrete `SmeltResponse`
    /// runtime type (a status line, a `SmeltHeaders`, and a single-use
    /// `SmeltBody`).
    Response,
}

/// Reserved synthetic class name for a `RegExp` match result value.
///
/// The name is not writable in user TypeScript (double-underscore prefix), so
/// it never collides with a source class; it exists only to carry the concrete
/// match shape through the internal type system.
pub const MATCH_CLASS_NAME: &str = "__SmeltMatch";

/// Reserved synthetic class name for `matchResult.groups` named-group access.
pub const MATCH_GROUPS_CLASS_NAME: &str = "__SmeltMatchGroups";

/// Return the stdlib class modeled by a TypeScript class type name.
///
/// Codegen consults this instead of comparing class symbol names inline so
/// stdlib class identities stay in one registry.
#[must_use]
pub fn typescript_stdlib_class(name: &str) -> Option<StdlibClass> {
    match name {
        "Date" => Some(StdlibClass::Date),
        "Headers" => Some(StdlibClass::Headers),
        "Map" => Some(StdlibClass::Map),
        MATCH_CLASS_NAME => Some(StdlibClass::Match),
        MATCH_GROUPS_CLASS_NAME => Some(StdlibClass::MatchGroups),
        "MatchFnResult" => Some(StdlibClass::MatchFnResult),
        "RegExp" => Some(StdlibClass::RegExp),
        "Set" => Some(StdlibClass::Set),
        "URLSearchParams" => Some(StdlibClass::UrlSearchParams),
        "Response" => Some(StdlibClass::Response),
        _ => None,
    }
}

/// The JavaScript typed-array view constructor names, in a stable order.
///
/// These are the eleven `TypedArray` element-view classes (`Uint8Array`,
/// `Int8Array`, ..., `BigInt64Array`, `BigUint64Array`). Each has a full
/// [`crate::host_object`] registry entry — its own identity marker, spec tag, and
/// element type — and this shared name list is the single source of truth every
/// frontend/codegen site consults to decide whether a constructor / bare
/// identifier / `instanceof` target names a typed array. Keeping the set here —
/// rather than re-spelling the `matches!(...)` arm in each site — stops the
/// construction side, the value-resolution side, and the `instanceof` side from
/// drifting apart.
pub const TYPED_ARRAY_CLASS_NAMES: [&str; 11] = [
    "Int8Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "Int16Array",
    "Uint16Array",
    "Int32Array",
    "Uint32Array",
    "Float32Array",
    "Float64Array",
    "BigInt64Array",
    "BigUint64Array",
];

/// Return whether a class name is one of the JavaScript typed-array views.
///
/// Consulted by the TypeScript frontend (constructor lowering, bare-value
/// resolution, `instanceof` targeting) and codegen so the eleven typed-array
/// names are recognized from one place. `BigInt64Array` / `BigUint64Array` are
/// included even though their elements are `BigInt` values: they are eight-byte
/// integer views like any other, and Smelt models a JavaScript `BigInt` as a
/// number, so they share both the recognizer and the element codec with the
/// numeric views.
#[must_use]
pub fn is_typed_array_class_name(name: &str) -> bool {
    TYPED_ARRAY_CLASS_NAMES.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact stdlib class names resolve to their registry identity.
    #[test]
    fn recognizes_stdlib_class_names() {
        assert_eq!(typescript_stdlib_class("Date"), Some(StdlibClass::Date));
        assert_eq!(
            typescript_stdlib_class("Headers"),
            Some(StdlibClass::Headers)
        );
        assert_eq!(typescript_stdlib_class("Map"), Some(StdlibClass::Map));
        assert_eq!(
            typescript_stdlib_class(MATCH_CLASS_NAME),
            Some(StdlibClass::Match)
        );
        assert_eq!(
            typescript_stdlib_class(MATCH_GROUPS_CLASS_NAME),
            Some(StdlibClass::MatchGroups)
        );
        assert_eq!(
            typescript_stdlib_class("MatchFnResult"),
            Some(StdlibClass::MatchFnResult)
        );
        assert_eq!(typescript_stdlib_class("RegExp"), Some(StdlibClass::RegExp));
        assert_eq!(typescript_stdlib_class("Set"), Some(StdlibClass::Set));
        assert_eq!(
            typescript_stdlib_class("URLSearchParams"),
            Some(StdlibClass::UrlSearchParams)
        );
    }

    /// User class names never resolve to a stdlib identity.
    #[test]
    fn rejects_user_class_names() {
        for name in ["Regexp", "RegExpLike", "Dates", "HashMap", "MyClass"] {
            assert_eq!(typescript_stdlib_class(name), None);
        }
    }

    /// Every typed-array view constructor is recognized, including the `BigInt`
    /// views, while plain `Array` and lookalikes are not.
    #[test]
    fn recognizes_typed_array_class_names() {
        for name in TYPED_ARRAY_CLASS_NAMES {
            assert!(
                is_typed_array_class_name(name),
                "expected `{name}` to be recognized as a typed array"
            );
        }
        assert!(is_typed_array_class_name("BigUint64Array"));
        for name in ["Array", "Uint8", "TypedArray", "Float16Array", "MyUint8Array"] {
            assert!(
                !is_typed_array_class_name(name),
                "did not expect `{name}` to be recognized as a typed array"
            );
        }
    }
}
