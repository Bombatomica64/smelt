use serde::{Deserialize, Serialize};

use crate::ids::{Symbol, id_index};

/// Interns strings into compact `Symbol` identifiers.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SymbolInterner {
    /// Strings in insertion order.
    symbols: Vec<String>,
}

impl SymbolInterner {
    /// Returns the `Symbol` for `value`, inserting it if needed.
    pub fn intern(&mut self, value: &str) -> Symbol {
        if let Some((idx, _)) = self
            .symbols
            .iter()
            .enumerate()
            .find(|(_, existing)| existing.as_str() == value)
        {
            return Symbol(id_index(idx));
        }
        let id = Symbol(id_index(self.symbols.len()));
        self.symbols.push(value.to_owned());
        id
    }

    #[must_use]
    /// Resolves `symbol` back to its interned string, if it still exists.
    pub fn get(&self, symbol: Symbol) -> Option<&str> {
        self.symbols.get(symbol.0 as usize).map(String::as_str)
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
