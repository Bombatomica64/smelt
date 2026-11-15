//! Closure expressions and their captures.

use serde::{Deserialize, Serialize};

use crate::ids::{BodyId, ItemId, LocalId, Span, Symbol, TypeId};
use crate::item::Param;

/// A first-class closure expression with explicit captures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClosureExpr {
    /// Closure parameters in source order.
    pub params: Vec<Param>,
    /// Index of the rest parameter, if this closure declares one.
    pub rest: Option<usize>,
    /// Number of leading parameters counted by JavaScript `Function.length`.
    pub required_params: Option<usize>,
    /// Return type produced by the closure body.
    pub return_ty: TypeId,
    /// Captured locals made visible to the closure body.
    pub captures: Vec<ClosureCapture>,
    /// HIR body containing the closure implementation.
    pub body: BodyId,
    /// Source function item when this closure is a bare function-item-as-value
    /// wrapper (e.g. passing `func1` directly as an argument), else `None`.
    ///
    /// JavaScript reference identity requires every reference to the same named
    /// function value to be the SAME runtime value. The frontend wraps a
    /// function item used as a value in a fresh forwarding closure per
    /// reference, so two references to `func1` would otherwise become two
    /// distinct closure values. Tagging the wrapper with its source item lets
    /// codegen cache one runtime wrapper per item so all references share it.
    /// User-written arrows like `(x) => func1(x)` keep `None` and retain fresh
    /// identity, matching JavaScript semantics.
    pub function_item: Option<ItemId>,
    /// Where this closure's `this` comes from (see [`ClosureThis`]).
    pub this_binding: ClosureThis,
    /// Source span covering the closure expression.
    pub span: Span,
}

/// One local captured by a closure expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClosureCapture {
    /// Source local in the enclosing body.
    pub source_local: LocalId,
    /// Local inside the closure body that receives this captured value.
    pub body_local: Option<LocalId>,
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

/// Where a closure's JavaScript `this` comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClosureThis {
    /// A `function`: the receiver its own call supplies (a plain call's is
    /// `undefined`).
    Own,
    /// An ARROW: none of its own; `ExprKind::ThisRead` in its body reads the
    /// receiver its enclosing function was called with.
    Lexical,
    /// A wrapper the compiler synthesizes for a function item used as a value:
    /// not a JavaScript function of its own -- the item is the function -- so
    /// the receiver its call supplies reaches the item untouched.
    Forwarded,
}
