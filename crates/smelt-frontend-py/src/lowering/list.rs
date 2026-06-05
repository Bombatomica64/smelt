impl ModuleBuilder<'_> {
    /// Resolve a class method by inspecting class metadata directly.
    fn class_method_item_by_name(&self, class_name: &str, method: &str) -> Option<ItemId> {
        for item in &self.ctx.krate.items {
            let Item::Class(class) = item else {
                continue;
            };
            if self.ctx.krate.symbols.get(class.name) != Some(class_name) {
                continue;
            }
            for method_item in &class.methods {
                let Item::Function(function) = self.item_ref(*method_item) else {
                    continue;
                };
                if self.ctx.krate.symbols.get(function.name) == Some(method) {
                    return Some(*method_item);
                }
            }
            if let Some(base) = class.base
                && let Some(base_name) = self.ctx.krate.symbols.get(base)
            {
                return self.class_method_item_by_name(base_name, method);
            }
        }
        None
    }

    /// Return targeted diagnostics for deferred Python stdlib/native library APIs.
    fn unsupported_deferred_stdlib_call(
        &self,
        call: &ruff_python_ast::ExprCall,
    ) -> Option<SmeltError> {
        let span = self.span(call.range);
        match call.func.as_ref() {
            Expr::Name(name) if name.id.as_str() == "open" => Some(SmeltError::unsupported(
                span,
                "Python open() is not supported yet; file IO needs a dedicated text-mode mapping",
            )),
            Expr::Attribute(attr) => {
                if matches!(
                    attr.value.as_ref(),
                    Expr::Call(inner)
                        if matches!(inner.func.as_ref(), Expr::Name(name) if name.id.as_str() == "open")
                ) {
                    return Some(SmeltError::unsupported(
                        span,
                        "Python open() is not supported yet; file IO needs a dedicated text-mode mapping",
                    ));
                }
                let Expr::Name(module) = attr.value.as_ref() else {
                    return None;
                };
                let message = match module.id.as_str() {
                    "datetime" => {
                        "Python datetime is not supported yet; datetime/date/timedelta need a chrono-backed mapping"
                    }
                    "urllib" | "urlparse" => {
                        "Python URL parsing is not supported yet; urllib/urlparse need a URL mapping policy"
                    }
                    "numpy" | "np" => {
                        "NumPy is deferred from Phase 6; array dtype, ownership, shape, and broadcasting semantics need a dedicated design"
                    }
                    "pandas" | "pd" => {
                        "pandas is out of scope for Phase 6; dataframe semantics need a dedicated native-data-library design"
                    }
                    _ => return None,
                };
                Some(SmeltError::unsupported(span, message))
            }
            _ => None,
        }
    }

    /// Lower direct Python container constructor calls.
    fn container_constructor_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        let constructor = name.id.as_str();
        if !matches!(constructor, "list" | "set" | "dict" | "tuple") {
            return Ok(None);
        }
        let span = self.span(call.range);
        if !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "container constructors do not support keyword arguments yet",
            ));
        }
        match (constructor, call.arguments.args.as_ref()) {
            ("list", []) => {
                let Some(ty) = type_hint else {
                    return Err(SmeltError::unsupported(
                        span,
                        "empty list() requires a list type annotation",
                    ));
                };
                if !matches!(self.ctx.krate.types.get(ty), Some(Type::List(_))) {
                    return Err(SmeltError::unsupported(
                        span,
                        "list() type annotation must be list[T]",
                    ));
                }
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::ListLit(Vec::new()),
                    ty,
                    span,
                })))
            }
            ("set", []) => {
                let Some(ty) = type_hint else {
                    return Err(SmeltError::unsupported(
                        span,
                        "empty set() requires a set type annotation",
                    ));
                };
                if !matches!(self.ctx.krate.types.get(ty), Some(Type::Set(_))) {
                    return Err(SmeltError::unsupported(
                        span,
                        "set() type annotation must be set[T]",
                    ));
                }
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::SetLit(Vec::new()),
                    ty,
                    span,
                })))
            }
            ("dict", []) => {
                let Some(ty) = type_hint else {
                    return Err(SmeltError::unsupported(
                        span,
                        "empty dict() requires a dict type annotation",
                    ));
                };
                if !matches!(self.ctx.krate.types.get(ty), Some(Type::Dict(_, _))) {
                    return Err(SmeltError::unsupported(
                        span,
                        "dict() type annotation must be dict[K, V]",
                    ));
                }
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::DictLit(Vec::new()),
                    ty,
                    span,
                })))
            }
            ("tuple", []) => {
                let ty = type_hint.unwrap_or_else(|| self.intern_type(Type::Tuple(Vec::new())));
                if !matches!(self.ctx.krate.types.get(ty), Some(Type::Tuple(_))) {
                    return Err(SmeltError::unsupported(
                        span,
                        "tuple() type annotation must be tuple[...]",
                    ));
                }
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::TupleLit(Vec::new()),
                    ty,
                    span,
                })))
            }
            ("list", [arg]) => self.list_constructor_from_arg(arg, body, span),
            ("set", [arg]) => self.set_constructor_from_arg(arg, body, span),
            ("dict", [arg]) => self.dict_constructor_from_arg(arg, body, span),
            ("tuple", [arg]) => self.tuple_constructor_from_arg(arg, body, span, type_hint),
            _ => Err(SmeltError::unsupported(
                span,
                "container constructors support zero arguments or one same-container argument",
            )),
        }
    }

    /// Lower `list(value)` for the currently supported direct container inputs.
    fn list_constructor_from_arg(
        &mut self,
        arg: &Expr,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        if let Expr::Call(call) = arg
            && let Some(expr) = self.map_filter_list_callback(call, body, span)?
        {
            return Ok(Some(expr));
        }
        let source = self.expression(arg, body)?;
        let source_ty = Self::expr_ty(body, source);
        let (kind, ty) = match self.ctx.krate.types.get(source_ty) {
            Some(Type::List(_)) => (ExprKind::ListCopy { list: source }, source_ty),
            Some(Type::Set(item_type)) => {
                let item_ty = *item_type;
                (
                    ExprKind::SetProjection {
                        op: SetProjectionOp::Values,
                        set: source,
                    },
                    self.intern_type(Type::List(item_ty)),
                )
            }
            Some(Type::Dict(key_type, _)) => {
                let key_ty = *key_type;
                (
                    ExprKind::DictProjection {
                        op: DictProjectionOp::Keys,
                        dict: source,
                    },
                    self.intern_type(Type::List(key_ty)),
                )
            }
            Some(Type::Tuple(items)) => {
                let item_ty = Self::homogeneous_tuple_item_ty(items, span)?;
                (
                    ExprKind::TupleToList { tuple: source },
                    self.intern_type(Type::List(item_ty)),
                )
            }
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "list(value) currently requires a list, set, dict, or homogeneous tuple value",
                ));
            }
        };
        Ok(Some(body.push_expr(HirExpr { kind, ty, span })))
    }

    /// Lower `list(map(callback, values))` and `list(filter(callback, values))`.
    fn map_filter_list_callback(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        let op = match name.id.as_str() {
            "map" => ListCallbackOp::Map,
            "filter" => ListCallbackOp::Filter,
            _ => return Ok(None),
        };
        if !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "map/filter callbacks do not support keyword arguments",
            ));
        }
        let [callback_arg, list_arg] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                span,
                "map/filter require callback and one list argument",
            ));
        };
        let list = self.expression(list_arg, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                span,
                "map/filter input must be a list",
            ));
        };
        let callback = self.python_callback_argument(callback_arg, &[*element_ty], body)?;
        let ty = if name.id.as_str() == "filter" {
            let bool_ty = self.intern_type(Type::Bool);
            if callback.return_ty != bool_ty {
                return Err(SmeltError::unsupported(
                    self.span(callback_arg.range()),
                    "filter callback must return bool",
                ));
            }
            list_ty
        } else {
            self.intern_type(Type::List(callback.return_ty))
        };
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListCallback {
                op,
                list,
                callback: callback.expr,
            },
            ty,
            span,
        })))
    }

    /// Lower either an inline lambda or an annotated local lambda callback.
    fn python_callback_argument(
        &mut self,
        arg: &Expr,
        expected_param_tys: &[TypeId],
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        match arg {
            Expr::Lambda(lambda) => {
                let callback = self.lambda_callback(lambda, expected_param_tys, body)?;
                let return_ty = callback.ty;
                let expr = self.callback_expr_to_closure(
                    &callback,
                    expected_param_tys,
                    self.span(lambda.range),
                    body,
                )?;
                Ok(ClosureCallback { expr, return_ty })
            }
            Expr::Name(name) => {
                let Some(callback) = self.local_callbacks.get(name.id.as_str()).cloned() else {
                    return Err(SmeltError::unsupported(
                        self.span(name.range),
                        format!("local callback `{}` is not defined", name.id),
                    ));
                };
                if callback.params != expected_param_tys {
                    return Err(SmeltError::unsupported(
                        self.span(name.range),
                        "local callback parameter type does not match input list",
                    ));
                }
                if callback.callback.ty != callback.return_ty {
                    return Err(SmeltError::unsupported(
                        self.span(name.range),
                        "local callback return type is inconsistent",
                    ));
                }
                let expr = self.callback_expr_to_closure(
                    &callback.callback,
                    &callback.params,
                    self.span(name.range),
                    body,
                )?;
                Ok(ClosureCallback {
                    expr,
                    return_ty: callback.return_ty,
                })
            }
            _ => Err(SmeltError::unsupported(
                self.span(arg.range()),
                "map/filter callbacks must be lambdas or annotated local lambdas",
            )),
        }
    }

    /// Store a Python callback expression as a HIR closure expression.
    fn callback_expr_to_closure(
        &mut self,
        callback: &CallbackExpr,
        params: &[TypeId],
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut closure_body = Body::new(None, span);
        let closure_params = params
            .iter()
            .enumerate()
            .map(|(index, ty)| {
                let name = self.intern_name(&format!("__callback_param_{index}"));
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
        let mut captures = self.callback_captures(callback, body);
        let mut capture_locals = HashMap::new();
        for capture in &mut captures {
            let local = closure_body.push_local(LocalDecl {
                name: Some(capture.symbol),
                ty: capture.ty,
                mutable: capture.mode == CaptureMode::ByMut,
                span,
            });
            capture.body_local = Some(local);
            capture_locals.insert(capture.source_local, local);
        }
        let param_exprs = closure_params
            .iter()
            .map(|param| {
                closure_body.push_expr(HirExpr {
                    kind: ExprKind::Local(param.local),
                    ty: param.ty,
                    span,
                })
            })
            .collect::<Vec<_>>();
        let tail = self.callback_expr_to_body_expr(
            callback,
            &param_exprs,
            &capture_locals,
            &mut closure_body,
        );
        let return_ty = callback.ty;
        let tail_expr = tail?;
        closure_body.blocks[0].tail = Some(tail_expr);
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.intern_type(Type::Function(FunctionType {
            params: params.to_vec(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        }));
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
                rest: None,
                required_params: None,
                return_ty,
                captures,
                body: body_id,
                span,
            }),
            ty: closure_ty,
            span,
        }))
    }

    /// Convert Python's compact callback subset into a normal closure-body expression.
    fn callback_expr_to_body_expr(
        &mut self,
        callback: &CallbackExpr,
        params: &[smelt_hir::ExprId],
        capture_locals: &HashMap<smelt_hir::LocalId, smelt_hir::LocalId>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = body.blocks[0].span;
        let kind = match &callback.kind {
            CallbackExprKind::Param(index) => {
                return params.get(*index).copied().ok_or_else(|| {
                    SmeltError::unsupported(span, "callback parameter index is out of bounds")
                });
            }
            CallbackExprKind::Capture(local) => ExprKind::Local(
                capture_locals
                    .get(local)
                    .copied()
                    .ok_or_else(|| SmeltError::unsupported(span, "callback capture is missing"))?,
            ),
            CallbackExprKind::Literal(literal) => ExprKind::Literal(literal.clone()),
            CallbackExprKind::ListLit(items) => ExprKind::ListLit(
                items
                    .iter()
                    .map(|item| self.callback_expr_to_body_expr(item, params, capture_locals, body))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            CallbackExprKind::Field { receiver, field } => ExprKind::Field {
                receiver: self.callback_expr_to_body_expr(
                    receiver,
                    params,
                    capture_locals,
                    body,
                )?,
                field: *field,
            },
            CallbackExprKind::Index { receiver, index } => {
                let receiver_expr =
                    self.callback_expr_to_body_expr(receiver, params, capture_locals, body)?;
                let literal_index = i64::try_from(*index).map_err(|error| {
                    SmeltError::unsupported(
                        span,
                        format!("callback index is too large for Rust: {error}"),
                    )
                })?;
                let index_ty = self.intern_type(Type::Int);
                let index_expr = body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::Int(literal_index)),
                    ty: index_ty,
                    span,
                });
                ExprKind::Index {
                    receiver: receiver_expr,
                    index: index_expr,
                }
            }
            CallbackExprKind::Binary { op, lhs, rhs } => ExprKind::BinOp {
                op: *op,
                lhs: self.callback_expr_to_body_expr(lhs, params, capture_locals, body)?,
                rhs: self.callback_expr_to_body_expr(rhs, params, capture_locals, body)?,
            },
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "Python callback expression is not lowered into a closure body yet",
                ));
            }
        };
        Ok(body.push_expr(HirExpr {
            kind,
            ty: callback.ty,
            span,
        }))
    }

    /// Collect explicit captures from a Python callback expression tree.
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
                if let Some(local_decl) = usize::try_from(local.0)
                    .ok()
                    .and_then(|index| body.locals.get(index))
                {
                    captures.entry(*local).or_insert_with(|| ClosureCapture {
                        source_local: *local,
                        body_local: None,
                        symbol: local_decl
                            .name
                            .unwrap_or_else(|| self.intern_name("__capture")),
                        ty: local_decl.ty,
                        mode: CaptureMode::ByRef,
                    });
                }
            }
            CallbackExprKind::AssignCapture { target, value } => {
                if let Some(local_decl) = usize::try_from(target.0)
                    .ok()
                    .and_then(|index| body.locals.get(index))
                {
                    captures.insert(
                        *target,
                        ClosureCapture {
                            source_local: *target,
                            body_local: None,
                            symbol: local_decl
                                .name
                                .unwrap_or_else(|| self.intern_name("__capture")),
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
            CallbackExprKind::Sequence { effects, result } => {
                for effect in effects {
                    self.collect_callback_captures(effect, body, captures);
                }
                self.collect_callback_captures(result, body, captures);
            }
            CallbackExprKind::DictLit(entries) => {
                for (_, value) in entries {
                    self.collect_callback_captures(value, body, captures);
                }
            }
            CallbackExprKind::Throw { message } => {
                if let Some(thrown_message) = message {
                    self.collect_callback_captures(thrown_message, body, captures);
                }
            }
            CallbackExprKind::Index { receiver, .. }
            | CallbackExprKind::Field { receiver, .. }
            | CallbackExprKind::HasField { receiver, .. }
            | CallbackExprKind::FieldTruthy { receiver, .. } => {
                self.collect_callback_captures(receiver, body, captures);
            }
            CallbackExprKind::DynamicIndex { receiver, index } => {
                self.collect_callback_captures(receiver, body, captures);
                self.collect_callback_captures(index, body, captures);
            }
            CallbackExprKind::HasDynamicField { receiver, field } => {
                self.collect_callback_captures(receiver, body, captures);
                self.collect_callback_captures(field, body, captures);
            }
            CallbackExprKind::Unary { operand, .. } => {
                self.collect_callback_captures(operand, body, captures);
            }
            CallbackExprKind::UnknownIs { value, .. } => {
                self.collect_callback_captures(value, body, captures);
            }
            CallbackExprKind::TypeofValue { value } => {
                self.collect_callback_captures(value, body, captures);
            }
            CallbackExprKind::Binary { lhs, rhs, .. } => {
                self.collect_callback_captures(lhs, body, captures);
                self.collect_callback_captures(rhs, body, captures);
            }
            CallbackExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                self.collect_callback_captures(cond, body, captures);
                self.collect_callback_captures(then_expr, body, captures);
                self.collect_callback_captures(else_expr, body, captures);
            }
            CallbackExprKind::Call { callee, args } => {
                self.collect_callback_captures(callee, body, captures);
                for arg in args {
                    self.collect_callback_captures(&arg.expr, body, captures);
                }
            }
            CallbackExprKind::MethodCall { receiver, args, .. } => {
                self.collect_callback_captures(receiver, body, captures);
                for arg in args {
                    self.collect_callback_captures(&arg.expr, body, captures);
                }
            }
            CallbackExprKind::FunctionTableLookup { key, .. } => {
                self.collect_callback_captures(key, body, captures);
            }
            CallbackExprKind::Param(_)
            | CallbackExprKind::Function(_)
            | CallbackExprKind::Literal(_) => {}
        }
    }

    /// Lower a Python lambda body into a callback expression tree.
    fn lambda_callback(
        &mut self,
        lambda: &ruff_python_ast::ExprLambda,
        expected_param_tys: &[TypeId],
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        let params = lambda.parameters.as_ref().ok_or_else(|| {
            SmeltError::unsupported(self.span(lambda.range), "lambda requires parameters")
        })?;
        if !params.kwonlyargs.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(lambda.range),
                "keyword-only lambda parameters need callable keyword parameter modelling",
            ));
        }
        let param_items = params
            .posonlyargs
            .iter()
            .chain(&params.args)
            .collect::<Vec<_>>();
        if param_items.len() > expected_param_tys.len() {
            return Err(SmeltError::unsupported(
                self.span(lambda.range),
                "lambda parameter count does not match callback input",
            ));
        }
        let mut callback_params = HashMap::new();
        for (index, param) in param_items.iter().enumerate() {
            callback_params.insert(
                param.parameter.name.as_str(),
                CallbackExpr {
                    kind: CallbackExprKind::Param(index),
                    ty: expected_param_tys[index],
                },
            );
        }
        if let Some(vararg) = &params.vararg {
            let rest_items = expected_param_tys
                .iter()
                .enumerate()
                .skip(param_items.len())
                .map(|(index, ty)| CallbackExpr {
                    kind: CallbackExprKind::Param(index),
                    ty: *ty,
                })
                .collect::<Vec<_>>();
            let item_ty = rest_items
                .first()
                .map_or_else(|| self.intern_type(Type::Unknown), |item| item.ty);
            callback_params.insert(
                vararg.name.as_str(),
                CallbackExpr {
                    kind: CallbackExprKind::ListLit(rest_items),
                    ty: self.intern_type(Type::List(item_ty)),
                },
            );
        }
        if let Some(kwarg) = &params.kwarg {
            let kwarg_ty = expected_param_tys.last().copied().unwrap_or_else(|| {
                let string_ty = self.intern_type(Type::String);
                let unknown_ty = self.intern_type(Type::Unknown);
                self.intern_type(Type::Dict(string_ty, unknown_ty))
            });
            callback_params.insert(
                kwarg.name.as_str(),
                CallbackExpr {
                    kind: CallbackExprKind::Param(expected_param_tys.len().saturating_sub(1)),
                    ty: kwarg_ty,
                },
            );
        }
        self.python_callback_expr(&lambda.body, &callback_params, body)
    }

    /// Lower default expressions for a Python lambda parameter list.
    fn lambda_defaults(
        &mut self,
        lambda: &ruff_python_ast::ExprLambda,
        body: &mut Body,
    ) -> Result<Vec<Option<smelt_hir::ExprId>>, SmeltError> {
        let params = lambda.parameters.as_ref().ok_or_else(|| {
            SmeltError::unsupported(self.span(lambda.range), "lambda requires parameters")
        })?;
        let param_items = params
            .posonlyargs
            .iter()
            .chain(&params.args)
            .collect::<Vec<_>>();
        param_items
            .iter()
            .map(|param| {
                param
                    .default
                    .as_ref()
                    .map(|default| self.expression(default, body))
                    .transpose()
            })
            .collect()
    }

    /// Lower expressions allowed inside Python callback bodies.
    fn python_callback_expr(
        &mut self,
        expr: &Expr,
        params: &HashMap<&str, CallbackExpr>,
        body: &Body,
    ) -> Result<CallbackExpr, SmeltError> {
        match expr {
            Expr::Name(name) => {
                if let Some(param) = params.get(name.id.as_str()).cloned() {
                    return Ok(param);
                }
                let Some(local) = self.locals.get(name.id.as_str()).copied() else {
                    return Err(SmeltError::unsupported(
                        self.span(name.range),
                        format!("unresolved callback identifier `{}`", name.id),
                    ));
                };
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Capture(local),
                    ty: Self::local_ty(body, local),
                })
            }
            Expr::NumberLiteral(number) => match &number.value {
                Number::Int(value) => Ok(CallbackExpr {
                    kind: CallbackExprKind::Literal(Literal::Int(value.as_i64().ok_or_else(
                        || {
                            SmeltError::unsupported(
                                self.span(number.range),
                                "integer literal is too large",
                            )
                        },
                    )?)),
                    ty: self.intern_type(Type::Int),
                }),
                Number::Float(value) => Ok(CallbackExpr {
                    kind: CallbackExprKind::Literal(Literal::Float(*value)),
                    ty: self.intern_type(Type::Float),
                }),
                Number::Complex { .. } => Err(SmeltError::unsupported(
                    self.span(number.range),
                    "complex callback literals are not supported",
                )),
            },
            Expr::BooleanLiteral(boolean) => Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::Bool(boolean.value)),
                ty: self.intern_type(Type::Bool),
            }),
            Expr::StringLiteral(string) => Ok(CallbackExpr {
                kind: CallbackExprKind::Literal(Literal::String(string.value.to_string())),
                ty: self.intern_type(Type::String),
            }),
            Expr::BinOp(binary) => {
                let op = match binary.op {
                    Operator::Add => BinOp::Add,
                    Operator::Sub => BinOp::Sub,
                    Operator::Mult => BinOp::Mul,
                    Operator::Div => BinOp::Div,
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.range),
                            "callback binary operator is not supported",
                        ));
                    }
                };
                let lhs = self.python_callback_expr(&binary.left, params, body)?;
                let rhs = self.python_callback_expr(&binary.right, params, body)?;
                let ty = lhs.ty;
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty,
                })
            }
            Expr::Compare(compare) => {
                let [cmp_op] = &*compare.ops else {
                    return Err(SmeltError::unsupported(
                        self.span(compare.range),
                        "callback chained comparisons are not supported",
                    ));
                };
                let [right_expr] = compare.comparators.as_ref() else {
                    return Err(SmeltError::unsupported(
                        self.span(compare.range),
                        "callback comparison is missing a right operand",
                    ));
                };
                let op = match cmp_op {
                    CmpOp::Eq => BinOp::Eq,
                    CmpOp::NotEq => BinOp::NotEq,
                    CmpOp::Lt => BinOp::Lt,
                    CmpOp::LtE => BinOp::Lte,
                    CmpOp::Gt => BinOp::Gt,
                    CmpOp::GtE => BinOp::Gte,
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(compare.range),
                            "callback comparison operator is not supported",
                        ));
                    }
                };
                let lhs = self.python_callback_expr(&compare.left, params, body)?;
                let rhs = self.python_callback_expr(right_expr, params, body)?;
                Ok(CallbackExpr {
                    kind: CallbackExprKind::Binary {
                        op,
                        lhs: Box::new(lhs),
                        rhs: Box::new(rhs),
                    },
                    ty: self.intern_type(Type::Bool),
                })
            }
            Expr::Subscript(sub) => {
                let receiver = self.python_callback_expr(&sub.value, params, body)?;
                match sub.slice.as_ref() {
                    Expr::NumberLiteral(number) => {
                        let Number::Int(value) = &number.value else {
                            return Err(SmeltError::unsupported(
                                self.span(number.range),
                                "callback list index must be an integer",
                            ));
                        };
                        let index = value.as_i64().ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(number.range),
                                "callback list index is too large",
                            )
                        })?;
                        if index < 0 {
                            return Err(SmeltError::unsupported(
                                self.span(number.range),
                                "callback list index must be non-negative",
                            ));
                        }
                        let index_usize = usize::try_from(index).map_err(|err| {
                            SmeltError::unsupported(
                                self.span(number.range),
                                format!("callback list index does not fit in usize: {err}"),
                            )
                        })?;
                        let item_ty = match self.ctx.krate.types.get(receiver.ty) {
                            Some(Type::Tuple(items)) => {
                                items.get(index_usize).copied().ok_or_else(|| {
                                    SmeltError::unsupported(
                                        self.span(sub.range),
                                        "callback tuple index is out of bounds",
                                    )
                                })?
                            }
                            Some(Type::List(item_ty)) => *item_ty,
                            _ => {
                                return Err(SmeltError::unsupported(
                                    self.span(sub.range),
                                    "callback numeric subscript receiver must be a tuple or list",
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
                    Expr::StringLiteral(string) => {
                        let value_ty = match self.ctx.krate.types.get(receiver.ty) {
                            Some(Type::Dict(_, value_ty)) => *value_ty,
                            _ => {
                                return Err(SmeltError::unsupported(
                                    self.span(sub.range),
                                    "callback string subscript receiver must be a dict",
                                ));
                            }
                        };
                        let field_text = string.value.to_string();
                        Ok(CallbackExpr {
                            kind: CallbackExprKind::Field {
                                receiver: Box::new(receiver),
                                field: self.intern_name(&field_text),
                            },
                            ty: value_ty,
                        })
                    }
                    _ => Err(SmeltError::unsupported(
                        self.span(sub.range),
                        "callback subscript index must be a static int or string",
                    )),
                }
            }
            _ => Err(SmeltError::unsupported(
                self.span(expr.range()),
                "callback expression is not supported yet",
            )),
        }
    }

    /// Lower `set(value)` for the currently supported direct container inputs.
    fn set_constructor_from_arg(
        &mut self,
        arg: &Expr,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let source = self.expression(arg, body)?;
        let source_ty = Self::expr_ty(body, source);
        let (kind, ty) = match self.ctx.krate.types.get(source_ty) {
            Some(Type::Set(_)) => (ExprKind::SetCopy { set: source }, source_ty),
            Some(Type::List(item_type)) => {
                let item_ty = *item_type;
                (
                    ExprKind::ListToSet { list: source },
                    self.intern_type(Type::Set(item_ty)),
                )
            }
            Some(Type::Tuple(items)) => {
                let item_ty = Self::homogeneous_tuple_item_ty(items, span)?;
                (
                    ExprKind::TupleToSet { tuple: source },
                    self.intern_type(Type::Set(item_ty)),
                )
            }
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "set(value) currently requires a set, list, or homogeneous tuple value",
                ));
            }
        };
        Ok(Some(body.push_expr(HirExpr { kind, ty, span })))
    }

    /// Lower `dict(value)` for the currently supported direct container inputs.
    fn dict_constructor_from_arg(
        &mut self,
        arg: &Expr,
        body: &mut Body,
        span: Span,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let source = self.expression(arg, body)?;
        let source_ty = Self::expr_ty(body, source);
        let (kind, ty) = match self.ctx.krate.types.get(source_ty) {
            Some(Type::Dict(_, _)) => (ExprKind::DictCopy { dict: source }, source_ty),
            Some(Type::List(item_ty)) => {
                let Some(Type::Tuple(items)) = self.ctx.krate.types.get(*item_ty) else {
                    return Err(SmeltError::unsupported(
                        span,
                        "dict(value) list input must contain 2-item tuples",
                    ));
                };
                let [key_ty, value_ty] = items.as_slice() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "dict(value) list input must contain 2-item tuples",
                    ));
                };
                (
                    ExprKind::ListPairsToDict { list: source },
                    self.intern_type(Type::Dict(*key_ty, *value_ty)),
                )
            }
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "dict(value) currently requires a dict value or list of 2-item tuples",
                ));
            }
        };
        Ok(Some(body.push_expr(HirExpr { kind, ty, span })))
    }

    /// Lower `tuple(value)` for the currently supported direct container inputs.
    fn tuple_constructor_from_arg(
        &mut self,
        arg: &Expr,
        body: &mut Body,
        span: Span,
        type_hint: Option<TypeId>,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let source = self.expression(arg, body)?;
        let source_ty = Self::expr_ty(body, source);
        match self.ctx.krate.types.get(source_ty) {
            Some(Type::Tuple(_)) => Ok(Some(source)),
            Some(Type::List(item_ty)) => {
                let Some(tuple_ty) = type_hint else {
                    return Err(SmeltError::unsupported(
                        span,
                        "tuple(list_value) requires a tuple type annotation",
                    ));
                };
                let Some(Type::Tuple(items)) = self.ctx.krate.types.get(tuple_ty) else {
                    return Err(SmeltError::unsupported(
                        span,
                        "tuple(list_value) type annotation must be tuple[...]",
                    ));
                };
                if !items.iter().all(|tuple_item| tuple_item == item_ty) {
                    return Err(SmeltError::unsupported(
                        span,
                        "tuple(list_value) annotation item types must match the list item type",
                    ));
                }
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::ListToTuple { list: source },
                    ty: tuple_ty,
                    span,
                })))
            }
            _ => Err(SmeltError::unsupported(
                span,
                "tuple(value) currently requires a tuple value or annotated list value",
            )),
        }
    }

    // Homogeneous tuple helpers continue in `math.rs`.
}
