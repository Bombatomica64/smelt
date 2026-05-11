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
