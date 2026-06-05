impl ModuleBuilder<'_> {
    /// Lower a `const name = (...) => ...` declaration into a HIR function item.
    fn arrow_function_const_declaration(
        &mut self,
        name_text: &str,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        self.arrow_function_const_declaration_inner(name_text, arrow, type_hint, false, false)
    }

    /// Lower a module-private arrow constant with a crate-unique internal function symbol.
    ///
    /// Multiple TypeScript modules legitimately use helper spellings such as
    /// `lazyImplementation`; their Rust items must remain distinct after all
    /// source modules are emitted into one generated crate.
    fn private_arrow_function_const_declaration(
        &mut self,
        name_text: &str,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        self.arrow_function_const_declaration_inner(name_text, arrow, type_hint, false, true)
    }

    /// Lower a synthetic object-table arrow function while preserving exact key spelling.
    fn arrow_function_const_declaration_with_source_name(
        &mut self,
        name_text: &str,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        self.arrow_function_const_declaration_inner(name_text, arrow, type_hint, true, false)
    }

    /// Shared implementation for arrow function constants.
    fn arrow_function_const_declaration_inner(
        &mut self,
        name_text: &str,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        preserve_source_name: bool,
        qualify_private_name: bool,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let function_hint = self.contextual_function_type(type_hint);
        let _type_params = self.push_type_parameter_scope(arrow.type_parameters.as_deref())?;
        let explicit_return_ty = match arrow
            .return_type
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()
        {
            Ok(value) => value,
            Err(error) => {
                self.pop_type_parameter_scope();
                return Err(error);
            }
        };
        let contextual_return_ty = function_hint.as_ref().map(|function| function.return_ty);
        let declared_return_ty = if arrow.r#async {
            explicit_return_ty.map_or_else(
                || {
                    contextual_return_ty.filter(|ty| {
                    matches!(self.ctx.krate.types.get(*ty), Some(Type::Future(_)))
                    })
                },
                Some,
            )
        } else {
            explicit_return_ty.or(contextual_return_ty)
        };
        if arrow.r#async
            && explicit_return_ty.is_some()
            && !matches!(
                explicit_return_ty.and_then(|ty| self.ctx.krate.types.get(ty)),
                Some(Type::Future(_))
            )
        {
            self.pop_type_parameter_scope();
            return Err(SmeltError::unsupported(
                self.span(arrow.span.start, arrow.span.end),
                "async arrow function constants must declare a Promise<T> return type",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_narrowed_locals = std::mem::take(&mut self.narrowed_locals);
        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        self.current_async = arrow.r#async;
        self.current_return_ty = declared_return_ty;
        let mut body = Body::new(None, self.span(arrow.body.span.start, arrow.body.span.end));
        let mut params = Vec::new();
        for (param_index, param) in arrow.params.items.iter().enumerate() {
            let param_annotation_ty = match param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()
            {
                Ok(value) => value,
                Err(error) => {
                    self.locals = saved_locals;
                    self.narrowed_locals = saved_narrowed_locals;
                    self.current_async = saved_async;
                    self.current_return_ty = saved_return_ty;
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
            };
            let mut ty = param_annotation_ty
                .or_else(|| {
                    function_hint
                        .as_ref()
                        .and_then(|function| function.params.get(params.len()).copied())
                })
                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
            if param.optional && !matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
                ty = self.ctx.krate.types.intern(Type::Optional(ty));
            }
            let (param_name, param_span) = match &param.pattern {
                BindingPattern::BindingIdentifier(binding) => (
                    self.intern_source_name(binding.name.as_str()),
                    self.span(binding.span.start, binding.span.end),
                ),
                _ => (
                    self.synthetic_param_symbol(param_index),
                    self.span(param.span.start, param.span.end),
                ),
            };
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span: param_span,
            });
            body.params.push(local);
            params.push(Param {
                name: param_name,
                local,
                ty,
                span: param_span,
            });
            match &param.pattern {
                BindingPattern::BindingIdentifier(binding) => {
                    self.locals.insert(binding.name.to_string(), local);
                }
                pattern => {
                    let value = body.push_expr(Expr {
                        kind: ExprKind::Local(local),
                        ty,
                        span: param_span,
                    });
                    let root = body.root;
                    self.binding_declaration(
                        pattern,
                        Some(value),
                        Some(ty),
                        false,
                        &mut body,
                        root,
                    )?;
                }
            }
        }
        let rest = if let Some(rest) = &arrow.params.rest {
            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                self.locals = saved_locals;
                self.narrowed_locals = saved_narrowed_locals;
                self.current_async = saved_async;
                self.current_return_ty = saved_return_ty;
                self.pop_type_parameter_scope();
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest arrow function parameters need rest binding lowering",
                ));
            };
            let Some(annotation) = &rest.type_annotation else {
                self.locals = saved_locals;
                self.narrowed_locals = saved_narrowed_locals;
                self.current_async = saved_async;
                self.current_return_ty = saved_return_ty;
                self.pop_type_parameter_scope();
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest arrow function parameters must have explicit array type annotations",
                ));
            };
            let ty = match self.ts_type_to_hir(&annotation.type_annotation) {
                Ok(ty) => ty,
                Err(error) => {
                self.locals = saved_locals;
                self.narrowed_locals = saved_narrowed_locals;
                self.current_async = saved_async;
                self.current_return_ty = saved_return_ty;
                self.pop_type_parameter_scope();
                return Err(error);
                }
            };
            let Ok((ty, item_ty)) = self.rest_param_array_type(ty) else {
                self.locals = saved_locals;
                self.narrowed_locals = saved_narrowed_locals;
                self.current_async = saved_async;
                self.current_return_ty = saved_return_ty;
                self.pop_type_parameter_scope();
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest arrow function parameter type must be an array type",
                ));
            };
            let param_name = self.intern_source_name(binding.name.as_str());
            let span = self.span(binding.span.start, binding.span.end);
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span,
            });
            body.params.push(local);
            self.locals.insert(binding.name.to_string(), local);
            let index = params.len();
            params.push(Param {
                name: param_name,
                local,
                ty,
                span,
            });
            Some(RestParam { index, item_ty })
        } else {
            None
        };

        let mut errors = Vec::new();
        let mut inferred_return_ty = None;
        if arrow.expression {
            match arrow.body.statements.as_slice() {
                [Statement::ExpressionStatement(statement)] => {
                    match self.expression_with_hint(
                        &statement.expression,
                        &mut body,
                        declared_return_ty,
                    ) {
                        Ok(value) => {
                            inferred_return_ty = Some(Self::expr_ty(&body, value));
                            body.push_stmt(Stmt::Return(Some(value)));
                        }
                        Err(error) => errors.push(error),
                    }
                }
                _ => errors.push(SmeltError::unsupported(
                    self.span(arrow.body.span.start, arrow.body.span.end),
                    "expression-bodied arrow functions must contain one expression",
                )),
            }
        } else {
            if let Err(error) =
                self.predeclare_local_function_declarations(&arrow.body.statements, &mut body)
            {
                errors.push(error);
            }
            if let Err(error) =
                self.predeclare_local_arrow_callbacks(&arrow.body.statements, &mut body)
            {
                errors.push(error);
            }
            for statement in &arrow.body.statements {
                if let Err(error) = self.statement(statement, &mut body) {
                    errors.push(error);
                }
            }
            inferred_return_ty = self.last_return_type(&body);
        }
        if arrow.r#async {
            body.build_async_state_machine();
        }
        self.locals = saved_locals;
        self.narrowed_locals = saved_narrowed_locals;
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        if let Some(error) = errors.into_iter().next() {
            self.pop_type_parameter_scope();
            return Err(error);
        }
        let inferred_return_ty = inferred_return_ty.or_else(|| {
            (arrow.r#async || !arrow.expression).then(|| self.ctx.krate.types.intern(Type::None))
        });
        let return_ty = if arrow.r#async {
            declared_return_ty.or_else(|| {
                inferred_return_ty.map(|inner| self.ctx.krate.types.intern(Type::Future(inner)))
            })
        } else {
            self.arrow_const_return_type(declared_return_ty, inferred_return_ty)
        }
            .ok_or_else(|| {
                self.pop_type_parameter_scope();
                SmeltError::unsupported(
                    self.span(arrow.span.start, arrow.span.end),
                    "block-bodied arrow function constants must have an explicit return type",
                )
            })?;
        self.pop_type_parameter_scope();

        let body_id = self.ctx.krate.push_body(body);
        let name = if preserve_source_name {
            self.intern_exact_source_name(name_text)
        } else if qualify_private_name {
            self.intern_source_name(&format!("{name_text}__module_{}", self.path))
        } else {
            self.intern_source_name(name_text)
        };
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name,
            span: self.span(arrow.span.start, arrow.span.end),
            params,
            rest: rest.map(|rest| rest.index),
            required_params: Some(
                arrow
                    .params
                    .items
                    .iter()
                    .position(|param| {
                        param.optional || Self::formal_parameter_has_default(param)
                    })
                    .unwrap_or(arrow.params.items.len()),
            ),
return_ty,
            is_async: arrow.r#async,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        self.items.insert(name_text.to_owned(), item);
        if let Some(rest) = rest {
            self.function_rests.insert(name_text.to_owned(), rest);
            self.ctx.function_rests.insert(name_text.to_owned(), rest);
        }
        Ok(item)
    }

    /// Pick the public return type for a lowered arrow-const function item.
    fn arrow_const_return_type(
        &mut self,
        declared_return_ty: Option<smelt_hir::TypeId>,
        inferred_return_ty: Option<smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
        match (declared_return_ty, inferred_return_ty) {
            (Some(declared), Some(inferred))
                if matches!(self.ctx.krate.types.get(declared), Some(Type::Class { .. }))
                    && matches!(self.ctx.krate.types.get(inferred), Some(Type::Function(_))) =>
            {
                Some(self.erase_opaque_callable_return(inferred))
            }
            (Some(declared), _) => Some(declared),
            (None, inferred) => inferred,
        }
    }

    /// Preserve callable parameters from an erased API surface while dropping precise return data.
    fn erase_opaque_callable_return(&mut self, inferred: smelt_hir::TypeId) -> smelt_hir::TypeId {
        let Some(Type::Function(function)) = self.ctx.krate.types.get(inferred).cloned() else {
            return inferred;
        };
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let return_ty = match self.ctx.krate.types.get(function.return_ty) {
            Some(Type::Future(_)) => self.ctx.krate.types.intern(Type::Future(unknown_ty)),
            _ => unknown_ty,
        };
        self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: function.params,
            rest: function.rest,
            required_params: None,
                    mutable_params: Vec::new(),
return_ty,
            is_async: function.is_async,
                            may_throw: false,
        }))
    }

    /// Create a stable internal symbol for a destructured parameter slot.
    fn synthetic_param_symbol(&mut self, index: usize) -> smelt_hir::Symbol {
        self.ctx
            .krate
            .symbols
            .intern(format!("__param{index}").as_str())
    }

    /// Infer a block-bodied arrow return type from reachable direct returns.
    fn last_return_type(&mut self, body: &Body) -> Option<smelt_hir::TypeId> {
        let mut found = Vec::new();
        Self::collect_return_types_from_block(body, body.root, &mut found);
        if let Some(first) = found.first().copied() {
            if found.iter().all(|ty| *ty == first) {
                return Some(first);
            }
            return Some(self.ctx.krate.types.intern(Type::Unknown));
        }
        let root = body.blocks.get(usize::try_from(body.root.0).ok()?)?;
        let last_stmt = root.stmts.last()?;
        match body.stmts.get(usize::try_from(last_stmt.0).ok()?) {
            Some(Stmt::Return(Some(expr))) => Some(Self::expr_ty(body, *expr)),
            _ => None,
        }
    }

    /// Collect return value types from a block and nested control-flow blocks.
    fn collect_return_types_from_block(
        body: &Body,
        block: smelt_hir::BlockId,
        out: &mut Vec<smelt_hir::TypeId>,
    ) {
        let Some(block) = body.blocks.get(usize::try_from(block.0).ok().unwrap_or(usize::MAX))
        else {
            return;
        };
        for stmt_id in &block.stmts {
            Self::collect_return_types_from_stmt(body, *stmt_id, out);
        }
    }

    /// Collect return value types from a statement and any nested statement bodies.
    fn collect_return_types_from_stmt(body: &Body, stmt_id: smelt_hir::StmtId, out: &mut Vec<smelt_hir::TypeId>) {
        let Some(stmt) = body.stmts.get(usize::try_from(stmt_id.0).ok().unwrap_or(usize::MAX))
        else {
            return;
        };
        match stmt {
            Stmt::Return(Some(expr)) => out.push(Self::expr_ty(body, *expr)),
            Stmt::If {
                then_block,
                else_block,
                ..
            } => {
                Self::collect_return_types_from_block(body, *then_block, out);
                if let Some(else_block) = else_block {
                    Self::collect_return_types_from_block(body, *else_block, out);
                }
            }
            Stmt::While { body: loop_body, .. }
            | Stmt::WhileUpdate {
                body: loop_body, ..
            }
            | Stmt::For { body: loop_body, .. } => {
                Self::collect_return_types_from_block(body, *loop_body, out);
            }
            Stmt::Match { arms, default, .. } => {
                for arm in arms {
                    Self::collect_return_types_from_block(body, arm.body, out);
                }
                if let Some(default) = default {
                    Self::collect_return_types_from_block(body, *default, out);
                }
            }
            Stmt::TryCatch {
                body: try_body,
                catch_body,
                finally_body,
                ..
            } => {
                Self::collect_return_types_from_block(body, *try_body, out);
                if let Some(catch_body) = catch_body {
                    Self::collect_return_types_from_block(body, *catch_body, out);
                }
                if let Some(finally_body) = finally_body {
                    Self::collect_return_types_from_block(body, *finally_body, out);
                }
            }
            _ => {}
        }
    }

    /// Lower date-fns-style `export const fpName = convertToFP(fn, arity)` wrappers.
    ///
    /// `convertToFP` reverses the first `arity` arguments before invoking the
    /// target function. Smelt does not model generic runtime currying yet, so
    /// this helper emits the direct uncurried wrapper shape used by generated
    /// date-fns `src/fp` entrypoints while preserving the real target return
    /// type and parameter types.
    fn fp_wrapper_const_declaration(
        &mut self,
        name_text: &str,
        init: &Expression<'_>,
    ) -> Result<Option<smelt_hir::ItemId>, SmeltError> {
        let Expression::CallExpression(call) = init else {
            return Ok(None);
        };
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "convertToFP" {
            return Ok(None);
        }
        let [target_arg, arity_arg] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "convertToFP exported const wrappers require function and arity arguments",
            ));
        };
        let Argument::Identifier(target_ident) = target_arg else {
            return Err(SmeltError::unsupported(
                self.span(target_arg.span().start, target_arg.span().end),
                "convertToFP exported const wrappers require an identifier function argument",
            ));
        };
        let arity = Self::fp_wrapper_arity(arity_arg, self.span(call.span.start, call.span.end))?;
        let Some(target_item) = self.items.get(target_ident.name.as_str()).copied() else {
            return Err(SmeltError::unsupported(
                self.span(target_ident.span.start, target_ident.span.end),
                format!(
                    "convertToFP exported const wrapper references unresolved function `{}`",
                    target_ident.name
                ),
            ));
        };
        self.pad_fp_wrapper_target_params(target_item, arity, self.span(call.span.start, call.span.end))?;
        let Item::Function(target_function) = self.item_ref(target_item).clone() else {
            return Err(SmeltError::unsupported(
                self.span(target_ident.span.start, target_ident.span.end),
                "convertToFP exported const wrappers require a function item argument",
            ));
        };

        let span = self.span(call.span.start, call.span.end);
        let mut body = Body::new(None, span);
        let mut wrapper_params = Vec::new();
        let mut wrapper_arg_locals = Vec::new();
        for target_param in target_function.params.iter().take(arity).rev() {
            let local = body.push_local(LocalDecl {
                name: Some(target_param.name),
                ty: target_param.ty,
                mutable: false,
                span,
            });
            body.params.push(local);
            wrapper_arg_locals.push(local);
            wrapper_params.push(Param {
                name: target_param.name,
                local,
                ty: target_param.ty,
                span,
            });
        }

        let callee_expr = body.push_expr(Expr {
            kind: ExprKind::Item(target_item),
            ty: self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: target_function
                    .params
                    .iter()
                    .map(|param| param.ty)
                    .collect(),
                rest: target_function.rest,
                required_params: None,
                    mutable_params: Vec::new(),
return_ty: target_function.return_ty,
                is_async: target_function.is_async,
                            may_throw: false,
            })),
            span,
        });
        let mut args = Vec::new();
        for (target_param, local) in target_function
            .params
            .iter()
            .take(arity)
            .zip(wrapper_arg_locals.into_iter().rev())
        {
            args.push(body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty: target_param.ty,
                span,
            }));
        }
        let call_expr = body.push_expr(Expr {
            kind: ExprKind::Call {
                callee: callee_expr,
                args,
            },
            ty: target_function.return_ty,
            span,
        });
        body.push_stmt(Stmt::Return(Some(call_expr)));

        let body_id = self.ctx.krate.push_body(body);
        let name = self.intern_source_name(name_text);
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            params: wrapper_params,
            rest: target_function.rest,
            required_params: None,
return_ty: target_function.return_ty,
            is_async: target_function.is_async,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        self.items.insert(name_text.to_owned(), item);
        self.ctx.export_aliases.insert(name_text.to_owned(), item);
        Ok(Some(item))
    }

    /// Pad a date-fns FP target function when an optional tail parameter was erased.
    ///
    /// Generated date-fns `WithOptions` FP modules use `convertToFP(fn, 3)` even
    /// when Smelt has only retained the target's required parameters. Keeping a
    /// synthetic unknown slot preserves the callable arity and lets the wrapper
    /// pass all collected FP arguments through in the original order.
    fn pad_fp_wrapper_target_params(
        &mut self,
        target_item: smelt_hir::ItemId,
        arity: usize,
        span: Span,
    ) -> Result<(), SmeltError> {
        let item_index = usize::try_from(target_item.0).unwrap_or(usize::MAX);
        let current_len = match self.ctx.krate.items.get(item_index) {
            Some(Item::Function(function)) => function.params.len(),
            _ => return Ok(()),
        };
        if current_len >= arity {
            return Ok(());
        }
        let body_id = match self.ctx.krate.items.get(item_index) {
            Some(Item::Function(function)) => function.body.ok_or_else(|| {
                SmeltError::unsupported(
                    span,
                    "convertToFP exported const wrapper target must have a body when arity padding is required",
                )
            })?,
            _ => return Ok(()),
        };
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let body_index = usize::try_from(body_id.0).unwrap_or(usize::MAX);
        let body = self.ctx.krate.bodies.get_mut(body_index).ok_or_else(|| {
            SmeltError::unsupported(
                span,
                "convertToFP exported const wrapper target body was not found",
            )
        })?;
        let mut padded_params = Vec::new();
        for index in current_len..arity {
            let name = self
                .ctx
                .krate
                .symbols
                .intern(format!("__fp_arg{index}").as_str());
            let local = body.push_local(LocalDecl {
                name: Some(name),
                ty: unknown_ty,
                mutable: false,
                span,
            });
            body.params.push(local);
            padded_params.push(Param {
                name,
                local,
                ty: unknown_ty,
                span,
            });
        }
        if let Some(Item::Function(function)) = self.ctx.krate.items.get_mut(item_index) {
            function.params.extend(padded_params);
        }
        Ok(())
    }

    /// Extract the literal arity used by generated date-fns `convertToFP` calls.
    fn fp_wrapper_arity(argument: &Argument<'_>, fallback_span: Span) -> Result<usize, SmeltError> {
        let Argument::NumericLiteral(literal) = argument else {
            return Err(SmeltError::unsupported(
                fallback_span,
                "convertToFP exported const wrappers require a numeric literal arity",
            ));
        };
        match literal.value {
            1.0_f64 => Ok(1),
            2.0_f64 => Ok(2),
            3.0_f64 => Ok(3),
            4.0_f64 => Ok(4),
            _ => Err(SmeltError::unsupported(
                fallback_span,
                "convertToFP exported const wrapper arity must be one of date-fns FP arities 1 through 4",
            )),
        }
    }

    /// Convert a supported TypeScript literal expression into an importable const value.
    fn literal_const_expression(
        &mut self,
        expression: &Expression<'_>,
    ) -> Result<ConstLiteral, SmeltError> {
        match expression {
            Expression::NumericLiteral(lit) => Ok(ConstLiteral {
                literal: Literal::Float(lit.value),
                ty: self.ctx.krate.types.intern(Type::Float),
            }),
            Expression::StringLiteral(lit) => Ok(ConstLiteral {
                literal: Literal::String(lit.value.to_string()),
                ty: self.ctx.krate.types.intern(Type::String),
            }),
            Expression::BooleanLiteral(lit) => Ok(ConstLiteral {
                literal: Literal::Bool(lit.value),
                ty: self.ctx.krate.types.intern(Type::Bool),
            }),
            Expression::NullLiteral(_) => Ok(ConstLiteral {
                literal: Literal::None,
                ty: self.ctx.krate.types.intern(Type::None),
            }),
            Expression::RegExpLiteral(lit) => Ok(ConstLiteral {
                literal: Literal::String(Self::regex_literal_pattern_text(lit)),
                ty: self.ctx.krate.types.intern(Type::String),
            }),
            Expression::Identifier(ident) => self
                .const_literals
                .get(ident.name.as_str())
                .cloned()
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(ident.span.start, ident.span.end),
                        format!(
                            "exported const expression references unresolved const `{}`",
                            ident.name
                        ),
                    )
                }),
            Expression::ParenthesizedExpression(parenthesized) => {
                self.literal_const_expression(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                self.literal_const_expression(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.literal_const_expression(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.literal_const_expression(&non_null.expression)
            }
            Expression::UnaryExpression(unary) => self.unary_literal_const_expression(unary),
            Expression::BinaryExpression(binary) => self.binary_literal_const_expression(binary),
            Expression::CallExpression(call) => self.call_literal_const_expression(call),
            _ => Err(SmeltError::unsupported(
                self.span(expression.span().start, expression.span().end),
                "exported const values currently support primitive literals and foldable primitive expressions",
            )),
        }
    }

    /// Convert a JavaScript regex literal into a Rust `regex` pattern string.
    fn regex_literal_pattern_text(literal: &oxc::ast::ast::RegExpLiteral<'_>) -> String {
        let flags = literal.regex.flags.to_string();
        let mut inline_flags = String::new();
        for flag in flags.chars() {
            match flag {
                'i' | 'm' | 's' => inline_flags.push(flag),
                _ => {}
            }
        }
        let pattern = Self::rust_regex_pattern_text(&literal.regex.pattern.text);
        if inline_flags.is_empty() {
            pattern
        } else {
            format!("(?{inline_flags}){pattern}")
        }
    }

    /// Convert a JavaScript regex literal pattern without applying flags.
    fn regex_literal_pattern_text_without_flags(
        literal: &oxc::ast::ast::RegExpLiteral<'_>,
    ) -> String {
        Self::rust_regex_pattern_text(&literal.regex.pattern.text)
    }

    /// Translate JavaScript regex syntax accepted by Smelt into Rust regex syntax.
    fn rust_regex_pattern_text(pattern: &str) -> String {
        pattern
            .replace("(?<", "(?P<")
            .replace("\\.{0,4096}", "\\.*")
            .replace(".{0,4096}?", ".*?")
            .replace("[^.[\\]]", "[^.\\[\\]]")
    }

    /// Return whether an exported const has known metadata value that is safe to skip.
    fn is_known_non_importable_exported_const(expression: &Expression<'_>) -> bool {
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return false;
        };
        let Expression::Identifier(object) = &member.object else {
            return false;
        };
        object.name == "Symbol" && member.property.name == "for"
    }

    /// Fold a supported unary expression inside an exported const initializer.
    fn unary_literal_const_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
    ) -> Result<ConstLiteral, SmeltError> {
        let value = self.literal_const_expression(&unary.argument)?;
        match (unary.operator, value.literal) {
            (UnaryOperator::UnaryPlus, Literal::Float(number)) => Ok(ConstLiteral {
                literal: Literal::Float(number),
                ty: self.ctx.krate.types.intern(Type::Float),
            }),
            (UnaryOperator::UnaryNegation, Literal::Float(number)) => Ok(ConstLiteral {
                literal: Literal::Float(-number),
                ty: self.ctx.krate.types.intern(Type::Float),
            }),
            (UnaryOperator::LogicalNot, Literal::Bool(value)) => Ok(ConstLiteral {
                literal: Literal::Bool(!value),
                ty: self.ctx.krate.types.intern(Type::Bool),
            }),
            _ => Err(SmeltError::unsupported(
                self.span(unary.span.start, unary.span.end),
                "exported const unary expressions currently support numeric plus, numeric negation, and boolean not",
            )),
        }
    }

    /// Fold a supported binary expression inside an exported const initializer.
    fn binary_literal_const_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
    ) -> Result<ConstLiteral, SmeltError> {
        let lhs = self.literal_const_expression(&binary.left)?;
        let rhs = self.literal_const_expression(&binary.right)?;
        match (lhs.literal, rhs.literal) {
            (Literal::Float(lhs), Literal::Float(rhs)) => {
                let value = match binary.operator {
                    BinaryOperator::Addition => lhs + rhs,
                    BinaryOperator::Subtraction => lhs - rhs,
                    BinaryOperator::Multiplication => lhs * rhs,
                    BinaryOperator::Division => lhs / rhs,
                    BinaryOperator::Remainder => lhs % rhs,
                    BinaryOperator::Exponential => lhs.powf(rhs),
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.span.start, binary.span.end),
                            "exported numeric const expressions support arithmetic operators only",
                        ));
                    }
                };
                Ok(ConstLiteral {
                    literal: Literal::Float(value),
                    ty: self.ctx.krate.types.intern(Type::Float),
                })
            }
            (Literal::String(lhs), Literal::String(rhs))
                if binary.operator == BinaryOperator::Addition =>
            {
                Ok(ConstLiteral {
                    literal: Literal::String(format!("{lhs}{rhs}")),
                    ty: self.ctx.krate.types.intern(Type::String),
                })
            }
            _ => Err(SmeltError::unsupported(
                self.span(binary.span.start, binary.span.end),
                "exported const binary expressions currently require matching primitive operands",
            )),
        }
    }

    /// Fold supported pure calls inside exported const initializers.
    fn call_literal_const_expression(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Result<ConstLiteral, SmeltError> {
        if matches!(&call.callee, Expression::Identifier(callee) if callee.name == "Symbol") {
            if call.arguments.len() > 1 {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "Symbol(...) supports at most one description argument",
                ));
            }
            return Ok(ConstLiteral {
                literal: Literal::Symbol(format!("Symbol()@{}", call.span.start)),
                ty: self.ctx.krate.types.intern(Type::Unknown),
            });
        }
        let Some(op) = stdlib_dispatch::pure_math_call(call) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "exported const call expressions currently support only Symbol and selected Math calls",
            ));
        };
        let args = call
            .arguments
            .iter()
            .map(|argument| self.number_literal_const_argument(argument))
            .collect::<Result<Vec<_>, _>>()?;
        let value = Self::fold_pure_math_const(op, &args).ok_or_else(|| {
            SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "exported const Math call has an unsupported argument count",
            )
        })?;
        Ok(ConstLiteral {
            literal: Literal::Float(value),
            ty: self.ctx.krate.types.intern(Type::Float),
        })
    }

    /// Fold one pure numeric Math operation using JavaScript-compatible f64 behavior.
    fn fold_pure_math_const(op: stdlib_dispatch::PureMathCall, args: &[f64]) -> Option<f64> {
        use stdlib_dispatch::PureMathCall;
        match op {
            PureMathCall::Abs => single_arg(args).map(f64::abs),
            PureMathCall::Floor => single_arg(args).map(f64::floor),
            PureMathCall::Ceil => single_arg(args).map(f64::ceil),
            PureMathCall::Round => single_arg(args).map(f64::round),
            PureMathCall::Trunc => single_arg(args).map(f64::trunc),
            PureMathCall::Max => Some(args.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
            PureMathCall::Min => Some(args.iter().copied().fold(f64::INFINITY, f64::min)),
            PureMathCall::Hypot => Some(args.iter().map(|arg| arg * arg).sum::<f64>().sqrt()),
            PureMathCall::Sqrt => single_arg(args).map(f64::sqrt),
            PureMathCall::Cbrt => single_arg(args).map(f64::cbrt),
            PureMathCall::Sign => single_arg(args).map(|arg| {
                if arg.is_nan() || arg == 0.0_f64 {
                    arg
                } else {
                    arg.signum()
                }
            }),
            PureMathCall::Sin => single_arg(args).map(f64::sin),
            PureMathCall::Cos => single_arg(args).map(f64::cos),
            PureMathCall::Tan => single_arg(args).map(f64::tan),
            PureMathCall::Asin => single_arg(args).map(f64::asin),
            PureMathCall::Acos => single_arg(args).map(f64::acos),
            PureMathCall::Atan => single_arg(args).map(f64::atan),
            PureMathCall::Log => single_arg(args).map(f64::ln),
            PureMathCall::Log10 => single_arg(args).map(f64::log10),
            PureMathCall::Log2 => single_arg(args).map(f64::log2),
            PureMathCall::Exp => single_arg(args).map(f64::exp),
            PureMathCall::Pow => two_args(args).map(|(base, exponent)| base.powf(exponent)),
            PureMathCall::Atan2 => two_args(args).map(|(y, x)| y.atan2(x)),
        }
    }

    /// Fold one numeric argument in an exported const call expression.
    fn number_literal_const_argument(
        &mut self,
        argument: &Argument<'_>,
    ) -> Result<f64, SmeltError> {
        let value = match argument {
            Argument::NumericLiteral(lit) => lit.value,
            Argument::Identifier(ident) => {
                let Some(ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                }) = self.const_literals.get(ident.name.as_str())
                else {
                    return Err(SmeltError::unsupported(
                        self.span(ident.span.start, ident.span.end),
                        format!(
                            "exported const Math.pow argument references non-numeric const `{}`",
                            ident.name
                        ),
                    ));
                };
                *value
            }
            Argument::CallExpression(call) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.call_literal_const_expression(call)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "exported const Math arguments must be numeric",
                    ));
                };
                value
            }
            Argument::UnaryExpression(unary) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.unary_literal_const_expression(unary)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(unary.span.start, unary.span.end),
                        "exported const Math.pow arguments must be numeric",
                    ));
                };
                value
            }
            Argument::BinaryExpression(binary) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.binary_literal_const_expression(binary)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(binary.span.start, binary.span.end),
                        "exported const Math.pow arguments must be numeric",
                    ));
                };
                value
            }
            Argument::ParenthesizedExpression(parenthesized) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.literal_const_expression(&parenthesized.expression)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(parenthesized.span.start, parenthesized.span.end),
                        "exported const Math arguments must be numeric",
                    ));
                };
                value
            }
            Argument::TSAsExpression(as_expr) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.literal_const_expression(&as_expr.expression)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(as_expr.span.start, as_expr.span.end),
                        "exported const Math arguments must be numeric",
                    ));
                };
                value
            }
            Argument::TSSatisfiesExpression(satisfies) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.literal_const_expression(&satisfies.expression)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(satisfies.span.start, satisfies.span.end),
                        "exported const Math arguments must be numeric",
                    ));
                };
                value
            }
            Argument::TSNonNullExpression(non_null) => {
                let ConstLiteral {
                    literal: Literal::Float(value),
                    ..
                } = self.literal_const_expression(&non_null.expression)?
                else {
                    return Err(SmeltError::unsupported(
                        self.span(non_null.span.start, non_null.span.end),
                        "exported const Math arguments must be numeric",
                    ));
                };
                value
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(argument.span().start, argument.span().end),
                    "exported const Math arguments must be foldable numeric expressions",
                ));
            }
        };
        Ok(value)
    }

    // Continued in the next split builder file.
}
