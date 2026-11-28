//! Control Flow Match emission helpers.

use super::*;
use crate::rust::RustExpr;
use control_flow::RegionExit;

impl FunctionEmitter<'_> {
    /// Emits a match expression.
    ///
    /// Three strategies handle the arm bodies:
    ///
    /// * When every arm target's *direct* terminator is either a `Goto` to one
    ///   shared block or a diverging terminator (`Return`/`Throw`),
    ///   the arms are straight-line and truly converge on a single continuation.
    ///   That continuation (the "join") is emitted once after the `match`, and
    ///   each arm drops its trailing `Goto` to it (see [`Self::emit_block_as_match_arm`]).
    ///
    /// * Otherwise, when `match_block` is given and its arms rejoin at its
    ///   immediate post-dominator `J` inside the region being emitted (see
    ///   [`Self::branch_region_join`]), each arm is emitted up to `J` and `J`
    ///   once after the `match`: a `case` that calls a function, branches or
    ///   loops before its `break` reaches `J` only transitively. Without this
    ///   rule each arm re-emitted the whole continuation, so `n` sequential
    ///   `switch` statements rendered the code after them `cases^n` times.
    ///
    /// * Otherwise each arm is emitted as a self-contained region via
    ///   [`Self::emit_region_flow`], following its control-flow tail to its
    ///   own continuation. `match_block` is `None` where the caller bounds the
    ///   region by a stop this function cannot see.
    pub(super) fn emit_match(
        &self,
        match_block: Option<smelt_mir::BlockId>,
        scrutinee: &Operand,
        arms: &[smelt_mir::MatchArm],
        default: Option<smelt_mir::BlockId>,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let scrutinee_text = self.match_scrutinee_expr(scrutinee)?;
        let scrutinee_ty = self.operand_ty(scrutinee)?;
        let join = match self.match_join(arms, default)? {
            Some(join) => Some(MatchJoin::Hoisted(join)),
            None => match_block
                .and_then(|block| self.branch_region_join(block, &match_targets(arms, default)))
                .map(MatchJoin::Region),
        };
        out.push_str(&format!(
            "    match {scrutinee_text} {{\n",
            scrutinee_text = scrutinee_text.delimited()
        ));
        let match_declared = self.declared_scope();
        // Without a join each arm renders its own tail, so the match diverges
        // only when every arm *and* the catch-all diverge. Track it across
        // arms rather than trusting the last arm emitted, so a fall-through
        // arm before a diverging one is not mistaken for total divergence.
        // With a join control always resumes at the join emitted below, which
        // sets the flag on its own.
        let mut arms_diverge = true;
        for arm in arms {
            out.push_str(&format!(
                "        {} => {{\n",
                self.match_label_text_for_scrutinee(&arm.label, scrutinee_ty)
            ));
            let arm_flow = self.emit_match_arm(self.block(arm.target)?, join, out)?;
            arms_diverge = arms_diverge && arm_flow.diverges();
            out.push_str("        }\n");
            match_declared.reset();
        }
        if let Some(default_block) = default {
            out.push_str("        _ => {\n");
            let arm_flow = self.emit_match_arm(self.block(default_block)?, join, out)?;
            arms_diverge = arms_diverge && arm_flow.diverges();
            out.push_str("        }\n");
        } else {
            // A synthesized empty catch-all arm falls through, so the match as
            // a whole never diverges.
            out.push_str("        _ => {}\n");
            arms_diverge = false;
        }
        // Every arm ended with the set reset to the snapshot; close the
        // scope HERE so a join emitted below keeps its declarations.
        match_declared.close();
        out.push_str("    }\n");
        if let Some(MatchJoin::Hoisted(rejoin) | MatchJoin::Region(rejoin)) = join {
            // Control resumes at the shared join; its tail decides divergence.
            self.emit_block(self.block(rejoin)?, out)?;
        } else {
            self.flow
                .record(flow_state::Flow::from_diverges(arms_diverge));
        }
        Ok(())
    }

    /// The block where the arms of the branch at `match_block` (a `match`
    /// whose arm and catch-all targets are `targets`, or a two-way switch
    /// whose targets they are) rejoin, when each arm can be emitted up to it
    /// and it once after the branch.
    ///
    /// The join is the block's immediate post-dominator over the paths that
    /// do not `throw`, nor `return` from inside one arm
    /// ([`Self::branch_rejoin_post_dominator`]): such an arm leaves through
    /// its own `return` and constrains nothing, as for a `try`. `None`
    /// keeps the per-arm emission when there is no such block, or when it is
    /// not inside the region being emitted:
    /// * it is an open loop's header, continue target, exit or exit join, or a
    ///   pending loop-body join (the walkers render an edge there as a jump);
    /// * an arm reaches one of those, or the `match` block itself, without
    ///   passing the join (a loop edge or region exit the per-arm emission
    ///   owns);
    /// * the code from the join on reads a call result bound inside an arm
    ///   (it would be out of scope after the `match`).
    pub(super) fn branch_region_join(
        &self,
        match_block: smelt_mir::BlockId,
        targets: &[smelt_mir::BlockId],
    ) -> Option<smelt_mir::BlockId> {
        let mut boundaries = self.flow.open_loop_boundaries();
        boundaries.extend(self.flow.pending_joins());
        let join = self.branch_rejoin_post_dominator(match_block, targets, &boundaries)?;
        if boundaries.contains(&join) || join == match_block {
            return None;
        }
        boundaries.push(match_block);
        for arm in targets {
            if *arm != join
                && boundaries.iter().any(|boundary| {
                    self.block_reaches_target_avoiding(
                        *arm,
                        *boundary,
                        &[join],
                        &mut BlockIdSet::default(),
                    )
                })
            {
                return None;
            }
        }
        (!self.region_result_read_after(targets, join, &boundaries)).then_some(join)
    }

    /// Emits a `match` that appears inside a loop body, routing each arm's tail
    /// through the loop's back-edge/exit machinery.
    ///
    /// When the arms rejoin inside the loop body (the `match` block's
    /// immediate post-dominator within the loop region, `loop_branch_join`),
    /// each arm ends at that join and the join is emitted once after the
    /// `match`, exactly as [`Self::emit_loop_branch_at_join`] does for a
    /// two-way branch: a run of sequential `switch` statements in a loop body
    /// otherwise multiplied the rest of the body by the case count at each
    /// step. Without a join (an arm reaches the join from a nested loop or
    /// forward region, or there is none) each arm renders its own tail via
    /// [`Self::emit_loop_branch_inner`], where a `Goto` to `continue_target`
    /// becomes `continue` and a `Goto` to `break_target` becomes `break`.
    /// This lets a loop whose body is a `switch` (`for (...) { switch (...)
    /// { ... } }`) emit as a real loop instead of a run-once straight-line
    /// block. Each arm gets its own clone of `visited` because sibling arms
    /// legitimately converge on the same latch block.
    #[expect(
        clippy::too_many_arguments,
        reason = "the match operands plus the loop's continue/break context and recursion guard"
    )]
    pub(super) fn emit_loop_match(
        &self,
        match_block: smelt_mir::BlockId,
        scrutinee: &Operand,
        arms: &[smelt_mir::MatchArm],
        default: Option<smelt_mir::BlockId>,
        continue_target: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
        visited: &mut BlockIdSet,
    ) -> Result<(), EmitError> {
        if let Some(join) = self.loop_branch_join(match_block, continue_target, break_target) {
            let join_scope = self.flow.push_loop_join_stop(join, continue_target);
            let mut arms_text = String::new();
            let emitted = self.emit_loop_match_arms(
                scrutinee,
                arms,
                default,
                continue_target,
                break_target,
                &mut arms_text,
                visited,
            );
            let violated = join_scope.finish();
            emitted?;
            if !violated {
                out.push_str(&arms_text);
                return self.emit_loop_branch_inner(
                    self.block(join)?,
                    continue_target,
                    break_target,
                    out,
                    visited,
                );
            }
        }
        self.emit_loop_match_arms(
            scrutinee,
            arms,
            default,
            continue_target,
            break_target,
            out,
            visited,
        )
    }

    /// The `match` of [`Self::emit_loop_match`], each arm emitted through
    /// the loop walker (ending at a pending join, if one is registered).
    #[expect(
        clippy::too_many_arguments,
        reason = "the match operands plus the loop's continue/break context and recursion guard"
    )]
    fn emit_loop_match_arms(
        &self,
        scrutinee: &Operand,
        arms: &[smelt_mir::MatchArm],
        default: Option<smelt_mir::BlockId>,
        continue_target: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
        visited: &BlockIdSet,
    ) -> Result<(), EmitError> {
        let scrutinee_text = self.match_scrutinee_expr(scrutinee)?;
        let scrutinee_ty = self.operand_ty(scrutinee)?;
        out.push_str(&format!(
            "    match {scrutinee_text} {{\n",
            scrutinee_text = scrutinee_text.delimited()
        ));
        let match_declared = self.declared_scope();
        for arm in arms {
            out.push_str(&format!(
                "        {} => {{\n",
                self.match_label_text_for_scrutinee(&arm.label, scrutinee_ty)
            ));
            self.emit_loop_branch_inner(
                self.block(arm.target)?,
                continue_target,
                break_target,
                out,
                &mut visited.clone(),
            )?;
            out.push_str("        }\n");
            match_declared.reset();
        }
        if let Some(default_block) = default {
            out.push_str("        _ => {\n");
            self.emit_loop_branch_inner(
                self.block(default_block)?,
                continue_target,
                break_target,
                out,
                &mut visited.clone(),
            )?;
            out.push_str("        }\n");
        } else {
            out.push_str("        _ => {}\n");
        }
        // Every arm ended with the set reset to the snapshot; close the
        // scope HERE so a join emitted below keeps its declarations.
        match_declared.close();
        out.push_str("    }\n");
        Ok(())
    }

    /// Emits a single arm body for the `match`'s `join` strategy.
    ///
    /// With a hoisted join the arm drops its trailing `Goto` to it; with a
    /// region join it runs up to the join; without one the arm emits its full
    /// control-flow tail inline.
    fn emit_match_arm(
        &self,
        block: &BasicBlock,
        join: Option<MatchJoin>,
        out: &mut String,
    ) -> Result<flow_state::Flow, EmitError> {
        match join {
            Some(MatchJoin::Hoisted(_)) => {
                self.emit_block_as_match_arm(block, out)?;
                Ok(self.flow.last_flow())
            }
            Some(MatchJoin::Region(rejoin)) => {
                if block.id != rejoin {
                    self.emit_block_until_goto(block, rejoin, RegionExit::Join, out)?;
                }
                Ok(self.flow.last_flow())
            }
            None => self.emit_region_flow(block, out),
        }
    }

    /// Emits a match arm body whose trailing `Goto` to the shared join is dropped.
    ///
    /// Only used on the hoisted-join path, where the join block is emitted once
    /// after the whole `match` and every arm falls straight through to it.
    pub(super) fn emit_block_as_match_arm(
        &self,
        block: &BasicBlock,
        out: &mut String,
    ) -> Result<(), EmitError> {
        for statement in &block.statements {
            self.emit_statement(statement, out)?;
        }
        match &block.terminator {
            Some(Terminator::Goto(_)) => Ok(()),
            Some(terminator) => self.emit_terminator(block.id, terminator, out),
            None => Err(EmitError::new("basic block has no terminator")),
        }
    }

    /// Finds a single join block that every arm falls straight through to.
    ///
    /// Returns `Some(join)` only when every arm target's direct terminator is a
    /// `Goto` to the same block or a diverging terminator; the shared block can
    /// then be hoisted out of the `match`. Returns `None` when the arms have
    /// heterogeneous successors (an arm ends in a `Switch`/`Call`/`Await`/`Match`,
    /// or two arms `Goto` different blocks), because in that case there is no
    /// single block to hoist and each arm must carry its own control-flow tail.
    pub(super) fn match_join(
        &self,
        arms: &[smelt_mir::MatchArm],
        default: Option<smelt_mir::BlockId>,
    ) -> Result<Option<smelt_mir::BlockId>, EmitError> {
        let mut join = None;
        for target in arms.iter().map(|arm| arm.target).chain(default) {
            let block = self.block(target)?;
            match &block.terminator {
                Some(Terminator::Goto(join_target)) => {
                    if join
                        .replace(*join_target)
                        .is_some_and(|seen| seen != *join_target)
                    {
                        return Ok(None);
                    }
                }
                // A diverging arm imposes no join constraint.
                Some(Terminator::Return(_) | Terminator::Throw(_)) => {}
                // An arm ending in a branch/loop/call reaches its continuation
                // only transitively, so no single block can be hoisted.
                _ => return Ok(None),
            }
        }
        Ok(join)
    }

    /// Converts a match scrutinee operand to its Rust text representation.
    pub(super) fn match_scrutinee_expr(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        let operand_ty = self.operand_ty(operand)?;
        // Compare the resolved type directly rather than interning `Type::String | Type::PyStr`
        // and comparing ids: a program that never uses a string (e.g. a switch
        // over a purely numeric enum) may not have `Type::String | Type::PyStr` in the type
        // table at all, and requiring it there would spuriously error.
        if self.mir.types.get(operand_ty).is_some_and(Type::is_text) {
            match operand {
                // A local narrowed to `&'static str` (see
                // `emitter::typeof_str_locals`) is already the `&str` the string
                // arms match against, and has no `.as_str()` to call.
                Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))
                    if self.local_is_static_typeof_str(*local) =>
                {
                    self.place_expr(&Place::Local(*local))
                }
                // A JavaScript string is matched by its text against the
                // `&str` case labels.
                Operand::Copy(place) | Operand::Move(place)
                    if self.mir.types.get(operand_ty) == Some(&Type::String) =>
                {
                    Ok(self
                        .place_expr(place)?
                        .postfix(".to_string_lossy().as_str()"))
                }
                Operand::Copy(place) | Operand::Move(place) => {
                    Ok(self.place_expr(place)?.postfix(".as_str()"))
                }
                Operand::Const(Constant::String(value)) => {
                    Ok(RustExpr::chain(format!("{value:?}")))
                }
                Operand::Const(_) => self.operand_expr(operand),
            }
        } else if matches!(
            self.mir.types.get(operand_ty),
            Some(Type::Optional(inner, _)) if self.mir.types.get(*inner).is_some_and(Type::is_text)
        ) {
            let receiver = self.operand_expr(operand)?;
            if matches!(self.mir.types.get(operand_ty), Some(Type::Optional(inner, _)) if self.mir.types.get(*inner) == Some(&Type::String))
            {
                return Ok(
                    receiver.postfix(".map(|smelt_text| smelt_text.to_string_lossy()).as_deref()")
                );
            }
            Ok(receiver.postfix(".as_deref()"))
        } else {
            self.operand_expr(operand)
        }
    }

    /// Converts a constant to its match label text.
    pub(super) fn match_label_expr(&self, constant: &Constant) -> RustExpr {
        if let Constant::String(value) = constant {
            RustExpr::chain(format!("{value:?}"))
        } else {
            constant_expr(constant, self.indexes_utf16())
        }
    }

    /// Converts a constant to a Rust match label for the scrutinee type.
    pub(super) fn match_label_text_for_scrutinee(
        &self,
        constant: &Constant,
        scrutinee_ty: TypeId,
    ) -> String {
        if matches!(
            self.mir.types.get(scrutinee_ty),
            Some(Type::Optional(inner, _)) if self.mir.types.get(*inner).is_some_and(Type::is_text)
        ) && let Constant::String(value) = constant
        {
            return format!("Some({value:?})");
        }
        self.match_label_expr(constant).into_string()
    }
}

/// Where the arms of a function-level `match` rejoin (see
/// [`FunctionEmitter::emit_match`]).
#[derive(Clone, Copy)]
enum MatchJoin {
    /// Every arm `Goto`s this block directly (or diverges).
    Hoisted(smelt_mir::BlockId),
    /// The arms reach this block, the `match`'s immediate post-dominator,
    /// through their own control flow.
    Region(smelt_mir::BlockId),
}

/// The arm targets of a `match`, then its catch-all target.
pub(super) fn match_targets(
    arms: &[smelt_mir::MatchArm],
    default: Option<smelt_mir::BlockId>,
) -> Vec<smelt_mir::BlockId> {
    arms.iter().map(|arm| arm.target).chain(default).collect()
}
