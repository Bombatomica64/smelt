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

    /// Converts `Array.from({ length }, mapper)` into an indexed Rust loop.
    pub(super) fn list_from_length_map_text(
        &self,
        length: &Operand,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let callback_body = self.closure_callback_body(callback)?;
        if self.mir.types.get(dest_ty) != Some(&Type::List(callback_body.ty)) {
            return Err(EmitError::new(
                "Array.from mapper destination must be a list of callback results",
            ));
        }
        if !matches!(
            self.mir.types.get(self.operand_ty(length)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("Array.from length must be numeric"));
        }
        let length_text = self.operand_text(length)?;
        let callback_text = self.callback_expr_text(callback_body, &["item", "index"])?;
        Ok(format!(
            "{{ let array_from_length = ({length_text} as f64).max(0.0).floor() as usize; (0..array_from_length).map(|index| {{ let item = SmeltUnknown::Null; let index = index as f64; {callback_text} }}).collect::<Vec<_>>() }}"
        ))
    }

    /// Converts `Array.from({ length })` into a sparse-like unknown vector.
    pub(super) fn list_from_length_text(
        &self,
        length: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if self.mir.types.get(dest_ty) != Some(&Type::List(self.type_id(Type::Unknown)?)) {
            return Err(EmitError::new(
                "Array.from length destination must be list[unknown]",
            ));
        }
        if !matches!(
            self.mir.types.get(self.operand_ty(length)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("Array.from length must be numeric"));
        }
        let length_text = self.operand_text(length)?;
        Ok(format!(
            "{{ let array_from_length = ({length_text} as f64).max(0.0).floor() as usize; vec![SmeltUnknown::Null; array_from_length] }}"
        ))
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
            smelt_hir::CallbackExprKind::FunctionTableLookup { key, cases } => {
                let key_text = self.callback_expr_text(key, params)?;
                let cases_text = cases
                    .iter()
                    .map(|(case_key, function)| {
                        let function_text = sanitize_ident(self.symbol_name(*function)?);
                        Ok(format!("{case_key:?} => {function_text}"))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                Ok(format!(
                    "match {key_text}.as_str() {{ {cases_text}, _ => panic!(\"unknown function table key: {{}}\", {key_text}) }}"
                ))
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
            smelt_hir::CallbackExprKind::Sequence { effects, result } => {
                let effects_text = effects
                    .iter()
                    .map(|effect| {
                        self.callback_expr_text(effect, params)
                            .map(|text| format!("{text};"))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(" ");
                let result_text = self.callback_expr_text(result, params)?;
                Ok(format!("{{ {effects_text} {result_text} }}"))
            }
            smelt_hir::CallbackExprKind::DictLit(entries) => match self.mir.types.get(expr.ty) {
                Some(Type::Dict(_, value_ty)) => {
                    let entries_text = entries
                        .iter()
                        .map(|(key, value)| {
                            let key_text = self.symbol_name(*key)?;
                            let value_text =
                                self.callback_expr_as_type_text(value, *value_ty, params)?;
                            Ok(format!("({key_text:?}.to_owned(), {value_text})"))
                        })
                        .collect::<Result<Vec<_>, EmitError>>()?
                        .join(", ");
                    Ok(format!(
                        "::std::collections::HashMap::from([{entries_text}])"
                    ))
                }
                Some(Type::Class { name, .. }) => {
                    self.callback_struct_literal_text(*name, entries, params)
                }
                _ => Err(EmitError::new(
                    "callback object literal requires a dict or structural result type",
                )),
            },
            smelt_hir::CallbackExprKind::Throw { message } => {
                if let Some(thrown_message) = message {
                    Ok(format!(
                        "panic!(\"{{}}\", {})",
                        self.callback_expr_text(thrown_message, params)?
                    ))
                } else {
                    Ok("panic!(\"callback threw\")".to_owned())
                }
            }
            smelt_hir::CallbackExprKind::Index { receiver, index } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                match self.mir.types.get(receiver.ty) {
                    Some(Type::Tuple(_)) => Ok(format!("{receiver_text}.{index}.clone()")),
                    Some(Type::List(_)) => Ok(format!("{receiver_text}[{index}].clone()")),
                    Some(Type::String) => Ok(format!(
                        "{receiver_text}.chars().nth({index}).unwrap_or_default().to_string()"
                    )),
                    _ => Err(EmitError::new(
                        "callback indexed access requires a tuple, list, or string receiver",
                    )),
                }
            }
            smelt_hir::CallbackExprKind::DynamicIndex { receiver, index } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                let index_text = self.callback_expr_text(index, params)?;
                match self.mir.types.get(receiver.ty) {
                    Some(Type::List(_)) => Ok(format!(
                        "{{ let callback_index_receiver = ({receiver_text}).clone(); let callback_index = {{ let len = callback_index_receiver.len() as i64; let index = {index_text} as i64; let normalized = if index < 0 {{ len + index }} else {{ index }}; usize::try_from(normalized).expect(\"negative index out of bounds\") }}; callback_index_receiver.get(callback_index).cloned().expect(\"index out of bounds\") }}"
                    )),
                    Some(Type::String) => Ok(format!(
                        "{{ let callback_index_receiver = ({receiver_text}).clone(); let callback_index = {{ let len = callback_index_receiver.chars().count() as i64; let index = {index_text} as i64; let normalized = if index < 0 {{ len + index }} else {{ index }}; usize::try_from(normalized).expect(\"negative index out of bounds\") }}; callback_index_receiver.chars().nth(callback_index).map(|ch| ch.to_string()).expect(\"index out of bounds\") }}"
                    )),
                    _ => Err(EmitError::new(
                        "callback dynamic indexed access requires a list or string receiver",
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
            smelt_hir::CallbackExprKind::HasDynamicField { receiver, field } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                let field_text = self.callback_expr_text(field, params)?;
                match self.mir.types.get(receiver.ty) {
                    Some(Type::Dict(key, _)) if self.mir.types.get(*key) == Some(&Type::String) => {
                        Ok(format!("{receiver_text}.contains_key(&{field_text})"))
                    }
                    Some(Type::Unknown) => Ok(format!(
                        "matches!({receiver_text}, SmeltUnknown::Object(ref map) if map.contains_key(&{field_text}))"
                    )),
                    Some(Type::Class { .. }) => Ok("true".to_owned()),
                    _ => Err(EmitError::new(
                        "callback dynamic field check requires a record, unknown, or class receiver",
                    )),
                }
            }
            smelt_hir::CallbackExprKind::FieldTruthy { receiver, field } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                let field_text = self.symbol_name(*field)?;
                match self.mir.types.get(receiver.ty) {
                    Some(Type::Optional(inner)) => match self.mir.types.get(*inner) {
                        Some(Type::Class { .. }) => Ok(format!(
                            "{receiver_text}.as_ref().is_some_and(|value| value.{})",
                            sanitize_ident(field_text)
                        )),
                        Some(Type::Dict(key, _))
                            if self.mir.types.get(*key) == Some(&Type::String) =>
                        {
                            Ok(format!(
                                "{receiver_text}.as_ref().and_then(|value| value.get({field_text:?})).copied().unwrap_or(false)"
                            ))
                        }
                        _ => Err(EmitError::new(
                            "callback optional field truthiness requires an optional class or record receiver",
                        )),
                    },
                    Some(Type::Class { .. }) => {
                        Ok(format!("{receiver_text}.{}", sanitize_ident(field_text)))
                    }
                    Some(Type::Dict(key, _)) if self.mir.types.get(*key) == Some(&Type::String) => {
                        Ok(format!(
                            "{receiver_text}.get({field_text:?}).copied().unwrap_or(false)"
                        ))
                    }
                    Some(Type::Unknown) => Ok(format!(
                        "matches!({receiver_text}, SmeltUnknown::Object(ref map) if matches!(map.get({field_text:?}), Some(SmeltUnknown::Bool(true))))"
                    )),
                    _ => Err(EmitError::new(
                        "callback field truthiness requires a class, record, optional, or unknown receiver",
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
            smelt_hir::CallbackExprKind::Binary { op, lhs, rhs } => {
                let lhs_text = self.callback_expr_text(lhs, params)?;
                let rhs_text = self.callback_expr_text(rhs, params)?;
                if *op == smelt_hir::BinOp::UShr {
                    return Ok(format!(
                        "{{ let smelt_shift_value = ({lhs_text}).trunc(); let smelt_shift_value = if smelt_shift_value.is_finite() {{ smelt_shift_value.rem_euclid(4294967296.0) as u32 }} else {{ 0_u32 }}; let smelt_shift_count = ({rhs_text}).trunc(); let smelt_shift_count = if smelt_shift_count.is_finite() {{ smelt_shift_count.rem_euclid(4294967296.0) as u32 }} else {{ 0_u32 }}; (smelt_shift_value >> (smelt_shift_count & 31)) as f64 }}"
                    ));
                }
                Ok(format!(
                    "({lhs_text} {} {rhs_text})",
                    smelt_hir::bin_op_text(*op)
                ))
            }
            smelt_hir::CallbackExprKind::UnknownIs { value, kind } => {
                let value_text = self.callback_expr_text(value, params)?;
                self.unknown_is_text_raw(&value_text, *kind)
            }
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
                if let smelt_hir::CallbackExprKind::FunctionTableLookup { key, cases } =
                    &callee.kind
                {
                    return self.callback_function_table_call_text(key, cases, args, params);
                }
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
            smelt_hir::CallbackExprKind::MethodCall {
                receiver,
                method,
                args,
            } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                let method_text = self.symbol_name(*method)?;
                let args_text = args
                    .iter()
                    .map(|arg| self.callback_expr_text(&arg.expr, params))
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                if method_text == "toString" {
                    Ok(format!("{receiver_text}.to_string()"))
                } else if method_text == "match" && args.len() == 1 {
                    let pattern = args.first().ok_or_else(|| {
                        EmitError::new("callback match call requires one pattern argument")
                    })?;
                    let pattern_text = self.callback_expr_text(&pattern.expr, params)?;
                    Ok(format!(
                        "regex::Regex::new(&{pattern_text}).expect(\"regex compile failed\").is_match(&{receiver_text})"
                    ))
                } else {
                    Ok(format!(
                        "{receiver_text}.{}({args_text})",
                        sanitize_ident(method_text)
                    ))
                }
            }
        }
    }

    /// Emits a dynamic call through a statically known function table.
    ///
    /// JavaScript libraries often select a formatter from an exported object
    /// and immediately call it. Rust has no direct equivalent for heterogenous
    /// function items in a map, so codegen lowers the table lookup to a
    /// key-dispatching `match` that calls the selected function directly.
    fn callback_function_table_call_text(
        &self,
        key: &smelt_hir::CallbackExpr,
        cases: &[(String, Symbol)],
        args: &[smelt_hir::CallbackCallArg],
        params: &[&str],
    ) -> Result<String, EmitError> {
        let key_text = self.callback_expr_text(key, params)?;
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
        let cases_text = cases
            .iter()
            .map(|(case_key, function)| {
                let function_text = sanitize_ident(self.symbol_name(*function)?);
                Ok(format!("{case_key:?} => {function_text}({args_text})"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        Ok(format!(
            "{{ let __smelt_function_key = {key_text}; match __smelt_function_key.as_str() {{ {cases_text}, _ => panic!(\"unknown function table key: {{}}\", __smelt_function_key) }} }}"
        ))
    }

    /// Emits a callback object literal as a structural Rust value.
    fn callback_struct_literal_text(
        &self,
        class: Symbol,
        entries: &[(Symbol, smelt_hir::CallbackExpr)],
        params: &[&str],
    ) -> Result<String, EmitError> {
        let class_name = sanitize_ident(self.symbol_name(class)?);
        let mir_class = self
            .mir
            .classes
            .iter()
            .find(|item| item.name == class)
            .ok_or_else(|| {
                EmitError::new("callback structural object literal references an unknown class")
            })?;
        let mut parts = Vec::new();
        for field in &mir_class.fields {
            let name = sanitize_ident(self.symbol_name(field.name)?);
            if let Some((_, value)) = entries
                .iter()
                .find(|(entry_name, _)| *entry_name == field.name)
            {
                let value_text = self.callback_expr_as_type_text(value, field.ty, params)?;
                parts.push(format!("{name}: {value_text}"));
            } else {
                parts.push(format!("{name}: {}", self.default_value(field.ty)?));
            }
        }
        Ok(format!("{class_name} {{ {} }}", parts.join(", ")))
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
        if self.mir.types.get(target) == Some(&Type::Unknown) {
            let text = self.callback_expr_text(expr, params)?;
            return match self.mir.types.get(expr.ty) {
                Some(Type::None) => Ok("SmeltUnknown::Null".to_owned()),
                Some(Type::Bool) => Ok(format!("SmeltUnknown::Bool({text})")),
                Some(Type::Int | Type::Float) => Ok(format!("SmeltUnknown::Number({text} as f64)")),
                Some(Type::String) => Ok(format!("SmeltUnknown::String({text})")),
                Some(Type::List(_)) => Ok(format!("SmeltUnknown::Array({text})")),
                Some(Type::Dict(_, _)) => Ok(format!("SmeltUnknown::Object({text})")),
                Some(Type::Unknown) => Ok(text),
                _ => Err(EmitError::new(
                    "callback expression cannot be wrapped as unknown",
                )),
            };
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
