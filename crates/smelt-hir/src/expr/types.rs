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
        BinOp::Eq => "==",
        BinOp::NotEq => "!=",
        BinOp::Lt => "<",
        BinOp::Lte => "<=",
        BinOp::Gt => ">",
        BinOp::Gte => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
        BinOp::Shl => "<<",
        BinOp::Shr => ">>",
        BinOp::UShr => ">>>",
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
