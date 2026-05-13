//! List query and fold emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a list search operation to Rust text.
    pub(super) fn list_search_text(
        &self,
        op: smelt_hir::ListSearchOp,
        list: &Operand,
        item: &Operand,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list search receiver must be a list"));
        };
        if self.operand_ty(item)? != *item_ty {
            return Err(EmitError::new(
                "list search item must match the list element type",
            ));
        }
        let method_name = match op {
            smelt_hir::ListSearchOp::Find => "position",
            smelt_hir::ListSearchOp::RFind => "rposition",
        };
        Ok(format!(
            "{}.iter().{method_name}(|item| item == &{}).map_or(-1.0, |idx| idx as f64)",
            self.operand_text(list)?,
            self.operand_text(item)?
        ))
    }

    /// Converts a capture-free callback list operation to Rust iterator text.
    pub(super) fn list_callback_text(
        &self,
        op: smelt_hir::ListCallbackOp,
        list: &Operand,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let callback_body = self.closure_callback_body(callback)?;
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(list_element_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list callback receiver must be a list"));
        };
        let element_ty = *list_element_ty;
        let list_text = self.operand_text(list)?;
        let callback_text = self.callback_expr_text(callback_body, &["item", "index", "array"])?;
        let closure = format!(
            "|(index, item)| {{ let item = (*item).clone(); let index = index as f64; let array = {list_text}.clone(); {callback_text} }}"
        );
        let ref_closure = format!(
            "|(index, item)| {{ let item = (**item).clone(); let index = index as f64; let array = {list_text}.clone(); {callback_text} }}"
        );
        match op {
            smelt_hir::ListCallbackOp::Map => {
                if self.mir.types.get(dest_ty) != Some(&Type::List(callback_body.ty)) {
                    return Err(EmitError::new("array map destination must be a list"));
                }
                Ok(format!(
                    "{list_text}.iter().enumerate().map({closure}).collect::<Vec<_>>()"
                ))
            }
            smelt_hir::ListCallbackOp::Filter => {
                self.validate_bool_callback(callback_body, "array filter")?;
                if dest_ty != list_ty {
                    return Err(EmitError::new(
                        "array filter destination must match the receiver list type",
                    ));
                }
                Ok(format!(
                    "{list_text}.iter().enumerate().filter({ref_closure}).map(|(_, item)| item.clone()).collect::<Vec<_>>()"
                ))
            }
            smelt_hir::ListCallbackOp::Find => {
                self.validate_bool_callback(callback_body, "array find")?;
                if self.mir.types.get(dest_ty) != Some(&Type::Optional(element_ty)) {
                    return Err(EmitError::new(
                        "array find destination must be optional element type",
                    ));
                }
                Ok(format!(
                    "{list_text}.iter().enumerate().find({ref_closure}).map(|(_, item)| item.clone())"
                ))
            }
            smelt_hir::ListCallbackOp::FindIndex => {
                self.validate_bool_callback(callback_body, "array findIndex")?;
                if self.mir.types.get(dest_ty) != Some(&Type::Float) {
                    return Err(EmitError::new(
                        "array findIndex destination must be a number",
                    ));
                }
                Ok(format!(
                    "{list_text}.iter().enumerate().position({closure}).map_or(-1.0, |idx| idx as f64)"
                ))
            }
            smelt_hir::ListCallbackOp::FindLast => {
                self.validate_bool_callback(callback_body, "array findLast")?;
                if self.mir.types.get(dest_ty) != Some(&Type::Optional(element_ty)) {
                    return Err(EmitError::new(
                        "array findLast destination must be optional element type",
                    ));
                }
                Ok(format!(
                    "{list_text}.iter().enumerate().rev().find({ref_closure}).map(|(_, item)| item.clone())"
                ))
            }
            smelt_hir::ListCallbackOp::FindLastIndex => {
                self.validate_bool_callback(callback_body, "array findLastIndex")?;
                if self.mir.types.get(dest_ty) != Some(&Type::Float) {
                    return Err(EmitError::new(
                        "array findLastIndex destination must be a number",
                    ));
                }
                Ok(format!(
                    "{list_text}.iter().enumerate().rposition({closure}).map_or(-1.0, |idx| idx as f64)"
                ))
            }
            smelt_hir::ListCallbackOp::Some => {
                self.validate_bool_callback(callback_body, "array some")?;
                if self.mir.types.get(dest_ty) != Some(&Type::Bool) {
                    return Err(EmitError::new("array some destination must be boolean"));
                }
                Ok(format!("{list_text}.iter().enumerate().any({closure})"))
            }
            smelt_hir::ListCallbackOp::Every => {
                self.validate_bool_callback(callback_body, "array every")?;
                if self.mir.types.get(dest_ty) != Some(&Type::Bool) {
                    return Err(EmitError::new("array every destination must be boolean"));
                }
                Ok(format!("{list_text}.iter().enumerate().all({closure})"))
            }
            smelt_hir::ListCallbackOp::ForEach => {
                if dest_ty != self.none_ty {
                    return Err(EmitError::new("array forEach destination must be none"));
                }
                Ok(format!(
                    "{{ {list_text}.iter().enumerate().for_each(|(index, item)| {{ let item = (*item).clone(); let index = index as f64; let array = {list_text}.clone(); let _ = {callback_text}; }}); () }}"
                ))
            }
            smelt_hir::ListCallbackOp::FlatMap => {
                let Some(Type::List(callback_item_ty)) = self.mir.types.get(callback_body.ty)
                else {
                    return Err(EmitError::new(
                        "array flatMap callback must return an array",
                    ));
                };
                if self.mir.types.get(dest_ty) != Some(&Type::List(*callback_item_ty)) {
                    return Err(EmitError::new(
                        "array flatMap destination must match callback array item type",
                    ));
                }
                Ok(format!(
                    "{list_text}.iter().enumerate().flat_map({closure}).collect::<Vec<_>>()"
                ))
            }
        }
    }

    /// Converts an array reduce callback into Rust `fold` text.
    pub(super) fn list_reduce_text(
        &self,
        list: &Operand,
        initial: Option<&Operand>,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let callback_body = self.closure_callback_body(callback)?;
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(list_element_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("array reduce receiver must be a list"));
        };
        let element_ty = *list_element_ty;
        if let Some(initial_operand) = initial {
            if self.operand_ty(initial_operand)? != dest_ty {
                return Err(EmitError::new(
                    "array reduce initial value and callback result must match the destination type",
                ));
            }
        }
        if callback_body.ty != dest_ty {
            return Err(EmitError::new(
                "array reduce initial value and callback result must match the destination type",
            ));
        }
        let list_text = self.operand_text(list)?;
        let callback_text =
            self.callback_expr_text(callback_body, &["acc", "item", "index", "array"])?;
        if let Some(initial_operand) = initial {
            let initial_text = self.operand_text(initial_operand)?;
            Ok(format!(
                "{list_text}.iter().enumerate().fold({initial_text}, |acc, (index, item)| {{ let item = (*item).clone(); let index = index as f64; let array = {list_text}.clone(); {callback_text} }})"
            ))
        } else if dest_ty == element_ty {
            Ok(format!(
                "{{ let mut reduce_items = {list_text}.iter().enumerate(); let (_, first) = reduce_items.next().expect(\"reduce of empty array with no initial value\"); reduce_items.fold(first.clone(), |acc, (index, item)| {{ let item = (*item).clone(); let index = index as f64; let array = {list_text}.clone(); {callback_text} }}) }}"
            ))
        } else {
            Err(EmitError::new(
                "array reduce without an initial value must produce the element type",
            ))
        }
    }

    /// Validates that a lowered callback expression returns a boolean.
    pub(super) fn validate_bool_callback(
        &self,
        callback: &smelt_hir::CallbackExpr,
        context: &'static str,
    ) -> Result<(), EmitError> {
        if self.mir.types.get(callback.ty) == Some(&Type::Bool) {
            Ok(())
        } else {
            Err(EmitError::new(format!(
                "{context} callback must return boolean"
            )))
        }
    }

    /// Converts a non-escaping MIR closure into a Rust closure literal.
    pub(super) fn closure_text(&self, id: smelt_mir::ClosureId) -> Result<String, EmitError> {
        let closure = self
            .mir
            .closures
            .get(usize::try_from(id.0).unwrap_or(usize::MAX))
            .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?;
        let body = if let Some(callback) = closure.callback_body.as_ref() {
            let params = closure
                .params
                .iter()
                .enumerate()
                .map(|(index, _)| format!("arg{index}"))
                .collect::<Vec<_>>();
            let param_refs = params.iter().map(String::as_str).collect::<Vec<_>>();
            let body_expr = self.callback_expr_text(callback, &param_refs)?;
            let body = if matches!(self.mir.types.get(closure.return_ty), Some(Type::Future(_))) {
                format!("Box::pin(async move {{ {body_expr} }})")
            } else {
                body_expr
            };
            format!("|{}| {{ {body} }}", params.join(", "))
        } else {
            let function = MirFunction {
                id: smelt_mir::FuncId(u32::MAX),
                name: Symbol(u32::MAX),
                origin: HirOrigin::Body(smelt_hir::BodyId(u32::MAX)),
                is_async: false,
                is_test: false,
                can_throw: false,
                params: closure.params.clone(),
                return_ty: closure.return_ty,
                locals: closure.locals.clone(),
                blocks: closure.blocks.clone(),
                entry: closure.entry,
            };
            let mut emitter = FunctionEmitter::new(self.mir, &function)?;
            for (index, param) in closure.params.iter().enumerate() {
                emitter.names.insert(*param, format!("closure_arg_{index}"));
            }
            for capture in &closure.captures {
                if let Some(target) = capture.target_local {
                    emitter
                        .names
                        .insert(target, self.local_name(capture.source_local)?.to_owned());
                }
            }
            let params = closure
                .params
                .iter()
                .map(|param| {
                    let local = emitter.local_decl(*param)?;
                    Ok(format!(
                        "{}: {}",
                        emitter.local_name(*param)?,
                        emitter.type_text(local.ty)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            let mut body_text = String::new();
            emitter.emit_closure_block(emitter.entry_block()?, &mut body_text)?;
            format!("|{params}| {{\n{body_text}    }}")
        };
        let capture_prefix = if closure.escapes
            || matches!(self.mir.types.get(closure.return_ty), Some(Type::Future(_)))
            || closure
                .captures
                .iter()
                .any(|capture| capture.mode == smelt_hir::CaptureMode::ByValue)
        {
            "move "
        } else {
            ""
        };
        Ok(format!("{capture_prefix}{body}"))
    }

    /// Emits a closure block with return terminators scoped to the closure body.
    pub(super) fn emit_closure_block(
        &self,
        block: &BasicBlock,
        out: &mut String,
    ) -> Result<(), EmitError> {
        for statement in &block.statements {
            self.emit_statement(statement, out)?;
        }
        let Some(terminator) = &block.terminator else {
            return Err(EmitError::new("closure basic block has no terminator"));
        };
        match terminator {
            Terminator::Return(operand) => {
                if self.function.return_ty == self.none_ty {
                    if !matches!(operand, Operand::Const(Constant::None)) {
                        out.push_str(&format!("    {};\n", self.operand_text(operand)?));
                    }
                    out.push_str("    ()\n");
                } else {
                    out.push_str(&format!(
                        "    {}\n",
                        self.operand_as_type_text(operand, self.function.return_ty)?
                    ));
                }
                Ok(())
            }
            Terminator::Goto(target) => self.emit_closure_block(self.block(*target)?, out),
            Terminator::Call {
                callee,
                args,
                dest,
                target,
            } => {
                let call_text = self.call_text(callee, args)?;
                let local = self.local_decl(*dest)?;
                let name = self.local_name(*dest)?;
                out.push_str(&format!(
                    "    let {name}: {} = {call_text};\n",
                    self.type_text(local.ty)?
                ));
                self.emit_closure_block(self.block(*target)?, out)
            }
            Terminator::Switch { .. } | Terminator::Match { .. } => Err(EmitError::new(
                "branching closure bodies are not supported in Rust codegen yet",
            )),
            Terminator::Throw(operand) => {
                out.push_str(&format!(
                    "    panic!(\"{{}}\", {});\n",
                    self.operand_text(operand)?
                ));
                Ok(())
            }
            Terminator::Unreachable => {
                out.push_str("    unreachable!()\n");
                Ok(())
            }
        }
    }

    /// Resolve a callback operand to the temporary MIR closure body it was constructed from.
    fn closure_callback_body(
        &self,
        operand: &Operand,
    ) -> Result<&smelt_hir::CallbackExpr, EmitError> {
        let local = match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => *local,
            _ => {
                return Err(EmitError::new(
                    "list callback must be a non-escaping closure local",
                ));
            }
        };
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign {
                    dest,
                    value: Rvalue::Closure { id, .. },
                } = statement
                    && *dest == local
                {
                    let closure = self
                        .mir
                        .closures
                        .get(usize::try_from(id.0).unwrap_or(usize::MAX))
                        .ok_or_else(|| {
                            EmitError::new("list callback references an unknown closure")
                        })?;
                    return closure.callback_body.as_ref().ok_or_else(|| {
                        EmitError::new("list callback closure has no callback body")
                    });
                }
            }
        }
        Err(EmitError::new(
            "list callback closure construction was not found",
        ))
    }

    /// Converts a callback expression tree to Rust source text.
    pub(super) fn callback_expr_text(
        &self,
        expr: &smelt_hir::CallbackExpr,
        params: &[&str],
    ) -> Result<String, EmitError> {
        match &expr.kind {
            smelt_hir::CallbackExprKind::Param(index) => params
                .get(*index)
                .map(|param| (*param).to_owned())
                .ok_or_else(|| EmitError::new("callback parameter index is out of bounds")),
            smelt_hir::CallbackExprKind::Capture(local) => {
                self.local_name(LocalId(local.0)).map(str::to_owned)
            }
            smelt_hir::CallbackExprKind::Function(function) => {
                Ok(sanitize_ident(self.symbol_name(*function)?))
            }
            smelt_hir::CallbackExprKind::AssignCapture { target, value } => {
                let target_text = self.local_name(LocalId(target.0))?;
                let value_text = self.callback_expr_text(value, params)?;
                Ok(format!(
                    "{{ {target_text} = {value_text}; {target_text}.clone() }}"
                ))
            }
            smelt_hir::CallbackExprKind::Literal(literal) => Ok(hir_literal_text(literal)),
            smelt_hir::CallbackExprKind::ListLit(items) => {
                let items_text = items
                    .iter()
                    .map(|item| self.callback_expr_text(item, params))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("vec![{items_text}]"))
            }
            smelt_hir::CallbackExprKind::Index { receiver, index } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                match self.mir.types.get(receiver.ty) {
                    Some(Type::Tuple(_)) => Ok(format!("{receiver_text}.{index}.clone()")),
                    Some(Type::List(_)) => Ok(format!("{receiver_text}[{index}].clone()")),
                    _ => Err(EmitError::new(
                        "callback indexed access requires a tuple or list receiver",
                    )),
                }
            }
            smelt_hir::CallbackExprKind::Field { receiver, field } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                let field_text = self.symbol_name(*field)?;
                match self.mir.types.get(receiver.ty) {
                    Some(Type::Dict(_, value_ty)) => Ok(format!(
                        "{receiver_text}.get({field_text:?}).cloned().unwrap_or({})",
                        self.default_value(*value_ty)?
                    )),
                    Some(Type::Class { .. }) => Ok(format!(
                        "{receiver_text}.{}.clone()",
                        sanitize_ident(field_text)
                    )),
                    _ => Err(EmitError::new(
                        "callback field access requires a record or class receiver",
                    )),
                }
            }
            smelt_hir::CallbackExprKind::HasField { receiver, field } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                let field_text = self.symbol_name(*field)?;
                match self.mir.types.get(receiver.ty) {
                    Some(Type::Dict(key, _)) if self.mir.types.get(*key) == Some(&Type::String) => {
                        Ok(format!("{receiver_text}.contains_key({field_text:?})"))
                    }
                    Some(Type::Unknown) => Ok(format!(
                        "matches!({receiver_text}, SmeltUnknown::Object(ref map) if map.contains_key({field_text:?}))"
                    )),
                    Some(Type::Class { .. }) => Ok("true".to_owned()),
                    _ => Err(EmitError::new(
                        "callback `in` check requires a record, unknown, or class receiver",
                    )),
                }
            }
            smelt_hir::CallbackExprKind::Unary { op, operand } => {
                let op_text = match op {
                    smelt_hir::UnaryOp::Not => "!",
                    smelt_hir::UnaryOp::Neg => "-",
                };
                Ok(format!(
                    "{op_text}({})",
                    self.callback_expr_text(operand, params)?
                ))
            }
            smelt_hir::CallbackExprKind::Binary { op, lhs, rhs } => Ok(format!(
                "({} {} {})",
                self.callback_expr_text(lhs, params)?,
                smelt_hir::bin_op_text(*op),
                self.callback_expr_text(rhs, params)?
            )),
            smelt_hir::CallbackExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => Ok(format!(
                "if {} {{ {} }} else {{ {} }}",
                self.callback_expr_text(cond, params)?,
                self.callback_expr_as_type_text(then_expr, expr.ty, params)?,
                self.callback_expr_as_type_text(else_expr, expr.ty, params)?
            )),
            smelt_hir::CallbackExprKind::Call { callee, args } => {
                let callee_text = self.callback_expr_text(callee, params)?;
                let args_text = args
                    .iter()
                    .map(|arg| {
                        let text = self.callback_expr_text(&arg.expr, params)?;
                        if arg.spread {
                            Ok(format!("{text}.clone()"))
                        } else {
                            Ok(text)
                        }
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                Ok(format!("{callee_text}({args_text})"))
            }
        }
    }

    /// Converts a callback expression to Rust text expected at a target type.
    pub(super) fn callback_expr_as_type_text(
        &self,
        expr: &smelt_hir::CallbackExpr,
        target: TypeId,
        params: &[&str],
    ) -> Result<String, EmitError> {
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            if self.mir.types.get(expr.ty) == Some(&Type::None) {
                return Ok("None".to_owned());
            }
            if expr.ty == *inner {
                return Ok(format!("Some({})", self.callback_expr_text(expr, params)?));
            }
        }
        self.callback_expr_text(expr, params)
    }

    /// Converts a list slice operation to Rust text.
    /// Converts a list slice operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a list slice operation to Rust text.
    /// Converts a list slice operation to Rust text.
    /// Converts a list count operation to Rust text.
    /// Converts a list count operation to Rust text.
    pub(super) fn list_count_text(
        &self,
        list: &Operand,
        item: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list count receiver must be a list"));
        };
        if self.operand_ty(item)? != *item_ty {
            return Err(EmitError::new(
                "list count item must match the list element type",
            ));
        }
        if !matches!(self.mir.types.get(dest_ty), Some(Type::Int)) {
            return Err(EmitError::new("list count destination must be int"));
        }
        Ok(format!(
            "{}.iter().filter(|item| *item == &{}).count() as i64",
            self.operand_text(list)?,
            self.operand_text(item)?
        ))
    }

    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    /// Converts a numeric list sum operation to Rust text.
    pub(super) fn list_sum_text(
        &self,
        list: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list sum receiver must be a list"));
        };
        if dest_ty != *item_ty {
            return Err(EmitError::new(
                "list sum destination must match the list element type",
            ));
        }
        match self.mir.types.get(*item_ty) {
            Some(Type::Int) => Ok(format!(
                "{}.iter().copied().sum::<i64>()",
                self.operand_text(list)?
            )),
            Some(Type::Float) => Ok(format!(
                "{}.iter().copied().sum::<f64>()",
                self.operand_text(list)?
            )),
            _ => Err(EmitError::new("list sum supports int and float lists")),
        }
    }

    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    /// Converts a boolean list fold operation to Rust text.
    pub(super) fn list_bool_fold_text(
        &self,
        op: smelt_hir::BoolFoldOp,
        list: &Operand,
    ) -> Result<String, EmitError> {
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(list_ty) else {
            return Err(EmitError::new("list boolean fold receiver must be a list"));
        };
        if !matches!(self.mir.types.get(*item_ty), Some(Type::Bool)) {
            return Err(EmitError::new(
                "list boolean fold supports boolean lists only",
            ));
        }
        let method_name = match op {
            smelt_hir::BoolFoldOp::All => "all",
            smelt_hir::BoolFoldOp::Any => "any",
        };
        Ok(format!(
            "{}.iter().copied().{method_name}(|value| value)",
            self.operand_text(list)?
        ))
    }

    // Sorted-list helpers continue in `list_ordering.rs`.
}
