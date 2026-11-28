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

/// The static member a [`ExprKind::StaticDispatch`] reaches on its receiver
/// class.
///
/// A static method's `this` is the class it was CALLED on, not the class that
/// declares it: `Sub.bump()` running `Base`'s `static bump() { this.count++ }`
/// writes `Sub.count`. Each way a static method uses `this` is one member of
/// the receiver class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StaticMember {
    /// `this.field`: the receiver's static field (own or inherited).
    Get(Symbol),
    /// `this.field = value`: a write that makes the field the receiver's own.
    Set(Symbol),
    /// `new this(..)`: an instance of the receiver class, at the declaring
    /// class's instance type.
    Construct,
    /// `this.method(..)` or `Class.method(..)`: the receiver's static method.
    Call(Symbol),
    /// `this` as a value: the receiver's class value.
    ClassValue,
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
            /// One step of `yield* delegate`: suspend exposing `value` (the
            /// delegate's last yield), then forward the command the consumer
            /// resumes with to `delegate` (`next(v)`, `throw(e)`,
            /// `return(v)`) and evaluate to the delegate's next step
            /// (`IteratorResult<T, R>`). A delegate that completes a forwarded
            /// `return(v)` ends this generator with its value, through the
            /// `finally` blocks around the suspension, as a `return` would.
            GeneratorYieldDelegated {
                /// The delegate generator the resume command goes to.
                delegate: ExprId,
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

/// Generates [`ExprKind::state_updated_receiver`] from the op table's
/// `updates_state` facts.
macro_rules! define_expr_state_update {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt $(@ $role:tt)?),* $(,)? })?
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
        impl ExprKind {
            /// The operand whose runtime-modelled state this op changes behind a
            /// shared handle (`RegExp.prototype.test` moving `lastIndex`), if any.
            ///
            /// A field read of a value of that operand's type observes the change,
            /// so an earlier sibling read is taken where it is evaluated.
            #[must_use]
            #[expect(
                unused_variables,
                reason = "each arm binds every field; a fact reads the ones it needs"
            )]
            pub const fn state_updated_receiver(&self) -> Option<ExprId> {
                match self {
                    $(
                        Self::$name $({ $($field,)* })? => {
                            $crate::op_updates_state!($($fact $fact_value)*)
                        }
                    )*
                    _ => None,
                }
            }
        }
    };
}

crate::for_each_op!(define_expr_state_update);

/// Generates [`ExprKind::runs_code_or_stores`] from the op table's `mutates`
/// and `side_effect` facts.
macro_rules! define_expr_runs_code_or_stores {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt $(@ $role:tt)?),* $(,)? })?
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
        impl ExprKind {
            /// Whether evaluating this expression itself (not its children) can
            /// run source code, suspend, or store in place: a read of a field or
            /// element before it can see a different value than before.
            ///
            /// An op answers from its `mutates` and `side_effect` facts, so a new
            /// op states it in the table. A function-typed operand the op is
            /// handed (an array callback) is the caller's to check.
            #[must_use]
            #[expect(
                unused_variables,
                reason = "each arm binds every field; a fact reads the ones it needs"
            )]
            pub const fn runs_code_or_stores(&self) -> bool {
                match self {
                    $(
                        Self::$name $({ $($field,)* })? => {
                            let mutated: Option<&ExprId> = $crate::op_mutates!($($fact $fact_value)*);
                            mutated.is_some() || $crate::op_side_effect!($($fact $fact_value)*)
                        }
                    )*
                    // The hand-written variants outside the op table. Listed,
                    // not `_`, so a new variant must state its effect here.
                    Self::GlobalSet { .. }
                    | Self::Call { .. }
                    | Self::GeneratorYield { .. }
                    | Self::GeneratorYieldDelegated { .. }
                    | Self::Method { .. }
                    | Self::Block(_)
                    | Self::New { .. }
                    | Self::BaseConstructorInit { .. }
                    | Self::Await(_) => true,
                    Self::Literal(_)
                    | Self::Local(_)
                    | Self::Item(_)
                    | Self::GlobalGet { .. }
                    | Self::Closure(_)
                    | Self::Field { .. }
                    | Self::Index { .. }
                    | Self::TypeAssert { .. }
                    | Self::BinOp { .. }
                    | Self::UnaryOp { .. }
                    | Self::Conditional { .. }
                    | Self::Lambda { .. }
                    | Self::ListLit(_)
                    | Self::SetLit(_)
                    | Self::DictLit(_)
                    | Self::TupleLit(_) => false,
                }
            }
        }
    };
}

crate::for_each_op!(define_expr_runs_code_or_stores);

#[cfg(test)]
mod tests {
    use super::*;

    /// `runs_code_or_stores` reads the op table: every in-place mutator
    /// (`mutates`) and every op that calls code (`side_effect`) answers true,
    /// a pure read false. The hand-kept list it replaced missed the `Set` and
    /// most `Dict` mutators, so a field read before `s.add(x)` was not ordered.
    #[test]
    fn runs_code_or_stores_follows_the_op_table_facts() {
        let id = ExprId(0);
        for effectful in [
            ExprKind::SetAdd { set: id, item: id },
            ExprKind::DictClear { dict: id },
            ExprKind::DictPopItem { dict: id },
            ExprKind::ClosureCall {
                callee: id,
                args: Vec::new(),
            },
            ExprKind::ListSplice {
                list: id,
                start: id,
                delete_count: None,
                items: Vec::new(),
                mutate: true,
            },
            ExprKind::Block(BlockId(0)),
        ] {
            assert!(effectful.runs_code_or_stores(), "{effectful:?}");
        }
        for pure in [
            ExprKind::ListContains { list: id, item: id },
            ExprKind::ListSplice {
                list: id,
                start: id,
                delete_count: None,
                items: Vec::new(),
                mutate: false,
            },
            ExprKind::Local(LocalId(0)),
        ] {
            assert!(!pure.runs_code_or_stores(), "{pure:?}");
        }
    }
}
