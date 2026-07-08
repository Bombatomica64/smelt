impl ModuleBuilder<'_> {
    /// Lower a Python call expression while applying an optional expected type.
    fn call_expression_with_hint(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(call.range);

        if let Some(expr) = self.stdlib_module_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.collection_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.numeric_module_call_expression(call, body, type_hint)? {
            return Ok(expr);
        }
        if let Some(error) = self.unsupported_deferred_stdlib_call(call) {
            return Err(error);
        }
        if let Some(expr) = self.cls_receiver_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.protocol_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.builtin_name_call_expression(call, body, type_hint, span)? {
            return Ok(expr);
        }
        if let Some(expr) = self.named_item_call_expression(call, body, span)? {
            return Ok(expr);
        }
        if let Some(expr) = self.callable_expression_call(call, body)? {
            return Ok(expr);
        }

        Err(SmeltError::unsupported(
            span,
            "only calls to top-level functions, class constructors, and print() are supported",
        ))
    }

    /// Try stdlib module / interop call handlers (file IO, datetime, urlparse,
    /// `int()` construction, class-method and module-member dispatch, asyncio).
    fn stdlib_module_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.file_io_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.datetime_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.urlparse_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.int_new_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.class_static_method_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.class_method_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.module_member_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.asyncio_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        Ok(None)
    }

    /// Try string-method and regex-expansion call handlers in source order.
    fn string_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.string_case_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.string_trim_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.string_affix_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.string_search_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.string_replace_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.string_remove_affix_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.string_predicate_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.re_expanded_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.string_split_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.string_join_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        Ok(None)
    }

    /// Try dict/list/set collection-method call handlers in source order.
    fn collection_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.dict_projection_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.list_append_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.list_extend_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.list_insert_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.list_reverse_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.list_pop_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.collection_clear_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.set_method_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.list_copy_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.list_count_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.list_index_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.list_remove_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.list_sort_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.dict_pop_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.requests_get_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.dict_get_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.dict_setdefault_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.dict_update_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.dict_copy_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        Ok(None)
    }

    /// Try math/random/json/re module-function call handlers in source order.
    fn numeric_module_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Some(expr) = self.math_module_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.random_module_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.json_dumps_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.json_loads_call_expression(call, body, type_hint)? {
            return Ok(Some(expr));
        }
        if let Some(expr) = self.re_module_call_expression(call, body)? {
            return Ok(Some(expr));
        }
        Ok(None)
    }

    /// Lower calls whose callee is a builtin name (`print`, `len`, container and
    /// primitive constructors, and the numeric/iterator builtins).
    fn builtin_name_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        type_hint: Option<TypeId>,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        // `print(...)` → CONSOLE_LOG_SYMBOL item (same as TS's `console.log`).
        if let Expr::Name(name) = call.func.as_ref() {
            if matches!(name.id.as_str(), "list" | "set" | "dict" | "tuple")
                && let Some(expr) =
                    self.container_constructor_call_expression(call, body, type_hint)?
            {
                return Ok(Some(expr));
            }
            if matches!(name.id.as_str(), "bool" | "int" | "float" | "str")
                && let Some(expr) = self.primitive_cast_call_expression(call, body)?
            {
                return Ok(Some(expr));
            }
            if name.id.as_str() == "abs" {
                return self.numeric_abs_call_expression(call, body).map(Some);
            }
            if matches!(name.id.as_str(), "max" | "min") {
                return self.numeric_extrema_call_expression(call, body).map(Some);
            }
            if name.id.as_str() == "sum"
                && let Some(expr) = self.numeric_sum_call_expression(call, body)?
            {
                return Ok(Some(expr));
            }
            if matches!(name.id.as_str(), "all" | "any")
                && let Some(expr) = self.bool_fold_call_expression(call, body)?
            {
                return Ok(Some(expr));
            }
            if name.id.as_str() == "sorted"
                && let Some(expr) = self.sorted_call_expression(call, body)?
            {
                return Ok(Some(expr));
            }
            if name.id.as_str() == "reversed"
                && let Some(expr) = self.reversed_call_expression(call, body)?
            {
                return Ok(Some(expr));
            }
            if name.id.as_str() == "enumerate"
                && let Some(expr) = self.enumerate_call_expression(call, body)?
            {
                return Ok(Some(expr));
            }
            if name.id.as_str() == "zip"
                && let Some(expr) = self.zip_call_expression(call, body)?
            {
                return Ok(Some(expr));
            }
            if name.id.as_str() == "range"
                && let Some(expr) = self.range_call_expression(call, body)?
            {
                return Ok(Some(expr));
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
                return Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::Call { callee, args },
                    ty: none_ty,
                    span,
                })));
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
                return Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::Len { operand },
                    ty,
                    span,
                })));
            }
        }
        Ok(None)
    }

    /// Lower a named function call or class constructor call resolved by item table.
    fn named_item_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        // Named function call OR class constructor call.
        if let Expr::Name(name) = call.func.as_ref() {
            let name_str = name.id.as_str();
            if let Some(expr) = self.local_callable_call(call, name_str, body)? {
                return Ok(Some(expr));
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
                        return Ok(Some(body.push_expr(HirExpr {
                            kind: ExprKind::New {
                                class: class_sym,
                                args,
                            },
                            ty: class_ty,
                            span,
                        })));
                    }
                    Item::Function(f) => {
                        let function = f.clone();
                        let return_ty = function.return_ty;
                        let callee = body.push_expr(HirExpr {
                            kind: ExprKind::Item(item_id),
                            ty: self.function_item_type(&function),
                            span,
                        });
                        let variadics = self.function_variadics.get(name_str).copied();
                        let params = function
                            .params
                            .iter()
                            .map(|param| param.ty)
                            .collect::<Vec<_>>();
                        let defaults = vec![None; params.len()];
                        let args = self.callable_call_args(
                            call,
                            body,
                            &CallableCallSignature {
                                params: &params,
                                vararg: variadics.and_then(|v| v.vararg),
                                kwarg: variadics.and_then(|v| v.kwarg),
                                defaults: &defaults,
                                label: "function",
                            },
                        )?;
                        return Ok(Some(body.push_expr(HirExpr {
                            kind: ExprKind::Call { callee, args },
                            ty: return_ty,
                            span,
                        })));
                    }
                    Item::Interface(_)
                    | Item::TypeAlias(_)
                    | Item::Const(_)
                    | Item::MutableGlobal(_) => {}
                }
            }
        }
        Ok(None)
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
        let defaults = callback_meta
            .as_ref()
            .map_or_else(|| vec![None; function.params.len()], |callback| {
                callback.defaults.clone()
            });
        let callee = self.identifier_expression(name, call.func.range(), body)?;
        let args = self.callable_call_args(
            call,
            body,
            &CallableCallSignature {
                params: &function.params,
                vararg: callback_meta.as_ref().and_then(|callback| callback.vararg),
                kwarg: callback_meta.as_ref().and_then(|callback| callback.kwarg),
                defaults: &defaults,
                label: "closure",
            },
        )?;
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: function.return_ty,
            span: self.span(call.range),
        })))
    }

    /// Lower a call whose callee expression has a statically known function type.
    fn callable_expression_call(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if matches!(call.func.as_ref(), Expr::Name(_)) {
            return Ok(None);
        }
        let callee = self.expression(&call.func, body)?;
        let callee_ty = Self::expr_ty(body, callee);
        let Some(Type::Function(function)) = self.ctx.krate.types.get(callee_ty).cloned() else {
            return Ok(None);
        };
        let defaults = vec![None; function.params.len()];
        let args = self.callable_call_args(
            call,
            body,
            &CallableCallSignature {
                params: &function.params,
                vararg: None,
                kwarg: None,
                defaults: &defaults,
                label: "callable expression",
            },
        )?;
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ClosureCall { callee, args },
            ty: function.return_ty,
            span: self.span(call.range),
        })))
    }

    /// Lower and validate Python call arguments for a statically typed callable.
    fn callable_call_args(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        signature: &CallableCallSignature<'_>,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let span = self.span(call.range);
        let supplied_arg_count = call.arguments.args.len();
        if signature.kwarg.is_none() && !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "{} keyword arguments require a **kwargs parameter",
                    signature.label
                ),
            ));
        }
        let packed_param_start = signature
            .vararg
            .map(|meta| meta.index)
            .or_else(|| signature.kwarg.map(|meta| meta.index))
            .unwrap_or(signature.params.len());
        let required_arg_count = signature
            .defaults
            .iter()
            .take(packed_param_start)
            .position(Option::is_some)
            .unwrap_or(packed_param_start);
        if supplied_arg_count < required_arg_count
            || (signature.vararg.is_none() && supplied_arg_count > packed_param_start)
        {
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "{} call argument count does not match parameters",
                    signature.label
                ),
            ));
        }
        // Positional arguments are lowered with the corresponding parameter
        // type as an expected-type hint. This is what lets a `lambda` argument
        // recover its parameter/return types from a `Callable[...]` parameter
        // (a bare lambda has no annotations of its own); it is otherwise inert
        // because the argument type is re-checked against the parameter below.
        let mut args = call
            .arguments
            .args
            .iter()
            .take(packed_param_start)
            .enumerate()
            .map(|(index, arg)| {
                self.expression_with_hint(arg, body, signature.params.get(index).copied())
            })
            .collect::<Result<Vec<_>, _>>()?;
        for index in supplied_arg_count..packed_param_start {
            let Some(default) = signature.defaults.get(index).and_then(|default| *default) else {
                return Err(SmeltError::unsupported(
                    span,
                    format!(
                        "{} call argument count does not match parameters",
                        signature.label
                    ),
                ));
            };
            args.push(default);
        }
        if let Some(meta) = signature.vararg {
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
        if let Some(meta) = signature.kwarg {
            args.push(self.kwargs_argument(&call.arguments.keywords, meta.value_ty, span, body)?);
        }
        for (arg, expected) in args.iter().zip(signature.params) {
            if Self::expr_ty(body, *arg) != *expected {
                return Err(SmeltError::unsupported(
                    span,
                    format!(
                        "{} call argument type does not match parameter",
                        signature.label
                    ),
                ));
            }
        }
        Ok(args)
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
    /// The receiver's HIR type must be a known class and the method must be
    /// declared on that class. Dispatch stays static and mirrors Python method
    /// binding:
    ///
    /// * instance methods lower to [`ExprKind::Method`] (`receiver.method(..)`);
    /// * `@classmethod` / `@staticmethod` members lower to a receiver-free
    ///   associated call [`ExprKind::Call`] (`Class::method(..)`), because the
    ///   implicit `cls` (or no receiver) is erased in the lowered signature.
    ///
    /// Because Python's `cls`/`self`/instance bindings all carry the same
    /// `Type::Class` HIR type, whether the method dispatches receiver-free is
    /// decided by the *method kind* (looked up in the classmethod/staticmethod
    /// registries), not by the receiver expression. `str(x)` is mapped to
    /// `x.__str__()` for class values that provide `__str__`.
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
            if self
                .class_method_dispatch(Self::expr_ty(body, receiver), "__str__")
                .is_some()
            {
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
        if self.class_method_dispatch(receiver_ty, method).is_none() {
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

    /// Lower a `cls(...)` construction inside a `@classmethod` body.
    ///
    /// Inside a `@classmethod` the implicit `cls` receiver is a local bound to
    /// the owning class's `Type::Class`, so `cls(args)` constructs that class
    /// ([`ExprKind::New`]) — the alternate-constructor idiom common in
    /// `result`/`returns`. `cls.helper(args)` is handled by the general method
    /// dispatch in [`Self::protocol_call_expression`] (the `cls` local is a class
    /// value with the method resolved via [`Self::class_method_dispatch`]).
    ///
    /// Returns `None` when the callee is not a `cls` name, so the ordinary call
    /// handlers still run.
    fn cls_receiver_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let span = self.span(call.range);
        // `cls(args)` — construct the owning class. `cls` is the class-method
        // receiver local bound to `Type::Class`; keying on the `cls` binding
        // name distinguishes the class object from an instance value (both carry
        // the same `Type::Class` and only the classmethod receiver is callable).
        if let Expr::Name(name) = call.func.as_ref()
            && name.id.as_str() == "cls"
            && let Some(&local) = self.locals.get("cls")
            && let Some(Type::Class { name: class_sym, .. }) =
                self.ctx.krate.types.get(Self::local_ty(body, local)).cloned()
        {
            let class_ty = self.intern_type(Type::Class {
                name: class_sym,
                args: vec![],
            });
            let args = call
                .arguments
                .args
                .iter()
                .map(|arg| self.expression(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Some(body.push_expr(HirExpr {
                kind: ExprKind::New {
                    class: class_sym,
                    args,
                },
                ty: class_ty,
                span,
            })));
        }
        Ok(None)
    }

    /// Create a HIR call/method expression for a statically-known class method.
    ///
    /// The [`ClassMethodDispatch`] resolved from the receiver type and method
    /// name decides the shape: an associated call for classmethods/staticmethods
    /// (the receiver is dropped) or a receiver method call for instance methods.
    fn protocol_method_expr(
        &mut self,
        call: ProtocolMethodCall,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let receiver = call.receiver;
        let method = call.method;
        let receiver_ty = Self::expr_ty(body, receiver);
        let span = Self::expr_span(body, receiver);
        let dispatch = self.class_method_dispatch(receiver_ty, &method).ok_or_else(|| {
            SmeltError::unsupported(
                span,
                format!("class method '{method}' is not statically known"),
            )
        })?;
        let Item::Function(function) = self.item_ref(dispatch.item) else {
            return Err(SmeltError::unsupported(
                span,
                format!("class method '{method}' is not a function item"),
            ));
        };
        let return_ty = function.return_ty;
        if dispatch.receiver_free {
            // Classmethod/staticmethod: drop the receiver and call the
            // associated function directly (`Class::method(args)`).
            let callee = body.push_expr(HirExpr {
                kind: ExprKind::Item(dispatch.item),
                ty: self.item_expr_type(dispatch.item),
                span,
            });
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::Call {
                    callee,
                    args: call.args,
                },
                ty: return_ty,
                span,
            }));
        }
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

    /// Resolve a class method by receiver type and name, classifying its
    /// dispatch shape (instance vs receiver-free associated call).
    ///
    /// `@staticmethod` and `@classmethod` members live in
    /// [`Self::class_static_methods`] and dispatch receiver-free; instance
    /// methods live in [`Self::class_methods`] and dispatch through the receiver.
    /// Returns `None` when the receiver type is not a known class or the class
    /// declares no such method.
    fn class_method_dispatch(&self, receiver_ty: TypeId, method: &str) -> Option<ClassMethodDispatch> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(receiver_ty) else {
            return None;
        };
        let class_name = self.ctx.krate.symbols.get(*name)?;
        // Static methods and classmethods dispatch receiver-free.
        if let Some(item) = self
            .class_static_methods
            .get(class_name)
            .and_then(|methods| methods.get(method))
            .copied()
        {
            return Some(ClassMethodDispatch {
                item,
                receiver_free: true,
            });
        }
        // Instance methods dispatch through the receiver.
        let item = self
            .class_methods
            .get(class_name)
            .and_then(|methods| methods.get(method))
            .copied()
            .or_else(|| self.class_method_item_by_name(class_name, method))?;
        Some(ClassMethodDispatch {
            item,
            receiver_free: false,
        })
    }
}

/// A resolved class-method call target plus its dispatch shape.
struct ClassMethodDispatch {
    /// The resolved method function item.
    item: ItemId,
    /// Whether the method dispatches receiver-free (`@classmethod`/`@staticmethod`),
    /// i.e. lowers to `Class::method(args)` rather than `receiver.method(args)`.
    receiver_free: bool,
}
