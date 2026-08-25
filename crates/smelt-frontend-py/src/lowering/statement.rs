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
                if let Expr::Call(call) = s.value.as_ref()
                    && self.super_init_statement(call, body, block)?
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

            // `del target[, target]...` — currently only `del dict[key]` is lowered.
            Stmt::Delete(delete_stmt) => self.delete_statement(delete_stmt, body, block),

            // `try: … except …: … else: … finally: …`
            Stmt::Try(try_stmt) => self.try_statement(try_stmt, body, block),

            Stmt::TypeAlias(_)
            | Stmt::Global(_)
            | Stmt::Nonlocal(_)
            | Stmt::IpyEscapeCommand(_) => Err(SmeltError::unsupported(
                self.span(stmt.range()),
                format!("unsupported statement: {}", stmt_kind_name(stmt)),
            )),
        }
    }

    /// Lower a Python `try/except/finally` statement to [`HirStmt::TryCatch`].
    ///
    /// The mapping onto Smelt's error model (a thrown value is a string message,
    /// mirroring the pytest.raises lowering and the TypeScript `try`/`catch`
    /// lowering) is:
    ///
    /// * The `try:` suite becomes the protected `body` block. A trailing
    ///   `else:` suite (which Python runs only when the `try` body did not
    ///   raise) is appended to the end of that same block — semantically the
    ///   no-exception continuation.
    /// * A single `except [E [as name]]:` handler becomes the `catch_body`
    ///   block. When the handler binds `as name`, a fresh string local named
    ///   `name` is introduced as the `catch_binding` so the handler body can
    ///   reference the caught message.
    /// * A `finally:` suite becomes the `finally_body` block.
    ///
    /// Smelt's HIR models a single catch handler, so shapes that need
    /// per-exception-type dispatch are rejected with a specific message rather
    /// than silently collapsing distinct handlers: multiple `except` clauses and
    /// `except*` (exception groups) remain explicit deferrals. Filtering on the
    /// caught exception *type* is likewise not modelled — every handler catches
    /// all thrown values — so the exception type expression is accepted but does
    /// not narrow the catch, matching how the rest of the error model treats
    /// thrown values as opaque strings.
    fn try_statement(
        &mut self,
        try_stmt: &StmtTry,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        if try_stmt.is_star {
            return Err(SmeltError::unsupported(
                self.span(try_stmt.range),
                "`except*` exception groups are not supported",
            ));
        }
        if try_stmt.handlers.len() > 1 {
            return Err(SmeltError::unsupported(
                self.span(try_stmt.range),
                "multiple `except` clauses are not supported; Smelt models a single catch handler",
            ));
        }

        // The protected block holds the `try:` suite followed by any `else:`
        // suite (the else suite only runs when the try suite did not raise, so
        // appending it to the same block keeps that ordering).
        let try_block = self.block_from_stmts(&try_stmt.body, body)?;
        for stmt in &try_stmt.orelse {
            self.statement_in_block(stmt, body, try_block)?;
        }

        // At most one handler reaches here (the multi-handler case is rejected
        // above), so lowering the first handler covers every accepted shape.
        let (catch_binding, catch_body) = match try_stmt.handlers.first() {
            None => (None, None),
            Some(ExceptHandler::ExceptHandler(handler)) => {
                // Bindings and body locals introduced by the handler must not
                // leak into the surrounding scope, matching the TypeScript
                // `catch` lowering.
                let previous_locals = self.locals.clone();
                let catch_binding = handler
                    .name
                    .as_ref()
                    .map(|name| self.except_binding(name.as_str(), name.range, body));
                let catch_block = self.block_from_stmts(&handler.body, body)?;
                self.locals = previous_locals;
                (catch_binding, Some(catch_block))
            }
        };

        let finally_body = if try_stmt.finalbody.is_empty() {
            None
        } else {
            Some(self.block_from_stmts(&try_stmt.finalbody, body)?)
        };

        body.push_stmt_to_block(
            block,
            HirStmt::TryCatch {
                body: try_block,
                catch_binding,
                catch_body,
                finally_body,
            },
        );
        Ok(())
    }

    /// Introduce the `except … as name` binding as a fresh string local.
    ///
    /// Smelt's error model represents a thrown value as a string message (see
    /// the pytest.raises lowering), so the caught exception name is bound to a
    /// `String` local and registered in the local scope for the handler body.
    fn except_binding(&mut self, name: &str, range: TextRange, body: &mut Body) -> smelt_hir::LocalId {
        let ty = self.intern_type(Type::String);
        let symbol = self.intern_name(name);
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span: self.span(range),
        });
        self.locals.insert(name.to_owned(), local);
        local
    }

    /// Lower a Python `del target[, target]...` statement.
    ///
    /// Only subscript deletion against a statically-known `dict` receiver is
    /// supported, lowering `del d[key]` to a [`ExprKind::DictRemoveKey`] whose
    /// (unused) `bool` result is discarded through an expression statement —
    /// the same HIR shape TypeScript's `Map.delete(key)` produces. Other forms
    /// (`del name`, `del list[i]`, `del obj.attr`, slice deletion) have no
    /// general HIR lowering yet and are rejected with a specific message so the
    /// blocker class stays narrow.
    fn delete_statement(
        &mut self,
        delete_stmt: &ruff_python_ast::StmtDelete,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        for target in &delete_stmt.targets {
            let Expr::Subscript(sub) = target else {
                return Err(SmeltError::unsupported(
                    self.span(target.range()),
                    "only `del dict[key]` deletion is supported",
                ));
            };
            if matches!(sub.slice.as_ref(), Expr::Slice(_)) {
                return Err(SmeltError::unsupported(
                    self.span(sub.range),
                    "slice deletion (`del seq[a:b]`) is not supported",
                ));
            }
            let dict = self.expression(&sub.value, body)?;
            let dict_ty = Self::expr_ty(body, dict);
            let Some(&Type::Dict(key_ty, _)) = self.ctx.krate.types.get(dict_ty) else {
                return Err(SmeltError::unsupported(
                    self.span(sub.range),
                    "`del receiver[key]` requires a dict receiver",
                ));
            };
            let key = self.expression(&sub.slice, body)?;
            if Self::expr_ty(body, key) != key_ty {
                return Err(SmeltError::unsupported(
                    self.span(sub.range),
                    "`del dict[key]` key must match the dict key type",
                ));
            }
            let bool_ty = self.intern_type(Type::Bool);
            let remove = body.push_expr(HirExpr {
                kind: ExprKind::DictRemoveKey { dict, key },
                ty: bool_ty,
                span: self.span(sub.range),
            });
            body.push_stmt_to_block(block, HirStmt::Expr(remove));
        }
        Ok(())
    }

    /// `x: T [= value]` → `Stmt::Let { pat, ty, value }`.
    ///
    /// An *attribute* target (`self.field: T = value`) is not a new binding —
    /// the field is already declared, either by a class-level annotation or by
    /// this very statement via
    /// [`Self::implicit_constructor_fields`](crate::lowering::ModuleBuilder::implicit_constructor_fields).
    /// It therefore lowers as an ordinary assignment to the field place, with
    /// the annotation supplying the expected type for the value.
    fn ann_assign(
        &mut self,
        ann: &StmtAnnAssign,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        if matches!(ann.target.as_ref(), Expr::Attribute(_)) {
            let ty = self.annotation_to_hir(&ann.annotation)?;
            let target = self.expression(&ann.target, body)?;
            let Some(value) = ann.value.as_deref() else {
                return Err(SmeltError::unsupported(
                    self.span(ann.range()),
                    "an annotated attribute declaration requires a value",
                ));
            };
            let value = self.expression_with_hint(value, body, Some(ty))?;
            body.push_stmt_to_block(block, HirStmt::Assign { target, value });
            return Ok(());
        }
        let Expr::Name(target_name) = ann.target.as_ref() else {
            return Err(SmeltError::unsupported(
                self.span(ann.range()),
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

    /// Lower `super().__init__(args)` inside a derived class's `__init__`.
    ///
    /// Rust has no class inheritance, so Smelt flattens a derived class's struct
    /// to carry its base's fields ahead of its own (`effective_class_fields` in
    /// `smelt-codegen-rust`). The call therefore has no callee to defer to:
    /// nothing runs the base's initialization unless it is emitted here against
    /// the derived `self`.
    ///
    /// The lowering matches the TypeScript frontend's `super(...)` handling
    /// (`decls/super_call.rs`) and emits only ordinary HIR — no dedicated node:
    ///
    /// ```text
    /// let __smelt_super: Base = Base(args);   // the base's own constructor
    /// self.<field> = __smelt_super.<field>;   // for each inherited field
    /// ```
    ///
    /// Because the base is built through its *own* constructor, everything that
    /// constructor does runs exactly once and in order — including its own
    /// `super().__init__(..)`. Multi-level inheritance therefore needs no
    /// special handling: each level only ever reproduces its immediate base, and
    /// the flattened layouts agree because a base struct's fields are a prefix
    /// of the derived struct's.
    ///
    /// Returns `false` when the statement is not a `super().__init__(..)` call,
    /// leaving it to the ordinary expression path.
    fn super_init_statement(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Some(method) = Self::super_receiver_method(call) else {
            return Ok(false);
        };
        let span = self.span(call.range());
        if method != "__init__" {
            // A `super().m(..)` call on an ordinary method has no place to
            // dispatch under flattening: an override replaces the inherited slot
            // in the derived impl, so the base body is simply not present to
            // call. Reject it explicitly rather than silently dispatching back
            // to the override, which would recurse forever.
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "super().{method}() is not supported yet; only super().__init__() is lowered"
                ),
            ));
        }

        // The enclosing class comes from the `self` receiver's type, which is
        // the owning class for every method body.
        let Some(&self_local) = self.locals.get("self") else {
            return Err(SmeltError::unsupported(
                span,
                "super().__init__() requires a `self` receiver",
            ));
        };
        let class_ty = Self::local_ty(body, self_local);
        let Some(Type::Class { name: class_sym, .. }) =
            self.ctx.krate.types.get(class_ty).cloned()
        else {
            return Err(SmeltError::unsupported(
                span,
                "super().__init__() requires a class receiver",
            ));
        };
        let Some(base_sym) = self.class_base_symbol(class_sym) else {
            return Err(SmeltError::unsupported(
                span,
                "super().__init__() requires the enclosing class to declare a base class",
            ));
        };
        let Some(base_name) = self.ctx.krate.symbols.get(base_sym).map(ToOwned::to_owned) else {
            return Err(SmeltError::unsupported(
                span,
                "super().__init__() base class name is not resolvable",
            ));
        };

        let args = call
            .arguments
            .args
            .iter()
            .map(|arg| self.expression(arg, body))
            .collect::<Result<Vec<_>, _>>()?;

        let base_ty = self.intern_type(Type::Class {
            name: base_sym,
            args: vec![],
        });
        let constructed = body.push_expr(HirExpr {
            kind: ExprKind::New {
                class: base_sym,
                args,
            },
            ty: base_ty,
            span,
        });
        let base_local_name = self.intern_name("__smelt_super");
        let base_local = body.push_local(LocalDecl {
            name: Some(base_local_name),
            ty: base_ty,
            mutable: false,
            span,
        });
        let pat = body.push_pattern(HirPattern::Binding(base_local));
        body.push_stmt_to_block(
            block,
            HirStmt::Let {
                pat,
                ty: base_ty,
                value: Some(constructed),
            },
        );

        for field in self.inherited_base_fields(&base_name) {
            let receiver = body.push_expr(HirExpr {
                kind: ExprKind::Local(self_local),
                ty: class_ty,
                span,
            });
            let target = body.push_expr(HirExpr {
                kind: ExprKind::Field {
                    receiver,
                    field: field.name,
                },
                ty: field.ty,
                span,
            });
            let source = body.push_expr(HirExpr {
                kind: ExprKind::Local(base_local),
                ty: base_ty,
                span,
            });
            let value = body.push_expr(HirExpr {
                kind: ExprKind::Field {
                    receiver: source,
                    field: field.name,
                },
                ty: field.ty,
                span,
            });
            body.push_stmt_to_block(block, HirStmt::Assign { target, value });
        }
        Ok(true)
    }

    /// Read a call whose callee is `super().<method>`, returning the method name.
    ///
    /// Only the zero-argument `super()` spelling is recognized: the explicit
    /// two-argument `super(Class, self)` form selects a different MRO entry and
    /// is not modelled.
    fn super_receiver_method(call: &ruff_python_ast::ExprCall) -> Option<&str> {
        let Expr::Attribute(attribute) = call.func.as_ref() else {
            return None;
        };
        let Expr::Call(receiver) = attribute.value.as_ref() else {
            return None;
        };
        let Expr::Name(callee) = receiver.func.as_ref() else {
            return None;
        };
        if callee.id.as_str() != "super"
            || !receiver.arguments.args.is_empty()
            || !receiver.arguments.keywords.is_empty()
        {
            return None;
        }
        Some(attribute.attr.as_str())
    }

    /// The base-class symbol declared by a lowered class, if it has one.
    fn class_base_symbol(&self, class_sym: Symbol) -> Option<Symbol> {
        self.ctx.krate.items.iter().find_map(|item| match item {
            Item::Class(class) if class.name == class_sym => class.base,
            _ => None,
        })
    }

    /// The instance fields a derived class inherits, walking the base chain so
    /// a multi-level base contributes its own inherited slots too.
    ///
    /// Ordering matches the flattened struct layout (`effective_class_fields`):
    /// base-most fields first. A name redeclared further down the chain keeps
    /// the nearest declaration, mirroring how the layout replaces the inherited
    /// slot.
    fn inherited_base_fields(&self, base_name: &str) -> Vec<Field> {
        let mut chain: Vec<Vec<Field>> = Vec::new();
        let mut visited: Vec<String> = Vec::new();
        let mut cursor = Some(base_name.to_owned());
        while let Some(name) = cursor {
            if visited.contains(&name) {
                break;
            }
            visited.push(name.clone());
            let Some(class) = self.ctx.krate.items.iter().find_map(|item| match item {
                Item::Class(class)
                    if self.ctx.krate.symbols.get(class.name) == Some(name.as_str()) =>
                {
                    Some(class)
                }
                _ => None,
            }) else {
                break;
            };
            chain.push(class.fields.clone());
            cursor = class
                .base
                .and_then(|base| self.ctx.krate.symbols.get(base))
                .map(ToOwned::to_owned);
        }
        // `chain` runs derived-to-base; the layout is base-first.
        let mut fields: Vec<Field> = Vec::new();
        for level in chain.into_iter().rev() {
            for field in level {
                match fields
                    .iter_mut()
                    .find(|existing| existing.name == field.name)
                {
                    Some(existing) => *existing = field,
                    None => fields.push(field),
                }
            }
        }
        fields
    }

    /// Lower a nested Python function definition as a local closure value.
    ///
    /// # Return type
    ///
    /// The closure's return type is *not* required to be annotated. A nested
    /// closure body is a single `return <expr>`, so the accurate return type is
    /// the HIR type the frontend already computes for that expression while
    /// lowering it into a [`CallbackExpr`]. The body is therefore lowered first
    /// and its type used directly when the source omits `-> T` (issue #93:
    /// idiomatic Python omits annotations, and the actual returned type is the
    /// one lowering must follow). A declared `-> T` still wins when present and
    /// is checked against the lowered body, so an explicit source contract is
    /// never silently widened.
    ///
    /// # Parameter types
    ///
    /// Parameter types must be known *before* the body is lowered (they seed the
    /// callback parameter environment), so they come from the annotation when
    /// present and otherwise from `ty`'s resolved type for that parameter node.
    /// A parameter `ty` cannot resolve stays an explicit error rather than being
    /// routed through an erased ABI.
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
        let declared_return_ty = func
            .returns
            .as_deref()
            .map(|annotation| self.annotation_to_hir(annotation))
            .transpose()?;
        let mut params = Vec::new();
        let mut callback_params = HashMap::new();
        let mut defaults = Vec::new();
        for (index, param_with_default) in func.parameters.iter_non_variadic_params().enumerate() {
            let param = &param_with_default.parameter;
            let param_ty = match param.annotation.as_deref() {
                Some(annotation) => self.annotation_to_hir(annotation)?,
                // No annotation: consult `ty` (issue #93) before erroring.
                None => self.resolved_param_ty(param).ok_or_else(|| {
                    SmeltError::type_constraint(
                        self.span(param.range),
                        "nested closure parameters must have explicit type annotations",
                    )
                })?,
            };
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
        // With no declared `-> T`, the lowered body's own type *is* the closure's
        // return type; with one, the declaration is the contract and the body
        // must satisfy it exactly.
        let return_ty = match declared_return_ty {
            None => callback.ty,
            Some(declared) => {
                if callback.ty != declared {
                    return Err(SmeltError::unsupported(
                        self.span(return_stmt.range),
                        "nested closure return type does not match its annotation",
                    ));
                }
                declared
            }
        };
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
                self.span(raises_call.range()),
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
