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

    /// Lower an expression with an optional expected type hint, in Python's
    /// view: a value carrying TypeScript text (it comes from a TypeScript
    /// module) is converted to Python's ([`Self::in_own_language_view`]).
    fn expression_with_hint(
        &mut self,
        expr: &Expr,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let lowered = self
            .expression_shape_with_hint(expr, body, type_hint)
            .map_err(|error| error.at_if_unplaced(self.span(expr.range())))?;
        Ok(self.in_own_language_view(lowered, body))
    }

    /// A value whose type carries TypeScript text (a call result, a field or
    /// a constant of a TypeScript module) as Python sees it: a `TypeAssert` to
    /// the Python view, which the backend converts. A function value is left
    /// alone; its calls convert their own arguments and results. See
    /// `smelt_hir::language_view`.
    pub(crate) fn in_own_language_view(&mut self, value: smelt_hir::ExprId, body: &mut Body) -> smelt_hir::ExprId {
        let ty = Self::expr_ty(body, value);
        let language = smelt_hir::language_view::SourceLanguage::Python;
        if matches!(self.ctx.krate.types.get(ty), Some(Type::Function(_)))
            || !smelt_hir::language_view::crosses_into(&self.ctx.krate.types, ty, language)
        {
            return value;
        }
        let span = body
            .exprs
            .get(usize::try_from(value.0).unwrap_or(usize::MAX))
            .map_or_else(|| Span::new(self.file_id, 0, 0), |expr| expr.span);
        let viewed = smelt_hir::language_view::view_in(&mut self.ctx.krate.types, ty, language);
        body.push_expr(HirExpr { kind: ExprKind::TypeAssert { value }, ty: viewed, span })
    }

    /// Lower one expression; see [`Self::expression_with_hint`].
    fn expression_shape_with_hint(
        &mut self,
        expr: &Expr,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match expr {
            // --- Literals ---
            Expr::NumberLiteral(n) => {
                let (kind, ty) = match &n.value {
                    // A Python integer literal is unbounded: one that fits a
                    // machine word is an `Int` literal, a wider one keeps its
                    // exact decimal digits; both are a Python `int`.
                    Number::Int(i) => {
                        let literal = match i.as_i64() {
                            Some(value) => Literal::Int(value),
                            None => Literal::BigInt(
                                crate::helpers::python_int_literal_digits(&i.to_string()).ok_or_else(|| {
                                    SmeltError::unsupported(self.span(n.range), "malformed integer literal")
                                })?,
                            ),
                        };
                        (ExprKind::Literal(literal), self.intern_type(Type::PyInt))
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
                let ty = self.intern_type(Type::PyStr);
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

            Expr::Call(call) => self.call_expression_with_hint(call, body, type_hint),

            Expr::Await(await_expr) => {
                if self.scope.kind != FnKind::Async {
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
                let span = self.span(await_expr.range);
                let result = body.push_expr(HirExpr { kind: ExprKind::Await(awaited), ty, span });
                Ok(self.gather_result_as_list(awaited, result, body))
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
                // A read through an imported module no rule above maps
                // (`sys.argv`) names the API rather than the module name.
                if let Some(error) = self.unmapped_import_use(expr, self.span(attr.range)) {
                    return Err(error);
                }
                let receiver = self.expression(&attr.value, body)?;
                let receiver_ty = Self::expr_ty(body, receiver);
                if receiver_ty == self.py_exception_type() {
                    return self.exception_attribute_expr(receiver, attr, body);
                }
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
                let index_is_int =
                    matches!(self.ctx.krate.types.get(Self::expr_ty(body, index)), Some(Type::PyInt));
                // A sequence position is a machine index: an `int` past one is
                // the `IndexError` `CPython` raises before it reads.
                let position = if index_is_int
                    && matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::List(_) | Type::PyStr))
                {
                    self.py_int_as_index(index, body)
                } else {
                    index
                };
                let kind = match self.ctx.krate.types.get(receiver_ty) {
                    Some(Type::Dict(..)) => ExprKind::PythonDictRead { dict: receiver, key: position },
                    Some(Type::List(_)) if index_is_int => ExprKind::PythonListRead { list: receiver, index: position },
                    Some(Type::PyStr) if index_is_int => {
                        ExprKind::PythonSequenceRead { sequence: receiver, index: position }
                    }
                    _ => ExprKind::Index { receiver, index: position },
                };
                Ok(body.push_expr(HirExpr {
                    kind,
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
            // come from an expected `Callable`/function `type_hint`; a lambda
            // without a hint is rejected.
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
    /// The body lowers through ordinary expression lowering in its own closure
    /// body ([`Self::python_closure`]), and the resulting closure value can be
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

        let closure =
            self.lambda_closure(lambda, &function.params, Some(function.return_ty), body)?;
        if closure.return_ty != function.return_ty {
            return Err(SmeltError::unsupported(
                self.span(lambda.range),
                "lambda return type does not match its expected callable annotation",
            ));
        }
        Ok(closure.expr)
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
        let str_ty = self.intern_type(Type::PyStr);
        let mut segments: Vec<Segment> = Vec::new();

        for part in fstring.value.iter() {
            match part {
                ruff_python_ast::FStringPartRef::Literal(literal) => {
                    segments.push(Segment::Literal(literal.as_str().to_owned()));
                }
                ruff_python_ast::FStringPartRef::FString(inner) => {
                    for element in &inner.elements {
                        match element {
                            ruff_python_ast::InterpolatedStringElement::Literal(literal) => {
                                segments.push(Segment::Literal(literal.value.to_string()));
                            }
                            ruff_python_ast::InterpolatedStringElement::Interpolation(interp) => {
                                // A format spec with a nested replacement field
                                // (`{x:{width}}`) is built at run time; only a
                                // spec written out in full is supported.
                                let spec = match &interp.format_spec {
                                    None => None,
                                    Some(format_spec) => {
                                        let mut text = String::new();
                                        for spec_element in &format_spec.elements {
                                            match spec_element {
                                                ruff_python_ast::InterpolatedStringElement::Literal(literal) => text.push_str(&literal.value),
                                                ruff_python_ast::InterpolatedStringElement::Interpolation(_) => {
                                                    return Err(SmeltError::unsupported(
                                                        self.span(interp.range),
                                                        "an f-string format spec with a nested replacement field (`{x:{width}}`) is not supported",
                                                    ));
                                                }
                                            }
                                        }
                                        Some(text).filter(|written| !written.is_empty())
                                    }
                                };
                                // `{x=}` writes its own source text, then the
                                // value: its `repr` unless a conversion or a
                                // format spec says otherwise.
                                let conversion = interp.debug_text.as_ref().map_or(interp.conversion, |debug| {
                                    segments.push(Segment::Literal(debug.as_str().to_owned()));
                                    if interp.conversion == ruff_python_ast::ConversionFlag::None && spec.is_none() {
                                        ruff_python_ast::ConversionFlag::Repr
                                    } else {
                                        interp.conversion
                                    }
                                });
                                let interpolated = self.expression(&interp.expression, body)?;
                                // `!r` is `repr(x)`; a container or an exception
                                // formats as its `str`, which is Python text the
                                // string `+` fold below would spell the
                                // JavaScript way.
                                let expr_ty = Self::expr_ty(body, interpolated);
                                let expr = if conversion == ruff_python_ast::ConversionFlag::Ascii {
                                    self.python_ascii_expr(interpolated, self.span(interp.range), body)?
                                } else if conversion == ruff_python_ast::ConversionFlag::Repr {
                                    self.python_text_expr(interpolated, true, self.span(interp.range), body)?
                                } else if conversion == ruff_python_ast::ConversionFlag::Str
                                    || !matches!(
                                        self.ctx.krate.types.get(expr_ty),
                                        Some(Type::Bool | Type::PyInt | Type::Float | Type::PyStr)
                                    ) && (self.has_python_text(expr_ty)
                                        || self.class_text_method(expr_ty, false).is_some())
                                {
                                    self.python_text_expr(interpolated, false, self.span(interp.range), body)?
                                } else {
                                    interpolated
                                };
                                // `f"{x}"` formats `x` with `format(x, "")`, which
                                // for `int`, `float` and `bool` is `str(x)`: the
                                // same `ToString` cast `str(x)` lowers to, so a
                                // float interpolates as `12.0` and an int keeps
                                // all its digits (the string `+` fold below
                                // would coerce it the JavaScript way).
                                let interp_span = self.span(interp.range);
                                let expr = if let Some(spec_text) = spec {
                                    self.formatted_value(expr, spec_text, interp_span, body)?
                                } else if matches!(
                                    self.ctx.krate.types.get(Self::expr_ty(body, expr)),
                                    Some(Type::Bool | Type::PyInt | Type::Float)
                                ) {
                                    body.push_expr(HirExpr {
                                        kind: ExprKind::PrimitiveCast {
                                            op: PrimitiveCastOp::ToString,
                                            operand: expr,
                                        },
                                        ty: str_ty,
                                        span: self.span(interp.range),
                                    })
                                } else {
                                    expr
                                };
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

    /// `str * int` or `int * str`: Python's sequence repetition, not arithmetic.
    ///
    /// The string operand keeps its own type (a `str` stays `PyStr`), and a
    /// count of zero or less yields the empty string, as `CPython` does; the
    /// emitter clamps an integer count at zero for that. `None` for any other
    /// operand pair, which stays a numeric multiplication.
    #[expect(clippy::too_many_arguments, reason = "one operation, its operands and its lowering position")]
    fn sequence_repeat(
        &mut self,
        operand_kinds: &(Option<Type>, Option<Type>),
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let (operand, count) = match operand_kinds {
            (Some(Type::String | Type::PyStr), Some(Type::PyInt)) => (lhs, rhs),
            (Some(Type::PyInt), Some(Type::String | Type::PyStr)) => (rhs, lhs),
            _ => return None,
        };
        // A count past a machine word is `CPython`'s `OverflowError`.
        let size = self.py_int_as_size(count, body);
        let ty = Self::expr_ty(body, operand);
        Some(body.push_expr(HirExpr {
            kind: ExprKind::StringRepeat { operand, count: size },
            ty,
            span,
        }))
    }

    /// `format(value, spec)` for a non-empty `spec`: an `int`, `float` or
    /// `str` through `CPython`'s format-spec mini-language. A `bool` formats as
    /// the `int` it is (`f"{True:>5}"` is `'    1'`); any other type would
    /// call its `__format__`, which is not supported.
    fn formatted_value(
        &mut self,
        value: smelt_hir::ExprId,
        spec: String,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let shown = match self.ctx.krate.types.get(Self::expr_ty(body, value)) {
            Some(Type::PyInt | Type::Float | Type::PyStr) => value,
            Some(Type::Bool) => {
                let ty = self.intern_type(Type::PyInt);
                body.push_expr(HirExpr { kind: ExprKind::TypeAssert { value }, ty, span })
            }
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "a format spec is supported for int, float, bool and str values",
                ));
            }
        };
        let ty = self.intern_type(Type::PyStr);
        Ok(body.push_expr(HirExpr { kind: ExprKind::PyFormat { value: shown, spec }, ty, span }))
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

    /// Lower a binary arithmetic operator expression.
    fn binop_expression(
        &mut self,
        b: &ruff_python_ast::ExprBinOp,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(b.range);
        if b.op == Operator::Mod
            && let Expr::StringLiteral(template) = b.left.as_ref()
        {
            return self.percent_format_expression(template.value.to_str(), &b.right, span, body);
        }
        let lhs = self.expression(&b.left, body)?;
        let rhs = self.expression(&b.right, body)?;
        if let Some(expr) = self.set_operator_expression(b.op, lhs, rhs, span, body)? {
            return Ok(expr);
        }
        let op = Self::python_arithmetic_op(b.op).ok_or_else(|| {
            SmeltError::unsupported(span, format!("binary operator '{}' is not supported", b.op))
        })?;
        self.python_arithmetic(op, lhs, rhs, span, body)
    }

    /// Python's in-place set operators: `s |= t`, `s &= t`, `s -= t` and
    /// `s ^= t` on a set `s` MUTATE it (`set.__ior__` and friends), so every
    /// alias sees the new members; rebinding `s` to `s | t` would leave the
    /// aliases behind. `None` when `target` is not a set.
    #[expect(clippy::too_many_arguments, reason = "one operation, its operands and its lowering position")]
    pub(super) fn set_in_place_update(
        &mut self,
        op: Operator,
        target: smelt_hir::ExprId,
        other: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let target_ty = Self::expr_ty(body, target);
        if !matches!(self.ctx.krate.types.get(target_ty), Some(Type::Set(_))) {
            return Ok(None);
        }
        let set_op = match op {
            Operator::BitOr => smelt_hir::SetBinaryOp::Union,
            Operator::BitAnd => smelt_hir::SetBinaryOp::Intersection,
            Operator::Sub => smelt_hir::SetBinaryOp::Difference,
            Operator::BitXor => smelt_hir::SetBinaryOp::SymmetricDifference,
            _ => return Err(SmeltError::unsupported(span, format!("augmented operator '{op}=' is not defined for sets"))),
        };
        if Self::expr_ty(body, other) != target_ty {
            return Err(SmeltError::unsupported(span, "in-place set operators require two sets of the same element type"));
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::SetUpdate { op: set_op, set: target, other },
            ty,
            span,
        })))
    }

    /// Python's in-place list operators: `xs += ys` and `xs *= n` MUTATE `xs`
    /// (`list.__iadd__` extends it, `list.__imul__` repeats it in place), so
    /// every alias of the list sees the change; rebinding `xs` to `xs + ys`
    /// would leave the aliases behind. Pushes the update and answers `true`
    /// when `target` is a list.
    #[expect(clippy::too_many_arguments, reason = "one operation, its operands and its lowering position")]
    pub(super) fn list_in_place_update(
        &mut self,
        op: Operator,
        target: smelt_hir::ExprId,
        other: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let target_ty = Self::expr_ty(body, target);
        if !matches!(self.ctx.krate.types.get(target_ty), Some(Type::List(_))) {
            return Ok(false);
        }
        let none_ty = self.intern_type(Type::None);
        match op {
            Operator::Add => {
                if Self::expr_ty(body, other) != target_ty {
                    return Err(SmeltError::unsupported(span, "list += needs a list of the same element type"));
                }
                let extend = body.push_expr(HirExpr { kind: ExprKind::ListExtend { list: target, other }, ty: none_ty, span });
                body.push_stmt_to_block(block, HirStmt::Expr(extend));
                Ok(true)
            }
            Operator::Mult => {
                if !matches!(self.ctx.krate.types.get(Self::expr_ty(body, other)), Some(Type::PyInt)) {
                    return Err(SmeltError::unsupported(span, "list *= needs an int count"));
                }
                // The repeated elements are read before the list is emptied.
                let size = self.py_int_as_size(other, body);
                let repeated = body.push_expr(HirExpr { kind: ExprKind::ListRepeat { list: target, count: size }, ty: target_ty, span });
                let repeated_local = body.push_local(LocalDecl {
                    name: None,
                    ty: target_ty,
                    mutable: false,
                    span,
                });
                let pattern = body.push_pattern(smelt_hir::Pattern::Binding(repeated_local));
                body.push_stmt_to_block(block, HirStmt::Let { pat: pattern, ty: target_ty, value: Some(repeated) });
                let read = body.push_expr(HirExpr { kind: ExprKind::Local(repeated_local), ty: target_ty, span });
                let clear = body.push_expr(HirExpr { kind: ExprKind::ListClear { list: target }, ty: none_ty, span });
                body.push_stmt_to_block(block, HirStmt::Expr(clear));
                let extend = body.push_expr(HirExpr { kind: ExprKind::ListExtend { list: target, other: read }, ty: none_ty, span });
                body.push_stmt_to_block(block, HirStmt::Expr(extend));
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    /// Python's set operators on two sets: `|` union, `&` intersection, `-`
    /// difference and `^` symmetric difference, each a NEW set (the operands
    /// are not mutated). `None` when either operand is not a set.
    #[expect(clippy::too_many_arguments, reason = "one operation, its operands and its lowering position")]
    fn set_operator_expression(
        &self,
        op: Operator,
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let left_ty = Self::expr_ty(body, lhs);
        let right_ty = Self::expr_ty(body, rhs);
        if !matches!(self.ctx.krate.types.get(left_ty), Some(Type::Set(_)))
            || !matches!(self.ctx.krate.types.get(right_ty), Some(Type::Set(_)))
        {
            return Ok(None);
        }
        let set_op = match op {
            Operator::BitOr => smelt_hir::SetBinaryOp::Union,
            Operator::BitAnd => smelt_hir::SetBinaryOp::Intersection,
            Operator::Sub => smelt_hir::SetBinaryOp::Difference,
            Operator::BitXor => smelt_hir::SetBinaryOp::SymmetricDifference,
            _ => return Err(SmeltError::unsupported(span, format!("binary operator '{op}' is not defined for sets"))),
        };
        if left_ty != right_ty {
            return Err(SmeltError::unsupported(span, "set operators require two sets of the same element type"));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::SetBinary { op: set_op, left: lhs, right: rhs },
            ty: left_ty,
            span,
        })))
    }

    /// The arithmetic a Python operator stands for, when Smelt models it.
    const fn python_arithmetic_op(op: Operator) -> Option<PythonArithmetic> {
        match op {
            Operator::Add => Some(PythonArithmetic::Plain(BinOp::Add)),
            Operator::Sub => Some(PythonArithmetic::Plain(BinOp::Sub)),
            Operator::Mult => Some(PythonArithmetic::Plain(BinOp::Mul)),
            Operator::Div => Some(PythonArithmetic::Division(PythonDivisionOp::TrueDiv)),
            Operator::FloorDiv => Some(PythonArithmetic::Division(PythonDivisionOp::FloorDiv)),
            Operator::Mod => Some(PythonArithmetic::Division(PythonDivisionOp::Mod)),
            Operator::Pow => Some(PythonArithmetic::Power),
            Operator::LShift => Some(PythonArithmetic::Shift(PyNumericOp::Shl)),
            Operator::RShift => Some(PythonArithmetic::Shift(PyNumericOp::Shr)),
            Operator::BitOr => Some(PythonArithmetic::Bitwise(BinOp::BitOr)),
            Operator::BitXor => Some(PythonArithmetic::Bitwise(BinOp::BitXor)),
            Operator::BitAnd => Some(PythonArithmetic::Bitwise(BinOp::BitAnd)),
            Operator::MatMult => None,
        }
    }

    /// Apply Python's numeric tower to two lowered operands.
    ///
    /// Arithmetic mixing `int` and `float` promotes the `int` (`1 + 2.5 == 3.5`)
    /// through the same raising conversion `float(x)` lowers to, so codegen
    /// never sees a `SmeltPyInt` meet an `f64`. Two `int`s stay exact: `+ - *`
    /// and the bitwise operators are `int` operators at every magnitude, `/`
    /// is correctly rounded true division into a `float` (`7 / 2 == 3.5`), `//`
    /// and `%` floor toward negative infinity, `**` is an exact `int` unless
    /// its exponent is a negative constant (then a `float`, `2 ** -1 == 0.5`),
    /// and every operator that can raise (`ZeroDivisionError`, a negative
    /// shift count) lowers to a [`PyNumericOp`] call a `try` can catch.
    #[expect(clippy::too_many_arguments, reason = "one operation, its operands and its lowering position")]
    fn python_arithmetic(
        &mut self,
        op: PythonArithmetic,
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let operand_kinds = (
            self.ctx.krate.types.get(Self::expr_ty(body, lhs)).cloned(),
            self.ctx.krate.types.get(Self::expr_ty(body, rhs)).cloned(),
        );
        let both_numeric = matches!(
            operand_kinds,
            (Some(Type::PyInt | Type::Float), Some(Type::PyInt | Type::Float))
        );
        let both_int = matches!(operand_kinds, (Some(Type::PyInt), Some(Type::PyInt)));
        // An `int | float` operand (a `min`/`max` of mixed arguments, a
        // function annotated `-> int | float`) needs the operation run per
        // member, which the HIR has no member test for yet: refuse it here
        // rather than emit Rust that narrows to one member.
        let numeric_union = |kind: &Option<Type>| {
            matches!(kind, Some(Type::Union(members)) if members.iter().any(|member| {
                matches!(self.ctx.krate.types.get(*member), Some(Type::PyInt | Type::Float))
            }))
        };
        if numeric_union(&operand_kinds.0) || numeric_union(&operand_kinds.1) {
            return Err(SmeltError::unsupported(
                span,
                "arithmetic on an `int | float` value is not supported yet",
            ));
        }
        let any_float = matches!(operand_kinds, (Some(Type::Float), _) | (_, Some(Type::Float)));
        if op == PythonArithmetic::Plain(BinOp::Mul)
            && let Some(repeat) = self.sequence_repeat(&operand_kinds, lhs, rhs, span, body)
        {
            return Ok(repeat);
        }
        if let PythonArithmetic::Plain(sequence_op @ (BinOp::Add | BinOp::Mul)) = op
            && let Some(sequence) = self.sequence_arithmetic(sequence_op, &operand_kinds, lhs, rhs, span, body)?
        {
            return Ok(sequence);
        }
        let (left, right) = if both_numeric && any_float {
            (self.promote_int_to_float(lhs, body), self.promote_int_to_float(rhs, body))
        } else {
            (lhs, rhs)
        };
        let lhs_ty = Self::expr_ty(body, left);
        match op {
            PythonArithmetic::Plain(plain_op) => {
                let ty = if plain_op == BinOp::Add {
                    self.python_add_return_ty(lhs_ty).unwrap_or(lhs_ty)
                } else {
                    lhs_ty
                };
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::BinOp { op: plain_op, lhs: left, rhs: right },
                    ty,
                    span,
                }))
            }
            PythonArithmetic::Division(division_op) if both_int => {
                let int_op = match division_op {
                    PythonDivisionOp::TrueDiv => PyNumericOp::TrueDiv,
                    PythonDivisionOp::FloorDiv => PyNumericOp::FloorDiv,
                    PythonDivisionOp::Mod => PyNumericOp::Mod,
                };
                Ok(self.py_numeric_call(int_op, vec![left, right], span, body))
            }
            PythonArithmetic::Division(division_op) if both_numeric => {
                let divisor_checked = !Self::is_nonzero_constant(right, body);
                let kind = if division_op == PythonDivisionOp::TrueDiv && !divisor_checked {
                    ExprKind::BinOp {
                        op: BinOp::Div,
                        lhs: left,
                        rhs: right,
                    }
                } else {
                    ExprKind::PythonDivision {
                        op: division_op,
                        lhs: left,
                        rhs: right,
                        divisor_checked,
                    }
                };
                Ok(body.push_expr(HirExpr {
                    kind,
                    ty: lhs_ty,
                    span,
                }))
            }
            PythonArithmetic::Division(PythonDivisionOp::TrueDiv) => {
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Div,
                        lhs: left,
                        rhs: right,
                    },
                    ty: lhs_ty,
                    span,
                }))
            }
            PythonArithmetic::Division(division_op) => Err(SmeltError::unsupported(
                span,
                format!("operator '{}' needs int or float operands", division_op.source_text()),
            )),
            PythonArithmetic::Power if both_int => {
                let pow = if Self::is_negative_int_constant(right, body) {
                    PyNumericOp::PowFloat
                } else {
                    PyNumericOp::Pow
                };
                Ok(self.py_numeric_call(pow, vec![left, right], span, body))
            }
            PythonArithmetic::Power if both_numeric => {
                Ok(self.py_numeric_call(PyNumericOp::FloatPow, vec![left, right], span, body))
            }
            PythonArithmetic::Shift(shift) if both_int => {
                Ok(self.py_numeric_call(shift, vec![left, right], span, body))
            }
            PythonArithmetic::Bitwise(bitwise_op) if both_int => Ok(body.push_expr(HirExpr {
                kind: ExprKind::BinOp { op: bitwise_op, lhs: left, rhs: right },
                ty: lhs_ty,
                span,
            })),
            PythonArithmetic::Power => Err(SmeltError::unsupported(
                span,
                "operator '**' needs int or float operands",
            )),
            PythonArithmetic::Shift(_) | PythonArithmetic::Bitwise(_) => Err(
                SmeltError::unsupported(span, "shift and bitwise operators need two int operands"),
            ),
        }
    }

    /// Python's sequence `+` and `*` on lists and tuples: `xs + ys` is a new
    /// list (or tuple) of both operands' elements, `xs * n` and `n * xs` the
    /// elements `n` times over, sharing the elements themselves as `CPython`
    /// does. `None` for any other operand pair.
    ///
    /// A tuple's length is part of its type, so tuple repetition needs a
    /// constant count; two lists (or tuples) of different element types would
    /// need a union element type the source did not spell, and are refused.
    #[expect(clippy::too_many_arguments, reason = "one operation, its operands and its lowering position")]
    fn sequence_arithmetic(
        &mut self,
        op: BinOp,
        operand_kinds: &(Option<Type>, Option<Type>),
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        match (op, operand_kinds) {
            (BinOp::Add, (Some(Type::List(left)), Some(Type::List(right)))) => {
                if left != right {
                    return Err(SmeltError::unsupported(
                        span,
                        "list + list needs two lists of the same element type",
                    ));
                }
                let ty = Self::expr_ty(body, lhs);
                Ok(Some(body.push_expr(HirExpr { kind: ExprKind::ListConcat { left: lhs, right: rhs }, ty, span })))
            }
            (BinOp::Mul, (Some(Type::List(_)), Some(Type::PyInt)) | (Some(Type::PyInt), Some(Type::List(_)))) => {
                let (list, count) = if matches!(operand_kinds.0, Some(Type::List(_))) { (lhs, rhs) } else { (rhs, lhs) };
                // A count past a machine word is `CPython`'s `OverflowError`.
                let size = self.py_int_as_size(count, body);
                let ty = Self::expr_ty(body, list);
                Ok(Some(body.push_expr(HirExpr { kind: ExprKind::ListRepeat { list, count: size }, ty, span })))
            }
            (BinOp::Add, (Some(Type::Tuple(left)), Some(Type::Tuple(right)))) => {
                let parts = [(lhs, left.clone()), (rhs, right.clone())];
                Ok(Some(self.tuple_from_parts(&parts, span, body)))
            }
            (BinOp::Mul, (Some(Type::Tuple(items)), Some(Type::PyInt)) | (Some(Type::PyInt), Some(Type::Tuple(items)))) => {
                let (tuple, count) = if matches!(operand_kinds.0, Some(Type::Tuple(_))) { (lhs, rhs) } else { (rhs, lhs) };
                let Some(constant) = Self::small_int_constant(count, body) else {
                    return Err(SmeltError::unsupported(
                        span,
                        "tuple * n needs a constant n: a tuple's length is part of its type",
                    ));
                };
                let times = usize::try_from(constant.max(0)).unwrap_or(0);
                let parts = vec![(tuple, items.clone()); times];
                Ok(Some(self.tuple_from_parts(&parts, span, body)))
            }
            _ => Ok(None),
        }
    }

    /// The tuple of every element of each `(tuple, element types)` part, in
    /// order: each part is evaluated once and read element by element.
    fn tuple_from_parts(
        &mut self,
        parts: &[(smelt_hir::ExprId, Vec<TypeId>)],
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let mut elements = Vec::new();
        let mut element_tys = Vec::new();
        for (tuple, items) in parts {
            for (index, item_ty) in items.iter().enumerate() {
                elements.push(body.push_expr(HirExpr {
                    kind: ExprKind::TupleIndex { tuple: *tuple, index },
                    ty: *item_ty,
                    span,
                }));
                element_tys.push(*item_ty);
            }
        }
        let ty = self.intern_type(Type::Tuple(element_tys));
        body.push_expr(HirExpr { kind: ExprKind::TupleLit(elements), ty, span })
    }

    /// A raising Python numeric operation over `args`, typed as the operation
    /// answers.
    pub(super) fn py_numeric_call(
        &mut self,
        op: PyNumericOp,
        args: Vec<smelt_hir::ExprId>,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let py_int = self.intern_type(Type::PyInt);
        let ty = self.intern_type(match op {
            PyNumericOp::Parse
            | PyNumericOp::ParseBase
            | PyNumericOp::FromFloat
            | PyNumericOp::FloorDiv
            | PyNumericOp::Mod
            | PyNumericOp::Pow
            | PyNumericOp::PowMod
            | PyNumericOp::Shl
            | PyNumericOp::Shr
            | PyNumericOp::RoundToInt
            | PyNumericOp::RoundInt => Type::PyInt,
            PyNumericOp::ToFloat
            | PyNumericOp::TrueDiv
            | PyNumericOp::PowFloat
            | PyNumericOp::FloatPow
            | PyNumericOp::ParseFloat
            | PyNumericOp::RoundFloat => Type::Float,
            PyNumericOp::ToIndex | PyNumericOp::ToSize | PyNumericOp::Ord => Type::Int,
            PyNumericOp::Chr => Type::PyStr,
            PyNumericOp::Range => Type::List(py_int),
        });
        body.push_expr(HirExpr {
            kind: ExprKind::PyNumericCall { op, args },
            ty,
            span,
        })
    }

    /// A machine integer (`len(xs)`, a `str.find` position, a `list.count`)
    /// as the Python `int` it is: `kind` is pushed at `i64` and converted, so
    /// the operations shared with TypeScript keep their machine type.
    pub(super) fn machine_int_as_py_int(
        &mut self,
        kind: ExprKind,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let machine_ty = self.intern_type(Type::Int);
        let value = body.push_expr(HirExpr { kind, ty: machine_ty, span });
        let ty = self.intern_type(Type::PyInt);
        body.push_expr(HirExpr { kind: ExprKind::TypeAssert { value }, ty, span })
    }

    /// A Python `int` operand at a position that takes a machine index (a
    /// subscript): converted with `IndexError` past `i64`. Any other operand
    /// is returned unchanged.
    pub(super) fn py_int_as_index(
        &mut self,
        operand: smelt_hir::ExprId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        self.py_int_as_machine(operand, PyNumericOp::ToIndex, body)
    }

    /// A Python `int` operand at a position that takes a machine size (a
    /// repeat count, a bound): converted with `OverflowError` past `i64`.
    pub(super) fn py_int_as_size(
        &mut self,
        operand: smelt_hir::ExprId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        self.py_int_as_machine(operand, PyNumericOp::ToSize, body)
    }

    /// A Python `int` slice bound as a machine one: `CPython` clamps a bound
    /// past a machine word to the word's range (`xs[:2**70]` is all of `xs`),
    /// which the saturating conversion is.
    pub(super) fn py_int_as_clamped_bound(
        &mut self,
        operand: smelt_hir::ExprId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        if !matches!(self.ctx.krate.types.get(Self::expr_ty(body, operand)), Some(Type::PyInt)) {
            return operand;
        }
        let span = Self::expr_span(body, operand);
        let ty = self.intern_type(Type::Int);
        if let Some(value) = Self::small_int_constant(operand, body) {
            return body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Int(value)), ty, span });
        }
        body.push_expr(HirExpr { kind: ExprKind::TypeAssert { value: operand }, ty, span })
    }

    /// See [`Self::py_int_as_index`]. A small constant converts statically.
    fn py_int_as_machine(
        &mut self,
        operand: smelt_hir::ExprId,
        op: PyNumericOp,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        if !matches!(self.ctx.krate.types.get(Self::expr_ty(body, operand)), Some(Type::PyInt)) {
            return operand;
        }
        let span = Self::expr_span(body, operand);
        if let Some(value) = Self::small_int_constant(operand, body) {
            let ty = self.intern_type(Type::Int);
            return body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Int(value)), ty, span });
        }
        self.py_numeric_call(op, vec![operand], span, body)
    }

    /// The literal `-value` of an `int` literal operand, in the narrowest
    /// spelling that holds it.
    fn negated_int_literal(operand: smelt_hir::ExprId, body: &Body) -> Option<Literal> {
        let index = usize::try_from(operand.0).ok()?;
        match &body.exprs.get(index)?.kind {
            ExprKind::Literal(Literal::Int(value)) => Some(
                value.checked_neg().map_or_else(
                    || Literal::BigInt(i128::from(*value).checked_neg().unwrap_or_default().to_string()),
                    Literal::Int,
                ),
            ),
            ExprKind::Literal(Literal::BigInt(digits)) => {
                let negated = digits.strip_prefix('-').map_or_else(|| format!("-{digits}"), str::to_owned);
                Some(negated.parse::<i64>().map_or(Literal::BigInt(negated), Literal::Int))
            }
            _ => None,
        }
    }

    /// The value of an `int` constant that fits `i64` (`3`, `-1`).
    fn small_int_constant(expr: smelt_hir::ExprId, body: &Body) -> Option<i64> {
        let index = usize::try_from(expr.0).ok()?;
        match &body.exprs.get(index)?.kind {
            ExprKind::Literal(Literal::Int(value)) => Some(*value),
            ExprKind::UnaryOp { op: UnaryOp::Neg, operand } => {
                Self::small_int_constant(*operand, body)?.checked_neg()
            }
            _ => None,
        }
    }

    /// Whether an operand is an `int` constant below zero (`-1`), which makes
    /// `**` answer a `float`.
    fn is_negative_int_constant(expr: smelt_hir::ExprId, body: &Body) -> bool {
        let index = usize::try_from(expr.0).unwrap_or(usize::MAX);
        match body.exprs.get(index).map(|found| &found.kind) {
            Some(ExprKind::UnaryOp { op: UnaryOp::Neg, operand }) => {
                Self::is_positive_int_constant(*operand, body)
            }
            Some(ExprKind::Literal(Literal::Int(value))) => *value < 0,
            _ => false,
        }
    }

    /// Whether an operand is an `int` constant above zero.
    fn is_positive_int_constant(expr: smelt_hir::ExprId, body: &Body) -> bool {
        let index = usize::try_from(expr.0).unwrap_or(usize::MAX);
        match body.exprs.get(index).map(|found| &found.kind) {
            Some(ExprKind::Literal(Literal::Int(value))) => *value > 0,
            Some(ExprKind::Literal(Literal::BigInt(digits))) => !digits.starts_with('-'),
            _ => false,
        }
    }

    /// The place a lowered subscript writes to: reading `d[k]` raises on a missing
    /// key, storing to it inserts one; reading `xs[i]` raises past the end,
    /// storing to it is the plain indexed store.
    fn subscript_store_target(
        target: smelt_hir::ExprId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let read_index = usize::try_from(target.0).unwrap_or(usize::MAX);
        let read = &body.exprs[read_index];
        let (dict, key) = match read.kind {
            ExprKind::PythonDictRead { dict, key } => (dict, key),
            ExprKind::PythonListRead { list, index } => (list, index),
            ExprKind::PythonSequenceRead { sequence, index } => (sequence, index),
            _ => return target,
        };
        let (ty, span) = (read.ty, read.span);
        body.push_expr(HirExpr {
            kind: ExprKind::Index {
                receiver: dict,
                index: key,
            },
            ty,
            span,
        })
    }

    /// Whether an operand is a numeric constant other than zero, which `/`, `//`
    /// and `%` cannot raise on.
    fn is_nonzero_constant(expr: smelt_hir::ExprId, body: &Body) -> bool {
        let index = usize::try_from(expr.0).unwrap_or(usize::MAX);
        match &body.exprs[index].kind {
            ExprKind::Literal(Literal::Int(value)) => *value != 0,
            ExprKind::Literal(Literal::Float(value)) => *value != 0.0,
            ExprKind::PrimitiveCast { operand, .. }
            | ExprKind::UnaryOp {
                op: UnaryOp::Neg,
                operand,
            } => Self::is_nonzero_constant(*operand, body),
            _ => false,
        }
    }

    /// Cast an `int`-typed operand to `float` (`float(x)`); any other operand
    /// is returned unchanged. See the numeric-tower rule in
    /// [`Self::binop_expression`].
    fn promote_int_to_float(
        &mut self,
        operand: smelt_hir::ExprId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, operand)),
            Some(Type::PyInt)
        ) {
            return operand;
        }
        let span = Self::expr_span(body, operand);
        // A constant converts exactly at compile time; anything else may be
        // past `f64`'s range, which `float(n)` raises on.
        if let Some(value) = Self::small_int_constant(operand, body)
            && value.unsigned_abs() <= 1_u64 << 53_u32
        {
            let float_ty = self.intern_type(Type::Float);
            #[expect(clippy::cast_precision_loss, clippy::as_conversions, reason = "the value is within 2^53, exact in f64")]
            let exact = value as f64;
            return body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Float(exact)), ty: float_ty, span });
        }
        self.py_numeric_call(PyNumericOp::ToFloat, vec![operand], span, body)
    }

    /// Return the concrete output type declared by a class's Python `__add__`.
    fn python_add_return_ty(&self, lhs_ty: TypeId) -> Option<TypeId> {
        let Type::Class { name, .. } = self.ctx.krate.types.get(lhs_ty)? else {
            return None;
        };
        let class = self.ctx.krate.items.iter().find_map(|item| match item {
            Item::Class(class) if class.name == *name => Some(class),
            _ => None,
        })?;
        class.protocols.iter().find_map(|protocol| match protocol {
            ClassProtocol::Add { method } => match self.item_ref(*method) {
                Item::Function(function) => Some(function.return_ty),
                _ => None,
            },
            ClassProtocol::Str { .. } | ClassProtocol::Repr { .. } => None,
        })
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
            acc = self.short_circuit(op, acc, rhs, span, body);
        }
        Ok(acc)
    }

    /// `lhs and rhs` / `lhs or rhs` over two `bool`s, evaluating `rhs` only
    /// when `lhs` does not decide the result, as Python does: a conditional
    /// (`rhs if lhs else False`, `True if lhs else rhs`), never an eager
    /// operator, so a call in `rhs` runs only when it is reached.
    #[expect(clippy::too_many_arguments, reason = "one operation, its operands and its lowering position")]
    fn short_circuit(
        &mut self,
        op: BinOp,
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let bool_ty = self.intern_type(Type::Bool);
        let decided = body.push_expr(HirExpr {
            kind: ExprKind::Literal(Literal::Bool(op == BinOp::Or)),
            ty: bool_ty,
            span,
        });
        let (then_expr, else_expr) = if op == BinOp::Or { (decided, rhs) } else { (rhs, decided) };
        body.push_expr(HirExpr {
            kind: ExprKind::Conditional { cond: lhs, then_expr, else_expr },
            ty: bool_ty,
            span,
        })
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
            && let Some(Type::Optional(inner, _)) = self.ctx.krate.types.get(hint)
            && ((*inner == lhs_ty && self.ctx.krate.types.get(rhs_ty) == Some(&Type::None))
                || (*inner == rhs_ty && self.ctx.krate.types.get(lhs_ty) == Some(&Type::None)))
        {
            return Some(hint);
        }
        if self.ctx.krate.types.get(rhs_ty) == Some(&Type::None) {
            return Some(self.intern_type(Type::Optional(lhs_ty, smelt_hir::Absence::Undefined)));
        }
        if self.ctx.krate.types.get(lhs_ty) == Some(&Type::None) {
            return Some(self.intern_type(Type::Optional(rhs_ty, smelt_hir::Absence::Undefined)));
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
            Some(Type::PyInt | Type::Float | Type::PyStr)
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

    /// Whether a value of type `ty` is a list, dict or set: a mutable object
    /// whose `is` comparison reads its identity rather than its contents.
    fn is_identity_container(&self, ty: TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::List(_) | Type::Dict(_, _) | Type::Set(_))
        )
    }

    /// Lower a comparison expression.  Only single-op, non-chained comparisons.
    fn compare_expression(
        &mut self,
        c: &ruff_python_ast::ExprCompare,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(c.range);
        // `a < b < c` is `a < b and b < c` with `b` evaluated once and `c`
        // only when `a < b` holds: each comparison reads the operand the one
        // before it lowered, and the conjunction short-circuits.
        let mut lhs = self.expression(c.first_operand(), body)?;
        let mut chained: Option<smelt_hir::ExprId> = None;
        for (op, comparator) in c.ops.iter().zip(c.comparators()) {
            let rhs = self.expression(comparator, body)?;
            let comparison = self.compare_pair(*op, lhs, rhs, span, body)?;
            chained = Some(chained.map_or(comparison, |previous| {
                self.short_circuit(BinOp::And, previous, comparison, span, body)
            }));
            lhs = rhs;
        }
        chained.ok_or_else(|| SmeltError::unsupported(span, "a comparison needs an operator"))
    }

    /// One comparison `lhs op rhs` of two lowered operands.
    #[expect(clippy::too_many_arguments, reason = "one operation, its operands and its lowering position")]
    fn compare_pair(
        &mut self,
        op: CmpOp,
        lhs: smelt_hir::ExprId,
        rhs: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let bin_op = match op {
            CmpOp::Eq => BinOp::Eq,
            CmpOp::NotEq => BinOp::NotEq,
            CmpOp::Lt => BinOp::Lt,
            CmpOp::LtE => BinOp::Lte,
            CmpOp::Gt => BinOp::Gt,
            CmpOp::GtE => BinOp::Gte,
            CmpOp::In | CmpOp::NotIn => return self.contains_compare(lhs, rhs, span, body, op),
            CmpOp::Is => BinOp::Eq,
            CmpOp::IsNot => BinOp::NotEq,
        };
        // Ordering two sets is Python's subset order: `a <= b` is
        // `a.issubset(b)`, `a < b` a proper subset, and `>=`/`>` the
        // superset forms.
        if let (Some(Type::Set(_)), Some(Type::Set(_))) = (
            self.ctx.krate.types.get(Self::expr_ty(body, lhs)),
            self.ctx.krate.types.get(Self::expr_ty(body, rhs)),
        ) && let Some(relation) = match op {
            CmpOp::LtE => Some(smelt_hir::SetRelationOp::IsSubset),
            CmpOp::GtE => Some(smelt_hir::SetRelationOp::IsSuperset),
            CmpOp::Lt => Some(smelt_hir::SetRelationOp::IsProperSubset),
            CmpOp::Gt => Some(smelt_hir::SetRelationOp::IsProperSuperset),
            _ => None,
        } {
            if Self::expr_ty(body, lhs) != Self::expr_ty(body, rhs) {
                return Err(SmeltError::unsupported(span, "set comparisons require two sets of the same element type"));
            }
            let ty = self.intern_type(Type::Bool);
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::SetRelation { op: relation, left: lhs, right: rhs },
                ty,
                span,
            }));
        }
        // `is` asks for identity. For a list, dict or set — mutable objects
        // with an identity of their own — that is the object-identity
        // comparison (`JsStrictEq`, which reads each container's id), not
        // value equality: `set(a) is a` is False although the two are equal.
        // For `None` and the immutable scalars `==` gives the same answer.
        let compared_op = match op {
            CmpOp::Is | CmpOp::IsNot
                if self.is_identity_container(Self::expr_ty(body, lhs))
                    && self.is_identity_container(Self::expr_ty(body, rhs)) =>
            {
                if matches!(op, CmpOp::Is) { BinOp::JsStrictEq } else { BinOp::JsStrictNotEq }
            }
            _ => bin_op,
        };
        let ty = self.intern_type(Type::Bool);
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::BinOp {
                op: compared_op,
                lhs,
                rhs,
            },
            ty,
            span,
        }))
    }

    /// Lower Python containment comparisons (`in` and `not in`) for strings,
    /// lists, sets, tuples and dicts, over two lowered operands.
    #[expect(clippy::too_many_arguments, reason = "one operation, its operands and its lowering position")]
    fn contains_compare(
        &mut self,
        needle: smelt_hir::ExprId,
        haystack: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
        op: CmpOp,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let needle_ty = Self::expr_ty(body, needle);
        let haystack_ty = Self::expr_ty(body, haystack);
        let bool_ty = self.intern_type(Type::Bool);
        let contains_kind = match self.ctx.krate.types.get(haystack_ty) {
            Some(Type::PyStr) if self.ctx.krate.types.get(needle_ty) == Some(&Type::PyStr) => {
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
                lookup: PropertyLookup::Own,
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
                let operand = self.expression(&u.operand, body)?;
                let operand_ty = Self::expr_ty(body, operand);
                return match (u.op, self.ctx.krate.types.get(operand_ty)) {
                    // `+x` of a number is the number itself.
                    (RuffUnaryOp::UAdd, Some(Type::PyInt | Type::Float)) => Ok(operand),
                    // `~x` is `-x - 1`, which is `x ^ -1` in infinite two's
                    // complement, the `int` operator `^` already is.
                    (RuffUnaryOp::Invert, Some(Type::PyInt)) => {
                        let minus_one = body.push_expr(HirExpr {
                            kind: ExprKind::Literal(Literal::Int(-1)),
                            ty: operand_ty,
                            span,
                        });
                        Ok(body.push_expr(HirExpr {
                            kind: ExprKind::BinOp { op: BinOp::BitXor, lhs: operand, rhs: minus_one },
                            ty: operand_ty,
                            span,
                        }))
                    }
                    _ => Err(SmeltError::unsupported(
                        span,
                        format!("unary operator '{}' needs an int operand", u.op),
                    )),
                };
            }
        };
        let operand = self.expression(&u.operand, body)?;
        // `-5` is the constant `-5`, not a negation evaluated at run time; a
        // `-9223372036854775808` reads its magnitude as wide digits first.
        if op == UnaryOp::Neg
            && matches!(self.ctx.krate.types.get(Self::expr_ty(body, operand)), Some(Type::PyInt))
            && let Some(negated) = Self::negated_int_literal(operand, body)
        {
            let ty = Self::expr_ty(body, operand);
            return Ok(body.push_expr(HirExpr { kind: ExprKind::Literal(negated), ty, span }));
        }
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
