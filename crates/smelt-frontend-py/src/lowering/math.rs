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
        let span = self.span(call.range);
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
            Some(Type::Int | Type::Float)
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
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "math" {
            return Ok(None);
        }
        let span = self.span(call.range);
        match attr.attr.as_str() {
            "sqrt" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "log" | "log10"
            | "log2" | "exp" => {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("math.{}() requires exactly one argument", attr.attr),
                    ));
                }
                let operand = self.expression(&call.arguments.args[0], body)?;
                if !matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, operand)),
                    Some(Type::Int | Type::Float)
                ) {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("math.{}() requires a numeric argument", attr.attr),
                    ));
                }
                let op = match attr.attr.as_str() {
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
                        Some(Type::Int | Type::Float)
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
                        Some(Type::Int | Type::Float)
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
                        format!("math.{}() requires exactly one argument", attr.attr),
                    ));
                }
                let operand = self.expression(&call.arguments.args[0], body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::Float) {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("math.{}() requires a float argument", attr.attr),
                    ));
                }
                let op = match attr.attr.as_str() {
                    "floor" => NumericRoundOp::Floor,
                    "ceil" => NumericRoundOp::Ceil,
                    "trunc" => NumericRoundOp::Trunc,
                    _ => return Ok(None),
                };
                let ty = self.intern_type(Type::Int);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::NumericRound { op, operand },
                    ty,
                    span,
                })))
            }
            "isfinite" | "isnan" => {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("math.{}() requires exactly one argument", attr.attr),
                    ));
                }
                let operand = self.expression(&call.arguments.args[0], body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::Float) {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("math.{}() requires a float argument", attr.attr),
                    ));
                }
                let op = match attr.attr.as_str() {
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
        let Expr::Name(module) = attr.value.as_ref() else {
            return None;
        };
        if module.id.as_str() != "math" {
            return None;
        }
        let value = match attr.attr.as_str() {
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
        let Some(rule) = stdlib_dispatch::call_rule(call) else {
            return Ok(None);
        };
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "random" {
            return Ok(None);
        }
        let span = self.span(call.range);
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
                if self.ctx.krate.types.get(Self::expr_ty(body, start)) != Some(&Type::Int)
                    || self.ctx.krate.types.get(Self::expr_ty(body, end)) != Some(&Type::Int)
                {
                    return Err(SmeltError::unsupported(
                        span,
                        "random.randint() requires integer bounds",
                    ));
                }
                let ty = self.intern_type(Type::Int);
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::NumericRandomInt { start, end },
                    ty,
                    span,
                })))
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
        let span = self.span(call.range);
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
        let ty = Self::expr_ty(body, args[0]);
        if !matches!(self.ctx.krate.types.get(ty), Some(Type::Int | Type::Float))
            || args.iter().any(|arg| Self::expr_ty(body, *arg) != ty)
        {
            return Err(SmeltError::unsupported(
                span,
                "Python min() and max() require all-int or all-float arguments",
            ));
        }
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
        let span = self.span(call.range);
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "string case methods do not take arguments",
            ));
        }
        let operand = self.expression(&attr.value, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                span,
                "string case methods require a str receiver",
            ));
        }
        let ty = self.intern_type(Type::String);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringCase { op, operand },
            ty,
            span,
        })))
    }

    // String formatting helpers continue in `format.rs`.
}
