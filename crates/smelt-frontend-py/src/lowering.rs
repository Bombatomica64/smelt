//! Python AST to smelt HIR lowering.
#![allow(
    clippy::redundant_pub_crate,
    reason = "the crate root constructs the lowering builder"
)]

use std::collections::HashMap;

use ruff_python_ast::{
    BoolOp, CmpOp, ElifElseClause, Expr, ModModule, Number, Operator, Pattern as RuffPattern,
    PatternMatchAs, Singleton, Stmt, StmtAnnAssign, StmtAugAssign, StmtClassDef, StmtFor,
    StmtFunctionDef, StmtIf, StmtImport, StmtImportFrom, StmtMatch, UnaryOp as RuffUnaryOp,
};
use ruff_text_size::{Ranged, TextRange};
use smelt_hir::{
    AsyncOp, BinOp, Body, Class, ClassKind, Expr as HirExpr, ExprKind, Field, FileId, Function,
    FunctionOwner, FunctionType, Import, Item, ItemId, Language, Literal, LocalDecl, MatchArm,
    Module, ModuleId, Param, Pattern as HirPattern, SourceFile, Span, Stmt as HirStmt,
    StringCaseOp, Symbol, Type, TypeId, UnaryOp, Visibility,
};

use crate::helpers::{
    collect_bitor_parts, decorator_frozen_kwarg, decorator_simple_name, expr_kind_name,
    expr_simple_name, expr_type_name, is_django_model_base, range_to_span, stmt_kind_name,
    two_type_args,
};
use crate::{HirCtx, SmeltError};

pub(crate) struct ModuleBuilder<'ctx> {
    file_id: FileId,
    path: String,
    ctx: &'ctx mut HirCtx,
    /// In-scope locals while lowering a body.
    locals: HashMap<String, smelt_hir::LocalId>,
    /// Module-level items (functions / classes) for call resolution.
    items: HashMap<String, ItemId>,
    /// Whether the current lowered function body is async.
    current_async: bool,
}

impl<'ctx> ModuleBuilder<'ctx> {
    pub(crate) fn new(file_id: FileId, path: String, ctx: &'ctx mut HirCtx) -> Self {
        let items = visible_items(ctx);
        Self {
            file_id,
            path,
            ctx,
            locals: HashMap::new(),
            items,
            current_async: false,
        }
    }

    // -----------------------------------------------------------------------
    // Module — two-pass lowering
    // -----------------------------------------------------------------------

    pub(crate) fn module(&mut self, module: &ModModule) -> Result<ModuleId, Vec<SmeltError>> {
        let source = SourceFile {
            path: self.path.clone(),
            language: Language::Python,
        };
        let mut hir_module = Module::new("main", source);
        let module_span = self.span(module.range);
        let mut body = Body::new(None, module_span);
        let mut errors: Vec<SmeltError> = Vec::new();

        // Pass 1 — collect top-level function/class declarations so later
        // statements can reference them in calls.
        for stmt in &module.body {
            match stmt {
                Stmt::FunctionDef(func) => match self.function_def(func) {
                    Ok(item_id) => hir_module.items.push(item_id),
                    Err(err) => errors.push(err),
                },
                Stmt::ClassDef(class) => match self.class_def(class, &mut hir_module) {
                    Ok(_) => {}
                    Err(err) => errors.push(err),
                },
                Stmt::Import(import) => self.import_stmt(import, &mut hir_module),
                Stmt::ImportFrom(import) => self.import_from_stmt(import, &mut hir_module),
                _ => {}
            }
        }

        // Pass 2 — lower module-level statements into the module body.
        for stmt in &module.body {
            if matches!(
                stmt,
                Stmt::FunctionDef(_) | Stmt::ClassDef(_) | Stmt::Import(_) | Stmt::ImportFrom(_)
            ) {
                continue; // already lowered in Pass 1
            }
            if let Err(err) = self.statement(stmt, &mut body) {
                errors.push(err);
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let body_id = self.ctx.krate.push_body(body);
        hir_module.body = Some(body_id);
        Ok(self.ctx.krate.push_module(hir_module))
    }

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
        }
    }

    /// Lower a Python `from module import name [as alias]` statement to HIR metadata.
    fn import_from_stmt(&mut self, stmt: &StmtImportFrom, hir_module: &mut Module) {
        let span = self.span(stmt.range);
        let module = stmt
            .module
            .as_ref()
            .map_or_else(String::new, |module| module.as_str().to_owned());
        for alias in &stmt.names {
            let imported = alias.name.as_str();
            let local = alias
                .asname
                .as_ref()
                .map_or(imported, |identifier| identifier.as_str());
            let name = self.intern_name(imported);
            let alias_symbol = (local != imported).then(|| self.intern_name(local));
            hir_module.imports.push(Import {
                module: module.clone(),
                name,
                alias: alias_symbol,
                span,
            });
            self.alias_imported_item(imported, local);
        }
    }

    /// Add a local alias for an imported item when it is already known.
    fn alias_imported_item(&mut self, imported: &str, local: &str) {
        if let Some(item) = self.items.get(imported).copied() {
            self.items.insert(local.to_owned(), item);
        }
    }

    /// Lower a Python class definition to HIR, adding all methods and
    /// synthesising a constructor for dataclasses.
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
                    let base_expr = positional[0];
                    if is_django_model_base(base_expr) {
                        return Err(SmeltError::django_unsupported(span, class_name_str));
                    }
                    match expr_simple_name(base_expr) {
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
                other => {
                    return Err(SmeltError::unsupported(
                        self.span(other.range()),
                        format!(
                            "class '{class_name_str}': unsupported class body statement '{}'",
                            stmt_kind_name(other)
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
        let class_item_id = self.ctx.krate.push_item(class_item);
        self.items.insert(class_name_str.to_owned(), class_item_id);
        hir_module.items.push(class_item_id);

        Ok(())
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

        let return_ty = if is_init {
            self.intern_type(Type::None)
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

        // Add `self` local for use inside the method body.
        let self_sym = self.intern_name("self");
        let self_local = fn_body.push_local(LocalDecl {
            name: Some(self_sym),
            ty: class_ty,
            mutable: false,
            span,
        });
        self.locals.insert("self".to_owned(), self_local);

        let mut first = true;
        for param_with_default in func.parameters.iter_non_variadic_params() {
            let p = &param_with_default.parameter;
            let param_name_str = p.name.as_str();
            // Skip `self` — already added above.
            if first && param_name_str == "self" {
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
            body: Some(body_id),
            owner,
        });
        Ok(self.ctx.krate.push_item(item))
    }

    /// Synthesise an `__init__` method for a `@dataclass` class.
    ///
    /// Creates one parameter per annotated field and emits
    /// `self.field = param` assignments in the body.
    fn synthesize_dataclass_init(
        &mut self,
        class_sym: Symbol,
        class_ty: TypeId,
        fields: &[Field],
        span: Span,
    ) -> Result<ItemId, SmeltError> {
        let saved_locals = std::mem::take(&mut self.locals);
        let none_ty = self.intern_type(Type::None);
        let mut fn_body = Body::new(None, span);
        let mut params: Vec<Param> = Vec::new();

        // `self` local
        let self_sym = self.intern_name("self");
        let self_local = fn_body.push_local(LocalDecl {
            name: Some(self_sym),
            ty: class_ty,
            mutable: false,
            span,
        });
        self.locals.insert("self".to_owned(), self_local);

        // One param per field.
        for field in fields {
            let local = fn_body.push_local(LocalDecl {
                name: Some(field.name),
                ty: field.ty,
                mutable: false,
                span: field.span,
            });
            fn_body.params.push(local);
            let field_name_str = self
                .ctx
                .krate
                .symbols
                .get(field.name)
                .unwrap_or("")
                .to_owned();
            self.locals.insert(field_name_str, local);
            params.push(Param {
                name: field.name,
                local,
                ty: field.ty,
                span: field.span,
            });
        }

        // Body: `self.field = param` for each field.
        let root_block = fn_body.root;
        for (i, field) in fields.iter().enumerate() {
            let param_local = fn_body.params[i];
            let param_ty = field.ty;

            let self_expr = fn_body.push_expr(HirExpr {
                kind: ExprKind::Local(self_local),
                ty: class_ty,
                span,
            });
            let field_lhs = fn_body.push_expr(HirExpr {
                kind: ExprKind::Field {
                    receiver: self_expr,
                    field: field.name,
                },
                ty: param_ty,
                span: field.span,
            });
            let param_expr = fn_body.push_expr(HirExpr {
                kind: ExprKind::Local(param_local),
                ty: param_ty,
                span: field.span,
            });
            fn_body.push_stmt_to_block(
                root_block,
                HirStmt::Assign {
                    target: field_lhs,
                    value: param_expr,
                },
            );
        }

        self.locals = saved_locals;
        let body_id = self.ctx.krate.push_body(fn_body);
        let init_sym = self.intern_name("__init__");
        let item = Item::Function(Function {
            name: init_sym,
            span,
            params,
            return_ty: none_ty,
            is_async: false,
            body: Some(body_id),
            owner: FunctionOwner::Constructor { class: class_sym },
        });
        Ok(self.ctx.krate.push_item(item))
    }

    // -----------------------------------------------------------------------
    // Function definition
    // -----------------------------------------------------------------------

    fn function_def(&mut self, func: &StmtFunctionDef) -> Result<ItemId, SmeltError> {
        let name_str = func.name.as_str();
        let name = self.intern_name(name_str);

        // Return type — required.
        let annotated_return_ty = func
            .returns
            .as_deref()
            .ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(func.range),
                    format!("function '{name_str}' must have an explicit return type annotation"),
                )
            })
            .and_then(|ann| self.annotation_to_hir(ann))?;
        let return_ty = if func.is_async {
            self.future_type(annotated_return_ty)
        } else {
            annotated_return_ty
        };

        // Save outer scope and start fresh for this function's body.
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_async = self.current_async;
        self.current_async = func.is_async;
        let func_span = self.span(func.range);
        let mut fn_body = Body::new(None, func_span);
        let mut params: Vec<Param> = Vec::new();

        // Parameters — only positional/keyword args, no *args / **kwargs.
        for param_with_default in func.parameters.iter_non_variadic_params() {
            let p = &param_with_default.parameter;
            let param_name_str = p.name.as_str();

            let param_ty = p
                .annotation
                .as_deref()
                .ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(p.range),
                        format!(
                            "parameter '{param_name_str}' must have an explicit type annotation"
                        ),
                    )
                })
                .and_then(|ann| self.annotation_to_hir(ann))?;

            let param_name = self.intern_name(param_name_str);
            let local = fn_body.push_local(LocalDecl {
                name: Some(param_name),
                ty: param_ty,
                mutable: false,
                span: self.span(p.range),
            });
            fn_body.params.push(local);
            self.locals.insert(param_name_str.to_owned(), local);
            params.push(Param {
                name: param_name,
                local,
                ty: param_ty,
                span: self.span(p.range),
            });
        }

        if func.parameters.vararg.is_some() || func.parameters.kwarg.is_some() {
            self.locals = saved_locals;
            self.current_async = saved_async;
            return Err(SmeltError::unsupported(
                self.span(func.range),
                format!("function '{name_str}': *args and **kwargs are not yet supported"),
            ));
        }

        // Lower the body statements.
        let body_error = func
            .body
            .iter()
            .find_map(|stmt| self.statement(stmt, &mut fn_body).err());
        if func.is_async {
            fn_body.build_async_state_machine();
        }

        self.locals = saved_locals;
        self.current_async = saved_async;

        if let Some(err) = body_error {
            return Err(err);
        }

        let body_id = self.ctx.krate.push_body(fn_body);
        let item = Item::Function(Function {
            name,
            span: func_span,
            params,
            return_ty,
            is_async: func.is_async,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        });
        let item_id = self.ctx.krate.push_item(item);
        self.items.insert(name_str.to_owned(), item_id);
        Ok(item_id)
    }

    // -----------------------------------------------------------------------
    // Type annotation lowering
    // -----------------------------------------------------------------------

    /// Lower a Python type annotation expression to a HIR [`TypeId`].
    fn annotation_to_hir(&mut self, annotation: &Expr) -> Result<TypeId, SmeltError> {
        match annotation {
            Expr::Name(name) => self.name_annotation(name.id.as_str(), name.range),
            Expr::NoneLiteral(_) => Ok(self.intern_type(Type::None)),
            // `typing.Optional[T]`, `typing.Union[T, U]` etc.
            Expr::Attribute(attr) => self.name_annotation(attr.attr.as_str(), attr.range),
            Expr::Subscript(sub) => self.subscript_annotation(sub),
            // PEP 604: `T | U`
            Expr::BinOp(b) if b.op == Operator::BitOr => self.bitor_annotation(annotation),
            // Bare tuple in annotation position (e.g. inside Callable)
            Expr::Tuple(t) => {
                let items = t
                    .elts
                    .iter()
                    .map(|e| self.annotation_to_hir(e))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(self.intern_type(Type::Tuple(items)))
            }
            _ => Err(SmeltError::unsupported(
                self.span(annotation.range()),
                "unsupported type annotation form",
            )),
        }
    }

    /// Lower a bare name in annotation position (e.g. `int`, `str`, `MyClass`).
    fn name_annotation(&mut self, name: &str, range: TextRange) -> Result<TypeId, SmeltError> {
        let span = self.span(range);
        match name {
            "int" => Ok(self.intern_type(Type::Int)),
            "float" => Ok(self.intern_type(Type::Float)),
            "str" => Ok(self.intern_type(Type::String)),
            "bool" => Ok(self.intern_type(Type::Bool)),
            "None" | "NoneType" => Ok(self.intern_type(Type::None)),
            "object" => {
                // Top type — no exact HIR equivalent, map to opaque Class.
                let sym = self.intern_name("object");
                Ok(self.intern_type(Type::Class {
                    name: sym,
                    args: vec![],
                }))
            }
            // Bare generic names without args are an error.
            "Optional" | "Union" | "List" | "Dict" | "Set" | "Tuple" | "Callable" | "Awaitable" => {
                Err(SmeltError::unsupported(
                    span,
                    format!("'{name}' requires type arguments, e.g. {name}[T]"),
                ))
            }
            other => {
                // Unknown → assume a class type (will be resolved later).
                let sym = self.intern_name(other);
                Ok(self.intern_type(Type::Class {
                    name: sym,
                    args: vec![],
                }))
            }
        }
    }

    /// Lower a subscript annotation: `list[T]`, `Optional[T]`, `dict[K, V]`, …
    fn subscript_annotation(
        &mut self,
        sub: &ruff_python_ast::ExprSubscript,
    ) -> Result<TypeId, SmeltError> {
        let span = self.span(sub.range);
        let type_name = expr_type_name(&sub.value).unwrap_or("");

        match type_name {
            "list" | "List" => {
                let item = self.annotation_to_hir(&sub.slice)?;
                Ok(self.intern_type(Type::List(item)))
            }
            "set" | "Set" => {
                let item = self.annotation_to_hir(&sub.slice)?;
                Ok(self.intern_type(Type::Set(item)))
            }
            "dict" | "Dict" => {
                let (k, v) = two_type_args(&sub.slice, span)?;
                let key = self.annotation_to_hir(k)?;
                let val = self.annotation_to_hir(v)?;
                Ok(self.intern_type(Type::Dict(key, val)))
            }
            "tuple" | "Tuple" => {
                // `tuple[()]` is the empty tuple; otherwise lower each element.
                let items = match sub.slice.as_ref() {
                    Expr::Tuple(t) => t
                        .elts
                        .iter()
                        .map(|e| self.annotation_to_hir(e))
                        .collect::<Result<Vec<_>, _>>()?,
                    single => vec![self.annotation_to_hir(single)?],
                };
                Ok(self.intern_type(Type::Tuple(items)))
            }
            "Optional" => {
                let inner = self.annotation_to_hir(&sub.slice)?;
                Ok(self.intern_type(Type::Optional(inner)))
            }
            "Union" => {
                let types = match sub.slice.as_ref() {
                    Expr::Tuple(t) => t
                        .elts
                        .iter()
                        .map(|e| self.annotation_to_hir(e))
                        .collect::<Result<Vec<_>, _>>()?,
                    single => return self.annotation_to_hir(single),
                };
                self.union_from_types(types, span)
            }
            "Callable" => {
                // `Callable[[P1, P2], R]`
                let (param_list_expr, return_expr) = two_type_args(&sub.slice, span)?;
                let params = match param_list_expr {
                    Expr::List(l) => l
                        .elts
                        .iter()
                        .map(|e| self.annotation_to_hir(e))
                        .collect::<Result<Vec<_>, _>>()?,
                    _ => {
                        return Err(SmeltError::unsupported(
                            span,
                            "Callable first argument must be a list of param types, e.g. [int, str]",
                        ));
                    }
                };
                let return_ty = self.annotation_to_hir(return_expr)?;
                Ok(self.intern_type(Type::Function(FunctionType {
                    params,
                    return_ty,
                    is_async: false,
                })))
            }
            "Awaitable" | "Coroutine" => {
                let inner = self.annotation_to_hir(&sub.slice)?;
                Ok(self.intern_type(Type::Future(inner)))
            }
            _ => {
                // Generic class: `Foo[T, U]`
                let sym = self.intern_name(type_name);
                let args = match sub.slice.as_ref() {
                    Expr::Tuple(t) => t
                        .elts
                        .iter()
                        .map(|e| self.annotation_to_hir(e))
                        .collect::<Result<Vec<_>, _>>()?,
                    single => vec![self.annotation_to_hir(single)?],
                };
                Ok(self.intern_type(Type::Class { name: sym, args }))
            }
        }
    }

    /// Lower a PEP 604 `T | U | V` expression to Optional or Union.
    fn bitor_annotation(&mut self, expr: &Expr) -> Result<TypeId, SmeltError> {
        let span = self.span(expr.range());
        let mut parts: Vec<&Expr> = Vec::new();
        collect_bitor_parts(expr, &mut parts);
        let types = parts
            .iter()
            .map(|p| self.annotation_to_hir(p))
            .collect::<Result<Vec<_>, _>>()?;
        self.union_from_types(types, span)
    }

    /// Apply Optional vs Union logic — mirrors `ts_type_to_hir`'s union branch.
    fn union_from_types(
        &mut self,
        mut types: Vec<TypeId>,
        _span: Span,
    ) -> Result<TypeId, SmeltError> {
        let none_ty = self.intern_type(Type::None);
        let has_none = types.iter().any(|&t| t == none_ty);
        types.retain(|&t| t != none_ty);

        match (types.len(), has_none) {
            (0, _) => Ok(none_ty),
            (1, true) => Ok(self.intern_type(Type::Optional(types[0]))),
            (1, false) => Ok(types[0]),
            (_, true) => {
                types.push(none_ty);
                Ok(self.intern_type(Type::Union(types)))
            }
            (_, false) => Ok(self.intern_type(Type::Union(types))),
        }
    }

    // -----------------------------------------------------------------------
    // Statement lowering
    // -----------------------------------------------------------------------

    fn statement(&mut self, stmt: &Stmt, body: &mut Body) -> Result<(), SmeltError> {
        self.statement_in_block(stmt, body, body.root)
    }

    fn statement_in_block(
        &mut self,
        stmt: &Stmt,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        match stmt {
            // `x: T = value` — typed variable declaration.
            Stmt::AnnAssign(ann) => self.ann_assign(ann, body, block),

            // `x = value` — assignment to an already-declared local.
            Stmt::Assign(s) => {
                if s.targets.len() != 1 {
                    return Err(SmeltError::unsupported(
                        self.span(s.range),
                        "multiple assignment targets are not supported",
                    ));
                }
                if Self::is_destructuring_target(&s.targets[0]) {
                    let value = self.expression(&s.value, body)?;
                    let value_ty = body.exprs[value.0 as usize].ty;
                    let pat =
                        self.binding_pattern_from_target(&s.targets[0], body, Some(value_ty))?;
                    body.push_stmt_to_block(
                        block,
                        HirStmt::Let {
                            pat,
                            ty: value_ty,
                            value: Some(value),
                        },
                    );
                    return Ok(());
                }
                let target = self.expression(&s.targets[0], body)?;
                let value = self.expression(&s.value, body)?;
                body.push_stmt_to_block(block, HirStmt::Assign { target, value });
                Ok(())
            }

            // `x += value` — augmented assignment.
            Stmt::AugAssign(aug) => self.aug_assign(aug, body, block),

            // `return [value]`
            Stmt::Return(ret) => {
                let value = ret
                    .value
                    .as_deref()
                    .map(|v| self.expression(v, body))
                    .transpose()?;
                body.push_stmt_to_block(block, HirStmt::Return(value));
                Ok(())
            }

            // `if … elif … else …`
            Stmt::If(if_stmt) => self.if_statement(if_stmt, body, block),

            // `while test: …`
            Stmt::While(while_stmt) => {
                if !while_stmt.orelse.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(while_stmt.range),
                        "while-else is not supported",
                    ));
                }
                let cond = self.expression(&while_stmt.test, body)?;
                let loop_block = self.block_from_stmts(&while_stmt.body, body)?;
                body.push_stmt_to_block(
                    block,
                    HirStmt::While {
                        cond,
                        body: loop_block,
                    },
                );
                Ok(())
            }

            // `for target in iter: …`
            Stmt::For(for_stmt) => self.for_statement(for_stmt, body, block),

            // `match subject: …`
            Stmt::Match(match_stmt) => self.match_statement(match_stmt, body, block),

            // `raise ExceptionType(…)`
            Stmt::Raise(raise_stmt) => {
                let expr = raise_stmt
                    .exc
                    .as_deref()
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(raise_stmt.range),
                            "bare re-raise is not supported",
                        )
                    })
                    .and_then(|e| self.expression(e, body))?;
                body.push_stmt_to_block(block, HirStmt::Throw(expr));
                Ok(())
            }

            // Standalone expression statement (e.g. a function call).
            Stmt::Expr(s) => {
                let expr_id = self.expression(&s.value, body)?;
                body.push_stmt_to_block(block, HirStmt::Expr(expr_id));
                Ok(())
            }

            Stmt::Break(_) => {
                body.push_stmt_to_block(block, HirStmt::Break);
                Ok(())
            }
            Stmt::Continue(_) => {
                body.push_stmt_to_block(block, HirStmt::Continue);
                Ok(())
            }

            // `pass` — no HIR equivalent; silently skip.
            Stmt::Pass(_) => Ok(()),

            // Imports are collected at the module level in a future pass.
            Stmt::Import(_) | Stmt::ImportFrom(_) => Ok(()),

            // Function/class defs at non-module scope not supported yet.
            Stmt::FunctionDef(f) => Err(SmeltError::unsupported(
                self.span(f.range),
                "nested function definitions are not yet supported",
            )),
            Stmt::ClassDef(c) => Err(SmeltError::unsupported(
                self.span(c.range),
                "nested class definitions are not yet supported",
            )),

            other => Err(SmeltError::unsupported(
                self.span(other.range()),
                format!("unsupported statement: {}", stmt_kind_name(other)),
            )),
        }
    }

    /// `x: T [= value]` → `Stmt::Let { pat, ty, value }`.
    fn ann_assign(
        &mut self,
        ann: &StmtAnnAssign,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let Expr::Name(target_name) = ann.target.as_ref() else {
            return Err(SmeltError::unsupported(
                self.span(ann.range),
                "annotated assignment target must be a simple name",
            ));
        };

        let ty = self.annotation_to_hir(&ann.annotation)?;
        let value = ann
            .value
            .as_deref()
            .map(|v| self.expression(v, body))
            .transpose()?;

        let name_str = target_name.id.as_str();
        let name_sym = self.intern_name(name_str);
        let local = body.push_local(LocalDecl {
            name: Some(name_sym),
            ty,
            mutable: true,
            span: self.span(target_name.range),
        });
        self.locals.insert(name_str.to_owned(), local);

        let pat = body.push_pattern(HirPattern::Binding(local));
        body.push_stmt_to_block(block, HirStmt::Let { pat, ty, value });
        Ok(())
    }

    /// Return whether an assignment target needs pattern-based binding.
    fn is_destructuring_target(target: &Expr) -> bool {
        matches!(target, Expr::Tuple(_) | Expr::List(_))
    }

    /// Lower a Python binding target into a HIR pattern and declare its locals.
    fn binding_pattern_from_target(
        &mut self,
        target: &Expr,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<smelt_hir::PatternId, SmeltError> {
        match target {
            Expr::Name(target_name) => {
                let name_str = target_name.id.as_str();
                let name_sym = self.intern_name(name_str);
                let ty = type_hint.unwrap_or_else(|| self.intern_type(Type::None));
                let local = body.push_local(LocalDecl {
                    name: Some(name_sym),
                    ty,
                    mutable: true,
                    span: self.span(target_name.range),
                });
                self.locals.insert(name_str.to_owned(), local);
                Ok(body.push_pattern(HirPattern::Binding(local)))
            }
            Expr::Tuple(tuple) => {
                let patterns = tuple
                    .elts
                    .iter()
                    .enumerate()
                    .map(|(index, element)| {
                        let element_hint =
                            type_hint.and_then(|hint| self.tuple_element_type(hint, index));
                        self.binding_pattern_from_target(element, body, element_hint)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(body.push_pattern(HirPattern::Tuple(patterns)))
            }
            Expr::List(list) => {
                let patterns = list
                    .elts
                    .iter()
                    .enumerate()
                    .map(|(index, element)| {
                        let element_hint = type_hint.and_then(|hint| {
                            self.tuple_element_type(hint, index)
                                .or_else(|| self.list_element_type(hint))
                        });
                        self.binding_pattern_from_target(element, body, element_hint)
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(body.push_pattern(HirPattern::Tuple(patterns)))
            }
            other => Err(SmeltError::unsupported(
                self.span(other.range()),
                "destructuring targets may only contain names, tuples, or lists",
            )),
        }
    }

    /// `x op= value` → `Stmt::Assign { target, value: BinOp(target, op, rhs) }`.
    fn aug_assign(
        &mut self,
        aug: &StmtAugAssign,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let op = match aug.op {
            Operator::Add => BinOp::Add,
            Operator::Sub => BinOp::Sub,
            Operator::Mult => BinOp::Mul,
            Operator::Div => BinOp::Div,
            other => {
                return Err(SmeltError::unsupported(
                    self.span(aug.range),
                    format!("augmented assignment operator '{other}' is not supported"),
                ));
            }
        };

        let target = self.expression(&aug.target, body)?;
        let rhs = self.expression(&aug.value, body)?;

        // Determine the result type from the target expression's type.
        let lhs_ty = body.exprs[target.0 as usize].ty;
        let compound = body.push_expr(HirExpr {
            kind: ExprKind::BinOp {
                op,
                lhs: target,
                rhs,
            },
            ty: lhs_ty,
            span: self.span(aug.range),
        });
        body.push_stmt_to_block(
            block,
            HirStmt::Assign {
                target,
                value: compound,
            },
        );
        Ok(())
    }

    /// Lower an `if` with optional `elif`/`else` clauses.
    ///
    /// `elif` clauses are flattened into nested `Stmt::If` in new blocks,
    /// mirroring how the TS frontend handles chained conditionals.
    fn if_statement(
        &mut self,
        if_stmt: &StmtIf,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let cond = self.expression(&if_stmt.test, body)?;
        let then_block = self.block_from_stmts(&if_stmt.body, body)?;

        let else_block = self.elif_else_chain(&if_stmt.elif_else_clauses, body)?;

        body.push_stmt_to_block(
            block,
            HirStmt::If {
                cond,
                then_block,
                else_block,
            },
        );
        Ok(())
    }

    /// Recursively lower `elif`/`else` clauses into nested `If` blocks.
    fn elif_else_chain(
        &mut self,
        clauses: &[ElifElseClause],
        body: &mut Body,
    ) -> Result<Option<smelt_hir::BlockId>, SmeltError> {
        let Some(first) = clauses.first() else {
            return Ok(None);
        };

        let block = body.push_block(self.span(first.range));

        if let Some(test) = &first.test {
            // `elif` — recurse for the tail.
            let cond = self.expression(test, body)?;
            let then_block = self.block_from_stmts(&first.body, body)?;
            let else_block = self.elif_else_chain(&clauses[1..], body)?;
            body.push_stmt_to_block(
                block,
                HirStmt::If {
                    cond,
                    then_block,
                    else_block,
                },
            );
        } else {
            // `else` — just lower the body into the new block.
            for stmt in &first.body {
                self.statement_in_block(stmt, body, block)?;
            }
        }

        Ok(Some(block))
    }

    /// `for target in iter` — supports simple and tuple/list binding targets.
    fn for_statement(
        &mut self,
        for_stmt: &StmtFor,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        if for_stmt.is_async {
            return Err(SmeltError::unsupported(
                self.span(for_stmt.range),
                "async for is not supported",
            ));
        }
        if !for_stmt.orelse.is_empty() {
            return Err(SmeltError::unsupported(
                self.span(for_stmt.range),
                "for-else is not supported",
            ));
        }

        let iter = self.expression(&for_stmt.iter, body)?;
        let iter_ty = body.exprs[iter.0 as usize].ty;
        let item_ty = self
            .iter_item_type(iter_ty)
            .unwrap_or_else(|| self.intern_type(Type::None));
        let pat = self.binding_pattern_from_target(&for_stmt.target, body, Some(item_ty))?;
        let loop_block = self.block_from_stmts(&for_stmt.body, body)?;

        body.push_stmt_to_block(
            block,
            HirStmt::For {
                pat,
                iter,
                body: loop_block,
            },
        );
        Ok(())
    }

    /// `match subject: case …` — only literal / wildcard patterns.
    fn match_statement(
        &mut self,
        match_stmt: &StmtMatch,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let scrutinee = self.expression(&match_stmt.subject, body)?;
        let mut arms: Vec<MatchArm> = Vec::new();
        let mut default_block: Option<smelt_hir::BlockId> = None;

        for case in &match_stmt.cases {
            if case.guard.is_some() {
                return Err(SmeltError::unsupported(
                    self.span(case.range),
                    "match guards are not supported",
                ));
            }

            match &case.pattern {
                // `case None:` / `case True:` / `case False:`
                RuffPattern::MatchSingleton(s) => {
                    let label = match s.value {
                        Singleton::None => Literal::None,
                        Singleton::True => Literal::Bool(true),
                        Singleton::False => Literal::Bool(false),
                    };
                    let arm_block = self.block_from_stmts(&case.body, body)?;
                    arms.push(MatchArm {
                        label,
                        body: arm_block,
                    });
                }

                // `case <literal>:`
                RuffPattern::MatchValue(mv) => {
                    let label = self.match_value_literal(&mv.value)?;
                    let arm_block = self.block_from_stmts(&case.body, body)?;
                    arms.push(MatchArm {
                        label,
                        body: arm_block,
                    });
                }

                // `case _:` — wildcard / default
                RuffPattern::MatchAs(PatternMatchAs {
                    pattern: None,
                    name: None,
                    ..
                }) => {
                    if default_block.is_some() {
                        return Err(SmeltError::unsupported(
                            self.span(case.range),
                            "match statement has more than one default (wildcard) case",
                        ));
                    }
                    default_block = Some(self.block_from_stmts(&case.body, body)?);
                }

                other => {
                    return Err(SmeltError::unsupported(
                        self.span(other.range()),
                        "only literal and wildcard match patterns are supported",
                    ));
                }
            }
        }

        body.push_stmt_to_block(
            block,
            HirStmt::Match {
                scrutinee,
                arms,
                default: default_block,
            },
        );
        Ok(())
    }

    /// Extract a `Literal` from the value expression inside a `case` arm.
    fn match_value_literal(&self, expr: &Expr) -> Result<Literal, SmeltError> {
        match expr {
            Expr::NumberLiteral(n) => match &n.value {
                Number::Int(i) => i.as_i64().map(Literal::Int).ok_or_else(|| {
                    SmeltError::unsupported(self.span(n.range), "integer literal out of i64 range")
                }),
                Number::Float(f) => Ok(Literal::Float(*f)),
                Number::Complex { .. } => Err(SmeltError::unsupported(
                    self.span(n.range),
                    "complex number literals are not supported in match patterns",
                )),
            },
            Expr::StringLiteral(s) => Ok(Literal::String(s.value.to_str().to_owned())),
            Expr::BooleanLiteral(b) => Ok(Literal::Bool(b.value)),
            Expr::NoneLiteral(_) => Ok(Literal::None),
            // Negative literal: `-42`
            Expr::UnaryOp(u) if u.op == RuffUnaryOp::USub => {
                if let Expr::NumberLiteral(n) = u.operand.as_ref() {
                    match &n.value {
                        Number::Int(i) => i.as_i64().map(|v| Literal::Int(-v)).ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(n.range),
                                "integer literal out of i64 range",
                            )
                        }),
                        Number::Float(f) => Ok(Literal::Float(-f)),
                        Number::Complex { .. } => Err(SmeltError::unsupported(
                            self.span(u.range),
                            "complex number literals are not supported",
                        )),
                    }
                } else {
                    Err(SmeltError::unsupported(
                        self.span(u.range),
                        "only literal values are supported in match patterns",
                    ))
                }
            }
            _ => Err(SmeltError::unsupported(
                self.span(expr.range()),
                "only literal values are supported in match patterns",
            )),
        }
    }

    /// Lower a slice of statements into a fresh block.
    fn block_from_stmts(
        &mut self,
        stmts: &[Stmt],
        body: &mut Body,
    ) -> Result<smelt_hir::BlockId, SmeltError> {
        let span = stmts
            .first()
            .map_or_else(|| Span::new(self.file_id, 0, 0), |s| self.span(s.range()));
        let block = body.push_block(span);
        for stmt in stmts {
            self.statement_in_block(stmt, body, block)?;
        }
        Ok(block)
    }

    // -----------------------------------------------------------------------
    // Expression lowering
    // -----------------------------------------------------------------------

    fn expression(
        &mut self,
        expr: &Expr,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.expression_with_hint(expr, body, None)
    }

    fn expression_with_hint(
        &mut self,
        expr: &Expr,
        body: &mut Body,
        type_hint: Option<TypeId>,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        match expr {
            // --- Literals ---
            Expr::NumberLiteral(n) => {
                let (kind, ty) = match &n.value {
                    Number::Int(i) => {
                        let v = i.as_i64().ok_or_else(|| {
                            SmeltError::unsupported(
                                self.span(n.range),
                                "integer literal out of i64 range",
                            )
                        })?;
                        (
                            ExprKind::Literal(Literal::Int(v)),
                            self.intern_type(Type::Int),
                        )
                    }
                    Number::Float(f) => (
                        ExprKind::Literal(Literal::Float(*f)),
                        self.intern_type(Type::Float),
                    ),
                    Number::Complex { .. } => {
                        return Err(SmeltError::unsupported(
                            self.span(n.range),
                            "complex number literals are not supported",
                        ));
                    }
                };
                Ok(body.push_expr(HirExpr {
                    kind,
                    ty,
                    span: self.span(n.range),
                }))
            }

            Expr::StringLiteral(s) => {
                let ty = self.intern_type(Type::String);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::String(s.value.to_str().to_owned())),
                    ty,
                    span: self.span(s.range),
                }))
            }

            Expr::BooleanLiteral(b) => {
                let ty = self.intern_type(Type::Bool);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::Bool(b.value)),
                    ty,
                    span: self.span(b.range),
                }))
            }

            Expr::NoneLiteral(n) => {
                let ty = self.intern_type(Type::None);
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Literal(Literal::None),
                    ty,
                    span: self.span(n.range),
                }))
            }

            // --- Name lookup ---
            Expr::Name(name) => self.identifier_expression(name.id.as_str(), name.range, body),

            // --- Binary / boolean / comparison operators ---
            Expr::BinOp(b) => self.binop_expression(b, body),
            Expr::BoolOp(b) => self.boolop_expression(b, body),
            Expr::Compare(c) => self.compare_expression(c, body),

            // --- Unary operators ---
            Expr::UnaryOp(u) => self.unary_expression(u, body),

            // --- Calls ---
            Expr::Call(call) => self.call_expression(call, body),

            // --- Await ---
            Expr::Await(await_expr) => {
                if !self.current_async {
                    return Err(SmeltError::unsupported(
                        self.span(await_expr.range),
                        "await expressions are only supported inside async functions",
                    ));
                }
                let awaited = self.expression(&await_expr.value, body)?;
                let awaited_ty = body.exprs[awaited.0 as usize].ty;
                let ty = self.future_inner_type(awaited_ty).ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(await_expr.range),
                        "await expressions require an Awaitable[T] operand",
                    )
                })?;
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Await(awaited),
                    ty,
                    span: self.span(await_expr.range),
                }))
            }

            // --- Attribute access: `obj.field` ---
            Expr::Attribute(attr) => {
                let receiver = self.expression(&attr.value, body)?;
                let receiver_ty = body.exprs[receiver.0 as usize].ty;
                let field_ty = self.field_type(receiver_ty)?;
                let field = self.intern_name(attr.attr.as_str());
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Field { receiver, field },
                    ty: field_ty,
                    span: self.span(attr.range),
                }))
            }

            // --- Subscript: `obj[index]` ---
            Expr::Subscript(sub) => {
                let receiver = self.expression(&sub.value, body)?;
                let receiver_ty = body.exprs[receiver.0 as usize].ty;
                let index_ty = self.index_type(receiver_ty)?;
                let index = self.expression(&sub.slice, body)?;
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Index { receiver, index },
                    ty: index_ty,
                    span: self.span(sub.range),
                }))
            }

            // --- Collection literals ---
            Expr::List(l) => {
                let elts: Vec<_> = l
                    .elts
                    .iter()
                    .map(|e| self.expression(e, body))
                    .collect::<Result<_, _>>()?;
                // Infer element type from hint or first element.
                let ty = match type_hint {
                    Some(h) => h,
                    None => match elts.first().copied() {
                        Some(first_id) => {
                            let elem_ty = body.exprs[first_id.0 as usize].ty;
                            self.intern_type(Type::List(elem_ty))
                        }
                        None => {
                            let none = self.intern_type(Type::None);
                            self.intern_type(Type::List(none))
                        }
                    },
                };
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::ListLit(elts),
                    ty,
                    span: self.span(l.range),
                }))
            }

            Expr::Tuple(t) => {
                let elts: Vec<_> = t
                    .elts
                    .iter()
                    .map(|e| self.expression(e, body))
                    .collect::<Result<_, _>>()?;
                let elem_types: Vec<TypeId> = elts
                    .iter()
                    .map(|&id| body.exprs[id.0 as usize].ty)
                    .collect();
                let ty = self.intern_type(Type::Tuple(elem_types));
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::TupleLit(elts),
                    ty,
                    span: self.span(t.range),
                }))
            }

            Expr::Set(s) => {
                let elts: Vec<_> = s
                    .elts
                    .iter()
                    .map(|e| self.expression(e, body))
                    .collect::<Result<_, _>>()?;
                let ty = match type_hint {
                    Some(h) => h,
                    None => match elts.first().copied() {
                        Some(first_id) => {
                            let elem_ty = body.exprs[first_id.0 as usize].ty;
                            self.intern_type(Type::Set(elem_ty))
                        }
                        None => {
                            let none = self.intern_type(Type::None);
                            self.intern_type(Type::Set(none))
                        }
                    },
                };
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::SetLit(elts),
                    ty,
                    span: self.span(s.range),
                }))
            }

            Expr::Dict(d) => {
                let mut entries: Vec<(smelt_hir::ExprId, smelt_hir::ExprId)> = Vec::new();
                for item in &d.items {
                    let key_expr = item.key.as_ref().ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(d.range),
                            "dictionary unpacking (**dict) is not supported",
                        )
                    })?;
                    let key = self.expression(key_expr, body)?;
                    let val = self.expression(&item.value, body)?;
                    entries.push((key, val));
                }
                let ty = match type_hint {
                    Some(h) => h,
                    None => match entries.first().copied() {
                        Some((k_id, v_id)) => {
                            let k_ty = body.exprs[k_id.0 as usize].ty;
                            let v_ty = body.exprs[v_id.0 as usize].ty;
                            self.intern_type(Type::Dict(k_ty, v_ty))
                        }
                        None => {
                            let none = self.intern_type(Type::None);
                            self.intern_type(Type::Dict(none, none))
                        }
                    },
                };
                Ok(body.push_expr(HirExpr {
                    kind: ExprKind::DictLit(entries),
                    ty,
                    span: self.span(d.range),
                }))
            }

            other => Err(SmeltError::unsupported(
                self.span(other.range()),
                format!("unsupported expression: {}", expr_kind_name(other)),
            )),
        }
    }

    /// Lower a binary arithmetic/comparison operator expression.
    fn binop_expression(
        &mut self,
        b: &ruff_python_ast::ExprBinOp,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(b.range);
        let (op, result_is_bool) = match b.op {
            Operator::Add => (BinOp::Add, false),
            Operator::Sub => (BinOp::Sub, false),
            Operator::Mult => (BinOp::Mul, false),
            Operator::Div => (BinOp::Div, false),
            other => {
                return Err(SmeltError::unsupported(
                    span,
                    format!("binary operator '{other}' is not supported"),
                ));
            }
        };
        let lhs = self.expression(&b.left, body)?;
        let rhs = self.expression(&b.right, body)?;
        let ty = if result_is_bool {
            self.intern_type(Type::Bool)
        } else {
            body.exprs[lhs.0 as usize].ty
        };
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty,
            span,
        }))
    }

    /// Lower a boolean operator — fold `a and b and c` into left-associative pairs.
    fn boolop_expression(
        &mut self,
        b: &ruff_python_ast::ExprBoolOp,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let op = match b.op {
            BoolOp::And => BinOp::And,
            BoolOp::Or => BinOp::Or,
        };
        let bool_ty = self.intern_type(Type::Bool);

        // Left-fold: ((a op b) op c)
        let mut acc = self.expression(&b.values[0], body)?;
        for value in &b.values[1..] {
            let rhs = self.expression(value, body)?;
            let span = Span::new(
                self.file_id,
                body.exprs[acc.0 as usize].span.start,
                body.exprs[rhs.0 as usize].span.end,
            );
            acc = body.push_expr(HirExpr {
                kind: ExprKind::BinOp { op, lhs: acc, rhs },
                ty: bool_ty,
                span,
            });
        }
        Ok(acc)
    }

    /// Lower a comparison expression.  Only single-op, non-chained comparisons.
    fn compare_expression(
        &mut self,
        c: &ruff_python_ast::ExprCompare,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(c.range);
        if c.ops.len() != 1 || c.comparators.len() != 1 {
            return Err(SmeltError::unsupported(
                span,
                "chained comparisons (e.g. a < b < c) are not supported",
            ));
        }
        let op = match c.ops[0] {
            CmpOp::Eq => BinOp::Eq,
            CmpOp::NotEq => BinOp::NotEq,
            CmpOp::Lt => BinOp::Lt,
            CmpOp::LtE => BinOp::Lte,
            CmpOp::Gt => BinOp::Gt,
            CmpOp::GtE => BinOp::Gte,
            other => {
                return Err(SmeltError::unsupported(
                    span,
                    format!("comparison operator '{other}' is not supported"),
                ));
            }
        };
        let lhs = self.expression(&c.left, body)?;
        let rhs = self.expression(&c.comparators[0], body)?;
        let ty = self.intern_type(Type::Bool);
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::BinOp { op, lhs, rhs },
            ty,
            span,
        }))
    }

    /// Lower a unary expression.
    fn unary_expression(
        &mut self,
        u: &ruff_python_ast::ExprUnaryOp,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(u.range);
        let (op, result_is_bool) = match u.op {
            RuffUnaryOp::Not => (UnaryOp::Not, true),
            RuffUnaryOp::USub => (UnaryOp::Neg, false),
            other => {
                return Err(SmeltError::unsupported(
                    span,
                    format!("unary operator '{other}' is not supported"),
                ));
            }
        };
        let operand = self.expression(&u.operand, body)?;
        let ty = if result_is_bool {
            self.intern_type(Type::Bool)
        } else {
            body.exprs[operand.0 as usize].ty
        };
        Ok(body.push_expr(HirExpr {
            kind: ExprKind::UnaryOp { op, operand },
            ty,
            span,
        }))
    }

    /// Lower a call expression.
    fn call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(call.range);

        if let Some(expr) = self.asyncio_call_expression(call, body)? {
            return Ok(expr);
        }
        if let Some(expr) = self.string_case_call_expression(call, body)? {
            return Ok(expr);
        }

        // `print(...)` → CONSOLE_LOG_SYMBOL item (same as TS's `console.log`).
        if let Expr::Name(name) = call.func.as_ref() {
            if name.id.as_str() == "print" {
                let print_item = self.ensure_print_item(span);
                let none_ty = self.intern_type(Type::None);
                let callee = body.push_expr(HirExpr {
                    kind: ExprKind::Item(print_item),
                    ty: none_ty,
                    span,
                });
                let args: Vec<_> = call
                    .arguments
                    .args
                    .iter()
                    .map(|a| self.expression(a, body))
                    .collect::<Result<_, _>>()?;
                return Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Call { callee, args },
                    ty: none_ty,
                    span,
                }));
            }
            if name.id.as_str() == "len" {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "len() requires exactly one argument",
                    ));
                }
                let operand = self.expression(&call.arguments.args[0], body)?;
                let operand_ty = body.exprs[operand.0 as usize].ty;
                if !self.supports_stdlib_len(operand_ty) {
                    return Err(SmeltError::unsupported(
                        span,
                        "len() is only supported for list, dict, tuple, and str values",
                    ));
                }
                let ty = self.intern_type(Type::Int);
                return Ok(body.push_expr(HirExpr {
                    kind: ExprKind::Len { operand },
                    ty,
                    span,
                }));
            }
        }

        // Named function call OR class constructor call.
        if let Expr::Name(name) = call.func.as_ref() {
            let name_str = name.id.as_str();
            if let Some(&item_id) = self.items.get(name_str) {
                let item = &self.ctx.krate.items[item_id.0 as usize];
                match item {
                    Item::Class(c) => {
                        // Constructor call: `MyClass(args...)`
                        let class_sym = c.name;
                        let class_ty = self.intern_type(Type::Class {
                            name: class_sym,
                            args: vec![],
                        });
                        let args: Vec<_> = call
                            .arguments
                            .args
                            .iter()
                            .map(|a| self.expression(a, body))
                            .collect::<Result<_, _>>()?;
                        return Ok(body.push_expr(HirExpr {
                            kind: ExprKind::New {
                                class: class_sym,
                                args,
                            },
                            ty: class_ty,
                            span,
                        }));
                    }
                    Item::Function(f) => {
                        let return_ty = f.return_ty;
                        let callee = body.push_expr(HirExpr {
                            kind: ExprKind::Item(item_id),
                            ty: return_ty,
                            span,
                        });
                        let args: Vec<_> = call
                            .arguments
                            .args
                            .iter()
                            .map(|a| self.expression(a, body))
                            .collect::<Result<_, _>>()?;
                        return Ok(body.push_expr(HirExpr {
                            kind: ExprKind::Call { callee, args },
                            ty: return_ty,
                            span,
                        }));
                    }
                    _ => {}
                }
            }
        }

        Err(SmeltError::unsupported(
            span,
            "only calls to top-level functions, class constructors, and print() are supported",
        ))
    }

    /// Lower direct Python string case methods.
    fn string_case_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let op = match attr.attr.as_str() {
            "lower" => StringCaseOp::Lower,
            "upper" => StringCaseOp::Upper,
            _ => return Ok(None),
        };
        let span = self.span(call.range);
        if !call.arguments.args.is_empty() {
            return Err(SmeltError::unsupported(
                span,
                "string case methods do not take arguments",
            ));
        }
        let operand = self.expression(&attr.value, body)?;
        let operand_ty = body.exprs[operand.0 as usize].ty;
        if self.ctx.krate.types.get(operand_ty) != Some(&Type::String) {
            return Err(SmeltError::unsupported(
                span,
                "string case methods require a str receiver",
            ));
        }
        let ty = self.intern_type(Type::String);
        Ok(Some(body.push_expr(HirExpr {
            kind: ExprKind::StringCase { op, operand },
            ty,
            span,
        })))
    }

    /// Returns true when Python `len(...)` can lower directly to Rust `.len()`.
    fn supports_stdlib_len(&self, ty: TypeId) -> bool {
        matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::List(_) | Type::Dict(_, _) | Type::Tuple(_) | Type::String)
        )
    }

    /// Lower supported `asyncio.*` calls into shared async runtime operations.
    fn asyncio_call_expression(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
    ) -> Result<Option<smelt_hir::ExprId>, SmeltError> {
        let Expr::Attribute(attr) = call.func.as_ref() else {
            return Ok(None);
        };
        let Expr::Name(module) = attr.value.as_ref() else {
            return Ok(None);
        };
        if module.id.as_str() != "asyncio" {
            return Ok(None);
        }

        let span = self.span(call.range);
        match smelt_asyncio::classify_attr(attr.attr.as_str()) {
            smelt_asyncio::AsyncioApi::Gather => {
                let args = call
                    .arguments
                    .args
                    .iter()
                    .map(|arg| self.expression(arg, body))
                    .collect::<Result<Vec<_>, _>>()?;
                let outputs = args
                    .iter()
                    .map(|arg| {
                        self.future_inner_type(body.exprs[arg.0 as usize].ty)
                            .ok_or_else(|| {
                                SmeltError::unsupported(
                                    span,
                                    "asyncio.gather arguments must be awaitable",
                                )
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let output_ty = self.intern_type(Type::Tuple(outputs));
                let ty = self.intern_type(Type::Future(output_ty));
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::All,
                        args,
                    },
                    ty,
                    span,
                })))
            }
            smelt_asyncio::AsyncioApi::Sleep => {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "asyncio.sleep requires exactly one duration argument",
                    ));
                }
                let duration = self.expression(&call.arguments.args[0], body)?;
                let none_ty = self.intern_type(Type::None);
                let ty = self.intern_type(Type::Future(none_ty));
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::Sleep,
                        args: vec![duration],
                    },
                    ty,
                    span,
                })))
            }
            smelt_asyncio::AsyncioApi::CreateTask => {
                if call.arguments.args.len() != 1 {
                    return Err(SmeltError::unsupported(
                        span,
                        "asyncio.create_task requires exactly one awaitable argument",
                    ));
                }
                let future = self.expression(&call.arguments.args[0], body)?;
                let inner = self
                    .future_inner_type(body.exprs[future.0 as usize].ty)
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            span,
                            "asyncio.create_task argument must be awaitable",
                        )
                    })?;
                let ty = self.intern_type(Type::Future(inner));
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::CreateTask,
                        args: vec![future],
                    },
                    ty,
                    span,
                })))
            }
            smelt_asyncio::AsyncioApi::WaitFor => {
                if call.arguments.args.len() < 2 {
                    return Err(SmeltError::unsupported(
                        span,
                        "asyncio.wait_for requires an awaitable and timeout",
                    ));
                }
                let future = self.expression(&call.arguments.args[0], body)?;
                let timeout = self.expression(&call.arguments.args[1], body)?;
                let inner = self
                    .future_inner_type(body.exprs[future.0 as usize].ty)
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            span,
                            "asyncio.wait_for first argument must be awaitable",
                        )
                    })?;
                let ty = self.intern_type(Type::Future(inner));
                Ok(Some(body.push_expr(HirExpr {
                    kind: ExprKind::AsyncOp {
                        op: AsyncOp::WaitFor,
                        args: vec![future, timeout],
                    },
                    ty,
                    span,
                })))
            }
            smelt_asyncio::AsyncioApi::Queue | smelt_asyncio::AsyncioApi::Lock => {
                Err(SmeltError::unsupported(
                    span,
                    format!(
                        "asyncio.{} needs runtime object support and is not lowered in this slice",
                        attr.attr
                    ),
                ))
            }
            smelt_asyncio::AsyncioApi::LowerLevelLoop => Err(SmeltError::unsupported(
                span,
                format!(
                    "asyncio.{} is a lower-level event-loop API and is not supported",
                    attr.attr
                ),
            )),
            smelt_asyncio::AsyncioApi::Unknown => Err(SmeltError::unsupported(
                span,
                format!("asyncio.{} is not lowered yet", attr.attr),
            )),
        }
    }

    /// Resolve a name to a local variable or module-level item.
    fn identifier_expression(
        &mut self,
        name: &str,
        range: TextRange,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(range);

        if let Some(&local) = self.locals.get(name) {
            let ty = body.locals[local.0 as usize].ty;
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::Local(local),
                ty,
                span,
            }));
        }

        if let Some(&item_id) = self.items.get(name) {
            let ty = match &self.ctx.krate.items[item_id.0 as usize] {
                Item::Function(f) => f.return_ty,
                Item::Class(c) => {
                    let sym = c.name;
                    self.intern_type(Type::Class {
                        name: sym,
                        args: vec![],
                    })
                }
                _ => self.intern_type(Type::None),
            };
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::Item(item_id),
                ty,
                span,
            }));
        }

        Err(SmeltError::unsupported(
            span,
            format!("unresolved name '{name}'"),
        ))
    }

    // -----------------------------------------------------------------------
    // Type helpers
    // -----------------------------------------------------------------------

    /// Infer the element type of a field access on `receiver_ty`.
    fn field_type(&self, receiver_ty: TypeId) -> Result<TypeId, SmeltError> {
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::Class { .. }) => {
                // Field types on classes will be resolved when class lowering lands.
                Ok(receiver_ty)
            }
            _ => Err(SmeltError::unsupported(
                Span::new(self.file_id, 0, 0),
                "attribute access is only supported on class instances",
            )),
        }
    }

    /// Infer the element type of an index access on `receiver_ty`.
    fn index_type(&self, receiver_ty: TypeId) -> Result<TypeId, SmeltError> {
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::List(elem)) | Some(Type::Set(elem)) => Ok(*elem),
            Some(Type::Dict(_, val)) => Ok(*val),
            Some(Type::Tuple(items)) => items.first().copied().ok_or_else(|| {
                SmeltError::unsupported(
                    Span::new(self.file_id, 0, 0),
                    "cannot index an empty tuple",
                )
            }),
            _ => Err(SmeltError::unsupported(
                Span::new(self.file_id, 0, 0),
                "subscript access requires a list, set, dict, or tuple",
            )),
        }
    }

    /// Infer one item type from a Python iterable type.
    fn iter_item_type(&self, iter_ty: TypeId) -> Option<TypeId> {
        match self.ctx.krate.types.get(iter_ty) {
            Some(Type::List(elem) | Type::Set(elem)) => Some(*elem),
            Some(Type::Dict(key, _)) => Some(*key),
            Some(Type::Tuple(items)) => items.first().copied(),
            Some(Type::String) => Some(iter_ty),
            _ => None,
        }
    }

    /// Return the indexed element type for fixed-size tuple hints.
    fn tuple_element_type(&self, tuple_ty: TypeId, index: usize) -> Option<TypeId> {
        match self.ctx.krate.types.get(tuple_ty) {
            Some(Type::Tuple(items)) => items.get(index).copied(),
            _ => None,
        }
    }

    /// Return the repeated element type for list hints.
    fn list_element_type(&self, list_ty: TypeId) -> Option<TypeId> {
        match self.ctx.krate.types.get(list_ty) {
            Some(Type::List(elem) | Type::Set(elem)) => Some(*elem),
            _ => None,
        }
    }

    /// Wrap a Python async return annotation in the shared HIR future type.
    fn future_type(&mut self, inner: TypeId) -> TypeId {
        match self.ctx.krate.types.get(inner) {
            Some(Type::Future(_)) => inner,
            _ => self.intern_type(Type::Future(inner)),
        }
    }

    /// Extract the output type from a HIR future type.
    fn future_inner_type(&self, ty: TypeId) -> Option<TypeId> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Future(inner)) => Some(*inner),
            _ => None,
        }
    }

    // -----------------------------------------------------------------------
    // Built-in items
    // -----------------------------------------------------------------------

    /// Ensure the `print` built-in item exists in the crate and return its id.
    fn ensure_print_item(&mut self, span: Span) -> ItemId {
        if let Some(&id) = self.items.get(smelt_hir::CONSOLE_LOG_SYMBOL) {
            return id;
        }
        let name = self.intern_name(smelt_hir::CONSOLE_LOG_SYMBOL);
        let none_ty = self.intern_type(Type::None);
        let item = Item::Function(Function {
            name,
            span,
            params: Vec::new(),
            return_ty: none_ty,
            is_async: false,
            body: None,
            owner: FunctionOwner::Module,
        });
        let id = self.ctx.krate.push_item(item);
        self.items
            .insert(smelt_hir::CONSOLE_LOG_SYMBOL.to_owned(), id);
        id
    }

    // -----------------------------------------------------------------------
    // Interning helpers
    // -----------------------------------------------------------------------

    fn intern_name(&mut self, name: &str) -> Symbol {
        self.ctx.krate.symbols.intern(name)
    }

    fn intern_type(&mut self, ty: Type) -> TypeId {
        self.ctx.krate.types.intern(ty)
    }

    // -----------------------------------------------------------------------
    // Span helpers
    // -----------------------------------------------------------------------

    fn span(&self, range: TextRange) -> Span {
        range_to_span(self.file_id, range)
    }
}

/// Collect items already present in the shared crate for cross-module references.
fn visible_items(ctx: &HirCtx) -> HashMap<String, ItemId> {
    let mut items = HashMap::new();
    for (idx, item) in ctx.krate.items.iter().enumerate() {
        let Ok(item_idx) = u32::try_from(idx) else {
            continue;
        };
        let item_id = ItemId(item_idx);
        let Some(name) = item_name(&ctx.krate, item) else {
            continue;
        };
        items.insert(name.to_owned(), item_id);
    }
    items
}

/// Return an item's source name when available.
fn item_name<'a>(krate: &'a smelt_hir::Crate, hir_item: &Item) -> Option<&'a str> {
    let symbol = match hir_item {
        Item::Function(function) => function.name,
        Item::Class(class) => class.name,
        Item::Interface(interface) => interface.name,
        Item::TypeAlias(alias) => alias.name,
        Item::Const(const_item) => const_item.name,
    };
    krate.symbols.get(symbol)
}
