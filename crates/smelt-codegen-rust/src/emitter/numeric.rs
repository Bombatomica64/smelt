//! Numeric emission helpers.

use super::*;
use crate::rust::RustExpr;
use smelt_hir::NumericFormatMethod;

impl FunctionEmitter<'_> {
    /// Emits Rust text that truncates a numeric expression through `f64`.
    ///
    /// JavaScript numeric coercions such as bit shifts and `toString(radix)` accept
    /// either integer-like or float-like operands. Rust's `.trunc()` method exists
    /// only on floats, so generated code must cast the receiver before truncating
    /// when the MIR operand may lower to an integer expression.
    pub(super) fn numeric_trunc_f64_expr(&self, value_text: &RustExpr) -> RustExpr {
        RustExpr::atom(format!(
            "({value_text} as f64).trunc()",
            value_text = value_text.as_cast_operand()
        ))
    }

    /// Converts an operand for len() to its Rust text representation.
    pub(super) fn len_operand_expr(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_expr(place),
            Operand::Const(_) => self.operand_expr(operand),
        }
    }

    /// Converts a length rvalue to Rust text for the destination numeric type.
    pub(super) fn len_expr(
        &self,
        operand: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let fixed_width = match self.mir.types.get(dest_ty) {
            Some(Type::FixedInt(width)) => Some(*width),
            _ => None,
        };
        let cast = match self.mir.types.get(dest_ty) {
            Some(Type::Int) => "i64",
            Some(Type::Float) => "f64",
            Some(Type::FixedInt(width)) => width.rust_name(),
            _ => return Err(EmitError::new("length destination must be numeric")),
        };
        let receiver_text = self.len_operand_expr(operand)?;
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
        let mut is_float_literal = false;
        let len_expr = match self.mir.types.get(len_ty) {
            // `Function.prototype.length` is the declared parameter count, and
            // WHERE that count lives depends on the value's emitted Rust
            // representation, not on its MIR shape. Only the `SmeltErasedFunction`
            // struct carries the arity in a `length` field; a borrowed `&dyn Fn`
            // handle (every function *parameter*, whatever its MIR shape — see
            // `param_type`) has no field storage at all, so reading `.length`
            // off it is E0609. Ask the one authority that already answers "which
            // representation is this value" for the call-shape ladder, so the two
            // cannot drift apart again.
            Some(Type::Function(_)) if self.callee_uses_erased_call_method(operand)? => {
                format!(
                    "{receiver_text}.length",
                    receiver_text = receiver_text.tight()
                )
            }
            // Any other callable is a concrete handle whose arity is fixed by its
            // declared signature, so the count is a compile-time constant.
            Some(Type::Function(_)) => {
                is_float_literal = true;
                format!("{}.0", self.operand_function_length(operand)?)
            }
            Some(Type::String | Type::PyStr) => {
                self.string_len_text(&receiver_text.parenthesized_if_needed())
            }
            Some(Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::String | Type::PyStr)) =>
            {
                format!(
                    "{receiver_text}.as_ref().map_or(0, |smelt_value| {})",
                    self.string_len_text("smelt_value"),
                    receiver_text = receiver_text.tight()
                )
            }
            Some(Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::List(_))) =>
            {
                format!(
                    "{receiver_text}.as_ref().map_or(0, |smelt_value| smelt_value.len())",
                    receiver_text = receiver_text.tight()
                )
            }
            Some(Type::Optional(inner))
                if matches!(self.mir.types.get(*inner), Some(Type::Unknown)) =>
            {
                self.carrier_format(
                    self.carrier()?,
                    format_args!(
                        "{receiver_text}.as_ref().map_or(0, SmeltUnknown::len)",
                        receiver_text = receiver_text.tight()
                    ),
                )
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
                self.carrier_format(self.carrier()?, format_args!(
                    "{receiver_text}.as_ref().map_or(0, |smelt_value| match smelt_value.clone().into_smelt_unknown() {{ SmeltUnknown::String(smelt_value) => {string_len}, SmeltUnknown::Array(smelt_value) => smelt_value.len(), SmeltUnknown::Object(smelt_value) => match smelt_get_object_field(&smelt_value, \"length\") {{ SmeltUnknown::Number(smelt_value) => smelt_value as usize, _ => {function_length}(&SmeltUnknown::Object(smelt_value)) as usize }}, smelt_value @ SmeltUnknown::Function(_) => {function_length}(&smelt_value) as usize, _ => 0 }})",
                    function_length = smelt_stdlib::runtime_symbols::function_length::READ,
                    string_len = self.string_len_text("smelt_value"), receiver_text = receiver_text.tight(),
                ))
            }
            // A callable's `.length` is its declared arity, which the erasure
            // boundary parks in the function-length registry (an `Rc<dyn Fn>` has
            // nowhere to carry it). Without the `Function` arm an erased callable
            // answered `0`, and es-toolkit `rest(func)` — whose default split point
            // is `func.length - 1` — computed `-1` and reshaped every call wrongly.
            Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                // The receiver is bound to a reserved name before the match, not
                // re-spelled inside it: every arm binds `value`, so a receiver
                // whose own Rust name IS `value` (a source parameter called
                // `value`, which is the commonest name in a generic helper) had
                // its arm binding shadow it and the `Function` fallbacks passed
                // `&&SmeltObject` where a `&SmeltUnknown` was expected (E0308).
                // A `Map` or `Set` behind the carrier counts its entries: a
                // count read off one is its `size` (`length` is no member of
                // either, so TypeScript only lets `size` reach here).
                self.carrier_format(self.carrier()?, format_args!(
                    "{{ let smelt_length_receiver = &{receiver_text}; match smelt_length_receiver {{ SmeltUnknown::String(smelt_value) => {string_len}, SmeltUnknown::Array(smelt_value) => smelt_value.len(), SmeltUnknown::Object(smelt_value) if smelt_value.contains_key(\"__smelt_map\") || smelt_value.contains_key(\"__smelt_set\") => match smelt_get_unknown_field(smelt_length_receiver, \"size\") {{ SmeltUnknown::Number(smelt_value) => smelt_value as usize, _ => 0 }}, SmeltUnknown::Object(smelt_value) => match smelt_get_object_field(smelt_value, \"length\") {{ SmeltUnknown::Number(smelt_value) => smelt_value as usize, _ => {function_length}(smelt_length_receiver) as usize }}, SmeltUnknown::Function(_) => {function_length}(smelt_length_receiver) as usize, _ => 0 }} }}",
                    function_length = smelt_stdlib::runtime_symbols::function_length::READ,
                    string_len = self.string_len_text("smelt_value"), receiver_text = receiver_text.tight(),
                ))
            }
            // A fixed-arity tuple (e.g. a `[key, value]` narrowing) has no Rust
            // `.len()` method; its JavaScript `.length` is the compile-time
            // arity, so emit that constant rather than a method call.
            Some(Type::Tuple(items)) => format!("{}", items.len()),
            _ => format!(
                "{receiver_text}.len()",
                receiver_text = receiver_text.tight()
            ),
        };
        if let Some(width) = fixed_width {
            return Ok(Self::fixed_int_len_expr(len_expr, is_float_literal, width));
        }
        Ok(RustExpr::cast(format!("{len_expr} as {cast}")))
    }

    /// Pins a float literal receiver to `f64` before a method call: a bare `5.5`
    /// has no inferred width and `5.5.floor()` does not compile (E0689). Any other
    /// operand already has a concrete type and renders unchanged.
    fn float_method_receiver(rendered: RustExpr, operand: &Operand) -> RustExpr {
        if matches!(operand, Operand::Const(Constant::Float(_))) {
            RustExpr::atom(format!(
                "({text} as f64)",
                text = rendered.as_cast_operand()
            ))
        } else {
            rendered
        }
    }

    /// Converts a numeric absolute-value operation to Rust text.
    pub(super) fn numeric_abs_expr(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("numeric abs operand must be numeric"));
        }
        Ok(Self::float_method_receiver(self.operand_expr(operand)?, operand).postfix(".abs()"))
    }

    /// Converts a numeric rounding operation to Rust text.
    pub(super) fn numeric_round_expr(
        &self,
        op: smelt_hir::NumericRoundOp,
        operand: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let operand_ty = self.operand_ty(operand)?;
        if matches!(self.mir.types.get(operand_ty), Some(Type::Int)) {
            let operand_expr = self.operand_expr(operand)?;
            return Ok(if matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
                operand_expr
            } else {
                RustExpr::cast(format!(
                    "{operand_expr} as f64",
                    operand_expr = operand_expr.as_cast_operand()
                ))
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
        let operand_expr = if matches!(self.mir.types.get(operand_ty), Some(Type::Float)) {
            self.operand_expr(operand)?
        } else if self.has_dynamic_shape(operand_ty) {
            self.value_at_type(operand, self.type_id(Type::Float)?)?
        } else {
            return Err(EmitError::new("numeric round operand must be numeric"));
        };
        let text = match method_name {
            Some(method_name) => Self::float_method_receiver(operand_expr, operand)
                .postfix(format!(".{method_name}()")),
            None => RustExpr::chain(format!(
                "{helper}({operand_expr})",
                helper = smelt_stdlib::runtime_symbols::math::ROUND,
                operand_expr = operand_expr.delimited(),
            )),
        };
        if matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
            Ok(RustExpr::cast(format!(
                "{text} as i64",
                text = text.as_cast_operand()
            )))
        } else {
            Ok(text)
        }
    }

    /// Converts a numeric extrema operation to Rust text.
    pub(super) fn numeric_extrema_expr(
        &self,
        op: smelt_hir::NumericExtremaOp,
        args: &[Operand],
        spread: Option<&Operand>,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let dest_is_int = matches!(self.mir.types.get(dest_ty), Some(Type::Int));
        for arg in args {
            if self.operand_ty(arg)? != dest_ty
                || !matches!(self.mir.types.get(dest_ty), Some(Type::Int | Type::Float))
            {
                return Ok(if dest_is_int {
                    RustExpr::path("0_i64")
                } else {
                    RustExpr::path("0.0")
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
                Ok(format!("({text} as f64)", text = text.as_cast_operand()))
            } else {
                Ok(text.into_string())
            }
        };
        // Fold the scalar arguments into a seed value, or fall back to the
        // identity element when the extrema is driven purely by a spread list
        // (`Math.max(...values)` with no leading scalar operands).
        let mut rendered = match args.split_first() {
            Some((first, rest)) => {
                let mut seed = render_arg(first)?;
                for arg in rest {
                    seed = RustExpr::atom(seed)
                        .postfix(format!(".{method_name}({})", render_arg(arg)?))
                        .into_string();
                }
                seed
            }
            None => RustExpr::from(identity.to_owned()).into_string(),
        };
        // Reduce every element of a spread numeric list with the same
        // `min`/`max` method, converting each element to the destination type.
        if let Some(spread) = spread {
            let list_text = list_read_expr(&self.operand_expr(spread)?);
            let element_ty = match self.mir.types.get(self.operand_ty(spread)?) {
                Some(Type::List(element_ty)) => *element_ty,
                _ => return Err(EmitError::new("numeric extrema spread must be a list")),
            };
            let element_text = self.rendered_at_type(
                &RustExpr::path("smelt_element"),
                element_ty,
                dest_ty,
                &self.render_scope(),
            )?;
            rendered = list_text.postfix(format!(
                ".iter().cloned().fold({rendered}, |smelt_acc, smelt_element| smelt_acc.{method_name}({element_text}))", element_text = element_text.delimited()
            )).into_string();
        }
        Ok(RustExpr::from(rendered))
    }

    /// Converts a numeric hypot operation to Rust text.
    pub(super) fn numeric_hypot_expr(&self, args: &[Operand]) -> Result<RustExpr, EmitError> {
        for arg in args {
            if !matches!(
                self.mir.types.get(self.operand_ty(arg)?),
                Some(Type::Int | Type::Float)
            ) {
                return Err(EmitError::new("numeric hypot operands must be numeric"));
            }
        }
        let mut rendered = RustExpr::path("0.0f64");
        for arg in args {
            rendered = rendered.postfix(format!(
                ".hypot({})",
                self.float_operand_expr(arg)?.delimited()
            ));
        }
        Ok(rendered)
    }

    /// Converts a numeric predicate operation to Rust text.
    pub(super) fn numeric_predicate_expr(
        &self,
        op: smelt_hir::NumericPredicateOp,
        operand: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let operand_ty = self.operand_ty(operand)?;
        let is_numeric_operand = matches!(
            self.mir.types.get(operand_ty),
            Some(Type::Int | Type::Float)
        );
        let is_erased_operand = self.has_dynamic_shape(operand_ty);
        // An optional NUMBER reaches here only from a non-coercing
        // `Number.*` predicate (the frontend asserts presence for the
        // coercing global ones): absent is "not a number", so the predicate
        // is `false` there and tested on the value where present.
        if let Some(Type::Optional(inner)) = self.mir.types.get(operand_ty)
            && matches!(self.mir.types.get(*inner), Some(Type::Int | Type::Float))
        {
            let test = match op {
                smelt_hir::NumericPredicateOp::IsFinite => "smelt_value.is_finite()",
                smelt_hir::NumericPredicateOp::IsInteger => "smelt_value.fract() == 0.0",
                smelt_hir::NumericPredicateOp::IsNaN => "smelt_value.is_nan()",
                smelt_hir::NumericPredicateOp::IsSafeInteger => {
                    "smelt_value.fract() == 0.0 && smelt_value.abs() <= 9007199254740991.0"
                }
            };
            return Ok({
                let receiver = self.operand_expr(operand)?;
                receiver.postfix(format!(".map_or(false, |smelt_value| {{ let smelt_value = smelt_value as f64; {test} }})"))
            });
        }
        // `Number.isSafeInteger` performs NO type coercion: ECMAScript returns
        // `false` for every argument whose type is not Number, so `'1'` is not a
        // safe integer even though `Number('1')` is. An erased operand therefore
        // has to be tested on its RUNTIME TAG rather than pushed through the
        // number coercion the other predicates here use.
        if matches!(op, smelt_hir::NumericPredicateOp::IsSafeInteger) && !is_numeric_operand {
            let carries_runtime_tag = is_erased_operand
                || matches!(self.mir.types.get(operand_ty), Some(Type::Optional(_)));
            if !carries_runtime_tag {
                return Ok(RustExpr::path("false"));
            }
            let value_text = self.value_at_type(operand, self.type_id(Type::Unknown)?)?;
            return Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "(match {value_text} {{ SmeltUnknown::Number(smelt_value) => smelt_value.fract() == 0.0 && smelt_value.abs() <= 9007199254740991.0, _ => false }})", value_text = value_text.delimited()
            ))));
        }
        let operand_expr = if is_numeric_operand {
            self.float_operand_expr(operand)?
        } else if is_erased_operand {
            self.value_at_type(operand, self.type_id(Type::Float)?)?
        } else {
            return Ok(RustExpr::path("false"));
        };
        Ok(match op {
            smelt_hir::NumericPredicateOp::IsFinite => operand_expr.postfix(".is_finite()"),
            smelt_hir::NumericPredicateOp::IsInteger => RustExpr::loose(format!(
                "{operand_expr}.fract() == 0.0",
                operand_expr = operand_expr.tight()
            )),
            // `Number.isSafeInteger(x)` is `Number.isInteger(x)` narrowed to the
            // range a double represents exactly. The operand is bound once so a
            // side-effecting expression is not evaluated twice.
            smelt_hir::NumericPredicateOp::IsSafeInteger => RustExpr::atom(format!(
                "{{ let smelt_value: f64 = {operand_expr}; smelt_value.fract() == 0.0 && smelt_value.abs() <= 9007199254740991.0 }}",
                operand_expr = operand_expr.delimited()
            )),
            smelt_hir::NumericPredicateOp::IsNaN => operand_expr.postfix(".is_nan()"),
        })
    }

    /// Converts a direct unary numeric function to Rust text.
    pub(super) fn numeric_unary_func_expr(
        &self,
        op: smelt_hir::NumericUnaryFuncOp,
        operand: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new(
                "numeric unary function operand must be numeric",
            ));
        }
        if op == smelt_hir::NumericUnaryFuncOp::Clz32 {
            let operand_expr = self.float_operand_expr(operand)?;
            return Ok(RustExpr::atom(format!(
                "f64::from({}.leading_zeros())",
                Self::js_to_uint32_expr(&operand_expr).tight()
            )));
        }
        let method_name = match op {
            smelt_hir::NumericUnaryFuncOp::Clz32 => "leading_zeros",
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
        let operand_expr = self.float_operand_expr(operand)?;
        // `f64::signum` answers 1.0 for 0.0, while `Math.sign` returns a zero (and
        // NaN) operand unchanged.
        if op == smelt_hir::NumericUnaryFuncOp::Sign {
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_sign_operand = {operand_expr} as f64; if smelt_sign_operand == 0.0 || smelt_sign_operand.is_nan() {{ smelt_sign_operand }} else {{ smelt_sign_operand.signum() }} }}",
                operand_expr = operand_expr.as_cast_operand(),
            )));
        }
        // A literal operand (`Math.sqrt(2)` renders `2.0`) has no inferred
        // float width, and a method call on `{float}` does not compile; pin it
        // with the same `as f64` the binary helpers use.
        Ok(RustExpr::atom(format!(
            "({operand_expr} as f64).{method_name}()",
            operand_expr = operand_expr.as_cast_operand()
        )))
    }

    /// Converts a numeric power operation to Rust text.
    pub(super) fn numeric_pow_expr(
        &self,
        base: &Operand,
        exponent: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(base)?),
            Some(Type::Int | Type::Float)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(exponent)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("numeric pow operands must be numeric"));
        }
        let base_text = self.float_operand_expr(base)?;
        let exponent_text = self.float_operand_expr(exponent)?;
        Ok(RustExpr::atom(format!(
            "({base_text} as f64).powf({exponent_text} as f64)",
            base_text = base_text.as_cast_operand(),
            exponent_text = exponent_text.as_cast_operand()
        )))
    }

    /// Converts a two-argument arctangent operation to Rust text.
    pub(super) fn numeric_atan2_expr(
        &self,
        y_operand: &Operand,
        x_operand: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(y_operand)?),
            Some(Type::Int | Type::Float)
        ) || !matches!(
            self.mir.types.get(self.operand_ty(x_operand)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("numeric atan2 operands must be numeric"));
        }
        let y_text = self.float_operand_expr(y_operand)?;
        let x_text = self.float_operand_expr(x_operand)?;
        Ok(Self::float_method_receiver(y_text, y_operand)
            .postfix(format!(".atan2({x_text})", x_text = x_text.delimited())))
    }

    /// Converts an inclusive integer random operation to Rust text.
    pub(super) fn numeric_random_int_expr(
        &self,
        start: &Operand,
        end: &Operand,
    ) -> Result<RustExpr, EmitError> {
        if self.mir.types.get(self.operand_ty(start)?) != Some(&Type::Int)
            || self.mir.types.get(self.operand_ty(end)?) != Some(&Type::Int)
        {
            return Err(EmitError::new("random integer bounds must be integers"));
        }
        Ok(RustExpr::atom(format!(
            "rand::random_range({}..={})",
            self.operand_expr(start)?.delimited(),
            self.operand_expr(end)?.delimited()
        )))
    }

    /// Converts a numeric value to string text with a JavaScript-style radix.
    pub(super) fn numeric_to_string_radix_expr(
        &self,
        operand: &Operand,
        radix: &Operand,
    ) -> Result<RustExpr, EmitError> {
        // `bigint.toString(radix)` keeps every digit: the value is never
        // narrowed to a machine integer first.
        if self.is_bigint_type(self.operand_ty(operand)?) {
            let radix_operand = self.operand_expr(radix)?;
            let radix_text = if matches!(
                self.mir.types.get(self.operand_ty(radix)?),
                Some(Type::Float)
            ) {
                radix_operand
            } else {
                RustExpr::cast(format!(
                    "{radix_operand} as f64",
                    radix_operand = radix_operand.as_cast_operand()
                ))
            };
            // The frontend lowers a bigint receiver to the fallible
            // `BigIntOp::ToStringRadix` call; this rvalue form only remains
            // should a pass rebuild one, so its throw takes the panic route.
            return Ok(RustExpr::chain(format!(
                "smelt_bigint_to_string_radix({}, {radix_text}).unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error))",
                self.bigint_ref_expr(operand)?.delimited(),
                radix_text = radix_text.delimited()
            )));
        }
        // An ERASED receiver decides at run time, as JavaScript does:
        // `Number.prototype.toString(radix)` for a number, and the receiver's
        // own `toString` (which ignores the argument) for anything else. The
        // number and bigint arms answer a host `String`, converted (`.into()`)
        // to the fallback's string type, which is the program's.
        // `Array.prototype.map.call(bytes, (x) => x.toString(16))` hands the
        // callback an erased element.
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Unknown)
        ) && matches!(
            self.mir.types.get(self.operand_ty(radix)?),
            Some(Type::Int | Type::Float)
        ) {
            let string_ty = self.type_id(Type::String)?;
            let fallback =
                self.primitive_cast_expr(smelt_hir::PrimitiveCastOp::ToString, operand, string_ty)?;
            let radix_text = self.operand_expr(radix)?;
            let number_text = RustExpr::chain(format!(
                "smelt_number_format_checked(\"toString\", smelt_radix_value, Some({radix} as f64)).unwrap_or_else(|smelt_message| smelt_panic_throw(smelt_number_range_error(smelt_message))).into()",
                radix = radix_text.as_cast_operand()
            ));
            return Ok(RustExpr::atom(self.carrier_format(self.carrier()?, format_args!(
                "match {}.clone() {{ SmeltUnknown::Number(smelt_radix_value) => {number_text}, {}_ => {fallback} }}",
                self.operand_expr(operand)?.tight(),
                self.bigint_unknown_arm(&self.carrier_format(self.carrier()?, format_args!(
                    "SmeltUnknown::BigInt(smelt_radix_value) => smelt_bigint_to_string_radix(&smelt_radix_value, {radix_text} as f64).unwrap_or_else(|smelt_error| smelt_panic_throw(smelt_error)).into(),", radix_text = radix_text.as_cast_operand()
                ))), number_text = number_text.delimited(), fallback = fallback.delimited()
            ))));
        }
        // A NUMBER receiver lowers to `Rvalue::NumericFormat` (its radix can
        // throw a `RangeError`, which only that op's call form can carry).
        Err(EmitError::new(
            "internal: a number's toString(radix) lowers to NumericFormat",
        ))
    }

    /// The argument of a `Number.prototype` formatting method as the `usize`
    /// the runtime formatter takes, for an argument the frontend proved is a
    /// constant inside the method's range (so `as usize` is `ToIntegerOrInfinity`).
    fn numeric_format_argument(&self, digits: &Operand) -> Result<RustExpr, EmitError> {
        let digits_text = self.operand_expr(digits)?;
        Ok(RustExpr::cast(format!(
            "{} as usize",
            digits_text.as_cast_operand()
        )))
    }

    /// Renders a `Number.prototype` formatting method whose argument cannot
    /// throw (absent, or a constant in range): a direct call of the runtime
    /// formatter (`number_format.rs`), which rounds exact ties away from zero
    /// as the spec does.
    pub(super) fn numeric_format_expr(
        &self,
        method: NumericFormatMethod,
        operand: &Operand,
        digits: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let value_text = self.float_operand_expr(operand)?;
        if method == NumericFormatMethod::Radix {
            let radix = digits
                .ok_or_else(|| EmitError::new("internal: toString(radix) needs its radix"))?;
            return Ok(RustExpr::chain(format!(
                "smelt_number_to_string_radix({}, {} as u32)",
                value_text.delimited(),
                self.operand_expr(radix)?.as_cast_operand()
            )));
        }
        let argument = digits
            .map(|written| self.numeric_format_argument(written))
            .transpose()?;
        let call = match (method, argument) {
            (NumericFormatMethod::Fixed, fixed_digits) => format!(
                "smelt_number_to_fixed({}, {})",
                value_text.delimited(),
                fixed_digits.map_or_else(|| "0".to_owned(), |count| count.delimited().to_owned())
            ),
            (NumericFormatMethod::Precision | NumericFormatMethod::Exponential, count) => {
                format!(
                    "{}({}, {})",
                    if method == NumericFormatMethod::Precision {
                        "smelt_number_to_precision"
                    } else {
                        "smelt_number_to_exponential"
                    },
                    value_text.delimited(),
                    count.map_or_else(
                        || "None".to_owned(),
                        |written| format!("Some({})", written.delimited())
                    )
                )
            }
            (NumericFormatMethod::Radix, _) => {
                return Err(EmitError::new(
                    "internal: toString(radix) is rendered above",
                ));
            }
        };
        Ok(RustExpr::chain(call))
    }

    /// Renders a `Number.prototype` formatting method whose argument may fall
    /// outside the method's range: the checked formatter, propagating the
    /// `RangeError` it raises.
    pub(super) fn numeric_format_call(
        &self,
        method: NumericFormatMethod,
        operand: &Operand,
        digits: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let value_text = self.float_operand_expr(operand)?;
        let argument = match digits {
            Some(written) => format!(
                "Some({} as f64)",
                self.operand_expr(written)?.as_cast_operand()
            ),
            None => "None".to_owned(),
        };
        Ok(RustExpr::chain(format!(
            "smelt_number_format_checked(\"{}\", {}, {argument}).map_err(smelt_number_range_error)",
            method.js_name(),
            value_text.delimited()
        )))
    }

    /// Parses an integer from a string with a JavaScript-style numeric radix.
    ///
    /// Mirrors `Number.parseInt`/`parseInt(str, radix)`: trims leading
    /// whitespace, accepts an optional sign, consumes the longest valid digit
    /// prefix in the radix, and yields `NaN` when no digits parse or the radix
    /// is out of the 2..=36 range. The result is `f64` to match the existing
    /// `parseInt` destination type.
    ///
    /// An `Optional<String>` operand (a `string | undefined` value, such as a
    /// regexp match-array element `m[0]`) follows JavaScript's `ToString`:
    /// `undefined` becomes the string `"undefined"`. That is `NaN` in radix
    /// 2..=30, but `u` is a digit from radix 31 up (`parseInt(undefined, 36)`
    /// is `86464843759093`), so the absent arm runs the same parser over the
    /// literal `"undefined"` instead of being hard-coded to `NaN`.
    pub(super) fn parse_int_radix_expr(
        &self,
        operand: &Operand,
        radix: &Operand,
    ) -> Result<RustExpr, EmitError> {
        let operand_ty = self.mir.types.get(self.operand_ty(operand)?);
        let operand_is_string = matches!(operand_ty, Some(Type::String | Type::PyStr));
        let operand_is_optional_string = matches!(
            operand_ty,
            Some(Type::Optional(inner)) if matches!(self.mir.types.get(*inner), Some(Type::String | Type::PyStr))
        );
        if !(operand_is_string || operand_is_optional_string)
            || !matches!(
                self.mir.types.get(self.operand_ty(radix)?),
                Some(Type::Int | Type::Float)
            )
        {
            return Err(EmitError::new(
                "parseInt(radix) requires a string operand and a numeric radix",
            ));
        }
        let operand_expr = self.operand_expr(operand)?;
        let radix_text = self.operand_expr(radix)?;
        if operand_is_optional_string {
            let present =
                self.parse_int_radix_string_expr(&RustExpr::path("smelt_present"), &radix_text);
            let absent = self.parse_int_radix_string_expr(
                &RustExpr::chain("\"undefined\".to_owned()"),
                &radix_text,
            );
            return Ok(RustExpr::atom(format!(
                "match {operand_expr} {{ Some(smelt_present) => {present}, None => {absent} }}",
                operand_expr = operand_expr.delimited(),
                present = present.delimited(),
                absent = absent.delimited()
            )));
        }
        Ok(self.parse_int_radix_string_expr(&operand_expr, &radix_text))
    }

    /// Render the `parseInt(str, radix)` call of the `smelt_parse_int` runtime
    /// helper over a Rust `String` expression `string_text` and a numeric
    /// `radix_text` (`0` stands for an absent radix).
    ///
    /// Shared by both operand shapes of [`Self::parse_int_radix_expr`] so the
    /// present and absent arms of an optional operand cannot drift apart.
    fn parse_int_radix_string_expr(
        &self,
        string_text: &RustExpr,
        radix_text: &RustExpr,
    ) -> RustExpr {
        let radix_trunc_text = self.numeric_trunc_f64_expr(radix_text);
        RustExpr::chain(format!(
            "smelt_parse_int(&{string_text}, {radix_trunc_text})",
            string_text = string_text.tight(),
            radix_trunc_text = radix_trunc_text.delimited()
        ))
    }

    /// Converts a numeric operand to text usable as an `f64` receiver or argument.
    pub(super) fn float_operand_expr(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        let operand_expr = self.operand_expr(operand)?;
        if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Int)
        ) {
            Ok(RustExpr::atom(format!(
                "({operand_expr} as f64)",
                operand_expr = operand_expr.as_cast_operand()
            )))
        } else {
            Ok(operand_expr)
        }
    }
}
