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
    /// A JavaScript bigint literal (`123n`), as its exact decimal digits.
    ///
    /// Digits rather than a fixed-width integer because a bigint literal is
    /// unbounded (`9_999_999_999_999_999_999_999n` does not fit `i64`). The
    /// text is canonical: no `n` suffix, no numeric separators, no radix
    /// prefix, an optional leading `-`, and no leading zeros.
    BigInt(String),
    /// A string literal.
    String(String),
    /// A JavaScript symbol literal lowered as an opaque runtime value.
    Symbol(String),
    /// The `undefined` literal value.
    Undefined,
    /// The None/null literal.
    None,
}

/// Runtime tags supported by TypeScript `unknown` narrowing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnknownKind {
    /// JavaScript `null`.
    Null,
    /// JavaScript `undefined`.
    Undefined,
    /// Boolean value.
    Bool,
    /// Number value.
    Number,
    /// JavaScript `bigint` value.
    BigInt,
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
    /// Promise value represented by the erased runtime sentinel.
    Promise,
}

impl UnknownKind {
    /// Whether a value whose runtime kind is `self` passes a runtime-kind test
    /// for `check`.
    ///
    /// Exact, except that `typeof x === "object"` also holds for arrays,
    /// promises and `null`: JavaScript gives all three the `typeof` string
    /// `"object"` (see [`Self::typeof_name`]).
    #[must_use]
    pub fn satisfies(self, check: Self) -> bool {
        self == check
            || (check == Self::Object && matches!(self, Self::Array | Self::Promise | Self::Null))
    }

    /// The string JavaScript's `typeof` answers for a value of this kind.
    #[must_use]
    pub const fn typeof_name(self) -> &'static str {
        match self {
            Self::Bool => "boolean",
            Self::Number => "number",
            Self::BigInt => "bigint",
            Self::String => "string",
            Self::Symbol => "symbol",
            Self::Function => "function",
            Self::Undefined => "undefined",
            Self::Null | Self::Array | Self::Object | Self::Promise => "object",
        }
    }
}
