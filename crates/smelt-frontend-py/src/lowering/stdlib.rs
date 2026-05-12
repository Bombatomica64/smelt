//! Focused Python standard-library and built-in operation lowering helpers.

use ruff_python_ast::{Expr, ExprSubscript};
use ruff_text_size::Ranged;
use smelt_hir::{
    Body, BoolFoldOp, Expr as HirExpr, ExprKind, FileId, PrimitiveCastOp, RegexMatchOp,
    SetBinaryOp, SetRelationOp, SetRemoveOp, Span, StringReplaceOp, Type, UrlField,
};
use smelt_stdlib::RuleId;

use super::{ModuleBuilder, SmeltError, stdlib_dispatch};

// Standard-library call lowering helpers.
include!("stdlib_call.rs");
// Type and conversion helpers used by stdlib lowering.
include!("stdlib_types.rs");
// Collection-related stdlib lowering helpers.
include!("stdlib_collections.rs");
// Slice lowering helpers used across collection methods.
include!("stdlib_slice.rs");
