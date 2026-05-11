impl ModuleBuilder<'_> {
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

    /// Adapt TypeScript for-of iterables whose Rust representation is not indexable.
    fn for_of_iterable(
        &mut self,
        iter: smelt_hir::ExprId,
        source: &Expression<'_>,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let iter_ty = Self::expr_ty(body, iter);
        match self.ctx.krate.types.get(iter_ty).cloned() {
            Some(Type::Set(item_ty)) => {
                let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                body.push_expr(Expr {
                    kind: ExprKind::SetProjection {
                        op: SetProjectionOp::Values,
                        set: iter,
                    },
                    ty,
                    span: self.expression_span(source),
                })
            }
            Some(Type::Dict(key_ty, value_ty)) => {
                let entry_ty = self
                    .ctx
                    .krate
                    .types
                    .intern(Type::Tuple(vec![key_ty, value_ty]));
                let ty = self.ctx.krate.types.intern(Type::List(entry_ty));
                body.push_expr(Expr {
                    kind: ExprKind::DictProjection {
                        op: DictProjectionOp::Entries,
                        dict: iter,
                    },
                    ty,
                    span: self.expression_span(source),
                })
            }
            _ => iter,
        }
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

    // Continued in the next split builder file.
}
