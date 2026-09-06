//! The one table that relates a well-known ECMAScript symbol's *value* to the
//! property *key* it indexes.
//!
//! `Symbol.iterator` is two different things in a JavaScript program:
//!
//! * a **value** — `typeof Symbol.iterator === 'symbol'`, it can be stored in a
//!   variable, passed to `isSymbol`, and compared for identity, and
//! * a **property key** — `obj[Symbol.iterator]` names one member, and every
//!   reference to that symbol names the same member because the symbol is a
//!   constant of the language.
//!
//! Smelt models the value as `SmeltUnknown::Symbol(<spelling>)` (the same
//! representation `Symbol()` and `Symbol.for()` values use) and the key as the
//! synthetic member name `__smelt_symbol_<snake_name>`. Those two spellings have
//! to agree, or a value-position read and a key-position read of one symbol
//! disagree about which member they name. This module owns both spellings and
//! the mapping between them, so the frontend (which lowers the value and folds
//! static computed keys) and the Rust runtime prelude (which maps a *runtime*
//! symbol value to a storage key in `smelt_property_key`) cannot drift apart.
//!
//! Only well-known symbols belong here. A unique `Symbol('d')` has fresh
//! identity per evaluation and no stable key; a `Symbol.for('d')` registry
//! symbol has its own description-derived scheme (see the frontend's
//! `computed_key_symbols` module).

/// Synthetic member-name prefix for well-known ECMAScript symbols.
///
/// The historical `Symbol.iterator` key (`__smelt_symbol_iterator`) predates the
/// table and keeps its exact spelling; every other well-known symbol derives its
/// key from this prefix plus the snake-cased symbol name, so the spellings never
/// collide with ordinary string members.
const KEY_PREFIX: &str = "__smelt_symbol_";

/// The well-known symbols Smelt models as static members.
///
/// A name absent from this list keeps the honest dynamic-key path rather than
/// folding to a fabricated member.
pub const WELL_KNOWN_SYMBOL_NAMES: &[&str] = &[
    "iterator",
    "asyncIterator",
    "hasInstance",
    "isConcatSpreadable",
    "match",
    "matchAll",
    "replace",
    "search",
    "species",
    "split",
    "toPrimitive",
    "toStringTag",
    "unscopables",
];

/// Return whether `name` is a modeled well-known symbol name.
#[must_use]
pub fn is_well_known_symbol_name(name: &str) -> bool {
    WELL_KNOWN_SYMBOL_NAMES.contains(&name)
}

/// The stable synthetic property key `Symbol.<name>` indexes.
///
/// Returns `None` for symbol names Smelt does not model as static members.
#[must_use]
pub fn storage_key(name: &str) -> Option<String> {
    if !is_well_known_symbol_name(name) {
        return None;
    }
    if name == "iterator" {
        // Keep the pre-existing spelling that member access, the interface
        // lookup and the coercion emitter already read.
        return Some(format!("{KEY_PREFIX}iterator"));
    }
    Some(format!("{KEY_PREFIX}{}", camel_to_snake_ascii(name)))
}

/// The runtime *value* spelling of `Symbol.<name>`.
///
/// Mirrors JavaScript, where `Symbol.iterator.description` is
/// `"Symbol.iterator"` and `String(Symbol.iterator)` is
/// `"Symbol(Symbol.iterator)"`. The spelling is what
/// `SmeltUnknown::Symbol(..)` carries, and [`storage_key_for_spelling`] maps it
/// back to the key.
#[must_use]
pub fn value_spelling(name: &str) -> Option<String> {
    if !is_well_known_symbol_name(name) {
        return None;
    }
    Some(format!("Symbol.{name}"))
}

/// The property key a runtime well-known symbol *value* indexes.
///
/// `spelling` is the description carried by `SmeltUnknown::Symbol(..)`; the
/// inverse of [`value_spelling`]. Returns `None` for a unique or registry
/// symbol, whose keys use the generic `__smelt_symbol:<description>` form.
#[must_use]
pub fn storage_key_for_spelling(spelling: &str) -> Option<String> {
    storage_key(spelling.strip_prefix("Symbol.")?)
}

/// Every `(value spelling, storage key)` pair, for emitters that need the whole
/// table (the generated Rust prelude renders it as a `match`).
#[must_use]
pub fn spelling_key_pairs() -> Vec<(String, String)> {
    WELL_KNOWN_SYMBOL_NAMES
        .iter()
        .filter_map(|name| Some((value_spelling(name)?, storage_key(name)?)))
        .collect()
}

/// Lower-case an ASCII `camelCase` symbol name into `snake_case`.
///
/// Well-known symbol names are fixed ASCII identifiers (`asyncIterator`,
/// `toStringTag`, …), so this is a small dedicated converter rather than a reuse
/// of general source-name folding, keeping the synthetic spelling stable and
/// independent of interning rules.
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

    /// `Symbol.iterator` keeps the established key spelling.
    #[test]
    fn iterator_keeps_established_spelling() {
        assert_eq!(storage_key("iterator").as_deref(), Some("__smelt_symbol_iterator"));
    }

    /// Multi-word names snake-case into the shared prefix scheme.
    #[test]
    fn multi_word_names_use_snake_case_scheme() {
        assert_eq!(
            storage_key("asyncIterator").as_deref(),
            Some("__smelt_symbol_async_iterator")
        );
        assert_eq!(
            storage_key("toStringTag").as_deref(),
            Some("__smelt_symbol_to_string_tag")
        );
    }

    /// An unmodeled name folds to no key at all.
    #[test]
    fn unmodeled_symbol_name_does_not_fold() {
        assert_eq!(storage_key("madeUpSymbol"), None);
        assert_eq!(value_spelling("madeUpSymbol"), None);
    }

    /// The value spelling and the key round-trip through one table, which is the
    /// property that keeps `Symbol.x` as a value and as a key in agreement.
    #[test]
    fn value_spelling_round_trips_to_its_key() {
        for name in WELL_KNOWN_SYMBOL_NAMES {
            let spelling = value_spelling(name).expect("modeled name has a spelling");
            assert_eq!(
                storage_key_for_spelling(&spelling),
                storage_key(name),
                "{name} must map back to its own key"
            );
        }
    }

    /// A unique or registry symbol description is not a well-known spelling.
    #[test]
    fn other_symbol_spellings_do_not_map() {
        assert_eq!(storage_key_for_spelling("Symbol(a)@42"), None);
        assert_eq!(storage_key_for_spelling("Symbol.for(a)"), None);
    }
}
