//! Higher intermediate representation (HIR) for the Smelt compiler.
//!
//! This crate defines the core data structures for representing Smelt programs after
//! frontend lowering. It includes types for expressions, statements, items (functions,
//! classes, interfaces), control flow, and type information.

mod body;
mod expr;
mod format;
mod ids;
mod item;
mod krate;
mod symbol;
mod ty;
mod validate;

pub use body::{
    AsyncState, AsyncStateId, AsyncStateMachine, AsyncSuspensionPoint, Block, Body, LocalDecl,
    MatchArm, Pattern, Stmt,
};
pub use expr::{AsyncOp, BinOp, Expr, ExprKind, Literal, StringCaseOp, UnaryOp, bin_op_text};
pub use format::format_compact;
pub use ids::{
    BlockId, BodyId, ExprId, FileId, ItemId, LocalId, ModuleId, PatternId, Span, StmtId, Symbol,
    TypeId,
};
pub use item::{
    Class, ClassKind, ConstItem, Field, Function, FunctionOwner, Interface, Item, MethodSig, Param,
    ParamSig, TypeAlias, Visibility,
};
pub use krate::{CONSOLE_LOG_SYMBOL, Crate, Import, Language, Module, SourceFile};
pub use symbol::{OriginalNameTable, SymbolInterner};
pub use ty::{FunctionType, Type, TypeInterner};
pub use validate::{ValidationError, validate};
