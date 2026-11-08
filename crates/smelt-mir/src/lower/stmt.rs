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

use crate::{
    BlockId, Constant, ExceptionHandler, GeneratorCleanup, HirOrigin, LocalDecl, LocalId, LocalKind, NegativeIndex, Operand, Place, Rvalue, Statement,
    Terminator,
};

use super::context::LoweringCtx;
use super::scope::{ControlScope, FinallyClause, LabelScope, LoopExit, LoopTargets, Protected};
use super::{LoweredFunction, LowerError, lower_literal};

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
        if self.is_terminated()? {
            return Ok((self.function, self.closures));
        }

        if let Some(tail) = root.tail {
            let operand = self.lower_expr(tail)?;
            self.terminate(Terminator::Return(operand))?;
        } else if matches!(self.function.origin, HirOrigin::ClassConstructor { .. }) {
            self.terminate(Terminator::Return(Operand::Move(Place::Local(LocalId(0)))))?;
        } else {
            self.terminate(Terminator::Return(self.valueless_return_operand()))?;
        }

        Ok((self.function, self.closures))
    }

    /// The operand a value-less `return` yields.
    ///
    /// Falling off the end of a JavaScript function — or a bare `return;` —
    /// evaluates to `undefined`, never `null`. The two are one `Constant::None`
    /// once a typed return has erased them, but at an *erased* return type the
    /// distinction is observable: es-toolkit's `cloneDeepWith` guards its
    /// customizer with `if (cloned !== undefined) return cloned;`, so a
    /// customizer that falls through (`(v) => { if (typeof v === 'number') return
    /// v * 2; }`) used to answer `null`, satisfy the guard on its very first
    /// call, and make the whole clone collapse to `null`.
    ///
    /// Only the erased return type switches: a `None`/void return emits Rust `()`
    /// and an `Optional<T>` return needs Rust `None`, so both keep
    /// `Constant::None`. An explicit `return null` carries a `Literal::None`
    /// operand and never reaches here.
    fn valueless_return_operand(&self) -> Operand {
        if matches!(
            self.krate.types.get(self.function.return_ty),
            Some(smelt_hir::Type::Unknown)
        ) {
            return Operand::Const(Constant::Undefined);
        }
        Operand::Const(Constant::None)
    }

    /// Lowers all statements in a HIR block.
    pub(super) fn lower_block_stmts(&mut self, block_id: smelt_hir::BlockId) -> Result<(), LowerError> {
        let block = self.hir_block(block_id)?;
        let stmt_ids = block.stmts.clone();
        self.lower_stmt_run(&stmt_ids)
    }

    /// Lowers a run of statements of one block, a labelled region
    /// ([`HirStmt::LabelStart`] .. [`HirStmt::LabelEnd`]) as one unit.
    fn lower_stmt_run(&mut self, stmt_ids: &[smelt_hir::StmtId]) -> Result<(), LowerError> {
        let mut index = 0;
        while let Some(stmt_id) = stmt_ids.get(index) {
            if self.is_terminated()? {
                break;
            }
            let stmt = self.hir_stmt(*stmt_id)?.clone();
            index = index.saturating_add(1);
            if let HirStmt::LabelStart { label, loops } = stmt {
                let rest = stmt_ids.get(index..).unwrap_or_default();
                let Some(length) = rest.iter().position(|id| {
                    matches!(self.hir_stmt(*id), Ok(HirStmt::LabelEnd(end)) if *end == label)
                }) else {
                    return Err(self.error("a labelled statement has no end marker", None));
                };
                self.lower_labelled(label, loops, rest.get(..length).unwrap_or_default())?;
                index = index.saturating_add(length).saturating_add(1);
                continue;
            }
            self.lower_stmt(&stmt)?;
        }
        Ok(())
    }

    /// Lowers the statements of one labelled region with `break label` (and,
    /// for a labelled loop, `continue label`) in scope, then resumes after it.
    fn lower_labelled(
        &mut self,
        label: smelt_hir::Symbol,
        loops: bool,
        stmt_ids: &[smelt_hir::StmtId],
    ) -> Result<(), LowerError> {
        let span = self.block()?.span;
        let break_target = self.function.push_block(span);
        let enclosing = std::mem::replace(&mut self.pending_loop_label, loops.then_some(label));
        let lowered = self.with_scope(ControlScope::Label(LabelScope { label, break_target }), |this| {
            this.lower_stmt_run(stmt_ids)?;
            this.terminate_if_open(Terminator::Goto(break_target))
        });
        self.pending_loop_label = enclosing;
        lowered?;
        self.switch_to(break_target);
        Ok(())
    }

    /// Lowers a single HIR statement to MIR.
    pub(super) fn lower_stmt(&mut self, stmt: &HirStmt) -> Result<(), LowerError> {
        match stmt {
            HirStmt::Let { pat, ty, value } => {
                let hir_local = match self.hir_pattern(*pat)? {
                    smelt_hir::Pattern::Binding(hir_local) => *hir_local,
                    smelt_hir::Pattern::Tuple(_) => {
                        let Some(value_expr) = value else {
                            return Err(self.error("a destructuring let needs a value", None));
                        };
                        let span = self.block()?.span;
                        let source = self.push_temp(*ty, span);
                        let lowered = self.lower_expr(*value_expr)?;
                        self.block_mut()?.statements.push(Statement::Assign {
                            dest: source,
                            value: Rvalue::Use(lowered),
                        });
                        return self.bind_tuple_pattern(*pat, source, *ty);
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
                let (place, writebacks) = self.lower_place(*target)?;
                let lowered_value = self.lower_expr(*value)?;
                self.block_mut()?.statements.push(Statement::AssignPlace {
                    place,
                    value: Rvalue::Use(lowered_value),
                });
                // A projected receiver (`a.b[i] = v`) was copied into a
                // temporary to root the place; commit it back or the write is
                // lost for every value representation. See `PlaceWritebacks`.
                self.write_back_place_receivers(writebacks)?;
                Ok(())
            }
            HirStmt::Expr(expr) => {
                self.lower_expr(*expr)?;
                Ok(())
            }
            HirStmt::Return(Some(expr)) => {
                let lowered_operand = self.lower_expr(*expr)?;
                // A finalizer runs *after* the return expression is evaluated but
                // *before* the value leaves the function, so it may reassign the
                // very binding the operand names — `try { return x } finally { x = 2 }`
                // must still answer the original `x`. Copy such a value into a temp
                // at the expression's own type first. Constants and compiler temps
                // need no copy (no source statement can name them), which keeps the
                // MIR of every function without this shape unchanged.
                if self.has_pending_finally()
                    && !self.operand_is_finalizer_stable(&lowered_operand)?
                {
                    let expr_info = self.hir_expr(*expr)?;
                    let (ty, span) = (expr_info.ty, expr_info.span);
                    let frozen = self.push_temp(ty, span);
                    self.block_mut()?.statements.push(Statement::Assign {
                        dest: frozen,
                        value: Rvalue::Use(lowered_operand),
                    });
                    return self.lower_return(Operand::Move(Place::Local(frozen)));
                }
                self.lower_return(lowered_operand)
            }
            HirStmt::Return(None) => {
                let lowered_operand = self.valueless_return_operand();
                self.lower_return(lowered_operand)
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
                self.lower_throw(lowered_operand)
            }
            HirStmt::TryCatch {
                body,
                catch_binding,
                catch_body,
                finally_body,
            } => self.lower_try_catch(*body, *catch_binding, *catch_body, *finally_body),
            HirStmt::Break => self.lower_loop_exit(LoopExit::Break),
            HirStmt::Continue => self.lower_loop_exit(LoopExit::Continue),
            HirStmt::BreakLabel(label) => self.lower_label_exit(LoopExit::Break, *label),
            HirStmt::ContinueLabel(label) => self.lower_label_exit(LoopExit::Continue, *label),
            HirStmt::LabelStart { .. } | HirStmt::LabelEnd(_) => {
                Err(self.error("a label marker outside its block's statement run", None))
            }
        }
    }

    /// Whether an inlined `finally` clause can be trusted not to change `operand`.
    ///
    /// Constants are immutable and a compiler temporary has no source-level name,
    /// so no statement inside a finalizer can assign to either. A parameter or a
    /// user binding can be reassigned, and so can any field/index projection off
    /// one, so those must be copied before the finalizer runs.
    fn operand_is_finalizer_stable(&self, operand: &Operand) -> Result<bool, LowerError> {
        match operand {
            Operand::Const(_) => Ok(true),
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
                Ok(matches!(self.mir_local(*local)?.kind, LocalKind::Temp))
            }
            Operand::Copy(_) | Operand::Move(_) => Ok(false),
        }
    }

    /// Terminates the cursor's block with a throw of `value`: into the
    /// innermost enclosing handler when a `try` encloses the cursor, out of the
    /// function otherwise.
    fn lower_throw(&mut self, value: Operand) -> Result<(), LowerError> {
        match self.current_exception_handler() {
            Some(handler) => {
                if let Some(exception_local) = handler.exception_local {
                    self.block_mut()?.statements.push(Statement::Assign {
                        dest: exception_local,
                        value: Rvalue::Use(value),
                    });
                }
                self.terminate(Terminator::Goto(handler.catch_block))
            }
            None => self.terminate(Terminator::Throw(value)),
        }
    }

    /// Lowers try/catch/finally into explicit catch and cleanup blocks.
    ///
    /// The `try` body and the `catch` body are each lowered inside their own
    /// [`ControlScope::Protected`] region (pushed with [`Self::with_scope`]), so
    /// a throw inside either finds its handler and an early exit finds the
    /// `finally` clause through the one scope stack. The `finally` clause's
    /// exceptional-exit protocol is owned by [`FinallyPlan`].
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
                Ok((block, self.function.push_block(span)))
            })
            .transpose()?;
        let normal_exit = finally_mir_block.map_or(after, |(_, block)| block);
        let catch_mir_block = catch_body_hir
            .map(|block| {
                let span = self.hir_block(block)?.span;
                Ok(self.function.push_block(span))
            })
            .transpose()?;
        let finally = finally_mir_block
            .map(|(hir, block)| FinallyPlan::new(self, hir, block, after, try_span))
            .transpose()?;
        if let Some(plan) = &finally {
            plan.arm(self)?;
        }
        self.terminate(Terminator::Goto(try_block))?;

        let clause = finally.as_ref().map(FinallyPlan::clause);
        let catch_handler = catch_mir_block
            .map(|catch_block| {
                let exception_local = catch_binding
                    .map(|local| {
                        self.locals.get(&local).copied().ok_or_else(|| {
                            self.error("catch binding references an unknown local", None)
                        })
                    })
                    .transpose()?;
                Ok(ExceptionHandler {
                    catch_block,
                    exception_local,
                })
            })
            .transpose()?;
        let rethrow_handler = finally.as_ref().map(FinallyPlan::handler);

        // The `try` body: a throw goes to the `catch`, or — without one —
        // straight to the `finally`'s exceptional exit.
        self.switch_to(try_block);
        self.with_scope(
            ControlScope::Protected(Protected {
                handler: catch_handler.or(rethrow_handler),
                finally: clause,
            }),
            |this| {
                this.lower_block_stmts(body_hir)?;
                this.terminate_if_open(Terminator::Goto(normal_exit))
            },
        )?;

        // The `catch` body: still guarded by the `finally`, whose exceptional
        // exit now receives a throw out of the handler itself.
        if let (Some(catch_hir), Some(catch_block)) = (catch_body_hir, catch_mir_block) {
            self.switch_to(catch_block);
            self.with_scope(
                ControlScope::Protected(Protected {
                    handler: rethrow_handler,
                    finally: clause,
                }),
                |this| {
                    this.lower_block_stmts(catch_hir)?;
                    this.terminate_if_open(Terminator::Goto(normal_exit))
                },
            )?;
        }

        // The clause itself is lowered after both regions are popped, so an
        // exit written *inside* it resumes at the next enclosing clause instead
        // of re-entering this one.
        if let Some(plan) = &finally {
            plan.lower_exceptional_entry(self)?;
            plan.lower_clause(self)?;
        }

        self.switch_to(after);
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
        self.terminate(Terminator::Switch {
            cond: lowered_cond,
            then_block: then_mir,
            else_block: else_mir,
        })?;

        self.switch_to(then_mir);
        self.lower_block_stmts(then_hir)?;

        if let Some(else_block_hir) = else_hir {
            let join = self.function.push_block(self.block()?.span);
            self.terminate_if_open(Terminator::Goto(join))?;

            self.switch_to(else_mir);
            self.lower_block_stmts(else_block_hir)?;
            self.terminate_if_open(Terminator::Goto(join))?;
            self.switch_to(join);
        } else {
            self.terminate_if_open(Terminator::Goto(else_mir))?;
            self.switch_to(else_mir);
        }

        Ok(())
    }

    /// Lowers a loop body at the cursor with the loop's `break`/`continue`
    /// targets in scope, then falls through to `fallthrough` (the header or
    /// latch) unless the body already left.
    fn lower_loop_body(
        &mut self,
        body_hir: smelt_hir::BlockId,
        mut targets: LoopTargets,
        fallthrough: BlockId,
    ) -> Result<(), LowerError> {
        targets.label = self.pending_loop_label.take();
        self.with_scope(ControlScope::Loop(targets), |this| {
            this.lower_block_stmts(body_hir)?;
            this.terminate_if_open(Terminator::Goto(fallthrough))
        })
    }

    /// Lowers a while loop to MIR with switch terminator.
    pub(super) fn lower_while(
        &mut self,
        cond: ExprId,
        body_hir: smelt_hir::BlockId,
    ) -> Result<(), LowerError> {
        let preheader_span = self.block()?.span;
        let header = self.function.push_block(preheader_span);
        self.terminate(Terminator::Goto(header))?;
        self.switch_to(header);
        let body_span = self.hir_block(body_hir)?.span;
        let body_mir = self.function.push_block(body_span);
        let after = self.function.push_block(preheader_span);
        let lowered_cond = self.lower_expr(cond)?;
        self.terminate(Terminator::Switch {
            cond: lowered_cond,
            then_block: body_mir,
            else_block: after,
        })?;

        self.switch_to(body_mir);
        self.lower_loop_body(
            body_hir,
            LoopTargets {
                break_target: after,
                continue_target: header,
                label: None,
            },
            header,
        )?;
        self.switch_to(after);
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
        self.terminate(Terminator::Goto(header))?;
        self.switch_to(header);
        let body_span = self.hir_block(body_hir)?.span;
        let body_mir = self.function.push_block(body_span);
        let latch = self.function.push_block(body_span);
        let after = self.function.push_block(preheader_span);
        let lowered_cond = self.lower_expr(cond)?;
        self.terminate(Terminator::Switch {
            cond: lowered_cond,
            then_block: body_mir,
            else_block: after,
        })?;

        self.switch_to(body_mir);
        self.lower_loop_body(
            body_hir,
            LoopTargets {
                break_target: after,
                continue_target: latch,
                label: None,
            },
            latch,
        )?;

        self.switch_to(latch);
        let (place, writebacks) = self.lower_place(update_target)?;
        let value = self.lower_expr(update_value)?;
        self.block_mut()?.statements.push(Statement::AssignPlace {
            place,
            value: Rvalue::Use(value),
        });
        self.write_back_place_receivers(writebacks)?;
        self.terminate_if_open(Terminator::Goto(header))?;
        self.switch_to(after);
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
        self.terminate(Terminator::Goto(header))?;
        self.switch_to(header);
        let body_span = self.hir_block(body_hir)?.span;
        let body_mir = self.function.push_block(body_span);
        let latch = self.function.push_block(body_span);
        let after = self.function.push_block(preheader_span);
        let lowered_cond = self.lower_expr(cond)?;
        self.terminate(Terminator::Switch {
            cond: lowered_cond,
            then_block: body_mir,
            else_block: after,
        })?;

        self.switch_to(body_mir);
        self.lower_loop_body(
            body_hir,
            LoopTargets {
                break_target: after,
                continue_target: latch,
                label: None,
            },
            latch,
        )?;

        self.switch_to(latch);
        self.lower_block_stmts(update_hir)?;
        self.terminate_if_open(Terminator::Goto(header))?;
        self.switch_to(after);
        Ok(())
    }

    /// Lowers a for loop to MIR with index-based iteration.
    pub(super) fn lower_for(
        &mut self,
        pat: smelt_hir::PatternId,
        iter: ExprId,
        body_hir: smelt_hir::BlockId,
    ) -> Result<(), LowerError> {
        let iter_expr = self.hir_expr(iter)?;
        let iter_span = iter_expr.span;
        let iter_ty = iter_expr.ty;
        // A destructuring target (`for k, v in pairs`) binds each item to a
        // temporary and splits it into the pattern's locals at the top of the
        // body; see `bind_tuple_pattern`.
        let (item_local, item_pattern) = match self.hir_pattern(pat)? {
            smelt_hir::Pattern::Binding(hir_local) => {
                let local = self
                    .locals
                    .get(hir_local)
                    .copied()
                    .ok_or_else(|| self.error("for pattern references an unknown local", None))?;
                (local, None)
            }
            smelt_hir::Pattern::Tuple(_) => {
                let item_ty = match self.krate.types.get(iter_ty) {
                    Some(smelt_hir::Type::List(item) | smelt_hir::Type::Set(item)) => *item,
                    _ => {
                        return Err(self.error(
                            "a destructuring for target needs a list or set iterable",
                            Some(iter_span),
                        ));
                    }
                };
                (self.push_temp(item_ty, iter_span), Some(item_ty))
            }
        };
        let iter_operand = self.lower_expr(iter)?;
        // The loop indexes its iterable by place, so the iterable needs a base
        // it can index. A source `for (const x of a.b)` — or of a call result,
        // or of `m[k]` — hands back a projection or a value, not a local, and
        // demanding a local here rejected the program outright
        // (`for (const child of node.#patterns)` in Hono's trie router). A
        // hand-written Rust loop over `a.b` binds it first, and so does this:
        // JavaScript evaluates the iterable expression exactly once and then
        // iterates that value, so one temporary is both what the semantics ask
        // for and what the emitter needs.
        let iter_local = self.materialize_operand_local(iter_operand, iter_ty, iter_span)?;
        let float_ty = self.loop_index_ty;
        let bool_ty = self.loop_bool_ty;
        let idx = self.push_temp(float_ty, iter_span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: idx,
            value: Rvalue::Use(Operand::Const(Constant::Float(0.0))),
        });

        let header = self.function.push_block(self.block()?.span);
        self.terminate(Terminator::Goto(header))?;
        self.switch_to(header);
        let len = self.push_temp(float_ty, self.block()?.span);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: len,
            value: Rvalue::Len {
                    operand: Operand::Copy(Place::Local(iter_local)),
                },
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
        self.terminate(Terminator::Switch {
            cond: Operand::Copy(Place::Local(cond)),
            then_block: body_mir,
            else_block: after,
        })?;

        self.switch_to(body_mir);
        self.block_mut()?.statements.push(Statement::Assign {
            dest: item_local,
            value: Rvalue::Use(Operand::Copy(Place::Index {
                base: iter_local,
                index: Box::new(Operand::Copy(Place::Local(idx))),
                // A desugared `for..of` counter only ever runs 0..len, so the
                // negative rule is unreachable here either way.
                negative: NegativeIndex::OutOfRange,
            })),
        });
        if let Some(item_ty) = item_pattern {
            self.bind_tuple_pattern(pat, item_local, item_ty)?;
        }
        self.lower_loop_body(
            body_hir,
            LoopTargets {
                break_target: after,
                continue_target: update_block,
                label: None,
            },
            update_block,
        )?;
        self.switch_to(update_block);
        let one = Operand::Const(Constant::Float(1.0));
        self.block_mut()?.statements.push(Statement::AssignPlace {
            place: Place::Local(idx),
            value: Rvalue::Binary {
                op: smelt_hir::BinOp::Add,
                lhs: Operand::Copy(Place::Local(idx)),
                rhs: one,
            },
        });
        self.terminate(Terminator::Goto(header))?;
        self.switch_to(after);
        Ok(())
    }

    /// Splits the tuple in `source` (of type `source_ty`) into the locals a
    /// tuple pattern binds, element by element, nested patterns included.
    ///
    /// Each binding is a `tuple_index` read of its position, so `k, v = pair`
    /// and `for k, v in pairs` bind exactly what indexing the tuple would.
    fn bind_tuple_pattern(
        &mut self,
        pat: smelt_hir::PatternId,
        source: LocalId,
        source_ty: smelt_hir::TypeId,
    ) -> Result<(), LowerError> {
        let smelt_hir::Pattern::Tuple(items) = self.hir_pattern(pat)?.clone() else {
            return Err(self.error("internal: a tuple binding needs a tuple pattern", None));
        };
        let Some(smelt_hir::Type::Tuple(item_types)) = self.krate.types.get(source_ty).cloned() else {
            return Err(self.error("a destructured value must be a tuple", None));
        };
        if item_types.len() != items.len() {
            return Err(self.error("a tuple pattern must bind every element", None));
        }
        for (index, (item, item_ty)) in items.into_iter().zip(item_types).enumerate() {
            let value = Rvalue::TupleIndex { tuple: Operand::Copy(Place::Local(source)), index };
            match self.hir_pattern(item)?.clone() {
                smelt_hir::Pattern::Binding(hir_local) => {
                    let dest = self
                        .locals
                        .get(&hir_local)
                        .copied()
                        .ok_or_else(|| self.error("tuple pattern references an unknown local", None))?;
                    self.block_mut()?.statements.push(Statement::Assign { dest, value });
                }
                smelt_hir::Pattern::Tuple(_) => {
                    let span = self.block()?.span;
                    let nested = self.push_temp(item_ty, span);
                    self.block_mut()?.statements.push(Statement::Assign { dest: nested, value });
                    self.bind_tuple_pattern(item, nested, item_ty)?;
                }
            }
        }
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
        // it an id higher than every arm target and the default keeps every `goto join` a forward edge so the
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
            self.terminate_block(target, Terminator::Goto(join))?;
        }
        self.terminate(Terminator::Match {
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
            self.switch_to(target);
            self.lower_block_stmts(hir_block)?;
            self.terminate_if_open(Terminator::Goto(join))?;
        }

        if let (Some(target), Some(default_block)) = (default_target, default)
            && !default_already_lowered
        {
            self.switch_to(target);
            self.lower_block_stmts(default_block)?;
            self.terminate_if_open(Terminator::Goto(join))?;
        }

        self.switch_to(join);
        Ok(())
    }
}

/// The `finally` clause of one `try`, and the only writer of its
/// exceptional-exit protocol.
///
/// A `finally` runs on the EXCEPTIONAL exit too: a throw (or a rejected
/// `await`) out of the `try` body when there is no `catch`, or out of the
/// `catch` body, runs the clause and then rethrows the same value. The handler
/// stores the value in `thrown` and raises `pending`, then joins the one
/// `finally` block the normal exit uses; the clause's end rethrows when the
/// flag is up. The clause is emitted once, so its statements are not
/// duplicated per exit. Before, only the normal and `return` exits ran it, so
/// `try { await race } finally { clearTimeout(t) }` left the timer armed when
/// the race rejected.
///
/// The protocol, in the order the methods must be called:
/// 1. [`Self::arm`] at the `try` entry clears `thrown` and `pending` (both are
///    read at the clause's end on every path into it);
/// 2. the guarded regions throw into [`Self::handler`];
/// 3. [`Self::lower_exceptional_entry`] fills that handler block: raise the
///    flag, join the clause;
/// 4. [`Self::lower_clause`] lowers the clause and its exit: rethrow when the
///    flag is up, continue after the `try` otherwise.
struct FinallyPlan {
    /// The clause's HIR block.
    hir: smelt_hir::BlockId,
    /// The MIR block the clause is lowered into (the normal exit's target).
    block: BlockId,
    /// The continuation after the whole `try` statement.
    after: BlockId,
    /// The exceptional exit's handler block.
    handler_block: BlockId,
    /// The local holding the thrown value while the clause runs.
    thrown: LocalId,
    /// Its type: the erased `thrown_ty`, or a Python body's exception type.
    thrown_ty: smelt_hir::TypeId,
    /// Whether the clause was entered by a throw (and must rethrow).
    pending: LocalId,
    /// The `try` body's span, for the synthesized rethrow block.
    span: smelt_hir::Span,
}

impl FinallyPlan {
    /// Allocates the clause's handler block and its two protocol locals.
    ///
    /// The thrown value is held at the erased type `lower_hir` interns for a
    /// crate with a `finally` (`thrown_ty`). `lower_hir` derives that from the
    /// same HIR statements this lowers, so a missing type is an internal
    /// inconsistency: it is reported as an error rather than quietly lowering
    /// the clause without its exceptional exit (which would let a throw skip
    /// the cleanup).
    fn new(
        ctx: &mut LoweringCtx<'_>,
        hir: smelt_hir::BlockId,
        block: BlockId,
        after: BlockId,
        span: smelt_hir::Span,
    ) -> Result<Self, LowerError> {
        let Some((unknown_ty, name)) = ctx.thrown_ty else {
            return Err(ctx.error(
                "internal error: a `finally` clause was lowered without the crate's \
                 pending-thrown type",
                Some(span),
            ));
        };
        let thrown_ty = match (ctx.source_language(span), ctx.py_thrown_ty) {
            (smelt_hir::Language::Python, Some(py_thrown_ty)) => py_thrown_ty,
            _ => unknown_ty,
        };
        let thrown = ctx.function.push_local(LocalDecl {
            ty: thrown_ty,
            kind: LocalKind::UserBinding(name),
            span,
        });
        let pending = ctx.push_temp(ctx.loop_bool_ty, span);
        let handler_block = ctx.function.push_block(span);
        Ok(Self {
            hir,
            block,
            after,
            handler_block,
            thrown,
            thrown_ty,
            pending,
            span,
        })
    }

    /// The clause as the guarded regions see it on the scope stack.
    const fn clause(&self) -> FinallyClause {
        FinallyClause {
            hir: self.hir,
            cleanup: GeneratorCleanup {
                block: self.block,
                after: self.after,
            },
        }
    }

    /// The exception handler the guarded regions throw into.
    const fn handler(&self) -> ExceptionHandler {
        ExceptionHandler {
            catch_block: self.handler_block,
            exception_local: Some(self.thrown),
        }
    }

    /// Clears the protocol locals at the `try` entry (cursor's block).
    fn arm(&self, ctx: &mut LoweringCtx<'_>) -> Result<(), LowerError> {
        // A Python exception local has no `undefined`; it starts as an
        // exception of no class and no arguments, which no path observes.
        let cleared = if Some(self.thrown_ty) == ctx.py_thrown_ty {
            Rvalue::PyExceptionNew { classes: Vec::new(), args: Vec::new() }
        } else {
            Rvalue::Use(Operand::Const(Constant::Undefined))
        };
        let statements = &mut ctx.block_mut()?.statements;
        statements.push(Statement::Assign { dest: self.thrown, value: cleared });
        statements.push(Statement::Assign {
            dest: self.pending,
            value: Rvalue::Use(Operand::Const(Constant::Bool(false))),
        });
        Ok(())
    }

    /// Fills the handler block: raise the flag and join the clause.
    fn lower_exceptional_entry(&self, ctx: &mut LoweringCtx<'_>) -> Result<(), LowerError> {
        ctx.switch_to(self.handler_block);
        ctx.block_mut()?.statements.push(Statement::Assign {
            dest: self.pending,
            value: Rvalue::Use(Operand::Const(Constant::Bool(true))),
        });
        ctx.terminate(Terminator::Goto(self.block))
    }

    /// Lowers the clause into its block, then its exit: a rethrow of the
    /// pending value when the flag is up, the code after the `try` otherwise.
    ///
    /// An abrupt exit inside the clause (`return`, `break`, its own `throw`)
    /// wins over a pending throw, as it does in JavaScript and Python: such a
    /// path is already terminated and gets no rethrow edge.
    fn lower_clause(&self, ctx: &mut LoweringCtx<'_>) -> Result<(), LowerError> {
        ctx.switch_to(self.block);
        ctx.lower_block_stmts(self.hir)?;
        if ctx.is_terminated()? {
            return Ok(());
        }
        let rethrow_block = ctx.function.push_block(self.span);
        ctx.terminate(Terminator::Switch {
            cond: Operand::Copy(Place::Local(self.pending)),
            then_block: rethrow_block,
            else_block: self.after,
        })?;
        ctx.switch_to(rethrow_block);
        ctx.lower_throw(Operand::Copy(Place::Local(self.thrown)))
    }
}
