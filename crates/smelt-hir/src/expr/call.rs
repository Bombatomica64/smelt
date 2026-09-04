//! Legacy callback-expression nodes for narrow frontend and sort paths.
//!
//! Normal callback bodies lower into ordinary closure CFG bodies. `CallbackExpr`
//! remains as a transient frontend expression tree while those bodies are built
//! and as the persisted representation for JavaScript sort comparators.

use serde::{Deserialize, Serialize};

use crate::expr::UnknownKind;
use crate::ids::{BodyId, ItemId, LocalId, Span, Symbol, TypeId};
use crate::item::Param;

use super::{BinOp, Literal, UnaryOp};

/// A legacy callback expression tree.
///
/// Do not use this as callback-body storage for new HIR features. New callback
/// bodies should lower into `ClosureExpr` CFG bodies; persisted `CallbackExpr`
/// values are reserved for sort comparators until that path is migrated too.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallbackExpr {
    /// Callback expression kind.
    pub kind: CallbackExprKind,
    /// Callback expression type.
    pub ty: TypeId,
}

/// One argument passed by a callback-local call expression.
///
/// This is part of the legacy expression-tree bridge and should not be used by
/// normal closure CFG bodies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallbackCallArg {
    /// Argument expression.
    pub expr: CallbackExpr,
    /// Whether the source argument used JavaScript spread syntax.
    pub spread: bool,
}

/// The kind of a legacy callback expression.
///
/// These nodes model enough expression structure for frontend callback-to-CFG
/// lowering and the remaining direct sort-comparator renderer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CallbackExprKind {
    /// A callback parameter reference by zero-based position.
    Param(usize),
    /// A local captured from the enclosing HIR body.
    Capture(LocalId),
    /// A free function or callable item referenced from inside a callback.
    Function(Symbol),
    /// A dynamic lookup into a static object table whose values are functions.
    FunctionTableLookup {
        /// Runtime key used to select a function from the table.
        key: Box<CallbackExpr>,
        /// Statically known table entries, preserving source keys.
        cases: Vec<(String, Symbol)>,
    },
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
    /// Side-effect callback expressions followed by a final value expression.
    Sequence {
        /// Expressions evaluated for side effects.
        effects: Vec<CallbackExpr>,
        /// Final value produced by the callback expression.
        result: Box<CallbackExpr>,
    },
    /// A string-keyed object literal built inside a callback.
    DictLit(Vec<(Symbol, CallbackExpr)>),
    /// A thrown/panicking callback branch.
    Throw {
        /// Optional panic message.
        message: Option<Box<CallbackExpr>>,
    },
    /// Indexed access inside a callback expression.
    Index {
        /// Receiver being indexed.
        receiver: Box<CallbackExpr>,
        /// Zero-based static index.
        index: usize,
    },
    /// Runtime indexed access inside a callback expression.
    DynamicIndex {
        /// Receiver being indexed.
        receiver: Box<CallbackExpr>,
        /// Runtime index expression.
        index: Box<CallbackExpr>,
    },
    /// Static field/key access inside a callback expression.
    Field {
        /// Receiver being projected.
        receiver: Box<CallbackExpr>,
        /// Static field or string key.
        field: Symbol,
    },
    /// Static field/key presence check inside a callback expression.
    HasField {
        /// Receiver being checked.
        receiver: Box<CallbackExpr>,
        /// Static field or string key.
        field: Symbol,
    },
    /// Dynamic field/key presence check inside a callback expression.
    HasDynamicField {
        /// Receiver being checked.
        receiver: Box<CallbackExpr>,
        /// Runtime field/key expression.
        field: Box<CallbackExpr>,
    },
    /// JavaScript truthiness of an arbitrary callback value.
    ///
    /// This is the callback-tree spelling of `ExprKind::PrimitiveCast` with
    /// `PrimitiveCastOp::ToBool`: a full JS truthiness test, so an erased or
    /// type-parameter operand answers `false` for `0`, `NaN`, `""`, `false` and
    /// nullish. It exists because a boolean-position operand inside a callback
    /// body has no other way to spell truthiness, and a tag check
    /// (`UnknownIs { kind: Bool }`) is close to the inverse of it.
    ValueTruthy {
        /// Value whose truthiness is tested.
        value: Box<CallbackExpr>,
    },
    /// JavaScript truthiness of a possibly optional field access.
    FieldTruthy {
        /// Receiver being checked.
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
    /// Runtime `typeof`-style check against an unknown callback value.
    UnknownIs {
        /// Value being checked.
        value: Box<CallbackExpr>,
        /// Unknown tag to test.
        kind: UnknownKind,
    },
    /// Runtime JavaScript `typeof` string for an erased callback value.
    TypeofValue {
        /// Value whose JavaScript type name is being computed.
        value: Box<CallbackExpr>,
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
    /// A call performed inside a callback expression.
    Call {
        /// Callable expression.
        callee: Box<CallbackExpr>,
        /// Arguments passed to the callable.
        args: Vec<CallbackCallArg>,
    },
    /// A method call performed on a callback expression receiver.
    MethodCall {
        /// Receiver expression.
        receiver: Box<CallbackExpr>,
        /// Source method name.
        method: Symbol,
        /// Arguments passed to the method.
        args: Vec<CallbackCallArg>,
    },
}

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
