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
//!
//! Bun's `int32` slots (`FixedInt(I32)`, from `import type { int32 } from
//! "bun"`) are storage only: the TypeScript frontend converts every read to a
//! `number` (`ToFloat`) and every proven store to the slot (`ToInt32`), so an
//! `i32` never takes part in arithmetic here. Integer-closed arithmetic is
//! rendered only for the range-analysis widths (`usize`, `i64`).

use super::*;

impl FunctionEmitter<'_> {
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
        let counter = |ty: TypeId| {
            matches!(
                self.mir.types.get(ty),
                Some(Type::FixedInt(smelt_hir::IntWidth::Usize | smelt_hir::IntWidth::I64))
            )
        };
        let lhs_fixed = counter(self.operand_ty(lhs)?);
        let rhs_fixed = counter(self.operand_ty(rhs)?);
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

    /// Renders a primitive conversion into or out of a `FixedInt`, if this is one.
    ///
    /// Every `FixedInt` value is exactly representable as an `f64`, so the
    /// `ToFloat`/`ToJsNumber` conversion is the plain `as f64` cast (mapped
    /// over an `Option` for an optional int32 slot). `ToInt32` stores a value
    /// the frontend proved to be a whole number in the `i32` range, so its
    /// `as i32` is exact as well.
    pub(super) fn fixed_int_cast_text(
        &self,
        op: smelt_hir::PrimitiveCastOp,
        dest: &Type,
        operand: &Type,
        operand_text: &str,
    ) -> Option<String> {
        let is_fixed = |ty: &TypeId| matches!(self.mir.types.get(*ty), Some(Type::FixedInt(_)));
        let is_float = |ty: &TypeId| matches!(self.mir.types.get(*ty), Some(Type::Float));
        match (op, dest, operand) {
            (
                smelt_hir::PrimitiveCastOp::ToFloat | smelt_hir::PrimitiveCastOp::ToJsNumber,
                Type::Float,
                Type::FixedInt(_),
            ) => Some(format!("{operand_text} as f64")),
            (smelt_hir::PrimitiveCastOp::ToFloat, Type::Optional(to), Type::Optional(from))
                if is_float(to) && is_fixed(from) =>
            {
                Some(format!("{operand_text}.map(f64::from)"))
            }
            (smelt_hir::PrimitiveCastOp::ToInt32, Type::FixedInt(width), Type::Float) => {
                Some(format!("{operand_text} as {}", width.rust_name()))
            }
            (smelt_hir::PrimitiveCastOp::ToInt32, Type::Optional(to), Type::Optional(from))
                if is_fixed(to) && is_float(from) =>
            {
                Some(format!("{operand_text}.map(|value| value as i32)"))
            }
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

    /// JavaScript `ToInt32` of a numeric expression, as an `i32` expression.
    ///
    /// Truncates toward zero, takes the result modulo `2^32` and reads it as a
    /// signed 32-bit integer; `NaN` and `±Infinity` are `0`. This is the exact
    /// spec algorithm (ECMA-262 `ToInt32`), so `x | 0`, `x as int32` and every
    /// int32 operator agree with every JavaScript engine.
    pub(super) fn js_to_int32_text(value_text: &str) -> String {
        format!(
            "{{ let smelt_int32_v = ({value_text} as f64).trunc(); if smelt_int32_v.is_finite() {{ smelt_int32_v.rem_euclid(4294967296.0) as u32 as i32 }} else {{ 0_i32 }} }}"
        )
    }

    /// JavaScript `ToUint32` of a numeric expression, as a `u32` expression.
    pub(super) fn js_to_uint32_text(value_text: &str) -> String {
        format!(
            "{{ let smelt_uint32_v = ({value_text} as f64).trunc(); if smelt_uint32_v.is_finite() {{ smelt_uint32_v.rem_euclid(4294967296.0) as u32 }} else {{ 0_u32 }} }}"
        )
    }

    /// JavaScript `a << b`, `a >> b` and `Math.imul(a, b)` on `number`s.
    ///
    /// All three take `ToInt32(a)`; the shifts take the count as
    /// `ToUint32(b) & 31`, `<<` wraps out of the int32 range, `>>` is the
    /// arithmetic (sign-propagating) shift, and `Math.imul` is the low 32 bits
    /// of the product read as signed. The int32 result converts to `f64`
    /// exactly (or to `i64` for an `Int` destination).
    pub(super) fn js_int32_binary_text(
        op: smelt_hir::BinOp,
        lhs_text: &str,
        rhs_text: &str,
        result_cast: &str,
    ) -> String {
        let lhs = Self::js_to_int32_text(lhs_text);
        let int32 = match op {
            smelt_hir::BinOp::IMul => format!("({lhs}).wrapping_mul({})", Self::js_to_int32_text(rhs_text)),
            smelt_hir::BinOp::Shl => {
                format!("({lhs}).wrapping_shl({} & 31)", Self::js_to_uint32_text(rhs_text))
            }
            _ => format!("({lhs}).wrapping_shr({} & 31)", Self::js_to_uint32_text(rhs_text)),
        };
        format!("(({int32}) as {result_cast})")
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
