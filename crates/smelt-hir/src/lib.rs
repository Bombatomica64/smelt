//! Higher intermediate representation (HIR) for the Smelt compiler.
//!
//! This crate defines the core data structures for representing Smelt programs after
//! frontend lowering. It includes types for expressions, statements, items (functions,
//! classes, interfaces), control flow, and type information.

/// HIR body structures and control-flow nodes.
mod body;
/// HIR expression nodes and operators.
mod expr;
/// Compact HIR formatting utilities.
mod format;
/// Typed ID newtypes and source span primitives.
pub mod ids;
/// HIR top-level items (functions, classes, interfaces).
mod item;
/// Crate/module metadata and imports.
mod krate;
/// Symbol interning and original-name tracking.
mod symbol;
/// HIR type system and type interning.
mod ty;
/// HIR validation passes.
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
