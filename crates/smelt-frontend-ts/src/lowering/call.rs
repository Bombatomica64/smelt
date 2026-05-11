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
            let (return_ty, _) = self.resolve_method(Self::expr_ty(body, receiver), method)?;
            let mut args = Vec::new();
            for arg in &call.arguments {
                args.push(self.argument(arg, body)?);
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
        if let Expression::Identifier(callee_ident) = &call.callee {
            let Some(item) = self.items.get(callee_ident.name.as_str()).copied() else {
                return Err(SmeltError::unsupported(
                    self.span(callee_ident.span.start, callee_ident.span.end),
                    format!("unresolved function `{}`", callee_ident.name),
                ));
            };
            let (params, return_ty, is_async) = if let Item::Function(function) = self.item_ref(item)
            {
                (
                    function.params.iter().map(|param| param.ty).collect(),
                    function.return_ty,
                    function.is_async,
                )
            } else {
                return Err(SmeltError::unsupported(
                    self.span(callee_ident.span.start, callee_ident.span.end),
                    "callee item is not a function",
                ));
            };
            let mut args = Vec::new();
            for arg in &call.arguments {
                args.push(self.argument(arg, body)?);
            }
            let callee = body.push_expr(Expr {
                kind: ExprKind::Item(item),
                ty: self.ctx.krate.types.intern(Type::Function(smelt_hir::FunctionType {
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
}
