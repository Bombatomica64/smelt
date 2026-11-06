//! Lowering of the op table's transcribed group.
//!
//! A transcribed operation lowers each child in field order into the one rvalue
//! of the same name, assigned to a temporary or, when the op can throw, carried
//! by a call terminator; this module is that rule, once, for every entry of
//! `smelt_hir::for_each_transcribed_op!`.

use smelt_hir::{Expr, ExprKind};

use crate::{Callee, MirListSpliceItem, Operand, Place, Rvalue, Terminator};

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
        $($fact:ident $fact_value:tt)*
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
        $($fact:ident $fact_value:tt)*
        => $display:tt;
    )*) => {
        impl LoweringCtx<'_> {
            /// Lowers a transcribed operation: each child in field order, then
            /// one rvalue placed by [`LoweringCtx::finish_op`].
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
                self.finish_op(expr, value)
            }
        }
    };
}

smelt_hir::for_each_transcribed_op!(define_lower_transcribed_op);

impl LoweringCtx<'_> {
    /// Places a lowered operation: an assignment, or for an op that can throw a
    /// call terminator.
    ///
    /// A `Statement::Assign` has no unwind edge, so a throwing op there could
    /// never reach an enclosing `try`: the catch block would have no predecessor
    /// and MIR would drop the handler the source wrote. With no handler active,
    /// the throwing pass sees the same terminator and marks the function.
    fn finish_op(&mut self, expr: &Expr, value: Rvalue) -> Result<Operand, LowerError> {
        if !value.is_fallible() {
            return self.assign_temp(expr.ty, expr.span, value);
        }
        let dest = self.push_temp(expr.ty, expr.span);
        let target = self.function.push_block(expr.span);
        self.terminate(Terminator::Call {
            callee: Callee::Op(Box::new(value)),
            args: Vec::new(),
            dest,
            target,
            unwind: self.current_exception_handler(),
        })?;
        self.switch_to(target);
        Ok(Operand::Copy(Place::Local(dest)))
    }
}
