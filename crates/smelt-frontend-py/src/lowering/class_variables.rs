// Python class variables: class-body `NAME = <literal>` bindings.
// Included into lowering.rs via include!, so this file uses plain comments.
//
// A class variable is an attribute of the class object. `A.count` reads it,
// `A.count += 1` rebinds the one value every instance sees, and
// `self.count` / `obj.count` read it as long as the instance has no attribute
// of its own by that name (CPython looks the instance dict up first, then the
// class and its bases).
//
// Representation (the type is always the literal's: `int`, `float`, `str`,
// `bool`):
//
// * A class variable the program never stores to through its class is a
//   constant: `StaticField::value` is its literal and every read folds it.
// * One the module stores to (`A.count = v`, `A.count += v`, or `cls.count`
//   in a classmethod) is live storage: an `Item::MutableGlobal` named in
//   `StaticField::storage`, read with `GlobalGet` and written with
//   `GlobalSet`, exactly as a TypeScript mutable static.
//
// Which ones are stored to is decided before any class is lowered, by a scan
// of the whole module (`ClassAttributeStores`), so a method lowered before a
// later write still reads the live value. The same scan records the module's
// class hierarchy, which `type(x).__name__` reads (`lowering/type_name.rs`).
//
// Instance shadowing: `self.count = v` at the top level of `__init__`
// declares an instance attribute (`implicit_constructor_fields`, which takes
// the class variable's type when the value's own type is not knowable
// up front), and reads through an instance then see the instance attribute,
// as in CPython. A store through an instance outside that declaration, and a
// store through a subclass (`B.count = v`, which in CPython gives `B` its own
// attribute shadowing the inherited one), are refused with a diagnostic
// rather than lowered to a write of the shared value.

/// What one module stores to `<name>.<attr>`, collected before lowering.
#[derive(Default)]
struct ClassAttributeStores {
    /// `(receiver, attribute)` for every attribute store target whose
    /// receiver is a bare name; a store through `cls` is recorded under the
    /// enclosing class's name.
    stores: HashSet<(String, String)>,
    /// Every name some class of the module lists as a base.
    bases: HashSet<String>,
    /// Each class of the module, by name, with the names of its bases, in
    /// source order.
    class_bases: Vec<(String, Vec<String>)>,
    /// The enclosing class names while walking, innermost last.
    classes: Vec<String>,
}

impl ClassAttributeStores {
    /// Record the attribute stores in one assignment target.
    fn record_target(&mut self, target: &Expr) {
        match target {
            Expr::Attribute(attribute) => {
                if let Expr::Name(receiver_name) = attribute.value.as_ref() {
                    let owner = match receiver_name.id.as_str() {
                        "cls" => self.classes.last().cloned(),
                        name => Some(name.to_owned()),
                    };
                    if let Some(owner_name) = owner {
                        self.stores.insert((owner_name, attribute.attr.as_str().to_owned()));
                    }
                }
            }
            Expr::Tuple(tuple) => tuple.elts.iter().for_each(|elt| self.record_target(elt)),
            Expr::List(list) => list.elts.iter().for_each(|elt| self.record_target(elt)),
            Expr::Starred(starred) => self.record_target(&starred.value),
            _ => {}
        }
    }
}

impl<'a> ruff_python_ast::visitor::Visitor<'a> for ClassAttributeStores {
    fn visit_stmt(&mut self, stmt: &'a Stmt) {
        match stmt {
            Stmt::ClassDef(class) => {
                let mut named_bases = Vec::new();
                for base_expr in class.bases() {
                    if let Expr::Name(base_name) = base_expr {
                        self.bases.insert(base_name.id.as_str().to_owned());
                        named_bases.push(base_name.id.as_str().to_owned());
                    }
                }
                self.class_bases.push((class.name.as_str().to_owned(), named_bases));
                self.classes.push(class.name.as_str().to_owned());
                ruff_python_ast::visitor::walk_stmt(self, stmt);
                self.classes.pop();
                return;
            }
            Stmt::Assign(assign) => assign.targets.iter().for_each(|target| self.record_target(target)),
            Stmt::AugAssign(aug) => self.record_target(&aug.target),
            Stmt::AnnAssign(ann) => self.record_target(&ann.target),
            Stmt::Delete(delete) => delete.targets.iter().for_each(|target| self.record_target(target)),
            Stmt::For(for_stmt) => self.record_target(&for_stmt.target),
            Stmt::With(with_stmt) => {
                for item in &with_stmt.items {
                    if let Some(vars) = item.optional_vars.as_deref() {
                        self.record_target(vars);
                    }
                }
            }
            _ => {}
        }
        ruff_python_ast::visitor::walk_stmt(self, stmt);
    }
}

/// Where a store to a class variable lands.
#[derive(Debug, Clone, Copy)]
struct ClassVariableStore {
    /// The variable's live storage.
    item: ItemId,
    /// Its type.
    ty: TypeId,
}

impl ModuleBuilder<'_> {
    /// Scan `module` for the attribute stores that make a class variable live
    /// storage; see the file header.
    fn collect_class_attribute_stores(&mut self, module: &ModModule) {
        let mut stores = ClassAttributeStores::default();
        for stmt in &module.body {
            ruff_python_ast::visitor::Visitor::visit_stmt(&mut stores, stmt);
        }
        self.class_attribute_stores = stores.stores;
        self.subclassed_names = stores.bases;
        self.module_class_bases = stores.class_bases;
    }

    /// Lower the class variables of `class`'s body (every `NAME = <literal>`
    /// that is not enum or layout metadata), before any method is lowered.
    ///
    /// A variable the module stores to gets its storage global here, pushed
    /// onto `hir_module`. A class-body assignment of any other shape is
    /// refused.
    fn class_variables(
        &mut self,
        class: &StmtClassDef,
        is_int_enum: bool,
        hir_module: &mut Module,
    ) -> Result<Vec<smelt_hir::StaticField>, SmeltError> {
        let class_name = class.name.as_str();
        let mut fields = Vec::new();
        for statement in &class.body {
            let Stmt::Assign(assign) = statement else {
                continue;
            };
            if is_int_enum || is_class_metadata_assignment(assign) {
                continue;
            }
            let Some(mut field) = self.class_var_static_field(assign)? else {
                return Err(SmeltError::unsupported(
                    self.span(assign.range),
                    format!(
                        "class '{class_name}': class-level assignment must be a single name bound to a literal"
                    ),
                ));
            };
            let field_name = self.ctx.krate.symbols.get(field.name).unwrap_or_default().to_owned();
            if self
                .class_attribute_stores
                .contains(&(class_name.to_owned(), field_name.clone()))
                && let Some(value) = field.value.clone()
            {
                let symbol = self.intern_synthesized_name(&format!("{class_name}_{field_name}"));
                let storage = self.ctx.krate.push_item(Item::MutableGlobal(smelt_hir::MutableGlobalItem {
                    name: symbol,
                    ty: field.ty,
                    init: smelt_hir::MutableGlobalInit::Literal(value),
                    visibility: Visibility::Public,
                    span: field.span,
                }));
                hir_module.items.push(storage);
                field.mutable = true;
                field.storage = Some(storage);
            }
            fields.push(field);
        }
        Ok(fields)
    }

    /// Store `fields` as the static fields of the class item in `slot`.
    fn set_class_static_fields(&mut self, slot: ItemId, fields: Vec<smelt_hir::StaticField>) {
        if let Some(Item::Class(class)) = usize::try_from(slot.0)
            .ok()
            .and_then(|index| self.ctx.krate.items.get_mut(index))
        {
            class.static_fields = fields;
        }
    }

    /// The class variable `member` of class `class_sym` or of the nearest
    /// base declaring it, and whether `class_sym` itself declares it.
    ///
    /// An instance field or descriptor of that name declared on the way up
    /// does not stop the walk: callers that read through an instance check
    /// the instance attributes first.
    fn class_variable(&self, class_sym: Symbol, member: &str) -> Option<(smelt_hir::StaticField, bool)> {
        let mut current = Some(class_sym);
        let mut own = true;
        for _ in 0..64_u32 {
            let class = self.class_item_by_symbol(current?)?;
            if let Some(field) = class
                .static_fields
                .iter()
                .find(|field| self.ctx.krate.symbols.get(field.name) == Some(member))
            {
                return Some((field.clone(), own));
            }
            current = class.base;
            own = false;
        }
        None
    }

    /// The class item named by `class_sym`, through this module's items.
    fn class_item_by_symbol(&self, class_sym: Symbol) -> Option<&Class> {
        let name = self.ctx.krate.symbols.get(class_sym)?;
        let item = self.items.get(name).copied()?;
        match self.item_ref(item) {
            Item::Class(class) => Some(class),
            _ => None,
        }
    }

    /// The class an attribute's receiver denotes as a class object: a class
    /// name (bare or module-qualified), or the `cls` of a classmethod (bound
    /// to its own class, as `cls(...)` is). The flag is whether it is `cls`.
    fn class_object_receiver(&self, attr: &ruff_python_ast::ExprAttribute, body: &Body) -> Option<(Symbol, bool)> {
        if let Expr::Name(name) = attr.value.as_ref()
            && name.id.as_str() == "cls"
            && let Some(&local) = self.scope.locals.get("cls")
            && let Some(Type::Class { name: class_sym, .. }) = self.ctx.krate.types.get(Self::local_ty(body, local))
        {
            return Some((*class_sym, true));
        }
        let (class_name, _) = self.class_member_path(attr)?;
        let item = self.items.get(class_name).copied()?;
        match self.item_ref(item) {
            Item::Class(class) => Some((class.name, false)),
            _ => None,
        }
    }

    /// The read of a class variable: its live storage, or its folded literal.
    fn class_variable_read(field: &smelt_hir::StaticField, span: Span, body: &mut Body) -> Option<smelt_hir::ExprId> {
        let kind = match (field.storage, &field.value) {
            (Some(item), _) => ExprKind::GlobalGet { item },
            (None, Some(value)) => ExprKind::Literal(value.clone()),
            (None, None) => return None,
        };
        Some(body.push_expr(HirExpr { kind, ty: field.ty, span }))
    }

    /// Lower `Class.NAME` / `cls.NAME` when it names a class variable of the
    /// class or one of its bases, or `None` so the caller falls through.
    fn class_static_member_expression(
        &self,
        attr: &ruff_python_ast::ExprAttribute,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some((class_sym, _)) = self.class_object_receiver(attr, body) else {
            return Ok(None);
        };
        let Some((field, _)) = self.class_variable(class_sym, attr.attr.as_str()) else {
            return Ok(None);
        };
        Ok(Self::class_variable_read(&field, self.span(attr.range), body))
    }

    /// Lower `obj.NAME` read through an instance of a class with no instance
    /// attribute `NAME` but a class variable `NAME`: the class variable.
    ///
    /// The receiver must be a name, whose evaluation has no effect to keep;
    /// another receiver expression is refused.
    fn instance_class_variable_read(
        &mut self,
        attr: &ruff_python_ast::ExprAttribute,
        receiver_ty: TypeId,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Some(&Type::Class { name: class_sym, .. }) = self.ctx.krate.types.get(receiver_ty) else {
            return Ok(None);
        };
        let member = attr.attr.as_str();
        let member_sym = self.intern_name(member);
        if self.field_type_on_class(class_sym, member_sym).is_some() {
            return Ok(None);
        }
        let Some((field, _)) = self.class_variable(class_sym, member) else {
            return Ok(None);
        };
        if !matches!(attr.value.as_ref(), Expr::Name(_)) {
            return Err(SmeltError::unsupported(
                self.span(attr.range),
                format!("reading class variable `{member}` through a computed instance; bind the instance to a name first"),
            ));
        }
        Ok(Self::class_variable_read(&field, self.span(attr.range), body))
    }

    /// Where a store to `attr` lands when it names a class variable: the
    /// variable's live storage for `Class.NAME` / `cls.NAME` on the class
    /// declaring it, or `None` when `attr` names no class variable.
    ///
    /// A store `CPython` would give a different meaning is refused: through a
    /// subclass (which creates the subclass's own attribute), through the
    /// `cls` of a class that has subclasses (which writes whichever class
    /// the method was called on), and through an instance with no instance
    /// attribute of that name (which creates the instance's own attribute).
    fn class_variable_store(
        &mut self,
        attr: &ruff_python_ast::ExprAttribute,
        body: &Body,
    ) -> Result<Option<ClassVariableStore>, SmeltError> {
        let member = attr.attr.as_str();
        let span = self.span(attr.range);
        let Some((class_sym, via_cls)) = self.class_object_receiver(attr, body) else {
            return self.instance_class_variable_store(attr, body);
        };
        let Some((field, own)) = self.class_variable(class_sym, member) else {
            return Ok(None);
        };
        let class_name = self.ctx.krate.symbols.get(class_sym).unwrap_or_default();
        if !own {
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "storing `{member}` through `{class_name}` gives it its own class attribute shadowing the inherited one; per-subclass class attributes are not modeled"
                ),
            ));
        }
        if via_cls && self.subclassed_names.contains(class_name) {
            return Err(SmeltError::unsupported(
                span,
                format!(
                    "storing `cls.{member}` writes the class the method is called on, and `{class_name}` has subclasses; store through `{class_name}.{member}`"
                ),
            ));
        }
        let Some(item) = field.storage else {
            return Err(SmeltError::unsupported(
                span,
                format!("class variable `{class_name}.{member}` has no storage for this store"),
            ));
        };
        Ok(Some(ClassVariableStore { item, ty: field.ty }))
    }

    /// Refuse a store through an instance to a class variable the instance
    /// has no attribute for (see [`Self::class_variable_store`]).
    fn instance_class_variable_store(
        &mut self,
        attr: &ruff_python_ast::ExprAttribute,
        body: &Body,
    ) -> Result<Option<ClassVariableStore>, SmeltError> {
        let Expr::Name(receiver) = attr.value.as_ref() else {
            return Ok(None);
        };
        let Some(&local) = self.scope.locals.get(receiver.id.as_str()) else {
            return Ok(None);
        };
        let Some(&Type::Class { name: class_sym, .. }) = self.ctx.krate.types.get(Self::local_ty(body, local)) else {
            return Ok(None);
        };
        let member = attr.attr.as_str();
        let member_sym = self.intern_name(member);
        if self.field_type_on_class(class_sym, member_sym).is_some()
            || self.class_variable(class_sym, member).is_none()
        {
            return Ok(None);
        }
        Err(SmeltError::unsupported(
            self.span(attr.range),
            format!(
                "storing `{}.{member}` gives the instance its own attribute shadowing the class variable; assign `self.{member}` at the top of `__init__` to declare it",
                receiver.id
            ),
        ))
    }

    /// Lower `target = value` when `target` is a class variable, or `None`.
    fn class_variable_assign(
        &mut self,
        target: &Expr,
        value: &Expr,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Option<Result<(), SmeltError>> {
        let Expr::Attribute(attr) = target else {
            return None;
        };
        let store = match self.class_variable_store(attr, body) {
            Ok(store) => store?,
            Err(error) => return Some(Err(error)),
        };
        let span = self.span(attr.range);
        let result = (|| {
            let stored = self.expression_with_hint(value, body, Some(store.ty))?;
            if Self::expr_ty(body, stored) != store.ty {
                return Err(SmeltError::unsupported(
                    span,
                    format!("this assignment changes the type of class variable `{}`", attr.attr),
                ));
            }
            let set = body.push_expr(HirExpr {
                kind: ExprKind::GlobalSet { item: store.item, value: stored },
                ty: store.ty,
                span,
            });
            body.push_stmt_to_block(block, HirStmt::Expr(set));
            Ok(())
        })();
        Some(result)
    }

    /// Lower `target op= value` when `target` is a class variable, or `None`.
    fn class_variable_aug_assign(
        &mut self,
        aug: &StmtAugAssign,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Option<Result<(), SmeltError>> {
        let Expr::Attribute(attr) = aug.target.as_ref() else {
            return None;
        };
        let store = match self.class_variable_store(attr, body) {
            Ok(store) => store?,
            Err(error) => return Some(Err(error)),
        };
        Some(self.global_aug_assign(store.item, store.ty, aug, "class variable", body, block))
    }
}
