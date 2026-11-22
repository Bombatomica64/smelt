// Numeric and arithmetic lowering helpers for `ModuleBuilder`.
// Included into lowering.rs via include!, so this file uses plain comments.
impl ModuleBuilder<'_> {
    /// Return the shared item type for a tuple that must be non-empty and homogeneous.
    fn homogeneous_tuple_item_ty(items: &[TypeId], span: Span) -> Result<TypeId, SmeltError> {
        let Some(first_ty) = items.first().copied() else {
            return Err(SmeltError::unsupported(
                span,
                "tuple conversion requires a non-empty homogeneous tuple",
            ));
        };
        if !items.iter().all(|item| *item == first_ty) {
            return Err(SmeltError::unsupported(
                span,
                "tuple conversion requires homogeneous tuple item types",
            ));
        }
        Ok(first_ty)
    }

    /// Lower direct Python `abs(...)` calls for numeric values.
    fn numeric_abs_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(call.range());
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                span,
                "abs() requires exactly one argument",
            ));
        }
        let [operand_expr] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "abs() requires exactly one argument",
            ));
        };
        let operand = self.expression(operand_expr, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if !matches!(
            self.ctx.krate.types.get(operand_ty),
            Some(Type::PyInt | Type::Float)
        ) {
            return Err(SmeltError::unsupported(
                span,
                "abs() is only supported for int and float values",
            ));
        }
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::NumericAbs { operand },
            ty: operand_ty,
            span,
        }))
    }

    /// Lower direct Python `math.*` numeric calls.
    fn math_module_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let callee = self.qualified_callee(call);
        let Some(member) = callee.as_deref().and_then(|callee| callee.strip_prefix("math.")) else {
            return Ok(None);
        };
        let span = self.span(call.range());
        match member {
            "sqrt" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "log" | "log10"
            | "log2" | "exp" => {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("math.{member}() requires exactly one argument"),
                    ));
                }
                let operand = self.expression(&call.arguments.args[0], body)?;
                if !matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, operand)),
                    Some(Type::PyInt | Type::Float)
                ) {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("math.{member}() requires a numeric argument"),
                    ));
                }
                let op = match member {
                    "sqrt" => NumericUnaryFuncOp::Sqrt,
                    "sin" => NumericUnaryFuncOp::Sin,
                    "cos" => NumericUnaryFuncOp::Cos,
                    "tan" => NumericUnaryFuncOp::Tan,
                    "asin" => NumericUnaryFuncOp::Asin,
                    "acos" => NumericUnaryFuncOp::Acos,
                    "atan" => NumericUnaryFuncOp::Atan,
                    "log" => NumericUnaryFuncOp::Log,
                    "log10" => NumericUnaryFuncOp::Log10,
                    "log2" => NumericUnaryFuncOp::Log2,
                    "exp" => NumericUnaryFuncOp::Exp,
                    _ => return Ok(None),
                };
                let ty = self.intern_type(Type::Float);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::NumericUnaryFunc { op, operand },
                    ty,
                    span,
                })))
            }
            "pow" => {
                if call.arguments.args.len() != 2 {
                    return Err(SmeltError::unsupported(
                        span,
                        "math.pow() requires exactly two arguments",
                    ));
                }
                let base = self.expression(&call.arguments.args[0], body)?;
                let exponent = self.expression(&call.arguments.args[1], body)?;
                if [base, exponent].iter().any(|arg| {
                    !matches!(
                        self.ctx.krate.types.get(Self::expr_ty(body, *arg)),
                        Some(Type::PyInt | Type::Float)
                    )
                }) {
                    return Err(SmeltError::unsupported(
                        span,
                        "math.pow() requires numeric arguments",
                    ));
                }
                let ty = self.intern_type(Type::Float);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::NumericPow { base, exponent },
                    ty,
                    span,
                })))
            }
            "atan2" => {
                if call.arguments.args.len() != 2 {
                    return Err(SmeltError::unsupported(
                        span,
                        "math.atan2() requires exactly two arguments",
                    ));
                }
                let y_coord = self.expression(&call.arguments.args[0], body)?;
                let x_coord = self.expression(&call.arguments.args[1], body)?;
                if [y_coord, x_coord].iter().any(|arg| {
                    !matches!(
                        self.ctx.krate.types.get(Self::expr_ty(body, *arg)),
                        Some(Type::PyInt | Type::Float)
                    )
                }) {
                    return Err(SmeltError::unsupported(
                        span,
                        "math.atan2() requires numeric arguments",
                    ));
                }
                let ty = self.intern_type(Type::Float);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::NumericAtan2 {
                        y: y_coord,
                        x: x_coord,
                    },
                    ty,
                    span,
                })))
            }
            "floor" | "ceil" | "trunc" => {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("math.{member}() requires exactly one argument"),
                    ));
                }
                let operand = self.expression(&call.arguments.args[0], body)?;
                match self.ctx.krate.types.get(Self::expr_ty(body, operand)) {
                    // An `int` is already whole: `math.floor(n)` is `n`.
                    Some(Type::PyInt) => return Ok(Some(operand)),
                    Some(Type::Float) => {}
                    _ => {
                        return Err(SmeltError::unsupported(
                            span,
                            format!("math.{member}() requires a float or int argument"),
                        ));
                    }
                }
                let op = match member {
                    "floor" => NumericRoundOp::Floor,
                    "ceil" => NumericRoundOp::Ceil,
                    "trunc" => NumericRoundOp::Trunc,
                    _ => return Ok(None),
                };
                // The whole `float`, then the `int` of it, exact at every
                // magnitude (`math.floor(1e300)` has 301 digits); `NaN` and the
                // infinities raise as `CPython`'s do.
                let float_ty = self.intern_type(Type::Float);
                let rounded = body.push_expr(HirExpr {
                    kind: ExprKind::NumericRound { op, operand },
                    ty: float_ty,
                    span,
                });
                Ok(Some(self.py_numeric_call(PyNumericOp::FromFloat, vec![rounded], span, body)))
            }
            "isfinite" | "isnan" => {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("math.{member}() requires exactly one argument"),
                    ));
                }
                let operand = self.expression(&call.arguments.args[0], body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::Float) {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("math.{member}() requires a float argument"),
                    ));
                }
                let op = match member {
                    "isfinite" => NumericPredicateOp::IsFinite,
                    "isnan" => NumericPredicateOp::IsNaN,
                    _ => return Ok(None),
                };
                let ty = self.intern_type(Type::Bool);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::NumericPredicate { op, operand },
                    ty,
                    span,
                })))
            }
            _ => Ok(None),
        }
    }

    /// Lower direct Python `math` constants to float literals.
    fn math_constant_expression(
        &mut self,
        attr: &ruff_python_ast::ExprAttribute,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let qualified = self.qualified_attribute(attr)?;
        let value = match qualified.strip_prefix("math.")? {
            "pi" => std::f64::consts::PI,
            "e" => std::f64::consts::E,
            "tau" => std::f64::consts::TAU,
            _ => return None,
        };
        let ty = self.intern_type(Type::Float);
        Some(body.push_expr(HirExpr {
            kind: ExprKind::Literal(Literal::Float(value)),
            ty,
            span: self.span(attr.range),
        }))
    }

    /// Lower direct Python `random.*` numeric calls.
    fn random_module_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(rule) = self.stdlib_call_rule(call) else {
            return Ok(None);
        };
        let span = self.span(call.range());
        match rule {
            RuleId::PyRandomRandom => {
                if !call.arguments.args.is_empty() {
                    return Err(SmeltError::unsupported(
                        span,
                        "random.random() requires no arguments",
                    ));
                }
                let ty = self.intern_type(Type::Float);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::NumericRandom,
                    ty,
                    span,
                })))
            }
            RuleId::PyRandomRandInt => {
                if call.arguments.args.len() != 2 {
                    return Err(SmeltError::unsupported(
                        span,
                        "random.randint() requires exactly two integer arguments",
                    ));
                }
                let start = self.expression(&call.arguments.args[0], body)?;
                let end = self.expression(&call.arguments.args[1], body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, start)) != Some(&Type::PyInt)
                    || self.ctx.krate.types.get(Self::expr_ty(body, end)) != Some(&Type::PyInt)
                {
                    return Err(SmeltError::unsupported(
                        span,
                        "random.randint() requires integer bounds",
                    ));
                }
                let low = self.py_int_as_size(start, body);
                let high = self.py_int_as_size(end, body);
                Ok(Some(self.machine_int_as_py_int(ExprKind::NumericRandomInt { start: low, end: high }, span, body)))
            }
            RuleId::PyRandomChoice => {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "random.choice() requires exactly one list argument",
                    ));
                }
                let list = self.expression(&call.arguments.args[0], body)?;
                let list_ty = Self::expr_ty(body, list);
                let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) else {
                    return Err(SmeltError::unsupported(
                        span,
                        "random.choice() currently requires a list argument",
                    ));
                };
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::ListRandomChoice { list },
                    ty: *item_ty,
                    span,
                })))
            }
            _ => Ok(None),
        }
    }

    /// Lower direct Python `min(...)` and `max(...)` calls.
    fn numeric_extrema_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(call.range());
        let Expr::Name(name) = call.func.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "numeric extrema requires a name call",
            ));
        };
        let op = match name.id.as_str() {
            "max" => NumericExtremaOp::Max,
            "min" => NumericExtremaOp::Min,
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "numeric extrema requires min or max",
                ));
            }
        };
        if call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "Python min() and max() require at least one argument",
            ));
        }
        let args = call
            .arguments
            .args
            .iter()
            .map(|arg| self.expression(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        // `min(iterable)` / `max(iterable)`: one positional iterable argument
        // compares its items (raising `ValueError` when it is empty).
        if let [iterable] = args.as_slice()
            && call.arguments.keywords.is_empty()
        {
            let list = self.order_free_iterable_as_list(*iterable, span, body);
            if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(Self::expr_ty(body, list)).cloned() {
                if !self.is_python_orderable(item_ty) {
                    return Err(SmeltError::unsupported(
                        span,
                        "Python min() and max() of an iterable require bool, int, float or str items",
                    ));
                }
                return Ok(body.push_expr(HirExpr {
                    kind: ExprKind::PythonListExtrema { op, list },
                    ty: item_ty,
                    span,
                }));
            }
        }
        let int_ty = self.intern_type(Type::PyInt);
        let float_ty = self.intern_type(Type::Float);
        let arg_tys: Vec<TypeId> = args.iter().map(|arg| Self::expr_ty(body, *arg)).collect();
        if arg_tys.iter().any(|ty| *ty != int_ty && *ty != float_ty) {
            return Err(SmeltError::unsupported(
                span,
                "Python min() and max() of several arguments require int or float arguments",
            ));
        }
        // `min`/`max` return one of their arguments unchanged, so an `int`
        // argument stays an `int` even next to a `float` (`max(3, 2.5)` is
        // `3`): mixed arguments give `int | float`, whose members the emitter
        // compares by exact value.
        let ty = if arg_tys.iter().all(|ty| *ty == arg_tys[0]) {
            arg_tys[0]
        } else {
            self.union_from_types(vec![int_ty, float_ty], span)?
        };
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::NumericExtrema {
                op,
                args,
                spread: None,
            },
            ty,
            span,
        }))
    }

    /// Lower direct Python string case methods.
    fn string_case_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let op = match attr.attr.as_str() {
            "lower" => StringCaseOp::Lower,
            "upper" => StringCaseOp::Upper,
            _ => return Ok(None),
        };
        let span = self.span(call.range());
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "string case methods do not take arguments",
            ));
        }
        let operand = self.expression(&attr.value, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::PyStr) {
            return Err(SmeltError::unsupported(
                span,
                "string case methods require a str receiver",
            ));
        }
        let ty = self.intern_type(Type::PyStr);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringCase { op, operand },
            ty,
            span,
        })))
    }

    // String formatting helpers continue in `format.rs`.

    /// Lower `round`, `divmod`, `pow`, `chr` and `ord` with `CPython`'s
    /// semantics: `round(x)` rounds half to even into an `int`, `round(x, n)`
    /// is correctly rounded at `n` digits (an `int` stays an `int`),
    /// `divmod(a, b)` is `(a // b, a % b)`, `pow(a, b)` is `a ** b` and
    /// `pow(a, b, m)` modular exponentiation, `chr`/`ord` convert between a
    /// code point and a one-character `str`. `None` for any other name.
    fn numeric_builtin_call_expression(
        &mut self,
        name: &str,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !matches!(name, "round" | "divmod" | "pow" | "chr" | "ord") {
            return Ok(None);
        }
        if !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(span, format!("{name}() keyword arguments are not supported")));
        }
        let args = call
            .arguments
            .args
            .iter()
            .map(|arg| if matches!(arg, Expr::NoneLiteral(_)) { Ok(None) } else { self.expression(arg, body).map(Some) })
            .collect::<Result<Vec<_>, _>>()?;
        let kind = |this: &Self, value: smelt_hir::ExprId, within: &Body| this.ctx.krate.types.get(Self::expr_ty(within, value)).cloned();
        let wrong = || SmeltError::unsupported(span, format!("{name}() is not supported for these argument types"));
        match (name, args.as_slice()) {
            ("round", [Some(value)] | [Some(value), None]) => match kind(self, *value, body) {
                Some(Type::PyInt) => Ok(Some(*value)),
                Some(Type::Float) => Ok(Some(self.py_numeric_call(PyNumericOp::RoundToInt, vec![*value], span, body))),
                _ => Err(wrong()),
            },
            ("round", [Some(value), Some(ndigits)]) => {
                if kind(self, *ndigits, body) != Some(Type::PyInt) {
                    return Err(wrong());
                }
                let digits = self.py_int_as_clamped_bound(*ndigits, body);
                match kind(self, *value, body) {
                    Some(Type::PyInt) => Ok(Some(self.py_numeric_call(PyNumericOp::RoundInt, vec![*value, digits], span, body))),
                    Some(Type::Float) => Ok(Some(self.py_numeric_call(PyNumericOp::RoundFloat, vec![*value, digits], span, body))),
                    _ => Err(wrong()),
                }
            }
            ("divmod", [Some(lhs), Some(rhs)]) => {
                let quotient = self.python_arithmetic(PythonArithmetic::Division(PythonDivisionOp::FloorDiv), *lhs, *rhs, span, body)?;
                let remainder = self.python_arithmetic(PythonArithmetic::Division(PythonDivisionOp::Mod), *lhs, *rhs, span, body)?;
                let ty = self.intern_type(Type::Tuple(vec![Self::expr_ty(body, quotient), Self::expr_ty(body, remainder)]));
                Ok(Some(body.push_expr(HirExpr { kind: ExprKind::TupleLit(vec![quotient, remainder]), ty, span })))
            }
            ("pow", [Some(base), Some(exponent)] | [Some(base), Some(exponent), None]) => {
                self.python_arithmetic(PythonArithmetic::Power, *base, *exponent, span, body).map(Some)
            }
            ("pow", [Some(base), Some(exponent), Some(modulus)]) => {
                if [*base, *exponent, *modulus].iter().any(|value| kind(self, *value, body) != Some(Type::PyInt)) {
                    return Err(SmeltError::unsupported(span, "pow() with a modulus needs three ints"));
                }
                Ok(Some(self.py_numeric_call(PyNumericOp::PowMod, vec![*base, *exponent, *modulus], span, body)))
            }
            ("chr", [Some(code)]) if kind(self, *code, body) == Some(Type::PyInt) => {
                let point = self.py_int_as_clamped_bound(*code, body);
                Ok(Some(self.py_numeric_call(PyNumericOp::Chr, vec![point], span, body)))
            }
            ("ord", [Some(text)]) if kind(self, *text, body) == Some(Type::PyStr) => {
                let ord = ExprKind::PyNumericCall { op: PyNumericOp::Ord, args: vec![*text] };
                Ok(Some(self.machine_int_as_py_int(ord, span, body)))
            }
            _ => Err(wrong()),
        }
    }
}
