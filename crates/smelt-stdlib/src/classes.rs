//! Synthetic standard-library class recognition shared by frontends and codegen.

/// Standard-library class modeled with dedicated runtime support instead of
/// user-defined struct emission.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum StdlibClass {
    /// JavaScript `Date`, represented by timestamp and date helper operations.
    Date,
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
        "Map" => Some(StdlibClass::Map),
        MATCH_CLASS_NAME => Some(StdlibClass::Match),
        MATCH_GROUPS_CLASS_NAME => Some(StdlibClass::MatchGroups),
        "MatchFnResult" => Some(StdlibClass::MatchFnResult),
        "RegExp" => Some(StdlibClass::RegExp),
        "Set" => Some(StdlibClass::Set),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact stdlib class names resolve to their registry identity.
    #[test]
    fn recognizes_stdlib_class_names() {
        assert_eq!(typescript_stdlib_class("Date"), Some(StdlibClass::Date));
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
    }

    /// User class names never resolve to a stdlib identity.
    #[test]
    fn rejects_user_class_names() {
        for name in ["Regexp", "RegExpLike", "Dates", "HashMap", "MyClass"] {
            assert_eq!(typescript_stdlib_class(name), None);
        }
    }
}
