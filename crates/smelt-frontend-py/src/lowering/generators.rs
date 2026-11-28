// Generator functions: a `def` whose body contains `yield`.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// A generator function answers the shared generator carrier
// (`Type::Generator`, the runtime's `SmeltGenerator<Y, R, S>`, which the
// TypeScript frontend's generators use too): its body is a generator body
// (`Body::set_generator`), each `yield v` suspends it
// (`ExprKind::GeneratorYield`) and evaluates to what the consumer sends
// back, and `return value` completes it with the value `StopIteration`
// carries. Its return annotation names the three types:
// `Generator[Y, S, R]` (`Generator[Y]` defaults S and R to `None`) or
// `Iterator[Y]` (nothing sent, nothing returned).
//
// Consumers resume it one value at a time, as CPython does:
// - a `for` loop, once per iteration;
// - `next(g)`, `next(g, default)` and `g.send(v)`, once per call, raising
//   `StopIteration` (its `args` the return value when there is one) or
//   answering the default once it is exhausted;
// - `yield from g` inside another generator, forwarding each value and each
//   `send` (`ExprKind::GeneratorDelegate`, JavaScript's `yield*`), and
//   evaluating to `g`'s return value; `yield from` of a list yields each item;
// - `zip`, `enumerate`, `any` and `all` over a generator object, through the
//   same per-iteration loop (`lowering/lazy_iteration.rs`), so a value is
//   produced only when the consumer asks for it.
// `list`, `sum`, `min`, `max` and `sorted`, which consume every element,
// drain it (`ExprKind::GeneratorCollect`).
//
// - `g.throw(exc)`, raising `exc` at the suspended `yield` (or at the start of
//   a just-started generator, which is then done) and answering what the
//   generator yields next, as `next` does;
// - `g.close()`, raising `GeneratorExit` there: the generator's `finally`
//   blocks and `except GeneratorExit` handlers run, a `yield` after it is a
//   `RuntimeError`, and `GeneratorExit` (or `StopIteration`) ends the call;
// - `g.send(v)` of a value that may be other than `None` to a just-started
//   generator raises `TypeError`, as CPython does;
// - `except StopIteration as e: e.value` reads the generator's return value as
//   its Python text (`StopIteration.value` is `Any`; see
//   `exception_attribute_expr`).

impl ModuleBuilder<'_> {
    /// `Iterator[Y]` / `Generator[Y, S, R]` (`Generator[Y]` and
    /// `Generator[Y, S]` default the rest to `None`): the generator carrier
    /// yielding `Y`, sent `S`, returning `R`.
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
        if rest.len() > 2 {
            return Err(SmeltError::unsupported(span, "'Generator' takes at most three type arguments"));
        }
        let yield_ty = self.annotation_to_hir(yielded)?;
        let none_ty = self.intern_type(Type::None);
        let next_ty = match rest.first() {
            Some(sent) => self.annotation_to_hir(sent)?,
            None => none_ty,
        };
        let return_ty = match rest.get(1) {
            Some(returned) => self.annotation_to_hir(returned)?,
            None => none_ty,
        };
        Ok(self.intern_type(Type::Generator { is_async: false, yield_ty, return_ty, next_ty }))
    }

    /// The generator type and its parts of `func` when it is a generator
    /// function (its body, outside nested functions, contains `yield`), read
    /// from its return annotation; `None` for an ordinary function.
    fn generator_signature(&mut self, func: &StmtFunctionDef) -> Result<Option<(TypeId, GeneratorScope)>, SmeltError> {
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
        let annotated = match func.returns.as_deref() {
            Some(annotation) => Some(self.annotation_to_hir(annotation)?),
            None => None,
        };
        match annotated.and_then(|ty| self.ctx.krate.types.get(ty).cloned().map(|found| (ty, found))) {
            Some((ty, Type::Generator { is_async: false, yield_ty, return_ty, next_ty })) => {
                Ok(Some((ty, GeneratorScope { yielded: yield_ty, sent: next_ty, returned: return_ty })))
            }
            _ => Err(SmeltError::unsupported(
                self.span(func.range),
                format!(
                    "generator function '{}' needs an `Iterator[T]` or `Generator[Y, S, R]` return annotation",
                    func.name
                ),
            )),
        }
    }

    /// A `yield value` (or bare `yield`) statement in a generator function:
    /// the expression, its sent value discarded.
    fn yield_statement(
        &mut self,
        yield_expr: &ruff_python_ast::ExprYield,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let suspend = self.yield_expression(yield_expr, body)?;
        body.push_stmt_to_block(block, HirStmt::Expr(suspend));
        Ok(())
    }

    /// `yield value` (or bare `yield`, which yields `None`): suspend,
    /// exposing the value, and evaluate to what the consumer sends back (`S`;
    /// `None` for `next()` and a `for` loop).
    fn yield_expression(
        &mut self,
        yield_expr: &ruff_python_ast::ExprYield,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(yield_expr.range);
        let Some(generator) = self.scope.generator else {
            return Err(SmeltError::unsupported(span, "`yield` outside a generator function is not supported"));
        };
        let value = if let Some(yielded) = yield_expr.value.as_deref() {
            self.expression_with_hint(yielded, body, Some(generator.yielded))?
        } else {
            let none_ty = self.intern_type(Type::None);
            body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::None), ty: none_ty, span })
        };
        if Self::expr_ty(body, value) != generator.yielded {
            return Err(SmeltError::unsupported(span, "the yielded value must have the generator's yield type"));
        }
        Ok(body.push_expr(HirExpr { kind: ExprKind::GeneratorYield { value }, ty: generator.sent, span }))
    }

    /// `yield from source` in a generator function.
    ///
    /// A generator source is delegated to (`ExprKind::GeneratorDelegate`):
    /// each value it yields is yielded on, each value sent in is forwarded to
    /// it, and the expression evaluates to its return value. Its yield type
    /// must be this generator's, and what this generator is sent must be what
    /// it accepts. A list source yields each item in turn (a `str` each
    /// character) and evaluates to `None`.
    fn yield_from_expression(
        &mut self,
        yield_from: &ruff_python_ast::ExprYieldFrom,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(yield_from.range);
        let Some(generator) = self.scope.generator else {
            return Err(SmeltError::unsupported(span, "`yield from` outside a generator function is not supported"));
        };
        let source = self.expression(&yield_from.value, body)?;
        let source_ty = Self::expr_ty(body, source);
        match self.ctx.krate.types.get(source_ty).cloned() {
            Some(Type::Generator { is_async: false, yield_ty, return_ty, next_ty }) => {
                if yield_ty != generator.yielded || next_ty != generator.sent {
                    return Err(SmeltError::unsupported(
                        span,
                        "`yield from` needs a generator with this generator's yield and send types",
                    ));
                }
                Ok(body.push_expr(HirExpr { kind: ExprKind::GeneratorDelegate { generator: source }, ty: return_ty, span }))
            }
            // Each item (each character of a `str`), its sent values
            // discarded; the expression is `None`.
            Some(Type::List(item)) if item == generator.yielded => {
                let none_ty = self.intern_type(Type::None);
                Ok(body.push_expr(HirExpr { kind: ExprKind::GeneratorDelegate { generator: source }, ty: none_ty, span }))
            }
            Some(Type::PyStr) if source_ty == generator.yielded => {
                let none_ty = self.intern_type(Type::None);
                Ok(body.push_expr(HirExpr { kind: ExprKind::GeneratorDelegate { generator: source }, ty: none_ty, span }))
            }
            _ => Err(SmeltError::unsupported(
                span,
                "`yield from` needs a generator, or a list (or `str`) of the generator's yield type",
            )),
        }
    }

    /// Resume `generator` once, as `next(g)` (`sent` absent) or `g.send(v)`
    /// do, and evaluate to the value it yields. Once it is exhausted the
    /// expression raises `StopIteration`, whose `args` hold the generator's
    /// return value unless that is `None`, or evaluates to `default` when one
    /// is given (`next(g, default)`).
    ///
    /// ```text
    /// step = resume(generator, sent)
    /// if step is done: raise StopIteration(step's value)   # or: result = default
    /// else: result = step's value
    /// result
    /// ```
    ///
    /// `kind` is the command the resume sends: `Next` for `next`/`send`,
    /// `Throw` for `g.throw(exc)` (`sent` is then the exception).
    #[expect(clippy::too_many_arguments, reason = "one resumption, what it sends, its default and its lowering position")]
    fn generator_resume_expression(
        &mut self,
        generator: smelt_hir::ExprId,
        sent: Option<smelt_hir::ExprId>,
        kind: smelt_hir::GeneratorResumeKind,
        default: Option<smelt_hir::ExprId>,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let generator_ty = Self::expr_ty(body, generator);
        let Some(Type::Generator { is_async: false, yield_ty, return_ty, .. }) = self.ctx.krate.types.get(generator_ty).cloned() else {
            return Err(SmeltError::unsupported(span, "internal error: expected a generator"));
        };
        if let Some(fallback) = default
            && Self::expr_ty(body, fallback) != yield_ty
        {
            return Err(SmeltError::unsupported(span, "next()'s default must have the generator's yield type"));
        }
        let bool_ty = self.intern_type(Type::Bool);
        let none_ty = self.intern_type(Type::None);
        let step_ty = self.intern_type(Type::GeneratorResult { yield_ty, return_ty });
        let block = body.push_block(span);
        let next = body.push_expr(HirExpr {
            kind: ExprKind::GeneratorNext { generator, value: sent, kind },
            ty: step_ty,
            span,
        });
        let step = body.push_local(self.synthetic_local("__smelt_step", step_ty, false, span));
        let step_pat = body.push_pattern(HirPattern::Binding(step));
        body.push_stmt_to_block(block, HirStmt::Let { pat: step_pat, ty: step_ty, value: Some(next) });
        let result = body.push_local(self.synthetic_local("__smelt_next", yield_ty, true, span));
        let result_pat = body.push_pattern(HirPattern::Binding(result));
        body.push_stmt_to_block(block, HirStmt::Let { pat: result_pat, ty: yield_ty, value: None });
        let step_read = body.push_expr(HirExpr { kind: ExprKind::Local(step), ty: step_ty, span });
        let done = body.push_expr(HirExpr { kind: ExprKind::GeneratorDone { result: step_read }, ty: bool_ty, span });
        let exhausted = body.push_block(span);
        if let Some(fallback) = default {
            let target = body.push_expr(HirExpr { kind: ExprKind::Local(result), ty: yield_ty, span });
            body.push_stmt_to_block(exhausted, HirStmt::Assign { target, value: fallback });
        } else {
            let mut args = Vec::new();
            if return_ty != none_ty {
                if !self.has_python_text(return_ty) {
                    return Err(SmeltError::unsupported(
                        span,
                        "the `StopIteration` an exhausted generator raises keeps its return value in `args`, so that value must have Python text",
                    ));
                }
                let step_value = body.push_expr(HirExpr { kind: ExprKind::Local(step), ty: step_ty, span });
                args.push(body.push_expr(HirExpr { kind: ExprKind::GeneratorValue { result: step_value }, ty: return_ty, span }));
            }
            let chain = builtin_exception_chain("StopIteration").unwrap_or_default();
            let classes = chain.iter().map(|class| self.intern_name(class)).collect();
            let exception_ty = self.py_exception_type();
            let exception = body.push_expr(HirExpr { kind: ExprKind::PyExceptionNew { classes, args }, ty: exception_ty, span });
            body.push_stmt_to_block(exhausted, HirStmt::Throw(exception));
        }
        let produced = body.push_block(span);
        let step_value = body.push_expr(HirExpr { kind: ExprKind::Local(step), ty: step_ty, span });
        let value = body.push_expr(HirExpr { kind: ExprKind::GeneratorValue { result: step_value }, ty: yield_ty, span });
        let target = body.push_expr(HirExpr { kind: ExprKind::Local(result), ty: yield_ty, span });
        body.push_stmt_to_block(produced, HirStmt::Assign { target, value });
        body.push_stmt_to_block(block, HirStmt::If { cond: done, then_block: exhausted, else_block: Some(produced) });
        let read = body.push_expr(HirExpr { kind: ExprKind::Local(result), ty: yield_ty, span });
        let block_index = usize::try_from(block.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[block_index].tail = Some(read);
        Ok(body.push_expr(HirExpr { kind: ExprKind::Block(block), ty: yield_ty, span }))
    }

    /// `g.send(v)`: `CPython` refuses a value other than `None` to a
    /// just-started generator with `TypeError`, leaving it unstarted, before
    /// resuming it once.
    ///
    /// ```text
    /// generator = g; value = v                   # evaluated once, in order
    /// if not started(generator) and value is not None:
    ///     raise TypeError("can't send non-None value to a just-started generator")
    /// resume(generator, value)
    /// ```
    ///
    /// The `value is not None` test is the static one when the send type
    /// decides it: never for a `None` send type, always for one without
    /// `None`, at run time for an optional one.
    fn generator_send_expression(
        &mut self,
        generator: smelt_hir::ExprId,
        sent: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let generator_ty = Self::expr_ty(body, generator);
        let sent_ty = Self::expr_ty(body, sent);
        let none_ty = self.intern_type(Type::None);
        if sent_ty == none_ty {
            return self.generator_resume_expression(generator, Some(sent), smelt_hir::GeneratorResumeKind::Next, None, span, body);
        }
        let bool_ty = self.intern_type(Type::Bool);
        let block = body.push_block(span);
        let handle = self.bind_synthetic(block, "__smelt_generator", generator, generator_ty, span, body);
        let value = self.bind_synthetic(block, "__smelt_sent", sent, sent_ty, span, body);
        let handle_read = body.push_expr(HirExpr { kind: ExprKind::Local(handle), ty: generator_ty, span });
        let started = body.push_expr(HirExpr { kind: ExprKind::GeneratorStarted { generator: handle_read }, ty: bool_ty, span });
        let mut refused = body.push_expr(HirExpr { kind: ExprKind::UnaryOp { op: UnaryOp::Not, operand: started }, ty: bool_ty, span });
        if matches!(self.ctx.krate.types.get(sent_ty), Some(Type::Optional(..))) {
            let value_read = body.push_expr(HirExpr { kind: ExprKind::Local(value), ty: sent_ty, span });
            let none = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::None), ty: none_ty, span });
            let present = self.compare_pair(CmpOp::IsNot, value_read, none, span, body)?;
            refused = self.short_circuit(BinOp::And, refused, present, span, body);
        }
        let raise = body.push_block(span);
        let error = self.builtin_exception_with_message("TypeError", "can't send non-None value to a just-started generator", span, body);
        body.push_stmt_to_block(raise, HirStmt::Throw(error));
        body.push_stmt_to_block(block, HirStmt::If { cond: refused, then_block: raise, else_block: None });
        let resumed_handle = body.push_expr(HirExpr { kind: ExprKind::Local(handle), ty: generator_ty, span });
        let value_read = body.push_expr(HirExpr { kind: ExprKind::Local(value), ty: sent_ty, span });
        let resumed = self.generator_resume_expression(resumed_handle, Some(value_read), smelt_hir::GeneratorResumeKind::Next, None, span, body)?;
        let yield_ty = Self::expr_ty(body, resumed);
        Self::set_block_tail(block, resumed, span, body)?;
        Ok(body.push_expr(HirExpr { kind: ExprKind::Block(block), ty: yield_ty, span }))
    }

    /// The exception `g.throw(exc)` raises in the generator: an exception
    /// instance, or a class, which Python instantiates with no arguments.
    fn thrown_into_generator(&mut self, thrown: &Expr, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(thrown.range());
        let value = match thrown {
            Expr::Name(name) if let Some(chain) = self.exception_class_chain(name.id.as_str()) => {
                self.exception_new_expr(&chain, &[], span, body)?
            }
            other => self.expression(other, body)?,
        };
        self.raised_exception(value, span, body)
            .ok_or_else(|| SmeltError::unsupported(span, "generator.throw() needs an exception class or instance"))
    }

    /// `g.close()`: raise `GeneratorExit` at the suspended `yield`, so the
    /// generator's `finally` blocks and `except GeneratorExit` handlers run.
    ///
    /// ```text
    /// try:
    ///     step = throw(g, GeneratorExit())
    ///     if step is not done: raise RuntimeError("generator ignored GeneratorExit")
    /// except BaseException as e:
    ///     if not isinstance(e, (GeneratorExit, StopIteration)): raise
    /// None
    /// ```
    ///
    /// A just-started or finished generator runs nothing: the runtime raises
    /// the injected `GeneratorExit` at once (`SmeltGenerator::try_resume`),
    /// or answers its completion.
    fn generator_close_expression(
        &mut self,
        generator: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let generator_ty = Self::expr_ty(body, generator);
        let Some(Type::Generator { is_async: false, yield_ty, return_ty, .. }) = self.ctx.krate.types.get(generator_ty).cloned() else {
            return Err(SmeltError::unsupported(span, "internal error: expected a generator"));
        };
        let bool_ty = self.intern_type(Type::Bool);
        let none_ty = self.intern_type(Type::None);
        let step_ty = self.intern_type(Type::GeneratorResult { yield_ty, return_ty });
        let exception_ty = self.py_exception_type();
        let block = body.push_block(span);
        let try_block = body.push_block(span);
        let exit = self.builtin_exception_with_args("GeneratorExit", Vec::new(), span, body);
        let next = body.push_expr(HirExpr {
            kind: ExprKind::GeneratorNext { generator, value: Some(exit), kind: smelt_hir::GeneratorResumeKind::Throw },
            ty: step_ty,
            span,
        });
        let step = self.bind_synthetic(try_block, "__smelt_step", next, step_ty, span, body);
        let step_read = body.push_expr(HirExpr { kind: ExprKind::Local(step), ty: step_ty, span });
        let done = body.push_expr(HirExpr { kind: ExprKind::GeneratorDone { result: step_read }, ty: bool_ty, span });
        let yielded = body.push_expr(HirExpr { kind: ExprKind::UnaryOp { op: UnaryOp::Not, operand: done }, ty: bool_ty, span });
        let ignored = body.push_block(span);
        let error = self.builtin_exception_with_message("RuntimeError", "generator ignored GeneratorExit", span, body);
        body.push_stmt_to_block(ignored, HirStmt::Throw(error));
        body.push_stmt_to_block(try_block, HirStmt::If { cond: yielded, then_block: ignored, else_block: None });
        let caught = body.push_local(self.synthetic_local("__smelt_exception", exception_ty, false, span));
        let catch_block = body.push_block(span);
        let caught_read = body.push_expr(HirExpr { kind: ExprKind::Local(caught), ty: exception_ty, span });
        let classes = ["GeneratorExit", "StopIteration"].into_iter().map(|class| self.intern_name(class)).collect();
        let ends_close = body.push_expr(HirExpr {
            kind: ExprKind::PyExceptionCaughtBy { exception: caught_read, classes, catches_unclassified: false },
            ty: bool_ty,
            span,
        });
        let propagates = body.push_expr(HirExpr { kind: ExprKind::UnaryOp { op: UnaryOp::Not, operand: ends_close }, ty: bool_ty, span });
        let rethrow = body.push_block(span);
        let rethrown = body.push_expr(HirExpr { kind: ExprKind::Local(caught), ty: exception_ty, span });
        body.push_stmt_to_block(rethrow, HirStmt::Throw(rethrown));
        body.push_stmt_to_block(catch_block, HirStmt::If { cond: propagates, then_block: rethrow, else_block: None });
        body.push_stmt_to_block(
            block,
            HirStmt::TryCatch { body: try_block, catch_binding: Some(caught), catch_body: Some(catch_block), finally_body: None },
        );
        let none = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::None), ty: none_ty, span });
        Self::set_block_tail(block, none, span, body)?;
        Ok(body.push_expr(HirExpr { kind: ExprKind::Block(block), ty: none_ty, span }))
    }

    /// Bind `value` to a fresh synthetic local in `block`, returning it.
    #[expect(clippy::too_many_arguments, reason = "one binding, its value, type and lowering position")]
    fn bind_synthetic(
        &mut self,
        block: smelt_hir::BlockId,
        name: &str,
        value: smelt_hir::ExprId,
        ty: TypeId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::LocalId {
        let local = body.push_local(self.synthetic_local(name, ty, false, span));
        let pat = body.push_pattern(HirPattern::Binding(local));
        body.push_stmt_to_block(block, HirStmt::Let { pat, ty, value: Some(value) });
        local
    }

    /// Make `tail` the value of `block`.
    fn set_block_tail(block: smelt_hir::BlockId, tail: smelt_hir::ExprId, span: Span, body: &mut Body) -> Result<(), SmeltError> {
        let index = usize::try_from(block.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        let Some(slot) = body.blocks.get_mut(index) else {
            return Err(SmeltError::unsupported(span, "internal error: missing block"));
        };
        slot.tail = Some(tail);
        Ok(())
    }

    /// A builtin exception `name` whose one argument is `message`.
    fn builtin_exception_with_message(&mut self, name: &str, message: &str, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        let str_ty = self.intern_type(Type::PyStr);
        let text = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::String(message.to_owned())), ty: str_ty, span });
        self.builtin_exception_with_args(name, vec![text], span, body)
    }

    /// A builtin exception `name` built from `args` (each kept as its Python
    /// text).
    fn builtin_exception_with_args(&mut self, name: &str, args: Vec<smelt_hir::ExprId>, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        let chain = builtin_exception_chain(name).unwrap_or_default();
        let classes = chain.iter().map(|class| self.intern_name(class)).collect();
        let ty = self.py_exception_type();
        body.push_expr(HirExpr { kind: ExprKind::PyExceptionNew { classes, args }, ty, span })
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
