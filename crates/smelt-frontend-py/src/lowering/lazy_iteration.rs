// Lazy consumers of a generator object: `any`, `all`, `enumerate`, `zip`.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// CPython's builtins pull from an iterator one value at a time and stop as
// soon as they can, so a generator's effects interleave with the consumer's
// and the values never asked for are never computed. Draining the generator
// into a list first would run its body to the end, so a generator object is
// consumed through resumptions (`ExprKind::GeneratorNext`) instead:
//
// - `any(g)` / `all(g)` is a loop that resumes `g` until a value decides the
//   answer (Python's truth test of each value).
// - `enumerate(g, start)` and `zip(a, b)` (either operand a generator) answer
//   a new generator, an immediately called synthesized generator closure
//   whose body pulls from its sources in CPython's order. Whatever consumes it
//   (a `for` loop, `next`, `list`, another `zip`) resumes it, and so its
//   sources, one step at a time.
//
// `zip` pulls its first operand, then its second, each round, and stops at the
// first one exhausted: a generator first operand is pulled once more after a
// shorter list second operand runs out (that value is lost), exactly as
// CPython's `zip` does.

impl ModuleBuilder<'_> {
    /// Resume the generator in `generator` (a local of `generator_ty`) once,
    /// in `block`: when it is exhausted `exhausted` runs (a `break` or a
    /// `return`), otherwise the answer reads the value it yielded.
    #[expect(clippy::too_many_arguments, reason = "one resumption, its source and its lowering position")]
    fn pull_generator(
        &mut self,
        generator: smelt_hir::LocalId,
        generator_ty: TypeId,
        exhausted: HirStmt,
        span: Span,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(Type::Generator { is_async: false, yield_ty, return_ty, .. }) = self.ctx.krate.types.get(generator_ty).cloned() else {
            return Err(SmeltError::unsupported(span, "internal error: expected a generator"));
        };
        let bool_ty = self.intern_type(Type::Bool);
        let step_ty = self.intern_type(Type::GeneratorResult { yield_ty, return_ty });
        let handle = body.push_expr(HirExpr { kind: ExprKind::Local(generator), ty: generator_ty, span });
        let next = body.push_expr(HirExpr {
            kind: ExprKind::GeneratorNext { generator: handle, value: None, kind: smelt_hir::GeneratorResumeKind::Next },
            ty: step_ty,
            span,
        });
        let step = body.push_local(self.synthetic_local("__smelt_step", step_ty, false, span));
        let step_pat = body.push_pattern(HirPattern::Binding(step));
        body.push_stmt_to_block(block, HirStmt::Let { pat: step_pat, ty: step_ty, value: Some(next) });
        let step_read = body.push_expr(HirExpr { kind: ExprKind::Local(step), ty: step_ty, span });
        let done = body.push_expr(HirExpr { kind: ExprKind::GeneratorDone { result: step_read }, ty: bool_ty, span });
        let exhausted_block = body.push_block(span);
        body.push_stmt_to_block(exhausted_block, exhausted);
        body.push_stmt_to_block(block, HirStmt::If { cond: done, then_block: exhausted_block, else_block: None });
        let step_value = body.push_expr(HirExpr { kind: ExprKind::Local(step), ty: step_ty, span });
        let value = body.push_expr(HirExpr { kind: ExprKind::GeneratorValue { result: step_value }, ty: yield_ty, span });
        let item = body.push_local(self.synthetic_local("__smelt_item", yield_ty, false, span));
        let item_pat = body.push_pattern(HirPattern::Binding(item));
        body.push_stmt_to_block(block, HirStmt::Let { pat: item_pat, ty: yield_ty, value: Some(value) });
        Ok(body.push_expr(HirExpr { kind: ExprKind::Local(item), ty: yield_ty, span }))
    }

    /// `any(g)` / `all(g)` of a generator object, resumed until a value
    /// decides:
    ///
    /// ```text
    /// result = False                 # True for all()
    /// while True:
    ///     item = next(g) or break
    ///     if item:                   # `not item` for all(); Python's truth test
    ///         result = True          # False for all()
    ///         break
    /// result
    /// ```
    fn lazy_bool_fold(
        &mut self,
        op: smelt_hir::BoolFoldOp,
        generator: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let generator_ty = Self::expr_ty(body, generator);
        let bool_ty = self.intern_type(Type::Bool);
        let outer = body.push_block(span);
        let handle = body.push_local(self.synthetic_local("__smelt_generator", generator_ty, false, span));
        let handle_pat = body.push_pattern(HirPattern::Binding(handle));
        body.push_stmt_to_block(outer, HirStmt::Let { pat: handle_pat, ty: generator_ty, value: Some(generator) });
        let seed = op == smelt_hir::BoolFoldOp::All;
        let result = body.push_local(self.synthetic_local("__smelt_fold", bool_ty, true, span));
        let result_pat = body.push_pattern(HirPattern::Binding(result));
        let seed_expr = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(seed)), ty: bool_ty, span });
        body.push_stmt_to_block(outer, HirStmt::Let { pat: result_pat, ty: bool_ty, value: Some(seed_expr) });
        let loop_block = body.push_block(span);
        let item = self.pull_generator(handle, generator_ty, HirStmt::Break, span, body, loop_block)?;
        let truth = self.python_truth(item, span, body)?;
        let decides = if seed {
            body.push_expr(HirExpr { kind: ExprKind::UnaryOp { op: UnaryOp::Not, operand: truth }, ty: bool_ty, span })
        } else {
            truth
        };
        let deciding = body.push_block(span);
        let target = body.push_expr(HirExpr { kind: ExprKind::Local(result), ty: bool_ty, span });
        let answer = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(!seed)), ty: bool_ty, span });
        body.push_stmt_to_block(deciding, HirStmt::Assign { target, value: answer });
        body.push_stmt_to_block(deciding, HirStmt::Break);
        body.push_stmt_to_block(loop_block, HirStmt::If { cond: decides, then_block: deciding, else_block: None });
        let forever = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(true)), ty: bool_ty, span });
        body.push_stmt_to_block(outer, HirStmt::While { cond: forever, body: loop_block });
        let tail = body.push_expr(HirExpr { kind: ExprKind::Local(result), ty: bool_ty, span });
        let outer_index = usize::try_from(outer.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[outer_index].tail = Some(tail);
        Ok(body.push_expr(HirExpr { kind: ExprKind::Block(outer), ty: bool_ty, span }))
    }

    /// `enumerate(g, start)` of a generator object: the generator
    ///
    /// ```text
    /// def __smelt_enumerate(it, start):
    ///     index = start
    ///     while True:
    ///         item = next(it) or return
    ///         yield (index, item)
    ///         index = index + 1
    /// ```
    ///
    /// called with `g` and `start` (`0` when absent).
    fn lazy_enumerate(
        &mut self,
        generator: smelt_hir::ExprId,
        start: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let generator_ty = Self::expr_ty(body, generator);
        let Some(Type::Generator { yield_ty, .. }) = self.ctx.krate.types.get(generator_ty).cloned() else {
            return Err(SmeltError::unsupported(span, "internal error: expected a generator"));
        };
        let int_ty = self.intern_type(Type::PyInt);
        let bool_ty = self.intern_type(Type::Bool);
        let none_ty = self.intern_type(Type::None);
        let pair_ty = self.intern_type(Type::Tuple(vec![int_ty, yield_ty]));
        let result_ty = self.intern_type(Type::Generator { is_async: false, yield_ty: pair_ty, return_ty: none_ty, next_ty: none_ty });
        let mut adapter = Body::new(None, span);
        adapter.set_generator(true);
        let params = self.adapter_params(&mut adapter, &[("__smelt_iterator", generator_ty), ("__smelt_start", int_ty)], span);
        let (Some(iterator), Some(start_param)) = (params.first().map(|param| param.local), params.get(1).map(|param| param.local)) else {
            return Err(SmeltError::unsupported(span, "internal error: enumerate adapter parameters"));
        };
        let index = adapter.push_local(self.synthetic_local("__smelt_index", int_ty, true, span));
        let index_pat = adapter.push_pattern(HirPattern::Binding(index));
        let start_read = adapter.push_expr(HirExpr { kind: ExprKind::Local(start_param), ty: int_ty, span });
        adapter.push_stmt(HirStmt::Let { pat: index_pat, ty: int_ty, value: Some(start_read) });
        let loop_block = adapter.push_block(span);
        let item = self.pull_generator(iterator, generator_ty, HirStmt::Return(None), span, &mut adapter, loop_block)?;
        let index_read = adapter.push_expr(HirExpr { kind: ExprKind::Local(index), ty: int_ty, span });
        let pair = adapter.push_expr(HirExpr { kind: ExprKind::TupleLit(vec![index_read, item]), ty: pair_ty, span });
        let suspend = adapter.push_expr(HirExpr { kind: ExprKind::GeneratorYield { value: pair }, ty: none_ty, span });
        adapter.push_stmt_to_block(loop_block, HirStmt::Expr(suspend));
        let current = adapter.push_expr(HirExpr { kind: ExprKind::Local(index), ty: int_ty, span });
        let one = adapter.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Int(1)), ty: int_ty, span });
        let next_index = self.python_arithmetic(PythonArithmetic::Plain(BinOp::Add), current, one, span, &mut adapter)?;
        let target = adapter.push_expr(HirExpr { kind: ExprKind::Local(index), ty: int_ty, span });
        adapter.push_stmt_to_block(loop_block, HirStmt::Assign { target, value: next_index });
        let forever = adapter.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(true)), ty: bool_ty, span });
        adapter.push_stmt(HirStmt::While { cond: forever, body: loop_block });
        Ok(self.call_adapter(adapter, params, vec![generator, start], result_ty, span, body))
    }

    /// `zip(left, right)` where an operand is a generator object (the other a
    /// generator or a list): the generator pulling from both in `CPython`'s
    /// order (see the file header), called with the two operands.
    fn lazy_zip(
        &mut self,
        left: smelt_hir::ExprId,
        right: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let left_ty = Self::expr_ty(body, left);
        let right_ty = Self::expr_ty(body, right);
        let item_of = |this: &Self, ty: TypeId| match this.ctx.krate.types.get(ty) {
            Some(Type::Generator { is_async: false, yield_ty, .. }) => Some((*yield_ty, true)),
            Some(Type::List(item)) => Some((*item, false)),
            _ => None,
        };
        let (Some((left_item, left_lazy)), Some((right_item, right_lazy))) = (item_of(self, left_ty), item_of(self, right_ty)) else {
            return Err(SmeltError::unsupported(span, "zip() of a generator takes a generator or a list, set, dict or str"));
        };
        let bool_ty = self.intern_type(Type::Bool);
        let none_ty = self.intern_type(Type::None);
        let pair_ty = self.intern_type(Type::Tuple(vec![left_item, right_item]));
        let result_ty = self.intern_type(Type::Generator { is_async: false, yield_ty: pair_ty, return_ty: none_ty, next_ty: none_ty });
        let mut adapter = Body::new(None, span);
        adapter.set_generator(true);
        let params = self.adapter_params(&mut adapter, &[("__smelt_left", left_ty), ("__smelt_right", right_ty)], span);
        let (Some(left_param), Some(right_param)) = (params.first().map(|param| param.local), params.get(1).map(|param| param.local)) else {
            return Err(SmeltError::unsupported(span, "internal error: zip adapter parameters"));
        };
        let loop_block = adapter.push_block(span);
        // A list operand is the loop's own iteration; a generator is pulled
        // inside it.
        let (left_value, right_value) = match (left_lazy, right_lazy) {
            (true, true) => {
                let left_value = self.pull_generator(left_param, left_ty, HirStmt::Return(None), span, &mut adapter, loop_block)?;
                let right_value = self.pull_generator(right_param, right_ty, HirStmt::Return(None), span, &mut adapter, loop_block)?;
                (left_value, right_value)
            }
            (false, true) => {
                let item = adapter.push_local(self.synthetic_local("__smelt_left_item", left_item, false, span));
                let left_value = adapter.push_expr(HirExpr { kind: ExprKind::Local(item), ty: left_item, span });
                let right_value = self.pull_generator(right_param, right_ty, HirStmt::Return(None), span, &mut adapter, loop_block)?;
                let pair = adapter.push_expr(HirExpr { kind: ExprKind::TupleLit(vec![left_value, right_value]), ty: pair_ty, span });
                let suspend = adapter.push_expr(HirExpr { kind: ExprKind::GeneratorYield { value: pair }, ty: none_ty, span });
                adapter.push_stmt_to_block(loop_block, HirStmt::Expr(suspend));
                let pat = adapter.push_pattern(HirPattern::Binding(item));
                let iter = adapter.push_expr(HirExpr { kind: ExprKind::Local(left_param), ty: left_ty, span });
                adapter.push_stmt(HirStmt::For { pat, iter, body: loop_block });
                return Ok(self.call_adapter(adapter, params, vec![left, right], result_ty, span, body));
            }
            (true, false) => {
                // `for item in right: left = next(left) or return; yield ...`,
                // then one more pull of the left generator: CPython pulls it
                // before finding the right list exhausted (that value is lost).
                let item = adapter.push_local(self.synthetic_local("__smelt_right_item", right_item, false, span));
                let left_value = self.pull_generator(left_param, left_ty, HirStmt::Return(None), span, &mut adapter, loop_block)?;
                let right_value = adapter.push_expr(HirExpr { kind: ExprKind::Local(item), ty: right_item, span });
                let pair = adapter.push_expr(HirExpr { kind: ExprKind::TupleLit(vec![left_value, right_value]), ty: pair_ty, span });
                let suspend = adapter.push_expr(HirExpr { kind: ExprKind::GeneratorYield { value: pair }, ty: none_ty, span });
                adapter.push_stmt_to_block(loop_block, HirStmt::Expr(suspend));
                let pat = adapter.push_pattern(HirPattern::Binding(item));
                let iter = adapter.push_expr(HirExpr { kind: ExprKind::Local(right_param), ty: right_ty, span });
                adapter.push_stmt(HirStmt::For { pat, iter, body: loop_block });
                let root = adapter.root;
                let _lost = self.pull_generator(left_param, left_ty, HirStmt::Return(None), span, &mut adapter, root)?;
                return Ok(self.call_adapter(adapter, params, vec![left, right], result_ty, span, body));
            }
            (false, false) => {
                return Err(SmeltError::unsupported(span, "internal error: zip of two lists is not lazy"));
            }
        };
        let pair = adapter.push_expr(HirExpr { kind: ExprKind::TupleLit(vec![left_value, right_value]), ty: pair_ty, span });
        let suspend = adapter.push_expr(HirExpr { kind: ExprKind::GeneratorYield { value: pair }, ty: none_ty, span });
        adapter.push_stmt_to_block(loop_block, HirStmt::Expr(suspend));
        let forever = adapter.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(true)), ty: bool_ty, span });
        adapter.push_stmt(HirStmt::While { cond: forever, body: loop_block });
        Ok(self.call_adapter(adapter, params, vec![left, right], result_ty, span, body))
    }

    /// Declare a synthesized adapter's parameters, in order.
    fn adapter_params(&mut self, adapter: &mut Body, specs: &[(&str, TypeId)], span: Span) -> Vec<Param> {
        specs
            .iter()
            .map(|&(name, ty)| {
                let symbol = self.intern_synthesized_name(name);
                let local = adapter.push_local(LocalDecl { name: Some(symbol), ty, mutable: false, span });
                adapter.params.push(local);
                Param { name: symbol, local, ty, span }
            })
            .collect()
    }

    /// The call of the synthesized generator closure `adapter` with `args`,
    /// answering the generator of type `generator_ty`. It captures nothing:
    /// its sources are its parameters.
    #[expect(clippy::too_many_arguments, reason = "the adapter, its signature and arguments, and its lowering position")]
    fn call_adapter(
        &mut self,
        adapter: Body,
        params: Vec<Param>,
        args: Vec<smelt_hir::ExprId>,
        generator_ty: TypeId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let param_tys = params.iter().map(|param| param.ty).collect::<Vec<_>>();
        let body_id = self.ctx.krate.push_body(adapter);
        let closure_ty = self.intern_type(Type::Function(FunctionType {
            params: param_tys,
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: generator_ty,
            is_async: false,
            may_throw: false,
        }));
        let closure = body.push_expr(HirExpr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params,
                rest: None,
                required_params: None,
                return_ty: generator_ty,
                captures: Vec::new(),
                body: body_id,
                name: None,
                length: None,
                function_item: None,
                this_binding: smelt_hir::ClosureThis::Own,
                span,
            }),
            ty: closure_ty,
            span,
        });
        body.push_expr(HirExpr { kind: ExprKind::ClosureCall { callee: closure, args }, ty: generator_ty, span })
    }
}
