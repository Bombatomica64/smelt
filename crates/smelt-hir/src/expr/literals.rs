//! Literal and `unknown` tag expression payloads.

use serde::{Deserialize, Serialize};

/// A literal value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    /// A boolean literal.
    Bool(bool),
    /// An integer literal.
    Int(i64),
    /// A floating-point literal.
    Float(f64),
    /// A string literal.
    String(String),
    /// A JavaScript symbol literal lowered as an opaque runtime value.
    Symbol(String),
    /// The None/null literal.
    None,
}

/// Runtime tags supported by TypeScript `unknown` narrowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnknownKind {
    /// JavaScript `null`.
    Null,
    /// Boolean value.
    Bool,
    /// Number value.
    Number,
    /// String value.
    String,
    /// Symbol value.
    Symbol,
    /// Function value.
    Function,
    /// Array value.
    Array,
    /// Object value.
    Object,
}
