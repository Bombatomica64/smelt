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
            Expr::BoolOp(b) => self.boolop_expression(b, body),
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
                if let Some(member_expr) = self.module_member_expression(attr, body)? {
                    return Ok(member_expr);
                }
                if let Some(url_expr) = self.urlparse_attribute_expression(attr, body)? {
                    return Ok(url_expr);
                }
                let receiver = self.expression(&attr.value, body)?;
                let receiver_ty = Self::expr_ty(body, receiver);
                let field_ty = self.field_type(receiver_ty)?;
                let field = self.intern_name(attr.attr.as_str());
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

            Expr::Named(_)
            | Expr::Lambda(_)
            | Expr::ListComp(_)
            | Expr::SetComp(_)
            | Expr::DictComp(_)
            | Expr::Generator(_)
            | Expr::Yield(_)
            | Expr::YieldFrom(_)
            | Expr::FString(_)
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

    /// Lower a boolean operator — fold `a and b and c` into left-associative pairs.
    fn boolop_expression(
        &mut self,
        b: &ruff_python_ast::ExprBoolOp,
        body: &mut Body,
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
            acc = body.push_expr(HirExpr {
                kind: ExprKind::BinOp { op, lhs: acc, rhs },
                ty: bool_ty,
                span,
            });
        }
        Ok(acc)
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
                ExprKind::StringContains { haystack, needle }
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
