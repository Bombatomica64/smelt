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

/// Synthetic member-name prefix for `Symbol.for(...)` registry symbols.
///
/// The registry description string is sanitized into an identifier-safe suffix
/// (see [`registry_key_suffix`]) so, for example,
/// `Symbol.for("@ts-pattern/matcher")` and a `const matcher = Symbol.for(...)`
/// alias both resolve to `__smelt_symbol_for_ts_pattern_matcher`.
const REGISTRY_SYMBOL_PREFIX: &str = "__smelt_symbol_for_";

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

/// Return the stable synthetic member key for a `Symbol.for(description)`.
///
/// The description string is sanitized so the resulting key is a valid,
/// collision-resistant identifier while remaining a pure function of the
/// registry description (every reference to the same registry symbol folds to
/// the same key).
pub(in crate::lowering) fn registry_symbol_key(description: &str) -> String {
    format!("{REGISTRY_SYMBOL_PREFIX}{}", registry_key_suffix(description))
}

/// Extract the registry description from a lowered `Symbol.for(...)` literal.
///
/// `Symbol.for(desc)` values lower to the stable literal string
/// `"Symbol.for(<desc>)"` (see the `Symbol` call dispatch), while unique
/// `Symbol(...)` values carry an unstable span-tagged spelling. Only the
/// registry form yields a stable key, so this returns `Some(desc)` for the
/// former and `None` for the latter.
pub(in crate::lowering) fn registry_description_of_symbol_literal(value: &str) -> Option<&str> {
    value
        .strip_prefix("Symbol.for(")
        .and_then(|rest| rest.strip_suffix(')'))
}

/// Sanitize a `Symbol.for` description into an identifier-safe key suffix.
///
/// Non-alphanumeric characters collapse to single underscores and leading and
/// trailing underscores are trimmed, so `"@ts-pattern/matcher"` becomes
/// `"ts_pattern_matcher"`. An empty or all-symbol description falls back to
/// `"anonymous"` so the produced key is always a valid identifier.
fn registry_key_suffix(description: &str) -> String {
    let mut suffix = String::with_capacity(description.len());
    let mut pending_underscore = false;
    for ch in description.chars() {
        if ch.is_ascii_alphanumeric() {
            if pending_underscore && !suffix.is_empty() {
                suffix.push('_');
            }
            pending_underscore = false;
            suffix.push(ch.to_ascii_lowercase());
        } else {
            pending_underscore = true;
        }
    }
    if suffix.is_empty() {
        "anonymous".to_owned()
    } else {
        suffix
    }
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

    #[test]
    fn registry_key_is_deterministic_and_sanitized() {
        assert_eq!(
            registry_symbol_key("@ts-pattern/matcher"),
            "__smelt_symbol_for_ts_pattern_matcher"
        );
        assert_eq!(
            registry_symbol_key("@ts-pattern/override"),
            "__smelt_symbol_for_ts_pattern_override"
        );
        // Same description -> same key, regardless of how it was referenced.
        assert_eq!(
            registry_symbol_key("app.event"),
            registry_symbol_key("app.event")
        );
    }

    #[test]
    fn empty_registry_description_falls_back() {
        assert_eq!(registry_symbol_key(""), "__smelt_symbol_for_anonymous");
        assert_eq!(registry_symbol_key("///"), "__smelt_symbol_for_anonymous");
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
