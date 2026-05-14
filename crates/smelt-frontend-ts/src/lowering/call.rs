impl ModuleBuilder<'_> {
    /// Lower call expressions, including stdlib shims and direct function/method invokes.
    fn call_expression(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(expr) = self.type_test_call(call, body)? {
            return Ok(expr);
        }
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
        if let Some(expr) = self.symbol_call(call, body)? {
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
        if let Expression::CallExpression(callee_call) = &call.callee {
            let callee = self.call_expression(callee_call, body)?;
            let Some(Type::Function(function)) =
                self.ctx.krate.types.get(Self::expr_ty(body, callee)).cloned()
            else {
                return Err(SmeltError::unsupported(
                    self.span(callee_call.span.start, callee_call.span.end),
                    "call expression callee must return a function",
                ));
            };
            if call.arguments.len() != function.params.len() {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "curried call argument count does not match selected overload",
                ));
            }
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
                    .any(|param| self.concrete_type_requires_never_value(*param))
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
            let (params, implementation_return_ty, is_async) = if let Item::Function(function) = self.item_ref(item)
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
            let mut rest = self.function_rests.get(callee_ident.name.as_str()).copied();
            let selected_overload = self.selected_overload_signature(
                callee_ident.name.as_str(),
                &call.arguments,
                call.span,
                body,
            )?;
            if rest.is_none()
                && selected_overload.is_some()
                && params.len() == 1
                && let Some(param_ty) = params.first()
                && let Some(Type::List(item_ty)) = self.ctx.krate.types.get(*param_ty)
            {
                rest = Some(RestParam {
                    index: 0,
                    item_ty: *item_ty,
                });
            }
            let return_ty = selected_overload
                .as_ref()
                .map_or(implementation_return_ty, |signature| signature.return_ty);
            if params
                .iter()
                .any(|param| self.concrete_type_requires_never_value(*param))
            {
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
                    return_ty: implementation_return_ty,
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

    /// Lower JavaScript `Symbol(description)` branding calls as opaque strings.
    ///
    /// Smelt does not model symbol identity yet. Type-level branding libraries
    /// only need a stable opaque value here so the containing module can lower.
    fn symbol_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "Symbol" {
            return Ok(None);
        }
        if call.arguments.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Symbol(...) supports at most one description argument",
            ));
        }
        let description = call
            .arguments
            .first()
            .map(|argument| self.argument(argument, body))
            .transpose()?;
        let ty = self.ctx.krate.types.intern(Type::String);
        if let Some(description) = description
            && !matches!(self.ctx.krate.types.get(Self::expr_ty(body, description)), Some(Type::String))
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Symbol(...) description must be a string",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::String("symbol".to_owned())),
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
        let Some(signatures) = self
            .function_overloads
            .get(name)
            .cloned()
            .or_else(|| self.ctx.overloads.get(name).cloned())
        else {
            return Ok(None);
        };
        let mut lowered_arg_tys = Vec::new();
        for argument in arguments {
            let arg = self.argument(argument, body)?;
            lowered_arg_tys.push(Self::expr_ty(body, arg));
        }
        for signature in signatures {
            if signature.params.len() != lowered_arg_tys.len() {
                continue;
            }
            let mut substitutions = HashMap::new();
            if signature
                .params
                .iter()
                .zip(&lowered_arg_tys)
                .all(|(expected, actual)| {
                    self.infer_overload_type(*expected, *actual, &mut substitutions)
                })
            {
                return Ok(Some(self.instantiate_overload_signature(signature, &substitutions)));
            }
        }
        Err({
                SmeltError::unsupported(
                    self.span(span.start, span.end),
                    format!("no overload of `{name}` matches this call"),
                )
            })
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
            params,
            return_ty,
            is_async: signature.is_async,
        }
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
            (Some(Type::Optional(inner)), _) if inner == actual => true,
            (Some(Type::Optional(inner)), _) => self.infer_overload_type(inner, actual, substitutions),
            (Some(Type::List(expected_item)), Some(Type::List(actual_item)))
            | (Some(Type::Set(expected_item)), Some(Type::Set(actual_item)))
            | (Some(Type::Future(expected_item)), Some(Type::Future(actual_item))) => {
                self.infer_overload_type(expected_item, actual_item, substitutions)
            }
            (Some(Type::Dict(expected_key, expected_value)), Some(Type::Dict(actual_key, actual_value))) => {
                self.infer_overload_type(expected_key, actual_key, substitutions)
                    && self.infer_overload_type(expected_value, actual_value, substitutions)
            }
            (Some(Type::Tuple(expected_items)), Some(Type::Tuple(actual_items))) => {
                expected_items.len() == actual_items.len()
                    && expected_items
                        .into_iter()
                        .zip(actual_items)
                        .all(|(expected_item, actual_item)| {
                            self.infer_overload_type(expected_item, actual_item, substitutions)
                        })
            }
            (Some(Type::Class { name: expected_name, args: expected_args }), Some(Type::Class { name: actual_name, args: actual_args })) => {
                expected_name == actual_name
                    && expected_args.len() == actual_args.len()
                    && expected_args.into_iter().zip(actual_args).all(
                        |(expected_arg, actual_arg)| {
                            self.infer_overload_type(expected_arg, actual_arg, substitutions)
                        },
                    )
            }
            (Some(Type::Function(expected_function)), Some(Type::Function(actual_function))) => {
                self.infer_overload_function_type(&expected_function, &actual_function, substitutions)
            }
            (Some(Type::Union(expected_items)), _) => expected_items
                .into_iter()
                .any(|item| self.infer_overload_type(item, actual, &mut substitutions.clone())),
            (_, Some(Type::Union(actual_items))) => actual_items
                .into_iter()
                .any(|item| self.infer_overload_type(expected, item, &mut substitutions.clone())),
            _ => false,
        }
    }

    /// Infer compatibility for function-typed overload parameters.
    fn infer_overload_function_type(
        &mut self,
        expected: &FunctionType,
        actual: &FunctionType,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        if expected.is_async != actual.is_async || expected.params.len() != actual.params.len() {
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
            _ => {
                self.infer_overload_type(actual_param, required_input, actual_substitutions)
            }
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
            let _ = self.argument(value, body)?;
        }
        let ty = self.ctx.krate.types.intern(Type::None);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(call.span.start, call.span.end),
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
            Expression::StaticMemberExpression(member) => self.type_test_root_expression(&member.object),
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
            Expression::StaticMemberExpression(member) => self.type_test_root_expression(&member.object),
            Expression::ComputedMemberExpression(member) => {
                self.type_test_root_expression(&member.object)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.type_test_root_expression(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => self.type_test_root_expression(&as_expr.expression),
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
}
