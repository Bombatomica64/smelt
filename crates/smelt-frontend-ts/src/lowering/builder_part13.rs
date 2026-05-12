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

    /// Lower callback-heavy TypeScript array methods.
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
        let callback = self.callback_argument(
            callback_argument,
            &[element_ty, index_ty, list_ty],
            "array callback",
            body,
        )?;
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let ty = match op {
            ListCallbackOp::Map => self.ctx.krate.types.intern(Type::List(callback.return_ty)),
            ListCallbackOp::Filter => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array filter")?;
                list_ty
            }
            ListCallbackOp::Find => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array find")?;
                self.ctx.krate.types.intern(Type::Optional(element_ty))
            }
            ListCallbackOp::FindIndex => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array findIndex")?;
                self.ctx.krate.types.intern(Type::Float)
            }
            ListCallbackOp::FindLast => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array findLast")?;
                self.ctx.krate.types.intern(Type::Optional(element_ty))
            }
            ListCallbackOp::FindLastIndex => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array findLastIndex")?;
                self.ctx.krate.types.intern(Type::Float)
            }
            ListCallbackOp::Some => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array some")?;
                bool_ty
            }
            ListCallbackOp::Every => {
                self.require_callback_ty(callback.return_ty, bool_ty, call, "array every")?;
                bool_ty
            }
            ListCallbackOp::ForEach => self.ctx.krate.types.intern(Type::None),
            ListCallbackOp::FlatMap => {
                let Some(Type::List(item_ty)) = self.ctx.krate.types.get(callback.return_ty) else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "array flatMap callback must return an array",
                    ));
                };
                self.ctx.krate.types.intern(Type::List(*item_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListCallback {
                op,
                list,
                callback: callback.expr,
            },
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
        let callback = self.callback_argument(
            callback_argument,
            &[accumulator_ty, element_ty, index_ty, list_ty],
            "array reduce",
            body,
        )?;
        self.require_callback_ty(callback.return_ty, accumulator_ty, call, "array reduce")?;
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListReduce {
                list,
                initial,
                callback: callback.expr,
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

    /// Store a lowered callback expression as a first-class closure expression.
    fn callback_expr_to_closure(
        &mut self,
        callback: CallbackExpr,
        params: &[smelt_hir::TypeId],
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        self.callback_expr_to_closure_with_return_ty(callback.ty, callback, params, span, body)
    }

    /// Store a lowered callback expression as a closure with an explicit return type.
    fn callback_expr_to_closure_with_return_ty(
        &mut self,
        return_ty: smelt_hir::TypeId,
        callback: CallbackExpr,
        params: &[smelt_hir::TypeId],
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let mut closure_body = Body::new(None, span);
        let closure_params = params
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                let name = self
                    .ctx
                    .krate
                    .symbols
                    .intern(&format!("__callback_param_{index}"));
                let local = closure_body.push_local(LocalDecl {
                    name: Some(name),
                    ty: *ty,
                    mutable: false,
                    span,
                });
                closure_body.params.push(local);
                Param {
                    name,
                    local,
                    ty: *ty,
                    span,
                }
            })
            .collect::<Vec<_>>();
        let body_id = self.ctx.krate.push_body(closure_body);
        let captures = self.callback_captures(&callback, body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: params.to_vec(),
            return_ty,
            is_async: false,
        }));
        body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                return_ty,
                captures,
                body: body_id,
                callback_body: Some(callback),
                span,
            }),
            ty: closure_ty,
            span,
        })
    }

    /// Collect explicit captures from a callback expression tree.
    fn callback_captures(&mut self, callback: &CallbackExpr, body: &Body) -> Vec<ClosureCapture> {
        let mut captures = HashMap::new();
        self.collect_callback_captures(callback, body, &mut captures);
        captures.into_values().collect()
    }

    /// Recursively collect captures and upgrade assigned captures to mutable mode.
    fn collect_callback_captures(
        &mut self,
        callback: &CallbackExpr,
        body: &Body,
        captures: &mut HashMap<smelt_hir::LocalId, ClosureCapture>,
    ) {
        match &callback.kind {
            CallbackExprKind::Capture(local) => {
                if let Some(local_decl) = body.locals.get(local.0 as usize) {
                    captures.entry(*local).or_insert_with(|| ClosureCapture {
                        source_local: *local,
                        symbol: local_decl
                            .name
                            .unwrap_or_else(|| self.ctx.krate.symbols.intern("__capture")),
                        ty: local_decl.ty,
                        mode: CaptureMode::ByRef,
                    });
                }
            }
            CallbackExprKind::AssignCapture { target, value } => {
                if let Some(local_decl) = body.locals.get(target.0 as usize) {
                    captures.insert(
                        *target,
                        ClosureCapture {
                            source_local: *target,
                            symbol: local_decl
                                .name
                                .unwrap_or_else(|| self.ctx.krate.symbols.intern("__capture")),
                            ty: local_decl.ty,
                            mode: CaptureMode::ByMut,
                        },
                    );
                }
                self.collect_callback_captures(value, body, captures);
            }
            CallbackExprKind::ListLit(items) => {
                for item in items {
                    self.collect_callback_captures(item, body, captures);
                }
            }
            CallbackExprKind::Index { receiver, .. } | CallbackExprKind::Field { receiver, .. } => {
                self.collect_callback_captures(receiver, body, captures);
            }
            CallbackExprKind::Unary { operand, .. } => {
                self.collect_callback_captures(operand, body, captures);
            }
            CallbackExprKind::Binary { lhs, rhs, .. } => {
                self.collect_callback_captures(lhs, body, captures);
                self.collect_callback_captures(rhs, body, captures);
            }
            CallbackExprKind::Param(_) | CallbackExprKind::Literal(_) => {}
        }
    }

    /// Lower a supported arrow callback to a typed expression tree.
    fn arrow_callback(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        let Argument::ArrowFunctionExpression(arrow) = argument else {
            return Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                "array callback methods currently require arrow function callbacks",
            ));
        };
        if arrow.r#async {
            return Err(SmeltError::unsupported(
                self.span(arrow.span.start, arrow.span.end),
                "async callbacks need closure-body lowering",
            ));
        }
        self.arrow_callback_from_params(arrow, expected_param_tys, body)
    }

    /// Lower an arrow callback after the expected parameter types are known.
    fn arrow_callback_from_params(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        if (arrow.params.items.is_empty() && arrow.params.rest.is_none())
            || arrow.params.items.len() > expected_param_tys.len()
        {
            return Err(SmeltError::unsupported(
                self.span(arrow.span.start, arrow.span.end),
                "array callback parameter count is not supported for this method",
            ));
        }
        let mut params = HashMap::new();
        for (index, param) in arrow.params.items.iter().enumerate() {
            let Some(expected_ty) = expected_param_tys.get(index).copied() else {
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "array callback parameter count is not supported for this method",
                ));
            };
            self.bind_callback_param_pattern(&param.pattern, index, expected_ty, &mut params)?;
        }
        if let Some(rest) = &arrow.params.rest {
            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest callback parameters need closure-body lowering",
                ));
            };
            let rest_index = arrow.params.items.len();
            let rest_ty = expected_param_tys.get(rest_index).copied().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest callback parameter has no expected input type",
                )
            })?;
            if matches!(self.ctx.krate.types.get(rest_ty), Some(Type::List(_)))
                && expected_param_tys.len() == rest_index + 1
            {
                params.insert(
                    binding.name.as_str(),
                    CallbackExpr {
                        kind: CallbackExprKind::Param(rest_index),
                        ty: rest_ty,
                    },
                );
            } else {
                let rest_items = expected_param_tys
                    .iter()
                    .enumerate()
                    .skip(rest_index)
                    .map(|(index, ty)| CallbackExpr {
                        kind: CallbackExprKind::Param(index),
                        ty: *ty,
                    })
                    .collect::<Vec<_>>();
                let item_ty = rest
                    .type_annotation
                    .as_ref()
                    .and_then(|annotation| {
                        let list_ty = self.ts_type_to_hir(&annotation.type_annotation).ok()?;
                        match self.ctx.krate.types.get(list_ty) {
                            Some(Type::List(item_ty)) => Some(*item_ty),
                            _ => None,
                        }
                    })
                    .or_else(|| rest_items.first().map(|item| item.ty))
                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                params.insert(
                    binding.name.as_str(),
                    CallbackExpr {
                        kind: CallbackExprKind::ListLit(rest_items),
                        ty: self.ctx.krate.types.intern(Type::List(item_ty)),
                    },
                );
            }
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
        self.callback_expression(expression, &params, body)
    }

    /// Bind names from a callback parameter pattern to callback expressions.
    fn bind_callback_param_pattern<'a>(
        &mut self,
        pattern: &'a BindingPattern<'a>,
        param_index: usize,
        param_ty: smelt_hir::TypeId,
        params: &mut HashMap<&'a str, CallbackExpr>,
    ) -> Result<(), SmeltError> {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                params.insert(
                    binding.name.as_str(),
                    CallbackExpr {
                        kind: CallbackExprKind::Param(param_index),
                        ty: param_ty,
                    },
                );
                Ok(())
            }
            BindingPattern::ArrayPattern(array) => {
                let item_tys = match self.ctx.krate.types.get(param_ty) {
                    Some(Type::Tuple(items)) => items.clone(),
                    Some(Type::List(item)) => vec![*item; array.elements.len()],
                    _ => Vec::new(),
                };
                for (item_index, element) in array.elements.iter().enumerate() {
                    let Some(element_pattern) = element else {
                        continue;
                    };
                    let item_ty = item_tys.get(item_index).copied().unwrap_or(param_ty);
                    let BindingPattern::BindingIdentifier(binding) = element_pattern else {
                        return Err(SmeltError::unsupported(
                            self.span(element_pattern.span().start, element_pattern.span().end),
                            "nested callback parameter destructuring needs closure-body lowering",
                        ));
                    };
                    params.insert(
                        binding.name.as_str(),
                        CallbackExpr {
                            kind: CallbackExprKind::Index {
                                receiver: Box::new(CallbackExpr {
                                    kind: CallbackExprKind::Param(param_index),
                                    ty: param_ty,
                                }),
                                index: item_index,
                            },
                            ty: item_ty,
                        },
                    );
                }
                Ok(())
            }
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    if property.computed {
                        return Err(SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "computed callback parameter destructuring needs closure-body lowering",
                        ));
                    }
                    let field_text = match &property.key {
                        PropertyKey::StaticIdentifier(identifier) => identifier.name.as_str(),
                        PropertyKey::StringLiteral(literal) => literal.value.as_str(),
                        _ => {
                            return Err(SmeltError::unsupported(
                                self.span(property.key.span().start, property.key.span().end),
                                "dynamic callback parameter destructuring needs closure-body lowering",
                            ));
                        }
                    };
                    let BindingPattern::BindingIdentifier(binding) = &property.value else {
                        return Err(SmeltError::unsupported(
                            self.span(property.value.span().start, property.value.span().end),
                            "nested callback parameter destructuring needs closure-body lowering",
                        ));
                    };
                    let field = self.intern_source_name(field_text);
                    let field_ty = match self.ctx.krate.types.get(param_ty) {
                        Some(Type::Dict(_, value)) => *value,
                        Some(Type::Class { .. }) => self.class_field_type(param_ty, field)?,
                        _ => param_ty,
                    };
                    params.insert(
                        binding.name.as_str(),
                        CallbackExpr {
                            kind: CallbackExprKind::Field {
                                receiver: Box::new(CallbackExpr {
                                    kind: CallbackExprKind::Param(param_index),
                                    ty: param_ty,
                                }),
                                field,
                            },
                            ty: field_ty,
                        },
                    );
                }
                Ok(())
            }
            BindingPattern::AssignmentPattern(_) => Err(SmeltError::unsupported(
                self.span(pattern.span().start, pattern.span().end),
                "default callback parameter destructuring needs closure-body lowering",
            )),
        }
    }

    /// Read explicit parameter types from a local arrow function callback.
    fn arrow_callback_param_types(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
    ) -> Result<Vec<smelt_hir::TypeId>, SmeltError> {
        let mut params = Vec::new();
        for param in &arrow.params.items {
            let BindingPattern::BindingIdentifier(_) = &param.pattern else {
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "destructured closure parameters are not supported yet",
                ));
            };
            let ty = param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "local closure parameters must have explicit type annotations",
                    )
                })?;
            params.push(ty);
        }
        if let Some(rest) = &arrow.params.rest {
            let BindingPattern::BindingIdentifier(_) = &rest.rest.argument else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest closure parameters need closure-body lowering",
                ));
            };
            let ty = rest
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(rest.span.start, rest.span.end),
                        "rest closure parameters must have explicit array type annotations",
                    )
                })?;
            if !matches!(self.ctx.krate.types.get(ty), Some(Type::List(_))) {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest closure parameter type must be an array type",
                ));
            }
            params.push(ty);
        }
        Ok(params)
    }

    /// Lower either an inline arrow callback or a local closure callback value.
    fn callback_argument(
        &mut self,
        argument: &Argument<'_>,
        expected_param_tys: &[smelt_hir::TypeId],
        context: &'static str,
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        if let Argument::Identifier(identifier) = argument {
            let Some(callback) = self.local_callbacks.get(identifier.name.as_str()).cloned() else {
                return Err(SmeltError::unsupported(
                    self.span(identifier.span.start, identifier.span.end),
                    format!("{context} local callback `{}` is not defined", identifier.name),
                ));
            };
            if callback.params.is_empty() || callback.params.len() > expected_param_tys.len() {
                return Err(SmeltError::unsupported(
                    self.span(identifier.span.start, identifier.span.end),
                    format!("{context} local callback parameter count is not supported"),
                ));
            }
            for (actual, expected) in callback.params.iter().zip(expected_param_tys) {
                if actual != expected {
                    return Err(SmeltError::unsupported(
                        self.span(identifier.span.start, identifier.span.end),
                        format!("{context} local callback parameter type does not match receiver"),
                    ));
                }
            }
            if callback.callback.ty != callback.return_ty {
                return Err(SmeltError::unsupported(
                    self.span(identifier.span.start, identifier.span.end),
                    format!("{context} local callback return type is inconsistent"),
                ));
            }
            let expr = self.callback_expr_to_closure_with_return_ty(
                callback.return_ty,
                callback.callback,
                &callback.params,
                self.span(identifier.span.start, identifier.span.end),
                body,
            );
            return Ok(ClosureCallback {
                expr,
                return_ty: callback.return_ty,
            });
        }
        let callback = self.arrow_callback(argument, expected_param_tys, body)?;
        let return_ty = callback.ty;
        let expr = self.callback_expr_to_closure(
            callback,
            expected_param_tys,
            self.span(argument.span().start, argument.span().end),
            body,
        );
        Ok(ClosureCallback { expr, return_ty })
    }

    /// Lower a supported callback expression.
    fn callback_expression(
        &mut self,
        expression: &Expression<'_>,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        match expression {
            Expression::Identifier(identifier) => {
                if let Some(param) = params.get(identifier.name.as_str()).cloned() {
                    return Ok(param);
                }
                let Some(local) = self.locals.get(identifier.name.as_str()).copied() else {
                    return Err(SmeltError::unsupported(
                        self.span(identifier.span.start, identifier.span.end),
                        format!("unresolved callback identifier `{}`", identifier.name),
                    ));
                };
                let ty = Self::local_ty(body, local);
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Capture(local),
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
                            if let Some(param) = params.get(identifier.name.as_str()).cloned() {
                                param
                            } else if let Some(local) =
                                self.locals.get(identifier.name.as_str()).copied()
                            {
                                let ty = Self::local_ty(body, local);
                                CallbackExpr {
                                    kind: CallbackExprKind::Capture(local),
                                    ty,
                                }
                            } else {
                                return Err(SmeltError::unsupported(
                                    self.span(identifier.span.start, identifier.span.end),
                                    format!(
                                        "unresolved callback identifier `{}`",
                                        identifier.name
                                    ),
                                ));
                            }
                        }
                        ArrayExpressionElement::BinaryExpression(binary) => {
                            let op = self.callback_binary_op(
                                binary.operator,
                                binary.span.start,
                                binary.span.end,
                            )?;
                            let lhs = self.callback_expression(&binary.left, params, body)?;
                            let rhs = self.callback_expression(&binary.right, params, body)?;
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
                self.callback_expression(&parenthesized.expression, params, body)
            }
            Expression::ComputedMemberExpression(member) => {
                let receiver = self.callback_expression(&member.object, params, body)?;
                let Expression::NumericLiteral(index) = &member.expression else {
                    return Err(SmeltError::unsupported(
                        self.span(member.span.start, member.span.end),
                        "callback computed access needs a static numeric index",
                    ));
                };
                if index.value.fract() != 0.0 || index.value < 0.0 {
                    return Err(SmeltError::unsupported(
                        self.span(index.span.start, index.span.end),
                        "callback computed access index must be a non-negative integer",
                    ));
                }
                let index_usize =
                    index
                        .value
                        .to_string()
                        .parse::<usize>()
                        .map_err(|err| {
                            SmeltError::unsupported(
                                self.span(index.span.start, index.span.end),
                                format!("callback computed access index is invalid: {err}"),
                            )
                        })?;
                let item_ty = match self.ctx.krate.types.get(receiver.ty) {
                    Some(Type::Tuple(items)) => items
                        .get(index_usize)
                        .copied()
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(member.span.start, member.span.end),
                                "callback tuple index is out of bounds",
                            )
                        })?,
                    Some(Type::List(item_ty)) => *item_ty,
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(member.span.start, member.span.end),
                            "callback computed access receiver must be a tuple or array",
                        ));
                    }
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Index {
                        receiver: Box::new(receiver),
                        index: index_usize,
                    },
                    ty: item_ty,
                })
            }
            Expression::AssignmentExpression(assign) => {
                let AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left else {
                    return Err(SmeltError::unsupported(
                        self.span(assign.span.start, assign.span.end),
                        "callback assignment targets must be captured locals",
                    ));
                };
                if params.contains_key(target.name.as_str()) {
                    return Err(SmeltError::unsupported(
                        self.span(target.span.start, target.span.end),
                        "callback parameter assignment is not supported yet",
                    ));
                }
                let Some(local) = self.locals.get(target.name.as_str()).copied() else {
                    return Err(SmeltError::unsupported(
                        self.span(target.span.start, target.span.end),
                        format!("unresolved callback assignment target `{}`", target.name),
                    ));
                };
                let local_index = usize::try_from(local.0).map_err(|err| {
                    SmeltError::unsupported(
                        self.span(target.span.start, target.span.end),
                        format!("callback assignment target index is invalid: {err}"),
                    )
                })?;
                let local_decl = body.locals.get(local_index).ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(target.span.start, target.span.end),
                        "callback assignment target does not resolve to a local",
                    )
                })?;
                if !local_decl.mutable {
                    return Err(SmeltError::unsupported(
                        self.span(target.span.start, target.span.end),
                        "callback assignment to captured const local is not supported",
                    ));
                }
                let right = self.callback_expression(&assign.right, params, body)?;
                let value = match assign.operator {
                    AssignmentOperator::Assign => right,
                    AssignmentOperator::Addition
                    | AssignmentOperator::Subtraction
                    | AssignmentOperator::Multiplication
                    | AssignmentOperator::Division => {
                        let op = match assign.operator {
                            AssignmentOperator::Addition => BinOp::Add,
                            AssignmentOperator::Subtraction => BinOp::Sub,
                            AssignmentOperator::Multiplication => BinOp::Mul,
                            AssignmentOperator::Division => BinOp::Div,
                            other => {
                                return Err(SmeltError::unsupported(
                                    self.span(assign.span.start, assign.span.end),
                                    format!(
                                        "callback assignment operator is not supported yet: {other:?}"
                                    ),
                                ));
                            }
                        };
                        CallbackExpr {
                            kind: CallbackExprKind::Binary {
                                op,
                                lhs: Box::new(CallbackExpr {
                                    kind: CallbackExprKind::Capture(local),
                                    ty: local_decl.ty,
                                }),
                                rhs: Box::new(right),
                            },
                            ty: local_decl.ty,
                        }
                    }
                    other => {
                        return Err(SmeltError::unsupported(
                            self.span(assign.span.start, assign.span.end),
                            format!(
                                "callback assignment operator is not supported yet: {other:?}"
                            ),
                        ));
                    }
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::AssignCapture {
                        target: local,
                        value: Box::new(value),
                    },
                    ty: local_decl.ty,
                })
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
                let operand = self.callback_expression(&unary.argument, params, body)?;
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
                let lhs = self.callback_expression(&binary.left, params, body)?;
                let rhs = self.callback_expression(&binary.right, params, body)?;
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
                let lhs = self.callback_expression(&logical.left, params, body)?;
                let rhs = self.callback_expression(&logical.right, params, body)?;
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
            BinaryOperator::Remainder => Ok(BinOp::Rem),
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
