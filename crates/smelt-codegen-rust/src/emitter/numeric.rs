//! Numeric emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts an operand for len() to its Rust text representation.
    pub(super) fn len_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_text(place),
            Operand::Const(_) => self.operand_text(operand),
        }
    }

    /// Converts a length rvalue to Rust text for the destination numeric type.
    /// Converts a length rvalue to Rust text for the destination numeric type.
    pub(super) fn len_text(&self, operand: &Operand, dest_ty: TypeId) -> Result<String, EmitError> {
        let cast = match self.mir.types.get(dest_ty) {
            Some(Type::Int) => "i64",
            Some(Type::Float) => "f64",
            _ => {
                return Ok("Default::default()".to_owned());
            }
        };
        let receiver_text = self.len_operand_text(operand)?;
        let len_expr = if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String)
        ) {
            format!("{receiver_text}.chars().count()")
        } else {
            format!("{receiver_text}.len()")
        };
        Ok(format!("{len_expr} as {cast}"))
    }

    /// Converts a numeric absolute-value operation to Rust text.
    /// Converts a numeric absolute-value operation to Rust text.
    pub(super) fn numeric_abs_text(&self, operand: &Operand) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("numeric abs operand must be numeric"));
        }
        Ok(format!("{}.abs()", self.operand_text(operand)?))
    }

    /// Converts a numeric rounding operation to Rust text.
    /// Converts a numeric rounding operation to Rust text.
    pub(super) fn numeric_round_text(
        &self,
        op: smelt_hir::NumericRoundOp,
        operand: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Float)
        ) {
            return Ok(if matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
                "0_i64".to_owned()
            } else {
                "0.0".to_owned()
            });
        }
        let method_name = match op {
            smelt_hir::NumericRoundOp::Floor => "floor",
            smelt_hir::NumericRoundOp::Ceil => "ceil",
            smelt_hir::NumericRoundOp::Round => "round",
            smelt_hir::NumericRoundOp::Trunc => "trunc",
        };
        let text = format!("{}.{}()", self.operand_text(operand)?, method_name);
        if matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
            Ok(format!("{text} as i64"))
        } else {
            Ok(text)
        }
    }

    /// Converts a numeric extrema operation to Rust text.
    /// Converts a numeric extrema operation to Rust text.
    pub(super) fn numeric_extrema_text(
        &self,
        op: smelt_hir::NumericExtremaOp,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let dest_is_int = matches!(self.mir.types.get(dest_ty), Some(Type::Int));
        for arg in args {
            if self.operand_ty(arg)? != dest_ty
                || !matches!(self.mir.types.get(dest_ty), Some(Type::Int | Type::Float))
            {
                return Ok(if dest_is_int {
                    "0_i64".to_owned()
                } else {
                    "0.0".to_owned()
                });
            }
        }
        let identity = match op {
            smelt_hir::NumericExtremaOp::Min if dest_is_int => "i64::MAX",
            smelt_hir::NumericExtremaOp::Max if dest_is_int => "i64::MIN",
            smelt_hir::NumericExtremaOp::Min => "f64::INFINITY",
            smelt_hir::NumericExtremaOp::Max => "f64::NEG_INFINITY",
        };
        let method_name = match op {
            smelt_hir::NumericExtremaOp::Min => "min",
            smelt_hir::NumericExtremaOp::Max => "max",
        };
        let Some((first, rest)) = args.split_first() else {
            return Ok(identity.to_owned());
        };
        let mut rendered = self.operand_text(first)?;
        for arg in rest {
            rendered = format!("{rendered}.{method_name}({})", self.operand_text(arg)?);
        }
        Ok(rendered)
    }

    /// Converts a numeric hypot operation to Rust text.
    /// Converts a numeric hypot operation to Rust text.
    pub(super) fn numeric_hypot_text(&self, args: &[Operand]) -> Result<String, EmitError> {
        for arg in args {
            if !matches!(
                self.mir.types.get(self.operand_ty(arg)?),
                Some(Type::Int | Type::Float)
            ) {
                return Err(EmitError::new("numeric hypot operands must be numeric"));
            }
        }
        let mut rendered = "0.0f64".to_owned();
        for arg in args {
            rendered = format!("{rendered}.hypot({})", self.float_operand_text(arg)?);
        }
        Ok(rendered)
    }

    /// Converts a numeric predicate operation to Rust text.
    /// Converts a numeric predicate operation to Rust text.
    pub(super) fn numeric_predicate_text(
        &self,
        op: smelt_hir::NumericPredicateOp,
        operand: &Operand,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Int | Type::Float)
        ) {
            return Ok("false".to_owned());
        }
        let method_name = match op {
            smelt_hir::NumericPredicateOp::IsFinite => "is_finite",
            smelt_hir::NumericPredicateOp::IsInteger => "fract() == 0.0",
            smelt_hir::NumericPredicateOp::IsNaN => "is_nan",
        };
        if matches!(op, smelt_hir::NumericPredicateOp::IsInteger) {
            return Ok(format!(
                "{}.fract() == 0.0",
                self.float_operand_text(operand)?
            ));
        }
        Ok(format!(
            "{}.{}()",
            self.float_operand_text(operand)?,
            method_name
        ))
    }

    /// Converts a direct unary numeric function to Rust text.
    /// Converts a direct unary numeric function to Rust text.
    pub(super) fn numeric_unary_func_text(
        &self,
        op: smelt_hir::NumericUnaryFuncOp,
        operand: &Operand,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new(
                "numeric unary function operand must be numeric",
            ));
        }
        let method_name = match op {
            smelt_hir::NumericUnaryFuncOp::Sqrt => "sqrt",
            smelt_hir::NumericUnaryFuncOp::Cbrt => "cbrt",
            smelt_hir::NumericUnaryFuncOp::Sign => "signum",
            smelt_hir::NumericUnaryFuncOp::Sin => "sin",
            smelt_hir::NumericUnaryFuncOp::Cos => "cos",
            smelt_hir::NumericUnaryFuncOp::Tan => "tan",
            smelt_hir::NumericUnaryFuncOp::Asin => "asin",
            smelt_hir::NumericUnaryFuncOp::Acos => "acos",
            smelt_hir::NumericUnaryFuncOp::Atan => "atan",
            smelt_hir::NumericUnaryFuncOp::Log => "ln",
            smelt_hir::NumericUnaryFuncOp::Log10 => "log10",
            smelt_hir::NumericUnaryFuncOp::Log2 => "log2",
            smelt_hir::NumericUnaryFuncOp::Exp => "exp",
        };
        let operand_text = self.float_operand_text(operand)?;
        Ok(format!("{operand_text}.{method_name}()"))
    }

    /// Converts a numeric power operation to Rust text.
    /// Converts a numeric power operation to Rust text.
    pub(super) fn numeric_pow_text(
        &self,
        base: &Operand,
        exponent: &Operand,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(base)?),
            Some(Type::Int | Type::Float)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(exponent)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("numeric pow operands must be numeric"));
        }
        let base_text = self.float_operand_text(base)?;
        let exponent_text = self.float_operand_text(exponent)?;
        Ok(format!("{base_text}.powf({exponent_text})"))
    }

    /// Converts a two-argument arctangent operation to Rust text.
    /// Converts a two-argument arctangent operation to Rust text.
    pub(super) fn numeric_atan2_text(
        &self,
        y_operand: &Operand,
        x_operand: &Operand,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(y_operand)?),
            Some(Type::Int | Type::Float)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(x_operand)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("numeric atan2 operands must be numeric"));
        }
        let y_text = self.float_operand_text(y_operand)?;
        let x_text = self.float_operand_text(x_operand)?;
        Ok(format!("{y_text}.atan2({x_text})"))
    }

    /// Converts an inclusive integer random operation to Rust text.
    /// Converts an inclusive integer random operation to Rust text.
    pub(super) fn numeric_random_int_text(
        &self,
        start: &Operand,
        end: &Operand,
    ) -> Result<String, EmitError> {
        if self.mir.types.get(self.operand_ty(start)?) != Some(&Type::Int)
            || self.mir.types.get(self.operand_ty(end)?) != Some(&Type::Int)
        {
            return Err(EmitError::new("random integer bounds must be integers"));
        }
        Ok(format!(
            "rand::random_range({}..={})",
            self.operand_text(start)?,
            self.operand_text(end)?
        ))
    }

    /// Converts a numeric value to string text with a JavaScript-style radix.
    pub(super) fn numeric_to_string_radix_text(
        &self,
        operand: &Operand,
        radix: &Operand,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Int | Type::Float)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(radix)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new(
                "number.toString(radix) requires numeric operands",
            ));
        }
        let operand_text = self.operand_text(operand)?;
        let radix_text = self.operand_text(radix)?;
        Ok(format!(
            "{{ let value = {operand_text}.trunc() as i128; let radix = ({radix_text}.trunc() as u32).clamp(2, 36); let negative = value < 0; let mut n = value.unsigned_abs(); let mut digits = Vec::new(); if n == 0 {{ digits.push('0'); }} while n > 0 {{ let digit = (n % u128::from(radix)) as u8; digits.push(if digit < 10 {{ (b'0' + digit) as char }} else {{ (b'a' + digit - 10) as char }}); n /= u128::from(radix); }} if negative {{ digits.push('-'); }} digits.iter().rev().collect::<String>() }}"
        ))
    }

    /// Converts a numeric operand to text usable as an `f64` receiver or argument.
    /// Converts a numeric operand to text usable as an `f64` receiver or argument.
    pub(super) fn float_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let operand_text = self.operand_text(operand)?;
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Int)
        ) {
            Ok(format!("({operand_text} as f64)"))
        } else {
            Ok(operand_text)
        }
    }

    // Converts a primitive Python-style cast operation to Rust text.
}
