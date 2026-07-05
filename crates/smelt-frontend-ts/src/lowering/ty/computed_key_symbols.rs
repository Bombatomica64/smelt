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

/// Synthetic member-name prefix for well-known ECMAScript symbols.
///
/// The historical `Symbol.iterator` key (`__smelt_symbol_iterator`) predates
/// this table and keeps its exact spelling for source compatibility; every other
/// well-known symbol derives its key from this prefix plus the snake-cased
/// symbol name so the spellings never collide with ordinary string members.
const WELL_KNOWN_SYMBOL_PREFIX: &str = "__smelt_symbol_";

/// Synthetic member-name prefix for `Symbol.for(...)` registry symbols.
///
/// The registry description string is sanitized into an identifier-safe suffix
/// (see [`registry_key_suffix`]) so, for example,
/// `Symbol.for("@ts-pattern/matcher")` and a `const matcher = Symbol.for(...)`
/// alias both resolve to `__smelt_symbol_for_ts_pattern_matcher`.
const REGISTRY_SYMBOL_PREFIX: &str = "__smelt_symbol_for_";

/// Return the stable synthetic member key for a well-known `Symbol.<name>`.
///
/// Returns `None` for symbol names Smelt does not model as static members, which
/// keeps genuinely unsupported symbol keys on the honest dynamic-key path.
///
/// `Symbol.iterator` retains its established `__smelt_symbol_iterator` spelling
/// so previously-lowered iterator members keep matching; the remaining
/// well-known symbols use the shared `__smelt_symbol_<name>` scheme.
pub(in crate::lowering) fn well_known_symbol_key(name: &str) -> Option<String> {
    let modeled = matches!(
        name,
        "iterator"
            | "asyncIterator"
            | "hasInstance"
            | "isConcatSpreadable"
            | "match"
            | "matchAll"
            | "replace"
            | "search"
            | "species"
            | "split"
            | "toPrimitive"
            | "toStringTag"
            | "unscopables"
    );
    if !modeled {
        return None;
    }
    if name == "iterator" {
        // Keep the pre-existing spelling that member access and the coercion
        // emitter already read (`__smelt_symbol_iterator`).
        return Some(format!("{WELL_KNOWN_SYMBOL_PREFIX}iterator"));
    }
    Some(format!(
        "{WELL_KNOWN_SYMBOL_PREFIX}{}",
        camel_to_snake_ascii(name)
    ))
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

/// Lower-case an ASCII `camelCase` symbol name into `snake_case`.
///
/// Well-known symbol names are fixed ASCII identifiers (`asyncIterator`,
/// `toStringTag`, …), so this is a small dedicated converter rather than a reuse
/// of the general source-name folding, keeping the synthetic spelling stable and
/// independent of source-name interning rules.
fn camel_to_snake_ascii(name: &str) -> String {
    let mut output = String::with_capacity(name.len() + 4);
    for (index, ch) in name.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if index != 0 {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
        } else {
            output.push(ch);
        }
    }
    output
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
