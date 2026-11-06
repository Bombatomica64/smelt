//! The expression tree a Python `lambda` or comprehension lowers into before it
//! becomes a closure body.
//!
//! This is the Python frontend's own intermediate form: no HIR expression stores
//! it, so it lives with its only user.

use smelt_hir::{BinOp, Literal, LocalId, Symbol, TypeId, UnaryOp};

/// A Python callback body as an expression tree, rebuilt into a closure body.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct CallbackExpr {
    /// Callback expression kind.
    pub(super) kind: CallbackExprKind,
    /// Callback expression type.
    pub(super) ty: TypeId,
}

/// The kind of a callback expression.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum CallbackExprKind {
    /// A callback parameter reference by zero-based position.
    Param(usize),
    /// A local captured from the enclosing HIR body.
    Capture(LocalId),
    /// A literal value.
    Literal(Literal),
    /// A list literal built inside a callback.
    ListLit(Vec<CallbackExpr>),
    /// Indexed access inside a callback expression.
    Index {
        /// Receiver being indexed.
        receiver: Box<CallbackExpr>,
        /// Zero-based static index.
        index: usize,
    },
    /// Static field/key access inside a callback expression.
    Field {
        /// Receiver being projected.
        receiver: Box<CallbackExpr>,
        /// Static field or string key.
        field: Symbol,
    },
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
    /// A conditional expression.
    Conditional {
        /// Boolean condition.
        cond: Box<CallbackExpr>,
        /// Expression evaluated when the condition is true.
        then_expr: Box<CallbackExpr>,
        /// Expression evaluated when the condition is false.
        else_expr: Box<CallbackExpr>,
    },
}
