//! Numeric emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Emits Rust text that truncates a numeric expression through `f64`.
    ///
    /// JavaScript numeric coercions such as bit shifts and `toString(radix)` accept
    /// either integer-like or float-like operands. Rust's `.trunc()` method exists
    /// only on floats, so generated code must cast the receiver before truncating
    /// when the MIR operand may lower to an integer expression.
    pub(super) fn numeric_trunc_f64_text(&self, value_text: &str) -> String {
        format!("({value_text} as f64).trunc()")
    }

    /// Converts an operand for len() to its Rust text representation.
    pub(super) fn len_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_text(place),
            Operand::Const(_) => self.operand_text(operand),
        }
    }

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
        let operand_ty = self.operand_ty(operand)?;
        let emitted_ty = match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
                self.local_decl(*local)?.ty
            }
            _ => operand_ty,
        };
        let len_ty = if matches!(
            self.mir.types.get(emitted_ty),
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
        ) || self.is_erased_class_type(emitted_ty)
        {
            emitted_ty
        } else {
            operand_ty
        };
        let len_expr = match self.mir.types.get(len_ty) {
            Some(Type::Function(function))
                if self.is_erased_unknown_rest_function(function) && !function.may_throw =>
            {
                format!("{receiver_text}.length")
            }
            Some(Type::Function(_)) => {
                format!("{}.0", self.operand_function_length(operand)?)
            }
            Some(Type::String) => format!("{receiver_text}.chars().count()"),
            Some(Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::String)) =>
            {
                format!("{receiver_text}.as_ref().map_or(0, |value| value.chars().count())")
            }
            Some(Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::List(_))) =>
            {
                format!("{receiver_text}.as_ref().map_or(0, Vec::len)")
            }
            Some(Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Unknown)) =>
            {
                format!("{receiver_text}.as_ref().map_or(0, SmeltUnknown::len)")
            }
            Some(Type::Optional(inner))
                if matches!(
                    self.mir.types.get(*inner),
                    Some(Type::Union(_) | Type::TypeParam { .. })
                ) || self.is_erased_class_type(*inner) =>
            {
                // The unwrapped value is a concrete union, type parameter, or
                // erased class rather than a `SmeltUnknown`, so `SmeltUnknown::len`
                // cannot be used as the `map_or` mapper (its `&SmeltUnknown`
                // receiver mismatches the concrete borrow, E0631). JS `.length`
                // is a dynamic property whose meaning depends on the runtime
                // variant (string char count vs array length vs a length-bearing
                // object), so erase the borrowed value at this genuinely dynamic
                // boundary and inspect it, mirroring the non-optional case below.
                format!(
                    "{receiver_text}.as_ref().map_or(0, |value| match value.clone().into_smelt_unknown() {{ SmeltUnknown::String(value) => value.chars().count(), SmeltUnknown::Array(value) => value.len(), SmeltUnknown::Object(value) => match smelt_get_object_field(&value, \"length\") {{ SmeltUnknown::Number(value) => value as usize, _ => 0 }}, _ => 0 }})"
                )
            }
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                format!(
                    "match &{receiver_text} {{ SmeltUnknown::String(value) => value.chars().count(), SmeltUnknown::Array(value) => value.len(), SmeltUnknown::Object(value) => match smelt_get_object_field(value, \"length\") {{ SmeltUnknown::Number(value) => value as usize, _ => 0 }}, _ => 0 }}"
                )
            }
            // A fixed-arity tuple (e.g. a `[key, value]` narrowing) has no Rust
            // `.len()` method; its JavaScript `.length` is the compile-time
            // arity, so emit that constant rather than a method call.
            Some(Type::Tuple(items)) => format!("{}", items.len()),
            _ => format!("{receiver_text}.len()"),
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
        let operand_ty = self.operand_ty(operand)?;
        if matches!(self.mir.types.get(operand_ty), Some(Type::Int)) {
            let operand_text = self.operand_text(operand)?;
            return Ok(if matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
                operand_text
            } else {
                format!("{operand_text} as f64")
            });
        }
        // `floor`/`ceil`/`trunc` mean the same thing in both languages and map to
        // their `f64` methods. `Math.round` does not: JavaScript rounds a tie toward
        // +∞ while Rust's `f64::round` rounds a tie away from zero, so
        // `Math.round(-1.5)` is `-1` in JavaScript and `-2.0` in Rust. That one goes
        // through the runtime helper, which carries the JavaScript rule (and the
        // large-magnitude and `-0` edges with it).
        let method_name = match op {
            smelt_hir::NumericRoundOp::Floor => Some("floor"),
            smelt_hir::NumericRoundOp::Ceil => Some("ceil"),
            smelt_hir::NumericRoundOp::Round => None,
            smelt_hir::NumericRoundOp::Trunc => Some("trunc"),
        };
        let operand_text = if matches!(self.mir.types.get(operand_ty), Some(Type::Float)) {
            self.operand_text(operand)?
        } else if matches!(
            self.mir.types.get(operand_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(operand_ty)
        {
            self.value_at_type(operand, self.type_id(Type::Float)?)?
        } else {
            return Err(EmitError::new("numeric round operand must be numeric"));
        };
        let text = match method_name {
            Some(method_name) => format!("{operand_text}.{method_name}()"),
            None => format!(
                "{helper}({operand_text})",
                helper = smelt_stdlib::runtime_symbols::math::ROUND,
            ),
        };
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
        spread: Option<&Operand>,
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
        let render_arg = |arg: &Operand| -> Result<String, EmitError> {
            let text = self.value_at_type(arg, dest_ty)?;
            if !dest_is_int && matches!(arg, Operand::Const(Constant::Float(_))) {
                Ok(format!("({text} as f64)"))
            } else {
                Ok(text)
            }
        };
        // Fold the scalar arguments into a seed value, or fall back to the
        // identity element when the extrema is driven purely by a spread list
        // (`Math.max(...values)` with no leading scalar operands).
        let mut rendered = match args.split_first() {
            Some((first, rest)) => {
                let mut seed = render_arg(first)?;
                for arg in rest {
                    seed = format!("{seed}.{method_name}({})", render_arg(arg)?);
                }
                seed
            }
            None => identity.to_owned(),
        };
        // Reduce every element of a spread numeric list with the same
        // `min`/`max` method, converting each element to the destination type.
        if let Some(spread) = spread {
            let list_text = self.operand_text(spread)?;
            let element_ty = match self.mir.types.get(self.operand_ty(spread)?) {
                Some(Type::List(element_ty)) => *element_ty,
                _ => return Err(EmitError::new("numeric extrema spread must be a list")),
            };
            let element_text = self.value_at_type_text("smelt_element", element_ty, dest_ty)?;
            rendered = format!(
                "{list_text}.iter().cloned().fold({rendered}, |smelt_acc, smelt_element| smelt_acc.{method_name}({element_text}))"
            );
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
    pub(super) fn numeric_predicate_text(
        &self,
        op: smelt_hir::NumericPredicateOp,
        operand: &Operand,
    ) -> Result<String, EmitError> {
        let operand_ty = self.operand_ty(operand)?;
        let operand_text = if matches!(
            self.mir.types.get(operand_ty),
            Some(Type::Int | Type::Float)
        ) {
            self.float_operand_text(operand)?
        } else if matches!(
            self.mir.types.get(operand_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(operand_ty)
        {
            self.value_at_type(operand, self.type_id(Type::Float)?)?
        } else {
            return Ok("false".to_owned());
        };
        Ok(match op {
            smelt_hir::NumericPredicateOp::IsFinite => format!("{operand_text}.is_finite()"),
            smelt_hir::NumericPredicateOp::IsInteger => format!("{operand_text}.fract() == 0.0"),
            smelt_hir::NumericPredicateOp::IsNaN => format!("{operand_text}.is_nan()"),
        })
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
        Ok(format!("({base_text} as f64).powf({exponent_text} as f64)"))
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
        let value_trunc_text = self.numeric_trunc_f64_text(&operand_text);
        let radix_trunc_text = self.numeric_trunc_f64_text(&radix_text);
        Ok(format!(
            "{{ let value = {value_trunc_text} as i128; let radix = ({radix_trunc_text} as u32).clamp(2, 36); let negative = value < 0; let mut n = value.unsigned_abs(); let mut digits = Vec::new(); if n == 0 {{ digits.push('0'); }} while n > 0 {{ let digit = (n % u128::from(radix)) as u8; digits.push(if digit < 10 {{ (b'0' + digit) as char }} else {{ (b'a' + digit - 10) as char }}); n /= u128::from(radix); }} if negative {{ digits.push('-'); }} digits.iter().rev().collect::<String>() }}"
        ))
    }

    /// Formats a numeric value as a fixed-point decimal string.
    ///
    /// Mirrors `Number.prototype.toFixed(digits)`: the operand is rendered with
    /// exactly `digits` fractional digits (clamped to the JavaScript-supported
    /// `0..=100` range). The fractional count is truncated to an integer first,
    /// matching JavaScript's `ToInteger` coercion of the argument.
    pub(super) fn numeric_to_fixed_text(
        &self,
        operand: &Operand,
        digits: &Operand,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Int | Type::Float)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(digits)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new(
                "number.toFixed(digits) requires numeric operands",
            ));
        }
        let value_text = self.float_operand_text(operand)?;
        let digits_text = self.operand_text(digits)?;
        let digits_trunc_text = self.numeric_trunc_f64_text(&digits_text);
        Ok(format!(
            "{{ let smelt_value: f64 = {value_text}; let smelt_digits = ({digits_trunc_text} as i64).clamp(0, 100) as usize; format!(\"{{:.*}}\", smelt_digits, smelt_value) }}"
        ))
    }

    /// Parses an integer from a string with a JavaScript-style numeric radix.
    ///
    /// Mirrors `Number.parseInt`/`parseInt(str, radix)`: trims leading
    /// whitespace, accepts an optional sign, consumes the longest valid digit
    /// prefix in the radix, and yields `NaN` when no digits parse or the radix
    /// is out of the 2..=36 range. The result is `f64` to match the existing
    /// `parseInt` destination type.
    pub(super) fn parse_int_radix_text(
        &self,
        operand: &Operand,
        radix: &Operand,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::String)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(radix)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new(
                "parseInt(radix) requires a string operand and a numeric radix",
            ));
        }
        let operand_text = self.operand_text(operand)?;
        let radix_text = self.operand_text(radix)?;
        let radix_trunc_text = self.numeric_trunc_f64_text(&radix_text);
        Ok(format!(
            "{{ let smelt_src = {operand_text}; let smelt_radix = {{ let r = {radix_trunc_text} as i64; if r == 0 {{ 10 }} else {{ r }} }}; if !(2..=36).contains(&smelt_radix) {{ f64::NAN }} else {{ let smelt_trimmed = smelt_src.trim_start(); let (smelt_neg, smelt_rest) = match smelt_trimmed.strip_prefix('-') {{ Some(rest) => (true, rest), None => (false, smelt_trimmed.strip_prefix('+').unwrap_or(smelt_trimmed)) }}; let smelt_prefix: String = smelt_rest.chars().take_while(|c| c.to_digit(smelt_radix as u32).is_some()).collect(); if smelt_prefix.is_empty() {{ f64::NAN }} else {{ i64::from_str_radix(&smelt_prefix, smelt_radix as u32).map(|v| (if smelt_neg {{ -v }} else {{ v }}) as f64).unwrap_or(f64::NAN) }} }} }}"
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
