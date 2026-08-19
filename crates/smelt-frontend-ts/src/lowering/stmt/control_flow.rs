//! Control-flow statement lowering (C-style `for`, `for-of`, `for-in`, switch case labels).

use oxc::span::GetSpan;

use crate::lowering::{
    BindingPattern, Body, DictProjectionOp, Expr, ExprKind, Expression, ForStatementInit,
    ForStatementLeft, Literal, LocalDecl, ModuleBuilder, Pattern, SetProjectionOp, SmeltError,
    Statement, Stmt, Type,
};

impl ModuleBuilder<'_> {
    /// Lower a C-style TypeScript `for` statement into HIR control-flow blocks.
    pub(in crate::lowering) fn c_for_statement(
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
                    if let Some(set) = self.try_global_assignment_expression(assign, body)? {
                        body.push_stmt_to_block(block, Stmt::Expr(set));
                    } else {
                        let (target, value) = self.assignment_parts(assign, body)?;
                        body.push_stmt_to_block(block, Stmt::Assign { target, value });
                    }
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
            // Lower the update into its OWN block rather than appending it to the
            // loop body. MIR turns that update block into a latch that becomes
            // the `continue` target, so a `continue` inside the body re-runs the
            // update before re-testing the condition. Appending to the body would
            // make `continue` skip the update and spin forever.
            let update_block = body.push_block(self.expression_span(update));
            self.push_for_update(update, body, update_block)?;
            body.push_stmt_to_block(
                block,
                Stmt::WhileUpdateBlock {
                    cond,
                    body: loop_body,
                    update: update_block,
                },
            );
        } else {
            body.push_stmt_to_block(
                block,
                Stmt::While {
                    cond,
                    body: loop_body,
                },
            );
        }
        Ok(())
    }

    /// Lower a C-style `for` loop update clause into assignment statements
    /// appended to the given `loop_body` block (in practice the dedicated update
    /// block created by [`Self::c_for_statement`]).
    ///
    /// The update is either a single assignment (`i += 2`), an increment or
    /// decrement (`i++`, `--j`), or a comma sequence of those forms
    /// (`step++, resultIndex++`). A sequence evaluates each sub-expression
    /// left-to-right, so each one is lowered into its own `Assign` statement in
    /// source order.
    pub(in crate::lowering) fn push_for_update(
        &mut self,
        update: &Expression<'_>,
        body: &mut Body,
        loop_body: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        match update {
            Expression::SequenceExpression(sequence) => {
                for sub_update in &sequence.expressions {
                    self.push_for_update(sub_update, body, loop_body)?;
                }
                Ok(())
            }
            Expression::AssignmentExpression(assign) => {
                if let Some(set) = self.try_global_assignment_expression(assign, body)? {
                    body.push_stmt_to_block(loop_body, Stmt::Expr(set));
                    return Ok(());
                }
                let (target, value) = self.assignment_parts(assign, body)?;
                body.push_stmt_to_block(loop_body, Stmt::Assign { target, value });
                Ok(())
            }
            Expression::UpdateExpression(update_expr) => {
                if let Some(set) = self.try_global_update_expression(update_expr, body, false)? {
                    body.push_stmt_to_block(loop_body, Stmt::Expr(set));
                    return Ok(());
                }
                let (target, value) = self.update_parts(update_expr, body)?;
                body.push_stmt_to_block(loop_body, Stmt::Assign { target, value });
                Ok(())
            }
            _ => Err(SmeltError::unsupported(
                self.expression_span(update),
                "for-loop update must be assignment or increment/decrement",
            )),
        }
    }

    /// Extract pattern from for-of left side.
    pub(in crate::lowering) fn for_left_pattern(
        &mut self,
        left: &ForStatementLeft<'_>,
        iter_ty: smelt_hir::TypeId,
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
        let ty = if let Some(annotation) = &declarator.type_annotation {
            self.ts_type_to_hir(&annotation.type_annotation)?
        } else {
            self.index_type(iter_ty)?
        };
        let name = binding.name.as_str();
        let symbol = self.intern_source_name(name);
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: true,
            span: self.span(binding.span.start, binding.span.end),
        });
        self.scope.bind(name.to_owned(), local);
        Ok(body.push_pattern(Pattern::Binding(local)))
    }

    /// Extract a destructured for-of binding, lowering the loop item into a temp local.
    ///
    /// MIR for-loops currently assign each iteration item into a single binding
    /// pattern. For destructuring, Smelt binds that item to a compiler-generated
    /// local and inserts ordinary destructuring `let` statements at the start of
    /// the loop body.
    pub(in crate::lowering) fn for_left_destructuring<'a>(
        &mut self,
        left: &'a ForStatementLeft<'a>,
        iter_ty: smelt_hir::TypeId,
        body: &mut Body,
    ) -> Result<
        Option<(
            smelt_hir::PatternId,
            smelt_hir::ExprId,
            &'a BindingPattern<'a>,
            Option<smelt_hir::TypeId>,
            bool,
        )>,
        SmeltError,
    > {
        let ForStatementLeft::VariableDeclaration(decl) = left else {
            return Ok(None);
        };
        if decl.declarations.len() != 1 {
            return Ok(None);
        }
        let Some(declarator) = decl.declarations.first() else {
            return Ok(None);
        };
        if matches!(declarator.id, BindingPattern::BindingIdentifier(_)) {
            return Ok(None);
        }
        let item_ty = declarator
            .type_annotation
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .unwrap_or(self.index_type(iter_ty)?);
        let symbol = self.intern_source_name("__smelt_for_item");
        let local = body.push_local(LocalDecl {
            name: Some(symbol),
            ty: item_ty,
            mutable: true,
            span: self.span(declarator.span.start, declarator.span.end),
        });
        let value = body.push_expr(Expr {
            kind: ExprKind::Local(local),
            ty: item_ty,
            span: self.span(declarator.span.start, declarator.span.end),
        });
        let pat = body.push_pattern(Pattern::Binding(local));
        let mutable = decl.kind == oxc::ast::ast::VariableDeclarationKind::Let;
        Ok(Some((pat, value, &declarator.id, Some(item_ty), mutable)))
    }

    /// Lower a `for…of` over a synchronous generator via the resume protocol.
    ///
    /// A generator has no indexable Rust representation, so the ordinary
    /// index-based [`Stmt::For`] lowering cannot drive it. Instead desugar the
    /// loop to the standard iterator protocol reusing the same
    /// `GeneratorNext`/`GeneratorDone`/`GeneratorValue` machinery that `.next()`
    /// lowers to: the generator handle is stored in a stable local (so it is
    /// resumed, not re-created), one resume is primed before the loop, the
    /// condition tests "not done", each iteration binds the yielded value, and
    /// the latch resumes again. Wiring the resume onto the latch — which
    /// `WhileUpdate` makes the `continue` target — keeps `continue` advancing
    /// the generator rather than spinning on a stale step.
    ///
    /// Only concrete `Generator` receivers reach here; async generators keep the
    /// existing unsupported path because their resume returns a future.
    pub(in crate::lowering) fn lower_generator_for_of(
        &mut self,
        for_stmt: &oxc::ast::ast::ForOfStatement<'_>,
        iter: smelt_hir::ExprId,
        yield_ty: smelt_hir::TypeId,
        return_ty: smelt_hir::TypeId,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let ForStatementLeft::VariableDeclaration(decl) = &for_stmt.left else {
            return Err(SmeltError::unsupported(
                self.span(for_stmt.left.span().start, for_stmt.left.span().end),
                "for...of targets must be variable declarations for now",
            ));
        };
        let [declarator] = decl.declarations.as_slice() else {
            return Err(SmeltError::unsupported(
                self.span(decl.span.start, decl.span.end),
                "for...of currently supports exactly one loop binding",
            ));
        };
        let annotated_ty = declarator
            .type_annotation
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?;
        let mutable = decl.kind == oxc::ast::ast::VariableDeclarationKind::Let;

        let iter_span = self.expression_span(&for_stmt.right);
        let generator_ty = Self::expr_ty(body, iter);
        let result_ty = self.ctx.krate.types.intern(Type::GeneratorResult {
            yield_ty,
            return_ty,
        });
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);

        // Store the generator handle so every resume drives the same state
        // machine rather than re-evaluating the producing call.
        let generator_symbol = self.intern_source_name("__smelt_generator");
        let generator_local = body.push_local(LocalDecl {
            name: Some(generator_symbol),
            ty: generator_ty,
            mutable: false,
            span: iter_span,
        });
        let generator_pat = body.push_pattern(Pattern::Binding(generator_local));
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat: generator_pat,
                ty: generator_ty,
                value: Some(iter),
            },
        );

        // Prime the first resume before the loop so the initial condition test
        // sees a real step.
        let step_symbol = self.intern_source_name("__smelt_generator_step");
        let first_resume = self.generator_resume_expr(generator_local, generator_ty, iter_span, body);
        let step_local = body.push_local(LocalDecl {
            name: Some(step_symbol),
            ty: result_ty,
            mutable: true,
            span: iter_span,
        });
        let step_pat = body.push_pattern(Pattern::Binding(step_local));
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat: step_pat,
                ty: result_ty,
                value: Some(first_resume),
            },
        );

        // Loop body: bind the yielded value, then run the source statements.
        let loop_body = body.push_block(self.statement_span(&for_stmt.body));
        let step_expr = body.push_expr(Expr {
            kind: ExprKind::Local(step_local),
            ty: result_ty,
            span: iter_span,
        });
        let value_expr = body.push_expr(Expr {
            kind: ExprKind::GeneratorValue { result: step_expr },
            ty: yield_ty,
            span: iter_span,
        });
        self.binding_declaration(
            &declarator.id,
            Some(value_expr),
            annotated_ty,
            mutable,
            body,
            loop_body,
        )?;
        if let Statement::BlockStatement(block_stmt) = &for_stmt.body {
            for nested_statement in &block_stmt.body {
                self.statement_in_block(nested_statement, body, loop_body)?;
            }
        } else {
            self.statement_in_block(&for_stmt.body, body, loop_body)?;
        }

        // Condition: continue while the primed step has not completed.
        let done_step_expr = body.push_expr(Expr {
            kind: ExprKind::Local(step_local),
            ty: result_ty,
            span: iter_span,
        });
        let done_expr = body.push_expr(Expr {
            kind: ExprKind::GeneratorDone {
                result: done_step_expr,
            },
            ty: bool_ty,
            span: iter_span,
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::UnaryOp {
                op: smelt_hir::UnaryOp::Not,
                operand: done_expr,
            },
            ty: bool_ty,
            span: iter_span,
        });

        // Latch: resume the generator into the step local before re-testing.
        let update_target = body.push_expr(Expr {
            kind: ExprKind::Local(step_local),
            ty: result_ty,
            span: iter_span,
        });
        let update_value = self.generator_resume_expr(generator_local, generator_ty, iter_span, body);

        body.push_stmt_to_block(
            block,
            Stmt::WhileUpdate {
                cond,
                body: loop_body,
                update_target,
                update_value,
            },
        );
        Ok(())
    }

    /// Build a `generator.next()` HIR expression resuming the stored handle.
    ///
    /// Shared by the primed resume before a generator `for…of` loop and the
    /// latch resume that advances it; both read the generator local and emit a
    /// `Next` command carrying no sent value (the loop never passes one).
    fn generator_resume_expr(
        &mut self,
        generator_local: smelt_hir::LocalId,
        generator_ty: smelt_hir::TypeId,
        span: smelt_hir::Span,
        body: &mut Body,
    ) -> smelt_hir::ExprId {
        let result_ty = match self.ctx.krate.types.get(generator_ty) {
            Some(Type::Generator {
                yield_ty,
                return_ty,
                ..
            }) => {
                let (yield_ty, return_ty) = (*yield_ty, *return_ty);
                self.ctx.krate.types.intern(Type::GeneratorResult {
                    yield_ty,
                    return_ty,
                })
            }
            _ => self.ctx.krate.types.intern(Type::Unknown),
        };
        let generator_expr = body.push_expr(Expr {
            kind: ExprKind::Local(generator_local),
            ty: generator_ty,
            span,
        });
        body.push_expr(Expr {
            kind: ExprKind::GeneratorNext {
                generator: generator_expr,
                value: None,
                kind: smelt_hir::GeneratorResumeKind::Next,
            },
            ty: result_ty,
            span,
        })
    }

    /// Adapt TypeScript for-of iterables whose Rust representation is not indexable.
    pub(in crate::lowering) fn for_of_iterable(
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
            Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) => {
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
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let ty = self.ctx.krate.types.intern(Type::List(item_ty));
                body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: iter },
                    ty,
                    span: self.expression_span(source),
                })
            }
            _ => iter,
        }
    }

    /// Lower a TypeScript `for...in` object into an iterable list of keys.
    ///
    /// JavaScript iterates enumerable property names. Smelt models record-like
    /// objects with dictionaries, so `for (const key in object)` becomes a
    /// dictionary key projection and then reuses the ordinary HIR `For` loop.
    pub(in crate::lowering) fn for_in_iterable(
        &mut self,
        source: &Expression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let mut object = self.expression(source, body)?;
        let object_ty = self.type_param_constraint_or_self(Self::expr_ty(body, object));
        match self.ctx.krate.types.get(object_ty).cloned() {
            Some(Type::Dict(key_ty, _)) => {
                let list_ty = self.ctx.krate.types.intern(Type::List(key_ty));
                Ok(body.push_expr(Expr {
                    kind: ExprKind::DictProjection {
                        op: DictProjectionOp::ForInKeys,
                        dict: object,
                    },
                    ty: list_ty,
                    span: self.expression_span(source),
                }))
            }
            Some(Type::Function(_)) => {
                // A `for...in` over a function value (`function fn() {}; fn.a = 1;
                // for (const key in fn)`, the lodash-compat idiom es-toolkit's
                // `defaultsDeep` spec iterates). Smelt models a function as a
                // property-less closure, so it exposes no enumerable own string
                // keys: the loop iterates the empty list. This is the sound
                // projection for Smelt's function representation and compiles
                // cleanly (a closure has no `SmeltUnknown`/record view to cast
                // into); attaching enumerable properties to a function is a
                // dynamic-JS shape Smelt does not yet model.
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let list_ty = self.ctx.krate.types.intern(Type::List(key_ty));
                Ok(body.push_expr(Expr {
                    kind: ExprKind::ListLit(Vec::new()),
                    ty: list_ty,
                    span: self.expression_span(source),
                }))
            }
            Some(Type::Unknown | Type::Class { .. } | Type::TypeParam { .. } |
Type::Optional(_)) => {
                // A `for...in` over an erased object surface — an unconstrained
                // generic `T`, an erased object, or an optional object —
                // iterates string-keyed properties, so it is cast to
                // `Record<string, unknown>`.
                let key_ty = self.ctx.krate.types.intern(Type::String);
                let value_ty = self.ctx.krate.types.intern(Type::Unknown);
                let dict_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
                object = body.push_expr(Expr {
                    kind: ExprKind::UnknownCast {
                        value: object,
                        target: dict_ty,
                    },
                    ty: dict_ty,
                    span: self.expression_span(source),
                });
                let list_ty = self.ctx.krate.types.intern(Type::List(key_ty));
                Ok(body.push_expr(Expr {
                    kind: ExprKind::DictProjection {
                        op: DictProjectionOp::ForInKeys,
                        dict: object,
                    },
                    ty: list_ty,
                    span: self.expression_span(source),
                }))
            }
            _ => Err(SmeltError::unsupported(
                self.expression_span(source),
                "for...in is only lowered for record-like objects",
            )),
        }
    }

    /// Convert a switch case label expression to a literal.
    ///
    /// Beyond direct literals, lodash/es-toolkit compat code labels cases with
    /// references to module string constants such as `case stringTag:` where
    /// `export const stringTag = '[object String]'`. These fold to the same
    /// literal at compile time, so resolve any constant-foldable label through
    /// the shared exported-const folder before failing.
    pub(in crate::lowering) fn literal_case_label(&mut self, expression: &Expression<'_>) -> Result<Literal, SmeltError> {
        match expression {
            Expression::StringLiteral(lit) => Ok(Literal::String(lit.value.to_string())),
            Expression::NumericLiteral(lit) => Ok(Literal::Float(lit.value)),
            Expression::BooleanLiteral(lit) => Ok(Literal::Bool(lit.value)),
            Expression::NullLiteral(_) => Ok(Literal::None),
            _ => match self.literal_const_expression(expression) {
                Ok(value) => Ok(value.literal),
                Err(_) => Err(SmeltError::unsupported(
                    self.expression_span(expression),
                    "switch case labels must be string, number, boolean, or null literals",
                )),
            },
        }
    }

    /// Lower an expression without type hint.
    pub(in crate::lowering) fn expression(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        self.expression_with_hint(expression, body, None)
    }

    // Continued in the next split builder file.
}
