pub mod checker;

use std::collections::HashMap;

use oxc::allocator::Allocator;
use oxc::ast::ast::{Argument, BindingPattern, Expression, Program, Statement};
use oxc::parser::{ParseOptions, Parser};
use oxc::span::{GetSpan, SourceType};
use smelt_hir::{
    Body, Crate as HirCrate, Expr, ExprKind, FileId, Language, Literal, LocalDecl, Module,
    ModuleId, Pattern, SourceFile, Span, Stmt, Type,
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
}

impl<'ctx> ModuleBuilder<'ctx> {
    fn new(file_id: FileId, ctx: &'ctx mut HirCtx) -> Self {
        Self {
            file_id,
            ctx,
            locals: HashMap::new(),
        }
    }

    fn program(&mut self, program: &Program<'_>) -> Result<ModuleId, Vec<SmeltError>> {
        let span = self.span(program.span.start, program.span.end);
        let mut body = Body::new(None, span);
        let mut errors = Vec::new();

        for statement in &program.body {
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let body_id = self.ctx.krate.push_body(body);
        let mut module = Module::new(
            "main",
            SourceFile {
                path: "<memory>".to_owned(),
                language: Language::TypeScript,
            },
        );
        module.body = Some(body_id);
        Ok(self.ctx.krate.push_module(module))
    }

    fn statement(&mut self, statement: &Statement<'_>, body: &mut Body) -> Result<(), SmeltError> {
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
                    body.push_stmt(Stmt::Let { pat, ty, value });
                }
                Ok(())
            }
            Statement::ExpressionStatement(expr_stmt) => {
                let expr = self.expression(&expr_stmt.expression, body)?;
                body.push_stmt(Stmt::Expr(expr));
                Ok(())
            }
            _ => Err(SmeltError::unsupported(
                self.statement_span(statement),
                format!("statement kind is not lowered yet: {statement:?}"),
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
            Expression::CallExpression(call) => {
                if let Expression::StaticMemberExpression(member) = &call.callee {
                    if let Expression::Identifier(object) = &member.object {
                        if object.name == "console" && member.property.name == "log" {
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
                            let callee_item = self.ensure_console_log_item(
                                self.span(member.span.start, member.span.end),
                            );
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
                    }
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
        let name = self.ctx.krate.symbols.intern("console_log");
        let none = self.ctx.krate.types.intern(Type::None);
        self.ctx
            .krate
            .push_item(smelt_hir::Item::Function(smelt_hir::Function {
                name,
                span,
                params: Vec::new(),
                return_ty: none,
                is_async: false,
                body: smelt_hir::BodyId(u32::MAX),
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
    fn normalizes_camel_case() {
        assert_eq!(camel_to_snake("myFunction"), "my_function");
        assert_eq!(camel_to_snake("URLParser"), "url_parser");
        assert_eq!(camel_to_snake("IPAddr"), "ip_addr");
        assert_eq!(camel_to_snake("_internal"), "_internal");
    }
}
