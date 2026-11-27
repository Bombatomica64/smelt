// Python exceptions: exception classes, `raise`, and `try`/`except`.
//
// A raised exception crosses the error channel as one HIR type, the reserved
// `PY_EXCEPTION_CLASS_NAME` class, which codegen renders as the runtime's
// `SmeltPyException`: the chain an `except` clause matches (the class's own
// name, then each base up to `BaseException`), and either the constructor
// arguments of a builtin exception (`ExprKind::PyExceptionNew`) or the instance
// of a user exception class (`ExprKind::PyExceptionFromInstance`).
//
// A user exception class is an ordinary class item: fields, methods and
// overriding lower as for any class. Its topmost program class (the *exception
// root*, whose base is a builtin exception class) has no HIR base and gets the
// members `BaseException` contributes: the `args` field and default `__str__`
// / `__repr__` methods. An `except` clause naming only user classes binds the
// caught instance at their nearest common class (`ExprKind::PyExceptionInstance`).
// See `docs/design/python-exceptions.md`.

/// The chain of the builtin exception class `name`: its own name, then each
/// base up to `BaseException` (`smelt_stdlib::py_exceptions`).
fn builtin_exception_chain(name: &str) -> Option<Vec<String>> {
    smelt_stdlib::py_exceptions::builtin_exception_chain(name)
        .map(|chain| chain.into_iter().map(ToOwned::to_owned).collect())
}

/// How instances of one user exception class are constructed
/// (`docs/design/python-exceptions.md` §3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExceptionConstructor {
    /// Whether `args` still holds the constructor's positional arguments when
    /// construction ends (`BaseException.__new__` stored them and no
    /// `BaseException.__init__` call overwrote them). Such a class's
    /// constructor takes them packed as a leading hidden parameter.
    keeps_args: bool,
    /// Whether the class or a program base defines `__init__`; without one
    /// every constructor argument goes to `args` only.
    has_init: bool,
}

/// The `__init__` a class statement defines itself.
fn own_init(class: &StmtClassDef) -> Option<&StmtFunctionDef> {
    class.body.iter().find_map(|stmt| match stmt {
        Stmt::FunctionDef(function) if function.name.as_str() == "__init__" => Some(function),
        _ => None,
    })
}

/// Whether `stmt` is the expression statement `super().__init__(...)`.
fn is_super_init_statement(stmt: &Stmt) -> bool {
    matches!(stmt, Stmt::Expr(expr) if matches!(
        expr.value.as_ref(),
        Expr::Call(call) if ModuleBuilder::super_receiver_method(call) == Some("__init__")
    ))
}

/// Whether `stmt` reads or writes an attribute named `args` anywhere in it.
fn mentions_args_attribute(stmt: &Stmt) -> bool {
    /// Finds an `.args` attribute.
    struct Finder(bool);
    impl<'a> ruff_python_ast::visitor::Visitor<'a> for Finder {
        fn visit_expr(&mut self, expr: &'a Expr) {
            if let Expr::Attribute(attribute) = expr
                && attribute.attr.as_str() == "args"
            {
                self.0 = true;
            }
            ruff_python_ast::visitor::walk_expr(self, expr);
        }
    }
    let mut finder = Finder(false);
    ruff_python_ast::visitor::Visitor::visit_stmt(&mut finder, stmt);
    finder.0
}

impl ModuleBuilder<'_> {
    /// Record every top-level class of `module` that derives from an exception
    /// class, builtin or one recorded before it, with its chain, and how each
    /// is constructed.
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
        let mut statements: HashMap<String, &StmtClassDef> = HashMap::new();
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
                statements.insert(class.name.as_str().to_owned(), class);
            }
        }
        let names: Vec<String> = statements.keys().cloned().collect();
        for name in names {
            let constructor = ExceptionConstructor {
                keeps_args: self.exception_keeps_args(&name, &statements),
                has_init: Self::chain_defines_init(&self.exception_classes[&name], &statements),
            };
            self.exception_constructors.insert(name, constructor);
        }
    }

    /// Whether constructing the user exception class `name` leaves its
    /// constructor's positional arguments in `args`: its effective `__init__`
    /// (its own or the nearest program base's) does not call
    /// `super().__init__(...)` as a top-level statement before mentioning
    /// `args`, or that call reaches a program base that keeps them. A class with
    /// no `__init__` in its program chain keeps them (`BaseException.__init__`
    /// stores its arguments).
    fn exception_keeps_args(&self, name: &str, statements: &HashMap<String, &StmtClassDef>) -> bool {
        let Some(chain) = self.exception_classes.get(name) else {
            return true;
        };
        for (index, class_name) in chain.iter().enumerate() {
            let Some(class) = statements.get(class_name) else {
                return true;
            };
            let Some(init) = own_init(class) else {
                continue;
            };
            for stmt in &init.body {
                if is_super_init_statement(stmt) {
                    // The call reaches the parent's effective `__init__`, or
                    // `BaseException.__init__` (which overwrites `args`) when
                    // no program class above defines one.
                    let above = chain.get(index..).and_then(|rest| rest.get(1..)).unwrap_or_default();
                    return match above.first() {
                        Some(parent) if Self::chain_defines_init(above, statements) => {
                            self.exception_keeps_args(parent, statements)
                        }
                        _ => false,
                    };
                }
                if mentions_args_attribute(stmt) {
                    return true;
                }
            }
            return true;
        }
        true
    }

    /// Whether a program class of `chain` (a class's own name and its bases,
    /// in order) defines `__init__`.
    fn chain_defines_init(chain: &[String], statements: &HashMap<String, &StmtClassDef>) -> bool {
        chain.iter().map_while(|class| statements.get(class)).any(|class| own_init(class).is_some())
    }

    /// Whether `super().__init__(...)` in a method of `class_name` reaches
    /// `BaseException.__init__`: the class is an exception root, or no program
    /// base above it defines `__init__`.
    fn super_init_reaches_base_exception(&self, class_name: &str) -> bool {
        self.is_exception_root(class_name)
            || self.exception_classes.get(class_name).is_some_and(|chain| {
                chain
                    .get(1)
                    .and_then(|parent| self.exception_constructors.get(parent))
                    .is_some_and(|parent| !parent.has_init)
            })
    }

    /// Whether `name` is an exception root: a user exception class whose base
    /// is a builtin exception class.
    fn is_exception_root(&self, name: &str) -> bool {
        self.exception_classes
            .get(name)
            .and_then(|chain| chain.get(1))
            .is_some_and(|parent| !self.exception_classes.contains_key(parent))
    }

    /// The exception root of the user exception class `name` and the builtin
    /// class it derives from.
    fn exception_root_and_builtin(&self, name: &str) -> Option<(&str, &str)> {
        let chain = self.exception_classes.get(name)?;
        let builtin = chain.iter().position(|class| !self.exception_classes.contains_key(class))?;
        let root = chain.get(builtin.checked_sub(1)?)?;
        Some((root.as_str(), chain.get(builtin)?.as_str()))
    }

    /// The type of `e.args`, the runtime's `SmeltPyExceptionArgs`.
    fn py_exception_args_type(&mut self) -> TypeId {
        let name = self.intern_synthesized_name(smelt_stdlib::PY_EXCEPTION_ARGS_CLASS_NAME);
        self.intern_type(Type::Class { name, args: vec![] })
    }

    /// The user exception class a value of type `ty` is an instance of.
    fn user_exception_class_of(&self, ty: TypeId) -> Option<Symbol> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(ty) else {
            return None;
        };
        let spelled = self.ctx.krate.symbols.get(*name)?;
        self.exception_classes.contains_key(spelled).then_some(*name)
    }

    /// The members an exception root gets from `BaseException`, added before
    /// its own methods are lowered so they can read them: the `args` field, its
    /// getter (which codegen's `SmeltPyExceptionObject` impl calls), and
    /// `BaseException.__str__`/`__repr__` unless the root defines its own.
    ///
    /// Returns the synthesized method items.
    fn exception_root_members(
        &mut self,
        class: &StmtClassDef,
        target: &MaterializedClassTarget<'_>,
        fields: &mut Vec<Field>,
    ) -> Result<Vec<ItemId>, SmeltError> {
        let span = target.span;
        let args_sym = self.intern_name("args");
        if fields.iter().any(|field| field.name == args_sym) {
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "exception class '{}': an instance field named `args` would replace `BaseException.args`",
                    target.name
                ),
            ));
        }
        let args_ty = self.py_exception_args_type();
        let field = Field { name: args_sym, ty: args_ty, visibility: Visibility::Public, optional: false, span };
        self.class_fields.entry(target.name.to_owned()).or_default().push(field.clone());
        fields.push(field);
        let getter = self.synthesize_exception_method(target, "__smelt$args", args_ty, |_, body, receiver| {
            body.push_expr(HirExpr { kind: ExprKind::Field { receiver, field: args_sym }, ty: args_ty, span })
        });
        self.exception_args_getters.insert(target.name.to_owned(), getter);
        let mut methods = vec![getter];
        let str_ty = self.intern_type(Type::PyStr);
        for (name, repr) in [("__str__", false), ("__repr__", true)] {
            let defined = class
                .body
                .iter()
                .any(|stmt| matches!(stmt, Stmt::FunctionDef(function) if function.name.as_str() == name));
            if defined {
                continue;
            }
            let method = self.synthesize_exception_method(target, name, str_ty, |_, body, instance| {
                body.push_expr(HirExpr { kind: ExprKind::PyExceptionDefaultText { instance, repr }, ty: str_ty, span })
            });
            self.class_methods.entry(target.name.to_owned()).or_default().insert(name.to_owned(), method);
            methods.push(method);
        }
        Ok(methods)
    }

    /// A synthesized instance method `name(self) -> return_ty` of the class
    /// `target` returning what `value` builds from a read of `self`.
    fn synthesize_exception_method(
        &mut self,
        target: &MaterializedClassTarget<'_>,
        name: &str,
        return_ty: TypeId,
        value: impl FnOnce(&mut Self, &mut Body, smelt_hir::ExprId) -> smelt_hir::ExprId,
    ) -> ItemId {
        let span = target.span;
        let class_ty = target.ty;
        let (fn_body, self_local) = self.with_fn_scope(FnKind::Sync, |this| {
            let mut fn_body = Body::new(None, span);
            let self_local =
                fn_body.push_local(LocalDecl { name: Some(this.intern_name("self")), ty: class_ty, mutable: false, span });
            fn_body.params.push(self_local);
            let receiver = fn_body.push_expr(HirExpr { kind: ExprKind::Local(self_local), ty: class_ty, span });
            let result = value(this, &mut fn_body, receiver);
            let root = fn_body.root;
            fn_body.push_stmt_to_block(root, HirStmt::Return(Some(result)));
            (fn_body, self_local)
        });
        let body_id = self.ctx.krate.push_body(fn_body);
        let self_param = Param { name: self.intern_name("self"), local: self_local, ty: class_ty, span };
        let method_sym = if name.contains('$') { self.intern_synthesized_name(name) } else { self.intern_name(name) };
        self.ctx.krate.push_item(Item::Function(Function {
            name: method_sym,
            span,
            type_params: Vec::new(),
            params: vec![self_param],
            rest: None,
            required_params: None,
            return_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::ClassMethod { class: target.symbol, method: method_sym },
        }))
    }

    /// Give the constructor `init` of an exception class that keeps its
    /// constructor arguments the leading hidden parameter `__smelt_args`, and
    /// start its body with `self.args = __smelt_args`
    /// (`docs/design/python-exceptions.md` §3).
    fn add_exception_args_param(&mut self, init: ItemId, class_ty: TypeId, span: Span) -> Result<(), SmeltError> {
        let args_ty = self.py_exception_args_type();
        let args_sym = self.intern_name("args");
        let param_sym = self.intern_synthesized_name("__smelt_args");
        let index = usize::try_from(init.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        let Some(Item::Function(function)) = self.ctx.krate.items.get(index).cloned() else {
            return Err(SmeltError::unsupported(span, "internal error: exception constructor is not a function"));
        };
        let Some(body_id) = function.body else {
            return Err(SmeltError::unsupported(span, "internal error: exception constructor has no body"));
        };
        let body_index = usize::try_from(body_id.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        let self_sym = self.intern_name("self");
        let Some(body) = self.ctx.krate.bodies.get_mut(body_index) else {
            return Err(SmeltError::unsupported(span, "internal error: exception constructor body is missing"));
        };
        let Some(self_local) = body
            .locals
            .iter()
            .position(|local| local.name == Some(self_sym))
            .and_then(|position| u32::try_from(position).ok())
            .map(smelt_hir::LocalId)
        else {
            return Err(SmeltError::unsupported(span, "internal error: exception constructor has no `self`"));
        };
        let param_local = body.push_local(LocalDecl { name: Some(param_sym), ty: args_ty, mutable: false, span });
        body.params.insert(0, param_local);
        let receiver = body.push_expr(HirExpr { kind: ExprKind::Local(self_local), ty: class_ty, span });
        let target = body.push_expr(HirExpr { kind: ExprKind::Field { receiver, field: args_sym }, ty: args_ty, span });
        let value = body.push_expr(HirExpr { kind: ExprKind::Local(param_local), ty: args_ty, span });
        let root = body.root;
        let stmt = body.push_stmt_to_block(root, HirStmt::Assign { target, value });
        let root_index = usize::try_from(root.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        if let Some(block) = body.blocks.get_mut(root_index) {
            block.stmts.pop();
            block.stmts.insert(0, stmt);
        }
        let mut updated = function;
        updated.params.insert(0, Param { name: param_sym, local: param_local, ty: args_ty, span });
        updated.required_params = updated.required_params.map(|count| count.saturating_add(1));
        if let Some(slot) = self.ctx.krate.items.get_mut(index) {
            *slot = Item::Function(updated);
        }
        Ok(())
    }

    /// The constructor of an exception root with no `__init__`:
    /// `BaseException.__init__(*args)`, which stores its arguments, taken
    /// packed as the hidden parameter.
    fn synthesize_exception_root_init(&mut self, class_sym: Symbol, class_ty: TypeId, span: Span) -> Result<ItemId, SmeltError> {
        let init = self.synthesize_default_init(class_sym, class_ty, span);
        self.add_exception_args_param(init, class_ty, span)?;
        Ok(init)
    }

    /// The protocol every user exception class carries: its builtin base and
    /// its root's `args` getter.
    fn exception_class_protocol(&mut self, class_name: &str, span: Span) -> Result<Option<ClassProtocol>, SmeltError> {
        let Some((root, builtin_name)) = self
            .exception_root_and_builtin(class_name)
            .map(|(root, builtin)| (root.to_owned(), builtin.to_owned()))
        else {
            return Ok(None);
        };
        let Some(&args) = self.exception_args_getters.get(&root) else {
            return Err(SmeltError::unsupported(
                span,
                format!("exception class '{class_name}': its base class '{root}' did not lower"),
            ));
        };
        let builtin_base = self.intern_name(&builtin_name);
        Ok(Some(ClassProtocol::PyException { builtin_base, args }))
    }

    /// `super().__init__(args)` reaching `BaseException.__init__` (see
    /// `super_init_reaches_base_exception`), which stores `args` on the
    /// instance `receiver` (`self`, a local of type `class_ty`).
    fn exception_root_super_init(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        (self_local, class_ty): (smelt_hir::LocalId, TypeId),
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(call.range());
        if !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(span, "`BaseException.__init__` takes no keyword arguments"));
        }
        let args = self.exception_args_values(&call.arguments.args, body)?;
        let args_ty = self.py_exception_args_type();
        let args_sym = self.intern_name("args");
        let packed = body.push_expr(HirExpr { kind: ExprKind::PyExceptionArgsNew { args }, ty: args_ty, span });
        let self_read = body.push_expr(HirExpr { kind: ExprKind::Local(self_local), ty: class_ty, span });
        let target = body.push_expr(HirExpr { kind: ExprKind::Field { receiver: self_read, field: args_sym }, ty: args_ty, span });
        body.push_stmt_to_block(block, HirStmt::Assign { target, value: packed });
        Ok(())
    }

    /// Lower the values `BaseException` keeps as `args`: each needs a Python
    /// `str` and `repr`.
    fn exception_args_values(&mut self, args: &[Expr], body: &mut Body) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        let mut lowered = Vec::with_capacity(args.len());
        for arg in args {
            if matches!(arg, Expr::Starred(_)) {
                return Err(SmeltError::unsupported(
                    self.span(arg.range()),
                    "a starred exception constructor argument is not supported",
                ));
            }
            let value = self.expression(arg, body)?;
            self.require_exception_arg_text(value, arg.range(), body)?;
            lowered.push(value);
        }
        Ok(lowered)
    }

    /// Refuse a value an exception keeps in `args` when it has no Python text.
    fn require_exception_arg_text(&self, value: smelt_hir::ExprId, range: TextRange, body: &Body) -> Result<(), SmeltError> {
        if self.has_python_text(Self::expr_ty(body, value)) {
            return Ok(());
        }
        Err(SmeltError::unsupported(
            self.span(range),
            "an exception keeps this argument in `args`, so it must be a str, int, float, bool, None, an exception, a class with `__repr__`, or a list or tuple of those",
        ))
    }

    /// `C(args)` for the user exception class `class_name`: an ordinary
    /// construction, whose constructor first takes the positional arguments
    /// packed when the class keeps them as `args` (each evaluated once).
    fn user_exception_new(
        &mut self,
        class_name: &str,
        call_args: &[Expr],
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(&item_id) = self.items.get(class_name) else {
            return Err(if self.failed_definitions.contains(class_name) {
                SmeltError::cascade(span, class_name)
            } else {
                SmeltError::unsupported(span, format!("exception class '{class_name}' is not defined here"))
            });
        };
        let Item::Class(class) = self.item_ref(item_id) else {
            return Err(SmeltError::unsupported(span, format!("'{class_name}' is not a class")));
        };
        if matches!(class.kind, ClassKind::Abstract) {
            return Err(SmeltError::unsupported(span, format!("abstract class '{class_name}' cannot be constructed")));
        }
        let class_sym = class.name;
        let class_ty = self.intern_type(Type::Class { name: class_sym, args: vec![] });
        let constructor = self
            .exception_constructors
            .get(class_name)
            .copied()
            .unwrap_or(ExceptionConstructor { keeps_args: true, has_init: false });
        if !constructor.keeps_args {
            let args = call_args.iter().map(|arg| self.expression(arg, body)).collect::<Result<Vec<_>, _>>()?;
            return Ok(body.push_expr(HirExpr { kind: ExprKind::New { class: class_sym, args }, ty: class_ty, span }));
        }
        let args_ty = self.py_exception_args_type();
        if !constructor.has_init {
            let values = self.exception_args_values(call_args, body)?;
            let packed = body.push_expr(HirExpr { kind: ExprKind::PyExceptionArgsNew { args: values }, ty: args_ty, span });
            return Ok(body.push_expr(HirExpr { kind: ExprKind::New { class: class_sym, args: vec![packed] }, ty: class_ty, span }));
        }
        // Each argument is packed and also passed on to `__init__`: it is read
        // twice, so anything but a local is bound once to a temporary.
        let block = body.push_block(span);
        let mut packed_reads = Vec::with_capacity(call_args.len());
        let mut passed = Vec::with_capacity(call_args.len());
        for arg in call_args {
            if matches!(arg, Expr::Starred(_)) {
                return Err(SmeltError::unsupported(
                    self.span(arg.range()),
                    "a starred exception constructor argument is not supported",
                ));
            }
            let value = self.expression(arg, body)?;
            self.require_exception_arg_text(value, arg.range(), body)?;
            let ty = Self::expr_ty(body, value);
            let index = usize::try_from(value.0).unwrap_or(usize::MAX);
            let local = if let Some(ExprKind::Local(local)) = body.exprs.get(index).map(|expr| &expr.kind) {
                *local
            } else {
                let name = self.intern_synthesized_name("__smelt_exception_arg");
                let local = body.push_local(LocalDecl { name: Some(name), ty, mutable: false, span });
                let pat = body.push_pattern(HirPattern::Binding(local));
                body.push_stmt_to_block(block, HirStmt::Let { pat, ty, value: Some(value) });
                local
            };
            packed_reads.push(body.push_expr(HirExpr { kind: ExprKind::Local(local), ty, span }));
            passed.push(body.push_expr(HirExpr { kind: ExprKind::Local(local), ty, span }));
        }
        let packed = body.push_expr(HirExpr { kind: ExprKind::PyExceptionArgsNew { args: packed_reads }, ty: args_ty, span });
        let mut args = vec![packed];
        args.extend(passed);
        let constructed = body.push_expr(HirExpr { kind: ExprKind::New { class: class_sym, args }, ty: class_ty, span });
        let block_index = usize::try_from(block.0).map_err(|error| SmeltError::unsupported(span, error.to_string()))?;
        body.blocks[block_index].tail = Some(constructed);
        Ok(body.push_expr(HirExpr { kind: ExprKind::Block(block), ty: class_ty, span }))
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
        let name = self.intern_synthesized_name(smelt_stdlib::PY_EXCEPTION_CLASS_NAME);
        self.intern_type(Type::Class { name, args: vec![] })
    }

    /// Whether the runtime writes Python's `str` and `repr` of a value of type
    /// `ty` (its `SmeltPyRepr`): a number, a string, a boolean, `None`, an
    /// exception or its `args`, and a list, tuple, set or dict of those. Such a
    /// value can be an exception constructor argument and the operand of
    /// `str()`, `repr()` and an f-string interpolation.
    fn has_python_text(&self, ty: TypeId) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::PyInt | Type::Float | Type::Bool | Type::PyStr | Type::None) => true,
            Some(Type::Optional(item, _) | Type::List(item)) => self.has_python_text(*item),
            // A union of such members writes its live member's text (the
            // emitter's `union_has_py_repr` holds the same condition).
            Some(Type::Union(union_members)) => {
                let members = union_members.clone();
                members.iter().any(|member| matches!(self.ctx.krate.types.get(*member), Some(Type::PyInt | Type::PyStr)))
                    && members.iter().all(|member| {
                        !matches!(self.ctx.krate.types.get(*member), Some(Type::Union(_))) && self.has_python_text(*member)
                    })
            }
            // The runtime writes a tuple's `repr` for one to six elements.
            Some(Type::Tuple(items)) => {
                (1..=6).contains(&items.len()) && items.iter().all(|item| self.has_python_text(*item))
            }
            Some(Type::Set(item)) => matches!(
                self.ctx.krate.types.get(*item),
                Some(Type::PyInt | Type::Bool | Type::PyStr)
            ),
            Some(Type::Dict(key, value)) => {
                matches!(self.ctx.krate.types.get(*key), Some(Type::PyInt | Type::Bool | Type::PyStr))
                    && self.has_python_text(*value)
            }
            // A class instance shows its `__repr__` inside a container; a
            // class with only `__str__` has `CPython`'s address-bearing repr,
            // which no port can reproduce.
            Some(Type::Class { .. }) if self.class_text_method(ty, true).is_some() => true,
            Some(Type::Class { name, .. }) => matches!(
                self.ctx.krate.symbols.get(*name),
                Some(smelt_stdlib::PY_EXCEPTION_CLASS_NAME | smelt_stdlib::PY_EXCEPTION_ARGS_CLASS_NAME)
            ),
            _ => false,
        }
    }

    /// The method that answers `repr(x)` (`repr`) or `str(x)` for a class
    /// instance of type `ty`: `__repr__`, and for `str` the class's `__str__`
    /// or, without one, its `__repr__` (`object.__str__` calls `repr`).
    fn class_text_method(&self, ty: TypeId, repr: bool) -> Option<&'static str> {
        let candidates: &[&'static str] = if repr { &["__repr__"] } else { &["__str__", "__repr__"] };
        candidates.iter().copied().find(|method| self.class_method_dispatch(ty, method).is_some())
    }

    /// `ascii(value)`: `repr(value)` with every non-ASCII code point escaped
    /// (`\xe9`, `\u20ac`, `\U0001f600`), as `CPython` writes it for any
    /// object; also `!a` in an f-string and `%a`.
    fn python_ascii_expr(
        &mut self,
        value: smelt_hir::ExprId,
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let text = self.python_text_expr(value, true, span, body)?;
        let ty = self.intern_type(Type::PyStr);
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::PyStrCall { op: smelt_hir::PyStrOp::AsciiEscape, args: vec![text] },
            ty,
            span,
        }))
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
        let str_ty = self.intern_type(Type::PyStr);
        if !repr && value_ty == str_ty {
            return Ok(value);
        }
        // A class instance's text is its own `__str__`/`__repr__`.
        if let Some(method) = self.class_text_method(value_ty, repr) {
            return self.protocol_method_expr(
                ProtocolMethodCall { receiver: value, method: method.to_owned(), args: Vec::new() },
                body,
            );
        }
        // `repr(n)` of an `int` is `str(n)`, its decimal digits.
        if matches!(self.ctx.krate.types.get(value_ty), Some(Type::PyInt)) {
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::PrimitiveCast { op: PrimitiveCastOp::ToString, operand: value },
                ty: str_ty,
                span,
            }));
        }
        if !repr && matches!(self.ctx.krate.types.get(value_ty), Some(Type::Bool | Type::PyInt | Type::Float)) {
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
    /// the positional arguments `args`: a builtin exception's
    /// `SmeltPyException`, or for a user exception class an ordinary
    /// construction raised through `PyExceptionFromInstance`.
    fn exception_new_expr(
        &mut self,
        chain: &[String],
        args: &[Expr],
        span: Span,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(class) = chain.first()
            && self.exception_classes.contains_key(class)
        {
            return self.user_exception_new(&class.clone(), args, span, body);
        }
        let lowered = self.exception_args_values(args, body)?;
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

    /// The `SmeltPyException` a `raise` of `value` raises: the value itself,
    /// or the exception carrying it when it is an instance of a user
    /// exception class. `None` when `value` is not an exception.
    fn raised_exception(&mut self, value: smelt_hir::ExprId, span: Span, body: &mut Body) -> Option<smelt_hir::ExprId> {
        let ty = self.py_exception_type();
        let value_ty = Self::expr_ty(body, value);
        if value_ty == ty {
            return Some(value);
        }
        self.user_exception_class_of(value_ty)?;
        Some(body.push_expr(HirExpr { kind: ExprKind::PyExceptionFromInstance { instance: value }, ty, span }))
    }

    /// Lower `raise`, `raise E`, `raise E(args)` and `raise E from cause`.
    ///
    /// A bare `raise` re-raises the exception the innermost enclosing `except`
    /// clause caught. A class named without a call is instantiated with no
    /// arguments, as Python does. The `from` cause only sets `__cause__`, which
    /// nothing but a traceback shows; it is evaluated after the exception, as
    /// `CPython` does, unless it is a name or `None` (whose evaluation cannot be
    /// observed).
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
                let value = self.exception_new_expr(&chain, &[], span, body)?;
                self.raised_exception(value, span, body)
                    .ok_or_else(|| SmeltError::unsupported(span, "`raise` needs an exception class or instance"))?
            }
            Some(exc) => {
                let value = self.expression(exc, body)?;
                self.raised_exception(value, span, body).ok_or_else(|| {
                    SmeltError::unsupported(self.span(exc.range()), "`raise` needs an exception class or instance")
                })?
            }
        };
        let thrown = match raise_stmt.cause.as_deref() {
            None | Some(Expr::Name(_) | Expr::NoneLiteral(_)) => exception,
            Some(cause) => {
                // The exception is evaluated first: bind it, then evaluate the
                // cause for its effects.
                let ty = self.py_exception_type();
                let name = self.intern_synthesized_name("__smelt_raised");
                let local = body.push_local(LocalDecl { name: Some(name), ty, mutable: false, span });
                let pat = body.push_pattern(HirPattern::Binding(local));
                body.push_stmt_to_block(block, HirStmt::Let { pat, ty, value: Some(exception) });
                let cause_value = self.expression(cause, body)?;
                let cause_ty = Self::expr_ty(body, cause_value);
                if cause_ty != ty && self.user_exception_class_of(cause_ty).is_none() {
                    return Err(SmeltError::unsupported(
                        self.span(cause.range()),
                        "`raise ... from` needs an exception class, an exception instance or None",
                    ));
                }
                body.push_stmt_to_block(block, HirStmt::Expr(cause_value));
                body.push_expr(HirExpr { kind: ExprKind::Local(local), ty, span })
            }
        };
        body.push_stmt_to_block(block, HirStmt::Throw(thrown));
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
            // A clause naming only user classes binds the caught instance at
            // their nearest common class; any other binds the exception.
            let bound_class = match handler.type_.as_deref() {
                Some(type_expr) if handler.name.is_some() => self.except_clause_common_class(type_expr)?,
                _ => None,
            };
            self.with_block_scope(|this| {
                if let Some(name) = &handler.name {
                    let caught_read = body.push_expr(HirExpr { kind: ExprKind::Local(caught), ty: exception_ty, span: handler_span });
                    let (binding_ty, value) = bound_class.map_or((exception_ty, caught_read), |class| {
                        let class_ty = this.intern_type(Type::Class { name: class, args: vec![] });
                        let instance = body.push_expr(HirExpr {
                            kind: ExprKind::PyExceptionInstance { exception: caught_read, class },
                            ty: class_ty,
                            span: handler_span,
                        });
                        (class_ty, instance)
                    });
                    let local = body.push_local(LocalDecl {
                        name: Some(this.intern_name(name.as_str())),
                        ty: binding_ty,
                        mutable: true,
                        span: this.span(name.range),
                    });
                    this.scope.locals.insert(name.as_str().to_owned(), local);
                    let pat = body.push_pattern(HirPattern::Binding(local));
                    body.push_stmt_to_block(handler_block, HirStmt::Let { pat, ty: binding_ty, value: Some(value) });
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

    /// The class an `except` clause naming `type_expr` binds its caught
    /// instance as: the nearest class every named class derives from, when
    /// each is a user exception class and that common class is one too.
    fn except_clause_common_class(&mut self, type_expr: &Expr) -> Result<Option<Symbol>, SmeltError> {
        let chains = self.except_clause_chains(type_expr)?;
        let Some((first, rest)) = chains.split_first() else {
            return Ok(None);
        };
        let common = first.iter().find(|class| {
            self.exception_classes.contains_key(*class) && rest.iter().all(|chain| chain.contains(class))
        });
        Ok(match common {
            Some(class) if self.items.contains_key(class) => Some(self.intern_name(class)),
            _ => None,
        })
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
        let name = self.intern_synthesized_name(smelt_stdlib::PY_EXCEPTION_ARGS_CLASS_NAME);
        let ty = self.intern_type(Type::Class { name, args: vec![] });
        Ok(body.push_expr(HirExpr { kind: ExprKind::PyExceptionArgs { exception }, ty, span }))
    }
}
