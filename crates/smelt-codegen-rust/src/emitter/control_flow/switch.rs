//! `Switch` terminators: structured `if`/`else` emission and the joins where
//! branches meet again.

use super::*;

impl FunctionEmitter<'_> {
    /// Emits an if/else or while loop from a switch terminator.
    ///
    /// `rejoin` allows the post-dominator join
    /// ([`Self::emit_switch_at_post_dominator`]); it is `false` where the
    /// caller bounds the region by a stop that rule cannot see.
    pub(in crate::emitter) fn emit_switch(
        &self,
        current: smelt_mir::BlockId,
        cond: &Operand,
        then_block: smelt_mir::BlockId,
        else_block: smelt_mir::BlockId,
        rejoin: bool,
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
                self.truthy_operand_expr(cond)?.delimited()
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
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
            self.emit_block_until_goto(then, else_block, RegionExit::Join, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return self.emit_block(else_, out);
        }

        self.emit_switch_two_gotos(current, cond, then_block, else_block, rejoin, out)
    }

    /// A switch whose arms are, or begin with, plain jumps.
    fn emit_switch_two_gotos(
        &self,
        current: smelt_mir::BlockId,
        cond: &Operand,
        then_block: smelt_mir::BlockId,
        else_block: smelt_mir::BlockId,
        rejoin: bool,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let then = self.block(then_block)?;
        let else_ = self.block(else_block)?;
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
                    self.truthy_operand_expr(cond)?.delimited(),
                    then_text = then_text.delimited(),
                    else_text = else_text.delimited()
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
                    self.rendered_type(local.ty)?,
                    default_text = default_text.delimited()
                ));
                self.mark_local_declared(then_dest);
                let branch_declared = self.declared_scope();
                out.push_str(&format!(
                    "    if {} {{\n",
                    self.truthy_operand_expr(cond)?.delimited()
                ));
                for statement in then_prefix {
                    self.emit_statement(statement, out)?;
                }
                let then_text = self.rvalue_expr_for_dest(then_value, local.ty)?;
                out.push_str(&format!(
                    "    {name} = {then_text};\n",
                    then_text = then_text.delimited()
                ));
                out.push_str("    } else {\n");
                branch_declared.reset();
                for statement in else_prefix {
                    self.emit_statement(statement, out)?;
                }
                let else_text = self.rvalue_expr_for_dest(else_value, local.ty)?;
                out.push_str(&format!(
                    "    {name} = {else_text};\n",
                    else_text = else_text.delimited()
                ));
                out.push_str("    }\n");
                branch_declared.close();
                return self.emit_block(self.block(*then_target)?, out);
            }
            let branch_declared = self.declared_scope();
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
            self.emit_block_until_goto(then, *then_target, RegionExit::Join, out)?;
            out.push_str("    } else {\n");
            branch_declared.reset();
            self.emit_block_until_goto(else_, *else_target, RegionExit::Join, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return self.emit_block(self.block(*then_target)?, out);
        }

        if rejoin
            && self.emit_switch_at_post_dominator(current, cond, then_block, else_block, out)?
        {
            return Ok(());
        }

        // This reconstruction runs the then-branch's region up to `else_target`,
        // `break`s out of the synthetic label, then resumes at `else_target` —
        // so it models `if cond { then… } else { else… }` where BOTH arms rejoin
        // at `else_target`. The then-branch's region is every block its `Goto`
        // reaches before `else_target` (a `catch` handler a `throw` inside a
        // `try` jumps to, say), and it is emitted in full inside the arm. That
        // is only sound when the then-branch actually rejoins the else
        // continuation; when the then-branch instead diverges into its own
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
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
            self.emit_block_until_goto(then, *else_target, RegionExit::Join, out)?;
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
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
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
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
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

    /// Emits the branch at `current` as `if cond { then..J } else { else..J }`
    /// followed by `J` once, where `J` is where its arms rejoin
    /// ([`Self::branch_region_join`]: the immediate post-dominator over the
    /// paths that do not throw, inside the region being emitted). Answers `false`, having emitted nothing, when
    /// there is no such block.
    ///
    /// The rules after this one pick a join from the arms' first jumps. That
    /// guess is wrong when an arm leaves through an edge the other cannot see:
    /// `try { if (c) { x = f(); } else { .. } } catch { .. }` joins the `if`
    /// after the `try`, not where the call's normal path meets the `else`,
    /// and the call's `catch` arm then re-emitted the rest of the function
    /// (`4^n` copies for `n` such statements). And an arm that may throw
    /// (`if (a) { if (b) throw e; .. } else { .. }`) has no join at all when a
    /// `throw` ends a path, so each such `if` doubled the rest.
    fn emit_switch_at_post_dominator(
        &self,
        current: smelt_mir::BlockId,
        cond: &Operand,
        then_block: smelt_mir::BlockId,
        else_block: smelt_mir::BlockId,
        out: &mut String,
    ) -> Result<bool, EmitError> {
        let Some(join) = self.branch_region_join(current, &[then_block, else_block]) else {
            return Ok(false);
        };
        let cond_text = self.truthy_operand_expr(cond)?;
        let branch_declared = self.declared_scope();
        if then_block == join {
            out.push_str(&format!(
                "    if !({cond_text}) {{\n",
                cond_text = cond_text.delimited()
            ));
            self.emit_block_until_goto(self.block(else_block)?, join, RegionExit::Join, out)?;
        } else {
            out.push_str(&format!(
                "    if {cond_text} {{\n",
                cond_text = cond_text.delimited()
            ));
            self.emit_block_until_goto(self.block(then_block)?, join, RegionExit::Join, out)?;
            if else_block != join {
                out.push_str("    } else {\n");
                branch_declared.reset();
                self.emit_block_until_goto(self.block(else_block)?, join, RegionExit::Join, out)?;
            }
        }
        out.push_str("    }\n");
        branch_declared.close();
        self.emit_block(self.block(join)?, out)?;
        Ok(true)
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
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
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
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
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
        // `emit_block_until_goto(else_, then_target)` stopping at the join, so
        // the join cannot be trapped inside a block the then-arm jumps over.
        // Restricted to a forward `then_target` so backward (loop-latch) edges
        // keep their existing handling.
        if let Some(Terminator::Goto(then_target)) = then.terminator
            && then_target.0 > current.0
            && self.block_can_reach(else_.id, then_target, &mut BlockIdSet::default())
        {
            let branch_declared = self.declared_scope();
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
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
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
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
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
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
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
            self.emit_block(then, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            // A lower block id is only a hint of a back edge (see the `Goto`
            // arm of `emit_terminator`): the join after a `try`, allocated
            // before the blocks inside it, is lower too, and is the code that
            // runs next. Only a target that can come back here is the
            // enclosing loop's own continuation.
            if else_.id.0 <= current.0
                && self.block_can_reach(else_.id, current, &mut BlockIdSet::default())
            {
                out.push_str("    loop { break; }\n");
                return Ok(());
            }
            return self.emit_block(else_, out);
        }

        if self.block_eventually_terminates(else_.id, &mut BlockIdSet::default())? {
            let branch_declared = self.declared_scope();
            out.push_str(&format!(
                "    if !({}) {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
            self.emit_block(else_, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            if then.id.0 <= current.0
                && self.block_can_reach(then.id, current, &mut BlockIdSet::default())
            {
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
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
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
        // shapes that nothing else structures.
        if self.block_never_falls_through(then.id, &mut BlockIdSet::default())?
            && self.block_never_falls_through(else_.id, &mut BlockIdSet::default())?
        {
            let branch_declared = self.declared_scope();
            out.push_str(&format!(
                "    if {} {{\n",
                self.truthy_operand_expr(cond)?.delimited()
            ));
            self.emit_block(then, out)?;
            out.push_str("    } else {\n");
            branch_declared.reset();
            self.emit_block(else_, out)?;
            out.push_str("    }\n");
            branch_declared.close();
            return Ok(());
        }

        let branch_declared = self.declared_scope();
        out.push_str(&format!(
            "    if {} {{\n",
            self.truthy_operand_expr(cond)?.delimited()
        ));
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
