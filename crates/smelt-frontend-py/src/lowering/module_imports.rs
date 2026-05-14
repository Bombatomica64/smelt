impl ModuleBuilder<'_> {

    /// Lower a Python `import module [as alias]` statement to HIR metadata.
    fn import_stmt(&mut self, stmt: &StmtImport, hir_module: &mut Module) {
        let span = self.span(stmt.range);
        for alias in &stmt.names {
            let module_name = alias.name.as_str();
            let local = alias
                .asname
                .as_ref()
                .map_or(module_name, |identifier| identifier.as_str());
            let name = self.intern_name(module_name);
            let alias_symbol = (local != module_name).then(|| self.intern_name(local));
            hir_module.imports.push(Import {
                module: module_name.to_owned(),
                name,
                alias: alias_symbol,
                span,
            });
            self.alias_imported_item(module_name, local);
            if let Some(namespace) = self.ctx.module_namespaces.get(module_name).cloned() {
                self.module_namespaces.insert(local.to_owned(), namespace);
            }
            if let Some(namespace) = self.module_namespaces.get(module_name).cloned() {
                self.module_namespaces.insert(local.to_owned(), namespace);
            }
        }
    }

    /// Lower a Python `from module import name [as alias]` statement to HIR metadata.
    fn import_from_stmt(&mut self, stmt: &StmtImportFrom, hir_module: &mut Module) {
        let span = self.span(stmt.range);
        let module = stmt
            .module
            .as_ref()
            .map_or_else(String::new, |module| module.as_str().to_owned());
        let resolved_module = self.resolve_import_from_module(&module, stmt.level);
        for alias in &stmt.names {
            let imported = alias.name.as_str();
            let local = alias
                .asname
                .as_ref()
                .map_or(imported, |identifier| identifier.as_str());
            let name = self.intern_name(imported);
            let alias_symbol = (local != imported).then(|| self.intern_name(local));
            hir_module.imports.push(Import {
                module: resolved_module.clone(),
                name,
                alias: alias_symbol,
                span,
            });
            self.alias_imported_item_from_module(&resolved_module, imported, local);
            if let Some(item) = self.items.get(local).copied()
                && !hir_module.items.contains(&item)
            {
                self.exports.insert(local.to_owned(), item);
            }
        }
    }

    /// Add a local alias for an imported item when it is already known.
    fn alias_imported_item(&mut self, imported: &str, local: &str) {
        if let Some(item) = self.items.get(imported).copied() {
            self.items.insert(local.to_owned(), item);
        }
    }

    /// Resolve a `from ... import ...` source through package-aware namespace metadata.
    fn alias_imported_item_from_module(&mut self, module: &str, imported: &str, local: &str) {
        if let Some(item) = self
            .module_namespaces
            .get(module)
            .and_then(|members| members.get(imported))
            .copied()
            .or_else(|| {
                self.ctx
                    .module_namespaces
                    .get(module)
                    .and_then(|members| members.get(imported))
                    .copied()
            })
        {
            self.items.insert(local.to_owned(), item);
            self.exports.insert(local.to_owned(), item);
            return;
        }
        self.alias_imported_item(imported, local);
        if let Some(item) = self.items.get(local).copied() {
            self.exports.insert(local.to_owned(), item);
        }
    }

    /// Convert relative import metadata to the package/module key stored by previous files.
    fn resolve_import_from_module(&self, module: &str, level: u32) -> String {
        if level == 0 {
            return module.to_owned();
        }
        let package = package_name_from_path(&self.path);
        match (package.as_deref(), module.is_empty()) {
            (Some(package_name), true) => package_name.to_owned(),
            (Some(package_name), false) => format!("{package_name}.{module}"),
            (None, _) => module.to_owned(),
        }
    }

    /// Register this module's lowered items for later `import module` namespace access.
    fn register_module_namespace(&mut self, hir_module: &Module) {
        let mut namespace = self.exports.clone();
        for item_id in &hir_module.items {
            let Some(item) = self
                .ctx
                .krate
                .items
                .get(usize::try_from(item_id.0).unwrap_or(usize::MAX))
            else {
                continue;
            };
            let Some(name) = item_name(&self.ctx.krate, item) else {
                continue;
            };
            namespace.insert(name.to_owned(), *item_id);
        }
        if namespace.is_empty() {
            return;
        }
        for module_name in module_names_from_path(&self.path) {
            self.ctx
                .module_namespaces
                .insert(module_name.clone(), namespace.clone());
            self.module_namespaces
                .insert(module_name, namespace.clone());
        }
    }

    /// Lower `NAME = ClassName(...)` into an importable constructed constant item.
    ///
    /// Only constructors already known as class items are accepted.  The value
    /// is stored as a small HIR body so package imports can inline the same
    /// object construction at use sites without modelling Python module
    /// initialization order.
    fn constructed_constant_assign(
        &mut self,
        assign: &ruff_python_ast::StmtAssign,
        hir_module: &mut Module,
    ) -> Result<(), SmeltError> {
        let Some((name, call)) = Self::constructed_constant_shape(assign) else {
            return Ok(());
        };
        let Some(constructor) = stdlib_dispatch::constructed_constant_constructor(call) else {
            return Ok(());
        };
        let Some(&class_item) = self.items.get(constructor) else {
            return Ok(());
        };
        let Item::Class(class) = self.item_ref(class_item) else {
            return Ok(());
        };
        let class_sym = class.name;
        let span = self.span(assign.range);
        let class_ty = self.intern_type(Type::Class {
            name: class_sym,
            args: vec![],
        });
        let mut const_body = Body::new(None, span);
        let args = call
            .arguments
            .args
            .iter()
            .map(|arg| self.expression(arg, &mut const_body))
            .collect::<Result<Vec<_>, _>>()?;
        let value = const_body.push_expr(HirExpr {
            kind: ExprKind::New {
                class: class_sym,
                args,
            },
            ty: class_ty,
            span,
        });
        let body_id = self.ctx.krate.push_body(const_body);
        let name_sym = self.intern_name(name);
        let item = self.ctx.krate.push_item(Item::Const(ConstItem {
            name: name_sym,
            ty: class_ty,
            value,
            body: body_id,
            span,
        }));
        self.items.insert(name.to_owned(), item);
        self.exports.insert(name.to_owned(), item);
        hir_module.items.push(item);
        Ok(())
    }

    /// Return whether a module statement was already captured as a constructed constant.
    fn is_constructed_constant_assignment(&self, stmt: &Stmt) -> bool {
        let Stmt::Assign(assign) = stmt else {
            return false;
        };
        let Some((_name, call)) = Self::constructed_constant_shape(assign) else {
            return false;
        };
        stdlib_dispatch::constructed_constant_constructor(call)
            .is_some_and(|constructor| matches!(self.items.get(constructor), Some(item) if matches!(self.item_ref(*item), Item::Class(_))))
    }

    /// Extract the direct `NAME = ClassName(...)` constructed-constant shape.
    fn constructed_constant_shape(
        assign: &ruff_python_ast::StmtAssign,
    ) -> Option<(&str, &ruff_python_ast::ExprCall)> {
        let [target] = assign.targets.as_slice() else {
            return None;
        };
        let Expr::Name(name) = target else {
            return None;
        };
        let Expr::Call(call) = assign.value.as_ref() else {
            return None;
        };
        Some((name.id.as_str(), call))
    }

    /// Read the type attached to an expression id.
    fn expr_ty(body: &Body, expr_id: smelt_hir::ExprId) -> TypeId {
        let index = usize::try_from(expr_id.0).unwrap_or(usize::MAX);
        body.exprs[index].ty
    }

    /// Read the span attached to an expression id.
    fn expr_span(body: &Body, expr_id: smelt_hir::ExprId) -> Span {
        let index = usize::try_from(expr_id.0).unwrap_or(usize::MAX);
        body.exprs[index].span
    }

    /// Read the type attached to a local id.
    fn local_ty(body: &Body, local_id: smelt_hir::LocalId) -> TypeId {
        let index = usize::try_from(local_id.0).unwrap_or(usize::MAX);
        body.locals[index].ty
    }

    /// Read a crate item by id.
    fn item_ref(&self, item_id: ItemId) -> &Item {
        let index = usize::try_from(item_id.0).unwrap_or(usize::MAX);
        &self.ctx.krate.items[index]
    }

    // Class lowering continues in `class.rs`.
}
