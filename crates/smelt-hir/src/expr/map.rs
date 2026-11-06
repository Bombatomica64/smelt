//! Structural child-expression remapping for HIR expression kinds.
//!
//! [`ExprKind::try_map_child_exprs`] rebuilds an expression kind with every
//! direct child [`ExprId`] passed through a caller-supplied mapping function.
//! It exists so passes that copy expressions between bodies (for example
//! const-item inlining in the frontends) do not need to enumerate every
//! expression variant themselves: the match below is exhaustive, so adding a
//! new [`ExprKind`] variant forces this mapper to be updated alongside it.
//!
//! The mapper is intentionally shallow. It only rewrites the [`ExprId`]s
//! stored directly inside one `ExprKind` value; callers drive recursion by
//! re-invoking the mapper from inside their mapping function. Identifiers
//! that point into crate-global arenas ([`crate::ids::BodyId`],
//! [`crate::ids::ItemId`], [`crate::ids::TypeId`], [`crate::ids::Symbol`])
//! are preserved untouched, as are body-local references
//! ([`crate::ids::LocalId`] in [`ExprKind::Local`] and
//! [`crate::ids::BlockId`] in [`ExprKind::Block`]) — callers that move
//! expressions across bodies must decide separately whether those are valid
//! in the destination body.

use super::kinds::{ExprKind, ListSpliceItem};
use crate::ids::ExprId;

/// Map every element of a child-expression list through `f`.
fn map_vec<E>(
    items: Vec<ExprId>,
    f: &mut impl FnMut(ExprId) -> Result<ExprId, E>,
) -> Result<Vec<ExprId>, E> {
    items.into_iter().map(f).collect()
}

/// Map an optional child expression through `f`, preserving `None`.
fn map_opt<E>(
    item: Option<ExprId>,
    f: &mut impl FnMut(ExprId) -> Result<ExprId, E>,
) -> Result<Option<ExprId>, E> {
    item.map(f).transpose()
}

/// Key-value pairs whose values are expressions.
type ExprPairs<K> = Vec<(K, ExprId)>;

/// Map the value of every key-value pair through `f`, preserving the keys.
fn map_pairs<K, E>(
    items: ExprPairs<K>,
    f: &mut impl FnMut(ExprId) -> Result<ExprId, E>,
) -> Result<ExprPairs<K>, E> {
    items
        .into_iter()
        .map(|(key, value)| Ok((key, f(value)?)))
        .collect()
}

/// Expands to one op field with each child [`ExprId`] passed through `$f`.
macro_rules! map_op_field {
    ($f:ident, one, $field:ident) => { $f($field)? };
    ($f:ident, opt, $field:ident) => { map_opt($field, $f)? };
    ($f:ident, many, $field:ident) => { map_vec($field, $f)? };
    ($f:ident, splice, $field:ident) => {
        $field
            .into_iter()
            .map(|item| {
                Ok(ListSpliceItem {
                    value: $f(item.value)?,
                    spread: item.spread,
                })
            })
            .collect::<Result<Vec<_>, _>>()?
    };
    ($f:ident, symbol_pairs, $field:ident) => { map_pairs($field, $f)? };
    ($f:ident, { $key:ty }, $field:ident) => { map_pairs($field, $f)? };
    ($f:ident, [$data:ty], $field:ident) => { $field };
    ($f:ident, (sym $label:literal), $field:ident) => { $field };
    ($f:ident, (ty), $field:ident) => { $field };
}

/// Generates [`ExprKind::try_map_child_exprs`]: the arms of the forms only HIR
/// has, then one generated arm per op-table entry.
macro_rules! define_try_map_child_exprs {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt $(@ $role:tt)?),* $(,)? })?
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
        impl ExprKind {
            /// Rebuild this expression kind with each direct child [`ExprId`] mapped
            /// through `f`, leaving all other payload (operators, symbols, types,
            /// bodies, blocks, locals) unchanged.
            ///
            /// # Errors
            ///
            /// Returns the first error produced by `f`; no partial mapping is
            /// observable because the kind is rebuilt by value.
            #[expect(
                clippy::too_many_lines,
                reason = "exhaustive match over every ExprKind variant is intentionally one function"
            )]
            pub fn try_map_child_exprs<E>(
                self,
                f: &mut impl FnMut(ExprId) -> Result<ExprId, E>,
            ) -> Result<Self, E> {
                Ok(match self {
                    // Leaf kinds without direct child expression IDs. `Closure` and
                    // `Lambda` reference crate-global bodies; `Local` and `Block`
                    // reference body-local arenas that this shallow mapper preserves.
                    kind @ (Self::Literal(_)
                    | Self::Local(_)
                    | Self::Item(_)
                    | Self::GlobalGet { .. }
                    | Self::Closure(_)
                    | Self::Block(_)
                    | Self::Lambda { .. }) => kind,
                    Self::Call { callee, args } => Self::Call {
                        callee: f(callee)?,
                        args: map_vec(args, f)?,
                    },
                    Self::GeneratorYield { value } => Self::GeneratorYield { value: f(value)? },
                    Self::Method {
                        receiver,
                        method,
                        args,
                    } => Self::Method {
                        receiver: f(receiver)?,
                        method,
                        args: map_vec(args, f)?,
                    },
                    Self::Field { receiver, field } => Self::Field {
                        receiver: f(receiver)?,
                        field,
                    },
                    Self::Index { receiver, index } => Self::Index {
                        receiver: f(receiver)?,
                        index: f(index)?,
                    },
                    Self::GlobalSet { item, value } => Self::GlobalSet {
                        item,
                        value: f(value)?,
                    },
                    Self::TypeAssert { value } => Self::TypeAssert { value: f(value)? },
                    Self::BinOp { op, lhs, rhs } => Self::BinOp {
                        op,
                        lhs: f(lhs)?,
                        rhs: f(rhs)?,
                    },
                    Self::UnaryOp { op, operand } => Self::UnaryOp {
                        op,
                        operand: f(operand)?,
                    },
                    Self::Conditional {
                        cond,
                        then_expr,
                        else_expr,
                    } => Self::Conditional {
                        cond: f(cond)?,
                        then_expr: f(then_expr)?,
                        else_expr: f(else_expr)?,
                    },
                    Self::ListLit(items) => Self::ListLit(map_vec(items, f)?),
                    Self::SetLit(items) => Self::SetLit(map_vec(items, f)?),
                    Self::DictLit(entries) => Self::DictLit(
                        entries
                            .into_iter()
                            .map(|(key, value)| Ok((f(key)?, f(value)?)))
                            .collect::<Result<Vec<_>, E>>()?,
                    ),
                    Self::TupleLit(items) => Self::TupleLit(map_vec(items, f)?),
                    Self::New { class, args } => Self::New {
                        class,
                        args: map_vec(args, f)?,
                    },
                    Self::BaseConstructorInit {
                        base,
                        receiver,
                        args,
                    } => Self::BaseConstructorInit {
                        base,
                        receiver: f(receiver)?,
                        args: map_vec(args, f)?,
                    },
                    Self::Await(value) => Self::Await(f(value)?),
                    $(
                        Self::$name $({ $($field),* })? => Self::$name $({
                            $($field: map_op_field!(f, $kind, $field)),*
                        })?,
                    )*
                })
            }
        }
    };
}

crate::for_each_op!(define_try_map_child_exprs);
