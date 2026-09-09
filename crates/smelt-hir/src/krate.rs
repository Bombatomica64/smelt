//! Crate and module representation for HIR.

use serde::{Deserialize, Serialize};

use crate::body::Body;
use crate::ids::{BodyId, FileId, ItemId, ModuleId, Span, Symbol, id_index};
use crate::item::Item;
use crate::symbol::{OriginalNameTable, SymbolInterner};
use crate::ty::TypeInterner;

/// Symbol name used for console.log function.
pub const CONSOLE_LOG_SYMBOL: &str = "console_log";
/// Internal exact stdout write builtin used for specialization replay.
pub const CONSOLE_WRITE_SYMBOL: &str = "console_write";
/// Internal exact stderr write builtin used for specialization replay.
pub const CONSOLE_ERROR_WRITE_SYMBOL: &str = "console_error_write";
/// Synthesized private field name backing a class/interface index signature.
///
/// A class declaring `[key: string]: T` (issue #84) carries a real runtime
/// keyed store under this field so dynamic keyed writes round-trip. The
/// frontend synthesizes the field and codegen routes keyed access to it; the
/// name is shared here so both agree on the single store field identifier.
pub const CLASS_INDEX_STORE_FIELD: &str = "__smelt_index_store";

/// A crate containing all HIR data structures.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Crate {
    /// All modules in this crate.
    pub modules: Vec<Module>,
    /// All items (functions, classes, etc.) in this crate.
    pub items: Vec<Item>,
    /// All function/constant bodies in this crate.
    pub bodies: Vec<Body>,
    /// Type interner for managing interned types.
    pub types: TypeInterner,
    /// Symbol interner for managing interned strings.
    pub symbols: SymbolInterner,
    /// Table of original names before any transformations.
    pub names: OriginalNameTable,
}

impl Crate {
    /// Creates a new empty crate.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a module to the crate and returns its ID.
    pub fn push_module(&mut self, module: Module) -> ModuleId {
        let id = ModuleId(id_index(self.modules.len()));
        self.modules.push(Module { id, ..module });
        id
    }

    /// Adds an item to the crate and returns its ID.
    pub fn push_item(&mut self, item: Item) -> ItemId {
        let id = ItemId(id_index(self.items.len()));
        self.items.push(item);
        id
    }

    /// Adds a body to the crate and returns its ID.
    pub fn push_body(&mut self, body: Body) -> BodyId {
        let id = BodyId(id_index(self.bodies.len()));
        self.bodies.push(Body { id, ..body });
        id
    }
}

/// A module in a crate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    /// The module ID.
    pub id: ModuleId,
    /// The module name.
    pub name: String,
    /// Source file information.
    pub source: SourceFile,
    /// Imports in this module.
    pub imports: Vec<Import>,
    /// Item IDs defined in this module.
    pub items: Vec<ItemId>,
    /// Optional module-level body.
    pub body: Option<BodyId>,
    /// Whether the module body needs the async runtime.
    ///
    /// True when top-level code awaits, or when it starts work the event loop
    /// has to finish before the program exits (a floating promise, a timer).
    /// The module body becomes the emitted `main`, so this is what makes that
    /// `main` an `async fn` with a drain at the end rather than a synchronous
    /// function that returns while work is still queued.
    pub is_async: bool,
}

impl Module {
    /// Creates a new module with the given name and source file.
    #[must_use]
    pub fn new(name: impl Into<String>, source: SourceFile) -> Self {
        Self {
            id: ModuleId(u32::MAX),
            name: name.into(),
            source,
            is_async: false,
            imports: Vec::new(),
            items: Vec::new(),
            body: None,
        }
    }
}

/// Source file information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    /// The file path.
    pub path: String,
    /// The programming language of the source file.
    pub language: Language,
    /// The file this module was lowered from.
    ///
    /// Every [`crate::Span`] carries the same `FileId`, so this is what lets a
    /// later pass recover the SOURCE LANGUAGE of an arbitrary expression: build
    /// a `FileId -> Language` table from the crate's modules and look the span
    /// up in it. One crate can mix TypeScript and Python modules (the frontend
    /// is chosen per file), so the language is not a crate-wide property and
    /// cannot be recovered any other way once HIR is flattened.
    pub file: FileId,
}

/// Programming language for a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Language {
    /// TypeScript source file.
    TypeScript,
    /// Python source file.
    Python,
}

/// An import statement in a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    /// The module being imported from.
    pub module: String,
    /// The name being imported.
    pub name: Symbol,
    /// Optional alias for the imported name.
    pub alias: Option<Symbol>,
    /// Source location of the import.
    pub span: Span,
}
