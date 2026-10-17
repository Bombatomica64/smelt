//! Emission of proven fixed-width integers ([`Type::FixedInt`]).
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631): a JavaScript `number`
//! that the compiler proved integral is kept "in a register as an integer". MIR
//! integer range analysis (`smelt_mir::opt::IntegerRange`) retypes such locals
//! and confines them to a few integer-closed forms, so this module only has to
//! render those forms:
//!
//! * `a + b`, `a - b` and the comparisons between `FixedInt` operands and
//!   integer constants, as plain Rust integer operators (the analysis proved
//!   the result stays in range, so no wrapping or checking is needed);
//! * the exact `ToFloat` conversion the analysis inserts wherever the value
//!   meets a `number` context (`i as f64`);
//! * a `FixedInt` list index, which needs no `f64` round trip.

use super::*;

impl FunctionEmitter<'_> {
    /// Whether `ty` is a proven fixed-width integer.
    pub(super) fn is_fixed_int_type(&self, ty: TypeId) -> bool {
        matches!(self.mir.types.get(ty), Some(Type::FixedInt(_)))
    }

    /// Renders an integer-closed binary operation over `FixedInt` operands.
    ///
    /// Returns `None` unless at least one operand is a `FixedInt`; the other is
    /// then a `FixedInt` of the same width or an integer constant (rendered
    /// unsuffixed, so Rust infers the width). Only `+`, `-` and comparisons are
    /// produced by the range analysis; any other operator is left to the
    /// general emitter, which sees the operands' declared types.
    pub(super) fn fixed_int_binary_text(
        &self,
        op: smelt_hir::BinOp,
        lhs: &Operand,
        rhs: &Operand,
    ) -> Result<Option<String>, EmitError> {
        let lhs_fixed = self.is_fixed_int_type(self.operand_ty(lhs)?);
        let rhs_fixed = self.is_fixed_int_type(self.operand_ty(rhs)?);
        if !lhs_fixed && !rhs_fixed {
            return Ok(None);
        }
        let integer_side = |operand: &Operand, fixed: bool| {
            fixed || matches!(operand, Operand::Const(Constant::Int(_)))
        };
        if !integer_side(lhs, lhs_fixed) || !integer_side(rhs, rhs_fixed) {
            return Ok(None);
        }
        if !matches!(
            op,
            smelt_hir::BinOp::Add
                | smelt_hir::BinOp::Sub
                | smelt_hir::BinOp::Lt
                | smelt_hir::BinOp::Lte
                | smelt_hir::BinOp::Gt
                | smelt_hir::BinOp::Gte
                | smelt_hir::BinOp::Eq
                | smelt_hir::BinOp::NotEq
                | smelt_hir::BinOp::StrictEq
                | smelt_hir::BinOp::StrictNotEq
                | smelt_hir::BinOp::JsStrictEq
                | smelt_hir::BinOp::JsStrictNotEq
        ) {
            return Ok(None);
        }
        Ok(Some(format!(
            "{} {} {}",
            self.operand_text(lhs)?,
            smelt_hir::bin_op_text(op),
            self.operand_text(rhs)?
        )))
    }

    /// Renders a primitive conversion out of a `FixedInt`, if this is one.
    ///
    /// Every `FixedInt` value is exactly representable as an `f64`, so the
    /// `ToFloat`/`ToJsNumber` conversion is the plain `as f64` cast.
    pub(super) fn fixed_int_cast_text(
        op: smelt_hir::PrimitiveCastOp,
        dest: &Type,
        operand: &Type,
        operand_text: &str,
    ) -> Option<String> {
        match (op, dest, operand) {
            (
                smelt_hir::PrimitiveCastOp::ToFloat | smelt_hir::PrimitiveCastOp::ToJsNumber,
                Type::Float,
                Type::FixedInt(_),
            ) => Some(format!("{operand_text} as f64")),
            _ => None,
        }
    }

    /// The Rust cast suffix for a `.length` read stored in a `FixedInt` of `width`.
    ///
    /// A collection's `len()` is already a `usize`, so a `usize` destination
    /// needs no cast at all; `len_expr` is returned unchanged in that case
    /// unless it is a float literal (a function's declared arity).
    pub(super) fn fixed_int_len_text(len_expr: String, width: smelt_hir::IntWidth) -> String {
        if width == smelt_hir::IntWidth::Usize && !len_expr.ends_with(".0") {
            return len_expr;
        }
        format!("{len_expr} as {}", width.rust_name())
    }

    /// A `FixedInt` element index rendered as a Rust `usize`, if `index` is one.
    ///
    /// A `usize` index is used as is. A signed one goes through the same
    /// "negative addresses nothing" conversion as any other index, but straight
    /// from the integer, without the `f64` round trip.
    pub(super) fn fixed_int_index_text(
        &self,
        index: &Operand,
        usize_conversion: &str,
    ) -> Result<Option<String>, EmitError> {
        match self.mir.types.get(self.operand_ty(index)?) {
            Some(Type::FixedInt(smelt_hir::IntWidth::Usize)) => {
                Ok(Some(self.operand_text(index)?))
            }
            // `usize::try_from` accepts any signed width directly, so the
            // integer is bound as it is.
            Some(Type::FixedInt(_)) => Ok(Some(format!(
                "{{ let smelt_normalized = {}; {usize_conversion} }}",
                self.operand_text(index)?
            ))),
            _ => Ok(None),
        }
    }
}
