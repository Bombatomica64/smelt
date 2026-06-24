impl ModuleBuilder<'_> {
    /// Return the HIR type for a parametrization literal.
    fn pytest_literal_type(
        &mut self,
        value: &PytestLiteral,
        span: Span,
    ) -> Result<TypeId, SmeltError> {
        match value {
            PytestLiteral::Bool(_) => Ok(self.intern_type(Type::Bool)),
            PytestLiteral::Int(_) => Ok(self.intern_type(Type::Int)),
            PytestLiteral::Float(_) => Ok(self.intern_type(Type::Float)),
            PytestLiteral::String(_) => Ok(self.intern_type(Type::String)),
            PytestLiteral::None => Ok(self.intern_type(Type::None)),
            PytestLiteral::Tuple(items) => {
                let types = items
                    .iter()
                    .map(|item| self.pytest_literal_type(item, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.intern_type(Type::Tuple(types)))
            }
            PytestLiteral::List(items) => {
                let Some(first) = items.first() else {
                    let none = self.intern_type(Type::None);
                    return Ok(self.intern_type(Type::List(none)));
                };
                let first_ty = self.pytest_literal_type(first, span)?;
                for item in &items[1..] {
                    let item_ty = self.pytest_literal_type(item, span)?;
                    if item_ty != first_ty {
                        return Err(SmeltError::unsupported(
                            span,
                            "pytest.mark.parametrize list literals must be homogeneous",
                        ));
                    }
                }
                Ok(self.intern_type(Type::List(first_ty)))
            }
        }
    }

    /// Push a HIR expression for a parametrization literal.
    fn pytest_literal_expr(
        &mut self,
        literal_value: &PytestLiteral,
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match literal_value {
            PytestLiteral::Bool(inner) => {
                let ty = self.intern_type(Type::Bool);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::Bool(*inner)),
                    ty,
                    span,
                }))
            }
            PytestLiteral::Int(inner) => {
                let ty = self.intern_type(Type::Int);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::Int(*inner)),
                    ty,
                    span,
                }))
            }
            PytestLiteral::Float(inner) => {
                let ty = self.intern_type(Type::Float);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::Float(*inner)),
                    ty,
                    span,
                }))
            }
            PytestLiteral::String(inner) => {
                let ty = self.intern_type(Type::String);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::String(inner.clone())),
                    ty,
                    span,
                }))
            }
            PytestLiteral::None => {
                let ty = self.intern_type(Type::None);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span,
                }))
            }
            PytestLiteral::Tuple(items) => {
                let elts = items
                    .iter()
                    .map(|item| self.pytest_literal_expr(item, body, span))
                    .collect::<Result<Vec<_>, _>>()?;
                let elem_types = elts.iter().map(|&expr| Self::expr_ty(body, expr)).collect();
                let ty = self.intern_type(Type::Tuple(elem_types));
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::TupleLit(elts),
                    ty,
                    span,
                }))
            }
            PytestLiteral::List(items) => {
                let elts = items
                    .iter()
                    .map(|item| self.pytest_literal_expr(item, body, span))
                    .collect::<Result<Vec<_>, _>>()?;
                let ty = if let Some(first) = elts.first().copied() {
                    let elem_ty = Self::expr_ty(body, first);
                    self.intern_type(Type::List(elem_ty))
                } else {
                    let none = self.intern_type(Type::None);
                    self.intern_type(Type::List(none))
                };
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::ListLit(elts),
                    ty,
                    span,
                }))
            }
        }
    }

    /// Lower a Python function definition into an HIR function item.
    fn function_def(&mut self, func: &StmtFunctionDef) -> Result<ItemId, SmeltError> {
        let name_str = func.name.as_str();
        let name = self.intern_name(name_str);

        // Return type — required except for pytest test functions, where an
        // omitted annotation is treated as `-> None`.
        let annotated_return_ty = match func.returns.as_deref() {
            Some(annotation) => self.annotation_to_hir(annotation)?,
            None if self.is_pytest_test_function(func) => self.intern_type(Type::None),
            None => {
                return Err(SmeltError::type_constraint(
                    self.span(func.range),
                    format!("function '{name_str}' must have an explicit return type annotation"),
                ));
            }
        };
        let return_ty = if func.is_async {
            self.future_type(annotated_return_ty)
        } else {
            annotated_return_ty
        };

        // Save outer scope and start fresh for this function's body.
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_async = self.current_async;
        self.current_async = func.is_async;
        let func_span = self.span(func.range);
        let mut fn_body = Body::new(None, func_span);
        let mut params: Vec<Param> = Vec::new();
        let is_pytest_test = self.is_pytest_test_function(func);

        if is_pytest_test {
            for fixture in self.pytest_fixtures.values().copied() {
                if fixture.autouse {
                    let callee = fn_body.push_expr(HirExpr {
                        kind: ExprKind::Item(fixture.item),
                        ty: fixture.ty,
                        span: func_span,
                    });
                    let value = fn_body.push_expr(HirExpr {
                        kind: ExprKind::Call {
                            callee,
                            args: Vec::new(),
                        },
                        ty: fixture.ty,
                        span: func_span,
                    });
                    fn_body.push_stmt(HirStmt::Expr(value));
                }
            }
        }

        // Parameters, with Python variadics represented as list/dict parameters.
        for param_with_default in func.parameters.iter_non_variadic_params() {
            let p = &param_with_default.parameter;
            let param_name_str = p.name.as_str();
            if is_pytest_test
                && let Some(fixture) = self.pytest_fixtures.get(param_name_str).copied()
            {
                let param_name = self.intern_name(param_name_str);
                let local = fn_body.push_local(LocalDecl {
                    name: Some(param_name),
                    ty: fixture.ty,
                    mutable: false,
                    span: self.span(p.range),
                });
                self.locals.insert(param_name_str.to_owned(), local);
                let callee = fn_body.push_expr(HirExpr {
                    kind: ExprKind::Item(fixture.item),
                    ty: fixture.ty,
                    span: self.span(p.range),
                });
                let value = fn_body.push_expr(HirExpr {
                    kind: ExprKind::Call {
                        callee,
                        args: Vec::new(),
                    },
                    ty: fixture.ty,
                    span: self.span(p.range),
                });
                let pat = fn_body.push_pattern(HirPattern::Binding(local));
                fn_body.push_stmt(HirStmt::Let {
                    pat,
                    ty: fixture.ty,
                    value: Some(value),
                });
                continue;
            }

            let param_ty = p
                .annotation
                .as_deref()
                .ok_or_else(|| {
                    SmeltError::type_constraint(
                        self.span(p.range),
                        format!(
                            "parameter '{param_name_str}' must have an explicit type annotation"
                        ),
                    )
                })
                .and_then(|ann| self.annotation_to_hir(ann))?;

            let param_name = self.intern_name(param_name_str);
            let local = fn_body.push_local(LocalDecl {
                name: Some(param_name),
                ty: param_ty,
                mutable: false,
                span: self.span(p.range),
            });
            fn_body.params.push(local);
            self.locals.insert(param_name_str.to_owned(), local);
            params.push(Param {
                name: param_name,
                local,
                ty: param_ty,
                span: self.span(p.range),
            });
        }

        let mut variadics = FunctionVariadics::default();
        if let Some(vararg) = &func.parameters.vararg {
            let param_name_str = vararg.name.as_str();
            let item_ty = vararg
                .annotation
                .as_deref()
                .ok_or_else(|| {
                    SmeltError::type_constraint(
                        self.span(vararg.range),
                        format!("parameter '*{param_name_str}' must have an explicit type annotation"),
                    )
                })
                .and_then(|ann| self.annotation_to_hir(ann))?;
            let list_ty = self.intern_type(Type::List(item_ty));
            let param_name = self.intern_name(param_name_str);
            let local = fn_body.push_local(LocalDecl {
                name: Some(param_name),
                ty: list_ty,
                mutable: false,
                span: self.span(vararg.range),
            });
            fn_body.params.push(local);
            self.locals.insert(param_name_str.to_owned(), local);
            let index = params.len();
            params.push(Param {
                name: param_name,
                local,
                ty: list_ty,
                span: self.span(vararg.range),
            });
            variadics.vararg = Some(VarArgParam { index, item_ty });
        }
        if let Some(kwarg) = &func.parameters.kwarg {
            let param_name_str = kwarg.name.as_str();
            let value_ty = kwarg
                .annotation
                .as_deref()
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(kwarg.range),
                        format!("parameter '**{param_name_str}' must have an explicit value type annotation"),
                    )
                })
                .and_then(|ann| self.annotation_to_hir(ann))?;
            let string_ty = self.intern_type(Type::String);
            let dict_ty = self.intern_type(Type::Dict(string_ty, value_ty));
            let param_name = self.intern_name(param_name_str);
            let local = fn_body.push_local(LocalDecl {
                name: Some(param_name),
                ty: dict_ty,
                mutable: false,
                span: self.span(kwarg.range),
            });
            fn_body.params.push(local);
            self.locals.insert(param_name_str.to_owned(), local);
            let index = params.len();
            params.push(Param {
                name: param_name,
                local,
                ty: dict_ty,
                span: self.span(kwarg.range),
            });
            variadics.kwarg = Some(KwArgParam { index, value_ty });
        }

        // Lower the body statements.
        let body_error = func
            .body
            .iter()
            .find_map(|stmt| self.statement(stmt, &mut fn_body).err());
        if func.is_async {
            fn_body.build_async_state_machine();
        }

        self.locals = saved_locals;
        self.current_async = saved_async;

        if let Some(err) = body_error {
            return Err(err);
        }

        let body_id = self.ctx.krate.push_body(fn_body);
        let item = Item::Function(Function {
            name,
            span: func_span,
            params,
            rest: None,
            required_params: None,
return_ty,
            is_async: func.is_async,
            is_test: self.is_pytest_test_function(func),
            body: Some(body_id),
            owner: FunctionOwner::Module,
        });
        let item_id = self.ctx.krate.push_item(item);
        self.items.insert(name_str.to_owned(), item_id);
        self.exports.insert(name_str.to_owned(), item_id);
        if variadics.vararg.is_some() || variadics.kwarg.is_some() {
            self.function_variadics
                .insert(name_str.to_owned(), variadics);
        }
        Ok(item_id)
    }

    /// Return whether `func` is a top-level pytest test function in pytest mode.
    fn is_pytest_test_function(&self, func: &StmtFunctionDef) -> bool {
        self.pytest_mode && test_support::is_pytest_test_function(func.name.as_str())
    }
}
