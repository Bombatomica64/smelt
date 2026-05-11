//! Capture-free callback expression nodes for list-style operations.

use serde::{Deserialize, Serialize};

use crate::ids::TypeId;

use super::{BinOp, Literal, UnaryOp};

/// A capture-free callback expression tree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallbackExpr {
    /// Callback expression kind.
    pub kind: CallbackExprKind,
    /// Callback expression type.
    pub ty: TypeId,
}

/// The kind of a capture-free callback expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CallbackExprKind {
    /// A callback parameter reference by zero-based position.
    Param(usize),
    /// A literal value.
    Literal(Literal),
    /// A list literal built inside a callback.
    ListLit(Vec<CallbackExpr>),
    /// A unary operation.
    Unary {
        /// Operation to apply.
        op: UnaryOp,
        /// Operand expression.
        operand: Box<CallbackExpr>,
    },
    /// A binary operation.
    Binary {
        /// Operation to apply.
        op: BinOp,
        /// Left operand.
        lhs: Box<CallbackExpr>,
        /// Right operand.
        rhs: Box<CallbackExpr>,
    },
}
