use serde::{Deserialize, Serialize};

use crate::ids::{BlockId, BodyId, ExprId, ItemId, LocalId, Span, Symbol, TypeId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Expr {
    pub kind: ExprKind,
    pub ty: TypeId,
    pub span: Span,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExprKind {
    Literal(Literal),
    Local(LocalId),
    Item(ItemId),
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
    Method {
        receiver: ExprId,
        method: Symbol,
        args: Vec<ExprId>,
    },
    Field {
        receiver: ExprId,
        field: Symbol,
    },
    Index {
        receiver: ExprId,
        index: ExprId,
    },
    BinOp {
        op: BinOp,
        lhs: ExprId,
        rhs: ExprId,
    },
    UnaryOp {
        op: UnaryOp,
        operand: ExprId,
    },
    Block(BlockId),
    Lambda {
        body: BodyId,
        return_ty: TypeId,
    },
    ListLit(Vec<ExprId>),
    SetLit(Vec<ExprId>),
    DictLit(Vec<(ExprId, ExprId)>),
    TupleLit(Vec<ExprId>),
    New {
        class: Symbol,
        args: Vec<ExprId>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Literal {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Lt,
    Lte,
    Gt,
    Gte,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Not,
    Neg,
}
