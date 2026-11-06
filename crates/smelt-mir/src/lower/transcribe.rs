//! Lowering of the op table's transcribed group.
//!
//! A transcribed operation lowers each child in field order and assigns the one
//! rvalue of the same name to a temporary; this module is that rule, once, for
//! every entry of `smelt_hir::for_each_transcribed_op!`.

use smelt_hir::{Expr, ExprKind};

use crate::{MirListSpliceItem, Operand, Rvalue};

use super::LowerError;
use super::context::LoweringCtx;

/// Lowers one op field from its HIR form to its MIR form.
macro_rules! lower_op_field {
    ($ctx:ident, one, $field:ident) => { $ctx.lower_expr(*$field)? };
    ($ctx:ident, opt, $field:ident) => {
        $field.as_ref().map(|child| $ctx.lower_expr(*child)).transpose()?
    };
    ($ctx:ident, many, $field:ident) => {
        $field
            .iter()
            .map(|child| $ctx.lower_expr(*child))
            .collect::<Result<Vec<_>, _>>()?
    };
    ($ctx:ident, splice, $field:ident) => {
        $field
            .iter()
            .map(|item| {
                Ok(MirListSpliceItem {
                    value: $ctx.lower_expr(item.value)?,
                    spread: item.spread,
                })
            })
            .collect::<Result<Vec<_>, LowerError>>()?
    };
    ($ctx:ident, symbol_pairs, $field:ident) => { lower_op_field!($ctx, { _ }, $field) };
    ($ctx:ident, { $key:ty }, $field:ident) => {
        $field
            .iter()
            .map(|(key, value)| Ok((key.clone(), $ctx.lower_expr(*value)?)))
            .collect::<Result<Vec<_>, LowerError>>()?
    };
    ($ctx:ident, [$data:ty], $field:ident) => { $field.clone() };
    ($ctx:ident, (sym $label:literal), $field:ident) => { *$field };
    ($ctx:ident, (ty), $field:ident) => { *$field };
}

/// Expands, in pattern position, to every transcribed [`ExprKind`] variant.
macro_rules! transcribed_op_kinds {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt),* $(,)? })?
        => $display:tt;
    )*) => {
        $(ExprKind::$name $({ $($field: _),* })?)|*
    };
}

/// [`transcribed_op_kinds`] as a parameterless pattern for `lower_expr`'s match.
macro_rules! transcribed_op_pattern {
    () => {
        smelt_hir::for_each_transcribed_op!($crate::lower::transcribe::transcribed_op_kinds)
    };
}

pub(super) use {transcribed_op_kinds, transcribed_op_pattern};

/// Generates [`LoweringCtx::lower_transcribed_op`] from the transcribed group.
macro_rules! define_lower_transcribed_op {
    ($(
        $(#[$meta:meta])*
        $name:ident $({ $($(#[$field_meta:meta])* $field:ident : $kind:tt),* $(,)? })?
        => $display:tt;
    )*) => {
        impl LoweringCtx<'_> {
            /// Lowers a transcribed operation: each child in field order, then
            /// one rvalue assigned to a temporary of the expression's type.
            ///
            /// `lower_expr` admits only [`transcribed_op_pattern`] here, so the
            /// last arm is unreachable from source.
            pub(super) fn lower_transcribed_op(
                &mut self,
                expr: &Expr,
            ) -> Result<Operand, LowerError> {
                let value = match &expr.kind {
                    $(
                        ExprKind::$name $({ $($field),* })? => {
                            $($(let $field = lower_op_field!(self, $kind, $field);)*)?
                            Rvalue::$name $({ $($field),* })?
                        }
                    )*
                    _ => {
                        return Err(self.error(
                            "expression is not a transcribed operation",
                            Some(expr.span),
                        ));
                    }
                };
                self.assign_temp(expr.ty, expr.span, value)
            }
        }
    };
}

smelt_hir::for_each_transcribed_op!(define_lower_transcribed_op);
