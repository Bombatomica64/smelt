//! Control Flow Match emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Emits a match expression.
    pub(super) fn emit_match(
        &self,
        scrutinee: &Operand,
        arms: &[smelt_mir::MatchArm],
        default: Option<smelt_mir::BlockId>,
        out: &mut String,
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
            self.emit_block_as_match_arm(self.block(arm.target)?, out)?;
            out.push_str("        }\n");
            self.restore_declared_locals(match_declared.clone());
        }
        if let Some(default_block) = default {
            out.push_str("        _ => {\n");
            self.emit_block_as_match_arm(self.block(default_block)?, out)?;
            out.push_str("        }\n");
            self.restore_declared_locals(match_declared);
        } else {
            out.push_str("        _ => {}\n");
        }
        out.push_str("    }\n");
        if let Some(join) = self.match_join(arms, default)? {
            self.emit_block(self.block(join)?, out)?;
        }
        Ok(())
    }

    /// Emits a match arm body.
    /// Emits a match arm body.
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

    /// Finds the join block where all match arms converge.
    /// Finds the join block where all match arms converge.
    pub(super) fn match_join(
        &self,
        arms: &[smelt_mir::MatchArm],
        default: Option<smelt_mir::BlockId>,
    ) -> Result<Option<smelt_mir::BlockId>, EmitError> {
        let mut join = None;
        for target in arms.iter().map(|arm| arm.target).chain(default) {
            let block = self.block(target)?;
            if let Some(Terminator::Goto(join_target)) = block.terminator {
                if join
                    .replace(join_target)
                    .is_some_and(|seen| seen != join_target)
                {
                    return Err(EmitError::new(
                        "match codegen requires all non-terminating arms to share one join block",
                    ));
                }
            }
        }
        Ok(join)
    }

    /// Emits a block's statements until reaching a goto to the stop target.
    /// Converts a match scrutinee operand to its Rust text representation.
    pub(super) fn match_scrutinee_text(&self, operand: &Operand) -> Result<String, EmitError> {
        let operand_ty = self.operand_ty(operand)?;
        if operand_ty == self.type_id(Type::String)? {
            match operand {
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
