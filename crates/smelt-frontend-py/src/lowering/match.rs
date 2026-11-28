impl ModuleBuilder<'_> {
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

    /// `iter` as the list a builtin that materializes its iterable reads
    /// (`zip`, `enumerate`): [`Self::for_iterable`], and a `str` as the list
    /// of its code points, one-character strings in order, as iterating a
    /// `str` yields them.
    fn iterable_as_list(&mut self, iter: smelt_hir::ExprId, body: &mut Body) -> smelt_hir::ExprId {
        let iter_ty = Self::expr_ty(body, iter);
        if !matches!(self.ctx.krate.types.get(iter_ty), Some(Type::PyStr)) {
            return self.for_iterable(iter, body);
        }
        let span = usize::try_from(iter.0)
            .ok()
            .and_then(|index| body.exprs.get(index))
            .map_or_else(|| Span::new(self.file_id, 0, 0), |expr| expr.span);
        let ty = self.intern_type(Type::List(iter_ty));
        body.push_expr(HirExpr { kind: ExprKind::StringChars { haystack: iter }, ty, span })
    }

    /// `match subject: case …` — only literal / wildcard patterns.
    fn match_statement(
        &mut self,
        match_stmt: &StmtMatch,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let guarded = match_stmt.cases.iter().any(|case| {
            case.guard.is_some()
                || matches!(
                    &case.pattern,
                    RuffPattern::MatchOr(_) | RuffPattern::MatchAs(PatternMatchAs { name: Some(_), .. })
                )
        });
        if guarded {
            return self.guarded_match_statement(match_stmt, body, block);
        }
        let scrutinee = self.expression(&match_stmt.subject, body)?;
        let mut arms: Vec<MatchArm> = Vec::new();
        let mut default_block: Option<smelt_hir::BlockId> = None;

        for case in &match_stmt.cases {

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

    /// A `match` with a guard, a capture (`case x:`), an `as` binding or an
    /// `or` pattern, lowered as the cases Python tries in order:
    ///
    /// ```text
    /// subject = <subject>; matched = False
    /// if not matched and <pattern test>:   # literal `==`, singleton `is`
    ///     <bind captures>                  # bound before the guard runs
    ///     if <guard>: matched = True; <case body>
    /// ...
    /// ```
    ///
    /// The subject is evaluated once, each guard only when its pattern
    /// matched, and a capture stays bound when its guard fails, as in `CPython`.
    fn guarded_match_statement(
        &mut self,
        match_stmt: &StmtMatch,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(match_stmt.range);
        let subject_value = self.expression(&match_stmt.subject, body)?;
        let subject_ty = Self::expr_ty(body, subject_value);
        let subject = body.push_local(self.synthetic_local("__smelt_match_subject", subject_ty, false, span));
        let subject_pat = body.push_pattern(HirPattern::Binding(subject));
        body.push_stmt_to_block(block, HirStmt::Let { pat: subject_pat, ty: subject_ty, value: Some(subject_value) });
        let bool_ty = self.intern_type(Type::Bool);
        let matched = body.push_local(self.synthetic_local("__smelt_matched", bool_ty, true, span));
        let matched_pat = body.push_pattern(HirPattern::Binding(matched));
        let unmatched = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(false)), ty: bool_ty, span });
        body.push_stmt_to_block(block, HirStmt::Let { pat: matched_pat, ty: bool_ty, value: Some(unmatched) });
        for case in &match_stmt.cases {
            let case_span = self.span(case.range);
            let (pattern, capture) = match &case.pattern {
                RuffPattern::MatchAs(PatternMatchAs { pattern, name, .. }) => (pattern.as_deref(), name.as_ref()),
                other => (Some(other), None),
            };
            let test = pattern.map(|inner| self.match_pattern_test(inner, subject, subject_ty, body)).transpose()?;
            let matched_read = body.push_expr(HirExpr { kind: ExprKind::Local(matched), ty: bool_ty, span: case_span });
            let not_matched = body.push_expr(HirExpr {
                kind: ExprKind::UnaryOp { op: UnaryOp::Not, operand: matched_read },
                ty: bool_ty,
                span: case_span,
            });
            let cond = test.map_or(not_matched, |pattern_test| {
                self.short_circuit(BinOp::And, not_matched, pattern_test, case_span, body)
            });
            let case_block = body.push_block(case_span);
            if let Some(name) = capture {
                let read = body.push_expr(HirExpr { kind: ExprKind::Local(subject), ty: subject_ty, span: case_span });
                self.bind_match_capture(name.as_str(), read, subject_ty, self.span(name.range), body, (block, case_block));
            }
            let run_block = body.push_block(case_span);
            let flag_target = body.push_expr(HirExpr { kind: ExprKind::Local(matched), ty: bool_ty, span: case_span });
            let flag_set = body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(true)), ty: bool_ty, span: case_span });
            body.push_stmt_to_block(run_block, HirStmt::Assign { target: flag_target, value: flag_set });
            self.with_narrowings(&[], |this| {
                for stmt in &case.body {
                    this.statement_in_block(stmt, body, run_block)?;
                }
                Ok::<(), SmeltError>(())
            })?;
            let guard_cond = if let Some(guard) = case.guard.as_deref() {
                self.python_condition(guard, body)?
            } else {
                body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(true)), ty: bool_ty, span: case_span })
            };
            body.push_stmt_to_block(case_block, HirStmt::If { cond: guard_cond, then_block: run_block, else_block: None });
            body.push_stmt_to_block(block, HirStmt::If { cond, then_block: case_block, else_block: None });
        }
        Ok(())
    }

    /// Whether the match subject (the local `subject`) matches `pattern`: a
    /// literal compares with `==`, `None`/`True`/`False` with `is`, an `or`
    /// pattern matches when any alternative does. A pattern that binds or
    /// destructures is refused.
    fn match_pattern_test(
        &mut self,
        pattern: &RuffPattern,
        subject: smelt_hir::LocalId,
        subject_ty: TypeId,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let span = self.span(pattern.range());
        let read = body.push_expr(HirExpr { kind: ExprKind::Local(subject), ty: subject_ty, span });
        match pattern {
            RuffPattern::MatchValue(value) => {
                let expected = self.expression(&value.value, body)?;
                self.compare_pair(CmpOp::Eq, read, expected, span, body)
            }
            RuffPattern::MatchSingleton(singleton) => {
                let (literal, ty) = match singleton.value {
                    Singleton::None => (Literal::None, self.intern_type(Type::None)),
                    Singleton::True => (Literal::Bool(true), self.intern_type(Type::Bool)),
                    Singleton::False => (Literal::Bool(false), self.intern_type(Type::Bool)),
                };
                let expected = body.push_expr(HirExpr { kind: ExprKind::Literal(literal), ty, span });
                self.compare_pair(CmpOp::Is, read, expected, span, body)
            }
            RuffPattern::MatchOr(alternatives) => {
                let mut tests = Vec::with_capacity(alternatives.patterns.len());
                for alternative in &alternatives.patterns {
                    tests.push(self.match_pattern_test(alternative, subject, subject_ty, body)?);
                }
                tests
                    .into_iter()
                    .reduce(|either, next| self.short_circuit(BinOp::Or, either, next, span, body))
                    .ok_or_else(|| SmeltError::unsupported(span, "an `or` pattern needs alternatives"))
            }
            _ => Err(SmeltError::unsupported(
                span,
                "only literal, `None`/`True`/`False`, capture, wildcard and `|` match patterns are supported",
            )),
        }
    }

    /// Bind the match capture `name` to `value` in the case block (the second
    /// of `blocks`): the variable of that name when one of the same type is in
    /// scope (a capture assigns it), else a new one declared in the enclosing
    /// block (the first), so it stays readable after the `match`.
    #[expect(clippy::too_many_arguments, reason = "the binding, its value and where it lowers")]
    fn bind_match_capture(
        &mut self,
        name: &str,
        value: smelt_hir::ExprId,
        ty: TypeId,
        span: Span,
        body: &mut Body,
        (outer, case_block): (smelt_hir::BlockId, smelt_hir::BlockId),
    ) {
        let variable = match self.scope.locals.get(name) {
            Some(&existing) if Self::local_ty(body, existing) == ty => existing,
            _ => {
                let local = body.push_local(LocalDecl { name: Some(self.intern_name(name)), ty, mutable: true, span });
                let pat = body.push_pattern(HirPattern::Binding(local));
                body.push_stmt_to_block(outer, HirStmt::Let { pat, ty, value: None });
                self.scope.locals.insert(name.to_owned(), local);
                local
            }
        };
        let target = body.push_expr(HirExpr { kind: ExprKind::Local(variable), ty, span });
        body.push_stmt_to_block(case_block, HirStmt::Assign { target, value });
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
        self.with_narrowings(&[], |this| {
            for stmt in stmts {
                this.statement_in_block(stmt, body, block)?;
            }
            Ok(block)
        })
    }
}
