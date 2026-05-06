//! Higher intermediate representation (HIR) for the Smelt compiler.
//!
//! This crate defines the core data structures for representing Smelt programs after
//! frontend lowering. It includes types for expressions, statements, items (functions,
//! classes, interfaces), control flow, and type information.

#![expect(
    clippy::exhaustive_enums,
    reason = "HIR is the internal compiler data model and downstream crates pattern-match variants directly"
)]
#![expect(
    clippy::exhaustive_structs,
    reason = "HIR structs are constructed across workspace crates during lowering and tests"
)]
#![expect(
    clippy::indexing_slicing,
    reason = "HIR arenas use compact IDs validated by construction"
)]
#![expect(
    clippy::as_conversions,
    reason = "HIR compact IDs are u32 indexes and conversion hardening is tracked separately"
)]
#![expect(
    clippy::arithmetic_side_effects,
    reason = "HIR arena index arithmetic is bounded by vector lengths in current builders"
)]
#![expect(
    clippy::shadow_reuse,
    reason = "formatter code intentionally reuses domain names while descending nested structures"
)]
#![expect(
    clippy::shadow_unrelated,
    reason = "formatter code intentionally reuses short domain names across independent match arms"
)]
#![expect(
    clippy::too_many_lines,
    reason = "HIR compact formatter sections are kept together for readable output structure"
)]

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
