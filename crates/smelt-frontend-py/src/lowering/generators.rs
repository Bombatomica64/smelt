// Generator functions: a `def` whose body contains `yield`.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// A generator function answers the shared generator carrier
// (`Type::Generator`, the runtime's `SmeltGenerator<T, R, N>`, which the
// TypeScript frontend's generators use too): its body is a generator body
// (`Body::set_generator`), each `yield v` statement suspends it
// (`ExprKind::GeneratorYield`). A `for` loop resumes it once per iteration;
// `list`, `sum`, `min`, `max` and `sorted`, which consume every element,
// drain it (`ExprKind::GeneratorCollect`). A consumer that may stop early
// (`zip`, `enumerate`, `any`, `all` of a generator object) is refused, since
// draining would compute elements CPython never asks for. Its return annotation names the yield type:
// `Iterator[T]`, or `Generator[T, None, None]` (nothing sent, nothing
// returned). `send()`, `yield` used as a value, `yield from` and a
// `return value` are refused.

impl ModuleBuilder<'_> {
    /// `Iterator[T]` / `Generator[T, None, None]` (also `Generator[T]`, whose
    /// other arguments default to `None`): the generator carrier yielding `T`.
    fn generator_annotation(
        &mut self,
        sub: &ruff_python_ast::ExprSubscript,
        type_name: &str,
        span: Span,
    ) -> Result<TypeId, SmeltError> {
        let args: Vec<&Expr> = match sub.slice.as_ref() {
            Expr::Tuple(tuple) => tuple.elts.iter().collect(),
            other => vec![other],
        };
        let Some((yielded, rest)) = args.split_first() else {
            return Err(SmeltError::unsupported(span, format!("'{type_name}' needs a yield type")));
        };
        if type_name == "Iterator" && !rest.is_empty() {
            return Err(SmeltError::unsupported(span, "'Iterator' takes one type argument"));
        }
        if rest.iter().any(|arg| !matches!(arg, Expr::NoneLiteral(_))) {
            return Err(SmeltError::unsupported(
                span,
                "a generator that is sent values or returns one (`Generator[T, S, R]` with S or R not None) is not supported",
            ));
        }
        let yield_ty = self.annotation_to_hir(yielded)?;
        let none_ty = self.intern_type(Type::None);
        Ok(self.intern_type(Type::Generator { is_async: false, yield_ty, return_ty: none_ty, next_ty: none_ty }))
    }

    /// The generator type and yield type of `func` when it is a generator
    /// function (its body, outside nested functions, contains `yield`), read
    /// from its return annotation; `None` for an ordinary function.
    fn generator_signature(&mut self, func: &StmtFunctionDef) -> Result<Option<(TypeId, TypeId)>, SmeltError> {
        /// Finds a `yield` outside nested functions and lambdas.
        struct Finder(bool);
        impl<'a> ruff_python_ast::visitor::Visitor<'a> for Finder {
            fn visit_stmt(&mut self, stmt: &'a Stmt) {
                if !matches!(stmt, Stmt::FunctionDef(_) | Stmt::ClassDef(_)) {
                    ruff_python_ast::visitor::walk_stmt(self, stmt);
                }
            }
            fn visit_expr(&mut self, expr: &'a Expr) {
                match expr {
                    Expr::Yield(_) | Expr::YieldFrom(_) => self.0 = true,
                    Expr::Lambda(_) => {}
                    _ => ruff_python_ast::visitor::walk_expr(self, expr),
                }
            }
        }
        let mut finder = Finder(false);
        for stmt in &func.body {
            ruff_python_ast::visitor::Visitor::visit_stmt(&mut finder, stmt);
        }
        if !finder.0 {
            return Ok(None);
        }
        if func.is_async {
            return Err(SmeltError::unsupported(self.span(func.range), "async generators are not supported"));
        }
        let return_ty = match func.returns.as_deref() {
            Some(annotation) => Some(self.annotation_to_hir(annotation)?),
            None => None,
        };
        match return_ty.and_then(|ty| self.ctx.krate.types.get(ty).cloned().map(|found| (ty, found))) {
            Some((ty, Type::Generator { is_async: false, yield_ty, .. })) => Ok(Some((ty, yield_ty))),
            _ => Err(SmeltError::unsupported(
                self.span(func.range),
                format!(
                    "generator function '{}' needs an `Iterator[T]` or `Generator[T, None, None]` return annotation",
                    func.name
                ),
            )),
        }
    }

    /// A `yield value` (or bare `yield`) statement in a generator function:
    /// suspend, exposing the value. What the caller sends back is `None`.
    fn yield_statement(
        &mut self,
        yield_expr: &ruff_python_ast::ExprYield,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(yield_expr.range);
        let Some(yield_ty) = self.scope.generator else {
            return Err(SmeltError::unsupported(span, "`yield` outside a generator function is not supported"));
        };
        let value = if let Some(yielded) = yield_expr.value.as_deref() {
            self.expression_with_hint(yielded, body, Some(yield_ty))?
        } else {
            let none_ty = self.intern_type(Type::None);
            body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::None), ty: none_ty, span })
        };
        if Self::expr_ty(body, value) != yield_ty {
            return Err(SmeltError::unsupported(span, "the yielded value must have the generator's yield type"));
        }
        let none_ty = self.intern_type(Type::None);
        let suspend = body.push_expr(HirExpr { kind: ExprKind::GeneratorYield { value }, ty: none_ty, span });
        body.push_stmt_to_block(block, HirStmt::Expr(suspend));
        Ok(())
    }

    /// The list a generator drains to, when `value` is a generator: only for
    /// a consumer that reads every element (`list`, `sum`, `min`, `max`,
    /// `sorted`).
    fn generator_as_list(&mut self, value: smelt_hir::ExprId, body: &mut Body) -> Option<smelt_hir::ExprId> {
        let Some(Type::Generator { is_async: false, yield_ty, .. }) = self.ctx.krate.types.get(Self::expr_ty(body, value)).cloned() else {
            return None;
        };
        let span = usize::try_from(value.0)
            .ok()
            .and_then(|index| body.exprs.get(index))
            .map_or_else(|| Span::new(self.file_id, 0, 0), |expr| expr.span);
        let ty = self.intern_type(Type::List(yield_ty));
        Some(body.push_expr(HirExpr { kind: ExprKind::GeneratorCollect { generator: value }, ty, span }))
    }

    /// `for target in generator:` as the steps Python takes: resume the
    /// generator once per iteration, stop when it is exhausted, bind the
    /// yielded value. Each value is computed only when the loop asks for it,
    /// so the generator's effects interleave with the loop body's.
    ///
    /// ```text
    /// generator = <iter>
    /// while True:
    ///     step = next(generator)
    ///     if step is done: break
    ///     target = step's value
    ///     <body>
    /// ```
    fn generator_for_loop(
        &mut self,
        for_stmt: &StmtFor,
        generator: smelt_hir::ExprId,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(for_stmt.range);
        let generator_ty = Self::expr_ty(body, generator);
        let Some(Type::Generator { yield_ty, return_ty, .. }) = self.ctx.krate.types.get(generator_ty).cloned() else {
            return Err(SmeltError::unsupported(span, "internal error: expected a generator"));
        };
        let handle = body.push_local(self.synthetic_local("__smelt_generator", generator_ty, true, span));
        let handle_pat = body.push_pattern(HirPattern::Binding(handle));
        body.push_stmt_to_block(block, HirStmt::Let { pat: handle_pat, ty: generator_ty, value: Some(generator) });
        let bool_ty = self.intern_type(Type::Bool);
        let step_ty = self.intern_type(Type::GeneratorResult { yield_ty, return_ty });
        let loop_block = body.push_block(span);
        let resumed = body.push_expr(HirExpr { kind: ExprKind::Local(handle), ty: generator_ty, span });
        let next = body.push_expr(HirExpr {
            kind: ExprKind::GeneratorNext { generator: resumed, value: None, kind: smelt_hir::GeneratorResumeKind::Next },
            ty: step_ty,
            span,
        });
        let step = body.push_local(self.synthetic_local("__smelt_step", step_ty, false, span));
        let step_pat = body.push_pattern(HirPattern::Binding(step));
        body.push_stmt_to_block(loop_block, HirStmt::Let { pat: step_pat, ty: step_ty, value: Some(next) });
        let step_read = body.push_expr(HirExpr { kind: ExprKind::Local(step), ty: step_ty, span });
        let done = body.push_expr(HirExpr { kind: ExprKind::GeneratorDone { result: step_read }, ty: bool_ty, span });
        let exhausted = body.push_block(span);
        body.push_stmt_to_block(exhausted, HirStmt::Break);
        body.push_stmt_to_block(loop_block, HirStmt::If { cond: done, then_block: exhausted, else_block: None });
        let step_value = body.push_expr(HirExpr { kind: ExprKind::Local(step), ty: step_ty, span });
        let value = body.push_expr(HirExpr { kind: ExprKind::GeneratorValue { result: step_value }, ty: yield_ty, span });
        let pat = self.binding_pattern_from_target(&for_stmt.target, body, Some(yield_ty))?;
        body.push_stmt_to_block(loop_block, HirStmt::Let { pat, ty: yield_ty, value: Some(value) });
        self.with_narrowings(&[], |this| {
            for stmt in &for_stmt.body {
                this.statement_in_block(stmt, body, loop_block)?;
            }
            Ok::<(), SmeltError>(())
        })?;
        let forever = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(true)), ty: bool_ty, span });
        body.push_stmt_to_block(block, HirStmt::While { cond: forever, body: loop_block });
        Ok(())
    }
}
