impl ModuleBuilder<'_> {
    /// Return whether a Python function definition is decorated as a class method.
    fn is_classmethod(&self, func: &StmtFunctionDef) -> Result<bool, SmeltError> {
        let mut is_classmethod = false;
        for decorator in &func.decorator_list {
            match decorator_simple_name(decorator) {
                Some("classmethod") => is_classmethod = true,
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
}
