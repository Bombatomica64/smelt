//! Core expression node wrapper.

use serde::{Deserialize, Serialize};

use crate::ids::{Span, TypeId};

use super::ExprKind;

/// An expression in the HIR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expr {
    /// The kind of expression.
    pub kind: ExprKind,
    /// The type of this expression.
    pub ty: TypeId,
    /// Source location of the expression.
    pub span: Span,
}
