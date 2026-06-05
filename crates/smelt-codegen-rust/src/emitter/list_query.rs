//! List query and fold emission helpers.

use super::*;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// Return whether callback field checks should inspect the value at runtime.
    ///
    /// Callback expressions carry the static receiver type from HIR. Unions and
    /// erased classes still lower to `SmeltUnknown` storage in Rust, so JS
    /// membership checks such as `"lazy" in op` must inspect their runtime shape
    /// instead of folding to `false`.
    fn callback_field_receiver_is_erased(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(ty)
    }

    /// Returns a Rust closure return type, wrapping throwing closures in `Result`.
    fn closure_return_type_text(
        &self,
        return_ty: TypeId,
        can_throw: bool,
    ) -> Result<String, EmitError> {
        let inner = self.type_text_with_impl_trait(return_ty, false)?;
        if can_throw {
            Ok(format!("Result<{inner}, Box<dyn std::error::Error>>"))
        } else {
            Ok(inner)
        }
    }

    /// Return true when rendered callback argument text is a generated no-op.
    fn callback_arg_text_is_default(arg: &str) -> bool {
        arg.contains("Rc<dyn Fn")
            || arg.starts_with("&mut |")
            || arg.contains("let smelt_default_callback")
    }

    /// Return the field type for a concrete generated class.
    fn callback_class_field_type(
        &self,
        class_ty: TypeId,
        field: Symbol,
    ) -> Result<TypeId, EmitError> {
        let Some(Type::Class { name, .. }) = self.mir.types.get(class_ty) else {
            return Err(EmitError::new("callback class field type requires a class"));
        };
        if let Some(class) = self.mir.classes.iter().find(|item| item.name == *name)
            && let Some(class_field) = crate::classes::effective_class_fields(self.mir, class)
                .into_iter()
                .find(|item| item.name == field)
        {
            return Ok(class_field.ty);
        }
        if let Some(interface) = self.mir.interfaces.iter().find(|item| item.name == *name)
            && let Some(interface_field) =
                crate::classes::effective_interface_fields(self.mir, interface)
                    .into_iter()
                    .find(|item| item.name == field)
        {
            return Ok(interface_field.ty);
        }
        Err(EmitError::new("callback class field is missing"))
    }

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
        let item_text = if self.operand_ty(item)? == *item_ty {
            self.operand_text(item)?
        } else {
            self.value_at_type_text(&self.operand_text(item)?, self.operand_ty(item)?, *item_ty)?
        };
        let method_name = match op {
            smelt_hir::ListSearchOp::Find => "position",
            smelt_hir::ListSearchOp::RFind => "rposition",
        };
        let list_text = self.operand_text(list)?;
        if self.list_item_uses_same_value_zero(*item_ty) {
            if self.mir.types.get(*item_ty) == Some(&Type::Float) {
                return Ok(format!(
                    "{{ let smelt_needle = {item_text}; {list_text}.iter().{method_name}(|item| *item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())).map_or(-1.0, |idx| idx as f64) }}"
                ));
            }
            return Ok(format!(
                "{{ let smelt_needle = {item_text}; {list_text}.iter().{method_name}(|item| item.same_js_key(&smelt_needle)).map_or(-1.0, |idx| idx as f64) }}"
            ));
        }
        Ok(format!(
            "{list_text}.iter().{method_name}(|item| item == &{item_text}).map_or(-1.0, |idx| idx as f64)"
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
        let list_ty = self.operand_ty(list)?;
        let Some(Type::List(list_element_ty)) = self.mir.types.get(list_ty) else {
            return Ok("Default::default()".to_owned());
        };
        let element_ty = *list_element_ty;
        let callback_body = match self.closure_callback_body(callback) {
            Ok(callback_body) => callback_body,
            Err(_) => {
                return self
                    .list_callback_closure_text(op, list, list_ty, element_ty, callback, dest_ty);
            }
        };
        let list_text = if matches!(self.mir.types.get(element_ty), Some(Type::Function(_))) {
            match list {
                Operand::Copy(place) | Operand::Move(place) => self.place_text(place)?,
                Operand::Const(_) => self.operand_text(list)?,
            }
        } else {
            self.operand_text(list)?
        };
        let raw_callback_text =
            self.callback_expr_text(callback_body, &["item", "index", "array"])?;
        if raw_callback_text == "Default::default()"
            && matches!(op, smelt_hir::ListCallbackOp::Map)
            && matches!(
                self.mir.types.get(self.operand_ty(callback)?),
                Some(Type::Function(_))
            )
        {
            return self.list_map_closure_text(list, list_ty, element_ty, callback, dest_ty);
        }
        if raw_callback_text == "Default::default()"
            && matches!(op, smelt_hir::ListCallbackOp::ForEach)
            && matches!(
                self.mir.types.get(self.operand_ty(callback)?),
                Some(Type::Function(_))
            )
        {
            return self.list_for_each_closure_text(list, list_ty, element_ty, callback, dest_ty);
        }
        if raw_callback_text == "Default::default()"
            && matches!(op, smelt_hir::ListCallbackOp::FlatMap)
            && matches!(
                self.mir.types.get(self.operand_ty(callback)?),
                Some(Type::Function(_))
            )
        {
            return self.list_flat_map_closure_text(list, list_ty, element_ty, callback, dest_ty);
        }
        let callback_text = if raw_callback_text == "Default::default()"
            && matches!(
                self.mir.types.get(callback_body.ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) {
            "SmeltUnknown::Null".to_owned()
        } else {
            raw_callback_text
        };
        if matches!(
            callback_text.as_str(),
            "Default::default()" | "SmeltUnknown::Null"
        ) && matches!(op, smelt_hir::ListCallbackOp::Map)
        {
            return self.list_map_closure_text(list, list_ty, element_ty, callback, dest_ty);
        }
        let callback_is_static_default =
            matches!(callback_text.as_str(), "None" | "Default::default()" | "()");
        let callback_uses_item =
            !callback_is_static_default && callback_uses_param(&self.mir.types, callback_body, 0);
        let callback_uses_index =
            !callback_is_static_default && callback_uses_param(&self.mir.types, callback_body, 1);
        let function_element = matches!(self.mir.types.get(element_ty), Some(Type::Function(_)));
        let callback_uses_array =
            !callback_is_static_default && callback_uses_param(&self.mir.types, callback_body, 2);
        let callback_index_ty = match self.mir.types.get(self.operand_ty(callback)?) {
            Some(Type::Function(function)) => function.params.get(1).copied(),
            _ => None,
        };
        let index_value_text =
            if callback_index_ty.is_some_and(|ty| self.mir.types.get(ty) == Some(&Type::Int)) {
                "index as i64"
            } else {
                "index as f64"
            };
        let ref_index_value_text =
            if callback_index_ty.is_some_and(|ty| self.mir.types.get(ty) == Some(&Type::Int)) {
                "*index as i64"
            } else {
                "*index as f64"
            };
        let function_item_closure = || {
            let index_binding = if callback_uses_index {
                format!("let index = {index_value_text}; ")
            } else {
                String::new()
            };
            let array_binding = if callback_uses_array {
                format!("let array = {list_text}.clone(); ")
            } else {
                String::new()
            };
            format!("|(index, item)| {{ {index_binding}{array_binding}{callback_text} }}")
        };
        let item_binding = if callback_uses_item {
            "let item = (*item).clone(); "
        } else {
            ""
        };
        let ref_item_binding = if callback_uses_item {
            "let item = (**item).clone(); "
        } else {
            ""
        };
        let index_binding = if callback_uses_index {
            format!("let index = {index_value_text}; ")
        } else {
            String::new()
        };
        let ref_index_binding = if callback_uses_index {
            format!("let index = {ref_index_value_text}; ")
        } else {
            String::new()
        };
        let array_binding = if callback_uses_array {
            format!("let array = {list_text}.clone(); ")
        } else {
            String::new()
        };
        let closure = format!(
            "|(index, item)| {{ {item_binding}{index_binding}{array_binding}{callback_text} }}"
        );
        let ref_closure = format!(
            "|(index, item)| {{ {ref_item_binding}{ref_index_binding}{array_binding}{callback_text} }}"
        );
        match op {
            smelt_hir::ListCallbackOp::Map => {
                let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
                    return Err(EmitError::new("array map destination must be a list"));
                };
                if callback_text.contains("return Err(")
                    || self.callback_expr_calls_throwing_function(callback_body)
                {
                    return self.throwing_list_map_text(
                        &list_text,
                        item_binding,
                        &index_binding,
                        &array_binding,
                        &callback_text,
                        callback_body.ty,
                        *dest_item_ty,
                    );
                }
                let callback_value = self.callback_expr_as_type_text(
                    callback_body,
                    *dest_item_ty,
                    &["item", "index", "array"],
                )?;
                let typed_closure = format!(
                    "|(index, item)| {{ {item_binding}{index_binding}{array_binding}{callback_value} }}"
                );
                Ok(format!(
                    "{list_text}.iter().enumerate().map({typed_closure}).collect::<Vec<_>>()"
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
                if function_element {
                    return Ok(format!(
                        "{list_text}.iter_mut().enumerate().any({})",
                        function_item_closure()
                    ));
                }
                Ok(format!("{list_text}.iter().enumerate().any({closure})"))
            }
            smelt_hir::ListCallbackOp::Every => {
                self.validate_bool_callback(callback_body, "array every")?;
                if self.mir.types.get(dest_ty) != Some(&Type::Bool) {
                    return Err(EmitError::new("array every destination must be boolean"));
                }
                if function_element {
                    return Ok(format!(
                        "{list_text}.iter_mut().enumerate().all({})",
                        function_item_closure()
                    ));
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

    /// Emits a list callback operation through a normal MIR closure body.
    fn list_callback_closure_text(
        &self,
        op: smelt_hir::ListCallbackOp,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        match op {
            smelt_hir::ListCallbackOp::Map => {
                return self.list_map_closure_text(list, list_ty, element_ty, callback, dest_ty);
            }
            smelt_hir::ListCallbackOp::ForEach => {
                return self
                    .list_for_each_closure_text(list, list_ty, element_ty, callback, dest_ty);
            }
            smelt_hir::ListCallbackOp::FlatMap => {
                return self
                    .list_flat_map_closure_text(list, list_ty, element_ty, callback, dest_ty);
            }
            _ => {}
        }
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Ok("Default::default()".to_owned());
        };
        let Some(item_param_ty) = function_ty.params.first().copied() else {
            return Ok("Default::default()".to_owned());
        };
        if self.mir.types.get(function_ty.return_ty) != Some(&Type::Bool) {
            return Err(EmitError::new(
                "array predicate callback must return boolean",
            ));
        }
        let list_text = self.operand_text(list)?;
        let closure_text = match self.closure_operand_text_for_declared_type(callback) {
            Ok(closure_text) => closure_text,
            Err(_) => self.operand_text(callback)?,
        };
        let item_text = self.value_at_type_text("item.clone()", element_ty, item_param_ty)?;
        let mut call_args = vec![item_text];
        if let Some(index_param_ty) = function_ty.params.get(1).copied() {
            let index_source_ty = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                self.type_id(Type::Int)?
            } else {
                self.type_id(Type::Float)?
            };
            let index_value = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                "index as i64"
            } else {
                "index as f64"
            };
            call_args.push(self.value_at_type_text(
                index_value,
                index_source_ty,
                index_param_ty,
            )?);
        }
        if let Some(array_param_ty) = function_ty.params.get(2).copied() {
            call_args.push(self.value_at_type_text(
                "smelt_array.clone()",
                list_ty,
                array_param_ty,
            )?);
        }
        let call_text = format!("(smelt_callback)({})", call_args.join(", "));
        let prefix = format!(
            "let mut smelt_callback = {closure_text}; let smelt_array = {list_text}.clone();"
        );
        match op {
            smelt_hir::ListCallbackOp::Filter => {
                if dest_ty != list_ty {
                    return Err(EmitError::new(
                        "array filter destination must match the receiver list type",
                    ));
                }
                Ok(format!(
                    "{{ {prefix} smelt_array.iter().enumerate().filter_map(|(index, item)| if {call_text} {{ Some(item.clone()) }} else {{ None }}).collect::<Vec<_>>() }}"
                ))
            }
            smelt_hir::ListCallbackOp::Find | smelt_hir::ListCallbackOp::FindLast => {
                if self.mir.types.get(dest_ty) != Some(&Type::Optional(element_ty)) {
                    return Err(EmitError::new(
                        "array find destination must be optional element type",
                    ));
                }
                let direction = if matches!(op, smelt_hir::ListCallbackOp::FindLast) {
                    ".rev()"
                } else {
                    ""
                };
                Ok(format!(
                    "{{ {prefix} smelt_array.iter().enumerate(){direction}.find_map(|(index, item)| if {call_text} {{ Some(item.clone()) }} else {{ None }}) }}"
                ))
            }
            smelt_hir::ListCallbackOp::FindIndex | smelt_hir::ListCallbackOp::FindLastIndex => {
                if self.mir.types.get(dest_ty) != Some(&Type::Float) {
                    return Err(EmitError::new(
                        "array findIndex destination must be a number",
                    ));
                }
                let direction = if matches!(op, smelt_hir::ListCallbackOp::FindLastIndex) {
                    ".rev()"
                } else {
                    ""
                };
                Ok(format!(
                    "{{ {prefix} smelt_array.iter().enumerate(){direction}.find_map(|(index, item)| if {call_text} {{ Some(index as f64) }} else {{ None }}).unwrap_or(-1.0) }}"
                ))
            }
            smelt_hir::ListCallbackOp::Some | smelt_hir::ListCallbackOp::Every => {
                if self.mir.types.get(dest_ty) != Some(&Type::Bool) {
                    return Err(EmitError::new(
                        "array predicate destination must be boolean",
                    ));
                }
                let method = if matches!(op, smelt_hir::ListCallbackOp::Some) {
                    "any"
                } else {
                    "all"
                };
                Ok(format!(
                    "{{ {prefix} smelt_array.iter().enumerate().{method}(|(index, item)| {call_text}) }}"
                ))
            }
            smelt_hir::ListCallbackOp::Map
            | smelt_hir::ListCallbackOp::ForEach
            | smelt_hir::ListCallbackOp::FlatMap => Err(EmitError::new(
                "list callback operation should have been handled before predicate emission",
            )),
        }
    }

    /// Emit `Array.map` when the callback can throw.
    ///
    /// Callback expression lowering represents source-language `throw` as a
    /// Rust `return Err(...)`. That only type-checks when the Rust iterator
    /// closure itself returns `Result<T, _>`, so map collection must transpose
    /// `Iterator<Item = Result<T, _>>` back into `Result<Vec<T>, _>`.
    fn throwing_list_map_text(
        &self,
        list_text: &str,
        item_binding: &str,
        index_binding: &str,
        array_binding: &str,
        callback_text: &str,
        source_item_ty: TypeId,
        dest_item_ty: TypeId,
    ) -> Result<String, EmitError> {
        let item_ty_text = self.type_text_with_impl_trait(dest_item_ty, false)?;
        let throwing_callback_text = callback_text
            .replace(".expect(\"throwing function table call failed\")", "?")
            .replace(
                ".expect(\"throwing call failed inside non-throwing closure\")",
                "?",
            );
        let callback_value =
            self.value_at_type_text("smelt_map_value", source_item_ty, dest_item_ty)?;
        let closure = format!(
            "|(index, item)| -> Result<{item_ty_text}, Box<dyn std::error::Error>> {{ {item_binding}{index_binding}{array_binding}let smelt_map_value = {{ {throwing_callback_text} }}; Ok::<_, Box<dyn std::error::Error>>({callback_value}) }}"
        );
        let collected = format!(
            "{list_text}.iter().enumerate().map({closure}).collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()"
        );
        if self.function.can_throw {
            Ok(format!("{collected}?"))
        } else {
            Ok(format!(
                "{collected}.expect(\"throwing array map callback failed\")"
            ))
        }
    }

    /// Return whether a legacy callback expression calls a generated throwing function.
    fn callback_expr_calls_throwing_function(&self, expr: &smelt_hir::CallbackExpr) -> bool {
        use smelt_hir::CallbackExprKind;
        match &expr.kind {
            CallbackExprKind::Call { callee, args } => {
                self.callback_expr_calls_throwing_function(callee)
                    || args
                        .iter()
                        .any(|arg| self.callback_expr_calls_throwing_function(&arg.expr))
            }
            CallbackExprKind::MethodCall { receiver, args, .. } => {
                self.callback_expr_calls_throwing_function(receiver)
                    || args
                        .iter()
                        .any(|arg| self.callback_expr_calls_throwing_function(&arg.expr))
            }
            CallbackExprKind::Function(function) => self.callback_function_can_throw(*function),
            CallbackExprKind::FunctionTableLookup { key, cases } => {
                self.callback_expr_calls_throwing_function(key)
                    || cases
                        .iter()
                        .any(|(_, function)| self.callback_function_can_throw(*function))
            }
            CallbackExprKind::Unary { operand, .. } => {
                self.callback_expr_calls_throwing_function(operand)
            }
            CallbackExprKind::Binary { lhs, rhs, .. } => {
                self.callback_expr_calls_throwing_function(lhs)
                    || self.callback_expr_calls_throwing_function(rhs)
            }
            CallbackExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                self.callback_expr_calls_throwing_function(cond)
                    || self.callback_expr_calls_throwing_function(then_expr)
                    || self.callback_expr_calls_throwing_function(else_expr)
            }
            CallbackExprKind::ListLit(items) => items
                .iter()
                .any(|item| self.callback_expr_calls_throwing_function(item)),
            CallbackExprKind::DictLit(entries) => entries
                .iter()
                .any(|(_, value)| self.callback_expr_calls_throwing_function(value)),
            CallbackExprKind::Index { receiver, .. }
            | CallbackExprKind::Field { receiver, .. }
            | CallbackExprKind::HasField { receiver, .. }
            | CallbackExprKind::FieldTruthy { receiver, .. }
            | CallbackExprKind::UnknownIs {
                value: receiver, ..
            }
            | CallbackExprKind::TypeofValue {
                value: receiver, ..
            } => self.callback_expr_calls_throwing_function(receiver),
            CallbackExprKind::DynamicIndex { receiver, index }
            | CallbackExprKind::HasDynamicField {
                receiver,
                field: index,
            } => {
                self.callback_expr_calls_throwing_function(receiver)
                    || self.callback_expr_calls_throwing_function(index)
            }
            CallbackExprKind::AssignCapture { value, .. } => {
                self.callback_expr_calls_throwing_function(value)
            }
            CallbackExprKind::Sequence { effects, result } => {
                effects
                    .iter()
                    .any(|effect| self.callback_expr_calls_throwing_function(effect))
                    || self.callback_expr_calls_throwing_function(result)
            }
            CallbackExprKind::Literal(_)
            | CallbackExprKind::Param(_)
            | CallbackExprKind::Capture(_)
            | CallbackExprKind::Throw { .. } => false,
        }
    }

    /// Emit a boolean truthiness projection for a concrete class field.
    ///
    /// Optional boolean fields are common in TypeScript option bags. Rust's
    /// `Option::is_some_and` callback must return `bool`, so `Option<bool>`
    /// fields are collapsed with `unwrap_or(false)` at the projection site.
    fn callback_class_field_truthiness_text(
        &self,
        receiver_ty: TypeId,
        field: Symbol,
        receiver_text: &str,
    ) -> Result<String, EmitError> {
        let field_name = sanitize_ident(self.symbol_name(field)?);
        let Some(fields) = self.structural_record_fields(receiver_ty) else {
            return Ok(format!("{receiver_text}.{field_name}"));
        };
        let field_ty = fields
            .iter()
            .find(|candidate| candidate.name == field)
            .map(|candidate| candidate.ty);
        if let Some(structural_field_ty) = field_ty
            && let Some(Type::Optional(inner)) = self.mir.types.get(structural_field_ty)
            && self.mir.types.get(*inner) == Some(&Type::Bool)
        {
            return Ok(format!(
                "{receiver_text}.{field_name}.clone().unwrap_or(false)"
            ));
        }
        Ok(format!("{receiver_text}.{field_name}"))
    }

    /// Emits `Array.map` for full MIR closures that cannot use callback trees.
    fn list_map_closure_text(
        &self,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("array map destination must be a list"));
        };
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Ok("Default::default()".to_owned());
        };
        let Some(item_param_ty) = function_ty.params.first().copied() else {
            return Ok("Default::default()".to_owned());
        };
        let list_text = self.operand_text(list)?;
        let closure_text = match self.closure_operand_text_for_declared_type(callback) {
            Ok(closure_text) => closure_text,
            Err(_) => self.operand_text(callback)?,
        };
        let item_text = self.value_at_type_text("item.clone()", element_ty, item_param_ty)?;
        let mut call_args = vec![item_text];
        if let Some(index_param_ty) = function_ty.params.get(1).copied() {
            let index_source_ty = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                self.type_id(Type::Int)?
            } else {
                self.type_id(Type::Float)?
            };
            let index_value = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                "index as i64"
            } else {
                "index as f64"
            };
            call_args.push(self.value_at_type_text(
                index_value,
                index_source_ty,
                index_param_ty,
            )?);
        }
        if let Some(array_param_ty) = function_ty.params.get(2).copied() {
            call_args.push(self.value_at_type_text(
                "smelt_array.clone()",
                list_ty,
                array_param_ty,
            )?);
        }
        let call_text = format!("(smelt_callback)({})", call_args.join(", "));
        let value_text =
            self.value_at_type_text(&call_text, function_ty.return_ty, *dest_item_ty)?;
        Ok(format!(
            "{{ let mut smelt_callback = {closure_text}; let smelt_array = {list_text}.clone(); smelt_array.iter().enumerate().map(|(index, item)| {{ {value_text} }}).collect::<Vec<_>>() }}"
        ))
    }

    /// Emits `Array.forEach` for function callbacks without inline callback trees.
    fn list_for_each_closure_text(
        &self,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if dest_ty != self.none_ty {
            return Err(EmitError::new("array forEach destination must be none"));
        }
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Ok("Default::default()".to_owned());
        };
        let Some(item_param_ty) = function_ty.params.first().copied() else {
            return Ok("Default::default()".to_owned());
        };
        let list_text = self.operand_text(list)?;
        let closure_text = match self.closure_operand_text_for_declared_type(callback) {
            Ok(closure_text) => closure_text,
            Err(_) => self.operand_text(callback)?,
        };
        let item_text = self.value_at_type_text("item.clone()", element_ty, item_param_ty)?;
        let mut call_args = vec![item_text];
        if let Some(index_param_ty) = function_ty.params.get(1).copied() {
            let index_source_ty = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                self.type_id(Type::Int)?
            } else {
                self.type_id(Type::Float)?
            };
            let index_value = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                "index as i64"
            } else {
                "index as f64"
            };
            call_args.push(self.value_at_type_text(
                index_value,
                index_source_ty,
                index_param_ty,
            )?);
        }
        if let Some(array_param_ty) = function_ty.params.get(2).copied() {
            call_args.push(self.value_at_type_text(
                "smelt_array.clone()",
                list_ty,
                array_param_ty,
            )?);
        }
        let call_text = format!("(smelt_callback)({})", call_args.join(", "));
        Ok(format!(
            "{{ let mut smelt_callback = {closure_text}; let smelt_array = {list_text}.clone(); smelt_array.iter().enumerate().for_each(|(index, item)| {{ let _ = {call_text}; }}); () }}"
        ))
    }

    /// Emits `Array.flatMap` for function callbacks without inline callback trees.
    fn list_flat_map_closure_text(
        &self,
        list: &Operand,
        list_ty: TypeId,
        element_ty: TypeId,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let Some(Type::List(dest_item_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("array flatMap destination must be a list"));
        };
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Ok("Default::default()".to_owned());
        };
        let Some(item_param_ty) = function_ty.params.first().copied() else {
            return Ok("Default::default()".to_owned());
        };
        let list_text = self.operand_text(list)?;
        let closure_text = match self.closure_operand_text_for_declared_type(callback) {
            Ok(closure_text) => closure_text,
            Err(_) => self.operand_text(callback)?,
        };
        let item_text = self.value_at_type_text("item.clone()", element_ty, item_param_ty)?;
        let mut call_args = vec![item_text];
        if let Some(index_param_ty) = function_ty.params.get(1).copied() {
            let index_source_ty = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                self.type_id(Type::Int)?
            } else {
                self.type_id(Type::Float)?
            };
            let index_value = if self.mir.types.get(index_param_ty) == Some(&Type::Int) {
                "index as i64"
            } else {
                "index as f64"
            };
            call_args.push(self.value_at_type_text(
                index_value,
                index_source_ty,
                index_param_ty,
            )?);
        }
        if let Some(array_param_ty) = function_ty.params.get(2).copied() {
            call_args.push(self.value_at_type_text(
                "smelt_array.clone()",
                list_ty,
                array_param_ty,
            )?);
        }
        let call_text = format!("(smelt_callback)({})", call_args.join(", "));
        let flattened_text = match self.mir.types.get(function_ty.return_ty) {
            Some(Type::List(callback_item_ty)) => {
                let value_text =
                    self.value_at_type_text("value", *callback_item_ty, *dest_item_ty)?;
                format!("smelt_result.into_iter().map(|value| {value_text}).collect::<Vec<_>>()")
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                let unknown_ty = self.type_id(Type::Unknown)?;
                let value_text = self.value_at_type_text("value", unknown_ty, *dest_item_ty)?;
                format!(
                    "match smelt_result {{ SmeltUnknown::Array(values) => values.into_iter().map(|value| {value_text}).collect::<Vec<_>>(), value => vec![{value_text}] }}"
                )
            }
            _ => {
                let value_text =
                    self.value_at_type_text("smelt_result", function_ty.return_ty, *dest_item_ty)?;
                format!("vec![{value_text}]")
            }
        };
        Ok(format!(
            "{{ let mut smelt_callback = {closure_text}; let smelt_array = {list_text}.clone(); smelt_array.iter().enumerate().flat_map(|(index, item)| {{ let smelt_result = {call_text}; {flattened_text} }}).collect::<Vec<_>>() }}"
        ))
    }

    /// Converts `Array.from({ length }, mapper)` into an indexed Rust loop.
    pub(super) fn list_from_length_map_text(
        &self,
        length: &Operand,
        callback: &Operand,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(length)?),
            Some(Type::Int | Type::Float)
        ) {
            return Err(EmitError::new("Array.from length must be numeric"));
        }
        let length_text = self.operand_text(length)?;
        if let Ok(callback_body) = self.closure_callback_body(callback) {
            if self.mir.types.get(dest_ty) != Some(&Type::List(callback_body.ty)) {
                return Err(EmitError::new(
                    "Array.from mapper destination must be a list of callback results",
                ));
            }
            let callback_text = self.callback_expr_text(callback_body, &["item", "index"])?;
            return Ok(format!(
                "{{ let array_from_length = ({length_text} as f64).max(0.0).floor() as usize; (0..array_from_length).map(|index| {{ let item = SmeltUnknown::Null; let index = index as f64; {callback_text} }}).collect::<Vec<_>>() }}"
            ));
        }
        let Some(Type::Function(function_ty)) = self.mir.types.get(self.operand_ty(callback)?)
        else {
            return Ok("Default::default()".to_owned());
        };
        if self.mir.types.get(dest_ty) != Some(&Type::List(function_ty.return_ty)) {
            return Err(EmitError::new(
                "Array.from mapper destination must be a list of callback results",
            ));
        }
        let closure_text = self.closure_operand_text_for_declared_type(callback)?;
        let unknown_ty = self.type_id(Type::Unknown)?;
        let float_ty = self.type_id(Type::Float)?;
        let mut call_args = Vec::new();
        if let Some(item_param_ty) = function_ty.params.first().copied() {
            call_args.push(self.value_at_type_text(
                "SmeltUnknown::Null",
                unknown_ty,
                item_param_ty,
            )?);
        }
        if let Some(index_param_ty) = function_ty.params.get(1).copied() {
            call_args.push(self.value_at_type_text("index as f64", float_ty, index_param_ty)?);
        }
        let call_text = format!("(smelt_callback)({})", call_args.join(", "));
        Ok(format!(
            "{{ let mut smelt_callback = {closure_text}; let array_from_length = ({length_text} as f64).max(0.0).floor() as usize; (0..array_from_length).map(|index| {call_text}).collect::<Vec<_>>() }}"
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
        let legacy_callback_body = self.closure_callback_body(callback).ok();
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
        if let Some(legacy_body) = legacy_callback_body
            && legacy_body.ty != dest_ty
        {
            return Err(EmitError::new(
                "array reduce initial value and callback result must match the destination type",
            ));
        }
        let list_text = self.operand_text(list)?;
        let callback_text = if let Some(legacy_body) = legacy_callback_body {
            let legacy_text =
                self.callback_expr_text(legacy_body, &["acc", "item", "index", "array"])?;
            if self.callback_text_is_closure_literal(&legacy_text) {
                let callback_closure = match self.closure_operand_text_for_declared_type(callback) {
                    Ok(callback_closure) => callback_closure,
                    Err(_) => return Ok("Default::default()".to_owned()),
                };
                format!(
                    "{{ let smelt_callback = {callback_closure}; (smelt_callback)(acc, item, index, array) }}"
                )
            } else {
                legacy_text
            }
        } else {
            let callback_closure = match self.closure_operand_text_for_declared_type(callback) {
                Ok(callback_closure) => callback_closure,
                Err(_) => return Ok("Default::default()".to_owned()),
            };
            format!(
                "{{ let smelt_callback = {callback_closure}; (smelt_callback)(acc, item, index, array) }}"
            )
        };
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
        self.closure_text_with_extra_params(id, 0, None)
    }

    /// Converts a MIR closure into a Rust closure literal shaped for `dest_ty`.
    pub(super) fn closure_text_for_type(
        &self,
        id: smelt_mir::ClosureId,
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        let extra_params = match self.mir.types.get(dest_ty) {
            Some(Type::Function(function)) => {
                let closure = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    .ok_or_else(|| {
                        EmitError::new("closure rvalue references an unknown closure")
                    })?;
                function.params.len().saturating_sub(closure.params.len())
            }
            _ => 0,
        };
        let has_captures = !self
            .mir
            .closures
            .get(id_index(id.0, "closure id does not fit usize")?)
            .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?
            .captures
            .is_empty();
        let captures_borrowed_callback = self
            .mir
            .closures
            .get(id_index(id.0, "closure id does not fit usize")?)
            .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?
            .captures
            .iter()
            .any(|capture| {
                self.capture_is_borrowed_callback_param(capture.source_local)
                    .unwrap_or(false)
                    || self
                        .capture_symbol_is_borrowed_callback_param(capture.symbol, capture.ty)
                        .unwrap_or(false)
            });
        if captures_borrowed_callback
            && self
                .mir
                .closures
                .get(id_index(id.0, "closure id does not fit usize")?)
                .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?
                .escapes
            && matches!(self.mir.types.get(dest_ty), Some(Type::Function(_)))
        {
            return self.default_value(dest_ty);
        }
        let target_return_ty = match self.mir.types.get(dest_ty) {
            Some(Type::Function(function)) => Some(function.return_ty),
            _ => None,
        };
        let closure = self.closure_text_with_extra_params(id, extra_params, target_return_ty)?;
        if matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))) {
            let adjusted_closure = if !has_captures
                || closure.starts_with("move ")
                || closure.starts_with('{')
            {
                closure
            } else {
                let source_closure = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    .ok_or_else(|| {
                        EmitError::new("closure rvalue references an unknown closure")
                    })?;
                let mut cloned_captures = HashSet::new();
                let mut shared_replacements = Vec::new();
                let capture_prelude = source_closure
                    .captures
                    .iter()
                    .filter_map(|capture| {
                        let local = self.local_decl(capture.source_local).ok()?;
                        if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                            && matches!(local.kind, LocalKind::Param { .. })
                        {
                            return None;
                        }
                        if !matches!(local.kind, LocalKind::Param { .. })
                            && !self
                                .declared_locals
                                .borrow()
                                .contains(&capture.source_local)
                        {
                            return None;
                        }
                        let name = self.local_name(capture.source_local).ok()?.to_owned();
                        if name.starts_with("(*smelt_capture_") {
                            return None;
                        }
                        cloned_captures.insert(name.clone()).then(|| {
                            if self.closure_capture_needs_shared_access(source_closure, capture)
                                || self.local_uses_shared_capture_storage(capture.source_local)
                            {
                                shared_replacements.push((
                                    name.clone(),
                                    format!("(*smelt_capture_{name}.borrow_mut())"),
                                ));
                                return format!(
                                    "let smelt_capture_{name} = smelt_capture_{name}.clone();"
                                );
                            }
                            let mutability = if self.mutable_locals.contains(&capture.source_local)
                                || source_closure
                                    .captures
                                    .iter()
                                    .find(|candidate| {
                                        candidate.source_local == capture.source_local
                                    })
                                    .is_some_and(|candidate| {
                                        self.closure_capture_body_writes(source_closure, candidate)
                                    })
                                || matches!(
                                    self.mir.types.get(local.ty),
                                    Some(Type::List(_) | Type::Set(_) | Type::Dict(_, _))
                                ) {
                                "mut "
                            } else {
                                ""
                            };
                            format!("let {mutability}{name} = {name}.clone();")
                        })
                    })
                    .collect::<Vec<_>>();
                let adjusted_closure = replace_shared_capture_uses(closure, &shared_replacements);
                if capture_prelude.is_empty() {
                    format!("move {adjusted_closure}")
                } else {
                    format!(
                        "{{\n    {}\n    move {adjusted_closure}\n}}",
                        capture_prelude.join("\n    ")
                    )
                }
            };
            let callback_text = format!("::std::rc::Rc::new({adjusted_closure})");
            if let Some(Type::Function(function)) = self.mir.types.get(dest_ty)
                && self.is_erased_unknown_rest_function(function)
                && !function.may_throw
            {
                let source_closure = self
                    .mir
                    .closures
                    .get(id_index(id.0, "closure id does not fit usize")?)
                    .ok_or_else(|| {
                        EmitError::new("closure rvalue references an unknown closure")
                    })?;
                let params = source_closure
                    .params
                    .iter()
                    .map(|param| {
                        let local_index =
                            id_index(param.0, "closure param index does not fit usize")?;
                        source_closure
                            .locals
                            .get(local_index)
                            .map(|local| local.ty)
                            .ok_or_else(|| EmitError::new("closure param has no local declaration"))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?;
                let source_function = FunctionType {
                    params,
                    rest: source_closure.rest,
                    required_params: source_closure.required_params,
                    mutable_params: Vec::new(),
                    return_ty: source_closure.return_ty,
                    is_async: false,
                    may_throw: source_closure.can_throw,
                };
                let args = self.function_args_from_smelt_args_text(&source_function)?;
                let call = if source_closure.can_throw {
                    format!(
                        "(smelt_callback)({args}).unwrap_or_else(|error| panic!(\"{{}}\", error))"
                    )
                } else {
                    format!("(smelt_callback)({args})")
                };
                let unknown_ty = self.type_id(Type::Unknown)?;
                let return_text =
                    if self.mir.types.get(source_closure.return_ty) == Some(&Type::None) {
                        format!("{{ {call}; SmeltUnknown::Null }}")
                    } else {
                        self.value_at_type_text(&call, source_closure.return_ty, unknown_ty)?
                    };
                let length = source_closure
                    .required_params
                    .unwrap_or_else(|| source_closure.rest.unwrap_or(source_closure.params.len()));
                return Ok(format!(
                    "SmeltErasedFunction {{ callback: {{ let smelt_callback = ::std::rc::Rc::new({adjusted_closure}); ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {return_text}) }}, length: {length}.0, object: None }}"
                ));
            }
            return Ok(callback_text);
        }
        Ok(closure)
    }

    /// Emits ignored trailing parameters when a JS callback accepts fewer
    /// arguments than the contextual function type provides.
    fn closure_text_with_extra_params(
        &self,
        id: smelt_mir::ClosureId,
        extra_params: usize,
        return_override: Option<TypeId>,
    ) -> Result<String, EmitError> {
        let closure = self
            .mir
            .closures
            .get(usize::try_from(id.0).unwrap_or(usize::MAX))
            .ok_or_else(|| EmitError::new("closure rvalue references an unknown closure"))?;
        if closure.escapes
            && closure.captures.iter().any(|capture| {
                self.capture_is_borrowed_callback_param(capture.source_local)
                    .unwrap_or(false)
                    || self
                        .capture_symbol_is_borrowed_callback_param(capture.symbol, capture.ty)
                        .unwrap_or(false)
            })
        {
            let return_ty = return_override.unwrap_or(closure.return_ty);
            let mut param_decls = closure
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    let local_index = id_index(param.0, "closure param index does not fit usize")?;
                    let local = closure
                        .locals
                        .get(local_index)
                        .ok_or_else(|| EmitError::new("closure param has no local declaration"))?;
                    Ok(format!(
                        "arg{index}: {}",
                        self.type_text_with_impl_trait(local.ty, false)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?;
            param_decls.extend((0..extra_params).map(|index| format!("_arg{index}: SmeltUnknown")));
            return Ok(format!(
                "|{}| -> {} {{ {} }}",
                param_decls.join(", "),
                self.type_text_with_impl_trait(return_ty, false)?,
                self.default_value(return_ty)?
            ));
        }
        let body = if let Some(callback) = closure.callback_body.as_ref() {
            let mut param_names = closure
                .params
                .iter()
                .enumerate()
                .map(|(index, _)| format!("arg{index}"))
                .collect::<Vec<_>>();
            param_names.extend((0..extra_params).map(|index| format!("_arg{index}")));
            let param_refs = param_names.iter().map(String::as_str).collect::<Vec<_>>();
            let return_ty = return_override.unwrap_or(closure.return_ty);
            let raw_body_expr =
                self.callback_expr_as_type_text(callback, return_ty, &param_refs)?;
            let body_expr = if closure.can_throw {
                format!("Ok::<_, Box<dyn std::error::Error>>({raw_body_expr})")
            } else {
                raw_body_expr
            };
            let body = if matches!(self.mir.types.get(return_ty), Some(Type::Future(_))) {
                format!("Box::pin(async move {{ {body_expr} }})")
            } else {
                body_expr
            };
            let mut param_decls = closure
                .params
                .iter()
                .enumerate()
                .map(|(index, param)| {
                    let local_index = id_index(param.0, "closure param index does not fit usize")?;
                    let local = closure
                        .locals
                        .get(local_index)
                        .ok_or_else(|| EmitError::new("closure param has no local declaration"))?;
                    Ok(format!(
                        "arg{index}: {}",
                        self.type_text_with_impl_trait(local.ty, false)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?;
            param_decls.extend((0..extra_params).map(|index| format!("_arg{index}: SmeltUnknown")));
            format!(
                "|{}| -> {} {{ {body} }}",
                param_decls.join(", "),
                self.closure_return_type_text(return_ty, closure.can_throw)?
            )
        } else {
            let mut closure_locals = closure.locals.clone();
            let fallback_span = closure_locals.first().map_or(
                Span {
                    file: FileId(0),
                    start: 0,
                    end: 0,
                },
                |local| local.span,
            );
            for capture in &closure.captures {
                for captured_local in [Some(capture.source_local), capture.target_local]
                    .into_iter()
                    .flatten()
                {
                    let index = id_index(captured_local.0, "local index does not fit usize")?;
                    if closure_locals.len() <= index {
                        closure_locals.resize(
                            index.saturating_add(1),
                            LocalDecl {
                                ty: capture.ty,
                                kind: LocalKind::Temp,
                                span: fallback_span,
                            },
                        );
                        if let Some(local) = closure_locals.get_mut(index) {
                            local.ty = capture.ty;
                        }
                    }
                }
            }
            let function = MirFunction {
                id: FuncId(u32::MAX),
                name: Symbol(u32::MAX),
                origin: HirOrigin::Body(smelt_hir::BodyId(u32::MAX)),
                is_async: false,
                is_test: false,
                can_throw: closure.can_throw,
                params: closure.params.clone(),
                rest: closure.rest,
                return_ty: return_override.unwrap_or(closure.return_ty),
                locals: closure_locals,
                blocks: closure.blocks.clone(),
                entry: closure.entry,
            };
            let mut emitter = FunctionEmitter::new(self.mir, self.context, &function)?;
            for (index, param) in closure.params.iter().enumerate() {
                emitter.names.insert(*param, format!("closure_arg_{index}"));
            }
            for capture in &closure.captures {
                if let Some(target) = capture.target_local {
                    let source = self.local_decl(capture.source_local)?;
                    let source_name = self.local_name(capture.source_local)?.to_owned();
                    if matches!(self.mir.types.get(source.ty), Some(Type::Function(_)))
                        && matches!(source.kind, LocalKind::Param { .. })
                        && !self.function_parameter_requires_owned(capture.source_local)?
                    {
                        emitter.borrowed_callback_names.insert(source_name.clone());
                    }
                    let capture_name = if self.closure_capture_needs_shared_access(closure, capture)
                        || self.local_uses_shared_capture_storage(capture.source_local)
                    {
                        format!("(*smelt_capture_{source_name}.borrow_mut())")
                    } else {
                        source_name
                    };
                    emitter.names.insert(target, capture_name);
                    emitter.mark_local_declared(target);
                }
            }
            let mut params = closure
                .params
                .iter()
                .map(|param| {
                    let local = emitter.local_decl(*param)?;
                    let mutability = if emitter.local_binding_needs_mut(*param) {
                        "mut "
                    } else {
                        ""
                    };
                    Ok(format!(
                        "{mutability}{}: {}",
                        emitter.local_name(*param)?,
                        emitter.type_text_with_impl_trait(local.ty, false)?
                    ))
                })
                .collect::<Result<Vec<_>, EmitError>>()?;
            params.extend((0..extra_params).map(|index| format!("_arg{index}")));
            let params_text = params.join(", ");
            let mut body_text = String::new();
            emitter.emit_mutable_local_preludes(&mut body_text)?;
            emitter.emit_closure_block(emitter.entry_block()?, &mut body_text)?;
            let returns_future = matches!(
                emitter.mir.types.get(function.return_ty),
                Some(Type::Future(_))
            );
            let awaits_inside_body = body_text.contains(".await")
                && !body_text
                    .contains("let _smelt_tmp_1: ::std::pin::Pin<Box<dyn ::std::future::Future");
            if returns_future || awaits_inside_body {
                let output_ty = match emitter.mir.types.get(function.return_ty) {
                    Some(Type::Future(item)) => *item,
                    _ => function.return_ty,
                };
                let output_text = emitter.type_text_with_impl_trait(output_ty, false)?;
                let return_ty = if closure.can_throw {
                    format!("Result<{output_text}, Box<dyn std::error::Error>>")
                } else {
                    output_text.clone()
                };
                let async_value_needs_await = async_body_returns_future_value(&body_text);
                let return_value = if closure.can_throw && async_value_needs_await {
                    if matches!(
                        emitter.mir.types.get(output_ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || emitter.is_erased_class_type(output_ty)
                    {
                        "let smelt_async_output = smelt_async_value?.await; Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_async_output.into_smelt_unknown())".to_owned()
                    } else {
                        format!(
                            "let smelt_async_output = smelt_async_value?.await; Ok::<{output_text}, Box<dyn std::error::Error>>(smelt_async_output)"
                        )
                    }
                } else if closure.can_throw {
                    if matches!(
                        emitter.mir.types.get(output_ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || emitter.is_erased_class_type(output_ty)
                    {
                        "Ok::<SmeltUnknown, Box<dyn std::error::Error>>(smelt_async_value?.into_smelt_unknown())".to_owned()
                    } else {
                        format!(
                            "Ok::<{output_text}, Box<dyn std::error::Error>>(smelt_async_value?)"
                        )
                    }
                } else if matches!(
                    emitter.mir.types.get(output_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || emitter.is_erased_class_type(output_ty)
                {
                    if async_value_needs_await {
                        "let smelt_async_output = smelt_async_value.await; smelt_async_output.into_smelt_unknown()".to_owned()
                    } else {
                        "smelt_async_value.into_smelt_unknown()".to_owned()
                    }
                } else if async_value_needs_await {
                    "let smelt_async_output = smelt_async_value.await; smelt_async_output"
                        .to_owned()
                } else {
                    "smelt_async_value".to_owned()
                };
                let mut cloned_async_captures = HashSet::new();
                let async_capture_lines = closure
                    .captures
                    .iter()
                    .filter_map(|capture| {
                        let local = self.local_decl(capture.source_local).ok()?;
                        if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                            && matches!(local.kind, LocalKind::Param { .. })
                            && !self
                                .function_parameter_requires_owned(capture.source_local)
                                .ok()?
                        {
                            return None;
                        }
                        let name = self.local_name(capture.source_local).ok()?.to_owned();
                        cloned_async_captures
                            .insert(name.clone())
                            .then(|| format!("let {name} = {name}.clone();"))
                    })
                    .collect::<Vec<_>>();
                let async_capture_prelude = if async_capture_lines.is_empty() {
                    String::new()
                } else {
                    format!("{} ", async_capture_lines.join(" "))
                };
                format!(
                    "|{params_text}| {{ {async_capture_prelude}Box::pin(async move {{\n        let smelt_async_value = {{\n{body_text}        }};\n        {return_value}\n    }}) as ::std::pin::Pin<Box<dyn ::std::future::Future<Output = {return_ty}>>> }}"
                )
            } else {
                let wrapped_body_text = if closure
                    .captures
                    .iter()
                    .any(|capture| self.closure_capture_needs_shared_access(closure, capture))
                {
                    format!("unsafe {{\n{body_text}    }}\n")
                } else {
                    body_text
                };
                format!("|{params_text}| {{\n{wrapped_body_text}    }}")
            }
        };
        let capture_prefix = if closure.escapes
            || matches!(self.mir.types.get(closure.return_ty), Some(Type::Future(_)))
            || !closure.captures.is_empty()
            || closure.captures.iter().any(|capture| {
                capture.mode == smelt_hir::CaptureMode::ByValue
                    && !self
                        .capture_is_borrowed_callback_param(capture.source_local)
                        .unwrap_or(false)
                    && !self
                        .capture_symbol_is_borrowed_callback_param(capture.symbol, capture.ty)
                        .unwrap_or(false)
            }) {
            "move "
        } else {
            ""
        };
        if capture_prefix.is_empty() {
            return Ok(body);
        }
        let mut cloned_captures = HashSet::new();
        let mut shared_replacements = Vec::new();
        let capture_prelude = closure
            .captures
            .iter()
            .filter_map(|capture| {
                let local = self.local_decl(capture.source_local).ok()?;
                if matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                    && matches!(local.kind, LocalKind::Param { .. })
                    && !self
                        .function_parameter_requires_owned(capture.source_local)
                        .ok()?
                {
                    return None;
                }
                let name = self.local_name(capture.source_local).ok()?.to_owned();
                if name.starts_with("(*smelt_capture_") {
                    return None;
                }
                cloned_captures.insert(name.clone()).then(|| {
                    if self.closure_capture_needs_shared_access(closure, capture)
                        || self.local_uses_shared_capture_storage(capture.source_local)
                    {
                        shared_replacements.push((
                            name.clone(),
                            format!("(*smelt_capture_{name}.borrow_mut())"),
                        ));
                        return format!("let smelt_capture_{name} = smelt_capture_{name}.clone();");
                    }
                    let mutability = if self.mutable_locals.contains(&capture.source_local)
                        || self.closure_capture_body_writes(closure, capture)
                        || matches!(
                            self.mir.types.get(local.ty),
                            Some(Type::List(_) | Type::Set(_) | Type::Dict(_, _))
                        ) {
                        "mut "
                    } else {
                        ""
                    };
                    format!("let {mutability}{name} = {name}.clone();")
                })
            })
            .collect::<Vec<_>>();
        let adjusted_body = replace_shared_capture_uses(body, &shared_replacements);
        if capture_prelude.is_empty() {
            Ok(format!("{capture_prefix}{adjusted_body}"))
        } else {
            Ok(format!(
                "{{\n    {}\n    {capture_prefix}{adjusted_body}\n}}",
                capture_prelude.join("\n    ")
            ))
        }
    }

    /// Emits a closure block with return terminators scoped to the closure body.
    pub(super) fn emit_closure_block(
        &self,
        block: &BasicBlock,
        out: &mut String,
    ) -> Result<(), EmitError> {
        self.emit_closure_block_inner(block, out, &mut Vec::new(), None)
    }

    /// Emits a closure block while guarding against unstructured CFG cycles.
    fn emit_closure_block_inner(
        &self,
        block: &BasicBlock,
        out: &mut String,
        active: &mut Vec<*const BasicBlock>,
        stop: Option<smelt_mir::BlockId>,
    ) -> Result<(), EmitError> {
        if Some(block.id) == stop {
            return Ok(());
        }
        let block_ptr = std::ptr::from_ref(block);
        if active.contains(&block_ptr) {
            out.push_str("    panic!(\"recursive closure control flow is not structured yet\")\n");
            return Ok(());
        }
        active.push(block_ptr);
        for statement in &block.statements {
            self.emit_statement(statement, out)?;
        }
        let Some(terminator) = &block.terminator else {
            return Err(EmitError::new("closure basic block has no terminator"));
        };
        let result = match terminator {
            Terminator::Return(operand) => {
                if self.function.return_ty == self.none_ty {
                    if !matches!(operand, Operand::Const(Constant::None))
                        && self.operand_ty(operand)? != self.none_ty
                    {
                        out.push_str(&format!("    {};\n", self.operand_text(operand)?));
                    }
                    if self.function.can_throw {
                        out.push_str("    Ok::<(), Box<dyn std::error::Error>>(())\n");
                    } else {
                        out.push_str("    ()\n");
                    }
                } else {
                    let return_ty = match self.mir.types.get(self.function.return_ty) {
                        Some(Type::Future(item)) => *item,
                        _ => self.function.return_ty,
                    };
                    let value = self.value_at_type(operand, return_ty)?;
                    if self.function.can_throw {
                        let return_ty_text = self.type_text_with_impl_trait(return_ty, false)?;
                        out.push_str(&format!(
                            "    Ok::<{return_ty_text}, Box<dyn std::error::Error>>({value})\n"
                        ));
                    } else {
                        out.push_str(&format!("    {value}\n"));
                    }
                }
                Ok(())
            }
            Terminator::Goto(target) => {
                if Some(*target) == stop {
                    Ok(())
                } else {
                    self.emit_closure_block_inner(self.block(*target)?, out, active, stop)
                }
            }
            Terminator::Call {
                callee,
                args,
                dest,
                target,
                unwind: _,
            } => {
                let local = self.local_decl(*dest)?;
                let call_text = self.closure_call_text_for_dest(callee, args, local.ty)?;
                let name = self.local_name(*dest)?;
                out.push_str(&format!(
                    "    let {name}: {} = {call_text};\n",
                    self.type_text_with_impl_trait(local.ty, false)?
                ));
                self.mark_local_declared(*dest);
                self.emit_closure_block_inner(self.block(*target)?, out, active, stop)
            }
            Terminator::Await {
                future,
                dest,
                target,
                unwind: _,
            } => {
                let local = self.local_decl(*dest)?;
                let name = self.local_name(*dest)?;
                let value = format!("{}.await", self.await_operand_text(future)?);
                out.push_str(&format!(
                    "    let {name}: {} = {value};\n",
                    self.type_text_with_impl_trait(local.ty, false)?
                ));
                self.mark_local_declared(*dest);
                self.emit_closure_block_inner(self.block(*target)?, out, active, stop)
            }
            Terminator::Switch {
                cond,
                then_block,
                else_block,
            } => {
                let branch_declared = self.declared_locals_snapshot();
                out.push_str(&format!("    if {} {{\n", self.truthy_operand_text(cond)?));
                self.emit_closure_block_inner(self.block(*then_block)?, out, active, stop)?;
                out.push_str("    } else {\n");
                self.restore_declared_locals(branch_declared.clone());
                self.emit_closure_block_inner(self.block(*else_block)?, out, active, stop)?;
                out.push_str("    }\n");
                self.restore_declared_locals(branch_declared);
                Ok(())
            }
            Terminator::Match {
                scrutinee,
                arms,
                default,
            } => self.emit_closure_match(scrutinee, arms, *default, out, active, stop),
            Terminator::Throw(operand) => {
                out.push_str(&format!(
                    "    return Err(std::io::Error::new(std::io::ErrorKind::Other, format!(\"{{}}\", {})).into());\n",
                    self.operand_text(operand)?
                ));
                Ok(())
            }
            Terminator::Unreachable => {
                out.push_str("    unreachable!()\n");
                Ok(())
            }
        };
        active.pop();
        result
    }

    /// Emits a MIR match inside a Rust closure body.
    ///
    /// Closure blocks can end as Rust tail expressions, unlike ordinary function
    /// blocks that usually emit explicit `return` statements. This helper keeps
    /// shared match join blocks outside the generated `match` expression and
    /// lets non-joining matches remain the closure tail expression.
    fn emit_closure_match(
        &self,
        scrutinee: &Operand,
        arms: &[smelt_mir::MatchArm],
        default: Option<smelt_mir::BlockId>,
        out: &mut String,
        active: &mut Vec<*const BasicBlock>,
        stop: Option<smelt_mir::BlockId>,
    ) -> Result<(), EmitError> {
        let join = self.match_join(arms, default)?;
        let scrutinee_text = self.match_scrutinee_text(scrutinee)?;
        out.push_str(&format!("    match {scrutinee_text} {{\n"));
        let match_declared = self.declared_locals_snapshot();
        for arm in arms {
            out.push_str(&format!(
                "        {} => {{\n",
                self.match_label_text(&arm.label)
            ));
            self.emit_closure_block_inner(self.block(arm.target)?, out, active, join)?;
            out.push_str("        }\n");
            self.restore_declared_locals(match_declared.clone());
        }
        if let Some(default_block) = default {
            out.push_str("        _ => {\n");
            self.emit_closure_block_inner(self.block(default_block)?, out, active, join)?;
            out.push_str("        }\n");
            self.restore_declared_locals(match_declared);
        } else {
            out.push_str("        _ => {}\n");
        }
        if join.is_some() {
            out.push_str("    };\n");
        } else {
            out.push_str("    }\n");
        }
        if let Some(join_block) = join {
            self.emit_closure_block_inner(self.block(join_block)?, out, active, stop)?;
        }
        Ok(())
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

    /// Resolve a callback operand to the Rust closure expression that constructed it.
    pub(super) fn closure_operand_text(&self, operand: &Operand) -> Result<String, EmitError> {
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
                    return self.closure_text(*id);
                }
            }
        }
        Err(EmitError::new(
            "list callback closure construction was not found",
        ))
    }

    /// Resolve a local to the closure local it aliases, following simple copy assignments.
    fn closure_source_local(&self, local: LocalId) -> LocalId {
        let mut current = local;
        for _ in 0_u8..8 {
            let mut next = None;
            for block in &self.function.blocks {
                for statement in &block.statements {
                    if let Statement::Assign {
                        dest,
                        value:
                            Rvalue::Use(
                                Operand::Copy(Place::Local(source))
                                | Operand::Move(Place::Local(source)),
                            ),
                    } = statement
                        && *dest == current
                    {
                        next = Some(*source);
                    }
                }
            }
            let Some(source) = next else {
                return current;
            };
            if source == current {
                return current;
            }
            current = source;
        }
        current
    }

    /// Resolve a callback operand to a Rust closure shaped like its function type.
    ///
    /// JavaScript array callbacks may declare fewer formal parameters than the
    /// runtime supplies. The declared operand type records the contextual
    /// callback shape, so this path asks closure emission to add ignored
    /// trailing parameters before iterator code calls the callback with
    /// `(item, index, array)` or `(acc, item, index, array)`.
    fn closure_operand_text_for_declared_type(
        &self,
        operand: &Operand,
    ) -> Result<String, EmitError> {
        let local = match operand {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => *local,
            _ => {
                return Err(EmitError::new(
                    "list callback must be a non-escaping closure local",
                ));
            }
        };
        let dest_ty = self.operand_ty(operand)?;
        let source_local = self.closure_source_local(local);
        for block in &self.function.blocks {
            for statement in &block.statements {
                if let Statement::Assign {
                    dest,
                    value: Rvalue::Closure { id, .. },
                } = statement
                    && *dest == source_local
                {
                    return self.closure_text_for_type(*id, dest_ty);
                }
            }
        }
        Err(EmitError::new(
            "list callback closure construction was not found",
        ))
    }

    /// Return true when legacy callback expression text is really a closure.
    fn callback_text_is_closure_literal(&self, text: &str) -> bool {
        let trimmed = text.trim_start();
        trimmed.starts_with('|') || trimmed.starts_with("move |") || trimmed.starts_with("{\n")
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
                let local_id = LocalId(local.0);
                let local_ty = self.local_decl(local_id).ok().map(|decl| decl.ty);
                self.local_name(LocalId(local.0)).map(|name| {
                    if self.is_borrowed_callback_capture_name(name)
                        || local_ty.is_some_and(|ty| {
                            matches!(self.mir.types.get(ty), Some(Type::Future(_)))
                        })
                    {
                        name.to_owned()
                    } else {
                        format!("{name}.clone()")
                    }
                })
            }
            smelt_hir::CallbackExprKind::Function(function) => {
                self.callback_function_rust_name(*function)
            }
            smelt_hir::CallbackExprKind::FunctionTableLookup { key, cases } => {
                let key_text = self.callback_expr_text(key, params)?;
                let cases_text = cases
                    .iter()
                    .map(|(case_key, function)| {
                        let function_text = self.callback_function_rust_name(*function)?;
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
                if value_text.contains(target_text) {
                    return Ok(format!(
                        "{{ let smelt_next_value = {value_text}; {target_text} = smelt_next_value; {target_text}.clone() }}"
                    ));
                }
                Ok(format!(
                    "{{ {target_text} = {value_text}; {target_text}.clone() }}"
                ))
            }
            smelt_hir::CallbackExprKind::Literal(literal) => Ok(hir_literal_text(literal)),
            smelt_hir::CallbackExprKind::ListLit(items) => {
                if matches!(
                    self.mir.types.get(expr.ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) || self.is_erased_class_type(expr.ty)
                {
                    let items_text = items
                        .iter()
                        .map(|item| {
                            let mut text = self.callback_expr_text(item, params)?;
                            if matches!(
                                item.kind,
                                smelt_hir::CallbackExprKind::Param(_)
                                    | smelt_hir::CallbackExprKind::Capture(_)
                            ) && !self.type_contains_function(item.ty)
                            {
                                text = format!("{text}.clone()");
                            }
                            if self.callback_expr_renders_numeric(item) {
                                Ok(format!("SmeltUnknown::Number(({text}) as f64)"))
                            } else {
                                self.erase_value_text(&text, item.ty)
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    return Ok(format!("SmeltUnknown::Array(vec![{items_text}].into())"));
                }
                let list_item_ty = match self.mir.types.get(expr.ty) {
                    Some(Type::List(item_ty)) => Some(*item_ty),
                    _ => None,
                };
                let items_text = items
                    .iter()
                    .map(|item| {
                        if let Some(target_item_ty) = list_item_ty {
                            self.callback_expr_as_type_text(item, target_item_ty, params)
                        } else {
                            let text = self.callback_expr_text(item, params)?;
                            if matches!(
                                item.kind,
                                smelt_hir::CallbackExprKind::Param(_)
                                    | smelt_hir::CallbackExprKind::Capture(_)
                            ) && !self.type_contains_function(item.ty)
                            {
                                Ok(format!("{text}.clone()"))
                            } else {
                                Ok(text)
                            }
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!("vec![{items_text}]"))
            }
            smelt_hir::CallbackExprKind::Sequence { effects, result } => {
                let effects_text = effects
                    .iter()
                    .map(|effect| {
                        self.callback_expr_text(effect, params)
                            .map(|text| format!("let _ = {text};"))
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(" ");
                let result_text = self.callback_expr_text(result, params)?;
                Ok(format!("{{ {effects_text} {result_text} }}"))
            }
            smelt_hir::CallbackExprKind::DictLit(entries) => match self.mir.types.get(expr.ty) {
                Some(Type::Dict(key_ty, value_ty)) => {
                    let entries_text = entries
                        .iter()
                        .map(|(key, value)| {
                            let key_text = self.symbol_source_name(*key)?;
                            let value_text =
                                self.callback_expr_as_type_text(value, *value_ty, params)?;
                            Ok(format!("({key_text:?}.to_owned(), {value_text})"))
                        })
                        .collect::<Result<Vec<_>, EmitError>>()?
                        .join(", ");
                    if self.dict_uses_smelt_record(*key_ty) {
                        Ok(format!("SmeltRecord::from([{entries_text}])"))
                    } else {
                        Ok(format!(
                            "::std::collections::HashMap::from([{entries_text}])"
                        ))
                    }
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
                        "return Err(std::io::Error::new(std::io::ErrorKind::Other, format!(\"{{}}\", {})).into())",
                        self.callback_expr_text(thrown_message, params)?
                    ))
                } else {
                    Ok("return Err(std::io::Error::new(std::io::ErrorKind::Other, \"callback threw\").into())".to_owned())
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
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    | Some(Type::Class { .. })
                        if self.is_erased_class_type(receiver.ty)
                            || matches!(
                                self.mir.types.get(receiver.ty),
                                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                            ) =>
                    {
                        Ok(self.callback_unknown_index_text(
                            &receiver_text,
                            &format!("{index} as i64"),
                            &format!("{:?}.to_owned()", index.to_string()),
                        ))
                    }
                    _ => Ok("Default::default()".to_owned()),
                }
            }
            smelt_hir::CallbackExprKind::DynamicIndex { receiver, index } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                match self.mir.types.get(receiver.ty) {
                    Some(Type::List(_)) => Ok(format!(
                        "{{ let callback_index_receiver = ({receiver_text}).clone(); let callback_index = {{ let len = callback_index_receiver.len() as i64; let index = {} as i64; let normalized = if index < 0 {{ len + index }} else {{ index }}; usize::try_from(normalized).expect(\"negative index out of bounds\") }}; callback_index_receiver.get(callback_index).cloned().expect(\"index out of bounds\") }}",
                        self.callback_expr_text(index, params)?
                    )),
                    Some(Type::String) => Ok(format!(
                        "{{ let callback_index_receiver = ({receiver_text}).clone(); let callback_index = {{ let len = callback_index_receiver.chars().count() as i64; let index = {} as i64; let normalized = if index < 0 {{ len + index }} else {{ index }}; usize::try_from(normalized).expect(\"negative index out of bounds\") }}; callback_index_receiver.chars().nth(callback_index).map(|ch| ch.to_string()).expect(\"index out of bounds\") }}",
                        self.callback_expr_text(index, params)?
                    )),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    | Some(Type::Class { .. })
                        if self.is_erased_class_type(receiver.ty)
                            || matches!(
                                self.mir.types.get(receiver.ty),
                                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                            ) =>
                    {
                        let index_text = self.callback_expr_text(index, params)?;
                        let numeric_index_text = match self.mir.types.get(index.ty) {
                            Some(Type::Int | Type::Float) => index_text.clone(),
                            Some(Type::Bool) => {
                                format!("if {index_text} {{ 1.0 }} else {{ 0.0 }}")
                            }
                            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                            | Some(Type::Class { .. })
                                if self.is_erased_class_type(index.ty)
                                    || matches!(
                                        self.mir.types.get(index.ty),
                                        Some(
                                            Type::Unknown | Type::TypeParam { .. } | Type::Union(_)
                                        )
                                    ) =>
                            {
                                format!(
                                    "match {index_text}.clone() {{ SmeltUnknown::Number(value) => value, SmeltUnknown::String(value) => value.parse::<f64>().unwrap_or(f64::NAN), SmeltUnknown::Bool(value) => if value {{ 1.0 }} else {{ 0.0 }}, SmeltUnknown::Null | SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => f64::NAN }}"
                                )
                            }
                            _ => "f64::NAN".to_owned(),
                        };
                        let key_text = self.property_key_to_string_text(&index_text, index.ty)?;
                        Ok(self.callback_unknown_index_text(
                            &receiver_text,
                            &numeric_index_text,
                            &key_text,
                        ))
                    }
                    Some(Type::Dict(key_ty, value_ty))
                        if self.mir.types.get(*key_ty) == Some(&Type::String) =>
                    {
                        let index_text = self.callback_expr_as_type_text(index, *key_ty, params)?;
                        if self.dict_uses_smelt_record(*key_ty) {
                            Ok(format!(
                                "{receiver_text}.get(&{index_text}).unwrap_or({})",
                                self.default_value(*value_ty)?
                            ))
                        } else {
                            Ok(format!(
                                "{receiver_text}.get(&{index_text}).cloned().unwrap_or({})",
                                self.default_value(*value_ty)?
                            ))
                        }
                    }
                    _ => Ok("Default::default()".to_owned()),
                }
            }
            smelt_hir::CallbackExprKind::Field { receiver, field } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                let field_text = self.symbol_name(*field)?;
                let source_field_text = self.symbol_source_name(*field)?;
                if field_text == "call" {
                    return Ok("Default::default()".to_owned());
                }
                match self.mir.types.get(receiver.ty) {
                    Some(Type::String) => self.string_field_text(&receiver_text, *field),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                        if field_text == "length" =>
                    {
                        Ok(format!(
                            "match &{receiver_text} {{ SmeltUnknown::String(value) => value.chars().count() as f64, SmeltUnknown::Array(value) => value.len() as f64, SmeltUnknown::Object(value) => match smelt_get_object_field(value, \"length\") {{ SmeltUnknown::Number(value) => value, _ => 0.0 }}, _ => 0.0 }}"
                        ))
                    }
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => Ok(format!(
                        "match &{receiver_text} {{ SmeltUnknown::Object(value) => smelt_get_object_field(value, {source_field_text:?}), _ => SmeltUnknown::Null }}"
                    )),
                    Some(Type::Dict(key_ty, value_ty)) => {
                        if self.dict_uses_smelt_record(*key_ty) {
                            Ok(format!(
                                "{receiver_text}.get({source_field_text:?}).unwrap_or({})",
                                self.default_value(*value_ty)?
                            ))
                        } else {
                            Ok(format!(
                                "{receiver_text}.get({source_field_text:?}).cloned().unwrap_or({})",
                                self.default_value(*value_ty)?
                            ))
                        }
                    }
                    Some(Type::Optional(inner)) => match self.mir.types.get(*inner) {
                        Some(Type::Class { .. }) if !self.is_erased_class_type(*inner) => {
                            let default = self
                                .default_value(self.callback_class_field_type(*inner, *field)?)?;
                            Ok(format!(
                                "{receiver_text}.clone().map_or({default}, |value| value.{}.clone())",
                                sanitize_ident(field_text)
                            ))
                        }
                        Some(Type::Dict(key_ty, value_ty))
                            if self.mir.types.get(*key_ty) == Some(&Type::String) =>
                        {
                            let default = self.default_value(*value_ty)?;
                            if self.dict_uses_smelt_record(*key_ty) {
                                Ok(format!(
                                    "{receiver_text}.as_ref().and_then(|value| value.get({source_field_text:?})).unwrap_or({default})"
                                ))
                            } else {
                                Ok(format!(
                                    "{receiver_text}.as_ref().and_then(|value| value.get({source_field_text:?}).cloned()).unwrap_or({default})"
                                ))
                            }
                        }
                        _ => Ok("Default::default()".to_owned()),
                    },
                    Some(Type::Class { .. }) if !self.is_erased_class_type(receiver.ty) => Ok(
                        format!("{receiver_text}.{}.clone()", sanitize_ident(field_text)),
                    ),
                    _ => Ok("Default::default()".to_owned()),
                }
            }
            smelt_hir::CallbackExprKind::HasField { receiver, field } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                let field_text = self.symbol_source_name(*field)?;
                match self.mir.types.get(receiver.ty) {
                    Some(Type::Dict(key, _)) if self.mir.types.get(*key) == Some(&Type::String) => {
                        Ok(format!("{receiver_text}.contains_key({field_text:?})"))
                    }
                    Some(Type::Unknown) if self.callback_field_receiver_is_erased(receiver.ty) => {
                        Ok(format!(
                            "matches!({receiver_text}, SmeltUnknown::Object(ref map) if map.contains_key({field_text:?}))"
                        ))
                    }
                    Some(Type::TypeParam { .. } | Type::Union(_))
                        if self.callback_field_receiver_is_erased(receiver.ty) =>
                    {
                        Ok(format!(
                            "matches!({receiver_text}, SmeltUnknown::Object(ref map) if map.contains_key({field_text:?}))"
                        ))
                    }
                    Some(Type::Class { .. }) if self.is_erased_class_type(receiver.ty) => {
                        Ok(format!(
                            "matches!({receiver_text}, SmeltUnknown::Object(ref map) if map.contains_key({field_text:?}))"
                        ))
                    }
                    Some(Type::Class { .. }) => Ok("true".to_owned()),
                    _ => Ok("false".to_owned()),
                }
            }
            smelt_hir::CallbackExprKind::HasDynamicField { receiver, field } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                let field_text = self.callback_expr_text(field, params)?;
                match self.mir.types.get(receiver.ty) {
                    Some(Type::Dict(key, _)) if self.mir.types.get(*key) == Some(&Type::String) => {
                        Ok(format!("{receiver_text}.contains_key(&{field_text})"))
                    }
                    Some(Type::Unknown) if self.callback_field_receiver_is_erased(receiver.ty) => {
                        Ok(format!(
                            "matches!({receiver_text}, SmeltUnknown::Object(ref map) if map.contains_key(&{field_text}))"
                        ))
                    }
                    Some(Type::TypeParam { .. } | Type::Union(_))
                        if self.callback_field_receiver_is_erased(receiver.ty) =>
                    {
                        Ok(format!(
                            "matches!({receiver_text}, SmeltUnknown::Object(ref map) if map.contains_key(&{field_text}))"
                        ))
                    }
                    Some(Type::Class { .. }) if self.is_erased_class_type(receiver.ty) => {
                        Ok(format!(
                            "matches!({receiver_text}, SmeltUnknown::Object(ref map) if map.contains_key(&{field_text}))"
                        ))
                    }
                    Some(Type::Class { .. }) => Ok("true".to_owned()),
                    _ => Ok("false".to_owned()),
                }
            }
            smelt_hir::CallbackExprKind::FieldTruthy { receiver, field } => {
                let receiver_text = self.callback_expr_text(receiver, params)?;
                let source_field_text = self.symbol_source_name(*field)?;
                match self.mir.types.get(receiver.ty) {
                    Some(Type::Optional(inner)) => match self.mir.types.get(*inner) {
                        Some(Type::Class { .. }) => {
                            let projection =
                                self.callback_class_field_truthiness_text(*inner, *field, "value")?;
                            Ok(format!(
                                "{receiver_text}.as_ref().is_some_and(|value| {projection})"
                            ))
                        }
                        Some(Type::Dict(key, _))
                            if self.mir.types.get(*key) == Some(&Type::String) =>
                        {
                            Ok(format!(
                                "{receiver_text}.as_ref().and_then(|value| value.get({source_field_text:?})).copied().unwrap_or(false)"
                            ))
                        }
                        _ => Err(EmitError::new(
                            "callback optional field truthiness requires an optional class or record receiver",
                        )),
                    },
                    Some(Type::Class { .. }) => self.callback_class_field_truthiness_text(
                        receiver.ty,
                        *field,
                        &receiver_text,
                    ),
                    Some(Type::Dict(key, _)) if self.mir.types.get(*key) == Some(&Type::String) => {
                        Ok(format!(
                            "{receiver_text}.get({source_field_text:?}).copied().unwrap_or(false)"
                        ))
                    }
                    Some(Type::Unknown) => Ok(format!(
                        "matches!({receiver_text}, SmeltUnknown::Object(ref map) if matches!(map.get({source_field_text:?}), Some(SmeltUnknown::Bool(true))))"
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
                if matches!(op, smelt_hir::BinOp::And | smelt_hir::BinOp::Or) {
                    let lhs_text = self.callback_expr_truthy_text(lhs, params)?;
                    let rhs_text = self.callback_expr_truthy_text(rhs, params)?;
                    return Ok(format!(
                        "({lhs_text} {} {rhs_text})",
                        smelt_hir::bin_op_text(*op)
                    ));
                }
                if matches!(op, smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq) {
                    if matches!(self.mir.types.get(lhs.ty), Some(Type::Optional(_)))
                        && self.mir.types.get(rhs.ty) == Some(&Type::None)
                    {
                        let lhs_text = self.callback_expr_text(lhs, params)?;
                        return Ok(match op {
                            smelt_hir::BinOp::Eq | smelt_hir::BinOp::StrictEq => {
                                format!("{lhs_text}.is_none()")
                            }
                            smelt_hir::BinOp::NotEq | smelt_hir::BinOp::StrictNotEq => {
                                format!("{lhs_text}.is_some()")
                            }
                            smelt_hir::BinOp::Add
                            | smelt_hir::BinOp::Sub
                            | smelt_hir::BinOp::Mul
                            | smelt_hir::BinOp::Div
                            | smelt_hir::BinOp::Rem
                            | smelt_hir::BinOp::Lt
                            | smelt_hir::BinOp::Lte
                            | smelt_hir::BinOp::Gt
                            | smelt_hir::BinOp::Gte
                            | smelt_hir::BinOp::And
                            | smelt_hir::BinOp::Or
                            | smelt_hir::BinOp::Shl
                            | smelt_hir::BinOp::Shr
                            | smelt_hir::BinOp::UShr => {
                                return Err(EmitError::new(
                                    "unsupported optional comparison operator",
                                ));
                            }
                        });
                    }
                    if self.mir.types.get(lhs.ty) == Some(&Type::None)
                        && matches!(self.mir.types.get(rhs.ty), Some(Type::Optional(_)))
                    {
                        let rhs_text = self.callback_expr_text(rhs, params)?;
                        return Ok(match op {
                            smelt_hir::BinOp::Eq | smelt_hir::BinOp::StrictEq => {
                                format!("{rhs_text}.is_none()")
                            }
                            smelt_hir::BinOp::NotEq | smelt_hir::BinOp::StrictNotEq => {
                                format!("{rhs_text}.is_some()")
                            }
                            smelt_hir::BinOp::Add
                            | smelt_hir::BinOp::Sub
                            | smelt_hir::BinOp::Mul
                            | smelt_hir::BinOp::Div
                            | smelt_hir::BinOp::Rem
                            | smelt_hir::BinOp::Lt
                            | smelt_hir::BinOp::Lte
                            | smelt_hir::BinOp::Gt
                            | smelt_hir::BinOp::Gte
                            | smelt_hir::BinOp::And
                            | smelt_hir::BinOp::Or
                            | smelt_hir::BinOp::Shl
                            | smelt_hir::BinOp::Shr
                            | smelt_hir::BinOp::UShr => {
                                return Err(EmitError::new(
                                    "unsupported optional comparison operator",
                                ));
                            }
                        });
                    }
                }
                let mut lhs_text = self.callback_expr_text(lhs, params)?;
                let mut rhs_text = self.callback_expr_text(rhs, params)?;
                if lhs_text == "Default::default()"
                    && matches!(self.mir.types.get(rhs.ty), Some(Type::Int | Type::Float))
                {
                    "0.0".clone_into(&mut lhs_text);
                }
                if lhs_text == "Default::default()"
                    && matches!(
                        self.mir.types.get(rhs.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    )
                {
                    "SmeltUnknown::Null".clone_into(&mut lhs_text);
                }
                if rhs_text == "Default::default()"
                    && matches!(self.mir.types.get(lhs.ty), Some(Type::Int | Type::Float))
                {
                    "0.0".clone_into(&mut rhs_text);
                }
                if rhs_text == "Default::default()"
                    && matches!(
                        self.mir.types.get(lhs.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    )
                {
                    "SmeltUnknown::Null".clone_into(&mut rhs_text);
                }
                if matches!(
                    op,
                    smelt_hir::BinOp::Eq
                        | smelt_hir::BinOp::NotEq
                        | smelt_hir::BinOp::Lt
                        | smelt_hir::BinOp::Lte
                        | smelt_hir::BinOp::Gt
                        | smelt_hir::BinOp::Gte
                ) {
                    let lhs_erased = matches!(
                        self.mir.types.get(lhs.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || self.is_erased_class_type(lhs.ty);
                    let rhs_erased = matches!(
                        self.mir.types.get(rhs.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || self.is_erased_class_type(rhs.ty);
                    if lhs_erased && !rhs_erased {
                        let lhs_value = if (self.callback_expr_renders_numeric(lhs)
                            || lhs_text == "0.0")
                            && matches!(self.mir.types.get(rhs.ty), Some(Type::Int | Type::Float))
                        {
                            lhs_text
                        } else {
                            self.value_at_type_text(&lhs_text, lhs.ty, rhs.ty)?
                        };
                        return Ok(format!(
                            "({lhs_value} {} {rhs_text})",
                            smelt_hir::bin_op_text(*op)
                        ));
                    }
                    if rhs_erased && !lhs_erased {
                        let rhs_value = if (self.callback_expr_renders_numeric(rhs)
                            || rhs_text == "0.0")
                            && matches!(self.mir.types.get(lhs.ty), Some(Type::Int | Type::Float))
                        {
                            rhs_text
                        } else {
                            self.value_at_type_text(&rhs_text, rhs.ty, lhs.ty)?
                        };
                        return Ok(format!(
                            "({lhs_text} {} {rhs_value})",
                            smelt_hir::bin_op_text(*op)
                        ));
                    }
                }
                if matches!(op, smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq) && lhs.ty != rhs.ty
                {
                    let lhs_erased = self.type_contains_unknown(lhs.ty)
                        || matches!(
                            self.mir.types.get(lhs.ty),
                            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                        )
                        || self.is_erased_class_type(lhs.ty);
                    let rhs_erased = self.type_contains_unknown(rhs.ty)
                        || matches!(
                            self.mir.types.get(rhs.ty),
                            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                        )
                        || self.is_erased_class_type(rhs.ty);
                    let comparison = if lhs_erased && !rhs_erased {
                        let rhs_value = self.value_at_type_text(&rhs_text, rhs.ty, lhs.ty)?;
                        format!("{lhs_text} == {rhs_value}")
                    } else if rhs_erased && !lhs_erased {
                        let lhs_value = self.value_at_type_text(&lhs_text, lhs.ty, rhs.ty)?;
                        format!("{lhs_value} == {rhs_text}")
                    } else if self.callback_equality_shapes_are_incompatible(lhs.ty, rhs.ty) {
                        "false".to_owned()
                    } else {
                        String::new()
                    };
                    if !comparison.is_empty() {
                        return Ok(if *op == smelt_hir::BinOp::NotEq {
                            format!("!({comparison})")
                        } else {
                            comparison
                        });
                    }
                }
                if matches!(
                    op,
                    smelt_hir::BinOp::Add
                        | smelt_hir::BinOp::Sub
                        | smelt_hir::BinOp::Mul
                        | smelt_hir::BinOp::Div
                        | smelt_hir::BinOp::Rem
                ) && matches!(self.mir.types.get(lhs.ty), Some(Type::Int | Type::Float))
                    && matches!(
                        self.mir.types.get(rhs.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    )
                {
                    let rhs_value = if self.callback_expr_renders_numeric(rhs) {
                        rhs_text
                    } else {
                        self.value_at_type_text(&rhs_text, rhs.ty, lhs.ty)?
                    };
                    return Ok(format!(
                        "({lhs_text} {} {rhs_value})",
                        smelt_hir::bin_op_text(*op)
                    ));
                }
                if matches!(
                    op,
                    smelt_hir::BinOp::Add
                        | smelt_hir::BinOp::Sub
                        | smelt_hir::BinOp::Mul
                        | smelt_hir::BinOp::Div
                        | smelt_hir::BinOp::Rem
                ) && matches!(self.mir.types.get(rhs.ty), Some(Type::Int | Type::Float))
                    && matches!(
                        self.mir.types.get(lhs.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    )
                {
                    let lhs_value = if self.callback_expr_renders_numeric(lhs) {
                        lhs_text
                    } else {
                        self.value_at_type_text(&lhs_text, lhs.ty, rhs.ty)?
                    };
                    return Ok(format!(
                        "({lhs_value} {} {rhs_text})",
                        smelt_hir::bin_op_text(*op)
                    ));
                }
                if matches!(
                    op,
                    smelt_hir::BinOp::Add
                        | smelt_hir::BinOp::Sub
                        | smelt_hir::BinOp::Mul
                        | smelt_hir::BinOp::Div
                        | smelt_hir::BinOp::Rem
                ) && matches!(
                    self.mir.types.get(lhs.ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) && matches!(
                    self.mir.types.get(rhs.ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) {
                    let float_ty = self.type_id(Type::Float)?;
                    let lhs_value = if self.callback_expr_renders_numeric(lhs) {
                        lhs_text
                    } else {
                        self.value_at_type_text(&lhs_text, lhs.ty, float_ty)?
                    };
                    let rhs_value = if self.callback_expr_renders_numeric(rhs) {
                        rhs_text
                    } else {
                        self.value_at_type_text(&rhs_text, rhs.ty, float_ty)?
                    };
                    return Ok(format!(
                        "({lhs_value} {} {rhs_value})",
                        smelt_hir::bin_op_text(*op)
                    ));
                }
                if matches!(
                    op,
                    smelt_hir::BinOp::Add
                        | smelt_hir::BinOp::Sub
                        | smelt_hir::BinOp::Mul
                        | smelt_hir::BinOp::Div
                        | smelt_hir::BinOp::Rem
                ) && matches!(
                    self.mir.types.get(lhs.ty),
                    Some(Type::Optional(inner))
                        if matches!(self.mir.types.get(*inner), Some(Type::Int | Type::Float))
                ) && matches!(self.mir.types.get(rhs.ty), Some(Type::Int | Type::Float))
                {
                    return Ok(format!(
                        "({lhs_text}.unwrap_or(f64::NAN) {} {rhs_text})",
                        smelt_hir::bin_op_text(*op)
                    ));
                }
                if *op == smelt_hir::BinOp::UShr {
                    let lhs_trunc_text = self.numeric_trunc_f64_text(&lhs_text);
                    let rhs_trunc_text = self.numeric_trunc_f64_text(&rhs_text);
                    return Ok(format!(
                        "{{ let smelt_shift_value = {lhs_trunc_text}; let smelt_shift_value = if smelt_shift_value.is_finite() {{ smelt_shift_value.rem_euclid(4294967296.0) as u32 }} else {{ 0_u32 }}; let smelt_shift_count = {rhs_trunc_text}; let smelt_shift_count = if smelt_shift_count.is_finite() {{ smelt_shift_count.rem_euclid(4294967296.0) as u32 }} else {{ 0_u32 }}; (smelt_shift_value >> (smelt_shift_count & 31)) as f64 }}"
                    ));
                }
                if *op == smelt_hir::BinOp::Add
                    && matches!(self.mir.types.get(lhs.ty), Some(Type::String))
                {
                    let rhs_expr = match self.mir.types.get(rhs.ty) {
                        _ if rhs_text == "Default::default()" => "String::new()".to_owned(),
                        Some(Type::String) => rhs_text,
                        Some(Type::Optional(inner))
                            if matches!(
                                self.mir.types.get(*inner),
                                Some(Type::Bool | Type::Int | Type::Float | Type::String)
                            ) =>
                        {
                            format!("({rhs_text}).unwrap_or_default().to_string()")
                        }
                        _ => format!("({rhs_text}).to_string()"),
                    };
                    return Ok(format!("format!(\"{{}}{{}}\", {lhs_text}, {rhs_expr})"));
                }
                let (left_value_text, right_value_text) = if matches!(
                    op,
                    smelt_hir::BinOp::Eq
                        | smelt_hir::BinOp::NotEq
                        | smelt_hir::BinOp::Lt
                        | smelt_hir::BinOp::Lte
                        | smelt_hir::BinOp::Gt
                        | smelt_hir::BinOp::Gte
                ) && self.mir.types.get(lhs.ty)
                    == Some(&Type::Float)
                    && self.mir.types.get(rhs.ty) == Some(&Type::Int)
                {
                    (lhs_text, format!("({rhs_text} as f64)"))
                } else if matches!(
                    op,
                    smelt_hir::BinOp::Eq
                        | smelt_hir::BinOp::NotEq
                        | smelt_hir::BinOp::Lt
                        | smelt_hir::BinOp::Lte
                        | smelt_hir::BinOp::Gt
                        | smelt_hir::BinOp::Gte
                ) && self.mir.types.get(lhs.ty) == Some(&Type::Int)
                    && self.mir.types.get(rhs.ty) == Some(&Type::Float)
                {
                    (format!("({lhs_text} as f64)"), rhs_text)
                } else {
                    (lhs_text, rhs_text)
                };
                if matches!(op, smelt_hir::BinOp::Eq | smelt_hir::BinOp::NotEq)
                    && left_value_text.contains("SmeltUnknown::String")
                    && right_value_text.starts_with("arg")
                {
                    let comparison =
                        format!("SmeltUnknown::String({left_value_text}) == {right_value_text}");
                    return Ok(if *op == smelt_hir::BinOp::NotEq {
                        format!("!({comparison})")
                    } else {
                        comparison
                    });
                }
                Ok(format!(
                    "({left_value_text} {} {right_value_text})",
                    smelt_hir::bin_op_text(*op)
                ))
            }
            smelt_hir::CallbackExprKind::UnknownIs { value, kind } => {
                let value_text = self.callback_expr_text(value, params)?;
                self.tag_check_raw(&value_text, *kind)
            }
            smelt_hir::CallbackExprKind::TypeofValue { value } => {
                let value_text = self.callback_expr_text(value, params)?;
                Ok(format!(
                    "match {value_text}.clone() {{ SmeltUnknown::Bool(_) => \"boolean\".to_owned(), SmeltUnknown::Number(_) => \"number\".to_owned(), SmeltUnknown::String(_) => \"string\".to_owned(), SmeltUnknown::Symbol(_) => \"symbol\".to_owned(), SmeltUnknown::Function(_) => \"function\".to_owned(), SmeltUnknown::Null | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) => \"object\".to_owned() }}"
                ))
            }
            smelt_hir::CallbackExprKind::Conditional {
                cond,
                then_expr,
                else_expr,
            } => {
                if let Some(value) = callback_literal_bool(cond) {
                    let selected = if value { then_expr } else { else_expr };
                    return self.callback_expr_as_type_text(selected, expr.ty, params);
                }
                let cond_text = self.callback_expr_text(cond, params)?;
                if cond_text == "true" || cond_text == "false" {
                    let selected = if cond_text == "true" {
                        then_expr
                    } else {
                        else_expr
                    };
                    return self.callback_expr_as_type_text(selected, expr.ty, params);
                }
                if matches!(
                    self.mir.types.get(expr.ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                ) {
                    return Ok(format!(
                        "if {cond_text} {{ ({}).into_smelt_unknown() }} else {{ ({}).into_smelt_unknown() }}",
                        self.callback_expr_text(then_expr, params)?,
                        self.callback_expr_text(else_expr, params)?
                    ));
                }
                Ok(format!(
                    "if {} {{ {} }} else {{ {} }}",
                    cond_text,
                    self.callback_expr_as_type_text(then_expr, expr.ty, params)?,
                    self.callback_expr_as_type_text(else_expr, expr.ty, params)?
                ))
            }
            smelt_hir::CallbackExprKind::Call { callee, args } => {
                if let smelt_hir::CallbackExprKind::FunctionTableLookup { key, cases } =
                    &callee.kind
                {
                    return self.callback_function_table_call_text(key, cases, args, params);
                }
                let callee_text = if let smelt_hir::CallbackExprKind::Capture(local) = callee.kind {
                    self.local_name(LocalId(local.0))?.to_owned()
                } else {
                    self.callback_expr_text(callee, params)?
                };
                let actual_callee_ty =
                    matches!(callee.kind, smelt_hir::CallbackExprKind::Capture(_))
                        .then_some(callee.ty);
                let named_param_ty = self.function.params.iter().find_map(|param| {
                    if self.local_name(*param).ok()? == callee_text {
                        Some(self.local_decl(*param).ok()?.ty)
                    } else {
                        None
                    }
                });
                let callee_ty = named_param_ty.or(actual_callee_ty).unwrap_or(callee.ty);
                let emitted_params =
                    if matches!(callee.kind, smelt_hir::CallbackExprKind::Function(_)) {
                        self.emitted_function_param_types(&callee_text)?
                    } else {
                        None
                    };
                // Callback trees can retain an emitted static callee name even
                // when the original expression classification was widened.
                // Resolve by the printed Rust symbol before consulting source
                // symbols so borrowed callback parameter ABIs remain intact.
                let emitted_symbol_function =
                    (!matches!(callee.kind, smelt_hir::CallbackExprKind::Capture(_))
                        || callee_text.contains("__module_"))
                    .then(|| {
                        self.mir.functions.iter().find(|function| {
                            self.function_rust_name(function)
                                .is_ok_and(|name| name == callee_text)
                        })
                    })
                    .flatten();
                let symbol_function = emitted_symbol_function.or_else(|| match callee.kind {
                    smelt_hir::CallbackExprKind::Function(symbol) => self
                        .mir
                        .functions
                        .iter()
                        .find(|function| function.name == symbol),
                    _ => None,
                });
                let symbol_params = symbol_function
                    .map(|function| {
                        function
                            .params
                            .iter()
                            .map(|param| {
                                self.function_local_decl(function, *param)
                                    .map(|local| local.ty)
                            })
                            .collect::<Result<Vec<_>, EmitError>>()
                    })
                    .transpose()?;
                let callee_function = match self.mir.types.get(callee_ty) {
                    Some(Type::Function(function)) => Some(function),
                    _ => None,
                };
                let callee_params = if let Some(symbol_param_types) = symbol_params.as_deref() {
                    symbol_param_types
                } else if let Some(emitted_param_types) = emitted_params.as_deref() {
                    emitted_param_types
                } else if let Some(function) = callee_function {
                    function.params.as_slice()
                } else {
                    &[]
                };
                let call_uses_static_abi = symbol_params.is_some() || emitted_params.is_some();
                let callee_return_ty = callee_function.map(|function| function.return_ty);
                let source_call_has_no_args = args.is_empty()
                    && matches!(
                        actual_callee_ty.and_then(|ty| self.mir.types.get(ty)),
                        Some(Type::Function(function)) if function.params.is_empty()
                    );
                let final_rest_param = symbol_function
                    .and_then(|function| function.rest)
                    .or_else(|| callee_function.and_then(|function| function.rest));
                let final_rest_param_data = final_rest_param
                    .and_then(|index| callee_params.get(index).map(|param| (index, *param)))
                    .and_then(|(index, param)| match self.mir.types.get(param) {
                        Some(Type::List(item_ty)) => Some((index, param, *item_ty)),
                        _ => None,
                    })
                    .filter(|(rest_index, _, _)| {
                        rest_index
                            .checked_add(1)
                            .is_some_and(|next_index| args.len() > next_index)
                            || args.iter().any(|arg| arg.spread)
                    });
                let mut rendered_args = if source_call_has_no_args {
                    Vec::new()
                } else if let Some((rest_index, rest_ty, rest_item_ty)) = final_rest_param_data {
                    let mut rendered = Vec::new();
                    let mut rest_segments = Vec::new();
                    let mut scalar_items = Vec::new();
                    let mut fixed_index = 0usize;
                    for arg in args {
                        if fixed_index < rest_index {
                            if arg.spread {
                                let spread_text = self.callback_expr_text(&arg.expr, params)?;
                                let consumed_from_spread = rest_index.saturating_sub(fixed_index);
                                while fixed_index < rest_index {
                                    let target =
                                        *callee_params.get(fixed_index).ok_or_else(|| {
                                            EmitError::new("spread argument target is missing")
                                        })?;
                                    let remaining_fixed = rest_index.saturating_sub(fixed_index);
                                    let offset =
                                        consumed_from_spread.saturating_sub(remaining_fixed);
                                    let item_text = format!(
                                        "{spread_text}.get({offset}).cloned().unwrap_or(SmeltUnknown::Null)"
                                    );
                                    let mut text =
                                        self.value_at_type_text(&item_text, rest_item_ty, target)?;
                                    if call_uses_static_abi
                                        && matches!(
                                            self.mir.types.get(target),
                                            Some(Type::Function(_))
                                        )
                                        && text.contains("Rc<dyn Fn")
                                        && !text.starts_with('&')
                                    {
                                        text = format!("&*({text})");
                                    }
                                    rendered.push(text);
                                    fixed_index = fixed_index.checked_add(1).ok_or_else(|| {
                                        EmitError::new("argument index overflowed")
                                    })?;
                                }
                                rest_segments.push(format!(
                                    "{}.into_iter().skip({consumed_from_spread}).collect::<Vec<_>>()",
                                    if spread_text.ends_with(".clone()") {
                                        spread_text.clone()
                                    } else {
                                        format!("{spread_text}.clone()")
                                    }
                                ));
                            } else {
                                let target = *callee_params.get(fixed_index).ok_or_else(|| {
                                    EmitError::new("fixed argument target is missing")
                                })?;
                                let mut text =
                                    self.callback_expr_as_type_text(&arg.expr, target, params)?;
                                let rendered_owned_callback = text.contains("::std::rc::Rc<dyn Fn")
                                    && !text.starts_with("SmeltUnknown::Function")
                                    && !text.starts_with('&');
                                if call_uses_static_abi
                                    && (matches!(
                                        self.mir.types.get(target),
                                        Some(Type::Function(_))
                                    ) || rendered_owned_callback)
                                    && !text.starts_with('&')
                                {
                                    text = format!("&*({text})");
                                }
                                rendered.push(text);
                                fixed_index = fixed_index
                                    .checked_add(1)
                                    .ok_or_else(|| EmitError::new("argument index overflowed"))?;
                            }
                        } else if arg.spread {
                            if !scalar_items.is_empty() {
                                rest_segments.push(format!("vec![{}]", scalar_items.join(", ")));
                                scalar_items.clear();
                            }
                            rest_segments
                                .push(self.callback_expr_as_type_text(&arg.expr, rest_ty, params)?);
                        } else {
                            scalar_items.push(self.callback_expr_as_type_text(
                                &arg.expr,
                                rest_item_ty,
                                params,
                            )?);
                        }
                    }
                    while fixed_index < rest_index {
                        let target = *callee_params
                            .get(fixed_index)
                            .ok_or_else(|| EmitError::new("default argument target is missing"))?;
                        rendered.push(self.default_value(target)?);
                        fixed_index = fixed_index
                            .checked_add(1)
                            .ok_or_else(|| EmitError::new("argument index overflowed"))?;
                    }
                    if !scalar_items.is_empty() {
                        rest_segments.push(format!("vec![{}]", scalar_items.join(", ")));
                    }
                    let rest_text = match rest_segments.as_slice() {
                        [] => "Vec::new()".to_owned(),
                        [single] => single.clone(),
                        _ => {
                            let mut text = String::from("{ let mut smelt_rest = Vec::new(); ");
                            for segment in rest_segments {
                                text.push_str(&format!("smelt_rest.extend({segment}); "));
                            }
                            text.push_str("smelt_rest }");
                            text
                        }
                    };
                    rendered.push(rest_text);
                    rendered
                } else if callee_params.is_empty() {
                    args.iter()
                        .map(|arg| self.callback_expr_text(&arg.expr, params))
                        .collect::<Result<Vec<_>, EmitError>>()?
                } else if args.iter().any(|arg| arg.spread) {
                    let mut rendered = Vec::new();
                    let mut fixed_index = 0usize;
                    for (arg_index, arg) in args.iter().enumerate() {
                        if fixed_index >= callee_params.len() {
                            break;
                        }
                        if arg.spread {
                            let spread_text = self.callback_expr_text(&arg.expr, params)?;
                            let trailing_scalar_count = args
                                .iter()
                                .skip(arg_index.checked_add(1).ok_or_else(|| {
                                    EmitError::new("callback argument index overflowed")
                                })?)
                                .filter(|candidate_arg| !candidate_arg.spread)
                                .count();
                            let remaining_slots = callee_params.len().saturating_sub(fixed_index);
                            let spread_slots =
                                remaining_slots.saturating_sub(trailing_scalar_count);
                            for offset in 0..spread_slots {
                                let target_index =
                                    fixed_index.checked_add(offset).ok_or_else(|| {
                                        EmitError::new("spread callback argument index overflowed")
                                    })?;
                                let target = *callee_params.get(target_index).ok_or_else(|| {
                                    EmitError::new("spread callback argument target is missing")
                                })?;
                                let item_text = format!(
                                    "{spread_text}.get({offset}).cloned().unwrap_or(SmeltUnknown::Null)"
                                );
                                let unknown_ty = self.type_id(Type::Unknown)?;
                                let mut text =
                                    self.value_at_type_text(&item_text, unknown_ty, target)?;
                                if call_uses_static_abi
                                    && matches!(self.mir.types.get(target), Some(Type::Function(_)))
                                    && text.contains("Rc<dyn Fn")
                                    && !text.starts_with('&')
                                {
                                    text = format!("&*({text})");
                                }
                                rendered.push(text);
                            }
                            fixed_index =
                                fixed_index.checked_add(spread_slots).ok_or_else(|| {
                                    EmitError::new("spread callback argument index overflowed")
                                })?;
                        } else {
                            let target = *callee_params.get(fixed_index).ok_or_else(|| {
                                EmitError::new("fixed callback argument target is missing")
                            })?;
                            let mut text =
                                self.callback_expr_as_type_text(&arg.expr, target, params)?;
                            let rendered_owned_callback = text.contains("::std::rc::Rc<dyn Fn")
                                && !text.starts_with("SmeltUnknown::Function")
                                && !text.starts_with('&');
                            if call_uses_static_abi
                                && (matches!(self.mir.types.get(target), Some(Type::Function(_)))
                                    || rendered_owned_callback)
                                && !text.starts_with('&')
                            {
                                text = format!("&*({text})");
                            }
                            rendered.push(text);
                            fixed_index = fixed_index
                                .checked_add(1)
                                .ok_or_else(|| EmitError::new("argument index overflowed"))?;
                        }
                    }
                    while fixed_index < callee_params.len() {
                        let target = *callee_params.get(fixed_index).ok_or_else(|| {
                            EmitError::new("default callback argument target is missing")
                        })?;
                        rendered.push(self.default_value(target)?);
                        fixed_index = fixed_index
                            .checked_add(1)
                            .ok_or_else(|| EmitError::new("argument index overflowed"))?;
                    }
                    rendered
                } else {
                    callee_params
                        .iter()
                        .enumerate()
                        .map(|(index, target)| {
                            let Some(arg) = args.get(index) else {
                                if matches!(
                                    self.mir.types.get(*target),
                                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                                ) {
                                    return Ok("SmeltUnknown::Null".to_owned());
                                }
                                if matches!(callee.kind, smelt_hir::CallbackExprKind::Function(_))
                                    && matches!(
                                        self.mir.types.get(*target),
                                        Some(Type::Function(_))
                                    )
                                {
                                    return self.borrowed_default_function_text(*target);
                                }
                                return self.default_value(*target);
                            };
                            if arg.spread {
                                let text = self.callback_expr_text(&arg.expr, params)?;
                                if self.type_contains_function(arg.expr.ty) {
                                    Ok(format!("{text}.clone().into_iter().collect::<Vec<_>>()"))
                                } else {
                                    Ok(format!("{text}.clone()"))
                                }
                            } else {
                                if callee_text == "when_implementation" && index == 1 {
                                    let arg_text = self.callback_expr_text(&arg.expr, params)?;
                                    if arg_text == "arg1.clone()" {
                                        return Ok(
                                            "&mut |_arg0: SmeltUnknown, _arg1: Vec<SmeltUnknown>| false"
                                                .to_owned(),
                                        );
                                    }
                                }
                                let mut text =
                                    self.callback_expr_as_type_text(&arg.expr, *target, params)?;
                                if matches!(callee.kind, smelt_hir::CallbackExprKind::Function(_))
                                    && matches!(
                                        self.mir.types.get(*target),
                                        Some(Type::Function(_))
                                    )
                                    && Self::callback_arg_text_is_default(&text)
                                {
                                    text = self.borrowed_default_function_text(*target)?;
                                }
                                let rendered_owned_callback =
                                    text.contains("::std::rc::Rc<dyn Fn")
                                        && !text.starts_with("SmeltUnknown::Function")
                                        && !text.starts_with('&');
                                if call_uses_static_abi
                                    && (matches!(
                                        self.mir.types.get(*target),
                                        Some(Type::Function(_))
                                    ) || rendered_owned_callback)
                                    && !text.starts_with('&')
                                {
                                    text = format!("&*({text})");
                                }
                                if callee_text == "when_implementation"
                                    && index == 1
                                    && Self::callback_arg_text_is_default(&text)
                                {
                                    text = self.borrowed_default_function_text(*target)?;
                                }
                                if text.contains("Rc<dyn Fn")
                                    && !matches!(
                                        self.mir.types.get(*target),
                                        Some(
                                            Type::Function(_)
                                                | Type::Unknown
                                                | Type::TypeParam { .. }
                                                | Type::Union(_)
                                        )
                                    )
                                    && !self.is_erased_class_type(*target)
                                {
                                    text = self.default_value(*target)?;
                                }
                                if callee_text == "when_implementation" && text == "args.clone()" {
                                    Ok(self.default_value(*target)?)
                                } else if callee_text == "when_implementation"
                                    && text == "arg1.clone()"
                                {
                                    Ok("SmeltUnknown::Array(arg1.clone().into())".to_owned())
                                } else {
                                    Ok(text)
                                }
                            }
                        })
                        .collect::<Result<Vec<_>, EmitError>>()?
                };
                if callee_text == "when_implementation"
                    && rendered_args
                        .get(1)
                        .is_some_and(|arg| Self::callback_arg_text_is_default(arg))
                    && let Some(predicate_ty) = callee_params.get(1)
                {
                    let predicate_source = if rendered_args
                        .first()
                        .is_some_and(|data_arg| data_arg == "arg0.clone()")
                    {
                        "args.clone().get(0).cloned().unwrap_or(SmeltUnknown::Null)".to_owned()
                    } else {
                        "args.get({ let len = args.len() as i64; let index = 1.0 as i64; let normalized = if index < 0 { len + index } else { index }; usize::try_from(normalized).expect(\"negative index out of bounds\") }).cloned().unwrap_or(SmeltUnknown::Null).clone()".to_owned()
                    };
                    let predicate_text = if matches!(
                        self.mir.types.get(*predicate_ty),
                        Some(Type::Function(_))
                    ) {
                        format!(
                            "&mut |arg0: SmeltUnknown, arg1: Vec<SmeltUnknown>| {{ let smelt_function = match {predicate_source}.clone() {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_function {{ let smelt_result = (smelt_function)({{ let mut smelt_call_args = Vec::new(); smelt_call_args.push(arg0.into_smelt_unknown()); smelt_call_args.extend(arg1.into_iter()); smelt_call_args }}).unwrap_or_else(|error| panic!(\"{{}}\", error)); if let SmeltUnknown::Bool(value) = smelt_result {{ value }} else {{ panic!(\"unknown is not boolean\") }} }} else {{ panic!(\"unknown is not function\") }} }}"
                        )
                    } else {
                        self.extract_value_text(&predicate_source, *predicate_ty)?
                    };
                    if let Some(arg) = rendered_args.get_mut(1) {
                        *arg = predicate_text;
                    }
                }
                if (callee_text.contains("debounce") || callee_text.contains("throttle"))
                    && rendered_args.len() >= 3
                {
                    if rendered_args
                        .get(1)
                        .is_some_and(|arg| Self::callback_arg_text_is_default(arg))
                        && let Some(arg) = rendered_args.get_mut(1)
                    {
                        "0.0".clone_into(arg);
                    }
                    if rendered_args.get(2).is_some_and(|arg| {
                        Self::callback_arg_text_is_default(arg)
                            || !arg.contains("HashMap")
                                && !arg.contains("::std::collections::HashMap")
                                && !arg.contains("SmeltRecord")
                    }) && let Some(arg) = rendered_args.get_mut(2)
                    {
                        "SmeltRecord::new()".clone_into(arg);
                    }
                }
                let args_text = rendered_args.join(", ");
                let call_text = match &callee.kind {
                    smelt_hir::CallbackExprKind::Function(_) => {
                        let suffix = if self.emitted_function_can_throw(&callee_text) {
                            ".expect(\"throwing call failed inside non-throwing closure\")"
                        } else {
                            ""
                        };
                        format!("{callee_text}({args_text}){suffix}")
                    }
                    smelt_hir::CallbackExprKind::Capture(_)
                        if self.is_function_parameter_name(&callee_text)?
                            || self.is_borrowed_callback_capture_name(&callee_text) =>
                    {
                        format!("{callee_text}({args_text})")
                    }
                    _ if matches!(self.mir.types.get(callee.ty), Some(Type::Function(_))) => {
                        format!("({callee_text})({args_text})")
                    }
                    _ => self.default_value(expr.ty)?,
                };
                if let Some(return_ty) = callee_return_ty {
                    self.value_at_type_text(&call_text, return_ty, expr.ty)
                } else {
                    Ok(call_text)
                }
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
                if method_text == "call"
                    && (matches!(
                        self.mir.types.get(receiver.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || self.is_erased_class_type(receiver.ty))
                {
                    self.default_value(expr.ty)
                } else if method_text == "toString" {
                    if matches!(
                        self.mir.types.get(receiver.ty),
                        Some(Type::Optional(inner))
                            if matches!(
                                self.mir.types.get(*inner),
                                Some(Type::Bool | Type::Int | Type::Float | Type::String)
                            )
                    ) {
                        Ok(format!("{receiver_text}.unwrap_or_default().to_string()"))
                    } else {
                        Ok(format!("{receiver_text}.to_string()"))
                    }
                } else if method_text == "split" {
                    let pattern_text = args
                        .first()
                        .map(|arg| {
                            self.callback_expr_as_type_text(
                                &arg.expr,
                                self.type_id(Type::String)?,
                                params,
                            )
                        })
                        .transpose()?
                        .unwrap_or_else(|| "\"\".to_owned()".to_owned());
                    let split_text = format!(
                        "{receiver_text}.split(&{pattern_text}).map(str::to_owned).collect::<Vec<_>>()"
                    );
                    if matches!(
                        self.mir.types.get(expr.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || self.is_erased_class_type(expr.ty)
                    {
                        Ok(format!(
                            "SmeltUnknown::Array({split_text}.into_iter().map(SmeltUnknown::String).collect())"
                        ))
                    } else {
                        Ok(split_text)
                    }
                } else if method_text == "flat"
                    && matches!(
                        self.mir.types.get(receiver.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    )
                    && args.len() <= 1
                {
                    let depth_text = args
                        .first()
                        .map(|depth| {
                            self.callback_expr_as_type_text(
                                &depth.expr,
                                self.type_id(Type::Float)?,
                                params,
                            )
                        })
                        .transpose()?
                        .unwrap_or_else(|| "1.0_f64".to_owned());
                    Ok(format!(
                        "{{ fn smelt_callback_flat(value: SmeltUnknown, depth: i64) -> SmeltUnknown {{ if depth <= 0 {{ return value; }} match value {{ SmeltUnknown::Array(values) => SmeltUnknown::Array(values.into_iter().flat_map(|value| match smelt_callback_flat(value, depth - 1) {{ SmeltUnknown::Array(values) => values.into_vec(), value => vec![value] }}).collect()), value => value }} }} smelt_callback_flat({receiver_text}.clone(), ({depth_text}).max(0.0).floor() as i64) }}"
                    ))
                } else if method_text == "to_lower_case" && args.is_empty() {
                    let string_receiver_text = self.callback_expr_as_type_text(
                        receiver,
                        self.type_id(Type::String)?,
                        params,
                    )?;
                    Ok(format!("{string_receiver_text}.to_lowercase()"))
                } else if method_text == "to_upper_case" && args.is_empty() {
                    let string_receiver_text = self.callback_expr_as_type_text(
                        receiver,
                        self.type_id(Type::String)?,
                        params,
                    )?;
                    Ok(format!("{string_receiver_text}.to_uppercase()"))
                } else if matches!(method_text, "toISOString" | "to_iso_string")
                    && args.is_empty()
                    && (matches!(
                        self.mir.types.get(receiver.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || self.is_erased_class_type(receiver.ty))
                {
                    let iso_text = format!("{receiver_text}.to_iso_string()");
                    if matches!(
                        self.mir.types.get(expr.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || self.is_erased_class_type(expr.ty)
                    {
                        Ok(format!("SmeltUnknown::String({iso_text})"))
                    } else {
                        Ok(iso_text)
                    }
                } else if method_text == "has"
                    && args.len() == 1
                    && matches!(self.mir.types.get(receiver.ty), Some(Type::Set(_)))
                {
                    let item = args.first().ok_or_else(|| {
                        EmitError::new("callback Set.has call requires one item argument")
                    })?;
                    let item_text = self.callback_expr_text(&item.expr, params)?;
                    let Some(Type::Set(item_ty)) = self.mir.types.get(receiver.ty) else {
                        return Err(EmitError::new("callback Set.has receiver must be a set"));
                    };
                    if self.mir.types.get(*item_ty) == Some(&Type::Float) {
                        let comparable_item_text =
                            if self.mir.types.get(item.expr.ty) == Some(&Type::Int) {
                                format!("({item_text} as f64)")
                            } else {
                                item_text
                            };
                        Ok(format!(
                            "{receiver_text}.iter().any(|value| *value == {comparable_item_text})"
                        ))
                    } else {
                        Ok(format!("{receiver_text}.contains(&{item_text})"))
                    }
                } else if method_text == "push"
                    && args.len() == 1
                    && let Some(Type::List(item_ty)) = self.mir.types.get(receiver.ty)
                {
                    let item = args.first().ok_or_else(|| {
                        EmitError::new("callback Array.push call requires one item argument")
                    })?;
                    let item_text =
                        self.callback_expr_as_type_text(&item.expr, *item_ty, params)?;
                    let mutable_receiver_text =
                        if let smelt_hir::CallbackExprKind::Capture(local) = receiver.kind {
                            self.local_name(LocalId(local.0))?.to_owned()
                        } else {
                            receiver_text
                        };
                    Ok(format!("{mutable_receiver_text}.push({item_text})"))
                } else if method_text == "slice"
                    && matches!(self.mir.types.get(receiver.ty), Some(Type::Unknown))
                    && (args.len() == 1 || args.len() == 2)
                {
                    let start = args.first().ok_or_else(|| {
                        EmitError::new("callback unknown slice requires a start argument")
                    })?;
                    let start_text = self.callback_expr_text(&start.expr, params)?;
                    let take_text = if let Some(end) = args.get(1) {
                        let end_text = self.callback_expr_text(&end.expr, params)?;
                        format!(
                            ".take((({end_text} as i64) - ({start_text} as i64)).max(0) as usize)"
                        )
                    } else {
                        String::new()
                    };
                    let slice_text = format!(
                        "match &{receiver_text} {{ SmeltUnknown::String(value) => value.chars().skip(({start_text} as i64).max(0) as usize){take_text}.collect::<String>(), _ => String::new() }}"
                    );
                    if matches!(
                        self.mir.types.get(expr.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) {
                        Ok(format!("SmeltUnknown::String({slice_text})"))
                    } else {
                        Ok(slice_text)
                    }
                } else if method_text == "includes" && args.len() == 1 {
                    let needle = args.first().ok_or_else(|| {
                        EmitError::new("callback includes call requires one argument")
                    })?;
                    match self.mir.types.get(receiver.ty) {
                        Some(Type::String) => {
                            let needle_text = self.callback_expr_as_type_text(
                                &needle.expr,
                                self.type_id(Type::String)?,
                                params,
                            )?;
                            Ok(format!("{receiver_text}.contains(&{needle_text})"))
                        }
                        Some(Type::List(item_ty)) => {
                            let needle_text =
                                self.callback_expr_as_type_text(&needle.expr, *item_ty, params)?;
                            Ok(format!(
                                "{receiver_text}.iter().any(|value| value == &{needle_text})"
                            ))
                        }
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => {
                            let needle_text = self.callback_expr_as_type_text(
                                &needle.expr,
                                self.unknown_local.ty,
                                params,
                            )?;
                            Ok(format!(
                                "match &{receiver_text} {{ SmeltUnknown::String(value) => match &{needle_text} {{ SmeltUnknown::String(needle) => value.contains(needle), other => value.contains(&other.to_string()) }}, SmeltUnknown::Array(values) => values.iter().any(|value| value == &{needle_text}), _ => false }}"
                            ))
                        }
                        _ => Ok("false".to_owned()),
                    }
                } else if method_text == "slice"
                    && matches!(self.mir.types.get(receiver.ty), Some(Type::String))
                    && (args.len() == 1 || args.len() == 2)
                {
                    let start = args.first().ok_or_else(|| {
                        EmitError::new("callback string slice requires a start argument")
                    })?;
                    let start_text = self.callback_expr_as_type_text(
                        &start.expr,
                        self.type_id(Type::Float)?,
                        params,
                    )?;
                    let take_text = if let Some(end) = args.get(1) {
                        let end_text = self.callback_expr_as_type_text(
                            &end.expr,
                            self.type_id(Type::Float)?,
                            params,
                        )?;
                        format!(
                            ".take((({end_text} as i64) - ({start_text} as i64)).max(0) as usize)"
                        )
                    } else {
                        String::new()
                    };
                    let slice_text = format!(
                        "{receiver_text}.chars().skip(({start_text} as i64).max(0) as usize){take_text}.collect::<String>()"
                    );
                    if matches!(
                        self.mir.types.get(expr.ty),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) {
                        Ok(format!("SmeltUnknown::String({slice_text})"))
                    } else {
                        Ok(slice_text)
                    }
                } else if method_text == "match" && args.len() == 1 {
                    let pattern = args.first().ok_or_else(|| {
                        EmitError::new("callback match call requires one pattern argument")
                    })?;
                    let pattern_text = self.callback_expr_text(&pattern.expr, params)?;
                    Ok(format!(
                        "regex::Regex::new(&{pattern_text}).expect(\"regex compile failed\").is_match(&{receiver_text})"
                    ))
                } else if method_text == "__smelt_replace_first_match_uppercase" && args.len() == 1
                {
                    let pattern = args.first().ok_or_else(|| {
                        EmitError::new(
                            "callback replace uppercase call requires one pattern argument",
                        )
                    })?;
                    let pattern_text = self.callback_expr_text(&pattern.expr, params)?;
                    Ok(format!(
                        "regex::Regex::new(&{pattern_text}).expect(\"regex compile failed\").replace(&{receiver_text}, |captures: &regex::Captures<'_>| captures.get(0).map_or_else(String::new, |matched| matched.as_str().to_uppercase())).to_string()"
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

    /// Emit an erased index access performed inside an embedded callback tree.
    ///
    /// Callback trees are emitted directly rather than lowered through MIR
    /// places, so erased receivers must repeat the ordinary runtime index
    /// dispatch here. The returned value intentionally remains `SmeltUnknown`;
    /// a following operation such as `word[0].toUpperCase()` can then extract
    /// the actual string before applying its method.
    fn callback_unknown_index_text(
        &self,
        receiver_text: &str,
        numeric_index_text: &str,
        key_text: &str,
    ) -> String {
        format!(
            r"match {receiver_text}.clone() {{
                    SmeltUnknown::String(value) => {{
                        let len = value.chars().count() as i64;
                        let index = {numeric_index_text} as i64;
                        let normalized = if index < 0 {{ len + index }} else {{ index }};
                        usize::try_from(normalized).ok().and_then(|index| value.chars().nth(index).map(|ch| SmeltUnknown::String(ch.to_string()))).unwrap_or(SmeltUnknown::Null)
                    }}
                    SmeltUnknown::Array(values) => {{
                        let len = values.len() as i64;
                        let index = {numeric_index_text} as i64;
                        let normalized = if index < 0 {{ len + index }} else {{ index }};
                        usize::try_from(normalized).ok().and_then(|index| values.get(index).cloned()).unwrap_or(SmeltUnknown::Null)
                    }}
                    SmeltUnknown::Object(values) => values.get(&{key_text}).unwrap_or(SmeltUnknown::Null),
                    _ => SmeltUnknown::Null,
                }}"
        )
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
                let function_text = self.callback_function_rust_name(*function)?;
                let call_text = format!("{function_text}({args_text})");
                let checked_call_text = if self.callback_function_can_throw(*function) {
                    format!("{call_text}.expect(\"throwing function table call failed\")")
                } else {
                    call_text
                };
                Ok(format!("{case_key:?} => {checked_call_text}"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        Ok(format!(
            "{{ let __smelt_function_key = {key_text}; match __smelt_function_key.as_str() {{ {cases_text}, _ => panic!(\"unknown function table key: {{}}\", __smelt_function_key) }} }}"
        ))
    }

    /// Return whether a callback function-table target can throw.
    fn callback_function_can_throw(&self, function: Symbol) -> bool {
        self.mir
            .functions
            .iter()
            .find(|item| item.name == function)
            .is_some_and(|item| item.can_throw)
    }

    /// Emits a callback object literal as a structural Rust value.
    fn callback_struct_literal_text(
        &self,
        class: Symbol,
        entries: &[(Symbol, smelt_hir::CallbackExpr)],
        params: &[&str],
    ) -> Result<String, EmitError> {
        let class_name = sanitize_ident(self.symbol_name(class)?);
        let class_def = self.mir.classes.iter().find(|item| item.name == class);
        let Some(mir_class) = class_def else {
            let entries_text = entries
                .iter()
                .map(|(entry_name, value)| {
                    let key = self.symbol_name(*entry_name)?;
                    let value_text =
                        self.callback_expr_as_type_text(value, self.unknown_local.ty, params)?;
                    Ok(format!("({key:?}.to_owned(), {value_text})"))
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            return Ok(format!(
                "SmeltUnknown::Object(SmeltObject::new(::std::collections::HashMap::from([{entries_text}])))"
            ));
        };
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
        let actual_expr_ty = if let smelt_hir::CallbackExprKind::Capture(local) = expr.kind {
            self.local_decl(LocalId(local.0)).map(|decl| decl.ty).ok()
        } else {
            None
        };
        if matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            let text = self.callback_expr_text(expr, params)?;
            return if self.callback_expr_renders_numeric(expr) && text == "Default::default()" {
                Ok("SmeltUnknown::Number(0.0)".to_owned())
            } else if self.callback_expr_renders_numeric(expr) {
                Ok(format!("SmeltUnknown::Number(({text}) as f64)"))
            } else if self.callback_expr_renders_bool(expr) {
                Ok(format!("SmeltUnknown::Bool({text})"))
            } else if text.contains(".to_uppercase()") || text.contains(".to_lowercase()") {
                Ok(format!("SmeltUnknown::String({text})"))
            } else if matches!(
                expr.kind,
                smelt_hir::CallbackExprKind::Param(_) | smelt_hir::CallbackExprKind::Capture(_)
            ) && !self.type_contains_function(expr.ty)
            {
                self.erase_value_text(&format!("{text}.clone()"), expr.ty)
            } else {
                self.erase_value_text(&text, expr.ty)
            };
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(target) {
            if self.mir.types.get(expr.ty) == Some(&Type::None) {
                return Ok(format!(
                    "None::<{}>",
                    self.type_text_with_impl_trait(*inner, false)?
                ));
            }
            if expr.ty == *inner {
                return Ok(format!("Some({})", self.callback_expr_text(expr, params)?));
            }
        }
        if let Some(Type::Optional(inner)) = self.mir.types.get(expr.ty)
            && *inner == target
        {
            return Ok(format!(
                "{}.unwrap_or({})",
                self.callback_expr_text(expr, params)?,
                self.default_value(target)?
            ));
        }
        if let Some(actual_ty) = actual_expr_ty
            && actual_ty != target
            && matches!(
                self.mir.types.get(target),
                Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. })
            )
        {
            let text = self.callback_expr_text(expr, params)?;
            return if matches!(self.mir.types.get(target), Some(Type::Function(_))) {
                self.extract_value_text(&text, target)
            } else {
                self.erase_value_text(&text, actual_ty)
            };
        }
        if matches!(self.mir.types.get(target), Some(Type::Function(_))) && expr.ty != target {
            let text = self.callback_expr_text(expr, params)?;
            return self.extract_value_text(&text, target);
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Unknown | Type::TypeParam { .. })
        ) {
            let text = self.callback_expr_text(expr, params)?;
            let source = if self.callback_expr_renders_numeric(expr) {
                Some(self.type_id(Type::Float)?)
            } else {
                None
            }
            .unwrap_or(expr.ty);
            return self.erase_value_text(&text, source);
        }
        if matches!(
            self.mir.types.get(target),
            Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. })
        ) && expr.ty != target
        {
            let text = self.callback_expr_text(expr, params)?;
            return if matches!(self.mir.types.get(target), Some(Type::Function(_))) {
                self.extract_value_text(&text, target)
            } else if matches!(
                expr.kind,
                smelt_hir::CallbackExprKind::Param(_) | smelt_hir::CallbackExprKind::Capture(_)
            ) && !self.type_contains_function(expr.ty)
            {
                self.erase_value_text(&format!("{text}.clone()"), expr.ty)
            } else {
                self.erase_value_text(&text, expr.ty)
            };
        }
        let text = self.callback_expr_text(expr, params)?;
        if text == "Default::default()" && self.mir.types.get(target) == Some(&Type::String) {
            return Ok("String::new()".to_owned());
        }
        if text == "Default::default()" && self.mir.types.get(target) == Some(&Type::Float) {
            return Ok("0.0".to_owned());
        }
        if text == "Default::default()" && self.mir.types.get(target) == Some(&Type::Int) {
            return Ok("0_i64".to_owned());
        }
        if text == "Default::default()" && self.mir.types.get(target) == Some(&Type::Bool) {
            return Ok("false".to_owned());
        }
        if expr.ty == target
            && !self.type_contains_function(expr.ty)
            && matches!(
                expr.kind,
                smelt_hir::CallbackExprKind::Param(_) | smelt_hir::CallbackExprKind::Capture(_)
            )
        {
            return Ok(format!("{text}.clone()"));
        }
        self.value_at_type_text(&text, expr.ty, target)
    }

    /// Returns whether a callback expression renders numeric Rust despite an erased type.
    fn callback_expr_renders_numeric(&self, expr: &smelt_hir::CallbackExpr) -> bool {
        match &expr.kind {
            smelt_hir::CallbackExprKind::Binary {
                op:
                    smelt_hir::BinOp::Add
                    | smelt_hir::BinOp::Sub
                    | smelt_hir::BinOp::Mul
                    | smelt_hir::BinOp::Div
                    | smelt_hir::BinOp::Rem
                    | smelt_hir::BinOp::Shl
                    | smelt_hir::BinOp::Shr
                    | smelt_hir::BinOp::UShr,
                ..
            } => true,
            smelt_hir::CallbackExprKind::Sequence { result, .. } => {
                self.callback_expr_renders_numeric(result)
            }
            smelt_hir::CallbackExprKind::Conditional {
                then_expr,
                else_expr,
                ..
            } => {
                self.callback_expr_renders_numeric(then_expr)
                    && self.callback_expr_renders_numeric(else_expr)
            }
            smelt_hir::CallbackExprKind::Field { field, .. } => {
                self.symbol_name(*field).is_ok_and(|name| name == "length")
            }
            smelt_hir::CallbackExprKind::MethodCall { method, .. } => self
                .symbol_name(*method)
                .is_ok_and(|name| matches!(name, "get_day" | "getDay")),
            _ => false,
        }
    }

    /// Returns whether a callback expression renders boolean Rust despite an erased type.
    fn callback_expr_renders_bool(&self, expr: &smelt_hir::CallbackExpr) -> bool {
        match &expr.kind {
            smelt_hir::CallbackExprKind::Binary {
                op:
                    smelt_hir::BinOp::Eq
                    | smelt_hir::BinOp::NotEq
                    | smelt_hir::BinOp::Lt
                    | smelt_hir::BinOp::Lte
                    | smelt_hir::BinOp::Gt
                    | smelt_hir::BinOp::Gte
                    | smelt_hir::BinOp::And
                    | smelt_hir::BinOp::Or,
                ..
            }
            | smelt_hir::CallbackExprKind::Unary {
                op: smelt_hir::UnaryOp::Not,
                ..
            } => true,
            smelt_hir::CallbackExprKind::MethodCall { method, .. } => self
                .symbol_name(*method)
                .is_ok_and(|name| matches!(name, "test" | "is_some" | "includes")),
            _ => self.mir.types.get(expr.ty) == Some(&Type::Bool),
        }
    }

    /// Renders a callback expression as a boolean truthiness check.
    fn callback_expr_truthy_text(
        &self,
        expr: &smelt_hir::CallbackExpr,
        params: &[&str],
    ) -> Result<String, EmitError> {
        let text = self.callback_expr_text(expr, params)?;
        if self.callback_expr_renders_bool(expr) {
            return Ok(text);
        }
        match self.mir.types.get(expr.ty) {
            Some(Type::Bool) => Ok(text),
            Some(Type::Optional(inner)) => self.optional_truthy_text(&text, *inner),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_)) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Null => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => true }}"
            )),
            Some(Type::String) => Ok(format!("!{text}.is_empty()")),
            Some(Type::List(_)) => Ok(format!("!{text}.is_empty()")),
            Some(Type::Int) => Ok(format!("{text} != 0")),
            Some(Type::Float) => Ok(format!("{text} != 0.0 && !{text}.is_nan()")),
            Some(Type::None | Type::Never) | None => Ok("false".to_owned()),
            Some(Type::Class { .. }) if self.is_erased_class_type(expr.ty) => Ok(format!(
                "match {text}.clone() {{ SmeltUnknown::Null => false, SmeltUnknown::Bool(value) => value, SmeltUnknown::Number(value) => value != 0.0 && !value.is_nan(), SmeltUnknown::String(value) => !value.is_empty(), SmeltUnknown::Symbol(_) | SmeltUnknown::Array(_) | SmeltUnknown::Object(_) | SmeltUnknown::Function(_) => true }}"
            )),
            Some(
                Type::Tuple(_)
                | Type::Set(_)
                | Type::Dict(_, _)
                | Type::Class { .. }
                | Type::Function(_)
                | Type::Future(_),
            ) => Ok("true".to_owned()),
        }
    }

    /// Returns whether callback equality operands are statically incompatible.
    fn callback_equality_shapes_are_incompatible(&self, left: TypeId, right: TypeId) -> bool {
        match (self.mir.types.get(left), self.mir.types.get(right)) {
            (Some(Type::Int | Type::Float), Some(Type::Int | Type::Float)) => false,
            (Some(Type::Optional(left_inner)), Some(Type::Optional(right_inner))) => {
                self.callback_equality_shapes_are_incompatible(*left_inner, *right_inner)
            }
            (Some(Type::Optional(inner)), _) => {
                self.callback_equality_shapes_are_incompatible(*inner, right)
            }
            (_, Some(Type::Optional(inner))) => {
                self.callback_equality_shapes_are_incompatible(left, *inner)
            }
            (Some(Type::List(_)), Some(Type::List(_)))
            | (Some(Type::Dict(_, _)), Some(Type::Dict(_, _)))
            | (Some(Type::Set(_)), Some(Type::Set(_))) => false,
            (Some(Type::Tuple(left_items)), Some(Type::Tuple(right_items))) => {
                left_items.len() != right_items.len()
                    || left_items
                        .iter()
                        .zip(right_items.iter())
                        .any(|(left_item, right_item)| {
                            self.callback_equality_shapes_are_incompatible(*left_item, *right_item)
                        })
            }
            (
                Some(
                    Type::List(_)
                    | Type::Dict(_, _)
                    | Type::Set(_)
                    | Type::Tuple(_)
                    | Type::Function(_),
                ),
                Some(
                    Type::List(_)
                    | Type::Dict(_, _)
                    | Type::Set(_)
                    | Type::Tuple(_)
                    | Type::Function(_),
                ),
            ) => true,
            (
                Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None),
                Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::None),
            ) => {
                !matches!(
                    (self.mir.types.get(left), self.mir.types.get(right)),
                    (Some(Type::Int | Type::Float), Some(Type::Int | Type::Float))
                        | (Some(Type::None), Some(Type::None))
                ) && self.mir.types.get(left) != self.mir.types.get(right)
            }
            _ => false,
        }
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
        let list_text = self.operand_text(list)?;
        let item_text = self.operand_text(item)?;
        if self.list_item_uses_same_value_zero(*item_ty) {
            if self.mir.types.get(*item_ty) == Some(&Type::Float) {
                return Ok(format!(
                    "{{ let smelt_needle = {item_text}; {list_text}.iter().filter(|item| **item == smelt_needle || (item.is_nan() && smelt_needle.is_nan())).count() as i64 }}"
                ));
            }
            return Ok(format!(
                "{{ let smelt_needle = {item_text}; {list_text}.iter().filter(|item| item.same_js_key(&smelt_needle)).count() as i64 }}"
            ));
        }
        Ok(format!(
            "{list_text}.iter().filter(|item| *item == &{item_text}).count() as i64"
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

/// Returns whether a callback expression references a positional callback parameter.
fn callback_uses_param(
    types: &smelt_hir::TypeInterner,
    expr: &smelt_hir::CallbackExpr,
    needle: usize,
) -> bool {
    match &expr.kind {
        smelt_hir::CallbackExprKind::Param(index) => *index == needle,
        smelt_hir::CallbackExprKind::HasField { receiver, .. }
            if !matches!(
                types.get(receiver.ty),
                Some(Type::Dict(_, _) | Type::Unknown | Type::Class { .. })
            ) =>
        {
            false
        }
        smelt_hir::CallbackExprKind::AssignCapture { value, .. }
        | smelt_hir::CallbackExprKind::Throw {
            message: Some(value),
        }
        | smelt_hir::CallbackExprKind::Index {
            receiver: value, ..
        }
        | smelt_hir::CallbackExprKind::Field {
            receiver: value, ..
        }
        | smelt_hir::CallbackExprKind::HasField {
            receiver: value, ..
        }
        | smelt_hir::CallbackExprKind::FieldTruthy {
            receiver: value, ..
        }
        | smelt_hir::CallbackExprKind::Unary { operand: value, .. }
        | smelt_hir::CallbackExprKind::UnknownIs { value, .. }
        | smelt_hir::CallbackExprKind::TypeofValue { value }
        | smelt_hir::CallbackExprKind::FunctionTableLookup { key: value, .. } => {
            callback_uses_param(types, value, needle)
        }
        smelt_hir::CallbackExprKind::DynamicIndex { receiver, index }
        | smelt_hir::CallbackExprKind::HasDynamicField {
            receiver,
            field: index,
        }
        | smelt_hir::CallbackExprKind::Binary {
            lhs: receiver,
            rhs: index,
            ..
        } => {
            callback_uses_param(types, receiver, needle)
                || callback_uses_param(types, index, needle)
        }
        smelt_hir::CallbackExprKind::Sequence { effects, result } => {
            effects
                .iter()
                .any(|item| callback_uses_param(types, item, needle))
                || callback_uses_param(types, result, needle)
        }
        smelt_hir::CallbackExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => {
            if let Some(value) = callback_literal_bool(cond) {
                return callback_uses_param(
                    types,
                    if value { then_expr } else { else_expr },
                    needle,
                );
            }
            callback_uses_param(types, cond, needle)
                || callback_uses_param(types, then_expr, needle)
                || callback_uses_param(types, else_expr, needle)
        }
        smelt_hir::CallbackExprKind::Call { callee, args } => {
            callback_uses_param(types, callee, needle)
                || args
                    .iter()
                    .any(|arg| callback_uses_param(types, &arg.expr, needle))
        }
        smelt_hir::CallbackExprKind::MethodCall { receiver, args, .. } => {
            callback_uses_param(types, receiver, needle)
                || args
                    .iter()
                    .any(|arg| callback_uses_param(types, &arg.expr, needle))
        }
        smelt_hir::CallbackExprKind::ListLit(items) => items
            .iter()
            .any(|item| callback_uses_param(types, item, needle)),
        smelt_hir::CallbackExprKind::DictLit(entries) => entries
            .iter()
            .any(|(_, value)| callback_uses_param(types, value, needle)),
        smelt_hir::CallbackExprKind::Throw { message: None }
        | smelt_hir::CallbackExprKind::Capture(_)
        | smelt_hir::CallbackExprKind::Function(_)
        | smelt_hir::CallbackExprKind::Literal(_) => false,
    }
}

/// Extracts a static boolean value from a callback expression when it is literal.
fn callback_literal_bool(expr: &smelt_hir::CallbackExpr) -> Option<bool> {
    match &expr.kind {
        smelt_hir::CallbackExprKind::Literal(smelt_hir::Literal::Bool(value)) => Some(*value),
        _ => None,
    }
}

/// Return whether the emitted async block body leaves a future as its final value.
fn async_body_returns_future_value(body_text: &str) -> bool {
    let mut non_empty_lines = body_text.lines().rev().filter_map(|line| {
        let trimmed = line.trim().trim_end_matches(';');
        (!trimmed.is_empty()).then_some(trimmed)
    });
    let Some(final_expr) = non_empty_lines.next() else {
        return false;
    };
    if final_expr == "}" {
        return non_empty_lines.any(|line| {
            line.contains("as ::std::pin::Pin<Box<dyn ::std::future::Future<Output =")
        });
    }
    body_text.lines().any(|line| {
        line.trim_start().starts_with(&format!(
            "let {final_expr}: ::std::pin::Pin<Box<dyn ::std::future::Future"
        ))
    })
}

/// Rewrites emitted closure text so shared captures use their `RefCell` storage.
///
/// Some closure emission paths wrap an already-rendered closure with a capture
/// prelude. The prelude creates `smelt_capture_<name>`, but the rendered body
/// can still mention the source name. This textual pass is intentionally
/// limited to identifier-boundary replacements for those wrapper-only cases.
fn replace_shared_capture_uses(mut text: String, replacements: &[(String, String)]) -> String {
    for (source, target) in replacements {
        text = replace_identifier(&text, source, target);
    }
    text
}

/// Replaces complete Rust identifier occurrences in generated text.
fn replace_identifier(text: &str, source: &str, target: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while let Some(offset) = text[index..].find(source) {
        let start = index.saturating_add(offset);
        let end = start.saturating_add(source.len());
        out.push_str(&text[index..start]);
        let before = text[..start].chars().next_back();
        let after = text[end..].chars().next();
        if before.is_some_and(is_rust_ident_char) || after.is_some_and(is_rust_ident_char) {
            out.push_str(source);
        } else {
            out.push_str(target);
        }
        index = end;
    }
    out.push_str(&text[index..]);
    out
}

/// Returns true for characters that can be part of emitted Rust identifiers.
fn is_rust_ident_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}
