//! HIR expression kinds: the `ExprKind` enum, one variant per lowered
//! expression form, with the per-subsystem operation enums it carries.

use super::{BinOp, ClosureExpr, Literal, UnaryOp};
use crate::ids::{BlockId, BodyId, ExprId, ItemId, LocalId, Symbol, TypeId};
use serde::{Deserialize, Serialize};

/// Protocol operation used to resume or abruptly complete a generator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GeneratorResumeKind {
    /// Continue execution and make the supplied value the result of `yield`.
    Next,
    /// Complete the generator with the supplied return value.
    Return,
    /// Inject an exception at the suspended `yield` expression.
    Throw,
}

/// How far a property-presence test is allowed to look for the key.
///
/// JavaScript has two presence tests over one property name, and they answer
/// differently for anything a value inherits: `key in value` walks the
/// prototype chain, so `'toString' in {}` is `true`, while
/// `Object.hasOwn(value, key)` (and `hasOwnProperty` / `propertyIsEnumerable`)
/// stop at the value's own properties, so `Object.hasOwn({}, 'toString')` is
/// `false`. Both lower to `DictContainsKey`, so the containment expression has
/// to carry which of the two it is; without it the emitter had to pick one
/// reach for both spellings and was wrong about the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyLookup {
    /// Own properties only — `Object.hasOwn`, `hasOwnProperty`, and every
    /// typed-collection membership test (`Map.has`, a `Dict` key probe), whose
    /// keys are all own by construction.
    Own,
    /// Own properties and everything inherited — the `in` operator.
    PrototypeChain,
}

/// A replacement argument passed to array splice-style operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListSpliceItem {
    /// Replacement value or array value when `spread` is true.
    pub value: ExprId,
    /// Whether the source argument used JavaScript spread syntax.
    pub spread: bool,
}

/// Generates [`ExprKind`]: the forms only HIR has, then every operation in the
/// op table.
macro_rules! define_expr_kind {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt $(@ $role:tt)?),* $(,)? })?
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
        #[derive(Debug, Clone, Serialize, Deserialize)]
        #[expect(
            missing_docs,
            reason = "Variant docs remain in formatter and lowering code"
        )]
        pub enum ExprKind {
            Literal(Literal),
            Local(LocalId),
            Item(ItemId),
            /// Read of a module-level mutable global binding.
            ///
            /// `item` references an [`crate::Item::MutableGlobal`]; the expression is
            /// typed as that binding's declared type. Reads of a lifted module `let`
            /// lower to this instead of being const-inlined.
            GlobalGet {
                /// The mutable-global item being read.
                item: ItemId,
            },
            /// Store to a module-level mutable global binding.
            ///
            /// Evaluates `value`, stores it into the global referenced by `item`, and
            /// evaluates to the stored value so that `++`/`--` and compound assignments
            /// compose as expressions. `item` references an
            /// [`crate::Item::MutableGlobal`].
            GlobalSet {
                /// The mutable-global item being written.
                item: ItemId,
                /// The value expression to store.
                value: ExprId,
            },
            Call {
                callee: ExprId,
                args: Vec<ExprId>,
            },
            /// Suspend the current generator and expose `value` to its caller.
            GeneratorYield {
                /// Value exposed by the suspension point.
                value: ExprId,
            },
            Closure(ClosureExpr),
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
            TypeAssert {
                value: ExprId,
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
            Conditional {
                cond: ExprId,
                then_expr: ExprId,
                else_expr: ExprId,
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
            /// Run a source-declared base class's constructor as an INITIALIZER over an
            /// existing derived instance, evaluating to that (now initialised) instance.
            ///
            /// This is a derived constructor's `super(args)`. JavaScript has one object:
            /// the base constructor initialises the instance the derived constructor is
            /// building, so a closure it stores captures the derived instance as `this`
            /// and a later write by the derived constructor is visible through it.
            /// Constructing a separate base value and copying its fields (`New` + field
            /// moves) cannot reproduce that, and cannot run a generic base whose
            /// constructed value does not fit the derived class's flattened layout.
            ///
            /// Codegen re-emits the base constructor body over the derived `Self`, the
            /// way inherited methods are re-emitted (see `effective_class_methods`).
            BaseConstructorInit {
                /// The base class whose constructor runs.
                base: Symbol,
                /// The derived instance under construction (the constructor's `this`).
                receiver: ExprId,
                /// The `super(..)` arguments, as written.
                args: Vec<ExprId>,
            },
            Await(ExprId),
            $(
                $(#[$meta])*
                $name $({
                    $(
                        $(#[$field_meta])*
                        $field: $crate::op_field_ty!(ExprId, ListSpliceItem, $kind),
                    )*
                })?,
            )*
        }
    };
}

crate::for_each_op!(define_expr_kind);
