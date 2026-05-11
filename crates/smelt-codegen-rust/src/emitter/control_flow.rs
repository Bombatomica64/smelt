//! Control Flow emission helpers.

use super::*;

impl FunctionEmitter<'_> {

    /// Emits a basic block's statements and terminator.
    pub(super) fn emit_block(&self, block: &BasicBlock, out: &mut String) -> Result<(), EmitError> {
        if let Some((cond, then_block, else_block, cond_statement_idx)) =
            self.while_header(block)?
        {
            for (idx, statement) in block.statements.iter().enumerate() {
                if idx != cond_statement_idx {
                    self.emit_statement(statement, out)?;
                }
            }
            let then = self.block(then_block)?;
            let else_ = self.block(else_block)?;
            out.push_str(&format!("    while {cond} {{\n"));
            self.emit_block_until_goto(then, block.id, Some(else_block), out)?;
            out.push_str("    }\n");
            return self.emit_block(else_, out);
        }

        if let Some((cond, then_block, latch_block, else_block, cond_statement_idx)) =
            self.while_header_with_latch(block)?
        {
            for (idx, statement) in block.statements.iter().enumerate() {
                if idx != cond_statement_idx {
                    self.emit_statement(statement, out)?;
                }
            }
            let then = self.block(then_block)?;
            let latch = self.block(latch_block)?;
            let else_ = self.block(else_block)?;
            out.push_str(&format!("    while {cond} {{\n"));
            self.emit_block_until_goto(then, latch_block, Some(else_block), out)?;
            for statement in &latch.statements {
                self.emit_statement(statement, out)?;
            }
            out.push_str("    }\n");
            return self.emit_block(else_, out);
        }

        for statement in &block.statements {
            self.emit_statement(statement, out)?;
        }

        let Some(terminator) = &block.terminator else {
            return Err(EmitError::new("basic block has no terminator"));
        };
        self.emit_terminator(block.id, terminator, out)
    }

    /// Emits a single statement.
    /// Emits a single statement.
    pub(super) fn emit_statement(&self, statement: &Statement, out: &mut String) -> Result<(), EmitError> {
        match statement {
            Statement::Assign { dest, value } => {
                let local = self.local_decl(*dest)?;
                let name = self.local_name(*dest)?;
                let rendered_value = self.rvalue_text_for_dest(value, local.ty)?;
                let mutability = if self.mutable_locals.contains(dest)
                    || matches!(self.mir.types.get(local.ty), Some(Type::Class { .. }))
                {
                    "mut "
                } else {
                    ""
                };
                out.push_str(&format!(
                    "    let {mutability}{name}: {} = {rendered_value};\n",
                    self.type_text(local.ty)?
                ));
                Ok(())
            }
            Statement::AssignPlace { place, value } => {
                self.emit_assign_place_statement(place, value, out)
            }
            Statement::StorageLive(_) | Statement::StorageDead(_) => Ok(()),
        }
    }

    /// Emits an assignment to a local, field, or index place.
    /// Emits an assignment to a local, field, or index place.
    pub(super) fn emit_assign_place_statement(
        &self,
        place: &Place,
        value: &Rvalue,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let rendered_value = self.rvalue_text(value)?;
        match place {
            Place::Field { base, field } => {
                let base_ty = self.local_decl(*base)?.ty;
                if let Some(Type::Dict(key, _)) = self.mir.types.get(base_ty)
                    && self.mir.types.get(*key) == Some(&Type::String)
                {
                    out.push_str(&format!(
                        "    {}.insert({:?}.to_owned(), {rendered_value});\n",
                        self.local_name(*base)?,
                        self.symbol_name(*field)?
                    ));
                    return Ok(());
                }
            }
            Place::Index { base, index } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::Dict(_, _)) => {
                        out.push_str(&format!(
                            "    {}.insert({}, {rendered_value});\n",
                            self.local_name(*base)?,
                            self.operand_text(index)?
                        ));
                        return Ok(());
                    }
                    Some(Type::List(_)) => {
                        let base_text = self.local_name(*base)?;
                        let index_text =
                            self.normalized_index_text(&format!("{base_text}.len()"), index)?;
                        out.push_str(&format!(
                            "    {{ let index = {index_text}; {base_text}[index] = {rendered_value}; }}\n"
                        ));
                        return Ok(());
                    }
                    _ => {}
                }
            }
            Place::Local(_) => {}
        }

        let assignment = self.assignment_place_text(place)?;
        out.push_str(&format!("    {assignment} = {rendered_value};\n"));
        Ok(())
    }

    /// Emits a block terminator.
    /// Emits a block terminator.
    pub(super) fn emit_terminator(
        &self,
        current: smelt_mir::BlockId,
        terminator: &Terminator,
        out: &mut String,
    ) -> Result<(), EmitError> {
        match terminator {
            Terminator::Goto(target) => self.emit_block(self.block(*target)?, out),
            Terminator::Call {
                callee,
                args,
                dest,
                target,
            } => {
                let call_text = self.call_text(callee, args)?;
                let local = self.local_decl(*dest)?;
                let name = self.local_name(*dest)?;
                let mutability = if self.mutable_locals.contains(dest)
                    || matches!(self.mir.types.get(local.ty), Some(Type::Class { .. }))
                {
                    "mut "
                } else {
                    ""
                };
                if matches!(self.mir.types.get(local.ty), Some(Type::Future(_))) {
                    out.push_str(&format!("    let {mutability}{name} = {call_text};\n"));
                } else {
                    out.push_str(&format!(
                        "    let {mutability}{name}: {} = {call_text};\n",
                        self.type_text(local.ty)?
                    ));
                }
                self.emit_block(self.block(*target)?, out)
            }
            Terminator::Switch {
                cond,
                then_block,
                else_block,
            } => self.emit_switch(current, cond, *then_block, *else_block, out),
            Terminator::Match {
                scrutinee,
                arms,
                default,
            } => self.emit_match(scrutinee, arms, *default, out),
            Terminator::Return(operand) => {
                if matches!(self.function.origin, HirOrigin::ClassConstructor { .. }) {
                    if self.function.can_throw {
                        out.push_str(&format!(
                            "    return Ok({});\n",
                            self.operand_text(operand)?
                        ));
                    } else {
                        out.push_str(&format!("    return {};\n", self.operand_text(operand)?));
                    }
                } else if self.function.can_throw {
                    if self.function.return_ty == self.none_ty {
                        out.push_str("    return Ok(());\n");
                    } else {
                        out.push_str(&format!(
                            "    return Ok({});\n",
                            self.operand_as_type_text(operand, self.function.return_ty)?
                        ));
                    }
                } else if self.function.return_ty == self.none_ty {
                    if !matches!(operand, Operand::Const(Constant::None)) {
                        out.push_str(&format!("    {};\n", self.operand_text(operand)?));
                    }
                } else {
                    out.push_str(&format!(
                        "    return {};\n",
                        self.operand_as_type_text(operand, self.function.return_ty)?
                    ));
                }
                Ok(())
            }
            Terminator::Throw(operand) => {
                out.push_str(&format!(
                    "    return Err(std::io::Error::new(std::io::ErrorKind::Other, format!(\"{{}}\", {})).into());\n",
                    self.operand_text(operand)?
                ));
                Ok(())
            }
            Terminator::Unreachable => {
                out.push_str("    unreachable!();\n");
                Ok(())
            }
        }
    }

    /// Emits an if/else or while loop from a switch terminator.
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

        if matches!(then.terminator, Some(Terminator::Goto(target)) if target == current) {
            out.push_str(&format!("    while {} {{\n", self.operand_text(cond)?));
            self.emit_block_until_goto(then, current, Some(else_block), out)?;
            out.push_str("    }\n");
            return self.emit_block(else_, out);
        }

        if matches!(then.terminator, Some(Terminator::Goto(target)) if target == else_block) {
            out.push_str(&format!("    if {} {{\n", self.operand_text(cond)?));
            self.emit_block_until_goto(then, else_block, None, out)?;
            out.push_str("    }\n");
            return self.emit_block(else_, out);
        }

        if let (Some(Terminator::Goto(then_target)), Some(Terminator::Goto(else_target))) =
            (&then.terminator, &else_.terminator)
            && then_target == else_target
        {
            out.push_str(&format!("    if {} {{\n", self.operand_text(cond)?));
            self.emit_block_until_goto(then, *then_target, None, out)?;
            out.push_str("    } else {\n");
            self.emit_block_until_goto(else_, *else_target, None, out)?;
            out.push_str("    }\n");
            return self.emit_block(self.block(*then_target)?, out);
        }

        if let (Some(Terminator::Goto(then_target)), Some(Terminator::Goto(else_target))) =
            (&then.terminator, &else_.terminator)
            && then_target != else_target
        {
            out.push_str(&format!("    if {} {{\n", self.operand_text(cond)?));
            for statement in &then.statements {
                self.emit_statement(statement, out)?;
            }
            if then_target.0 <= current.0 {
                out.push_str("    continue;\n");
            } else {
                out.push_str("    break;\n");
            }
            out.push_str("    }\n");
            self.emit_block_until_goto(else_, *else_target, None, out)?;
            return self.emit_block(self.block(*else_target)?, out);
        }

        if let Some(Terminator::Goto(then_target)) = then.terminator {
            out.push_str(&format!("    if {} {{\n", self.operand_text(cond)?));
            for statement in &then.statements {
                self.emit_statement(statement, out)?;
            }
            if then_target.0 <= current.0 {
                out.push_str("    continue;\n");
                out.push_str("    }\n");
                self.emit_block_until_goto(else_, then_target, None, out)?;
                return Ok(());
            }
            out.push_str("    break;\n");
            out.push_str("    }\n");
            return self.emit_block(else_, out);
        }

        if block_terminates(then) && block_terminates(else_) {
            out.push_str(&format!("    if {} {{\n", self.operand_text(cond)?));
            self.emit_block(then, out)?;
            out.push_str("    } else {\n");
            self.emit_block(else_, out)?;
            out.push_str("    }\n");
            return Ok(());
        }

        if block_terminates(then) {
            out.push_str(&format!("    if {} {{\n", self.operand_text(cond)?));
            self.emit_block(then, out)?;
            out.push_str("    }\n");
            return self.emit_block(else_, out);
        }

        Err(EmitError::new(
            "if/else codegen requires structured branches with a shared join or terminating arms",
        ))
    }

    /// Checks if a block starts a while loop and returns loop details.
    /// Checks if a block starts a while loop and returns loop details.
    pub(super) fn while_header(
        &self,
        block: &BasicBlock,
    ) -> Result<Option<(String, smelt_mir::BlockId, smelt_mir::BlockId, usize)>, EmitError> {
        let Some(Terminator::Switch {
            cond: Operand::Copy(Place::Local(cond_local)),
            then_block,
            else_block,
        }) = &block.terminator
        else {
            return Ok(None);
        };
        let then = self.block(*then_block)?;
        if !self.block_exits_to_loop(then, block.id, *else_block, &mut HashSet::new())? {
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
        Ok(Some((
            self.rvalue_text(value)?,
            *then_block,
            *else_block,
            idx,
        )))
    }

    /// Checks if a block eventually exits to a loop target.
    /// Checks if a block eventually exits to a loop target.
    pub(super) fn block_exits_to_loop(
        &self,
        block: &BasicBlock,
        continue_target: smelt_mir::BlockId,
        break_target: smelt_mir::BlockId,
        visited: &mut HashSet<smelt_mir::BlockId>,
    ) -> Result<bool, EmitError> {
        if !visited.insert(block.id) {
            return Ok(true);
        }
        match &block.terminator {
            Some(Terminator::Goto(target)) => {
                Ok(*target == continue_target || *target == break_target)
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
            _ => Ok(false),
        }
    }

    /// Checks if a block starts a while loop with a latch block.
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
            cond: Operand::Copy(Place::Local(cond_local)),
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
        Ok(Some((
            self.rvalue_text(value)?,
            *then_block,
            latch_block,
            *else_block,
            idx,
        )))
    }

    /// Emits a match expression.
    /// Emits a block's statements until reaching a goto to the stop target.
    pub(super) fn emit_block_until_goto(
        &self,
        block: &BasicBlock,
        stop: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
    ) -> Result<(), EmitError> {
        for statement in &block.statements {
            self.emit_statement(statement, out)?;
        }
        match &block.terminator {
            Some(Terminator::Goto(target)) if *target == stop => Ok(()),
            Some(Terminator::Goto(target)) if Some(*target) == break_target => {
                out.push_str("    break;\n");
                Ok(())
            }
            Some(Terminator::Switch {
                cond,
                then_block,
                else_block,
            }) if break_target.is_some() => {
                out.push_str(&format!("    if {} {{\n", self.operand_text(cond)?));
                self.emit_loop_branch(self.block(*then_block)?, stop, break_target, out)?;
                out.push_str("    } else {\n");
                self.emit_loop_branch(self.block(*else_block)?, stop, break_target, out)?;
                out.push_str("    }\n");
                Ok(())
            }
            Some(terminator) => self.emit_terminator(block.id, terminator, out),
            None => Err(EmitError::new("basic block has no terminator")),
        }
    }

    /// Emits a branch inside a loop.
    /// Emits a branch inside a loop.
    pub(super) fn emit_loop_branch(
        &self,
        block: &BasicBlock,
        continue_target: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
    ) -> Result<(), EmitError> {
        for statement in &block.statements {
            self.emit_statement(statement, out)?;
        }
        match &block.terminator {
            Some(Terminator::Goto(target)) if *target == continue_target => {
                out.push_str("    continue;\n");
                Ok(())
            }
            Some(Terminator::Goto(target)) if Some(*target) == break_target => {
                out.push_str("    break;\n");
                Ok(())
            }
            Some(Terminator::Switch {
                cond,
                then_block,
                else_block,
            }) => {
                out.push_str(&format!("    if {} {{\n", self.operand_text(cond)?));
                self.emit_loop_branch(
                    self.block(*then_block)?,
                    continue_target,
                    break_target,
                    out,
                )?;
                out.push_str("    } else {\n");
                self.emit_loop_branch(
                    self.block(*else_block)?,
                    continue_target,
                    break_target,
                    out,
                )?;
                out.push_str("    }\n");
                Ok(())
            }
            Some(terminator) => self.emit_terminator(block.id, terminator, out),
            None => Err(EmitError::new("basic block has no terminator")),
        }
    }

    // Converts an rvalue to its Rust text representation.

}
