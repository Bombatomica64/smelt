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
/// A class declaring `[key: string]: T` carries a real runtime
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
    /// Every decision a frontend made to erase a value to `Type::Unknown`,
    /// with its reason and source span; see [`crate::erasure`].
    ///
    /// A side table rather than a field on the type: `Type::Unknown` is
    /// interned once, so the reason cannot live on the `TypeId`.
    #[serde(default)]
    pub erasures: Vec<crate::erasure::ErasureRecord>,
    /// Record fields that some object literal supplies through a `get`
    /// accessor, as `(record name, field name)`.
    ///
    /// JavaScript evaluates a getter on every read, so a hand port stores
    /// such a field as a closure and calls it where the field is read. HIR
    /// keeps the field's declared type (`readonly cachedValue: T`); the
    /// backend renders the storage as an accessor of `T` for exactly these
    /// fields, so plain data records keep plain fields. A literal that
    /// supplies one of these fields with a plain value is wrapped as a
    /// constant accessor.
    #[serde(default)]
    pub accessor_fields: Vec<(crate::Symbol, crate::Symbol)>,
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

    /// Adds an interface item, or replaces the one already lowered from the
    /// same declaration, and returns its ID.
    ///
    /// One source declaration is one item. A frontend lowers the same
    /// interface declaration more than once (predeclaration passes, then the
    /// module's own lowering; a type alias resolved again from another site),
    /// and each later lowering sees more of the crate, so it REPLACES the
    /// earlier item. The declaration
    /// is identified by name and span; a same-named interface at another span
    /// (interface merging, a `.d.ts` then its `.ts`) is a separate item.
    /// Appending instead left several interfaces of one name, which MIR
    /// rejects; radash's `DebounceFunction`, remeda's
    /// `LazyFunction`).
    pub fn upsert_interface(&mut self, interface: crate::Interface) -> ItemId {
        let earlier = self.items.iter().position(|existing| {
            matches!(existing, Item::Interface(other)
                if other.name == interface.name && other.span == interface.span)
        });
        if let Some(position) = earlier
            && let Some(slot) = self.items.get_mut(position)
        {
            *slot = Item::Interface(interface);
            return ItemId(id_index(position));
        }
        self.push_item(Item::Interface(interface))
    }

    /// Adds a body to the crate and returns its ID.
    pub fn push_body(&mut self, mut body: Body) -> BodyId {
        let id = BodyId(id_index(self.bodies.len()));
        body.id = id;
        self.bodies.push(body);
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
    /// Items of other modules this module exports under a name of its own
    /// (`export { twice } from "./impl"`, a Python package's
    /// `from .impl import twice`), in source order: the module's public
    /// surface beyond [`Self::items`], which a declaration stub of it lists.
    #[serde(default)]
    pub reexports: Vec<(Symbol, ItemId)>,
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
    /// Where this module's body runs when the program starts, or `None` when
    /// it does not run (a test module, a module no entry reaches).
    ///
    /// A JavaScript or Python program evaluates its module graph once,
    /// dependencies before the modules that import them, its entries in the
    /// order the manifest lists them: each module body runs exactly once, at
    /// its position in that order. The transpiler numbers the modules so
    /// (`crate::lowering`'s `module_run_order`) and the backend runs the bodies
    /// in that order from `main`.
    #[serde(default)]
    pub run_order: Option<u32>,
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
            run_order: None,
            imports: Vec::new(),
            items: Vec::new(),
            reexports: Vec::new(),
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

#[cfg(test)]
mod tests {
    //! One interface declaration is one item (audit C12).

    use crate::{Crate, FileId, Interface, Item, Span, Symbol};

    /// An empty interface `name` declared at `start`.
    fn interface(name: u32, start: u32) -> Interface {
        Interface {
            name: Symbol(name),
            span: Span::new(FileId(0), start, start + 1),
            type_params: Vec::new(),
            extends: Vec::new(),
            fields: Vec::new(),
            methods: Vec::new(),
            decode_facts: Vec::new(),
        }
    }

    /// The same declaration lowered again replaces its item; a same-named
    /// interface at another span is a separate item.
    #[test]
    fn an_interface_lowered_twice_is_one_item() {
        let mut krate = Crate::new();
        let first = krate.upsert_interface(interface(1, 10));
        let again = krate.upsert_interface(interface(1, 10));
        assert_eq!(first, again);
        assert_eq!(krate.items.len(), 1);
        let merged = krate.upsert_interface(interface(1, 99));
        assert_ne!(merged, first);
        assert_eq!(
            krate
                .items
                .iter()
                .filter(|item| matches!(item, Item::Interface(_)))
                .count(),
            2
        );
    }
}
