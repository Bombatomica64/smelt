//! Callback expression nodes for list-style operations.

use serde::{Deserialize, Serialize};

use crate::ids::{BodyId, LocalId, Span, Symbol, TypeId};
use crate::item::Param;

use super::{BinOp, Literal, UnaryOp};

/// A callback expression tree embedded in a list-style operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallbackExpr {
    /// Callback expression kind.
    pub kind: CallbackExprKind,
    /// Callback expression type.
    pub ty: TypeId,
}

/// The kind of a callback expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CallbackExprKind {
    /// A callback parameter reference by zero-based position.
    Param(usize),
    /// A local captured from the enclosing HIR body.
    Capture(LocalId),
    /// Assignment to a mutable local captured from the enclosing HIR body.
    AssignCapture {
        /// Captured local assigned by the callback.
        target: LocalId,
        /// Value assigned into the captured local.
        value: Box<CallbackExpr>,
    },
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
}

/// A first-class closure expression with explicit captures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosureExpr {
    /// Closure parameters in source order.
    pub params: Vec<Param>,
    /// Return type produced by the closure body.
    pub return_ty: TypeId,
    /// Captured locals made visible to the closure body.
    pub captures: Vec<ClosureCapture>,
    /// HIR body containing the closure implementation.
    pub body: BodyId,
    /// Callback-expression bridge used while callback APIs migrate to closure bodies.
    pub callback_body: Option<CallbackExpr>,
    /// Source span covering the closure expression.
    pub span: Span,
}

/// One local captured by a closure expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosureCapture {
    /// Source local in the enclosing body.
    pub source_local: LocalId,
    /// Source symbol for diagnostics and codegen naming.
    pub symbol: Symbol,
    /// Captured value type.
    pub ty: TypeId,
    /// Capture ownership/mutability mode.
    pub mode: CaptureMode,
}

/// How a closure captures one local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureMode {
    /// Shared immutable capture.
    ByRef,
    /// Mutable capture.
    ByMut,
    /// Moved value capture.
    ByValue,
}
