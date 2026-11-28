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
        self.numeric_abs(operand, span, body)
    }

    /// `abs(operand)` of a lowered `int` or `float`, or of a union of them
    /// (on the member it holds, `union_member_dispatch`).
    fn numeric_abs(&mut self, operand: smelt_hir::ExprId, span: Span, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let operand_ty = Self::expr_ty(body, operand);
        if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Union(_))) {
            return self.union_member_dispatch(operand, span, body, |this, member, inner| this.numeric_abs(member, span, inner));
        }
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
        // compares its items (raising `ValueError` when it is empty), or with
        // `key=` the keys the callback maps them to.
        if let [iterable] = args.as_slice() {
            let list = self.order_free_iterable_as_list(*iterable, span, body);
            if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(Self::expr_ty(body, list)).cloned() {
                let (default_keywords, key_keywords): (Vec<_>, Vec<_>) = call
                    .arguments
                    .keywords
                    .iter()
                    .cloned()
                    .partition(|keyword| keyword.arg.as_ref().map(ruff_python_ast::Identifier::as_str) == Some("default"));
                let key = self.extrema_key_argument(&key_keywords, item_ty, body)?;
                if key.is_none() && !self.is_python_orderable(item_ty) {
                    return Err(SmeltError::unsupported(
                        span,
                        "Python min() and max() of an iterable require bool, int, float or str items",
                    ));
                }
                if let Some(default) = default_keywords.first() {
                    return self.extrema_with_default(op, list, item_ty, key, &default.value, span, body);
                }
                return Ok(body.push_expr(HirExpr {
                    kind: ExprKind::PythonListExtrema { op, list, key },
                    ty: item_ty,
                    span,
                }));
            }
        }
        if let Some(keyword) = call.arguments.keywords.first() {
            return Err(SmeltError::unsupported(
                self.span(keyword.range),
                "Python min() and max() take keyword arguments only with one iterable argument",
            ));
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

    /// `min(iterable, default=d)` / `max(..)`: `d` when the iterable is
    /// empty, else its extremum (`CPython` raises `ValueError` only without a
    /// default). `d` is evaluated once, before the test, as a call argument
    /// is. A default of another type than the items makes the result their
    /// union, as `ty` types it (`max(xs, default=None)` is `int | None`).
    #[expect(clippy::too_many_arguments, reason = "the extremum, its operands and where it lowers")]
    fn extrema_with_default(
        &mut self,
        op: NumericExtremaOp,
        list: smelt_hir::ExprId,
        item_ty: TypeId,
        key: Option<smelt_hir::ExprId>,
        default: &Expr,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let list_ty = Self::expr_ty(body, list);
        let outer = body.push_block(span);
        let list_local = body.push_local(self.synthetic_local("__smelt_extrema_items", list_ty, false, span));
        let list_pat = body.push_pattern(HirPattern::Binding(list_local));
        body.push_stmt_to_block(outer, HirStmt::Let { pat: list_pat, ty: list_ty, value: Some(list) });
        let default_value = self.expression(default, body)?;
        let default_ty = Self::expr_ty(body, default_value);
        let default_local = body.push_local(self.synthetic_local("__smelt_extrema_default", default_ty, false, span));
        let default_pat = body.push_pattern(HirPattern::Binding(default_local));
        body.push_stmt_to_block(outer, HirStmt::Let { pat: default_pat, ty: default_ty, value: Some(default_value) });
        let ty = if default_ty == item_ty {
            item_ty
        } else {
            let joined = self.union_from_types(vec![item_ty, default_ty], span)?;
            if !self.type_assignable_to(item_ty, joined) || !self.type_assignable_to(default_ty, joined) {
                return Err(SmeltError::unsupported(span, "min()/max() default must be the items' type, or a type a union with it holds"));
            }
            joined
        };
        let items = body.push_expr(HirExpr { kind: ExprKind::Local(list_local), ty: list_ty, span });
        let nonempty = self.nonempty_test(items, span, body);
        let items = body.push_expr(HirExpr { kind: ExprKind::Local(list_local), ty: list_ty, span });
        let extremum = body.push_expr(HirExpr { kind: ExprKind::PythonListExtrema { op, list: items, key }, ty: item_ty, span });
        let fallback = body.push_expr(HirExpr { kind: ExprKind::Local(default_local), ty: default_ty, span });
        let chosen = body.push_expr(HirExpr {
            kind: ExprKind::Conditional { cond: nonempty, then_expr: extremum, else_expr: fallback },
            ty,
            span,
        });
        let outer_idx = usize::try_from(outer.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[outer_idx].tail = Some(chosen);
        Ok(body.push_expr(HirExpr { kind: ExprKind::Block(outer), ty, span }))
    }

    /// The `key=` callback of `min(iterable, key=...)` / `max(...)`, lowered
    /// as `sorted`'s key is (a lambda or a function, called with one item);
    /// `key=None` is no key. Its result must be Python-orderable, since the
    /// keys are compared with `<` / `>`.
    ///
    /// # Errors
    ///
    /// Any keyword but `key` (`default=` is not supported yet), or a key whose
    /// result Python cannot order.
    fn extrema_key_argument(
        &mut self,
        keywords: &[ruff_python_ast::Keyword],
        item_ty: TypeId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let mut key = None;
        for keyword in keywords {
            if keyword.arg.as_ref().map(ruff_python_ast::Identifier::as_str) != Some("key") {
                return Err(SmeltError::unsupported(
                    self.span(keyword.range),
                    "Python min() and max() support only the key keyword argument",
                ));
            }
            if matches!(&keyword.value, Expr::NoneLiteral(_)) {
                continue;
            }
            let callback = self.python_callback_argument(&keyword.value, &[item_ty], body)?;
            if !self.is_python_orderable(callback.return_ty) {
                return Err(SmeltError::unsupported(
                    self.span(keyword.range),
                    "a min() or max() key must return bool, int, float, str or a tuple of them",
                ));
            }
            key = Some(callback.expr);
        }
        Ok(key)
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
    /// code point and a one-character `str`; `hex`, `bin`, `oct` and
    /// `format` as `radix_and_format_call` lowers them. `None` for any
    /// other name.
    fn numeric_builtin_call_expression(
        &mut self,
        name: &str,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !matches!(name, "round" | "divmod" | "pow" | "chr" | "ord" | "hex" | "bin" | "oct" | "format") {
            return Ok(None);
        }
        if let Some(formatted) = self.radix_and_format_call(name, call, body, span)? {
            return Ok(Some(formatted));
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
            ("round", [Some(value)] | [Some(value), None]) => self.round_to_int(*value, span, body).map(Some),
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

    /// `round(value)` of a lowered `int` (itself), `float` (half to even, an
    /// `int`) or a union of them (on the member it holds).
    fn round_to_int(&mut self, value: smelt_hir::ExprId, span: Span, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        match self.ctx.krate.types.get(Self::expr_ty(body, value)) {
            Some(Type::PyInt) => Ok(value),
            Some(Type::Float) => Ok(self.py_numeric_call(PyNumericOp::RoundToInt, vec![value], span, body)),
            Some(Type::Union(_)) => self.union_member_dispatch(value, span, body, |this, member, inner| this.round_to_int(member, span, inner)),
            _ => Err(SmeltError::unsupported(span, "round() is not supported for these argument types")),
        }
    }

    /// `hex(n)`, `bin(n)`, `oct(n)` and `format(value[, spec])`, as the
    /// format-spec lowering `f"{value:spec}"` uses: `hex(n)` is
    /// `format(n, "#x")` (`-0xff` for `-255`), `bin` `"#b"`, `oct` `"#o"`,
    /// each taking an `int` only, as `CPython` does without `__index__`.
    /// `format`'s spec must be a string literal, as an f-string's is.
    /// `None` for `round`, `divmod`, `pow`, `chr`, `ord`.
    fn radix_and_format_call(
        &mut self,
        name: &str,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let radix = match name {
            "hex" => "#x",
            "bin" => "#b",
            "oct" => "#o",
            "format" => "",
            _ => return Ok(None),
        };
        let args = call.arguments.args.as_ref();
        let (value_arg, spec) = match (name, args) {
            ("format", [value]) => (value, String::new()),
            ("format", [value, Expr::StringLiteral(spec)]) => (value, spec.value.to_str().to_owned()),
            ("format", _) => {
                return Err(SmeltError::unsupported(span, "format() takes a value and a string-literal spec"));
            }
            (_, [value]) => (value, radix.to_owned()),
            _ => return Err(SmeltError::unsupported(span, format!("{name}() takes exactly one argument"))),
        };
        let value = self.expression(value_arg, body)?;
        if radix.is_empty() && spec.is_empty() {
            // `format(x)` is `str(x)` for the types a spec formats.
            let ty = self.intern_type(Type::PyStr);
            return match self.ctx.krate.types.get(Self::expr_ty(body, value)) {
                Some(Type::PyStr) => Ok(Some(value)),
                Some(Type::Bool | Type::PyInt | Type::Float) => Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::PrimitiveCast { op: PrimitiveCastOp::ToString, operand: value },
                    ty,
                    span,
                }))),
                _ => Err(SmeltError::unsupported(span, "format() is supported for int, float, bool and str values")),
            };
        }
        if !radix.is_empty() && self.ctx.krate.types.get(Self::expr_ty(body, value)) != Some(&Type::PyInt) {
            return Err(SmeltError::unsupported(span, format!("{name}() takes an int")));
        }
        self.formatted_value(value, spec, span, body).map(Some)
    }
}
