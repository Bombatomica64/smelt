impl ModuleBuilder<'_> {
    // -----------------------------------------------------------------------
    // Function definition
    // -----------------------------------------------------------------------

    /// Lower a Python function definition, expanding simple pytest parametrization.
    fn function_defs(&mut self, func: &StmtFunctionDef) -> Result<Vec<ItemId>, SmeltError> {
        if self.is_pytest_skipped_or_xfailed(func)? {
            return Ok(Vec::new());
        }
        if let Some(parametrize) = self.pytest_parametrize(func)? {
            return self.parametrize_function_defs(func, &parametrize);
        }
        let item = self.function_def(func)?;
        if Self::is_pytest_fixture(func) {
            let return_ty = self.function_return_ty(item)?;
            self.pytest_fixtures.insert(
                func.name.as_str().to_owned(),
                Self::pytest_fixture_metadata(func, item, return_ty),
            );
        }
        Ok(vec![item])
    }

    /// Return whether a function is decorated with `@pytest.fixture`.
    fn is_pytest_fixture(func: &StmtFunctionDef) -> bool {
        func.decorator_list
            .iter()
            .any(|decorator| decorator_simple_name(decorator) == Some("fixture"))
    }

    /// Extract the supported fixture metadata while accepting unknown pytest options.
    fn pytest_fixture_metadata(func: &StmtFunctionDef, item: ItemId, ty: TypeId) -> PytestFixture {
        let autouse = func.decorator_list.iter().any(|decorator| {
            if decorator_simple_name(decorator) != Some("fixture") {
                return false;
            }
            let Expr::Call(call) = &decorator.expression else {
                return false;
            };
            call.arguments.keywords.iter().any(|keyword| {
                keyword
                    .arg
                    .as_ref()
                    .is_some_and(|arg| arg.as_str() == "autouse")
                    && matches!(&keyword.value, Expr::BooleanLiteral(value) if value.value)
            })
        });
        PytestFixture { item, ty, autouse }
    }

    /// Return whether a pytest function should be emitted as a skipped test.
    fn is_pytest_skipped_or_xfailed(&self, func: &StmtFunctionDef) -> Result<bool, SmeltError> {
        if !self.pytest_mode || !test_support::is_pytest_test_function(func.name.as_str()) {
            return Ok(false);
        }
        for decorator in &func.decorator_list {
            match decorator_simple_name(decorator) {
                Some("skip") | Some("xfail") => return Ok(true),
                Some("skipif") => {
                    let Expr::Call(call) = &decorator.expression else {
                        return Err(SmeltError::unsupported(
                            self.span(decorator.range),
                            "pytest.mark.skipif must be called",
                        ));
                    };
                    let Some(condition) = call.arguments.args.first() else {
                        return Err(SmeltError::unsupported(
                            self.span(call.range()),
                            "pytest.mark.skipif requires a boolean condition",
                        ));
                    };
                    let Expr::BooleanLiteral(value) = condition else {
                        return Err(SmeltError::unsupported(
                            self.span(condition.range()),
                            "pytest.mark.skipif supports only literal boolean conditions",
                        ));
                    };
                    if value.value {
                        return Ok(true);
                    }
                }
                _ => {}
            }
        }
        Ok(false)
    }

    /// Return the declared return type for a lowered function item.
    fn function_return_ty(&self, item: ItemId) -> Result<TypeId, SmeltError> {
        let index = usize::try_from(item.0).map_err(|err| {
            SmeltError::unsupported(
                Span::new(self.file_id, 0, 0),
                format!("internal error: fixture item id does not fit in usize: {err}"),
            )
        })?;
        let Some(Item::Function(function)) = self.ctx.krate.items.get(index) else {
            return Err(SmeltError::unsupported(
                Span::new(self.file_id, 0, 0),
                "internal error: pytest fixture did not lower to a function",
            ));
        };
        Ok(function.return_ty)
    }

    /// Extract a simple `@pytest.mark.parametrize(...)` decorator, if present.
    fn pytest_parametrize(
        &self,
        func: &StmtFunctionDef,
    ) -> Result<Option<PytestParametrize>, SmeltError> {
        if !self.is_pytest_test_function(func) {
            return Ok(None);
        }
        let Some(decorator) = func
            .decorator_list
            .iter()
            .find(|decorator| decorator_simple_name(decorator) == Some("parametrize"))
        else {
            return Ok(None);
        };
        let Expr::Call(call) = &decorator.expression else {
            return Err(SmeltError::unsupported(
                self.span(decorator.range),
                "pytest.mark.parametrize must be called",
            ));
        };
        let [names_expr, rows_expr] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "pytest.mark.parametrize requires names and rows",
            ));
        };
        for keyword in &call.arguments.keywords {
            let Some(arg) = keyword.arg.as_ref().map(|arg| arg.as_str()) else {
                return Err(SmeltError::unsupported(
                    self.span(keyword.range),
                    "pytest.mark.parametrize **kwargs are not supported yet",
                ));
            };
            if arg != "ids" {
                return Err(SmeltError::unsupported(
                    self.span(keyword.range),
                    format!(
                        "pytest.mark.parametrize keyword argument '{arg}' is not supported yet"
                    ),
                ));
            }
        }
        let names = self.pytest_parametrize_names(names_expr)?;
        let rows = self.pytest_parametrize_rows(rows_expr, names.len())?;
        Ok(Some(PytestParametrize { names, rows }))
    }

    /// Lower one parametrized pytest function into one Rust test item per row.
    fn parametrize_function_defs(
        &mut self,
        func: &StmtFunctionDef,
        parametrize: &PytestParametrize,
    ) -> Result<Vec<ItemId>, SmeltError> {
        let mut items = Vec::new();
        for (case_index, row) in parametrize.rows.iter().enumerate() {
            items.push(self.parametrize_function_def(func, parametrize, row, case_index)?);
        }
        Ok(items)
    }

    /// Lower one pytest parametrization row into a concrete no-argument test.
    fn parametrize_function_def(
        &mut self,
        func: &StmtFunctionDef,
        parametrize: &PytestParametrize,
        row: &[PytestLiteral],
        case_index: usize,
    ) -> Result<ItemId, SmeltError> {
        let base_name = func.name.as_str();
        let generated_name = format!("{base_name}__case_{case_index}");
        let name = self.intern_name(&generated_name);
        let return_ty = self.intern_type(Type::None);
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_async = self.current_async;
        self.current_async = false;
        let func_span = self.span(func.range);
        let mut fn_body = Body::new(None, func_span);
        let root_block = fn_body.root;

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
                fn_body.push_stmt_to_block(root_block, HirStmt::Expr(value));
            }
        }

        for (param_name, value) in parametrize.names.iter().zip(row) {
            let ty = self.pytest_literal_type(value, func_span)?;
            let sym = self.intern_name(param_name);
            let local = fn_body.push_local(LocalDecl {
                name: Some(sym),
                ty,
                mutable: false,
                span: func_span,
            });
            self.locals.insert(param_name.clone(), local);
            let value_expr = self.pytest_literal_expr(value, &mut fn_body, func_span)?;
            let pat = fn_body.push_pattern(HirPattern::Binding(local));
            fn_body.push_stmt_to_block(
                root_block,
                HirStmt::Let {
                    pat,
                    ty,
                    value: Some(value_expr),
                },
            );
        }

        let body_error = func
            .body
            .iter()
            .find_map(|stmt| self.statement(stmt, &mut fn_body).err());
        self.locals = saved_locals;
        self.current_async = saved_async;
        if let Some(err) = body_error {
            return Err(err);
        }
        let body_id = self.ctx.krate.push_body(fn_body);
        let item = Item::Function(Function {
            name,
            span: func_span,
            // Generic free functions are a TypeScript-frontend feature; the
            // Python frontend declares no free-function type params here.
            type_params: Vec::new(),
            params: Vec::new(),
            rest: None,
            required_params: None,
return_ty,
            is_async: false,
            is_test: true,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        });
        let item_id = self.ctx.krate.push_item(item);
        self.items.insert(generated_name, item_id);
        Ok(item_id)
    }

    /// Parse pytest parametrization names from a string literal.
    fn pytest_parametrize_names(&self, expr: &Expr) -> Result<Vec<String>, SmeltError> {
        let Expr::StringLiteral(value) = expr else {
            return Err(SmeltError::unsupported(
                self.span(expr.range()),
                "pytest.mark.parametrize names must be a string literal",
            ));
        };
        let names = value
            .value
            .to_str()
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        if names.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(expr.range()),
                "pytest.mark.parametrize requires at least one parameter name",
            ));
        }
        Ok(names)
    }

    /// Parse pytest parametrization rows from a list or tuple literal.
    fn pytest_parametrize_rows(
        &self,
        expr: &Expr,
        width: usize,
    ) -> Result<Vec<Vec<PytestLiteral>>, SmeltError> {
        let row_exprs = match expr {
            Expr::List(list) => list.elts.as_slice(),
            Expr::Tuple(tuple) => tuple.elts.as_slice(),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(expr.range()),
                    "pytest.mark.parametrize rows must be a list or tuple literal",
                ));
            }
        };
        let mut rows = Vec::new();
        for row_expr in row_exprs {
            let row = if width == 1 {
                vec![self.pytest_literal(row_expr)?]
            } else if let Expr::Call(call) = row_expr
                && Self::is_pytest_param_call(call)
            {
                if call.arguments.args.len() < width {
                    return Err(SmeltError::unsupported(
                        self.span(call.range()),
                        "pytest.param row width does not match parameter names",
                    ));
                }
                call.arguments
                    .args
                    .iter()
                    .take(width)
                    .map(|value| self.pytest_literal(value))
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                let values = match row_expr {
                    Expr::List(list) => list.elts.as_slice(),
                    Expr::Tuple(tuple) => tuple.elts.as_slice(),
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(row_expr.range()),
                            "multi-argument pytest.mark.parametrize rows must be tuples or lists",
                        ));
                    }
                };
                if values.len() != width {
                    return Err(SmeltError::unsupported(
                        self.span(row_expr.range()),
                        "pytest.mark.parametrize row width does not match parameter names",
                    ));
                }
                values
                    .iter()
                    .map(|value| self.pytest_literal(value))
                    .collect::<Result<Vec<_>, _>>()?
            };
            rows.push(row);
        }
        if rows.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(expr.range()),
                "pytest.mark.parametrize requires at least one row",
            ));
        }
        Ok(rows)
    }

    /// Parse one supported pytest parametrization literal.
    fn pytest_literal(&self, expr: &Expr) -> Result<PytestLiteral, SmeltError> {
        match expr {
            Expr::Call(call) if Self::is_pytest_param_call(call) => {
                let Some(value) = call.arguments.args.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.range()),
                        "pytest.param requires at least one value",
                    ));
                };
                self.pytest_literal(value)
            }
            Expr::BooleanLiteral(value) => Ok(PytestLiteral::Bool(value.value)),
            Expr::NumberLiteral(number) => match &number.value {
                Number::Int(value) => value.as_i64().map(PytestLiteral::Int).ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(number.range),
                        "pytest.mark.parametrize integer literal out of i64 range",
                    )
                }),
                Number::Float(value) => Ok(PytestLiteral::Float(*value)),
                Number::Complex { .. } => Err(SmeltError::unsupported(
                    self.span(number.range),
                    "pytest.mark.parametrize does not support complex literals",
                )),
            },
            Expr::StringLiteral(value) => {
                Ok(PytestLiteral::String(value.value.to_str().to_owned()))
            }
            Expr::NoneLiteral(_) => Ok(PytestLiteral::None),
            Expr::Tuple(tuple) => tuple
                .elts
                .iter()
                .map(|elt| self.pytest_literal(elt))
                .collect::<Result<Vec<_>, _>>()
                .map(PytestLiteral::Tuple),
            Expr::List(list) => list
                .elts
                .iter()
                .map(|elt| self.pytest_literal(elt))
                .collect::<Result<Vec<_>, _>>()
                .map(PytestLiteral::List),
            _ => Err(SmeltError::unsupported(
                self.span(expr.range()),
                "pytest.mark.parametrize supports only bool, number, string, None, tuple, and list literals",
            )),
        }
    }

    /// Return whether a call expression is `pytest.param(...)` or imported `param(...)`.
    fn is_pytest_param_call(call: &ruff_python_ast::ExprCall) -> bool {
        match call.func.as_ref() {
            Expr::Attribute(attr) if attr.attr.as_str() == "param" => {
                matches!(attr.value.as_ref(), Expr::Name(name) if name.id.as_str() == "pytest")
            }
            Expr::Name(name) => name.id.as_str() == "param",
            _ => false,
        }
    }
}
