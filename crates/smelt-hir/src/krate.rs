use serde::{Deserialize, Serialize};

use crate::body::Body;
use crate::ids::{BodyId, ItemId, ModuleId, Span, Symbol};
use crate::item::Item;
use crate::symbol::{OriginalNameTable, SymbolInterner};
use crate::ty::TypeInterner;

pub const CONSOLE_LOG_SYMBOL: &str = "console_log";

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Crate {
    pub modules: Vec<Module>,
    pub items: Vec<Item>,
    pub bodies: Vec<Body>,
    pub types: TypeInterner,
    pub symbols: SymbolInterner,
    pub names: OriginalNameTable,
}

impl Crate {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_module(&mut self, module: Module) -> ModuleId {
        let id = ModuleId(self.modules.len() as u32);
        self.modules.push(Module { id, ..module });
        id
    }

    pub fn push_item(&mut self, item: Item) -> ItemId {
        let id = ItemId(self.items.len() as u32);
        self.items.push(item);
        id
    }

    pub fn push_body(&mut self, body: Body) -> BodyId {
        let id = BodyId(self.bodies.len() as u32);
        self.bodies.push(Body { id, ..body });
        id
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub id: ModuleId,
    pub name: String,
    pub source: SourceFile,
    pub imports: Vec<Import>,
    pub items: Vec<ItemId>,
    pub body: Option<BodyId>,
}

impl Module {
    #[must_use]
    pub fn new(name: impl Into<String>, source: SourceFile) -> Self {
        Self {
            id: ModuleId(u32::MAX),
            name: name.into(),
            source,
            imports: Vec::new(),
            items: Vec::new(),
            body: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    pub path: String,
    pub language: Language,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    TypeScript,
    Python,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub module: String,
    pub name: Symbol,
    pub alias: Option<Symbol>,
    pub span: Span,
}
