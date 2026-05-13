impl ModuleBuilder<'_> {
    /// Return whether an iterable type is a statically known empty protocol.
    fn is_empty_static_protocol_iterable(&self, iter_ty: TypeId) -> bool {
        let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(iter_ty) else {
            return false;
        };
        let Some(class_name) = self.ctx.krate.symbols.get(*name) else {
            return false;
        };
        class_name == "NullFile"
            && self.class_methods.get(class_name).is_some_and(|methods| {
                methods.contains_key("__iter__") && methods.contains_key("__next__")
            })
    }

    /// Adapt Python iterables whose Rust representation is not directly indexable.
    fn for_iterable(&mut self, iter: smelt_hir::ExprId, body: &mut Body) -> smelt_hir::ExprId {
        let iter_ty = Self::expr_ty(body, iter);
        let iter_span = usize::try_from(iter.0)
            .ok()
            .and_then(|index| body.exprs.get(index))
            .map_or_else(|| Span::new(self.file_id, 0, 0), |expr| expr.span);
        match self.ctx.krate.types.get(iter_ty).cloned() {
            Some(Type::Set(item_ty)) => {
                let ty = self.intern_type(Type::List(item_ty));
                body.push_expr(HirExpr {
                    kind: ExprKind::SetProjection {
                        op: SetProjectionOp::Values,
                        set: iter,
                    },
                    ty,
                    span: iter_span,
                })
            }
            Some(Type::Dict(key_ty, _)) => {
                let ty = self.intern_type(Type::List(key_ty));
                body.push_expr(HirExpr {
                    kind: ExprKind::DictProjection {
                        op: DictProjectionOp::Keys,
                        dict: iter,
                    },
                    ty,
                    span: iter_span,
                })
            }
            _ => iter,
        }
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

                RuffPattern::MatchSequence(_)
                | RuffPattern::MatchMapping(_)
                | RuffPattern::MatchClass(_)
                | RuffPattern::MatchStar(_)
                | RuffPattern::MatchAs(_)
                | RuffPattern::MatchOr(_) => {
                    return Err(SmeltError::unsupported(
                        self.span(case.range),
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
                        Number::Int(i) => i
                            .as_i64()
                            .and_then(|v| v.checked_neg())
                            .map(Literal::Int)
                            .ok_or_else(|| {
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
            Expr::UnaryOp(_) => Err(SmeltError::unsupported(
                self.span(expr.range()),
                "only literal values are supported in match patterns",
            )),
            Expr::BoolOp(_)
            | Expr::Named(_)
            | Expr::BinOp(_)
            | Expr::Lambda(_)
            | Expr::If(_)
            | Expr::Dict(_)
            | Expr::Set(_)
            | Expr::ListComp(_)
            | Expr::SetComp(_)
            | Expr::DictComp(_)
            | Expr::Generator(_)
            | Expr::Await(_)
            | Expr::Yield(_)
            | Expr::YieldFrom(_)
            | Expr::Compare(_)
            | Expr::Call(_)
            | Expr::FString(_)
            | Expr::TString(_)
            | Expr::BytesLiteral(_)
            | Expr::EllipsisLiteral(_)
            | Expr::Attribute(_)
            | Expr::Subscript(_)
            | Expr::Starred(_)
            | Expr::Name(_)
            | Expr::List(_)
            | Expr::Tuple(_)
            | Expr::Slice(_)
            | Expr::IpyEscapeCommand(_) => Err(SmeltError::unsupported(
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
}
