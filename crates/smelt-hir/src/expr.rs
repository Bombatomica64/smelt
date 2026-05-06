//! Expression and literal representation for HIR.

use serde::{Deserialize, Serialize};

use crate::ids::{BlockId, BodyId, ExprId, ItemId, LocalId, Span, Symbol, TypeId};

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

/// The kind of an expression.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExprKind {
    /// A literal value.
    Literal(Literal),
    /// A reference to a local variable.
    Local(LocalId),
    /// A reference to an item (function, const, etc.).
    Item(ItemId),
    /// A function call.
    Call {
        /// The function being called.
        callee: ExprId,
        /// The arguments to the call.
        args: Vec<ExprId>,
    },
    /// A method call.
    Method {
        /// The receiver object.
        receiver: ExprId,
        /// The method name.
        method: Symbol,
        /// The arguments to the method.
        args: Vec<ExprId>,
    },
    /// A field access.
    Field {
        /// The object whose field is accessed.
        receiver: ExprId,
        /// The field name.
        field: Symbol,
    },
    /// An index expression.
    Index {
        /// The object being indexed.
        receiver: ExprId,
        /// The index expression.
        index: ExprId,
    },
    /// The length of a string or collection.
    Len {
        /// The value whose length is read.
        operand: ExprId,
    },
    /// Change the case of a string value.
    StringCase {
        /// Operation to apply to the string.
        op: StringCaseOp,
        /// The string value being transformed.
        operand: ExprId,
    },
    /// Test whether one string contains another string.
    StringContains {
        /// The string value being searched.
        haystack: ExprId,
        /// The substring to search for.
        needle: ExprId,
    },
    /// A binary operation.
    BinOp {
        /// The operator.
        op: BinOp,
        /// The left-hand side.
        lhs: ExprId,
        /// The right-hand side.
        rhs: ExprId,
    },
    /// A unary operation.
    UnaryOp {
        /// The operator.
        op: UnaryOp,
        /// The operand.
        operand: ExprId,
    },
    /// A block expression.
    Block(BlockId),
    /// A lambda function.
    Lambda {
        /// The body of the lambda.
        body: BodyId,
        /// The return type.
        return_ty: TypeId,
    },
    /// A list literal.
    ListLit(Vec<ExprId>),
    /// A set literal.
    SetLit(Vec<ExprId>),
    /// A dictionary literal.
    DictLit(Vec<(ExprId, ExprId)>),
    /// A tuple literal.
    TupleLit(Vec<ExprId>),
    /// A constructor call.
    New {
        /// The class being instantiated.
        class: Symbol,
        /// The constructor arguments.
        args: Vec<ExprId>,
    },
    /// An await expression that suspends on a future.
    Await(ExprId),
    /// A runtime-backed async operation.
    AsyncOp {
        /// Operation to perform.
        op: AsyncOp,
        /// Input future/value expressions.
        args: Vec<ExprId>,
    },
}

/// A directly lowered string case conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StringCaseOp {
    /// Convert a string to lower case.
    Lower,
    /// Convert a string to upper case.
    Upper,
}

/// Runtime-backed async operation represented in HIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsyncOp {
    /// Wait for all futures and produce all outputs.
    All,
    /// Resolve when the first future completes.
    Race,
    /// Wait for all futures and keep settled outputs.
    AllSettled,
    /// Sleep for a duration in milliseconds.
    Sleep,
    /// Create a task from a future.
    CreateTask,
    /// Wait for a future with a timeout.
    WaitFor,
}

/// A literal value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Literal {
    /// A boolean literal.
    Bool(bool),
    /// An integer literal.
    Int(i64),
    /// A floating-point literal.
    Float(f64),
    /// A string literal.
    String(String),
    /// The None/null literal.
    None,
}

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
}

/// Returns the text representation of a binary operator.
#[must_use]
pub const fn bin_op_text(op: BinOp) -> &'static str {
    match op {
        BinOp::Add => "+",
        BinOp::Sub => "-",
        BinOp::Mul => "*",
        BinOp::Div => "/",
        BinOp::Eq => "==",
        BinOp::NotEq => "!=",
        BinOp::Lt => "<",
        BinOp::Lte => "<=",
        BinOp::Gt => ">",
        BinOp::Gte => ">=",
        BinOp::And => "&&",
        BinOp::Or => "||",
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
