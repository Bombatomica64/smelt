// Two-phase module lowering: declarations first, bodies second.
//
// Python resolves the names a function body reads when the body *runs*, not
// when the `def` statement executes, so a body may construct, annotate, call
// and read the members of any class or function the module defines, wherever
// it is written:
//
//     def make() -> "Point":
//         return Point(1, 2)        # `Point` is defined below
//
//     class Point: ...
//
// Lowering every body in source order cannot see `Point`'s constructor, its
// fields or its methods from `make`. A team porting the module to Rust by hand
// declares all items first (Rust items are order-independent), and so does this
// lowering: while pass 1 declares the module's top-level functions and classes
// (`ModuleBuilder::defer_bodies`), each function's and method's *signature* is
// lowered at once -- its item, parameters, defaults prologue and return type,
// so calls and `super()` aliases resolve against it -- and its statements are
// queued here. `lower_deferred_bodies` then lowers the queued statements, in
// declaration order, once every module-level declaration exists.
//
// Behavioral requirement: each body evaluates exactly the statements it did
// before, in the same order; only *when* the frontend lowers them moves.
// Type requirement: a body reads the same concrete item, field and method types
// it would read had the referenced definition been written first.
//
// Deferral is limited to the plain declaration pass. A definition lowered
// inside temporary capture aliases (`with_capture_aliases`, materialized
// specialization) stays eager, because its body reads aliases that only exist
// while it is declared.

/// A declared function or method whose statements are lowered after every
/// module-level declaration (see the module comment).
#[derive(Debug)]
struct DeferredBody {
    /// The source definition (for a stubbed `@abstractmethod`, the stub).
    func: StmtFunctionDef,
    /// The body already pushed for the item, holding its parameter locals and
    /// default-argument prologue; its statements are appended to it.
    body: smelt_hir::BodyId,
    /// The function scope its signature built: parameters, kind, generator.
    scope: FnScope,
    /// The function type parameters in scope for its annotations.
    type_params: Vec<String>,
    /// The top-level definition it belongs to (the class for a method), which
    /// is recorded as failed when the body does not lower.
    definition: String,
}

impl ModuleBuilder<'_> {
    /// Lower the statements of `func` into `fn_body`, the shared tail of an
    /// eager and a deferred function body.
    fn lower_function_statements(
        &mut self,
        func: &StmtFunctionDef,
        fn_body: &mut Body,
    ) -> Result<(), SmeltError> {
        let body_error = func
            .body
            .iter()
            .find_map(|stmt| self.statement(stmt, fn_body).err());
        if func.is_async {
            fn_body.finish_async();
        }
        body_error.map_or(Ok(()), Err)
    }

    /// Finish a function or method body whose signature was just lowered into
    /// `fn_body` in the current function scope: lower its statements now, or,
    /// while bodies are deferred, return the scope to queue with it.
    fn lower_or_defer_statements(
        &mut self,
        func: &StmtFunctionDef,
        fn_body: &mut Body,
    ) -> Result<Option<FnScope>, SmeltError> {
        if self.defer_bodies {
            return Ok(Some(std::mem::take(&mut self.scope)));
        }
        self.lower_function_statements(func, fn_body)?;
        Ok(None)
    }

    /// Queue the statements of `func`, whose signature was lowered into the
    /// pushed body `body` with the function scope `scope`.
    fn defer_body(
        &mut self,
        func: &StmtFunctionDef,
        body: smelt_hir::BodyId,
        scope: FnScope,
        definition: &str,
    ) {
        let definition = definition
            .split_once('.')
            .map_or(definition, |(top, _)| top)
            .to_owned();
        self.deferred_bodies.push(DeferredBody {
            func: func.clone(),
            body,
            scope,
            type_params: self.function_type_params.clone(),
            definition,
        });
    }

    /// Run `lower` with body deferral off: what it declares lowers its body at
    /// once (see the module comment for when that is required).
    fn eagerly<T>(&mut self, lower: impl FnOnce(&mut Self) -> T) -> T {
        let deferring = std::mem::replace(&mut self.defer_bodies, false);
        let result = lower(self);
        self.defer_bodies = deferring;
        result
    }

    /// Lower every queued body, in declaration order, recording a body that
    /// fails as a failed definition and its diagnostic in `errors`.
    fn lower_deferred_bodies(&mut self, errors: &mut Vec<SmeltError>) {
        for job in std::mem::take(&mut self.deferred_bodies) {
            let Ok(index) = usize::try_from(job.body.0) else {
                continue;
            };
            let span = self.span(job.func.range);
            let Some(slot) = self.ctx.krate.bodies.get_mut(index) else {
                continue;
            };
            let mut fn_body = std::mem::replace(slot, Body::new(None, span));
            let enclosing_params = std::mem::replace(&mut self.function_type_params, job.type_params);
            let enclosing_scope = std::mem::replace(&mut self.scope, job.scope);
            let result = self.lower_function_statements(&job.func, &mut fn_body);
            self.scope = enclosing_scope;
            self.function_type_params = enclosing_params;
            if let Some(slot) = self.ctx.krate.bodies.get_mut(index) {
                *slot = fn_body;
            }
            if let Err(err) = result {
                self.failed_definitions.insert(job.definition);
                if !err.is_cascade() {
                    errors.push(err);
                }
            }
        }
    }

    /// Whether `body` is a queued body whose source statements run nothing:
    /// only `pass`, `...` or a docstring, which lower to no statement.
    fn deferred_body_is_empty(&self, body: smelt_hir::BodyId) -> Option<bool> {
        let job = self.deferred_bodies.iter().find(|job| job.body == body)?;
        Some(job.func.body.iter().all(|stmt| match stmt {
            Stmt::Pass(_) => true,
            Stmt::Expr(expr) => matches!(
                expr.value.as_ref(),
                Expr::StringLiteral(_) | Expr::EllipsisLiteral(_)
            ),
            _ => false,
        }))
    }
}
