/// Accumulated results of walking a class body: fields, constructor/method
/// item ids, and enum/abstract-method metadata used to assemble the class item.
struct ClassBodyLowering {
    /// Instance fields declared via annotated assignments.
    fields: Vec<Field>,
    /// Item id of the user-defined `__init__`, if any.
    constructor_id: Option<ItemId>,
    /// Item ids of non-constructor methods in declaration order.
    method_ids: Vec<ItemId>,
    /// Item ids of `@staticmethod` methods in declaration order.
    static_methods: Vec<ItemId>,
    /// Static (class-level) fields declared by bare/annotated class-body
    /// assignments with a concrete literal initializer.
    static_fields_out: Vec<smelt_hir::StaticField>,
    /// Whether the class derives from `IntEnum`.
    is_int_enum: bool,
    /// Collected `IntEnum` member name → integer value pairs.
    enum_members: HashMap<String, i64>,
    /// Abstract-method signatures collected from `@abstractmethod` members.
    abstract_methods: Vec<MethodSig>,
}

impl ModuleBuilder<'_> {
    /// Lower a Python class definition into the current HIR module.
    fn class_def(
        &mut self,
        class: &StmtClassDef,
        module: &ModModule,
        hir_module: &mut Module,
    ) -> Result<(), SmeltError> {
        let span = self.span(class.range);
        let class_name_str = class.name.as_str();
        let class_sym = self.intern_name(class_name_str);
        let class_ty = self.intern_type(Type::Class {
            name: class_sym,
            args: vec![],
        });
        let materialized = self.materialized_class_definition(class_name_str).cloned();

        // --- Decorator check: only @dataclass is allowed ---
        let mut kind =
            Self::class_kind_from_decorators(class, materialized.as_ref(), span, class_name_str)?;

        // --- Metaclass and base classes ---
        let (is_abstract_class, type_params, base) =
            self.resolve_class_bases(class, materialized.as_ref(), span, class_name_str)?;
        if is_abstract_class {
            kind = ClassKind::Abstract;
        }

        // --- Fields and methods ---
        let target = MaterializedClassTarget {
            name: class_name_str,
            symbol: class_sym,
            ty: class_ty,
            span,
        };
        let (mut lowered, class_item_id) = self.lower_class_body(
            class,
            materialized.as_ref(),
            &target,
            base,
            &type_params,
            &mut kind,
            hir_module,
        )?;

        self.finalize_class_definition(
            class,
            materialized.as_ref(),
            &target,
            module,
            base,
            type_params,
            kind,
            &mut lowered,
            class_item_id,
            hir_module,
        )
    }

    /// Determine the class kind from its decorators, validating that only
    /// `@dataclass` (or a materialized dataclass) is present.
    fn class_kind_from_decorators(
        class: &StmtClassDef,
        materialized: Option<&smelt_specialize::ClassDefinition>,
        span: Span,
        class_name_str: &str,
    ) -> Result<ClassKind, SmeltError> {
        let mut kind = if materialized.is_some_and(|manifest_class| {
            manifest_class
                .metadata
                .iter()
                .any(|metadata| metadata.key == "python.dataclass_fields")
        }) {
            ClassKind::DataclassLike { frozen: false }
        } else {
            ClassKind::Plain
        };
        let runtime_decorators = if materialized.is_none() {
            class.decorator_list.as_slice()
        } else {
            &[]
        };
        for dec in runtime_decorators {
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
        Ok(kind)
    }

    /// Resolve the metaclass abstractness flag, generic type parameters, and the
    /// single supported base class of a class definition.
    fn resolve_class_bases(
        &mut self,
        class: &StmtClassDef,
        materialized: Option<&smelt_specialize::ClassDefinition>,
        span: Span,
        class_name_str: &str,
    ) -> Result<(bool, Vec<TypeParamDef>, Option<Symbol>), SmeltError> {
        // --- Metaclass check ---
        let mut is_abstract_class = false;
        if let Some(args) = class.arguments.as_deref() {
            for kw in args.keywords.iter() {
                if kw.arg.as_ref().map(|a| a.as_str()) == Some("metaclass") {
                    if matches!(expr_simple_name(&kw.value), Some("ABCMeta" | "abc.ABCMeta")) {
                        is_abstract_class = true;
                    } else if materialized.is_none() {
                        return Err(SmeltError::no_metaclass(span, class_name_str));
                    }
                }
            }
        }

        // --- Base classes ---
        let mut type_params: Vec<TypeParamDef> = Vec::new();
        let base: Option<Symbol> = if let Some(args) = class.arguments.as_deref() {
            let positional: Vec<&Expr> = args
                .args
                .iter()
                .filter(|base_expr| {
                    let base_name = base_class_name(base_expr);
                    if matches!(base_name, Some("ABC" | "abc.ABC")) {
                        is_abstract_class = true;
                    }
                    if matches!(base_name, Some("Generic" | "typing.Generic")) {
                        type_params = self.generic_marker_type_params(base_expr);
                    }
                    !matches!(
                        base_name,
                        Some(
                            "object"
                                | "ABC"
                                | "abc.ABC"
                                | "Generic"
                                | "typing.Generic"
                                | "Protocol"
                                | "typing.Protocol"
                        )
                    )
                })
                .collect();
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
                    if is_django_model_base(base_expr) && materialized.is_none() {
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
        Ok((is_abstract_class, type_params, base))
    }

    /// Register the class item stub then walk the class body, collecting fields,
    /// constructor, methods, enum members, and abstract-method signatures.
    ///
    /// `kind` may be promoted to [`ClassKind::Abstract`] when an
    /// `@abstractmethod` is encountered.
    #[expect(
        clippy::too_many_arguments,
        reason = "class body lowering needs the parsed class, resolved class state, and HIR output"
    )]
    fn lower_class_body(
        &mut self,
        class: &StmtClassDef,
        materialized: Option<&smelt_specialize::ClassDefinition>,
        target: &MaterializedClassTarget<'_>,
        base: Option<Symbol>,
        type_params: &[TypeParamDef],
        kind: &mut ClassKind,
        hir_module: &mut Module,
    ) -> Result<(ClassBodyLowering, ItemId), SmeltError> {
        let class_name_str = target.name;
        let class_sym = target.symbol;
        let class_ty = target.ty;
        let span = target.span;
        let mut fields: Vec<Field> = Vec::new();
        let mut constructor_id: Option<ItemId> = None;
        let mut method_ids: Vec<ItemId> = Vec::new();
        let mut static_methods: Vec<ItemId> = Vec::new();
        let mut static_fields_out: Vec<smelt_hir::StaticField> = Vec::new();
        let is_int_enum = base
            .and_then(|base_sym| self.ctx.krate.symbols.get(base_sym))
            .is_some_and(|base_name| base_name == "IntEnum");
        let class_item_id = self.ctx.krate.push_item(Item::Class(Class {
            name: class_sym,
            span,
            kind: kind.clone(),
            type_params: type_params.to_vec(),
            base,
            base_args: Vec::new(),
            fields: Vec::new(),
            static_fields: Vec::new(),
            descriptors: Vec::new(),
            constructor: None,
            methods: Vec::new(),
            static_methods: Vec::new(),
            abstract_methods: Vec::new(),
            implements: vec![],
        }));
        self.items.insert(class_name_str.to_owned(), class_item_id);
        // Pre-register every method's `ItemId` so a body can call siblings
        // declared later in the class (`self.helper()`, `cls.helper()`).
        let method_slots =
            self.reserve_class_method_slots(class, class_name_str, class_sym, materialized.is_some())?;
        let mut enum_members = HashMap::new();
        let mut abstract_methods = Vec::new();

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
                    // A Python data field is optional when its annotation lowers to
                    // `Optional[T]` / `T | None`. Those spellings already intern a
                    // `Type::Optional`, which is what Rust codegen turns into
                    // `Option<T>`; recording the flag here keeps the HIR field
                    // metadata honest so downstream shape checks (interface
                    // satisfaction, dynamic-field wrapping) see the same optionality
                    // TypeScript class fields carry for the `?` spelling.
                    let field_optional = matches!(
                        self.ctx.krate.types.get(field_ty),
                        Some(Type::Optional(_))
                    );
                    let field = Field {
                        name: field_sym,
                        ty: field_ty,
                        visibility: Visibility::Public,
                        optional: field_optional,
                        span: self.span(ann.range),
                    };
                    self.class_fields
                        .entry(class_name_str.to_owned())
                        .or_default()
                        .push(field.clone());
                    fields.push(field);
                }
                Stmt::Assign(assign) if is_int_enum => {
                    self.int_enum_member_assign(class_name_str, assign, &mut enum_members)?;
                }
                // A bare `NAME = <literal>` in a non-enum class body declares a
                // class variable (a static member shared by the class). Lower it
                // to a materialized static field resolvable via `Class.NAME`.
                Stmt::Assign(assign) if !is_int_enum => {
                    if let Some(static_field) = self.class_var_static_field(assign)? {
                        static_fields_out.push(static_field);
                    } else {
                        return Err(SmeltError::unsupported(
                            self.span(assign.range),
                            format!(
                                "class '{class_name_str}': class-level assignment must be a single name bound to a literal"
                            ),
                        ));
                    }
                }
                Stmt::FunctionDef(func) => {
                    let method_name = func.name.as_str();
                    if materialized.is_some()
                        && matches!(method_name, "__init_subclass__" | "__set_name__")
                    {
                        continue;
                    }
                    if Self::is_abstractmethod(func) {
                        *kind = ClassKind::Abstract;
                        abstract_methods.push(self.abstract_method_sig(func)?);
                        continue;
                    }
                    // A slot pre-reserved by `reserve_class_method_slots` is
                    // overwritten in place so forward references resolved during
                    // an earlier sibling body keep a valid `ItemId`.
                    let reserved_slot = method_slots.get(&func.range.start().to_u32()).copied();
                    if Self::is_staticmethod(func) {
                        let mid = self.class_method_with_receiver(
                            class_name_str,
                            class_sym,
                            class_ty,
                            func,
                            true,
                            reserved_slot,
                        )?;
                        static_methods.push(mid);
                        hir_module.items.push(mid);
                        continue;
                    }
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
                        // Both instance methods and `@classmethod`s take the
                        // body-lowering path with `is_static = false` so the
                        // implicit `self`/`cls` local is created. A classmethod
                        // then dispatches receiver-free (its `owner` becomes
                        // `ClassStaticMethod`), so it is bookkept as a static
                        // method and registered in `class_static_methods`.
                        let is_classmethod = self.is_classmethod(func)?;
                        let mid = self.class_method_with_receiver(
                            class_name_str,
                            class_sym,
                            class_ty,
                            func,
                            false,
                            reserved_slot,
                        )?;
                        self.rename_materialized_descriptor_callable(class_name_str, func, mid)?;
                        if is_classmethod {
                            static_methods.push(mid);
                            self.class_static_methods
                                .entry(class_name_str.to_owned())
                                .or_default()
                                .insert(method_name.to_owned(), mid);
                        } else {
                            method_ids.push(mid);
                            self.class_methods
                                .entry(class_name_str.to_owned())
                                .or_default()
                                .insert(method_name.to_owned(), mid);
                        }
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

        Ok((
            ClassBodyLowering {
                fields,
                constructor_id,
                method_ids,
                static_methods,
                static_fields_out,
                is_int_enum,
                enum_members,
                abstract_methods,
            },
            class_item_id,
        ))
    }

    /// Merge materialized descriptors/methods, synthesize the constructor, record
    /// enum members, and write the finished class item back into its slot.
    #[expect(
        clippy::too_many_arguments,
        reason = "final class assembly threads all accumulated class-definition phase outputs"
    )]
    fn finalize_class_definition(
        &mut self,
        class: &StmtClassDef,
        materialized: Option<&smelt_specialize::ClassDefinition>,
        target: &MaterializedClassTarget<'_>,
        module: &ModModule,
        base: Option<Symbol>,
        type_params: Vec<TypeParamDef>,
        kind: ClassKind,
        lowered: &mut ClassBodyLowering,
        class_item_id: ItemId,
        hir_module: &mut Module,
    ) -> Result<(), SmeltError> {
        let class_name_str = target.name;
        let class_sym = target.symbol;
        let class_ty = target.ty;
        let span = target.span;
        let fields = &mut lowered.fields;
        let method_ids = &mut lowered.method_ids;
        let mut constructor_id = lowered.constructor_id;

        let descriptors = if let Some(manifest_class) = materialized {
            self.merge_materialized_class_fields(class_name_str, manifest_class, fields, span);
            self.lower_materialized_descriptors(manifest_class, span)?
        } else {
            Vec::new()
        };
        let descriptor_names = descriptors
            .iter()
            .map(|descriptor| descriptor.name)
            .collect::<HashSet<_>>();
        fields.retain(|field| {
            if !descriptor_names.contains(&field.name) {
                return true;
            }
            descriptors
                .iter()
                .find(|descriptor| descriptor.name == field.name)
                .is_some_and(|descriptor| {
                    !descriptor.data_descriptor
                        && Self::constructor_assigns_instance_field(class, field.name, self)
                })
        });
        if let Some(manifest_class) = materialized {
            let generated = self.lower_materialized_class_methods(
                &MaterializedClassTarget {
                    name: class_name_str,
                    symbol: class_sym,
                    ty: class_ty,
                    span,
                },
                manifest_class,
                module,
            )?;
            hir_module.items.extend(generated.iter().copied());
            method_ids.extend(generated);
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
            let init_id = self.synthesize_dataclass_init(class_sym, class_ty, fields, span)?;
            constructor_id = Some(init_id);
            hir_module.items.push(init_id);
        } else if constructor_id.is_none() {
            let init_id = self.synthesize_default_init(class_sym, class_ty, span);
            constructor_id = Some(init_id);
            hir_module.items.push(init_id);
        }
        if lowered.is_int_enum {
            self.ctx
                .enum_members
                .insert(class_name_str.to_owned(), lowered.enum_members.clone());
            self.enum_members
                .insert(class_name_str.to_owned(), std::mem::take(&mut lowered.enum_members));
        }

        let class_item = Item::Class(Class {
            name: class_sym,
            span,
            kind,
            type_params,
            base,
            base_args: Vec::new(),
            fields: std::mem::take(fields),
            static_fields: std::mem::take(&mut lowered.static_fields_out),
            descriptors,
            constructor: constructor_id,
            methods: std::mem::take(method_ids),
            static_methods: std::mem::take(&mut lowered.static_methods),
            abstract_methods: std::mem::take(&mut lowered.abstract_methods),
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

    /// Returns whether `__init__` directly assigns the named instance field.
    fn constructor_assigns_instance_field(
        class: &StmtClassDef,
        field: Symbol,
        builder: &Self,
    ) -> bool {
        let Some(field_name) = builder.ctx.krate.symbols.get(field) else {
            return false;
        };
        class.body.iter().any(|statement| {
            let Stmt::FunctionDef(function) = statement else {
                return false;
            };
            function.name.as_str() == "__init__"
                && function.body.iter().any(|body_statement| {
                    Self::statement_assigns_self_field(body_statement, field_name)
                })
        })
    }

    /// Returns whether one simple statement assigns `self.<field>`.
    fn statement_assigns_self_field(statement: &Stmt, field: &str) -> bool {
        let is_target = |target: &Expr| {
            matches!(
                target,
                Expr::Attribute(attribute)
                    if attribute.attr.as_str() == field
                        && matches!(
                            attribute.value.as_ref(),
                            Expr::Name(receiver) if receiver.id.as_str() == "self"
                        )
            )
        };
        match statement {
            Stmt::Assign(assign) => assign.targets.iter().any(is_target),
            Stmt::AnnAssign(assign) => is_target(&assign.target),
            Stmt::AugAssign(assign) => is_target(&assign.target),
            _ => false,
        }
    }

    /// Lower one targeted `IntEnum` member assignment from a class body.
    ///
    /// This intentionally handles only the HTTPX-style enum forms needed by
    /// the object-model slice: `NAME = 200`, `NAME = 200, "OK"`, and member
    /// aliases that refer to another integer member on the same class.
    fn int_enum_member_assign(
        &mut self,
        class_name: &str,
        assign: &StmtAssign,
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

    /// Return whether a Python method is decorated with `@abstractmethod`.
    fn is_abstractmethod(func: &StmtFunctionDef) -> bool {
        func.decorator_list.iter().any(|decorator| {
            matches!(
                decorator_simple_name(decorator),
                Some("abstractmethod" | "abc.abstractmethod")
            )
        })
    }

    /// Return whether a method is decorated with `@staticmethod`.
    fn is_staticmethod(func: &StmtFunctionDef) -> bool {
        func.decorator_list
            .iter()
            .any(|decorator| decorator_simple_name(decorator) == Some("staticmethod"))
    }

    /// Lower a class-level `NAME = <literal>` assignment to a static field.
    ///
    /// Returns `None` when the assignment is not a single `Name` target bound to
    /// a concrete literal, so the caller can reject unsupported class-variable
    /// forms. The static field is resolvable as `Class.NAME` and lowers to a
    /// receiver-free associated accessor in Rust codegen.
    fn class_var_static_field(
        &mut self,
        assign: &StmtAssign,
    ) -> Result<Option<smelt_hir::StaticField>, SmeltError> {
        let [target] = assign.targets.as_slice() else {
            return Ok(None);
        };
        let Expr::Name(target_name) = target else {
            return Ok(None);
        };
        let Some((value, ty)) = self.class_var_literal(assign.value.as_ref())? else {
            return Ok(None);
        };
        let name = self.intern_name(target_name.id.as_str());
        Ok(Some(smelt_hir::StaticField {
            name,
            ty,
            visibility: Visibility::Public,
            value: Some(value),
            span: self.span(assign.range),
        }))
    }

    /// Convert a class-variable initializer expression to a concrete literal and
    /// its Smelt type, or `None` when it is not a supported literal form.
    fn class_var_literal(
        &mut self,
        expr: &Expr,
    ) -> Result<Option<(Literal, TypeId)>, SmeltError> {
        let pair = match expr {
            Expr::NumberLiteral(number) => match &number.value {
                Number::Int(int_value) => {
                    let value = int_value.as_i64().ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(number.range),
                            "class-variable integer literal is out of i64 range",
                        )
                    })?;
                    (Literal::Int(value), self.intern_type(Type::Int))
                }
                Number::Float(value) => {
                    (Literal::Float(*value), self.intern_type(Type::Float))
                }
                Number::Complex { .. } => return Ok(None),
            },
            Expr::StringLiteral(value) => (
                Literal::String(value.value.to_str().to_owned()),
                self.intern_type(Type::String),
            ),
            Expr::BooleanLiteral(value) => {
                (Literal::Bool(value.value), self.intern_type(Type::Bool))
            }
            _ => return Ok(None),
        };
        Ok(Some(pair))
    }

    /// Lower an abstract Python method declaration to a HIR method signature.
    fn abstract_method_sig(&mut self, func: &StmtFunctionDef) -> Result<MethodSig, SmeltError> {
        let span = self.span(func.range);
        let mut params = Vec::new();
        for param in func.parameters.args.iter().skip(1) {
            let annotation = param.parameter.annotation.as_deref().ok_or_else(|| {
                SmeltError::unsupported(
                    span,
                    format!(
                        "abstract method '{}': parameters require annotations",
                        func.name
                    ),
                )
            })?;
            params.push(ParamSig {
                name: self.intern_name(param.parameter.name.as_str()),
                ty: self.annotation_to_hir(annotation)?,
                span: self.span(param.parameter.range),
            });
        }
        let return_ty = func
            .returns
            .as_deref()
            .ok_or_else(|| {
                SmeltError::type_constraint(
                    span,
                    format!(
                        "abstract method '{}': return type annotation required",
                        func.name
                    ),
                )
            })
            .and_then(|annotation| self.annotation_to_hir(annotation))?;
        Ok(MethodSig {
            name: self.intern_name(func.name.as_str()),
            params,
            rest: None,
            required_params: None,
            return_ty,
            visibility: Visibility::Public,
            is_async: false,
            span,
        })
    }

    /// Extract type parameters from a `Generic[T, U]` marker base.
    fn generic_marker_type_params(&mut self, base_expr: &Expr) -> Vec<TypeParamDef> {
        let Expr::Subscript(subscript) = base_expr else {
            return Vec::new();
        };
        let items = if let Expr::Tuple(tuple) = subscript.slice.as_ref() {
            tuple.elts.iter().collect::<Vec<_>>()
        } else {
            vec![subscript.slice.as_ref()]
        };
        let mut params = Vec::new();
        for item in items {
            if let Expr::Name(name) = item {
                let symbol = self.intern_name(name.id.as_str());
                params.push(TypeParamDef {
                    name: symbol,
                    constraint: None,
                    default: None,
                    span: self.span(item.range()),
                });
            }
        }
        params
    }

    /// Reserve an [`ItemId`] for every ordinary/class/static method in `class`
    /// *before* any method body is lowered.
    ///
    /// Method resolution at call sites (`self.helper()`, `cls.helper()`,
    /// `Class.helper()`) looks methods up by name in [`Self::class_methods`] /
    /// [`Self::class_static_methods`]. Those registries were previously filled
    /// incrementally as each body was lowered, so a call to a sibling declared
    /// *later* in the class body could not be resolved (issue #94's dominant
    /// blocker). Reserving placeholder items up front — keyed by each method's
    /// source-range start so the main pass can overwrite the exact slot — makes
    /// intra-class forward references resolvable regardless of declaration order.
    ///
    /// The reservation set mirrors the main class-body loop exactly: it skips
    /// `@abstractmethod`, `__init__`/`__new__` (constructor-shaped), materialized
    /// descriptor callables (property getters/setters), and — under a
    /// materialized class — the `__init_subclass__`/`__set_name__` hooks. Each
    /// remaining instance method is registered in [`Self::class_methods`]; each
    /// `@staticmethod` / `@classmethod` is registered in
    /// [`Self::class_static_methods`] (both dispatch receiver-free).
    fn reserve_class_method_slots(
        &mut self,
        class: &StmtClassDef,
        class_name_str: &str,
        class_sym: Symbol,
        materialized_hooks_skipped: bool,
    ) -> Result<HashMap<u32, ItemId>, SmeltError> {
        let mut slots: HashMap<u32, ItemId> = HashMap::new();
        for body_stmt in &class.body {
            let Stmt::FunctionDef(func) = body_stmt else {
                continue;
            };
            let method_name = func.name.as_str();
            if materialized_hooks_skipped
                && matches!(method_name, "__init_subclass__" | "__set_name__")
            {
                continue;
            }
            if Self::is_abstractmethod(func) || self.is_materialized_descriptor_callable(func) {
                continue;
            }
            let is_static = Self::is_staticmethod(func);
            // `__init__`/`__new__` are constructor-shaped and are not exposed
            // through the method registries; leave them to the main pass.
            if !is_static && matches!(method_name, "__init__" | "__new__") {
                continue;
            }
            // A `@classmethod` dispatches receiver-free like a `@staticmethod`
            // (its implicit `cls` is erased to the concrete class), so it is
            // reserved as a static method and registered in `class_static_methods`.
            let is_classmethod = !is_static && self.is_classmethod(func)?;
            let receiver_free = is_static || is_classmethod;
            let method_sym = self.intern_name(method_name);
            let owner = if receiver_free {
                FunctionOwner::ClassStaticMethod {
                    class: class_sym,
                    method: method_sym,
                }
            } else {
                FunctionOwner::ClassMethod {
                    class: class_sym,
                    method: method_sym,
                }
            };
            // Record the real return type now: a sibling body lowered *before*
            // this method resolves the call through this placeholder, so its
            // return type must already be accurate. This mirrors the return-type
            // reconciliation the main pass performs. When it cannot be resolved
            // the main pass reports the missing-annotation error; a `None`
            // placeholder here is only a stand-in until then.
            let return_ty = self
                .resolve_return_ty(func, func.returns.as_deref())
                .unwrap_or_else(|| self.intern_type(Type::None));
            let placeholder = Item::Function(Function {
                name: method_sym,
                span: self.span(func.range),
                params: Vec::new(),
                rest: None,
                required_params: None,
                return_ty,
                is_async: false,
                is_test: false,
                body: None,
                owner,
            });
            let item_id = self.ctx.krate.push_item(placeholder);
            let registry = if receiver_free {
                &mut self.class_static_methods
            } else {
                &mut self.class_methods
            };
            registry
                .entry(class_name_str.to_owned())
                .or_default()
                .insert(method_name.to_owned(), item_id);
            slots.insert(func.range.start().to_u32(), item_id);
        }
        Ok(slots)
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
        self.class_method_with_receiver(class_name_str, class_sym, class_ty, func, false, None)
    }

    /// Lower a Python class method to a HIR function item.
    ///
    /// When `is_static` is set the method is a `@staticmethod`: it takes no
    /// implicit `self`/`cls` receiver and is owned by
    /// [`FunctionOwner::ClassStaticMethod`] so codegen emits a receiver-free
    /// associated function resolvable via `Class.method(..)`.
    #[expect(
        clippy::too_many_arguments,
        reason = "class method lowering threads class identity and the static-receiver flag through one shared body"
    )]
    fn class_method_with_receiver(
        &mut self,
        class_name_str: &str,
        class_sym: Symbol,
        class_ty: TypeId,
        func: &StmtFunctionDef,
        is_static: bool,
        slot: Option<ItemId>,
    ) -> Result<ItemId, SmeltError> {
        let span = self.span(func.range);
        let method_name_str = func.name.as_str();
        let method_sym = self.intern_name(method_name_str);
        let is_init = !is_static && method_name_str == "__init__";
        let is_new = !is_static && method_name_str == "__new__";
        let is_classmethod = !is_static && self.is_classmethod(func)?;

        let return_ty = if is_init {
            self.intern_type(Type::None)
        } else if is_new && func.returns.is_none() {
            class_ty
        } else {
            // Reconcile any declared annotation with `ty`'s inferred actual
            // return type (issue #93); error only when neither is available.
            match self.resolve_return_ty(func, func.returns.as_deref()) {
                Some(ty) => ty,
                None => {
                    return Err(SmeltError::type_constraint(
                        span,
                        format!(
                            "method '{class_name_str}.{method_name_str}' must have an explicit return type annotation"
                        ),
                    ));
                }
            }
        };

        let saved_locals = std::mem::take(&mut self.locals);
        let mut fn_body = Body::new(None, span);
        let mut params: Vec<Param> = Vec::new();

        // Add the implicit receiver local for use inside the method body.
        // A `@staticmethod` has no implicit receiver, so this is skipped and all
        // declared parameters are lowered as ordinary function parameters.
        let implicit_receiver_name = if is_classmethod || is_new {
            "cls"
        } else {
            "self"
        };
        if !is_static {
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
        }

        let mut first = true;
        for param_with_default in func.parameters.iter_non_variadic_params() {
            let p = &param_with_default.parameter;
            let param_name_str = p.name.as_str();
            // Skip the implicit receiver; it was added above. Static methods
            // have no implicit receiver, so every parameter is a real argument.
            if !is_static
                && first
                && (param_name_str == implicit_receiver_name
                    || ((is_classmethod || is_new) && param_name_str == "self"))
            {
                first = false;
                continue;
            }
            first = false;

            let param_ty = match p.annotation.as_deref() {
                Some(ann) => self.annotation_to_hir(ann)?,
                // No annotation: consult `ty` (issue #93) before erroring.
                None => self.resolved_param_ty(p).ok_or_else(|| {
                    SmeltError::type_constraint(
                        self.span(p.range),
                        format!(
                            "parameter '{param_name_str}' in '{class_name_str}.{method_name_str}' must have a type annotation"
                        ),
                    )
                })?,
            };

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

        // A `@classmethod` lowers to a receiver-free associated function, the
        // same owner a `@staticmethod` uses. Python binds its implicit `cls` to
        // the class object, not an instance, so codegen must emit
        // `Class::method(args)` (no `&self`). `cls(..)`/`cls.helper(..)` inside
        // the body are lowered to concrete class construction / associated calls
        // (see `cls_receiver_call_expression` and `class_method_dispatch`), so
        // the `cls` local is never dereferenced as a receiver.
        let owner = if is_init {
            FunctionOwner::Constructor { class: class_sym }
        } else if is_static || is_classmethod {
            FunctionOwner::ClassStaticMethod {
                class: class_sym,
                method: method_sym,
            }
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
            rest: None,
            required_params: None,
            return_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner,
        });
        // Reuse the slot reserved by the pre-pass when present so any earlier
        // sibling body that resolved a forward reference to this method keeps a
        // valid `ItemId`; otherwise append a fresh item.
        match slot {
            Some(item_id) => {
                let index = usize::try_from(item_id.0).unwrap_or(usize::MAX);
                self.ctx.krate.items[index] = item;
                Ok(item_id)
            }
            None => Ok(self.ctx.krate.push_item(item)),
        }
    }

    // Class helper lowering continues in `class_init.rs`.
}
