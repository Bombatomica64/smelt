impl<'ctx> ModuleBuilder<'ctx> {
    /// Create a new module builder.
    fn new(file_id: FileId, path: String, source: String, ctx: &'ctx mut HirCtx) -> Self {
        let (items, classes, interfaces) = Self::visible_items(ctx);
        let const_literals = Self::visible_const_literals(ctx);
        let const_objects = ctx.object_consts.clone();
        let const_object_value_collections = ctx.object_value_collections.clone();
        let const_collections = ctx.const_collections.clone();
        let object_namespaces = ctx.object_namespaces.clone();
        let function_overloads = ctx.overloads.clone();
        let function_rests = ctx.function_rests.clone();
        let type_alias_fields = ctx.type_alias_fields.clone();
        let interface_extends = ctx.interface_extends.clone();
        let interface_index_values = ctx.interface_index_values.clone();
        let interface_call_signatures = ctx.interface_call_signatures.clone();
        let callable_fields = ctx.callable_fields.clone();
        let callable_object_aliases = ctx.callable_object_aliases.clone();
        let allow_unknown_index_access = Self::is_declaration_type_test_path(&path);
        Self {
            file_id,
            path,
            source,
            ctx,
            locals: HashMap::new(),
            date_value_locals: HashSet::new(),
            module_globals: HashMap::new(),
            items,
            classes,
            pending_class_names: HashSet::new(),
            interfaces,
            class_fields: HashMap::new(),
            class_methods: HashMap::new(),
            class_bases: HashMap::new(),
            type_alias_fields,
            interface_extends,
            interface_index_values,
            interface_call_signatures,
            callable_fields,
            callable_object_aliases,
            type_namespace_prefix: Vec::new(),
            current_class: None,
            current_async: false,
            current_return_ty: None,
            current_generator_yields: None,
            current_arguments_arities: Vec::new(),
            current_statement_block: None,
            deferred_postfix_updates: None,
            allow_unknown_index_access,
            test_builtins: HashSet::new(),
            namespace_imports: HashSet::new(),
            type_only_imports: HashSet::new(),
            value_imports: HashSet::new(),
            date_fns_timezone_factories: HashSet::new(),
            object_namespaces,
            const_literals,
            const_regexps: HashMap::new(),
            const_objects,
            const_collections,
            const_object_value_collections,
            assertion_functions: HashMap::new(),
            predicate_functions: HashMap::new(),
            narrowed_locals: Vec::new(),
            type_param_scopes: Vec::new(),
            type_param_constraint_scopes: Vec::new(),
            local_callbacks: HashMap::new(),
            function_rests,
            forward_function_types: HashMap::new(),
            local_function_items: HashMap::new(),
            function_overloads,
        }
    }

    /// Return whether a source path is a declaration-only type-test module.
    fn is_declaration_type_test_path(path: &str) -> bool {
        path.ends_with(".test-d.ts") || path.ends_with(".test.ts")
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
        let previous_export_aliases = self.ctx.export_aliases.clone();

        let mut before_each = Vec::new();
        let mut after_each = Vec::new();
        let mut top_level_test_setup = Vec::new();
        for statement in &program.body {
            if let Statement::ImportDeclaration(import) = statement {
                self.import_declaration(import, &mut module);
            }
        }
        let implemented_functions = implemented_function_names(program);
        self.shadow_cross_module_overloads(&implemented_functions);
        self.predeclare_type_alias_items(program);
        self.collect_module_globals(program);
        self.pending_class_names = Self::program_class_names(program);
        self.collect_overload_signatures(program, &implemented_functions);
        self.collect_forward_function_types(program, &implemented_functions);
        self.predeclare_function_items(program, &implemented_functions, &mut errors);
        let mut forward_arrow_consts = self.forward_arrow_const_names(program);
        forward_arrow_consts.extend(Self::object_namespace_arrow_const_names(program));
        let mut pending_arrows = program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::VariableDeclaration(variable) => Some(variable),
                _ => None,
            })
            .collect::<Vec<_>>();
        let mut lowered_arrows = HashSet::new();
        while !pending_arrows.is_empty() {
            let index = pending_arrows
                .iter()
                .position(|variable| {
                    self.arrow_const_dependencies_are_lowered(
                        variable,
                        &forward_arrow_consts,
                        &lowered_arrows,
                    )
                })
                .unwrap_or(0);
            let variable = pending_arrows.remove(index);
            match self.arrow_function_const_item_declarations(variable, &forward_arrow_consts) {
                Ok(items) => module.items.extend(items),
                Err(error) => errors.push(error),
            }
            lowered_arrows.extend(Self::arrow_const_declaration_names(variable));
        }
        for statement in &program.body {
            if let Statement::ImportDeclaration(_) = statement {
                continue;
            }
            if let Statement::VariableDeclaration(_) = statement {
                if !self.is_predeclared_arrow_const_statement(statement) {
                    top_level_test_setup.push(statement);
                }
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
            if let Statement::TSModuleDeclaration(module_decl) = statement {
                match self.type_namespace_declaration(module_decl) {
                    Ok(items) => module.items.extend(items),
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
                match self.table_test_declarations(
                    table_call,
                    None,
                    &top_level_test_setup,
                    &before_each,
                    &after_each,
                    &[],
                ) {
                    Ok(items) => module.items.extend(items),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && let Some(test_call) = self.test_case_call(&expr_stmt.expression)
            {
                match self.test_case_declaration(
                    test_call,
                    None,
                    &top_level_test_setup,
                    &before_each,
                    &after_each,
                    &[],
                ) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
                continue;
            }
            if let Statement::ExpressionStatement(expr_stmt) = statement
                && let Some(describe_call) = self.describe_call(&expr_stmt.expression)
            {
                match self.describe_declaration(
                    describe_call,
                    &top_level_test_setup,
                    &before_each,
                    &after_each,
                ) {
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
                } else if let Declaration::TSModuleDeclaration(module_decl) = decl {
                    match self.type_namespace_declaration(module_decl) {
                        Ok(items) => module.items.extend(items),
                        Err(error) => errors.push(error),
                    }
                } else if let Declaration::VariableDeclaration(variable) = decl {
                    match self.const_item_declarations(variable) {
                        Ok(items) => module.items.extend(items),
                        Err(error) => errors.push(error),
                    }
                }
            } else if let Statement::ExportNamedDeclaration(export) = statement {
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
                    | Statement::TSModuleDeclaration(_)
                    | Statement::TSTypeAliasDeclaration(_)
                    | Statement::ImportDeclaration(_)
                    | Statement::ExportNamedDeclaration(_)
                    | Statement::ExportAllDeclaration(_)
                    | Statement::ExportDefaultDeclaration(_)
            ) {
                continue;
            }
            if self.is_predeclared_arrow_const_statement(statement) {
                continue;
            }

            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        self.record_module_exports(&module, &previous_export_aliases);

        let body_id = self.ctx.krate.push_body(body);
        module.body = Some(body_id);
        Ok(self.ctx.krate.push_module(module))
    }

    /// Record item exports lowered from the current source path.
    fn record_module_exports(
        &mut self,
        module: &Module,
        previous_export_aliases: &HashMap<String, smelt_hir::ItemId>,
    ) {
        let mut exports = HashMap::new();
        for item_id in &module.items {
            let Some(item) = self
                .ctx
                .krate
                .items
                .get(usize::try_from(item_id.0).unwrap_or(usize::MAX))
            else {
                continue;
            };
            if let Some(name) = item_name(&self.ctx.krate, item) {
                exports.insert(name.to_owned(), *item_id);
            }
        }
        for (alias, item_id) in &self.ctx.export_aliases {
            if previous_export_aliases.get(alias) != Some(item_id) {
                exports.insert(alias.clone(), *item_id);
            }
        }
        self.ctx
            .module_exports
            .insert(self.path.clone(), exports.clone());
        if let Some(stripped) = self.path.strip_prefix("./") {
            self.ctx
                .module_exports
                .insert(stripped.to_owned(), exports.clone());
        }
        if let Some(canonical) = Self::canonical_module_path(&self.path) {
            self.ctx.module_exports.insert(canonical, exports);
        }
    }

    /// Return a canonical path string when the path exists on disk.
    fn canonical_module_path(path: &str) -> Option<String> {
        std::fs::canonicalize(path)
            .ok()
            .map(|path| path.display().to_string())
    }

    /// Collect class names declared in the current module before lowering eager arrow bodies.
    fn program_class_names(program: &Program<'_>) -> HashSet<String> {
        program
            .body
            .iter()
            .filter_map(|statement| {
                let Statement::ClassDeclaration(class) = statement else {
                    return None;
                };
                class.id.as_ref().map(|id| id.name.to_string())
            })
            .collect()
    }

    /// Drop imported overloads shadowed by implementations in the current module.
    ///
    /// Overload signatures are valid only for their concrete implementation.
    /// Because the builder carries visible items across files, a local helper
    /// with the same name as an imported overloaded function must not inherit
    /// that imported function's return surface.
    fn shadow_cross_module_overloads(&mut self, implemented_functions: &HashSet<String>) {
        for name in implemented_functions {
            self.function_overloads.insert(name.clone(), Vec::new());
        }
    }

    /// Collect typed top-level variables that functions may read or write.
    fn collect_module_globals(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            match statement {
                Statement::VariableDeclaration(variable) => {
                    self.collect_module_global_decl(variable);
                }
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
                if matches!(
                    decl.kind,
                    oxc::ast::ast::VariableDeclarationKind::Const
                        | oxc::ast::ast::VariableDeclarationKind::Let
                ) {
                    let ty = self.ctx.krate.types.intern(Type::Unknown);
                    Self::collect_module_global_pattern_bindings(
                        &declarator.id,
                        ty,
                        &mut self.module_globals,
                    );
                }
                continue;
            };
            if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                && let Some(init) = &declarator.init
                && let Some(object) = Self::object_const_initializer(init)
                && let Ok(value) = self.object_const_from_expression(object, None)
            {
                if let Some(collection) = self.const_collection_from_object_const(&value) {
                    self.const_object_value_collections
                        .insert(binding.name.as_str().to_owned(), collection.clone());
                    self.ctx
                        .object_value_collections
                        .insert(binding.name.as_str().to_owned(), collection);
                }
                self.const_objects
                    .insert(binding.name.as_str().to_owned(), value);
            } else if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                && let Some(init) = &declarator.init
                && let Some(object) = Self::object_const_initializer(init)
                && let Some(collection) = self.const_unknown_value_collection_from_object(object)
            {
                self.const_object_value_collections
                    .insert(binding.name.as_str().to_owned(), collection.clone());
                self.ctx
                    .object_value_collections
                    .insert(binding.name.as_str().to_owned(), collection);
            }
            if matches!(
                decl.kind,
                oxc::ast::ast::VariableDeclarationKind::Const
                    | oxc::ast::ast::VariableDeclarationKind::Let
            ) && let Some(init) = &declarator.init
                && let Expression::RegExpLiteral(literal) = init
            {
                let pattern = Self::regex_literal_pattern_text_without_flags(literal);
                let flags = literal.regex.flags.to_string();
                let ty = self.regexp_type();
                self.module_globals
                    .insert(binding.name.as_str().to_owned(), ty);
                if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const {
                    self.const_regexps
                        .insert(binding.name.as_str().to_owned(), (pattern, flags, ty));
                }
            }
            if matches!(
                decl.kind,
                oxc::ast::ast::VariableDeclarationKind::Const
                    | oxc::ast::ast::VariableDeclarationKind::Let
            ) && let Some(init) = &declarator.init
                && let Ok(value) = self.literal_const_expression(init)
            {
                self.module_globals
                    .insert(binding.name.as_str().to_owned(), value.ty);
                if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const {
                    self.const_literals
                        .insert(binding.name.as_str().to_owned(), value);
                }
            }
            let Some(annotation) = &declarator.type_annotation else {
                if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                    && let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init
                    && let Ok(ty) = self.local_arrow_function_type(arrow, None)
                {
                    self.module_globals
                        .insert(binding.name.as_str().to_owned(), ty);
                }
                if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                    && let Some(init) = &declarator.init
                    && (Self::is_module_global_array_initializer(init)
                        || Self::object_const_initializer(init).is_some())
                {
                    let ty = self
                        .infer_module_global_initializer_type(init)
                        .unwrap_or_else(|_| self.ctx.krate.types.intern(Type::Unknown));
                    self.module_globals
                        .insert(binding.name.as_str().to_owned(), ty);
                    if let Some(collection) = self.const_collection_from_initializer(init, ty) {
                        self.const_collections
                            .insert(binding.name.as_str().to_owned(), collection.clone());
                        self.ctx
                            .const_collections
                            .insert(binding.name.as_str().to_owned(), collection);
                    }
                }
                if decl.kind == oxc::ast::ast::VariableDeclarationKind::Const
                    && let Some(init) = &declarator.init
                    && !matches!(init, Expression::ArrowFunctionExpression(_))
                {
                    let ty = self
                        .infer_module_global_initializer_type(init)
                        .unwrap_or_else(|_| self.ctx.krate.types.intern(Type::Unknown));
                    self.module_globals
                        .insert(binding.name.as_str().to_owned(), ty);
                }
                continue;
            };
            if let Ok(ty) = self.ts_type_to_hir(&annotation.type_annotation) {
                self.module_globals
                    .insert(binding.name.as_str().to_owned(), ty);
            }
        }
    }

    /// Register simple names from a non-identifier module-level binding pattern.
    fn collect_module_global_pattern_bindings(
        pattern: &BindingPattern<'_>,
        ty: smelt_hir::TypeId,
        module_globals: &mut HashMap<String, smelt_hir::TypeId>,
    ) {
        match pattern {
            BindingPattern::BindingIdentifier(binding) => {
                module_globals.insert(binding.name.as_str().to_owned(), ty);
            }
            BindingPattern::ObjectPattern(object) => {
                for property in &object.properties {
                    Self::collect_module_global_pattern_bindings(
                        &property.value,
                        ty,
                        module_globals,
                    );
                }
                if let Some(rest) = &object.rest {
                    Self::collect_module_global_pattern_bindings(
                        &rest.argument,
                        ty,
                        module_globals,
                    );
                }
            }
            BindingPattern::ArrayPattern(array) => {
                for element in array.elements.iter().flatten() {
                    Self::collect_module_global_pattern_bindings(element, ty, module_globals);
                }
                if let Some(rest) = &array.rest {
                    Self::collect_module_global_pattern_bindings(
                        &rest.argument,
                        ty,
                        module_globals,
                    );
                }
            }
            BindingPattern::AssignmentPattern(assignment) => {
                Self::collect_module_global_pattern_bindings(&assignment.left, ty, module_globals);
            }
        }
    }

    /// Return true for top-level const-arrow declarations already emitted as items.
    fn is_predeclared_arrow_const_statement(&self, statement: &Statement<'_>) -> bool {
        let Statement::VariableDeclaration(decl) = statement else {
            return false;
        };
        if decl.kind != oxc::ast::ast::VariableDeclarationKind::Const {
            return false;
        }
        !decl.declarations.is_empty()
            && decl.declarations.iter().all(|declarator| {
                matches!(
                    declarator.init,
                    Some(Expression::ArrowFunctionExpression(_))
                ) && matches!(
                    &declarator.id,
                    BindingPattern::BindingIdentifier(binding)
                        if self.items.contains_key(binding.name.as_str())
                )
            })
    }

    /// Return true for const array initializers whose values must be visible in functions.
    fn is_module_global_array_initializer(init: &Expression<'_>) -> bool {
        match init {
            Expression::ArrayExpression(_) => true,
            Expression::NewExpression(new_expr) => matches!(
                &new_expr.callee,
                Expression::Identifier(callee)
                    if matches!(callee.name.as_str(), "Set" | "Map")
                        || Self::is_numeric_typed_array_constructor(callee.name.as_str())
            ),
            Expression::CallExpression(call) => {
                Self::object_values_identifier_argument(call).is_some()
            }
            Expression::TSAsExpression(as_expr) => {
                Self::is_module_global_array_initializer(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::is_module_global_array_initializer(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::is_module_global_array_initializer(&non_null.expression)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::is_module_global_array_initializer(&parenthesized.expression)
            }
            _ => false,
        }
    }

    /// Infer the type of a top-level constant initializer for later test-body reads.
    fn infer_module_global_initializer_type(
        &mut self,
        init: &Expression<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let mut body = Body::new(None, self.expression_span(init));
        let expr = self.expression_with_hint(init, &mut body, None)?;
        Ok(Self::expr_ty(&body, expr))
    }

    /// Extract literal array and set constants that nested function bodies can inline.
    fn const_collection_from_initializer(
        &mut self,
        init: &Expression<'_>,
        ty: smelt_hir::TypeId,
    ) -> Option<ConstCollection> {
        match init {
            Expression::ArrayExpression(array) => Some(ConstCollection {
                items: self.const_collection_items(array.elements.iter())?,
                ty,
                is_set: false,
            }),
            Expression::NewExpression(new_expr)
                if matches!(
                    &new_expr.callee,
                    Expression::Identifier(callee) if callee.name.as_str() == "Set"
                ) =>
            {
                let [Argument::ArrayExpression(array)] = new_expr.arguments.as_slice() else {
                    return None;
                };
                Some(ConstCollection {
                    items: self.const_collection_items(array.elements.iter())?,
                    ty,
                    is_set: true,
                })
            }
            Expression::CallExpression(call) => {
                let name = Self::object_values_identifier_argument(call)?;
                self.const_object_value_collections.get(&name).cloned()
            }
            Expression::TSAsExpression(as_expr) => {
                self.const_collection_from_initializer(&as_expr.expression, ty)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.const_collection_from_initializer(&satisfies.expression, ty)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.const_collection_from_initializer(&non_null.expression, ty)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.const_collection_from_initializer(&parenthesized.expression, ty)
            }
            _ => None,
        }
    }

    /// Extract literal elements from a module-level constant array.
    fn const_collection_items<'a>(
        &mut self,
        elements: impl Iterator<Item = &'a ArrayExpressionElement<'a>>,
    ) -> Option<Vec<ConstCollectionItem>> {
        let mut items = Vec::new();
        for element in elements {
            match element {
                ArrayExpressionElement::StringLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::String);
                    items.push(ConstCollectionItem {
                        value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::String(
                            literal.value.to_string(),
                        ))),
                        ty,
                    });
                }
                ArrayExpressionElement::NumericLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::Float);
                    items.push(ConstCollectionItem {
                        value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::Float(
                            literal.value,
                        ))),
                        ty,
                    });
                }
                ArrayExpressionElement::BooleanLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::Bool);
                    items.push(ConstCollectionItem {
                        value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::Bool(
                            literal.value,
                        ))),
                        ty,
                    });
                }
                ArrayExpressionElement::NullLiteral(_) => {
                    let ty = self.ctx.krate.types.intern(Type::None);
                    items.push(ConstCollectionItem {
                        value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::None)),
                        ty,
                    });
                }
                ArrayExpressionElement::SpreadElement(spread) => {
                    let Expression::Identifier(identifier) = &spread.argument else {
                        return None;
                    };
                    let collection = self.const_collections.get(identifier.name.as_str())?;
                    items.extend(collection.items.clone());
                }
                ArrayExpressionElement::Elision(_) => return None,
                _ => return None,
            }
        }
        Some(items)
    }

    /// Return the identifier passed to a direct `Object.values(identifier)` call.
    fn object_values_identifier_argument(
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Option<String> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        let Expression::Identifier(object) = &member.object else {
            return None;
        };
        if object.name != "Object" || member.property.name != "values" {
            return None;
        }
        let [Argument::Identifier(identifier)] = call.arguments.as_slice() else {
            return None;
        };
        Some(identifier.name.as_str().to_owned())
    }

    /// Build an `Object.values` collection from a reusable static object const.
    fn const_collection_from_object_const(
        &mut self,
        value: &ObjectConst,
    ) -> Option<ConstCollection> {
        let Type::Dict(_, value_ty) = self.ctx.krate.types.get(value.ty)? else {
            return None;
        };
        let items = value
            .entries
            .iter()
            .map(|entry| {
                let item_value = match &entry.value {
                    ObjectConstValue::Literal(literal) => {
                        ConstCollectionValue::Expr(ExprKind::Literal(literal.clone()))
                    }
                    ObjectConstValue::RegExp { .. } => ConstCollectionValue::UnknownObject,
                    ObjectConstValue::Expr(kind) => ConstCollectionValue::Expr(kind.clone()),
                    ObjectConstValue::List(_) => ConstCollectionValue::UnknownArray,
                    ObjectConstValue::Object(_) => ConstCollectionValue::UnknownObject,
                };
                ConstCollectionItem {
                    value: item_value,
                    ty: entry.value_ty,
                }
            })
            .collect();
        let ty = self.ctx.krate.types.intern(Type::List(*value_ty));
        Some(ConstCollection {
            items,
            ty,
            is_set: false,
        })
    }

    /// Build an approximate erased collection for `Object.values` over dynamic objects.
    ///
    /// Mixed JavaScript value-provider objects are frequently exported as module
    /// constants and projected in tests. The object itself still lowers as a
    /// runtime const; this side table only gives nested function/test bodies a
    /// stable list shape while preserving null versus undefined.
    fn const_unknown_value_collection_from_object(
        &mut self,
        object: &oxc::ast::ast::ObjectExpression<'_>,
    ) -> Option<ConstCollection> {
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let optional_unknown_ty = self.ctx.krate.types.intern(Type::Optional(unknown_ty));
        let mut items = Vec::new();
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return None;
            };
            if object_property.computed || object_property.method {
                return None;
            }
            items.push(self.const_unknown_value_item(&object_property.value, unknown_ty));
        }
        let ty = self.ctx.krate.types.intern(Type::List(optional_unknown_ty));
        Some(ConstCollection {
            items,
            ty,
            is_set: false,
        })
    }

    /// Approximate one dynamic object value as an optional erased JS value.
    fn const_unknown_value_item(
        &mut self,
        expression: &Expression<'_>,
        unknown_ty: smelt_hir::TypeId,
    ) -> ConstCollectionItem {
        match expression {
            Expression::StringLiteral(literal) => ConstCollectionItem {
                value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::String(
                    literal.value.to_string(),
                ))),
                ty: self.ctx.krate.types.intern(Type::String),
            },
            Expression::NumericLiteral(literal) => ConstCollectionItem {
                value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::Float(literal.value))),
                ty: self.ctx.krate.types.intern(Type::Float),
            },
            Expression::BooleanLiteral(literal) => ConstCollectionItem {
                value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::Bool(literal.value))),
                ty: self.ctx.krate.types.intern(Type::Bool),
            },
            Expression::NullLiteral(_) => ConstCollectionItem {
                value: ConstCollectionValue::UnknownNull,
                ty: unknown_ty,
            },
            Expression::Identifier(identifier) if identifier.name == "undefined" => {
                ConstCollectionItem {
                    value: ConstCollectionValue::Expr(ExprKind::Literal(Literal::Undefined)),
                    ty: self.ctx.krate.types.intern(Type::None),
                }
            }
            Expression::ArrayExpression(_) | Expression::NewExpression(_) => ConstCollectionItem {
                value: ConstCollectionValue::UnknownArray,
                ty: unknown_ty,
            },
            Expression::ArrowFunctionExpression(_) | Expression::FunctionExpression(_) => {
                ConstCollectionItem {
                    value: ConstCollectionValue::UnknownFunction,
                    ty: unknown_ty,
                }
            }
            Expression::TSAsExpression(as_expr) => {
                self.const_unknown_value_item(&as_expr.expression, unknown_ty)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                self.const_unknown_value_item(&satisfies.expression, unknown_ty)
            }
            Expression::TSNonNullExpression(non_null) => {
                self.const_unknown_value_item(&non_null.expression, unknown_ty)
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.const_unknown_value_item(&parenthesized.expression, unknown_ty)
            }
            _ => ConstCollectionItem {
                value: ConstCollectionValue::UnknownObject,
                ty: unknown_ty,
            },
        }
    }

    /// Collect TypeScript overload signatures for concrete implementations.
    fn collect_overload_signatures(
        &mut self,
        program: &Program<'_>,
        implemented_functions: &HashSet<String>,
    ) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function)
                    if is_implemented_overload_signature(function, implemented_functions) =>
                {
                    self.collect_overload_signature(function);
                }
                Statement::ExportNamedDeclaration(export) => {
                    if let Some(Declaration::FunctionDeclaration(function)) = &export.declaration
                        && is_implemented_overload_signature(function, implemented_functions)
                    {
                        self.collect_overload_signature(function);
                    }
                }
                _ => {}
            }
        }
    }

    /// Collect one overload signature, skipping signatures outside the current type surface.
    fn collect_overload_signature(&mut self, function: &oxc::ast::ast::Function<'_>) {
        let Some(id) = &function.id else {
            return;
        };
        if let Ok(signature) = self.overload_signature(function) {
            self.function_overloads
                .entry(id.name.to_string())
                .or_default()
                .push(signature.clone());
            self.ctx
                .overloads
                .entry(id.name.to_string())
                .or_default()
                .push(signature);
        }
    }

    /// Lower a TypeScript overload declaration into callable metadata.
    fn overload_signature(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> Result<OverloadSignature, SmeltError> {
        let type_params = self.push_type_parameter_scope(function.type_parameters.as_deref())?;
        let result = (|| {
            let mut params = Vec::new();
            for param in &function.params.items {
                let ty = param
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(param.span.start, param.span.end),
                            "overload parameters must have explicit type annotations",
                        )
                    })?;
                params.push(ty);
            }
            if let Some(rest) = &function.params.rest {
                let annotation = rest.type_annotation.as_ref().ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(rest.span.start, rest.span.end),
                        "overload rest parameters must have explicit type annotations",
                    )
                })?;
                let rest_ty = self.ts_type_to_hir(&annotation.type_annotation)?;
                params.push(self.type_param_constraint_or_self(rest_ty));
            }
            let return_ty = function
                .return_type
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(function.span.start, function.span.end),
                        "overload declarations must have explicit return types",
                    )
                })?;
            Ok(OverloadSignature {
                type_params,
                params,
                rest: function
                    .params
                    .rest
                    .as_ref()
                    .map(|_| function.params.items.len()),
                required_params: Some(
                    function
                        .params
                        .items
                        .iter()
                        .position(|param| {
                            param.optional || Self::formal_parameter_has_default(param)
                        })
                        .unwrap_or(function.params.items.len()),
                ),
                return_ty,
                is_async: function.r#async,
            })
        })();
        self.pop_type_parameter_scope();
        result
    }

    /// Collect top-level function signatures before lowering function bodies.
    fn collect_forward_function_types(
        &mut self,
        program: &Program<'_>,
        implemented_functions: &HashSet<String>,
    ) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    if function.declare
                        || is_implemented_overload_signature(function, implemented_functions)
                    {
                        continue;
                    }
                    self.collect_forward_function_type(function);
                }
                Statement::ExportNamedDeclaration(export) => {
                    let Some(Declaration::FunctionDeclaration(function)) = &export.declaration
                    else {
                        continue;
                    };
                    if function.declare
                        || is_implemented_overload_signature(function, implemented_functions)
                    {
                        continue;
                    }
                    self.collect_forward_function_type(function);
                }
                _ => {}
            }
        }
    }

    /// Collect one function declaration signature for hoisted callback references.
    fn collect_forward_function_type(&mut self, function: &oxc::ast::ast::Function<'_>) {
        let Some(id) = &function.id else {
            return;
        };
        let Ok(_type_params) = self.push_type_parameter_scope(function.type_parameters.as_deref())
        else {
            return;
        };
        let result = self.forward_function_type(function, id.name.as_str());
        self.pop_type_parameter_scope();
        if let Ok((symbol, ty, rest)) = result {
            self.forward_function_types
                .insert(id.name.to_string(), (symbol, ty));
            if let Some(rest) = rest {
                self.function_rests.insert(id.name.to_string(), rest);
                self.ctx.function_rests.insert(id.name.to_string(), rest);
            }
        }
    }

    /// Build the callable type for a forward function declaration.
    fn forward_function_type(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        name_text: &str,
    ) -> Result<(smelt_hir::Symbol, smelt_hir::TypeId, Option<RestParam>), SmeltError> {
        let name = self.intern_source_name(name_text);
        let mut params = Vec::new();
        for param in &function.params.items {
            params.push(self.function_parameter_type(param)?);
        }
        let rest = if let Some(rest) = &function.params.rest {
            let Some(annotation) = &rest.type_annotation else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest function parameters must have explicit array type annotations",
                ));
            };
            let ty = self.ts_type_to_hir(&annotation.type_annotation)?;
            let (ty, item_ty) = self.rest_param_array_type(ty).map_err(|_error| {
                SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest function parameter type must be an array type",
                )
            })?;
            let index = params.len();
            params.push(ty);
            Some(RestParam { index, item_ty })
        } else {
            None
        };
        let return_ty = self
            .function_return_type_or_overload(function, name_text)
            .unwrap_or_else(|_| self.ctx.krate.types.intern(Type::Unknown));
        let mutable_params = self.mutable_params_from_returned_tuple_state(&params, return_ty);
        let ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest: rest.map(|rest| rest.index),
            required_params: None,
            mutable_params,
            return_ty,
            is_async: function.r#async,
            may_throw: false,
        }));
        Ok((name, ty, rest))
    }

    /// Resolve a function return type from its annotation or implementation overloads.
    fn function_return_type_or_overload(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        name_text: &str,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if let Some(return_ty) =
            self.function_return_type_annotation_or_overload(function, name_text)?
        {
            return Ok(return_ty);
        }
        Err(SmeltError::unsupported(
            self.span(function.span.start, function.span.end),
            "function declarations must have an explicit return type",
        ))
    }

    /// Resolve an optional function return annotation or implementation overload.
    fn function_return_type_annotation_or_overload(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        name_text: &str,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        if let Some(return_type) = &function.return_type {
            return self.ts_type_to_hir(&return_type.type_annotation).map(Some);
        }
        if let Some(signature) = self
            .function_overloads
            .get(name_text)
            .and_then(|signatures| signatures.last())
        {
            return Ok(Some(signature.return_ty));
        }
        Ok(None)
    }

    /// Reserve HIR item slots for hoisted top-level function declarations.
    fn predeclare_function_items(
        &mut self,
        program: &Program<'_>,
        implemented_functions: &HashSet<String>,
        errors: &mut Vec<SmeltError>,
    ) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    if function.declare
                        || is_implemented_overload_signature(function, implemented_functions)
                    {
                        continue;
                    }
                    if let Err(error) = self.predeclare_function_item(function) {
                        errors.push(error);
                    }
                }
                Statement::ExportNamedDeclaration(export) => {
                    let Some(Declaration::FunctionDeclaration(function)) = &export.declaration
                    else {
                        continue;
                    };
                    if function.declare
                        || is_implemented_overload_signature(function, implemented_functions)
                    {
                        continue;
                    }
                    if let Err(error) = self.predeclare_function_item(function) {
                        errors.push(error);
                    }
                }
                _ => {}
            }
        }
    }

    /// Lower type aliases early so hoisted function signatures can use them.
    fn predeclare_type_alias_items(&mut self, program: &Program<'_>) {
        for _ in 0_usize..2_usize {
            for statement in &program.body {
                match statement {
                    Statement::TSTypeAliasDeclaration(alias) => {
                        drop(self.type_alias_declaration(alias));
                    }
                    Statement::TSModuleDeclaration(module_decl) => {
                        drop(self.type_namespace_declaration(module_decl));
                    }
                    Statement::ExportNamedDeclaration(export) => {
                        if let Some(Declaration::TSTypeAliasDeclaration(alias)) =
                            &export.declaration
                        {
                            drop(self.type_alias_declaration(alias));
                        } else if let Some(Declaration::TSModuleDeclaration(module_decl)) =
                            &export.declaration
                        {
                            drop(self.type_namespace_declaration(module_decl));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Reserve one function item with its callable signature and no body yet.
    fn predeclare_function_item(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> Result<(), SmeltError> {
        let id = function.id.as_ref().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "anonymous function declarations are not lowered yet",
            )
        })?;
        if self.local_function_items.contains_key(id.name.as_str()) {
            return Ok(());
        }
        let _type_params = self.push_type_parameter_scope(function.type_parameters.as_deref())?;
        let predicate_return = function
            .return_type
            .as_ref()
            .and_then(|annotation| self.predicate_return_type(&annotation.type_annotation))
            .transpose();
        let result = self.predeclared_function(function, id.name.as_str());
        let returns_date = result
            .as_ref()
            .is_ok_and(|predeclared| self.type_is_known_date_value(predeclared.return_ty));
        self.pop_type_parameter_scope();
        let item = self.ctx.krate.push_item(Item::Function(result?));
        if returns_date {
            self.ctx.date_returning_functions.insert(item);
        }
        if let Ok(Some((parameter_name, target))) = predicate_return
            && let Some(param_index) = function.params.items.iter().position(|param| {
                matches!(
                    &param.pattern,
                    BindingPattern::BindingIdentifier(binding)
                        if binding.name.as_str() == parameter_name
                )
            })
        {
            self.predicate_functions.insert(
                id.name.to_string(),
                AssertionNarrowing {
                    param_index,
                    target,
                },
            );
        }
        self.items.insert(id.name.to_string(), item);
        self.local_function_items.insert(id.name.to_string(), item);
        Ok(())
    }

    /// Build the body-less function item used by declaration hoisting.
    fn predeclared_function(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        name_text: &str,
    ) -> Result<Function, SmeltError> {
        let name = self.intern_source_name(name_text);
        let mut params = Vec::new();
        for (index, param) in function.params.items.iter().enumerate() {
            let ty = self.function_parameter_type(param)?;
            let (param_name, span) =
                if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                    (
                        self.intern_source_name(binding.name.as_str()),
                        self.span(binding.span.start, binding.span.end),
                    )
                } else {
                    (
                        self.intern_source_name(&format!("__param{index}")),
                        self.span(param.span.start, param.span.end),
                    )
                };
            params.push(Param {
                name: param_name,
                local: smelt_hir::LocalId(u32::try_from(index).unwrap_or(u32::MAX)),
                ty,
                span,
            });
        }
        let mut rest_index = None;
        if let Some(rest) = &function.params.rest {
            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest parameters need rest binding lowering",
                ));
            };
            let Some(annotation) = &rest.type_annotation else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest function parameters must have explicit array type annotations",
                ));
            };
            let ty = self.ts_type_to_hir(&annotation.type_annotation)?;
            let (ty, item_ty) = self.rest_param_array_type(ty).map_err(|_error| {
                SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest function parameter type must be an array type",
                )
            })?;
            let index = params.len();
            rest_index = Some(index);
            let rest_param = RestParam { index, item_ty };
            self.function_rests.insert(name_text.to_owned(), rest_param);
            self.ctx
                .function_rests
                .insert(name_text.to_owned(), rest_param);
            params.push(Param {
                name: self.intern_source_name(binding.name.as_str()),
                local: smelt_hir::LocalId(u32::try_from(index).unwrap_or(u32::MAX)),
                ty,
                span: self.span(binding.span.start, binding.span.end),
            });
        }
        let return_ty = self
            .function_return_type_or_overload(function, name_text)
            .unwrap_or_else(|_| self.ctx.krate.types.intern(Type::Unknown));
        let required_params = function
            .params
            .items
            .iter()
            .position(|param| param.optional || Self::formal_parameter_has_default(param))
            .unwrap_or(function.params.items.len());
        Ok(Function {
            name,
            span: self.span(function.span.start, function.span.end),
            params,
            rest: rest_index,
            required_params: Some(required_params),
            return_ty,
            is_async: function.r#async,
            is_test: false,
            body: None,
            owner: FunctionOwner::Module,
        })
    }

    /// Lower `export { name } from "module"` metadata and local aliases.
    fn reexport_named_declaration(
        &mut self,
        export: &oxc::ast::ast::ExportNamedDeclaration<'_>,
        module: &mut Module,
    ) {
        let Some(source) = &export.source else {
            for specifier in &export.specifiers {
                let local = module_export_name(&specifier.local);
                let exported = module_export_name(&specifier.exported);
                if let Some(item) = self.items.get(&local).copied() {
                    self.items.insert(exported.clone(), item);
                    self.ctx.export_aliases.insert(exported, item);
                }
            }
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
            self.alias_imported_item(source_text, &imported, &exported);
            if let Some(item) = self.items.get(&exported).copied() {
                self.ctx.export_aliases.insert(exported.clone(), item);
            }
            if let Some(namespace) = self.object_namespaces.get(&exported).cloned() {
                self.ctx
                    .object_namespaces
                    .insert(exported.clone(), namespace);
            }
            if let Some(value) = self.const_objects.get(&exported).cloned() {
                self.ctx.object_consts.insert(exported.clone(), value);
            }
            if let Some(overloads) = self.function_overloads.get(&exported).cloned() {
                self.ctx.overloads.insert(exported, overloads);
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
        if let Some(exported) = export
            .exported
            .as_ref()
            .map(|exported| module_export_name(exported))
        {
            self.alias_source_exports_under_namespace(source, &exported);
        }
    }

    /// Alias exports from one source module under a namespace re-export.
    ///
    /// For `export * as Types from "./types"`, this records aliases such as
    /// `Types.Id` only for exports that came from `./types`. Keeping this
    /// source-scoped prevents namespace imports in large barrel graphs from
    /// repeatedly re-aliasing every previously seen item.
    fn alias_source_exports_under_namespace(&mut self, source: &str, namespace: &str) {
        let Some(exports) = self.source_module_exports(source) else {
            return;
        };
        for (name, item) in exports {
            let alias = format!("{namespace}.{name}");
            self.ctx.export_aliases.insert(alias.clone(), item);
            self.items.insert(alias.clone(), item);
            if self.classes.values().any(|class_item| *class_item == item) {
                self.classes.insert(alias.clone(), item);
            }
            if self
                .interfaces
                .values()
                .any(|interface_item| *interface_item == item)
            {
                self.interfaces.insert(alias, item);
            }
        }
    }

    /// Return the exports for an import source resolved relative to this file.
    fn source_module_exports(&self, source: &str) -> Option<HashMap<String, smelt_hir::ItemId>> {
        self.resolved_module_export_keys(source)
            .into_iter()
            .find_map(|key| self.ctx.module_exports.get(&key).cloned())
    }

    /// Return candidate module-export keys for a TypeScript source specifier.
    fn resolved_module_export_keys(&self, source: &str) -> Vec<String> {
        let mut keys = Vec::new();
        Self::push_module_export_key(&mut keys, source);
        let source_path = Path::new(source);
        let base = if source_path.is_absolute() {
            source_path.to_path_buf()
        } else {
            Path::new(&self.path)
                .parent()
                .unwrap_or_else(|| Path::new(""))
                .join(source_path)
        };
        let candidates = [
            base.clone(),
            base.with_extension("ts"),
            base.with_extension("tsx"),
            base.with_extension("d.ts"),
            base.join("index.ts"),
            base.join("index.d.ts"),
        ];
        for candidate in candidates {
            Self::push_module_export_key(&mut keys, &candidate.display().to_string());
            if let Some(canonical) = Self::canonical_module_path(&candidate.display().to_string()) {
                Self::push_module_export_key(&mut keys, &canonical);
            }
        }
        keys
    }

    /// Push a module-export key and a leading-`./`-less variant.
    fn push_module_export_key(keys: &mut Vec<String>, key: &str) {
        keys.push(key.to_owned());
        if let Some(stripped) = key.strip_prefix("./") {
            keys.push(stripped.to_owned());
        }
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
                    self.alias_source_exports_under_namespace(source, &local);
                    ("*".to_owned(), local)
                }
            };
            if import.import_kind == ImportOrExportKind::Type {
                self.type_only_imports.insert(local.clone());
            } else {
                self.value_imports.insert(local.clone());
                if source == "@date-fns/tz" && imported == "tz" {
                    self.date_fns_timezone_factories.insert(local.clone());
                }
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
                self.alias_imported_item(source, &imported, &local);
                if !self.type_only_imports.contains(&local) && !self.import_alias_resolved(&local) {
                    let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
                    self.module_globals.insert(local.clone(), unknown_ty);
                }
            }
        }
    }

    /// Return whether an imported local already resolves to concrete frontend metadata.
    fn import_alias_resolved(&self, local: &str) -> bool {
        self.items.contains_key(local)
            || self.classes.contains_key(local)
            || self.interfaces.contains_key(local)
            || self.const_literals.contains_key(local)
            || self.const_objects.contains_key(local)
            || self.const_collections.contains_key(local)
            || self.object_namespaces.contains_key(local)
            || self.function_overloads.contains_key(local)
    }

    /// Add a local alias for an imported item when it is already known.
    fn alias_imported_item(&mut self, source: &str, imported: &str, local: &str) {
        if let Some(exports) = self.source_module_exports(source)
            && let Some(item) = exports.get(imported).copied()
        {
            self.items.insert(local.to_owned(), item);
            match self.item_ref(item) {
                Item::Class(_) => {
                    self.classes.insert(local.to_owned(), item);
                }
                Item::Interface(_) => {
                    self.interfaces.insert(local.to_owned(), item);
                }
                _ => {}
            }
            return;
        }
        if let Some(item) = self.items.get(imported).copied() {
            self.items.insert(local.to_owned(), item);
        }
        let imported_prefix = format!("{imported}.");
        let qualified_item_aliases = self
            .items
            .iter()
            .filter_map(|(name, item)| {
                let member = name.strip_prefix(&imported_prefix)?;
                Some((format!("{local}.{member}"), *item))
            })
            .collect::<Vec<_>>();
        for (alias, item) in qualified_item_aliases {
            self.items.insert(alias, item);
        }
        if let Some(item) = self.classes.get(imported).copied() {
            self.classes.insert(local.to_owned(), item);
        }
        let qualified_class_aliases = self
            .classes
            .iter()
            .filter_map(|(name, item)| {
                let member = name.strip_prefix(&imported_prefix)?;
                Some((format!("{local}.{member}"), *item))
            })
            .collect::<Vec<_>>();
        for (alias, item) in qualified_class_aliases {
            self.classes.insert(alias, item);
        }
        if let Some(item) = self.interfaces.get(imported).copied() {
            self.interfaces.insert(local.to_owned(), item);
        }
        let qualified_interface_aliases = self
            .interfaces
            .iter()
            .filter_map(|(name, item)| {
                let member = name.strip_prefix(&imported_prefix)?;
                Some((format!("{local}.{member}"), *item))
            })
            .collect::<Vec<_>>();
        for (alias, item) in qualified_interface_aliases {
            self.interfaces.insert(alias, item);
        }
        if let Some(value) = self.const_literals.get(imported).cloned() {
            self.const_literals.insert(local.to_owned(), value);
        }
        if let Some(value) = self.const_objects.get(imported).cloned() {
            self.const_objects.insert(local.to_owned(), value);
        }
        if let Some(value) = self.const_collections.get(imported).cloned() {
            self.const_collections.insert(local.to_owned(), value);
        }
        if let Some(namespace) = self.object_namespaces.get(imported).cloned() {
            self.object_namespaces.insert(local.to_owned(), namespace);
        }
        if let Some(overloads) = self.function_overloads.get(imported).cloned() {
            self.function_overloads.insert(local.to_owned(), overloads);
        } else if let Some(overloads) = self.ctx.overloads.get(imported).cloned() {
            self.function_overloads.insert(local.to_owned(), overloads);
        }
        if let Some(rest) = self.function_rests.get(imported).copied() {
            self.function_rests.insert(local.to_owned(), rest);
        } else if let Some(rest) = self.ctx.function_rests.get(imported).copied() {
            self.function_rests.insert(local.to_owned(), rest);
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
            let type_hint = declarator
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            if let Some(object) = Self::direct_object_initializer(init) {
                if self.object_namespace_const_declaration(
                    binding.name.as_str(),
                    object,
                    type_hint,
                )? {
                    continue;
                }
            }
            if let Some(object) = Self::object_const_initializer(init) {
                let item = if let Ok(item) =
                    self.object_const_declaration(binding.name.as_str(), object, type_hint)
                {
                    item
                } else {
                    if let Some(collection) =
                        self.const_unknown_value_collection_from_object(object)
                    {
                        self.const_object_value_collections
                            .insert(binding.name.as_str().to_owned(), collection.clone());
                        self.ctx
                            .object_value_collections
                            .insert(binding.name.as_str().to_owned(), collection);
                    }
                    self.dynamic_object_const_declaration(binding.name.as_str(), object, type_hint)?
                };
                items.push(item);
                continue;
            }
            if let Expression::ArrowFunctionExpression(arrow) = init {
                let item =
                    self.arrow_function_const_declaration(binding.name.as_str(), arrow, type_hint)?;
                items.push(item);
                continue;
            }
            if let Some(item) = self.fp_wrapper_const_declaration(binding.name.as_str(), init)? {
                items.push(item);
                continue;
            }
            if Self::is_module_global_array_initializer(init)
                && self.literal_const_expression(init).is_err()
            {
                let span = self.span(binding.span.start, binding.span.end);
                let mut body = Body::new(None, span);
                let expr = self.expression(init, &mut body)?;
                let ty = Self::expr_ty(&body, expr);
                let body_id = self.ctx.krate.push_body(body);
                let name_text = binding.name.as_str();
                let name = self.intern_source_name(name_text);
                let item = self.ctx.krate.push_item(Item::Const(ConstItem {
                    name,
                    ty,
                    value: expr,
                    body: body_id,
                    span,
                }));
                self.items.insert(name_text.to_owned(), item);
                self.ctx.export_aliases.insert(name_text.to_owned(), item);
                self.module_globals.insert(name_text.to_owned(), ty);
                if let Some(collection) = self.const_collection_from_initializer(init, ty) {
                    self.const_collections
                        .insert(name_text.to_owned(), collection.clone());
                    self.ctx
                        .const_collections
                        .insert(name_text.to_owned(), collection);
                }
                items.push(item);
                continue;
            }
            if let Some(identifier) = Self::imported_value_identifier(init)
                && self.value_imports.contains(identifier.name.as_str())
            {
                let span = self.span(binding.span.start, binding.span.end);
                let mut body = Body::new(None, span);
                let expr = self.expression(init, &mut body)?;
                let ty = Self::expr_ty(&body, expr);
                let body_id = self.ctx.krate.push_body(body);
                let name_text = binding.name.as_str();
                let name = self.intern_source_name(name_text);
                let item = self.ctx.krate.push_item(Item::Const(ConstItem {
                    name,
                    ty,
                    value: expr,
                    body: body_id,
                    span,
                }));
                self.items.insert(name_text.to_owned(), item);
                self.ctx.export_aliases.insert(name_text.to_owned(), item);
                self.module_globals.insert(name_text.to_owned(), ty);
                items.push(item);
                continue;
            }
            if matches!(
                init,
                Expression::CallExpression(_) | Expression::NewExpression(_)
            ) && self.literal_const_expression(init).is_err()
            {
                let span = self.span(binding.span.start, binding.span.end);
                let mut body = Body::new(None, span);
                let expr = self.expression(init, &mut body)?;
                let ty = Self::expr_ty(&body, expr);
                let body_id = self.ctx.krate.push_body(body);
                let name_text = binding.name.as_str();
                let name = self.intern_source_name(name_text);
                let item = self.ctx.krate.push_item(Item::Const(ConstItem {
                    name,
                    ty,
                    value: expr,
                    body: body_id,
                    span,
                }));
                self.items.insert(name_text.to_owned(), item);
                self.ctx.export_aliases.insert(name_text.to_owned(), item);
                self.module_globals.insert(name_text.to_owned(), ty);
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
            self.ctx.export_aliases.insert(name_text.to_owned(), item);
            self.const_literals.insert(name_text.to_owned(), value);
            items.push(item);
        }
        Ok(items)
    }

    /// Return the identifier behind a top-level const initializer that aliases an imported value.
    fn imported_value_identifier<'a>(
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::IdentifierReference<'a>> {
        match expression {
            Expression::Identifier(identifier) => Some(identifier),
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::imported_value_identifier(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::imported_value_identifier(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::imported_value_identifier(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::imported_value_identifier(&non_null.expression)
            }
            _ => None,
        }
    }

    /// Lower top-level local arrow `const` declarations into private callable items.
    fn arrow_function_const_item_declarations(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
        forward_arrow_consts: &HashSet<String>,
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        if decl.kind != oxc::ast::ast::VariableDeclarationKind::Const {
            return Ok(Vec::new());
        }
        let mut items = Vec::new();
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                continue;
            };
            let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init else {
                continue;
            };
            if !forward_arrow_consts.contains(binding.name.as_str()) {
                continue;
            }
            let type_hint = declarator
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            let item = self.private_arrow_function_const_declaration(
                binding.name.as_str(),
                arrow,
                type_hint,
            )?;
            items.push(item);
        }
        Ok(items)
    }

    /// Return top-level arrow binding names declared by one variable statement.
    fn arrow_const_declaration_names(
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
    ) -> Vec<String> {
        decl.declarations.iter().filter_map(|declarator| {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                return None;
            };
            matches!(
                declarator.init,
                Some(Expression::ArrowFunctionExpression(_))
            )
            .then(|| binding.name.to_string())
        }).collect()
    }

    /// Check whether a private arrow constant can resolve arrow values it reads.
    ///
    /// Top-level `const` arrows are lexical values rather than hoisted function
    /// declarations. When one arrow returns or passes a later arrow value, the
    /// referenced callable item must be lowered first so the value is not
    /// replaced by a conservative unresolved-global placeholder.
    fn arrow_const_dependencies_are_lowered(
        &self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
        candidates: &HashSet<String>,
        lowered: &HashSet<String>,
    ) -> bool {
        let declared = Self::arrow_const_declaration_names(decl)
            .into_iter()
            .collect::<HashSet<_>>();
        decl.declarations.iter().all(|declarator| {
            let Some(Expression::ArrowFunctionExpression(arrow)) = &declarator.init else {
                return true;
            };
            let text = self
                .source
                .get(
                    usize::try_from(arrow.span.start).unwrap_or(usize::MAX)
                        ..usize::try_from(arrow.span.end).unwrap_or(usize::MAX),
                )
                .unwrap_or_default();
            candidates.iter().all(|candidate| {
                declared.contains(candidate)
                    || !text.contains(candidate)
                    || lowered.contains(candidate)
            })
        })
    }

    /// Find top-level arrow consts that function bodies may reference before declaration order.
    fn forward_arrow_const_names(&self, program: &Program<'_>) -> HashSet<String> {
        let mut arrow_consts = Vec::new();
        let mut referrer_spans = Vec::new();
        for statement in &program.body {
            match statement {
                Statement::VariableDeclaration(variable) => {
                    if variable.kind != oxc::ast::ast::VariableDeclarationKind::Const {
                        continue;
                    }
                    for declarator in &variable.declarations {
                        if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                            && matches!(
                                declarator.init,
                                Some(Expression::ArrowFunctionExpression(_))
                            )
                        {
                            arrow_consts
                                .push((binding.name.as_str().to_owned(), binding.span.start));
                            referrer_spans.push((declarator.span.start, declarator.span.end));
                        }
                    }
                }
                Statement::FunctionDeclaration(function) => {
                    referrer_spans.push((function.span.start, function.span.end));
                }
                Statement::ExportNamedDeclaration(export) => match &export.declaration {
                    Some(Declaration::FunctionDeclaration(function)) => {
                        referrer_spans.push((function.span.start, function.span.end));
                    }
                    Some(Declaration::VariableDeclaration(variable)) => {
                        if variable.kind != oxc::ast::ast::VariableDeclarationKind::Const {
                            continue;
                        }
                        for declarator in &variable.declarations {
                            if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                                && matches!(
                                    declarator.init,
                                    Some(Expression::ArrowFunctionExpression(_))
                                )
                            {
                                arrow_consts
                                    .push((binding.name.as_str().to_owned(), binding.span.start));
                                referrer_spans.push((declarator.span.start, declarator.span.end));
                            }
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        arrow_consts
            .into_iter()
            .filter_map(|(name, const_start)| {
                let referenced = self
                    .source
                    .get(..usize::try_from(const_start).unwrap_or(usize::MAX))
                    .is_some_and(|text| text.contains(&name))
                    || referrer_spans
                        .iter()
                        .filter(|(referrer_start, referrer_end)| {
                            !(const_start >= *referrer_start && const_start <= *referrer_end)
                        })
                        .any(|(function_start, function_end)| {
                            self.source
                                .get(
                                    usize::try_from(*function_start).unwrap_or(usize::MAX)
                                        ..usize::try_from(*function_end).unwrap_or(usize::MAX),
                                )
                                .is_some_and(|text| text.contains(&name))
                        });
                referenced.then_some(name)
            })
            .collect()
    }

    /// Find arrow consts used as values in exported object function tables.
    ///
    /// Date-fns-style tables export objects whose properties point at local
    /// arrow functions and later call them through dynamic keys. Those arrows
    /// need real callable items before the export lowering records namespace
    /// metadata for the table.
    fn object_namespace_arrow_const_names(program: &Program<'_>) -> HashSet<String> {
        let mut arrow_consts = HashSet::new();
        for statement in &program.body {
            let Statement::VariableDeclaration(variable) = statement else {
                continue;
            };
            if variable.kind != oxc::ast::ast::VariableDeclarationKind::Const {
                continue;
            }
            for declarator in &variable.declarations {
                if let BindingPattern::BindingIdentifier(binding) = &declarator.id
                    && matches!(
                        declarator.init,
                        Some(Expression::ArrowFunctionExpression(_))
                    )
                {
                    arrow_consts.insert(binding.name.as_str().to_owned());
                }
            }
        }

        let mut referenced = HashSet::new();
        for statement in &program.body {
            let Statement::ExportNamedDeclaration(export) = statement else {
                continue;
            };
            let Some(Declaration::VariableDeclaration(variable)) = &export.declaration else {
                continue;
            };
            for declarator in &variable.declarations {
                let Some(init) = &declarator.init else {
                    continue;
                };
                let Some(object) = Self::direct_object_initializer(init) else {
                    continue;
                };
                for property in &object.properties {
                    let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                        continue;
                    };
                    if object_property.computed || object_property.method {
                        continue;
                    }
                    if let Expression::Identifier(identifier) = &object_property.value {
                        let name = identifier.name.as_str();
                        if arrow_consts.contains(name) {
                            referenced.insert(name.to_owned());
                        }
                    }
                }
            }
        }
        referenced
    }

    /// Return a directly written object initializer without stripping assertions.
    fn direct_object_initializer<'a>(
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::ObjectExpression<'a>> {
        match expression {
            Expression::ObjectExpression(object) => Some(object),
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::direct_object_initializer(&parenthesized.expression)
            }
            _ => None,
        }
    }

    /// Return an object initializer after removing TypeScript-only wrappers.
    fn object_const_initializer<'a>(
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::ObjectExpression<'a>> {
        match expression {
            Expression::ObjectExpression(object) => Some(object),
            Expression::ParenthesizedExpression(parenthesized) => {
                Self::object_const_initializer(&parenthesized.expression)
            }
            Expression::TSAsExpression(as_expr) => {
                Self::object_const_initializer(&as_expr.expression)
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                Self::object_const_initializer(&satisfies.expression)
            }
            Expression::TSNonNullExpression(non_null) => {
                Self::object_const_initializer(&non_null.expression)
            }
            _ => None,
        }
    }

    /// Lower an exported object constant that only groups existing exports into namespace metadata.
    fn object_namespace_const_declaration(
        &mut self,
        name_text: &str,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<bool, SmeltError> {
        let mut members = HashMap::new();
        let function_hint = self.function_table_value_type(type_hint);
        if type_hint.is_some() && function_hint.is_none() {
            return Ok(false);
        }
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(property.span().start, property.span().end),
                    "exported object namespace constants do not support spread properties yet",
                ));
            };
            if object_property.computed {
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
            if object_property.method {
                let item = self.object_namespace_method_item(
                    name_text,
                    &key_text,
                    object_property,
                    function_hint,
                )?;
                members.insert(key_text, item);
                continue;
            }
            if matches!(
                object_property.value,
                Expression::FunctionExpression(_) | Expression::ArrowFunctionExpression(_)
            ) {
                let item = self.object_namespace_method_item(
                    name_text,
                    &key_text,
                    object_property,
                    function_hint,
                )?;
                members.insert(key_text, item);
                continue;
            }
            let Expression::Identifier(value_ident) = &object_property.value else {
                return Ok(false);
            };
            let value_name = value_ident.name.as_str();
            let Some(item) = self.items.get(value_name).copied() else {
                return Ok(false);
            };
            members.insert(key_text, item);
        }
        self.object_namespaces
            .insert(name_text.to_owned(), members.clone());
        self.ctx
            .object_namespaces
            .insert(name_text.to_owned(), members);
        Ok(true)
    }

    /// Extract the callable value type from a string-keyed function table hint.
    fn function_table_value_type(
        &self,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Option<smelt_hir::TypeId> {
        let hint = type_hint?;
        let Type::Dict(_, value_ty) = self.ctx.krate.types.get(hint)? else {
            return None;
        };
        matches!(self.ctx.krate.types.get(*value_ty), Some(Type::Function(_))).then_some(*value_ty)
    }

    /// Lower one object-method member from an exported namespace object.
    ///
    /// Date-fns uses objects such as `lightFormatters` as plain function tables:
    /// each member is written with object method syntax and later called through
    /// `lightFormatters.y(...)`. Smelt models those members as private module
    /// functions and records the function item in namespace metadata.
    fn object_namespace_method_item(
        &mut self,
        namespace: &str,
        key: &str,
        property: &oxc::ast::ast::ObjectProperty<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let name_text = format!("{namespace}_{key}");
        match &property.value {
            Expression::FunctionExpression(function) => {
                self.function_expression_item_with_source_name(&name_text, function, type_hint)
            }
            Expression::ArrowFunctionExpression(arrow) => {
                self.arrow_function_const_declaration_with_source_name(&name_text, arrow, type_hint)
            }
            _ => Err(SmeltError::unsupported(
                self.span(property.span.start, property.span.end),
                "object namespace methods must lower from function expressions",
            )),
        }
    }

    /// Lower an exported static object constant into importable const metadata.
    fn object_const_declaration(
        &mut self,
        name_text: &str,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let value = self.object_const_from_expression(object, type_hint)?;
        let span = self.span(object.span.start, object.span.end);
        let mut body = Body::new(None, span);
        let expr =
            self.object_const_expression(&value, object.span.start, object.span.end, &mut body);
        let body_id = self.ctx.krate.push_body(body);
        let name = self.intern_source_name(name_text);
        let item = self.ctx.krate.push_item(Item::Const(ConstItem {
            name,
            ty: value.ty,
            value: expr,
            body: body_id,
            span,
        }));
        self.items.insert(name_text.to_owned(), item);
        self.ctx.export_aliases.insert(name_text.to_owned(), item);
        self.module_globals.insert(name_text.to_owned(), value.ty);
        if let Some(collection) = self.const_collection_from_object_const(&value) {
            self.const_object_value_collections
                .insert(name_text.to_owned(), collection.clone());
            self.ctx
                .object_value_collections
                .insert(name_text.to_owned(), collection);
        }
        self.const_objects
            .insert(name_text.to_owned(), value.clone());
        self.ctx.object_consts.insert(name_text.to_owned(), value);
        Ok(item)
    }

    /// Lower an exported object constant whose fields are runtime expressions.
    ///
    /// Static object constants are kept as reusable metadata. Date-fns locale
    /// tables also export object constants whose fields call local helpers such
    /// as `buildFormatLongFn(...)`; those need a real HIR const body instead of
    /// primitive literal folding.
    fn dynamic_object_const_declaration(
        &mut self,
        name_text: &str,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let span = self.span(object.span.start, object.span.end);
        let mut body = Body::new(None, span);
        let expr = self.object_expression(object, &mut body, type_hint)?;
        let ty = Self::expr_ty(&body, expr);
        let body_id = self.ctx.krate.push_body(body);
        let name = self.intern_source_name(name_text);
        let item = self.ctx.krate.push_item(Item::Const(ConstItem {
            name,
            ty,
            value: expr,
            body: body_id,
            span,
        }));
        self.items.insert(name_text.to_owned(), item);
        self.ctx.export_aliases.insert(name_text.to_owned(), item);
        self.module_globals.insert(name_text.to_owned(), ty);
        if let Some(collection) = self.const_unknown_value_collection_from_object(object) {
            self.const_object_value_collections
                .insert(name_text.to_owned(), collection.clone());
            self.ctx
                .object_value_collections
                .insert(name_text.to_owned(), collection);
        }
        Ok(item)
    }

    /// Convert a static object expression into reusable literal-object metadata.
    fn object_const_from_expression(
        &mut self,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<ObjectConst, SmeltError> {
        let mut entries = Vec::new();
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(property.span().start, property.span().end),
                    "object const spread properties are not lowered yet",
                ));
            };
            if object_property.computed || object_property.method {
                return Err(SmeltError::unsupported(
                    self.span(object_property.span.start, object_property.span.end),
                    "object consts require static data properties",
                ));
            }
            let key = match &object_property.key {
                PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
                PropertyKey::StringLiteral(lit) => lit.value.to_string(),
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(
                            object_property.key.span().start,
                            object_property.key.span().end,
                        ),
                        "object const keys must be static string keys",
                    ));
                }
            };
            let (value, value_ty) = self.object_const_entry_value(&object_property.value)?;
            entries.push(ObjectConstEntry {
                key,
                value,
                value_ty,
            });
        }
        let ty = self.object_const_type(&entries, type_hint);
        Ok(ObjectConst { entries, ty })
    }

    /// Lower one reusable static object-constant value.
    ///
    /// Primitive values are stored as literals. Function-valued lookup tables
    /// such as Remeda's `COMPARATORS` store their closure expression so later
    /// module-global reads can recreate the object with callable entries.
    fn object_const_entry_value(
        &mut self,
        expression: &Expression<'_>,
    ) -> Result<(ObjectConstValue, smelt_hir::TypeId), SmeltError> {
        match expression {
            Expression::ParenthesizedExpression(parenthesized) => {
                return self.object_const_entry_value(&parenthesized.expression);
            }
            Expression::TSAsExpression(as_expr) => {
                return self.object_const_entry_value(&as_expr.expression);
            }
            Expression::TSSatisfiesExpression(satisfies) => {
                return self.object_const_entry_value(&satisfies.expression);
            }
            Expression::TSNonNullExpression(non_null) => {
                return self.object_const_entry_value(&non_null.expression);
            }
            _ => {}
        }
        if let Expression::RegExpLiteral(literal) = expression {
            let ty = self.regexp_type();
            return Ok((
                ObjectConstValue::RegExp {
                    pattern: Self::regex_literal_pattern_text_without_flags(literal),
                    flags: literal.regex.flags.to_string(),
                },
                ty,
            ));
        }
        if matches!(expression, Expression::Identifier(identifier) if identifier.name == "undefined")
        {
            let ty = self.ctx.krate.types.intern(Type::Unknown);
            return Ok((ObjectConstValue::Literal(Literal::Undefined), ty));
        }
        if let Ok(literal) = self.literal_const_expression(expression) {
            return Ok((ObjectConstValue::Literal(literal.literal), literal.ty));
        }
        if let Expression::ArrayExpression(array) = expression {
            return self.array_object_const_entry_value(array);
        }
        if let Some(object) = Self::object_const_initializer(expression) {
            let value = self.object_const_from_expression(object, None)?;
            let ty = value.ty;
            return Ok((ObjectConstValue::Object(value), ty));
        }
        if matches!(expression, Expression::ArrowFunctionExpression(_)) {
            let span = self.span(expression.span().start, expression.span().end);
            let mut body = Body::new(None, span);
            let expr = self.expression(expression, &mut body)?;
            let value_ty = Self::expr_ty(&body, expr);
            let kind = body
                .exprs
                .get(usize::try_from(expr.0).ok().unwrap_or(usize::MAX))
                .map(|expr| expr.kind.clone())
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        span,
                        "object const function value did not produce an expression",
                    )
                })?;
            if !matches!(kind, ExprKind::Closure(_)) {
                return Err(SmeltError::unsupported(
                    span,
                    "object const function values must lower to closures",
                ));
            }
            return Ok((ObjectConstValue::Expr(kind), value_ty));
        }
        Err(SmeltError::unsupported(
            self.span(expression.span().start, expression.span().end),
            "object const values must be literals, arrays, objects, or function expressions",
        ))
    }

    /// Lower a literal array nested inside reusable object-constant metadata.
    fn array_object_const_entry_value(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
    ) -> Result<(ObjectConstValue, smelt_hir::TypeId), SmeltError> {
        let mut items = Vec::new();
        for element in &array.elements {
            let expression = match element {
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => {
                    return Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "object const array values do not support spread or elision yet",
                    ));
                }
                ArrayExpressionElement::NumericLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::Float);
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Literal(Literal::Float(literal.value)),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::BigIntLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::Int);
                    let value = literal.value.parse::<i64>().unwrap_or(0);
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Literal(Literal::Int(value)),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::StringLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::String);
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Literal(Literal::String(
                            literal.value.to_string(),
                        )),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::BooleanLiteral(literal) => {
                    let ty = self.ctx.krate.types.intern(Type::Bool);
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Literal(Literal::Bool(literal.value)),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::NullLiteral(_) => {
                    let ty = self.ctx.krate.types.intern(Type::None);
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Literal(Literal::None),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::ArrayExpression(nested_array) => {
                    let (value, ty) = self.array_object_const_entry_value(nested_array)?;
                    items.push(ObjectConstEntryValue { value, ty });
                    continue;
                }
                ArrayExpressionElement::ObjectExpression(object) => {
                    let value = self.object_const_from_expression(object, None)?;
                    let ty = value.ty;
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::Object(value),
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::RegExpLiteral(literal) => {
                    let ty = self.regexp_type();
                    items.push(ObjectConstEntryValue {
                        value: ObjectConstValue::RegExp {
                            pattern: Self::regex_literal_pattern_text_without_flags(literal),
                            flags: literal.regex.flags.to_string(),
                        },
                        ty,
                    });
                    continue;
                }
                ArrayExpressionElement::TSAsExpression(as_expr) => &as_expr.expression,
                ArrayExpressionElement::TSSatisfiesExpression(satisfies) => &satisfies.expression,
                ArrayExpressionElement::TSNonNullExpression(non_null) => &non_null.expression,
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "object const array values must be static literal values",
                    ));
                }
            };
            let (value, ty) = self.object_const_entry_value(expression)?;
            items.push(ObjectConstEntryValue { value, ty });
        }
        let item_ty = items
            .first()
            .map(|item| item.ty)
            .filter(|first_ty| items.iter().all(|item| item.ty == *first_ty))
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
        let ty = self.ctx.krate.types.intern(Type::List(item_ty));
        Ok((ObjectConstValue::List(items), ty))
    }

    /// Infer the HIR dictionary type for a static object const.
    fn object_const_type(
        &mut self,
        entries: &[ObjectConstEntry],
        type_hint: Option<smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        if let Some(ty) = type_hint
            && matches!(self.ctx.krate.types.get(ty), Some(Type::Dict(_, _)))
        {
            return ty;
        }
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let first_value_ty = entries.first().map(|entry| entry.value_ty);
        let value_ty = first_value_ty
            .filter(|first_ty| entries.iter().all(|entry| entry.value_ty == *first_ty))
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
        self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty))
    }

    /// Recreate a static object const inside the currently lowered body.
    fn object_const_expression(
        &mut self,
        value: &ObjectConst,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let key_ty = self.ctx.krate.types.intern(Type::String);
        let span = self.span(start, end);
        let entries = value
            .entries
            .iter()
            .map(|entry| {
                let key = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(entry.key.clone())),
                    ty: key_ty,
                    span,
                });
                let entry_value =
                    self.object_const_value_expression(&entry.value, entry.value_ty, span, body);
                (key, entry_value)
            })
            .collect();
        body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty: value.ty,
            span,
        })
    }

    /// Recreate one nested static object-constant value inside the active body.
    fn object_const_value_expression(
        &mut self,
        value: &ObjectConstValue,
        ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        match value {
            ObjectConstValue::Literal(literal) => body.push_expr(Expr {
                kind: ExprKind::Literal(literal.clone()),
                ty,
                span,
            }),
            ObjectConstValue::RegExp { pattern, flags } => {
                let string_ty = self.ctx.krate.types.intern(Type::String);
                let pattern = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(pattern.clone())),
                    ty: string_ty,
                    span,
                });
                let flags = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(flags.clone())),
                    ty: string_ty,
                    span,
                });
                body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: self.intern_type_name("RegExp"),
                        args: vec![pattern, flags],
                    },
                    ty,
                    span,
                })
            }
            ObjectConstValue::List(items) => {
                let values = items
                    .iter()
                    .map(|item| {
                        self.object_const_value_expression(&item.value, item.ty, span, body)
                    })
                    .collect();
                body.push_expr(Expr {
                    kind: ExprKind::ListLit(values),
                    ty,
                    span,
                })
            }
            ObjectConstValue::Object(object) => {
                self.object_const_expression(object, span.start, span.end, body)
            }
            ObjectConstValue::Expr(kind) => body.push_expr(Expr {
                kind: kind.clone(),
                ty,
                span,
            }),
        }
    }

    // Continued in the next split builder file.
}
