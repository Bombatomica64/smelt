//! Python frontend: source → Ruff AST → smelt HIR.
//!
//! Mirrors the structure of `smelt-frontend-ts`. Parsing is handled by
//! Astral's `ruff_python_parser`; lowering walks `ruff_python_ast` nodes and
//! produces nodes in `smelt-hir`.
//!
//! ## Design notes
//! * Type annotations in Python are `Expr` nodes in annotation position (not a
//!   separate grammar), so `annotation_to_hir` pattern-matches on `Expr` shape.
//! * `print(...)` is mapped to the same `CONSOLE_LOG_SYMBOL` item as TS's
//!   `console.log(...)` — both compile down to `println!` in codegen.
//! * Strict annotation policy mirrors TS: function params and return types must
//!   have explicit type hints; new local variables require annotated assignment
//!   (`x: int = 5`), bare assignment (`x = 5`) is only allowed to an already-
//!   declared local.

pub use ruff_python_ast as ast;

use std::collections::HashMap;

use ruff_python_ast::{
    BoolOp, CmpOp, ElifElseClause, Expr, Mod, ModModule, Number, Operator,
    Pattern as RuffPattern, PatternMatchAs, Singleton, Stmt, StmtAnnAssign, StmtAugAssign,
    StmtFor, StmtFunctionDef, StmtIf, StmtMatch, UnaryOp as RuffUnaryOp,
};
use ruff_python_parser::{Mode, ParseOptions, parse};
use ruff_text_size::{Ranged, TextRange};
use smelt_hir::{
    BinOp, Body, Crate as HirCrate, Expr as HirExpr, ExprKind, FileId, Function, FunctionType,
    Item, ItemId, Language, Literal, LocalDecl, MatchArm, Module, ModuleId, Param,
    Pattern as HirPattern, SourceFile, Span, Stmt as HirStmt, Type, TypeId, UnaryOp,
};

// ---------------------------------------------------------------------------
// Public API — kept in lockstep with smelt-frontend-ts.
// ---------------------------------------------------------------------------

/// Diagnostic produced by parsing or lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmeltError {
    /// Stable diagnostic code.
    pub code: &'static str,
    /// Source range the diagnostic refers to.
    pub span: Span,
    /// Human-readable message.
    pub message: String,
    /// Optional secondary note.
    pub note: Option<String>,
}

impl SmeltError {
    fn unsupported(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::unsupported-py",
            span,
            message: message.into(),
            note: None,
        }
    }

    fn parse(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::parse-error-py",
            span,
            message: message.into(),
            note: None,
        }
    }
}

/// Reusable lowering context — one per crate, shared across files.
#[derive(Debug)]
pub struct HirCtx {
    /// The crate being assembled.
    pub krate: HirCrate,
}

impl HirCtx {
    /// Create an empty context.
    #[must_use]
    pub fn new() -> Self {
        Self {
            krate: HirCrate::new(),
        }
    }
}

impl Default for HirCtx {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse `source` and return the raw Ruff [`ModModule`] AST.
///
/// Useful for debugging or tooling that only needs the parse tree.
///
/// # Errors
/// Returns `Err` if the source has any syntax errors.
pub fn parse_module(source: &str, file_id: FileId) -> Result<ModModule, Vec<SmeltError>> {
    let parsed = parse(source, ParseOptions::from(Mode::Module)).map_err(|err| {
        vec![SmeltError::parse(
            range_to_span(file_id, err.location),
            err.to_string(),
        )]
    })?;

    if !parsed.errors().is_empty() {
        return Err(parsed
            .errors()
            .iter()
            .map(|err| SmeltError::parse(range_to_span(file_id, err.location), err.to_string()))
            .collect());
    }

    match parsed.into_syntax() {
        Mod::Module(m) => Ok(m),
        Mod::Expression(_) => Err(vec![SmeltError::parse(
            Span::new(file_id, 0, 0),
            "expected a module, got a bare expression",
        )]),
    }
}

/// Parse `source` as a Python module and lower it to HIR.
///
/// # Errors
/// Returns `Err` for parse errors or unsupported Python constructs.
pub fn to_hir(
    source: &str,
    file_id: FileId,
    ctx: &mut HirCtx,
) -> Result<ModuleId, Vec<SmeltError>> {
    let module_ast = parse_module(source, file_id)?;
    let mut builder = ModuleBuilder::new(file_id, ctx);
    builder.module(&module_ast)
}

// ---------------------------------------------------------------------------
// Lowering
// ---------------------------------------------------------------------------

struct ModuleBuilder<'ctx> {
    file_id: FileId,
    ctx: &'ctx mut HirCtx,
    /// In-scope locals while lowering a body.
    locals: HashMap<String, smelt_hir::LocalId>,
    /// Module-level items (functions / classes) for call resolution.
    items: HashMap<String, ItemId>,
}

impl<'ctx> ModuleBuilder<'ctx> {
    fn new(file_id: FileId, ctx: &'ctx mut HirCtx) -> Self {
        Self {
            file_id,
            ctx,
            locals: HashMap::new(),
            items: HashMap::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Module — two-pass lowering
    // -----------------------------------------------------------------------

    fn module(&mut self, module: &ModModule) -> Result<ModuleId, Vec<SmeltError>> {
        let source = SourceFile {
            path: String::new(),
            language: Language::Python,
        };
        let mut hir_module = Module::new("main", source);
        let module_span = self.span(module.range);
        let mut body = Body::new(None, module_span);
        let mut errors: Vec<SmeltError> = Vec::new();

        // Pass 1 — collect top-level function/class declarations so later
        // statements can reference them in calls.
        for stmt in &module.body {
            if let Stmt::FunctionDef(func) = stmt {
                match self.function_def(func) {
                    Ok(item_id) => hir_module.items.push(item_id),
                    Err(err) => errors.push(err),
                }
            }
        }

        // Pass 2 — lower module-level statements into the module body.
        for stmt in &module.body {
            if matches!(stmt, Stmt::FunctionDef(_)) {
                continue; // already lowered
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

    // -----------------------------------------------------------------------
    // Function definition
    // -----------------------------------------------------------------------

    fn function_def(&mut self, func: &StmtFunctionDef) -> Result<ItemId, SmeltError> {
        if func.is_async {
            return Err(SmeltError::unsupported(
                self.span(func.range),
                "async functions are not yet supported",
            ));
        }

        let name_str = func.name.as_str();
        let name = self.intern_name(name_str);

        // Return type — required.
        let return_ty = func
            .returns
            .as_deref()
            .ok_or_else(|| {
                SmeltError::unsupported(
                    self.span(func.range),
                    format!("function '{name_str}' must have an explicit return type annotation"),
                )
            })
            .and_then(|ann| self.annotation_to_hir(ann))?;

        // Save outer scope and start fresh for this function's body.
        let saved_locals = std::mem::take(&mut self.locals);
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
                        format!("parameter '{param_name_str}' must have an explicit type annotation"),
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
            return Err(SmeltError::unsupported(
                self.span(func.range),
                format!(
                    "function '{name_str}': *args and **kwargs are not yet supported"
                ),
            ));
        }

        // Lower the body statements.
        let body_error = func
            .body
            .iter()
            .find_map(|stmt| self.statement(stmt, &mut fn_body).err());

        self.locals = saved_locals;

        if let Some(err) = body_error {
            return Err(err);
        }

        let body_id = self.ctx.krate.push_body(fn_body);
        let item = Item::Function(Function {
            name,
            span: func_span,
            params,
            return_ty,
            is_async: false,
            body: Some(body_id),
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
            "Optional" | "Union" | "List" | "Dict" | "Set" | "Tuple" | "Callable"
            | "Awaitable" => Err(SmeltError::unsupported(
                span,
                format!("'{name}' requires type arguments, e.g. {name}[T]"),
            )),
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
                        ))
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
                body.push_stmt_to_block(block, HirStmt::While { cond, body: loop_block });
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
                "class definitions are not yet supported",
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
                ))
            }
        };

        let target = self.expression(&aug.target, body)?;
        let rhs = self.expression(&aug.value, body)?;

        // Determine the result type from the target expression's type.
        let lhs_ty = body.exprs[target.0 as usize].ty;
        let compound = body.push_expr(HirExpr {
            kind: ExprKind::BinOp { op, lhs: target, rhs },
            ty: lhs_ty,
            span: self.span(aug.range),
        });
        body.push_stmt_to_block(block, HirStmt::Assign { target, value: compound });
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

    /// `for target in iter` — only simple name targets supported.
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

        let Expr::Name(target_name) = for_stmt.target.as_ref() else {
            return Err(SmeltError::unsupported(
                self.span(for_stmt.target.range()),
                "for loop target must be a simple name (destructuring not yet supported)",
            ));
        };

        let iter = self.expression(&for_stmt.iter, body)?;

        // Declare the loop variable with the element type of the iterator.
        // We use None type here; type inference will resolve it later.
        let none_ty = self.intern_type(Type::None);
        let name_str = target_name.id.as_str();
        let name_sym = self.intern_name(name_str);
        let local = body.push_local(LocalDecl {
            name: Some(name_sym),
            ty: none_ty,
            mutable: true,
            span: self.span(target_name.range),
        });
        self.locals.insert(name_str.to_owned(), local);

        let pat = body.push_pattern(HirPattern::Binding(local));
        let loop_block = self.block_from_stmts(&for_stmt.body, body)?;

        body.push_stmt_to_block(block, HirStmt::For { pat, iter, body: loop_block });
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
                    ))
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
                Number::Int(i) => i
                    .as_i64()
                    .map(Literal::Int)
                    .ok_or_else(|| SmeltError::unsupported(self.span(n.range), "integer literal out of i64 range")),
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
                        Number::Int(i) => i
                            .as_i64()
                            .map(|v| Literal::Int(-v))
                            .ok_or_else(|| SmeltError::unsupported(self.span(n.range), "integer literal out of i64 range")),
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

    fn expression(&mut self, expr: &Expr, body: &mut Body) -> Result<smelt_hir::ExprId, SmeltError> {
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
                        (ExprKind::Literal(Literal::Int(v)), self.intern_type(Type::Int))
                    }
                    Number::Float(f) => (
                        ExprKind::Literal(Literal::Float(*f)),
                        self.intern_type(Type::Float),
                    ),
                    Number::Complex { .. } => {
                        return Err(SmeltError::unsupported(
                            self.span(n.range),
                            "complex number literals are not supported",
                        ))
                    }
                };
                Ok(body.push_expr(HirExpr { kind, ty, span: self.span(n.range) }))
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
                ))
            }
        };
        let lhs = self.expression(&b.left, body)?;
        let rhs = self.expression(&b.right, body)?;
        let ty = if result_is_bool {
            self.intern_type(Type::Bool)
        } else {
            body.exprs[lhs.0 as usize].ty
        };
        Ok(body.push_expr(HirExpr { kind: ExprKind::BinOp { op, lhs, rhs }, ty, span }))
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
                ))
            }
        };
        let lhs = self.expression(&c.left, body)?;
        let rhs = self.expression(&c.comparators[0], body)?;
        let ty = self.intern_type(Type::Bool);
        Ok(body.push_expr(HirExpr { kind: ExprKind::BinOp { op, lhs, rhs }, ty, span }))
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
                ))
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
        }

        // Named function call — look up in items map.
        if let Expr::Name(name) = call.func.as_ref() {
            let name_str = name.id.as_str();
            if let Some(&item_id) = self.items.get(name_str) {
                // Determine return type from the HIR item.
                let return_ty = match &self.ctx.krate.items[item_id.0 as usize] {
                    Item::Function(f) => f.return_ty,
                    _ => self.intern_type(Type::None),
                };

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
        }

        Err(SmeltError::unsupported(
            span,
            "only calls to top-level functions and print() are supported",
        ))
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
                _ => self.intern_type(Type::None),
            };
            return Ok(body.push_expr(HirExpr {
                kind: ExprKind::Item(item_id),
                ty,
                span,
            }));
        }

        Err(SmeltError::unsupported(span, format!("unresolved name '{name}'")))
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
                SmeltError::unsupported(Span::new(self.file_id, 0, 0), "cannot index an empty tuple")
            }),
            _ => Err(SmeltError::unsupported(
                Span::new(self.file_id, 0, 0),
                "subscript access requires a list, set, dict, or tuple",
            )),
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
        });
        let id = self.ctx.krate.push_item(item);
        self.items.insert(smelt_hir::CONSOLE_LOG_SYMBOL.to_owned(), id);
        id
    }

    // -----------------------------------------------------------------------
    // Interning helpers
    // -----------------------------------------------------------------------

    fn intern_name(&mut self, name: &str) -> smelt_hir::Symbol {
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

// ---------------------------------------------------------------------------
// Free-function helpers
// ---------------------------------------------------------------------------

fn range_to_span(file_id: FileId, range: TextRange) -> Span {
    Span::new(file_id, range.start().to_u32(), range.end().to_u32())
}

/// Extract the type name from `Expr::Name` or the attribute from
/// `Expr::Attribute` (e.g. `typing.Optional` → `"Optional"`).
fn expr_type_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::Name(n) => Some(n.id.as_str()),
        Expr::Attribute(a) => Some(a.attr.as_str()),
        _ => None,
    }
}

/// Flatten a `T | U | V` tree into a flat list of parts.
fn collect_bitor_parts<'a>(expr: &'a Expr, parts: &mut Vec<&'a Expr>) {
    if let Expr::BinOp(b) = expr {
        if b.op == Operator::BitOr {
            collect_bitor_parts(&b.left, parts);
            collect_bitor_parts(&b.right, parts);
            return;
        }
    }
    parts.push(expr);
}

/// Expect exactly two type arguments from a subscript slice (e.g. `dict[K, V]`).
fn two_type_args(slice: &Expr, span: Span) -> Result<(&Expr, &Expr), SmeltError> {
    if let Expr::Tuple(t) = slice {
        if t.elts.len() == 2 {
            return Ok((&t.elts[0], &t.elts[1]));
        }
    }
    Err(SmeltError::unsupported(span, "expected exactly two type arguments"))
}

/// Return a short name for a statement kind (for error messages).
fn stmt_kind_name(stmt: &Stmt) -> &'static str {
    match stmt {
        Stmt::FunctionDef(_) => "def",
        Stmt::ClassDef(_) => "class",
        Stmt::Return(_) => "return",
        Stmt::Delete(_) => "del",
        Stmt::Assign(_) => "assign",
        Stmt::AugAssign(_) => "augmented assign",
        Stmt::AnnAssign(_) => "annotated assign",
        Stmt::TypeAlias(_) => "type alias",
        Stmt::For(_) => "for",
        Stmt::While(_) => "while",
        Stmt::If(_) => "if",
        Stmt::With(_) => "with",
        Stmt::Match(_) => "match",
        Stmt::Raise(_) => "raise",
        Stmt::Try(_) => "try",
        Stmt::Assert(_) => "assert",
        Stmt::Import(_) => "import",
        Stmt::ImportFrom(_) => "from import",
        Stmt::Global(_) => "global",
        Stmt::Nonlocal(_) => "nonlocal",
        Stmt::Expr(_) => "expr",
        Stmt::Pass(_) => "pass",
        Stmt::Break(_) => "break",
        Stmt::Continue(_) => "continue",
        Stmt::IpyEscapeCommand(_) => "ipy escape",
    }
}

/// Return a short name for an expression kind (for error messages).
fn expr_kind_name(expr: &Expr) -> &'static str {
    match expr {
        Expr::BoolOp(_) => "bool op",
        Expr::Named(_) => "walrus operator",
        Expr::BinOp(_) => "bin op",
        Expr::UnaryOp(_) => "unary op",
        Expr::Lambda(_) => "lambda",
        Expr::If(_) => "ternary if",
        Expr::Dict(_) => "dict",
        Expr::Set(_) => "set",
        Expr::ListComp(_) => "list comprehension",
        Expr::SetComp(_) => "set comprehension",
        Expr::DictComp(_) => "dict comprehension",
        Expr::Generator(_) => "generator",
        Expr::Await(_) => "await",
        Expr::Yield(_) => "yield",
        Expr::YieldFrom(_) => "yield from",
        Expr::Compare(_) => "compare",
        Expr::Call(_) => "call",
        Expr::FString(_) => "f-string",
        Expr::TString(_) => "t-string",
        Expr::StringLiteral(_) => "string",
        Expr::BytesLiteral(_) => "bytes",
        Expr::NumberLiteral(_) => "number",
        Expr::BooleanLiteral(_) => "bool",
        Expr::NoneLiteral(_) => "None",
        Expr::EllipsisLiteral(_) => "ellipsis",
        Expr::Attribute(_) => "attribute",
        Expr::Subscript(_) => "subscript",
        Expr::Starred(_) => "starred",
        Expr::Name(_) => "name",
        Expr::List(_) => "list",
        Expr::Tuple(_) => "tuple",
        Expr::Slice(_) => "slice",
        Expr::IpyEscapeCommand(_) => "ipy escape",
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{HirCtx, to_hir};
    use smelt_hir::{FileId, Language};

    #[test]
    fn empty_module_lowers_to_empty_hir() {
        let mut ctx = HirCtx::new();
        let module_id = to_hir("", FileId(0), &mut ctx).expect("empty module is valid");
        let module = &ctx.krate.modules[module_id.0 as usize];
        assert_eq!(module.source.language, Language::Python);
        assert!(module.items.is_empty());
    }

    #[test]
    fn parse_error_is_reported() {
        let mut ctx = HirCtx::new();
        let errors = to_hir("x = \"oops", FileId(0), &mut ctx).expect_err("should fail");
        assert!(!errors.is_empty());
        assert_eq!(errors[0].code, "smelt::parse-error-py");
    }

    #[test]
    fn simple_function_lowers() {
        let source = r#"
def add(x: int, y: int) -> int:
    return x + y
"#;
        let mut ctx = HirCtx::new();
        let module_id = to_hir(source, FileId(0), &mut ctx).expect("valid module");
        let module = &ctx.krate.modules[module_id.0 as usize];
        assert_eq!(module.items.len(), 1);
        let item = &ctx.krate.items[module.items[0].0 as usize];
        match item {
            smelt_hir::Item::Function(f) => {
                assert_eq!(ctx.krate.symbols.get(f.name).unwrap(), "add");
                assert_eq!(f.params.len(), 2);
                assert!(f.body.is_some());
            }
            _ => panic!("expected Function item"),
        }
    }

    #[test]
    fn annotated_assignment_lowers() {
        let source = "x: int = 42\n";
        let mut ctx = HirCtx::new();
        let module_id = to_hir(source, FileId(0), &mut ctx).expect("valid module");
        let module = &ctx.krate.modules[module_id.0 as usize];
        let body = &ctx.krate.bodies[module.body.unwrap().0 as usize];
        assert!(!body.stmts.is_empty());
    }

    #[test]
    fn type_annotations_lowered() {
        let source = r#"
def process(items: list[str], counts: dict[str, int]) -> bool:
    return True
"#;
        let mut ctx = HirCtx::new();
        to_hir(source, FileId(0), &mut ctx).expect("type annotations should lower");
    }

    #[test]
    fn optional_annotation_lowered() {
        let source = r#"
def find(x: int) -> str | None:
    return None
"#;
        let mut ctx = HirCtx::new();
        to_hir(source, FileId(0), &mut ctx)
            .expect("PEP 604 Optional annotation should lower");
    }

    #[test]
    fn missing_return_annotation_is_error() {
        let source = "def bad(x: int):\n    return x\n";
        let mut ctx = HirCtx::new();
        let errors = to_hir(source, FileId(0), &mut ctx).expect_err("should require return type");
        assert_eq!(errors[0].code, "smelt::unsupported-py");
    }

    #[test]
    fn print_call_lowers() {
        let source = r#"
x: int = 1
print(x)
"#;
        let mut ctx = HirCtx::new();
        to_hir(source, FileId(0), &mut ctx).expect("print() should lower");
    }
}
