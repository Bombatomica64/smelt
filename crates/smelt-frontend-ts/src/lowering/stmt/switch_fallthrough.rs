//! `switch` statements whose cases fall through.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::support::statement_terminates;
use oxc::ast::ast::Statement;
use smelt_hir::{BinOp, Body, Expr, ExprKind, Literal, LocalDecl, Pattern, Stmt, Type};

impl ModuleBuilder<'_> {
    /// Return whether a switch contains a case body that can reach the next
    /// case (genuine fallthrough), which the strict `Match` lowering rejects.
    pub(in crate::lowering) fn switch_has_fallthrough(
        switch_stmt: &oxc::ast::ast::SwitchStatement<'_>,
    ) -> bool {
        let case_count = switch_stmt.cases.len();
        for (case_index, case) in switch_stmt.cases.iter().enumerate() {
            if case.consequent.is_empty() || case_index + 1 == case_count {
                continue;
            }
            // What makes a case FALL THROUGH is control reaching the next case,
            // so any statement that leaves the case body answers the question —
            // a `break` leaves the switch and a `continue` leaves for the
            // enclosing loop's next iteration. Counting only `break` sent a
            // switch whose non-last case ends in `continue` down the
            // synthetic-loop lowering, which rejects `continue` outright (it
            // would bind to the synthetic loop), so a shape the match path
            // handles correctly was reported as unimplemented.
            // `statement_terminates` deliberately does not call `break`/
            // `continue` terminating — they leave a statement sequence without
            // leaving the function — hence the explicit check here.
            let has_top_level_exit = case.consequent.iter().any(|statement| match statement {
                Statement::BreakStatement(_) | Statement::ContinueStatement(_) => true,
                Statement::BlockStatement(block_stmt) => block_stmt.body.iter().any(|nested| {
                    matches!(
                        nested,
                        Statement::BreakStatement(_) | Statement::ContinueStatement(_)
                    )
                }),
                _ => false,
            });
            if !has_top_level_exit && !case.consequent.iter().any(statement_terminates) {
                return true;
            }
        }
        false
    }

    /// Lower a switch statement with genuine fallthrough.
    ///
    /// The switch becomes a single-iteration `while` loop: each case lowers to
    /// `if matched || scrutinee === label { matched = true; <body> }`, a JS
    /// `break` inside a case body maps to the loop break, and a trailing
    /// `default` body runs whenever control reaches the end of the chain —
    /// exactly when JavaScript reaches it (no label matched, or an earlier
    /// case fell through without breaking). A loop-tail `break` makes the
    /// loop run once.
    pub(in crate::lowering) fn switch_fallthrough_statement(
        &mut self,
        switch_stmt: &oxc::ast::ast::SwitchStatement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(switch_stmt.span.start, switch_stmt.span.end);
        let case_count = switch_stmt.cases.len();
        for (case_index, case) in switch_stmt.cases.iter().enumerate() {
            if case.test.is_none() && case_index + 1 != case_count {
                return Err(SmeltError::unsupported(
                    span,
                    "switch fallthrough lowering requires the default case to be last",
                ));
            }
            // A top-level `continue` would bind to the synthetic loop instead
            // of the enclosing one; keep the strict path's rejection.
            let has_top_level_continue = case.consequent.iter().any(|statement| match statement {
                Statement::ContinueStatement(_) => true,
                Statement::BlockStatement(block_stmt) => block_stmt
                    .body
                    .iter()
                    .any(|nested| matches!(nested, Statement::ContinueStatement(_))),
                _ => false,
            });
            if has_top_level_continue {
                return Err(SmeltError::unsupported(
                    span,
                    "switch continue lowering is not implemented yet",
                ));
            }
        }
        let scrutinee = self.expression(&switch_stmt.discriminant, body)?;
        let scrutinee_ty = Self::expr_ty(body, scrutinee);
        let scrutinee_name = self.intern_source_name("__switch_value");
        let scrutinee_local = body.push_local(LocalDecl {
            name: Some(scrutinee_name),
            ty: scrutinee_ty,
            mutable: false,
            span,
        });
        let scrutinee_pat = body.push_pattern(Pattern::Binding(scrutinee_local));
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat: scrutinee_pat,
                ty: scrutinee_ty,
                value: Some(scrutinee),
            },
        );
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let matched_name = self.intern_source_name("__switch_matched");
        let matched_local = body.push_local(LocalDecl {
            name: Some(matched_name),
            ty: bool_ty,
            mutable: true,
            span,
        });
        let matched_pat = body.push_pattern(Pattern::Binding(matched_local));
        let false_expr = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span,
        });
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat: matched_pat,
                ty: bool_ty,
                value: Some(false_expr),
            },
        );
        let loop_block = body.push_block(span);
        let mut pending_label_conds: Vec<smelt_hir::ExprId> = Vec::new();
        for case in &switch_stmt.cases {
            let Some(test) = &case.test else {
                // Trailing default: runs whenever the chain reaches it.
                for case_statement in &case.consequent {
                    self.statement_in_block(case_statement, body, loop_block)?;
                }
                continue;
            };
            let label = self.literal_case_label(test)?;
            let label_ty = self.ctx.krate.types.intern(match &label {
                Literal::String(_) => Type::String,
                Literal::Bool(_) => Type::Bool,
                Literal::None => Type::None,
                _ => Type::Float,
            });
            let label_expr = body.push_expr(Expr {
                kind: ExprKind::Literal(label),
                ty: label_ty,
                span,
            });
            let scrutinee_read = body.push_expr(Expr {
                kind: ExprKind::Local(scrutinee_local),
                ty: scrutinee_ty,
                span,
            });
            let cmp = body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::JsStrictEq,
                    lhs: scrutinee_read,
                    rhs: label_expr,
                },
                ty: bool_ty,
                span,
            });
            if case.consequent.is_empty() {
                pending_label_conds.push(cmp);
                continue;
            }
            let mut cond = body.push_expr(Expr {
                kind: ExprKind::Local(matched_local),
                ty: bool_ty,
                span,
            });
            for pending in std::mem::take(&mut pending_label_conds)
                .into_iter()
                .chain(std::iter::once(cmp))
            {
                cond = body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Or,
                        lhs: cond,
                        rhs: pending,
                    },
                    ty: bool_ty,
                    span,
                });
            }
            let case_block = body.push_block(span);
            let matched_target = body.push_expr(Expr {
                kind: ExprKind::Local(matched_local),
                ty: bool_ty,
                span,
            });
            let true_expr = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty: bool_ty,
                span,
            });
            body.push_stmt_to_block(
                case_block,
                Stmt::Assign {
                    target: matched_target,
                    value: true_expr,
                },
            );
            for case_statement in &case.consequent {
                self.statement_in_block(case_statement, body, case_block)?;
            }
            body.push_stmt_to_block(
                loop_block,
                Stmt::If {
                    cond,
                    then_block: case_block,
                    else_block: None,
                },
            );
        }
        body.push_stmt_to_block(loop_block, Stmt::Break);
        let loop_cond = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        body.push_stmt_to_block(
            block,
            Stmt::While {
                cond: loop_cond,
                body: loop_block,
            },
        );
        Ok(())
    }
}
