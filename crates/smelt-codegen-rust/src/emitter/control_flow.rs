//! Control Flow emission helpers.

use super::*;
use std::cell::Cell;

thread_local! {
    /// Per-thread recursion depth for structurally recursive block emission.
    static EMIT_BLOCK_DEPTH: Cell<usize> = const { Cell::new(0) };
    /// Per-thread recursion depth for nested block-until emission.
    static EMIT_UNTIL_DEPTH: Cell<usize> = const { Cell::new(0) };
}

impl FunctionEmitter<'_> {
    /// Emits a basic block's statements and terminator.
    pub(super) fn emit_block(&self, block: &BasicBlock, out: &mut String) -> Result<(), EmitError> {
        let limit = self.function.blocks.len().saturating_mul(2).clamp(16, 64);
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
            out.push_str("    // Smelt could not structurally emit this recursive control-flow region yet.\n");
            out.push_str(&self.default_return_statement()?);
            return Ok(());
        }
        let result = self.emit_block_body(block, out);
        EMIT_BLOCK_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        result
    }

    /// Emits a basic block after recursion-depth accounting has been applied.
    fn emit_block_body(&self, block: &BasicBlock, out: &mut String) -> Result<(), EmitError> {
        if let Some((cond, repeat_block, exit_block, repeat_when_true)) =
            self.do_while_body(block)?
        {
            let repeated = self.block(repeat_block)?;
            let exit = self.block(exit_block)?;
            let loop_declared = self.declared_locals_snapshot();
            out.push_str("    loop {\n");
            for statement in &block.statements {
                self.emit_statement_for_block(block, statement, out)?;
            }
            if repeat_when_true {
                out.push_str(&format!("    if !({cond}) {{ break; }}\n"));
            } else {
                out.push_str(&format!("    if {cond} {{ break; }}\n"));
            }
            self.emit_block_until_goto(repeated, block.id, Some(exit_block), out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(loop_declared);
            return self.emit_block(exit, out);
        }

        if let Some((cond, then_block, else_block, cond_statement_idx)) =
            self.while_header(block)?
        {
            let has_header_work = block
                .statements
                .iter()
                .enumerate()
                .any(|(idx, _)| idx != cond_statement_idx);
            let then = self.block(then_block)?;
            let else_ = self.block(else_block)?;
            let loop_declared = self.declared_locals_snapshot();
            if has_header_work {
                out.push_str("    loop {\n");
                for statement in &block.statements {
                    self.emit_statement_for_block(block, statement, out)?;
                }
                out.push_str(&format!(
                    "    if !({}) {{ break; }}\n",
                    self.truthy_operand_text(&Operand::Copy(Place::Local(
                        self.switch_cond_local(block)?
                    )))?
                ));
            } else {
                out.push_str(&format!("    while {cond} {{\n"));
            }
            self.emit_block_until_goto(then, block.id, Some(else_block), out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(loop_declared);
            return self.emit_block(else_, out);
        }

        if let Some((cond, then_block, latch_block, else_block, cond_statement_idx)) =
            self.while_header_with_latch(block)?
        {
            let has_header_work = block
                .statements
                .iter()
                .enumerate()
                .any(|(idx, _)| idx != cond_statement_idx);
            let then = self.block(then_block)?;
            let latch = self.block(latch_block)?;
            let else_ = self.block(else_block)?;
            let loop_declared = self.declared_locals_snapshot();
            if has_header_work {
                out.push_str("    loop {\n");
                for statement in &block.statements {
                    self.emit_statement_for_block(block, statement, out)?;
                }
                out.push_str(&format!(
                    "    if !({}) {{ break; }}\n",
                    self.truthy_operand_text(&Operand::Copy(Place::Local(
                        self.switch_cond_local(block)?
                    )))?
                ));
            } else {
                out.push_str(&format!("    while {cond} {{\n"));
            }
            self.emit_block_until_goto(then, latch_block, Some(else_block), out)?;
            for statement in &latch.statements {
                self.emit_statement(statement, out)?;
            }
            out.push_str("    }\n");
            self.restore_declared_locals(loop_declared);
            return self.emit_block(else_, out);
        }

        for statement in &block.statements {
            self.emit_statement_for_block(block, statement, out)?;
        }

        let Some(terminator) = &block.terminator else {
            return Err(EmitError::new("basic block has no terminator"));
        };
        self.emit_terminator(block.id, terminator, out)
    }

    /// Emits a statement unless it is a dead narrowing cast before an unknown return.
    fn emit_statement_for_block(
        &self,
        block: &BasicBlock,
        statement: &Statement,
        out: &mut String,
    ) -> Result<(), EmitError> {
        if self.statement_is_dead_unknown_return_cast(block, statement)? {
            return Ok(());
        }
        self.emit_statement(statement, out)
    }

    /// Returns whether a statement only narrows an unknown value immediately returned as unknown.
    fn statement_is_dead_unknown_return_cast(
        &self,
        block: &BasicBlock,
        statement: &Statement,
    ) -> Result<bool, EmitError> {
        if self.mir.types.get(self.function.return_ty) != Some(&Type::Unknown) {
            return Ok(false);
        }
        let Statement::Assign {
            dest,
            value: Rvalue::UnknownCast { value, .. },
        } = statement
        else {
            return Ok(false);
        };
        if self.mir.types.get(self.operand_ty(value)?) != Some(&Type::Unknown) {
            return Ok(false);
        }
        Ok(matches!(
            &block.terminator,
            Some(Terminator::Return(
                Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))
            )) if local == dest
        ))
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
                // A `Function`-typed `ClosureCall` result whose only consumer
                // erases it back to `SmeltUnknown` is re-rendered at the erase
                // site; the typed-callback binding would be a dead store that
                // re-evaluates the call. Suppress it (mirrors the call-terminator
                // path in `emit_call_terminator_statement`).
                if matches!(value, Rvalue::ClosureCall { .. })
                    && self.function_call_result_dead_when_erased(*dest)?
                {
                    self.mark_local_declared(*dest);
                    return Ok(());
                }
                let raw_rendered_value = self.rvalue_text_for_dest(value, local.ty)?;
                let rendered_value =
                    if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                        && raw_rendered_value == "Default::default()"
                    {
                        self.default_value(local.ty)?
                    } else {
                        raw_rendered_value
                    };
                if name == "SmeltUnknown::Null" {
                    out.push_str(&format!("    let _ = {rendered_value};\n"));
                    return Ok(());
                }
                if self.is_local_declared(*dest)
                    && (!matches!(local.kind, LocalKind::Temp)
                        || self.mutable_locals.contains(dest)
                        || self.predeclared_locals.contains(dest))
                {
                    let assignment = self.assignment_place_text(&Place::Local(*dest))?;
                    out.push_str(&format!("    {assignment} = {rendered_value};\n"));
                    return Ok(());
                }
                let mutability = if name != "_" && self.local_binding_needs_mut(*dest) {
                    "mut "
                } else {
                    ""
                };
                let annotation = if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                    && (self.local_binding_needs_mut(*dest)
                        || self.predeclared_locals.contains(dest)
                        || self.mutable_locals.contains(dest))
                {
                    format!(": {}", self.type_text_with_impl_trait(local.ty, false)?)
                } else if matches!(self.mir.types.get(local.ty), Some(Type::Function(_))) {
                    String::new()
                } else if matches!(self.function.origin, HirOrigin::ClassConstructor { .. })
                    && name == "this"
                {
                    ": Self".to_owned()
                } else {
                    format!(": {}", self.type_text_with_impl_trait(local.ty, false)?)
                };
                if self.local_uses_shared_capture_storage(*dest) {
                    out.push_str(&format!(
                        "    let smelt_capture_{name} = ::std::rc::Rc::new(::std::cell::RefCell::new({rendered_value}));\n",
                    ));
                } else {
                    out.push_str(&format!(
                        "    let {mutability}{name}{annotation} = {rendered_value};\n",
                    ));
                }
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
                if let Some(statement) =
                    self.descriptor_setter_statement(*base, *field, value)?
                {
                    out.push_str("    ");
                    out.push_str(&statement);
                    out.push('\n');
                    return Ok(());
                }
                if let Some(Type::Dict(key, item)) = self.mir.types.get(base_ty)
                    && self.mir.types.get(*key) == Some(&Type::String)
                {
                    let rendered_value = self.rvalue_text_for_dest(value, *item)?;
                    out.push_str(&format!(
                        "    {}.insert({:?}.to_owned(), {rendered_value});\n",
                        self.local_mut_value_text(*base)?,
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
                        "    match &mut {} {{ SmeltUnknown::Object(map) => {{ map.insert({:?}.to_owned(), {rendered_value}); }}, other => {{ let mut map = ::std::collections::HashMap::new(); map.insert({:?}.to_owned(), {rendered_value}); *other = SmeltUnknown::Object(SmeltObject::new(map)); }} }}\n",
                        self.local_mut_value_text(*base)?,
                        self.symbol_name(*field)?,
                        self.symbol_name(*field)?
                    ));
                    return Ok(());
                }
                // A dotted write to an UNDECLARED member on an index-signature
                // class (`bag.name = value`) inserts into the runtime store
                // (issue #84), mirroring the computed `bag[k] = value` write, so
                // it round-trips to a later read. Declared fields keep their
                // concrete struct-field assignment via the tail path below.
                if let Some((_key_ty, value_ty)) = self.class_index_store_types(base_ty)
                    && !self.class_has_named_field(base_ty, *field)
                {
                    let rendered_value = self.rvalue_text_for_dest(value, value_ty)?;
                    out.push_str(&format!(
                        "    {}.{}.insert({:?}.to_owned(), {rendered_value});\n",
                        self.local_mut_value_text(*base)?,
                        smelt_hir::CLASS_INDEX_STORE_FIELD,
                        self.symbol_source_name(*field)?,
                    ));
                    return Ok(());
                }
            }
            Place::Index { base, index } => {
                let base_ty = self.local_decl(*base)?.ty;
                match self.mir.types.get(base_ty) {
                    Some(Type::Dict(key, item)) => {
                        let rendered_value = self.rvalue_text_for_dest(value, *item)?;
                        let key_text = if self.mir.types.get(*key) == Some(&Type::String) {
                            let index_ty = self.operand_ty(index)?;
                            if index_ty == *key {
                                self.value_at_type(index, *key)?
                            } else {
                                let index_text = self.operand_text(index)?;
                                self.property_key_to_string_text(&index_text, index_ty)?
                            }
                        } else {
                            self.value_at_type(index, *key)?
                        };
                        out.push_str(&format!(
                            "    {}.insert({}, {rendered_value});\n",
                            self.local_mut_value_text(*base)?,
                            key_text
                        ));
                        return Ok(());
                    }
                    Some(Type::List(item)) => {
                        let rendered_value = self.rvalue_text_for_dest(value, *item)?;
                        let base_text = self.local_mut_value_text(*base)?;
                        let index_text =
                            self.normalized_index_text(&format!("{base_text}.len()"), index)?;
                        let default_value = self.default_value(*item)?;
                        out.push_str(&format!(
                            "    {{ let smelt_assign_index = {index_text}; if smelt_assign_index >= {base_text}.len() {{ {base_text}.resize(smelt_assign_index.saturating_add(1), {default_value}); }} {base_text}[smelt_assign_index] = {rendered_value}; }}\n"
                        ));
                        return Ok(());
                    }
                    Some(Type::Tuple(items)) => {
                        let tuple_index = self.tuple_index(index, items.len())?;
                        let item_ty = items
                            .get(tuple_index)
                            .copied()
                            .ok_or_else(|| EmitError::new("tuple index is out of bounds"))?;
                        let rendered_value = self.rvalue_text_for_dest(value, item_ty)?;
                        out.push_str(&format!(
                            "    {}.{tuple_index} = {rendered_value};\n",
                            self.local_mut_value_text(*base)?
                        ));
                        return Ok(());
                    }
                    Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. }) => {
                        let rendered_value =
                            self.rvalue_text_for_dest(value, self.type_id(Type::Unknown)?)?;
                        let index_ty = self.operand_ty(index)?;
                        let index_text = self.operand_text(index)?;
                        let key_text = self.property_key_to_string_text(&index_text, index_ty)?;
                        out.push_str(&format!(
                            "    {{ let smelt_key = {key_text}; let smelt_value = {rendered_value}; match &mut {} {{ SmeltUnknown::Object(map) => {{ map.insert(smelt_key, smelt_value); }}, other => {{ let mut map = ::std::collections::HashMap::new(); map.insert(smelt_key, smelt_value); *other = SmeltUnknown::Object(SmeltObject::new(map)); }} }} }}\n",
                            self.local_mut_value_text(*base)?
                        ));
                        return Ok(());
                    }
                    // A class with an index signature backs keyed writes with a
                    // real store field (issue #84): `bag[key] = value` inserts
                    // into `bag.__smelt_index_store` so the write round-trips to a
                    // later `bag[key]` read. The value is rendered at the store's
                    // declared value type, keeping `T` concrete.
                    Some(Type::Class { .. })
                        if self.class_index_store_types(base_ty).is_some() =>
                    {
                        let (key_ty, value_ty) = self
                            .class_index_store_types(base_ty)
                            .ok_or_else(|| EmitError::new("class index store types missing"))?;
                        let rendered_value = self.rvalue_text_for_dest(value, value_ty)?;
                        let key_text = if self.mir.types.get(key_ty) == Some(&Type::String) {
                            let index_ty = self.operand_ty(index)?;
                            if index_ty == key_ty {
                                self.value_at_type(index, key_ty)?
                            } else {
                                let index_text = self.operand_text(index)?;
                                self.property_key_to_string_text(&index_text, index_ty)?
                            }
                        } else {
                            self.value_at_type(index, key_ty)?
                        };
                        out.push_str(&format!(
                            "    {}.{}.insert({key_text}, {rendered_value});\n",
                            self.local_mut_value_text(*base)?,
                            smelt_hir::CLASS_INDEX_STORE_FIELD,
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
            if self.local_uses_shared_capture_storage(*local) {
                out.push_str(&format!(
                    "    let smelt_capture_{name}: ::std::rc::Rc<::std::cell::RefCell<{}>> = ::std::rc::Rc::new(::std::cell::RefCell::new({rendered_value}));\n",
                    self.type_text_with_impl_trait(decl.ty, false)?
                ));
            } else {
                out.push_str(&format!(
                    "    let {mutability}{name}: {} = {rendered_value};\n",
                    self.type_text_with_impl_trait(decl.ty, false)?
                ));
            }
            self.mark_local_declared(*local);
            return Ok(());
        }
        let assignment = self.assignment_place_text(place)?;
        if assignment == "SmeltUnknown::Null" {
            out.push_str(&format!("    let _ = {rendered_value};\n"));
            return Ok(());
        }
        if assignment.starts_with("(*smelt_capture_") && rendered_value.contains(&assignment) {
            out.push_str(&format!(
                "    {{ let smelt_next_value = {rendered_value}; {assignment} = smelt_next_value; }}\n"
            ));
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
            Terminator::Goto(target) => {
                if target.0 <= current.0 {
                    if self.block_eventually_terminates(*target, &mut HashSet::new())? {
                        return self.emit_block(self.block(*target)?, out);
                    }
                    return self.emit_fallthrough_return(out);
                }
                self.emit_block(self.block(*target)?, out)
            }
            Terminator::Call {
                callee,
                args,
                dest,
                target,
                unwind,
            } => {
                if let Some(handler) = unwind {
                    return self.emit_throwing_call_terminator(
                        callee, args, *dest, *target, *handler, out,
                    );
                }
                self.emit_call_terminator_statement(callee, args, *dest, out)?;
                self.emit_block(self.block(*target)?, out)
            }
            Terminator::Await {
                future,
                dest,
                target,
                unwind,
            } => {
                if let Some(handler) = unwind {
                    return self
                        .emit_throwing_await_terminator(future, *dest, *target, *handler, out);
                }
                let local = self.local_decl(*dest)?;
                let name = self.local_name(*dest)?;
                let mutability = if self.local_binding_needs_mut(*dest) {
                    "mut "
                } else {
                    ""
                };
                let value = format!("{}.await?", self.await_operand_text(future)?);
                if matches!(
                    self.mir.types.get(local.ty),
                    Some(Type::Future(_) | Type::Function(_))
                ) {
                    out.push_str(&format!("    let {mutability}{name} = {value};\n"));
                } else {
                    out.push_str(&format!(
                        "    let {mutability}{name}: {} = {value};\n",
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
                            self.value_at_type(operand, self.function.return_ty)?
                        ));
                    }
                } else if self.function.return_ty == self.none_ty {
                    if !matches!(operand, Operand::Const(Constant::None))
                        && self.operand_ty(operand)? != self.none_ty
                    {
                        out.push_str(&format!("    {};\n", self.operand_text(operand)?));
                    }
                    if self.function.is_async {
                        out.push_str("    return Ok(());\n");
                    } else {
                        out.push_str("    return;\n");
                    }
                } else if self.mir.types.get(self.function.return_ty) == Some(&Type::Unknown)
                    && let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) =
                        operand
                    && let Some(source) = self.single_assignment_unknown_cast_source(*local)
                    && self.mir.types.get(self.operand_ty(source)?) == Some(&Type::Unknown)
                {
                    out.push_str(&format!(
                        "    return {};\n",
                        self.value_at_type(source, self.function.return_ty)?
                    ));
                } else {
                    out.push_str(&format!(
                        "    return {};\n",
                        self.value_at_type(operand, self.function.return_ty)?
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

    /// Emits the assignment part of a call terminator without following it.
    fn emit_call_terminator_statement(
        &self,
        callee: &Callee,
        args: &[Operand],
        dest: LocalId,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let local = self.local_decl(dest)?;
        let name = self.local_name(dest)?;
        // A `Function`-typed call result whose only consumer erases it back to
        // `SmeltUnknown` re-renders the call at the erase site (see
        // `coercion::erased_call_assignment_text`). Emitting the typed-callback
        // binding here would be a dead store that also evaluates the call a
        // second time, double-moving its arguments. Suppress it so the single
        // re-inlined erase is the lone evaluation.
        if self.function_call_result_dead_when_erased(dest)? {
            self.mark_local_declared(dest);
            return Ok(());
        }
        if !self.local_has_uses(dest) && self.mir.types.get(local.ty) == Some(&Type::None) {
            let mut call_text = self.call_text(callee, args)?;
            if args.is_empty() && call_text.ends_with("(Vec::new())") {
                call_text = format!("{}()", call_text.trim_end_matches("(Vec::new())"));
            } else if call_text == "(fn_)(Vec::new())" {
                "(fn_)()".clone_into(&mut call_text);
            }
            out.push_str(&format!("    let _ = {call_text};\n"));
            self.mark_local_declared(dest);
            return Ok(());
        }
        let call_text = self.call_text_for_dest(callee, args, local.ty)?;
        let mutability = if self.local_binding_needs_mut(dest) {
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
        self.mark_local_declared(dest);
        Ok(())
    }

    /// Emits a throwing call with explicit normal and exception continuations.
    fn emit_throwing_call_terminator(
        &self,
        callee: &Callee,
        args: &[Operand],
        dest: LocalId,
        target: smelt_mir::BlockId,
        handler: smelt_mir::ExceptionHandler,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let local = self.local_decl(dest)?;
        let call_text = self.call_text(callee, args)?;
        let Some(raw_call) = call_text.strip_suffix('?') else {
            out.push_str(&format!(
                "    match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {call_text})) {{\n"
            ));
            out.push_str("        Ok(__smelt_value) => {\n");
            let name = self.local_name(dest)?;
            let mutability = if self.local_binding_needs_mut(dest) {
                "mut "
            } else {
                ""
            };
            if matches!(
                self.mir.types.get(local.ty),
                Some(Type::Future(_) | Type::Function(_))
            ) {
                out.push_str(&format!(
                    "            let {mutability}{name} = __smelt_value;\n"
                ));
            } else {
                out.push_str(&format!(
                    "            let {mutability}{name}: {} = __smelt_value;\n",
                    self.type_text_with_impl_trait(local.ty, false)?
                ));
            }
            self.mark_local_declared(dest);
            self.emit_block(self.block(target)?, out)?;
            out.push_str("        }\n");
            out.push_str("        Err(__smelt_panic) => {\n");
            out.push_str("            let __smelt_error = if let Some(message) = __smelt_panic.downcast_ref::<String>() { message.clone() } else if let Some(message) = __smelt_panic.downcast_ref::<&'static str>() { (*message).to_owned() } else { \"JavaScript exception\".to_owned() };\n");
            if let Some(exception_local) = handler.exception_local {
                let exception_name = self.local_name(exception_local)?;
                let exception_decl = self.local_decl(exception_local)?;
                let value = match self.mir.types.get(exception_decl.ty) {
                    Some(Type::String) => "__smelt_error".to_owned(),
                    Some(Type::Unknown) => "SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([(\"__smelt_error\".to_owned(), SmeltUnknown::Bool(true)), (\"message\".to_owned(), SmeltUnknown::String(__smelt_error))])))".to_owned(),
                    _ => self.default_value(exception_decl.ty)?,
                };
                out.push_str(&format!("            let {exception_name} = {value};\n"));
                self.mark_local_declared(exception_local);
            }
            self.emit_block(self.block(handler.catch_block)?, out)?;
            out.push_str("        }\n");
            out.push_str("    }\n");
            return Ok(());
        };
        let source_ty = self.call_source_ty(callee)?;
        out.push_str(&format!(
            "    match ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {raw_call})) {{\n"
        ));
        out.push_str("        Ok(Ok(__smelt_value)) => {\n");
        let value_text = self.value_at_type_text("__smelt_value", source_ty, local.ty)?;
        let name = self.local_name(dest)?;
        let mutability = if self.local_binding_needs_mut(dest) {
            "mut "
        } else {
            ""
        };
        if matches!(
            self.mir.types.get(local.ty),
            Some(Type::Future(_) | Type::Function(_))
        ) {
            out.push_str(&format!(
                "            let {mutability}{name} = {value_text};\n"
            ));
        } else {
            out.push_str(&format!(
                "            let {mutability}{name}: {} = {value_text};\n",
                self.type_text_with_impl_trait(local.ty, false)?
            ));
        }
        self.mark_local_declared(dest);
        self.emit_block(self.block(target)?, out)?;
        out.push_str("        }\n");
        out.push_str("        Ok(Err(__smelt_error)) => {\n");
        if let Some(exception_local) = handler.exception_local {
            let exception_name = self.local_name(exception_local)?;
            let exception_decl = self.local_decl(exception_local)?;
            let value = match self.mir.types.get(exception_decl.ty) {
                Some(Type::String) => "__smelt_error.to_string()".to_owned(),
                Some(Type::Unknown) => "SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([(\"__smelt_error\".to_owned(), SmeltUnknown::Bool(true)), (\"message\".to_owned(), SmeltUnknown::String(__smelt_error.to_string()))])))".to_owned(),
                _ => self.default_value(exception_decl.ty)?,
            };
            out.push_str(&format!("            let {exception_name} = {value};\n"));
            self.mark_local_declared(exception_local);
        }
        self.emit_block(self.block(handler.catch_block)?, out)?;
        out.push_str("        }\n");
        out.push_str("        Err(__smelt_panic) => {\n");
        out.push_str("            let __smelt_error = if let Some(message) = __smelt_panic.downcast_ref::<String>() { message.clone() } else if let Some(message) = __smelt_panic.downcast_ref::<&'static str>() { (*message).to_owned() } else { \"JavaScript exception\".to_owned() };\n");
        if let Some(exception_local) = handler.exception_local {
            let exception_name = self.local_name(exception_local)?;
            let exception_decl = self.local_decl(exception_local)?;
            let value = match self.mir.types.get(exception_decl.ty) {
                Some(Type::String) => "__smelt_error".to_owned(),
                Some(Type::Unknown) => "SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([(\"__smelt_error\".to_owned(), SmeltUnknown::Bool(true)), (\"message\".to_owned(), SmeltUnknown::String(__smelt_error))])))".to_owned(),
                _ => self.default_value(exception_decl.ty)?,
            };
            out.push_str(&format!("            let {exception_name} = {value};\n"));
            self.mark_local_declared(exception_local);
        }
        self.emit_block(self.block(handler.catch_block)?, out)?;
        out.push_str("        }\n");
        out.push_str("    }\n");
        Ok(())
    }

    /// Emits an awaited rejecting future with explicit normal and exception continuations.
    fn emit_throwing_await_terminator(
        &self,
        future: &Operand,
        dest: LocalId,
        target: smelt_mir::BlockId,
        handler: smelt_mir::ExceptionHandler,
        out: &mut String,
    ) -> Result<(), EmitError> {
        let local = self.local_decl(dest)?;
        let future_text = self.await_operand_text(future)?;
        out.push_str(&format!("    match {future_text}.await {{\n"));
        out.push_str("        Ok(__smelt_value) => {\n");
        let name = self.local_name(dest)?;
        let mutability = if self.local_binding_needs_mut(dest) {
            "mut "
        } else {
            ""
        };
        if matches!(
            self.mir.types.get(local.ty),
            Some(Type::Future(_) | Type::Function(_))
        ) {
            out.push_str(&format!(
                "            let {mutability}{name} = __smelt_value;\n"
            ));
        } else {
            out.push_str(&format!(
                "            let {mutability}{name}: {} = __smelt_value;\n",
                self.type_text_with_impl_trait(local.ty, false)?
            ));
        }
        self.mark_local_declared(dest);
        self.emit_block(self.block(target)?, out)?;
        out.push_str("        }\n");
        out.push_str("        Err(__smelt_error) => {\n");
        out.push_str("            let __smelt_error = __smelt_error.to_string();\n");
        if let Some(exception_local) = handler.exception_local {
            let exception_name = self.local_name(exception_local)?;
            let exception_decl = self.local_decl(exception_local)?;
            let value = match self.mir.types.get(exception_decl.ty) {
                Some(Type::String) => "__smelt_error".to_owned(),
                Some(Type::Unknown) => "SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([(\"__smelt_error\".to_owned(), SmeltUnknown::Bool(true)), (\"message\".to_owned(), SmeltUnknown::String(__smelt_error))])))".to_owned(),
                _ => self.default_value(exception_decl.ty)?,
            };
            out.push_str(&format!("            let {exception_name} = {value};\n"));
            self.mark_local_declared(exception_local);
        }
        self.emit_block(self.block(handler.catch_block)?, out)?;
        out.push_str("        }\n");
        out.push_str("    }\n");
        Ok(())
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

        // A short-circuit branch may call a function before reaching the same
        // join as its direct sibling. Keep emitting the common continuation
        // instead of treating the direct edge as an unstructured escape.
        if let Some(Terminator::Goto(then_target)) = then.terminator
            && self.branch_join_target(else_.id, &mut HashSet::new())? == Some(then_target)
        {
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            self.emit_block_until_goto(then, then_target, None, out)?;
            out.push_str("    } else {\n");
            self.restore_declared_locals(branch_declared.clone());
            self.emit_block_until_goto(else_, then_target, None, out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared);
            return self.emit_block(self.block(then_target)?, out);
        }

        if let Some(Terminator::Goto(else_target)) = else_.terminator
            && self.branch_join_target(then.id, &mut HashSet::new())? == Some(else_target)
        {
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            self.emit_block_until_goto(then, else_target, None, out)?;
            out.push_str("    } else {\n");
            self.restore_declared_locals(branch_declared.clone());
            self.emit_block_until_goto(else_, else_target, None, out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared);
            return self.emit_block(self.block(else_target)?, out);
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

        if let (Some(then_join), Some(else_join)) = (
            self.branch_join_target(then.id, &mut HashSet::new())?,
            self.branch_join_target(else_.id, &mut HashSet::new())?,
        ) && then_join == else_join
        {
            let branch_declared = self.declared_locals_snapshot();
            out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
            self.emit_block_until_goto(then, then_join, None, out)?;
            out.push_str("    } else {\n");
            self.restore_declared_locals(branch_declared.clone());
            self.emit_block_until_goto(else_, then_join, None, out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(branch_declared);
            return self.emit_block(self.block(then_join)?, out);
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

    /// Finds a straight-line join block reached by a branch.
    fn branch_join_target(
        &self,
        block_id: smelt_mir::BlockId,
        visited: &mut HashSet<smelt_mir::BlockId>,
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
    pub(super) fn block_eventually_terminates(
        &self,
        block_id: smelt_mir::BlockId,
        visiting: &mut HashSet<smelt_mir::BlockId>,
    ) -> Result<bool, EmitError> {
        if let Some(result) = self.termination_cache.borrow().get(&block_id).copied() {
            return Ok(result);
        }
        if !visiting.insert(block_id) {
            return Ok(false);
        }

        let block = self.block(block_id)?;
        let result = match &block.terminator {
            Some(Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable) => true,
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

    /// Checks if a block starts a while loop and returns loop details.
    /// Checks if a block starts a while loop and returns loop details.
    pub(super) fn while_header(
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
        if !self.block_reaches_target(*then_block, block.id, &mut HashSet::new()) {
            return Ok(None);
        }
        if !self.block_exits_to_loop(
            self.block(*then_block)?,
            block.id,
            *else_block,
            &mut HashSet::new(),
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
        Ok(Some((
            self.rvalue_text(value)?,
            *then_block,
            *else_block,
            idx,
        )))
    }

    /// Recognizes a loop whose one switch branch returns to the current block.
    ///
    /// TypeScript `do...while` commonly routes its back edge through a small
    /// latch block. Treating that edge as an ordinary conditional branch
    /// recursively repeats emitted source and eventually substitutes a default
    /// return value. The same `loop { ...; if exit { break; } }` form is valid
    /// for a pre-test header when it has the same one-sided cycle shape.
    fn do_while_body(
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
        let then_repeats = self.block_reaches_target(*then_block, block.id, &mut HashSet::new());
        let else_repeats = self.block_reaches_target(*else_block, block.id, &mut HashSet::new());
        if then_repeats == else_repeats {
            return Ok(None);
        }
        if then_repeats
            && self.block_exits_to_loop(
                self.block(*then_block)?,
                block.id,
                *else_block,
                &mut HashSet::new(),
            )?
        {
            return Ok(Some((
                self.truthy_operand_text(cond)?,
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
                &mut HashSet::new(),
            )?
        {
            return Ok(Some((
                self.truthy_operand_text(cond)?,
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
    /// Checks if a block eventually exits to a loop target.
    pub(super) fn block_exits_to_loop(
        &self,
        block: &BasicBlock,
        continue_target: smelt_mir::BlockId,
        break_target: smelt_mir::BlockId,
        visited: &mut HashSet<smelt_mir::BlockId>,
    ) -> Result<bool, EmitError> {
        let cache_key = (block.id, continue_target, break_target);
        if let Some(result) = self.loop_exit_cache.borrow().get(&cache_key).copied() {
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
            Some(Terminator::Call { target, .. }) => self.block_exits_to_loop(
                self.block(*target)?,
                continue_target,
                break_target,
                visited,
            ),
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
            Some(Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable) => {
                Ok(true)
            }
            _ => Ok(false),
        }?;
        self.loop_exit_cache.borrow_mut().insert(cache_key, result);
        Ok(result)
    }

    /// Returns whether a control-flow path can reach `target`.
    fn block_reaches_target(
        &self,
        block_id: smelt_mir::BlockId,
        target: smelt_mir::BlockId,
        visited: &mut HashSet<smelt_mir::BlockId>,
    ) -> bool {
        if block_id == target {
            return true;
        }
        if !visited.insert(block_id) {
            return false;
        }
        let Some(block) = self
            .function
            .blocks
            .iter()
            .find(|block| block.id == block_id)
        else {
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
    fn block_reaches_target_avoiding(
        &self,
        block_id: smelt_mir::BlockId,
        target: smelt_mir::BlockId,
        avoid: &[smelt_mir::BlockId],
        visited: &mut HashSet<smelt_mir::BlockId>,
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
        let Some(block) = self
            .function
            .blocks
            .iter()
            .find(|block| block.id == block_id)
        else {
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
        let limit = self.function.blocks.len().saturating_mul(8).max(128);
        let too_deep = EMIT_UNTIL_DEPTH.with(|depth| {
            let current = depth.get();
            if current > limit {
                true
            } else {
                depth.set(current.saturating_add(1));
                false
            }
        });
        if too_deep {
            out.push_str("    // Smelt could not structurally emit this nested loop edge yet.\n");
            out.push_str("    break;\n");
            return Ok(());
        }
        let result =
            self.emit_block_until_goto_inner(block, stop, break_target, out, &mut HashSet::new());
        EMIT_UNTIL_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
        result
    }

    /// Emits a block until a target while avoiding repeated unstructured cycles.
    fn emit_block_until_goto_inner(
        &self,
        block: &BasicBlock,
        stop: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
        visited: &mut HashSet<smelt_mir::BlockId>,
    ) -> Result<(), EmitError> {
        if !visited.insert(block.id) {
            out.push_str("    continue;\n");
            return Ok(());
        }
        if self.emit_nested_loop_until_goto(block, stop, break_target, out, visited)? {
            return Ok(());
        }
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
                let target_block = self.block(*target)?;
                self.emit_block_until_goto_inner(target_block, stop, break_target, out, visited)
            }
            Some(Terminator::Call {
                callee,
                args,
                dest,
                target,
                unwind: _,
            }) => {
                self.emit_call_terminator_statement(callee, args, *dest, out)?;
                self.emit_block_until_goto_inner(
                    self.block(*target)?,
                    stop,
                    break_target,
                    out,
                    visited,
                )
            }
            Some(terminator @ Terminator::Await { .. }) => {
                self.emit_terminator(block.id, terminator, out)
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
        if self.emit_nested_loop_until_goto(block, continue_target, break_target, out, visited)? {
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
                unwind: _,
            }) => {
                self.emit_call_terminator_statement(callee, args, *dest, out)?;
                self.emit_loop_branch_inner(
                    self.block(*target)?,
                    continue_target,
                    break_target,
                    out,
                    visited,
                )
            }
            Some(terminator @ Terminator::Await { .. }) => {
                self.emit_terminator(block.id, terminator, out)
            }
            Some(Terminator::Switch {
                cond,
                then_block,
                else_block,
            }) => {
                out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
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
                let mut else_visited = visited.clone();
                self.emit_loop_branch_inner(
                    self.block(*else_block)?,
                    continue_target,
                    break_target,
                    out,
                    &mut else_visited,
                )?;
                out.push_str("    }\n");
                Ok(())
            }
            Some(terminator) => self.emit_terminator(block.id, terminator, out),
            None => Err(EmitError::new("basic block has no terminator")),
        }
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
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
        visited: &mut HashSet<smelt_mir::BlockId>,
    ) -> Result<bool, EmitError> {
        let already_emitting_nested_region = EMIT_UNTIL_DEPTH.with(|depth| depth.get() > 1);
        if already_emitting_nested_region {
            return Ok(false);
        }
        if let Some((cond, then_block, else_block, cond_statement_idx)) =
            self.while_header(block)?
        {
            if self.block_reaches_target_avoiding(
                then_block,
                stop,
                &[block.id, else_block],
                &mut HashSet::new(),
            ) {
                return Ok(false);
            }
            let has_header_work = block
                .statements
                .iter()
                .enumerate()
                .any(|(idx, _)| idx != cond_statement_idx);
            let then = self.block(then_block)?;
            let loop_declared = self.declared_locals_snapshot();
            if has_header_work {
                out.push_str("    loop {\n");
                for statement in &block.statements {
                    self.emit_statement_for_block(block, statement, out)?;
                }
                out.push_str(&format!(
                    "    if !({}) {{ break; }}\n",
                    self.truthy_operand_text(&Operand::Copy(Place::Local(
                        self.switch_cond_local(block)?
                    )))?
                ));
            } else {
                out.push_str(&format!("    while {cond} {{\n"));
            }
            self.emit_block_until_goto(then, block.id, Some(else_block), out)?;
            out.push_str("    }\n");
            self.restore_declared_locals(loop_declared);
            self.emit_after_nested_loop(else_block, stop, break_target, out, visited)?;
            return Ok(true);
        }

        if let Some((cond, then_block, latch_block, else_block, cond_statement_idx)) =
            self.while_header_with_latch(block)?
        {
            if self.block_reaches_target_avoiding(
                then_block,
                stop,
                &[block.id, latch_block, else_block],
                &mut HashSet::new(),
            ) {
                return Ok(false);
            }
            let has_header_work = block
                .statements
                .iter()
                .enumerate()
                .any(|(idx, _)| idx != cond_statement_idx);
            let then = self.block(then_block)?;
            let latch = self.block(latch_block)?;
            let loop_declared = self.declared_locals_snapshot();
            if has_header_work {
                out.push_str("    loop {\n");
                for statement in &block.statements {
                    self.emit_statement_for_block(block, statement, out)?;
                }
                out.push_str(&format!(
                    "    if !({}) {{ break; }}\n",
                    self.truthy_operand_text(&Operand::Copy(Place::Local(
                        self.switch_cond_local(block)?
                    )))?
                ));
            } else {
                out.push_str(&format!("    while {cond} {{\n"));
            }
            self.emit_block_until_goto(then, latch_block, Some(else_block), out)?;
            for statement in &latch.statements {
                self.emit_statement(statement, out)?;
            }
            out.push_str("    }\n");
            self.restore_declared_locals(loop_declared);
            self.emit_after_nested_loop(else_block, stop, break_target, out, visited)?;
            return Ok(true);
        }

        Ok(false)
    }

    /// Emits the continuation reached after a nested loop exits.
    fn emit_after_nested_loop(
        &self,
        exit_block: smelt_mir::BlockId,
        stop: smelt_mir::BlockId,
        break_target: Option<smelt_mir::BlockId>,
        out: &mut String,
        visited: &mut HashSet<smelt_mir::BlockId>,
    ) -> Result<(), EmitError> {
        if exit_block == stop {
            return Ok(());
        }
        if Some(exit_block) == break_target {
            out.push_str("    break;\n");
            return Ok(());
        }
        self.emit_block_until_goto_inner(self.block(exit_block)?, stop, break_target, out, visited)
    }

    /// Emits a type-correct conservative return for unsupported control-flow regions.
    fn default_return_statement(&self) -> Result<String, EmitError> {
        let value = self.default_value(self.function.return_ty)?;
        if self.function.can_throw {
            Ok(format!(
                "    return Ok::<_, Box<dyn std::error::Error>>({value});\n"
            ))
        } else {
            Ok(format!("    return {value};\n"))
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

/// Returns successor blocks for MIR terminators that continue execution.
fn control_flow_successors(terminator: &Terminator) -> Vec<smelt_mir::BlockId> {
    match terminator {
        Terminator::Goto(target) => vec![*target],
        Terminator::Call { target, unwind, .. } | Terminator::Await { target, unwind, .. } => {
            unwind
                .iter()
                .map(|handler| handler.catch_block)
                .chain(std::iter::once(*target))
                .collect()
        }
        Terminator::Switch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
        Terminator::Match { arms, default, .. } => {
            let mut successors = arms.iter().map(|arm| arm.target).collect::<Vec<_>>();
            if let Some(default_block) = default {
                successors.push(*default_block);
            }
            successors
        }
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
    }
}
