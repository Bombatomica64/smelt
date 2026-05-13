impl ModuleBuilder<'_> {
    /// Lower a Python class definition into the current HIR module.
    fn class_def(
        &mut self,
        class: &StmtClassDef,
        hir_module: &mut Module,
    ) -> Result<(), SmeltError> {
        let span = self.span(class.range);
        let class_name_str = class.name.as_str();
        let class_sym = self.intern_name(class_name_str);
        let class_ty = self.intern_type(Type::Class {
            name: class_sym,
            args: vec![],
        });

        // --- Decorator check: only @dataclass is allowed ---
        let mut kind = ClassKind::Plain;
        for dec in &class.decorator_list {
            match decorator_simple_name(dec) {
                Some(n @ ("dataclass" | "dataclasses.dataclass")) => {
                    let frozen = decorator_frozen_kwarg(dec);
                    kind = ClassKind::DataclassLike { frozen };
                    let _ = n;
                }
                Some(other) => {
                    let other_owned = other.to_owned();
                    return Err(SmeltError::unsupported_decorator(
                        span,
                        class_name_str,
                        &other_owned,
                    ));
                }
                None => {
                    return Err(SmeltError::unsupported(
                        span,
                        format!(
                            "class '{class_name_str}': complex decorator expressions are not supported"
                        ),
                    ));
                }
            }
        }

        // --- Metaclass check ---
        if let Some(args) = class.arguments.as_deref() {
            for kw in args.keywords.iter() {
                if kw.arg.as_ref().map(|a| a.as_str()) == Some("metaclass") {
                    return Err(SmeltError::no_metaclass(span, class_name_str));
                }
            }
        }

        // --- Base classes ---
        let base: Option<Symbol> = if let Some(args) = class.arguments.as_deref() {
            let positional: Vec<&Expr> = args.args.iter().collect();
            match positional.len() {
                0 => None,
                1 => {
                    let [base_expr] = positional.as_slice() else {
                        return Err(SmeltError::unsupported(
                            span,
                            format!(
                                "class '{class_name_str}': complex base class expression not supported"
                            ),
                        ));
                    };
                    if is_django_model_base(base_expr) {
                        return Err(SmeltError::django_unsupported(span, class_name_str));
                    }
                    match base_class_name(base_expr) {
                        Some("object") => None,
                        Some(name) => Some(self.intern_name(name)),
                        None => {
                            return Err(SmeltError::unsupported(
                                span,
                                format!(
                                    "class '{class_name_str}': complex base class expression not supported"
                                ),
                            ));
                        }
                    }
                }
                _ => return Err(SmeltError::no_multiple_inheritance(span, class_name_str)),
            }
        } else {
            None
        };

        // --- Fields and methods ---
        let mut fields: Vec<Field> = Vec::new();
        let mut constructor_id: Option<ItemId> = None;
        let mut method_ids: Vec<ItemId> = Vec::new();
        let is_int_enum = base
            .and_then(|base_sym| self.ctx.krate.symbols.get(base_sym))
            .is_some_and(|base_name| base_name == "IntEnum");
        let class_item_id = self.ctx.krate.push_item(Item::Class(Class {
            name: class_sym,
            span,
            kind: kind.clone(),
            base,
            fields: Vec::new(),
            constructor: None,
            methods: Vec::new(),
            implements: vec![],
        }));
        self.items.insert(class_name_str.to_owned(), class_item_id);
        let mut enum_members = HashMap::new();

        for body_stmt in &class.body {
            match body_stmt {
                Stmt::AnnAssign(ann) => {
                    let Expr::Name(target_name) = ann.target.as_ref() else {
                        return Err(SmeltError::unsupported(
                            self.span(ann.range),
                            "class field target must be a simple name",
                        ));
                    };
                    let field_name_str = target_name.id.as_str();
                    let field_ty = self.annotation_to_hir(&ann.annotation)?;
                    let field_sym = self.intern_name(field_name_str);
                    fields.push(Field {
                        name: field_sym,
                        ty: field_ty,
                        visibility: Visibility::Public,
                        optional: false,
                        span: self.span(ann.range),
                    });
                }
                Stmt::Assign(assign) if is_int_enum => {
                    self.int_enum_member_assign(class_name_str, assign, &mut enum_members)?;
                }
                Stmt::FunctionDef(func) => {
                    let method_name = func.name.as_str();
                    if method_name == "__init__" {
                        if matches!(kind, ClassKind::DataclassLike { .. }) {
                            return Err(SmeltError::unsupported(
                                self.span(func.range),
                                format!(
                                    "class '{class_name_str}': @dataclass must not define __init__ manually"
                                ),
                            ));
                        }
                        let mid = self.class_method(class_name_str, class_sym, class_ty, func)?;
                        constructor_id = Some(mid);
                        hir_module.items.push(mid);
                    } else {
                        let mid = self.class_method(class_name_str, class_sym, class_ty, func)?;
                        method_ids.push(mid);
                        self.class_methods
                            .entry(class_name_str.to_owned())
                            .or_default()
                            .insert(method_name.to_owned(), mid);
                        hir_module.items.push(mid);
                    }
                }
                Stmt::Pass(_) => {}
                // Docstring (bare string literal as Expr statement)
                Stmt::Expr(e) => {
                    if !matches!(e.value.as_ref(), Expr::StringLiteral(_)) {
                        return Err(SmeltError::unsupported(
                            self.span(e.range),
                            format!("class '{class_name_str}': unsupported class body statement"),
                        ));
                    }
                }
                Stmt::ClassDef(_)
                | Stmt::Return(_)
                | Stmt::Delete(_)
                | Stmt::TypeAlias(_)
                | Stmt::Assign(_)
                | Stmt::AugAssign(_)
                | Stmt::For(_)
                | Stmt::While(_)
                | Stmt::If(_)
                | Stmt::With(_)
                | Stmt::Match(_)
                | Stmt::Raise(_)
                | Stmt::Try(_)
                | Stmt::Assert(_)
                | Stmt::Import(_)
                | Stmt::ImportFrom(_)
                | Stmt::Global(_)
                | Stmt::Nonlocal(_)
                | Stmt::Break(_)
                | Stmt::Continue(_)
                | Stmt::IpyEscapeCommand(_) => {
                    return Err(SmeltError::unsupported(
                        self.span(body_stmt.range()),
                        format!(
                            "class '{class_name_str}': unsupported class body statement '{}'",
                            stmt_kind_name(body_stmt)
                        ),
                    ));
                }
            }
        }

        // @dataclass: synthesize __init__ from fields
        if matches!(kind, ClassKind::DataclassLike { .. }) {
            if fields.is_empty() {
                return Err(SmeltError::unsupported(
                    span,
                    format!(
                        "class '{class_name_str}': @dataclass requires at least one annotated field"
                    ),
                ));
            }
            let init_id = self.synthesize_dataclass_init(class_sym, class_ty, &fields, span)?;
            constructor_id = Some(init_id);
            hir_module.items.push(init_id);
        } else if constructor_id.is_none() {
            let init_id = self.synthesize_default_init(class_sym, class_ty, span);
            constructor_id = Some(init_id);
            hir_module.items.push(init_id);
        }
        if is_int_enum {
            self.ctx
                .enum_members
                .insert(class_name_str.to_owned(), enum_members.clone());
            self.enum_members
                .insert(class_name_str.to_owned(), enum_members);
        }

        let class_item = Item::Class(Class {
            name: class_sym,
            span,
            kind,
            base,
            fields,
            constructor: constructor_id,
            methods: method_ids,
            implements: vec![],
        });
        let class_index = usize::try_from(class_item_id.0).map_err(|err| {
            SmeltError::unsupported(
                span,
                format!("internal error: class item id does not fit in usize: {err}"),
            )
        })?;
        if let Some(slot) = self.ctx.krate.items.get_mut(class_index) {
            *slot = class_item;
        }
        self.exports
            .insert(class_name_str.to_owned(), class_item_id);
        hir_module.items.push(class_item_id);

        Ok(())
    }

    /// Lower one targeted `IntEnum` member assignment from a class body.
    ///
    /// This intentionally handles only the HTTPX-style enum forms needed by
    /// the object-model slice: `NAME = 200`, `NAME = 200, "OK"`, and member
    /// aliases that refer to another integer member on the same class.
    fn int_enum_member_assign(
        &mut self,
        class_name: &str,
        assign: &ruff_python_ast::StmtAssign,
        enum_members: &mut HashMap<String, i64>,
    ) -> Result<(), SmeltError> {
        if assign.targets.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(assign.range),
                format!("class '{class_name}': IntEnum members require one assignment target"),
            ));
        }
        let [target] = assign.targets.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(assign.range),
                format!("class '{class_name}': IntEnum members require one assignment target"),
            ));
        };
        let Expr::Name(name) = target else {
            return Err(SmeltError::unsupported(
                self.span(target.range()),
                format!("class '{class_name}': IntEnum member target must be a simple name"),
            ));
        };
        let value = self.int_enum_member_value(class_name, &assign.value, enum_members)?;
        enum_members.insert(name.id.as_str().to_owned(), value);
        self.enum_members
            .entry(class_name.to_owned())
            .or_default()
            .insert(name.id.as_str().to_owned(), value);
        self.ctx
            .enum_members
            .entry(class_name.to_owned())
            .or_default()
            .insert(name.id.as_str().to_owned(), value);
        Ok(())
    }

    /// Extract the integer value from a supported `IntEnum` member expression.
    ///
    /// Tuple-valued enum declarations keep their first element as the member's
    /// integer value, matching `IntEnum.__new__` patterns without modelling the
    /// full Python enum metaclass.
    fn int_enum_member_value(
        &mut self,
        class_name: &str,
        expr: &Expr,
        enum_members: &HashMap<String, i64>,
    ) -> Result<i64, SmeltError> {
        match expr {
            Expr::NumberLiteral(number) => match &number.value {
                Number::Int(value) => value.as_i64().ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(number.range),
                        "IntEnum integer literal out of i64 range",
                    )
                }),
                Number::Float(_) | Number::Complex { .. } => Err(SmeltError::unsupported(
                    self.span(number.range),
                    "IntEnum members must use integer values",
                )),
            },
            Expr::Tuple(tuple) => {
                let Some(first) = tuple.elts.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(tuple.range),
                        "IntEnum tuple member declarations require an integer first element",
                    ));
                };
                self.int_enum_member_value(class_name, first, enum_members)
            }
            Expr::Attribute(attr) => {
                let Expr::Name(receiver) = attr.value.as_ref() else {
                    return Err(SmeltError::unsupported(
                        self.span(attr.range),
                        "IntEnum member aliases must refer to the enum class directly",
                    ));
                };
                if receiver.id.as_str() != class_name {
                    return Err(SmeltError::unsupported(
                        self.span(attr.range),
                        "IntEnum member aliases must refer to the enum class directly",
                    ));
                }
                enum_members
                    .get(attr.attr.as_str())
                    .copied()
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(attr.range),
                            format!(
                                "class '{class_name}': unknown IntEnum member '{}'",
                                attr.attr
                            ),
                        )
                    })
            }
            Expr::Call(call) if Self::is_class_dunder_new_call(class_name, call) => {
                let Some(value_expr) = call.arguments.args.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(call.range),
                        "IntEnum __new__ member calls require a value argument",
                    ));
                };
                self.int_enum_member_value(class_name, value_expr, enum_members)
            }
            _ => Err(SmeltError::unsupported(
                self.span(expr.range()),
                format!("class '{class_name}': unsupported IntEnum member value"),
            )),
        }
    }

    /// Return whether a call expression is `ClassName.__new__(...)`.
    fn is_class_dunder_new_call(class_name: &str, call: &ruff_python_ast::ExprCall) -> bool {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return false;
        };
        attr.attr.as_str() == "__new__"
            && matches!(attr.value.as_ref(), Expr::Name(receiver) if receiver.id.as_str() == class_name)
    }

    /// Lower a method or constructor inside a class body.
    ///
    /// Handles `self` parameter injection, param annotation enforcement, and
    /// body lowering.  Returns the [`ItemId`] of the generated `Function` item.
    fn class_method(
        &mut self,
        class_name_str: &str,
        class_sym: Symbol,
        class_ty: TypeId,
        func: &StmtFunctionDef,
    ) -> Result<ItemId, SmeltError> {
        let span = self.span(func.range);
        let method_name_str = func.name.as_str();
        let method_sym = self.intern_name(method_name_str);
        let is_init = method_name_str == "__init__";
        let is_new = method_name_str == "__new__";
        let is_classmethod = self.is_classmethod(func)?;

        let return_ty = if is_init {
            self.intern_type(Type::None)
        } else if is_new && func.returns.is_none() {
            class_ty
        } else {
            func.returns
                .as_deref()
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        span,
                        format!(
                            "method '{class_name_str}.{method_name_str}' must have an explicit return type annotation"
                        ),
                    )
                })
                .and_then(|ann| self.annotation_to_hir(ann))?
        };

        let saved_locals = std::mem::take(&mut self.locals);
        let mut fn_body = Body::new(None, span);
        let mut params: Vec<Param> = Vec::new();

        // Add the implicit receiver local for use inside the method body.
        let implicit_receiver_name = if is_classmethod || is_new {
            "cls"
        } else {
            "self"
        };
        let self_sym = self.intern_name(implicit_receiver_name);
        let self_local = fn_body.push_local(LocalDecl {
            name: Some(self_sym),
            ty: class_ty,
            mutable: false,
            span,
        });
        self.locals
            .insert(implicit_receiver_name.to_owned(), self_local);
        if !is_init && !is_classmethod && !is_new {
            fn_body.params.push(self_local);
            params.push(Param {
                name: self_sym,
                local: self_local,
                ty: class_ty,
                span,
            });
        }

        let mut first = true;
        for param_with_default in func.parameters.iter_non_variadic_params() {
            let p = &param_with_default.parameter;
            let param_name_str = p.name.as_str();
            // Skip the implicit receiver; it was added above.
            if first
                && (param_name_str == implicit_receiver_name
                    || ((is_classmethod || is_new) && param_name_str == "self"))
            {
                first = false;
                continue;
            }
            first = false;

            let param_ty = p
                .annotation
                .as_deref()
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(p.range),
                        format!(
                            "parameter '{param_name_str}' in '{class_name_str}.{method_name_str}' must have a type annotation"
                        ),
                    )
                })
                .and_then(|ann| self.annotation_to_hir(ann))?;

            let param_sym = self.intern_name(param_name_str);
            let local = fn_body.push_local(LocalDecl {
                name: Some(param_sym),
                ty: param_ty,
                mutable: false,
                span: self.span(p.range),
            });
            fn_body.params.push(local);
            self.locals.insert(param_name_str.to_owned(), local);
            params.push(Param {
                name: param_sym,
                local,
                ty: param_ty,
                span: self.span(p.range),
            });
        }

        for stmt in &func.body {
            if let Err(err) = self.statement(stmt, &mut fn_body) {
                self.locals = saved_locals;
                return Err(err);
            }
        }

        self.locals = saved_locals;

        let body_id = self.ctx.krate.push_body(fn_body);

        let owner = if is_init {
            FunctionOwner::Constructor { class: class_sym }
        } else {
            FunctionOwner::ClassMethod {
                class: class_sym,
                method: method_sym,
            }
        };

        let item = Item::Function(Function {
            name: method_sym,
            span,
            params,
            return_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner,
        });
        Ok(self.ctx.krate.push_item(item))
    }

    // Class helper lowering continues in `class_init.rs`.
}
