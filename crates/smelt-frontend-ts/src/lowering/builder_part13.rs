impl ModuleBuilder<'_> {
    fn map_projection_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "keys" => DictProjectionOp::Keys,
            "values" => DictProjectionOp::Values,
            "entries" => DictProjectionOp::Entries,
            _ => return Ok(None),
        };
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Map keys/values/entries require no arguments",
            ));
        }
        let dict = self.expression(&member.object, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(dict_key_ty, dict_value_ty)) = self.ctx.krate.types.get(dict_ty) else {
            return Ok(None);
        };
        let key_ty = *dict_key_ty;
        let value_ty = *dict_value_ty;
        let ty = match op {
            DictProjectionOp::Keys => self.ctx.krate.types.intern(Type::List(key_ty)),
            DictProjectionOp::Values => self.ctx.krate.types.intern(Type::List(value_ty)),
            DictProjectionOp::Entries => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![key_ty, value_ty]));
                self.ctx.krate.types.intern(Type::List(entry_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictProjection { op, dict },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Set` projection methods.
    fn set_projection_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "keys" | "values" => SetProjectionOp::Values,
            "entries" => SetProjectionOp::Entries,
            _ => return Ok(None),
        };
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Set keys/values/entries require no arguments",
            ));
        }
        let set = self.expression(&member.object, body)?;
        let set_ty = Self::expr_ty(body, set);
        let Some(Type::Set(set_item_ty)) = self.ctx.krate.types.get(set_ty) else {
            return Ok(None);
        };
        let item_ty = *set_item_ty;
        let ty = match op {
            SetProjectionOp::Values => self.ctx.krate.types.intern(Type::List(item_ty)),
            SetProjectionOp::Entries => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![item_ty, item_ty]));
                self.ctx.krate.types.intern(Type::List(entry_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::SetProjection { op, set },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.concat` for one same-typed array argument.
    fn list_concat_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "concat" {
            return Ok(None);
        }
        let [right_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array concat currently requires exactly one array argument",
            ));
        };
        let left = self.expression(&member.object, body)?;
        let right = self.argument(right_argument, body)?;
        let ty = Self::expr_ty(body, left);
        if self
            .ctx
            .krate
            .types
            .get(ty)
            .is_none_or(|ty| !matches!(ty, Type::List(_)))
            || Self::expr_ty(body, right) != ty
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array concat requires same-typed array receiver and argument",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListConcat { left, right },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower capture-free callback-heavy TypeScript array methods.
    fn list_callback_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "map" => ListCallbackOp::Map,
            "filter" => ListCallbackOp::Filter,
            "find" => ListCallbackOp::Find,
            "findIndex" => ListCallbackOp::FindIndex,
            "some" => ListCallbackOp::Some,
            "every" => ListCallbackOp::Every,
            "forEach" => ListCallbackOp::ForEach,
            _ => return Ok(None),
        };
        let [callback_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array callback methods require exactly one callback argument",
            ));
        };
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array callback method receiver must be an array",
            ));
        };
        let element_ty = *list_element_ty;
        let index_ty = self.ctx.krate.types.intern(Type::Float);
        let callback =
            self.capture_free_arrow_callback(callback_argument, &[element_ty, index_ty])?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let ty = match op {
            ListCallbackOp::Map => self.ctx.krate.types.intern(Type::List(callback.ty)),
            ListCallbackOp::Filter => {
                self.require_callback_ty(callback.ty, bool_ty, call, "array filter")?;
                list_ty
            }
            ListCallbackOp::Find => {
                self.require_callback_ty(callback.ty, bool_ty, call, "array find")?;
                self.ctx.krate.types.intern(Type::Optional(element_ty))
            }
            ListCallbackOp::FindIndex => {
                self.require_callback_ty(callback.ty, bool_ty, call, "array findIndex")?;
                self.ctx.krate.types.intern(Type::Float)
            }
            ListCallbackOp::FindLast => {
                self.require_callback_ty(callback.ty, bool_ty, call, "array findLast")?;
                self.ctx.krate.types.intern(Type::Optional(element_ty))
            }
            ListCallbackOp::FindLastIndex => {
                self.require_callback_ty(callback.ty, bool_ty, call, "array findLastIndex")?;
                self.ctx.krate.types.intern(Type::Float)
            }
            ListCallbackOp::Some => {
                self.require_callback_ty(callback.ty, bool_ty, call, "array some")?;
                bool_ty
            }
            ListCallbackOp::Every => {
                self.require_callback_ty(callback.ty, bool_ty, call, "array every")?;
                bool_ty
            }
            ListCallbackOp::ForEach => self.ctx.krate.types.intern(Type::None),
            ListCallbackOp::FlatMap => {
                let Some(Type::List(item_ty)) = self.ctx.krate.types.get(callback.ty) else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "array flatMap callback must return an array",
                    ));
                };
                self.ctx.krate.types.intern(Type::List(*item_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListCallback { op, list, callback },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `Array.prototype.reduce`, including element-typed calls without an initial value.
    fn list_reduce_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "reduce" {
            return Ok(None);
        }
        let ([callback_argument] | [callback_argument, _]) = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array reduce requires callback and at most one initial value",
            ));
        };
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array reduce receiver must be an array",
            ));
        };
        let element_ty = *list_element_ty;
        let initial = if let [_, initial_argument] = call.arguments.as_slice() {
            Some(self.argument(initial_argument, body)?)
        } else {
            None
        };
        let accumulator_ty = initial.map_or(element_ty, |initial| Self::expr_ty(body, initial));
        let index_ty = self.ctx.krate.types.intern(Type::Float);
        let callback = self.capture_free_arrow_callback(
            callback_argument,
            &[accumulator_ty, element_ty, index_ty],
        )?;
        self.require_callback_ty(callback.ty, accumulator_ty, call, "array reduce")?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListReduce {
                list,
                initial,
                callback,
            },
            ty: accumulator_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Validate the inferred callback return type for an array method.
    fn require_callback_ty(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
        call: &oxc::ast::ast::CallExpression<'_>,
        context: &'static str,
    ) -> Result<(), SmeltError> {
        if actual == expected {
            Ok(())
        } else {
            Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{context} callback returns an unsupported type"),
            ))
        }
    }

    /// Lower a simple capture-free arrow callback to a typed expression tree.
    fn capture_free_arrow_callback(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
    ) -> Result<CallbackExpr, SmeltError> {
        let Argument::ArrowFunctionExpression(arrow) = argument else {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "array callback methods currently require arrow function callbacks",
            ));
        };
        if arrow.r#async || arrow.type_parameters.is_some() || arrow.params.rest.is_some() {
            return Err(SmeltError::unsupported(
                self.span(arrow.span.start, arrow.span.end),
                "async, generic, and rest-parameter callbacks are not supported yet",
            ));
        }
        if arrow.params.items.is_empty() || arrow.params.items.len() > expected_param_tys.len() {
            return Err(SmeltError::unsupported(
                self.span(arrow.span.start, arrow.span.end),
                "array callback parameter count is not supported for this method",
            ));
        }
        let mut params = HashMap::new();
        for (index, param) in arrow.params.items.iter().enumerate() {
            let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "destructured callback parameters are not supported yet",
                ));
            };
            let Some(expected_ty) = expected_param_tys.get(index).copied() else {
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "array callback parameter count is not supported for this method",
                ));
            };
            params.insert(binding.name.as_str(), (index, expected_ty));
        }
        let expression = if arrow.expression {
            let [Statement::ExpressionStatement(statement)] = arrow.body.statements.as_slice()
            else {
                return Err(SmeltError::unsupported(
                    self.span(arrow.body.span.start, arrow.body.span.end),
                    "expression-bodied callbacks must contain one expression",
                ));
            };
            &statement.expression
        } else {
            let [Statement::ReturnStatement(statement)] = arrow.body.statements.as_slice() else {
                return Err(SmeltError::unsupported(
                    self.span(arrow.body.span.start, arrow.body.span.end),
                    "block-bodied callbacks currently require a single return statement",
                ));
            };
            statement.argument.as_ref().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(statement.span.start, statement.span.end),
                    "callback return statements must return a value",
                )
            })?
        };
        self.callback_expression(expression, &params)
    }

    /// Lower a supported callback expression and reject captures explicitly.
    fn callback_expression(
        &mut self,
        expression: &Expression<'_>,
        params: &HashMap<&str, (usize, smelt_hir::TypeId)>,
    ) -> Result<CallbackExpr, SmeltError> {
        match expression {
            Expression::Identifier(identifier) => {
                let Some((index, ty)) = params.get(identifier.name.as_str()).copied() else {
                    return Err(SmeltError::unsupported(
                        self.span(identifier.span.start, identifier.span.end),
                        "callback captures are not supported yet",
                    ));
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Param(index),
                    ty,
                })
            }
            Expression::NumericLiteral(literal) => Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::Float(literal.value)),
                ty: self.ctx.krate.types.intern(Type::Float),
            }),
            Expression::StringLiteral(literal) => Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::String(literal.value.to_string())),
                ty: self.ctx.krate.types.intern(Type::String),
            }),
            Expression::BooleanLiteral(literal) => Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::Bool(literal.value)),
                ty: self.ctx.krate.types.intern(Type::Bool),
            }),
            Expression::NullLiteral(_) => Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::None),
                ty: self.ctx.krate.types.intern(Type::None),
            }),
            Expression::ArrayExpression(array) => {
                let mut items = Vec::new();
                for element in &array.elements {
                    let expr = match element {
                        ArrayExpressionElement::SpreadElement(_) => {
                            return Err(SmeltError::unsupported(
                                self.span(element.span().start, element.span().end),
                                "callback array spread elements are not supported yet",
                            ));
                        }
                        ArrayExpressionElement::Elision(_) => {
                            return Err(SmeltError::unsupported(
                                self.span(element.span().start, element.span().end),
                                "callback array elisions are not supported",
                            ));
                        }
                        ArrayExpressionElement::NumericLiteral(literal) => CallbackExpr {
                            kind: CallbackExprKind::Literal(Literal::Float(literal.value)),
                            ty: self.ctx.krate.types.intern(Type::Float),
                        },
                        ArrayExpressionElement::StringLiteral(literal) => CallbackExpr {
                            kind: CallbackExprKind::Literal(Literal::String(
                                literal.value.to_string(),
                            )),
                            ty: self.ctx.krate.types.intern(Type::String),
                        },
                        ArrayExpressionElement::BooleanLiteral(literal) => CallbackExpr {
                            kind: CallbackExprKind::Literal(Literal::Bool(literal.value)),
                            ty: self.ctx.krate.types.intern(Type::Bool),
                        },
                        ArrayExpressionElement::Identifier(identifier) => {
                            let Some((index, ty)) = params.get(identifier.name.as_str()).copied()
                            else {
                                return Err(SmeltError::unsupported(
                                    self.span(identifier.span.start, identifier.span.end),
                                    "callback captures are not supported yet",
                                ));
                            };
                            CallbackExpr {
                                kind: CallbackExprKind::Param(index),
                                ty,
                            }
                        }
                        ArrayExpressionElement::BinaryExpression(binary) => {
                            let op = self.callback_binary_op(
                                binary.operator,
                                binary.span.start,
                                binary.span.end,
                            )?;
                            let lhs = self.callback_expression(&binary.left, params)?;
                            let rhs = self.callback_expression(&binary.right, params)?;
                            let ty = if matches!(
                                op,
                                BinOp::Eq
                                    | BinOp::NotEq
                                    | BinOp::Lt
                                    | BinOp::Lte
                                    | BinOp::Gt
                                    | BinOp::Gte
                            ) {
                                self.ctx.krate.types.intern(Type::Bool)
                            } else {
                                lhs.ty
                            };
                            CallbackExpr {
                                kind: CallbackExprKind::Binary {
                                    op,
                                    lhs: Box::new(lhs),
                                    rhs: Box::new(rhs),
                                },
                                ty,
                            }
                        }
                        _ => {
                            return Err(SmeltError::unsupported(
                                self.span(element.span().start, element.span().end),
                                "callback array element kind is not supported yet",
                            ));
                        }
                    };
                    items.push(expr);
                }
                let Some(first) = items.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "callback empty arrays require type context",
                    ));
                };
                let item_ty = first.ty;
                if !items.iter().all(|item| item.ty == item_ty) {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "callback array literal items must have one type",
                    ));
                }
                Ok(CallbackExpr {
                    kind: CallbackExprKind::ListLit(items),
                    ty: self.ctx.krate.types.intern(Type::List(item_ty)),
                })
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.callback_expression(&parenthesized.expression, params)
            }
            Expression::UnaryExpression(unary) => {
                let op = match unary.operator {
                    UnaryOperator::LogicalNot => UnaryOp::Not,
                    UnaryOperator::UnaryNegation => UnaryOp::Neg,
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(unary.span.start, unary.span.end),
                            format!(
                                "callback unary operator is not supported yet: {:?}",
                                unary.operator
                            ),
                        ));
                    }
                };
                let operand = self.callback_expression(&unary.argument, params)?;
                let ty = if matches!(op, UnaryOp::Not) {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    operand.ty
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Unary {
                        op,
                        operand: Box::new(operand),
                    },
                    ty,
                })
            }
            Expression::BinaryExpression(binary) => {
                let op =
                    self.callback_binary_op(binary.operator, binary.span.start, binary.span.end)?;
                let lhs = self.callback_expression(&binary.left, params)?;
                let rhs = self.callback_expression(&binary.right, params)?;
                let ty = if matches!(
                    op,
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte
                ) {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    lhs.ty
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty,
                })
            }
            Expression::LogicalExpression(logical) => {
                let op = match logical.operator {
                    LogicalOperator::And => BinOp::And,
                    LogicalOperator::Or => BinOp::Or,
                    LogicalOperator::Coalesce => {
                        return Err(SmeltError::unsupported(
                            self.span(logical.span.start, logical.span.end),
                            "callback nullish coalescing is not supported yet",
                        ));
                    }
                };
                let lhs = self.callback_expression(&logical.left, params)?;
                let rhs = self.callback_expression(&logical.right, params)?;
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty: self.ctx.krate.types.intern(Type::Bool),
                })
            }
            _ => Err(SmeltError::unsupported(
                self.expression_span(expression),
                "callback expression kind is not supported yet",
            )),
        }
    }

    /// Maps supported TypeScript callback binary operators to HIR operators.
    fn callback_binary_op(
        &self,
        operator: BinaryOperator,
        start: u32,
        end: u32,
    ) -> Result<BinOp, SmeltError> {
        match operator {
            BinaryOperator::Addition => Ok(BinOp::Add),
            BinaryOperator::Subtraction => Ok(BinOp::Sub),
            BinaryOperator::Multiplication => Ok(BinOp::Mul),
            BinaryOperator::Division => Ok(BinOp::Div),
            BinaryOperator::StrictEquality => Ok(BinOp::Eq),
            BinaryOperator::StrictInequality => Ok(BinOp::NotEq),
            BinaryOperator::LessThan => Ok(BinOp::Lt),
            BinaryOperator::LessEqualThan => Ok(BinOp::Lte),
            BinaryOperator::GreaterThan => Ok(BinOp::Gt),
            BinaryOperator::GreaterEqualThan => Ok(BinOp::Gte),
            _ => Err(SmeltError::unsupported(
                self.span(start, end),
                "callback binary operator is not supported yet",
            )),
        }
    }

    /// Lower direct TypeScript `Array.prototype.indexOf` and `lastIndexOf`.
    fn list_search_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "indexOf" => ListSearchOp::Find,
            "lastIndexOf" => ListSearchOp::RFind,
            _ => return Ok(None),
        };
        let [item_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf currently require exactly one item argument",
            ));
        };
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let item_ty = *element_ty;
        let item = self.argument(item_argument, body)?;
        if Self::expr_ty(body, item) != item_ty {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf argument must match the array element type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListSearch { op, list, item },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `.at(index)` on arrays and strings to Python-style HIR indexing.
    fn collection_at_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "at" {
            return Ok(None);
        }
        let [index_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array/string at requires exactly one numeric index",
            ));
        };
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let ty = match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(item_ty)) => *item_ty,
            Some(Type::String) => self.ctx.krate.types.intern(Type::String),
            _ => return Ok(None),
        };
        let index = self.argument(index_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, index)) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array/string at index must be a number",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Index { receiver, index },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    // Continued in the next split builder file.
}
