//! Statement, block, and control-flow terminator lowering.
//!
//! These [`LoweringCtx`] methods drive the block cursor: they lower the
//! statements of a HIR block in order, translate each [`HirStmt`] variant, and
//! build the basic-block CFG for structured control flow (`if`, `while`,
//! `while`-with-update, `for`, `match`, and `try`/`catch`/`finally`). Each
//! control-flow lowerer allocates the MIR blocks it needs, wires their
//! terminators, and leaves `current_block` pointing at the join/continuation
//! block so the caller can keep appending statements.

use std::collections::HashMap;

use smelt_hir::{ExprId, LocalId as HirLocalId, Stmt as HirStmt};

use crate::{BlockId, Constant, HirOrigin, LocalId, Operand, Place, Rvalue, Statement, Terminator};

use super::context::{ExceptionTarget, LoopTargets, LoweringCtx};
use super::{LoweredFunction, LowerError, lower_literal, usize_from_u32};

impl LoweringCtx<'_> {
    /// Lowers the entire function body to MIR.
    pub(super) fn lower(mut self) -> Result<LoweredFunction, LowerError> {
        if let HirOrigin::ClassConstructor { class, .. } = self.function.origin {
            let Some(this) = self.function.locals.first().map(|_| LocalId(0)) else {
                return Err(self.error("constructor is missing synthetic this local", None));
            };
            self.block_mut()?.statements.push(Statement::Assign {
                dest: this,
                value: Rvalue::Struct {
                    class,
                    fields: Vec::new(),
                },
            });
        }
        self.lower_block_stmts(self.body.root)?;

        let root = self.hir_block(self.body.root)?;
        if self.block()?.terminator.is_some() {
            return Ok((self.function, self.closures));
        }

        if let Some(tail) = root.tail {
            let operand = self.lower_expr(tail)?;
            self.set_terminator(Terminator::Return(operand))?;
        } else {
            if matches!(self.function.origin, HirOrigin::ClassConstructor { .. }) {
                self.set_terminator(Terminator::Return(Operand::Move(Place::Local(LocalId(0)))))?;
                return Ok((self.function, self.closures));
            }
            self.set_terminator(Terminator::Return(Operand::Const(Constant::None)))?;
        }

        Ok((self.function, self.closures))
    }

    /// Lowers all statements in a HIR block.
    pub(super) fn lower_block_stmts(&mut self, block_id: smelt_hir::BlockId) -> Result<(), LowerError> {
        let block = self.hir_block(block_id)?;
        let stmt_ids = block.stmts.clone();
        for stmt_id in stmt_ids {
            if self.block()?.terminator.is_some() {
                break;
            }
            let stmt = self.hir_stmt(stmt_id)?.clone();
            self.lower_stmt(&stmt)?;
        }
        Ok(())
    }

    /// Lowers a single HIR statement to MIR.
    pub(super) fn lower_stmt(&mut self, stmt: &HirStmt) -> Result<(), LowerError> {
        match stmt {
            HirStmt::Let { pat, value, .. } => {
                let hir_local = match self.hir_pattern(*pat)? {
                    smelt_hir::Pattern::Binding(hir_local) => *hir_local,
                    smelt_hir::Pattern::Wildcard
                    | smelt_hir::Pattern::Tuple(_)
                    | smelt_hir::Pattern::Literal(_) => {
                        return Err(
                            self.error("only binding let patterns can lower to MIR yet", None)
                        );
                    }
                };
                let Some(dest) = self.locals.get(&hir_local).copied() else {
                    return Err(self.error("let pattern references an unknown local", None));
                };
                // An uninitialized `let` binding holds `undefined` in JS, not
                // `null`. When the type is erased to `SmeltUnknown` this distinction
                // is observable: code that later guards the binding with
                // `x !== undefined` (e.g. `truncate`'s regex-separator loop) must
                // see the `Undefined` tag, not `Null`. Typed targets coerce both
                // constants to the same `default_value`, so this only changes the
                // erased case, where `Undefined` is the JS-correct sentinel.
                let lowered_value = value
                    .map(|expr| self.lower_expr(expr))
                    .transpose()?
                    .unwrap_or(Operand::Const(Constant::Undefined));
                self.block_mut()?.statements.push(Statement::Assign {
                    dest,
                    value: Rvalue::Use(lowered_value),
                });
                Ok(())
            }
            HirStmt::Assign { target, value } => {
                let place = self.lower_place(*target)?;
                let lowered_value = self.lower_expr(*value)?;
                self.block_mut()?.statements.push(Statement::AssignPlace {
                    place,
                    value: Rvalue::Use(lowered_value),
                });
                Ok(())
            }
            HirStmt::Expr(expr) => {
                self.lower_expr(*expr)?;
                Ok(())
            }
            HirStmt::Return(Some(expr)) => {
                let lowered_operand = self.lower_expr(*expr)?;
                self.set_terminator(Terminator::Return(lowered_operand))?;
                Ok(())
            }
            HirStmt::Return(None) => {
                self.set_terminator(Terminator::Return(Operand::Const(Constant::None)))?;
                Ok(())
            }
            HirStmt::If {
                cond,
                then_block,
                else_block,
            } => self.lower_if(*cond, *then_block, *else_block),
            HirStmt::Match {
                scrutinee,
                arms,
                default,
            } => self.lower_match(*scrutinee, arms, *default),
            HirStmt::While { cond, body } => self.lower_while(*cond, *body),
            HirStmt::WhileUpdate {
                cond,
                body,
                update_target,
                update_value,
            } => self.lower_while_update(*cond, *body, *update_target, *update_value),
            HirStmt::WhileUpdateBlock { cond, body, update } => {
                self.lower_while_update_block(*cond, *body, *update)
            }
            HirStmt::For { pat, iter, body } => self.lower_for(*pat, *iter, *body),
            HirStmt::Throw(expr) => {
                let lowered_operand = self.lower_expr(*expr)?;
                if let Some(target) = self.exception_targets.last().copied() {
                    if let Some(exception_local) = target.exception_local {
                        self.block_mut()?.statements.push(Statement::Assign {
                            dest: exception_local,
                            value: Rvalue::Use(lowered_operand),
                        });
                    }
                    self.set_terminator(Terminator::Goto(target.catch_block))?;
                } else {
                    self.set_terminator(Terminator::Throw(lowered_operand))?;
                }
                Ok(())
            }
            HirStmt::TryCatch {
                body,
                catch_binding,
                catch_body,
                finally_body,
            } => self.lower_try_catch(*body, *catch_binding, *catch_body, *finally_body),
            HirStmt::Break => {
                let Some(targets) = self.loops.last().copied() else {
                    return Err(self.error("break used outside a loop", None));
                };
                self.set_terminator(Terminator::Goto(targets.break_target))?;
                Ok(())
            }
            HirStmt::Continue => {
                let Some(targets) = self.loops.last().copied() else {
                    return Err(self.error("continue used outside a loop", None));
                };
                self.set_terminator(Terminator::Goto(targets.continue_target))?;
                Ok(())
            }
        }
    }

    /// Lowers try/catch/finally into explicit catch and cleanup blocks.
    pub(super) fn lower_try_catch(
        &mut self,
        body_hir: smelt_hir::BlockId,
        catch_binding: Option<HirLocalId>,
        catch_body_hir: Option<smelt_hir::BlockId>,
        finally_body_hir: Option<smelt_hir::BlockId>,
    ) -> Result<(), LowerError> {
        let try_span = self.hir_block(body_hir)?.span;
        let try_block = self.function.push_block(try_span);
        let after = self.function.push_block(self.block()?.span);
        let finally_mir_block = finally_body_hir
            .map(|block| {
                let span = self.hir_block(block)?.span;
                Ok(self.function.push_block(span))
            })
            .transpose()?;
        let normal_exit = finally_mir_block.unwrap_or(after);
        let catch_mir_block = catch_body_hir
            .map(|block| {
                let span = self.hir_block(block)?.span;
                Ok(self.function.push_block(span))
            })
            .transpose()?;

        self.set_terminator(Terminator::Goto(try_block))?;

        if let Some(finally_block) = finally_mir_block {
            self.generator_cleanups.push(crate::GeneratorCleanup {
                block: finally_block,
                after,
            });
        }

        if let Some(catch_block) = catch_mir_block {
            let exception_local = catch_binding
                .map(|local| {
                    self.locals.get(&local).copied().ok_or_else(|| {
                        self.error("catch binding references an unknown local", None)
                    })
                })
                .transpose()?;
            self.exception_targets.push(ExceptionTarget {
                catch_block,
                exception_local,
            });
        }

        self.current_block = try_block;
        self.lower_block_stmts(body_hir)?;
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(normal_exit))?;
        }

        if catch_mir_block.is_some() {
            self.exception_targets.pop();
        }

        if let (Some(catch_hir), Some(catch_block)) = (catch_body_hir, catch_mir_block) {
            self.current_block = catch_block;
            self.lower_block_stmts(catch_hir)?;
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(normal_exit))?;
            }
        }

        if finally_mir_block.is_some() {
            self.generator_cleanups.pop();
        }

        if let (Some(finally_hir), Some(finally_block)) = (finally_body_hir, finally_mir_block) {
            self.current_block = finally_block;
            self.lower_block_stmts(finally_hir)?;
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(after))?;
            }
        }

        self.current_block = after;
        Ok(())
    }

    /// Lowers an if statement to MIR with switch terminator.
    pub(super) fn lower_if(
        &mut self,
        cond: ExprId,
        then_hir: smelt_hir::BlockId,
        else_hir: Option<smelt_hir::BlockId>,
    ) -> Result<(), LowerError> {
        let lowered_cond = self.lower_expr(cond)?;
        let then_span = self.hir_block(then_hir)?.span;
        let else_span = if let Some(block) = else_hir {
            self.hir_block(block)?.span
        } else {
            self.block()?.span
        };
        let then_mir = self.function.push_block(then_span);
        let else_mir = self.function.push_block(else_span);
        self.set_terminator(Terminator::Switch {
            cond: lowered_cond,
            then_block: then_mir,
            else_block: else_mir,
        })?;

        self.current_block = then_mir;
        self.lower_block_stmts(then_hir)?;

        if let Some(else_block_hir) = else_hir {
            let join = self.function.push_block(self.block()?.span);
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(join))?;
            }

            self.current_block = else_mir;
            self.lower_block_stmts(else_block_hir)?;
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(join))?;
            }
            self.current_block = join;
        } else {
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(else_mir))?;
            }
            self.current_block = else_mir;
        }

        Ok(())
    }

    /// Lowers a while loop to MIR with switch terminator.
    pub(super) fn lower_while(
        &mut self,
        cond: ExprId,
        body_hir: smelt_hir::BlockId,
    ) -> Result<(), LowerError> {
        let preheader_span = self.block()?.span;
        let header = self.function.push_block(preheader_span);
        self.set_terminator(Terminator::Goto(header))?;
        self.current_block = header;
        let body_span = self.hir_block(body_hir)?.span;
        let body_mir = self.function.push_block(body_span);
        let after = self.function.push_block(preheader_span);
        let lowered_cond = self.lower_expr(cond)?;
        self.set_terminator(Terminator::Switch {
            cond: lowered_cond,
            then_block: body_mir,
            else_block: after,
        })?;

        self.loops.push(LoopTargets {
            break_target: after,
            continue_target: header,
        });
        self.current_block = body_mir;
        self.lower_block_stmts(body_hir)?;
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(header))?;
        }
        self.loops.pop();
        self.current_block = after;
        Ok(())
    }

    /// Lowers a while loop whose next-condition update must run on `continue`.
    pub(super) fn lower_while_update(
        &mut self,
        cond: ExprId,
        body_hir: smelt_hir::BlockId,
        update_target: ExprId,
        update_value: ExprId,
    ) -> Result<(), LowerError> {
        let preheader_span = self.block()?.span;
        let header = self.function.push_block(preheader_span);
        self.set_terminator(Terminator::Goto(header))?;
        self.current_block = header;
        let body_span = self.hir_block(body_hir)?.span;
        let body_mir = self.function.push_block(body_span);
        let latch = self.function.push_block(body_span);
        let after = self.function.push_block(preheader_span);
        let lowered_cond = self.lower_expr(cond)?;
        self.set_terminator(Terminator::Switch {
            cond: lowered_cond,
            then_block: body_mir,
            else_block: after,
        })?;

        self.loops.push(LoopTargets {
            break_target: after,
            continue_target: latch,
        });
        self.current_block = body_mir;
        self.lower_block_stmts(body_hir)?;
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(latch))?;
        }
        self.loops.pop();

        self.current_block = latch;
        let place = self.lower_place(update_target)?;
        let value = self.lower_expr(update_value)?;
        self.block_mut()?.statements.push(Statement::AssignPlace {
            place,
            value: Rvalue::Use(value),
        });
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(header))?;
        }
        self.current_block = after;
        Ok(())
    }

    /// Lowers a C-style `for` desugaring (while loop with an update *block*).
    ///
    /// The update block is lowered into a dedicated latch block that is the
    /// `continue` target, so a `continue` inside the body runs the update before
    /// re-testing the condition. This is the general fix for C-style `for`
    /// loops: appending the update to the body would let `continue` skip it and
    /// spin forever. Mirrors [`Self::lower_for`]'s `update_block` wiring, but the
    /// latch runs an arbitrary HIR block instead of a fixed index increment.
    pub(super) fn lower_while_update_block(
        &mut self,
        cond: ExprId,
        body_hir: smelt_hir::BlockId,
        update_hir: smelt_hir::BlockId,
    ) -> Result<(), LowerError> {
        let preheader_span = self.block()?.span;
        let header = self.function.push_block(preheader_span);
        self.set_terminator(Terminator::Goto(header))?;
        self.current_block = header;
        let body_span = self.hir_block(body_hir)?.span;
        let body_mir = self.function.push_block(body_span);
        let latch = self.function.push_block(body_span);
        let after = self.function.push_block(preheader_span);
        let lowered_cond = self.lower_expr(cond)?;
        self.set_terminator(Terminator::Switch {
            cond: lowered_cond,
            then_block: body_mir,
            else_block: after,
        })?;

        self.loops.push(LoopTargets {
            break_target: after,
            continue_target: latch,
        });
        self.current_block = body_mir;
        self.lower_block_stmts(body_hir)?;
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(latch))?;
        }
        self.loops.pop();

        self.current_block = latch;
        self.lower_block_stmts(update_hir)?;
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(header))?;
        }
        self.current_block = after;
        Ok(())
    }

    /// Lowers a for loop to MIR with index-based iteration.
    pub(super) fn lower_for(
        &mut self,
        pat: smelt_hir::PatternId,
        iter: ExprId,
        body_hir: smelt_hir::BlockId,
    ) -> Result<(), LowerError> {
        let hir_local = match self.hir_pattern(pat)? {
            smelt_hir::Pattern::Binding(hir_local) => *hir_local,
            smelt_hir::Pattern::Wildcard
            | smelt_hir::Pattern::Tuple(_)
            | smelt_hir::Pattern::Literal(_) => {
                return Err(self.error("only binding for patterns can lower to MIR yet", None));
            }
        };
        let item_local = self
            .locals
            .get(&hir_local)
            .copied()
            .ok_or_else(|| self.error("for pattern references an unknown local", None))?;
        let iter_operand = self.lower_expr(iter)?;
        let iter_span = self.hir_expr(iter)?.span;
        let iter_local = self.local_operand(iter_operand, iter_span)?;
        let float_ty = self.loop_index_ty;
        let bool_ty = self.loop_bool_ty;
        let idx = self.push_temp(float_ty, iter_span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: idx,
            value: Rvalue::Use(Operand::Const(Constant::Float(0.0))),
        });

        let header = self.function.push_block(self.block()?.span);
        self.set_terminator(Terminator::Goto(header))?;
        self.current_block = header;
        let len = self.push_temp(float_ty, self.block()?.span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: len,
            value: Rvalue::Len(Operand::Copy(Place::Local(iter_local))),
        });
        let cond = self.push_temp(bool_ty, self.block()?.span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: cond,
            value: Rvalue::Binary {
                op: smelt_hir::BinOp::Lt,
                lhs: Operand::Copy(Place::Local(idx)),
                rhs: Operand::Copy(Place::Local(len)),
            },
        });
        let body_mir = self.function.push_block(self.hir_block(body_hir)?.span);
        let update_block = self.function.push_block(self.block()?.span);
        let after = self.function.push_block(self.block()?.span);
        self.set_terminator(Terminator::Switch {
            cond: Operand::Copy(Place::Local(cond)),
            then_block: body_mir,
            else_block: after,
        })?;

        self.loops.push(LoopTargets {
            break_target: after,
            continue_target: update_block,
        });
        self.current_block = body_mir;
        self.block_mut()?.statements.push(Statement::Assign {
            dest: item_local,
            value: Rvalue::Use(Operand::Copy(Place::Index {
                base: iter_local,
                index: Box::new(Operand::Copy(Place::Local(idx))),
            })),
        });
        self.lower_block_stmts(body_hir)?;
        if self.block()?.terminator.is_none() {
            self.set_terminator(Terminator::Goto(update_block))?;
        }
        self.loops.pop();
        self.current_block = update_block;
        let one = Operand::Const(Constant::Float(1.0));
        self.block_mut()?.statements.push(Statement::AssignPlace {
            place: Place::Local(idx),
            value: Rvalue::Binary {
                op: smelt_hir::BinOp::Add,
                lhs: Operand::Copy(Place::Local(idx)),
                rhs: one,
            },
        });
        self.set_terminator(Terminator::Goto(header))?;
        self.current_block = after;
        Ok(())
    }

    /// Lowers a match expression to MIR with match terminator.
    pub(super) fn lower_match(
        &mut self,
        scrutinee: ExprId,
        arms: &[smelt_hir::MatchArm],
        default: Option<smelt_hir::BlockId>,
    ) -> Result<(), LowerError> {
        let lowered_scrutinee = self.lower_expr(scrutinee)?;
        let span = self.block()?.span;
        let mut mir_arms = Vec::new();
        let mut arm_blocks = Vec::new();
        let mut targets_by_hir_block = HashMap::new();

        for arm in arms {
            let target = *targets_by_hir_block
                .entry(arm.body)
                .or_insert_with(|| self.function.push_block(span));
            mir_arms.push(crate::MatchArm {
                label: lower_literal(&arm.label),
                target,
            });
            if !arm_blocks
                .iter()
                .any(|(_, hir_block): &(BlockId, smelt_hir::BlockId)| *hir_block == arm.body)
            {
                arm_blocks.push((target, arm.body));
            }
        }

        // Allocate the shared join (continuation) block *after* the arm blocks so
        // that its block id is higher than every arm *and* the default (whether a
        // real source `default:` arm or a synthesized empty one). The Rust emitter
        // treats a `goto` to a lower-id block as a loop back-edge (following it
        // only when the target provably terminates, else emitting a fallthrough
        // return). The join is a forward continuation, never a back-edge; giving
        // it the highest id keeps every `goto join` a forward edge so the
        // post-`switch` tail is always emitted. Allocating it first (as before)
        // made the tail of a no-`default` switch whose arms end in a call/branch
        // (so no join can be hoisted) look like an unterminating back-edge, and
        // the whole post-switch tail was silently dropped.
        let default_target = if let Some(default_body) = default {
            Some(
                *targets_by_hir_block
                    .entry(default_body)
                    .or_insert_with(|| self.function.push_block(span)),
            )
        } else {
            // Reserve an empty default block now so it ranks below `join`; its
            // `goto join` terminator is filled in once `join` exists.
            Some(self.function.push_block(span))
        };

        let join = self.function.push_block(span);

        if default.is_none()
            && let Some(target) = default_target
        {
            let target_index =
                usize_from_u32(target.0, "synthesized MIR match default block index")?;
            let Some(block) = self.function.blocks.get_mut(target_index) else {
                return Err(self.error(
                    "synthesized MIR match default block is missing after allocation",
                    Some(span),
                ));
            };
            block.terminator = Some(Terminator::Goto(join));
        }
        self.set_terminator(Terminator::Match {
            scrutinee: lowered_scrutinee,
            arms: mir_arms,
            default: default_target,
        })?;

        let default_already_lowered = default.is_some_and(|default_block| {
            arm_blocks
                .iter()
                .any(|(_, hir_block): &(BlockId, smelt_hir::BlockId)| *hir_block == default_block)
        });

        for (target, hir_block) in arm_blocks {
            self.current_block = target;
            self.lower_block_stmts(hir_block)?;
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(join))?;
            }
        }

        if let (Some(target), Some(default_block)) = (default_target, default)
            && !default_already_lowered
        {
            self.current_block = target;
            self.lower_block_stmts(default_block)?;
            if self.block()?.terminator.is_none() {
                self.set_terminator(Terminator::Goto(join))?;
            }
        }

        self.current_block = join;
        Ok(())
    }
}
