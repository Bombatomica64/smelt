impl ModuleBuilder<'_> {
    // -----------------------------------------------------------------------
    // Statement lowering
    // -----------------------------------------------------------------------

    /// Lower one statement into the body's root block.
    fn statement(&mut self, stmt: &Stmt, body: &mut Body) -> Result<(), SmeltError> {
        self.statement_in_block(stmt, body, body.root)
    }

    /// Lower one statement into a specific target block.
    fn statement_in_block(
        &mut self,
        stmt: &Stmt,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        match stmt {
            // `x: T = value` — typed variable declaration.
            Stmt::AnnAssign(ann) => self.ann_assign(ann, body, block),

            // `x = value` — assignment to an already-declared local.
            Stmt::Assign(s) => {
                if s.targets.len() != 1 {
                    return Err(SmeltError::unsupported(
                        self.span(s.range),
                        "multiple assignment targets are not supported",
                    ));
                }
                let [target_expr] = s.targets.as_slice() else {
                    return Err(SmeltError::unsupported(
                        self.span(s.range),
                        "multiple assignment targets are not supported",
                    ));
                };
                if Self::is_destructuring_target(target_expr) {
                    let value = self.expression(&s.value, body)?;
                    let value_ty = Self::expr_ty(body, value);
                    let pat =
                        self.binding_pattern_from_target(target_expr, body, Some(value_ty))?;
                    body.push_stmt_to_block(
                        block,
                        HirStmt::Let {
                            pat,
                            ty: value_ty,
                            value: Some(value),
                        },
                    );
                    return Ok(());
                }
                if let Expr::Name(target_name) = target_expr
                    && !self.locals.contains_key(target_name.id.as_str())
                {
                    let value = self.expression(&s.value, body)?;
                    let value_ty = Self::expr_ty(body, value);
                    let pat =
                        self.binding_pattern_from_target(target_expr, body, Some(value_ty))?;
                    body.push_stmt_to_block(
                        block,
                        HirStmt::Let {
                            pat,
                            ty: value_ty,
                            value: Some(value),
                        },
                    );
                    return Ok(());
                }
                let target = self.expression(target_expr, body)?;
                let value = self.expression(&s.value, body)?;
                body.push_stmt_to_block(block, HirStmt::Assign { target, value });
                Ok(())
            }

            // `x += value` — augmented assignment.
            Stmt::AugAssign(aug) => self.aug_assign(aug, body, block),

            // `return [value]`
            Stmt::Return(ret) => {
                let value = ret
                    .value
                    .as_deref()
                    .map(|v| self.expression(v, body))
                    .transpose()?;
                body.push_stmt_to_block(block, HirStmt::Return(value));
                Ok(())
            }

            // `if … elif … else …`
            Stmt::If(if_stmt) => self.if_statement(if_stmt, body, block),

            // `while test: …`
            Stmt::While(while_stmt) => {
                if !while_stmt.orelse.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(while_stmt.range),
                        "while-else is not supported",
                    ));
                }
                let cond = self.expression(&while_stmt.test, body)?;
                let loop_block = self.block_from_stmts(&while_stmt.body, body)?;
                body.push_stmt_to_block(
                    block,
                    HirStmt::While {
                        cond,
                        body: loop_block,
                    },
                );
                Ok(())
            }

            // `for target in iter: …`
            Stmt::For(for_stmt) => self.for_statement(for_stmt, body, block),

            // `match subject: …`
            Stmt::Match(match_stmt) => self.match_statement(match_stmt, body, block),

            // `raise ExceptionType(…)`
            Stmt::Raise(raise_stmt) => {
                if is_stop_iteration_raise(raise_stmt) {
                    let message = self.string_literal_expr("StopIteration", raise_stmt.range, body);
                    body.push_stmt_to_block(block, HirStmt::Throw(message));
                    return Ok(());
                }
                let expr = raise_stmt
                    .exc
                    .as_deref()
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(raise_stmt.range),
                            "bare re-raise is not supported",
                        )
                    })
                    .and_then(|e| self.expression(e, body))?;
                body.push_stmt_to_block(block, HirStmt::Throw(expr));
                Ok(())
            }

            // `assert expr[, message]`
            Stmt::Assert(assert_stmt) => self.assert_statement(assert_stmt, body, block),

            // `with pytest.raises(...): ...` or targeted static context managers.
            Stmt::With(with_stmt) => {
                if Self::with_is_pytest_raises(with_stmt) {
                    self.pytest_raises_with_statement(with_stmt, body, block)
                } else {
                    self.context_manager_with_statement(with_stmt, body, block)
                }
            }

            // Standalone expression statement (e.g. a function call).
            Stmt::Expr(s) => {
                if let Expr::Call(call) = s.value.as_ref()
                    && self.pytest_raises_callable_statement(call, body, block)?
                {
                    return Ok(());
                }
                let expr_id = self.expression(&s.value, body)?;
                body.push_stmt_to_block(block, HirStmt::Expr(expr_id));
                Ok(())
            }

            Stmt::Break(_) => {
                body.push_stmt_to_block(block, HirStmt::Break);
                Ok(())
            }
            Stmt::Continue(_) => {
                body.push_stmt_to_block(block, HirStmt::Continue);
                Ok(())
            }

            // `pass` — no HIR equivalent; silently skip.
            Stmt::Pass(_) => Ok(()),

            // Imports are collected at the module level in a future pass.
            Stmt::Import(_) | Stmt::ImportFrom(_) => Ok(()),

            // Nested functions become local closure values when their
            // signature and body can be represented by the closure IR.
            Stmt::FunctionDef(f) => self.nested_function_closure(f, body),
            Stmt::ClassDef(c) => Err(SmeltError::unsupported(
                self.span(c.range),
                "nested class definitions are not yet supported",
            )),

            Stmt::Delete(_)
            | Stmt::TypeAlias(_)
            | Stmt::Try(_)
            | Stmt::Global(_)
            | Stmt::Nonlocal(_)
            | Stmt::IpyEscapeCommand(_) => Err(SmeltError::unsupported(
                self.span(stmt.range()),
                format!("unsupported statement: {}", stmt_kind_name(stmt)),
            )),
        }
    }

    /// `x: T [= value]` → `Stmt::Let { pat, ty, value }`.
    fn ann_assign(
        &mut self,
        ann: &StmtAnnAssign,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let Expr::Name(target_name) = ann.target.as_ref() else {
            return Err(SmeltError::unsupported(
                self.span(ann.range),
                "annotated assignment target must be a simple name",
            ));
        };

        let ty = self.annotation_to_hir(&ann.annotation)?;
        if let Some(value) = ann.value.as_deref()
            && let Expr::Lambda(lambda) = value
        {
            return self.ann_lambda_assign((target_name.id.as_str(), lambda, ty, self.span(ann.range)), body);
        }
        let value = ann
            .value
            .as_deref()
            .map(|v| self.expression_with_hint(v, body, Some(ty)))
            .transpose()?;

        let name_str = target_name.id.as_str();
        let name_sym = self.intern_name(name_str);
        let local = body.push_local(LocalDecl {
            name: Some(name_sym),
            ty,
            mutable: true,
            span: self.span(target_name.range),
        });
        self.locals.insert(name_str.to_owned(), local);

        let pat = body.push_pattern(HirPattern::Binding(local));
        body.push_stmt_to_block(block, HirStmt::Let { pat, ty, value });
        Ok(())
    }

    /// Lower `name: Callable[[...], R] = lambda ...` as a local callback value.
    fn ann_lambda_assign(
        &mut self,
        assignment: (&str, &ruff_python_ast::ExprLambda, TypeId, Span),
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let (name, lambda, ty, span) = assignment;
        let Some(Type::Function(function)) = self.ctx.krate.types.get(ty).cloned() else {
            return Err(SmeltError::unsupported(
                span,
                "local lambda assignments require a Callable annotation",
            ));
        };
        let callback = self.lambda_callback(lambda, &function.params, body)?;
        let defaults = self.lambda_defaults(lambda, body)?;
        if callback.ty != function.return_ty {
            return Err(SmeltError::unsupported(
                self.span(lambda.range),
                "local lambda return type does not match Callable annotation",
            ));
        }
        let name_sym = self.intern_name(name);
        let local = body.push_local(LocalDecl {
            name: Some(name_sym),
            ty,
            mutable: true,
            span,
        });
        self.locals.insert(name.to_owned(), local);
        self.local_callbacks.insert(
            name.to_owned(),
            LocalCallback {
                callback,
                params: function.params.clone(),
                defaults: {
                    let mut resized_defaults = defaults;
                    resized_defaults.resize(function.params.len(), None);
                    resized_defaults
                },
                vararg: Self::lambda_vararg_metadata(
                    lambda,
                    &function.params,
                    &self.ctx.krate.types,
                ),
                kwarg: Self::lambda_kwarg_metadata(
                    lambda,
                    &function.params,
                    &self.ctx.krate.types,
                ),
                return_ty: function.return_ty,
            },
        );
        Ok(())
    }

    /// Lower a nested Python function definition as a local closure value.
    fn nested_function_closure(
        &mut self,
        func: &StmtFunctionDef,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        if func.is_async {
            return Err(SmeltError::unsupported(
                self.span(func.range),
                "async nested closures need async closure-body lowering",
            ));
        }
        let return_ty = func
            .returns
            .as_deref()
            .ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(func.range),
                    "nested closure return type must be explicit",
                )
            })
            .and_then(|annotation| self.annotation_to_hir(annotation))?;
        let mut params = Vec::new();
        let mut callback_params = HashMap::new();
        let mut defaults = Vec::new();
        for (index, param_with_default) in func.parameters.iter_non_variadic_params().enumerate() {
            let param = &param_with_default.parameter;
            let param_ty = param
                .annotation
                .as_deref()
                .ok_or_else(|| {
                    SmeltError::type_constraint(
                        self.span(param.range),
                        "nested closure parameters must have explicit type annotations",
                    )
                })
                .and_then(|annotation| self.annotation_to_hir(annotation))?;
            params.push(param_ty);
            callback_params.insert(
                param.name.as_str(),
                CallbackExpr {
                    kind: CallbackExprKind::Param(index),
                    ty: param_ty,
                },
            );
            defaults.push(
                param_with_default
                    .default
                    .as_ref()
                    .map(|default| self.expression(default, body))
                    .transpose()?,
            );
        }
        let vararg = if let Some(vararg) = &func.parameters.vararg {
            let item_ty = vararg
                .annotation
                .as_deref()
                .ok_or_else(|| {
                    SmeltError::type_constraint(
                        self.span(vararg.range),
                        "*args nested closure parameters must have explicit type annotations",
                    )
                })
                .and_then(|annotation| self.annotation_to_hir(annotation))?;
            let list_ty = self.intern_type(Type::List(item_ty));
            let index = params.len();
            params.push(list_ty);
            defaults.push(None);
            callback_params.insert(
                vararg.name.as_str(),
                CallbackExpr {
                    kind: CallbackExprKind::Param(index),
                    ty: list_ty,
                },
            );
            Some(VarArgParam { index, item_ty })
        } else {
            None
        };
        let kwarg = if let Some(kwarg) = &func.parameters.kwarg {
            let value_ty = kwarg
                .annotation
                .as_deref()
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(kwarg.range),
                        "**kwargs nested closure parameters must have explicit value type annotations",
                    )
                })
                .and_then(|annotation| self.annotation_to_hir(annotation))?;
            let string_ty = self.intern_type(Type::String);
            let dict_ty = self.intern_type(Type::Dict(string_ty, value_ty));
            let index = params.len();
            params.push(dict_ty);
            defaults.push(None);
            callback_params.insert(
                kwarg.name.as_str(),
                CallbackExpr {
                    kind: CallbackExprKind::Param(index),
                    ty: dict_ty,
                },
            );
            Some(KwArgParam { index, value_ty })
        } else {
            None
        };
        let [Stmt::Return(return_stmt)] = func.body.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(func.range),
                "nested closure bodies need a single return expression",
            ));
        };
        let return_expr = return_stmt.value.as_deref().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(return_stmt.range),
                "nested closure return statements must return a value",
            )
        })?;
        let callback = self.python_callback_expr(return_expr, &callback_params, body)?;
        if callback.ty != return_ty {
            return Err(SmeltError::unsupported(
                self.span(return_stmt.range),
                "nested closure return type does not match its annotation",
            ));
        }
        let name_text = func.name.as_str();
        let name = self.intern_name(name_text);
        let ty = self.intern_type(Type::Function(FunctionType {
            params: params.clone(),
            rest: None,
            required_params: None,
                    mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        }));
        let local = body.push_local(LocalDecl {
            name: Some(name),
            ty,
            mutable: true,
            span: self.span(func.range),
        });
        self.locals.insert(name_text.to_owned(), local);
        self.local_callbacks.insert(
            name_text.to_owned(),
            LocalCallback {
                callback,
                params,
                defaults,
                vararg,
                kwarg,
                return_ty,
            },
        );
        Ok(())
    }

    /// Return call-packing metadata for lambda `*args` when Callable uses one list.
    fn lambda_vararg_metadata(
        lambda: &ruff_python_ast::ExprLambda,
        params: &[TypeId],
        types: &smelt_hir::TypeInterner,
    ) -> Option<VarArgParam> {
        let lambda_params = lambda.parameters.as_ref()?;
        lambda_params.vararg.as_ref()?;
        let [param_ty] = params else {
            return None;
        };
        let Some(Type::List(item_ty)) = types.get(*param_ty) else {
            return None;
        };
        Some(VarArgParam {
            index: 0,
            item_ty: *item_ty,
        })
    }

    /// Return call-packing metadata for lambda `**kwargs` when Callable uses one dict.
    fn lambda_kwarg_metadata(
        lambda: &ruff_python_ast::ExprLambda,
        params: &[TypeId],
        types: &smelt_hir::TypeInterner,
    ) -> Option<KwArgParam> {
        let lambda_params = lambda.parameters.as_ref()?;
        lambda_params.kwarg.as_ref()?;
        let [param_ty] = params else {
            return None;
        };
        let Some(Type::Dict(_, value_ty)) = types.get(*param_ty) else {
            return None;
        };
        Some(KwArgParam {
            index: 0,
            value_ty: *value_ty,
        })
    }

    /// Lower a Python assert statement to a conditional failure path.
    fn assert_statement(
        &mut self,
        assert_stmt: &StmtAssert,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let cond = self.expression(&assert_stmt.test, body)?;
        let bool_ty = self.intern_type(Type::Bool);
        let negated = body.push_expr(HirExpr {
            kind: ExprKind::UnaryOp {
                op: UnaryOp::Not,
                operand: cond,
            },
            ty: bool_ty,
            span: self.span(assert_stmt.range),
        });
        let failure_block = body.push_block(self.span(assert_stmt.range));
        let message = assert_stmt
            .msg
            .as_deref()
            .map(|expr| self.expression(expr, body))
            .transpose()?
            .unwrap_or_else(|| {
                self.string_literal_expr("assertion failed", assert_stmt.range, body)
            });
        body.push_stmt_to_block(failure_block, HirStmt::Throw(message));
        body.push_stmt_to_block(
            block,
            HirStmt::If {
                cond: negated,
                then_block: failure_block,
                else_block: None,
            },
        );
        Ok(())
    }

    /// Return whether a `with` statement targets the pytest.raises special form.
    fn with_is_pytest_raises(with_stmt: &StmtWith) -> bool {
        let [item] = with_stmt.items.as_slice() else {
            return false;
        };
        Self::pytest_raises_call(&item.context_expr).is_some()
    }

    /// Lower a statically-known Python context manager protocol use.
    ///
    /// This supports the Rich-style shape `with value as name:` by emitting a
    /// direct `__enter__` method call, lowering the lexical body, then emitting
    /// a direct `__exit__` method call.  Exception suppression is intentionally
    /// not modelled in this slice.
    fn context_manager_with_statement(
        &mut self,
        with_stmt: &StmtWith,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        if with_stmt.is_async {
            return Err(SmeltError::unsupported(
                self.span(with_stmt.range),
                "async context managers are not supported",
            ));
        }
        let [item] = with_stmt.items.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(with_stmt.range),
                "context manager lowering supports exactly one context manager",
            ));
        };
        let manager = self.expression(&item.context_expr, body)?;
        let enter = self.protocol_method_expr(
            ProtocolMethodCall {
                receiver: manager,
                method: "__enter__".to_owned(),
                args: Vec::new(),
            },
            body,
        )?;
        if let Some(vars) = &item.optional_vars {
            let enter_ty = Self::expr_ty(body, enter);
            let pat = self.binding_pattern_from_target(vars, body, Some(enter_ty))?;
            body.push_stmt_to_block(
                block,
                HirStmt::Let {
                    pat,
                    ty: enter_ty,
                    value: Some(enter),
                },
            );
        } else {
            body.push_stmt_to_block(block, HirStmt::Expr(enter));
        }
        for stmt in &with_stmt.body {
            self.statement_in_block(stmt, body, block)?;
        }
        let exit = self.protocol_method_expr(
            ProtocolMethodCall {
                receiver: manager,
                method: "__exit__".to_owned(),
                args: Vec::new(),
            },
            body,
        )?;
        body.push_stmt_to_block(block, HirStmt::Expr(exit));
        Ok(())
    }

    /// Lower `with pytest.raises(...): ...` to native try/catch assertion flow.
    fn pytest_raises_with_statement(
        &mut self,
        with_stmt: &StmtWith,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        if with_stmt.is_async {
            return Err(SmeltError::unsupported(
                self.span(with_stmt.range),
                "async pytest.raises context managers are not supported",
            ));
        }
        let [item] = with_stmt.items.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(with_stmt.range),
                "pytest.raises lowering supports exactly one context manager",
            ));
        };
        let Some(raises_call) = Self::pytest_raises_call(&item.context_expr) else {
            return Err(SmeltError::unsupported(
                self.span(item.range),
                "only pytest.raises context managers are supported",
            ));
        };
        if raises_call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(raises_call.range),
                "pytest.raises requires an expected exception type",
            ));
        }
        let match_expr = self.pytest_raises_match_argument(raises_call, body)?;
        let explicit_catch_binding = item
            .optional_vars
            .as_ref()
            .map(|vars| self.pytest_raises_context_binding(vars, body))
            .transpose()?;
        let catch_binding = explicit_catch_binding.or_else(|| {
            match_expr
                .is_some()
                .then(|| self.pytest_raises_hidden_exception_local(item.range, body))
        });

        let bool_ty = self.intern_type(Type::Bool);
        let raised_sym = self.intern_name("__smelt_pytest_raised");
        let raised_local = body.push_local(LocalDecl {
            name: Some(raised_sym),
            ty: bool_ty,
            mutable: true,
            span: self.span(with_stmt.range),
        });
        let false_expr = body.push_expr(HirExpr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span: self.span(with_stmt.range),
        });
        let raised_pat = body.push_pattern(HirPattern::Binding(raised_local));
        body.push_stmt_to_block(
            block,
            HirStmt::Let {
                pat: raised_pat,
                ty: bool_ty,
                value: Some(false_expr),
            },
        );

        let try_body = self.block_from_stmts(&with_stmt.body, body)?;
        let catch_body = body.push_block(self.span(item.range));
        let raised_target = body.push_expr(HirExpr {
            kind: ExprKind::Local(raised_local),
            ty: bool_ty,
            span: self.span(item.range),
        });
        let true_expr = body.push_expr(HirExpr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span: self.span(item.range),
        });
        body.push_stmt_to_block(
            catch_body,
            HirStmt::Assign {
                target: raised_target,
                value: true_expr,
            },
        );
        if let (Some(exception_local), Some(pattern)) = (catch_binding, match_expr) {
            self.push_pytest_raises_match_assert(
                (exception_local, pattern),
                item.range,
                body,
                catch_body,
            );
        }
        body.push_stmt_to_block(
            block,
            HirStmt::TryCatch {
                body: try_body,
                catch_binding,
                catch_body: Some(catch_body),
                finally_body: None,
            },
        );

        let raised_check = body.push_expr(HirExpr {
            kind: ExprKind::Local(raised_local),
            ty: bool_ty,
            span: self.span(with_stmt.range),
        });
        let missing_raise = body.push_expr(HirExpr {
            kind: ExprKind::UnaryOp {
                op: UnaryOp::Not,
                operand: raised_check,
            },
            ty: bool_ty,
            span: self.span(with_stmt.range),
        });
        let failure_block = body.push_block(self.span(with_stmt.range));
        let message =
            self.string_literal_expr("pytest.raises(...) did not raise", with_stmt.range, body);
        body.push_stmt_to_block(failure_block, HirStmt::Throw(message));
        body.push_stmt_to_block(
            block,
            HirStmt::If {
                cond: missing_raise,
                then_block: failure_block,
                else_block: None,
            },
        );
        Ok(())
    }

    /// Lower a `with pytest.raises(...) as excinfo` binding to a catch local.
    fn pytest_raises_context_binding(
        &mut self,
        vars: &Expr,
        body: &mut Body,
    ) -> Result<smelt_hir::LocalId, SmeltError> {
        let Expr::Name(name) = vars else {
            return Err(SmeltError::unsupported(
                self.span(vars.range()),
                "pytest.raises context variables must be simple names",
            ));
        };
        let ty = self.intern_type(Type::String);
        let symbol = self.intern_name(name.id.as_str());
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span: self.span(name.range),
        });
        self.locals.insert(name.id.to_string(), local);
        Ok(local)
    }

    // Pytest callable-form raises lowering continues in `control_flow.rs`.
}
