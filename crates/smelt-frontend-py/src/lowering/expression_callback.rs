// Callbacks whose body is one expression: a `lambda`, or a nested `def` that
// returns one value. The body lowers through ordinary expression lowering inside
// a fresh closure body, so an expression means the same thing in a callback as
// anywhere else in the module.

/// How one source parameter name of an expression callback is bound.
#[derive(Debug, Clone)]
enum CallbackBinding {
    /// The closure parameter at this position.
    Param(usize),
    /// A list of the closure parameters at these positions: a lambda `*args`
    /// that collects the trailing parameters of its `Callable`.
    Packed(Vec<usize>),
}

/// A callback whose body is a single expression, kept as source so each use
/// lowers it against the locals visible there.
#[derive(Debug, Clone)]
struct ExprCallback {
    /// Source parameter names and what each is bound to.
    bindings: Vec<(String, CallbackBinding)>,
    /// Closure parameter types, in order.
    params: Vec<TypeId>,
    /// The body expression.
    body: Expr,
    /// The return type the context expects, used as the body's type hint.
    expected_return: Option<TypeId>,
}

/// An expression callback lowered into its own body, not yet stored in the crate.
struct LoweredCallback {
    /// The closure body, its tail the callback's value.
    body: Body,
    /// The closure parameters.
    params: Vec<Param>,
    /// Outer locals the body reads.
    captures: Vec<ClosureCapture>,
    /// The type of the body expression.
    return_ty: TypeId,
}

/// Collects the names an expression reads, in source order, without repeats.
#[derive(Default)]
struct ReadNames {
    /// Names in first-read order.
    names: indexmap::IndexSet<String>,
}

impl<'a> ruff_python_ast::visitor::Visitor<'a> for ReadNames {
    /// Records a loaded name, then walks the rest of the expression.
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Name(name) = expr
            && name.ctx.is_load()
        {
            self.names.insert(name.id.to_string());
        }
        ruff_python_ast::visitor::walk_expr(self, expr);
    }
}

impl ModuleBuilder<'_> {
    /// Describe a `lambda` as an expression callback over the parameter types
    /// its context supplies.
    ///
    /// Positional parameters take the leading closure parameters; `*args`
    /// collects the rest into a list and `**kwargs` binds the last one.
    fn lambda_expr_callback(
        &self,
        lambda: &ruff_python_ast::ExprLambda,
        expected_param_tys: &[TypeId],
        expected_return: Option<TypeId>,
    ) -> Result<ExprCallback, SmeltError> {
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
        let mut bindings = param_items
            .iter()
            .enumerate()
            .map(|(index, param)| {
                (param.parameter.name.to_string(), CallbackBinding::Param(index))
            })
            .collect::<Vec<_>>();
        if let Some(vararg) = &params.vararg {
            let rest = (param_items.len()..expected_param_tys.len()).collect();
            bindings.push((vararg.name.to_string(), CallbackBinding::Packed(rest)));
        }
        if let Some(kwarg) = &params.kwarg {
            bindings.push((
                kwarg.name.to_string(),
                CallbackBinding::Param(expected_param_tys.len().saturating_sub(1)),
            ));
        }
        Ok(ExprCallback {
            bindings,
            params: expected_param_tys.to_vec(),
            body: (*lambda.body).clone(),
            expected_return,
        })
    }

    /// Lower an expression callback into a closure body of its own.
    ///
    /// Every outer local the body reads becomes an explicit by-reference
    /// capture, in first-read order; a name that is a parameter of the callback
    /// shadows the outer local. Names of local callbacks are not captured: they
    /// hold no value of their own and are lowered at each use.
    fn lower_expr_callback(
        &mut self,
        callback: &ExprCallback,
        span: Span,
        outer: &Body,
    ) -> Result<LoweredCallback, SmeltError> {
        let mut closure_body = Body::new(None, span);
        let params = callback
            .params
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
        let mut read = ReadNames::default();
        ruff_python_ast::visitor::Visitor::visit_expr(&mut read, &callback.body);
        let mut captures = Vec::new();
        let mut visible = HashMap::new();
        for name in read.names {
            if callback.bindings.iter().any(|(bound, _)| *bound == name)
                || self.scope.local_callbacks.contains_key(&name)
            {
                continue;
            }
            let Some(&source_local) = self.scope.locals.get(&name) else {
                continue;
            };
            let Some(decl) = outer.locals.get(source_local.0 as usize) else {
                continue;
            };
            let symbol = decl.name.unwrap_or_else(|| self.intern_name("__capture"));
            let body_local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty: decl.ty,
                mutable: false,
                span,
            });
            captures.push(ClosureCapture {
                source_local,
                body_local: Some(body_local),
                symbol,
                ty: decl.ty,
                mode: CaptureMode::ByRef,
            });
            visible.insert(name, body_local);
        }
        for (name, binding) in &callback.bindings {
            let local = match binding {
                CallbackBinding::Param(index) => params
                    .get(*index)
                    .map(|param| param.local)
                    .ok_or_else(|| {
                        SmeltError::unsupported(span, "callback parameter index is out of bounds")
                    })?,
                CallbackBinding::Packed(indices) => {
                    self.packed_callback_param((name, indices), &params, span, &mut closure_body)
                }
            };
            visible.insert(name.clone(), local);
        }
        let tail = self.with_fn_scope(FnKind::Sync, |this| {
            this.scope.locals = visible;
            this.expression_with_hint(&callback.body, &mut closure_body, callback.expected_return)
        })?;
        let return_ty = Self::expr_ty(&closure_body, tail);
        let root = closure_body.root;
        closure_body.blocks[root.0 as usize].tail = Some(tail);
        Ok(LoweredCallback {
            body: closure_body,
            params,
            captures,
            return_ty,
        })
    }

    /// Bind a lambda `*args` to a list of the closure parameters it collects.
    ///
    /// With nothing to collect the item type is unknowable, so the list is
    /// erased with the same reason a missing annotation gets.
    fn packed_callback_param(
        &mut self,
        (name, indices): (&str, &[usize]),
        params: &[Param],
        span: Span,
        closure_body: &mut Body,
    ) -> smelt_hir::LocalId {
        let reads = indices
            .iter()
            .filter_map(|index| params.get(*index))
            .map(|param| {
                closure_body.push_expr(HirExpr {
                    kind: ExprKind::Local(param.local),
                    ty: param.ty,
                    span,
                })
            })
            .collect::<Vec<_>>();
        let item_ty = indices
            .first()
            .and_then(|index| params.get(*index))
            .map_or_else(
                || self.erase(smelt_hir::ErasureReason::MissingAnnotation, span, "*args"),
                |param| param.ty,
            );
        let list_ty = self.intern_type(Type::List(item_ty));
        let symbol = self.intern_name(name);
        let local = closure_body.push_local(LocalDecl {
            name: Some(symbol),
            ty: list_ty,
            mutable: false,
            span,
        });
        let value = closure_body.push_expr(HirExpr {
            kind: ExprKind::ListLit(reads),
            ty: list_ty,
            span,
        });
        let pat = closure_body.push_pattern(HirPattern::Binding(local));
        let root = closure_body.root;
        closure_body.push_stmt_to_block(
            root,
            HirStmt::Let {
                pat,
                ty: list_ty,
                value: Some(value),
            },
        );
        local
    }

    /// Store a lowered callback in the crate and answer the closure value.
    fn store_expr_callback(
        &mut self,
        lowered: LoweredCallback,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let LoweredCallback {
            body: closure_body,
            params,
            captures,
            return_ty,
        } = lowered;
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.intern_type(Type::Function(FunctionType {
            params: params.iter().map(|param| param.ty).collect(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        }));
        body.push_expr(HirExpr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params,
                rest: None,
                required_params: None,
                return_ty,
                captures,
                body: body_id,
                function_item: None,
                span,
            }),
            ty: closure_ty,
            span,
        })
    }

    /// Lower an expression callback and store it as a closure value here.
    fn expr_callback_closure(
        &mut self,
        callback: &ExprCallback,
        span: Span,
        body: &mut Body,
    ) -> Result<ClosureCallback, SmeltError> {
        let lowered = self.lower_expr_callback(callback, span, body)?;
        let return_ty = lowered.return_ty;
        let expr = self.store_expr_callback(lowered, span, body);
        Ok(ClosureCallback { expr, return_ty })
    }

    /// The type an expression callback's body has where it is defined.
    ///
    /// A local callback is lowered again at each use, so its definition only
    /// needs the type, to check it against the declared one; the lowered body is
    /// discarded.
    fn expr_callback_return_ty(
        &mut self,
        callback: &ExprCallback,
        span: Span,
        body: &Body,
    ) -> Result<TypeId, SmeltError> {
        Ok(self.lower_expr_callback(callback, span, body)?.return_ty)
    }
}
