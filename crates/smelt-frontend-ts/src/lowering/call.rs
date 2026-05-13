impl ModuleBuilder<'_> {
    /// Lower call expressions, including stdlib shims and direct function/method invokes.
    fn call_expression(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(expr) = self.date_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.regex_replace_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.object_assign_call(call, body)? {
            return Ok(expr);
        }
        if let Some(error) = self.unsupported_object_collection_call(call) {
            return Err(error);
        }
        if let Some(expr) = self.promise_static_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.timer_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.fetch_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.primitive_cast_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.math_abs_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.math_round_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.math_extrema_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.math_hypot_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.number_predicate_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.number_parse_float_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.number_parse_int_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.number_to_string_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.node_process_version_match_call(call, body) {
            return Ok(expr);
        }
        if let Some(expr) = self.math_unary_func_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.math_random_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.math_pow_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.math_atan2_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.object_from_entries_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.object_projection_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.object_has_own_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.map_has_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.map_get_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.map_mutation_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.map_projection_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.set_projection_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.array_is_array_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.json_stringify_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.json_parse_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.regexp_test_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_case_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_trim_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_affix_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_callback_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_reduce_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_search_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.collection_at_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_search_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_replace_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.collection_slice_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_push_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_unshift_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_reverse_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_sort_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.modern_array_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_pop_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_shift_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_repeat_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_pad_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_char_at_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_join_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_concat_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_contains_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.set_contains_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.set_mutation_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_contains_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_split_call(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.namespace_member_call(call, body)? {
            return Ok(expr);
        }
        if let Expression::ComputedMemberExpression(member) = &call.callee {
            let args = call
                .arguments
                .iter()
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
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
            && member.property.name == "log"
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
        if let Expression::StaticMemberExpression(member) = &call.callee {
            let receiver = self.expression(&member.object, body)?;
            let method = self.intern_source_name(member.property.name.as_str());
            let receiver_ty = Self::expr_ty(body, receiver);
            let optional_access =
                call.optional || member.optional || matches!(self.ctx.krate.types.get(receiver_ty), Some(Type::Optional(_)));
            let access_receiver_ty = self.optional_receiver_inner_type(receiver_ty);
            let (return_ty, _) = self.resolve_method(access_receiver_ty, method)?;
            let mut args = Vec::new();
            for arg in &call.arguments {
                args.push(self.argument(arg, body)?);
            }
            if optional_access {
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
        if let Expression::Identifier(callee_ident) = &call.callee {
            if self.locals.contains_key(callee_ident.name.as_str()) {
                let callee = self.identifier_expression(
                    callee_ident.name.as_str(),
                    callee_ident.span.start,
                    callee_ident.span.end,
                    body,
                )?;
                let Some(Type::Function(function)) =
                    self.ctx.krate.types.get(Self::expr_ty(body, callee)).cloned()
                else {
                    return Err(SmeltError::unsupported(
                        self.span(callee_ident.span.start, callee_ident.span.end),
                        format!("local `{}` is not callable", callee_ident.name),
                    ));
                };
                let mut args = Vec::new();
                for arg in &call.arguments {
                    args.push(self.argument(arg, body)?);
                }
                if function
                    .params
                    .iter()
                    .any(|param| self.type_contains_never(*param))
                {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "calls through function types with never parameters are not lowered",
                    ));
                }
                return Ok(body.push_expr(Expr {
                    kind: ExprKind::ClosureCall { callee, args },
                    ty: function.return_ty,
                    span: self.span(call.span.start, call.span.end),
                }));
            }
            let Some(item) = self.items.get(callee_ident.name.as_str()).copied() else {
                return Err(SmeltError::unsupported(
                    self.span(callee_ident.span.start, callee_ident.span.end),
                    format!("unresolved function `{}`", callee_ident.name),
                ));
            };
            let (params, return_ty, is_async) = if let Item::Function(function) = self.item_ref(item)
            {
                (
                    function
                        .params
                        .iter()
                        .map(|param| param.ty)
                        .collect::<Vec<_>>(),
                    function.return_ty,
                    function.is_async,
                )
            } else {
                return Err(SmeltError::unsupported(
                    self.span(callee_ident.span.start, callee_ident.span.end),
                    "callee item is not a function",
                ));
            };
            let rest = self.function_rests.get(callee_ident.name.as_str()).copied();
            if params.iter().any(|param| self.type_contains_never(*param)) {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "calls through function types with never parameters are not lowered",
                ));
            }
            let fixed_param_count = rest.map_or(params.len(), |rest| rest.index);
            if rest.is_none() && call.arguments.len() > fixed_param_count {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "function call argument count does not match parameters",
                ));
            }
            let mut args = call
                .arguments
                .iter()
                .take(fixed_param_count)
                .map(|arg| self.argument(arg, body))
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
            let callee = body.push_expr(Expr {
                kind: ExprKind::Item(item),
                ty: self.ctx.krate.types.intern(Type::Function(FunctionType {
                    params,
                    return_ty,
                    is_async,
                })),
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

    /// Lower a call whose callee is a local closure or function-typed local.
    fn local_callable_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee_ident) = &call.callee else {
            return Ok(None);
        };
        if self.items.contains_key(callee_ident.name.as_str()) {
            return Ok(None);
        }
        let callee = self.identifier_expression(
            callee_ident.name.as_str(),
            callee_ident.span.start,
            callee_ident.span.end,
            body,
        )?;
        let callee_ty = Self::expr_ty(body, callee);
        let Some(Type::Function(function)) = self.ctx.krate.types.get(callee_ty).cloned() else {
            return Ok(None);
        };
        let supplied_arg_count = call.arguments.len();
        let callback_meta = self.local_callbacks.get(callee_ident.name.as_str()).cloned();
        let variadic_item_ty = function.params.first().and_then(|param| {
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
            && call.arguments.iter().any(Argument::is_spread)
            && let Some(item_ty) = variadic_item_ty
        {
            let packed = self.packed_spread_call_arguments(item_ty, call, body)?;
            return Ok(Some(body.push_expr(Expr {
                kind: ExprKind::ClosureCall {
                    callee,
                    args: vec![packed],
                },
                ty: function.return_ty,
                span: self.span(call.span.start, call.span.end),
            })));
        }
        let defaults = callback_meta
            .as_ref()
            .map_or_else(|| vec![None; function.params.len()], |callback| {
                callback.defaults.clone()
            });
        let rest = callback_meta.as_ref().and_then(|callback| callback.rest);
        let fixed_param_count = rest.map_or(function.params.len(), |rest| rest.index);
        let required_arg_count = defaults
            .iter()
            .take(fixed_param_count)
            .position(Option::is_some)
            .unwrap_or(fixed_param_count);
        if supplied_arg_count < required_arg_count
            || (rest.is_none() && supplied_arg_count > function.params.len())
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "closure call argument count does not match closure parameters",
            ));
        }
        let mut args = call
            .arguments
            .iter()
            .take(fixed_param_count)
            .map(|arg| self.argument(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        for index in supplied_arg_count..fixed_param_count {
            let Some(default) = defaults.get(index).and_then(|default| *default) else {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "closure call argument count does not match closure parameters",
                ));
            };
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
            ty: function.return_ty,
            span: self.span(call.span.start, call.span.end),
        })))
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
}
