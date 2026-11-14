//! Core unary and binary operator kinds.

use serde::{Deserialize, Serialize};

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    /// Addition operator.
    Add,
    /// Subtraction operator.
    Sub,
    /// Multiplication operator.
    Mul,
    /// Division operator.
    Div,
    /// Remainder operator.
    Rem,
    /// Equality operator.
    Eq,
    /// Inequality operator.
    NotEq,
    /// `Object.is` / `SameValue` equality (`NaN` equals `NaN`, `+0` differs from
    /// `-0`, objects by reference). Produced by `Object.is` and the
    /// `a === b || Object.is(a, b)` `SameValueZero` idiom — NOT by bare `===`.
    StrictEq,
    /// Negation of [`BinOp::StrictEq`] (`Object.is` / `SameValue` inequality).
    StrictNotEq,
    /// JavaScript `===`: reference identity for objects/arrays/functions, value
    /// equality for primitives, and `NaN !== NaN` (`+0 === -0`). Distinct from
    /// [`BinOp::Eq`] (structural/deep, used by `isDeepEqual` and the
    /// `toEqual`/`toStrictEqual` matchers) and [`BinOp::StrictEq`] (`SameValue`).
    JsStrictEq,
    /// JavaScript `!==`, the negation of [`BinOp::JsStrictEq`].
    JsStrictNotEq,
    /// JavaScript `==` between operands whose types can hold values of
    /// different kinds (a `number | bigint`, a `string | number`, `unknown`):
    /// ECMA-262 `IsLooselyEqual`, which converts across kinds (`"1" == 1`,
    /// `2n == 2`, `null == undefined`). A `==` whose operands are of one kind
    /// is that kind's strict equality and lowers to [`BinOp::Eq`] (primitives)
    /// or [`BinOp::JsStrictEq`] (objects) instead.
    JsLooseEq,
    /// JavaScript `!=`, the negation of [`BinOp::JsLooseEq`].
    JsLooseNotEq,
    /// Less-than operator.
    Lt,
    /// Less-than-or-equal operator.
    Lte,
    /// Greater-than operator.
    Gt,
    /// Greater-than-or-equal operator.
    Gte,
    /// Logical AND operator.
    And,
    /// Logical OR operator.
    Or,
    /// Signed left shift operator.
    Shl,
    /// Signed right shift operator.
    Shr,
    /// Unsigned right shift operator.
    UShr,
    /// Bitwise AND operator (`&`). JavaScript coerces both operands to 32-bit
    /// signed integers (`ToInt32`) before the operation and returns a signed
    /// 32-bit number.
    BitAnd,
    /// Bitwise OR operator (`|`), with the same `ToInt32` JavaScript semantics
    /// as [`BinOp::BitAnd`].
    BitOr,
    /// Bitwise XOR operator (`^`), with the same `ToInt32` JavaScript semantics
    /// as [`BinOp::BitAnd`].
    BitXor,
    /// `Math.imul(a, b)`: the low 32 bits of `ToInt32(a) * ToInt32(b)`, read as
    /// a signed int32. An operator rather than a call so it is visible to the
    /// int32 proofs like the bitwise operators it belongs with.
    IMul,
}

/// Returns the text representation of a binary operator.
#[must_use]
pub const fn bin_op_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Rem => "%",
        BinOp::Eq | BinOp::StrictEq | BinOp::JsStrictEq | BinOp::JsLooseEq => "==",
        BinOp::NotEq | BinOp::StrictNotEq | BinOp::JsStrictNotEq | BinOp::JsLooseNotEq => "!=",
        BinOp::Lt => "<",
        BinOp::Lte => "<=",
        BinOp::Gt => ">",
        BinOp::Gte => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::UShr => ">>>",
        BinOp::BitAnd => "&",
        BinOp::BitOr => "|",
        BinOp::BitXor => "^",
        BinOp::IMul => "imul",
    }
}

impl BinOp {
    /// Whether this is one of the equality operators, in either polarity.
    ///
    /// Every pass that asks "is this a comparison" asks here, so a new equality
    /// operator is classified in one place (exhaustively matched below).
    #[must_use]
    pub const fn is_equality(self) -> bool {
        self.equality_polarity().is_some()
    }

    /// Whether this is an equality operator's negation (`!=`, `!==`, …).
    #[must_use]
    pub const fn is_negated_equality(self) -> bool {
        matches!(self.equality_polarity(), Some(true))
    }

    /// `Some(negated)` for an equality operator, `None` for any other.
    const fn equality_polarity(self) -> Option<bool> {
        match self {
            Self::Eq | Self::StrictEq | Self::JsStrictEq | Self::JsLooseEq => Some(false),
            Self::NotEq | Self::StrictNotEq | Self::JsStrictNotEq | Self::JsLooseNotEq => {
                Some(true)
            }
            Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Rem
            | Self::Lt
            | Self::Lte
            | Self::Gt
            | Self::Gte
            | Self::And
            | Self::Or
            | Self::Shl
            | Self::Shr
            | Self::UShr
            | Self::BitAnd
            | Self::BitOr
            | Self::BitXor
            | Self::IMul => None,
        }
    }

    /// Whether this compares its operands into a boolean: an equality or an
    /// ordering (`<`, `<=`, `>`, `>=`).
    #[must_use]
    pub const fn is_comparison(self) -> bool {
        self.is_equality() || matches!(self, Self::Lt | Self::Lte | Self::Gt | Self::Gte)
    }

    /// The comparison that answers the opposite (`<` → `>=`, `==` → `!=`), or
    /// `None` for an operator that is not a comparison.
    #[must_use]
    pub const fn negated(self) -> Option<Self> {
        Some(match self {
            Self::Lt => Self::Gte,
            Self::Lte => Self::Gt,
            Self::Gt => Self::Lte,
            Self::Gte => Self::Lt,
            Self::Eq => Self::NotEq,
            Self::NotEq => Self::Eq,
            Self::StrictEq => Self::StrictNotEq,
            Self::StrictNotEq => Self::StrictEq,
            Self::JsStrictEq => Self::JsStrictNotEq,
            Self::JsStrictNotEq => Self::JsStrictEq,
            Self::JsLooseEq => Self::JsLooseNotEq,
            Self::JsLooseNotEq => Self::JsLooseEq,
            Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Rem
            | Self::And
            | Self::Or
            | Self::Shl
            | Self::Shr
            | Self::UShr
            | Self::BitAnd
            | Self::BitOr
            | Self::BitXor
            | Self::IMul => return None,
        })
    }
}

/// A unary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    /// Logical NOT operator.
    Not,
    /// Negation operator.
    Neg,
}
