// Multiple inheritance: method resolution along the C3 MRO, and `__init__`
// as an ordinary method inside a multiple-inheritance hierarchy
// (`docs/design/python-multiple-inheritance.md`).

impl ModuleBuilder<'_> {
    /// Record the classes of this module that belong to a
    /// *multiple-inheritance hierarchy*: the classes connected by base edges
    /// (in either direction) to a class listing two or more of the module's
    /// classes as bases.
    ///
    /// The set is syntactic, read before any class is lowered, because a base
    /// class is lowered before the subclass that makes its hierarchy one: its
    /// `__init__` must already lower as a method then (§4 of the design).
    fn collect_multiple_inheritance_classes(&mut self) {
        let classes: HashSet<&str> = self
            .module_class_bases
            .iter()
            .map(|(class, _)| class.as_str())
            .collect();
        let mut neighbours: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut roots: Vec<&str> = Vec::new();
        for (class, bases) in &self.module_class_bases {
            let program_bases: Vec<&str> = bases
                .iter()
                .map(String::as_str)
                .filter(|base| classes.contains(base))
                .collect();
            if program_bases.len() > 1 {
                roots.push(class.as_str());
            }
            for base in program_bases {
                neighbours.entry(class.as_str()).or_default().push(base);
                neighbours.entry(base).or_default().push(class.as_str());
            }
        }
        let mut members: HashSet<String> = HashSet::new();
        let mut pending = roots;
        while let Some(class) = pending.pop() {
            if !members.insert(class.to_owned()) {
                continue;
            }
            pending.extend(neighbours.get(class).into_iter().flatten().copied());
        }
        self.multiple_inheritance_classes = members;
    }

    /// Whether the class named `class_name` (its qualified name, or its last
    /// segment for a nested class) belongs to a multiple-inheritance
    /// hierarchy.
    fn in_multiple_inheritance_hierarchy(&self, class_name: &str) -> bool {
        let last = class_name.rsplit('.').next().unwrap_or(class_name);
        self.multiple_inheritance_classes.contains(last)
    }

    /// The lowered class item named by `class_sym`.
    fn class_item_named(&self, class_sym: Symbol) -> Option<&Class> {
        self.ctx.krate.items.iter().find_map(|item| match item {
            Item::Class(class) if class.name == class_sym => Some(class),
            _ => None,
        })
    }

    /// The method resolution order of the lowered class `class_sym`: itself,
    /// then its program ancestors in C3 order. A class whose bases cannot be
    /// linearized was refused when it was declared; it answers itself alone.
    fn class_mro(&self, class_sym: Symbol) -> Vec<Symbol> {
        // A chain of single bases is its own MRO; only a class with several
        // bases needs the C3 merge.
        let mut chain = vec![class_sym];
        let mut current = self.class_item_named(class_sym);
        while let Some(found) = current {
            if !found.extra_bases.is_empty() {
                chain.clear();
                break;
            }
            match found.base {
                Some(base) if !chain.contains(&base) => {
                    chain.push(base);
                    current = self.class_item_named(base);
                }
                _ => break,
            }
        }
        if !chain.is_empty() {
            return chain;
        }
        let bases_of = |class: Symbol| {
            self.class_item_named(class)
                .map(Class::program_bases)
                .unwrap_or_default()
        };
        smelt_hir::mro::linearize(class_sym, &bases_of).unwrap_or_else(|_| vec![class_sym])
    }

    /// The instance method named `method` that class `class_sym` itself
    /// declares (inherited and alias methods excluded).
    fn own_method_named(&self, class_sym: Symbol, method: &str) -> Option<ItemId> {
        let class = self.class_item_named(class_sym)?;
        class.methods.iter().chain(&class.static_methods).copied().find(|id| {
            matches!(
                self.item_ref(*id),
                Item::Function(function)
                    if self.ctx.krate.symbols.source(function.name) == Some(method)
            )
        })
    }

    /// The `method` the class after `definer` in `instance_class`'s MRO runs:
    /// the first class following `definer` in that order that declares it.
    /// `None` when no later class does (for `__init__`, that is
    /// `object.__init__`).
    fn method_after_in_mro(
        &self,
        instance_class: Symbol,
        definer: Symbol,
        method: &str,
    ) -> Option<ItemId> {
        let mro = self.class_mro(instance_class);
        let position = mro.iter().position(|class| *class == definer)?;
        mro.iter()
            .skip(position.saturating_add(1))
            .find_map(|class| self.own_method_named(*class, method))
    }

    /// Check the program bases of class `class_name` (first base first): each
    /// must be a lowered program class, none generic, an exception class or a
    /// modeled builtin, and together they must have a C3 linearization.
    fn validate_multiple_bases(
        &self,
        class_name: &str,
        own: Symbol,
        bases: &[Symbol],
        span: Span,
    ) -> Result<(), SmeltError> {
        let refuse =
            |reason: String| Err(SmeltError::no_multiple_inheritance(span, class_name, &reason));
        for base in bases {
            let base_name = self.ctx.krate.symbols.source(*base).unwrap_or_default();
            let Some(class) = self.class_item_named(*base) else {
                return refuse(format!(
                    "needs every base to be a class of this module; '{base_name}' is not"
                ));
            };
            if !class.type_params.is_empty() {
                return refuse(format!("from the generic class '{base_name}' is not supported"));
            }
            if self.exception_classes.contains_key(base_name) {
                return refuse(format!(
                    "from the exception class '{base_name}' is not supported"
                ));
            }
        }
        if self.exception_classes.contains_key(class_name) {
            return refuse("of an exception class is not supported".to_owned());
        }
        let bases_of = |class: Symbol| {
            if class == own {
                return bases.to_vec();
            }
            self.class_item_named(class)
                .map(Class::program_bases)
                .unwrap_or_default()
        };
        match smelt_hir::mro::linearize(own, &bases_of) {
            Err(smelt_hir::mro::MroError::Inconsistent(_)) => refuse(
                "with these bases has no consistent method resolution order (CPython raises TypeError)"
                    .to_owned(),
            ),
            Err(smelt_hir::mro::MroError::Cycle(_)) => {
                refuse("forms a cycle among the bases".to_owned())
            }
            Ok(_) => Ok(()),
        }
    }

    /// The constructor of class `class_sym` in a multiple-inheritance
    /// hierarchy, where `__init__` is an ordinary method (`init`, the one the
    /// class's MRO resolves): the instance is allocated with every field at
    /// its default (as every constructor does), then `self.__init__(..)` runs
    /// with the constructor's own arguments, which are `init`'s.
    fn synthesize_method_init_constructor(
        &mut self,
        class_sym: Symbol,
        class_ty: TypeId,
        init: ItemId,
        span: Span,
    ) -> Result<ItemId, SmeltError> {
        let Item::Function(init_fn) = self.item_ref(init).clone() else {
            return Err(SmeltError::unsupported(span, "an `__init__` method is not a function"));
        };
        let init_sym = self.intern_name("__init__");
        let none_ty = self.intern_type(Type::None);
        let (body, params) = self.with_fn_scope(FnKind::Sync, |this| {
            let mut body = Body::new(None, span);
            let self_local = body.push_local(LocalDecl {
                name: Some(this.intern_name("self")),
                ty: class_ty,
                mutable: false,
                span,
            });
            this.scope.locals.insert("self".to_owned(), self_local);
            let mut params = Vec::new();
            let mut args = Vec::new();
            // The method's first parameter is its receiver.
            for inherited in init_fn.params.iter().skip(1) {
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
            let receiver = body.push_expr(HirExpr {
                kind: ExprKind::Local(self_local),
                ty: class_ty,
                span,
            });
            let call = body.push_expr(HirExpr {
                kind: ExprKind::Method {
                    receiver,
                    method: init_sym,
                    args,
                },
                ty: none_ty,
                span,
            });
            let root = body.root;
            body.push_stmt_to_block(root, HirStmt::Expr(call));
            let result = body.push_expr(HirExpr {
                kind: ExprKind::Local(self_local),
                ty: class_ty,
                span,
            });
            body.blocks[usize::try_from(root.0).unwrap_or(0)].tail = Some(result);
            (body, params)
        });
        let body_id = self.ctx.krate.push_body(body);
        Ok(self.ctx.krate.push_item(Item::Function(Function {
            name: init_sym,
            span,
            type_params: Vec::new(),
            params,
            rest: None,
            required_params: init_fn.required_params.map(|required| required.saturating_sub(1)),
            return_ty: class_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Constructor { class: class_sym },
        })))
    }

    /// The `__init__` alias of a class whose MRO has no `__init__` after the
    /// definer: `object.__init__()`, which takes no argument and does nothing.
    fn synthesize_object_init_alias(
        &mut self,
        class_sym: Symbol,
        class_ty: TypeId,
        alias_sym: Symbol,
        span: Span,
    ) -> ItemId {
        let none_ty = self.intern_type(Type::None);
        let (body, self_param) = self.with_fn_scope(FnKind::Sync, |this| {
            let mut body = Body::new(None, span);
            let self_sym = this.intern_name("self");
            let self_local = body.push_local(LocalDecl {
                name: Some(self_sym),
                ty: class_ty,
                mutable: false,
                span,
            });
            body.params.push(self_local);
            (
                body,
                Param {
                    name: self_sym,
                    local: self_local,
                    ty: class_ty,
                    span,
                },
            )
        });
        let body_id = self.ctx.krate.push_body(body);
        self.ctx.krate.push_item(Item::Function(Function {
            name: alias_sym,
            span,
            type_params: Vec::new(),
            params: vec![self_param],
            rest: None,
            required_params: None,
            return_ty: none_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::ClassMethod {
                class: class_sym,
                method: alias_sym,
            },
        }))
    }
}
