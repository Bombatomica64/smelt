pub mod checker;

use std::collections::HashMap;

use oxc::allocator::Allocator;
use oxc::ast::ast::{
    Argument, BindingPattern, Expression, ForStatementLeft, Program, Statement, TSType,
};
use oxc::parser::{ParseOptions, Parser};
use oxc::span::{GetSpan, SourceType};
use oxc::syntax::operator::BinaryOperator;
use smelt_hir::{
    BinOp, Body, Crate as HirCrate, Expr, ExprKind, FileId, Function, Item, Language, Literal,
    LocalDecl, MatchArm, Module, ModuleId, Param, Pattern, SourceFile, Span, Stmt, Type,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmeltError {
    pub code: &'static str,
    pub span: Span,
    pub message: String,
    pub note: Option<String>,
}

impl SmeltError {
    fn unsupported(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::unsupported-ts",
            span,
            message: message.into(),
            note: None,
        }
    }

    fn parse(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::parse-error",
            span,
            message: message.into(),
            note: None,
        }
    }
}

#[derive(Debug)]
pub struct HirCtx {
    pub krate: HirCrate,
}

impl HirCtx {
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

struct ModuleBuilder<'ctx> {
    file_id: FileId,
    ctx: &'ctx mut HirCtx,
    locals: HashMap<String, smelt_hir::LocalId>,
    items: HashMap<String, smelt_hir::ItemId>,
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
            if let Statement::FunctionDeclaration(function) = statement {
                match self.function_declaration(function) {
                    Ok(item) => module.items.push(item),
                    Err(error) => errors.push(error),
                }
            }
        }

        for statement in &program.body {
            if matches!(statement, Statement::FunctionDeclaration(_)) {
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
        }));
        self.items.insert(name_text.to_owned(), item);
        Ok(item)
    }

    fn statement(&mut self, statement: &Statement<'_>, body: &mut Body) -> Result<(), SmeltError> {
        self.statement_in_block(statement, body, body.root)
    }

    fn statement_in_block(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        match statement {
            Statement::VariableDeclaration(decl) => {
                for declarator in &decl.declarations {
                    let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                        return Err(SmeltError::unsupported(
                            self.span(declarator.span.start, declarator.span.end),
                            "destructuring declarations are not lowered yet",
                        ));
                    };

                    let value = match &declarator.init {
                        Some(init) => Some(self.expression(init, body)?),
                        None => None,
                    };
                    let ty = value
                        .map(|expr_id| body.exprs[expr_id.0 as usize].ty)
                        .unwrap_or_else(|| self.ctx.krate.types.intern(Type::None));
                    let name = binding.name.as_str();
                    let symbol = self.ctx.krate.symbols.intern(&camel_to_snake(name));
                    self.ctx.krate.names.record(symbol, name);
                    let local = body.push_local(LocalDecl {
                        name: Some(symbol),
                        ty,
                        mutable: matches!(
                            declarator.kind,
                            oxc::ast::ast::VariableDeclarationKind::Let
                        ),
                        span: self.span(binding.span.start, binding.span.end),
                    });
                    self.locals.insert(name.to_owned(), local);
                    let pat = body.push_pattern(Pattern::Binding(local));
                    body.push_stmt_to_block(block, Stmt::Let { pat, ty, value });
                }
                Ok(())
            }
            Statement::ExpressionStatement(expr_stmt) => {
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
            Statement::ForStatement(for_stmt) => Err(SmeltError::unsupported(
                self.span(for_stmt.span.start, for_stmt.span.end),
                "C-style for loops need assignment/update lowering; use for...of for now",
            )),
            Statement::SwitchStatement(switch_stmt) => {
                let scrutinee = self.expression(&switch_stmt.discriminant, body)?;
                let mut arms = Vec::new();
                let mut default = None;

                for case in &switch_stmt.cases {
                    let case_block = body.push_block(self.span(case.span.start, case.span.end));
                    for statement in &case.consequent {
                        if matches!(
                            statement,
                            Statement::BreakStatement(_) | Statement::ContinueStatement(_)
                        ) {
                            return Err(SmeltError::unsupported(
                                self.statement_span(statement),
                                "switch break/continue lowering is not implemented yet",
                            ));
                        }
                        self.statement_in_block(statement, body, case_block)?;
                    }
                    if !case.consequent.iter().any(statement_terminates) {
                        return Err(SmeltError::unsupported(
                            self.span(case.span.start, case.span.end),
                            "switch fallthrough is not lowered yet; each case must return or throw",
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
            Statement::ThrowStatement(throw_stmt) => Err(SmeltError::unsupported(
                self.span(throw_stmt.span.start, throw_stmt.span.end),
                "throw is exception control flow; try/catch/finally lowering is tracked separately",
            )),
            Statement::TryStatement(try_stmt) => Err(SmeltError::unsupported(
                self.span(try_stmt.span.start, try_stmt.span.end),
                "try/catch/finally is not lowered yet; Python try/else also needs an HIR decision",
            )),
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

    fn expression(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
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
            Expression::CallExpression(call) => {
                if let Expression::StaticMemberExpression(member) = &call.callee
                    && let Expression::Identifier(object) = &member.object
                    && object.name == "console"
                    && member.property.name == "log"
                {
                    let mut args = Vec::new();
                    for arg in &call.arguments {
                        let Argument::Identifier(ident) = arg else {
                            return Err(SmeltError::unsupported(
                                self.span(call.span.start, call.span.end),
                                "console.log currently accepts identifier arguments only",
                            ));
                        };
                        args.push(self.identifier_expression(
                            ident.name.as_str(),
                            ident.span.start,
                            ident.span.end,
                            body,
                        )?);
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
            _ => Err(SmeltError::unsupported(
                self.expression_span(expression),
                format!("expression kind is not lowered yet: {expression:?}"),
            )),
        }
    }

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
            _ => Err(SmeltError::unsupported(
                self.span(argument.span().start, argument.span().end),
                format!("call argument kind is not lowered yet: {argument:?}"),
            )),
        }
    }

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
                for member in &union.types {
                    let member_ty = self.ts_type_to_hir(member)?;
                    if !lowered.contains(&member_ty) {
                        lowered.push(member_ty);
                    }
                }
                if lowered.len() == 1 {
                    Ok(lowered[0])
                } else {
                    Ok(self.ctx.krate.types.intern(Type::Union(lowered)))
                }
            }
            _ => Err(SmeltError::unsupported(
                self.span(ty.span().start, ty.span().end),
                format!("type annotation is not lowered yet: {ty:?}"),
            )),
        }
    }

    fn intern_source_name(&mut self, name: &str) -> smelt_hir::Symbol {
        let symbol = self.ctx.krate.symbols.intern(&camel_to_snake(name));
        self.ctx.krate.names.record(symbol, name);
        symbol
    }

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
            }))
    }

    fn span(&self, start: u32, end: u32) -> Span {
        Span::new(self.file_id, start, end)
    }

    fn statement_span(&self, statement: &Statement<'_>) -> Span {
        let span = statement.span();
        self.span(span.start, span.end)
    }

    fn expression_span(&self, expression: &Expression<'_>) -> Span {
        let span = expression.span();
        self.span(span.start, span.end)
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
    fn reports_try_catch_finally_as_exception_control_flow() {
        let mut ctx = HirCtx::new();
        let errors = to_hir(
            "try {
  throw new Error('x');
} catch (error) {
  console.log(error);
} finally {
  console.log(error);
}
",
            FileId(0),
            &mut ctx,
        )
        .expect_err("try/catch/finally is deferred");

        assert!(errors[0].message.contains("try/catch/finally"));
        assert!(errors[0].message.contains("Python try/else"));
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
}
