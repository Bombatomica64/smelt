//! Control Flow emission helpers.

use super::*;
use std::cell::Cell;

thread_local! {
    /// Per-thread recursion depth for structurally recursive block emission.
    static EMIT_BLOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
}

impl FunctionEmitter<'_> {
    /// Emits a basic block's statements and terminator.
    pub(super) fn emit_block(&self, block: &BasicBlock, out: &mut String) -> Result<(), EmitError> {
        let limit = self.function.blocks.len().saturating_mul(8).max(128);
        let too_deep = EMIT_BLOCK_DEPTH.with(|depth| {
            let current = depth.get();
            if current > limit {
                true
            } else {
                depth.set(current.saturating_add(1));
                false
            }
        });
        if too_deep {
            out.push_str(
                "    panic!(\"unstructured recursive control flow is not emitted yet\");\n",
            );
            return Ok(());
        }
        let result = self.emit_block_body(block, out);
        EMIT_BLOCK_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        result
    }

    /// Emits a basic block after recursion-depth accounting has been applied.
    fn emit_block_body(&self, block: &BasicBlock, out: &mut String) -> Result<(), EmitError> {
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
            let loop_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    while {cond} {{\n"));
            self.emit_block_until_goto(then, block.id, Some(else_block), out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(loop_declared);
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
            let loop_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    while {cond} {{\n"));
            self.emit_block_until_goto(then, latch_block, Some(else_block), out)?;
            for statement in &latch.statements {
                self.emit_statement(statement, out)?;
            }
            out.push_str("    }\n");
            self.restore_declared_locals(loop_declared);
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
    pub(super) fn emit_statement(
        &self,
        statement: &Statement,
        out: &mut String,
    ) -> Result<(), EmitError> {
        match statement {
            Statement::Assign { dest, value } => {
                let local = self.local_decl(*dest)?;
                let name = self.local_name(*dest)?;
                if matches!(value, Rvalue::Closure { .. }) && !self.local_has_uses(*dest) {
                    return Ok(());
                }
                let raw_rendered_value = self.rvalue_text_for_dest(value, local.ty)?;
                let mut rendered_value =
                    if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                        && raw_rendered_value == "Default::default()"
                    {
                        self.default_value(local.ty)?
                    } else {
                        raw_rendered_value
                    };
                if let Some(Type::Function(function)) = self.mir.types.get(local.ty)
                    && matches!(self.mir.types.get(function.return_ty), Some(Type::Float))
                    && rendered_value.ends_with(".clone()")
                    && rendered_value.starts_with("_smelt_tmp_")
                {
                    let function_value = rendered_value.trim_end_matches(".clone()");
                    let params = function
                        .params
                        .iter()
                        .enumerate()
                        .map(|(index, param)| {
                            Ok(format!(
                                "arg{index}: {}",
                                self.type_text_with_impl_trait(*param, false)?
                            ))
                        })
                        .collect::<Result<Vec<_>, EmitError>>()?
                        .join(", ");
                    let call_args = (0..function.params.len())
                        .map(|index| format!("arg{index}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    rendered_value = format!(
                        "{{ let smelt_fn = {function_value}.clone(); ::std::rc::Rc::new(::std::cell::RefCell::new(move |{params}| -> f64 {{ (&mut *smelt_fn.borrow_mut())({call_args}).smelt_into_f64() }})) }}"
                    );
                }
                if name == "SmeltUnknown::Null" {
                    out.push_str(&format!("    let _ = {rendered_value};\n"));
                    return Ok(());
                }
                if self.is_local_declared(*dest)
                    && (!matches!(local.kind, LocalKind::Temp)
                        || self.mutable_locals.contains(dest)
                        || self.predeclared_locals().contains(dest))
                {
                    out.push_str(&format!("    {name} = {rendered_value};\n"));
                    return Ok(());
                }
                let mutability = if name != "_" && self.local_binding_needs_mut(*dest) {
                    "mut "
                } else {
                    ""
                };
                out.push_str(&format!(
                    "    let {mutability}{name}{} = {rendered_value};\n",
                    if matches!(self.mir.types.get(local.ty), Some(Type::Function(_))) {
                        String::new()
                    } else {
                        format!(": {}", self.type_text_with_impl_trait(local.ty, false)?)
                    }
                ));
                self.mark_local_declared(*dest);
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
        match place {
            Place::Field { base, field } => {
                let base_ty = self.local_decl(*base)?.ty;
                if let Some(Type::Dict(key, item)) = self.mir.types.get(base_ty)
                    && self.mir.types.get(*key) == Some(&Type::String)
                {
                    let rendered_value = self.rvalue_text_for_dest(value, *item)?;
                    out.push_str(&format!(
                        "    {}.insert({:?}.to_owned(), {rendered_value});\n",
                        self.local_name(*base)?,
                        self.symbol_name(*field)?
                    ));
                    return Ok(());
                }
                if matches!(
                    self.mir.types.get(base_ty),
                    Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
                ) || self.is_erased_class_type(base_ty)
                {
                    let unknown_ty = self.type_id(Type::Unknown)?;
                    let rendered_value = self.rvalue_text_for_dest(value, unknown_ty)?;
                    out.push_str(&format!(
                        "    match &mut {} {{ SmeltUnknown::Object(map) => {{ map.insert({:?}.to_owned(), {rendered_value}); }}, other => {{ let mut map = ::std::collections::HashMap::new(); map.insert({:?}.to_owned(), {rendered_value}); *other = SmeltUnknown::Object(map); }} }}\n",
                        self.local_name(*base)?,
                        self.symbol_name(*field)?,
                        self.symbol_name(*field)?
                    ));
                    return Ok(());
                }
            }
            Place::Index { base, index } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::Dict(key, item)) => {
                        let rendered_value = self.rvalue_text_for_dest(value, *item)?;
                        let key_text = self.operand_as_type_text(index, *key)?;
                        out.push_str(&format!(
                            "    {}.insert({}, {rendered_value});\n",
                            self.local_name(*base)?,
                            key_text
                        ));
                        return Ok(());
                    }
                    Some(Type::List(item)) => {
                        let rendered_value = self.rvalue_text_for_dest(value, *item)?;
                        let base_text = self.local_name(*base)?;
                        let index_text =
                            self.normalized_index_text(&format!("{base_text}.len()"), index)?;
                        out.push_str(&format!(
                            "    {{ let index = {index_text}; {base_text}[index] = {rendered_value}; }}\n"
                        ));
                        return Ok(());
                    }
                    _ => {
                        let rendered_value = self.rvalue_text(value)?;
                        out.push_str(&format!("    let _ = {rendered_value};\n"));
                        return Ok(());
                    }
                }
            }
            Place::Local(_) => {}
        }

        let rendered_value = self.rvalue_text_for_dest(value, self.place_ty(place)?)?;
        if let Place::Local(local) = place
            && (!self.is_local_declared(*local)
                || (matches!(self.local_decl(*local)?.kind, LocalKind::Temp)
                    && !self.mutable_locals.contains(local)))
        {
            let decl = self.local_decl(*local)?;
            let name = self.local_name(*local)?;
            let mutability = if self.mutable_locals.contains(local) {
                "mut "
            } else {
                ""
            };
            out.push_str(&format!(
                "    let {mutability}{name}: {} = {rendered_value};\n",
                self.type_text_with_impl_trait(decl.ty, false)?
            ));
            self.mark_local_declared(*local);
            return Ok(());
        }
        let assignment = self.assignment_place_text(place)?;
        if assignment == "SmeltUnknown::Null" {
            out.push_str(&format!("    let _ = {rendered_value};\n"));
            return Ok(());
        }
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
                let local = self.local_decl(*dest)?;
                let call_text = self.call_text_for_dest(callee, args, local.ty)?;
                let name = self.local_name(*dest)?;
                let mutability = if self.local_binding_needs_mut(*dest) {
                    "mut "
                } else {
                    ""
                };
                if matches!(
                    self.mir.types.get(local.ty),
                    Some(Type::Future(_) | Type::Function(_))
                ) {
                    out.push_str(&format!("    let {mutability}{name} = {call_text};\n"));
                } else {
                    out.push_str(&format!(
                        "    let {mutability}{name}: {} = {call_text};\n",
                        self.type_text_with_impl_trait(local.ty, false)?
                    ));
                }
                self.mark_local_declared(*dest);
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
                    } else if matches!(operand, Operand::Const(Constant::None))
                        && self.has_plain_default_value(self.function.return_ty)
                    {
                        out.push_str(&format!(
                            "    return Ok({});\n",
                            self.default_value(self.function.return_ty)?
                        ));
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
        if let Operand::Const(Constant::Bool(value)) = cond {
            return if *value {
                self.emit_block(then, out)
            } else {
                self.emit_block(else_, out)
            };
        }

        if matches!(then.terminator, Some(Terminator::Goto(target)) if target == current) {
            let loop_declared = self.declared_locals_snapshot();
            out.push_str(&format!(
                "    while {} {{\n",
                self.truthy_operand_text(cond)?
            ));
            self.emit_block_until_goto(then, current, Some(else_block), out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(loop_declared);
            return self.emit_block(else_, out);
        }

        if matches!(then.terminator, Some(Terminator::Goto(target)) if target == else_block) {
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            self.emit_block_until_goto(then, else_block, None, out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared);
            return self.emit_block(else_, out);
        }

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
                let then_text = self.rvalue_text_for_dest(then_value, local.ty)?;
                let else_text = self.rvalue_text_for_dest(else_value, local.ty)?;
                out.push_str(&format!(
                    "    let {name}: {} = if {} {{ {then_text} }} else {{ {else_text} }};\n",
                    self.type_text_with_impl_trait(local.ty, false)?,
                    self.truthy_operand_text(cond)?
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
                    self.type_text_with_impl_trait(local.ty, false)?
                ));
                self.mark_local_declared(then_dest);
                let branch_declared = self.declared_locals_snapshot();
                out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
                for statement in then_prefix {
                    self.emit_statement(statement, out)?;
                }
                let then_text = self.rvalue_text_for_dest(then_value, local.ty)?;
                out.push_str(&format!("    {name} = {then_text};\n"));
                out.push_str("    } else {\n");
                self.restore_declared_locals(branch_declared.clone());
                for statement in else_prefix {
                    self.emit_statement(statement, out)?;
                }
                let else_text = self.rvalue_text_for_dest(else_value, local.ty)?;
                out.push_str(&format!("    {name} = {else_text};\n"));
                out.push_str("    }\n");
                self.restore_declared_locals(branch_declared);
                return self.emit_block(self.block(*then_target)?, out);
            }
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            self.emit_block_until_goto(then, *then_target, None, out)?;
            out.push_str("    } else {\n");
            self.restore_declared_locals(branch_declared.clone());
            self.emit_block_until_goto(else_, *else_target, None, out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared);
            return self.emit_block(self.block(*then_target)?, out);
        }

        if let (Some(Terminator::Goto(then_target)), Some(Terminator::Goto(else_target))) =
            (&then.terminator, &else_.terminator)
            && then_target != else_target
        {
            let branch_label = format!(
                "'smelt_branch_{}_{}_{}_{}",
                current.0,
                then_block.0,
                else_block.0,
                out.len()
            );
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    {branch_label}: {{\n"));
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            for statement in &then.statements {
                self.emit_statement(statement, out)?;
            }
            out.push_str(&format!("    break {branch_label};\n"));
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared.clone());
            self.emit_block_until_goto(else_, *else_target, None, out)?;
            out.push_str("    };\n");
            self.restore_declared_locals(branch_declared);
            return self.emit_block(self.block(*else_target)?, out);
        }

        if let Some(Terminator::Goto(then_target)) = then.terminator
            && then_target.0 > current.0
            && (self.block_eventually_terminates(then_target, &mut HashSet::new())?
                || self.while_header(self.block(then_target)?)?.is_some()
                || self
                    .while_header_with_latch(self.block(then_target)?)?
                    .is_some())
        {
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            for statement in &then.statements {
                self.emit_statement(statement, out)?;
            }
            self.emit_block(self.block(then_target)?, out)?;
            out.push_str("    } else {\n");
            self.restore_declared_locals(branch_declared.clone());
            self.emit_block(else_, out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared);
            return Ok(());
        }

        if let Some(Terminator::Goto(then_target)) = then.terminator {
            let branch_label = format!(
                "'smelt_branch_{}_{}_{}_{}",
                current.0,
                then_block.0,
                else_block.0,
                out.len()
            );
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    {branch_label}: {{\n"));
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            for statement in &then.statements {
                self.emit_statement(statement, out)?;
            }
            if then_target.0 <= current.0 {
                out.push_str(&format!("    break {branch_label};\n"));
                out.push_str("    }\n");
                self.restore_declared_locals(branch_declared.clone());
                self.emit_block_until_goto(else_, then_target, None, out)?;
                out.push_str("    };\n");
                self.restore_declared_locals(branch_declared);
                return Ok(());
            }
            out.push_str(&format!("    break {branch_label};\n"));
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared.clone());
            self.emit_block(else_, out)?;
            out.push_str("    };\n");
            self.restore_declared_locals(branch_declared);
            return Ok(());
        }

        if self.block_eventually_terminates(then.id, &mut HashSet::new())?
            && self.block_eventually_terminates(else_.id, &mut HashSet::new())?
        {
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            self.emit_block(then, out)?;
            out.push_str("    } else {\n");
            self.restore_declared_locals(branch_declared.clone());
            self.emit_block(else_, out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared);
            return Ok(());
        }

        if self.block_eventually_terminates(then.id, &mut HashSet::new())? {
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            self.emit_block(then, out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared);
            if else_.id.0 <= current.0 {
                out.push_str("    loop { break; }\n");
                return Ok(());
            }
            return self.emit_block(else_, out);
        }

        if self.block_eventually_terminates(else_.id, &mut HashSet::new())? {
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!(
                "    if !({}) {{\n",
                self.truthy_operand_text(cond)?
            ));
            self.emit_block(else_, out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared);
            if then.id.0 <= current.0 {
                out.push_str("    loop { break; }\n");
                return Ok(());
            }
            return self.emit_block(then, out);
        }

        let branch_declared = self.declared_locals_snapshot();
        out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
        for statement in &then.statements {
            self.emit_statement(statement, out)?;
        }
        out.push_str("    } else {\n");
        self.restore_declared_locals(branch_declared.clone());
        for statement in &else_.statements {
            self.emit_statement(statement, out)?;
        }
        out.push_str("    }\n");
        self.restore_declared_locals(branch_declared);
        Ok(())
    }

    /// Returns whether every straight-line successor from `block_id` ends in a
    /// return, throw, or unreachable terminator before it can fall through.
    pub(super) fn block_eventually_terminates(
        &self,
        block_id: smelt_mir::BlockId,
        visiting: &mut HashSet<smelt_mir::BlockId>,
    ) -> Result<bool, EmitError> {
        if !visiting.insert(block_id) {
            return Ok(false);
        }

        let block = self.block(block_id)?;
        let result = match &block.terminator {
            Some(Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable) => true,
            Some(Terminator::Goto(target)) => {
                self.block_eventually_terminates(*target, visiting)?
            }
            Some(Terminator::Call { target, .. }) => {
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
        Ok(result)
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
            Some(Terminator::Goto(target)) => {
                self.emit_block_until_goto(self.block(*target)?, stop, break_target, out)
            }
            Some(Terminator::Switch {
                cond,
                then_block,
                else_block,
            }) if break_target.is_some() => {
                out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
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
        self.emit_loop_branch_inner(
            block,
            continue_target,
            break_target,
            out,
            &mut HashSet::new(),
        )
    }

    /// Emits a branch inside a loop while guarding against join-block cycles.
    fn emit_loop_branch_inner(
        &self,
        block: &BasicBlock,
        continue_target: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
        visited: &mut HashSet<smelt_mir::BlockId>,
    ) -> Result<(), EmitError> {
        if !visited.insert(block.id) {
            out.push_str("    continue;\n");
            return Ok(());
        }
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
            Some(Terminator::Goto(target)) => self.emit_loop_branch_inner(
                self.block(*target)?,
                continue_target,
                break_target,
                out,
                visited,
            ),
            Some(Terminator::Switch {
                cond,
                then_block,
                else_block,
            }) => {
                out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
                self.emit_loop_branch_inner(
                    self.block(*then_block)?,
                    continue_target,
                    break_target,
                    out,
                    visited,
                )?;
                out.push_str("    } else {\n");
                self.emit_loop_branch_inner(
                    self.block(*else_block)?,
                    continue_target,
                    break_target,
                    out,
                    visited,
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
