//! TypeScript AST lowering into Smelt HIR.

use std::collections::HashMap;

use crate::{HirCtx, SmeltError, camel_to_snake};
use oxc::allocator::Allocator;
use oxc::ast::ast::{
    Argument, ArrayExpressionElement, AssignmentTarget, BindingPattern, ClassElement, Declaration,
    Expression, ForStatementInit, ForStatementLeft, ImportDeclarationSpecifier,
    MethodDefinitionKind, ModuleExportName, ObjectPropertyKind, Program, PropertyKey,
    SimpleAssignmentTarget, Statement, TSAccessibility, TSSignature, TSTupleElement, TSType,
    TSTypeName,
};
use oxc::parser::{ParseOptions, Parser};
use oxc::span::{GetSpan, SourceType};
use oxc::syntax::operator::{
    AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator, UpdateOperator,
};
use smelt_hir::{
    AsyncOp, BinOp, Body, Class, DictProjectionOp, Expr, ExprKind, Field, FileId, Function,
    FunctionOwner, Import, Interface, Item, Language, ListSearchOp, Literal, LocalDecl, MatchArm,
    MethodSig, Module, ModuleId, NumericExtremaOp, NumericPredicateOp, NumericRoundOp,
    NumericUnaryFuncOp, Param, ParamSig, Pattern, SourceFile, Span, Stmt, StringAffixOp,
    StringCaseOp, StringReplaceOp, StringSearchOp, StringTrimSide, Type, UnaryOp, Visibility,
};

/// Check whether an actual class field type satisfies an interface field.
fn field_type_satisfies(
    krate: &smelt_hir::Crate,
    actual: smelt_hir::TypeId,
    required: &Field,
) -> bool {
    if actual == required.ty {
        return true;
    }
    if !required.optional {
        return false;
    }
    matches!(
        krate.types.get(required.ty),
        Some(Type::Optional(inner)) if *inner == actual
    )
}

/// Parse TypeScript source code and lower it to HIR.
///
/// # Errors
///
/// Returns a vector of errors if parsing or lowering fails.
pub fn to_hir(
    source: &str,
    file_id: FileId,
    ctx: &mut HirCtx,
) -> Result<ModuleId, Vec<SmeltError>> {
    to_hir_with_path(source, file_id, "<memory>", ctx)
}

/// Parse TypeScript source code from `path` and lower it to HIR.
///
/// # Errors
///
/// Returns a vector of errors if parsing or lowering fails.
pub fn to_hir_with_path(
    source: &str,
    file_id: FileId,
    path: &str,
    ctx: &mut HirCtx,
) -> Result<ModuleId, Vec<SmeltError>> {
    let allocator = Allocator::default();
    let source_type = SourceType::default().with_typescript(true);
    let parsed = Parser::new(&allocator, source, source_type)
        .with_options(ParseOptions::default())
        .parse();

    if !parsed.errors.is_empty() {
        return Err(parsed
            .errors
            .into_iter()
            .map(|error| {
                SmeltError::parse(
                    Span::new(file_id, 0, u32::try_from(source.len()).unwrap_or(u32::MAX)),
                    error.to_string(),
                )
            })
            .collect());
    }

    let mut builder = ModuleBuilder::new(file_id, path.to_owned(), ctx);
    builder.program(&parsed.program)
}

/// Builder for lowering TypeScript module to HIR.
///
/// Accumulates scoping information, items, and local variables during module construction.
struct ModuleBuilder<'ctx> {
    /// File ID for error reporting.
    file_id: FileId,
    /// Source path for module metadata.
    path: String,
    /// Mutable reference to the HIR context.
    ctx: &'ctx mut HirCtx,
    /// Local variable bindings in current scope.
    locals: HashMap<String, smelt_hir::LocalId>,
    /// Declared and imported items (functions, classes, interfaces).
    items: HashMap<String, smelt_hir::ItemId>,
    /// Class definitions by name.
    classes: HashMap<String, smelt_hir::ItemId>,
    /// Interface definitions by name.
    interfaces: HashMap<String, smelt_hir::ItemId>,
    /// Fields for each class.
    class_fields: HashMap<String, Vec<Field>>,
    /// Currently processing class name, if any.
    current_class: Option<String>,
    /// Whether the current lowered function body is async.
    current_async: bool,
}

impl<'ctx> ModuleBuilder<'ctx> {
    /// Create a new module builder.
    fn new(file_id: FileId, path: String, ctx: &'ctx mut HirCtx) -> Self {
        let (items, classes, interfaces) = Self::visible_items(ctx);
        Self {
            file_id,
            path,
            ctx,
            locals: HashMap::new(),
            items,
            classes,
            interfaces,
            class_fields: HashMap::new(),
            current_class: None,
            current_async: false,
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
            items.insert(name.to_owned(), item_id);
            match item {
                Item::Class(_) => {
                    classes.insert(name.to_owned(), item_id);
                }
                Item::Interface(_) => {
                    interfaces.insert(name.to_owned(), item_id);
                }
                Item::Function(_) | Item::TypeAlias(_) | Item::Const(_) => {}
            }
        }
        (items, classes, interfaces)
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

        for statement in &program.body {
            if let Statement::ImportDeclaration(import) = statement {
                self.import_declaration(import, &mut module);
                continue;
            }
            if let Statement::FunctionDeclaration(function) = statement {
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
            if let Statement::ExportNamedDeclaration(export) = statement
                && let Some(decl) = &export.declaration
            {
                if let Declaration::FunctionDeclaration(function) = decl {
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
                }
            }
        }

        for statement in &program.body {
            if matches!(
                statement,
                Statement::FunctionDeclaration(_)
                    | Statement::ClassDeclaration(_)
                    | Statement::TSInterfaceDeclaration(_)
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
                    (imported, local)
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier_data) => (
                    "default".to_owned(),
                    specifier_data.local.name.as_str().to_owned(),
                ),
                ImportDeclarationSpecifier::ImportNamespaceSpecifier(specifier_data) => (
                    "*".to_owned(),
                    specifier_data.local.name.as_str().to_owned(),
                ),
            };
            let name = self.intern_source_name(&imported);
            let alias = (local != imported).then(|| self.intern_source_name(&local));
            module.imports.push(Import {
                module: source.to_owned(),
                name,
                alias,
                span,
            });
            self.alias_imported_item(&imported, &local);
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
    }

    /// Lower a function declaration to HIR.
    fn function_declaration(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let id = function.id.as_ref().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "anonymous function declarations are not lowered yet",
            )
        })?;
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "declare functions are not lowered yet",
            ));
        };
        let name_text = id.name.as_str();
        let name = self.intern_source_name(name_text);
        let return_ty = function
            .return_type
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(function.span.start, function.span.end),
                    "function declarations must have an explicit return type",
                )
            })?;
        if function.r#async && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
        {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "async functions must declare a Promise<T> return type",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_async = self.current_async;
        self.current_async = function.r#async;
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut params = Vec::new();

        for param in &function.params.items {
            let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                self.locals = saved_locals;
                self.current_async = saved_async;
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "destructured parameters are not lowered yet",
                ));
            };
            let ty = param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "function parameters must have explicit type annotations",
                    )
                })?;
            let param_name = self.intern_source_name(binding.name.as_str());
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span: self.span(binding.span.start, binding.span.end),
            });
            body.params.push(local);
            self.locals.insert(binding.name.to_string(), local);
            params.push(Param {
                name: param_name,
                local,
                ty,
                span: self.span(binding.span.start, binding.span.end),
            });
        }

        let mut errors = Vec::new();
        for statement in &function_body.statements {
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }
        if function.r#async {
            body.build_async_state_machine();
        }
        self.locals = saved_locals;
        self.current_async = saved_async;

        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let body_id = self.ctx.krate.push_body(body);
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name,
            span: self.span(function.span.start, function.span.end),
            params,
            return_ty,
            is_async: function.r#async,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        }));
        self.items.insert(name_text.to_owned(), item);
        Ok(item)
    }

    /// Lower a class declaration to HIR.
    fn class_declaration(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let id = class.id.as_ref().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "anonymous classes are not lowered yet",
            )
        })?;
        if !class.decorators.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "decorators are not lowered yet",
            ));
        }
        if class.r#abstract {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "abstract classes are not lowered yet",
            ));
        }
        if class.type_parameters.is_some() {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "generic classes are not lowered yet",
            ));
        }
        if class.super_class.is_some() {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "class extends is not lowered yet",
            ));
        }

        let class_text = id.name.as_str();
        let class_name = self.intern_type_name(class_text);
        let class_ty = self.ctx.krate.types.intern(Type::Class {
            name: class_name,
            args: Vec::new(),
        });
        let mut fields = Vec::new();
        let mut constructor = None;
        let mut methods = Vec::new();

        for element in &class.body.body {
            if let ClassElement::PropertyDefinition(property) = element {
                if property.computed && !is_static_property_key(&property.key) {
                    return Err(SmeltError::unsupported(
                        self.span(property.span.start, property.span.end),
                        "dynamic computed property names are not lowered yet",
                    ));
                }
                if property.r#static {
                    return Err(SmeltError::unsupported(
                        self.span(property.span.start, property.span.end),
                        "static fields are not lowered yet",
                    ));
                }
                if property.value.is_some() {
                    return Err(SmeltError::unsupported(
                        self.span(property.span.start, property.span.end),
                        "field initializers are not lowered yet",
                    ));
                }
                if property.optional {
                    return Err(SmeltError::unsupported(
                        self.span(property.span.start, property.span.end),
                        "optional class fields are not lowered yet",
                    ));
                }
                let name = self.property_key_symbol(&property.key)?;
                let ty = property
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "class fields require explicit type annotations",
                        )
                    })?;
                fields.push(Field {
                    name,
                    ty,
                    visibility: visibility(property.accessibility),
                    optional: false,
                    span: self.span(property.span.start, property.span.end),
                });
            }
        }
        self.class_fields
            .insert(class_text.to_owned(), fields.clone());

        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(_) => {}
                ClassElement::MethodDefinition(method) => {
                    if !method.decorators.is_empty() {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "method decorators are not lowered yet",
                        ));
                    }
                    if method.computed && !is_static_property_key(&method.key) {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "dynamic computed method names are not lowered yet",
                        ));
                    }
                    if method.r#static {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "static methods are not lowered yet",
                        ));
                    }
                    if !matches!(
                        method.kind,
                        MethodDefinitionKind::Constructor | MethodDefinitionKind::Method
                    ) {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "getters and setters are not lowered yet",
                        ));
                    }
                    let item = if method.kind == MethodDefinitionKind::Constructor {
                        if constructor.is_some() {
                            return Err(SmeltError::unsupported(
                                self.span(method.span.start, method.span.end),
                                "duplicate constructors are not allowed",
                            ));
                        }
                        let item =
                            self.class_function(class_text, class_name, class_ty, method, true)?;
                        constructor = Some(item);
                        item
                    } else {
                        let item =
                            self.class_function(class_text, class_name, class_ty, method, false)?;
                        methods.push(item);
                        item
                    };
                    let _ = item;
                }
                ClassElement::AccessorProperty(accessor) => {
                    return Err(SmeltError::unsupported(
                        self.span(accessor.span.start, accessor.span.end),
                        "accessor properties are not lowered yet",
                    ));
                }
                ClassElement::StaticBlock(block) => {
                    return Err(SmeltError::unsupported(
                        self.span(block.span.start, block.span.end),
                        "static blocks are not lowered yet",
                    ));
                }
                ClassElement::TSIndexSignature(sig) => {
                    return Err(SmeltError::unsupported(
                        self.span(sig.span.start, sig.span.end),
                        "class index signatures are not lowered yet",
                    ));
                }
            }
        }

        if !fields.is_empty() && constructor.is_none() {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "classes with required fields must declare a constructor",
            ));
        }

        let implements = class
            .implements
            .iter()
            .map(|imp| self.implements_symbol(imp))
            .collect::<Result<Vec<_>, _>>()?;
        let item = self.ctx.krate.push_item(Item::Class(Class {
            name: class_name,
            span: self.span(class.span.start, class.span.end),
            kind: smelt_hir::ClassKind::Plain,
            base: None,
            fields,
            constructor,
            methods,
            implements,
        }));
        self.classes.insert(class_text.to_owned(), item);
        self.validate_implements(item)?;
        Ok(item)
    }

    /// Lower a class method or constructor to HIR.
    fn class_function(
        &mut self,
        class_text: &str,
        class_name: smelt_hir::Symbol,
        class_ty: smelt_hir::TypeId,
        method: &oxc::ast::ast::MethodDefinition<'_>,
        is_constructor: bool,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let Some(function_body) = &method.value.body else {
            return Err(SmeltError::unsupported(
                self.span(method.span.start, method.span.end),
                "declare methods are not lowered yet",
            ));
        };
        let method_name = if is_constructor {
            self.ctx.krate.symbols.intern("new")
        } else {
            self.property_key_symbol(&method.key)?
        };
        let return_ty = if is_constructor {
            if method.value.return_type.is_some() {
                return Err(SmeltError::unsupported(
                    self.span(method.span.start, method.span.end),
                    "constructors cannot declare return types",
                ));
            }
            class_ty
        } else {
            method
                .value
                .return_type
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(method.span.start, method.span.end),
                        "methods require explicit return types",
                    )
                })?
        };
        if method.value.r#async
            && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
        {
            return Err(SmeltError::unsupported(
                self.span(method.span.start, method.span.end),
                "async methods must declare a Promise<T> return type",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_class = self.current_class.replace(class_text.to_owned());
        let saved_async = self.current_async;
        self.current_async = method.value.r#async;
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut params = Vec::new();
        let this_symbol = self.ctx.krate.symbols.intern("this");
        let this_local = body.push_local(LocalDecl {
            name: Some(this_symbol),
            ty: class_ty,
            mutable: true,
            span: self.span(method.span.start, method.span.start),
        });
        self.locals.insert("this".to_owned(), this_local);
        if !is_constructor {
            body.params.push(this_local);
            params.push(Param {
                name: this_symbol,
                local: this_local,
                ty: class_ty,
                span: self.span(method.span.start, method.span.start),
            });
        }

        for param in &method.value.params.items {
            let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                self.locals = saved_locals;
                self.current_class = saved_class;
                self.current_async = saved_async;
                return Err(SmeltError::unsupported(
                    self.span(param.span.start, param.span.end),
                    "destructured parameters are not lowered yet",
                ));
            };
            let ty = param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "method parameters must have explicit type annotations",
                    )
                })?;
            let param_name = self.intern_source_name(binding.name.as_str());
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span: self.span(binding.span.start, binding.span.end),
            });
            body.params.push(local);
            self.locals.insert(binding.name.to_string(), local);
            params.push(Param {
                name: param_name,
                local,
                ty,
                span: self.span(binding.span.start, binding.span.end),
            });
        }

        let mut errors = Vec::new();
        for statement in &function_body.statements {
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }
        if method.value.r#async {
            body.build_async_state_machine();
        }
        self.locals = saved_locals;
        self.current_class = saved_class;
        self.current_async = saved_async;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        let body_id = self.ctx.krate.push_body(body);
        Ok(self.ctx.krate.push_item(Item::Function(Function {
            name: method_name,
            span: self.span(method.span.start, method.span.end),
            params,
            return_ty,
            is_async: method.value.r#async,
            body: Some(body_id),
            owner: if is_constructor {
                FunctionOwner::Constructor { class: class_name }
            } else {
                FunctionOwner::ClassMethod {
                    class: class_name,
                    method: method_name,
                }
            },
        })))
    }

    /// Lower a statement in the current scope.
    fn statement(&mut self, statement: &Statement<'_>, body: &mut Body) -> Result<(), SmeltError> {
        self.statement_in_block(statement, body, body.root)
    }

    /// Lower an interface declaration to HIR.
    fn interface_declaration(
        &mut self,
        interface: &oxc::ast::ast::TSInterfaceDeclaration<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        if interface.type_parameters.is_some() {
            return Err(SmeltError::unsupported(
                self.span(interface.span.start, interface.span.end),
                "generic interfaces are not lowered yet",
            ));
        }
        let name_text = interface.id.name.as_str();
        let name = self.intern_type_name(name_text);
        let mut fields = Vec::new();
        let mut methods = Vec::new();

        for heritage in &interface.extends {
            let parent_name = self.interface_heritage_symbol(heritage)?;
            let parent = self.find_interface(parent_name).ok_or_else(|| {
                let parent_name_text = self
                    .ctx
                    .krate
                    .symbols
                    .get(parent_name)
                    .unwrap_or("<unknown>");
                SmeltError::unsupported(
                    self.span(heritage.span.start, heritage.span.end),
                    format!("extended interface `{parent_name_text}` is not declared"),
                )
            })?;
            fields.extend(parent.fields.clone());
            methods.extend(parent.methods.clone());
        }

        for sig in &interface.body.body {
            match sig {
                TSSignature::TSPropertySignature(prop) => {
                    if prop.computed && !is_static_property_key(&prop.key) {
                        return Err(SmeltError::unsupported(
                            self.span(prop.span.start, prop.span.end),
                            "dynamic computed interface property names are not lowered yet",
                        ));
                    }
                    let ty = prop
                        .type_annotation
                        .as_ref()
                        .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                        .transpose()?
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(prop.span.start, prop.span.end),
                                "interface fields require explicit type annotations",
                            )
                        })?;
                    let field_ty = if prop.optional {
                        self.ctx.krate.types.intern(Type::Optional(ty))
                    } else {
                        ty
                    };
                    fields.push(Field {
                        name: self.property_key_symbol(&prop.key)?,
                        ty: field_ty,
                        visibility: Visibility::Public,
                        optional: prop.optional,
                        span: self.span(prop.span.start, prop.span.end),
                    });
                }
                TSSignature::TSMethodSignature(method) => {
                    if (method.computed && !is_static_property_key(&method.key))
                        || method.optional
                        || method.type_parameters.is_some()
                        || method.this_param.is_some()
                    {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "generic, optional, dynamic computed, and this-parameter interface methods are not lowered yet",
                        ));
                    }
                    let return_ty = method
                        .return_type
                        .as_ref()
                        .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                        .transpose()?
                        .ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(method.span.start, method.span.end),
                                "interface methods require explicit return types",
                            )
                        })?;
                    let mut params = Vec::new();
                    for param in &method.params.items {
                        let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                            return Err(SmeltError::unsupported(
                                self.span(param.span.start, param.span.end),
                                "destructured interface method parameters are not lowered yet",
                            ));
                        };
                        let ty = param
                            .type_annotation
                            .as_ref()
                            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                            .transpose()?
                            .ok_or_else(|| {
                                SmeltError::unsupported(
                                    self.span(param.span.start, param.span.end),
                                    "interface method parameters require explicit types",
                                )
                            })?;
                        params.push(ParamSig {
                            name: self.intern_source_name(binding.name.as_str()),
                            ty,
                            span: self.span(binding.span.start, binding.span.end),
                        });
                    }
                    methods.push(MethodSig {
                        name: self.property_key_symbol(&method.key)?,
                        params,
                        return_ty,
                        visibility: Visibility::Public,
                        is_async: matches!(
                            self.ctx.krate.types.get(return_ty),
                            Some(Type::Future(_))
                        ),
                        span: self.span(method.span.start, method.span.end),
                    });
                }
                TSSignature::TSCallSignatureDeclaration(_)
                | TSSignature::TSConstructSignatureDeclaration(_)
                | TSSignature::TSIndexSignature(_) => {
                    return Err(SmeltError::unsupported(
                        self.span(sig.span().start, sig.span().end),
                        "interface call, construct, and index signatures are not lowered yet",
                    ));
                }
            }
        }
        let item = self.ctx.krate.push_item(Item::Interface(Interface {
            name,
            span: self.span(interface.span.start, interface.span.end),
            fields,
            methods,
        }));
        self.interfaces.insert(name_text.to_owned(), item);
        Ok(item)
    }

    /// Lower a statement within a specific block.
    fn statement_in_block(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        match statement {
            Statement::VariableDeclaration(decl) => self.variable_declaration(decl, body, block),
            Statement::ExpressionStatement(expr_stmt) => {
                if let Expression::AssignmentExpression(assign) = &expr_stmt.expression {
                    let (target, value) = self.assignment_parts(assign, body)?;
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                    return Ok(());
                }
                if let Expression::UpdateExpression(update) = &expr_stmt.expression {
                    let (target, value) = self.update_parts(update, body)?;
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                    return Ok(());
                }
                let expr = self.expression(&expr_stmt.expression, body)?;
                body.push_stmt_to_block(block, Stmt::Expr(expr));
                Ok(())
            }
            Statement::ReturnStatement(return_stmt) => {
                let value = return_stmt
                    .argument
                    .as_ref()
                    .map(|argument| self.expression(argument, body))
                    .transpose()?;
                body.push_stmt_to_block(block, Stmt::Return(value));
                Ok(())
            }
            Statement::IfStatement(if_stmt) => {
                let cond = self.expression(&if_stmt.test, body)?;
                let then_block = self.block_from_statement(&if_stmt.consequent, body)?;
                let else_block = if_stmt
                    .alternate
                    .as_ref()
                    .map(|alternate| self.block_from_statement(alternate, body))
                    .transpose()?;
                body.push_stmt_to_block(
                    block,
                    Stmt::If {
                        cond,
                        then_block,
                        else_block,
                    },
                );
                Ok(())
            }
            Statement::WhileStatement(while_stmt) => {
                let cond = self.expression(&while_stmt.test, body)?;
                let loop_body = self.block_from_statement(&while_stmt.body, body)?;
                body.push_stmt_to_block(
                    block,
                    Stmt::While {
                        cond,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::ForOfStatement(for_stmt) => {
                if for_stmt.r#await {
                    return Err(SmeltError::unsupported(
                        self.span(for_stmt.span.start, for_stmt.span.end),
                        "for await...of is async control flow and is not lowered yet",
                    ));
                }
                let iter = self.expression(&for_stmt.right, body)?;
                let pat = self.for_left_pattern(&for_stmt.left, body)?;
                let loop_body = self.block_from_statement(&for_stmt.body, body)?;
                body.push_stmt_to_block(
                    block,
                    Stmt::For {
                        pat,
                        iter,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::ForStatement(for_stmt) => self.c_for_statement(for_stmt, body, block),
            Statement::SwitchStatement(switch_stmt) => {
                let scrutinee = self.expression(&switch_stmt.discriminant, body)?;
                let mut arms = Vec::new();
                let mut default = None;

                for case in &switch_stmt.cases {
                    let case_block = body.push_block(self.span(case.span.start, case.span.end));
                    let mut saw_break = false;
                    for case_statement in &case.consequent {
                        if matches!(case_statement, Statement::ContinueStatement(_)) {
                            return Err(SmeltError::unsupported(
                                self.statement_span(case_statement),
                                "switch continue lowering is not implemented yet",
                            ));
                        }
                        if matches!(case_statement, Statement::BreakStatement(_)) {
                            saw_break = true;
                            break;
                        }
                        self.statement_in_block(case_statement, body, case_block)?;
                    }
                    if !saw_break && !case.consequent.iter().any(statement_terminates) {
                        return Err(SmeltError::unsupported(
                            self.span(case.span.start, case.span.end),
                            "switch fallthrough is not lowered yet; each case must break, return, or throw",
                        ));
                    }

                    if let Some(test) = &case.test {
                        arms.push(MatchArm {
                            label: self.literal_case_label(test)?,
                            body: case_block,
                        });
                    } else if default.replace(case_block).is_some() {
                        return Err(SmeltError::unsupported(
                            self.span(case.span.start, case.span.end),
                            "switch statements can only have one default case",
                        ));
                    }
                }

                body.push_stmt_to_block(
                    block,
                    Stmt::Match {
                        scrutinee,
                        arms,
                        default,
                    },
                );
                Ok(())
            }
            Statement::BreakStatement(break_stmt) => {
                if break_stmt.label.is_some() {
                    return Err(SmeltError::unsupported(
                        self.span(break_stmt.span.start, break_stmt.span.end),
                        "labeled break is not lowered yet",
                    ));
                }
                body.push_stmt_to_block(block, Stmt::Break);
                Ok(())
            }
            Statement::ContinueStatement(continue_stmt) => {
                if continue_stmt.label.is_some() {
                    return Err(SmeltError::unsupported(
                        self.span(continue_stmt.span.start, continue_stmt.span.end),
                        "labeled continue is not lowered yet",
                    ));
                }
                body.push_stmt_to_block(block, Stmt::Continue);
                Ok(())
            }
            Statement::ThrowStatement(throw_stmt) => {
                let expr = self.expression(&throw_stmt.argument, body)?;
                body.push_stmt_to_block(block, Stmt::Throw(expr));
                Ok(())
            }
            Statement::TryStatement(try_stmt) => {
                let try_body = self.block_from_block_statement(&try_stmt.block, body)?;
                let (catch_binding, catch_body) = if let Some(handler) = &try_stmt.handler {
                    let previous_locals = self.locals.clone();
                    let catch_binding = handler
                        .param
                        .as_ref()
                        .map(|param| self.catch_binding(param, body))
                        .transpose()?;
                    let catch_body = self.block_from_block_statement(&handler.body, body)?;
                    self.locals = previous_locals;
                    (catch_binding, Some(catch_body))
                } else {
                    (None, None)
                };
                let finally_body = try_stmt
                    .finalizer
                    .as_ref()
                    .map(|finalizer| self.block_from_block_statement(finalizer, body))
                    .transpose()?;

                body.push_stmt_to_block(
                    block,
                    Stmt::TryCatch {
                        body: try_body,
                        catch_binding,
                        catch_body,
                        finally_body,
                    },
                );
                Ok(())
            }
            Statement::BlockStatement(block_stmt) => {
                for child in &block_stmt.body {
                    self.statement_in_block(child, body, block)?;
                }
                Ok(())
            }
            _ => Err(SmeltError::unsupported(
                self.statement_span(statement),
                format!("statement kind is not lowered yet: {statement:?}"),
            )),
        }
    }

    /// Create a block from a statement (wrapping if needed).
    fn block_from_statement(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::BlockId, SmeltError> {
        let span = self.statement_span(statement);
        let block = body.push_block(span);
        if let Statement::BlockStatement(block_stmt) = statement {
            for nested_statement in &block_stmt.body {
                self.statement_in_block(nested_statement, body, block)?;
            }
        } else {
            self.statement_in_block(statement, body, block)?;
        }
        Ok(block)
    }

    /// Create a HIR block from a JavaScript block statement.
    fn block_from_block_statement(
        &mut self,
        block_stmt: &oxc::ast::ast::BlockStatement<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::BlockId, SmeltError> {
        let block = body.push_block(self.span(block_stmt.span.start, block_stmt.span.end));
        for statement in &block_stmt.body {
            self.statement_in_block(statement, body, block)?;
        }
        Ok(block)
    }

    /// Lower a catch parameter to an optional HIR local binding.
    fn catch_binding(
        &mut self,
        param: &oxc::ast::ast::CatchParameter<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::LocalId, SmeltError> {
        let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
            return Err(SmeltError::unsupported(
                self.span(param.span.start, param.span.end),
                "destructured catch bindings are not lowered yet",
            ));
        };
        let ty = param
            .type_annotation
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::String));
        let name = binding.name.as_str();
        let symbol = self.intern_source_name(name);
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span: self.span(binding.span.start, binding.span.end),
        });
        self.locals.insert(name.to_owned(), local);
        Ok(local)
    }

    /// Lower a variable declaration statement.
    fn variable_declaration(
        &mut self,
        decl: &oxc::ast::ast::VariableDeclaration<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        for declarator in &decl.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                return Err(SmeltError::unsupported(
                    self.span(declarator.span.start, declarator.span.end),
                    "destructuring declarations are not lowered yet",
                ));
            };

            let annotated_ty = declarator
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            let value = declarator
                .init
                .as_ref()
                .map(|init| self.expression_with_hint(init, body, annotated_ty))
                .transpose()?;
            let ty = annotated_ty
                .or_else(|| value.map(|expr_id| Self::expr_ty(body, expr_id)))
                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::None));
            let name = binding.name.as_str();
            let symbol = self.ctx.krate.symbols.intern(&camel_to_snake(name));
            self.ctx.krate.names.record(symbol, name);
            let local = body.push_local(LocalDecl {
                name: Some(symbol),
                ty,
                mutable: matches!(declarator.kind, oxc::ast::ast::VariableDeclarationKind::Let),
                span: self.span(binding.span.start, binding.span.end),
            });
            self.locals.insert(name.to_owned(), local);
            let pat = body.push_pattern(Pattern::Binding(local));
            body.push_stmt_to_block(block, Stmt::Let { pat, ty, value });
        }
        Ok(())
    }

    /// Lower a C-style for loop.
    fn c_for_statement(
        &mut self,
        for_stmt: &oxc::ast::ast::ForStatement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        if let Some(init) = &for_stmt.init {
            match init {
                ForStatementInit::VariableDeclaration(decl) => {
                    self.variable_declaration(decl, body, block)?;
                }
                ForStatementInit::AssignmentExpression(assign) => {
                    let (target, value) = self.assignment_parts(assign, body)?;
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                }
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(init.span().start, init.span().end),
                        "for-loop init must be a variable declaration or assignment",
                    ));
                }
            }
        }

        let cond = if let Some(test) = &for_stmt.test {
            self.expression(test, body)?
        } else {
            let ty = self.ctx.krate.types.intern(Type::Bool);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty,
                span: self.span(for_stmt.span.start, for_stmt.span.end),
            })
        };
        let loop_body = self.block_from_statement(&for_stmt.body, body)?;
        if let Some(update) = &for_stmt.update {
            let (target, value) = match update {
                Expression::AssignmentExpression(assign) => self.assignment_parts(assign, body)?,
                Expression::UpdateExpression(update_expr) => {
                    self.update_parts(update_expr, body)?
                }
                _ => {
                    return Err(SmeltError::unsupported(
                        self.expression_span(update),
                        "for-loop update must be assignment or increment/decrement",
                    ));
                }
            };
            body.push_stmt_to_block(loop_body, Stmt::Assign { target, value });
        }
        body.push_stmt_to_block(
            block,
            Stmt::While {
                cond,
                body: loop_body,
            },
        );
        Ok(())
    }

    /// Extract pattern from for-of left side.
    fn for_left_pattern(
        &mut self,
        left: &ForStatementLeft<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::PatternId, SmeltError> {
        let ForStatementLeft::VariableDeclaration(decl) = left else {
            return Err(SmeltError::unsupported(
                self.span(left.span().start, left.span().end),
                "for...of targets must be variable declarations for now",
            ));
        };
        if decl.declarations.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(decl.span.start, decl.span.end),
                "for...of currently supports exactly one loop binding",
            ));
        }
        let Some(declarator) = decl.declarations.first() else {
            return Err(SmeltError::unsupported(
                self.span(decl.span.start, decl.span.end),
                "for...of currently supports exactly one loop binding",
            ));
        };
        let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
            return Err(SmeltError::unsupported(
                self.span(declarator.span.start, declarator.span.end),
                "destructured for...of bindings are not lowered yet",
            ));
        };
        let ty = declarator
            .type_annotation
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(declarator.span.start, declarator.span.end),
                    "for...of bindings must have explicit type annotations",
                )
            })?;
        let name = binding.name.as_str();
        let symbol = self.intern_source_name(name);
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: true,
            span: self.span(binding.span.start, binding.span.end),
        });
        self.locals.insert(name.to_owned(), local);
        Ok(body.push_pattern(Pattern::Binding(local)))
    }

    /// Convert a switch case label expression to a literal.
    fn literal_case_label(&self, expression: &Expression<'_>) -> Result<Literal, SmeltError> {
        match expression {
            Expression::StringLiteral(lit) => Ok(Literal::String(lit.value.to_string())),
            Expression::NumericLiteral(lit) => Ok(Literal::Float(lit.value)),
            Expression::BooleanLiteral(lit) => Ok(Literal::Bool(lit.value)),
            Expression::NullLiteral(_) => Ok(Literal::None),
            _ => Err(SmeltError::unsupported(
                self.expression_span(expression),
                "switch case labels must be string, number, boolean, or null literals",
            )),
        }
    }

    /// Lower an expression without type hint.
    fn expression(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.expression_with_hint(expression, body, None)
    }

    /// Lower an expression with optional type hint.
    fn expression_with_hint(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match expression {
            Expression::NumericLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Float);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::StringLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(lit.value.to_string())),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::BooleanLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::NullLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Expression::Identifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            Expression::ThisExpression(this_expr) => {
                self.identifier_expression("this", this_expr.span.start, this_expr.span.end, body)
            }
            Expression::ArrayExpression(array) => {
                let mut items = Vec::new();
                for element in &array.elements {
                    let expr = match element {
                        ArrayExpressionElement::SpreadElement(_) => {
                            return Err(SmeltError::unsupported(
                                self.span(element.span().start, element.span().end),
                                "array spread elements are not lowered yet",
                            ));
                        }
                        ArrayExpressionElement::Elision(_) => {
                            return Err(SmeltError::unsupported(
                                self.span(element.span().start, element.span().end),
                                "array elisions are not lowered",
                            ));
                        }
                        _ => self.array_element(element, body)?,
                    };
                    items.push(expr);
                }
                let ty = if let Some(hint) = type_hint {
                    hint
                } else if let Some(first) = items.first() {
                    let item_ty = Self::expr_ty(body, *first);
                    self.ctx.krate.types.intern(Type::List(item_ty))
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "empty arrays require an explicit type annotation",
                    ));
                };
                Ok(body.push_expr(Expr {
                    kind: if matches!(self.ctx.krate.types.get(ty), Some(Type::Tuple(_))) {
                        ExprKind::TupleLit(items)
                    } else {
                        ExprKind::ListLit(items)
                    },
                    ty,
                    span: self.span(array.span.start, array.span.end),
                }))
            }
            Expression::ObjectExpression(object) => {
                let Some(ty) = type_hint else {
                    return Err(SmeltError::unsupported(
                        self.span(object.span.start, object.span.end),
                        "object literals require a Record<string, T> annotation",
                    ));
                };
                if !matches!(self.ctx.krate.types.get(ty), Some(Type::Dict(_, _))) {
                    return Err(SmeltError::unsupported(
                        self.span(object.span.start, object.span.end),
                        "object literals currently require a Record<string, T> annotation",
                    ));
                }
                let mut entries = Vec::new();
                for property in &object.properties {
                    let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                        return Err(SmeltError::unsupported(
                            self.span(property.span().start, property.span().end),
                            "object spread properties are not lowered yet",
                        ));
                    };
                    if object_property.computed || object_property.method {
                        return Err(SmeltError::unsupported(
                            self.span(object_property.span.start, object_property.span.end),
                            "computed object keys and object methods are not lowered yet",
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
                                "object literal keys must be static string keys",
                            ));
                        }
                    };
                    let key_ty = self.ctx.krate.types.intern(Type::String);
                    let key = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::String(key_text)),
                        ty: key_ty,
                        span: self.span(
                            object_property.key.span().start,
                            object_property.key.span().end,
                        ),
                    });
                    let value = self.expression(&object_property.value, body)?;
                    entries.push((key, value));
                }
                Ok(body.push_expr(Expr {
                    kind: ExprKind::DictLit(entries),
                    ty,
                    span: self.span(object.span.start, object.span.end),
                }))
            }
            Expression::BinaryExpression(binary) => {
                let op = match binary.operator {
                    BinaryOperator::Addition => BinOp::Add,
                    BinaryOperator::Subtraction => BinOp::Sub,
                    BinaryOperator::Multiplication => BinOp::Mul,
                    BinaryOperator::Division => BinOp::Div,
                    BinaryOperator::StrictEquality => BinOp::Eq,
                    BinaryOperator::StrictInequality => BinOp::NotEq,
                    BinaryOperator::Equality | BinaryOperator::Inequality => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.span.start, binary.span.end),
                            "coercive equality is not lowered; use === or !==",
                        ));
                    }
                    BinaryOperator::LessThan => BinOp::Lt,
                    BinaryOperator::LessEqualThan => BinOp::Lte,
                    BinaryOperator::GreaterThan => BinOp::Gt,
                    BinaryOperator::GreaterEqualThan => BinOp::Gte,
                    BinaryOperator::Remainder
                    | BinaryOperator::Exponential
                    | BinaryOperator::ShiftLeft
                    | BinaryOperator::ShiftRight
                    | BinaryOperator::ShiftRightZeroFill
                    | BinaryOperator::BitwiseOR
                    | BinaryOperator::BitwiseXOR
                    | BinaryOperator::BitwiseAnd
                    | BinaryOperator::In
                    | BinaryOperator::Instanceof => {
                        return Err(SmeltError::unsupported(
                            self.span(binary.span.start, binary.span.end),
                            format!("binary operator is not lowered yet: {:?}", binary.operator),
                        ));
                    }
                };
                let lhs = self.expression(&binary.left, body)?;
                let rhs = self.expression(&binary.right, body)?;
                let ty = if matches!(
                    op,
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte
                ) {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    Self::expr_ty(body, lhs)
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::BinOp { op, lhs, rhs },
                    ty,
                    span: self.span(binary.span.start, binary.span.end),
                }))
            }
            Expression::LogicalExpression(logical) => {
                let op = match logical.operator {
                    LogicalOperator::And => BinOp::And,
                    LogicalOperator::Or => BinOp::Or,
                    LogicalOperator::Coalesce => {
                        return Err(SmeltError::unsupported(
                            self.span(logical.span.start, logical.span.end),
                            "nullish coalescing is not lowered yet",
                        ));
                    }
                };
                let lhs = self.expression(&logical.left, body)?;
                let rhs = self.expression(&logical.right, body)?;
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::BinOp { op, lhs, rhs },
                    ty,
                    span: self.span(logical.span.start, logical.span.end),
                }))
            }
            Expression::UnaryExpression(unary) => {
                let op = match unary.operator {
                    UnaryOperator::LogicalNot => UnaryOp::Not,
                    UnaryOperator::UnaryNegation => UnaryOp::Neg,
                    _ => {
                        return Err(SmeltError::unsupported(
                            self.span(unary.span.start, unary.span.end),
                            format!("unary operator is not lowered yet: {:?}", unary.operator),
                        ));
                    }
                };
                let operand = self.expression(&unary.argument, body)?;
                let ty = if matches!(op, UnaryOp::Not) {
                    self.ctx.krate.types.intern(Type::Bool)
                } else {
                    Self::expr_ty(body, operand)
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::UnaryOp { op, operand },
                    ty,
                    span: self.span(unary.span.start, unary.span.end),
                }))
            }
            Expression::AwaitExpression(await_expr) => {
                if !self.current_async {
                    return Err(SmeltError::unsupported(
                        self.span(await_expr.span.start, await_expr.span.end),
                        "await expressions are only lowered inside async functions",
                    ));
                }
                let awaited = self.expression(&await_expr.argument, body)?;
                let ty = self
                    .future_inner_type(Self::expr_ty(body, awaited))
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(await_expr.span.start, await_expr.span.end),
                            "await expressions require a Promise<T> operand",
                        )
                    })?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Await(awaited),
                    ty,
                    span: self.span(await_expr.span.start, await_expr.span.end),
                }))
            }
            Expression::StaticMemberExpression(member) => {
                if member.optional {
                    return Err(SmeltError::unsupported(
                        self.span(member.span.start, member.span.end),
                        "optional member access is not lowered yet",
                    ));
                }
                let receiver = self.expression(&member.object, body)?;
                let field = self.intern_source_name(member.property.name.as_str());
                if member.property.name == "length"
                    && self.supports_stdlib_length(Self::expr_ty(body, receiver))
                {
                    let ty = self.ctx.krate.types.intern(Type::Float);
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Len { operand: receiver },
                        ty,
                        span: self.span(member.span.start, member.span.end),
                    }));
                }
                let ty = self.class_field_type(Self::expr_ty(body, receiver), field)?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Field { receiver, field },
                    ty,
                    span: self.span(member.span.start, member.span.end),
                }))
            }
            Expression::ComputedMemberExpression(member) => {
                if member.optional {
                    return Err(SmeltError::unsupported(
                        self.span(member.span.start, member.span.end),
                        "optional index access is not lowered yet",
                    ));
                }
                let receiver = self.expression(&member.object, body)?;
                let index = self.expression(&member.expression, body)?;
                let ty = self.index_type(Self::expr_ty(body, receiver))?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Index { receiver, index },
                    ty,
                    span: self.span(member.span.start, member.span.end),
                }))
            }
            Expression::CallExpression(call) => {
                if let Some(expr) = self.promise_static_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.timer_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.fetch_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.math_abs_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.math_round_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.math_extrema_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.math_hypot_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.number_predicate_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.math_unary_func_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.math_pow_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.object_projection_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.array_is_array_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.string_case_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.string_trim_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.string_affix_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.list_search_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.string_search_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.string_replace_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.string_repeat_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.string_char_at_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.string_join_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.list_concat_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.list_contains_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.string_contains_call(call, body)? {
                    return Ok(expr);
                }
                if let Some(expr) = self.string_split_call(call, body)? {
                    return Ok(expr);
                }
                if let Expression::StaticMemberExpression(member) = &call.callee
                    && let Expression::Identifier(object) = &member.object
                    && object.name == "console"
                    && member.property.name == "log"
                {
                    let mut args = Vec::new();
                    for arg in &call.arguments {
                        args.push(self.argument(arg, body)?);
                    }
                    let ty = self.ctx.krate.types.intern(Type::None);
                    let callee_item =
                        self.ensure_console_log_item(self.span(member.span.start, member.span.end));
                    let callee = body.push_expr(Expr {
                        kind: ExprKind::Item(callee_item),
                        ty,
                        span: self.span(member.span.start, member.span.end),
                    });
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Call { callee, args },
                        ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                }
                if let Expression::StaticMemberExpression(member) = &call.callee {
                    let receiver = self.expression(&member.object, body)?;
                    let method = self.intern_source_name(member.property.name.as_str());
                    let (return_ty, _) =
                        self.resolve_method(Self::expr_ty(body, receiver), method)?;
                    let mut args = Vec::new();
                    for arg in &call.arguments {
                        args.push(self.argument(arg, body)?);
                    }
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Method {
                            receiver,
                            method,
                            args,
                        },
                        ty: return_ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                }
                if let Expression::Identifier(callee_ident) = &call.callee {
                    let Some(item) = self.items.get(callee_ident.name.as_str()).copied() else {
                        return Err(SmeltError::unsupported(
                            self.span(callee_ident.span.start, callee_ident.span.end),
                            format!("unresolved function `{}`", callee_ident.name),
                        ));
                    };
                    let (params, return_ty, is_async) =
                        if let Item::Function(function) = self.item_ref(item) {
                            (
                                function.params.iter().map(|param| param.ty).collect(),
                                function.return_ty,
                                function.is_async,
                            )
                        } else {
                            return Err(SmeltError::unsupported(
                                self.span(callee_ident.span.start, callee_ident.span.end),
                                "callee item is not a function",
                            ));
                        };
                    let mut args = Vec::new();
                    for arg in &call.arguments {
                        args.push(self.argument(arg, body)?);
                    }
                    let callee =
                        body.push_expr(Expr {
                            kind: ExprKind::Item(item),
                            ty: self.ctx.krate.types.intern(Type::Function(
                                smelt_hir::FunctionType {
                                    params,
                                    return_ty,
                                    is_async,
                                },
                            )),
                            span: self.span(callee_ident.span.start, callee_ident.span.end),
                        });
                    return Ok(body.push_expr(Expr {
                        kind: ExprKind::Call { callee, args },
                        ty: return_ty,
                        span: self.span(call.span.start, call.span.end),
                    }));
                }
                Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "call expression is not lowered yet",
                ))
            }
            Expression::NewExpression(new_expr) => {
                let Expression::Identifier(callee) = &new_expr.callee else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "new expressions require a direct class name",
                    ));
                };
                let Some(item) = self.classes.get(callee.name.as_str()).copied() else {
                    return Err(SmeltError::unsupported(
                        self.span(callee.span.start, callee.span.end),
                        format!("unresolved class `{}`", callee.name),
                    ));
                };
                let Item::Class(class) = self.item_ref(item) else {
                    return Err(SmeltError::unsupported(
                        self.span(new_expr.span.start, new_expr.span.end),
                        "new expressions require a class item",
                    ));
                };
                let class_name = class.name;
                let args = new_expr
                    .arguments
                    .iter()
                    .map(|arg| self.argument(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                let ty = self.ctx.krate.types.intern(Type::Class {
                    name: class_name,
                    args: Vec::new(),
                });
                Ok(body.push_expr(Expr {
                    kind: ExprKind::New {
                        class: class_name,
                        args,
                    },
                    ty,
                    span: self.span(new_expr.span.start, new_expr.span.end),
                }))
            }
            Expression::TemplateLiteral(tpl) => {
                let str_ty = self.ctx.krate.types.intern(Type::String);
                let span = self.span(tpl.span.start, tpl.span.end);

                // Build the first segment from the first quasi.
                let Some(first_quasi) = tpl.quasis.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(tpl.span.start, tpl.span.end),
                        "template literals must contain at least one quasi",
                    ));
                };
                let first_str = first_quasi
                    .value
                    .cooked
                    .as_ref()
                    .map_or_else(|| first_quasi.value.raw.as_str(), |c| c.as_str())
                    .to_owned();
                let mut acc = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(first_str)),
                    ty: str_ty,
                    span,
                });

                for (i, interp) in tpl.expressions.iter().enumerate() {
                    // Concatenate the interpolated expression
                    let part = self.expression(interp, body)?;
                    acc = body.push_expr(Expr {
                        kind: ExprKind::BinOp {
                            op: BinOp::Add,
                            lhs: acc,
                            rhs: part,
                        },
                        ty: str_ty,
                        span,
                    });
                    // Concatenate the next quasi string (skip empty ones to keep HIR tidy)
                    if let Some(quasi) = tpl.quasis.get(i.saturating_add(1)) {
                        let s = quasi
                            .value
                            .cooked
                            .as_ref()
                            .map_or_else(|| quasi.value.raw.as_str(), |c| c.as_str());
                        if !s.is_empty() {
                            let lit = body.push_expr(Expr {
                                kind: ExprKind::Literal(Literal::String(s.to_owned())),
                                ty: str_ty,
                                span,
                            });
                            acc = body.push_expr(Expr {
                                kind: ExprKind::BinOp {
                                    op: BinOp::Add,
                                    lhs: acc,
                                    rhs: lit,
                                },
                                ty: str_ty,
                                span,
                            });
                        }
                    }
                }
                Ok(acc)
            }
            Expression::TaggedTemplateExpression(tagged) => Err(SmeltError::unsupported(
                self.span(tagged.span.start, tagged.span.end),
                "tagged template literals are not supported",
            )),
            _ => Err(SmeltError::unsupported(
                self.expression_span(expression),
                format!("expression kind is not lowered yet: {expression:?}"),
            )),
        }
    }

    /// Lower a function call argument.
    fn argument(
        &mut self,
        argument: &Argument<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match argument {
            Argument::NumericLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Float);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::StringLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(lit.value.to_string())),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::BooleanLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::NullLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::None);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            Argument::Identifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            Argument::BinaryExpression(binary) => self.binary_expression(binary, body),
            Argument::LogicalExpression(logical) => self.logical_expression(logical, body),
            Argument::UnaryExpression(unary) => self.unary_expression(unary, body),
            Argument::ArrayExpression(array) => self.array_expression(array, body, None),
            Argument::ObjectExpression(object) => self.object_expression(object, body, None),
            Argument::CallExpression(call) => self.call_expression(call, body),
            Argument::ComputedMemberExpression(member) => self.computed_member(member, body),
            Argument::StaticMemberExpression(member) => self.static_member(member, body),
            _ => Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                format!("call argument kind is not lowered yet: {argument:?}"),
            )),
        }
    }

    /// Lower supported `Promise.*` calls into shared async runtime operations.
    fn promise_static_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Promise" {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "all" => AsyncOp::All,
            "race" => AsyncOp::Race,
            "allSettled" => AsyncOp::AllSettled,
            other => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("Promise.{other} is not lowered yet"),
                ));
            }
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Promise combinators require exactly one array argument",
            ));
        }
        let Some(first_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Promise combinators require exactly one array argument",
            ));
        };
        let Argument::ArrayExpression(array) = first_argument else {
            return Err(SmeltError::unsupported(
                self.span(first_argument.span().start, first_argument.span().end),
                "Promise combinators require an array literal argument",
            ));
        };
        let args = self.promise_array_args(array, body)?;
        let output_ty = match op {
            AsyncOp::All | AsyncOp::AllSettled => {
                let outputs = args
                    .iter()
                    .map(|arg| {
                        self.future_inner_type(Self::expr_ty(body, *arg))
                            .ok_or_else(|| {
                                SmeltError::unsupported(
                                    self.span(array.span.start, array.span.end),
                                    "Promise combinator entries must be Promise<T> values",
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.ctx.krate.types.intern(Type::Tuple(outputs))
            }
            AsyncOp::Race => {
                let Some(first) = args.first() else {
                    return Err(SmeltError::unsupported(
                        self.span(array.span.start, array.span.end),
                        "Promise.race requires at least one promise",
                    ));
                };
                self.future_inner_type(Self::expr_ty(body, *first))
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(array.span.start, array.span.end),
                            "Promise.race entries must be Promise<T> values",
                        )
                    })?
            }
            AsyncOp::Sleep | AsyncOp::CreateTask | AsyncOp::WaitFor | AsyncOp::HttpGetText => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    format!("Promise.{op:?} is not lowered yet"),
                ));
            }
        };
        let ty = self.ctx.krate.types.intern(Type::Future(output_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp { op, args },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower small TypeScript timer shims used by async fixtures.
    fn timer_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "setTimeout" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "setTimeout lowering supports the Smelt timer shim shape setTimeout(milliseconds)",
            ));
        }
        let Some(duration_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "setTimeout lowering supports the Smelt timer shim shape setTimeout(milliseconds)",
            ));
        };
        let duration = self.argument(duration_argument, body)?;
        let none_ty = self.ctx.krate.types.intern(Type::None);
        let ty = self.ctx.krate.types.intern(Type::Future(none_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op: AsyncOp::Sleep,
                args: vec![duration],
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower TypeScript `fetch(url)` into an async HTTP GET text operation.
    fn fetch_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(None);
        };
        if callee.name != "fetch" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch lowering supports fetch(url) with one string argument",
            ));
        }
        let Some(url_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch lowering supports fetch(url) with one string argument",
            ));
        };
        let url = self.argument(url_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, url)) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "fetch requires a string URL argument",
            ));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let ty = self.ctx.krate.types.intern(Type::Future(string_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::AsyncOp {
                op: AsyncOp::HttpGetText,
                args: vec![url],
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.abs(...)` calls.
    fn math_abs_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" || member.property.name != "abs" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.abs requires exactly one argument",
            ));
        }
        let Some(argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.abs requires exactly one argument",
            ));
        };
        let operand = self.argument(argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.abs requires a number argument",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericAbs { operand },
            ty: operand_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript numeric rounding calls.
    fn math_round_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "floor" => NumericRoundOp::Floor,
            "ceil" => NumericRoundOp::Ceil,
            "round" => NumericRoundOp::Round,
            "trunc" => NumericRoundOp::Trunc,
            _ => return Ok(None),
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        }
        let Some(argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        };
        let operand = self.argument(argument, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Math.{} requires a number argument", member.property.name),
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericRound { op, operand },
            ty: operand_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.max` and `Math.min` calls.
    fn math_extrema_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "max" => NumericExtremaOp::Max,
            "min" => NumericExtremaOp::Min,
            _ => return Ok(None),
        };
        let args = call
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        if args
            .iter()
            .any(|arg| self.ctx.krate.types.get(Self::expr_ty(body, *arg)) != Some(&Type::Float))
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Math.{} requires number arguments", member.property.name),
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericExtrema { op, args },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.hypot` calls.
    fn math_hypot_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" || member.property.name != "hypot" {
            return Ok(None);
        }
        let args = call
            .arguments
            .iter()
            .map(|argument| self.argument(argument, body))
            .collect::<Result<Vec<_>, _>>()?;
        if args
            .iter()
            .any(|arg| self.ctx.krate.types.get(Self::expr_ty(body, *arg)) != Some(&Type::Float))
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.hypot requires number arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericHypot { args },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Number.isFinite` and `Number.isNaN` calls.
    fn number_predicate_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Number" {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "isFinite" => NumericPredicateOp::IsFinite,
            "isNaN" => NumericPredicateOp::IsNaN,
            _ => return Ok(None),
        };
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Number.{} requires exactly one number argument",
                    member.property.name
                ),
            ));
        };
        let operand = self.argument(argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Number.{} requires a number argument", member.property.name),
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericPredicate { op, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.sqrt`, `Math.cbrt`, and `Math.sign` calls.
    fn math_unary_func_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "sqrt" => NumericUnaryFuncOp::Sqrt,
            "cbrt" => NumericUnaryFuncOp::Cbrt,
            "sign" => NumericUnaryFuncOp::Sign,
            _ => return Ok(None),
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        }
        let Some(argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Math.{} requires exactly one argument",
                    member.property.name
                ),
            ));
        };
        let operand = self.argument(argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::Float) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Math.{} requires a number argument", member.property.name),
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericUnaryFunc { op, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Math.pow` calls.
    fn math_pow_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Math" || member.property.name != "pow" {
            return Ok(None);
        }
        if call.arguments.len() != 2 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.pow requires exactly two arguments",
            ));
        }
        let [base_argument, exponent_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.pow requires exactly two arguments",
            ));
        };
        let base = self.argument(base_argument, body)?;
        let exponent = self.argument(exponent_argument, body)?;
        if [base, exponent]
            .iter()
            .any(|arg| self.ctx.krate.types.get(Self::expr_ty(body, *arg)) != Some(&Type::Float))
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Math.pow requires number arguments",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::NumericPow { base, exponent },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Object.keys`, `Object.values`, and `Object.entries` calls.
    fn object_projection_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Object" {
            return Ok(None);
        }
        let op = match member.property.name.as_str() {
            "keys" => DictProjectionOp::Keys,
            "values" => DictProjectionOp::Values,
            "entries" => DictProjectionOp::Entries,
            _ => return Ok(None),
        };
        let [dict_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!(
                    "Object.{} requires exactly one record argument",
                    member.property.name
                ),
            ));
        };
        let dict = self.argument(dict_argument, body)?;
        let dict_ty = Self::expr_ty(body, dict);
        let Some(Type::Dict(key_type, value_type)) = self.ctx.krate.types.get(dict_ty) else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("Object.{} requires a record argument", member.property.name),
            ));
        };
        let key_ty = *key_type;
        let value_ty = *value_type;
        let ty = match op {
            DictProjectionOp::Keys => self.ctx.krate.types.intern(Type::List(key_ty)),
            DictProjectionOp::Values => self.ctx.krate.types.intern(Type::List(value_ty)),
            DictProjectionOp::Entries => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![key_ty, value_ty]));
                self.ctx.krate.types.intern(Type::List(entry_ty))
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::DictProjection { op, dict },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.isArray` calls using static HIR types.
    fn array_is_array_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let Expression::Identifier(object) = &member.object else {
            return Ok(None);
        };
        if object.name != "Array" || member.property.name != "isArray" {
            return Ok(None);
        }
        let [argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "Array.isArray requires exactly one argument",
            ));
        };
        let value = self.argument(argument, body)?;
        let is_array = matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, value)),
            Some(Type::List(_))
        );
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(is_array)),
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string case methods.
    fn string_case_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "toLowerCase" => StringCaseOp::Lower,
            "toUpperCase" => StringCaseOp::Upper,
            _ => return Ok(None),
        };
        if call.arguments.is_empty() {
            let operand = self.expression(&member.object, body)?;
            let operand_ty = Self::expr_ty(body, operand);
            if self.ctx.krate.types.get(operand_ty) == Some(&Type::String) {
                let ty = self.ctx.krate.types.intern(Type::String);
                return Ok(Some(body.push_expr(Expr {
                    kind: ExprKind::StringCase { op, operand },
                    ty,
                    span: self.span(call.span.start, call.span.end),
                })));
            }
        }
        Err(SmeltError::unsupported(
            self.span(call.span.start, call.span.end),
            "string case methods require a string receiver and no arguments",
        ))
    }

    /// Lower direct TypeScript string trimming.
    fn string_trim_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let side = match member.property.name.as_str() {
            "trim" => StringTrimSide::Both,
            "trimStart" => StringTrimSide::Start,
            "trimEnd" => StringTrimSide::End,
            _ => return Ok(None),
        };
        if !call.arguments.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string trim requires no arguments",
            ));
        }
        let operand = self.expression(&member.object, body)?;
        let operand_ty = Self::expr_ty(body, operand);
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string trim requires a string receiver",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringTrim { side, operand },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string prefix and suffix tests.
    fn string_affix_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "startsWith" => StringAffixOp::StartsWith,
            "endsWith" => StringAffixOp::EndsWith,
            _ => return Ok(None),
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string prefix/suffix methods require exactly one argument",
            ));
        }
        let haystack = self.expression(&member.object, body)?;
        let Some(needle_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string prefix/suffix methods require exactly one argument",
            ));
        };
        let needle = self.argument(needle_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, needle)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string prefix/suffix methods require string receiver and argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringAffix {
                op,
                haystack,
                needle,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string search methods.
    fn string_search_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "indexOf" => StringSearchOp::Find,
            "lastIndexOf" => StringSearchOp::RFind,
            _ => return Ok(None),
        };
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string search optional fromIndex arguments are not supported yet",
            ));
        }
        let haystack = self.expression(&member.object, body)?;
        let Some(needle_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string search optional fromIndex arguments are not supported yet",
            ));
        };
        let needle = self.argument(needle_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, needle)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string search methods require string receiver and argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSearch {
                op,
                haystack,
                needle,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript literal string replacement.
    fn string_replace_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "replace" {
            return Ok(None);
        }
        let [pattern_argument, replacement_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string replace requires exactly two string arguments",
            ));
        };
        let haystack = self.expression(&member.object, body)?;
        let pattern = self.argument(pattern_argument, body)?;
        let replacement = self.argument(replacement_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, haystack)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, pattern)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, replacement)) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string replace requires string receiver, pattern, and replacement",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringReplace {
                op: StringReplaceOp::First,
                haystack,
                pattern,
                replacement,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string repetition.
    fn string_repeat_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "repeat" {
            return Ok(None);
        }
        let [count_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string repeat requires exactly one number argument",
            ));
        };
        let operand = self.expression(&member.object, body)?;
        let count = self.argument(count_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, count)) != Some(&Type::Float)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string repeat requires a string receiver and number argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::String);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringRepeat { operand, count },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `String.prototype.charAt` and `charCodeAt`.
    fn string_char_at_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let returns_code = match member.property.name.as_str() {
            "charAt" => false,
            "charCodeAt" => true,
            _ => return Ok(None),
        };
        let [index_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string charAt/charCodeAt requires exactly one number argument",
            ));
        };
        let operand = self.expression(&member.object, body)?;
        let index = self.argument(index_argument, body)?;
        if self.ctx.krate.types.get(Self::expr_ty(body, operand)) != Some(&Type::String)
            || self.ctx.krate.types.get(Self::expr_ty(body, index)) != Some(&Type::Float)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string charAt/charCodeAt requires a string receiver and number argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(if returns_code {
            Type::Float
        } else {
            Type::String
        });
        let kind = if returns_code {
            ExprKind::StringCharCodeAt { operand, index }
        } else {
            ExprKind::StringCharAt { operand, index }
        };
        Ok(Some(body.push_expr(Expr {
            kind,
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.join` for string arrays.
    fn string_join_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "join" {
            return Ok(None);
        }
        let items = self.expression(&member.object, body)?;
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let items_ty = Self::expr_ty(body, items);
        if self.ctx.krate.types.get(items_ty) != Some(&Type::List(string_ty)) {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array join currently requires a string[] receiver",
            ));
        }
        let separator = match call.arguments.as_slice() {
            [] => body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(",".to_owned())),
                ty: string_ty,
                span: self.span(call.span.start, call.span.end),
            }),
            [separator_argument] => {
                let separator = self.argument(separator_argument, body)?;
                if self.ctx.krate.types.get(Self::expr_ty(body, separator)) != Some(&Type::String) {
                    return Err(SmeltError::unsupported(
                        self.span(call.span.start, call.span.end),
                        "array join separator must be a string",
                    ));
                }
                separator
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(call.span.start, call.span.end),
                    "array join supports zero or one string separator argument",
                ));
            }
        };
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringJoin { items, separator },
            ty: string_ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string containment.
    fn string_contains_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "includes" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires exactly one argument",
            ));
        }
        let haystack = self.expression(&member.object, body)?;
        let Some(needle_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires exactly one argument",
            ));
        };
        let needle = self.argument(needle_argument, body)?;
        let haystack_ty = Self::expr_ty(body, haystack);
        let needle_ty = Self::expr_ty(body, needle);
        if self.ctx.krate.types.get(haystack_ty) != Some(&Type::String)
            || self.ctx.krate.types.get(needle_ty) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string includes requires string receiver and argument",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringContains { haystack, needle },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript array containment.
    fn list_contains_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "includes" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Ok(None);
        }
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(list_item_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let item_ty = *list_item_ty;
        let Some(item_argument) = call.arguments.first() else {
            return Ok(None);
        };
        let item = self.argument(item_argument, body)?;
        if Self::expr_ty(body, item) != item_ty {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array includes argument must match the array element type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListContains { list, item },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.concat` for one same-typed array argument.
    fn list_concat_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "concat" {
            return Ok(None);
        }
        let [right_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array concat currently requires exactly one array argument",
            ));
        };
        let left = self.expression(&member.object, body)?;
        let right = self.argument(right_argument, body)?;
        let ty = Self::expr_ty(body, left);
        if self
            .ctx
            .krate
            .types
            .get(ty)
            .is_none_or(|ty| !matches!(ty, Type::List(_)))
            || Self::expr_ty(body, right) != ty
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array concat requires same-typed array receiver and argument",
            ));
        }
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListConcat { left, right },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript `Array.prototype.indexOf` and `lastIndexOf`.
    fn list_search_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        let op = match member.property.name.as_str() {
            "indexOf" => ListSearchOp::Find,
            "lastIndexOf" => ListSearchOp::RFind,
            _ => return Ok(None),
        };
        let [item_argument] = call.arguments.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf currently require exactly one item argument",
            ));
        };
        let list = self.expression(&member.object, body)?;
        let list_ty = Self::expr_ty(body, list);
        let Some(Type::List(element_ty)) = self.ctx.krate.types.get(list_ty) else {
            return Ok(None);
        };
        let item_ty = *element_ty;
        let item = self.argument(item_argument, body)?;
        if Self::expr_ty(body, item) != item_ty {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "array indexOf/lastIndexOf argument must match the array element type",
            ));
        }
        let ty = self.ctx.krate.types.intern(Type::Float);
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::ListSearch { op, list, item },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower direct TypeScript string splitting.
    fn string_split_call(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(None);
        };
        if member.property.name != "split" {
            return Ok(None);
        }
        if call.arguments.len() != 1 {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires exactly one separator argument",
            ));
        }
        let haystack = self.expression(&member.object, body)?;
        let Some(separator_argument) = call.arguments.first() else {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires exactly one separator argument",
            ));
        };
        let separator = self.argument(separator_argument, body)?;
        let haystack_ty = Self::expr_ty(body, haystack);
        let separator_ty = Self::expr_ty(body, separator);
        if self.ctx.krate.types.get(haystack_ty) != Some(&Type::String)
            || self.ctx.krate.types.get(separator_ty) != Some(&Type::String)
        {
            return Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                "string split requires string receiver and separator",
            ));
        }
        let string_ty = self.ctx.krate.types.intern(Type::String);
        let ty = self.ctx.krate.types.intern(Type::List(string_ty));
        Ok(Some(body.push_expr(Expr {
            kind: ExprKind::StringSplit {
                haystack,
                separator,
            },
            ty,
            span: self.span(call.span.start, call.span.end),
        })))
    }

    /// Lower array entries passed to a `Promise.*` combinator.
    fn promise_array_args(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
    ) -> Result<Vec<smelt_hir::ExprId>, SmeltError> {
        array
            .elements
            .iter()
            .map(|element| match element {
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_) => {
                    Err(SmeltError::unsupported(
                        self.span(element.span().start, element.span().end),
                        "Promise combinator arrays cannot use spread or elision",
                    ))
                }
                _ => self.array_element(element, body),
            })
            .collect()
    }

    /// Lower an array element.
    fn array_element(
        &mut self,
        element: &ArrayExpressionElement<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match element {
            ArrayExpressionElement::NumericLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Float);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            ArrayExpressionElement::StringLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::String);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::String(lit.value.to_string())),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            ArrayExpressionElement::BooleanLiteral(lit) => {
                let ty = self.ctx.krate.types.intern(Type::Bool);
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(lit.value)),
                    ty,
                    span: self.span(lit.span.start, lit.span.end),
                }))
            }
            ArrayExpressionElement::Identifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            ArrayExpressionElement::BinaryExpression(binary) => {
                self.binary_expression(binary, body)
            }
            ArrayExpressionElement::LogicalExpression(logical) => {
                self.logical_expression(logical, body)
            }
            ArrayExpressionElement::UnaryExpression(unary) => self.unary_expression(unary, body),
            ArrayExpressionElement::CallExpression(call) => self.call_expression(call, body),
            ArrayExpressionElement::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            ArrayExpressionElement::StaticMemberExpression(member) => {
                self.static_member(member, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(element.span().start, element.span().end),
                format!("array element kind is not lowered yet: {element:?}"),
            )),
        }
    }

    /// Lower a binary expression.
    fn binary_expression(
        &mut self,
        binary: &oxc::ast::ast::BinaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let op = match binary.operator {
            BinaryOperator::Addition => BinOp::Add,
            BinaryOperator::Subtraction => BinOp::Sub,
            BinaryOperator::Multiplication => BinOp::Mul,
            BinaryOperator::Division => BinOp::Div,
            BinaryOperator::StrictEquality => BinOp::Eq,
            BinaryOperator::StrictInequality => BinOp::NotEq,
            BinaryOperator::Equality | BinaryOperator::Inequality => {
                return Err(SmeltError::unsupported(
                    self.span(binary.span.start, binary.span.end),
                    "coercive equality is not lowered; use === or !==",
                ));
            }
            BinaryOperator::LessThan => BinOp::Lt,
            BinaryOperator::LessEqualThan => BinOp::Lte,
            BinaryOperator::GreaterThan => BinOp::Gt,
            BinaryOperator::GreaterEqualThan => BinOp::Gte,
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(binary.span.start, binary.span.end),
                    format!("binary operator is not lowered yet: {:?}", binary.operator),
                ));
            }
        };
        let lhs = self.expression(&binary.left, body)?;
        let rhs = self.expression(&binary.right, body)?;
        let ty = match op {
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Lte | BinOp::Gt | BinOp::Gte => {
                self.ctx.krate.types.intern(Type::Bool)
            }
            _ => Self::expr_ty(body, lhs),
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty,
            span: self.span(binary.span.start, binary.span.end),
        }))
    }

    /// Lower a logical expression.
    fn logical_expression(
        &mut self,
        logical: &oxc::ast::ast::LogicalExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let op = match logical.operator {
            LogicalOperator::And => BinOp::And,
            LogicalOperator::Or => BinOp::Or,
            LogicalOperator::Coalesce => {
                return Err(SmeltError::unsupported(
                    self.span(logical.span.start, logical.span.end),
                    "nullish coalescing is not lowered yet",
                ));
            }
        };
        let lhs = self.expression(&logical.left, body)?;
        let rhs = self.expression(&logical.right, body)?;
        let ty = self.ctx.krate.types.intern(Type::Bool);
        Ok(body.push_expr(Expr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty,
            span: self.span(logical.span.start, logical.span.end),
        }))
    }

    /// Lower a unary expression.
    fn unary_expression(
        &mut self,
        unary: &oxc::ast::ast::UnaryExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let op = match unary.operator {
            UnaryOperator::LogicalNot => UnaryOp::Not,
            UnaryOperator::UnaryNegation => UnaryOp::Neg,
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(unary.span.start, unary.span.end),
                    format!("unary operator is not lowered yet: {:?}", unary.operator),
                ));
            }
        };
        let operand = self.expression(&unary.argument, body)?;
        let ty = match op {
            UnaryOp::Not => self.ctx.krate.types.intern(Type::Bool),
            UnaryOp::Neg => Self::expr_ty(body, operand),
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::UnaryOp { op, operand },
            ty,
            span: self.span(unary.span.start, unary.span.end),
        }))
    }

    /// Lower an array expression.
    fn array_expression(
        &mut self,
        array: &oxc::ast::ast::ArrayExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut items = Vec::new();
        for element in &array.elements {
            if matches!(
                element,
                ArrayExpressionElement::SpreadElement(_) | ArrayExpressionElement::Elision(_)
            ) {
                return Err(SmeltError::unsupported(
                    self.span(element.span().start, element.span().end),
                    "array spread elements and elisions are not lowered",
                ));
            }
            items.push(self.array_element(element, body)?);
        }
        let ty = if let Some(hint) = type_hint {
            hint
        } else if let Some(first) = items.first() {
            let item_ty = Self::expr_ty(body, *first);
            self.ctx.krate.types.intern(Type::List(item_ty))
        } else {
            return Err(SmeltError::unsupported(
                self.span(array.span.start, array.span.end),
                "empty arrays require an explicit type annotation",
            ));
        };
        Ok(body.push_expr(Expr {
            kind: if matches!(self.ctx.krate.types.get(ty), Some(Type::Tuple(_))) {
                ExprKind::TupleLit(items)
            } else {
                ExprKind::ListLit(items)
            },
            ty,
            span: self.span(array.span.start, array.span.end),
        }))
    }

    /// Lower an object expression.
    fn object_expression(
        &mut self,
        object: &oxc::ast::ast::ObjectExpression<'_>,
        body: &mut Body,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(ty) = type_hint else {
            return Err(SmeltError::unsupported(
                self.span(object.span.start, object.span.end),
                "object literals require a Record<string, T> annotation",
            ));
        };
        if !matches!(self.ctx.krate.types.get(ty), Some(Type::Dict(_, _))) {
            return Err(SmeltError::unsupported(
                self.span(object.span.start, object.span.end),
                "object literals currently require a Record<string, T> annotation",
            ));
        }
        let mut entries = Vec::new();
        for property in &object.properties {
            let ObjectPropertyKind::ObjectProperty(object_property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(property.span().start, property.span().end),
                    "object spread properties are not lowered yet",
                ));
            };
            if object_property.computed || object_property.method {
                return Err(SmeltError::unsupported(
                    self.span(object_property.span.start, object_property.span.end),
                    "computed object keys and object methods are not lowered yet",
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
                        "object literal keys must be static string keys",
                    ));
                }
            };
            let key_ty = self.ctx.krate.types.intern(Type::String);
            let key = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(key_text)),
                ty: key_ty,
                span: self.span(
                    object_property.key.span().start,
                    object_property.key.span().end,
                ),
            });
            let value = self.expression(&object_property.value, body)?;
            entries.push((key, value));
        }
        Ok(body.push_expr(Expr {
            kind: ExprKind::DictLit(entries),
            ty,
            span: self.span(object.span.start, object.span.end),
        }))
    }

    /// Lower a static member access expression.
    fn static_member(
        &mut self,
        member: &oxc::ast::ast::StaticMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if member.optional {
            return Err(SmeltError::unsupported(
                self.span(member.span.start, member.span.end),
                "optional member access is not lowered yet",
            ));
        }
        let receiver = self.expression(&member.object, body)?;
        let field = self.intern_source_name(member.property.name.as_str());
        if member.property.name == "length"
            && self.supports_stdlib_length(Self::expr_ty(body, receiver))
        {
            let ty = self.ctx.krate.types.intern(Type::Float);
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Len { operand: receiver },
                ty,
                span: self.span(member.span.start, member.span.end),
            }));
        }
        let ty = self.class_field_type(Self::expr_ty(body, receiver), field)?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Field { receiver, field },
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower a computed member access expression.
    fn computed_member(
        &mut self,
        member: &oxc::ast::ast::ComputedMemberExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if member.optional {
            return Err(SmeltError::unsupported(
                self.span(member.span.start, member.span.end),
                "optional index access is not lowered yet",
            ));
        }
        let receiver = self.expression(&member.object, body)?;
        let index = self.expression(&member.expression, body)?;
        let ty = self.index_type(Self::expr_ty(body, receiver))?;
        Ok(body.push_expr(Expr {
            kind: ExprKind::Index { receiver, index },
            ty,
            span: self.span(member.span.start, member.span.end),
        }))
    }

    /// Lower a call expression.
    fn call_expression(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        if let Some(expr) = self.fetch_call(call, body)? {
            return Ok(expr);
        }
        if let Expression::StaticMemberExpression(member) = &call.callee
            && let Expression::Identifier(object) = &member.object
            && object.name == "console"
            && member.property.name == "log"
        {
            let args = call
                .arguments
                .iter()
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            let ty = self.ctx.krate.types.intern(Type::None);
            let callee_item =
                self.ensure_console_log_item(self.span(member.span.start, member.span.end));
            let callee = body.push_expr(Expr {
                kind: ExprKind::Item(callee_item),
                ty,
                span: self.span(member.span.start, member.span.end),
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Call { callee, args },
                ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        if let Expression::Identifier(callee_ident) = &call.callee {
            let Some(item) = self.items.get(callee_ident.name.as_str()).copied() else {
                return Err(SmeltError::unsupported(
                    self.span(callee_ident.span.start, callee_ident.span.end),
                    format!("unresolved function `{}`", callee_ident.name),
                ));
            };
            let (params, return_ty, is_async) =
                if let Item::Function(function) = self.item_ref(item) {
                    (
                        function.params.iter().map(|param| param.ty).collect(),
                        function.return_ty,
                        function.is_async,
                    )
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(callee_ident.span.start, callee_ident.span.end),
                        "callee item is not a function",
                    ));
                };
            let args = call
                .arguments
                .iter()
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            let callee = body.push_expr(Expr {
                kind: ExprKind::Item(item),
                ty: self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Function(smelt_hir::FunctionType {
                        params,
                        return_ty,
                        is_async,
                    })),
                span: self.span(callee_ident.span.start, callee_ident.span.end),
            });
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Call { callee, args },
                ty: return_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        if let Expression::StaticMemberExpression(member) = &call.callee {
            let receiver = self.expression(&member.object, body)?;
            let method = self.intern_source_name(member.property.name.as_str());
            let (return_ty, _) = self.resolve_method(Self::expr_ty(body, receiver), method)?;
            let args = call
                .arguments
                .iter()
                .map(|arg| self.argument(arg, body))
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(body.push_expr(Expr {
                kind: ExprKind::Method {
                    receiver,
                    method,
                    args,
                },
                ty: return_ty,
                span: self.span(call.span.start, call.span.end),
            }));
        }
        Err(SmeltError::unsupported(
            self.span(call.span.start, call.span.end),
            "call expression is not lowered yet",
        ))
    }

    /// Extract target and value from assignment expression.
    fn assignment_parts(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, smelt_hir::ExprId), SmeltError> {
        let target = self.assignment_target_expr(&assign.left, body)?;
        let right = self.expression(&assign.right, body)?;
        let value = match assign.operator {
            AssignmentOperator::Assign => right,
            AssignmentOperator::Addition
            | AssignmentOperator::Subtraction
            | AssignmentOperator::Multiplication
            | AssignmentOperator::Division => {
                let op = match assign.operator {
                    AssignmentOperator::Addition => BinOp::Add,
                    AssignmentOperator::Subtraction => BinOp::Sub,
                    AssignmentOperator::Multiplication => BinOp::Mul,
                    AssignmentOperator::Division => BinOp::Div,
                    other => {
                        return Err(SmeltError::unsupported(
                            self.span(assign.span.start, assign.span.end),
                            format!("assignment operator is not lowered yet: {other:?}"),
                        ));
                    }
                };
                let ty = Self::expr_ty(body, target);
                body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op,
                        lhs: target,
                        rhs: right,
                    },
                    ty,
                    span: self.span(assign.span.start, assign.span.end),
                })
            }
            other => {
                return Err(SmeltError::unsupported(
                    self.span(assign.span.start, assign.span.end),
                    format!("assignment operator is not lowered yet: {other:?}"),
                ));
            }
        };
        Ok((target, value))
    }

    /// Extract target and value from increment/decrement expression.
    fn update_parts(
        &mut self,
        update: &oxc::ast::ast::UpdateExpression<'_>,
        body: &mut Body,
    ) -> Result<(smelt_hir::ExprId, smelt_hir::ExprId), SmeltError> {
        let target = self.simple_assignment_target_expr(&update.argument, body)?;
        let one_ty = Self::expr_ty(body, target);
        let one = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Float(1.0)),
            ty: one_ty,
            span: self.span(update.span.start, update.span.end),
        });
        let op = match update.operator {
            UpdateOperator::Increment => BinOp::Add,
            UpdateOperator::Decrement => BinOp::Sub,
        };
        let value = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op,
                lhs: target,
                rhs: one,
            },
            ty: one_ty,
            span: self.span(update.span.start, update.span.end),
        });
        Ok((target, value))
    }

    /// Convert assignment target to expression.
    fn assignment_target_expr(
        &mut self,
        target: &AssignmentTarget<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match target {
            AssignmentTarget::AssignmentTargetIdentifier(ident) => self.identifier_expression(
                ident.name.as_str(),
                ident.span.start,
                ident.span.end,
                body,
            ),
            AssignmentTarget::StaticMemberExpression(member) => self.static_member(member, body),
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(target.span().start, target.span().end),
                "assignment target must be a local, field, or index expression",
            )),
        }
    }

    /// Convert simple assignment target to expression.
    fn simple_assignment_target_expr(
        &mut self,
        target: &SimpleAssignmentTarget<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match target {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(ident) => self
                .identifier_expression(ident.name.as_str(), ident.span.start, ident.span.end, body),
            SimpleAssignmentTarget::StaticMemberExpression(member) => {
                self.static_member(member, body)
            }
            SimpleAssignmentTarget::ComputedMemberExpression(member) => {
                self.computed_member(member, body)
            }
            _ => Err(SmeltError::unsupported(
                self.span(target.span().start, target.span().end),
                "update target must be a local, field, or index expression",
            )),
        }
    }

    /// Convert TypeScript type to HIR type.
    fn ts_type_to_hir(&mut self, ty: &TSType<'_>) -> Result<smelt_hir::TypeId, SmeltError> {
        match ty {
            TSType::TSNumberKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Float)),
            TSType::TSStringKeyword(_) => Ok(self.ctx.krate.types.intern(Type::String)),
            TSType::TSBooleanKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Bool)),
            TSType::TSVoidKeyword(_) | TSType::TSNullKeyword(_) | TSType::TSUndefinedKeyword(_) => {
                Ok(self.ctx.krate.types.intern(Type::None))
            }
            TSType::TSLiteralType(literal) => match &literal.literal {
                oxc::ast::ast::TSLiteral::StringLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::String))
                }
                oxc::ast::ast::TSLiteral::NumericLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Float))
                }
                oxc::ast::ast::TSLiteral::BooleanLiteral(_) => {
                    Ok(self.ctx.krate.types.intern(Type::Bool))
                }
                _ => Err(SmeltError::unsupported(
                    self.span(ty.span().start, ty.span().end),
                    format!("literal type annotation is not lowered yet: {ty:?}"),
                )),
            },
            TSType::TSUnionType(union) => {
                let mut lowered = Vec::new();
                let mut nullish = Vec::new();
                for member in &union.types {
                    let member_ty = self.ts_type_to_hir(member)?;
                    if matches!(self.ctx.krate.types.get(member_ty), Some(Type::None)) {
                        nullish.push(member_ty);
                    } else if !lowered.contains(&member_ty) {
                        lowered.push(member_ty);
                    }
                }
                if lowered.len() == 1 && !nullish.is_empty() {
                    let single = lowered.remove(0);
                    Ok(self.ctx.krate.types.intern(Type::Optional(single)))
                } else if lowered.len() == 1 {
                    let single = lowered.remove(0);
                    Ok(single)
                } else {
                    lowered.extend(nullish);
                    Ok(self.ctx.krate.types.intern(Type::Union(lowered)))
                }
            }
            TSType::TSArrayType(array) => {
                let element_ty = self.ts_type_to_hir(&array.element_type)?;
                Ok(self.ctx.krate.types.intern(Type::List(element_ty)))
            }
            TSType::TSTupleType(tuple) => {
                let mut items = Vec::new();
                for item in &tuple.element_types {
                    items.push(self.tuple_element_type_to_hir(item)?);
                }
                Ok(self.ctx.krate.types.intern(Type::Tuple(items)))
            }
            TSType::TSTypeReference(reference) => self.type_reference_to_hir(reference),
            TSType::TSThisType(this_ty) => {
                let Some(class_name) = &self.current_class else {
                    return Err(SmeltError::unsupported(
                        self.span(this_ty.span.start, this_ty.span.end),
                        "this types outside classes are not lowered yet",
                    ));
                };
                let Some(class_item) = self.classes.get(class_name).copied() else {
                    return Err(SmeltError::unsupported(
                        self.span(this_ty.span.start, this_ty.span.end),
                        "this class type is not resolvable yet",
                    ));
                };
                let Item::Class(class) = self.item_ref(class_item) else {
                    return Err(SmeltError::unsupported(
                        self.span(this_ty.span.start, this_ty.span.end),
                        "this class type is not resolvable yet",
                    ));
                };
                Ok(self.ctx.krate.types.intern(Type::Class {
                    name: class.name,
                    args: Vec::new(),
                }))
            }
            _ => Err(SmeltError::unsupported(
                self.span(ty.span().start, ty.span().end),
                format!("type annotation is not lowered yet: {ty:?}"),
            )),
        }
    }

    /// Convert tuple element type to HIR type.
    fn tuple_element_type_to_hir(
        &mut self,
        item: &TSTupleElement<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match item {
            TSTupleElement::TSNumberKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Float)),
            TSTupleElement::TSStringKeyword(_) => Ok(self.ctx.krate.types.intern(Type::String)),
            TSTupleElement::TSBooleanKeyword(_) => Ok(self.ctx.krate.types.intern(Type::Bool)),
            TSTupleElement::TSNullKeyword(_)
            | TSTupleElement::TSUndefinedKeyword(_)
            | TSTupleElement::TSVoidKeyword(_) => Ok(self.ctx.krate.types.intern(Type::None)),
            TSTupleElement::TSArrayType(array) => {
                let element_ty = self.ts_type_to_hir(&array.element_type)?;
                Ok(self.ctx.krate.types.intern(Type::List(element_ty)))
            }
            TSTupleElement::TSTupleType(tuple) => {
                let mut items = Vec::new();
                for tuple_item in &tuple.element_types {
                    items.push(self.tuple_element_type_to_hir(tuple_item)?);
                }
                Ok(self.ctx.krate.types.intern(Type::Tuple(items)))
            }
            TSTupleElement::TSTypeReference(reference) => self.type_reference_to_hir(reference),
            TSTupleElement::TSOptionalType(optional) => {
                let inner = self.ts_type_to_hir(&optional.type_annotation)?;
                Ok(self.ctx.krate.types.intern(Type::Optional(inner)))
            }
            TSTupleElement::TSRestType(rest) => Err(SmeltError::unsupported(
                self.span(rest.span.start, rest.span.end),
                "tuple rest types are not lowered yet",
            )),
            TSTupleElement::TSNamedTupleMember(named) => {
                self.tuple_element_type_to_hir(&named.element_type)
            }
            _ => Err(SmeltError::unsupported(
                self.span(item.span().start, item.span().end),
                format!("tuple element type is not lowered yet: {item:?}"),
            )),
        }
    }

    /// Convert type reference to HIR type.
    fn type_reference_to_hir(
        &mut self,
        reference: &oxc::ast::ast::TSTypeReference<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let TSTypeName::IdentifierReference(name) = &reference.type_name else {
            return Err(SmeltError::unsupported(
                self.span(reference.span.start, reference.span.end),
                "qualified type references are not lowered yet",
            ));
        };
        let name_text = name.name.as_str();
        let args = reference
            .type_arguments
            .as_ref()
            .map(|args| args.params.iter().collect::<Vec<_>>())
            .unwrap_or_default();
        match (name_text, args.as_slice()) {
            ("Array", [item]) => {
                let lowered_item = self.ts_type_to_hir(item)?;
                Ok(self.ctx.krate.types.intern(Type::List(lowered_item)))
            }
            ("Record", [key, value]) => {
                let lowered_key = self.ts_type_to_hir(key)?;
                if self.ctx.krate.types.get(lowered_key) != Some(&Type::String) {
                    return Err(SmeltError::unsupported(
                        self.span(reference.span.start, reference.span.end),
                        "only Record<string, T> is lowered for now",
                    ));
                }
                let lowered_value = self.ts_type_to_hir(value)?;
                Ok(self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Dict(lowered_key, lowered_value)))
            }
            ("Promise", [item]) => {
                let lowered_item = self.ts_type_to_hir(item)?;
                Ok(self.ctx.krate.types.intern(Type::Future(lowered_item)))
            }
            (_, []) => {
                let symbol = self.intern_type_name(name_text);
                Ok(self.ctx.krate.types.intern(Type::Class {
                    name: symbol,
                    args: Vec::new(),
                }))
            }
            _ => Err(SmeltError::unsupported(
                self.span(reference.span.start, reference.span.end),
                format!("type reference is not lowered yet: {name_text}"),
            )),
        }
    }

    /// Resolve the type of a class field.
    fn class_field_type(
        &self,
        receiver_ty: smelt_hir::TypeId,
        field: smelt_hir::Symbol,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::Dict(_, value)) => Ok(*value),
            Some(Type::Class { name, .. }) => self
                .class_by_symbol(*name)
                .and_then(|class| {
                    class
                        .fields
                        .iter()
                        .find(|item| item.name == field)
                        .map(|item| item.ty)
                })
                .or_else(|| {
                    let class_name = self
                        .ctx
                        .krate
                        .names
                        .get(*name)
                        .or_else(|| self.ctx.krate.symbols.get(*name))?;
                    self.class_fields.get(class_name).and_then(|fields| {
                        fields
                            .iter()
                            .find(|item| item.name == field)
                            .map(|item| item.ty)
                    })
                })
                .ok_or_else(|| {
                    let field_name = self.ctx.krate.symbols.get(field).unwrap_or("<unknown>");
                    SmeltError::unsupported(
                        self.span(0, 0),
                        format!("unknown class field `{field_name}`"),
                    )
                }),
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                "field access is only lowered for Record<string, T> and class values for now",
            )),
        }
    }

    /// Look up a class by its symbol.
    fn class_by_symbol(&self, name: smelt_hir::Symbol) -> Option<&Class> {
        self.ctx.krate.items.iter().find_map(|item| {
            if let Item::Class(class) = item {
                if class.name == name {
                    return Some(class);
                }
            }
            None
        })
    }

    /// Resolve a method call on a type.
    fn resolve_method(
        &self,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
    ) -> Result<(smelt_hir::TypeId, smelt_hir::ItemId), SmeltError> {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(receiver_ty) else {
            return Err(SmeltError::unsupported(
                self.span(0, 0),
                "method calls are only lowered for class values for now",
            ));
        };
        let Some(class) = self.class_by_symbol(*name) else {
            return Err(SmeltError::unsupported(
                self.span(0, 0),
                "method receiver class is unknown",
            ));
        };
        for item in &class.methods {
            if let Item::Function(function) = self.item_ref(*item)
                && function.name == method
            {
                return Ok((function.return_ty, *item));
            }
        }
        let method_name = self.ctx.krate.symbols.get(method).unwrap_or("<unknown>");
        Err(SmeltError::unsupported(
            self.span(0, 0),
            format!("unknown class method `{method_name}`"),
        ))
    }

    /// Get the element type of an indexable type.
    fn index_type(
        &mut self,
        receiver_ty: smelt_hir::TypeId,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(item)) => Ok(*item),
            Some(Type::String) => Ok(self.ctx.krate.types.intern(Type::String)),
            Some(Type::Dict(_, value)) => Ok(*value),
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                "index access is only lowered for arrays, strings, and records for now",
            )),
        }
    }

    /// Returns true when TypeScript `.length` can lower directly to Rust `.len()`.
    fn supports_stdlib_length(&self, receiver_ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(receiver_ty),
            Some(Type::List(_) | Type::String | Type::Tuple(_))
        )
    }

    /// Return the inner item type for a `Promise<T>` / `Future<T>` value.
    fn future_inner_type(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Future(inner)) => Some(*inner),
            _ => None,
        }
    }

    /// Intern a source identifier name and convert from `camelCase` to `snake_case`.
    fn intern_source_name(&mut self, name: &str) -> smelt_hir::Symbol {
        let symbol = self.ctx.krate.symbols.intern(&camel_to_snake(name));
        self.ctx.krate.names.record(symbol, name);
        symbol
    }

    /// Intern a type name symbol.
    fn intern_type_name(&mut self, name: &str) -> smelt_hir::Symbol {
        let symbol = self.ctx.krate.symbols.intern(name);
        self.ctx.krate.names.record(symbol, name);
        symbol
    }

    /// Create an identifier expression from a local variable.
    fn identifier_expression(
        &self,
        name: &str,
        start: u32,
        end: u32,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(local) = self.locals.get(name).copied() else {
            return Err(SmeltError::unsupported(
                self.span(start, end),
                format!("unresolved identifier `{name}`"),
            ));
        };
        let ty = Self::local_ty(body, local);
        Ok(body.push_expr(Expr {
            kind: ExprKind::Local(local),
            ty,
            span: self.span(start, end),
        }))
    }

    /// Ensure a console.log item exists in the HIR.
    fn ensure_console_log_item(&mut self, span: Span) -> smelt_hir::ItemId {
        let name = self.ctx.krate.symbols.intern(smelt_hir::CONSOLE_LOG_SYMBOL);
        let none = self.ctx.krate.types.intern(Type::None);
        self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            params: Vec::new(),
            return_ty: none,
            is_async: false,
            body: None,
            owner: FunctionOwner::Module,
        }))
    }

    /// Create a Span from byte offsets.
    fn span(&self, start: u32, end: u32) -> Span {
        Span::new(self.file_id, start, end)
    }

    /// Get the span of a statement.
    fn statement_span(&self, statement: &Statement<'_>) -> Span {
        let span = statement.span();
        self.span(span.start, span.end)
    }

    /// Get the span of an expression.
    fn expression_span(&self, expression: &Expression<'_>) -> Span {
        let span = expression.span();
        self.span(span.start, span.end)
    }

    /// Convert a property key to a symbol.
    fn property_key_symbol(
        &mut self,
        key: &PropertyKey<'_>,
    ) -> Result<smelt_hir::Symbol, SmeltError> {
        match key {
            PropertyKey::StaticIdentifier(ident) => {
                Ok(self.intern_source_name(ident.name.as_str()))
            }
            PropertyKey::PrivateIdentifier(ident) => {
                Ok(self.intern_source_name(ident.name.as_str()))
            }
            PropertyKey::StringLiteral(lit) => Ok(self.intern_source_name(lit.value.as_str())),
            _ => Err(SmeltError::unsupported(
                self.span(key.span().start, key.span().end),
                "property names must be static identifiers or string literals",
            )),
        }
    }

    /// Resolve a class `implements` clause entry to an interface symbol.
    fn implements_symbol(
        &mut self,
        item: &oxc::ast::ast::TSClassImplements<'_>,
    ) -> Result<smelt_hir::Symbol, SmeltError> {
        if item.type_arguments.is_some() {
            return Err(SmeltError::unsupported(
                self.span(item.span.start, item.span.end),
                "generic implements clauses are not lowered yet",
            ));
        }
        let TSTypeName::IdentifierReference(name) = &item.expression else {
            return Err(SmeltError::unsupported(
                self.span(item.span.start, item.span.end),
                "qualified implements clauses are not lowered yet",
            ));
        };
        Ok(self.intern_type_name(name.name.as_str()))
    }

    /// Convert an interface heritage clause to the referenced interface symbol.
    fn interface_heritage_symbol(
        &mut self,
        item: &oxc::ast::ast::TSInterfaceHeritage<'_>,
    ) -> Result<smelt_hir::Symbol, SmeltError> {
        if item.type_arguments.is_some() {
            return Err(SmeltError::unsupported(
                self.span(item.span.start, item.span.end),
                "generic interface inheritance is not lowered yet",
            ));
        }
        let Expression::Identifier(name) = &item.expression else {
            return Err(SmeltError::unsupported(
                self.span(item.span.start, item.span.end),
                "qualified interface inheritance is not lowered yet",
            ));
        };
        Ok(self.intern_type_name(name.name.as_str()))
    }

    /// Find a previously lowered interface by symbol.
    fn find_interface(&self, name: smelt_hir::Symbol) -> Option<&Interface> {
        self.ctx.krate.items.iter().find_map(|item| {
            if let Item::Interface(interface) = item {
                if interface.name == name {
                    return Some(interface);
                }
            }
            None
        })
    }

    /// Validate that a lowered class satisfies all declared interfaces.
    fn validate_implements(&self, class_item: smelt_hir::ItemId) -> Result<(), SmeltError> {
        let Item::Class(class) = self.item_ref(class_item) else {
            return Ok(());
        };
        for interface_name in &class.implements {
            let interface = self
                .ctx
                .krate
                .items
                .iter()
                .find_map(|item| {
                    if let Item::Interface(interface) = item
                        && interface.name == *interface_name
                    {
                        return Some(interface);
                    }
                    None
                })
                .ok_or_else(|| {
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(*interface_name)
                        .unwrap_or("<unknown>");
                    SmeltError::unsupported(
                        class.span,
                        format!("implemented interface `{name}` is not declared"),
                    )
                })?;
            for required in &interface.fields {
                let Some(actual) = class
                    .fields
                    .iter()
                    .find(|field| field.name == required.name)
                else {
                    if required.optional {
                        continue;
                    }
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(required.name)
                        .unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(
                        required.span,
                        format!("class is missing implemented interface field `{name}`"),
                    ));
                };
                if !field_type_satisfies(&self.ctx.krate, actual.ty, required) {
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(required.name)
                        .unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(
                        actual.span,
                        format!("implemented interface field `{name}` has a mismatched type"),
                    ));
                }
            }
            for required in &interface.methods {
                let Some(actual_item) = class.methods.iter().find(|method_item| {
                    matches!(self.item_ref(**method_item), Item::Function(function) if function.name == required.name)
                }) else {
                    let name = self.ctx.krate.symbols.get(required.name).unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(required.span, format!("class is missing implemented interface method `{name}`")));
                };
                let Item::Function(actual) = self.item_ref(*actual_item) else {
                    return Err(SmeltError::unsupported(
                        required.span,
                        "implemented interface method has an unexpected item kind",
                    ));
                };
                let actual_params = actual
                    .params
                    .iter()
                    .filter(|param| self.ctx.krate.symbols.get(param.name) != Some("this"))
                    .map(|param| param.ty)
                    .collect::<Vec<_>>();
                let required_params = required
                    .params
                    .iter()
                    .map(|param| param.ty)
                    .collect::<Vec<_>>();
                if actual_params != required_params || actual.return_ty != required.return_ty {
                    let name = self
                        .ctx
                        .krate
                        .symbols
                        .get(required.name)
                        .unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(
                        actual.span,
                        format!("implemented interface method `{name}` has a mismatched signature"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Look up the type of an existing expression.
    fn expr_ty(body: &Body, expr: smelt_hir::ExprId) -> smelt_hir::TypeId {
        let index = usize::try_from(expr.0).expect("expr id should fit into usize");
        body.exprs
            .get(index)
            .expect("expr id should point to an existing expression")
            .ty
    }

    /// Look up the type of an existing local.
    fn local_ty(body: &Body, local: smelt_hir::LocalId) -> smelt_hir::TypeId {
        let index = usize::try_from(local.0).expect("local id should fit into usize");
        body.locals
            .get(index)
            .expect("local id should point to an existing local")
            .ty
    }

    /// Look up a lowered item by id.
    fn item_ref(&self, item: smelt_hir::ItemId) -> &Item {
        let index = usize::try_from(item.0).expect("item id should fit into usize");
        self.ctx
            .krate
            .items
            .get(index)
            .expect("item id should point to an existing item")
    }
}

/// Return an item's original source name when available.
fn item_name<'a>(krate: &'a smelt_hir::Crate, item: &Item) -> Option<&'a str> {
    let symbol = match item {
        Item::Function(function) => function.name,
        Item::Class(class) => class.name,
        Item::Interface(interface) => interface.name,
        Item::TypeAlias(alias) => alias.name,
        Item::Const(const_item) => const_item.name,
    };
    krate
        .names
        .get(symbol)
        .or_else(|| krate.symbols.get(symbol))
}

/// Return whether a property key can be resolved without runtime evaluation.
fn is_static_property_key(key: &PropertyKey<'_>) -> bool {
    matches!(
        key,
        PropertyKey::StaticIdentifier(_)
            | PropertyKey::PrivateIdentifier(_)
            | PropertyKey::StringLiteral(_)
    )
}

/// Extract the source text represented by a module export name.
fn module_export_name(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(ident) => ident.name.as_str().to_owned(),
        ModuleExportName::IdentifierReference(ident) => ident.name.as_str().to_owned(),
        ModuleExportName::StringLiteral(literal) => literal.value.as_str().to_owned(),
    }
}

/// Determine HIR visibility from TypeScript accessibility metadata.
fn visibility(accessibility: Option<TSAccessibility>) -> Visibility {
    match accessibility {
        Some(TSAccessibility::Private) => Visibility::Private,
        Some(TSAccessibility::Protected) => Visibility::Protected,
        Some(TSAccessibility::Public) | None => Visibility::Public,
    }
}

/// Return whether a statement always terminates control flow.
fn statement_terminates(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => block.body.iter().any(statement_terminates),
        Statement::IfStatement(if_stmt) => if_stmt.alternate.as_ref().is_some_and(|alternate| {
            statement_terminates(&if_stmt.consequent) && statement_terminates(alternate)
        }),
        Statement::BreakStatement(_)
        | Statement::ContinueStatement(_)
        | Statement::DebuggerStatement(_)
        | Statement::DoWhileStatement(_)
        | Statement::EmptyStatement(_)
        | Statement::ExpressionStatement(_)
        | Statement::ForInStatement(_)
        | Statement::ForOfStatement(_)
        | Statement::ForStatement(_)
        | Statement::LabeledStatement(_)
        | Statement::SwitchStatement(_)
        | Statement::TryStatement(_)
        | Statement::WhileStatement(_)
        | Statement::WithStatement(_)
        | Statement::VariableDeclaration(_)
        | Statement::FunctionDeclaration(_)
        | Statement::ClassDeclaration(_)
        | Statement::TSTypeAliasDeclaration(_)
        | Statement::TSInterfaceDeclaration(_)
        | Statement::TSEnumDeclaration(_)
        | Statement::TSModuleDeclaration(_)
        | Statement::TSGlobalDeclaration(_)
        | Statement::TSImportEqualsDeclaration(_)
        | Statement::ImportDeclaration(_)
        | Statement::ExportAllDeclaration(_)
        | Statement::ExportDefaultDeclaration(_)
        | Statement::ExportNamedDeclaration(_)
        | Statement::TSExportAssignment(_)
        | Statement::TSNamespaceExportDeclaration(_) => false,
    }
}
