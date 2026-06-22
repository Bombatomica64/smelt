/// Function pointer shape for builtin call lowering registry entries.
///
/// The builder lifetime is supplied by the impl block that owns the registry.
type BuiltinCallHandler<'builder> = fn(
    &mut ModuleBuilder<'builder>,
    &oxc::ast::ast::CallExpression<'_>,
    &mut Body,
) -> Result<Option<smelt_hir::ExprId>, SmeltError>;

impl<'builder> ModuleBuilder<'builder> {
    /// Lower call expressions, including stdlib shims and direct function/method invokes.
    fn call_expression(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(expr) = self.dispatch_builtin_call(call, body)? {
            return Ok(expr);
        }
        if let Expression::ComputedMemberExpression(member) = &call.callee {
            let args = call
                .arguments
                .iter()
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            let callee = self.computed_member(member, body)?;
            let callee_ty = Self::expr_ty(body, callee);
            if let Some(Type::Function(function)) = self.ctx.krate.types.get(callee_ty).cloned() {
                if args.len() < function.params.len() {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "computed member call argument count does not match function",
                    ));
                }
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall {
                        callee,
                        args: args.into_iter().take(function.params.len()).collect(),
                    },
                    ty: function.return_ty,
                    span: self.span(call.span.start, call.span.end),
                }));
            }
            if matches!(self.ctx.krate.types.get(callee_ty), Some(Type::Unknown)) {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(call.span.start, call.span.end),
                }));
            }
            if let Some(first) = args.first().copied() {
                return Ok(first);
            }
            return Err(SmeltError::unsupported(
                self.span(member.span.start, member.span.end),
                "computed member calls require at least one argument",
            ));
        }
        if let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::Identifier(object) = &member.object
            && object.name == "console"
            && matches!(member.property.name.as_str(), "log" | "warn" | "error")
        {
            let mut args = Vec::new();
            for arg in &call.arguments {
                args.push(self.argument(arg, body)?);
            }
            let ty = self.ctx.krate.types.intern(Type::None);
            let callee_item =
                self.ensure_console_log_item(self.span(member.span.start, member.span.end));
            let callee = body.push_expr(Expr {
                kind: ExprKind::Item(callee_item),
                ty,
                span: self.span(member.span.start, member.span.end),
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Call { callee, args },
                ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        if let Some(expr) = self.callable_static_member_call(call, body)? {
            return Ok(expr);
        }
        if let Expression::StaticMemberExpression(member) = &call.callee {
            if member.property.name == "next" && call.arguments.is_empty() {
                let receiver = self.expression(&member.object, body)?;
                let receiver_ty = Self::expr_ty(body, receiver);
                if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(receiver_ty) {
                    let ty = self.ctx.krate.types.intern(Type::Optional(*item_ty));
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::ListNext { list: receiver },
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                }
            }
            let receiver = self.expression(&member.object, body)?;
            let method = self.intern_source_name(member.property.name.as_str());
            let receiver_ty = Self::expr_ty(body, receiver);
            let optional_access = call.optional
                || member.optional
                || matches!(
                    self.ctx.krate.types.get(receiver_ty),
                    Some(Type::Optional(_))
                );
            let access_receiver_ty = self.optional_receiver_inner_type(receiver_ty);
            let (return_ty, method_item) =
                self.resolve_method(access_receiver_ty, method, member.span)?;
            let mut args = Vec::new();
            for arg in &call.arguments {
                if (member.property.name == "test"
                    || self.ctx.krate.types.get(return_ty) == Some(&Type::Unknown))
                    && matches!(
                        arg,
                        Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_)
                    )
                {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    args.push(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(arg.span().start, arg.span().end),
                    }));
                } else {
                    args.push(self.argument(arg, body)?);
                }
            }
            if method_item.0 == u32::MAX
                && self.ctx.krate.types.get(return_ty) == Some(&Type::Unknown)
            {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(call.span.start, call.span.end),
                }));
            }
            if optional_access {
                let receiver = if call.optional || member.optional {
                    self.optionalize_index_receiver(receiver, body)
                } else {
                    receiver
                };
                let ty = self.optional_chain_result_type(return_ty);
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::OptionalMethod {
                        receiver,
                        method,
                        args,
                    },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                }));
            }
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Method {
                    receiver,
                    method,
                    args,
                },
                ty: return_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        if let Some(expr) = self.local_callable_call(call, body)? {
            return Ok(expr);
        }
        if let Expression::CallExpression(callee_call) = &call.callee {
            if let Some(expr) = self.slice_string_nested_call(callee_call, call, body)? {
                return Ok(expr);
            }
            if let Some(expr) = self.partial_bind_nested_call(callee_call, call, body)? {
                return Ok(expr);
            }
            let callee = self.call_expression(callee_call, body)?;
            let callee_ty = Self::expr_ty(body, callee);
            let Some(function_ty) =
                self.function_member_type_for_arg_count(callee_ty, Some(call.arguments.len()))
            else {
                if self.ctx.krate.types.get(callee_ty) == Some(&Type::Unknown) {
                    for arg in &call.arguments {
                        let _ = self.argument(arg, body)?;
                    }
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                }
                return Err(SmeltError::unsupported(
                    self.span(callee_call.span.start, callee_call.span.end),
                    "call expression callee must return a function",
                ));
            };
            let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned()
            else {
                return Err(SmeltError::unsupported(
                    self.span(callee_call.span.start, callee_call.span.end),
                    "call expression callee must return a function",
                ));
            };
            let callee = if callee_ty == function_ty {
                callee
            } else {
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: callee },
                    ty: function_ty,
                    span: self.span(callee_call.span.start, callee_call.span.end),
                })
            };
            if call.arguments.len() < function.params.len() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "curried call argument count does not match selected overload",
                ));
            }
            let args = call
                .arguments
                .iter()
                .take(function.params.len())
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: function.return_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        let asserted_callee = Self::transparent_asserted_callee(&call.callee);
        if asserted_callee.is_some() {
            let callee_expression = asserted_callee.unwrap_or(&call.callee);
            let callee = self.expression(callee_expression, body)?;
            let callee_ty = Self::expr_ty(body, callee);
            let Some(Type::Function(function)) = self.ctx.krate.types.get(callee_ty).cloned()
            else {
                if self.erased_or_union_surface(callee_ty) {
                    for arg in &call.arguments {
                        let _ = self.argument(arg, body)?;
                    }
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                }
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "asserted call callee must be a function",
                ));
            };
            let args = call
                .arguments
                .iter()
                .take(function.params.len())
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: function.return_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        if let Expression::Identifier(callee_ident) = &call.callee {
            if matches!(
                callee_ident.name.as_str(),
                "Error"
                    | "EvalError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "TypeError"
                    | "URIError"
                    | "AggregateError"
            ) {
                return self.error_function_call(call, body);
            }
            if self.locals.contains_key(callee_ident.name.as_str()) {
                let callee = self.identifier_expression(
                    callee_ident.name.as_str(),
                    callee_ident.span.start,
                    callee_ident.span.end,
                    body,
                )?;
                let callee_ty = Self::expr_ty(body, callee);
                let callable_ty = match self.ctx.krate.types.get(callee_ty) {
                    Some(Type::Optional(inner)) => *inner,
                    _ => callee_ty,
                };
                let Some(Type::Function(function)) = self.ctx.krate.types.get(callable_ty).cloned()
                else {
                    if self.erased_or_union_surface(callee_ty) {
                        if self.call_has_spread_arguments_or_source_spread(call) {
                            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                            let args = self.packed_spread_call_arguments(unknown_ty, call, body)?;
                            let callee = if self.ctx.krate.types.get(callee_ty)
                                == Some(&Type::Unknown)
                            {
                                callee
                            } else {
                                body.push_expr(Expr {
                                    kind: ExprKind::TypeAssert { value: callee },
                                    ty: unknown_ty,
                                    span: self.span(callee_ident.span.start, callee_ident.span.end),
                                })
                            };
                            return Ok(body.push_expr(Expr {
                                kind: ExprKind::ClosureCallSpread { callee, args },
                                ty: unknown_ty,
                                span: self.span(call.span.start, call.span.end),
                            }));
                        }
                        let args = call
                            .arguments
                            .iter()
                            .map(|arg| self.argument(arg, body))
                            .collect::<Result<Vec<_>, _>>()?;
                        let ty = self.ctx.krate.types.intern(Type::Unknown);
                        let callee = if self.ctx.krate.types.get(callee_ty) == Some(&Type::Unknown)
                        {
                            callee
                        } else {
                            body.push_expr(Expr {
                                kind: ExprKind::TypeAssert { value: callee },
                                ty,
                                span: self.span(callee_ident.span.start, callee_ident.span.end),
                            })
                        };
                        return Ok(body.push_expr(Expr {
                            kind: ExprKind::ClosureCall { callee, args },
                            ty,
                            span: self.span(call.span.start, call.span.end),
                        }));
                    }
                    return Err(SmeltError::unsupported(
                        self.span(callee_ident.span.start, callee_ident.span.end),
                        format!(
                            "local `{}` is not callable ({:?})",
                            callee_ident.name,
                            self.ctx.krate.types.get(Self::expr_ty(body, callee))
                        ),
                    ));
                };
                let mut args = Vec::new();
                for arg in &call.arguments {
                    args.push(self.argument(arg, body)?);
                }
                if function
                    .params
                    .iter()
                    .any(|param| self.concrete_type_requires_never_value(*param))
                    && !Self::call_has_spread_arguments(call)
                {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "calls through function types with never parameters are not lowered",
                    ));
                }
                if self.call_has_spread_arguments_or_source_spread(call) {
                    let item_ty = function
                        .rest
                        .and_then(|index| function.params.get(index).copied())
                        .or_else(|| function.params.first().copied())
                        .and_then(|param| match self.ctx.krate.types.get(param) {
                            Some(Type::List(item_ty)) => Some(*item_ty),
                            _ => None,
                        })
                        .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                    let spread_args = self.packed_spread_call_arguments(item_ty, call, body)?;
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::ClosureCallSpread {
                            callee,
                            args: spread_args,
                        },
                        ty: function.return_ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                }
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall { callee, args },
                    ty: function.return_ty,
                    span: self.span(call.span.start, call.span.end),
                }));
            }
            let Some(item) = self.items.get(callee_ident.name.as_str()).copied() else {
                if let Some(callee_ty) =
                    self.module_globals.get(callee_ident.name.as_str()).copied()
                {
                    let args = call
                        .arguments
                        .iter()
                        .map(|arg| self.argument(arg, body))
                        .collect::<Result<Vec<_>, _>>()?;
                    if let Some(Type::Function(function)) =
                        self.ctx.krate.types.get(callee_ty).cloned()
                    {
                        let callee = self.identifier_expression(
                            callee_ident.name.as_str(),
                            callee_ident.span.start,
                            callee_ident.span.end,
                            body,
                        )?;
                        return Ok(body.push_expr(Expr {
                            kind: ExprKind::ClosureCall { callee, args },
                            ty: function.return_ty,
                            span: self.span(call.span.start, call.span.end),
                        }));
                    }
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                }
                if self.value_imports.contains(callee_ident.name.as_str()) {
                    for arg in &call.arguments {
                        let _ = self.argument(arg, body)?;
                    }
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                }
                if self.source_contains_forward_callable(callee_ident.name.as_str()) {
                    for arg in &call.arguments {
                        let _ = self.argument(arg, body)?;
                    }
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                }
                return Err(SmeltError::unsupported(
                    self.span(callee_ident.span.start, callee_ident.span.end),
                    format!("unresolved function `{}`", callee_ident.name),
                ));
            };
            let (params, item_rest, item_required_params, implementation_return_ty, is_async) =
                if let Item::Function(function) = self.item_ref(item) {
                    (
                        function
                            .params
                            .iter()
                            .map(|param| param.ty)
                            .collect::<Vec<_>>(),
                        function.rest,
                        function.required_params,
                        function.return_ty,
                        function.is_async,
                    )
                } else if self.value_imports.contains(callee_ident.name.as_str()) {
                    for arg in &call.arguments {
                        let _ = self.argument(arg, body)?;
                    }
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                } else {
                    let item_ty = self
                        .item_expr_type(
                            item,
                            self.span(callee_ident.span.start, callee_ident.span.end),
                        )
                        .unwrap_or_else(|_| self.ctx.krate.types.intern(Type::Unknown));
                    if let Some(Type::Function(function)) =
                        self.ctx.krate.types.get(item_ty).cloned()
                    {
                        let callee = body.push_expr(Expr {
                            kind: ExprKind::Literal(Literal::None),
                            ty: item_ty,
                            span: self.span(callee_ident.span.start, callee_ident.span.end),
                        });
                        let args = call
                            .arguments
                            .iter()
                            .map(|arg| self.argument(arg, body))
                            .collect::<Result<Vec<_>, _>>()?;
                        return Ok(body.push_expr(Expr {
                            kind: ExprKind::ClosureCall { callee, args },
                            ty: function.return_ty,
                            span: self.span(call.span.start, call.span.end),
                        }));
                    }
                    for arg in &call.arguments {
                        let _ = self.argument(arg, body)?;
                    }
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::None),
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                };
            let mut rest = self.function_rests.get(callee_ident.name.as_str()).copied();
            if rest.is_none()
                && let Some(index) = item_rest
                && let Some(Type::List(item_ty)) = params
                    .get(index)
                    .and_then(|param| self.ctx.krate.types.get(*param))
            {
                rest = Some(RestParam {
                    index,
                    item_ty: *item_ty,
                });
            }
            let selected_overload = self.selected_overload_signature(
                callee_ident.name.as_str(),
                &call.arguments,
                call.span,
                body,
            )?;
            if rest.is_none()
                && let Some(signature) = &selected_overload
                && let Some(index) = signature.rest
                && let Some(Type::List(item_ty)) = signature
                    .params
                    .get(index)
                    .and_then(|param| self.ctx.krate.types.get(*param))
            {
                rest = Some(RestParam {
                    index,
                    item_ty: *item_ty,
                });
            }
            let return_ty = selected_overload
                .as_ref()
                .map_or(implementation_return_ty, |signature| signature.return_ty);
            if params
                .iter()
                .any(|param| self.concrete_type_requires_never_value(*param))
                && !Self::call_has_spread_arguments(call)
            {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "calls through function types with never parameters are not lowered",
                ));
            }
            let fixed_param_count = rest.map_or(params.len(), |rest| rest.index);
            if rest.is_none() && call.arguments.len() > fixed_param_count {
                for arg in call.arguments.iter().skip(fixed_param_count) {
                    let _ = self.argument(arg, body)?;
                }
            }
            let function_ty = FunctionType {
                params: params.clone(),
                rest: rest.map(|rest| rest.index),
                required_params: item_required_params,
                    mutable_params: Vec::new(),
                return_ty: implementation_return_ty,
                is_async,
                may_throw: false,
            };
            let args = if rest.is_some() && call.arguments.iter().any(Argument::is_spread) {
                self.spread_closure_call_arguments(&function_ty, rest, call, body)?
            } else {
                let mut args = call
                    .arguments
                    .iter()
                    .take(fixed_param_count)
                    .enumerate()
                    .map(|(index, arg)| {
                        let implementation_hint = params.get(index).copied();
                        let hint = if matches!(arg, Argument::RegExpLiteral(_)) {
                            selected_overload
                                .as_ref()
                                .and_then(|signature| signature.params.get(index).copied())
                                .or(implementation_hint)
                        } else {
                            implementation_hint
                        };
                        self.argument_with_hint(arg, body, hint)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(rest) = rest {
                    let rest_args = call
                        .arguments
                        .iter()
                        .skip(rest.index)
                        .map(|arg| self.argument(arg, body))
                        .collect::<Result<Vec<_>, _>>()?;
                    let rest_ty = self.ctx.krate.types.intern(Type::List(rest.item_ty));
                    args.push(body.push_expr(Expr {
                        kind: ExprKind::ListLit(rest_args),
                        ty: rest_ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                }
                args
            };
            let callee = body.push_expr(Expr {
                kind: ExprKind::Item(item),
                ty: self.ctx.krate.types.intern(Type::Function(function_ty)),
                span: self.span(callee_ident.span.start, callee_ident.span.end),
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Call { callee, args },
                ty: return_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(call.span.start, call.span.end),
            "call expression is not lowered yet",
        ))
    }

    /// Lower a call through the ordered builtin/stdlib dispatch registry.
    ///
    /// This is the single seam that owns "which builtin lowering, if any, claims
    /// this callee". Each registry entry re-matches the callee AST and returns
    /// `Ok(Some(expr))` only when it handles the call. The registry is consulted
    /// strictly in declaration order, so the first matching handler wins exactly
    /// as the previous sequential guard chain did. Adding a new builtin means
    /// adding one entry to [`Self::builtin_call_handlers`] at the right position
    /// instead of editing the long inline guard chain.
    ///
    /// Returns `Ok(None)` when no builtin handler matches, leaving the caller to
    /// continue with the generic call-lowering paths. The unsupported-collection
    /// probe keeps its original semantics: a positive match surfaces as an error,
    /// not as a handled call.
    fn dispatch_builtin_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        for handler in Self::builtin_call_handlers() {
            if let Some(expr) = handler(self, call, body)? {
                return Ok(Some(expr));
            }
        }
        Ok(None)
    }

    /// Adapter so an infallible `Option`-returning handler fits the registry shape.
    ///
    /// Preserves the original guard `if let Some(expr) = self.typed_test_value_call(...)`
    /// which never produced an error.
    fn typed_test_value_call_entry(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        Ok(self.typed_test_value_call(call, body))
    }

    /// Adapter so the type-test handler fits the builtin registry shape.
    fn type_test_call_entry(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        self.type_test_call(call, body)
    }

    /// Adapter so the infallible node-process-version handler fits the registry shape.
    fn node_process_version_match_call_entry(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        Ok(self.node_process_version_match_call(call, body))
    }

    /// Adapter for the unsupported-object-collection probe.
    ///
    /// The original guard turned a positive match into an immediate `Err`, never
    /// a handled expression, so this surfaces the probe's error and otherwise
    /// reports "not handled".
    fn unsupported_object_collection_call_entry(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        _body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        self.unsupported_object_collection_call(call)
            .map_or(Ok(None), Err)
    }

    /// Ordered builtin/stdlib call lowering handlers.
    ///
    /// The order is load-bearing: earlier handlers win when two could match the
    /// same callee, exactly as in the former sequential guard chain. Do not
    /// reorder entries without verifying overlapping callees still resolve to the
    /// same handler.
    fn builtin_call_handlers() -> impl IntoIterator<Item = BuiltinCallHandler<'builder>> {
        [
            Self::type_test_call_entry,
            Self::typed_test_value_call_entry,
            Self::date_call,
            Self::intl_call,
            Self::regex_replace_call,
            Self::object_assign_call,
            Self::vitest_async_expect_call,
        Self::vitest_mock_function_call,
        Self::vitest_spy_on_call,
        Self::vitest_mock_handle_method_call,
        Self::vitest_fake_timer_call,
        Self::date_fns_timezone_context_call,
        Self::sinon_fake_timers_call,
        Self::structured_clone_call,
        Self::crypto_get_random_values_call,
        Self::unsupported_object_collection_call_entry,
        Self::exact_stdlib_call,
        Self::promise_continuation_call,
        Self::timer_call,
        Self::number_to_string_call,
        Self::node_process_version_match_call_entry,
        Self::node_process_cwd_call,
        Self::commonjs_require_call,
        Self::object_metadata_mutation_call,
        Self::buffer_alloc_call,
        Self::buffer_from_call,
        Self::lodash_negate_call,
        Self::lodash_has_call,
        Self::object_get_own_property_symbols_call,
        Self::object_projection_call,
        Self::object_has_own_call,
        Self::dispatch_collection_method,
        Self::map_projection_call,
        Self::regexp_test_call,
        Self::regexp_exec_call,
        Self::string_match_call,
        Self::string_match_all_call,
        Self::url_to_string_call,
        Self::string_case_call,
        Self::string_normalize_call,
        Self::string_trim_call,
        Self::string_affix_call,
        Self::lodash_for_each_call,
        Self::strapi_async_map_call,
        Self::lodash_fp_curried_call,
        Self::list_callback_call,
        Self::list_reduce_call,
        Self::list_search_call,
        Self::collection_at_call,
        Self::list_entries_call,
        Self::string_search_call,
        Self::string_replace_call,
        Self::collection_slice_call,
        Self::list_push_call,
        Self::list_unshift_call,
        Self::list_reverse_call,
        Self::list_sort_call,
        Self::modern_array_call,
        Self::list_pop_call,
        Self::list_shift_call,
        Self::string_repeat_call,
        Self::string_pad_call,
        Self::string_char_at_call,
        Self::node_path_static_call,
        Self::string_join_call,
        Self::list_concat_call,
        Self::list_contains_call,
        Self::string_contains_call,
        Self::string_split_call,
        Self::namespace_member_call,
            Self::function_bind_call,
        ]
    }

    /// Return whether the module source declares a callable with this local name.
    fn source_contains_forward_callable(&self, name: &str) -> bool {
        let const_prefix = format!("const {name}");
        let function_prefix = format!("function {name}(");
        self.source.contains(&function_prefix)
            || self
                .source
                .split(&const_prefix)
                .skip(1)
                .any(|suffix| suffix.starts_with(" =") || suffix.starts_with(':'))
    }

    /// Return whether the module source declares a class with this local name.
    fn source_contains_class(&self, name: &str) -> bool {
        let class_prefix = format!("class {name}");
        let exported_class_prefix = format!("export class {name}");
        self.source.contains(&class_prefix) || self.source.contains(&exported_class_prefix)
    }

    /// Rewrites the receiver for `value[index]?.method()` to optional indexing.
    ///
    /// In JavaScript, an out-of-bounds element access produces `undefined`,
    /// and the following optional method call short-circuits. Keeping the
    /// receiver as a strict `Index` would make generated Rust panic before
    /// optional chaining can observe the missing value.
    fn optionalize_index_receiver(
        &mut self,
        receiver: smelt_hir::ExprId,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let index = usize::try_from(receiver.0).expect("expr id should fit into usize");
        let expr = body
            .exprs
            .get(index)
            .expect("expr id should point to an existing expression")
            .clone();
        let ExprKind::Index {
            receiver: index_receiver,
            index: static_index,
        } = expr.kind
        else {
            return receiver;
        };
        let ty = self.optional_chain_result_type(expr.ty);
        body.push_expr(Expr {
            kind: ExprKind::OptionalIndex {
                receiver: index_receiver,
                index: static_index,
            },
            ty,
            span: expr.span,
        })
    }

    /// Lower `CommonJS` `require(path)` as an opaque JSON-like module object.
    fn commonjs_require_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if !matches!(&call.callee, Expression::Identifier(identifier) if identifier.name == "require")
        {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "require() requires exactly one module path argument",
            ));
        };
        let _ = self.argument(argument, body)?;
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let value_ty = self.ctx.krate.types.intern(Type::Unknown);
        let ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictLit(Vec::new()),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `structuredClone(value)` as a typed value copy.
    ///
    /// HIR values are immutable unless explicitly modeled otherwise, so the
    /// lowering keeps the source value and preserves its type instead of
    /// modeling JavaScript's object graph cloning behavior.
    fn structured_clone_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "structuredClone" {
            return Ok(None);
        }
        let [value] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "structuredClone requires exactly one value",
            ));
        };
        self.argument(value, body).map(Some)
    }

    /// Lower calls where an object property itself is a callable value.
    ///
    /// This covers TypeScript patterns like `args.callback(value)`, where the
    /// property is typed as a function or as a nullishable union containing a
    /// function. Class methods still use the normal method-call path.
    fn callable_static_member_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Ok(callee) = self.static_member(member, body) else {
            return Ok(None);
        };
        if self.static_member_is_concrete_class_method(callee, member, body) {
            return Ok(None);
        }
        let callee_ty = Self::expr_ty(body, callee);
        let has_spread = self.call_has_spread_arguments_or_source_spread(call);
        // A spread call dispatches through the runtime `SmeltUnknown` call ABI
        // whenever *either* the callee field value or the receiving object is
        // erased to `SmeltUnknown` at codegen. That ABI flattens the whole
        // argument list, so the spread must be packed into a single runtime
        // list operand (`ClosureCallSpread`) instead of letting `self.argument`
        // collapse `...rest` into a bare positional operand or letting the
        // concrete-rest branch below build a typed `[fixed.., rest_list]`
        // expansion (which the dynamic ABI would then re-wrap into a nested
        // array).
        //
        // Two independent signals force the dynamic ABI:
        //
        //   * the *callee field's* type is itself a dynamic call surface
        //     (`unknown`/type-parameter/union/erased callable object), or
        //   * the *receiver's* type erases to `SmeltUnknown` at codegen — e.g.
        //     a generic interface/type-alias instantiation such as
        //     `Funnel<Args>` whose member read therefore returns a runtime
        //     `SmeltUnknown` value regardless of the field's declared shape.
        let callee_dispatches_dynamically = self.type_is_dynamic_call_surface(callee_ty);
        let receiver_dispatches_dynamically =
            self.static_member_receiver_dispatches_dynamically(member, body);
        if has_spread && (callee_dispatches_dynamically || receiver_dispatches_dynamically) {
            let unknown = self.ctx.krate.types.intern(Type::Unknown);
            // Re-type the callee operand as `SmeltUnknown` whenever it would
            // otherwise be emitted as a concrete Rust function: an erased
            // callable object, or a field read whose *declared* type is a
            // concrete function but whose *receiver* erases to `SmeltUnknown`
            // (so the field read actually yields a runtime `SmeltUnknown`).
            // The `ClosureCallSpread` emitter dispatches a `SmeltUnknown`
            // callee through the runtime call ABI that consumes the packed
            // argument list, whereas a `Type::Function` callee would be called
            // as a typed Rust closure and reject the flattened `Vec` operand.
            // Type-parameter and union callees already carry a non-function
            // type, so they route through the runtime ABI without a cast.
            let callee_needs_unknown_cast = self.is_callable_object_erased_class(callee_ty)
                || matches!(self.ctx.krate.types.get(callee_ty), Some(Type::Function(_)));
            let callable = if callee_needs_unknown_cast {
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: callee },
                    ty: unknown,
                    span: self.span(member.span.start, member.span.end),
                })
            } else {
                callee
            };
            let packed = self.packed_spread_call_arguments(unknown, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread {
                    callee: callable,
                    args: packed,
                },
                ty: unknown,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        // A spread call whose callee is a *concrete* function with a rest
        // parameter keeps its typed `[fixed.., rest_list]` expansion: codegen
        // dispatches it through the concrete-function path whose Rust rest
        // parameter absorbs the trailing list. `self.argument` would otherwise
        // collapse `...rest` into a single bare positional operand and lose the
        // spread, so route it through the shared spread expansion helper.
        if has_spread
            && let Some(function_ty) = self.function_member_type(callee_ty)
            && let Some(Type::Function(function)) =
                self.ctx.krate.types.get(function_ty).cloned()
            && let Some(rest) = function.rest.and_then(|index| {
                let param = self.type_param_constraint_or_self(*function.params.get(index)?);
                match self.ctx.krate.types.get(param) {
                    Some(Type::List(item_ty)) => Some(RestParam {
                        index,
                        item_ty: *item_ty,
                    }),
                    _ => None,
                }
            })
        {
            let callable = if callee_ty == function_ty {
                callee
            } else {
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: callee },
                    ty: function_ty,
                    span: self.span(member.span.start, member.span.end),
                })
            };
            let args = self.spread_closure_call_arguments(&function, Some(rest), call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCall {
                    callee: callable,
                    args,
                },
                ty: function.return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let mut args = Vec::new();
        for arg in &call.arguments {
            args.push(self.argument(arg, body)?);
        }
        let (function_ty, function) =
            if let Some(function_ty) = self.function_member_type(callee_ty) {
                let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned()
                else {
                    return Ok(None);
                };
                (function_ty, function)
            } else if self.is_nullishable_type(callee_ty) || self.type_contains_unknown(callee_ty) {
                let unknown = self.ctx.krate.types.intern(Type::Unknown);
                let params = args
                    .iter()
                    .map(|arg| Self::expr_ty(body, *arg))
                    .collect::<Vec<_>>();
                let function = FunctionType {
                    params,
                    rest: None,
                    required_params: None,
                    mutable_params: Vec::new(),
                    return_ty: unknown,
                    is_async: false,
                    may_throw: false,
                };
                let function_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Function(function.clone()));
                (function_ty, function)
            } else {
                return Ok(None);
            };
        let callable = if callee_ty == function_ty {
            callee
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: function_ty,
                span: self.span(member.span.start, member.span.end),
            })
        };
        if function
            .params
            .iter()
            .any(|param| self.concrete_type_requires_never_value(*param))
            && !Self::call_has_spread_arguments(call)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "calls through function types with never parameters are not lowered",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: callable,
                args,
            },
            ty: function.return_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return true when a member access names an actual lowered class method.
    fn static_member_is_concrete_class_method(
        &mut self,
        callee: smelt_hir::ExprId,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &Body,
    ) -> bool {
        let Some(expr) = usize::try_from(callee.0)
            .ok()
            .and_then(|index| body.exprs.get(index))
        else {
            return false;
        };
        let (ExprKind::Field { receiver, .. } | ExprKind::OptionalField { receiver, .. }) =
            expr.kind
        else {
            return false;
        };
        let receiver_ty = self.optional_receiver_inner_type(Self::expr_ty(body, receiver));
        let method = self.intern_source_name(member.property.name.as_str());
        if matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, callee)),
            Some(Type::Function(_))
        ) && self.receiver_has_callable_storage_field(receiver_ty, method)
        {
            return false;
        }
        self.resolve_method(receiver_ty, method, member.span)
            .is_ok_and(|(_, item)| item.0 != u32::MAX)
    }

    /// Return true when a class or inherited base explicitly stores a callable
    /// field for the member. These fields model virtual dispatch slots and
    /// must be invoked as closures instead of direct class methods.
    fn receiver_has_callable_storage_field(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
        field: smelt_hir::Symbol,
    ) -> bool {
        let Some(Type::Class { name, args }) = self.ctx.krate.types.get(receiver_ty).cloned()
        else {
            return false;
        };
        let class_name = self
            .ctx
            .krate
            .names
            .get(name)
            .or_else(|| self.ctx.krate.symbols.get(name))
            .map(str::to_owned);
        if class_name
            .as_deref()
            .and_then(|class_name| self.class_fields.get(class_name))
            .and_then(|fields| fields.iter().find(|item| item.name == field))
            .is_some_and(|item| matches!(self.ctx.krate.types.get(item.ty), Some(Type::Function(_))))
        {
            return true;
        }
        let Some((base, base_args)) = self
            .class_by_symbol(name)
            .and_then(|class| class.base.map(|base| (base, class.base_args.clone())))
            .or_else(|| {
                class_name
                    .as_deref()
                    .and_then(|base_name| self.class_bases.get(base_name).cloned())
            })
        else {
            return false;
        };
        let substitutions = self
            .type_argument_substitution(
                &self
                    .class_by_symbol(name)
                    .map(|class| class.type_params.clone())
                    .unwrap_or_default(),
                &args,
                self.span(0, 0),
            )
            .ok();
        let base_args = if let Some(substitutions) = substitutions {
            base_args
                .into_iter()
                .map(|arg| self.substitute_type_params(arg, &substitutions))
                .collect()
        } else {
            base_args
        };
        let base_ty = self.ctx.krate.types.intern(Type::Class {
            name: base,
            args: base_args,
        });
        self.receiver_has_callable_storage_field(base_ty, field)
    }

    /// Lower Promise/Future `.then(...)` and `.catch(...)` continuation calls.
    fn promise_continuation_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let method = member.property.name.as_str();
        if !matches!(method, "then" | "catch") {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let Some(Type::Future(inner_ty)) = self.ctx.krate.types.get(receiver_ty).cloned() else {
            return Ok(None);
        };
        let [callback_arg, ..] = call.arguments.as_slice() else {
            return Ok(Some(receiver));
        };
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let callback_return_ty = self.ctx.krate.types.intern(Type::None);
        let callback_param_ty = if method == "catch" {
            unknown_ty
        } else {
            inner_ty
        };
        let callback_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![callback_param_ty],
            rest: None,
            required_params: Some(1),
            mutable_params: Vec::new(),
            return_ty: callback_return_ty,
            is_async: false,
            may_throw: false,
        }));
        let callback = self.argument_with_hint(callback_arg, body, Some(callback_ty))?;
        let (op, output_ty) = if method == "catch" {
            (AsyncOp::Catch, receiver_ty)
        } else {
            (AsyncOp::Then, self.ctx.krate.types.intern(Type::Future(unknown_ty)))
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op,
                args: vec![receiver, callback],
            },
            ty: output_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower `callable.bind(this_arg, ...bound)` into a closure with captured arguments.
    ///
    /// JavaScript `Function.prototype.bind` produces a new callable value. Smelt
    /// models the callable value directly: the generated closure captures the
    /// original callee plus each bound argument, then forwards the remaining
    /// parameters when the closure is called.
    fn function_bind_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        outer_body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "bind" {
            return Ok(None);
        }
        let receiver = self.expression(&member.object, outer_body)?;
        let receiver_ty = Self::expr_ty(outer_body, receiver);
        let Some(function_ty) = self.function_member_type(receiver_ty) else {
            return Ok(None);
        };
        let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned() else {
            return Ok(None);
        };
        let bound_source_args = call.arguments.iter().skip(1).collect::<Vec<_>>();
        if bound_source_args.len() > function.params.len() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Function.prototype.bind received more bound arguments than callable parameters",
            ));
        }

        let span = self.span(call.span.start, call.span.end);
        let callee_symbol = self.intern_source_name("__smelt_bind_callee");
        let callee_local =
            self.capture_bind_value(receiver, function_ty, callee_symbol, span, outer_body);
        let mut bound_captures = Vec::new();
        for (index, argument) in bound_source_args.iter().enumerate() {
            let param_ty = function.params.get(index).copied();
            let value = self.argument_with_hint(argument, outer_body, param_ty)?;
            let value_ty = Self::expr_ty(outer_body, value);
            let symbol = self.intern_source_name(&format!("__smelt_bind_arg_{index}"));
            let local = self.capture_bind_value(value, value_ty, symbol, span, outer_body);
            bound_captures.push((local, symbol, value_ty));
        }

        let remaining_params = function
            .params
            .iter()
            .copied()
            .skip(bound_captures.len())
            .collect::<Vec<_>>();
        let mut closure_body = Body::new(None, span);
        let mut captures = Vec::new();
        let callee_body_local = Self::push_bind_capture_local(
            callee_local,
            callee_symbol,
            function_ty,
            span,
            &mut closure_body,
            &mut captures,
        );
        let mut call_args = Vec::new();
        for (source_local, symbol, ty) in bound_captures {
            let body_local = Self::push_bind_capture_local(
                source_local,
                symbol,
                ty,
                span,
                &mut closure_body,
                &mut captures,
            );
            call_args.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(body_local),
                ty,
                span,
            }));
        }

        let mut closure_params = Vec::new();
        for (index, ty) in remaining_params.iter().copied().enumerate() {
            let symbol = self.synthetic_param_symbol(index);
            let local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span,
            });
            closure_body.params.push(local);
            closure_params.push(Param {
                name: symbol,
                local,
                ty,
                span,
            });
            call_args.push(closure_body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span,
            }));
        }

        let callee = closure_body.push_expr(Expr {
            kind: ExprKind::Local(callee_body_local),
            ty: function_ty,
            span,
        });
        let call_expr = closure_body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee,
                args: call_args,
            },
            ty: function.return_ty,
            span,
        });
        closure_body.push_stmt(Stmt::Return(Some(call_expr)));
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: remaining_params,
            rest: None,
            required_params: None,
                    mutable_params: Vec::new(),
            return_ty: function.return_ty,
            is_async: function.is_async,
            may_throw: function.may_throw,
        }));
        Ok(Some(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest: None,
                required_params: None,
                return_ty: function.return_ty,
                captures,
                body: body_id,
                span,
            }),
            ty: closure_ty,
            span,
        })))
    }

    /// Store a bind component in an outer local so a generated closure can capture it.
    fn capture_bind_value(
        &self,
        value: smelt_hir::ExprId,
        ty: smelt_hir::TypeId,
        symbol: smelt_hir::Symbol,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::LocalId {
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span,
        });
        let pat = body.push_pattern(Pattern::Binding(local));
        let stmt = Stmt::Let {
            pat,
            ty,
            value: Some(value),
        };
        if let Some(block) = self.current_statement_block {
            body.push_stmt_to_block(block, stmt);
        } else {
            body.push_stmt(stmt);
        }
        local
    }

    /// Add one captured bind component to a generated closure body.
    fn push_bind_capture_local(
        source_local: smelt_hir::LocalId,
        symbol: smelt_hir::Symbol,
        ty: smelt_hir::TypeId,
        span: Span,
        closure_body: &mut Body,
        captures: &mut Vec<ClosureCapture>,
    ) -> smelt_hir::LocalId {
        let body_local = closure_body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span,
        });
        captures.push(ClosureCapture {
            source_local,
            body_local: Some(body_local),
            symbol,
            ty,
            mode: CaptureMode::ByRef,
        });
        body_local
    }

    /// Lower JavaScript `Symbol(description)` branding calls as opaque strings.
    ///
    /// Smelt does not model symbol identity yet. Type-level branding libraries
    /// only need a stable opaque value here so the containing module can lower.
    fn symbol_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::Identifier(object) = &member.object
            && object.name == "Symbol"
            && member.property.name == "for"
        {
            return self.symbol_like_call(call, body, true);
        }
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "Symbol" {
            return Ok(None);
        }
        self.symbol_like_call(call, body, false)
    }

    /// Lower a supported symbol-producing call as a stable opaque string.
    fn symbol_like_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        global: bool,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if call.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Symbol(...) supports at most one description argument",
            ));
        }
        let description_arg = call.arguments.first();
        let checked_description = description_arg
            .map(|argument| self.argument(argument, body))
            .transpose()?;
        let ty = self.ctx.krate.types.intern(Type::Unknown);
        if let Some(description) = checked_description
            && !matches!(
                self.ctx.krate.types.get(Self::expr_ty(body, description)),
                Some(Type::String)
            )
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Symbol(...) description must be a string",
            ));
        }
        let description = description_arg
            .and_then(|argument| match argument {
                Argument::StringLiteral(literal) => Some(literal.value.as_str().to_owned()),
                _ => None,
            })
            .unwrap_or_default();
        let value = if global {
            format!("Symbol.for({description})")
        } else {
            format!("Symbol({description})@{}", call.span.start)
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Symbol(value)),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Select the TypeScript overload signature that matches a call site.
    fn selected_overload_signature(
        &mut self,
        name: &str,
        arguments: &[Argument<'_>],
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<Option<OverloadSignature>, SmeltError> {
        let Some(signatures) = (if let Some(signatures) = self.function_overloads.get(name) {
            Some(signatures.clone())
        } else {
            self.ctx.overloads.get(name).cloned()
        }) else {
            return Ok(None);
        };
        if signatures.is_empty() {
            return Ok(None);
        }
        let mut lowered_arg_tys = Vec::new();
        for argument in arguments {
            let arg = self.argument(argument, body)?;
            lowered_arg_tys.push(Self::expr_ty(body, arg));
        }
        let mut selected: Option<(usize, usize, usize, OverloadSignature, HashMap<_, _>)> = None;
        for (order, signature) in signatures.iter().enumerate() {
            let mut substitutions = HashMap::new();
            let constraint_scope = Self::overload_type_param_constraint_scope(signature);
            self.type_param_constraint_scopes.push(constraint_scope);
            let matches = self.overload_signature_matches_args(
                signature,
                arguments,
                &lowered_arg_tys,
                &mut substitutions,
            ) && self.overload_accepts_spread_shape(signature, arguments, &lowered_arg_tys)
                && self.overload_substitutions_satisfy_constraints(signature, &substitutions);
            self.type_param_constraint_scopes.pop();
            if matches {
                let score =
                    self.overload_signature_specificity_score(signature, lowered_arg_tys.len());
                let literal_score =
                    usize::MAX - self.overload_source_arg_mismatch_count(signature, arguments);
                let candidate = (
                    score,
                    literal_score,
                    usize::MAX - order,
                    signature.clone(),
                    substitutions,
                );
                if selected.as_ref().is_none_or(|current| {
                    (candidate.1, candidate.0, candidate.2) > (current.1, current.0, current.2)
                }) {
                    selected = Some(candidate);
                }
            }
        }
        if let Some((_, _, _, signature, substitutions)) = selected {
            return Ok(Some(
                self.instantiate_overload_signature(signature, &substitutions),
            ));
        }
        if self.has_ts_expect_error_before(span.start, "ts2353") {
            return Ok(None);
        }
        if self.allow_unknown_index_access {
            let mut loose_selected: Option<(
                usize,
                usize,
                usize,
                OverloadSignature,
                HashMap<_, _>,
            )> = None;
            for (order, signature) in signatures.iter().enumerate() {
                if !Self::overload_signature_arity_matches(signature, arguments.len()) {
                    continue;
                }
                let mut substitutions = HashMap::new();
                let constraint_scope = Self::overload_type_param_constraint_scope(signature);
                self.type_param_constraint_scopes.push(constraint_scope);
                let matches = self.loose_overload_signature_matches_args(
                    signature,
                    arguments,
                    &lowered_arg_tys,
                    &mut substitutions,
                ) && self.overload_accepts_spread_shape(signature, arguments, &lowered_arg_tys)
                    && self.overload_substitutions_satisfy_constraints(signature, &substitutions);
                self.type_param_constraint_scopes.pop();
                if !matches {
                    continue;
                }
                let score =
                    self.overload_signature_specificity_score(signature, lowered_arg_tys.len());
                let literal_score =
                    usize::MAX - self.overload_source_arg_mismatch_count(signature, arguments);
                let candidate = (
                    score,
                    literal_score,
                    usize::MAX - order,
                    signature.clone(),
                    substitutions,
                );
                if loose_selected.as_ref().is_none_or(|current| {
                    (candidate.1, candidate.0, candidate.2) > (current.1, current.0, current.2)
                }) {
                    loose_selected = Some(candidate);
                }
            }
            if let Some((_, _, _, signature, substitutions)) = loose_selected {
                return Ok(Some(
                    self.instantiate_overload_signature(signature, &substitutions),
                ));
            }
            return Ok(None);
        }
        Err(SmeltError::unsupported(
            self.span(span.start, span.end),
            format!(
                "no overload of `{name}` matches this call (args: {:?}, overloads: {:?})",
                lowered_arg_tys
                    .iter()
                    .map(|ty| self.ctx.krate.types.get(*ty))
                    .collect::<Vec<_>>(),
                signatures
                    .iter()
                    .map(|signature| signature
                        .params
                        .iter()
                        .map(|ty| self.ctx.krate.types.get(*ty))
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            ),
        ))
    }

    /// Build the active constraint scope needed while matching a stored overload signature.
    fn overload_type_param_constraint_scope(
        signature: &OverloadSignature,
    ) -> HashMap<smelt_hir::Symbol, smelt_hir::TypeId> {
        signature
            .type_params
            .iter()
            .filter_map(|param| param.constraint.map(|constraint| (param.name, constraint)))
            .collect()
    }

    /// Count obvious source literal conflicts with an overload parameter surface.
    fn overload_source_arg_mismatch_count(
        &mut self,
        signature: &OverloadSignature,
        arguments: &[Argument<'_>],
    ) -> usize {
        if arguments.len() < 2 {
            return 0;
        }
        let constraints = Self::overload_type_param_constraint_scope(signature);
        signature
            .params
            .iter()
            .take(arguments.len())
            .zip(arguments)
            .filter(|(expected, argument)| {
                !self.overload_source_arg_matches_param(**expected, argument, &constraints)
            })
            .count()
    }

    /// Return whether one source literal can inhabit an overload parameter surface.
    fn overload_source_arg_matches_param(
        &mut self,
        expected: smelt_hir::TypeId,
        argument: &Argument<'_>,
        constraints: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        let expected = match self.ctx.krate.types.get(expected).cloned() {
            Some(Type::TypeParam { name }) => constraints.get(&name).copied().unwrap_or(expected),
            _ => expected,
        };
        match self.ctx.krate.types.get(expected).cloned() {
            Some(Type::Unknown | Type::TypeParam { .. }) => true,
            Some(Type::Optional(inner)) => {
                matches!(argument, Argument::NullLiteral(_))
                    || self.overload_source_arg_matches_param(inner, argument, constraints)
            }
            Some(Type::Union(items)) => items
                .into_iter()
                .any(|item| self.overload_source_arg_matches_param(item, argument, constraints)),
            Some(Type::String) => matches!(
                argument,
                Argument::StringLiteral(_)
                    | Argument::TemplateLiteral(_)
                    | Argument::Identifier(_)
                    | Argument::CallExpression(_)
                    | Argument::StaticMemberExpression(_)
                    | Argument::ComputedMemberExpression(_)
            ),
            Some(Type::Int | Type::Float) => matches!(
                argument,
                Argument::NumericLiteral(_)
                    | Argument::BigIntLiteral(_)
                    | Argument::Identifier(_)
                    | Argument::CallExpression(_)
                    | Argument::StaticMemberExpression(_)
                    | Argument::ComputedMemberExpression(_)
            ),
            Some(Type::Bool) => matches!(
                argument,
                Argument::BooleanLiteral(_)
                    | Argument::Identifier(_)
                    | Argument::CallExpression(_)
                    | Argument::StaticMemberExpression(_)
                    | Argument::ComputedMemberExpression(_)
            ),
            Some(Type::None) => matches!(argument, Argument::NullLiteral(_)),
            Some(Type::Class { name, .. }) if self
                .ctx
                .krate
                .symbols
                .get(name)
                .is_some_and(|name| name == "RegExp") =>
            {
                matches!(
                    argument,
                    Argument::RegExpLiteral(_)
                        | Argument::Identifier(_)
                        | Argument::CallExpression(_)
                        | Argument::StaticMemberExpression(_)
                        | Argument::ComputedMemberExpression(_)
                )
            }
            _ => true,
        }
    }

    /// Return whether an overload accepts the call under permissive top-level shape checks.
    ///
    /// This is used only as the unknown-index fallback after strict inference has
    /// failed. It still rejects source-shape mismatches such as a primitive value
    /// being passed where an overload expects a tuple/case value, which keeps
    /// broad Remeda data-last signatures from shadowing data-first calls.
    fn loose_overload_signature_matches_args(
        &mut self,
        signature: &OverloadSignature,
        arguments: &[Argument<'_>],
        arg_tys: &[smelt_hir::TypeId],
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        if Self::overload_signature_arity_matches(signature, arg_tys.len())
            && signature.rest.is_none()
            && signature
                .params
                .iter()
                .take(arg_tys.len())
                .zip(arg_tys)
                .all(|(expected, actual)| {
                    self.loose_overload_type_matches(*expected, *actual, substitutions)
                })
        {
            return true;
        }
        let Some(rest_index) = signature.rest else {
            return false;
        };
        let Some((&rest_ty, fixed_params)) = signature.params.split_last() else {
            return arg_tys.is_empty();
        };
        if rest_index != fixed_params.len() || arg_tys.len() < fixed_params.len() {
            return false;
        }
        let mut rest_substitutions = substitutions.clone();
        let Some(fixed_args) = arg_tys.get(..fixed_params.len()) else {
            return false;
        };
        if !fixed_params
            .iter()
            .zip(fixed_args)
            .all(|(expected, actual)| {
                self.loose_overload_type_matches(*expected, *actual, &mut rest_substitutions)
            })
        {
            return false;
        }
        let Some(rest_args) = arg_tys.get(fixed_params.len()..) else {
            return false;
        };
        let Some(rest_arguments) = arguments.get(fixed_params.len()..) else {
            return false;
        };
        let rest_matches = match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(rest_ty))
            .cloned()
        {
            Some(Type::Tuple(_))
                if rest_args.len() == 1
                    && matches!(rest_arguments.first(), Some(Argument::SpreadElement(_))) =>
            {
                rest_args.first().is_some_and(|actual| {
                    self.loose_overload_type_matches(
                        rest_ty,
                        *actual,
                        &mut rest_substitutions,
                    )
                })
            }
            Some(Type::Tuple(items)) if items.len() == rest_args.len() => {
                items.iter().zip(rest_args).all(|(expected, actual)| {
                    self.loose_overload_type_matches(*expected, *actual, &mut rest_substitutions)
                })
            }
            Some(Type::List(item_ty)) => rest_args.iter().zip(rest_arguments).all(
                |(actual, argument)| {
                    let expected = if matches!(argument, Argument::SpreadElement(_)) {
                        rest_ty
                    } else {
                        item_ty
                    };
                    self.loose_overload_type_matches(
                        expected,
                        *actual,
                        &mut rest_substitutions,
                    )
                },
            ),
            _ => false,
        };
        if rest_matches {
            *substitutions = rest_substitutions;
        }
        rest_matches
    }

    /// Return whether an argument has a plausible top-level shape for an overload parameter.
    fn loose_overload_type_matches(
        &mut self,
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        let expected = self.substitute_type_params(expected, substitutions);
        if expected == actual || self.infer_overload_type(expected, actual, substitutions) {
            return true;
        }
        match (
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(expected))
                .cloned(),
            self.ctx.krate.types.get(actual).cloned(),
        ) {
            (Some(Type::TypeParam { name }), _) => {
                substitutions.insert(name, actual);
                true
            }
            (Some(Type::Unknown), _) | (_, Some(Type::Unknown | Type::TypeParam { .. })) => true,
            (Some(Type::Optional(_)), Some(Type::None)) => true,
            (Some(Type::Optional(inner)), _) => {
                self.loose_overload_type_matches(inner, actual, substitutions)
            }
            (Some(Type::Union(items)), _) => items.into_iter().any(|item| {
                self.loose_overload_type_matches(item, actual, &mut substitutions.clone())
            }),
            (_, Some(Type::Union(items))) => items.into_iter().any(|item| {
                self.loose_overload_type_matches(expected, item, &mut substitutions.clone())
            }),
            (Some(Type::Tuple(_)), Some(Type::Tuple(_) | Type::List(_))) => true,
            (Some(Type::List(_)), Some(Type::List(_) | Type::Tuple(_))) => true,
            (Some(Type::Function(_)), Some(Type::Function(_))) => true,
            (
                Some(Type::Class {
                    name: expected_name,
                    ..
                }),
                Some(Type::Class {
                    name: actual_name, ..
                }),
            ) => expected_name == actual_name,
            (Some(Type::Dict(_, _)), Some(Type::Dict(_, _))) => true,
            (Some(Type::Set(_)), Some(Type::Set(_))) => true,
            (Some(Type::Future(_)), Some(Type::Future(_))) => true,
            (Some(Type::Bool), Some(Type::Bool))
            | (Some(Type::String), Some(Type::String))
            | (Some(Type::Int | Type::Float), Some(Type::Int | Type::Float))
            | (Some(Type::None), Some(Type::None))
            | (Some(Type::Never), _) => true,
            _ => false,
        }
    }

    /// Route exact global and static namespace calls through shared recognition metadata.
    ///
    /// Receiver-, shape-, plugin-, and test-framework-dependent handlers remain
    /// in `call_expression` because they require semantic inspection and ordered
    /// overlap resolution rather than exact source-name recognition.
    fn exact_stdlib_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(rule) = stdlib_dispatch::call_rule(call) else {
            return Ok(None);
        };
        match rule {
            RuleId::TsStructuredClone => self.structured_clone_call(call, body),
            RuleId::TsPromiseStatic => self.promise_static_call(call, body),
            RuleId::TsFetch => self.fetch_call(call, body),
            RuleId::TsPrimitiveCast => self.primitive_cast_call(call, body),
            RuleId::TsSymbol => self.symbol_call(call, body),
            RuleId::TsMathNumeric => self.exact_math_numeric_call(call, body),
            RuleId::TsMathRandom => self.math_random_call(call, body),
            RuleId::TsNumberPredicate => self.number_predicate_call(call, body),
            RuleId::TsNumberParseFloat => self.number_parse_float_call(call, body),
            RuleId::TsNumberParseInt => self.number_parse_int_call(call, body),
            RuleId::TsObjectStatic => self.exact_object_static_call(call, body),
            RuleId::TsArrayStatic => self.exact_array_static_call(call, body),
            RuleId::TsJsonStringify => self.json_stringify_call(call, body),
            RuleId::TsJsonParse => self.json_parse_call(call, body),
            _ => Ok(None),
        }
    }

    /// Preserve the established ordering among exact deterministic `Math.*` handlers.
    fn exact_math_numeric_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.math_abs_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.math_round_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.math_extrema_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.math_hypot_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.math_unary_func_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.math_pow_call(call, body)? {
            return Ok(Some(expr));
        }
        self.math_atan2_call(call, body)
    }

    /// Preserve the established ordering among exact static `Object.*` handlers.
    fn exact_object_static_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.object_is_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.object_from_entries_call(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.object_create_call(call, body)? {
            return Ok(Some(expr));
        }
        self.object_get_prototype_of_call(call, body)
    }

    /// Preserve the established ordering among exact static `Array.*` handlers.
    fn exact_array_static_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.array_is_array_call(call, body)? {
            return Ok(Some(expr));
        }
        self.array_from_call(call, body)
    }

    /// Return whether an overload has the same source-level argument count as a call.
    fn overload_signature_arity_matches(
        signature: &OverloadSignature,
        argument_count: usize,
    ) -> bool {
        let minimum = signature.required_params.unwrap_or(signature.params.len());
        argument_count >= minimum
            && signature
                .rest
                .map_or(argument_count <= signature.params.len(), |_| true)
    }

    /// Score a matching overload by source-shape specificity.
    ///
    /// Broad purry overloads often have a generic data-first parameter that can
    /// accept an order-rule tuple. Counting concrete fixed/rest positions keeps
    /// the intended data-last overload from being shadowed for single-rule calls
    /// while still preferring data-first when it has an actual leading data
    /// argument plus rule arguments.
    fn overload_signature_specificity_score(
        &self,
        signature: &OverloadSignature,
        argument_count: usize,
    ) -> usize {
        let fixed_arity = signature.rest.unwrap_or(signature.params.len());
        let fixed_score = signature
            .params
            .iter()
            .take(fixed_arity)
            .filter(|ty| self.overload_param_is_specific(**ty))
            .count();
        let rest_score = signature.rest.map_or(0, |rest_index| {
            let rest_ty = signature.params.get(rest_index).copied();
            let rest_count = argument_count.saturating_sub(rest_index);
            if rest_ty.is_some_and(|ty| self.overload_param_is_specific(ty)) {
                rest_count
            } else {
                0
            }
        });
        fixed_arity * 100 + fixed_score + rest_score
    }

    /// Reject fixed tuple-rest overloads for variable-length spread tails.
    ///
    /// A call such as `fn(head, ...values)` cannot select an overload whose
    /// rest parameter is a fixed tuple unless the spread expression itself has
    /// a fixed tuple shape. Otherwise runtime values beyond the tuple width
    /// would be discarded by later destructuring.
    fn overload_accepts_spread_shape(
        &self,
        signature: &OverloadSignature,
        arguments: &[Argument<'_>],
        argument_tys: &[smelt_hir::TypeId],
    ) -> bool {
        let Some(rest_index) = signature.rest else {
            return true;
        };
        let Some(rest_ty) = signature.params.get(rest_index).copied() else {
            return false;
        };
        if !matches!(
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(rest_ty)),
            Some(Type::Tuple(_))
        ) {
            return true;
        }
        arguments
            .iter()
            .enumerate()
            .skip(rest_index)
            .filter(|(_, argument)| matches!(argument, Argument::SpreadElement(_)))
            .all(|(index, _)| {
                argument_tys.get(index).is_some_and(|ty| {
                    matches!(
                        self.ctx
                            .krate
                            .types
                            .get(self.type_param_constraint_or_self(*ty)),
                        Some(Type::Tuple(_))
                    )
                })
            })
    }

    /// Return whether an overload parameter contributes useful shape
    /// information beyond accepting any value.
    fn overload_param_is_specific(&self, ty: smelt_hir::TypeId) -> bool {
        !matches!(
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(ty)),
            Some(Type::Unknown | Type::TypeParam { .. })
        )
    }

    /// Return whether an overload signature accepts the lowered call argument types.
    ///
    /// TypeScript overload declarations represent `...rest: [A, B]` as a
    /// single tuple-typed parameter in HIR. Call sites still pass those tuple
    /// elements as individual source arguments, so overload matching expands a
    /// trailing tuple/list parameter when exact arity matching fails.
    fn overload_signature_matches_args(
        &mut self,
        signature: &OverloadSignature,
        arguments: &[Argument<'_>],
        arg_tys: &[smelt_hir::TypeId],
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        if Self::overload_signature_arity_matches(signature, arg_tys.len())
            && signature.rest.is_none()
            && signature
                .params
                .iter()
                .take(arg_tys.len())
                .zip(arg_tys)
                .all(|(expected, actual)| {
                    self.infer_overload_type(*expected, *actual, substitutions)
                })
        {
            return true;
        }
        let Some(rest_index) = signature.rest else {
            return false;
        };
        let Some((&rest_ty, fixed_params)) = signature.params.split_last() else {
            return arg_tys.is_empty();
        };
        if rest_index != fixed_params.len() {
            return false;
        }
        if arg_tys.len() < fixed_params.len() {
            return false;
        }
        let mut rest_substitutions = substitutions.clone();
        let Some(fixed_args) = arg_tys.get(..fixed_params.len()) else {
            return false;
        };
        if !fixed_params
            .iter()
            .zip(fixed_args)
            .all(|(expected, actual)| {
                self.infer_overload_type(*expected, *actual, &mut rest_substitutions)
            })
        {
            return false;
        }
        let Some(rest_args) = arg_tys.get(fixed_params.len()..) else {
            return false;
        };
        let Some(rest_arguments) = arguments.get(fixed_params.len()..) else {
            return false;
        };
        let rest_matches = match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(rest_ty))
            .cloned()
        {
            Some(Type::Tuple(_))
                if rest_args.len() == 1
                    && matches!(rest_arguments.first(), Some(Argument::SpreadElement(_))) =>
            {
                rest_args.first().is_some_and(|actual| {
                    self.infer_overload_type(rest_ty, *actual, &mut rest_substitutions)
                })
            }
            Some(Type::Tuple(items)) if items.len() == rest_args.len() => {
                items.iter().zip(rest_args).all(|(expected, actual)| {
                    self.infer_overload_type(*expected, *actual, &mut rest_substitutions)
                })
            }
            Some(Type::List(item_ty)) => rest_args.iter().zip(rest_arguments).all(
                |(actual, argument)| {
                    let expected = if matches!(argument, Argument::SpreadElement(_)) {
                        rest_ty
                    } else {
                        item_ty
                    };
                    self.infer_overload_type(expected, *actual, &mut rest_substitutions)
                },
            ),
            _ => false,
        };
        if rest_matches {
            *substitutions = rest_substitutions;
        }
        rest_matches
    }

    /// Instantiate a selected overload signature with inferred generic types.
    fn instantiate_overload_signature(
        &mut self,
        signature: OverloadSignature,
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> OverloadSignature {
        let params = signature
            .params
            .into_iter()
            .map(|param| self.substitute_type_params(param, substitutions))
            .collect();
        let return_ty = self.substitute_type_params(signature.return_ty, substitutions);
        OverloadSignature {
            type_params: signature.type_params,
            params,
            rest: signature.rest,
            required_params: signature.required_params,
            return_ty,
            is_async: signature.is_async,
        }
    }

    /// Return whether inferred generic overload arguments satisfy their declared bounds.
    ///
    /// Applicability of a TypeScript overload includes each generic `extends`
    /// constraint. Checking this after inference retains substitutions learned
    /// from nested parameter shapes while preventing broad purry overloads such
    /// as `<Options extends OptionsShape>(options?: Options)` from accepting a
    /// primitive data argument.
    fn overload_substitutions_satisfy_constraints(
        &mut self,
        signature: &OverloadSignature,
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        signature.type_params.iter().all(|param| {
            let Some(constraint) = param.constraint else {
                return true;
            };
            let Some(actual) = substitutions.get(&param.name).copied() else {
                return true;
            };
            let constraint = self.substitute_type_params(constraint, substitutions);
            if self.overload_constraint_contains_unresolved_type_param(constraint) {
                return true;
            }
            self.overload_constraint_accepts(actual, constraint)
        })
    }

    /// Return whether a generic bound still depends on contextual type inference.
    ///
    /// TypeScript can select a data-last overload such as
    /// `<T, K extends Keys<T>>(key: K) => (data: T) => ...` before `T` is
    /// known; the later callable context provides it. Enforcing `K`'s bound
    /// while `T` is still present rejects valid curried calls.
    fn overload_constraint_contains_unresolved_type_param(
        &self,
        constraint: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(constraint) {
            Some(Type::TypeParam { .. }) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
                self.overload_constraint_contains_unresolved_type_param(*item)
            }
            Some(Type::Dict(key, value)) => {
                self.overload_constraint_contains_unresolved_type_param(*key)
                    || self.overload_constraint_contains_unresolved_type_param(*value)
            }
            Some(Type::Tuple(items) | Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.overload_constraint_contains_unresolved_type_param(item)),
            Some(Type::Class { args, .. }) => args
                .iter()
                .copied()
                .any(|arg| self.overload_constraint_contains_unresolved_type_param(arg)),
            Some(Type::Function(function)) => {
                function.params.iter().copied().any(|param| {
                    self.overload_constraint_contains_unresolved_type_param(param)
                }) || self.overload_constraint_contains_unresolved_type_param(function.return_ty)
            }
            Some(
                Type::Bool
                | Type::Int
                | Type::Float
                | Type::String
                | Type::Unknown
                | Type::Never
                | Type::None,
            )
            | None => false,
        }
    }

    /// Return whether an inferred overload argument satisfies its `extends` surface.
    ///
    /// Lowered object literals use `Dict` storage even when their TypeScript
    /// annotation is a named structural object. Constraint applicability must
    /// bridge that erased representation while primitive values still fail
    /// object constraints.
    fn overload_constraint_accepts(
        &mut self,
        actual: smelt_hir::TypeId,
        constraint: smelt_hir::TypeId,
    ) -> bool {
        if self.type_assignable_to(actual, constraint) {
            return true;
        }
        if let (Some(Type::Function(expected)), Some(Type::Function(actual))) = (
            self.ctx.krate.types.get(constraint).cloned(),
            self.ctx.krate.types.get(actual).cloned(),
        ) {
            let mut inferred = HashMap::new();
            if self.infer_overload_function_type(&expected, &actual, &mut inferred) {
                return true;
            }
        }
        let (
            Some(Type::Dict(actual_key, actual_value)),
            Some(Type::Class { name, args }),
        ) = (
            self.ctx.krate.types.get(actual).cloned(),
            self.ctx.krate.types.get(constraint).cloned(),
        ) else {
            return false;
        };
        if !matches!(self.ctx.krate.types.get(actual_key), Some(Type::String)) {
            return false;
        }
        let Some(fields) = self.overload_structural_constraint_fields(name, &args) else {
            return false;
        };
        if fields.is_empty() {
            return true;
        }
        fields.into_iter().any(|field| {
            self.type_assignable_to(actual_value, field.ty)
                || (field.optional
                    && self.ctx.krate.types.get(field.ty).is_some_and(|ty| {
                        if let Type::Optional(inner) = ty {
                            self.type_assignable_to(actual_value, *inner)
                        } else {
                            false
                        }
                    }))
        })
    }

    /// Return declared fields for a named structural constraint with generic arguments applied.
    fn overload_structural_constraint_fields(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
    ) -> Option<Vec<Field>> {
        if let Some(class) = self.class_by_symbol(name).cloned() {
            let substitutions = self
                .type_argument_substitution(&class.type_params, args, self.span(0, 0))
                .ok()?;
            return Some(self.substituted_fields(&class.fields, &substitutions));
        }
        if let Some(interface) = self.find_interface(name).cloned() {
            let substitutions = self
                .type_argument_substitution(&interface.type_params, args, self.span(0, 0))
                .ok()?;
            return Some(self.substituted_fields(&interface.fields, &substitutions));
        }
        let alias = self.find_type_alias(name).cloned()?;
        let substitutions = self
            .type_argument_substitution(&alias.type_params, args, self.span(0, 0))
            .ok()?;
        self.type_alias_fields
            .get(&name)
            .cloned()
            .map(|fields| self.substituted_fields(&fields, &substitutions))
    }

    /// Infer generic overload substitutions while checking argument compatibility.
    fn infer_overload_type(
        &mut self,
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        let expected = self.substitute_type_params(expected, substitutions);
        if expected == actual {
            return true;
        }
        match (
            self.ctx.krate.types.get(expected).cloned(),
            self.ctx.krate.types.get(actual).cloned(),
        ) {
            (Some(Type::TypeParam { name }), _) => {
                substitutions.insert(name, actual);
                true
            }
            (Some(Type::Unknown), _)
            | (_, Some(Type::Unknown | Type::TypeParam { .. }))
            | (Some(Type::Float), Some(Type::Int))
            | (Some(Type::Int), Some(Type::Float)) => true,
            _ if self.is_date_constructor_arg_type(expected)
                && self.is_date_constructor_arg_type(actual) =>
            {
                true
            }
            (Some(Type::Optional(expected_inner)), Some(Type::Optional(actual_inner))) => {
                (self.function_member_type(expected_inner).is_some()
                    && self.function_member_type(actual_inner).is_some())
                    || self.infer_overload_type(expected_inner, actual_inner, substitutions)
            }
            (Some(Type::Optional(_)), Some(Type::None)) => true,
            (Some(Type::Optional(inner)), _) if inner == actual => true,
            (Some(Type::Optional(inner)), _) => {
                self.infer_overload_type(inner, actual, substitutions)
            }
            (Some(Type::List(expected_item)), Some(Type::List(actual_item)))
            | (Some(Type::Set(expected_item)), Some(Type::Set(actual_item)))
            | (Some(Type::Future(expected_item)), Some(Type::Future(actual_item))) => {
                self.infer_overload_type(expected_item, actual_item, substitutions)
            }
            (
                Some(Type::Dict(expected_key, expected_value)),
                Some(Type::Dict(actual_key, actual_value)),
            ) => {
                self.infer_overload_type(expected_key, actual_key, substitutions)
                    && self.infer_overload_type(expected_value, actual_value, substitutions)
            }
            (Some(Type::Tuple(expected_items)), Some(Type::Tuple(actual_items))) => {
                expected_items.len() == actual_items.len()
                    && expected_items.into_iter().zip(actual_items).all(
                        |(expected_item, actual_item)| {
                            self.infer_overload_type(expected_item, actual_item, substitutions)
                        },
                    )
            }
            (Some(Type::Tuple(expected_items)), Some(Type::List(actual_item))) => {
                expected_items.into_iter().all(|expected_item| {
                    self.infer_overload_type(expected_item, actual_item, substitutions)
                })
            }
            (
                Some(Type::Class {
                    name: expected_name,
                    args: expected_args,
                }),
                Some(Type::Class {
                    name: actual_name,
                    args: actual_args,
                }),
            ) => {
                expected_name == actual_name
                    && expected_args.len() == actual_args.len()
                    && expected_args.into_iter().zip(actual_args).all(
                        |(expected_arg, actual_arg)| {
                            self.infer_overload_type(expected_arg, actual_arg, substitutions)
                        },
                    )
            }
            (Some(Type::Function(expected_function)), Some(Type::Function(actual_function))) => {
                self.infer_overload_function_type(
                    &expected_function,
                    &actual_function,
                    substitutions,
                )
            }
            (Some(Type::Union(expected_items)), _) => {
                self.infer_overload_union_branch(expected_items, actual, substitutions)
            }
            (_, Some(Type::Union(actual_items))) => {
                self.infer_overload_actual_union_branch(expected, actual_items, substitutions)
            }
            _ => false,
        }
    }

    /// Infer against the first compatible expected union member and keep its substitutions.
    fn infer_overload_union_branch(
        &mut self,
        expected_items: Vec<smelt_hir::TypeId>,
        actual: smelt_hir::TypeId,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        for item in expected_items {
            let mut branch_substitutions = substitutions.clone();
            if self.infer_overload_type(item, actual, &mut branch_substitutions) {
                *substitutions = branch_substitutions;
                return true;
            }
        }
        false
    }

    /// Infer against the first compatible actual union member and keep its substitutions.
    fn infer_overload_actual_union_branch(
        &mut self,
        expected: smelt_hir::TypeId,
        actual_items: Vec<smelt_hir::TypeId>,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        for item in actual_items {
            let mut branch_substitutions = substitutions.clone();
            if self.infer_overload_type(expected, item, &mut branch_substitutions) {
                *substitutions = branch_substitutions;
                return true;
            }
        }
        false
    }

    /// Infer compatibility for function-typed overload parameters.
    fn infer_overload_function_type(
        &mut self,
        expected: &FunctionType,
        actual: &FunctionType,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        if expected.is_async != actual.is_async {
            return false;
        }
        if actual.params.len() > expected.params.len()
            && (actual.rest != actual.params.len().checked_sub(1)
                || actual.params.len() != expected.params.len() + 1)
        {
            return false;
        }
        let mut actual_substitutions = HashMap::new();
        for (expected_param, actual_param) in expected.params.iter().zip(&actual.params) {
            let expected_param = self.substitute_type_params(*expected_param, substitutions);
            if !self.infer_callable_parameter_type(
                expected_param,
                *actual_param,
                &mut actual_substitutions,
            ) {
                return false;
            }
        }
        let actual_return_ty = self.substitute_type_params(actual.return_ty, &actual_substitutions);
        self.infer_overload_type(expected.return_ty, actual_return_ty, substitutions)
    }

    /// Check that an actual callback can accept the input required by an overload parameter.
    fn infer_callable_parameter_type(
        &mut self,
        required_input: smelt_hir::TypeId,
        actual_param: smelt_hir::TypeId,
        actual_substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        let actual_param = self.substitute_type_params(actual_param, actual_substitutions);
        if actual_param == required_input {
            return true;
        }
        match self.ctx.krate.types.get(actual_param).cloned() {
            Some(Type::TypeParam { name }) => {
                actual_substitutions.insert(name, required_input);
                true
            }
            Some(Type::Unknown) => true,
            _ => self.infer_overload_type(actual_param, required_input, actual_substitutions),
        }
    }

    /// Lower a call whose callee is a local closure or function-typed local.
    fn local_callable_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee_ident) = &call.callee else {
            return Ok(None);
        };
        if self.items.contains_key(callee_ident.name.as_str())
            && !self.locals.contains_key(callee_ident.name.as_str())
        {
            return Ok(None);
        }
        let callee = self.identifier_expression(
            callee_ident.name.as_str(),
            callee_ident.span.start,
            callee_ident.span.end,
            body,
        )?;
        let callee_ty = Self::expr_ty(body, callee);
        // A `typeof callee === "function"` guard narrows the *expression* type to
        // a concrete function, but the backing local keeps its declared type. When
        // that declared type is a dynamic surface (union/unknown/type parameter/
        // erased callable), codegen still dispatches the call through the runtime
        // `SmeltUnknown` ABI. Track the declared type so a spread call is packed
        // into a single flattened argument vector even after narrowing hides the
        // dynamic surface behind a function type.
        let callee_dispatches_dynamically = self.type_is_dynamic_call_surface(callee_ty)
            || self
                .locals
                .get(callee_ident.name.as_str())
                .is_some_and(|&local| self.type_is_dynamic_call_surface(Self::local_ty(body, local)));
        let optional_function_ty = match self.ctx.krate.types.get(callee_ty) {
            Some(Type::Optional(inner))
                if call.optional
                    && matches!(self.ctx.krate.types.get(*inner), Some(Type::Function(_))) =>
            {
                Some(*inner)
            }
            _ => None,
        };
        let function_member_ty = optional_function_ty.or_else(|| {
            self.function_member_type_for_arg_count(callee_ty, Some(call.arguments.len()))
        });
        let (function_ty, function, optional_call) = match function_member_ty {
            Some(function_ty) => {
                let Some(Type::Function(function)) = self.ctx.krate.types.get(function_ty).cloned()
                else {
                    return Ok(None);
                };
                (function_ty, function, optional_function_ty.is_some())
            }
            _ if matches!(
                self.ctx.krate.types.get(callee_ty),
                Some(Type::Unknown | Type::TypeParam { .. })
            ) || self.is_callable_object_erased_class(callee_ty) =>
            {
                let ty = self.ctx.krate.types.intern(Type::Unknown);
                let callee = if self.is_callable_object_erased_class(callee_ty) {
                    body.push_expr(Expr {
                        kind: ExprKind::TypeAssert { value: callee },
                        ty,
                        span: self.span(callee_ident.span.start, callee_ident.span.end),
                    })
                } else {
                    callee
                };
                // Dynamically dispatched callees (unknown/type-parameter/erased
                // callable surfaces) are invoked through the runtime
                // `SmeltUnknown` call ABI, which flattens the entire argument
                // list. A spread call must therefore pack its flattened
                // arguments into a single runtime list operand and lower as
                // `ClosureCallSpread`. Emitting a dynamic `ClosureCall` here
                // would lose the spread, leaving the call-site flatten
                // heuristic to misread the individual operands.
                if self.call_has_spread_arguments_or_source_spread(call) {
                    let args = self.packed_spread_call_arguments(ty, call, body)?;
                    return Ok(Some(body.push_expr(Expr {
                        kind: ExprKind::ClosureCallSpread { callee, args },
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    })));
                }
                let args = call
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall { callee, args },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            _ => {
                return Ok(None);
            }
        };
        let callee = if callee_ty == function_ty {
            callee
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value: callee },
                ty: function_ty,
                span: self.span(callee_ident.span.start, callee_ident.span.end),
            })
        };
        let call_return_ty = if optional_call {
            self.optional_chain_result_type(function.return_ty)
        } else {
            function.return_ty
        };
        let supplied_arg_count = call.arguments.len();
        let callback_meta = self
            .local_callbacks
            .get(callee_ident.name.as_str())
            .cloned();
        // Dynamically dispatched callees (union/unknown/type-parameter/erased
        // callable surfaces) are invoked through the runtime `SmeltUnknown` call
        // ABI, which flattens the entire argument list. A spread must therefore
        // be packed into a single runtime argument vector even when the callee
        // carries local-callback metadata: the typed-shape `[fixed.., rest_list]`
        // lowering below is only correct for a concrete function whose Rust rest
        // parameter absorbs the trailing list, not for a dynamic dispatch that
        // would otherwise wrap the rest list in a single extra `Array` argument.
        if self.call_has_spread_arguments_or_source_spread(call) && callee_dispatches_dynamically {
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
            let packed = self.packed_spread_call_arguments(unknown_ty, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread {
                    callee,
                    args: packed,
                },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let variadic_item_ty = function.params.first().and_then(|param| {
            if function.rest.is_some() {
                return None;
            }
            if function.params.len() != 1 {
                return None;
            }
            match self.ctx.krate.types.get(*param) {
                Some(Type::List(item_ty)) => Some(*item_ty),
                Some(Type::Never) => Some(self.ctx.krate.types.intern(Type::Unknown)),
                _ => None,
            }
        });
        if callback_meta.is_none()
            && self.call_has_spread_arguments_or_source_spread(call)
            && let Some(item_ty) = variadic_item_ty
        {
            let packed = self.packed_spread_call_arguments(item_ty, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCallSpread {
                    callee,
                    args: packed,
                },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let defaults = callback_meta.as_ref().map_or_else(
            || vec![None; function.params.len()],
            |callback| callback.defaults.clone(),
        );
        let rest = callback_meta
            .as_ref()
            .and_then(|callback| callback.rest)
            .or_else(|| {
                function.rest.and_then(|index| {
                    let param = self.type_param_constraint_or_self(*function.params.get(index)?);
                    match self.ctx.krate.types.get(param) {
                        Some(Type::List(item_ty)) => Some(RestParam {
                            index,
                            item_ty: *item_ty,
                        }),
                        _ => None,
                    }
                })
            });
        let fixed_param_count = rest.map_or(function.params.len(), |rest| rest.index);
        if rest.is_some() && self.call_has_spread_arguments_or_source_spread(call) {
            let args = self.spread_closure_call_arguments(&function, rest, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCall { callee, args },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let required_arg_count = defaults
            .iter()
            .take(fixed_param_count)
            .position(Option::is_some)
            .unwrap_or(fixed_param_count);
        if supplied_arg_count < required_arg_count
            || (rest.is_none() && supplied_arg_count > function.params.len())
        {
            if rest.is_none() {
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall {
                        callee,
                        args: Vec::new(),
                    },
                    ty: call_return_ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "closure call argument count does not match closure parameters",
            ));
        }
        if rest.is_none() && call.arguments.iter().any(Argument::is_spread) {
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCall {
                    callee,
                    args: Vec::new(),
                },
                ty: call_return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let mut args = call
            .arguments
            .iter()
            .take(fixed_param_count)
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        for index in supplied_arg_count..fixed_param_count {
            let Some(default) = defaults.get(index).and_then(|default| default.as_ref()) else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "closure call argument count does not match closure parameters",
                ));
            };
            let default = self.local_callback_default_expr(
                default,
                &args,
                body,
                self.span(call.span.start, call.span.end),
            )?;
            args.push(default);
        }
        if let Some(rest) = rest {
            let rest_args = call
                .arguments
                .iter()
                .skip(rest.index)
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            let rest_ty = self.ctx.krate.types.intern(Type::List(rest.item_ty));
            args.push(body.push_expr(Expr {
                kind: ExprKind::ListLit(rest_args),
                ty: rest_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: call_return_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether a local's structural class type is an erased callable object.
    fn is_callable_object_erased_class(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Class { name, .. }) => self.callable_object_aliases.contains(name),
            _ => false,
        }
    }

    /// Return whether a callee of this type is invoked through the runtime
    /// `SmeltUnknown` call ABI rather than a concrete Rust function.
    ///
    /// Such callees (unknown, type parameters, unions, and erased callable
    /// objects) flatten their entire argument list at the call site, so spread
    /// arguments must be packed into a single runtime vector rather than passed
    /// as a trailing rest list.
    fn type_is_dynamic_call_surface(&self, ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_callable_object_erased_class(ty)
    }

    /// Return whether a `Type::Class` value erases to `SmeltUnknown` at codegen.
    ///
    /// This mirrors the Rust emitter's `is_erased_class_type` rule: a class-shaped
    /// type is emitted as `SmeltUnknown` when its name resolves to neither a
    /// declared class nor a declared interface. Generic structural type aliases
    /// such as `Funnel<Args>` (a `type` literal, not an `interface`) therefore
    /// erase, while `RegExp` keeps its dedicated runtime type. Keeping the
    /// predicate aligned with codegen lets the lowering decide, in the frontend,
    /// whether a receiver value will be a runtime `SmeltUnknown` at the call site.
    fn class_type_erases_to_unknown(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty) else {
            return false;
        };
        let Some(type_name) = self.ctx.krate.symbols.get(*name) else {
            return false;
        };
        if Self::is_ts_stdlib_class_name(type_name, smelt_stdlib::StdlibClass::RegExp) {
            return false;
        }
        !self.classes.contains_key(type_name) && !self.interfaces.contains_key(type_name)
    }

    /// Return whether a value of this type dispatches member calls through the
    /// runtime `SmeltUnknown` ABI rather than as a typed Rust value.
    ///
    /// A receiver dispatches dynamically when it is itself `unknown`, a type
    /// parameter, a union, or a class-shaped type that erases to `SmeltUnknown`
    /// (see [`Self::class_type_erases_to_unknown`]). Optional receivers unwrap to
    /// their inner type first so `Funnel<Args> | undefined` is recognized too.
    fn receiver_type_dispatches_dynamically(&self, ty: smelt_hir::TypeId) -> bool {
        let inner = match self.ctx.krate.types.get(ty) {
            Some(Type::Optional(inner)) => *inner,
            _ => ty,
        };
        matches!(
            self.ctx.krate.types.get(inner),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.class_type_erases_to_unknown(inner)
    }

    /// Return whether the receiving object of a static member call erases to
    /// `SmeltUnknown` at codegen.
    ///
    /// The receiver is lowered to recover its type; the produced expression is
    /// otherwise unused here because the member-read callee already lowered the
    /// receiver. When the receiver erases, the member read yields a runtime
    /// `SmeltUnknown`, so a spread call through it must be packed as a
    /// `ClosureCallSpread` regardless of the field's declared function type.
    fn static_member_receiver_dispatches_dynamically(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> bool {
        let Ok(receiver) = self.expression(&member.object, body) else {
            return false;
        };
        let receiver_ty = Self::expr_ty(body, receiver);
        self.receiver_type_dispatches_dynamically(receiver_ty)
    }

    /// Expand a JavaScript spread call into fixed function parameters plus rest.
    fn spread_closure_call_arguments(
        &mut self,
        function: &FunctionType,
        rest: Option<RestParam>,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let rest = rest.ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "spread closure calls require a rest-like list parameter",
            )
        })?;
        let fixed_param_count = rest.index;
        let mut args = Vec::new();
        let rest_ty = self.ctx.krate.types.intern(Type::List(rest.item_ty));
        let mut rest_items = Vec::new();
        let mut rest_list = None;
        let mut fixed_index = 0;
        for argument in &call.arguments {
            if fixed_index < fixed_param_count {
                let Argument::SpreadElement(spread) = argument else {
                    args.push(self.argument(argument, body)?);
                    fixed_index += 1;
                    continue;
                };
                let spread_list =
                    self.rest_spread_list_expression(&spread.argument, rest_ty, spread.span, body)?;
                let consumed = fixed_param_count - fixed_index;
                for offset in 0..consumed {
                    let index = self.usize_float_literal(
                        offset,
                        self.span(spread.span.start, spread.span.end),
                        body,
                    )?;
                    let ty = function
                        .params
                        .get(fixed_index + offset)
                        .copied()
                        .unwrap_or_else(|| {
                            self.index_type(Self::expr_ty(body, spread_list))
                                .unwrap_or(rest.item_ty)
                        });
                    let kind = if matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
                        ExprKind::OptionalIndex {
                            receiver: spread_list,
                            index,
                        }
                    } else {
                        ExprKind::Index {
                            receiver: spread_list,
                            index,
                        }
                    };
                    args.push(body.push_expr(Expr {
                        kind,
                        ty,
                        span: self.span(spread.span.start, spread.span.end),
                    }));
                }
                fixed_index = fixed_param_count;
                let spread_rest = if consumed == 0 {
                    spread_list
                } else {
                    let start = self.usize_float_literal(
                        consumed,
                        self.span(spread.span.start, spread.span.end),
                        body,
                    )?;
                    body.push_expr(Expr {
                        kind: ExprKind::ListSlice {
                            list: spread_list,
                            start: Some(start),
                            end: None,
                        },
                        ty: rest_ty,
                        span: self.span(spread.span.start, spread.span.end),
                    })
                };
                rest_list = Some(rest_list.map_or(spread_rest, |left| {
                    body.push_expr(Expr {
                        kind: ExprKind::ListConcat {
                            left,
                            right: spread_rest,
                        },
                        ty: rest_ty,
                        span: self.span(spread.span.start, spread.span.end),
                    })
                }));
                continue;
            }
            match argument {
                Argument::SpreadElement(spread) => {
                    Self::flush_rest_items(
                        &mut rest_items,
                        &mut rest_list,
                        rest_ty,
                        body,
                        self.span(spread.span.start, spread.span.end),
                    );
                    let spread_list =
                        self.rest_spread_list_expression(&spread.argument, rest_ty, spread.span, body)?;
                    rest_list = Some(rest_list.map_or(spread_list, |left| {
                        body.push_expr(Expr {
                            kind: ExprKind::ListConcat {
                                left,
                                right: spread_list,
                            },
                            ty: rest_ty,
                            span: self.span(spread.span.start, spread.span.end),
                        })
                    }));
                }
                _ => rest_items.push(self.argument(argument, body)?),
            }
        }
        if fixed_index < fixed_param_count {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "spread closure call does not provide enough fixed parameters",
            ));
        }
        Self::flush_rest_items(
            &mut rest_items,
            &mut rest_list,
            rest_ty,
            body,
            self.span(call.span.start, call.span.end),
        );
        args.push(rest_list.unwrap_or_else(|| {
            body.push_expr(Expr {
                kind: ExprKind::ListLit(Vec::new()),
                ty: rest_ty,
                span: self.span(call.span.start, call.span.end),
            })
        }));
        Ok(args)
    }

    /// Convert an erased array spread argument to the statically required rest-list type.
    ///
    /// Generic array parameters are represented as erased runtime values even
    /// when their source constraint is an array. A rest call still needs their
    /// array payload for concatenation, so extract that payload before building
    /// the packed argument vector.
    fn rest_spread_list_expression(
        &mut self,
        expression: &Expression<'_>,
        rest_ty: smelt_hir::TypeId,
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = self.expression(expression, body)?;
        let value_ty = Self::expr_ty(body, value);
        if value_ty == rest_ty {
            return Ok(value);
        }
        if self.type_contains_unknown(value_ty)
            || matches!(
                self.ctx.krate.types.get(value_ty),
                Some(Type::TypeParam { .. } | Type::Union(_))
            )
        {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::UnknownCast {
                    value,
                    target: rest_ty,
                },
                ty: rest_ty,
                span: self.span(span.start, span.end),
            }));
        }
        Ok(value)
    }

    /// Lower a small positional index as the numeric literal used by JS indexing.
    fn usize_float_literal(
        &mut self,
        value: usize,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value = value.to_string().parse::<f64>().map_err(|err| {
            SmeltError::unsupported(span, format!("spread index cannot be represented: {err}"))
        })?;
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(value)),
            ty,
            span,
        }))
    }

    /// Append queued rest items to the accumulated rest list expression.
    fn flush_rest_items(
        rest_items: &mut Vec<smelt_hir::ExprId>,
        rest_list: &mut Option<smelt_hir::ExprId>,
        rest_ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) {
        if rest_items.is_empty() {
            return;
        }
        let right = body.push_expr(Expr {
            kind: ExprKind::ListLit(std::mem::take(rest_items)),
            ty: rest_ty,
            span,
        });
        *rest_list = Some(rest_list.map_or(right, |left| {
            body.push_expr(Expr {
                kind: ExprKind::ListConcat { left, right },
                ty: rest_ty,
                span,
            })
        }));
    }

    /// Pack JavaScript spread call arguments into one variadic list argument.
    fn packed_spread_call_arguments(
        &mut self,
        item_ty: smelt_hir::TypeId,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let mut current_items = Vec::new();
        let mut packed = None;
        for arg in &call.arguments {
            match arg {
                Argument::SpreadElement(spread) => {
                    if !current_items.is_empty() {
                        let left = body.push_expr(Expr {
                            kind: ExprKind::ListLit(std::mem::take(&mut current_items)),
                            ty: list_ty,
                            span: self.span(call.span.start, call.span.end),
                        });
                        packed = Some(packed.map_or(left, |existing| {
                            body.push_expr(Expr {
                                kind: ExprKind::ListConcat {
                                    left: existing,
                                    right: left,
                                },
                                ty: list_ty,
                                span: self.span(call.span.start, call.span.end),
                            })
                        }));
                    }
                    let spread_expr = self.expression(&spread.argument, body)?;
                    packed = Some(packed.map_or(spread_expr, |existing| {
                        body.push_expr(Expr {
                            kind: ExprKind::ListConcat {
                                left: existing,
                                right: spread_expr,
                            },
                            ty: list_ty,
                            span: self.span(call.span.start, call.span.end),
                        })
                    }));
                }
                other => current_items.push(self.argument(other, body)?),
            }
        }
        if !current_items.is_empty() {
            let right = body.push_expr(Expr {
                kind: ExprKind::ListLit(current_items),
                ty: list_ty,
                span: self.span(call.span.start, call.span.end),
            });
            packed = Some(packed.map_or(right, |existing| {
                body.push_expr(Expr {
                    kind: ExprKind::ListConcat {
                        left: existing,
                        right,
                    },
                    ty: list_ty,
                    span: self.span(call.span.start, call.span.end),
                })
            }));
        }
        packed.ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "spread call requires at least one argument",
            )
        })
    }

    /// Lower type-test-only assertion calls to a no-op expression.
    ///
    /// APIs such as Vitest's `expectTypeOf` and `expect-type` assertions exist
    /// to make TypeScript's checker verify source types. They do not represent
    /// runtime behavior, so Smelt lowers the value under test to keep ordinary
    /// expression/type errors visible and erases the assertion call itself.
    fn type_test_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(root_call) = self.type_test_root_call(call) else {
            return Ok(None);
        };
        if let Some(value) = root_call.arguments.first() {
            let saved_allow_unknown_index_access = self.allow_unknown_index_access;
            self.allow_unknown_index_access = true;
            let result = self.argument(value, body);
            self.allow_unknown_index_access = saved_allow_unknown_index_access;
            let _ = result?;
        }
        let ty = self.ctx.krate.types.intern(Type::None);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Return whether a call uses JavaScript spread arguments.
    fn call_has_spread_arguments(call: &oxc::ast::ast::CallExpression<'_>) -> bool {
        call.arguments
            .iter()
            .any(|argument| matches!(argument, Argument::SpreadElement(_)))
    }

    /// Return whether a call contains JavaScript spread syntax.
    ///
    /// Some parser surfaces can preserve the argument expression while hiding
    /// the spread wrapper after TypeScript-specific normalization. The source
    /// span probe keeps spread-call lowering tied to source syntax instead of
    /// treating the argument as an ordinary array value.
    fn call_has_spread_arguments_or_source_spread(
        &self,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> bool {
        Self::call_has_spread_arguments(call)
            || self
                .source
                .get(
                    usize::try_from(call.span.start).unwrap_or(0)
                        ..usize::try_from(call.span.end).unwrap_or(0),
                )
                .is_some_and(|text| text.contains("..."))
            || call.arguments.iter().any(|argument| {
                let span = argument.span();
                let start = usize::try_from(call.span.start).unwrap_or(0);
                let argument_start = usize::try_from(span.start).unwrap_or(start);
                if self
                    .source
                    .get(argument_start..)
                    .is_some_and(|suffix| suffix.starts_with("..."))
                {
                    return true;
                }
                let end = argument_start;
                self.source
                    .get(start..end)
                    .is_some_and(|prefix| prefix.trim_end().ends_with("..."))
            })
    }

    /// Lower Remeda's `$typed<T>()` declaration-test helper to an opaque value.
    fn typed_test_value_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Option<smelt_hir::ExprId> {
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        if callee.name != "$typed" || !self.allow_unknown_index_access {
            return None;
        }
        let ty = self.ctx.krate.types.intern(Type::Unknown);
        Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
        }))
    }

    /// Fold Remeda partial-bind nested calls into direct closure calls.
    fn partial_bind_nested_call(
        &mut self,
        callee_call: &oxc::ast::ast::CallExpression<'_>,
        outer_call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &callee_call.callee else {
            return Ok(None);
        };
        let append_partial_last = match callee.name.as_str() {
            "partialBind" => false,
            "partialLastBind" => true,
            _ => return Ok(None),
        };
        let Some((function_arg, prefix_args)) = callee_call.arguments.split_first() else {
            return Err(SmeltError::unsupported(
                self.span(callee_call.span.start, callee_call.span.end),
                "partialBind requires a function argument",
            ));
        };
        let mut callee_expr = self.argument(function_arg, body)?;
        let mut function_ty = self
            .ctx
            .krate
            .types
            .get(Self::expr_ty(body, callee_expr))
            .cloned();
        if !matches!(function_ty, Some(Type::Function(_)))
            && let Argument::Identifier(identifier) = function_arg
            && let Some(callback) = self.local_callbacks.get(identifier.name.as_str()).cloned()
        {
            callee_expr = self.callback_expr_to_closure_with_return_ty(
                callback.return_ty,
                &callback.callback,
                &callback.params,
                callback.rest.map(|rest| rest.index),
                callback.required_params,
                self.span(identifier.span.start, identifier.span.end),
                body,
            )?;
            function_ty = Some(Type::Function(FunctionType {
                params: callback.params,
                rest: callback.rest.map(|rest| rest.index),
                required_params: callback.required_params,
                    mutable_params: Vec::new(),
                return_ty: callback.return_ty,
                is_async: false,
                may_throw: false,
            }));
        }
        let Some(Type::Function(function)) = function_ty else {
            return Err(SmeltError::unsupported(
                self.span(function_arg.span().start, function_arg.span().end),
                "partialBind function argument must be callable",
            ));
        };
        let mut args = Vec::new();
        let ordered_args: Box<dyn Iterator<Item = &Argument<'_>>> = if append_partial_last {
            Box::new(outer_call.arguments.iter().chain(prefix_args.iter()))
        } else {
            Box::new(prefix_args.iter().chain(outer_call.arguments.iter()))
        };
        for arg in ordered_args {
            args.push(self.argument(arg, body)?);
        }
        if args.len() != function.params.len() {
            return Err(SmeltError::unsupported(
                self.span(outer_call.span.start, outer_call.span.end),
                "partialBind folded call argument count does not match function",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ClosureCall {
                callee: callee_expr,
                args,
            },
            ty: function.return_ty,
            span: self.span(outer_call.span.start, outer_call.span.end),
        })))
    }

    /// Fold Remeda `sliceString(start, end?)(data)` into a string slice.
    fn slice_string_nested_call(
        &mut self,
        callee_call: &oxc::ast::ast::CallExpression<'_>,
        outer_call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &callee_call.callee else {
            return Ok(None);
        };
        if callee.name != "sliceString" {
            return Ok(None);
        }
        let [data_arg] = outer_call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(outer_call.span.start, outer_call.span.end),
                "sliceString data-last call requires one data argument",
            ));
        };
        let (start_arg, end_arg) = match callee_call.arguments.as_slice() {
            [start] => (start, None),
            [start, end] => (start, Some(end)),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(callee_call.span.start, callee_call.span.end),
                    "sliceString data-last call requires start and optional end",
                ));
            }
        };
        let operand = self.argument(data_arg, body)?;
        let start = Some(self.slice_index_argument(start_arg, body)?);
        let end = end_arg
            .map(|argument| self.slice_index_argument(argument, body))
            .transpose()?;
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSlice {
                operand,
                start,
                end,
            },
            ty,
            span: self.span(outer_call.span.start, outer_call.span.end),
        })))
    }

    /// Return the root `expectTypeOf(...)`-style call for a type-test chain.
    fn type_test_root_call<'a>(
        &self,
        call: &'a oxc::ast::ast::CallExpression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        if self.is_type_test_root_callee(&call.callee) {
            return Some(call);
        }
        match &call.callee {
            Expression::StaticMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            Expression::ComputedMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            _ => None,
        }
    }

    /// Return the root type-test call for an expression in a member chain.
    fn type_test_root_expression<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        match expression {
            Expression::CallExpression(call) => self.type_test_root_call(call),
            Expression::StaticMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            Expression::ComputedMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.type_test_root_expression(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                self.type_test_root_expression(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.type_test_root_expression(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.type_test_root_expression(&non_null.expression)
            }
            _ => None,
        }
    }

    /// Return whether `callee` starts a supported type-test assertion chain.
    fn is_type_test_root_callee(&self, callee: &Expression<'_>) -> bool {
        matches!(
            callee,
            Expression::Identifier(ident)
                if self.test_builtins.contains(ident.name.as_str())
                    && test_support::is_type_test_builtin_name(ident.name.as_str())
        )
    }

    /// Strip transparent wrappers around an asserted function-call callee.
    fn transparent_asserted_callee<'a>(callee: &'a Expression<'a>) -> Option<&'a Expression<'a>> {
        match callee {
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::transparent_asserted_callee(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => Some(&as_expr.expression),
            Expression::TSSatisfiesExpression(satisfies) => Some(&satisfies.expression),
            Expression::TSNonNullExpression(non_null) => Some(&non_null.expression),
            _ => None,
        }
    }
}
