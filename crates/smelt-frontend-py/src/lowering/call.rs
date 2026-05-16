impl ModuleBuilder<'_> {
    /// Lower a Python call expression while applying an optional expected type.
    fn call_expression_with_hint(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(call.range);

        if let Some(expr) = self.file_io_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.datetime_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.urlparse_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.int_new_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.class_method_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.module_member_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.asyncio_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_case_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_trim_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_affix_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_search_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_replace_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_remove_affix_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_predicate_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.re_expanded_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_split_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_join_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.dict_projection_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_append_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_extend_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_insert_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_reverse_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_pop_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.collection_clear_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.set_method_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_copy_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_count_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_index_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_remove_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.list_sort_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.dict_pop_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.requests_get_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.dict_get_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.dict_setdefault_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.dict_update_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.dict_copy_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.math_module_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.random_module_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.json_dumps_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.json_loads_call_expression(call, body, type_hint)? {
            return Ok(expr);
        }
        if let Some(expr) = self.re_module_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(error) = self.unsupported_deferred_stdlib_call(call) {
            return Err(error);
        }
        if let Some(expr) = self.protocol_call_expression(call, body)? {
            return Ok(expr);
        }
        // `print(...)` → CONSOLE_LOG_SYMBOL item (same as TS's `console.log`).
        if let Expr::Name(name) = call.func.as_ref() {
            if matches!(name.id.as_str(), "list" | "set" | "dict" | "tuple")
                && let Some(expr) =
                    self.container_constructor_call_expression(call, body, type_hint)?
            {
                return Ok(expr);
            }
            if matches!(name.id.as_str(), "bool" | "int" | "float" | "str")
                && let Some(expr) = self.primitive_cast_call_expression(call, body)?
            {
                return Ok(expr);
            }
            if name.id.as_str() == "abs" {
                return self.numeric_abs_call_expression(call, body);
            }
            if matches!(name.id.as_str(), "max" | "min") {
                return self.numeric_extrema_call_expression(call, body);
            }
            if name.id.as_str() == "sum"
                && let Some(expr) = self.numeric_sum_call_expression(call, body)?
            {
                return Ok(expr);
            }
            if matches!(name.id.as_str(), "all" | "any")
                && let Some(expr) = self.bool_fold_call_expression(call, body)?
            {
                return Ok(expr);
            }
            if name.id.as_str() == "sorted"
                && let Some(expr) = self.sorted_call_expression(call, body)?
            {
                return Ok(expr);
            }
            if name.id.as_str() == "reversed"
                && let Some(expr) = self.reversed_call_expression(call, body)?
            {
                return Ok(expr);
            }
            if name.id.as_str() == "enumerate"
                && let Some(expr) = self.enumerate_call_expression(call, body)?
            {
                return Ok(expr);
            }
            if name.id.as_str() == "zip"
                && let Some(expr) = self.zip_call_expression(call, body)?
            {
                return Ok(expr);
            }
            if name.id.as_str() == "range"
                && let Some(expr) = self.range_call_expression(call, body)?
            {
                return Ok(expr);
            }
            if name.id.as_str() == "print" {
                let print_item = self.ensure_print_item(span);
                let none_ty = self.intern_type(Type::None);
                let callee = body.push_expr(HirExpr {
                    kind: ExprKind::Item(print_item),
                    ty: none_ty,
                    span,
                });
                let args: Vec<_> = call
                    .arguments
                    .args
                    .iter()
                    .map(|a| self.expression(a, body))
                    .collect::<Result<_, _>>()?;
                return Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Call { callee, args },
                    ty: none_ty,
                    span,
                }));
            }
            if name.id.as_str() == "len" {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "len() requires exactly one argument",
                    ));
                }
                let [operand_expr] = call.arguments.args.as_ref() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "len() requires exactly one argument",
                    ));
                };
                let operand = self.expression(operand_expr, body)?;
                let operand_ty = Self::expr_ty(body, operand);
                if !self.supports_stdlib_len(operand_ty) {
                    return Err(SmeltError::unsupported(
                        span,
                        "len() is only supported for list, dict, tuple, and str values",
                    ));
                }
                let ty = self.intern_type(Type::Int);
                return Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Len { operand },
                    ty,
                    span,
                }));
            }
        }

        // Named function call OR class constructor call.
        if let Expr::Name(name) = call.func.as_ref() {
            let name_str = name.id.as_str();
            if let Some(expr) = self.local_callable_call(call, name_str, body)? {
                return Ok(expr);
            }
            if let Some(&item_id) = self.items.get(name_str) {
                let item = self.item_ref(item_id);
                match item {
                    Item::Class(c) => {
                        // Constructor call: `MyClass(args...)`
                        if matches!(c.kind, ClassKind::Abstract) {
                            return Err(SmeltError::unsupported(
                                span,
                                format!("abstract class '{name_str}' cannot be constructed"),
                            ));
                        }
                        let class_sym = c.name;
                        let class_ty = self.intern_type(Type::Class {
                            name: class_sym,
                            args: vec![],
                        });
                        let args: Vec<_> = call
                            .arguments
                            .args
                            .iter()
                            .map(|a| self.expression(a, body))
                            .collect::<Result<_, _>>()?;
                        return Ok(body.push_expr(HirExpr {
                            kind: ExprKind::New {
                                class: class_sym,
                                args,
                            },
                            ty: class_ty,
                            span,
                        }));
                    }
                    Item::Function(f) => {
                        let return_ty = f.return_ty;
                        let callee = body.push_expr(HirExpr {
                            kind: ExprKind::Item(item_id),
                            ty: return_ty,
                            span,
                        });
                        let variadics = self.function_variadics.get(name_str).copied();
                        if variadics.and_then(|v| v.kwarg).is_none()
                            && !call.arguments.keywords.is_empty()
                        {
                            return Err(SmeltError::unsupported(
                                span,
                                "function keyword arguments require a **kwargs parameter",
                            ));
                        }
                        let vararg_meta = variadics.and_then(|v| v.vararg);
                        let kwarg_meta = variadics.and_then(|v| v.kwarg);
                        let packed_param_start = vararg_meta
                            .map(|meta| meta.index)
                            .or_else(|| kwarg_meta.map(|meta| meta.index))
                            .unwrap_or(f.params.len());
                        if vararg_meta.is_none() && call.arguments.args.len() > packed_param_start {
                            return Err(SmeltError::unsupported(
                                span,
                                "function argument count does not match parameters",
                            ));
                        }
                        let mut args = call
                            .arguments
                            .args
                            .iter()
                            .take(packed_param_start)
                            .map(|a| self.expression(a, body))
                            .collect::<Result<Vec<_>, _>>()?;
                        if let Some(meta) = vararg_meta {
                            let packed_args = call
                                .arguments
                                .args
                                .iter()
                                .skip(meta.index)
                                .map(|arg| self.expression(arg, body))
                                .collect::<Result<Vec<_>, _>>()?;
                            let rest_ty = self.intern_type(Type::List(meta.item_ty));
                            args.push(body.push_expr(HirExpr {
                                kind: ExprKind::ListLit(packed_args),
                                ty: rest_ty,
                                span,
                            }));
                        }
                        if let Some(meta) = kwarg_meta {
                            args.push(self.kwargs_argument(
                                &call.arguments.keywords,
                                meta.value_ty,
                                span,
                                body,
                            )?);
                        }
                        return Ok(body.push_expr(HirExpr {
                            kind: ExprKind::Call { callee, args },
                            ty: return_ty,
                            span,
                        }));
                    }
                    Item::Interface(_) | Item::TypeAlias(_) | Item::Const(_) => {}
                }
            }
        }

        Err(SmeltError::unsupported(
            span,
            "only calls to top-level functions, class constructors, and print() are supported",
        ))
    }

    /// Lower a call whose callee is a local closure or function-typed local.
    fn local_callable_call(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        name: &str,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if self.items.contains_key(name) {
            return Ok(None);
        }
        let Some(local) = self.locals.get(name).copied() else {
            return Ok(None);
        };
        let local_ty = Self::local_ty(body, local);
        let Some(Type::Function(function)) = self.ctx.krate.types.get(local_ty).cloned() else {
            return Ok(None);
        };
        let callback_meta = self.local_callbacks.get(name).cloned();
        let vararg_meta = callback_meta.as_ref().and_then(|callback| callback.vararg);
        let kwarg_meta = callback_meta.as_ref().and_then(|callback| callback.kwarg);
        let supplied_arg_count = call.arguments.args.len();
        if kwarg_meta.is_none() && !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "closure keyword arguments require a **kwargs closure parameter",
            ));
        }
        let defaults = callback_meta
            .as_ref()
            .map_or_else(|| vec![None; function.params.len()], |callback| {
                callback.defaults.clone()
            });
        let packed_param_start = vararg_meta
            .map(|meta| meta.index)
            .or_else(|| kwarg_meta.map(|meta| meta.index))
            .unwrap_or(function.params.len());
        let required_arg_count = defaults
            .iter()
            .take(packed_param_start)
            .position(Option::is_some)
            .unwrap_or(packed_param_start);
        if supplied_arg_count < required_arg_count
            || (vararg_meta.is_none() && supplied_arg_count > packed_param_start)
        {
            return Err(SmeltError::unsupported(
                self.span(call.range),
                "closure call argument count does not match closure parameters",
            ));
        }
        let callee = self.identifier_expression(name, call.func.range(), body)?;
        let mut args = call
            .arguments
            .args
            .iter()
            .take(packed_param_start)
            .map(|arg| self.expression(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        for index in supplied_arg_count..packed_param_start {
            let Some(default) = defaults.get(index).and_then(|default| *default) else {
                return Err(SmeltError::unsupported(
                    self.span(call.range),
                    "closure call argument count does not match closure parameters",
                ));
            };
            args.push(default);
        }
        if let Some(meta) = vararg_meta {
            let packed_args = call
                .arguments
                .args
                .iter()
                .skip(meta.index)
                .map(|arg| self.expression(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            let rest_ty = self.intern_type(Type::List(meta.item_ty));
            args.push(body.push_expr(HirExpr {
                kind: ExprKind::ListLit(packed_args),
                ty: rest_ty,
                span: self.span(call.range),
            }));
        }
        if let Some(meta) = kwarg_meta {
            args.push(self.kwargs_argument(
                &call.arguments.keywords,
                meta.value_ty,
                self.span(call.range),
                body,
            )?);
        }
        for (arg, expected) in args.iter().zip(&function.params) {
            if Self::expr_ty(body, *arg) != *expected {
                return Err(SmeltError::unsupported(
                    self.span(call.range),
                    "closure call argument type does not match closure parameter",
                ));
            }
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: function.return_ty,
            span: self.span(call.range),
        })))
    }

    /// Packs Python keyword arguments into the lowered `**kwargs` dictionary argument.
    fn kwargs_argument(
        &mut self,
        keywords: &[ruff_python_ast::Keyword],
        value_ty: TypeId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let string_ty = self.intern_type(Type::String);
        let dict_ty = self.intern_type(Type::Dict(string_ty, value_ty));
        let mut chunks = Vec::new();
        let mut entries = Vec::new();
        for keyword in keywords {
            if let Some(arg) = &keyword.arg {
                let key = self.string_literal_expr(arg.as_str(), keyword.range, body);
                let value = self.expression(&keyword.value, body)?;
                entries.push((key, value));
            } else {
                Self::flush_kwargs_entries(&mut chunks, &mut entries, dict_ty, span, body);
                let unpacked = self.expression(&keyword.value, body)?;
                if Self::expr_ty(body, unpacked) != dict_ty {
                    return Err(SmeltError::unsupported(
                        self.span(keyword.value.range()),
                        "unpacked kwargs must match the **kwargs dictionary type",
                    ));
                }
                chunks.push(unpacked);
            }
        }
        Self::flush_kwargs_entries(&mut chunks, &mut entries, dict_ty, span, body);
        let Some((target, sources)) = chunks.split_first() else {
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::DictLit(Vec::new()),
                ty: dict_ty,
                span,
            }));
        };
        if sources.is_empty() {
            return Ok(*target);
        }
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::DictAssign {
                target: *target,
                sources: sources.to_vec(),
            },
            ty: dict_ty,
            span,
        }))
    }

    /// Flushes pending named kwargs into one dictionary chunk for ordered merging.
    fn flush_kwargs_entries(
        chunks: &mut Vec<smelt_hir::ExprId>,
        entries: &mut Vec<(smelt_hir::ExprId, smelt_hir::ExprId)>,
        dict_ty: TypeId,
        span: Span,
        body: &mut Body,
    ) {
        if entries.is_empty() {
            return;
        }
        chunks.push(body.push_expr(HirExpr {
            kind: ExprKind::DictLit(std::mem::take(entries)),
            ty: dict_ty,
            span,
        }));
    }

    /// Lower calls to statically-known Python protocol/class methods.
    ///
    /// The dispatch remains intentionally static: the receiver's HIR type must
    /// be a known class and the method must be declared on that class.  `str(x)`
    /// is mapped to `x.__str__()` for class values that provide `__str__`.
    fn protocol_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Expr::Name(name) = call.func.as_ref()
            && name.id.as_str() == "str"
            && call.arguments.args.len() == 1
            && call.arguments.keywords.is_empty()
        {
            let receiver = self.expression(&call.arguments.args[0], body)?;
            if self.class_has_method(Self::expr_ty(body, receiver), "__str__") {
                return self
                    .protocol_method_expr(
                        ProtocolMethodCall {
                            receiver,
                            method: "__str__".to_owned(),
                            args: Vec::new(),
                        },
                        body,
                    )
                    .map(Some);
            }
            return Ok(None);
        }

        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if let Expr::Name(name) = attr.value.as_ref()
            && !self.locals.contains_key(name.id.as_str())
            && !self.items.contains_key(name.id.as_str())
        {
            return Ok(None);
        }
        let receiver = self.expression(&attr.value, body)?;
        let receiver_ty = Self::expr_ty(body, receiver);
        let method = attr.attr.as_str();
        if !self.class_has_method(receiver_ty, method) {
            return Ok(None);
        }
        let args = call
            .arguments
            .args
            .iter()
            .map(|arg| self.expression(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        self.protocol_method_expr(
            ProtocolMethodCall {
                receiver,
                method: method.to_owned(),
                args,
            },
            body,
        )
        .map(Some)
    }

    /// Create a HIR method call for a statically-known class method.
    fn protocol_method_expr(
        &mut self,
        call: ProtocolMethodCall,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let receiver = call.receiver;
        let method = call.method;
        let receiver_ty = Self::expr_ty(body, receiver);
        let span = Self::expr_span(body, receiver);
        let return_ty = self
            .class_method_return_ty(receiver_ty, &method)
            .ok_or_else(|| {
                SmeltError::unsupported(
                    span,
                    format!("class method '{method}' is not statically known"),
                )
            })?;
        let method_sym = self.intern_name(&method);
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::Method {
                receiver,
                method: method_sym,
                args: call.args,
            },
            ty: return_ty,
            span,
        }))
    }

    /// Return whether a type is a class that declares `method`.
    fn class_has_method(&self, receiver_ty: TypeId, method: &str) -> bool {
        self.class_method_item(receiver_ty, method).is_some()
    }

    /// Return a statically-known class method's return type.
    fn class_method_return_ty(&self, receiver_ty: TypeId, method: &str) -> Option<TypeId> {
        let item = self.class_method_item(receiver_ty, method)?;
        let Item::Function(function) = self.item_ref(item) else {
            return None;
        };
        Some(function.return_ty)
    }

    /// Resolve a method item by receiver class type and source method name.
    fn class_method_item(&self, receiver_ty: TypeId, method: &str) -> Option<ItemId> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(receiver_ty) else {
            return None;
        };
        let class_name = self.ctx.krate.symbols.get(*name)?;
        self.class_methods
            .get(class_name)
            .and_then(|methods| methods.get(method))
            .copied()
            .or_else(|| self.class_method_item_by_name(class_name, method))
    }
}
