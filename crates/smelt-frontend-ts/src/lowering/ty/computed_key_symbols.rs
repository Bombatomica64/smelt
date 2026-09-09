//! Symbol-backed computed property-key folding helpers.
//!
//! A computed property key such as `[Symbol.asyncIterator]`, `[matcher]`, or
//! `[symbols.override]` names a *static* member whenever the underlying symbol
//! is globally fixed:
//!
//! * a **well-known symbol** (`Symbol.iterator`, `Symbol.asyncIterator`, …) is a
//!   constant of the language, so every reference names the same member, and
//! * a **registry symbol** (`Symbol.for("desc")`) is looked up in the process
//!   global symbol registry keyed by its description string, so every
//!   `Symbol.for("desc")` — whether spelled inline, aliased to a `const`, or
//!   read through a namespace import — is the same symbol.
//!
//! Both map deterministically to a stable synthetic member spelling that member
//! access and member declaration agree on, so they fold to named members exactly
//! like a spelled-out string key instead of hitting the dynamic-key gate
//! (issue #115, follow-up to #96).
//!
//! A *unique* symbol (`Symbol("desc")` without `.for`, or an opaque runtime
//! symbol value) is a fresh identity every time it is evaluated and has no
//! stable static spelling, so it deliberately does not fold here and stays on
//! the runtime-keyed path.


/// Return the stable synthetic member key for a well-known `Symbol.<name>`.
///
/// Delegates to [`smelt_stdlib::well_known_symbols`], the single table shared
/// with the generated Rust prelude: a well-known symbol's *value* spelling and
/// the property *key* it indexes must agree, so neither side owns its own copy
/// of the mapping. Returns `None` for symbol names Smelt does not model as
/// static members, which keeps genuinely unsupported symbol keys on the honest
/// dynamic-key path.
pub(in crate::lowering) fn well_known_symbol_key(name: &str) -> Option<String> {
    smelt_stdlib::well_known_symbols::storage_key(name)
}

/// Return the well-known property key a symbol *value* spelling indexes.
///
/// The inverse direction of [`well_known_symbol_key`]: a `const s =
/// Symbol.iterator` alias holds the value spelling, and using it as a computed
/// key (`{ [s]: 1 }`) must name the same member an inline `[Symbol.iterator]`
/// key names.
pub(in crate::lowering) fn well_known_key_of_symbol_literal(spelling: &str) -> Option<String> {
    smelt_stdlib::well_known_symbols::storage_key_for_spelling(spelling)
}

/// Return the runtime *value* spelling of a well-known `Symbol.<name>`.
///
/// `Symbol.iterator` in value position is a symbol, not a string: this is the
/// description `SmeltUnknown::Symbol(..)` carries for it.
pub(in crate::lowering) fn well_known_symbol_value_spelling(name: &str) -> Option<String> {
    smelt_stdlib::well_known_symbols::value_spelling(name)
}


/// Return the stable synthetic member key for a unique `Symbol(...)` VALUE that
/// a module-level `const` binds.
///
/// A unique symbol has fresh identity per evaluation, which is why its value
/// spelling is span-tagged (`Symbol(desc)@<offset>`) and why it does not fold to
/// a member key in general: a `Symbol()` inside a function body denotes a
/// different symbol on every call, so two reads through it are not the same
/// member.
///
/// A module-level `const` initializer is evaluated exactly once, so the symbol
/// it binds is one symbol for the program's lifetime and the span tag is a
/// stable, collision-free name for it. `const A = Symbol()` and
/// `const B = Symbol()` sit at different offsets and get different keys, while
/// every read of the same const folds to the same key. That is what makes
/// `class C { get [A]() { .. } }` an ordinary member with a symbol name and
/// `c[A]` an ordinary static read of it.
///
/// Returns `None` for any spelling that is not a unique symbol, so registry and
/// well-known symbols keep their own globally interned keys.
///
/// Delegates to [`smelt_stdlib::symbol_keys`], the single owner of the
/// value-spelling-to-key derivation: the generated Rust prelude derives the same
/// key from a *runtime* symbol value, so neither half may hold its own copy of
/// the scheme.
pub(in crate::lowering) fn unique_symbol_key(spelling: &str) -> Option<String> {
    smelt_stdlib::symbol_keys::unique_symbol_key(spelling)
}

/// Return the stable synthetic member key for a `Symbol.for(description)`.
///
/// The description string is sanitized so the resulting key is a valid,
/// collision-resistant identifier while remaining a pure function of the
/// registry description (every reference to the same registry symbol folds to
/// the same key).
///
/// Delegates to [`smelt_stdlib::symbol_keys`] for the same reason
/// [`unique_symbol_key`] does.
pub(in crate::lowering) fn registry_symbol_key(description: &str) -> String {
    smelt_stdlib::symbol_keys::registry_symbol_key(description)
}

/// Extract the registry description from a lowered `Symbol.for(...)` literal.
///
/// `Symbol.for(desc)` values lower to the stable literal string
/// `"Symbol.for(<desc>)"` (see the `Symbol` call dispatch), while unique
/// `Symbol(...)` values carry an unstable span-tagged spelling. Only the
/// registry form yields a stable key, so this returns `Some(desc)` for the
/// former and `None` for the latter.
pub(in crate::lowering) fn registry_description_of_symbol_literal(value: &str) -> Option<&str> {
    smelt_stdlib::symbol_keys::registry_description_of_value_spelling(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iterator_keeps_established_spelling() {
        assert_eq!(
            well_known_symbol_key("iterator").as_deref(),
            Some("__smelt_symbol_iterator")
        );
    }

    #[test]
    fn async_iterator_uses_snake_case_scheme() {
        assert_eq!(
            well_known_symbol_key("asyncIterator").as_deref(),
            Some("__smelt_symbol_async_iterator")
        );
        assert_eq!(
            well_known_symbol_key("toStringTag").as_deref(),
            Some("__smelt_symbol_to_string_tag")
        );
    }

    #[test]
    fn unmodeled_symbol_name_does_not_fold() {
        assert_eq!(well_known_symbol_key("madeUpSymbol"), None);
    }

    /// The folded key is a pure function of the description, distinguishes
    /// descriptions that differ only in punctuation, and is exactly the key the
    /// shared derivation gives a `Symbol.for` VALUE — the agreement the runtime
    /// half of the compiler also depends on.
    #[test]
    fn registry_key_agrees_with_the_shared_derivation() {
        assert_eq!(
            registry_symbol_key("@ts-pattern/matcher"),
            smelt_stdlib::symbol_keys::storage_key_for_value_spelling(
                "Symbol.for(@ts-pattern/matcher)"
            )
        );
        assert_ne!(
            registry_symbol_key("@ts-pattern/matcher"),
            registry_symbol_key("@ts-pattern/override")
        );
        assert_ne!(registry_symbol_key("a.b"), registry_symbol_key("a_b"));
        // Same description -> same key, regardless of how it was referenced.
        assert_eq!(
            registry_symbol_key("app.event"),
            registry_symbol_key("app.event")
        );
        assert!(
            registry_symbol_key("@ns/name")
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'),
            "a folded key must be spellable as a Rust identifier"
        );
    }

    #[test]
    fn registry_description_extraction() {
        assert_eq!(
            registry_description_of_symbol_literal("Symbol.for(@ts-pattern/matcher)"),
            Some("@ts-pattern/matcher")
        );
        // Unique symbols carry an unstable span tag and must not fold.
        assert_eq!(
            registry_description_of_symbol_literal("Symbol(desc)@42"),
            None
        );
    }
}
