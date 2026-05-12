impl<'ctx> ModuleBuilder<'ctx> {
    /// Create a new module builder.
    fn new(file_id: FileId, path: String, ctx: &'ctx mut HirCtx) -> Self {
        let (items, classes, interfaces) = Self::visible_items(ctx);
        let const_literals = Self::visible_const_literals(ctx);
        let object_namespaces = ctx.object_namespaces.clone();
        Self {
            file_id,
            path,
            ctx,
            locals: HashMap::new(),
            module_globals: HashMap::new(),
            items,
            classes,
            interfaces,
            class_fields: HashMap::new(),
            current_class: None,
            current_async: false,
            test_builtins: HashSet::new(),
            namespace_imports: HashSet::new(),
            type_only_imports: HashSet::new(),
            object_namespaces,
            const_literals,
            assertion_functions: HashMap::new(),
            narrowed_locals: Vec::new(),
            type_param_scopes: Vec::new(),
            local_callbacks: HashMap::new(),
            function_rests: HashMap::new(),
        }
    }

    /// Collect items already present in the shared crate for cross-module references.
    fn visible_items(
        ctx: &HirCtx,
    ) -> (
        HashMap<String, smelt_hir::ItemId>,
        HashMap<String, smelt_hir::ItemId>,
        HashMap<String, smelt_hir::ItemId>,
    ) {
        let mut items = HashMap::new();
        let mut classes = HashMap::new();
        let mut interfaces = HashMap::new();
        for (idx, item) in ctx.krate.items.iter().enumerate() {
            let item_id = smelt_hir::ItemId(u32::try_from(idx).unwrap_or(u32::MAX));
            let Some(name) = item_name(&ctx.krate, item) else {
                continue;
            };
            insert_visible_item(
                &mut items,
                &mut classes,
                &mut interfaces,
                name,
                item_id,
                item,
            );
        }
        for (alias, item_id) in &ctx.export_aliases {
            let Some(item) = ctx
                .krate
                .items
                .get(usize::try_from(item_id.0).unwrap_or(usize::MAX))
            else {
                continue;
            };
            insert_visible_item(
                &mut items,
                &mut classes,
                &mut interfaces,
                alias,
                *item_id,
                item,
            );
        }
        (items, classes, interfaces)
    }

    /// Collect literal constant items already present in the shared crate.
    fn visible_const_literals(ctx: &HirCtx) -> HashMap<String, ConstLiteral> {
        let mut values = HashMap::new();
        for item in &ctx.krate.items {
            let Item::Const(const_item) = item else {
                continue;
            };
            let Some(name) = item_name(&ctx.krate, item) else {
                continue;
            };
            if let Some(value) = const_literal_from_item(&ctx.krate, const_item) {
                values.insert(name.to_owned(), value);
            }
        }
        for (alias, item_id) in &ctx.export_aliases {
            let Some(Item::Const(const_item)) = ctx
                .krate
                .items
                .get(usize::try_from(item_id.0).unwrap_or(usize::MAX))
            else {
                continue;
            };
            if let Some(value) = const_literal_from_item(&ctx.krate, const_item) {
                values.insert(alias.to_owned(), value);
            }
        }
        values
    }

    /// Lower a TypeScript program to HIR module.
    fn program(&mut self, program: &Program<'_>) -> Result<ModuleId, Vec<SmeltError>> {
        let span = self.span(program.span.start, program.span.end);
        let mut body = Body::new(None, span);
        let mut errors = Vec::new();

        let mut module = Module::new(
            "main",
            SourceFile {
                path: self.path.clone(),
                language: Language::TypeScript,
            },
        );

        let mut before_each = Vec::new();
        let mut after_each = Vec::new();
        let implemented_functions = implemented_function_names(program);
        self.collect_module_globals(program);
        for statement in &program.body {
            if let Statement::ImportDeclaration(import) = statement {
                self.import_declaration(import, &mut module);
                continue;
            }
            if let Statement::FunctionDeclaration(function) = statement {
                if function.declare {
                    continue;
                }
                if is_implemented_overload_signature(function, &implemented_functions) {
                    continue;
                }
                match self.function_declaration(function) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ClassDeclaration(class) = statement {
                match self.class_declaration(class) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::TSInterfaceDeclaration(interface) = statement {
                match self.interface_declaration(interface) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::TSTypeAliasDeclaration(alias) = statement {
                match self.type_alias_declaration(alias) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && self.collect_lifecycle_hook(
                    &expr_stmt.expression,
                    &mut before_each,
                    &mut after_each,
                )
            {
                continue;
            }
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && let Some(table_call) = self.table_test_call(&expr_stmt.expression)
            {
                match self.table_test_declarations(table_call, None, &before_each, &after_each) {
                    Ok(items) => module.items.extend(items),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && let Some(test_call) = self.test_case_call(&expr_stmt.expression)
            {
                match self.test_case_declaration(test_call, None, &before_each, &after_each, &[]) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && let Some(describe_call) = self.describe_call(&expr_stmt.expression)
            {
                match self.describe_declaration(describe_call, &before_each, &after_each) {
                    Ok(items) => module.items.extend(items),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ExportNamedDeclaration(export) = statement
                && let Some(decl) = &export.declaration
            {
                if let Declaration::FunctionDeclaration(function) = decl {
                    if function.declare {
                        continue;
                    }
                    if is_implemented_overload_signature(function, &implemented_functions) {
                        continue;
                    }
                    match self.function_declaration(function) {
                        Ok(item) => module.items.push(item),
                        Err(error) => errors.push(error),
                    }
                } else if let Declaration::ClassDeclaration(class) = decl {
                    match self.class_declaration(class) {
                        Ok(item) => module.items.push(item),
                        Err(error) => errors.push(error),
                    }
                } else if let Declaration::TSInterfaceDeclaration(interface) = decl {
                    match self.interface_declaration(interface) {
                        Ok(item) => module.items.push(item),
                        Err(error) => errors.push(error),
                    }
                } else if let Declaration::TSTypeAliasDeclaration(alias) = decl {
                    match self.type_alias_declaration(alias) {
                        Ok(item) => module.items.push(item),
                        Err(error) => errors.push(error),
                    }
                } else if let Declaration::VariableDeclaration(variable) = decl {
                    match self.const_item_declarations(variable) {
                        Ok(items) => module.items.extend(items),
                        Err(error) => errors.push(error),
                    }
                }
            } else if let Statement::ExportNamedDeclaration(export) = statement
                && export.source.is_some()
            {
                self.reexport_named_declaration(export, &mut module);
            } else if let Statement::ExportAllDeclaration(export) = statement {
                self.reexport_all_declaration(export, &mut module);
            }
        }

        for statement in &program.body {
            if matches!(
                statement,
                Statement::FunctionDeclaration(_)
                    | Statement::ClassDeclaration(_)
                    | Statement::TSInterfaceDeclaration(_)
                    | Statement::TSTypeAliasDeclaration(_)
                    | Statement::ImportDeclaration(_)
                    | Statement::ExportNamedDeclaration(_)
                    | Statement::ExportAllDeclaration(_)
                    | Statement::ExportDefaultDeclaration(_)
            ) {
                continue;
            }

            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let body_id = self.ctx.krate.push_body(body);
        module.body = Some(body_id);
        Ok(self.ctx.krate.push_module(module))
    }

    /// Collect typed top-level variables that functions may read or write.
    fn collect_module_globals(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            match statement {
                Statement::VariableDeclaration(variable) => self.collect_module_global_decl(variable),
                Statement::ExportNamedDeclaration(export) => {
                    if let Some(Declaration::VariableDeclaration(variable)) = &export.declaration {
                        self.collect_module_global_decl(variable);
                    }
                }
                _ => {}
            }
        }
    }

    /// Register annotated module-level variables for later function-body lookup.
    fn collect_module_global_decl(&mut self, decl: &oxc::ast::ast::VariableDeclaration<'_>) {
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                continue;
            };
            let Some(annotation) = &declarator.type_annotation else {
                continue;
            };
            if let Ok(ty) = self.ts_type_to_hir(&annotation.type_annotation) {
                self.module_globals
                    .insert(binding.name.as_str().to_owned(), ty);
            }
        }
    }

    /// Lower `export { name } from "module"` metadata and local aliases.
    fn reexport_named_declaration(
        &mut self,
        export: &oxc::ast::ast::ExportNamedDeclaration<'_>,
        module: &mut Module,
    ) {
        let Some(source) = &export.source else {
            return;
        };
        let source_text = source.value.as_str();
        let span = self.span(export.span.start, export.span.end);
        for specifier in &export.specifiers {
            let imported = module_export_name(&specifier.local);
            let exported = module_export_name(&specifier.exported);
            let name = self.intern_source_name(&imported);
            let alias = (exported != imported).then(|| self.intern_source_name(&exported));
            module.imports.push(Import {
                module: source_text.to_owned(),
                name,
                alias,
                span,
            });
            self.alias_imported_item(&imported, &exported);
            if let Some(item) = self.items.get(&exported).copied() {
                self.ctx.export_aliases.insert(exported.clone(), item);
            }
            if let Some(namespace) = self.object_namespaces.get(&exported).cloned() {
                self.ctx.object_namespaces.insert(exported, namespace);
            }
        }
    }

    /// Lower `export * from "module"` metadata for dependency discovery.
    fn reexport_all_declaration(
        &mut self,
        export: &oxc::ast::ast::ExportAllDeclaration<'_>,
        module: &mut Module,
    ) {
        let source = export.source.value.as_str();
        let span = self.span(export.span.start, export.span.end);
        let name = self.intern_source_name("*");
        let alias = export
            .exported
            .as_ref()
            .map(|exported| self.intern_source_name(&module_export_name(exported)));
        module.imports.push(Import {
            module: source.to_owned(),
            name,
            alias,
            span,
        });
    }

    /// Lower an import declaration into module metadata and local item aliases.
    fn import_declaration(
        &mut self,
        import: &oxc::ast::ast::ImportDeclaration<'_>,
        module: &mut Module,
    ) {
        let source = import.source.value.as_str();
        let span = self.span(import.span.start, import.span.end);
        let Some(specifiers) = &import.specifiers else {
            return;
        };
        for specifier in specifiers {
            let (imported, local) = match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(specifier_data) => {
                    let imported = module_export_name(&specifier_data.imported);
                    let local = specifier_data.local.name.as_str().to_owned();
                    if import.import_kind == ImportOrExportKind::Type
                        || specifier_data.import_kind == ImportOrExportKind::Type
                    {
                        self.type_only_imports.insert(local.clone());
                    }
                    (imported, local)
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier_data) => (
                    "default".to_owned(),
                    specifier_data.local.name.as_str().to_owned(),
                ),
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier_data) => {
                    let local = specifier_data.local.name.as_str().to_owned();
                    self.namespace_imports.insert(local.clone());
                    if import.import_kind == ImportOrExportKind::Type {
                        self.type_only_imports.insert(local.clone());
                    }
                    ("*".to_owned(), local)
                }
            };
            if import.import_kind == ImportOrExportKind::Type {
                self.type_only_imports.insert(local.clone());
            }
            let name = self.intern_source_name(&imported);
            let alias = (local != imported).then(|| self.intern_source_name(&local));
            module.imports.push(Import {
                module: source.to_owned(),
                name,
                alias,
                span,
            });
            if test_support::is_vitest_compatible_module(source)
                && test_support::is_vitest_builtin_name(&imported)
            {
                self.test_builtins.insert(local.clone());
            } else if imported != "*" {
                self.alias_imported_item(&imported, &local);
            }
        }
    }

    /// Add a local alias for an imported item when it is already known.
    fn alias_imported_item(&mut self, imported: &str, local: &str) {
        if let Some(item) = self.items.get(imported).copied() {
            self.items.insert(local.to_owned(), item);
        }
        if let Some(item) = self.classes.get(imported).copied() {
            self.classes.insert(local.to_owned(), item);
        }
        if let Some(item) = self.interfaces.get(imported).copied() {
            self.interfaces.insert(local.to_owned(), item);
        }
        if let Some(value) = self.const_literals.get(imported).cloned() {
            self.const_literals.insert(local.to_owned(), value);
        }
        if let Some(namespace) = self.object_namespaces.get(imported).cloned() {
            self.object_namespaces.insert(local.to_owned(), namespace);
        }
    }

    /// Lower exported literal `const` declarations into importable HIR constant items.
    fn const_item_declarations(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        if decl.kind != oxc::ast::ast::VariableDeclarationKind::Const {
            return Err(SmeltError::unsupported(
                self.span(decl.span.start, decl.span.end),
                "exported variable declarations must use const",
            ));
        }
        let mut items = Vec::new();
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                return Err(SmeltError::unsupported(
                    self.span(declarator.span.start, declarator.span.end),
                    "exported const destructuring is not lowered yet",
                ));
            };
            let init = declarator.init.as_ref().ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(declarator.span.start, declarator.span.end),
                    "exported const declarations require an initializer",
                )
            })?;
            if let Expression::ObjectExpression(object) = init {
                self.object_namespace_const_declaration(binding.name.as_str(), object)?;
                continue;
            }
            if let Expression::ArrowFunctionExpression(arrow) = init {
                let type_hint = declarator
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?;
                let item =
                    self.arrow_function_const_declaration(binding.name.as_str(), arrow, type_hint)?;
                items.push(item);
                continue;
            }
            let value = match self.literal_const_expression(init) {
                Ok(value) => value,
                Err(error) if Self::is_known_non_importable_exported_const(init) => {
                    drop(error);
                    continue;
                }
                Err(error) => return Err(error),
            };
            let span = self.span(binding.span.start, binding.span.end);
            let mut body = Body::new(None, span);
            let expr = body.push_expr(Expr {
                kind: ExprKind::Literal(value.literal.clone()),
                ty: value.ty,
                span,
            });
            let body_id = self.ctx.krate.push_body(body);
            let name_text = binding.name.as_str();
            let name = self.intern_source_name(name_text);
            let item = self.ctx.krate.push_item(Item::Const(ConstItem {
                name,
                ty: value.ty,
                value: expr,
                body: body_id,
                span,
            }));
            self.items.insert(name_text.to_owned(), item);
            self.const_literals.insert(name_text.to_owned(), value);
            items.push(item);
        }
        Ok(items)
    }

    /// Lower an exported object constant that only groups existing exports into namespace metadata.
    fn object_namespace_const_declaration(
        &mut self,
        name_text: &str,
        object: &oxc::ast::ast::ObjectExpression<'_>,
    ) -> Result<(), SmeltError> {
        let mut members = HashMap::new();
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(property.span().start, property.span().end),
                    "exported object namespace constants do not support spread properties yet",
                ));
            };
            if object_property.computed || object_property.method {
                return Err(SmeltError::unsupported(
                    self.span(object_property.span.start, object_property.span.end),
                    "exported object namespace constants require static data properties",
                ));
            }
            let key_text = match &object_property.key {
                PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
                PropertyKey::StringLiteral(lit) => lit.value.to_string(),
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(
                            object_property.key.span().start,
                            object_property.key.span().end,
                        ),
                        "exported object namespace keys must be static string keys",
                    ));
                }
            };
            let Expression::Identifier(value_ident) = &object_property.value else {
                return Err(SmeltError::unsupported(
                    self.span(
                        object_property.value.span().start,
                        object_property.value.span().end,
                    ),
                    "exported object namespace values must reference existing items",
                ));
            };
            let value_name = value_ident.name.as_str();
            let Some(item) = self.items.get(value_name).copied() else {
                return Err(SmeltError::unsupported(
                    self.span(value_ident.span.start, value_ident.span.end),
                    format!("exported object namespace member `{value_name}` is unresolved"),
                ));
            };
            members.insert(key_text, item);
        }
        self.object_namespaces
            .insert(name_text.to_owned(), members.clone());
        self.ctx
            .object_namespaces
            .insert(name_text.to_owned(), members);
        Ok(())
    }

    // Continued in the next split builder file.
}
