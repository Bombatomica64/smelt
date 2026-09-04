//! Symbol interning and original-source-name bookkeeping for HIR.
//!
//! A `Symbol` has two distinct strings attached to it, and conflating them is a
//! correctness bug rather than a cosmetic one:
//!
//! * the **source key** — the exact spelling the symbol came from. It is what
//!   identity is decided on, because JavaScript names and object keys are
//!   case-sensitive: `Foo` and `foo` are two different things.
//! * the **rendered name** — the Rust-facing spelling (`camel_to_snake`d for
//!   value names, verbatim for type names). Several source keys may legitimately
//!   render to the same Rust identifier (`Foo` and `foo` both render `foo`), so
//!   the rendered name can never be the dedup key.

use serde::{Deserialize, Serialize};

use crate::ids::{Symbol, id_index};

/// Interns strings into compact `Symbol` identifiers.
///
/// A symbol's identity is the **pair** (rendered name, source spelling), so a
/// case fold applied on the way to a Rust identifier can never make two
/// different source names one symbol. When the two halves are equal — every
/// verbatim [`SymbolInterner::intern`] and every already-`snake_case` source
/// name — the pair degenerates to the single string, which is exactly the
/// historical keying.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SymbolInterner {
    /// Rust-facing renderings in insertion order.
    symbols: Vec<String>,
    /// Source spellings in insertion order, empty when equal to the rendering.
    ///
    /// Defaulted for backwards compatibility with previously serialized
    /// interners, where a missing entry means "source spelling == rendering".
    #[serde(default)]
    sources: Vec<String>,
}

impl SymbolInterner {
    /// Returns whether slot `idx` was interned from `rendered`/`key`.
    fn slot_matches(&self, idx: usize, key: &str, rendered: &str) -> bool {
        if self.symbols.get(idx).map(String::as_str) != Some(rendered) {
            return false;
        }
        let stored = self.sources.get(idx).map_or("", String::as_str);
        // An empty (or absent) stored spelling means it equals the rendering.
        if stored.is_empty() {
            key == rendered
        } else {
            stored == key
        }
    }

    /// Returns the `Symbol` for `value`, inserting it if needed.
    ///
    /// Both the identity key and the rendering are `value`; use
    /// [`SymbolInterner::intern_rendered`] when the two differ.
    pub fn intern(&mut self, value: &str) -> Symbol {
        self.intern_rendered(value, value)
    }

    /// Returns the `Symbol` for source spelling `key` rendered as `rendered` in
    /// generated Rust, inserting it if needed.
    ///
    /// Two source spellings that only differ by case never share a `Symbol`
    /// even though they render identically — otherwise a declaration named
    /// `Foo` and a property named `foo` alias, and whichever is lowered last
    /// silently owns the key string both are read with
    /// (`OriginalNameTable::record` is last-writer-wins).
    pub fn intern_rendered(&mut self, key: &str, rendered: &str) -> Symbol {
        for idx in 0..self.symbols.len() {
            if self.slot_matches(idx, key, rendered) {
                return Symbol(id_index(idx));
            }
        }
        let id = Symbol(id_index(self.symbols.len()));
        self.symbols.push(rendered.to_owned());
        self.sources.resize(self.symbols.len() - 1, String::new());
        self.sources
            .push(if key == rendered { String::new() } else { key.to_owned() });
        id
    }

    #[must_use]
    /// Resolves `symbol` back to its Rust-facing rendering, if it still exists.
    pub fn get(&self, symbol: Symbol) -> Option<&str> {
        self.symbols.get(symbol.0 as usize).map(String::as_str)
    }

    #[must_use]
    /// Resolves `symbol` back to the exact source spelling it was interned from.
    pub fn source(&self, symbol: Symbol) -> Option<&str> {
        let idx = symbol.0 as usize;
        let rendered = self.symbols.get(idx).map(String::as_str)?;
        Some(match self.sources.get(idx).map(String::as_str) {
            Some(stored) if !stored.is_empty() => stored,
            _ => rendered,
        })
    }
}

/// Remembers the original source names for symbols.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct OriginalNameTable {
    /// Original names indexed by symbol ID.
    names: Vec<Option<String>>,
}

impl OriginalNameTable {
    /// Records the original textual name for `symbol`.
    pub fn record(&mut self, symbol: Symbol, original: impl Into<String>) {
        let idx = symbol.0 as usize;
        if self.names.len() <= idx {
            self.names.resize_with(idx + 1, || None);
        }
        self.names[idx] = Some(original.into());
    }

    #[must_use]
    /// Returns the recorded original name for `symbol`, if present.
    pub fn get(&self, symbol: Symbol) -> Option<&str> {
        self.names.get(symbol.0 as usize).and_then(Option::as_deref)
    }
}

#[cfg(test)]
mod tests {
    use super::SymbolInterner;

    /// JavaScript names are case-sensitive, so two spellings that fold to the
    /// same Rust identifier must still be two symbols.
    #[test]
    fn case_differing_source_names_get_distinct_symbols() {
        let mut interner = SymbolInterner::default();
        let upper = interner.intern_rendered("Foo", "foo");
        let lower = interner.intern_rendered("foo", "foo");
        assert_ne!(upper, lower);
        assert_eq!(interner.get(upper), Some("foo"));
        assert_eq!(interner.get(lower), Some("foo"));
        assert_eq!(interner.source(upper), Some("Foo"));
        assert_eq!(interner.source(lower), Some("foo"));
    }

    /// Re-interning the same source spelling is idempotent.
    #[test]
    fn same_source_name_reuses_its_symbol() {
        let mut interner = SymbolInterner::default();
        let first = interner.intern_rendered("fooBar", "foo_bar");
        let second = interner.intern_rendered("fooBar", "foo_bar");
        assert_eq!(first, second);
        assert_eq!(interner.get(first), Some("foo_bar"));
    }

    /// A camel-cased source name and its already-snake_case spelling are two
    /// different source identifiers and must not collide either.
    #[test]
    fn snake_and_camel_spellings_are_distinct() {
        let mut interner = SymbolInterner::default();
        let camel = interner.intern_rendered("fooBar", "foo_bar");
        let snake = interner.intern_rendered("foo_bar", "foo_bar");
        assert_ne!(camel, snake);
    }

    /// Identity is the (rendering, spelling) pair, so a verbatim type name and
    /// a case-folded value name spelled the same stay two symbols — the
    /// renderings differ.
    #[test]
    fn verbatim_and_folded_spellings_of_one_name_stay_distinct() {
        let mut interner = SymbolInterner::default();
        let type_name = interner.intern("Foo");
        let value_name = interner.intern_rendered("Foo", "foo");
        assert_ne!(type_name, value_name);
        assert_eq!(interner.get(type_name), Some("Foo"));
        assert_eq!(interner.get(value_name), Some("foo"));
    }

    /// A folded name whose rendering is unchanged keys exactly like a verbatim
    /// intern of the same string, preserving the historical single-string key.
    #[test]
    fn unfolded_source_name_shares_the_verbatim_key() {
        let mut interner = SymbolInterner::default();
        let verbatim = interner.intern("foo");
        let folded = interner.intern_rendered("foo", "foo");
        assert_eq!(verbatim, folded);
    }
}
