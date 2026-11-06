/// A Python lambda or single-`return` nested `def`, ready to become a closure.
struct PythonClosure<'a> {
    /// The closure's parameter types in order.
    params: &'a [TypeId],
    /// The source name each parameter is bound to, when the source names it.
    names: Vec<Option<&'a str>>,
    /// `*name` bound to the list of every parameter from `first` on.
    packed_rest: Option<PackedRest<'a>>,
    /// The expression the closure answers.
    body: &'a Expr,
    /// The declared return type, when one is known before the body is lowered.
    expected_return: Option<TypeId>,
    /// Source span of the whole closure.
    span: Span,
}

/// A `*name` parameter whose arguments arrive as separate closure parameters.
struct PackedRest<'a> {
    /// The source name of the packed list.
    name: &'a str,
    /// Index of the first closure parameter it collects.
    first: usize,
}

/// A lowered closure and the type it answers.
#[derive(Debug, Clone, Copy)]
struct LoweredClosure {
    /// HIR expression id of the closure value.
    expr: smelt_hir::ExprId,
    /// Return type of the closure.
    return_ty: TypeId,
}

/// Every name read inside an expression, in source order.
#[derive(Default)]
struct NamesRead<'a> {
    /// The names, first appearance first and each once.
    names: indexmap::IndexSet<&'a str>,
}

impl<'a> ruff_python_ast::visitor::Visitor<'a> for NamesRead<'a> {
    fn visit_expr(&mut self, expr: &'a Expr) {
        if let Expr::Name(name) = expr {
            self.names.insert(name.id.as_str());
        }
        ruff_python_ast::visitor::walk_expr(self, expr);
    }
}

impl ModuleBuilder<'_> {
    /// Lower a lambda against the parameter types its context supplies.
    ///
    /// A lambda carries no annotations, so `expected_param_tys` is the only
    /// source of its parameter types. `expected_return` is the context's declared
    /// return, the hint the body is lowered under when there is one.
    fn lambda_closure(
        &mut self,
        lambda: &ruff_python_ast::ExprLambda,
        expected_param_tys: &[TypeId],
        expected_return: Option<TypeId>,
        body: &mut Body,
    ) -> Result<LoweredClosure, SmeltError> {
        let closure = self.lambda_spec(lambda, expected_param_tys, expected_return)?;
        self.python_closure(&closure, body)
    }

    /// Lower default expressions for a Python lambda parameter list.
    fn lambda_defaults(
        &mut self,
        lambda: &ruff_python_ast::ExprLambda,
        body: &mut Body,
    ) -> Result<Vec<Option<smelt_hir::ExprId>>, SmeltError> {
        let Some(params) = lambda.parameters.as_ref() else {
            return Ok(Vec::new());
        };
        params
            .posonlyargs
            .iter()
            .chain(&params.args)
            .map(|param| {
                param
                    .default
                    .as_ref()
                    .map(|default| self.expression(default, body))
                    .transpose()
            })
            .collect()
    }

    /// Bind a lambda's parameter list to the closure parameters it receives.
    fn lambda_spec<'a>(
        &self,
        lambda: &'a ruff_python_ast::ExprLambda,
        expected_param_tys: &'a [TypeId],
        expected_return: Option<TypeId>,
    ) -> Result<PythonClosure<'a>, SmeltError> {
        let span = self.span(lambda.range);
        let mut names = vec![None; expected_param_tys.len()];
        let mut packed_rest = None;
        if let Some(params) = lambda.parameters.as_ref() {
            if !params.kwonlyargs.is_empty() {
                return Err(SmeltError::unsupported(
                    span,
                    "keyword-only lambda parameters need callable keyword parameter modelling",
                ));
            }
            let positional = params.posonlyargs.iter().chain(&params.args);
            let positional_count = positional.clone().count();
            if positional_count > expected_param_tys.len() {
                return Err(SmeltError::unsupported(
                    span,
                    "lambda parameter count does not match callback input",
                ));
            }
            for (slot, param) in names.iter_mut().zip(positional) {
                *slot = Some(param.parameter.name.as_str());
            }
            if let Some(kwarg) = &params.kwarg
                && let Some(slot) = names.last_mut()
            {
                *slot = Some(kwarg.name.as_str());
            }
            if let Some(vararg) = &params.vararg {
                let single_packed_list = Self::lambda_vararg_metadata(
                    lambda,
                    expected_param_tys,
                    &self.ctx.krate.types,
                )
                .is_some();
                if single_packed_list && positional_count == 0 {
                    names[0] = Some(vararg.name.as_str());
                } else {
                    packed_rest = Some(PackedRest {
                        name: vararg.name.as_str(),
                        first: positional_count,
                    });
                }
            }
        } else if !expected_param_tys.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "lambda parameter count does not match callback input",
            ));
        }
        Ok(PythonClosure {
            params: expected_param_tys,
            names,
            packed_rest,
            body: &lambda.body,
            expected_return,
            span,
        })
    }

    /// Lower a closure body through ordinary expression lowering.
    ///
    /// The closure gets its own [`Body`] and [`FnScope`]: parameters and the
    /// enclosing locals its body reads become closure locals, so every Python
    /// expression means inside a closure what it means anywhere else. A captured
    /// callable keeps its call-packing metadata, without defaults: those are
    /// expressions of the defining body.
    #[expect(clippy::iter_over_hash_type, reason = "each entry is cleared on its own, so order cannot show")]
    fn python_closure(
        &mut self,
        closure: &PythonClosure<'_>,
        body: &mut Body,
    ) -> Result<LoweredClosure, SmeltError> {
        let span = closure.span;
        let mut closure_body = Body::new(None, span);
        let mut locals = HashMap::new();
        let closure_params = closure
            .params
            .iter()
            .zip(&closure.names)
            .enumerate()
            .map(|(index, (ty, source_name))| {
                let name = self.intern_name(
                    &source_name.map_or_else(|| format!("__callback_param_{index}"), str::to_owned),
                );
                let local = closure_body.push_local(LocalDecl {
                    name: Some(name),
                    ty: *ty,
                    mutable: false,
                    span,
                });
                closure_body.params.push(local);
                if let Some(source_name) = source_name {
                    locals.insert((*source_name).to_owned(), local);
                }
                Param {
                    name,
                    local,
                    ty: *ty,
                    span,
                }
            })
            .collect::<Vec<_>>();
        if let Some(rest) = &closure.packed_rest {
            let local = self.packed_rest_local(rest, &closure_params, &mut closure_body, span);
            locals.insert(rest.name.to_owned(), local);
        }
        let captures = self.closure_captures(closure.body, &mut locals, &mut closure_body, body);
        let mut callbacks = self.scope.local_callbacks.clone();
        for callback in callbacks.values_mut() {
            callback.defaults.fill(None);
        }
        let tail = self.with_fn_scope(FnKind::Sync, |this| {
            this.scope.locals = locals;
            this.scope.local_callbacks = callbacks;
            this.expression_with_hint(closure.body, &mut closure_body, closure.expected_return)
        })?;
        let return_ty = Self::expr_ty(&closure_body, tail);
        closure_body.blocks[0].tail = Some(tail);
        let body_id = self.ctx.krate.push_body(closure_body);
        let closure_ty = self.intern_type(Type::Function(FunctionType {
            params: closure.params.to_vec(),
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty,
            is_async: false,
            may_throw: false,
        }));
        let expr = body.push_expr(HirExpr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params: closure_params,
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
        });
        Ok(LoweredClosure { expr, return_ty })
    }

    /// Declare the list a `*name` binds and fill it from its closure parameters.
    fn packed_rest_local(
        &mut self,
        rest: &PackedRest<'_>,
        params: &[Param],
        closure_body: &mut Body,
        span: Span,
    ) -> smelt_hir::LocalId {
        let packed = params.get(rest.first..).unwrap_or_default();
        let item_ty = packed.first().map_or_else(
            || self.erase(smelt_hir::ErasureReason::MissingAnnotation, span, "*args"),
            |param| param.ty,
        );
        let list_ty = self.intern_type(Type::List(item_ty));
        let items = packed
            .iter()
            .map(|param| {
                closure_body.push_expr(HirExpr {
                    kind: ExprKind::Local(param.local),
                    ty: param.ty,
                    span,
                })
            })
            .collect();
        let value = closure_body.push_expr(HirExpr {
            kind: ExprKind::ListLit(items),
            ty: list_ty,
            span,
        });
        let name = self.intern_name(rest.name);
        let local = closure_body.push_local(LocalDecl {
            name: Some(name),
            ty: list_ty,
            mutable: false,
            span,
        });
        let pat = closure_body.push_pattern(HirPattern::Binding(local));
        let root = closure_body.root;
        closure_body.push_stmt_to_block(root, HirStmt::Let { pat, ty: list_ty, value: Some(value) });
        local
    }

    /// Capture each enclosing local the closure body reads, first read first.
    ///
    /// A name already in `locals` is a parameter and shadows the enclosing one.
    /// A capture is by reference: Python closures see later writes to the
    /// enclosing variable.
    fn closure_captures(
        &mut self,
        closure_expr: &Expr,
        locals: &mut HashMap<String, smelt_hir::LocalId>,
        closure_body: &mut Body,
        body: &Body,
    ) -> Vec<ClosureCapture> {
        use ruff_python_ast::visitor::Visitor as _;
        let mut read = NamesRead::default();
        read.visit_expr(closure_expr);
        let mut captures = Vec::new();
        for name in read.names {
            if locals.contains_key(name) {
                continue;
            }
            let Some(&source_local) = self.scope.locals.get(name) else {
                continue;
            };
            let ty = Self::local_ty(body, source_local);
            let symbol = self.intern_name(name);
            let span = body
                .locals
                .get(usize::try_from(source_local.0).unwrap_or(usize::MAX))
                .map_or(closure_body.blocks[0].span, |decl| decl.span);
            let body_local = closure_body.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: false,
                span,
            });
            locals.insert(name.to_owned(), body_local);
            captures.push(ClosureCapture {
                source_local,
                body_local: Some(body_local),
                symbol,
                ty,
                mode: CaptureMode::ByRef,
            });
        }
        captures
    }
}
