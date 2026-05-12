impl ModuleBuilder<'_> {
    fn expression_with_hint(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match expression {
            Expression::NumericLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Float);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::StringLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(lit.value.to_string())),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::BooleanLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::NullLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::Identifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            Expression::ThisExpression(this_expr) => {
                self.identifier_expression("this", this_expr.span.start, this_expr.span.end, body)
            }
            Expression::ArrayExpression(array) => {
                let mut items = Vec::new();
                for element in &array.elements {
                    let expr = match element {
                        ArrayExpressionElement::SpreadElement(_) => {
                            return Err(SmeltError::unsupported(
                                self.span(element.span().start, element.span().end),
                                "array spread elements are not lowered yet",
                            ));
                        }
                        ArrayExpressionElement::Elision(_) => {
                            return Err(SmeltError::unsupported(
                                self.span(element.span().start, element.span().end),
                                "array elisions are not lowered",
                            ));
                        }
                        _ => self.array_element(element, body)?,
                    };
                    items.push(expr);
                }
                let ty = if let Some(hint) = type_hint {
                    hint
                } else if let Some(first) = items.first() {
                    let item_ty = Self::expr_ty(body, *first);
                    self.ctx.krate.types.intern(Type::List(item_ty))
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "empty arrays require an explicit type annotation",
                    ));
                };
                Ok(body.push_expr(Expr {
                    kind: if matches!(self.ctx.krate.types.get(ty), Some(Type::Tuple(_))) {
                        ExprKind::TupleLit(items)
                    } else {
                        ExprKind::ListLit(items)
                    },
                    ty,
                    span: self.span(array.span.start, array.span.end),
                }))
            }
            Expression::ObjectExpression(object) => {
                self.object_expression(object, body, type_hint)
            }
            Expression::BinaryExpression(binary) => {
                if binary.operator == BinaryOperator::Instanceof {
                    return self.instanceof_expression(binary, body);
                }
                if binary.operator == BinaryOperator::In {
                    let ty = self.ctx.krate.types.intern(Type::Bool);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::Bool(false)),
                        ty,
                        span: self.span(binary.span.start, binary.span.end),
                    }));
                }
                if let Some(expr) = self.unknown_typeof_comparison(binary, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.unknown_null_comparison(binary, body)? {
                    return Ok(expr);
                }
                let op = match binary.operator {
                    BinaryOperator::Addition => BinOp::Add,
                    BinaryOperator::Subtraction => BinOp::Sub,
                    BinaryOperator::Multiplication => BinOp::Mul,
                    BinaryOperator::Division => BinOp::Div,
                    BinaryOperator::Remainder => BinOp::Rem,
                    BinaryOperator::StrictEquality => BinOp::Eq,
                    BinaryOperator::StrictInequality => BinOp::NotEq,
                    BinaryOperator::Equality | BinaryOperator::Inequality => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.span.start, binary.span.end),
                            "coercive equality is not lowered; use === or !==",
                        ));
                    }
                    BinaryOperator::LessThan => BinOp::Lt,
                    BinaryOperator::LessEqualThan => BinOp::Lte,
                    BinaryOperator::GreaterThan => BinOp::Gt,
                    BinaryOperator::GreaterEqualThan => BinOp::Gte,
                    BinaryOperator::Exponential
                    | BinaryOperator::ShiftLeft
                    | BinaryOperator::ShiftRight
                    | BinaryOperator::ShiftRightZeroFill
                    | BinaryOperator::BitwiseOR
                    | BinaryOperator::BitwiseXOR
                    | BinaryOperator::BitwiseAnd
                    | BinaryOperator::In
                    | BinaryOperator::Instanceof => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.span.start, binary.span.end),
                            format!("binary operator is not lowered yet: {:?}", binary.operator),
                        ));
                    }
                };
                let lhs = self.expression(&binary.left, body)?;
                let rhs = self.expression(&binary.right, body)?;
                let ty = if matches!(
                    op,
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte
                ) {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    Self::expr_ty(body, lhs)
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::BinOp { op, lhs, rhs },
                    ty,
                    span: self.span(binary.span.start, binary.span.end),
                }))
            }
            Expression::LogicalExpression(logical) => {
                if logical.operator == LogicalOperator::Or
                    && matches!(logical.left, Expression::ChainExpression(_))
                {
                    return self.expression(&logical.right, body);
                }
                let op = match logical.operator {
                    LogicalOperator::And => BinOp::And,
                    LogicalOperator::Or => BinOp::Or,
                    LogicalOperator::Coalesce => {
                        return Err(SmeltError::unsupported(
                            self.span(logical.span.start, logical.span.end),
                            "nullish coalescing is not lowered yet",
                        ));
                    }
                };
                let lhs = self.expression(&logical.left, body)?;
                let rhs = self.expression(&logical.right, body)?;
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::BinOp { op, lhs, rhs },
                    ty,
                    span: self.span(logical.span.start, logical.span.end),
                }))
            }
            Expression::ConditionalExpression(conditional) => {
                let cond = self.expression(&conditional.test, body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, cond)) != Some(&Type::Bool) {
                    return Err(SmeltError::unsupported(
                        self.span(conditional.test.span().start, conditional.test.span().end),
                        "conditional expression condition must be boolean",
                    ));
                }
                let then_expr =
                    self.expression_with_hint(&conditional.consequent, body, type_hint)?;
                let branch_hint = Some(Self::expr_ty(body, then_expr));
                let else_expr =
                    self.expression_with_hint(&conditional.alternate, body, branch_hint)?;
                let then_ty = Self::expr_ty(body, then_expr);
                let else_ty = Self::expr_ty(body, else_expr);
                if then_ty != else_ty {
                    return Err(SmeltError::unsupported(
                        self.span(conditional.span.start, conditional.span.end),
                        "conditional expression branches must have the same lowered type",
                    ));
                }
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Conditional {
                        cond,
                        then_expr,
                        else_expr,
                    },
                    ty: then_ty,
                    span: self.span(conditional.span.start, conditional.span.end),
                }))
            }
            Expression::UnaryExpression(unary) => {
                let op = match unary.operator {
                    UnaryOperator::LogicalNot => UnaryOp::Not,
                    UnaryOperator::UnaryNegation => UnaryOp::Neg,
                    UnaryOperator::UnaryPlus => {
                        let operand = self.expression(&unary.argument, body)?;
                        let operand_ty = Self::expr_ty(body, operand);
                        if matches!(self.ctx.krate.types.get(operand_ty), Some(Type::Int | Type::Float)) {
                            return Ok(operand);
                        }
                        if self.is_date_constructor_arg_type(operand_ty) {
                            let ty = self.ctx.krate.types.intern(Type::Float);
                            return Ok(body.push_expr(Expr {
                                kind: ExprKind::PrimitiveCast {
                                    op: PrimitiveCastOp::ToFloat,
                                    operand,
                                },
                                ty,
                                span: self.span(unary.span.start, unary.span.end),
                            }));
                        }
                        return Err(SmeltError::unsupported(
                            self.span(unary.span.start, unary.span.end),
                            "unary plus requires a numeric or DateArg-compatible operand",
                        ));
                    }
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(unary.span.start, unary.span.end),
                            format!("unary operator is not lowered yet: {:?}", unary.operator),
                        ));
                    }
                };
                let operand = self.expression(&unary.argument, body)?;
                let ty = if matches!(op, UnaryOp::Not) {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    Self::expr_ty(body, operand)
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::UnaryOp { op, operand },
                    ty,
                    span: self.span(unary.span.start, unary.span.end),
                }))
            }
            Expression::AwaitExpression(await_expr) => {
                if !self.current_async {
                    return Err(SmeltError::unsupported(
                        self.span(await_expr.span.start, await_expr.span.end),
                        "await expressions are only lowered inside async functions",
                    ));
                }
                let awaited = self.expression(&await_expr.argument, body)?;
                let ty = self
                    .future_inner_type(Self::expr_ty(body, awaited))
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(await_expr.span.start, await_expr.span.end),
                            "await expressions require a Promise<T> operand",
                        )
                    })?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Await(awaited),
                    ty,
                    span: self.span(await_expr.span.start, await_expr.span.end),
                }))
            }
            Expression::StaticMemberExpression(member) => self.static_member(member, body),
            Expression::ComputedMemberExpression(member) => self.computed_member(member, body),
            Expression::CallExpression(call) => self.call_expression(call, body),
            Expression::ChainExpression(chain) => self.chain_expression(chain, body),
            Expression::TSAsExpression(as_expr) => self.type_assertion_expression(
                &as_expr.expression,
                &as_expr.type_annotation,
                as_expr.span,
                body,
            ),
            Expression::TSTypeAssertion(assertion) => self.type_assertion_expression(
                &assertion.expression,
                &assertion.type_annotation,
                assertion.span,
                body,
            ),
            Expression::TSSatisfiesExpression(satisfies) => {
                self.expression(&satisfies.expression, body)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.expression(&non_null.expression, body)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.expression_with_hint(&parenthesized.expression, body, type_hint)
            }
            Expression::NewExpression(new_expr) => {
                if let Some(expr) = self.set_constructor_expression(new_expr, body, type_hint)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.map_constructor_expression(new_expr, body, type_hint)? {
                    return Ok(expr);
                }
                let Expression::Identifier(callee) = &new_expr.callee else {
                    if let Some(expr) = self.dynamic_date_constructor_expression(new_expr, body)? {
                        return Ok(expr);
                    }
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "new expressions require a direct class name",
                    ));
                };
                if callee.name == "Date" {
                    return self.new_date_expression(new_expr, body);
                }
                if callee.name == "URL" {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "TypeScript URL is not supported yet; URL construction and URL field access need a URL mapping policy",
                    ));
                }
                let Some(item) = self.classes.get(callee.name.as_str()).copied() else {
                    return Err(SmeltError::unsupported(
                        self.span(callee.span.start, callee.span.end),
                        format!("unresolved class `{}`", callee.name),
                    ));
                };
                let Item::Class(class) = self.item_ref(item) else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "new expressions require a class item",
                    ));
                };
                let class_name = class.name;
                let args = new_expr
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                let ty = self.ctx.krate.types.intern(Type::Class {
                    name: class_name,
                    args: Vec::new(),
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: class_name,
                        args,
                    },
                    ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                }))
            }
            Expression::TemplateLiteral(tpl) => {
                let str_ty = self.ctx.krate.types.intern(Type::String);
                let span = self.span(tpl.span.start, tpl.span.end);

                // Build the first segment from the first quasi.
                let Some(first_quasi) = tpl.quasis.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(tpl.span.start, tpl.span.end),
                        "template literals must contain at least one quasi",
                    ));
                };
                let first_str = first_quasi
                    .value
                    .cooked
                    .as_ref()
                    .map_or_else(|| first_quasi.value.raw.as_str(), |c| c.as_str())
                    .to_owned();
                let mut acc = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(first_str)),
                    ty: str_ty,
                    span,
                });

                for (i, interp) in tpl.expressions.iter().enumerate() {
                    // Concatenate the interpolated expression
                    let part = self.expression(interp, body)?;
                    acc = body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Add,
                            lhs: acc,
                            rhs: part,
                        },
                        ty: str_ty,
                        span,
                    });
                    // Concatenate the next quasi string (skip empty ones to keep HIR tidy)
                    if let Some(quasi) = tpl.quasis.get(i.saturating_add(1)) {
                        let s = quasi
                            .value
                            .cooked
                            .as_ref()
                            .map_or_else(|| quasi.value.raw.as_str(), |c| c.as_str());
                        if !s.is_empty() {
                            let lit = body.push_expr(Expr {
                                kind: ExprKind::Literal(Literal::String(s.to_owned())),
                                ty: str_ty,
                                span,
                            });
                            acc = body.push_expr(Expr {
                                kind: ExprKind::BinOp {
                                    op: BinOp::Add,
                                    lhs: acc,
                                    rhs: lit,
                                },
                                ty: str_ty,
                                span,
                            });
                        }
                    }
                }
                Ok(acc)
            }
            Expression::TaggedTemplateExpression(tagged) => Err(SmeltError::unsupported(
                self.span(tagged.span.start, tagged.span.end),
                "tagged template literals are not supported",
            )),
            _ => Err(SmeltError::unsupported(
                self.expression_span(expression),
                format!("expression kind is not lowered yet: {expression:?}"),
            )),
        }
    }

    // Continued in the next split builder file.
}
