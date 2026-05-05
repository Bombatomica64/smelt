//! Identifier types and source location information for HIR.

use serde::{Deserialize, Serialize};

/// Unique identifier for a source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileId(pub u32);

/// Unique identifier for a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModuleId(pub u32);

/// Unique identifier for an item (function, class, const, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ItemId(pub u32);

/// Unique identifier for a body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BodyId(pub u32);

/// Unique identifier for a local variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalId(pub u32);

/// Unique identifier for an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ExprId(pub u32);

/// Unique identifier for a statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StmtId(pub u32);

/// Unique identifier for a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockId(pub u32);

/// Unique identifier for a pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PatternId(pub u32);

/// Unique identifier for a symbol (interned string).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Symbol(pub u32);

/// Unique identifier for a type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TypeId(pub u32);

/// Converts a collection length into a compact HIR ID index.
///
/// HIR IDs are stored as `u32`; this helper centralizes the overflow check so
/// callers do not silently truncate large collections.
#[must_use]
pub fn id_index(len: usize) -> u32 {
    u32::try_from(len).expect("HIR ID space exhausted")
}

/// Source location information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    /// The file containing this span.
    pub file: FileId,
    /// The starting byte offset.
    pub start: u32,
    /// The ending byte offset.
    pub end: u32,
}

impl Span {
    /// Creates a new span with the given file and byte range.
    #[must_use]
    pub const fn new(file: FileId, start: u32, end: u32) -> Self {
        Self { file, start, end }
    }
}
