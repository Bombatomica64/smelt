use serde::{Deserialize, Serialize};

use crate::ids::Symbol;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SymbolInterner {
    symbols: Vec<String>,
}

impl SymbolInterner {
    pub fn intern(&mut self, value: &str) -> Symbol {
        if let Some((idx, _)) = self
            .symbols
            .iter()
            .enumerate()
            .find(|(_, existing)| existing.as_str() == value)
        {
            return Symbol(idx as u32);
        }
        let id = Symbol(self.symbols.len() as u32);
        self.symbols.push(value.to_owned());
        id
    }

    #[must_use]
    pub fn get(&self, symbol: Symbol) -> Option<&str> {
        self.symbols.get(symbol.0 as usize).map(String::as_str)
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct OriginalNameTable {
    names: Vec<Option<String>>,
}

impl OriginalNameTable {
    pub fn record(&mut self, symbol: Symbol, original: impl Into<String>) {
        let idx = symbol.0 as usize;
        if self.names.len() <= idx {
            self.names.resize_with(idx + 1, || None);
        }
        self.names[idx] = Some(original.into());
    }

    #[must_use]
    pub fn get(&self, symbol: Symbol) -> Option<&str> {
        self.names.get(symbol.0 as usize).and_then(Option::as_deref)
    }
}
