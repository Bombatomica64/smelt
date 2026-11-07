//! `Switch` terminators: structured `if`/`else` emission and the joins where
//! branches meet again.

use super::*;

impl FunctionEmitter<'_> {
    /// Emits an if/else or while loop from a switch terminator.
    pub(super) fn emit_switch(
        &self,
        current: smelt_mir::BlockId,
        cond: &Operand,
        then_block: smelt_mir::BlockId,
        else_block: smelt_mir::BlockId,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let then = self.block(then_block)?;
        let else_ = self.block(else_block)?;
        if let Operand::Const(Constant::Bool(value)) = cond {
            return if *value {
                self.emit_block(then, out)
            } else {
                self.emit_block(else_, out)
            };
        }

        if matches!(then.terminator, Some(Terminator::Goto(target)) if target == current) {
            let loop_declared = self.declared_scope();
            out.push_str(&format!(
                "    while {} {{\n",
                self.truthy_operand_expr(cond)?
            ));
            self.emit_block_until_goto(
                then,
                current,
                RegionExit::LoopBody {
                    break_block: Some(else_block),
                },
                out,
            )?;
            out.push_str("    }\n");
            loop_declared.close();
            return self.emit_block(else_, out);
        }

        if matches!(then.terminator, Some(Terminator::Goto(target)) if target == else_block) {
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            self.emit_block_until_goto(then, else_block, RegionExit::Join, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return self.emit_block(else_, out);
        }

        self.emit_switch_two_gotos(current, cond, then_block, else_block, then, else_, out)
    }

    /// A switch whose arms are, or begin with, plain jumps.
    fn emit_switch_two_gotos(
        &self,
        current: smelt_mir::BlockId,
        cond: &Operand,
        then_block: smelt_mir::BlockId,
        else_block: smelt_mir::BlockId,
        then: &BasicBlock,
        else_: &BasicBlock,
        out: &mut String,
    ) -> Result<(), EmitError> {
        if let (Some(Terminator::Goto(then_target)), Some(Terminator::Goto(else_target))) =
            (&then.terminator, &else_.terminator)
            && then_target == else_target
        {
            if let (
                [
                    Statement::Assign {
                        dest: then_dest,
                        value: then_value,
                    },
                ],
                [
                    Statement::Assign {
                        dest: else_dest,
                        value: else_value,
                    },
                ],
            ) = (then.statements.as_slice(), else_.statements.as_slice())
                && then_dest == else_dest
            {
                let local = self.local_decl(*then_dest)?;
                let name = self.local_name(*then_dest)?;
                let then_text = self.rvalue_expr_for_dest(then_value, local.ty)?;
                let else_text = self.rvalue_expr_for_dest(else_value, local.ty)?;
                out.push_str(&format!(
                    "    let {name}: {} = if {} {{ {then_text} }} else {{ {else_text} }};\n",
                    self.rendered_type(local.ty)?,
                    self.truthy_operand_expr(cond)?
                ));
                self.mark_local_declared(*then_dest);
                return self.emit_block(self.block(*then_target)?, out);
            }
            if let (
                Some((then_prefix, then_dest, then_value)),
                Some((else_prefix, else_dest, else_value)),
            ) = (
                branch_trailing_assignment(then.statements.as_slice()),
                branch_trailing_assignment(else_.statements.as_slice()),
            ) && then_dest == else_dest
            {
                let local = self.local_decl(then_dest)?;
                let name = self.local_name(then_dest)?;
                let default_text = self.default_value(local.ty)?;
                out.push_str(&format!(
                    "    let mut {name}: {} = {default_text};\n",
                    self.rendered_type(local.ty)?
                ));
                self.mark_local_declared(then_dest);
                let branch_declared = self.declared_scope();
                out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
                for statement in then_prefix {
                    self.emit_statement(statement, out)?;
                }
                let then_text = self.rvalue_expr_for_dest(then_value, local.ty)?;
                out.push_str(&format!("    {name} = {then_text};\n"));
                out.push_str("    } else {\n");
                branch_declared.reset();
                for statement in else_prefix {
                    self.emit_statement(statement, out)?;
                }
                let else_text = self.rvalue_expr_for_dest(else_value, local.ty)?;
                out.push_str(&format!("    {name} = {else_text};\n"));
                out.push_str("    }\n");
                branch_declared.close();
                return self.emit_block(self.block(*then_target)?, out);
            }
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            self.emit_block_until_goto(then, *then_target, RegionExit::Join, out)?;
            out.push_str("    } else {\n");
            branch_declared.reset();
            self.emit_block_until_goto(else_, *else_target, RegionExit::Join, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return self.emit_block(self.block(*then_target)?, out);
        }

        // This reconstruction runs the then-branch's statements, `break`s out of
        // the synthetic label, then resumes at `else_target` — so it models
        // `if cond { then… } else { else… }` where BOTH arms rejoin at
        // `else_target`. It silently discards the then-branch's own `Goto`
        // target. That is only sound when the then-branch actually rejoins the
        // else continuation; when the then-branch instead diverges into its own
        // terminating region (e.g. a `for` loop that always `return`s, as in
        // es-toolkit `some`'s non-array branch), dropping `then_target` deletes
        // that whole region and control wrongly falls through into the else
        // continuation, reading its loop counter uninitialized (E0381). Leave
        // such a diverging then-branch to the structured-if case below, which
        // emits `then_target` in full inside the `if` arm.
        if let (Some(Terminator::Goto(then_target)), Some(Terminator::Goto(else_target))) =
            (&then.terminator, &else_.terminator)
            && then_target != else_target
            && self.block_can_reach(*then_target, *else_target, &mut BlockIdSet::default())
        {
            let branch_label = format!(
                "'smelt_branch_{}_{}_{}_{}",
                current.0,
                then_block.0,
                else_block.0,
                out.len()
            );
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    {branch_label}: {{\n"));
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            for statement in &then.statements {
                self.emit_statement(statement, out)?;
            }
            out.push_str(&format!("    break {branch_label};\n"));
            out.push_str("    }\n");
            branch_declared.reset();
            self.emit_block_until_goto(else_, *else_target, RegionExit::Join, out)?;
            out.push_str("    };\n");
            branch_declared.close();
            return self.emit_block(self.block(*else_target)?, out);
        }

        // A short-circuit branch may call a function before reaching the same
        // join as its direct sibling. Keep emitting the common continuation
        // instead of treating the direct edge as an unstructured escape.
        if let Some(Terminator::Goto(then_target)) = then.terminator
            && self.branch_join_target(else_.id, &mut BlockIdSet::default())? == Some(then_target)
        {
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            self.emit_block_until_goto(then, then_target, RegionExit::Join, out)?;
            out.push_str("    } else {\n");
            branch_declared.reset();
            self.emit_block_until_goto(else_, then_target, RegionExit::Join, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return self.emit_block(self.block(then_target)?, out);
        }

        if let Some(Terminator::Goto(else_target)) = else_.terminator
            && self.branch_join_target(then.id, &mut BlockIdSet::default())? == Some(else_target)
        {
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            self.emit_block_until_goto(then, else_target, RegionExit::Join, out)?;
            out.push_str("    } else {\n");
            branch_declared.reset();
            self.emit_block_until_goto(else_, else_target, RegionExit::Join, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return self.emit_block(self.block(else_target)?, out);
        }

        self.emit_switch_forward_branches(current, cond, then_block, else_block, then, else_, out)
    }

    /// A branch that jumps forward past its sibling, or back to a loop header.
    fn emit_switch_forward_branches(
        &self,
        current: smelt_mir::BlockId,
        cond: &Operand,
        then_block: smelt_mir::BlockId,
        else_block: smelt_mir::BlockId,
        then: &BasicBlock,
        else_: &BasicBlock,
        out: &mut String,
    ) -> Result<(), EmitError> {
        // An `if` with no `else` whose arm is itself structured control flow
        // (a loop, a nested branch) and then falls through to the `else`
        // block: that block is the statement's JOIN, not an alternative arm.
        // MIR allocates the join before lowering the arm, so the arm's exit
        // edge to it points to a LOWER block id, and the rule below — which
        // emits `if c { arm } else { join }` — dropped that edge as if it were
        // a loop back-edge: every run through the arm silently skipped the
        // rest of the function (Hono's reg-exp router `add` returned right
        // after creating a new method's handler maps). Emit it as the `if`
        // it is: the arm up to the join, then the join once.
        if then_block.0 > current.0
            && else_block.0 > current.0
            && matches!(
                then.terminator,
                Some(Terminator::Goto(_) | Terminator::Call { .. } | Terminator::Switch { .. })
            )
            && self.block_reaches_within_region(
                then_block,
                else_block,
                current,
                &mut BlockIdSet::default(),
            )
            && !self.block_reaches_within_region(
                else_block,
                then_block,
                current,
                &mut BlockIdSet::default(),
            )
        {
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            self.emit_block_until_goto(then, else_block, RegionExit::Join, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return self.emit_block(else_, out);
        }

        if let Some(Terminator::Goto(then_target)) = then.terminator
            && then_target.0 > current.0
            && (self.block_eventually_terminates(then_target, &mut BlockIdSet::default())?
                || self.while_header(self.block(then_target)?)?.is_some()
                || self
                    .while_header_with_latch(self.block(then_target)?)?
                    .is_some())
        {
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            for statement in &then.statements {
                self.emit_statement(statement, out)?;
            }
            self.emit_block(self.block(then_target)?, out)?;
            out.push_str("    } else {\n");
            branch_declared.reset();
            self.emit_block(else_, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return Ok(());
        }

        // Short-circuit join where the else arm is itself a diamond (e.g. the
        // `&&` in `x = f() || (g() && h())`) so its terminator is not a bare
        // `Goto` — `branch_join_target` (which only chases `Goto`/`Call`) cannot
        // see that the diamond ultimately rejoins `then_target`, so the
        // structured-if case above (`branch_join_target(else_) == then_target`)
        // does not fire. Without this branch, control falls through to the
        // labeled-block reconstruction below, whose `self.emit_block(else_)`
        // pulls the ENTIRE shared join continuation (the `<tail>; return true`)
        // *inside* the synthetic label. The then-arm's `break {label}` then
        // skips that whole tail and control falls off the label into the
        // function epilogue (`return SmeltUnknown::Null` -> coerced false),
        // silently deleting the join — the root cause of the es-toolkit
        // isEqualWith / isEqual deep-object failures.
        //
        // When the else arm provably rejoins `then_target`, emit a plain
        // structured `if/else` and emit the shared join `then_target` exactly
        // ONCE after it. Both arms resume at the join: the then-arm via its
        // implicit `Goto(then_target)` fallthrough, the else-arm via
        // `emit_block_until_goto(else_, then_target)` stopping at the join. The
        // label/break is unnecessary and, crucially, the join is no longer
        // trapped inside a block the then-arm jumps over. Restricted to a
        // forward `then_target` so backward (loop-latch) edges keep their
        // existing handling.
        if let Some(Terminator::Goto(then_target)) = then.terminator
            && then_target.0 > current.0
            && self.block_can_reach(else_.id, then_target, &mut BlockIdSet::default())
        {
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            for statement in &then.statements {
                self.emit_statement(statement, out)?;
            }
            out.push_str("    } else {\n");
            branch_declared.reset();
            self.emit_block_until_goto(else_, then_target, RegionExit::Join, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return self.emit_block(self.block(then_target)?, out);
        }

        if let Some(Terminator::Goto(then_target)) = then.terminator {
            let branch_label = format!(
                "'smelt_branch_{}_{}_{}_{}",
                current.0,
                then_block.0,
                else_block.0,
                out.len()
            );
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    {branch_label}: {{\n"));
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            for statement in &then.statements {
                self.emit_statement(statement, out)?;
            }
            if then_target.0 <= current.0 {
                out.push_str(&format!("    break {branch_label};\n"));
                out.push_str("    }\n");
                branch_declared.reset();
                self.emit_block_until_goto(else_, then_target, RegionExit::Join, out)?;
                out.push_str("    };\n");
                branch_declared.close();
                // After the labeled block, the `break {branch_label}` path (the
                // then-branch) rejoins the shared continuation at `then_target`.
                // When that continuation always diverges (returns/throws), re-emit
                // it here so the reconstructed region itself diverges rather than
                // falling off the labeled-block statement with a `()` value in a
                // tail/value position (E0308). This mirrors the forward-join
                // reconstruction above, which re-emits `else_target` after its
                // labeled block. True loop latches — backward edges whose target
                // does not eventually terminate — keep the previous behavior so we
                // do not re-emit a loop body outside its loop.
                if self.block_eventually_terminates(then_target, &mut BlockIdSet::default())? {
                    return self.emit_block(self.block(then_target)?, out);
                }
                // The labeled-block statement falls through (see the forward
                // sibling below); a diverging terminator emitted inside it does
                // not diverge the enclosing function, so clear the flag to keep
                // `emit_body`'s trailing fallthrough return.
                self.flow.record(Flow::FallsThrough);
                return Ok(());
            }
            out.push_str(&format!("    break {branch_label};\n"));
            out.push_str("    }\n");
            branch_declared.reset();
            self.emit_block(else_, out)?;
            out.push_str("    };\n");
            branch_declared.close();
            // Control always falls through the labeled-block *statement*: the
            // `break {branch_label}` path exits the block and the else body
            // rejoins after it, so the reconstruction never diverges the
            // enclosing function even when a branch emitted inside it ended in a
            // `return`/`throw` (which would leave the recorded flow at `Flow::Diverges`). The
            // shared forward continuation (`then_target`) is not re-emitted here,
            // so the body genuinely falls off the block; clear the flag so
            // `emit_body` still appends its trailing fallthrough return instead of
            // leaving the labeled block's `()` value in tail position (E0308 in
            // es-toolkit `has`/`slice`/`updateWith`).
            self.flow.record(Flow::FallsThrough);
            return Ok(());
        }

        self.emit_switch_terminating_branches(current, cond, then, else_, out)
    }

    /// Branches at least one of which ends the region, then the plain `if`/`else`.
    fn emit_switch_terminating_branches(
        &self,
        current: smelt_mir::BlockId,
        cond: &Operand,
        then: &BasicBlock,
        else_: &BasicBlock,
        out: &mut String,
    ) -> Result<(), EmitError> {
        if self.block_eventually_terminates(then.id, &mut BlockIdSet::default())?
            && self.block_eventually_terminates(else_.id, &mut BlockIdSet::default())?
        {
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            self.emit_block(then, out)?;
            out.push_str("    } else {\n");
            branch_declared.reset();
            self.emit_block(else_, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return Ok(());
        }

        if self.block_eventually_terminates(then.id, &mut BlockIdSet::default())? {
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            self.emit_block(then, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            if else_.id.0 <= current.0 {
                out.push_str("    loop { break; }\n");
                return Ok(());
            }
            return self.emit_block(else_, out);
        }

        if self.block_eventually_terminates(else_.id, &mut BlockIdSet::default())? {
            let branch_declared = self.declared_scope();
            out.push_str(&format!(
                "    if !({}) {{\n",
                self.truthy_operand_expr(cond)?
            ));
            self.emit_block(else_, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            if then.id.0 <= current.0 {
                out.push_str("    loop { break; }\n");
                return Ok(());
            }
            return self.emit_block(then, out);
        }

        if let (Some(then_join), Some(else_join)) = (
            self.branch_join_target(then.id, &mut BlockIdSet::default())?,
            self.branch_join_target(else_.id, &mut BlockIdSet::default())?,
        ) && then_join == else_join
        {
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            self.emit_block_until_goto(then, then_join, RegionExit::Join, out)?;
            out.push_str("    } else {\n");
            branch_declared.reset();
            self.emit_block_until_goto(else_, then_join, RegionExit::Join, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return self.emit_block(self.block(then_join)?, out);
        }

        // Each arm is a SELF-CONTAINED region: no path out of either one falls
        // through to a shared continuation, so there is no join to find and each
        // can simply be emitted in full inside its own arm.
        //
        // This is the same reconstruction as the
        // `block_eventually_terminates(then) && block_eventually_terminates(else_)`
        // arm above, asked with the predicate that can see through a loop. That
        // arm cannot fire when either region contains one, because a loop header
        // is a `Switch` whose body edge cycles back and
        // `block_eventually_terminates` scores a back edge as "does not
        // terminate" — so `if (cond) { for (…) {…} return }` followed by a
        // continuation missed every structured case and fell into the fallback
        // below.
        //
        // The fallback below emits only each block's OWN statements and drops
        // both terminators, which deletes everything reachable through them. In
        // Hono's `RegExpRouter#add` that silently deleted the entire tail of the
        // method: the then-branch had no statements of its own (its content
        // hangs off a `Call` terminator), so it emitted as `if cond { }` with the
        // continuation moved into the `else`, losing two loop nests, the
        // `#insertPath` calls and the middleware writes. Nothing reported it —
        // the E0308 that surfaced was only the function then falling off its own
        // end.
        //
        // So this arm sits immediately before that fallback: it can only claim
        // shapes whose code was previously discarded.
        if self.block_never_falls_through(then.id, &mut BlockIdSet::default())?
            && self.block_never_falls_through(else_.id, &mut BlockIdSet::default())?
        {
            let branch_declared = self.declared_scope();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
            self.emit_block(then, out)?;
            out.push_str("    } else {\n");
            branch_declared.reset();
            self.emit_block(else_, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return Ok(());
        }

        let branch_declared = self.declared_scope();
        out.push_str(&format!("    if {} {{\n", self.truthy_operand_expr(cond)?));
        for statement in &then.statements {
            self.emit_statement(statement, out)?;
        }
        out.push_str("    } else {\n");
        branch_declared.reset();
        for statement in &else_.statements {
            self.emit_statement(statement, out)?;
        }
        out.push_str("    }\n");
        branch_declared.close();
        Ok(())
    }

    /// Finds a straight-line join block reached by a branch.
    fn branch_join_target(
        &self,
        block_id: smelt_mir::BlockId,
        visited: &mut BlockIdSet,
    ) -> Result<Option<smelt_mir::BlockId>, EmitError> {
        if !visited.insert(block_id) {
            return Ok(None);
        }
        let block = self.block(block_id)?;
        match &block.terminator {
            Some(Terminator::Goto(target)) => Ok(Some(*target)),
            Some(Terminator::Call { target, .. }) => self.branch_join_target(*target, visited),
            _ => Ok(None),
        }
    }

    /// Returns whether every straight-line successor from `block_id` ends in a
    /// return, throw, or unreachable terminator before it can fall through.
    pub(in crate::emitter) fn block_eventually_terminates(
        &self,
        block_id: smelt_mir::BlockId,
        visiting: &mut BlockIdSet,
    ) -> Result<bool, EmitError> {
        if let Some(result) = self.termination_cache.borrow().get(&block_id).copied() {
            return Ok(result);
        }
        if !visiting.insert(block_id) {
            return Ok(false);
        }

        let block = self.block(block_id)?;
        let result = match &block.terminator {
            Some(Terminator::Return(_) | Terminator::Throw(_)) => true,
            Some(Terminator::Goto(target)) => {
                self.block_eventually_terminates(*target, visiting)?
            }
            Some(Terminator::Call { target, .. } | Terminator::Await { target, .. }) => {
                self.block_eventually_terminates(*target, visiting)?
            }
            Some(Terminator::Switch {
                then_block,
                else_block,
                ..
            }) => {
                self.block_eventually_terminates(*then_block, visiting)?
                    && self.block_eventually_terminates(*else_block, visiting)?
            }
            Some(Terminator::Match { arms, default, .. }) => {
                let default_terminates = if let Some(target) = default {
                    self.block_eventually_terminates(*target, visiting)?
                } else {
                    false
                };
                default_terminates
                    && arms.iter().try_fold(true, |all_terminate, arm| {
                        Ok::<bool, EmitError>(
                            all_terminate
                                && self.block_eventually_terminates(arm.target, visiting)?,
                        )
                    })?
            }
            None => false,
        };

        visiting.remove(&block_id);
        self.termination_cache.borrow_mut().insert(block_id, result);
        Ok(result)
    }

    /// Find where the two arms of a switch inside a forward join region meet
    /// again, before (or at) the region's own `stop`.
    ///
    /// The answer is the branch's immediate post-dominator within the region:
    /// the block `J` every non-diverging path of each arm reaches before
    /// `stop`, chosen as the NEAREST such block (every other candidate is
    /// reached from it). An arm that diverges before `stop` (returns/throws on
    /// every path) places no constraint, so `if (b) return; rest` joins at
    /// `rest`. `stop` itself is always a candidate — the region ends there.
    ///
    /// `None` when both arms diverge (there is nothing to rejoin, the plain
    /// switch emission is right) or when an arm can cycle back to the switch
    /// block itself: that is a loop edge this forward-region rule does not
    /// own, and the existing emission keeps handling it.
    pub(super) fn join_region_branch_join(
        &self,
        switch_block: smelt_mir::BlockId,
        then_block: smelt_mir::BlockId,
        else_block: smelt_mir::BlockId,
        stop: smelt_mir::BlockId,
    ) -> Result<Option<smelt_mir::BlockId>, EmitError> {
        let arms = [then_block, else_block];
        let mut continuing = Vec::new();
        for arm in arms {
            if self.block_reaches_target_avoiding(
                arm,
                switch_block,
                &[stop],
                &mut BlockIdSet::default(),
            ) {
                return Ok(None);
            }
            if !self.block_diverges_before(arm, stop, &mut BlockIdSet::default())? {
                continuing.push(arm);
            }
        }
        if continuing.is_empty() {
            return Ok(None);
        }
        // Candidates: every block a continuing arm reaches without passing
        // `stop`, plus `stop`.
        let mut candidates = vec![stop];
        let mut seen = BlockIdSet::default();
        let mut work = continuing.clone();
        while let Some(block_id) = work.pop() {
            if block_id == stop || !seen.insert(block_id) {
                continue;
            }
            candidates.push(block_id);
            if let Some(terminator) = &self.block(block_id)?.terminator {
                work.extend(control_flow_successors(terminator));
            }
        }
        let post_dominates = |candidate: smelt_mir::BlockId| {
            candidate == stop
                || continuing.iter().all(|arm| {
                    self.block_reaches_target_avoiding(
                        *arm,
                        candidate,
                        &[stop],
                        &mut BlockIdSet::default(),
                    ) && !self.block_reaches_target_avoiding(
                        *arm,
                        stop,
                        &[candidate],
                        &mut BlockIdSet::default(),
                    )
                })
        };
        let valid = candidates
            .into_iter()
            .filter(|candidate| post_dominates(*candidate))
            .collect::<Vec<_>>();
        Ok(valid
            .iter()
            .copied()
            .find(|candidate| {
                valid.iter().all(|other| {
                    other == candidate
                        || self.block_reaches_target_avoiding(
                            *candidate,
                            *other,
                            &[],
                            &mut BlockIdSet::default(),
                        )
                })
            }))
    }

    /// Emit one arm of a join-region branch: in full when it diverges before
    /// the region's `stop` (it never reaches the join), otherwise up to the
    /// branch's inner `join`.
    pub(super) fn emit_join_region_arm(
        &self,
        arm: smelt_mir::BlockId,
        join: smelt_mir::BlockId,
        stop: smelt_mir::BlockId,
        out: &mut String,
    ) -> Result<(), EmitError> {
        if self.block_diverges_before(arm, stop, &mut BlockIdSet::default())? {
            return self.emit_block(self.block(arm)?, out);
        }
        self.emit_block_until_goto(self.block(arm)?, join, RegionExit::Join, out)
    }

    /// Return whether every path from `block_id` returns, throws or is
    /// unreachable WITHOUT first reaching `stop`.
    ///
    /// [`Self::block_eventually_terminates`] asks the same question of the
    /// whole remaining function, so an arm that merely continues to a join
    /// whose own continuation ends in `return` scores as terminating. Inside a
    /// region bounded by `stop` that is the wrong answer: reaching `stop` is
    /// falling through. A cycle is not proven to terminate, so it answers
    /// `false`.
    fn block_diverges_before(
        &self,
        block_id: smelt_mir::BlockId,
        stop: smelt_mir::BlockId,
        visiting: &mut BlockIdSet,
    ) -> Result<bool, EmitError> {
        if block_id == stop || !visiting.insert(block_id) {
            return Ok(false);
        }
        let block = self.block(block_id)?;
        let result = match &block.terminator {
            Some(Terminator::Return(_) | Terminator::Throw(_)) => true,
            Some(Terminator::Goto(target)) => self.block_diverges_before(*target, stop, visiting)?,
            // A call with an unwind edge forks the region; both successors
            // must diverge.
            Some(terminator @ (Terminator::Call { .. } | Terminator::Await { .. })) => {
                let mut all = true;
                for next in control_flow_successors(terminator) {
                    all = all && self.block_diverges_before(next, stop, visiting)?;
                }
                all
            }
            Some(Terminator::Switch {
                then_block,
                else_block,
                ..
            }) => {
                self.block_diverges_before(*then_block, stop, visiting)?
                    && self.block_diverges_before(*else_block, stop, visiting)?
            }
            Some(Terminator::Match { arms, default, .. }) => {
                let mut all = match default {
                    Some(target) => self.block_diverges_before(*target, stop, visiting)?,
                    None => false,
                };
                for arm in arms {
                    all = all && self.block_diverges_before(arm.target, stop, visiting)?;
                }
                all
            }
            None => false,
        };
        visiting.remove(&block_id);
        Ok(result)
    }

    /// Return true when no path out of `block_id` ever FALLS OUT of the region
    /// it starts — every path either returns, throws, or loops forever.
    ///
    /// This is [`Self::block_eventually_terminates`] with one difference, and
    /// the difference is the whole point: a back edge answers `true` here
    /// instead of `false`. `block_eventually_terminates` asks "does control
    /// reach a `return`?", and for that question a cycle it cannot prove exits
    /// must answer `false`. The question asked HERE is "does control escape this
    /// region and rejoin a continuation?", and a path that goes round a loop has
    /// not escaped — it is still inside the region, so it says nothing either
    /// way and must not veto the answer.
    ///
    /// That distinction matters because `block_eventually_terminates` requires
    /// BOTH arms of a `Switch` to terminate, so a single loop anywhere in a
    /// region — a loop header IS a `Switch` whose body edge cycles back —
    /// poisons the whole region to `false`. A `for` loop that ends in `return`
    /// is exactly that shape, and it is the ordinary way to write an early-exit
    /// branch.
    ///
    /// A region with a genuinely infinite loop also answers `true`, which is
    /// correct for this question: control never reaches the continuation either
    /// way.
    fn block_never_falls_through(
        &self,
        block_id: smelt_mir::BlockId,
        visiting: &mut BlockIdSet,
    ) -> Result<bool, EmitError> {
        // A back edge: this path is still inside the region, so it neither
        // escapes nor terminates. Neutral, which for an `&&`-fold is `true`.
        if !visiting.insert(block_id) {
            return Ok(true);
        }
        let block = self.block(block_id)?;
        let result = match &block.terminator {
            Some(Terminator::Return(_) | Terminator::Throw(_)) => true,
            Some(
                Terminator::Goto(target)
                | Terminator::Call { target, .. }
                | Terminator::Await { target, .. },
            ) => self.block_never_falls_through(*target, visiting)?,
            Some(Terminator::Switch {
                then_block,
                else_block,
                ..
            }) => {
                self.block_never_falls_through(*then_block, visiting)?
                    && self.block_never_falls_through(*else_block, visiting)?
            }
            Some(Terminator::Match { arms, default, .. }) => {
                let default_escapes = match default {
                    Some(target) => self.block_never_falls_through(*target, visiting)?,
                    None => false,
                };
                default_escapes
                    && arms.iter().try_fold(true, |all, arm| {
                        Ok::<bool, EmitError>(
                            all && self.block_never_falls_through(arm.target, visiting)?,
                        )
                    })?
            }
            // No terminator: control runs off the end of the block into the
            // continuation, which IS falling through.
            None => false,
        };
        visiting.remove(&block_id);
        Ok(result)
    }
}

/// Returns the prefix and final local assignment for branch bodies that end by
/// assigning the value consumed after the branch rejoins.
fn branch_trailing_assignment(
    statements: &[Statement],
) -> Option<(&[Statement], LocalId, &Rvalue)> {
    let (last, prefix) = statements.split_last()?;
    let Statement::Assign { dest, value } = last else {
        return None;
    };
    Some((prefix, *dest, value))
}
