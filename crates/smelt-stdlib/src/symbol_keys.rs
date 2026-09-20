//! The one place that relates a symbol *value* to the property *key* it indexes,
//! for the symbol kinds that are not language constants.
//!
//! A symbol is two things in a JavaScript program: a value with a unique
//! run-time identity, and a property key. Smelt models the value as
//! `SmeltUnknown::Symbol(<spelling>)` and the key as a synthetic member name, so
//! the two spellings have to agree — a computed key the frontend folds
//! statically (`{ [KEY]: 1 }`, `class C { get [KEY]() {} }`) and a key the
//! generated Rust derives at run time from an erased symbol value (`obj[prop]`
//! where `prop` reached the callee through a `SmeltUnknown` slot) must name the
//! SAME entry of the same record.
//!
//! [`well_known_symbols`](crate::well_known_symbols) owns that agreement for the
//! language's constant symbols. This module owns it for the other two kinds the
//! frontend also folds to static member spellings:
//!
//! * a **registry** symbol (`Symbol.for("d")`) is interned globally by its
//!   description, so every reference denotes one symbol, and
//! * a **unique** symbol (`Symbol("d")`) whose lowered value spelling carries the
//!   binding's source offset (`Symbol(d)@141`), which names that one symbol.
//!
//! Both fold to an identifier-safe synthetic member name, because such a key can
//! also name a *class* member and therefore has to be spellable as a Rust
//! identifier. Any other symbol spelling keeps the generic
//! `__smelt_symbol:<description>` storage form.
//!
//! Two properties are load-bearing, and both are tested here:
//!
//! 1. **Totality.** [`storage_key_for_value_spelling`] answers for every symbol
//!    spelling, and it is what both halves of the compiler consult: the frontend
//!    when it folds a computed key, and the emitted `smelt_symbol_property_key`
//!    prelude function when it keys a runtime symbol value. A symbol-keyed
//!    property an object literal wrote through the folded key was invisible to a
//!    dynamic read of the same symbol precisely because the two halves derived
//!    the key differently.
//! 2. **Reversibility.** [`value_spelling_for_storage_key`] recovers the symbol
//!    value from its stored key, which is what `Object.getOwnPropertySymbols`,
//!    `Reflect.ownKeys` and erased-Map enumeration hand back to the program. A
//!    key that merely *sanitized* the description could not be inverted, so the
//!    escape below is a reversible encoding rather than a sanitizer: an
//!    alphanumeric character stands for itself and everything else becomes
//!    `_<hex>_`.

use crate::well_known_symbols;

/// Synthetic member-name prefix for a `Symbol.for(...)` registry symbol.
pub const REGISTRY_SYMBOL_PREFIX: &str = "__smelt_symbol_for_";

/// Synthetic member-name prefix for a unique `Symbol(...)` value.
pub const UNIQUE_SYMBOL_PREFIX: &str = "__smelt_symbol_unique_";

/// Separator between a unique symbol's escaped description and its source
/// offset, mirroring the `@` of the value spelling `Symbol(d)@141`.
///
/// The offset is always decimal digits appended last, so the *last* occurrence
/// of this separator is always the one this module wrote, even when an escaped
/// description contains the same characters.
pub const UNIQUE_OFFSET_SEPARATOR: &str = "_at_";

/// Storage-key prefix for a symbol with no stable static member spelling.
pub const OPAQUE_SYMBOL_PREFIX: &str = "__smelt_symbol:";

/// The shared stem of every symbol storage key, in every spelling.
///
/// Key enumeration (`Object.keys`, `for...in`, `Object.values`) must not report
/// symbol-keyed properties, and this is the one test that covers the folded
/// keys, the well-known keys and the opaque form at once.
pub const SYMBOL_KEY_STEM: &str = "__smelt_symbol";

/// Extract the registry description from a lowered `Symbol.for(...)` value.
///
/// `Symbol.for(d)` values lower to the stable spelling `"Symbol.for(d)"`, while a
/// unique `Symbol(...)` value carries a span-tagged spelling; only the former
/// yields a description that is a global identity.
#[must_use]
pub fn registry_description_of_value_spelling(spelling: &str) -> Option<&str> {
    spelling
        .strip_prefix("Symbol.for(")
        .and_then(|rest| rest.strip_suffix(')'))
}

/// Split a unique symbol's value spelling into `(description, source offset)`.
///
/// `Symbol(d)@141` is the lowered spelling of a unique `Symbol("d")`, and the
/// offset is what distinguishes one `Symbol()` call site from another. Returns
/// `None` for any spelling that is not that shape — a well-known or registry
/// symbol, or an opaque description that came from outside the program.
#[must_use]
pub fn unique_description_and_offset(spelling: &str) -> Option<(&str, &str)> {
    if registry_description_of_value_spelling(spelling).is_some()
        || well_known_symbols::storage_key_for_spelling(spelling).is_some()
    {
        return None;
    }
    let (head, offset) = spelling.rsplit_once('@')?;
    if offset.is_empty() || !offset.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let description = head.strip_prefix("Symbol(")?.strip_suffix(')')?;
    Some((description, offset))
}

/// The synthetic member key a `Symbol.for(description)` indexes.
#[must_use]
pub fn registry_symbol_key(description: &str) -> String {
    format!("{REGISTRY_SYMBOL_PREFIX}{}", escape(description))
}

/// The synthetic member key a unique symbol's value spelling indexes.
///
/// Returns `None` for every spelling that is not a unique-symbol value spelling
/// (see [`unique_description_and_offset`]), so registry and well-known symbols
/// keep their own keys.
#[must_use]
pub fn unique_symbol_key(spelling: &str) -> Option<String> {
    let (description, offset) = unique_description_and_offset(spelling)?;
    Some(format!(
        "{UNIQUE_SYMBOL_PREFIX}{}{UNIQUE_OFFSET_SEPARATOR}{offset}",
        escape(description)
    ))
}

/// The property key the symbol value spelled `spelling` indexes.
///
/// Total over the four symbol kinds, in the order their identities are fixed:
/// well-known (a language constant), registry (interned by description), unique
/// with a span-tagged spelling (one binding, one symbol), and anything else,
/// which keeps the generic `__smelt_symbol:<description>` form so that a symbol
/// key never collides with the plain string key of its own description.
#[must_use]
pub fn storage_key_for_value_spelling(spelling: &str) -> String {
    if let Some(key) = well_known_symbols::storage_key_for_spelling(spelling) {
        return key;
    }
    if let Some(description) = registry_description_of_value_spelling(spelling) {
        return registry_symbol_key(description);
    }
    if let Some(key) = unique_symbol_key(spelling) {
        return key;
    }
    format!("{OPAQUE_SYMBOL_PREFIX}{spelling}")
}

/// The symbol value spelling a stored property key denotes, if it is one of the
/// folded symbol keys this module owns.
///
/// The exact inverse of [`storage_key_for_value_spelling`] for the registry and
/// unique forms; well-known keys invert through
/// [`well_known_symbols`](crate::well_known_symbols) and the opaque form is a
/// plain prefix strip, so the emitted prelude composes all three.
#[must_use]
pub fn value_spelling_for_storage_key(key: &str) -> Option<String> {
    if let Some(escaped) = key.strip_prefix(UNIQUE_SYMBOL_PREFIX) {
        let (escaped_description, offset) = escaped.rsplit_once(UNIQUE_OFFSET_SEPARATOR)?;
        return Some(format!(
            "Symbol({})@{offset}",
            unescape(escaped_description)
        ));
    }
    let escaped = key.strip_prefix(REGISTRY_SYMBOL_PREFIX)?;
    Some(format!("Symbol.for({})", unescape(escaped)))
}

/// Whether a stored record key is a symbol key in any of its spellings.
#[must_use]
pub fn is_symbol_storage_key(key: &str) -> bool {
    key.starts_with(SYMBOL_KEY_STEM)
}

/// Encode a symbol description as an identifier-safe, reversible key tail.
///
/// An ASCII alphanumeric character stands for itself; every other character
/// becomes `_<lowercase hex code point>_`. Since `_` itself is escaped
/// (`_5f_`), an underscore in the output can only ever open or close an escape,
/// which is what makes [`unescape`] unambiguous. `"@ts-pattern/matcher"` becomes
/// `_40_ts_2d_pattern_2f_matcher`.
///
/// The generated Rust prelude renders this same algorithm (it has to derive the
/// key from a run-time spelling and hand the spelling back for
/// `Object.getOwnPropertySymbols`), which is why the reference implementation
/// lives here rather than in either half of the compiler.
#[must_use]
pub fn escape(description: &str) -> String {
    let mut escaped = String::with_capacity(description.len());
    for ch in description.chars() {
        if ch.is_ascii_alphanumeric() {
            escaped.push(ch);
        } else {
            use ::std::fmt::Write as _;
            let _ = write!(escaped, "_{:x}_", u32::from(ch));
        }
    }
    escaped
}

/// Decode an [`escape`]d description.
///
/// An unterminated or non-hexadecimal escape cannot come from [`escape`]; it is
/// passed through verbatim rather than dropped, so decoding a key Smelt did not
/// write degrades to the key's own text instead of losing characters.
#[must_use]
pub fn unescape(escaped: &str) -> String {
    let mut decoded = String::with_capacity(escaped.len());
    let mut rest = escaped;
    while let Some(index) = rest.find('_') {
        decoded.push_str(&rest[..index]);
        let tail = &rest[index + 1..];
        let Some((hex, remainder)) = tail.split_once('_') else {
            decoded.push('_');
            decoded.push_str(tail);
            return decoded;
        };
        if let Some(ch) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
            decoded.push(ch);
        } else {
            decoded.push('_');
            decoded.push_str(hex);
            decoded.push('_');
        }
        rest = remainder;
    }
    decoded.push_str(rest);
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A registry symbol's key is a pure function of its description.
    #[test]
    fn registry_key_is_deterministic() {
        assert_eq!(
            registry_symbol_key("@ts-pattern/matcher"),
            "__smelt_symbol_for__40_ts_2d_pattern_2f_matcher"
        );
        assert_eq!(
            registry_symbol_key("app.event"),
            registry_symbol_key("app.event")
        );
        assert_ne!(registry_symbol_key("a.b"), registry_symbol_key("a_b"));
    }

    /// A unique symbol folds only when its spelling carries the span tag that
    /// identifies the one binding it came from.
    #[test]
    fn unique_key_needs_a_span_tag() {
        assert_eq!(
            unique_symbol_key("Symbol(sym)@141").as_deref(),
            Some("__smelt_symbol_unique_sym_at_141")
        );
        assert_eq!(unique_symbol_key("Symbol(sym)"), None);
        assert_eq!(unique_symbol_key("Symbol.for(sym)"), None);
        assert_eq!(unique_symbol_key("Symbol.iterator"), None);
    }

    /// The value spelling and the folded key agree for every symbol kind: the
    /// invariant a statically folded computed key and a runtime-derived key both
    /// depend on.
    #[test]
    fn value_spelling_maps_to_the_key_the_frontend_folds() {
        assert_eq!(
            storage_key_for_value_spelling("Symbol.iterator"),
            "__smelt_symbol_iterator"
        );
        assert_eq!(
            storage_key_for_value_spelling("Symbol.for(app.event)"),
            registry_symbol_key("app.event")
        );
        assert_eq!(
            storage_key_for_value_spelling("Symbol(sym)@141"),
            unique_symbol_key("Symbol(sym)@141").expect("a span-tagged spelling folds")
        );
    }

    /// An opaque symbol keeps a storage form distinct from the plain string key
    /// of its own description.
    #[test]
    fn opaque_symbol_keeps_a_distinct_key() {
        assert_eq!(storage_key_for_value_spelling("sym"), "__smelt_symbol:sym");
        assert!(is_symbol_storage_key("__smelt_symbol:sym"));
        assert!(is_symbol_storage_key("__smelt_symbol_unique_sym_at_141"));
        assert!(is_symbol_storage_key("__smelt_symbol_iterator"));
        assert!(!is_symbol_storage_key("sym"));
    }

    /// Every folded key inverts to the exact value spelling it came from, which
    /// is what lets `Object.getOwnPropertySymbols` hand back a symbol that still
    /// indexes the property it was found on.
    #[test]
    fn folded_keys_round_trip_back_to_their_value_spelling() {
        for spelling in [
            "Symbol(sym)@141",
            "Symbol()@7",
            "Symbol(a_at_b)@42",
            "Symbol(@ts-pattern/matcher)@3",
            "Symbol.for(app.event)",
            "Symbol.for(@ts-pattern/matcher)",
            "Symbol.for()",
        ] {
            let key = storage_key_for_value_spelling(spelling);
            assert_eq!(
                value_spelling_for_storage_key(&key).as_deref(),
                Some(spelling),
                "{key} must invert to {spelling}"
            );
            assert!(
                key.chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'),
                "{key} must be spellable as a Rust identifier"
            );
        }
    }

    /// The escape is reversible over the characters a description can contain.
    #[test]
    fn escape_round_trips() {
        for description in ["", "sym", "a_b", "@ns/name", "a.b-c", "über", "1", "_"] {
            assert_eq!(unescape(&escape(description)), description);
        }
    }

    /// A key Smelt did not write decodes to its own text rather than losing
    /// characters.
    #[test]
    fn a_malformed_escape_passes_through() {
        assert_eq!(unescape("a_zz_b"), "a_zz_b");
        assert_eq!(unescape("trailing_"), "trailing_");
    }
}
