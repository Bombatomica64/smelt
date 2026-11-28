// Python `enum.Enum` classes.
//
// An `Enum` class is an ordinary reference class whose instances are its
// members, each a singleton:
//
// - every member is a module global holding one instance, built once by an
//   initializer (`Color("RED", 1)` through the class's constructor), and
//   `Color.RED` reads that global, so `is` and `==` (identity, as for any
//   class without `__eq__`) compare members;
// - an instance has the fields `name: str` and `value: T`, `T` the type every
//   member's value shares; `auto()` numbers members from 1 after the previous
//   integer value, as `Enum._generate_next_value_` does;
// - `str(member)` is `Color.RED` and `repr(member)` is `<Color.RED: 1>`;
// - `Color(v)` is a lookup by value (`ValueError: 3 is not a valid Color` for
//   no match), not a construction;
// - the class used as a value (`for m in Color`, `list(Color)`, `len(Color)`)
//   is the list of its members in definition order.
//
// Methods and properties declared in the class body are ordinary methods.
// Not modelled (refused): a custom `__init__`/`__new__`, member values of
// different types, aliases (two members with one value), `Color["RED"]`.

/// What lowering an `Enum` class records for later uses of the class.
#[derive(Debug, Clone)]
struct EnumClass {
    /// The class symbol.
    class: Symbol,
    /// The members in definition order, each with the global holding it.
    members: Vec<(String, ItemId)>,
    /// The module function answering `Color(value)`.
    lookup: ItemId,
}

/// One member as the class body spells it.
struct EnumMemberDef {
    /// The member's name.
    name: String,
    /// Its value.
    value: Literal,
    /// Where it is declared.
    span: Span,
}

/// The parts of an `Enum` class added to its class item.
struct EnumParts {
    /// The `name` and `value` fields.
    fields: Vec<Field>,
    /// One class variable per member, read from the member's global.
    static_fields: Vec<smelt_hir::StaticField>,
    /// The constructor the member initializers call.
    constructor: ItemId,
    /// `__str__` and `__repr__`, unless the class declares them.
    methods: Vec<ItemId>,
}

impl ModuleBuilder<'_> {
    /// The members `class`'s body declares (`NAME = <literal>` or
    /// `NAME = auto()`), and the type their values share.
    fn enum_member_definitions(
        &mut self,
        class: &StmtClassDef,
        class_name: &str,
    ) -> Result<(Vec<EnumMemberDef>, TypeId), SmeltError> {
        let scope = self.bindings.class_statement_scope(class.range.start());
        let owner = format!("class '{class_name}'");
        let mut members: Vec<EnumMemberDef> = Vec::new();
        let mut last_int: Option<i64> = None;
        let mut value_ty: Option<TypeId> = None;
        for statement in &class.body {
            let Stmt::Assign(assign) = statement else {
                continue;
            };
            if is_class_metadata_assignment(assign) {
                continue;
            }
            let span = self.span(assign.range);
            let [Expr::Name(target)] = assign.targets.as_slice() else {
                return Err(SmeltError::unsupported(span, format!("{owner}: an Enum member must be one name")));
            };
            let value = match assign.value.as_ref() {
                Expr::Call(call)
                    if call.arguments.args.is_empty()
                        && call.arguments.keywords.is_empty()
                        && self.denoted_stdlib_name(&call.func, scope, &owner)? == Some(StdlibName::Auto) =>
                {
                    Literal::Int(last_int.map_or(Some(1), |last| last.checked_add(1)).ok_or_else(|| {
                        SmeltError::unsupported(span, format!("{owner}: auto() value out of range"))
                    })?)
                }
                other => self.class_var_literal(other)?.map(|(literal, _)| literal).ok_or_else(|| {
                    SmeltError::unsupported(
                        span,
                        format!("{owner}: an Enum member value must be a literal or auto()"),
                    )
                })?,
            };
            let ty = match &value {
                Literal::Int(int) => {
                    last_int = Some(*int);
                    self.intern_type(Type::PyInt)
                }
                Literal::String(_) => self.intern_type(Type::PyStr),
                Literal::Float(_) => self.intern_type(Type::Float),
                Literal::Bool(_) => self.intern_type(Type::Bool),
                _ => {
                    return Err(SmeltError::unsupported(
                        span,
                        format!("{owner}: an Enum member value must be an int, str, float or bool"),
                    ));
                }
            };
            if value_ty.is_some_and(|shared| shared != ty) {
                return Err(SmeltError::unsupported(
                    span,
                    format!("{owner}: Enum members of different value types are not supported"),
                ));
            }
            value_ty = Some(ty);
            if members.iter().any(|member| member.value == value) {
                return Err(SmeltError::unsupported(
                    span,
                    format!("{owner}: an Enum alias (two members with one value) is not supported"),
                ));
            }
            members.push(EnumMemberDef { name: target.id.as_str().to_owned(), value, span });
        }
        let value_ty = value_ty.ok_or_else(|| {
            SmeltError::unsupported(self.span(class.range), format!("{owner}: an Enum needs at least one member"))
        })?;
        Ok((members, value_ty))
    }

    /// Lower the members, constructor, text methods and lookup of the
    /// `Enum` class `class` (see the file header).
    fn lower_enum_parts(
        &mut self,
        class: &StmtClassDef,
        target: &MaterializedClassTarget<'_>,
    ) -> Result<EnumParts, SmeltError> {
        let (class_name, class_sym, class_ty, span) = (target.name, target.symbol, target.ty, target.span);
        for statement in &class.body {
            if let Stmt::FunctionDef(function) = statement
                && matches!(function.name.as_str(), "__init__" | "__new__")
            {
                return Err(SmeltError::unsupported(
                    self.span(function.range),
                    format!("class '{class_name}': an Enum with a custom {} is not supported", function.name),
                ));
            }
        }
        let (definitions, value_ty) = self.enum_member_definitions(class, class_name)?;
        let str_ty = self.intern_type(Type::PyStr);
        let name_sym = self.intern_name("name");
        let value_sym = self.intern_name("value");
        let fields = vec![
            Field { name: name_sym, ty: str_ty, visibility: Visibility::Public, optional: false, span },
            Field { name: value_sym, ty: value_ty, visibility: Visibility::Public, optional: false, span },
        ];
        for field in &fields {
            self.class_fields.entry(class_name.to_owned()).or_default().push(field.clone());
        }
        let constructor = self.enum_constructor(class_sym, class_ty, (name_sym, str_ty), (value_sym, value_ty), span);
        let mut static_fields = Vec::with_capacity(definitions.len());
        let mut members = Vec::with_capacity(definitions.len());
        for definition in &definitions {
            let global = self.enum_member_global(class_name, class_sym, class_ty, definition, value_ty);
            members.push((definition.name.clone(), global));
            static_fields.push(smelt_hir::StaticField {
                name: self.intern_name(&definition.name),
                ty: class_ty,
                visibility: Visibility::Public,
                value: None,
                mutable: false,
                storage: Some(global),
                span: definition.span,
                inherited: None,
            });
        }
        let declares = |name: &str| {
            class.body.iter().any(|statement| matches!(statement, Stmt::FunctionDef(function) if function.name.as_str() == name))
        };
        let mut methods = Vec::new();
        if !declares("__str__") {
            methods.push(self.enum_text_method(class_name, class_sym, class_ty, (name_sym, value_sym, value_ty), false, span)?);
        }
        if !declares("__repr__") {
            methods.push(self.enum_text_method(class_name, class_sym, class_ty, (name_sym, value_sym, value_ty), true, span)?);
        }
        let lookup = self.enum_lookup_function(class_name, class_ty, &members, (value_sym, value_ty), span)?;
        self.enum_classes.insert(class_name.to_owned(), EnumClass { class: class_sym, members, lookup });
        Ok(EnumParts { fields, static_fields, constructor, methods })
    }

    /// The constructor the member initializers call: `self.name = name`,
    /// `self.value = value`.
    fn enum_constructor(
        &mut self,
        class_sym: Symbol,
        class_ty: TypeId,
        (name_sym, str_ty): (Symbol, TypeId),
        (value_sym, value_ty): (Symbol, TypeId),
        span: Span,
    ) -> ItemId {
        let (body, params) = self.with_fn_scope(FnKind::Sync, |this| {
            let mut body = Body::new(None, span);
            let self_local = body.push_local(LocalDecl { name: Some(this.intern_name("self")), ty: class_ty, mutable: false, span });
            let mut params = Vec::new();
            let root = body.root;
            for (field, ty) in [(name_sym, str_ty), (value_sym, value_ty)] {
                let local = body.push_local(LocalDecl { name: Some(field), ty, mutable: false, span });
                body.params.push(local);
                params.push(Param { name: field, local, ty, span });
                let receiver = body.push_expr(HirExpr { kind: ExprKind::Local(self_local), ty: class_ty, span });
                let target = body.push_expr(HirExpr { kind: ExprKind::Field { receiver, field }, ty, span });
                let value = body.push_expr(HirExpr { kind: ExprKind::Local(local), ty, span });
                body.push_stmt_to_block(root, HirStmt::Assign { target, value });
            }
            let result = body.push_expr(HirExpr { kind: ExprKind::Local(self_local), ty: class_ty, span });
            body.blocks[usize::try_from(root.0).unwrap_or(0)].tail = Some(result);
            (body, params)
        });
        let body_id = self.ctx.krate.push_body(body);
        let init_sym = self.intern_name("__init__");
        self.ctx.krate.push_item(Item::Function(Function {
            name: init_sym,
            span,
            type_params: Vec::new(),
            params,
            rest: None,
            required_params: None,
            return_ty: class_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Constructor { class: class_sym },
        }))
    }

    /// The module global holding member `definition`, initialized once with
    /// `Class(name, value)`.
    fn enum_member_global(
        &mut self,
        class_name: &str,
        class_sym: Symbol,
        class_ty: TypeId,
        definition: &EnumMemberDef,
        value_ty: TypeId,
    ) -> ItemId {
        let span = definition.span;
        let str_ty = self.intern_type(Type::PyStr);
        let mut init_body = Body::new(None, span);
        let name = init_body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::String(definition.name.clone())), ty: str_ty, span });
        let value = init_body.push_expr(HirExpr { kind: ExprKind::Literal(definition.value.clone()), ty: value_ty, span });
        let member = init_body.push_expr(HirExpr { kind: ExprKind::New { class: class_sym, args: vec![name, value] }, ty: class_ty, span });
        init_body.push_stmt(HirStmt::Return(Some(member)));
        let body_id = self.ctx.krate.push_body(init_body);
        let spelled = format!("{}${}", class_name.replace('.', "$"), definition.name);
        let global_name = self.intern_synthesized_name(&format!("smelt_enum${spelled}"));
        let global = self.ctx.krate.push_item(Item::MutableGlobal(smelt_hir::MutableGlobalItem {
            name: global_name,
            ty: class_ty,
            init: smelt_hir::MutableGlobalInit::Pending,
            visibility: Visibility::Private,
            span,
        }));
        let init_name = self.intern_synthesized_name(&format!("smelt_global_init$enum${spelled}"));
        let init_item = self.ctx.krate.push_item(Item::Function(Function {
            name: init_name,
            span,
            type_params: Vec::new(),
            params: Vec::new(),
            rest: None,
            required_params: Some(0),
            return_ty: class_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        if let Some(Item::MutableGlobal(item)) = self.ctx.krate.items.get_mut(usize::try_from(global.0).unwrap_or(usize::MAX)) {
            item.init = smelt_hir::MutableGlobalInit::Initializer(init_item);
        }
        self.pending_module_items.push(init_item);
        self.pending_module_items.push(global);
        global
    }

    /// `__str__` (`Color.RED`) or, with `repr`, `__repr__` (`<Color.RED: 1>`,
    /// the value's `repr`).
    fn enum_text_method(
        &mut self,
        class_name: &str,
        class_sym: Symbol,
        class_ty: TypeId,
        (name_sym, value_sym, value_ty): (Symbol, Symbol, TypeId),
        repr: bool,
        span: Span,
    ) -> Result<ItemId, SmeltError> {
        let str_ty = self.intern_type(Type::PyStr);
        let display_name = class_name.rsplit('.').next().unwrap_or(class_name).to_owned();
        let (body, self_param) = self.with_fn_scope(FnKind::Sync, |this| -> Result<(Body, Param), SmeltError> {
            let mut body = Body::new(None, span);
            let self_sym = this.intern_name("self");
            let self_local = body.push_local(LocalDecl { name: Some(self_sym), ty: class_ty, mutable: false, span });
            body.params.push(self_local);
            let text = |body: &mut Body, value: &str| {
                body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::String(value.to_owned())), ty: str_ty, span })
            };
            let field = |body: &mut Body, field: Symbol, ty: TypeId| {
                let receiver = body.push_expr(HirExpr { kind: ExprKind::Local(self_local), ty: class_ty, span });
                body.push_expr(HirExpr { kind: ExprKind::Field { receiver, field }, ty, span })
            };
            let mut parts = vec![text(&mut body, &format!("{}{display_name}.", if repr { "<" } else { "" })), field(&mut body, name_sym, str_ty)];
            if repr {
                parts.push(text(&mut body, ": "));
                let value = field(&mut body, value_sym, value_ty);
                parts.push(body.push_expr(HirExpr { kind: ExprKind::PyRepr { value }, ty: str_ty, span }));
                parts.push(text(&mut body, ">"));
            }
            let mut joined = parts[0];
            for part in parts.into_iter().skip(1) {
                joined = this.python_arithmetic(PythonArithmetic::Plain(BinOp::Add), joined, part, span, &mut body)?;
            }
            body.push_stmt(HirStmt::Return(Some(joined)));
            Ok((body, Param { name: self_sym, local: self_local, ty: class_ty, span }))
        })?;
        let body_id = self.ctx.krate.push_body(body);
        let method = self.intern_name(if repr { "__repr__" } else { "__str__" });
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name: method,
            span,
            type_params: Vec::new(),
            params: vec![self_param],
            rest: None,
            required_params: None,
            return_ty: str_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::ClassMethod { class: class_sym, method },
        }));
        self.class_methods
            .entry(class_name.to_owned())
            .or_default()
            .insert(if repr { "__repr__" } else { "__str__" }.to_owned(), item);
        Ok(item)
    }

    /// The module function answering `Class(value)`: the member whose value
    /// equals `value`, else `ValueError: <repr(value)> is not a valid Class`.
    fn enum_lookup_function(
        &mut self,
        class_name: &str,
        class_ty: TypeId,
        members: &[(String, ItemId)],
        (value_sym, value_ty): (Symbol, TypeId),
        span: Span,
    ) -> Result<ItemId, SmeltError> {
        let str_ty = self.intern_type(Type::PyStr);
        let display_name = class_name.rsplit('.').next().unwrap_or(class_name).to_owned();
        let (body, param) = self.with_fn_scope(FnKind::Sync, |this| -> Result<(Body, Param), SmeltError> {
            let mut body = Body::new(None, span);
            let local = body.push_local(LocalDecl { name: Some(value_sym), ty: value_ty, mutable: false, span });
            body.params.push(local);
            let root = body.root;
            for (_, global) in members {
                let member = body.push_expr(HirExpr { kind: ExprKind::GlobalGet { item: *global }, ty: class_ty, span });
                let member_value = body.push_expr(HirExpr { kind: ExprKind::Field { receiver: member, field: value_sym }, ty: value_ty, span });
                let wanted = body.push_expr(HirExpr { kind: ExprKind::Local(local), ty: value_ty, span });
                let test = this.compare_pair(CmpOp::Eq, member_value, wanted, span, &mut body)?;
                let found = body.push_block(span);
                let answer = body.push_expr(HirExpr { kind: ExprKind::GlobalGet { item: *global }, ty: class_ty, span });
                body.push_stmt_to_block(found, HirStmt::Return(Some(answer)));
                body.push_stmt_to_block(root, HirStmt::If { cond: test, then_block: found, else_block: None });
            }
            let wanted = body.push_expr(HirExpr { kind: ExprKind::Local(local), ty: value_ty, span });
            let shown = body.push_expr(HirExpr { kind: ExprKind::PyRepr { value: wanted }, ty: str_ty, span });
            let suffix = body.push_expr(HirExpr {
                kind: ExprKind::Literal(Literal::String(format!(" is not a valid {display_name}"))),
                ty: str_ty,
                span,
            });
            let message = this.python_arithmetic(PythonArithmetic::Plain(BinOp::Add), shown, suffix, span, &mut body)?;
            let classes = ["ValueError", "Exception", "BaseException"].iter().map(|class| this.intern_name(class)).collect();
            let exception_ty = this.py_exception_type();
            let error = body.push_expr(HirExpr { kind: ExprKind::PyExceptionNew { classes, args: vec![message] }, ty: exception_ty, span });
            body.push_stmt_to_block(root, HirStmt::Throw(error));
            Ok((body, Param { name: value_sym, local, ty: value_ty, span }))
        })?;
        let body_id = self.ctx.krate.push_body(body);
        let name = self.intern_synthesized_name(&format!("smelt_enum_lookup${}", class_name.replace('.', "$")));
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            type_params: Vec::new(),
            params: vec![param],
            rest: None,
            required_params: Some(1),
            return_ty: class_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        self.pending_module_items.push(item);
        Ok(item)
    }

    /// `Class(value)` for an `Enum` class: the member lookup.
    fn enum_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Name(name) = call.func.as_ref() else {
            return Ok(None);
        };
        if self.scope.locals.contains_key(name.id.as_str()) {
            return Ok(None);
        }
        let Some(enum_class) = self.enum_classes.get(name.id.as_str()).cloned() else {
            return Ok(None);
        };
        let span = self.span(call.range());
        let [value_arg] = call.arguments.args.as_ref() else {
            return Err(SmeltError::unsupported(span, "an Enum class called takes one value to look up"));
        };
        if !call.arguments.keywords.is_empty() {
            return Err(SmeltError::unsupported(span, "an Enum class called takes no keyword argument"));
        }
        let value = self.expression(value_arg, body)?;
        let callee_ty = self.item_expr_type(enum_class.lookup);
        let callee = body.push_expr(HirExpr { kind: ExprKind::Item(enum_class.lookup), ty: callee_ty, span });
        let class_ty = self.intern_type(Type::Class { name: enum_class.class, args: Vec::new() });
        Ok(Some(body.push_expr(HirExpr { kind: ExprKind::Call { callee, args: vec![value] }, ty: class_ty, span })))
    }

    /// The `Enum` class named `name` used as a value: the list of its members
    /// in definition order.
    fn enum_members_expression(&mut self, name: &str, span: Span, body: &mut Body) -> Option<smelt_hir::ExprId> {
        if self.scope.locals.contains_key(name) {
            return None;
        }
        let enum_class = self.enum_classes.get(name)?.clone();
        let class_ty = self.intern_type(Type::Class { name: enum_class.class, args: Vec::new() });
        let list_ty = self.intern_type(Type::List(class_ty));
        let items = enum_class
            .members
            .iter()
            .map(|(_, global)| body.push_expr(HirExpr { kind: ExprKind::GlobalGet { item: *global }, ty: class_ty, span }))
            .collect();
        Some(body.push_expr(HirExpr { kind: ExprKind::ListLit(items), ty: list_ty, span }))
    }
}
