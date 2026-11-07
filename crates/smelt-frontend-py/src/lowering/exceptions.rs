// Python exceptions: exception classes, `raise`, and `try`/`except`.
//
// Every exception value, whatever its class, has one HIR type, the reserved
// `PY_EXCEPTION_CLASS_NAME` class, which codegen renders as the runtime's
// `SmeltPyException`. An exception class is therefore not a HIR class item: it
// is its *chain*, its own name followed by each base up to `BaseException`, and
// an instance is that chain plus the constructor arguments. A `raise` builds
// one (`ExprKind::PyExceptionNew`), and an `except` clause asks whether the
// caught one's chain names any class the clause names
// (`ExprKind::PyExceptionCaughtBy`).

/// Python's builtin exception classes, each with its direct base.
///
/// `BaseException` is the root; every other class names a class earlier in the
/// table, so a chain is found by walking bases.
const BUILTIN_EXCEPTIONS: &[(&str, &str)] = &[
    ("BaseException", ""),
    ("BaseExceptionGroup", "BaseException"),
    ("GeneratorExit", "BaseException"),
    ("KeyboardInterrupt", "BaseException"),
    ("SystemExit", "BaseException"),
    ("Exception", "BaseException"),
    ("ArithmeticError", "Exception"),
    ("FloatingPointError", "ArithmeticError"),
    ("OverflowError", "ArithmeticError"),
    ("ZeroDivisionError", "ArithmeticError"),
    ("AssertionError", "Exception"),
    ("AttributeError", "Exception"),
    ("BufferError", "Exception"),
    ("EOFError", "Exception"),
    ("ExceptionGroup", "BaseExceptionGroup"),
    ("ImportError", "Exception"),
    ("ModuleNotFoundError", "ImportError"),
    ("LookupError", "Exception"),
    ("IndexError", "LookupError"),
    ("KeyError", "LookupError"),
    ("MemoryError", "Exception"),
    ("NameError", "Exception"),
    ("UnboundLocalError", "NameError"),
    ("OSError", "Exception"),
    ("BlockingIOError", "OSError"),
    ("ChildProcessError", "OSError"),
    ("ConnectionError", "OSError"),
    ("BrokenPipeError", "ConnectionError"),
    ("ConnectionAbortedError", "ConnectionError"),
    ("ConnectionRefusedError", "ConnectionError"),
    ("ConnectionResetError", "ConnectionError"),
    ("FileExistsError", "OSError"),
    ("FileNotFoundError", "OSError"),
    ("InterruptedError", "OSError"),
    ("IsADirectoryError", "OSError"),
    ("NotADirectoryError", "OSError"),
    ("PermissionError", "OSError"),
    ("ProcessLookupError", "OSError"),
    ("TimeoutError", "OSError"),
    ("ReferenceError", "Exception"),
    ("RuntimeError", "Exception"),
    ("NotImplementedError", "RuntimeError"),
    ("PythonFinalizationError", "RuntimeError"),
    ("RecursionError", "RuntimeError"),
    ("StopAsyncIteration", "Exception"),
    ("StopIteration", "Exception"),
    ("SyntaxError", "Exception"),
    ("IndentationError", "SyntaxError"),
    ("TabError", "IndentationError"),
    ("SystemError", "Exception"),
    ("TypeError", "Exception"),
    ("ValueError", "Exception"),
    ("UnicodeError", "ValueError"),
    ("UnicodeDecodeError", "UnicodeError"),
    ("UnicodeEncodeError", "UnicodeError"),
    ("UnicodeTranslateError", "UnicodeError"),
    ("Warning", "Exception"),
    ("BytesWarning", "Warning"),
    ("DeprecationWarning", "Warning"),
    ("EncodingWarning", "Warning"),
    ("FutureWarning", "Warning"),
    ("ImportWarning", "Warning"),
    ("PendingDeprecationWarning", "Warning"),
    ("ResourceWarning", "Warning"),
    ("RuntimeWarning", "Warning"),
    ("SyntaxWarning", "Warning"),
    ("UnicodeWarning", "Warning"),
    ("UserWarning", "Warning"),
];

/// Builtin names that are the same class object as another builtin exception.
const BUILTIN_EXCEPTION_ALIASES: &[(&str, &str)] =
    &[("EnvironmentError", "OSError"), ("IOError", "OSError")];

/// The chain of the builtin exception class `name`: its own name, then each
/// base up to `BaseException`. An alias answers the chain of the class it is.
fn builtin_exception_chain(name: &str) -> Option<Vec<String>> {
    let class_name = BUILTIN_EXCEPTION_ALIASES
        .iter()
        .find_map(|(alias, class)| (*alias == name).then_some(*class))
        .unwrap_or(name);
    let mut chain = Vec::new();
    let mut current = class_name;
    while !current.is_empty() {
        let (class, base) = BUILTIN_EXCEPTIONS.iter().find(|(class, _)| *class == current)?;
        chain.push((*class).to_owned());
        current = base;
    }
    Some(chain)
}

/// Whether a class body statement is one an exception class may hold: `pass`,
/// `...`, or a docstring.
fn is_exception_class_filler(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Pass(_) => true,
        Stmt::Expr(expr) => matches!(
            expr.value.as_ref(),
            Expr::StringLiteral(_) | Expr::EllipsisLiteral(_)
        ),
        _ => false,
    }
}

impl ModuleBuilder<'_> {
    /// Record every top-level class of `module` that derives from an exception
    /// class, builtin or one recorded before it, with its chain.
    ///
    /// Runs before any body is lowered, so a function may construct or catch
    /// a class defined after it, as Python allows.
    fn collect_exception_classes(&mut self, module: &ModModule) {
        let module_names: HashSet<&str> = module
            .body
            .iter()
            .filter_map(|stmt| match stmt {
                Stmt::ClassDef(class) => Some(class.name.as_str()),
                Stmt::FunctionDef(function) => Some(function.name.as_str()),
                _ => None,
            })
            .collect();
        for stmt in &module.body {
            let Stmt::ClassDef(class) = stmt else {
                continue;
            };
            let Some(arguments) = class.arguments.as_deref() else {
                continue;
            };
            let [Expr::Name(base_expr)] = arguments.args.as_ref() else {
                continue;
            };
            let base = base_expr.id.as_str();
            let found = self.exception_classes.get(base).cloned().or_else(|| {
                (!module_names.contains(base) && !self.import_aliases.contains_key(base))
                    .then(|| builtin_exception_chain(base))
                    .flatten()
            });
            if let Some(base_chain) = found {
                let mut chain = vec![class.name.as_str().to_owned()];
                chain.extend(base_chain);
                self.exception_classes.insert(class.name.as_str().to_owned(), chain);
            }
        }
    }

    /// The chain of the exception class `name` denotes here, or `None` when
    /// it denotes no exception class: a user exception class, or a builtin
    /// one the program does not shadow.
    fn exception_class_chain(&self, name: &str) -> Option<Vec<String>> {
        if self.scope.locals.contains_key(name) {
            return None;
        }
        if let Some(chain) = self.exception_classes.get(name) {
            return Some(chain.clone());
        }
        if self.program_binds_name(name) {
            return None;
        }
        builtin_exception_chain(name)
    }

    /// The one HIR type of every Python exception value.
    fn py_exception_type(&mut self) -> TypeId {
        let name = self.intern_name(smelt_stdlib::PY_EXCEPTION_CLASS_NAME);
        self.intern_type(Type::Class { name, args: vec![] })
    }

    /// Accept the definition of a user exception class.
    ///
    /// It declares no HIR item: the class is its chain, recorded by
    /// [`Self::collect_exception_classes`]. Its body may only be `pass`, `...`
    /// or a docstring, because an instance holds nothing but its chain and its
    /// constructor arguments.
    fn exception_class_def(&self, class: &StmtClassDef) -> Result<(), SmeltError> {
        if !class.decorator_list.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(class.range),
                format!("exception class '{}': decorators are not supported", class.name),
            ));
        }
        if let Some(stmt) = class.body.iter().find(|stmt| !is_exception_class_filler(stmt)) {
            return Err(SmeltError::unsupported(
                self.span(stmt.range()),
                format!(
                    "exception class '{}': only a `pass`, `...` or docstring body is supported (no fields or methods)",
                    class.name
                ),
            ));
        }
        Ok(())
    }

    /// Whether the runtime writes Python's `str` and `repr` of a value of type
    /// `ty` (its `SmeltPyRepr`): a number, a string, a boolean, `None`, an
    /// exception or its `args`, and a list, tuple, set or dict of those. Such a
    /// value can be an exception constructor argument and the operand of
    /// `str()`, `repr()` and an f-string interpolation.
    fn has_python_text(&self, ty: TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Int | Type::Float | Type::Bool | Type::String | Type::None) => true,
            Some(Type::Optional(item) | Type::List(item)) => self.has_python_text(*item),
            // The runtime writes a tuple's `repr` for one to six elements.
            Some(Type::Tuple(items)) => {
                (1..=6).contains(&items.len()) && items.iter().all(|item| self.has_python_text(*item))
            }
            Some(Type::Set(item)) => matches!(
                self.ctx.krate.types.get(*item),
                Some(Type::Int | Type::Bool | Type::String)
            ),
            Some(Type::Dict(key, value)) => {
                matches!(self.ctx.krate.types.get(*key), Some(Type::Int | Type::Bool | Type::String))
                    && self.has_python_text(*value)
            }
            Some(Type::Class { name, .. }) => matches!(
                self.ctx.krate.symbols.get(*name),
                Some(smelt_stdlib::PY_EXCEPTION_CLASS_NAME | smelt_stdlib::PY_EXCEPTION_ARGS_CLASS_NAME)
            ),
            _ => false,
        }
    }

    /// `str(value)` or `repr(value)` of a lowered value with Python text:
    /// a primitive `str()` stays the `ToString` cast every other path uses.
    fn python_text_expr(
        &mut self,
        value: smelt_hir::ExprId,
        repr: bool,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let value_ty = Self::expr_ty(body, value);
        let str_ty = self.intern_type(Type::String);
        if !repr && value_ty == str_ty {
            return Ok(value);
        }
        if !repr && matches!(self.ctx.krate.types.get(value_ty), Some(Type::Bool | Type::Int | Type::Float)) {
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::PrimitiveCast { op: PrimitiveCastOp::ToString, operand: value },
                ty: str_ty,
                span,
            }));
        }
        if !self.has_python_text(value_ty) {
            return Err(SmeltError::unsupported(
                span,
                "str()/repr() needs a str, int, float, bool, None, an exception, or a list, tuple, set or dict of those",
            ));
        }
        let kind = if repr { ExprKind::PyRepr { value } } else { ExprKind::PyStr { value } };
        Ok(body.push_expr(HirExpr { kind, ty: str_ty, span }))
    }

    /// An instance of the exception class whose chain is `chain`, built from
    /// the positional arguments `args`.
    fn exception_new_expr(
        &mut self,
        chain: &[String],
        args: &[Expr],
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut lowered = Vec::with_capacity(args.len());
        for arg in args {
            if matches!(arg, Expr::Starred(_)) {
                return Err(SmeltError::unsupported(
                    self.span(arg.range()),
                    "a starred exception constructor argument is not supported",
                ));
            }
            let value = self.expression(arg, body)?;
            if !self.has_python_text(Self::expr_ty(body, value)) {
                return Err(SmeltError::unsupported(
                    self.span(arg.range()),
                    "an exception constructor argument must be a str, int, float, bool, None, an exception, or a list or tuple of those",
                ));
            }
            lowered.push(value);
        }
        let classes = chain.iter().map(|class| self.intern_name(class)).collect();
        let ty = self.py_exception_type();
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::PyExceptionNew { classes, args: lowered },
            ty,
            span,
        }))
    }

    /// Lower `C(args)` where `C` denotes an exception class.
    fn exception_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        let Some(chain) = self.exception_class_chain(name.id.as_str()) else {
            return Ok(None);
        };
        if !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.range()),
                "keyword arguments to an exception constructor are not supported",
            ));
        }
        self.exception_new_expr(&chain, &call.arguments.args, self.span(call.range()), body)
            .map(Some)
    }

    /// Lower `raise`, `raise E`, `raise E(args)` and `raise E from cause`.
    ///
    /// A bare `raise` re-raises the exception the innermost enclosing `except`
    /// clause caught. A class named without a call is instantiated with no
    /// arguments, as Python does. The `from` cause only sets `__cause__`, which
    /// nothing but a traceback shows, so it is not evaluated.
    fn raise_statement(
        &mut self,
        raise_stmt: &ruff_python_ast::StmtRaise,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(raise_stmt.range);
        let exception = match raise_stmt.exc.as_deref() {
            None => {
                let Some(caught) = self.handled_exceptions.last().copied() else {
                    return Err(SmeltError::unsupported(
                        span,
                        "a bare `raise` outside an `except` clause is not supported",
                    ));
                };
                let ty = self.py_exception_type();
                body.push_expr(HirExpr { kind: ExprKind::Local(caught), ty, span })
            }
            Some(Expr::Name(name)) if let Some(chain) = self.exception_class_chain(name.id.as_str()) => {
                self.exception_new_expr(&chain, &[], span, body)?
            }
            Some(exc) => {
                let value = self.expression(exc, body)?;
                let ty = self.py_exception_type();
                if Self::expr_ty(body, value) != ty {
                    return Err(SmeltError::unsupported(
                        self.span(exc.range()),
                        "`raise` needs an exception class or instance",
                    ));
                }
                value
            }
        };
        body.push_stmt_to_block(block, HirStmt::Throw(exception));
        Ok(())
    }

    /// The class chains an `except` clause's type expression names: one class
    /// or a tuple of them.
    fn except_clause_chains(&self, type_expr: &Expr) -> Result<Vec<Vec<String>>, SmeltError> {
        let names: Vec<&Expr> = match type_expr {
            Expr::Tuple(tuple) => tuple.elts.iter().collect(),
            other => vec![other],
        };
        names
            .into_iter()
            .map(|name| {
                let Expr::Name(name_expr) = name else {
                    return Err(SmeltError::unsupported(
                        self.span(name.range()),
                        "an `except` clause must name exception classes",
                    ));
                };
                self.exception_class_chain(name_expr.id.as_str()).ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(name.range()),
                        format!("`{}` is not an exception class", name_expr.id),
                    )
                })
            })
            .collect()
    }

    /// Lower a Python `try` statement to [`HirStmt::TryCatch`].
    ///
    /// The handlers become one catch block that binds the caught exception and
    /// tests each `except` clause in source order, running the first whose
    /// classes the exception's chain names and re-raising it when none does.
    ///
    /// An `else:` suite runs only when the `try` suite raised nothing, after it
    /// and before `finally:`, and an exception it raises is not caught by the
    /// statement's handlers. It is lowered as
    ///
    /// ```text
    /// try:                      # only when there is a `finally:`
    ///     completed = False
    ///     try: body; completed = True
    ///     except ...: ...
    ///     if completed: else-suite
    /// finally: finally-suite
    /// ```
    fn try_statement(
        &mut self,
        try_stmt: &StmtTry,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(try_stmt.range);
        if try_stmt.is_star {
            return Err(SmeltError::unsupported(span, "`except*` exception groups are not supported"));
        }
        let finally_body = if try_stmt.finalbody.is_empty() {
            None
        } else {
            Some(self.block_from_stmts(&try_stmt.finalbody, body)?)
        };
        if try_stmt.orelse.is_empty() {
            let try_block = self.block_from_stmts(&try_stmt.body, body)?;
            let (catch_binding, catch_body) = self.except_handlers(&try_stmt.handlers, span, body)?;
            body.push_stmt_to_block(
                block,
                HirStmt::TryCatch { body: try_block, catch_binding, catch_body, finally_body },
            );
            return Ok(());
        }

        let outer = if finally_body.is_some() { body.push_block(span) } else { block };
        let bool_ty = self.intern_type(Type::Bool);
        let completed = body.push_local(self.synthetic_local("__smelt_try_completed", bool_ty, true, span));
        let completed_pat = body.push_pattern(HirPattern::Binding(completed));
        let false_expr = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(false)), ty: bool_ty, span });
        body.push_stmt_to_block(outer, HirStmt::Let { pat: completed_pat, ty: bool_ty, value: Some(false_expr) });
        let try_block = self.block_from_stmts(&try_stmt.body, body)?;
        let completed_target = body.push_expr(HirExpr { kind: ExprKind::Local(completed), ty: bool_ty, span });
        let true_expr = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(true)), ty: bool_ty, span });
        body.push_stmt_to_block(try_block, HirStmt::Assign { target: completed_target, value: true_expr });
        let (catch_binding, catch_body) = self.except_handlers(&try_stmt.handlers, span, body)?;
        body.push_stmt_to_block(
            outer,
            HirStmt::TryCatch { body: try_block, catch_binding, catch_body, finally_body: None },
        );
        let else_block = self.block_from_stmts(&try_stmt.orelse, body)?;
        let completed_read = body.push_expr(HirExpr { kind: ExprKind::Local(completed), ty: bool_ty, span });
        body.push_stmt_to_block(
            outer,
            HirStmt::If { cond: completed_read, then_block: else_block, else_block: None },
        );
        if finally_body.is_some() {
            body.push_stmt_to_block(
                block,
                HirStmt::TryCatch { body: outer, catch_binding: None, catch_body: None, finally_body },
            );
        }
        Ok(())
    }

    /// The catch binding and catch block of a `try` statement's `except`
    /// clauses, or neither when it has none.
    fn except_handlers(
        &mut self,
        handlers: &[ExceptHandler],
        span: Span,
        body: &mut Body,
    ) -> Result<(Option<smelt_hir::LocalId>, Option<smelt_hir::BlockId>), SmeltError> {
        if handlers.is_empty() {
            return Ok((None, None));
        }
        let exception_ty = self.py_exception_type();
        let caught = body.push_local(self.synthetic_local("__smelt_exception", exception_ty, false, span));
        let catch_block = body.push_block(span);
        let mut current = catch_block;
        let mut exhaustive = false;
        for ExceptHandler::ExceptHandler(handler) in handlers {
            let handler_span = self.span(handler.range);
            // A clause naming no class catches everything, so it runs straight
            // in the block the earlier clauses fell through to.
            let test = handler
                .type_
                .as_deref()
                .map(|type_expr| self.except_clause_test(type_expr, caught, handler_span, body))
                .transpose()?;
            let handler_block = if test.is_some() { body.push_block(handler_span) } else { current };
            self.with_block_scope(|this| {
                if let Some(name) = &handler.name {
                    let local = body.push_local(LocalDecl {
                        name: Some(this.intern_name(name.as_str())),
                        ty: exception_ty,
                        mutable: true,
                        span: this.span(name.range),
                    });
                    this.scope.locals.insert(name.as_str().to_owned(), local);
                    let pat = body.push_pattern(HirPattern::Binding(local));
                    let value = body.push_expr(HirExpr { kind: ExprKind::Local(caught), ty: exception_ty, span: handler_span });
                    body.push_stmt_to_block(handler_block, HirStmt::Let { pat, ty: exception_ty, value: Some(value) });
                }
                this.handled_exceptions.push(caught);
                let lowered = handler
                    .body
                    .iter()
                    .try_for_each(|stmt| this.statement_in_block(stmt, body, handler_block));
                this.handled_exceptions.pop();
                lowered
            })?;
            let Some(cond) = test else {
                exhaustive = true;
                break;
            };
            let next = body.push_block(handler_span);
            body.push_stmt_to_block(
                current,
                HirStmt::If { cond, then_block: handler_block, else_block: Some(next) },
            );
            current = next;
        }
        if !exhaustive {
            let rethrown = body.push_expr(HirExpr { kind: ExprKind::Local(caught), ty: exception_ty, span });
            body.push_stmt_to_block(current, HirStmt::Throw(rethrown));
        }
        Ok((Some(caught), Some(catch_block)))
    }

    /// The test of one `except` clause naming `type_expr`: whether the caught
    /// exception's chain names any of its classes.
    ///
    /// A clause naming a builtin class also catches an error the runtime
    /// raised without recording a class, since that error is some builtin
    /// exception; a clause naming only user classes never does.
    fn except_clause_test(
        &mut self,
        type_expr: &Expr,
        caught: smelt_hir::LocalId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let chains = self.except_clause_chains(type_expr)?;
        let catches_unclassified = chains
            .iter()
            .any(|chain| chain.first().is_none_or(|class| !self.exception_classes.contains_key(class)));
        let classes = chains
            .iter()
            .filter_map(|chain| chain.first())
            .map(|class| self.intern_name(class))
            .collect();
        let exception_ty = self.py_exception_type();
        let bool_ty = self.intern_type(Type::Bool);
        let exception = body.push_expr(HirExpr { kind: ExprKind::Local(caught), ty: exception_ty, span });
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::PyExceptionCaughtBy { exception, classes, catches_unclassified },
            ty: bool_ty,
            span,
        }))
    }

    /// Lower `e.attr` on an exception: `e.args`, the constructor arguments.
    ///
    /// `BaseException.args` is `tuple[Any, ...]`, whose elements have no static
    /// type, so it is the runtime's `SmeltPyExceptionArgs`, a value whose
    /// `str` and `repr` are those of the tuple and that offers nothing else.
    fn exception_attribute_expr(
        &mut self,
        exception: smelt_hir::ExprId,
        attr: &ruff_python_ast::ExprAttribute,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(attr.range);
        if attr.attr.as_str() != "args" {
            return Err(SmeltError::unsupported(
                span,
                format!("exception attribute `{}` is not supported; only `args` is", attr.attr),
            ));
        }
        let name = self.intern_name(smelt_stdlib::PY_EXCEPTION_ARGS_CLASS_NAME);
        let ty = self.intern_type(Type::Class { name, args: vec![] });
        Ok(body.push_expr(HirExpr { kind: ExprKind::PyExceptionArgs { exception }, ty, span }))
    }
}
