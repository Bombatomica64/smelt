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
                    RuffPattern::MatchOr(_)
                        | RuffPattern::MatchAs(PatternMatchAs { name: Some(_), .. })
                        | RuffPattern::MatchClass(_)
                )
                // A value pattern naming a constant (`case Color.RED:`) is
                // compared with `==`, not switched on as a literal.
                || matches!(&case.pattern, RuffPattern::MatchValue(value) if matches!(value.value.as_ref(), Expr::Attribute(_)))
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
        let whole = PatternSubject { local: subject, local_ty: subject_ty, steps: Vec::new() };
        for case in &match_stmt.cases {
            let case_span = self.span(case.range);
            let compiled = self.compile_pattern(&case.pattern, &whole, body)?;
            let matched_read = body.push_expr(HirExpr { kind: ExprKind::Local(matched), ty: bool_ty, span: case_span });
            let not_matched = body.push_expr(HirExpr {
                kind: ExprKind::UnaryOp { op: UnaryOp::Not, operand: matched_read },
                ty: bool_ty,
                span: case_span,
            });
            let cond = compiled.test.map_or(not_matched, |pattern_test| {
                self.short_circuit(BinOp::And, not_matched, pattern_test, case_span, body)
            });
            let case_block = body.push_block(case_span);
            for (name, name_span, captured) in &compiled.captures {
                let read = self.pattern_subject_read(captured, *name_span, body);
                self.bind_match_capture(name, read, captured.ty(), *name_span, body, (block, case_block));
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

    /// Compile `pattern`, matched against `subject`, into its test (`None`
    /// when it always matches) and the captures it binds, in source order.
    ///
    /// A class pattern `C(p0, k=pk)` tests `isinstance(subject, C)` first,
    /// then each sub-pattern against an attribute of the subject read as a
    /// `C` (positional sub-patterns name the attributes `C.__match_args__`
    /// lists, a dataclass's fields by default), all short-circuited in source
    /// order as `CPython` tries them. Literals, singletons, captures,
    /// wildcards, `as` and `|` (without captures) nest anywhere.
    fn compile_pattern(
        &mut self,
        pattern: &RuffPattern,
        subject: &PatternSubject,
        body: &mut Body,
    ) -> Result<CompiledPattern, SmeltError> {
        let span = self.span(pattern.range());
        match pattern {
            RuffPattern::MatchAs(PatternMatchAs { pattern: inner, name, .. }) => {
                let mut compiled = match inner.as_deref() {
                    Some(inner) => self.compile_pattern(inner, subject, body)?,
                    None => CompiledPattern { test: None, captures: Vec::new() },
                };
                if let Some(name) = name {
                    compiled.captures.push((name.as_str().to_owned(), self.span(name.range), subject.clone()));
                }
                Ok(compiled)
            }
            RuffPattern::MatchOr(alternatives) => {
                let mut tests = Vec::with_capacity(alternatives.patterns.len());
                for alternative in &alternatives.patterns {
                    let compiled = self.compile_pattern(alternative, subject, body)?;
                    if !compiled.captures.is_empty() {
                        return Err(SmeltError::unsupported(span, "an `|` pattern that binds names is not supported"));
                    }
                    let Some(test) = compiled.test else {
                        return Ok(CompiledPattern { test: None, captures: Vec::new() });
                    };
                    tests.push(test);
                }
                let test = tests.into_iter().reduce(|either, next| self.short_circuit(BinOp::Or, either, next, span, body));
                Ok(CompiledPattern { test, captures: Vec::new() })
            }
            RuffPattern::MatchClass(class_pattern) => self.compile_class_pattern(class_pattern, subject, body),
            RuffPattern::MatchValue(value) => {
                let read = self.pattern_subject_read(subject, span, body);
                let expected = self.expression(&value.value, body)?;
                let test = self.compare_pair(CmpOp::Eq, read, expected, span, body)?;
                Ok(CompiledPattern { test: Some(test), captures: Vec::new() })
            }
            RuffPattern::MatchSingleton(singleton) => {
                let read = self.pattern_subject_read(subject, span, body);
                let (literal, ty) = match singleton.value {
                    Singleton::None => (Literal::None, self.intern_type(Type::None)),
                    Singleton::True => (Literal::Bool(true), self.intern_type(Type::Bool)),
                    Singleton::False => (Literal::Bool(false), self.intern_type(Type::Bool)),
                };
                let expected = body.push_expr(HirExpr { kind: ExprKind::Literal(literal), ty, span });
                let test = self.compare_pair(CmpOp::Is, read, expected, span, body)?;
                Ok(CompiledPattern { test: Some(test), captures: Vec::new() })
            }
            RuffPattern::MatchSequence(_) | RuffPattern::MatchMapping(_) | RuffPattern::MatchStar(_) => Err(
                SmeltError::unsupported(span, "sequence, mapping and star match patterns are not supported"),
            ),
        }
    }

    /// The class pattern `C(..)` against `subject` (see [`Self::compile_pattern`]).
    fn compile_class_pattern(
        &mut self,
        class_pattern: &ruff_python_ast::PatternMatchClass,
        subject: &PatternSubject,
        body: &mut Body,
    ) -> Result<CompiledPattern, SmeltError> {
        let span = self.span(class_pattern.range);
        let class_name = match class_pattern.cls.as_ref() {
            Expr::Name(name) => name.id.as_str().to_owned(),
            other => self.nested_class_path(other).ok_or_else(|| {
                SmeltError::unsupported(span, "a class pattern must name a class")
            })?,
        };
        let class_sym = match self.items.get(&class_name).map(|item| self.item_ref(*item)) {
            Some(Item::Class(class)) => class.name,
            _ => return Err(SmeltError::unsupported(span, format!("class pattern `{class_name}` does not name a class of the program"))),
        };
        let class_ty = self.intern_type(Type::Class { name: class_sym, args: Vec::new() });
        let read = self.pattern_subject_read(subject, span, body);
        let mut tests = Vec::new();
        match self.instance_test(read, &[InstanceTarget::Class(class_sym)], span, body)? {
            InstanceTest::Constant(true) => {}
            InstanceTest::Constant(false) => {
                let bool_ty = self.intern_type(Type::Bool);
                tests.push(body.push_expr(HirExpr { kind: ExprKind::Literal(Literal::Bool(false)), ty: bool_ty, span }));
            }
            InstanceTest::Dynamic(test) => tests.push(test),
        }
        let mut narrowed = subject.clone();
        if subject.ty() != class_ty {
            narrowed.steps.push(SubjectStep::Cast(class_ty));
        }
        let positional = &class_pattern.arguments.patterns;
        let match_args = if positional.is_empty() { Vec::new() } else { self.class_match_args(class_sym) };
        if positional.len() > match_args.len() {
            return Err(SmeltError::unsupported(
                span,
                format!("`{class_name}()` accepts {} positional sub-patterns ({} given)", match_args.len(), positional.len()),
            ));
        }
        let mut attributes: Vec<(String, &RuffPattern)> =
            match_args.into_iter().zip(positional.iter()).collect();
        attributes.extend(
            class_pattern.arguments.keywords.iter().map(|keyword| (keyword.attr.as_str().to_owned(), &keyword.pattern)),
        );
        let mut captures = Vec::new();
        for (attribute, sub_pattern) in attributes {
            let field = self.intern_name(&attribute);
            let field_ty = self.field_type(class_ty, field).map_err(|_| {
                SmeltError::unsupported(span, format!("class pattern: `{class_name}` has no attribute `{attribute}`"))
            })?;
            let mut attribute_subject = narrowed.clone();
            attribute_subject.steps.push(SubjectStep::Field(field, field_ty));
            let compiled = self.compile_pattern(sub_pattern, &attribute_subject, body)?;
            tests.extend(compiled.test);
            captures.extend(compiled.captures);
        }
        let test = tests.into_iter().reduce(|both, next| self.short_circuit(BinOp::And, both, next, span, body));
        Ok(CompiledPattern { test, captures })
    }

    /// The attribute names positional sub-patterns of a class pattern match:
    /// the class's `__match_args__` (its own or the nearest base's), else a
    /// dataclass's fields in declaration order (what `@dataclass` generates),
    /// else none.
    fn class_match_args(&self, class_sym: Symbol) -> Vec<String> {
        let mut current = Some(class_sym);
        while let Some(class) = current {
            if let Some(names) = self.class_match_args.get(&class) {
                return names.clone();
            }
            let Some(Item::Class(item)) = self.items.values().map(|id| self.item_ref(*id)).find(|item| matches!(item, Item::Class(found) if found.name == class)) else {
                break;
            };
            if matches!(item.kind, ClassKind::DataclassLike { .. }) {
                let mut names: Vec<String> = item
                    .base
                    .map(|base| self.class_match_args(base))
                    .unwrap_or_default();
                for field in &item.fields {
                    let name = self.ctx.krate.symbols.get(field.name).unwrap_or_default().to_owned();
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
                return names;
            }
            current = item.base;
        }
        Vec::new()
    }

    /// Build a read of `subject`: its local, then each cast and attribute.
    fn pattern_subject_read(&self, subject: &PatternSubject, span: Span, body: &mut Body) -> smelt_hir::ExprId {
        let mut read = body.push_expr(HirExpr { kind: ExprKind::Local(subject.local), ty: subject.local_ty, span });
        for step in &subject.steps {
            read = match *step {
                SubjectStep::Cast(target) => {
                    body.push_expr(HirExpr { kind: ExprKind::UnknownCast { value: read, target }, ty: target, span })
                }
                SubjectStep::Field(field, ty) => {
                    body.push_expr(HirExpr { kind: ExprKind::Field { receiver: read, field }, ty, span })
                }
            };
        }
        read
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

/// One step from the match subject to the sub-value a nested pattern tests.
#[derive(Debug, Clone, Copy)]
enum SubjectStep {
    /// The value read as the given class (after its class test passed).
    Cast(TypeId),
    /// The value's attribute, at its type.
    Field(Symbol, TypeId),
}

/// The value a (sub-)pattern is matched against: the match subject's local
/// and the steps that reach it.
#[derive(Debug, Clone)]
struct PatternSubject {
    /// The local holding the whole match subject.
    local: smelt_hir::LocalId,
    /// That local's type.
    local_ty: TypeId,
    /// The casts and attribute reads from the subject to this value.
    steps: Vec<SubjectStep>,
}

impl PatternSubject {
    /// The value's type.
    fn ty(&self) -> TypeId {
        match self.steps.last() {
            Some(SubjectStep::Cast(ty) | SubjectStep::Field(_, ty)) => *ty,
            None => self.local_ty,
        }
    }
}

/// A compiled pattern (`ModuleBuilder::compile_pattern`).
struct CompiledPattern {
    /// The test, `None` when the pattern always matches.
    test: Option<smelt_hir::ExprId>,
    /// The names it binds, each with its span and the value it captures.
    captures: Vec<(String, Span, PatternSubject)>,
}
