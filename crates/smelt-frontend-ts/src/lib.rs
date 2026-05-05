//! TypeScript frontend for the Smelt compiler.
//!
//! This module provides parsing and lowering of TypeScript code into the Smelt HIR (High-level
//! Intermediate Representation). It handles type annotations, classes, interfaces, functions,
//! and various control flow constructs.

#![expect(
    clippy::too_many_lines,
    reason = "TypeScript lowering is still organized around large AST match functions"
)]
#![expect(
    clippy::too_many_arguments,
    reason = "class and function lowering currently pass explicit context instead of builder structs"
)]
#![expect(
    clippy::type_complexity,
    reason = "Oxc AST types are verbose and will be wrapped by local aliases in a later cleanup"
)]
#![expect(
    clippy::many_single_char_names,
    reason = "short names appear in generated TypeScript AST pattern matches"
)]
#![expect(
    clippy::cast_possible_truncation,
    reason = "HIR IDs are compact u32 indexes and overflow checks are being centralized incrementally"
)]
#![expect(
    clippy::single_match,
    reason = "declaration lowering keeps match structure ready for nearby variants"
)]
#![expect(
    clippy::doc_markdown,
    reason = "diagnostic docs mention source-language tokens without full rustdoc markup yet"
)]
#![expect(
    clippy::missing_const_for_fn,
    reason = "utility const qualification will be handled after behavior cleanup"
)]
#![expect(
    clippy::must_use_candidate,
    reason = "frontend helpers are mostly internal and will get must_use annotations in a focused pass"
)]
#![expect(
    clippy::missing_errors_doc,
    reason = "public checker docs need a dedicated polish pass"
)]

pub mod checker;

use std::collections::HashMap;

use oxc::allocator::Allocator;
use oxc::ast::ast::{
    Argument, ArrayExpressionElement, AssignmentTarget, BindingPattern, ClassElement, Declaration,
    Expression, ForStatementInit, ForStatementLeft, MethodDefinitionKind, ObjectPropertyKind,
    Program, PropertyKey, SimpleAssignmentTarget, Statement, TSAccessibility, TSSignature,
    TSTupleElement, TSType, TSTypeName,
};
use oxc::parser::{ParseOptions, Parser};
use oxc::span::{GetSpan, SourceType};
use oxc::syntax::operator::{
    AssignmentOperator, BinaryOperator, LogicalOperator, UnaryOperator, UpdateOperator,
};
use smelt_hir::{
    BinOp, Body, Class, Crate as HirCrate, Expr, ExprKind, Field, FileId, Function, FunctionOwner,
    Interface, Item, Language, Literal, LocalDecl, MatchArm, MethodSig, Module, ModuleId, Param,
    ParamSig, Pattern, SourceFile, Span, Stmt, Type, UnaryOp, Visibility,
};

/// Error type for Smelt TypeScript frontend.
///
/// Contains diagnostic information about parse or lowering errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmeltError {
    /// Error code identifying the error type.
    pub code: &'static str,
    /// Source location of the error.
    pub span: Span,
    /// Human-readable error message.
    pub message: String,
    /// Optional note with additional context.
    pub note: Option<String>,
}

impl SmeltError {
    /// Create an unsupported TypeScript feature error.
    fn unsupported(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::unsupported-ts",
            span,
            message: message.into(),
            note: None,
        }
    }

    /// Create a parse error.
    fn parse(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::parse-error",
            span,
            message: message.into(),
            note: None,
        }
    }
}

/// Context for building HIR from TypeScript source.
///
/// Manages the crate structure and accumulates items during lowering.
#[derive(Debug)]
pub struct HirCtx {
    /// The HIR crate being constructed.
    pub krate: HirCrate,
}

impl HirCtx {
    /// Create a new empty HIR context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            krate: HirCrate::new(),
        }
    }
}

impl Default for HirCtx {
    /// Create a new HIR context (same as `new`).
    fn default() -> Self {
        Self::new()
    }
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
                    Span::new(file_id, 0, source.len() as u32),
                    error.to_string(),
                )
            })
            .collect());
    }

    let mut builder = ModuleBuilder::new(file_id, ctx);
    builder.program(&parsed.program)
}

/// Builder for lowering TypeScript module to HIR.
///
/// Accumulates scoping information, items, and local variables during module construction.
struct ModuleBuilder<'ctx> {
    /// File ID for error reporting.
    file_id: FileId,
    /// Mutable reference to the HIR context.
    ctx: &'ctx mut HirCtx,
    /// Local variable bindings in current scope.
    locals: HashMap<String, smelt_hir::LocalId>,
    /// Declared items (functions, classes, interfaces).
    items: HashMap<String, smelt_hir::ItemId>,
    /// Class definitions by name.
    classes: HashMap<String, smelt_hir::ItemId>,
    /// Interface definitions by name.
    interfaces: HashMap<String, smelt_hir::ItemId>,
    /// Fields for each class.
    class_fields: HashMap<String, Vec<Field>>,
    /// Currently processing class name, if any.
    current_class: Option<String>,
}

impl<'ctx> ModuleBuilder<'ctx> {
    /// Create a new module builder.
    fn new(file_id: FileId, ctx: &'ctx mut HirCtx) -> Self {
        Self {
            file_id,
            ctx,
            locals: HashMap::new(),
            items: HashMap::new(),
            classes: HashMap::new(),
            interfaces: HashMap::new(),
            class_fields: HashMap::new(),
            current_class: None,
        }
    }

    /// Lower a TypeScript program to HIR module.
    fn program(&mut self, program: &Program<'_>) -> Result<ModuleId, Vec<SmeltError>> {
        let span = self.span(program.span.start, program.span.end);
        let mut body = Body::new(None, span);
        let mut errors = Vec::new();

        let mut module = Module::new(
            "main",
            SourceFile {
                path: "<memory>".to_owned(),
                language: Language::TypeScript,
            },
        );

        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    match self.function_declaration(function) {
                        Ok(item) => module.items.push(item),
                        Err(error) => errors.push(error),
                    }
                }
                Statement::ClassDeclaration(class) => match self.class_declaration(class) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                },
                Statement::TSInterfaceDeclaration(interface) => {
                    match self.interface_declaration(interface) {
                        Ok(item) => module.items.push(item),
                        Err(error) => errors.push(error),
                    }
                }
                Statement::ExportNamedDeclaration(export) => {
                    let Some(decl) = &export.declaration else {
                        continue;
                    };
                    match decl {
                        Declaration::FunctionDeclaration(f) => match self.function_declaration(f) {
                            Ok(item) => module.items.push(item),
                            Err(e) => errors.push(e),
                        },
                        Declaration::ClassDeclaration(c) => match self.class_declaration(c) {
                            Ok(item) => module.items.push(item),
                            Err(e) => errors.push(e),
                        },
                        Declaration::TSInterfaceDeclaration(i) => {
                            match self.interface_declaration(i) {
                                Ok(item) => module.items.push(item),
                                Err(e) => errors.push(e),
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
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
        if function.r#async {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "async functions are not lowered yet",
            ));
        }

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

        let saved_locals = std::mem::take(&mut self.locals);
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut params = Vec::new();

        for param in &function.params.items {
            let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                self.locals = saved_locals;
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
        self.locals = saved_locals;

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
            match element {
                ClassElement::PropertyDefinition(property) => {
                    if property.computed {
                        return Err(SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "computed property names are not lowered yet",
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
                _ => {}
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
                    if method.computed {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "computed method names are not lowered yet",
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
                    if method.value.r#async {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "async methods are not lowered yet",
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

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_class = self.current_class.replace(class_text.to_owned());
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
        self.locals = saved_locals;
        self.current_class = saved_class;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        let body_id = self.ctx.krate.push_body(body);
        Ok(self.ctx.krate.push_item(Item::Function(Function {
            name: method_name,
            span: self.span(method.span.start, method.span.end),
            params,
            return_ty,
            is_async: false,
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
        if !interface.extends.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(interface.span.start, interface.span.end),
                "interface inheritance is not lowered yet",
            ));
        }
        let name_text = interface.id.name.as_str();
        let name = self.intern_type_name(name_text);
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        for sig in &interface.body.body {
            match sig {
                TSSignature::TSPropertySignature(prop) => {
                    if prop.computed {
                        return Err(SmeltError::unsupported(
                            self.span(prop.span.start, prop.span.end),
                            "computed interface property names are not lowered yet",
                        ));
                    }
                    if prop.optional {
                        return Err(SmeltError::unsupported(
                            self.span(prop.span.start, prop.span.end),
                            "optional interface fields are not lowered yet",
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
                    fields.push(Field {
                        name: self.property_key_symbol(&prop.key)?,
                        ty,
                        visibility: Visibility::Public,
                        optional: false,
                        span: self.span(prop.span.start, prop.span.end),
                    });
                }
                TSSignature::TSMethodSignature(method) => {
                    if method.computed
                        || method.optional
                        || method.type_parameters.is_some()
                        || method.this_param.is_some()
                    {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "generic, optional, computed, and this-parameter interface methods are not lowered yet",
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
                        is_async: false,
                        span: self.span(method.span.start, method.span.end),
                    });
                }
                _ => {
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
                    for statement in &case.consequent {
                        if matches!(statement, Statement::ContinueStatement(_)) {
                            return Err(SmeltError::unsupported(
                                self.statement_span(statement),
                                "switch continue lowering is not implemented yet",
                            ));
                        }
                        if matches!(statement, Statement::BreakStatement(_)) {
                            saw_break = true;
                            break;
                        }
                        self.statement_in_block(statement, body, case_block)?;
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
        match statement {
            Statement::BlockStatement(block_stmt) => {
                for statement in &block_stmt.body {
                    self.statement_in_block(statement, body, block)?;
                }
            }
            _ => self.statement_in_block(statement, body, block)?,
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
            let value = match &declarator.init {
                Some(init) => Some(self.expression_with_hint(init, body, annotated_ty)?),
                None => None,
            };
            let ty = annotated_ty
                .or_else(|| value.map(|expr_id| body.exprs[expr_id.0 as usize].ty))
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
                Expression::UpdateExpression(update) => self.update_parts(update, body)?,
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
        let declarator = &decl.declarations[0];
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
                    let item_ty = body.exprs[first.0 as usize].ty;
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
                    let ObjectPropertyKind::ObjectProperty(property) = property else {
                        return Err(SmeltError::unsupported(
                            self.span(property.span().start, property.span().end),
                            "object spread properties are not lowered yet",
                        ));
                    };
                    if property.computed || property.method {
                        return Err(SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "computed object keys and object methods are not lowered yet",
                        ));
                    }
                    let key_text = match &property.key {
                        PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
                        PropertyKey::StringLiteral(lit) => lit.value.to_string(),
                        _ => {
                            return Err(SmeltError::unsupported(
                                self.span(property.key.span().start, property.key.span().end),
                                "object literal keys must be static string keys",
                            ));
                        }
                    };
                    let key_ty = self.ctx.krate.types.intern(Type::String);
                    let key = body.push_expr(Expr {
                        kind: ExprKind::Literal(Literal::String(key_text)),
                        ty: key_ty,
                        span: self.span(property.key.span().start, property.key.span().end),
                    });
                    let value = self.expression(&property.value, body)?;
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
                    _ => body.exprs[lhs.0 as usize].ty,
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
                let ty = match op {
                    UnaryOp::Not => self.ctx.krate.types.intern(Type::Bool),
                    UnaryOp::Neg => body.exprs[operand.0 as usize].ty,
                };
                Ok(body.push_expr(Expr {
                    kind: ExprKind::UnaryOp { op, operand },
                    ty,
                    span: self.span(unary.span.start, unary.span.end),
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
                let ty = self.class_field_type(body.exprs[receiver.0 as usize].ty, field)?;
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
                let ty = self.index_type(body.exprs[receiver.0 as usize].ty)?;
                Ok(body.push_expr(Expr {
                    kind: ExprKind::Index { receiver, index },
                    ty,
                    span: self.span(member.span.start, member.span.end),
                }))
            }
            Expression::CallExpression(call) => {
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
                        self.resolve_method(body.exprs[receiver.0 as usize].ty, method)?;
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
                    let (params, return_ty, is_async) = match &self.ctx.krate.items[item.0 as usize]
                    {
                        Item::Function(function) => (
                            function.params.iter().map(|param| param.ty).collect(),
                            function.return_ty,
                            function.is_async,
                        ),
                        _ => {
                            return Err(SmeltError::unsupported(
                                self.span(callee_ident.span.start, callee_ident.span.end),
                                "callee item is not a function",
                            ));
                        }
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
                let Item::Class(class) = &self.ctx.krate.items[item.0 as usize] else {
                    unreachable!();
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

                // Build the first segment from quasi[0]
                let first_str = tpl.quasis[0]
                    .value
                    .cooked
                    .as_ref()
                    .map_or_else(|| tpl.quasis[0].value.raw.as_str(), |c| c.as_str())
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
                    if let Some(quasi) = tpl.quasis.get(i + 1) {
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
            _ => body.exprs[lhs.0 as usize].ty,
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
            UnaryOp::Neg => body.exprs[operand.0 as usize].ty,
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
            let item_ty = body.exprs[first.0 as usize].ty;
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
            let ObjectPropertyKind::ObjectProperty(property) = property else {
                return Err(SmeltError::unsupported(
                    self.span(property.span().start, property.span().end),
                    "object spread properties are not lowered yet",
                ));
            };
            if property.computed || property.method {
                return Err(SmeltError::unsupported(
                    self.span(property.span.start, property.span.end),
                    "computed object keys and object methods are not lowered yet",
                ));
            }
            let key_text = match &property.key {
                PropertyKey::StaticIdentifier(ident) => ident.name.as_str().to_owned(),
                PropertyKey::StringLiteral(lit) => lit.value.to_string(),
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(property.key.span().start, property.key.span().end),
                        "object literal keys must be static string keys",
                    ));
                }
            };
            let key_ty = self.ctx.krate.types.intern(Type::String);
            let key = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::String(key_text)),
                ty: key_ty,
                span: self.span(property.key.span().start, property.key.span().end),
            });
            let value = self.expression(&property.value, body)?;
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
        let ty = self.class_field_type(body.exprs[receiver.0 as usize].ty, field)?;
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
        let ty = self.index_type(body.exprs[receiver.0 as usize].ty)?;
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
            let (params, return_ty, is_async) = match &self.ctx.krate.items[item.0 as usize] {
                Item::Function(function) => (
                    function.params.iter().map(|param| param.ty).collect(),
                    function.return_ty,
                    function.is_async,
                ),
                _ => {
                    return Err(SmeltError::unsupported(
                        self.span(callee_ident.span.start, callee_ident.span.end),
                        "callee item is not a function",
                    ));
                }
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
            let (return_ty, _) = self.resolve_method(body.exprs[receiver.0 as usize].ty, method)?;
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
                    _ => unreachable!(),
                };
                let ty = body.exprs[target.0 as usize].ty;
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
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(assign.span.start, assign.span.end),
                    format!(
                        "assignment operator is not lowered yet: {:?}",
                        assign.operator
                    ),
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
        let one_ty = body.exprs[target.0 as usize].ty;
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
                    Ok(self.ctx.krate.types.intern(Type::Optional(lowered[0])))
                } else if lowered.len() == 1 {
                    Ok(lowered[0])
                } else {
                    lowered.extend(nullish);
                    Ok(self.ctx.krate.types.intern(Type::Union(lowered)))
                }
            }
            TSType::TSArrayType(array) => {
                let item = self.ts_type_to_hir(&array.element_type)?;
                Ok(self.ctx.krate.types.intern(Type::List(item)))
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
                let Item::Class(class) = &self.ctx.krate.items[class_item.0 as usize] else {
                    unreachable!();
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
                let item = self.ts_type_to_hir(&array.element_type)?;
                Ok(self.ctx.krate.types.intern(Type::List(item)))
            }
            TSTupleElement::TSTupleType(tuple) => {
                let mut items = Vec::new();
                for item in &tuple.element_types {
                    items.push(self.tuple_element_type_to_hir(item)?);
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
        match name_text {
            "Array" if args.len() == 1 => {
                let item = self.ts_type_to_hir(args[0])?;
                Ok(self.ctx.krate.types.intern(Type::List(item)))
            }
            "Record" if args.len() == 2 => {
                let key = self.ts_type_to_hir(args[0])?;
                if self.ctx.krate.types.get(key) != Some(&Type::String) {
                    return Err(SmeltError::unsupported(
                        self.span(reference.span.start, reference.span.end),
                        "only Record<string, T> is lowered for now",
                    ));
                }
                let value = self.ts_type_to_hir(args[1])?;
                Ok(self.ctx.krate.types.intern(Type::Dict(key, value)))
            }
            _ if args.is_empty() => {
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
        self.ctx.krate.items.iter().find_map(|item| match item {
            Item::Class(class) if class.name == name => Some(class),
            _ => None,
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
            if let Item::Function(function) = &self.ctx.krate.items[item.0 as usize]
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
    fn index_type(&self, receiver_ty: smelt_hir::TypeId) -> Result<smelt_hir::TypeId, SmeltError> {
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(item)) => Ok(*item),
            Some(Type::Dict(_, value)) => Ok(*value),
            _ => Err(SmeltError::unsupported(
                self.span(0, 0),
                "index access is only lowered for arrays and records for now",
            )),
        }
    }

    /// Intern a source identifier name and convert from camelCase to snake_case.
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
        let ty = body.locals[local.0 as usize].ty;
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
        self.ctx
            .krate
            .push_item(smelt_hir::Item::Function(smelt_hir::Function {
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

    fn validate_implements(&self, class_item: smelt_hir::ItemId) -> Result<(), SmeltError> {
        let Item::Class(class) = &self.ctx.krate.items[class_item.0 as usize] else {
            return Ok(());
        };
        for interface_name in &class.implements {
            let interface = self
                .ctx
                .krate
                .items
                .iter()
                .find_map(|item| match item {
                    Item::Interface(interface) if interface.name == *interface_name => {
                        Some(interface)
                    }
                    _ => None,
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
                if actual.ty != required.ty {
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
                let Some(actual_item) = class.methods.iter().find(|item| {
                    matches!(&self.ctx.krate.items[item.0 as usize], Item::Function(function) if function.name == required.name)
                }) else {
                    let name = self.ctx.krate.symbols.get(required.name).unwrap_or("<unknown>");
                    return Err(SmeltError::unsupported(required.span, format!("class is missing implemented interface method `{name}`")));
                };
                let Item::Function(actual) = &self.ctx.krate.items[actual_item.0 as usize] else {
                    unreachable!();
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
}

fn visibility(accessibility: Option<TSAccessibility>) -> Visibility {
    match accessibility {
        Some(TSAccessibility::Private) => Visibility::Private,
        Some(TSAccessibility::Protected) => Visibility::Protected,
        Some(TSAccessibility::Public) | None => Visibility::Public,
    }
}

fn statement_terminates(statement: &Statement<'_>) -> bool {
    match statement {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => block.body.iter().any(statement_terminates),
        Statement::IfStatement(if_stmt) => if_stmt.alternate.as_ref().is_some_and(|alternate| {
            statement_terminates(&if_stmt.consequent) && statement_terminates(alternate)
        }),
        _ => false,
    }
}

pub fn camel_to_snake(name: &str) -> String {
    let chars: Vec<char> = name.chars().collect();
    let mut out = String::with_capacity(name.len());

    for (idx, ch) in chars.iter().copied().enumerate() {
        if ch == '_' {
            out.push(ch);
            continue;
        }

        if ch.is_ascii_uppercase() {
            let prev = idx.checked_sub(1).and_then(|prev| chars.get(prev)).copied();
            let next = chars.get(idx + 1).copied();
            let prev_is_word =
                prev.is_some_and(|prev| prev.is_ascii_lowercase() || prev.is_ascii_digit());
            let acronym_boundary = prev.is_some_and(|prev| prev.is_ascii_uppercase())
                && next.is_some_and(|next| next.is_ascii_lowercase());

            if (prev_is_word || acronym_boundary) && !out.ends_with('_') {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_top_level_let_and_console_log() {
        let mut ctx = HirCtx::new();
        let module_id = to_hir(
            "let x = 6;
console.log(x);
",
            FileId(0),
            &mut ctx,
        )
        .expect("valid HIR");
        let module = &ctx.krate.modules[module_id.0 as usize];
        let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

        assert_eq!(body.locals.len(), 1);
        assert_eq!(body.stmts.len(), 2);
        assert_eq!(body.exprs.len(), 4);
        assert!(smelt_hir::validate(&ctx.krate).is_empty());
    }

    #[test]
    fn rejects_unknown_identifier() {
        let mut ctx = HirCtx::new();
        let errors = to_hir("console.log(x);", FileId(0), &mut ctx).expect_err("unknown x");
        assert_eq!(errors[0].code, "smelt::unsupported-ts");
        assert!(errors[0].message.contains("unresolved identifier"));
    }

    #[test]
    fn formats_compact_hir() {
        let mut ctx = HirCtx::new();
        let module_id = to_hir(
            "let count = 42;
console.log(count);
",
            FileId(0),
            &mut ctx,
        )
        .expect("valid HIR");

        let output = smelt_hir::format_compact(&ctx.krate, &[("sample.ts".to_owned(), module_id)]);

        assert_eq!(
            output,
            "module sample.ts (ModuleId(0))\n  body BodyId(0)\n  locals\n    %0 let count: Float\n  exprs\n    #0: Float = 42.0\n    #1: Float = %0\n    #2: None = @0(console_log)\n    #3: None = call #2(#1)\n  stmts\n    s0: let %0: Float = #0\n    s1: #3\n\ninterned types\n  t0 = Float\n  t1 = None\n"
        );
    }

    #[test]
    fn normalizes_camel_case() {
        assert_eq!(camel_to_snake("myFunction"), "my_function");
        assert_eq!(camel_to_snake("URLParser"), "url_parser");
        assert_eq!(camel_to_snake("IPAddr"), "ip_addr");
        assert_eq!(camel_to_snake("_internal"), "_internal");
    }

    #[test]
    fn lowers_function_declaration_and_direct_call() {
        let mut ctx = HirCtx::new();
        let module_id = to_hir(
            "function add(a: number, b: number): number {
  return a + b;
}
const result = add(2, 3);
console.log(result);
",
            FileId(0),
            &mut ctx,
        )
        .expect("valid HIR");
        let module = &ctx.krate.modules[module_id.0 as usize];

        assert_eq!(module.items.len(), 1);
        assert_eq!(ctx.krate.items.len(), 2);
        assert_eq!(ctx.krate.bodies.len(), 2);
        assert!(smelt_hir::validate(&ctx.krate).is_empty());
    }

    #[test]
    fn lowers_if_else_while_and_for_of_to_hir() {
        let mut ctx = HirCtx::new();
        let module_id = to_hir(
            "let count = 0;
if (count < 10) {
  console.log(count);
} else {
  console.log(count);
}
while (count < 10) {
  break;
}
for (let item: number of count) {
  continue;
}
",
            FileId(0),
            &mut ctx,
        )
        .expect("valid HIR");
        let module = &ctx.krate.modules[module_id.0 as usize];
        let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

        assert!(
            body.stmts
                .iter()
                .any(|stmt| matches!(stmt, Stmt::If { .. }))
        );
        assert!(
            body.stmts
                .iter()
                .any(|stmt| matches!(stmt, Stmt::While { .. }))
        );
        assert!(
            body.stmts
                .iter()
                .any(|stmt| matches!(stmt, Stmt::For { .. }))
        );
        assert!(smelt_hir::validate(&ctx.krate).is_empty());
    }

    #[test]
    fn lowers_try_catch_finally_to_hir() {
        let mut ctx = HirCtx::new();
        let module_id = to_hir(
            "try {
  throw 'x';
} catch (error) {
  console.log(error);
} finally {
  console.log('done');
}
",
            FileId(0),
            &mut ctx,
        )
        .expect("valid HIR");
        let module = &ctx.krate.modules[module_id.0 as usize];
        let body = &ctx.krate.bodies[module.body.expect("module body").0 as usize];

        let Some(Stmt::TryCatch {
            body: try_body,
            catch_binding: Some(_),
            catch_body: Some(catch_body),
            finally_body: Some(finally_body),
        }) = body
            .stmts
            .iter()
            .find(|stmt| matches!(stmt, Stmt::TryCatch { .. }))
        else {
            panic!("expected try/catch/finally to lower to HIR");
        };
        assert!(
            body.blocks[try_body.0 as usize]
                .stmts
                .iter()
                .any(|stmt| matches!(body.stmts[stmt.0 as usize], Stmt::Throw(_)))
        );
        assert!(!body.blocks[catch_body.0 as usize].stmts.is_empty());
        assert!(!body.blocks[finally_body.0 as usize].stmts.is_empty());
        assert!(smelt_hir::validate(&ctx.krate).is_empty());
    }

    #[test]
    fn rejects_missing_implemented_interface_field() {
        let mut ctx = HirCtx::new();
        let errors = to_hir(
            "interface Named { name: string; }
class User implements Named {
  constructor() {}
}
",
            FileId(0),
            &mut ctx,
        )
        .expect_err("missing field");
        assert_eq!(errors[0].code, "smelt::unsupported-ts");
        assert!(errors[0].span.end >= errors[0].span.start);
        assert!(errors[0].message.contains("field `name`"));
    }

    #[test]
    fn rejects_implemented_method_signature_mismatch() {
        let mut ctx = HirCtx::new();
        let errors = to_hir(
            "interface Named { label(prefix: string): string; }
class User implements Named {
  label(prefix: number): string { return \"x\"; }
}
",
            FileId(0),
            &mut ctx,
        )
        .expect_err("mismatch");
        assert_eq!(errors[0].code, "smelt::unsupported-ts");
        assert!(errors[0].span.end >= errors[0].span.start);
        assert!(errors[0].message.contains("mismatched signature"));
    }

    #[test]
    fn lowers_literal_switch_to_hir_match() {
        let mut ctx = HirCtx::new();
        let module_id = to_hir(
            "function label(status: \"pending\" | \"approved\" | \"rejected\"): string {
  switch (status) {
    case \"pending\":
      return \"Waiting\";
    case \"approved\":
      return \"Approved\";
    case \"rejected\":
      return \"Rejected\";
  }
}
const result = label(\"approved\");
console.log(result);
",
            FileId(0),
            &mut ctx,
        )
        .expect("valid HIR");
        let module = &ctx.krate.modules[module_id.0 as usize];
        let smelt_hir::Item::Function(function) = &ctx.krate.items[module.items[0].0 as usize]
        else {
            panic!("expected function item");
        };
        let body = &ctx.krate.bodies[function.body.expect("function body").0 as usize];

        let Some(Stmt::Match { arms, default, .. }) = body
            .stmts
            .iter()
            .find(|stmt| matches!(stmt, Stmt::Match { .. }))
        else {
            panic!("expected switch to lower to HIR match");
        };
        assert_eq!(arms.len(), 3);
        assert!(default.is_none());
        assert!(smelt_hir::validate(&ctx.krate).is_empty());
    }

    #[test]
    fn rejects_coercive_equality() {
        let mut ctx = HirCtx::new();
        let errors = to_hir(
            "function same(a: number, b: number): boolean {
  return a == b;
}
",
            FileId(0),
            &mut ctx,
        )
        .expect_err("coercive equality is unsupported");

        assert!(errors[0].message.contains("coercive equality"));
    }

    #[test]
    fn rejects_untyped_for_of_binding() {
        let mut ctx = HirCtx::new();
        let errors = to_hir(
            "let values = 1;
for (let item of values) {
  continue;
}
",
            FileId(0),
            &mut ctx,
        )
        .expect_err("for-of binding must be typed");

        assert!(errors[0].message.contains("explicit type annotations"));
    }

    #[test]
    fn rejects_async_functions_until_async_lowering_exists() {
        let mut ctx = HirCtx::new();
        let errors = to_hir(
            "async function load(): string {
  return \"done\";
}
",
            FileId(0),
            &mut ctx,
        )
        .expect_err("async functions are unsupported");

        assert!(errors[0].message.contains("async functions"));
    }

    #[test]
    fn rejects_switch_fallthrough_until_it_is_modeled() {
        let mut ctx = HirCtx::new();
        let errors = to_hir(
            "function label(status: \"pending\" | \"approved\"): string {
  switch (status) {
    case \"pending\":
      const waiting = \"waiting\";
    case \"approved\":
      return \"Approved\";
  }
}
",
            FileId(0),
            &mut ctx,
        )
        .expect_err("switch fallthrough is unsupported");

        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("switch fallthrough")),
            "expected switch fallthrough error, got {errors:?}"
        );
    }

    #[test]
    fn lowers_template_literal_to_string_concat() {
        let mut ctx = HirCtx::new();
        let _module_id = to_hir(
            "const name: string = \"world\";\nconst msg: string = `Hello ${name}!`;",
            FileId(0),
            &mut ctx,
        )
        .expect("template literal should lower");
        assert!(smelt_hir::validate(&ctx.krate).is_empty());
    }

    #[test]
    fn accepts_import_and_export_declarations() {
        let mut ctx = HirCtx::new();
        let _module_id = to_hir(
            "import { foo } from './foo';\nexport function bar(): number { return 1; }",
            FileId(0),
            &mut ctx,
        )
        .expect("import and export should not crash");
    }
}
