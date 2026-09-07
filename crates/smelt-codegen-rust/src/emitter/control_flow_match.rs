//! Control Flow Match emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Emits a match expression.
    ///
    /// Two strategies handle the arm bodies:
    ///
    /// * When every arm target's *direct* terminator is either a `Goto` to one
    ///   shared block or a diverging terminator (`Return`/`Throw`/`Unreachable`),
    ///   the arms are straight-line and truly converge on a single continuation.
    ///   That continuation (the "join") is emitted once after the `match`, and
    ///   each arm drops its trailing `Goto` to it (see [`Self::emit_block_as_match_arm`]).
    ///
    /// * Otherwise the arms have heterogeneous successors: some diverge, some
    ///   fall through, and some reach the shared continuation only transitively
    ///   through nested loops or branches whose own blocks end in a `Switch` or a
    ///   `Goto` back into the arm body. There is no single block to hoist, so each
    ///   arm is emitted as a self-contained region via [`Self::emit_block`], which
    ///   already lowers nested loops, branches, and returns and follows each arm's
    ///   control-flow tail to its own continuation. The shared continuation is
    ///   duplicated into every arm that reaches it, mirroring how arms ending in a
    ///   non-`Goto` terminator already emit their tail inline.
    pub(super) fn emit_match(
        &self,
        scrutinee: &Operand,
        arms: &[smelt_mir::MatchArm],
        default: Option<smelt_mir::BlockId>,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let scrutinee_text = self.match_scrutinee_text(scrutinee)?;
        let scrutinee_ty = self.operand_ty(scrutinee)?;
        let hoisted_join = self.match_join(arms, default)?;
        out.push_str(&format!("    match {scrutinee_text} {{\n"));
        let match_declared = self.declared_locals_snapshot();
        // On the no-hoisted-join path each arm renders its own tail, so the
        // match diverges only when every arm *and* the catch-all diverge. Track
        // it across arms rather than trusting the last arm emitted, so a
        // fall-through arm before a diverging one is not mistaken for total
        // divergence. On the hoisted-join path control always resumes at the
        // join emitted below, which sets the flag on its own.
        let mut arms_diverge = true;
        for arm in arms {
            out.push_str(&format!(
                "        {} => {{\n",
                self.match_label_text_for_scrutinee(&arm.label, scrutinee_ty)
            ));
            self.emit_match_arm(self.block(arm.target)?, hoisted_join, out)?;
            arms_diverge = arms_diverge && control_flow::last_emit_diverged();
            out.push_str("        }\n");
            self.restore_declared_locals(match_declared.clone());
        }
        if let Some(default_block) = default {
            out.push_str("        _ => {\n");
            self.emit_match_arm(self.block(default_block)?, hoisted_join, out)?;
            arms_diverge = arms_diverge && control_flow::last_emit_diverged();
            out.push_str("        }\n");
            self.restore_declared_locals(match_declared);
        } else {
            // A synthesized empty catch-all arm falls through, so the match as
            // a whole never diverges.
            out.push_str("        _ => {}\n");
            arms_diverge = false;
        }
        out.push_str("    }\n");
        if let Some(join) = hoisted_join {
            // Control resumes at the shared join; its tail decides divergence.
            self.emit_block(self.block(join)?, out)?;
        } else {
            control_flow::set_last_emit_diverged(arms_diverge);
        }
        Ok(())
    }

    /// Emits a `match` that appears inside a loop body, routing each arm's tail
    /// through the loop's back-edge/exit machinery.
    ///
    /// Unlike [`Self::emit_match`], no shared join is hoisted: a loop-body
    /// `switch` arm reaches the loop's latch/exit only transitively (through the
    /// increment block, a `break`, or a nested loop), so each arm renders its own
    /// tail via [`Self::emit_loop_branch_inner`], where a `Goto` to
    /// `continue_target` becomes `continue` and a `Goto` to `break_target`
    /// becomes `break`. This lets a loop whose body is a `switch`
    /// (`for (...) { switch (...) { ... } }`) emit as a real loop instead of a
    /// run-once straight-line block. Each arm gets its own clone of `visited`
    /// because sibling arms legitimately converge on the same latch block.
    pub(super) fn emit_loop_match(
        &self,
        scrutinee: &Operand,
        arms: &[smelt_mir::MatchArm],
        default: Option<smelt_mir::BlockId>,
        continue_target: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
        visited: &BlockIdSet,
    ) -> Result<(), EmitError> {
        let scrutinee_text = self.match_scrutinee_text(scrutinee)?;
        let scrutinee_ty = self.operand_ty(scrutinee)?;
        out.push_str(&format!("    match {scrutinee_text} {{\n"));
        let match_declared = self.declared_locals_snapshot();
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
            self.restore_declared_locals(match_declared.clone());
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
            self.restore_declared_locals(match_declared);
        } else {
            out.push_str("        _ => {}\n");
        }
        out.push_str("    }\n");
        Ok(())
    }

    /// Emits a single arm body, dispatching on whether a shared join was hoisted.
    ///
    /// With a hoisted join the arm drops its trailing `Goto` to it; without one
    /// the arm emits its full control-flow tail inline.
    fn emit_match_arm(
        &self,
        block: &BasicBlock,
        hoisted_join: Option<smelt_mir::BlockId>,
        out: &mut String,
    ) -> Result<(), EmitError> {
        if hoisted_join.is_some() {
            self.emit_block_as_match_arm(block, out)
        } else {
            self.emit_block(block, out)
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
                Some(Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable) => {}
                // An arm ending in a branch/loop/call reaches its continuation
                // only transitively, so no single block can be hoisted.
                _ => return Ok(None),
            }
        }
        Ok(join)
    }

    /// Emits a block's statements until reaching a goto to the stop target.
    /// Converts a match scrutinee operand to its Rust text representation.
    pub(super) fn match_scrutinee_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let operand_ty = self.operand_ty(operand)?;
        // Compare the resolved type directly rather than interning `Type::String`
        // and comparing ids: a program that never uses a string (e.g. a switch
        // over a purely numeric enum) may not have `Type::String` in the type
        // table at all, and requiring it there would spuriously error.
        if self.mir.types.get(operand_ty) == Some(&Type::String) {
            match operand {
                // A local narrowed to `&'static str` (see
                // `emitter::typeof_str_locals`) is already the `&str` the string
                // arms match against, and has no `.as_str()` to call.
                Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))
                    if self.local_is_static_typeof_str(*local) =>
                {
                    self.place_text(&Place::Local(*local))
                }
                Operand::Copy(place) | Operand::Move(place) => {
                    Ok(format!("{}.as_str()", self.place_text(place)?))
                }
                Operand::Const(Constant::String(value)) => Ok(format!("{value:?}")),
                Operand::Const(_) => self.operand_text(operand),
            }
        } else if matches!(
            self.mir.types.get(operand_ty),
            Some(Type::Optional(inner)) if self.mir.types.get(*inner) == Some(&Type::String)
        ) {
            Ok(format!("{}.as_deref()", self.operand_text(operand)?))
        } else {
            self.operand_text(operand)
        }
    }

    /// Converts a constant to its match label text.
    /// Converts a constant to its match label text.
    pub(super) fn match_label_text(&self, constant: &Constant) -> String {
        match constant {
            Constant::String(value) => format!("{value:?}"),
            _ => constant_text(constant),
        }
    }

    /// Converts a constant to a Rust match label for the scrutinee type.
    fn match_label_text_for_scrutinee(&self, constant: &Constant, scrutinee_ty: TypeId) -> String {
        if matches!(
            self.mir.types.get(scrutinee_ty),
            Some(Type::Optional(inner)) if self.mir.types.get(*inner) == Some(&Type::String)
        ) && let Constant::String(value) = constant
        {
            return format!("Some({value:?})");
        }
        self.match_label_text(constant)
    }

    // Gets the type of an operand.
}
