impl ModuleBuilder<'_> {
    fn pytest_raises_callable_statement(
        &mut self,
        call: &ruff_python_ast::ExprCall,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        if !Self::is_pytest_raises_call(call) || call.arguments.args.len() < 2 {
            return Ok(false);
        }
        for keyword in &call.arguments.keywords {
            let Some(arg) = keyword.arg.as_ref().map(|arg| arg.as_str()) else {
                return Err(SmeltError::unsupported(
                    self.span(keyword.range),
                    "pytest.raises **kwargs are not supported yet",
                ));
            };
            if arg != "match" {
                return Err(SmeltError::unsupported(
                    self.span(keyword.range),
                    format!("pytest.raises keyword argument '{arg}' is not supported yet"),
                ));
            }
        }

        let bool_ty = self.intern_type(Type::Bool);
        let raised_sym = self.intern_name("__smelt_pytest_raised");
        let raised_local = body.push_local(LocalDecl {
            name: Some(raised_sym),
            ty: bool_ty,
            mutable: true,
            span: self.span(call.range),
        });
        let false_expr = body.push_expr(HirExpr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span: self.span(call.range),
        });
        let raised_pat = body.push_pattern(HirPattern::Binding(raised_local));
        body.push_stmt_to_block(
            block,
            HirStmt::Let {
                pat: raised_pat,
                ty: bool_ty,
                value: Some(false_expr),
            },
        );

        let try_body = body.push_block(self.span(call.range));
        let callee = self.expression(&call.arguments.args[1], body)?;
        let args = call
            .arguments
            .args
            .iter()
            .skip(2)
            .map(|arg| self.expression(arg, body))
            .collect::<Result<Vec<_>, _>>()?;
        let value = body.push_expr(HirExpr {
            kind: ExprKind::Call { callee, args },
            ty: self.intern_type(Type::None),
            span: self.span(call.range),
        });
        body.push_stmt_to_block(try_body, HirStmt::Expr(value));

        let catch_body = body.push_block(self.span(call.range));
        let raised_target = body.push_expr(HirExpr {
            kind: ExprKind::Local(raised_local),
            ty: bool_ty,
            span: self.span(call.range),
        });
        let true_expr = body.push_expr(HirExpr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span: self.span(call.range),
        });
        body.push_stmt_to_block(
            catch_body,
            HirStmt::Assign {
                target: raised_target,
                value: true_expr,
            },
        );
        body.push_stmt_to_block(
            block,
            HirStmt::TryCatch {
                body: try_body,
                catch_binding: None,
                catch_body: Some(catch_body),
                finally_body: None,
            },
        );

        let raised_check = body.push_expr(HirExpr {
            kind: ExprKind::Local(raised_local),
            ty: bool_ty,
            span: self.span(call.range),
        });
        let missing_raise = body.push_expr(HirExpr {
            kind: ExprKind::UnaryOp {
                op: UnaryOp::Not,
                operand: raised_check,
            },
            ty: bool_ty,
            span: self.span(call.range),
        });
        let failure_block = body.push_block(self.span(call.range));
        let message =
            self.string_literal_expr("pytest.raises(...) did not raise", call.range, body);
        body.push_stmt_to_block(failure_block, HirStmt::Throw(message));
        body.push_stmt_to_block(
            block,
            HirStmt::If {
                cond: missing_raise,
                then_block: failure_block,
                else_block: None,
            },
        );
        Ok(true)
    }

    /// Return the call expression for a supported `pytest.raises(...)` call.
    fn pytest_raises_call(expr: &Expr) -> Option<&ruff_python_ast::ExprCall> {
        let Expr::Call(call) = expr else {
            return None;
        };
        Self::is_pytest_raises_call(call).then_some(call)
    }

    /// Return whether a call is `pytest.raises(...)` or imported `raises(...)`.
    fn is_pytest_raises_call(call: &ruff_python_ast::ExprCall) -> bool {
        match call.func.as_ref() {
            Expr::Attribute(attr) if attr.attr.as_str() == "raises" => {
                matches!(attr.value.as_ref(), Expr::Name(name) if name.id.as_str() == "pytest")
            }
            Expr::Name(name) => name.id.as_str() == "raises",
            _ => false,
        }
    }

    /// Create a string literal expression for synthesized diagnostics.
    fn string_literal_expr(
        &mut self,
        value: &str,
        range: TextRange,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let ty = self.intern_type(Type::String);
        body.push_expr(HirExpr {
            kind: ExprKind::Literal(Literal::String(value.to_owned())),
            ty,
            span: self.span(range),
        })
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
            Expr::BoolOp(_)
            | Expr::Named(_)
            | Expr::BinOp(_)
            | Expr::UnaryOp(_)
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
            | Expr::StringLiteral(_)
            | Expr::BytesLiteral(_)
            | Expr::NumberLiteral(_)
            | Expr::BooleanLiteral(_)
            | Expr::NoneLiteral(_)
            | Expr::EllipsisLiteral(_)
            | Expr::Attribute(_)
            | Expr::Subscript(_)
            | Expr::Starred(_)
            | Expr::Slice(_)
            | Expr::IpyEscapeCommand(_) => Err(SmeltError::unsupported(
                self.span(target.range()),
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
            Operator::MatMult
            | Operator::Mod
            | Operator::Pow
            | Operator::LShift
            | Operator::RShift
            | Operator::BitOr
            | Operator::BitXor
            | Operator::BitAnd
            | Operator::FloorDiv => {
                let op = aug.op;
                return Err(SmeltError::unsupported(
                    self.span(aug.range),
                    format!("augmented assignment operator '{op:?}' is not supported"),
                ));
            }
        };

        let target = self.expression(&aug.target, body)?;
        let rhs = self.expression(&aug.value, body)?;

        // Determine the result type from the target expression's type.
        let lhs_ty = Self::expr_ty(body, target);
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
            let else_block = self.elif_else_chain(clauses.get(1..).unwrap_or(&[]), body)?;
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

        let raw_iter = self.expression(&for_stmt.iter, body)?;
        if self.is_empty_static_protocol_iterable(Self::expr_ty(body, raw_iter)) {
            return Ok(());
        }
        let iter = self.for_iterable(raw_iter, body);
        let iter_ty = Self::expr_ty(body, iter);
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

    // Iterator protocol helpers continue in `match.rs`.
}
