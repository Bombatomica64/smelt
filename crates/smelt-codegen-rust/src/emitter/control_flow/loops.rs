//! Loops: recognizing `while`, `do while` and compound headers in the CFG, and
//! emitting a loop body up to its back edge or exit.

use super::*;

impl FunctionEmitter<'_> {
    /// Writes the labelled jump an edge to `target` needs when it ends an
    /// iteration of, or leaves, a generated loop other than the innermost one
    /// (a source `continue outer` / `break outer`), and answers whether it did.
    ///
    /// A `continue` to a loop that closes each iteration with a latch runs the
    /// latch's statements first, as the MIR edge into the latch does. See
    /// `ControlFlowState::enclosing_loop_jump` for which edges qualify.
    pub(in crate::emitter) fn emit_enclosing_loop_jump(
        &self,
        target: smelt_mir::BlockId,
        out: &mut String,
    ) -> Result<bool, EmitError> {
        let Some(jump) = self.flow.enclosing_loop_jump(target) else {
            return Ok(false);
        };
        self.emit_loop_jump(jump, out)?;
        Ok(true)
    }

    /// Writes `jump`: the latch statements a `continue` must run first, then
    /// the jump statement itself.
    pub(in crate::emitter) fn emit_loop_jump(&self, jump: LoopJump, out: &mut String) -> Result<(), EmitError> {
        if let Some(latch) = jump.latch() {
            for statement in &self.block(latch)?.statements {
                self.emit_statement(statement, out)?;
            }
        }
        out.push_str(&jump.statement());
        self.flow.record(Flow::Diverges);
        Ok(())
    }

    /// Closes a cycle a region walker found: `block` is already on the path
    /// being emitted.
    ///
    /// The cycle is a `continue` only when `block` heads, or ends an iteration
    /// of, the innermost generated loop, which is where a Rust `continue`
    /// goes. Any other re-entered block is a loop the walker did not
    /// recognise; a bare `continue` there restarts the wrong loop (it skipped
    /// the outer update of a source `continue outer` and never terminated),
    /// and ending the region drops the cycle. Both are silent miscompiles, so
    /// it is an error.
    fn emit_region_cycle(&self, block: smelt_mir::BlockId, out: &mut String) -> Result<(), EmitError> {
        if self.flow.is_innermost_loop_continue(block) {
            out.push_str("    continue;\n");
            return Ok(());
        }
        Err(self.unstructured_control_flow(format!("block {} closes a cycle outside the loop being emitted", block.0)))
    }

    /// Opens the loop of a `while` header: a plain `while cond` when the header
    /// block computes nothing but the condition, or a `loop` that re-runs its
    /// statements and breaks on the condition when it does.
    pub(super) fn open_while_loop(
        &self,
        block: &BasicBlock,
        cond: &str,
        cond_statement_idx: usize,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let has_header_work = block
            .statements
            .iter()
            .enumerate()
            .any(|(idx, _)| idx != cond_statement_idx);
        if !has_header_work {
            out.push_str(&format!("    while {cond} {{\n"));
            return Ok(());
        }
        out.push_str("    loop {\n");
        for statement in &block.statements {
            self.emit_statement_for_block(block, statement, out)?;
        }
        out.push_str(&format!(
            "    if !({}) {{ break; }}\n",
            self.truthy_operand_expr(&Operand::Copy(Place::Local(self.switch_cond_local(block)?)))?.delimited()
        ));
        Ok(())
    }

    /// Checks if a block starts a while loop and returns loop details.
    pub(in crate::emitter) fn while_header(
        &self,
        block: &BasicBlock,
    ) -> Result<Option<(String, smelt_mir::BlockId, smelt_mir::BlockId, usize)>, EmitError> {
        if self.while_header_with_latch(block)?.is_some() {
            return Ok(None);
        }
        let Some(Terminator::Switch {
            cond:
                Operand::Copy(Place::Local(cond_local)) | Operand::Move(Place::Local(cond_local)),
            then_block,
            else_block,
        }) = &block.terminator
        else {
            return Ok(None);
        };
        if !self.block_reaches_target(*then_block, block.id, &mut BlockIdSet::default()) {
            return Ok(None);
        }
        if !self.is_back_edge_target(*then_block, block.id) {
            return Ok(None);
        }
        if !self.block_exits_to_loop(
            self.block(*then_block)?,
            block.id,
            *else_block,
            &mut BlockIdSet::default(),
        )? {
            return Ok(None);
        }
        let Some((idx, Statement::Assign { dest, value })) = block
            .statements
            .iter()
            .enumerate()
            .rev()
            .find(|(_, statement)| matches!(statement, Statement::Assign { .. }))
        else {
            return Ok(None);
        };
        if dest != cond_local {
            return Ok(None);
        }
        // Render the condition at the switch local's (boolean) type. Using the
        // default `none` destination would make a truthiness `PrimitiveCast`
        // (`ToBool`) fall through to the unit default and emit `while ()`.
        let cond_ty = self.local_decl(*cond_local)?.ty;
        Ok(Some((
            self.rvalue_expr_for_dest(value, cond_ty)?.into_string(),
            *then_block,
            *else_block,
            idx,
        )))
    }

    /// Recognizes the header of a loop whose condition is the constant `true`
    /// (`while True:`), answering its body entry and its exit.
    ///
    /// [`Self::while_header`] only claims a header whose condition is a local
    /// the header computes; a constant condition has no such local, so inside
    /// a nested region the header was emitted as a plain `if` and its back
    /// edge re-entered it until the recursion guard cut the region off. The
    /// shape checks are the same as `while_header`'s.
    fn constant_true_loop_header(
        &self,
        block: &BasicBlock,
    ) -> Result<Option<(smelt_mir::BlockId, smelt_mir::BlockId)>, EmitError> {
        let Some(Terminator::Switch {
            cond: Operand::Const(Constant::Bool(true)),
            then_block,
            else_block,
        }) = &block.terminator
        else {
            return Ok(None);
        };
        if !self.block_reaches_target(*then_block, block.id, &mut BlockIdSet::default())
            || !self.is_back_edge_target(*then_block, block.id)
            || !self.block_exits_to_loop(
                self.block(*then_block)?,
                block.id,
                *else_block,
                &mut BlockIdSet::default(),
            )?
        {
            return Ok(None);
        }
        Ok(Some((*then_block, *else_block)))
    }

    /// Recognizes a loop whose one switch branch returns to the current block.
    ///
    /// TypeScript `do...while` commonly routes its back edge through a small
    /// latch block. Treating that edge as an ordinary conditional branch
    /// recursively repeats emitted source and eventually substitutes a default
    /// return value. The same `loop { ...; if exit { break; } }` form is valid
    /// for a pre-test header when it has the same one-sided cycle shape.
    pub(super) fn do_while_body(
        &self,
        block: &BasicBlock,
    ) -> Result<Option<(String, smelt_mir::BlockId, smelt_mir::BlockId, bool)>, EmitError> {
        let Some(Terminator::Switch {
            cond,
            then_block,
            else_block,
        }) = &block.terminator
        else {
            return Ok(None);
        };
        let then_repeats = self.block_reaches_target(*then_block, block.id, &mut BlockIdSet::default());
        let else_repeats = self.block_reaches_target(*else_block, block.id, &mut BlockIdSet::default());
        if then_repeats == else_repeats {
            return Ok(None);
        }
        if then_repeats
            && self.block_exits_to_loop(
                self.block(*then_block)?,
                block.id,
                *else_block,
                &mut BlockIdSet::default(),
            )?
        {
            return Ok(Some((
                self.truthy_operand_expr(cond)?.into_string(),
                *then_block,
                *else_block,
                true,
            )));
        }
        if else_repeats
            && self.block_exits_to_loop(
                self.block(*else_block)?,
                block.id,
                *then_block,
                &mut BlockIdSet::default(),
            )?
        {
            return Ok(Some((
                self.truthy_operand_expr(cond)?.into_string(),
                *else_block,
                *then_block,
                false,
            )));
        }
        Ok(None)
    }

    /// Returns the local that stores a structured switch condition.
    fn switch_cond_local(&self, block: &BasicBlock) -> Result<LocalId, EmitError> {
        let Some(Terminator::Switch {
            cond:
                Operand::Copy(Place::Local(cond_local)) | Operand::Move(Place::Local(cond_local)),
            ..
        }) = &block.terminator
        else {
            return Err(EmitError::new(
                "structured loop header must switch on a local",
            ));
        };
        Ok(*cond_local)
    }

    /// Checks if a block eventually exits to a loop target.
    fn block_exits_to_loop(
        &self,
        block: &BasicBlock,
        continue_target: smelt_mir::BlockId,
        break_target: smelt_mir::BlockId,
        visited: &mut BlockIdSet,
    ) -> Result<bool, EmitError> {
        let cache_key = (block.id, continue_target, break_target);
        if let Some(result) = self.caches.loop_exit_cache.borrow().get(&cache_key).copied() {
            return Ok(result);
        }
        if !visited.insert(block.id) {
            return Ok(true);
        }
        let result = match &block.terminator {
            Some(Terminator::Goto(target)) => {
                if *target == continue_target || *target == break_target {
                    Ok(true)
                } else {
                    self.block_exits_to_loop(
                        self.block(*target)?,
                        continue_target,
                        break_target,
                        visited,
                    )
                }
            }
            Some(
                Terminator::Call {
                    target, unwind, ..
                }
                | Terminator::Await {
                    target, unwind, ..
                },
            ) => {
                // A throwing call and an `await` both fork: the normal edge and,
                // when the region sits inside a `try`, the handler edge. Both are
                // real successors, so the block only exits to the loop when both
                // do — the same rule the `Match` arm below applies.
                //
                // An `await` in a loop body must not hit the `_ => Ok(false)` catch-all
                // below: `while_header` recognition has to accept it, or the loop is
                // emitted as a straight-line region whose back edge re-inlines the header.
                if !self.block_exits_to_loop(
                    self.block(*target)?,
                    continue_target,
                    break_target,
                    visited,
                )? {
                    Ok(false)
                } else if let Some(handler) = unwind {
                    self.block_exits_to_loop(
                        self.block(handler.catch_block)?,
                        continue_target,
                        break_target,
                        visited,
                    )
                } else {
                    Ok(true)
                }
            }
            Some(Terminator::Switch {
                then_block,
                else_block,
                ..
            }) => Ok(self.block_exits_to_loop(
                self.block(*then_block)?,
                continue_target,
                break_target,
                visited,
            )? && self.block_exits_to_loop(
                self.block(*else_block)?,
                continue_target,
                break_target,
                visited,
            )?),
            Some(Terminator::Match { arms, default, .. }) => {
                // A `switch` in the loop body is a control-flow fork just like a
                // `Switch`: the block exits to the loop only when every arm (and
                // the default) does. Without this, a loop whose body is a
                // `switch` (e.g. `for (...) { switch (typeof x) { ... } }`)
                // fails `while_header` recognition and is emitted as a run-once
                // straight-line block instead of a loop.
                let mut all_exit = true;
                for target in arms
                    .iter()
                    .map(|arm| arm.target)
                    .chain(default.iter().copied())
                {
                    if !self.block_exits_to_loop(
                        self.block(target)?,
                        continue_target,
                        break_target,
                        visited,
                    )? {
                        all_exit = false;
                        break;
                    }
                }
                Ok(all_exit)
            }
            Some(Terminator::Return(_) | Terminator::Throw(_)) => {
                Ok(true)
            }
            _ => Ok(false),
        }?;
        self.caches.loop_exit_cache.borrow_mut().insert(cache_key, result);
        Ok(result)
    }

    /// Returns whether a control-flow path can reach `target`.
    pub(in crate::emitter) fn block_reaches_target(
        &self,
        block_id: smelt_mir::BlockId,
        target: smelt_mir::BlockId,
        visited: &mut BlockIdSet,
    ) -> bool {
        if block_id == target {
            return true;
        }
        if !visited.insert(block_id) {
            return false;
        }
        let Some(block) = self.block(block_id).ok() else {
            return false;
        };
        let Some(terminator) = &block.terminator else {
            return false;
        };
        control_flow_successors(terminator)
            .into_iter()
            .any(|successor| self.block_reaches_target(successor, target, visited))
    }

    /// Returns whether `block_id` reaches `target` without crossing avoided blocks.
    pub(in crate::emitter) fn block_reaches_target_avoiding(
        &self,
        block_id: smelt_mir::BlockId,
        target: smelt_mir::BlockId,
        avoid: &[smelt_mir::BlockId],
        visited: &mut BlockIdSet,
    ) -> bool {
        if avoid.contains(&block_id) {
            return false;
        }
        if block_id == target {
            return true;
        }
        if !visited.insert(block_id) {
            return false;
        }
        let Some(block) = self.block(block_id).ok() else {
            return false;
        };
        let Some(terminator) = &block.terminator else {
            return false;
        };
        control_flow_successors(terminator)
            .into_iter()
            .any(|successor| self.block_reaches_target_avoiding(successor, target, avoid, visited))
    }

    /// Checks if a block starts a while loop with a latch block.
    pub(super) fn while_header_with_latch(
        &self,
        block: &BasicBlock,
    ) -> Result<
        Option<(
            String,
            smelt_mir::BlockId,
            smelt_mir::BlockId,
            smelt_mir::BlockId,
            usize,
        )>,
        EmitError,
    > {
        let Some(Terminator::Switch {
            cond:
                Operand::Copy(Place::Local(cond_local)) | Operand::Move(Place::Local(cond_local)),
            then_block,
            else_block,
        }) = &block.terminator
        else {
            return Ok(None);
        };
        let then = self.block(*then_block)?;
        let Some(Terminator::Goto(latch_block)) = then.terminator else {
            return Ok(None);
        };
        let latch = self.block(latch_block)?;
        if !matches!(latch.terminator, Some(Terminator::Goto(target)) if target == block.id) {
            return Ok(None);
        }
        let Some((idx, Statement::Assign { dest, value })) = block
            .statements
            .iter()
            .enumerate()
            .rev()
            .find(|(_, statement)| matches!(statement, Statement::Assign { .. }))
        else {
            return Ok(None);
        };
        if dest != cond_local {
            return Ok(None);
        }
        // Render the condition at the switch local's (boolean) type so a
        // truthiness `PrimitiveCast` does not fall through to the unit default
        // and emit `while ()` (see `while_header`).
        let cond_ty = self.local_decl(*cond_local)?.ty;
        Ok(Some((
            self.rvalue_expr_for_dest(value, cond_ty)?.into_string(),
            *then_block,
            latch_block,
            *else_block,
            idx,
        )))
    }

    /// Recognizes a `while` loop whose condition is a short-circuit region.
    ///
    /// A compound source condition such as `while (a && b)` (or `a || b`, or a
    /// longer chain) lowers its first operand into the header block `H`'s switch
    /// and evaluates the remaining operands in follow-on blocks that reconverge
    /// at a single decision block `D`. `D` switches on the fully-evaluated
    /// condition: one branch is the loop body (it back-edges to `H`) and the
    /// other leaves the loop. The single-block recognizers (`while_header`,
    /// `while_header_with_latch`) do not fire because the loop decision lives in
    /// `D`, not in `H`, so without this recognizer `H` is emitted as a plain
    /// `if`/`else` and the body's back-edge collapses to a `continue` that
    /// targets the enclosing Rust loop instead of re-checking the condition.
    ///
    /// Returns `(decision, body_entry, exit_entry, body_is_then)` when `block`
    /// is such a header. `avoid` lists the enclosing loop's boundary blocks (its
    /// continue/break targets); a decision branch that can only reach `H` by
    /// crossing that boundary belongs to the enclosing loop, not to this loop's
    /// back-edge, so it is classified as the exit.
    pub(super) fn compound_while(
        &self,
        block: &BasicBlock,
        avoid: &[smelt_mir::BlockId],
    ) -> Result<
        Option<(
            smelt_mir::BlockId,
            smelt_mir::BlockId,
            smelt_mir::BlockId,
            bool,
        )>,
        EmitError,
    > {
        let Some(Terminator::Switch { .. }) = &block.terminator else {
            return Ok(None);
        };
        // Blocks that strictly dominate the header `H`. A genuine loop back-edge
        // re-enters `H` from a block dominated by `H` (i.e. from inside the
        // loop), so it can reach `H` without passing through any of these. When a
        // candidate's "repeating" branch can only reach `H` by crossing a strict
        // dominator of `H`, that path is the back-edge of an *enclosing* loop
        // feeding forward into `H`, not `H`'s own loop, and `H` is not a loop
        // header at all. Avoiding these dominators on the repeat test rejects
        // that false positive (e.g. omit's inner `if` guard block, whose only
        // path back to itself runs through the outer `for` header).
        let header_dominators = self.strict_dominators(block.id);
        // Locate the decision block `D` that every forward condition path from
        // `H` funnels into and that actually decides the loop (one branch
        // back-edges to `H`, the other exits). Scanning in block order and
        // taking the first match selects the final decision switch: earlier
        // short-circuit reconvergence joins also funnel from `H`, but neither of
        // their branches back-edges to `H`, so they are skipped.
        for candidate in &self.function.blocks {
            if candidate.id == block.id {
                continue;
            }
            let Some(Terminator::Switch {
                then_block,
                else_block,
                ..
            }) = &candidate.terminator
            else {
                continue;
            };
            if !self.all_paths_reach_decision(
                block.id,
                candidate.id,
                block.id,
                &mut BlockIdSet::default(),
            )? {
                continue;
            }
            // `H` must dominate `D`: every path from the function entry to `D`
            // passes through `H`. Without this, an ordinary inner `if` block
            // whose body loops around to an enclosing loop header would be
            // mistaken for the compound-condition header, with the true outer
            // header misread as its decision block. If `D` is reachable from the
            // entry while avoiding `H`, `H` does not dominate `D`, so reject.
            if self.block_reaches_target_avoiding(
                self.function.entry,
                candidate.id,
                &[block.id],
                &mut BlockIdSet::default(),
            ) {
                continue;
            }
            let mut branch_avoid = vec![candidate.id];
            branch_avoid.extend_from_slice(avoid);
            branch_avoid.extend_from_slice(&header_dominators);
            let then_repeats = self.block_reaches_target_avoiding(
                *then_block,
                block.id,
                &branch_avoid,
                &mut BlockIdSet::default(),
            );
            let else_repeats = self.block_reaches_target_avoiding(
                *else_block,
                block.id,
                &branch_avoid,
                &mut BlockIdSet::default(),
            );
            if then_repeats == else_repeats {
                continue;
            }
            let (body_entry, exit_entry) = if then_repeats {
                (*then_block, *else_block)
            } else {
                (*else_block, *then_block)
            };
            return Ok(Some((candidate.id, body_entry, exit_entry, then_repeats)));
        }
        Ok(None)
    }

    /// Returns whether `from` returns to `header` along a genuine BACK EDGE —
    /// a path that stays inside `header`'s own region, never passing through
    /// one of `header`'s strict dominators.
    ///
    /// Plain reachability is not enough: every block inside an OUTER loop
    /// reaches every other one by going around that loop. The `if` inside
    /// `for (..) { if (c) { for (..) {..} } }` reaches itself through the outer
    /// latch and header, so it was mistaken for a `while` header, the inner
    /// loop's preheader was emitted inside a spurious `loop {}`, and the inner
    /// loop's `continue` re-ran that preheader forever (Hono's trie-router
    /// `#pushHandlerSets` hang). Returning through a strict dominator means
    /// the path LEFT the region and came back in through its entry — no loop.
    fn is_back_edge_target(&self, from: smelt_mir::BlockId, header: smelt_mir::BlockId) -> bool {
        let dominators = self.strict_dominators(header);
        self.block_reaches_target_avoiding(from, header, &dominators, &mut BlockIdSet::default())
    }

    /// Returns the blocks that strictly dominate `header`.
    ///
    /// A block `d` strictly dominates `header` when `d != header` and every path
    /// from the function entry to `header` passes through `d`; equivalently, the
    /// entry cannot reach `header` while avoiding `d`. Used by
    /// [`Self::compound_while`] to distinguish a real loop back-edge (which
    /// re-enters the header from inside the loop, avoiding every strict
    /// dominator) from an enclosing loop's edge that merely flows forward into
    /// the header through one of its dominators.
    fn strict_dominators(&self, header: smelt_mir::BlockId) -> Vec<smelt_mir::BlockId> {
        self.function
            .blocks
            .iter()
            .map(|candidate| candidate.id)
            .filter(|&candidate| {
                candidate != header
                    && !self.block_reaches_target_avoiding(
                        self.function.entry,
                        header,
                        &[candidate],
                        &mut BlockIdSet::default(),
                    )
            })
            .collect()
    }

    /// Returns whether every forward path from `block_id` reaches `decision`.
    ///
    /// Used to detect the reconvergence point of a short-circuit condition
    /// region: the region is walked through `Goto`/`Switch` terminators only.
    /// Returning to `header` (a loop back-edge) or hitting a terminator that
    /// leaves the region (return, throw, call, match) before `decision` means
    /// `decision` is not mandatory, so the path does not funnel.
    fn all_paths_reach_decision(
        &self,
        block_id: smelt_mir::BlockId,
        decision: smelt_mir::BlockId,
        header: smelt_mir::BlockId,
        visiting: &mut BlockIdSet,
    ) -> Result<bool, EmitError> {
        if block_id == decision {
            return Ok(true);
        }
        if block_id == header && !visiting.is_empty() {
            return Ok(false);
        }
        if !visiting.insert(block_id) {
            return Ok(false);
        }
        let block = self.block(block_id)?;
        let result = match &block.terminator {
            Some(Terminator::Goto(target)) => {
                self.all_paths_reach_decision(*target, decision, header, visiting)?
            }
            Some(Terminator::Switch {
                then_block,
                else_block,
                ..
            }) => {
                self.all_paths_reach_decision(*then_block, decision, header, visiting)?
                    && self.all_paths_reach_decision(*else_block, decision, header, visiting)?
            }
            _ => false,
        };
        visiting.remove(&block_id);
        Ok(result)
    }

    /// Emits a compound `while` header's short-circuit condition region.
    ///
    /// Walks the acyclic region between the header `H` and its decision block
    /// `D`, re-emitting each header/operand block's statements and rendering the
    /// short-circuit switches as nested `if`/`else` so that the boolean the
    /// decision switches on is computed exactly as in the source. Emission stops
    /// at `decision`; `visited` guards against re-emitting a reconvergence join
    /// twice.
    fn emit_condition_region(
        &self,
        block: &BasicBlock,
        decision: smelt_mir::BlockId,
        out: &mut String,
        visited: &mut BlockIdSet,
    ) -> Result<(), EmitError> {
        if block.id == decision {
            return Ok(());
        }
        if !visited.insert(block.id) {
            return Ok(());
        }
        for statement in &block.statements {
            self.emit_statement(statement, out)?;
        }
        match &block.terminator {
            Some(Terminator::Goto(target)) => {
                if *target == decision {
                    Ok(())
                } else {
                    self.emit_condition_region(self.block(*target)?, decision, out, visited)
                }
            }
            Some(Terminator::Switch {
                cond,
                then_block,
                else_block,
            }) => {
                out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?.delimited()));
                self.emit_condition_region(self.block(*then_block)?, decision, out, visited)?;
                out.push_str("    } else {\n");
                self.emit_condition_region(self.block(*else_block)?, decision, out, visited)?;
                out.push_str("    }\n");
                Ok(())
            }
            _ => Err(EmitError::new(
                "compound condition region reached a non-condition terminator",
            )),
        }
    }

    /// Emits a recognized compound `while` loop; the caller continues at its exit block.
    ///
    /// Emits `loop { <condition region>; if <decision> { <body> } else { break }
    /// }` (with the arms swapped when the body is the decision's else branch) so
    /// the body's back-edge to the header becomes a natural loop iteration that
    /// re-evaluates the full compound condition. The caller continues at
    /// `exit_entry`, the loop exit continuation, which it emits next.
    pub(super) fn emit_compound_while(
        &self,
        block: &BasicBlock,
        decision: smelt_mir::BlockId,
        body_entry: smelt_mir::BlockId,
        exit_entry: smelt_mir::BlockId,
        body_is_then: bool,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let cond_text = {
            let Some(Terminator::Switch { cond, .. }) = &self.block(decision)?.terminator else {
                return Err(EmitError::new("compound while decision must be a switch"));
            };
            self.truthy_operand_expr(cond)?
        };
        let loop_declared = self.declared_scope();
        let open_loop = self.flow.open_loop(block.id, None, Some(exit_entry), out.len());
        out.push_str("    loop {\n");
        self.emit_condition_region(block, decision, out, &mut BlockIdSet::default())?;
        out.push_str(&format!("    if {cond_text} {{\n", cond_text = cond_text.delimited()));
        if body_is_then {
            self.emit_block_until_goto(
                self.block(body_entry)?,
                block.id,
                RegionExit::LoopBody {
                    break_block: Some(exit_entry),
                },
                out,
            )?;
            out.push_str("    } else {\n    break;\n    }\n");
        } else {
            out.push_str("    break;\n    } else {\n");
            self.emit_block_until_goto(
                self.block(body_entry)?,
                block.id,
                RegionExit::LoopBody {
                    break_block: Some(exit_entry),
                },
                out,
            )?;
            out.push_str("    }\n");
        }
        out.push_str("    }\n");
        open_loop.close(out);
        loop_declared.close();
        Ok(())
    }

    /// Emits a block's statements until reaching a goto to the stop target.
    pub(in crate::emitter) fn emit_block_until_goto(
        &self,
        block: &BasicBlock,
        stop: smelt_mir::BlockId,
        exit: RegionExit,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let limit = self.function.blocks.len().saturating_mul(8).max(128);
        // The bound is structural: a structured region is at most a few
        // passes over the function's blocks. Past it the walker is cycling
        // through a shape it cannot structure, and the only honest answer is
        // an error rather than dropping the rest of the loop body.
        let Some(_depth) = self.flow.enter_until(limit) else {
            return Err(self.unstructured_control_flow(format!("nested loop regions exceed {limit} levels")));
        };
        self.emit_block_until_goto_inner(block, stop, exit, out, &mut BlockIdSet::default())
    }

    /// Emits a block until a target while avoiding repeated unstructured cycles.
    pub(in crate::emitter) fn emit_block_until_goto_inner(
        &self,
        block: &BasicBlock,
        stop: smelt_mir::BlockId,
        exit: RegionExit,
        out: &mut String,
        visited: &mut BlockIdSet,
    ) -> Result<(), EmitError> {
        let break_target = exit.break_target();
        // Reaching the stop or the loop exit ends the region whichever edge got
        // here. A `while` whose condition is a call has a header ending in a
        // `Call`, so the edge back to the decision block is that call's normal
        // successor, not a `Goto`; tested only on `Goto`, the walker ran through
        // the stop into a second copy of the loop body.
        if block.id == stop {
            return Ok(());
        }
        if Some(block.id) == break_target {
            out.push_str("    break;\n");
            return Ok(());
        }
        if self.emit_enclosing_loop_jump(block.id, out)? {
            return Ok(());
        }
        // In a loop body the region's stop IS the loop header, which is the
        // loop context a pending join was registered under.
        let loop_context = matches!(exit, RegionExit::LoopBody { .. }).then_some(stop);
        if self.loop_join_stop_hit(block.id, loop_context) {
            return Ok(());
        }
        if !visited.insert(block.id) {
            return self.emit_region_cycle(block.id, out);
        }
        if self.emit_nested_loop_until_goto(block, stop, exit, out, visited)? {
            return Ok(());
        }
        for statement in &block.statements {
            self.emit_statement(statement, out)?;
        }
        match &block.terminator {
            Some(Terminator::Goto(target)) => {
                let target_block = self.block(*target)?;
                self.emit_block_until_goto_inner(target_block, stop, exit, out, visited)
            }
            Some(Terminator::Call {
                callee,
                args,
                dest,
                target,
                unwind,
            }) => {
                // A `try`/`catch` around the call is a real fork of the region,
                // so it has to be emitted here rather than dropped. Ignoring
                // `unwind` used to emit the call through the plain `?` template,
                // which silently discarded the `catch` block: a `for` loop
                // wrapping `try { return f(i); } catch {}` propagated the first
                // error out of the function instead of retrying.
                //
                // Both arms of the fork continue in *this* region, so the
                // continuation comes from `exit`: inside a loop body an edge to
                // `stop` is the back edge (`continue`), inside a forward `if`
                // arm it is the end of the arm.
                if let Some(handler) = unwind {
                    return self.emit_throwing_call_terminator(
                        callee,
                        args,
                        *dest,
                        *target,
                        *handler,
                        &exit.continuation(stop, visited),
                        out,
                    );
                }
                self.emit_call_terminator_statement(callee, args, *dest, out)?;
                self.emit_block_until_goto_inner(self.block(*target)?, stop, exit, out, visited)
            }
            Some(Terminator::Await {
                future,
                dest,
                target,
                unwind,
            }) => {
                if let Some(handler) = unwind {
                    return self.emit_throwing_await_terminator(
                        future,
                        *dest,
                        *target,
                        *handler,
                        &exit.continuation(stop, visited),
                        out,
                    );
                }
                self.emit_await_terminator_statement(future, *dest, out)?;
                self.emit_block_until_goto_inner(self.block(*target)?, stop, exit, out, visited)
            }
            Some(Terminator::Switch {
                cond,
                then_block,
                else_block,
            }) if break_target.is_some() => {
                out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?.delimited()));
                self.emit_loop_branch(self.block(*then_block)?, stop, break_target, out)?;
                out.push_str("    } else {\n");
                self.emit_loop_branch(self.block(*else_block)?, stop, break_target, out)?;
                out.push_str("    }\n");
                Ok(())
            }
            // A branch inside a FORWARD join region (an `if` arm that runs on to
            // the caller's join `stop`). Handing the switch to `emit_terminator`
            // lost `stop`: the arm then re-derived its continuation from the
            // whole function and, on meeting the (lower-numbered) join, rendered
            // the function's default return instead of ending the arm, so the
            // enclosing `if` silently returned `null`/`undefined` where the
            // source falls through (Hono `getQueryParam` answered `undefined`
            // for a key it only finds on the slow path). The structured form is
            // the branch up to its own inner join, then the inner join onward,
            // still bounded by `stop` — see `join_region_branch_join`.
            Some(Terminator::Switch {
                cond,
                then_block,
                else_block,
            }) if matches!(exit, RegionExit::Join) => {
                let Some(join) =
                    self.join_region_branch_join(block.id, *then_block, *else_block, stop)?
                else {
                    return self.emit_terminator(
                        block.id,
                        block.terminator.as_ref().ok_or_else(|| {
                            EmitError::new("basic block has no terminator")
                        })?,
                        out,
                    );
                };
                let cond_text = self.truthy_operand_expr(cond)?;
                let branch_declared = self.declared_scope();
                if *then_block == join && *else_block == join {
                    // Both edges already are the join: no branch to emit.
                } else if *then_block == join {
                    out.push_str(&format!("    if !({cond_text}) {{\n", cond_text = cond_text.delimited()));
                    self.emit_join_region_arm(*else_block, join, stop, out)?;
                    out.push_str("    }\n");
                } else {
                    out.push_str(&format!("    if {cond_text} {{\n", cond_text = cond_text.delimited()));
                    self.emit_join_region_arm(*then_block, join, stop, out)?;
                    if *else_block != join {
                        out.push_str("    } else {\n");
                        branch_declared.reset();
                        self.emit_join_region_arm(*else_block, join, stop, out)?;
                    }
                    out.push_str("    }\n");
                }
                branch_declared.close();
                if join == stop {
                    return Ok(());
                }
                self.emit_block_until_goto_inner(self.block(join)?, stop, exit, out, visited)
            }
            Some(Terminator::Match {
                scrutinee,
                arms,
                default,
            }) if break_target.is_some() => self.emit_loop_match(
                scrutinee,
                arms,
                *default,
                stop,
                break_target,
                out,
                visited,
            ),
            Some(terminator) => self.emit_terminator(block.id, terminator, out),
            None => Err(EmitError::new("basic block has no terminator")),
        }
    }

    /// Emits a branch inside a loop.
    fn emit_loop_branch(
        &self,
        block: &BasicBlock,
        continue_target: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
    ) -> Result<(), EmitError> {
        self.emit_loop_branch_inner(
            block,
            continue_target,
            break_target,
            out,
            &mut BlockIdSet::default(),
        )
    }

    /// Emits a branch inside a loop while guarding against join-block cycles.
    pub(in crate::emitter) fn emit_loop_branch_inner(
        &self,
        block: &BasicBlock,
        continue_target: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
        visited: &mut BlockIdSet,
    ) -> Result<(), EmitError> {
        // As in `emit_block_until_goto_inner`: the back edge and the exit are
        // recognised on the block reached, not on the kind of edge reaching it.
        if block.id == continue_target {
            out.push_str("    continue;\n");
            return Ok(());
        }
        if Some(block.id) == break_target {
            out.push_str("    break;\n");
            return Ok(());
        }
        if self.emit_enclosing_loop_jump(block.id, out)? {
            return Ok(());
        }
        if self.loop_join_stop_hit(block.id, Some(continue_target)) {
            return Ok(());
        }
        if !visited.insert(block.id) {
            return self.emit_region_cycle(block.id, out);
        }
        if self.emit_nested_loop_until_goto(
            block,
            continue_target,
            RegionExit::LoopBody {
                break_block: break_target,
            },
            out,
            visited,
        )? {
            return Ok(());
        }
        for statement in &block.statements {
            self.emit_statement(statement, out)?;
        }
        match &block.terminator {
            Some(Terminator::Goto(target)) => {
                let target_block = self.block(*target)?;
                self.emit_loop_branch_inner(
                    target_block,
                    continue_target,
                    break_target,
                    out,
                    visited,
                )
            }
            Some(Terminator::Call {
                callee,
                args,
                dest,
                target,
                unwind,
            }) => {
                // Same fork as in `emit_block_until_goto_inner`: the handler edge
                // must be rendered, and both edges continue inside this loop.
                if let Some(handler) = unwind {
                    return self.emit_throwing_call_terminator(
                        callee,
                        args,
                        *dest,
                        *target,
                        *handler,
                        &Continuation::InLoop {
                            continue_target,
                            break_target,
                            visited,
                        },
                        out,
                    );
                }
                self.emit_call_terminator_statement(callee, args, *dest, out)?;
                self.emit_loop_branch_inner(
                    self.block(*target)?,
                    continue_target,
                    break_target,
                    out,
                    visited,
                )
            }
            Some(Terminator::Await {
                future,
                dest,
                target,
                unwind,
            }) => {
                if let Some(handler) = unwind {
                    return self.emit_throwing_await_terminator(
                        future,
                        *dest,
                        *target,
                        *handler,
                        &Continuation::InLoop {
                            continue_target,
                            break_target,
                            visited,
                        },
                        out,
                    );
                }
                self.emit_await_terminator_statement(future, *dest, out)?;
                self.emit_loop_branch_inner(
                    self.block(*target)?,
                    continue_target,
                    break_target,
                    out,
                    visited,
                )
            }
            Some(Terminator::Switch {
                cond,
                then_block,
                else_block,
            }) => {
                if self.emit_loop_branch_at_join(
                    block.id,
                    cond,
                    *then_block,
                    *else_block,
                    continue_target,
                    break_target,
                    out,
                    visited,
                )? {
                    return Ok(());
                }
                // Each arm is its own Rust scope: a local first declared
                // (`let mut ..`) while emitting the then-arm — commonly a join
                // block both arms duplicate — is NOT in scope in the else-arm,
                // which must declare it again (E0425 `_smelt_tmp_N` when the
                // else-arm assigned it bare).
                let branch_declared = self.declared_scope();
                out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?.delimited()));
                // Each branch can legitimately converge on the same join block.
                // Sharing the recursion guard across siblings makes the later
                // branch look cyclic and incorrectly emits `continue`.
                let mut then_visited = visited.clone();
                self.emit_loop_branch_inner(
                    self.block(*then_block)?,
                    continue_target,
                    break_target,
                    out,
                    &mut then_visited,
                )?;
                out.push_str("    } else {\n");
                branch_declared.reset();
                let mut else_visited = visited.clone();
                self.emit_loop_branch_inner(
                    self.block(*else_block)?,
                    continue_target,
                    break_target,
                    out,
                    &mut else_visited,
                )?;
                out.push_str("    }\n");
                branch_declared.close();
                Ok(())
            }
            Some(Terminator::Match {
                scrutinee,
                arms,
                default,
            }) => self.emit_loop_match(
                scrutinee,
                arms,
                *default,
                continue_target,
                break_target,
                out,
                visited,
            ),
            Some(terminator) => self.emit_terminator(block.id, terminator, out),
            None => Err(EmitError::new("basic block has no terminator")),
        }
    }

    /// Returns whether `block` is a pending loop-body join of this emitter,
    /// i.e. whether the current arm must END here instead of emitting it.
    ///
    /// `loop_context` is the continue target of the loop the walker is
    /// currently emitting (`None` for a forward join region). Reaching a join
    /// from any context other than the one it was registered in means control
    /// leaves a nested construct straight into the join; ending the arm there
    /// would be wrong, so the hit is recorded as a violation and the caller
    /// discards its join-based attempt. The stops are this emitter's own (see
    /// `flow_state`), so a closure body's emitter never sees its parent's.
    pub(in crate::emitter) fn loop_join_stop_hit(
        &self,
        block: smelt_mir::BlockId,
        loop_context: Option<smelt_mir::BlockId>,
    ) -> bool {
        self.flow.loop_join_stop_hit(block, loop_context)
    }

    /// The JOIN of a two-way branch inside a loop body: the branch block's
    /// immediate post-dominator within the loop region, if it is a real block.
    ///
    /// The region is every block reachable from `switch` without passing the
    /// loop's continue target, its break target, or an enclosing pending join;
    /// those edges, and returns/throws, all lead to one virtual exit. A block
    /// every path from the branch goes through before that exit is where both
    /// arms reconverge, so the code after it belongs after the `if`, once.
    fn loop_branch_join(
        &self,
        switch: smelt_mir::BlockId,
        continue_target: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
    ) -> Option<smelt_mir::BlockId> {
        let pending = self.flow.pending_joins();
        let is_exit = |block: smelt_mir::BlockId| {
            block == continue_target || Some(block) == break_target || pending.contains(&block)
        };
        let successors_of = |block_id: smelt_mir::BlockId| {
            self.block(block_id)
                .ok()
                .and_then(|block| block.terminator.as_ref())
                .map(control_flow_successors)
                .unwrap_or_default()
        };
        let mut region = Vec::new();
        let mut seen = BlockIdSet::default();
        let mut stack = vec![switch];
        while let Some(block_id) = stack.pop() {
            if is_exit(block_id) || !seen.insert(block_id) {
                continue;
            }
            region.push(block_id);
            stack.extend(successors_of(block_id));
        }
        // Node `n` is `region[n]`; node `region.len()` is the virtual exit.
        let exit = region.len();
        let node_of = |block_id: smelt_mir::BlockId| {
            if is_exit(block_id) {
                exit
            } else {
                region.iter().position(|candidate| *candidate == block_id).unwrap_or(exit)
            }
        };
        let region_successors = region
            .iter()
            .map(|block_id| {
                let mut next = successors_of(*block_id).into_iter().map(node_of).collect::<Vec<_>>();
                if next.is_empty() {
                    next.push(exit);
                }
                next
            })
            .collect::<Vec<_>>();
        // Iterative post-dominator sets over `region + exit`: a node's set is
        // itself plus the intersection of its successors' sets.
        let width = exit.saturating_add(1);
        let mut post_dominators: Vec<Vec<bool>> = (0..width)
            .map(|node| (0..width).map(|member| node != exit || member == exit).collect())
            .collect();
        let mut changed = true;
        while changed {
            changed = false;
            for (node, next_nodes) in region_successors.iter().enumerate().rev() {
                let mut next = vec![true; width];
                for successor in next_nodes {
                    if let Some(successor_set) = post_dominators.get(*successor) {
                        for (slot, member) in next.iter_mut().zip(successor_set) {
                            *slot &= *member;
                        }
                    }
                }
                if let Some(own) = next.get_mut(node) {
                    *own = true;
                }
                if let Some(current) = post_dominators.get_mut(node)
                    && *current != next
                {
                    *current = next;
                    changed = true;
                }
            }
        }
        let start = node_of(switch);
        let start_set = post_dominators.get(start)?;
        // The immediate post-dominator is the strict post-dominator that is
        // itself post-dominated by all the others: the one with the most.
        region
            .iter()
            .zip(&post_dominators)
            .enumerate()
            .filter(|(node, _)| *node != start && start_set.get(*node).copied().unwrap_or(false))
            .max_by_key(|(_, (_, set))| set.iter().filter(|member| **member).count())
            .map(|(_, (block_id, _))| *block_id)
    }

    /// Emits `if cond { then } else { else }` inside a loop body ending both
    /// arms at their JOIN, then the join and everything after it ONCE.
    ///
    /// Without a join every arm is emitted through to the loop latch, so the
    /// code after the `if` is duplicated into both arms; a run of sequential
    /// branches doubles it at each step (es-toolkit's `mergeWith` loop body
    /// grew to 100k+ lines). Returns `false`, having emitted nothing, when there
    /// is no join or when an arm reached the join from a nested loop or forward
    /// region (where ending the arm would skip code); the caller then uses the
    /// arm-contained form.
    #[expect(
        clippy::too_many_arguments,
        reason = "the join emitter threads the branch operands and the loop's continue/break context"
    )]
    pub(in crate::emitter) fn emit_loop_branch_at_join(
        &self,
        switch: smelt_mir::BlockId,
        cond: &Operand,
        then_block: smelt_mir::BlockId,
        else_block: smelt_mir::BlockId,
        continue_target: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
        visited: &mut BlockIdSet,
    ) -> Result<bool, EmitError> {
        let Some(join) = self.loop_branch_join(switch, continue_target, break_target) else {
            return Ok(false);
        };
        let branch_declared = self.declared_scope();
        let join_scope = self.flow.push_loop_join_stop(join, continue_target);
        let mut arms = String::new();
        let emitted = (|| -> Result<(), EmitError> {
            arms.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?.delimited()));
            let mut then_visited = visited.clone();
            self.emit_loop_branch_inner(
                self.block(then_block)?,
                continue_target,
                break_target,
                &mut arms,
                &mut then_visited,
            )?;
            arms.push_str("    } else {\n");
            branch_declared.reset();
            let mut else_visited = visited.clone();
            self.emit_loop_branch_inner(
                self.block(else_block)?,
                continue_target,
                break_target,
                &mut arms,
                &mut else_visited,
            )?;
            arms.push_str("    }\n");
            Ok(())
        })();
        let violated = join_scope.finish();
        branch_declared.close();
        emitted?;
        if violated {
            return Ok(false);
        }
        out.push_str(&arms);
        self.emit_loop_branch_inner(self.block(join)?, continue_target, break_target, out, visited)?;
        Ok(true)
    }

    /// Emits a structured loop discovered while emitting another control-flow region.
    ///
    /// Top-level block emission runs loop recognition before emitting a switch as
    /// an `if`. Nested regions need the same pass so inner `continue` statements
    /// target the generated inner loop instead of the surrounding Rust loop.
    fn emit_nested_loop_until_goto(
        &self,
        block: &BasicBlock,
        stop: smelt_mir::BlockId,
        exit: RegionExit,
        out: &mut String,
        visited: &mut BlockIdSet,
    ) -> Result<bool, EmitError> {
        let break_target = exit.break_target();
        // No depth cut-off: a loop nested three deep (a `for..of` inside a
        // `while` inside a `for`, Hono's trie-router `search`) is still a loop,
        // and a header emitted as a plain `if` would turn its back edge into a
        // `continue` of the ENCLOSING loop. `while_header` only claims a genuine
        // back-edge target, so a nested region cannot misread a branch as a
        // header. `emit_block_until_goto` keeps its own recursion limit.
        // A compound short-circuit `while` header (e.g. `while (a && b)`) is
        // checked before the single-block recognizers: `while_header` also
        // matches such a header (it computes its own first-operand switch local)
        // but then bails on the escaping-body guard, which would drop the loop
        // back to a plain `if` whose back-edge collapses to a wrong-target
        // `continue`. `compound_while` only returns `Some` for a genuine
        // multi-block condition region, so trying it first is safe.
        let mut boundary = vec![stop];
        if let Some(break_block) = break_target {
            boundary.push(break_block);
        }
        if let Some((decision, body_entry, exit_entry, body_is_then)) =
            self.compound_while(block, &boundary)?
            && !self.block_reaches_target_avoiding(
                body_entry,
                stop,
                &[block.id, exit_entry],
                &mut BlockIdSet::default(),
            )
        {
            self.emit_compound_while(block, decision, body_entry, exit_entry, body_is_then, out)?;
            self.emit_after_nested_loop(exit_entry, stop, exit, out, visited)?;
            return Ok(true);
        }

        if let Some((cond, then_block, else_block, cond_statement_idx)) =
            self.while_header(block)?
        {
            let Some(break_block) =
                self.nested_loop_break_block(then_block, else_block, stop, exit, &[block.id, else_block])
            else {
                return Ok(false);
            };
            let then = self.block(then_block)?;
            let loop_declared = self.declared_scope();
            let open_loop = self.flow.open_loop(block.id, None, Some(break_block), out.len());
            self.open_while_loop(block, &cond, cond_statement_idx, out)?;
            self.emit_block_until_goto(
                then,
                block.id,
                RegionExit::LoopBody {
                    break_block: Some(break_block),
                },
                out,
            )?;
            out.push_str("    }\n");
            open_loop.close(out);
            loop_declared.close();
            self.emit_after_nested_loop(else_block, stop, exit, out, visited)?;
            return Ok(true);
        }

        if let Some((then_block, else_block)) = self.constant_true_loop_header(block)? {
            let Some(break_block) =
                self.nested_loop_break_block(then_block, else_block, stop, exit, &[block.id, else_block])
            else {
                return Ok(false);
            };
            let then = self.block(then_block)?;
            let loop_declared = self.declared_scope();
            let open_loop = self.flow.open_loop(block.id, None, Some(break_block), out.len());
            out.push_str("    loop {\n");
            for statement in &block.statements {
                self.emit_statement_for_block(block, statement, out)?;
            }
            self.emit_block_until_goto(
                then,
                block.id,
                RegionExit::LoopBody {
                    break_block: Some(break_block),
                },
                out,
            )?;
            out.push_str("    }\n");
            open_loop.close(out);
            loop_declared.close();
            self.emit_after_nested_loop(else_block, stop, exit, out, visited)?;
            return Ok(true);
        }

        if let Some((cond, then_block, latch_block, else_block, cond_statement_idx)) =
            self.while_header_with_latch(block)?
        {
            let Some(break_block) = self.nested_loop_break_block(
                then_block,
                else_block,
                stop,
                exit,
                &[block.id, latch_block, else_block],
            ) else {
                return Ok(false);
            };
            let then = self.block(then_block)?;
            let latch = self.block(latch_block)?;
            let loop_declared = self.declared_scope();
            let open_loop = self.flow.open_loop(
                block.id,
                Some(latch_block),
                Some(break_block),
                out.len(),
            );
            self.open_while_loop(block, &cond, cond_statement_idx, out)?;
            self.emit_block_until_goto(
                then,
                latch_block,
                RegionExit::LoopBody {
                    break_block: Some(break_block),
                },
                out,
            )?;
            for statement in &latch.statements {
                self.emit_statement(statement, out)?;
            }
            out.push_str("    }\n");
            open_loop.close(out);
            loop_declared.close();
            self.emit_after_nested_loop(else_block, stop, exit, out, visited)?;
            return Ok(true);
        }

        if let Some((repeat_block, exit_block)) = self.call_condition_loop(block, &boundary)? {
            let Some(break_block) =
                self.nested_loop_break_block(repeat_block, exit_block, stop, exit, &[block.id, exit_block])
            else {
                return Ok(false);
            };
            self.emit_call_condition_loop(block, break_block, out)?;
            self.emit_after_nested_loop(exit_block, stop, exit, out, visited)?;
            return Ok(true);
        }

        Ok(false)
    }

    /// Recognizes `block` as the header of a loop whose condition is a call or
    /// `await`, answering the body entry and the exit.
    ///
    /// A condition that calls splits the header: `block` ends in the `Call`
    /// (or `Await`), possibly a chain of them for `await f()`, and the decision
    /// is the `Switch` the chain runs into, so no single-block recogniser sees
    /// it. The repeating arm must return to `block` without crossing
    /// `boundary` or an open loop's header, continue target or exit: plain
    /// reachability holds for every block inside an outer loop, by going round
    /// that loop.
    pub(super) fn call_condition_loop(
        &self,
        block: &BasicBlock,
        boundary: &[smelt_mir::BlockId],
    ) -> Result<Option<(smelt_mir::BlockId, smelt_mir::BlockId)>, EmitError> {
        let mut avoid = boundary.to_vec();
        avoid.extend(self.flow.open_loop_boundaries());
        if avoid.contains(&block.id) {
            return Ok(None);
        }
        let mut seen = BlockIdSet::default();
        seen.insert(block.id);
        let mut current = block;
        let (then_block, else_block) = loop {
            match &current.terminator {
                Some(Terminator::Call { target, .. } | Terminator::Await { target, .. }) => {
                    if !seen.insert(*target) {
                        return Ok(None);
                    }
                    current = self.block(*target)?;
                }
                Some(Terminator::Switch { then_block, else_block, .. })
                    if current.id != block.id =>
                {
                    break (*then_block, *else_block);
                }
                _ => return Ok(None),
            }
        };
        avoid.push(current.id);
        let repeats = |arm: smelt_mir::BlockId| {
            self.block_reaches_target_avoiding(arm, block.id, &avoid, &mut BlockIdSet::default())
        };
        Ok(match (repeats(then_block), repeats(else_block)) {
            (true, false) => Some((then_block, else_block)),
            (false, true) => Some((else_block, then_block)),
            _ => None,
        })
    }

    /// Emits a loop recognized by [`Self::call_condition_loop`] as
    /// `loop { <condition>; <decision and body> }`.
    ///
    /// The condition is evaluated once per iteration inside the loop, so its
    /// temporaries belong to the iteration. From the header's terminator on the
    /// ordinary loop-branch walker does the rest: an edge back to `block` is
    /// `continue`, an edge to `break_block` is `break`, and a call inside a
    /// `try` forks through its handler.
    pub(super) fn emit_call_condition_loop(
        &self,
        block: &BasicBlock,
        break_block: smelt_mir::BlockId,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let loop_declared = self.declared_scope();
        let open_loop = self.flow.open_loop(block.id, None, Some(break_block), out.len());
        out.push_str("    loop {\n");
        for statement in &block.statements {
            self.emit_statement_for_block(block, statement, out)?;
        }
        let mut visited = BlockIdSet::default();
        visited.insert(block.id);
        let in_loop = Continuation::InLoop {
            continue_target: block.id,
            break_target: Some(break_block),
            visited: &visited,
        };
        match &block.terminator {
            Some(Terminator::Call { callee, args, dest, target, unwind }) => {
                if let Some(handler) = unwind {
                    self.emit_throwing_call_terminator(callee, args, *dest, *target, *handler, &in_loop, out)?;
                } else {
                    self.emit_call_terminator_statement(callee, args, *dest, out)?;
                    self.emit_continuation(*target, &in_loop, out)?;
                }
            }
            Some(Terminator::Await { future, dest, target, unwind }) => {
                if let Some(handler) = unwind {
                    self.emit_throwing_await_terminator(future, *dest, *target, *handler, &in_loop, out)?;
                } else {
                    self.emit_await_terminator_statement(future, *dest, out)?;
                    self.emit_continuation(*target, &in_loop, out)?;
                }
            }
            _ => return Err(EmitError::new("call-condition loop header must end in a call or await")),
        }
        out.push_str("    }\n");
        open_loop.close(out);
        loop_declared.close();
        Ok(())
    }

    /// The block a nested loop's body `break`s to, or `None` when the loop
    /// cannot be structured inside a region ending at `stop`.
    ///
    /// Normally that is the loop's exit, `else_block`. A body path that leaves
    /// for the region's `stop` without passing the exit (a `throw` inside a
    /// `try` whose handler rejoins after it) cannot end inside the loop, so the
    /// loop is refused, unless the exit itself only jumps on to `stop`: then
    /// arriving at `stop` from the body is exactly a `break`, and `stop` is
    /// what the body breaks to.
    ///
    /// When the region is itself a loop body, `stop` is the enclosing loop's
    /// continue target, and a body path reaching it (a source `continue outer`,
    /// possibly through the outer latch) is a labelled `continue` of that
    /// loop, which `emit_enclosing_loop_jump` writes once the nested loop is
    /// open. The nested loop then keeps its own exit. Refusing it instead
    /// emitted the nested header as an `if` whose back edge became a bare
    /// `continue` of the OUTER loop, re-running the nested loop's
    /// initialisation forever.
    fn nested_loop_break_block(
        &self,
        then_block: smelt_mir::BlockId,
        else_block: smelt_mir::BlockId,
        stop: smelt_mir::BlockId,
        exit: RegionExit,
        avoiding: &[smelt_mir::BlockId],
    ) -> Option<smelt_mir::BlockId> {
        if !self.block_reaches_target_avoiding(then_block, stop, avoiding, &mut BlockIdSet::default()) {
            return Some(else_block);
        }
        if self.empty_goto_chain_reaches(else_block, stop) {
            return Some(stop);
        }
        (matches!(exit, RegionExit::LoopBody { .. }) && self.flow.in_open_loop()).then_some(else_block)
    }

    /// Whether following blocks that hold no statement and only `goto` leads
    /// from `from` to `target`.
    fn empty_goto_chain_reaches(&self, from: smelt_mir::BlockId, target: smelt_mir::BlockId) -> bool {
        let mut current = from;
        let mut seen = BlockIdSet::default();
        while current != target {
            if !seen.insert(current) {
                return false;
            }
            let Ok(block) = self.block(current) else {
                return false;
            };
            match &block.terminator {
                Some(Terminator::Goto(next)) if block.statements.is_empty() => current = *next,
                _ => return false,
            }
        }
        true
    }

    /// Emits the continuation reached after a nested loop exits.
    fn emit_after_nested_loop(
        &self,
        exit_block: smelt_mir::BlockId,
        stop: smelt_mir::BlockId,
        exit: RegionExit,
        out: &mut String,
        visited: &mut BlockIdSet,
    ) -> Result<(), EmitError> {
        if exit_block == stop {
            return Ok(());
        }
        if Some(exit_block) == exit.break_target() {
            out.push_str("    break;\n");
            return Ok(());
        }
        self.emit_block_until_goto_inner(self.block(exit_block)?, stop, exit, out, visited)
    }
}
