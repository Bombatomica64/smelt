// Python source templates: a library adapter written as Python statements.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// Some standard-library callables are defined by CPython as a short loop over
// their arguments (`itertools.chain` is `for it in iterables: yield from it`).
// Building that loop node by node in HIR restates, by hand, what the frontend
// already does for the same Python: `for` over any iterable (a list, a
// `str`, a generator resumed one value at a time), `break`/`for ... else`,
// truth tests, list methods, comparisons, calls of a function value. A
// template instead *is* that Python, lowered by the frontend itself:
//
// - its statements are parsed from a fixed source text and every expression
//   is relocated to the call being lowered, so a diagnostic points at the
//   user's call;
// - its parameters are already-lowered, typed values of the caller (bound as
//   locals of the template's own body), so every name the template reads has
//   a concrete type and nothing in it is inferred from spelling;
// - it reads no builtin name (a program may rebind `len` or `list`), only its
//   parameters, the locals it assigns, operators, and methods of their
//   static types;
// - a generator template runs in a fresh function scope as the body of a
//   synthesized generator closure (`call_adapter`), so it is resumed one value
//   at a time by whatever consumes it, exactly like a `def` with `yield`.
//
// Behavioural requirement: the template is the CPython definition (or its
// documented "roughly equivalent" Python) of the callable, so the lowered
// code runs what CPython runs, in the same order. Type requirement: the
// template's values are typed from its parameters; a generator template
// declares its yield type, and each `yield` is checked against it like any
// generator function's.

/// One parameter of a source template: the name the template reads and the
/// caller's value bound to it.
struct TemplateParam {
    /// The name the template's source reads (`__smelt_it`).
    name: String,
    /// The caller's lowered value.
    value: smelt_hir::ExprId,
}

/// Relocates every expression of a parsed template to one source range.
struct RelocateTemplate {
    /// The range of the call the template implements.
    range: TextRange,
}

impl ruff_python_ast::visitor::transformer::Transformer for RelocateTemplate {
    fn visit_expr(&self, expr: &mut Expr) {
        ruff_python_ast::relocate::relocate_expr(expr, self.range);
    }
}

impl ModuleBuilder<'_> {
    /// Parse a template's statements, relocated to `range`.
    fn template_statements(source: &str, range: TextRange, span: Span) -> Result<Vec<Stmt>, SmeltError> {
        let parsed = ruff_python_parser::parse_module(source)
            .map_err(|error| SmeltError::unsupported(span, format!("internal error: template does not parse: {error}")))?;
        let mut statements = parsed.into_syntax().body;
        let relocate = RelocateTemplate { range };
        for statement in &mut statements {
            ruff_python_ast::visitor::transformer::Transformer::visit_stmt(&relocate, statement);
        }
        Ok(statements.into_iter().collect())
    }

    /// Bind each function-typed template parameter as a callable local, so a
    /// call of it (`__smelt_key(value)`) lowers as a closure call taking every
    /// argument positionally.
    fn register_template_callable(&mut self, name: &str, ty: TypeId) {
        if let Some(Type::Function(function)) = self.ctx.krate.types.get(ty) {
            let defaults = vec![None; function.params.len()];
            self.scope.local_callbacks.insert(name.to_owned(), LocalCallback { defaults, vararg: None, kwarg: None });
        }
    }

    /// The generator a generator template answers: `source` is the body of a
    /// generator function yielding `yield_ty` (its return and sent types
    /// `None`), whose parameters are `params`; the answer is that function
    /// called with the parameters' values. `prepare` runs inside the
    /// template's body before its statements, with the parameters bound: it
    /// may bind further locals the template reads (a nested template's
    /// closure).
    #[expect(clippy::too_many_arguments, reason = "the template, its signature, and its lowering position")]
    fn generator_template(
        &mut self,
        source: &str,
        params: &[TemplateParam],
        yield_ty: TypeId,
        range: TextRange,
        body: &mut Body,
        prepare: impl FnOnce(&mut Self, &mut Body) -> Result<(), SmeltError>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let none_ty = self.intern_type(Type::None);
        let generator_ty = self.intern_type(Type::Generator { is_async: false, yield_ty, return_ty: none_ty, next_ty: none_ty });
        let specs = params.iter().map(|param| (param.name.clone(), Self::expr_ty(body, param.value))).collect::<Vec<_>>();
        let (adapter, declared) = self.generator_template_body(source, &specs, yield_ty, range, prepare)?;
        let args = params.iter().map(|param| param.value).collect();
        Ok(self.call_adapter(adapter, declared, args, generator_ty, span, body))
    }

    /// The closure value of a generator template (see
    /// [`Self::generator_template`]), not called: a function of parameters
    /// named and typed by `specs`, answering the generator.
    fn generator_template_closure(
        &mut self,
        source: &str,
        specs: &[(String, TypeId)],
        yield_ty: TypeId,
        range: TextRange,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);
        let none_ty = self.intern_type(Type::None);
        let generator_ty = self.intern_type(Type::Generator { is_async: false, yield_ty, return_ty: none_ty, next_ty: none_ty });
        let (adapter, declared) = self.generator_template_body(source, specs, yield_ty, range, |_, _| Ok(()))?;
        Ok(self.adapter_closure(adapter, declared, generator_ty, span, body))
    }

    /// Lower a generator template's statements into a fresh generator body
    /// whose parameters are named and typed by `specs`.
    fn generator_template_body(
        &mut self,
        source: &str,
        specs: &[(String, TypeId)],
        yield_ty: TypeId,
        range: TextRange,
        prepare: impl FnOnce(&mut Self, &mut Body) -> Result<(), SmeltError>,
    ) -> Result<(Body, Vec<Param>), SmeltError> {
        let span = self.span(range);
        let statements = Self::template_statements(source, range, span)?;
        let none_ty = self.intern_type(Type::None);
        let mut adapter = Body::new(None, span);
        adapter.set_generator(true);
        let declared = self.adapter_params(&mut adapter, &specs.iter().map(|(name, ty)| (name.as_str(), *ty)).collect::<Vec<_>>(), span);
        self.with_fn_scope(FnKind::Sync, |this| {
            this.scope.generator = Some(GeneratorScope { yielded: yield_ty, sent: none_ty, returned: none_ty });
            for (param, (name, ty)) in declared.iter().zip(specs) {
                this.scope.locals.insert(name.clone(), param.local);
                this.register_template_callable(name, *ty);
            }
            prepare(this, &mut adapter)?;
            statements.iter().try_for_each(|statement| this.statement(statement, &mut adapter))
        })?;
        Ok((adapter, declared))
    }

    /// Lower template statements into `block` of the current body, reading
    /// `bound` (locals of the current body, already holding the caller's
    /// values) under the template's names for the statements' duration only:
    /// a check a callable makes when it is called, before any value is
    /// pulled.
    fn inline_template(
        &mut self,
        source: &str,
        bound: &[(&'static str, smelt_hir::LocalId)],
        range: TextRange,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(range);
        let statements = Self::template_statements(source, range, span)?;
        self.with_block_scope(|this| {
            for (name, local) in bound {
                this.scope.locals.insert((*name).to_owned(), *local);
            }
            statements.iter().try_for_each(|statement| this.statement_in_block(statement, body, block))
        })
    }

    /// Bind `value` to a fresh immutable local of `block`, so a template and
    /// the call it guards both read it, evaluated once.
    fn spill_template_value(&mut self, name: &str, value: smelt_hir::ExprId, body: &mut Body, block: smelt_hir::BlockId) -> smelt_hir::LocalId {
        let ty = Self::expr_ty(body, value);
        let span = body.exprs.get(usize::try_from(value.0).unwrap_or(usize::MAX)).map_or_else(|| Span::new(self.file_id, 0, 0), |expr| expr.span);
        let local = body.push_local(self.synthetic_local(name, ty, false, span));
        let pat = body.push_pattern(HirPattern::Binding(local));
        body.push_stmt_to_block(block, HirStmt::Let { pat, ty, value: Some(value) });
        local
    }
}
