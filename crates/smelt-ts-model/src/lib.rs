//! Plain-data model of TypeScript 7 type facts.
//!
//! `smelt-ts-types` embeds the tsrs checker (an unpublished git dependency)
//! and produces these facts; Smelt's TypeScript frontend and reports consume
//! them. Keeping the data in its own dependency-free crate lets the
//! frontend stay publishable while still reading the checker's answers: the
//! frontend depends on this crate only, and the transpiler hands it the facts
//! when the checker is available.
//!
//! [`TsTypeFacts`] maps an exact source span (UTF-8 byte offsets, the same
//! spans oxc reports) to the type the checker resolved for the node there.
//! [`model`] describes those types.

pub mod model;

use std::collections::HashMap;

pub use model::{TsType, TsTypeId, TsTypeKind, TsTypeTable};

/// Byte span `(start, end)` of a node's tokens.
pub type SpanKey = (u32, u32);

/// Type of each value-position node in one file, keyed by token span.
pub type FileTypes = HashMap<SpanKey, TsTypeId>;

/// Severity of a [`TsDiagnostic`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum TsDiagnosticCategory {
    /// A type or syntax error; the program is not valid TypeScript.
    Error,
    /// A warning.
    Warning,
    /// A suggestion.
    Suggestion,
    /// An informational message.
    Message,
}

/// One TypeScript diagnostic.
#[derive(Clone, PartialEq, Eq, Debug)]
#[expect(
    clippy::exhaustive_structs,
    reason = "plain data built by the checker bridge in another crate"
)]
pub struct TsDiagnostic {
    /// Program path of the file, or `None` for global diagnostics.
    pub file: Option<String>,
    /// Byte offset where the diagnostic starts.
    pub start: Option<u32>,
    /// Byte offset where it ends.
    pub end: Option<u32>,
    /// TypeScript error code (`2322` for TS2322).
    pub code: i32,
    /// Severity.
    pub category: TsDiagnosticCategory,
    /// Message text with its elaboration chain flattened, as `tsc` prints it.
    pub message: String,
}

/// Everything the TypeScript checker resolved for one set of sources.
#[derive(Debug, Clone, Default)]
pub struct TsTypeFacts {
    /// Arena of every type referenced by the facts.
    pub types: TsTypeTable,
    /// Diagnostics for the whole program, in checker order.
    pub diagnostics: Vec<TsDiagnostic>,
    /// Per input path, the type of each value-position node keyed by token span.
    by_span: HashMap<String, FileTypes>,
    /// Per input path, the evaluated type of each type annotation node.
    type_nodes: HashMap<String, FileTypes>,
    /// Per input path, the final type of each evolving-array variable
    /// (`const xs = []` grown by pushes), keyed by its declaration name.
    evolved_arrays: HashMap<String, FileTypes>,
}

impl TsTypeFacts {
    /// Assembles facts from their parts (the checker bridge's constructor).
    #[must_use]
    pub fn new(
        types: TsTypeTable,
        diagnostics: Vec<TsDiagnostic>,
        by_span: HashMap<String, FileTypes>,
        type_nodes: HashMap<String, FileTypes>,
    ) -> Self {
        Self {
            types,
            diagnostics,
            by_span,
            type_nodes,
            evolved_arrays: HashMap::new(),
        }
    }

    /// Adds the evolved types of evolving-array variables (see
    /// [`Self::evolved_array_type_id_at`]).
    #[must_use]
    pub fn with_evolved_arrays(mut self, evolved_arrays: HashMap<String, FileTypes>) -> Self {
        self.evolved_arrays = evolved_arrays;
        self
    }

    /// The type TypeScript evolved for the variable whose declaration name
    /// spans `start..end` in `path`, when it is declared `= []` without an
    /// annotation and grown afterwards (`xs.push(v)`).
    ///
    /// TypeScript reports `any[]` at the declaration itself and only shows the
    /// evolved type (`T[]`) at later reads; this is that type at the last read
    /// in source order. `None` when nothing was pushed (`never[]`), the element
    /// stays `any`, or the variable is never read.
    #[must_use]
    pub fn evolved_array_type_id_at(&self, path: &str, start: u32, end: u32) -> Option<TsTypeId> {
        self.evolved_arrays.get(path)?.get(&(start, end)).copied()
    }

    /// The type TypeScript evaluates the type annotation spanning
    /// `start..end` in `path` to (`getTypeFromTypeNode`): utility types,
    /// conditional, indexed-access and mapped types come back evaluated
    /// wherever their arguments are not free type parameters.
    #[must_use]
    pub fn type_node_at(&self, path: &str, start: u32, end: u32) -> Option<&TsType> {
        self.types.get(self.type_node_id_at(path, start, end)?)
    }

    /// Like [`Self::type_node_at`] but returns the interned id.
    #[must_use]
    pub fn type_node_id_at(&self, path: &str, start: u32, end: u32) -> Option<TsTypeId> {
        self.type_nodes.get(path)?.get(&(start, end)).copied()
    }

    /// The checker's type for the node spanning `start..end` in `path`.
    ///
    /// `path` is the path the file was analysed under and the offsets are
    /// UTF-8 byte offsets of the node's tokens (oxc's `Span`). Returns `None`
    /// when no value-position node has exactly that span.
    #[must_use]
    pub fn type_at(&self, path: &str, start: u32, end: u32) -> Option<&TsType> {
        let id = *self.by_span.get(path)?.get(&(start, end))?;
        self.types.get(id)
    }

    /// Like [`Self::type_at`] but returns the interned id.
    #[must_use]
    pub fn type_id_at(&self, path: &str, start: u32, end: u32) -> Option<TsTypeId> {
        self.by_span.get(path)?.get(&(start, end)).copied()
    }

    /// Whether facts were collected for `path`.
    #[must_use]
    pub fn has_file(&self, path: &str) -> bool {
        self.by_span.contains_key(path)
    }

    /// Diagnostics that are errors.
    pub fn errors(&self) -> impl Iterator<Item = &TsDiagnostic> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.category == TsDiagnosticCategory::Error)
    }
}
