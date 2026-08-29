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

    /// Synthesise an `__init__` method for a `@dataclass` class.
    ///
    /// Creates one parameter per annotated field and emits
    /// `self.field = param` assignments in the body.
    fn synthesize_dataclass_init(
        &mut self,
        class_sym: Symbol,
        class_ty: TypeId,
        fields: &[Field],
        span: Span,
    ) -> Result<ItemId, SmeltError> {
        let saved_locals = std::mem::take(&mut self.locals);
        let none_ty = self.intern_type(Type::None);
        let mut fn_body = Body::new(None, span);
        let mut params: Vec<Param> = Vec::new();
        let mut field_locals: Vec<smelt_hir::LocalId> = Vec::new();

        // `self` local
        let self_sym = self.intern_name("self");
        let self_local = fn_body.push_local(LocalDecl {
            name: Some(self_sym),
            ty: class_ty,
            mutable: false,
            span,
        });
        self.locals.insert("self".to_owned(), self_local);

        // One param per field.
        for field in fields {
            let local = fn_body.push_local(LocalDecl {
                name: Some(field.name),
                ty: field.ty,
                mutable: false,
                span: field.span,
            });
            fn_body.params.push(local);
            field_locals.push(local);
            let field_name_str = self
                .ctx
                .krate
                .symbols
                .get(field.name)
                .unwrap_or("")
                .to_owned();
            self.locals.insert(field_name_str, local);
            params.push(Param {
                name: field.name,
                local,
                ty: field.ty,
                span: field.span,
            });
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

        self.locals = saved_locals;
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
        let saved_locals = std::mem::take(&mut self.locals);
        let mut fn_body = Body::new(None, span);
        let self_sym = self.intern_name("self");
        let self_local = fn_body.push_local(LocalDecl {
            name: Some(self_sym),
            ty: class_ty,
            mutable: false,
            span,
        });
        self.locals.insert("self".to_owned(), self_local);
        let self_expr = fn_body.push_expr(HirExpr {
            kind: ExprKind::Local(self_local),
            ty: class_ty,
            span,
        });
        fn_body.blocks[usize::try_from(fn_body.root.0).unwrap_or(0)].tail = Some(self_expr);
        self.locals = saved_locals;
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

        let saved_locals = std::mem::take(&mut self.locals);
        let mut body = Body::new(None, span);
        let self_local = body.push_local(LocalDecl {
            name: Some(self.intern_name("self")),
            ty: class_ty,
            mutable: false,
            span,
        });
        self.locals.insert("self".to_owned(), self_local);

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

        let base_ty = self.intern_type(Type::Class {
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
            name: Some(self.intern_name("__smelt_super")),
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

        for field in self.inherited_base_fields(&base_name) {
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
        self.locals = saved_locals;

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
