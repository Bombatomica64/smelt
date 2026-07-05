impl ModuleBuilder<'_> {
    // -----------------------------------------------------------------------
    // Expression lowering
    // -----------------------------------------------------------------------

    /// Lower an expression without a type hint.
    fn expression(
        &mut self,
        expr: &Expr,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.expression_with_hint(expr, body, None)
    }

    /// Lower an expression with an optional expected type hint.
    fn expression_with_hint(
        &mut self,
        expr: &Expr,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match expr {
            // --- Literals ---
            Expr::NumberLiteral(n) => {
                let (kind, ty) = match &n.value {
                    Number::Int(i) => {
                        let v = i.as_i64().ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(n.range),
                                "integer literal out of i64 range",
                            )
                        })?;
                        (
                            ExprKind::Literal(Literal::Int(v)),
                            self.intern_type(Type::Int),
                        )
                    }
                    Number::Float(f) => (
                        ExprKind::Literal(Literal::Float(*f)),
                        self.intern_type(Type::Float),
                    ),
                    Number::Complex { .. } => {
                        return Err(SmeltError::unsupported(
                            self.span(n.range),
                            "complex number literals are not supported",
                        ));
                    }
                };
                Ok(body.push_expr(HirExpr {
                    kind,
                    ty,
                    span: self.span(n.range),
                }))
            }

            Expr::StringLiteral(s) => {
                let ty = self.intern_type(Type::String);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::String(s.value.to_str().to_owned())),
                    ty,
                    span: self.span(s.range),
                }))
            }

            Expr::BooleanLiteral(b) => {
                let ty = self.intern_type(Type::Bool);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::Bool(b.value)),
                    ty,
                    span: self.span(b.range),
                }))
            }

            Expr::NoneLiteral(n) => {
                let ty = self.intern_type(Type::None);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(n.range),
                }))
            }

            // --- Name lookup ---
            Expr::Name(name) => self.identifier_expression(name.id.as_str(), name.range, body),

            // --- Binary / boolean / comparison operators ---
            Expr::BinOp(b) => self.binop_expression(b, body),
            Expr::BoolOp(b) => self.boolop_expression(b, body, type_hint),
            Expr::Compare(c) => self.compare_expression(c, body),

            // --- Unary operators ---
            Expr::UnaryOp(u) => self.unary_expression(u, body),

            // --- Conditional expression: `a if cond else b` ---
            Expr::If(if_expr) => self.if_expression(if_expr, body),

            // --- Calls ---
            Expr::Call(call) => self.call_expression_with_hint(call, body, type_hint),

            // --- Await ---
            Expr::Await(await_expr) => {
                if !self.current_async {
                    return Err(SmeltError::unsupported(
                        self.span(await_expr.range),
                        "await expressions are only supported inside async functions",
                    ));
                }
                let awaited = self.expression(&await_expr.value, body)?;
                let awaited_ty = Self::expr_ty(body, awaited);
                let ty = self.future_inner_type(awaited_ty).ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(await_expr.range),
                        "await expressions require an Awaitable[T] operand",
                    )
                })?;
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Await(awaited),
                    ty,
                    span: self.span(await_expr.range),
                }))
            }

            // --- Attribute access: `obj.field` ---
            Expr::Attribute(attr) => {
                if let Some(constant) = self.math_constant_expression(attr, body) {
                    return Ok(constant);
                }
                if let Some(member_expr) = self.enum_member_expression(attr, body)? {
                    return Ok(member_expr);
                }
                if let Some(member_expr) = self.class_static_member_expression(attr, body)? {
                    return Ok(member_expr);
                }
                if let Some(member_expr) = self.module_member_expression(attr, body)? {
                    return Ok(member_expr);
                }
                if let Some(url_expr) = self.urlparse_attribute_expression(attr, body)? {
                    return Ok(url_expr);
                }
                let receiver = self.expression(&attr.value, body)?;
                let receiver_ty = Self::expr_ty(body, receiver);
                let field = self.intern_name(attr.attr.as_str());
                let field_ty = self.field_type(receiver_ty, field)?;
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Field { receiver, field },
                    ty: field_ty,
                    span: self.span(attr.range),
                }))
            }

            // --- Subscript: `obj[index]` ---
            Expr::Subscript(sub) => {
                if let Some(slice) = self.slice_subscript(sub, body)? {
                    return Ok(slice);
                }
                let receiver = self.expression(&sub.value, body)?;
                let receiver_ty = Self::expr_ty(body, receiver);
                if let Some((index, ty)) =
                    self.tuple_index_subscript(receiver_ty, &sub.slice, self.span(sub.range))?
                {
                    return Ok(body.push_expr(HirExpr {
                        kind: ExprKind::TupleIndex {
                            tuple: receiver,
                            index,
                        },
                        ty,
                        span: self.span(sub.range),
                    }));
                }
                let index_ty = self.index_type(receiver_ty)?;
                let index = self.expression(&sub.slice, body)?;
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Index { receiver, index },
                    ty: index_ty,
                    span: self.span(sub.range),
                }))
            }

            // --- Collection literals ---
            Expr::List(l) => {
                let elts: Vec<_> = l
                    .elts
                    .iter()
                    .map(|e| self.expression(e, body))
                    .collect::<Result<_, _>>()?;
                // Infer element type from hint or first element.
                let ty = type_hint.unwrap_or_else(|| {
                    if let Some(first_id) = elts.first().copied() {
                        let elem_ty = Self::expr_ty(body, first_id);
                        self.intern_type(Type::List(elem_ty))
                    } else {
                        let none = self.intern_type(Type::None);
                        self.intern_type(Type::List(none))
                    }
                });
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::ListLit(elts),
                    ty,
                    span: self.span(l.range),
                }))
            }

            Expr::Tuple(t) => {
                let elts: Vec<_> = t
                    .elts
                    .iter()
                    .map(|e| self.expression(e, body))
                    .collect::<Result<_, _>>()?;
                let elem_types: Vec<TypeId> =
                    elts.iter().map(|&id| Self::expr_ty(body, id)).collect();
                let ty = self.intern_type(Type::Tuple(elem_types));
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::TupleLit(elts),
                    ty,
                    span: self.span(t.range),
                }))
            }

            Expr::Set(s) => {
                let elts: Vec<_> = s
                    .elts
                    .iter()
                    .map(|e| self.expression(e, body))
                    .collect::<Result<_, _>>()?;
                let ty = type_hint.unwrap_or_else(|| {
                    if let Some(first_id) = elts.first().copied() {
                        let elem_ty = Self::expr_ty(body, first_id);
                        self.intern_type(Type::Set(elem_ty))
                    } else {
                        let none = self.intern_type(Type::None);
                        self.intern_type(Type::Set(none))
                    }
                });
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::SetLit(elts),
                    ty,
                    span: self.span(s.range),
                }))
            }

            Expr::Dict(d) => {
                let mut entries: Vec<(smelt_hir::ExprId, smelt_hir::ExprId)> = Vec::new();
                for item in &d.items {
                    let key_expr = item.key.as_ref().ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(d.range),
                            "dictionary unpacking (**dict) is not supported",
                        )
                    })?;
                    let key = self.expression(key_expr, body)?;
                    let val = self.expression(&item.value, body)?;
                    entries.push((key, val));
                }
                let ty = type_hint.unwrap_or_else(|| {
                    if let Some((k_id, v_id)) = entries.first().copied() {
                        let k_ty = Self::expr_ty(body, k_id);
                        let v_ty = Self::expr_ty(body, v_id);
                        self.intern_type(Type::Dict(k_ty, v_ty))
                    } else {
                        let none = self.intern_type(Type::None);
                        self.intern_type(Type::Dict(none, none))
                    }
                });
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::DictLit(entries),
                    ty,
                    span: self.span(d.range),
                }))
            }

            Expr::FString(f) => self.fstring_expression(f, body),

            Expr::ListComp(c) => self.list_comprehension(c, body),
            Expr::SetComp(c) => self.set_comprehension(c, body),
            Expr::DictComp(c) => self.dict_comprehension(c, body),
            Expr::Generator(c) => self.generator_expression(c, body),

            // `lambda a, b: expr` — a first-class closure value. Parameter types
            // come from an expected `Callable`/function `type_hint`; without one
            // the parameters must be individually annotated.
            Expr::Lambda(lambda) => self.lambda_expression(lambda, body, type_hint),

            Expr::Named(_)
            | Expr::Yield(_)
            | Expr::YieldFrom(_)
            | Expr::TString(_)
            | Expr::BytesLiteral(_)
            | Expr::EllipsisLiteral(_)
            | Expr::Starred(_)
            | Expr::Slice(_)
            | Expr::IpyEscapeCommand(_) => Err(SmeltError::unsupported(
                self.span(expr.range()),
                format!("unsupported expression: {}", expr_kind_name(expr)),
            )),
        }
    }

    /// Lower a Python `lambda` expression as a first-class closure value.
    ///
    /// A lambda is lowered through the same compact callback IR the frontend
    /// already uses for `map`/`filter`/`sorted(key=...)` lambdas: the body is
    /// classified into a [`CallbackExpr`] tree by [`Self::lambda_callback`], then
    /// materialized into a real [`ExprKind::Closure`] CFG body by
    /// [`Self::callback_expr_to_closure`]. The resulting closure value can be
    /// stored in a local, passed to a call argument, or returned.
    ///
    /// Python lambda parameters never carry type annotations, so their types can
    /// only come from an expected `Callable[...]`/function `type_hint` supplied
    /// by the surrounding context (an annotated assignment, an annotated call
    /// parameter, a typed return). When no function-typed hint is available the
    /// lambda's parameter types are unknowable and the construct is rejected with
    /// a specific message rather than routed through an erased ABI — keeping the
    /// static-shape-first policy. (Bare `x = lambda ...` without an annotation is
    /// the documented deferral; annotate the target with `Callable[...]`.)
    fn lambda_expression(
        &mut self,
        lambda: &ruff_python_ast::ExprLambda,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(hint) = type_hint else {
            return Err(SmeltError::unsupported(
                self.span(lambda.range),
                "lambda expressions need an expected `Callable[...]` type from their context \
                 (annotate the assignment target or call parameter)",
            ));
        };
        let Some(Type::Function(function)) = self.ctx.krate.types.get(hint).cloned() else {
            return Err(SmeltError::unsupported(
                self.span(lambda.range),
                "lambda expressions require a `Callable[...]`/function-typed context",
            ));
        };

        let callback = self.lambda_callback(lambda, &function.params, body)?;
        if callback.ty != function.return_ty {
            return Err(SmeltError::unsupported(
                self.span(lambda.range),
                "lambda return type does not match its expected callable annotation",
            ));
        }
        self.callback_expr_to_closure(&callback, &function.params, self.span(lambda.range), body)
    }

    /// Lower a Python f-string (`f"a{expr}b"`) as runtime string concatenation.
    ///
    /// Mirrors the TypeScript template-literal lowering: each literal chunk and
    /// interpolated expression is folded together with `BinOp::Add` on a `String`
    /// result type. The Rust emitter coerces non-string operands of a string
    /// addition via `to_string()`, which matches the common `f"{value}"` case
    /// where `value` formats through `str(...)`.
    ///
    /// Format specifications (`{x:.2f}`), the `repr`/`ascii` conversions
    /// (`{x!r}`, `{x!a}`) and self-documenting expressions (`{x=}`) are not yet
    /// modeled, so they are rejected as unsupported rather than silently
    /// dropped. Implicitly concatenated literal parts (`"a" f"b{x}"`) are
    /// preserved by walking the f-string parts directly instead of the
    /// `elements()` helper, which skips plain string literal parts.
    fn fstring_expression(
        &mut self,
        fstring: &ruff_python_ast::ExprFString,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // A segment is either a static literal chunk or a lowered interpolation.
        enum Segment {
            Literal(String),
            Expr(smelt_hir::ExprId),
        }

        let span = self.span(fstring.range);
        let str_ty = self.intern_type(Type::String);
        let mut segments: Vec<Segment> = Vec::new();

        for part in fstring.value.iter() {
            match part {
                ruff_python_ast::FStringPart::Literal(literal) => {
                    segments.push(Segment::Literal(literal.as_str().to_owned()));
                }
                ruff_python_ast::FStringPart::FString(inner) => {
                    for element in &inner.elements {
                        match element {
                            ruff_python_ast::InterpolatedStringElement::Literal(literal) => {
                                segments.push(Segment::Literal(literal.value.to_string()));
                            }
                            ruff_python_ast::InterpolatedStringElement::Interpolation(interp) => {
                                if interp.debug_text.is_some() {
                                    return Err(SmeltError::unsupported(
                                        self.span(interp.range),
                                        "f-string self-documenting expressions (trailing `=`) are not supported",
                                    ));
                                }
                                if interp.format_spec.is_some() {
                                    return Err(SmeltError::unsupported(
                                        self.span(interp.range),
                                        "f-string format specifications (a `:` conversion suffix) are not supported",
                                    ));
                                }
                                if !matches!(
                                    interp.conversion,
                                    ruff_python_ast::ConversionFlag::None
                                        | ruff_python_ast::ConversionFlag::Str
                                ) {
                                    return Err(SmeltError::unsupported(
                                        self.span(interp.range),
                                        "f-string `!r`/`!a` conversions are not supported",
                                    ));
                                }
                                let expr = self.expression(&interp.expression, body)?;
                                segments.push(Segment::Expr(expr));
                            }
                        }
                    }
                }
            }
        }

        // Fold segments into a chain of `String` additions, using the first
        // segment as the accumulator base so a literal-only f-string lowers to a
        // single string literal.
        let mut iter = segments.into_iter();
        let mut acc = match iter.next() {
            Some(Segment::Literal(text)) => body.push_expr(HirExpr {
                kind: ExprKind::Literal(Literal::String(text)),
                ty: str_ty,
                span,
            }),
            Some(Segment::Expr(expr)) => {
                // Anchor on an empty string literal so the first interpolation is
                // emitted through the string-addition path (which coerces the
                // operand to `String`).
                let empty = body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::String(String::new())),
                    ty: str_ty,
                    span,
                });
                body.push_expr(HirExpr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Add,
                        lhs: empty,
                        rhs: expr,
                    },
                    ty: str_ty,
                    span,
                })
            }
            None => body.push_expr(HirExpr {
                kind: ExprKind::Literal(Literal::String(String::new())),
                ty: str_ty,
                span,
            }),
        };

        for segment in iter {
            let rhs = match segment {
                Segment::Literal(text) => {
                    if text.is_empty() {
                        continue;
                    }
                    body.push_expr(HirExpr {
                        kind: ExprKind::Literal(Literal::String(text)),
                        ty: str_ty,
                        span,
                    })
                }
                Segment::Expr(expr) => expr,
            };
            acc = body.push_expr(HirExpr {
                kind: ExprKind::BinOp {
                    op: BinOp::Add,
                    lhs: acc,
                    rhs,
                },
                ty: str_ty,
                span,
            });
        }

        Ok(acc)
    }

    /// Lower a Python conditional expression (`then if cond else else_`).
    fn if_expression(
        &mut self,
        if_expr: &ruff_python_ast::ExprIf,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let cond = self.expression(&if_expr.test, body)?;
        let then_expr = self.expression(&if_expr.body, body)?;
        let else_expr = self.expression(&if_expr.orelse, body)?;
        let ty = Self::expr_ty(body, then_expr);
        if Self::expr_ty(body, else_expr) != ty {
            return Err(SmeltError::unsupported(
                self.span(if_expr.range),
                "conditional expression branches must have the same type",
            ));
        }
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            },
            ty,
            span: self.span(if_expr.range),
        }))
    }

    /// Lower a binary arithmetic/comparison operator expression.
    fn binop_expression(
        &mut self,
        b: &ruff_python_ast::ExprBinOp,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(b.range);
        let (op, result_is_bool) = match b.op {
            Operator::Add => (BinOp::Add, false),
            Operator::Sub => (BinOp::Sub, false),
            Operator::Mult => (BinOp::Mul, false),
            Operator::Div => (BinOp::Div, false),
            Operator::MatMult
            | Operator::Mod
            | Operator::Pow
            | Operator::LShift
            | Operator::RShift
            | Operator::BitOr
            | Operator::BitXor
            | Operator::BitAnd
            | Operator::FloorDiv => {
                return Err(SmeltError::unsupported(
                    span,
                    format!("binary operator '{}' is not supported", b.op),
                ));
            }
        };
        let lhs = self.expression(&b.left, body)?;
        let rhs = self.expression(&b.right, body)?;
        let ty = if result_is_bool {
            self.intern_type(Type::Bool)
        } else {
            Self::expr_ty(body, lhs)
        };
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty,
            span,
        }))
    }

    /// Lower a boolean operator or Python's value-returning `or` fallback.
    fn boolop_expression(
        &mut self,
        b: &ruff_python_ast::ExprBoolOp,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let op = match b.op {
            BoolOp::And => BinOp::And,
            BoolOp::Or => BinOp::Or,
        };
        let bool_ty = self.intern_type(Type::Bool);

        // Left-fold: ((a op b) op c)
        let values: &[Expr] = b.values.as_ref();
        let Some((first, rest)) = values.split_first() else {
            return Err(SmeltError::unsupported(
                self.span(b.range),
                "boolean operations require at least one value",
            ));
        };
        let mut acc = self.expression(first, body)?;
        for value in rest {
            let rhs = self.expression(value, body)?;
            let span = Span::new(
                self.file_id,
                Self::expr_span(body, acc).start,
                Self::expr_span(body, rhs).end,
            );
            if b.op == BoolOp::Or {
                if let Some(value_or) =
                    self.value_or_expression((acc, rhs), span, body, type_hint)?
                {
                    acc = value_or;
                    continue;
                }
            }
            acc = body.push_expr(HirExpr {
                kind: ExprKind::BinOp { op, lhs: acc, rhs },
                ty: bool_ty,
                span,
            });
        }
        Ok(acc)
    }

    /// Lower `lhs or rhs` when Python returns one of the operands instead of a bool.
    fn value_or_expression(
        &mut self,
        operands: (smelt_hir::ExprId, smelt_hir::ExprId),
        span: Span,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let (lhs, rhs) = operands;
        let lhs_ty = Self::expr_ty(body, lhs);
        let rhs_ty = Self::expr_ty(body, rhs);
        if self.ctx.krate.types.get(lhs_ty) == Some(&Type::Bool)
            && self.ctx.krate.types.get(rhs_ty) == Some(&Type::Bool)
        {
            return Ok(None);
        }
        let Some(ty) = self.value_or_result_type(lhs_ty, rhs_ty, type_hint) else {
            return Ok(None);
        };
        let cond = self.truthiness_expr(lhs, span, body)?;
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::Conditional {
                cond,
                then_expr: lhs,
                else_expr: rhs,
            },
            ty,
            span,
        })))
    }

    /// Resolve the HIR result type for value-returning Python `or`.
    fn value_or_result_type(
        &mut self,
        lhs_ty: TypeId,
        rhs_ty: TypeId,
        type_hint: Option<TypeId>,
    ) -> Option<TypeId> {
        if lhs_ty == rhs_ty {
            return Some(lhs_ty);
        }
        if let Some(hint) = type_hint
            && let Some(Type::Optional(inner)) = self.ctx.krate.types.get(hint)
            && ((*inner == lhs_ty && self.ctx.krate.types.get(rhs_ty) == Some(&Type::None))
                || (*inner == rhs_ty && self.ctx.krate.types.get(lhs_ty) == Some(&Type::None)))
        {
            return Some(hint);
        }
        if self.ctx.krate.types.get(rhs_ty) == Some(&Type::None) {
            return Some(self.intern_type(Type::Optional(lhs_ty)));
        }
        if self.ctx.krate.types.get(lhs_ty) == Some(&Type::None) {
            return Some(self.intern_type(Type::Optional(rhs_ty)));
        }
        None
    }

    /// Build a boolean condition for Python truthiness in supported fallback expressions.
    fn truthiness_expr(
        &mut self,
        operand: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) == Some(&Type::Bool) {
            return Ok(operand);
        }
        if !matches!(
            self.ctx.krate.types.get(operand_ty),
            Some(Type::Int | Type::Float | Type::String)
        ) {
            return Err(SmeltError::unsupported(
                span,
                "value-returning `or` currently supports bool, int, float, and str left operands",
            ));
        }
        let ty = self.intern_type(Type::Bool);
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::PrimitiveCast {
                op: PrimitiveCastOp::ToBool,
                operand,
            },
            ty,
            span,
        }))
    }

    /// Lower a comparison expression.  Only single-op, non-chained comparisons.
    fn compare_expression(
        &mut self,
        c: &ruff_python_ast::ExprCompare,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(c.range);
        if c.ops.len() != 1 || c.comparators.len() != 1 {
            return Err(SmeltError::unsupported(
                span,
                "chained comparisons (e.g. a < b < c) are not supported",
            ));
        }
        let [op] = c.ops.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "chained comparisons (e.g. a < b < c) are not supported",
            ));
        };
        let bin_op = match op {
            CmpOp::Eq => BinOp::Eq,
            CmpOp::NotEq => BinOp::NotEq,
            CmpOp::Lt => BinOp::Lt,
            CmpOp::LtE => BinOp::Lte,
            CmpOp::Gt => BinOp::Gt,
            CmpOp::GtE => BinOp::Gte,
            CmpOp::In | CmpOp::NotIn => return self.contains_compare(c, body, *op),
            CmpOp::Is => BinOp::Eq,
            CmpOp::IsNot => BinOp::NotEq,
        };
        let lhs = self.expression(&c.left, body)?;
        let [rhs_expr] = c.comparators.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "chained comparisons (e.g. a < b < c) are not supported",
            ));
        };
        let rhs = self.expression(rhs_expr, body)?;
        let ty = self.intern_type(Type::Bool);
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::BinOp {
                op: bin_op,
                lhs,
                rhs,
            },
            ty,
            span,
        }))
    }

    /// Lower Python containment comparisons for strings and lists.
    fn contains_compare(
        &mut self,
        c: &ruff_python_ast::ExprCompare,
        body: &mut Body,
        op: CmpOp,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(c.range);
        let needle = self.expression(&c.left, body)?;
        let [haystack_expr] = c.comparators.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "containment requires a single comparison target",
            ));
        };
        let haystack = self.expression(haystack_expr, body)?;
        let needle_ty = Self::expr_ty(body, needle);
        let haystack_ty = Self::expr_ty(body, haystack);
        let bool_ty = self.intern_type(Type::Bool);
        let contains_kind = match self.ctx.krate.types.get(haystack_ty) {
            Some(Type::String) if self.ctx.krate.types.get(needle_ty) == Some(&Type::String) => {
                // Python's `in` has no start position; `from_index` is a
                // JavaScript `includes(needle, position)` affordance.
                ExprKind::StringContains {
                    haystack,
                    needle,
                    from_index: None,
                }
            }
            Some(Type::List(item_ty)) if needle_ty == *item_ty => ExprKind::ListContains {
                list: haystack,
                item: needle,
            },
            Some(Type::Set(item_ty)) if needle_ty == *item_ty => ExprKind::SetContains {
                set: haystack,
                item: needle,
            },
            Some(Type::Tuple(items)) if items.iter().any(|item_ty| *item_ty == needle_ty) => {
                ExprKind::TupleContains {
                    tuple: haystack,
                    item: needle,
                }
            }
            Some(Type::Dict(key_ty, _)) if needle_ty == *key_ty => ExprKind::DictContainsKey {
                dict: haystack,
                key: needle,
            },
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "containment requires str operands or an item matching the collection element/key type",
                ));
            }
        };
        let contains = body.push_expr(HirExpr {
            kind: contains_kind,
            ty: bool_ty,
            span,
        });
        if op == CmpOp::NotIn {
            Ok(body.push_expr(HirExpr {
                kind: ExprKind::UnaryOp {
                    op: UnaryOp::Not,
                    operand: contains,
                },
                ty: bool_ty,
                span,
            }))
        } else {
            Ok(contains)
        }
    }

    /// Lower a unary expression.
    fn unary_expression(
        &mut self,
        u: &ruff_python_ast::ExprUnaryOp,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(u.range);
        let (op, result_is_bool) = match u.op {
            RuffUnaryOp::Not => (UnaryOp::Not, true),
            RuffUnaryOp::USub => (UnaryOp::Neg, false),
            RuffUnaryOp::Invert | RuffUnaryOp::UAdd => {
                return Err(SmeltError::unsupported(
                    span,
                    format!("unary operator '{}' is not supported", u.op),
                ));
            }
        };
        let operand = self.expression(&u.operand, body)?;
        let ty = if result_is_bool {
            self.intern_type(Type::Bool)
        } else {
            Self::expr_ty(body, operand)
        };
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::UnaryOp { op, operand },
            ty,
            span,
        }))
    }

    // Call lowering continues in `call.rs`.
}
