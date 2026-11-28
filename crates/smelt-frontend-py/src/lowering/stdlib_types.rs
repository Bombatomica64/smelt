impl ModuleBuilder<'_> {
    /// Return whether a type can be represented by Python JSON serialization helpers.
    fn is_json_serializable_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Bool | Type::PyInt | Type::Float | Type::PyStr) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item, _)) => {
                self.is_json_serializable_type(*item)
            }
            Some(Type::Tuple(items)) => items
                .iter()
                .all(|item| self.is_json_serializable_type(*item)),
            Some(Type::Dict(key, value)) => {
                matches!(self.ctx.krate.types.get(*key), Some(Type::PyStr))
                    && self.is_json_serializable_type(*value)
            }
            _ => false,
        }
    }

    /// Whether a tuple appears anywhere in the JSON-compatible type `ty`.
    fn contains_tuple_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Tuple(_)) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item, _) | Type::Dict(_, item)) => {
                self.contains_tuple_type(*item)
            }
            _ => false,
        }
    }

    /// Whether a set appears anywhere in the JSON-compatible type `ty`.
    fn contains_set_type(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Set(_)) => true,
            Some(Type::List(item) | Type::Optional(item, _)) => self.contains_set_type(*item),
            Some(Type::Tuple(items)) => items.iter().any(|item| self.contains_set_type(*item)),
            Some(Type::Dict(_, value)) => self.contains_set_type(*value),
            _ => false,
        }
    }

    /// Lower primitive Python conversion builtins for bool, int, float, and string values.
    pub(super) fn primitive_cast_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        let (op, result_ty) = match name.id.as_str() {
            "bool" => (PrimitiveCastOp::ToBool, Type::Bool),
            "int" => (PrimitiveCastOp::ToInt, Type::PyInt),
            "float" => (PrimitiveCastOp::ToFloat, Type::Float),
            "str" => (PrimitiveCastOp::ToString, Type::PyStr),
            _ => return Ok(None),
        };
        let span = self.span(call.range());
        if !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "primitive conversions take positional arguments only",
            ));
        }
        // `int(text, base)`: the base form only reads a string.
        if let ([text_arg, base_arg], PrimitiveCastOp::ToInt) = (call.arguments.args.as_ref(), op) {
            let text = self.expression(text_arg, body)?;
            let base = self.expression(base_arg, body)?;
            if !matches!(self.ctx.krate.types.get(Self::expr_ty(body, text)), Some(Type::PyStr))
                || !matches!(self.ctx.krate.types.get(Self::expr_ty(body, base)), Some(Type::PyInt))
            {
                return Err(SmeltError::unsupported(span, "int(x, base) needs a str and an int base"));
            }
            return Ok(Some(self.py_numeric_call(smelt_hir::PyNumericOp::ParseBase, vec![text, base], span, body)));
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                span,
                "primitive conversions currently support exactly one positional argument",
            ));
        }
        let operand = self.expression(&call.arguments.args[0], body)?;
        let operand_kind = self.ctx.krate.types.get(Self::expr_ty(body, operand)).cloned();
        if op == PrimitiveCastOp::ToString
            && !matches!(operand_kind, Some(Type::Bool | Type::PyInt | Type::Float | Type::PyStr))
        {
            return self.python_text_expr(operand, false, span, body).map(Some);
        }
        // `bool(x)` is `x`'s truth test, whatever its type.
        if op == PrimitiveCastOp::ToBool {
            return self.python_truth(operand, span, body).map(Some);
        }
        if !matches!(operand_kind, Some(Type::Bool | Type::PyInt | Type::Float | Type::PyStr)) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "primitive conversions currently support bool, int, float, and str values",
            ));
        }
        // The conversions that can raise (`int("x")`, `int(nan)`,
        // `float(2**1024)`, `float("x")`, `str(10**5000)`) are numeric calls a
        // `try` can catch; an `int` already is one; the rest are plain casts.
        let raising = match (op, &operand_kind) {
            (PrimitiveCastOp::ToInt, Some(Type::PyInt)) => return Ok(Some(operand)),
            (PrimitiveCastOp::ToInt, Some(Type::PyStr)) => Some(smelt_hir::PyNumericOp::Parse),
            (PrimitiveCastOp::ToInt, Some(Type::Float)) => Some(smelt_hir::PyNumericOp::FromFloat),
            (PrimitiveCastOp::ToFloat, Some(Type::PyInt)) => return Ok(Some(self.promote_int_to_float(operand, body))),
            (PrimitiveCastOp::ToFloat, Some(Type::PyStr)) => Some(smelt_hir::PyNumericOp::ParseFloat),
            _ => None,
        };
        if let Some(raising_op) = raising {
            return Ok(Some(self.py_numeric_call(raising_op, vec![operand], span, body)));
        }
        if op == PrimitiveCastOp::ToInt && matches!(operand_kind, Some(Type::Bool)) {
            let ty = self.intern_type(Type::PyInt);
            return Ok(Some(body.push_expr(HirExpr { kind: ExprKind::TypeAssert { value: operand }, ty, span })));
        }
        let ty = self.intern_type(result_ty);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::PrimitiveCast { op, operand },
            ty,
            span,
        })))
    }

    /// Lower Python `sum(values)` calls for int and float lists.
    pub(super) fn numeric_sum_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "sum" {
            return Ok(None);
        }
        // `sum(values, start)` / `sum(values, start=s)`: `start` is added
        // first, so the total is the sum of `[start, *values]` in that order
        // (which a float total depends on).
        let start_ast = match (call.arguments.args.as_ref(), call.arguments.keywords.as_ref()) {
            ([_], []) => None,
            ([_, start], []) => Some(start),
            ([_], [keyword]) if keyword.arg.as_ref().is_some_and(|arg| arg.as_str() == "start") => Some(&keyword.value),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.range()),
                    "sum() takes one list argument and an optional start",
                ));
            }
        };
        let iterable = self.expression(&call.arguments.args[0], body)?;
        let mut list = self.order_free_iterable_as_list(iterable, self.span(call.arguments.args[0].range()), body);
        if let Some(start_expr) = start_ast {
            let list_ty = Self::expr_ty(body, list);
            let item_hint = self.list_element_type(list_ty);
            let start = self.expression_with_hint(start_expr, body, item_hint)?;
            if Some(Self::expr_ty(body, start)) != item_hint {
                return Err(SmeltError::unsupported(
                    self.span(start_expr.range()),
                    "sum()'s start must have the list's element type",
                ));
            }
            let span = self.span(call.range());
            let head = body.push_expr(HirExpr { kind: ExprKind::ListLit(vec![start]), ty: list_ty, span });
            list = body.push_expr(HirExpr { kind: ExprKind::ListConcat { left: head, right: list }, ty: list_ty, span });
        }
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "sum() argument must be an int or float list",
            ));
        };
        if !matches!(
            self.ctx.krate.types.get(*item_ty),
            Some(Type::PyInt | Type::Float)
        ) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "sum() argument must be an int or float list",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListSum { list },
            ty: *item_ty,
            span: self.span(call.range()),
        })))
    }

    /// Lower Python `all(values)` and `any(values)` calls for boolean lists.
    pub(super) fn bool_fold_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        let op = match name.id.as_str() {
            "all" => BoolFoldOp::All,
            "any" => BoolFoldOp::Any,
            _ => return Ok(None),
        };
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "all() and any() currently support exactly one bool list argument",
            ));
        }
        // Over a generator expression, the fold stops at the first deciding
        // element, as Python's lazy generator does: later elements (and their
        // side effects) are never computed.
        if let Expr::Generator(generator) = &call.arguments.args[0]
            && let [clause] = generator.generators.as_slice()
            && !clause.is_async
        {
            return self
                .with_block_scope(|this| this.short_circuit_fold(op, generator, clause, body))
                .map(Some);
        }
        let iterable = self.expression(&call.arguments.args[0], body)?;
        // A generator object is resumed until a value decides; draining it
        // first would run the rest (`lowering/lazy_iteration.rs`).
        if matches!(self.ctx.krate.types.get(Self::expr_ty(body, iterable)), Some(Type::Generator { is_async: false, .. })) {
            return self.lazy_bool_fold(op, iterable, self.span(call.range()), body).map(Some);
        }
        let list = self.order_free_iterable_as_list(iterable, self.span(call.arguments.args[0].range()), body);
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "all() and any() argument must be a bool list",
            ));
        };
        if self.ctx.krate.types.get(*item_ty) != Some(&Type::Bool) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "all() and any() argument must be a bool list",
            ));
        }
        let ty = self.intern_type(Type::Bool);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListBoolFold { op, list },
            ty,
            span: self.span(call.range()),
        })))
    }

    /// `any(elt for t in it if c)` / `all(...)` over one `for` clause, as the
    /// loop that consumes the generator lazily:
    ///
    /// ```text
    /// result = False            # True for all()
    /// for t in it:
    ///     if c:                  # each `if` clause, nested
    ///         if elt:            # `not elt` for all(); Python's truth test
    ///             result = True  # False for all()
    ///             break
    /// result
    /// ```
    fn short_circuit_fold(
        &mut self,
        op: BoolFoldOp,
        generator: &ruff_python_ast::ExprGenerator,
        clause: &ruff_python_ast::Comprehension,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(generator.range);
        let bool_ty = self.intern_type(Type::Bool);
        let raw_iter = self.expression(&clause.iter, body)?;
        let iter = self.iterable_as_list(raw_iter, body);
        let Some(item_ty) = self.iter_item_type(Self::expr_ty(body, iter)) else {
            return Err(SmeltError::unsupported(
                self.span(clause.iter.range()),
                "a generator expression can only iterate over a list, set, dict, or string",
            ));
        };
        let pat = self.binding_pattern_from_target(&clause.target, body, Some(item_ty))?;
        let outer = body.push_block(span);
        let seed = op == BoolFoldOp::All;
        let result = body.push_local(self.synthetic_local("__smelt_fold", bool_ty, true, span));
        let result_pat = body.push_pattern(smelt_hir::Pattern::Binding(result));
        let seed_expr = body.push_expr(HirExpr { kind: ExprKind::Literal(smelt_hir::Literal::Bool(seed)), ty: bool_ty, span });
        body.push_stmt_to_block(outer, smelt_hir::Stmt::Let { pat: result_pat, ty: bool_ty, value: Some(seed_expr) });
        let loop_block = body.push_block(span);
        let mut current = loop_block;
        for condition in &clause.ifs {
            let test = self.python_condition(condition, body)?;
            let passed = body.push_block(span);
            body.push_stmt_to_block(current, smelt_hir::Stmt::If { cond: test, then_block: passed, else_block: None });
            current = passed;
        }
        let element = self.python_condition(&generator.elt, body)?;
        let decides = if seed {
            body.push_expr(HirExpr { kind: ExprKind::UnaryOp { op: smelt_hir::UnaryOp::Not, operand: element }, ty: bool_ty, span })
        } else {
            element
        };
        let deciding_block = body.push_block(span);
        let target = body.push_expr(HirExpr { kind: ExprKind::Local(result), ty: bool_ty, span });
        let answer = body.push_expr(HirExpr { kind: ExprKind::Literal(smelt_hir::Literal::Bool(!seed)), ty: bool_ty, span });
        body.push_stmt_to_block(deciding_block, smelt_hir::Stmt::Assign { target, value: answer });
        body.push_stmt_to_block(deciding_block, smelt_hir::Stmt::Break);
        body.push_stmt_to_block(current, smelt_hir::Stmt::If { cond: decides, then_block: deciding_block, else_block: None });
        body.push_stmt_to_block(outer, smelt_hir::Stmt::For { pat, iter, body: loop_block });
        let tail = body.push_expr(HirExpr { kind: ExprKind::Local(result), ty: bool_ty, span });
        let outer_index = usize::try_from(outer.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[outer_index].tail = Some(tail);
        Ok(body.push_expr(HirExpr { kind: ExprKind::Block(outer), ty: bool_ty, span }))
    }

    /// Lower Python `sorted(values, key=..., reverse=...)` for sortable lists.
    ///
    /// Lists of scalar items sort directly; an optional `key=` lambda or local
    /// callable sorts by its mapped value, and `reverse=True` produces a
    /// descending order. This mirrors the TypeScript `Array.prototype.sort`
    /// comparator support so the two frontends keep parity.
    pub(super) fn sorted_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "sorted" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "sorted() currently supports exactly one list argument",
            ));
        }
        let iterable = self.expression(&call.arguments.args[0], body)?;
        let list = self.order_free_iterable_as_list(iterable, self.span(call.arguments.args[0].range()), body);
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "sorted() argument must be a sortable list",
            ));
        };
        let element_ty = *item_ty;
        let (key, reverse) =
            self.sort_keyword_arguments(&call.arguments.keywords, element_ty, body)?;
        if key.is_none() && !self.is_python_orderable(element_ty) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "sorted() argument must be a sortable list",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListSorted {
                list,
                key,
                reverse,
            },
            ty: list_ty,
            span: self.span(call.range()),
        })))
    }

    /// Whether Python orders values of `ty` with `<` and Smelt sorts them: the
    /// scalars, and tuples of orderable items, which compare lexicographically
    /// (as Rust's tuples do).
    pub(super) fn is_python_orderable(&self, ty: smelt_hir::TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Bool | Type::PyInt | Type::Float | Type::PyStr) => true,
            Some(Type::Tuple(items)) => !items.is_empty() && items.iter().all(|item| self.is_python_orderable(*item)),
            _ => false,
        }
    }

    /// Parse Python `key=` and `reverse=` sort keyword arguments.
    ///
    /// A `key` lambda or local callable lowers to a closure mapping one list
    /// item to an orderable sort value (a scalar or a tuple of orderable
    /// values); `key=None` is treated as absent. `reverse`
    /// accepts only boolean literals because non-literal flags would need
    /// runtime branching that the sort lowering does not model yet.
    fn sort_keyword_arguments(
        &mut self,
        keywords: &[ruff_python_ast::Keyword],
        element_ty: smelt_hir::TypeId,
        body: &mut Body,
    ) -> Result<(Option<smelt_hir::ExprId>, bool), SmeltError> {
        let mut key = None;
        let mut reverse = false;
        for keyword in keywords {
            match keyword.arg.as_ref().map(ruff_python_ast::Identifier::as_str) {
                Some("key") => {
                    if matches!(&keyword.value, Expr::NoneLiteral(_)) {
                        continue;
                    }
                    let callback =
                        self.python_callback_argument(&keyword.value, &[element_ty], body)?;
                    // A tuple key (`key=lambda p: (p.a, -p.b)`) orders
                    // lexicographically, as Python's tuples do.
                    if !self.is_python_orderable(callback.return_ty) {
                        return Err(SmeltError::unsupported(
                            self.span(keyword.range),
                            "sort key must return bool, int, float, str, or a tuple of them",
                        ));
                    }
                    key = Some(callback.expr);
                }
                Some("reverse") => match &keyword.value {
                    Expr::BooleanLiteral(value) => reverse = value.value,
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(keyword.range),
                            "sort reverse must be a boolean literal",
                        ));
                    }
                },
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(keyword.range),
                        "sort supports only key and reverse keyword arguments",
                    ));
                }
            }
        }
        Ok((key, reverse))
    }

    /// Lower Python `reversed(values)` calls for list values.
    pub(super) fn reversed_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "reversed" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "reversed() currently supports exactly one list argument and no keywords",
            ));
        }
        let list = self.expression(&call.arguments.args[0], body)?;
        let list_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(list_ty), Some(Type::List(_))) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "reversed() argument must be a list",
            ));
        }
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListReversed { list },
            ty: list_ty,
            span: self.span(call.range()),
        })))
    }

    /// Lower Python `enumerate(values)` as a materialized list of `(index, value)` tuples.
    pub(super) fn enumerate_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "enumerate" {
            return Ok(None);
        }
        // `enumerate(xs)`, `enumerate(xs, start)` or `enumerate(xs, start=n)`.
        let start_arg = match (call.arguments.args.as_ref(), call.arguments.keywords.as_ref()) {
            ([_], []) => None,
            ([_, start], []) => Some(start),
            ([_], [keyword]) if keyword.arg.as_ref().is_some_and(|arg| arg.as_str() == "start") => Some(&keyword.value),
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.range()),
                    "enumerate() takes one iterable and an optional start",
                ));
            }
        };
        let raw_iter = self.expression(&call.arguments.args[0], body)?;
        if matches!(self.ctx.krate.types.get(Self::expr_ty(body, raw_iter)), Some(Type::Generator { is_async: false, .. })) {
            // A lazy generator of the pairs (`lowering/lazy_iteration.rs`).
            let span = self.span(call.range());
            let int_ty = self.intern_type(Type::PyInt);
            let start = match start_arg {
                Some(start_expr) => {
                    let lowered = self.expression(start_expr, body)?;
                    if Self::expr_ty(body, lowered) != int_ty {
                        return Err(SmeltError::unsupported(self.span(start_expr.range()), "enumerate() start must be an int"));
                    }
                    lowered
                }
                None => body.push_expr(HirExpr { kind: ExprKind::Literal(smelt_hir::Literal::Int(0)), ty: int_ty, span }),
            };
            return self.lazy_enumerate(raw_iter, start, span, body).map(Some);
        }
        let list = self.iterable_as_list(raw_iter, body);
        let list_ty = Self::expr_ty(body, list);
        let item_ty = if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(list_ty) {
            *item_ty
        } else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "enumerate() argument must be a list, set, dict or str",
            ));
        };
        let int_ty = self.intern_type(Type::PyInt);
        let tuple_ty = self.intern_type(Type::Tuple(vec![int_ty, item_ty]));
        let ty = self.intern_type(Type::List(tuple_ty));
        let span = self.span(call.range());
        let enumerated = body.push_expr(HirExpr { kind: ExprKind::ListEnumerate { list }, ty, span });
        start_arg.map_or(Ok(Some(enumerated)), |start| {
            self.enumerate_from(enumerated, start, item_ty, span, body).map(Some)
        })
    }

    /// Lower Python `zip(left, right)` as a materialized list of pair tuples.
    pub(super) fn zip_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "zip" {
            return Ok(None);
        }
        if call.arguments.args.is_empty() || !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "zip() needs at least one iterable argument and no keyword (`strict=`)",
            ));
        }
        if call.arguments.args.len() != 2 {
            return self.zip_n_call_expression(call, body).map(Some);
        }
        let raw_left = self.expression(&call.arguments.args[0], body)?;
        let raw_right = self.expression(&call.arguments.args[1], body)?;
        let is_generator = |this: &Self, value: smelt_hir::ExprId| {
            matches!(this.ctx.krate.types.get(Self::expr_ty(body, value)), Some(Type::Generator { is_async: false, .. }))
        };
        let (left_lazy, right_lazy) = (is_generator(self, raw_left), is_generator(self, raw_right));
        let left = if left_lazy { raw_left } else { self.iterable_as_list(raw_left, body) };
        let right = if right_lazy { raw_right } else { self.iterable_as_list(raw_right, body) };
        if left_lazy || right_lazy {
            // A lazy generator of the pairs (`lowering/lazy_iteration.rs`).
            return self.lazy_zip(left, right, self.span(call.range()), body).map(Some);
        }
        let left_ty = Self::expr_ty(body, left);
        let right_ty = Self::expr_ty(body, right);
        let left_item_ty = if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(left_ty) {
            *item_ty
        } else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "zip() left argument must be a list, set, dict or str",
            ));
        };
        let right_item_ty = if let Some(Type::List(item_ty)) = self.ctx.krate.types.get(right_ty) {
            *item_ty
        } else {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[1].range()),
                "zip() right argument must be a list, set, dict or str",
            ));
        };
        let tuple_ty = self.intern_type(Type::Tuple(vec![left_item_ty, right_item_ty]));
        let ty = self.intern_type(Type::List(tuple_ty));
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListZip { left, right },
            ty,
            span: self.span(call.range()),
        })))
    }

    /// `zip(a)` and `zip(a, b, c, ..)`: the tuples of the operands' items
    /// position by position, as many as the shortest operand has, materialized
    /// as `zip(a, b)` is.
    ///
    /// Built from the pairwise zip (`ExprKind::ListZip`): the operands are
    /// zipped left to right into nested pairs, `((a, b), c)`, and one loop
    /// flattens each into `(a, b, c)`. Every operand is evaluated once, in
    /// source order, and the shortest decides the length, as in `CPython`. A
    /// generator operand is refused (materializing it would pull items
    /// `CPython`'s lazy zip never pulls).
    fn zip_n_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(call.range());
        let mut lists = Vec::with_capacity(call.arguments.args.len());
        let mut item_tys = Vec::with_capacity(call.arguments.args.len());
        for arg in &call.arguments.args {
            let raw = self.expression(arg, body)?;
            if matches!(self.ctx.krate.types.get(Self::expr_ty(body, raw)), Some(Type::Generator { .. })) {
                return Err(SmeltError::unsupported(
                    self.span(arg.range()),
                    "zip() of a generator takes exactly two operands",
                ));
            }
            let list = self.iterable_as_list(raw, body);
            let Some(Type::List(item_ty)) = self.ctx.krate.types.get(Self::expr_ty(body, list)).cloned() else {
                return Err(SmeltError::unsupported(self.span(arg.range()), "zip() argument must be a list, set, dict or str"));
            };
            lists.push(list);
            item_tys.push(item_ty);
        }
        let none_ty = self.intern_type(Type::None);
        let tuple_ty = self.intern_type(Type::Tuple(item_tys.clone()));
        let list_ty = self.intern_type(Type::List(tuple_ty));
        let outer = body.push_block(span);
        let acc_local = body.push_local(self.synthetic_local("__smelt_zip_acc", list_ty, true, span));
        let acc_pat = body.push_pattern(HirPattern::Binding(acc_local));
        let empty = body.push_expr(HirExpr { kind: ExprKind::ListLit(Vec::new()), ty: list_ty, span });
        body.push_stmt_to_block(outer, HirStmt::Let { pat: acc_pat, ty: list_ty, value: Some(empty) });
        // The nested pairs and the pattern taking them apart.
        let mut item_locals = Vec::with_capacity(lists.len());
        let mut zipped = lists[0];
        let mut zipped_item_ty = item_tys[0];
        let first_local = body.push_local(self.synthetic_local("__smelt_zip_item", item_tys[0], false, span));
        item_locals.push(first_local);
        let mut pattern = body.push_pattern(HirPattern::Binding(first_local));
        for (list, item_ty) in lists.iter().zip(&item_tys).skip(1) {
            let pair_ty = self.intern_type(Type::Tuple(vec![zipped_item_ty, *item_ty]));
            let pairs_ty = self.intern_type(Type::List(pair_ty));
            zipped = body.push_expr(HirExpr { kind: ExprKind::ListZip { left: zipped, right: *list }, ty: pairs_ty, span });
            zipped_item_ty = pair_ty;
            let local = body.push_local(self.synthetic_local("__smelt_zip_item", *item_ty, false, span));
            item_locals.push(local);
            let binding = body.push_pattern(HirPattern::Binding(local));
            pattern = body.push_pattern(HirPattern::Tuple(vec![pattern, binding]));
        }
        let loop_body = body.push_block(span);
        let items = item_locals
            .iter()
            .zip(&item_tys)
            .map(|(local, ty)| body.push_expr(HirExpr { kind: ExprKind::Local(*local), ty: *ty, span }))
            .collect::<Vec<_>>();
        let tuple = body.push_expr(HirExpr { kind: ExprKind::TupleLit(items), ty: tuple_ty, span });
        let acc = body.push_expr(HirExpr { kind: ExprKind::Local(acc_local), ty: list_ty, span });
        let push = body.push_expr(HirExpr { kind: ExprKind::ListPush { list: acc, item: tuple }, ty: none_ty, span });
        body.push_stmt_to_block(loop_body, HirStmt::Expr(push));
        body.push_stmt_to_block(outer, HirStmt::For { pat: pattern, iter: zipped, body: loop_body });
        let tail = body.push_expr(HirExpr { kind: ExprKind::Local(acc_local), ty: list_ty, span });
        let outer_idx = usize::try_from(outer.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[outer_idx].tail = Some(tail);
        Ok(body.push_expr(HirExpr { kind: ExprKind::Block(outer), ty: list_ty, span }))
    }

    /// Lower Python `range(...)` as a materialized `list[int]`.
    pub(super) fn range_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if name.id.as_str() != "range" {
            return Ok(None);
        }
        let span = self.span(call.range());
        if !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "range() keyword arguments are not supported",
            ));
        }
        let int_ty = self.intern_type(Type::PyInt);
        let int_literal = |hir_body: &mut Body, value| {
            hir_body.push_expr(HirExpr {
                kind: ExprKind::Literal(smelt_hir::Literal::Int(value)),
                ty: int_ty,
                span,
            })
        };
        let (start, end, step) = match call.arguments.args.as_ref() {
            [end_expr] => (
                int_literal(body, 0),
                self.expression(end_expr, body)?,
                int_literal(body, 1),
            ),
            [start_expr, end_expr] => (
                self.expression(start_expr, body)?,
                self.expression(end_expr, body)?,
                int_literal(body, 1),
            ),
            [start_expr, end_expr, step_expr] => (
                self.expression(start_expr, body)?,
                self.expression(end_expr, body)?,
                self.expression(step_expr, body)?,
            ),
            _ => {
                return Err(SmeltError::unsupported(
                    span,
                    "range() requires one, two, or three integer arguments",
                ));
            }
        };
        for bound in [start, end, step] {
            if self.ctx.krate.types.get(Self::expr_ty(body, bound)) != Some(&Type::PyInt) {
                return Err(SmeltError::unsupported(
                    span,
                    "range() arguments must be integers",
                ));
            }
        }
        // Materialized as `CPython` would iterate it, exact at every
        // magnitude. Only a step that may be zero can raise (`ValueError`), so
        // a nonzero constant step keeps the infallible form.
        if Self::is_nonzero_constant(step, body) {
            let ty = self.intern_type(Type::List(int_ty));
            return Ok(Some(body.push_expr(HirExpr {
                kind: ExprKind::ListRange { start, end, step },
                ty,
                span,
            })));
        }
        Ok(Some(self.py_numeric_call(smelt_hir::PyNumericOp::Range, vec![start, end, step], span, body)))
    }

    /// Lower Python `list.extend(other)` calls for same-typed lists.
    pub(super) fn list_extend_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "extend" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "list.extend() requires exactly one list argument",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        if !matches!(self.ctx.krate.types.get(list_ty), Some(Type::List(_))) {
            return Ok(None);
        }
        let other = self.expression(&call.arguments.args[0], body)?;
        if Self::expr_ty(body, other) != list_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "list.extend() argument must match the receiver list type",
            ));
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListExtend { list, other },
            ty,
            span: self.span(call.range()),
        })))
    }

    /// Lower Python `list.insert(index, item)` calls with integer indexes.
    pub(super) fn list_insert_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "insert" {
            return Ok(None);
        }
        if call.arguments.args.len() != 2 {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "list.insert() requires index and item arguments",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let index = self.expression(&call.arguments.args[0], body)?;
        if !matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, index)),
            Some(Type::PyInt)
        ) {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "list.insert() index must be int",
            ));
        }
        let item = self.expression(&call.arguments.args[1], body)?;
        if Self::expr_ty(body, item) != element_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[1].range()),
                "list.insert() item must match the list element type",
            ));
        }
        let position = self.py_int_as_size(index, body);
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListInsert { list, index: position, item },
            ty,
            span: self.span(call.range()),
        })))
    }

    /// Lower Python `list.index(item)` calls without start or stop arguments.
    pub(super) fn list_index_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "index" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "list.index() currently supports exactly one item argument",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let item = self.expression(&call.arguments.args[0], body)?;
        if Self::expr_ty(body, item) != element_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "list.index() item must match the list element type",
            ));
        }
        let span = self.span(call.range());
        Ok(Some(self.machine_int_as_py_int(ExprKind::ListIndex { list, item }, span, body)))
    }

    /// Lower Python `list.remove(item)` calls.
    pub(super) fn list_remove_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "remove" {
            return Ok(None);
        }
        if call.arguments.args.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "list.remove() requires exactly one item argument",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let item = self.expression(&call.arguments.args[0], body)?;
        if Self::expr_ty(body, item) != element_ty {
            return Err(SmeltError::unsupported(
                self.span(call.arguments.args[0].range()),
                "list.remove() item must match the list element type",
            ));
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListRemove { list, item },
            ty,
            span: self.span(call.range()),
        })))
    }

    /// Lower Python `list.sort(key=..., reverse=...)` calls in place.
    ///
    /// Scalar lists sort directly; an optional `key=` callable sorts by its
    /// mapped value and `reverse=True` orders descending, matching the
    /// `sorted()` lowering and the TypeScript array sort comparator support.
    pub(super) fn list_sort_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        if attr.attr.as_str() != "sort" {
            return Ok(None);
        }
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "list.sort() currently supports no positional arguments",
            ));
        }
        let list = self.expression(&attr.value, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let element_ty = *list_element_ty;
        let (key, reverse) =
            self.sort_keyword_arguments(&call.arguments.keywords, element_ty, body)?;
        if key.is_none()
            && !matches!(
                self.ctx.krate.types.get(element_ty),
                Some(Type::Bool | Type::PyInt | Type::Float | Type::PyStr)
            )
        {
            return Err(SmeltError::unsupported(
                self.span(attr.value.range()),
                "list.sort() supports bool, int, float, and str lists for now",
            ));
        }
        let ty = self.intern_type(Type::None);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::ListSort {
                list,
                comparator: None,
                key,
                reverse,
            },
            ty,
            span: self.span(call.range()),
        })))
    }
}
