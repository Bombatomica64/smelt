impl ModuleBuilder<'_> {
    /// Return whether a Python function definition is decorated as a class method.
    fn is_classmethod(&self, func: &StmtFunctionDef) -> Result<bool, SmeltError> {
        let mut is_classmethod = false;
        for decorator in &func.decorator_list {
            match decorator_simple_name(decorator) {
                Some("classmethod") => is_classmethod = true,
                // A `@property` getter is an ordinary instance method here; the
                // read-only descriptor that exposes it under field syntax is
                // registered separately by `property_descriptor`.
                Some("property") => {}
                Some(_) if self.is_materialized_descriptor_callable(func) => {}
                Some(other) => {
                    return Err(SmeltError::unsupported_decorator(
                        self.span(decorator.range),
                        func.name.as_str(),
                        other,
                    ));
                }
                None => {
                    return Err(SmeltError::unsupported(
                        self.span(decorator.range),
                        format!(
                            "method '{}': complex decorator expressions are not supported",
                            func.name
                        ),
                    ));
                }
            }
        }
        Ok(is_classmethod)
    }

    /// Whether a method is a source `@property` getter.
    ///
    /// Only the read side is recognized: `@property` itself, not the paired
    /// `@name.setter` / `@name.deleter`, which stay unsupported and are reported
    /// by [`Self::is_classmethod`]'s decorator check.
    fn is_property_getter(&self, func: &StmtFunctionDef) -> bool {
        // A manifest-backed descriptor already declares this member, with richer
        // information than the source getter carries (write type, data-descriptor
        // precedence, materialized instance state). Registering a source
        // descriptor as well would declare the member twice.
        if self.is_materialized_descriptor_callable(func) {
            return false;
        }
        func.decorator_list
            .iter()
            .any(|decorator| decorator_simple_name(decorator) == Some("property"))
    }

    /// Build the read-only descriptor a source `@property` getter declares.
    ///
    /// Python exposes a property under *field* syntax (`value.ok_value`) while
    /// the source defines it as a method. Smelt already models exactly that
    /// shape for host-materialized descriptors — a `Descriptor` whose `getter`
    /// is a class method — and codegen emits `receiver.getter()` for a read
    /// whose getter lives on the receiver's own class. Registering the property
    /// as such a descriptor therefore reuses the whole existing path instead of
    /// adding a second property mechanism.
    ///
    /// The descriptor is read-only (`write_ty: None`, `data_descriptor: false`)
    /// because a bare `@property` has no setter; assigning through it is an
    /// error in Python too.
    fn property_descriptor(
        &mut self,
        func: &StmtFunctionDef,
        getter: ItemId,
    ) -> Result<smelt_hir::Descriptor, SmeltError> {
        let read_ty = match self.item_ref(getter) {
            Item::Function(function) => function.return_ty,
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(func.range()),
                    format!("property '{}' did not lower as a method", func.name),
                ));
            }
        };
        Ok(smelt_hir::Descriptor {
            name: self.intern_name(func.name.as_str()),
            read_ty,
            write_ty: None,
            getter: Some(getter),
            setter: None,
            data_descriptor: false,
            is_static: false,
            value_fields: Vec::new(),
        })
    }

    /// Read a dataclass field's class-body value as a `dataclasses.field(...)`
    /// call: `field(default=v)` is the default `v`, `field(default_factory=f)`
    /// the call `f()`. `None` when `value` is not a `field` call (an ordinary
    /// default). Any other `field` argument is refused.
    fn dataclass_field_default(&self, value: &Expr) -> Result<Option<DataclassDefault>, SmeltError> {
        let Expr::Call(call) = value else {
            return Ok(None);
        };
        let names_field = match call.func.as_ref() {
            Expr::Name(name) => self.import_aliases.get(name.id.as_str()).is_some_and(|target| target == "dataclasses.field"),
            Expr::Attribute(attr) => {
                attr.attr.as_str() == "field"
                    && matches!(attr.value.as_ref(), Expr::Name(module) if self.import_aliases.get(module.id.as_str()).is_some_and(|target| target == "dataclasses"))
            }
            _ => false,
        };
        if !names_field {
            return Ok(None);
        }
        let span = self.span(call.range());
        let unsupported = || {
            SmeltError::unsupported(span, "`dataclasses.field` is supported with exactly one of `default=` or `default_factory=`")
        };
        let ([], [keyword]) = (call.arguments.args.as_ref(), call.arguments.keywords.as_ref()) else {
            return Err(unsupported());
        };
        match keyword.arg.as_ref().map(ruff_python_ast::Identifier::as_str) {
            Some("default") => Ok(Some(DataclassDefault::Value(keyword.value.clone()))),
            Some("default_factory") => {
                let parsed = ruff_python_parser::parse_expression("__smelt_factory()").map_err(|error| SmeltError::unsupported(span, format!("internal: synthesized factory call does not parse: {error}")))?;
                let mut factory_call = *parsed.into_syntax().body;
                ruff_python_ast::relocate::relocate_expr(&mut factory_call, call.range());
                if let Expr::Call(synthesized) = &mut factory_call {
                    *synthesized.func = keyword.value.clone();
                }
                Ok(Some(DataclassDefault::Factory(factory_call)))
            }
            _ => Err(unsupported()),
        }
    }

    /// Synthesise an `__init__` method for a `@dataclass` class.
    ///
    /// Creates one parameter per annotated field and emits
    /// `self.field = param` assignments in the body. A field declared with a
    /// default (`label: str = "origin"`) takes it when the caller leaves the
    /// argument out, through the same callee-side default every function
    /// parameter uses (see `default_params.rs`).
    #[expect(clippy::too_many_arguments, reason = "one operation, its operands and its lowering position")]
    fn synthesize_dataclass_init(
        &mut self,
        class: &StmtClassDef,
        class_sym: Symbol,
        class_ty: TypeId,
        fields: &[Field],
        span: Span,
    ) -> Result<ItemId, SmeltError> {
        // Each field's default: its value, `field(default=v)`'s `v`, or for
        // `field(default_factory=f)` the call `f()`, which runs anew for
        // every instance that leaves the argument out.
        let mut defaults: HashMap<String, Expr> = HashMap::new();
        let mut factories: HashSet<String> = HashSet::new();
        for stmt in &class.body {
            let Stmt::AnnAssign(ann) = stmt else {
                continue;
            };
            let (Expr::Name(name), Some(value)) = (ann.target.as_ref(), ann.value.as_deref()) else {
                continue;
            };
            let field_name = name.id.as_str().to_owned();
            match self.dataclass_field_default(value)? {
                Some(DataclassDefault::Value(default)) => {
                    defaults.insert(field_name, default);
                }
                Some(DataclassDefault::Factory(call)) => {
                    defaults.insert(field_name.clone(), call);
                    factories.insert(field_name);
                }
                None => {
                    defaults.insert(field_name, value.clone());
                }
            }
        }
        let none_ty = self.intern_type(Type::None);
        let (fn_body, params) = self.with_fn_scope(FnKind::Sync, |this| {
            let mut fn_body = Body::new(None, span);
            let mut params: Vec<Param> = Vec::new();
            let mut field_locals: Vec<smelt_hir::LocalId> = Vec::new();

            // `self` local
            let self_sym = this.intern_name("self");
            let self_local = fn_body.push_local(LocalDecl {
                name: Some(self_sym),
                ty: class_ty,
                mutable: false,
                span,
            });
            this.scope.locals.insert("self".to_owned(), self_local);

            // One param per field; a defaulted one is `Optional` to callers.
            let mut pending_defaults = Vec::new();
            for field in fields {
                let field_name_str = this
                    .ctx
                    .krate
                    .symbols
                    .get(field.name)
                    .unwrap_or("")
                    .to_owned();
                let defaulted = match defaults.get(&field_name_str) {
                    // A factory's call is evaluated by every call that needs
                    // it, never shared.
                    Some(_) if factories.contains(&field_name_str) => Some(DefaultedParam {
                        declared_ty: field.ty,
                        optional_ty: this.intern_type(Type::Optional(field.ty, smelt_hir::Absence::Undefined)),
                        shared: None,
                    }),
                    Some(default) => this.defaulted_param(&field_name_str, field.ty, default, field.span)?,
                    None => None,
                };
                let param_ty = defaulted.as_ref().map_or(field.ty, |found| found.optional_ty);
                let local = fn_body.push_local(LocalDecl {
                    name: Some(field.name),
                    ty: param_ty,
                    mutable: false,
                    span: field.span,
                });
                fn_body.params.push(local);
                field_locals.push(local);
                this.scope.locals.insert(field_name_str.clone(), local);
                params.push(Param {
                    name: field.name,
                    local,
                    ty: param_ty,
                    span: field.span,
                });
                if let Some(found) = defaulted {
                    pending_defaults.push((field_name_str, local, found));
                }
            }
            for (name, local, found) in pending_defaults {
                let default = defaults.get(&name).cloned().ok_or_else(|| {
                    SmeltError::unsupported(span, "a dataclass default disappeared")
                })?;
                this.default_prologue(&name, local, &found, &default, &mut fn_body)?;
                // The prologue rebinds the name to the value with its default.
                if let Some(rebound) = this.scope.locals.get(&name).copied()
                    && let Some(slot) = field_locals.iter_mut().find(|slot| **slot == local)
                {
                    *slot = rebound;
                }
            }

            // Body: `self.field = param` for each field.
            let root_block = fn_body.root;
            for (field, param_local) in fields.iter().zip(field_locals.iter().copied()) {
                let param_ty = field.ty;

                let self_expr = fn_body.push_expr(HirExpr {
                    kind: ExprKind::Local(self_local),
                    ty: class_ty,
                    span,
                });
                let field_lhs = fn_body.push_expr(HirExpr {
                    kind: ExprKind::Field {
                        receiver: self_expr,
                        field: field.name,
                    },
                    ty: param_ty,
                    span: field.span,
                });
                let param_expr = fn_body.push_expr(HirExpr {
                    kind: ExprKind::Local(param_local),
                    ty: param_ty,
                    span: field.span,
                });
                fn_body.push_stmt_to_block(
                    root_block,
                    HirStmt::Assign {
                        target: field_lhs,
                        value: param_expr,
                    },
                );
            }
            Ok((fn_body, params))
        })?;
        let body_id = self.ctx.krate.push_body(fn_body);
        let init_sym = self.intern_name("__init__");
        let item = Item::Function(Function {
            name: init_sym,
            span,
            // Constructors take generics from the owning class.
            type_params: Vec::new(),
            params,
            rest: None,
            required_params: None,
return_ty: none_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Constructor { class: class_sym },
        });
        Ok(self.ctx.krate.push_item(item))
    }

    /// Synthesise `__repr__` for a `@dataclass` class that does not define
    /// one: `Point(x=1, y='a')`, each field's `repr` after its name, as the
    /// dataclass decorator generates. `None` when a field's type has no Python
    /// text, which leaves the class without one.
    fn synthesize_dataclass_repr(
        &mut self,
        class_sym: Symbol,
        class_ty: TypeId,
        fields: &[Field],
        span: Span,
    ) -> Result<Option<ItemId>, SmeltError> {
        if fields.iter().any(|field| !self.has_python_text(field.ty)) {
            return Ok(None);
        }
        let class_name = self.ctx.krate.symbols.source(class_sym).unwrap_or("").to_owned();
        let str_ty = self.intern_type(Type::PyStr);
        let (fn_body, self_local) = self.with_fn_scope(FnKind::Sync, |this| {
            let mut fn_body = Body::new(None, span);
            let self_sym = this.intern_name("self");
            let self_local = fn_body.push_local(LocalDecl { name: Some(self_sym), ty: class_ty, mutable: false, span });
            fn_body.params.push(self_local);
            this.scope.locals.insert("self".to_owned(), self_local);
            let literal = |target: &mut Body, text: String| {
                target.push_expr(HirExpr { kind: ExprKind::Literal(Literal::String(text)), ty: str_ty, span })
            };
            let concat = |target: &mut Body, lhs, rhs| {
                target.push_expr(HirExpr { kind: ExprKind::BinOp { op: BinOp::Add, lhs, rhs }, ty: str_ty, span })
            };
            let mut text = literal(&mut fn_body, format!("{class_name}("));
            for (index, field) in fields.iter().enumerate() {
                let name = this.ctx.krate.symbols.get(field.name).unwrap_or("").to_owned();
                let separator = if index == 0 { "" } else { ", " };
                let label = literal(&mut fn_body, format!("{separator}{name}="));
                text = concat(&mut fn_body, text, label);
                let receiver = fn_body.push_expr(HirExpr { kind: ExprKind::Local(self_local), ty: class_ty, span });
                let value = fn_body.push_expr(HirExpr {
                    kind: ExprKind::Field { receiver, field: field.name },
                    ty: field.ty,
                    span: field.span,
                });
                let shown = this.python_text_expr(value, true, span, &mut fn_body)?;
                text = concat(&mut fn_body, text, shown);
            }
            let close = literal(&mut fn_body, ")".to_owned());
            text = concat(&mut fn_body, text, close);
            let root = fn_body.root;
            fn_body.push_stmt_to_block(root, HirStmt::Return(Some(text)));
            Ok::<_, SmeltError>((fn_body, self_local))
        })?;
        let body_id = self.ctx.krate.push_body(fn_body);
        let self_param = Param {
            name: self.intern_name("self"),
            local: self_local,
            ty: class_ty,
            span,
        };
        let repr_sym = self.intern_name("__repr__");
        Ok(Some(self.ctx.krate.push_item(Item::Function(Function {
            name: repr_sym,
            span,
            type_params: Vec::new(),
            params: vec![self_param],
            rest: None,
            required_params: None,
            return_ty: str_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::ClassMethod { class: class_sym, method: repr_sym },
        }))))
    }

    /// Synthesize an empty constructor for a plain Python class without `__init__`.
    ///
    /// Rust codegen expects every constructed class to have a callable `new`.
    /// Python classes have an implicit zero-argument constructor when no custom
    /// initializer is present, so this keeps `ClassName()` available for known
    /// class constructors such as Rich's `NullFile`.
    fn synthesize_default_init(
        &mut self,
        class_sym: Symbol,
        class_ty: TypeId,
        span: Span,
    ) -> ItemId {
        let fn_body = self.with_fn_scope(FnKind::Sync, |this| {
            let mut fn_body = Body::new(None, span);
            let self_sym = this.intern_name("self");
            let self_local = fn_body.push_local(LocalDecl {
                name: Some(self_sym),
                ty: class_ty,
                mutable: false,
                span,
            });
            this.scope.locals.insert("self".to_owned(), self_local);
            let self_expr = fn_body.push_expr(HirExpr {
                kind: ExprKind::Local(self_local),
                ty: class_ty,
                span,
            });
            fn_body.blocks[usize::try_from(fn_body.root.0).unwrap_or(0)].tail = Some(self_expr);
            fn_body
        });
        let body_id = self.ctx.krate.push_body(fn_body);
        let init_sym = self.intern_name("__init__");
        self.ctx.krate.push_item(Item::Function(Function {
            name: init_sym,
            span,
            // Constructors take generics from the owning class.
            type_params: Vec::new(),
            params: Vec::new(),
            rest: None,
            required_params: None,
return_ty: class_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Constructor { class: class_sym },
        }))
    }

    /// Whether a constructor takes no arguments and runs no statements, so a
    /// subclass inheriting it has nothing to initialize from its base.
    fn constructor_does_nothing(&self, constructor: ItemId) -> bool {
        let Item::Function(function) = self.item_ref(constructor) else {
            return false;
        };
        // A queued body (`lowering/deferred_bodies.rs`) has no statements yet:
        // its source decides.
        function.params.is_empty()
            && function.body.is_some_and(|body_id| {
                self.ctx
                    .krate
                    .bodies
                    .get(usize::try_from(body_id.0).unwrap_or(usize::MAX))
                    .is_some_and(|body| body.stmts.is_empty())
                    && self.deferred_body_is_empty(body_id).unwrap_or(true)
            })
    }

    /// Synthesize the initializer Python inherits from a direct base class.
    ///
    /// Smelt flattens base fields into the derived Rust struct, so the base
    /// constructor cannot be reused as a Rust method returning the base type.
    /// The derived constructor instead accepts the same arguments, constructs
    /// the base value, and copies its effective fields into the derived `self`.
    /// This is the implicit counterpart of an explicit
    /// `super().__init__(...)` and composes through arbitrarily deep chains.
    fn synthesize_inherited_init(
        &mut self,
        class_sym: Symbol,
        class_ty: TypeId,
        base_sym: Symbol,
        span: Span,
    ) -> Result<ItemId, SmeltError> {
        let (base_constructor, base_name) = self
            .ctx
            .krate
            .items
            .iter()
            .find_map(|item| match item {
                Item::Class(class) if class.name == base_sym => class.constructor.map(|id| {
                    (
                        match self.item_ref(id) {
                            Item::Function(function) => Some(function.clone()),
                            _ => None,
                        },
                        self.ctx
                            .krate
                            .symbols
                            .get(base_sym)
                            .unwrap_or("")
                            .to_owned(),
                    )
                }),
                _ => None,
            })
            .and_then(|(constructor, name)| constructor.map(|value| (value, name)))
            .ok_or_else(|| {
                SmeltError::unsupported(span, "base class constructor is not available")
            })?;

        let (body, params) = self.with_fn_scope(FnKind::Sync, |this| {
            let mut body = Body::new(None, span);
            let self_local = body.push_local(LocalDecl {
                name: Some(this.intern_name("self")),
                ty: class_ty,
                mutable: false,
                span,
            });
            this.scope.locals.insert("self".to_owned(), self_local);

            let mut params = Vec::with_capacity(base_constructor.params.len());
            let mut args = Vec::with_capacity(base_constructor.params.len());
            for inherited in &base_constructor.params {
                let local = body.push_local(LocalDecl {
                    name: Some(inherited.name),
                    ty: inherited.ty,
                    mutable: false,
                    span: inherited.span,
                });
                body.params.push(local);
                params.push(Param {
                    local,
                    ..inherited.clone()
                });
                args.push(body.push_expr(HirExpr {
                    kind: ExprKind::Local(local),
                    ty: inherited.ty,
                    span: inherited.span,
                }));
            }

            let base_ty = this.intern_type(Type::Class {
                name: base_sym,
                args: Vec::new(),
            });
            let constructed = body.push_expr(HirExpr {
                kind: ExprKind::New {
                    class: base_sym,
                    args,
                },
                ty: base_ty,
                span,
            });
            let base_local = body.push_local(LocalDecl {
                name: Some(this.intern_synthesized_name("__smelt_super")),
                ty: base_ty,
                mutable: false,
                span,
            });
            let pattern = body.push_pattern(HirPattern::Binding(base_local));
            let root = body.root;
            body.push_stmt_to_block(
                root,
                HirStmt::Let {
                    pat: pattern,
                    ty: base_ty,
                    value: Some(constructed),
                },
            );

            for field in this.inherited_base_fields(&base_name) {
                let receiver = body.push_expr(HirExpr {
                    kind: ExprKind::Local(self_local),
                    ty: class_ty,
                    span,
                });
                let target = body.push_expr(HirExpr {
                    kind: ExprKind::Field {
                        receiver,
                        field: field.name,
                    },
                    ty: field.ty,
                    span: field.span,
                });
                let base = body.push_expr(HirExpr {
                    kind: ExprKind::Local(base_local),
                    ty: base_ty,
                    span,
                });
                let value = body.push_expr(HirExpr {
                    kind: ExprKind::Field {
                        receiver: base,
                        field: field.name,
                    },
                    ty: field.ty,
                    span: field.span,
                });
                body.push_stmt_to_block(root, HirStmt::Assign { target, value });
            }
            let result = body.push_expr(HirExpr {
                kind: ExprKind::Local(self_local),
                ty: class_ty,
                span,
            });
            body.blocks[usize::try_from(root.0).unwrap_or(0)].tail = Some(result);
            (body, params)
        });

        let body_id = self.ctx.krate.push_body(body);
        let item = Function {
            name: self.intern_name("__init__"),
            span,
            type_params: Vec::new(),
            params,
            rest: None,
            required_params: base_constructor.required_params,
            return_ty: class_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Constructor { class: class_sym },
        };
        Ok(self.ctx.krate.push_item(Item::Function(item)))
    }
}

/// A dataclass field's default, read from a `dataclasses.field(...)` call.
enum DataclassDefault {
    /// `field(default=v)`: the value `v`.
    Value(Expr),
    /// `field(default_factory=f)`: the call `f()`, run per instance.
    Factory(Expr),
}
