//! Call emission helpers.

use super::*;

impl FunctionEmitter<'_> {
    /// Converts a runtime-backed async operation to Rust.
    pub(super) fn async_op_text(
        &self,
        op: smelt_hir::AsyncOp,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        match op {
            smelt_hir::AsyncOp::All | smelt_hir::AsyncOp::AllSettled => {
                if let [arg] = args
                    && self.async_list_operand_item_type(arg)?.is_some()
                {
                    let list = self.await_operand_text(arg)?;
                    return Ok(format!(
                        "Box::pin(async move {{ let mut __smelt_values = Vec::new(); for __smelt_future in {list} {{ __smelt_values.push(__smelt_future.await); }} __smelt_values }})"
                    ));
                }
                let rendered_args = args
                    .iter()
                    .map(|arg| self.await_operand_text(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let body = match rendered_args.as_slice() {
                    [] => "()".to_owned(),
                    [single] => format!("({single}.await,)"),
                    _ => format!("tokio::join!({})", rendered_args.join(", ")),
                };
                Ok(format!("Box::pin(async move {{ {body} }})"))
            }
            smelt_hir::AsyncOp::Race => {
                let rendered_args = args
                    .iter()
                    .map(|arg| self.await_operand_text(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                let body = match rendered_args.as_slice() {
                    [] => {
                        return Err(EmitError::new(
                            "async race requires at least one future operand",
                        ));
                    }
                    [single] => format!("{single}.await"),
                    _ => {
                        let arms = rendered_args
                            .iter()
                            .map(|arg| format!("value = {arg} => value"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("tokio::select! {{ {arms} }}")
                    }
                };
                Ok(format!("Box::pin(async move {{ {body} }})"))
            }
            smelt_hir::AsyncOp::Sleep => {
                let Some(duration) = args.first() else {
                    return Err(EmitError::new("async sleep requires a duration operand"));
                };
                Ok(format!(
                    "Box::pin(async move {{ tokio::time::sleep(::std::time::Duration::from_millis({} as u64)).await; }})",
                    self.operand_text(duration)?
                ))
            }
            smelt_hir::AsyncOp::CreateTask => {
                let Some(future) = args.first() else {
                    return Err(EmitError::new(
                        "async task creation requires a future operand",
                    ));
                };
                let future_text = self.await_operand_text(future)?;
                Ok(format!(
                    "Box::pin(async move {{ tokio::spawn(async move {{ {future_text}.await }}).await.expect(\"async task panicked\") }})"
                ))
            }
            smelt_hir::AsyncOp::WaitFor => {
                let [future, timeout, ..] = args else {
                    return Err(EmitError::new(
                        "async wait_for requires a future and timeout operand",
                    ));
                };
                let future_text = self.await_operand_text(future)?;
                Ok(format!(
                    "Box::pin(async move {{ tokio::time::timeout(::std::time::Duration::from_millis({} as u64), {future_text}).await.expect(\"async timeout\") }})",
                    self.operand_text(timeout)?
                ))
            }
            smelt_hir::AsyncOp::HttpGetText => {
                let Some(url) = args.first() else {
                    return Err(EmitError::new("async HTTP GET requires a URL operand"));
                };
                if !matches!(
                    self.mir.types.get(self.operand_ty(url)?),
                    Some(Type::String)
                ) {
                    return Err(EmitError::new("async HTTP GET URL must be a string"));
                }
                Ok(format!(
                    "Box::pin(async move {{ reqwest::get({}).await.expect(\"HTTP GET failed\").text().await.expect(\"HTTP response body read failed\") }})",
                    self.operand_text(url)?
                ))
            }
        }
    }

    /// Return the future item type when an async combinator operand is a list of futures.
    fn async_list_operand_item_type(&self, operand: &Operand) -> Result<Option<TypeId>, EmitError> {
        let operand_ty = self.operand_ty(operand)?;
        let Some(Type::List(item_ty)) = self.mir.types.get(operand_ty) else {
            return Ok(None);
        };
        if matches!(self.mir.types.get(*item_ty), Some(Type::Future(_))) {
            Ok(Some(*item_ty))
        } else {
            Ok(None)
        }
    }

    /// Converts an operand for len() to its Rust text representation.
    /// Converts a function call to its Rust text representation.
    pub(super) fn call_text(&self, callee: &Callee, args: &[Operand]) -> Result<String, EmitError> {
        match callee {
            Callee::Builtin(BuiltinFn::ConsoleLog) => {
                let rendered_args = args
                    .iter()
                    .map(|arg| self.console_arg_text(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                if rendered_args.is_empty() {
                    Ok("{ println!(); }".to_owned())
                } else {
                    let format = rendered_args
                        .iter()
                        .map(|(format, _)| *format)
                        .collect::<Vec<_>>()
                        .join(" ");
                    let arg_values = rendered_args
                        .into_iter()
                        .map(|(_, value)| value)
                        .collect::<Vec<_>>()
                        .join(", ");
                    Ok(format!("{{ println!(\"{format}\", {arg_values}); }}"))
                }
            }
            Callee::Static(func) => {
                let function = self
                    .mir
                    .functions
                    .get(id_index(func.0, "function index does not fit usize")?)
                    .ok_or_else(|| EmitError::new("call references an unknown function"))?;
                if let HirOrigin::ClassConstructor { class, .. } = function.origin {
                    let class_name = sanitize_ident(self.symbol_name(class)?);
                    let arg_values = args
                        .iter()
                        .map(|arg| self.operand_text(arg))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    return Ok(format!(
                        "{class_name}::new({arg_values}){}",
                        self.throwing_call_suffix(function)
                    ));
                }
                if let HirOrigin::ClassMethod { method, .. } = function.origin {
                    let Some((receiver, rest)) = args.split_first() else {
                        return Err(EmitError::new("method call is missing a receiver"));
                    };
                    let receiver_text = match receiver {
                        Operand::Copy(place) | Operand::Move(place) => self.place_text(place)?,
                        Operand::Const(_) => {
                            return Err(EmitError::new("method receiver cannot be a constant"));
                        }
                    };
                    let arg_values = rest
                        .iter()
                        .map(|arg| self.operand_text(arg))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    let method_name = sanitize_ident(self.symbol_name(method)?);
                    return if arg_values.is_empty() {
                        Ok(format!(
                            "{receiver_text}.{method_name}(){}",
                            self.throwing_call_suffix(function)
                        ))
                    } else {
                        Ok(format!(
                            "{receiver_text}.{method_name}({arg_values}){}",
                            self.throwing_call_suffix(function)
                        ))
                    };
                }
                let name = self.symbol_name(function.name)?;
                let arg_values = args
                    .iter()
                    .zip(function.params.iter())
                    .map(|(arg, param)| {
                        let local = self.local_decl(*param)?;
                        self.operand_as_type_text(arg, local.ty)
                    })
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                Ok(format!(
                    "{}({arg_values}){}",
                    sanitize_ident(name),
                    self.throwing_call_suffix(function)
                ))
            }
            Callee::Indirect(_) => Err(EmitError::new("indirect calls are not implemented yet")),
        }
    }

    /// Returns the Rust suffix needed when calling a throwing function.
    /// Converts an operand to console.log argument format and returns format string and value.
    pub(super) fn console_arg_text(
        &self,
        operand: &Operand,
    ) -> Result<(&'static str, String), EmitError> {
        if self.operand_ty(operand)? == self.none_ty {
            Ok(("{}", "\"null\"".to_owned()))
        } else if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::List(_) | Type::Dict(_, _) | Type::Tuple(_) | Type::Optional(_))
        ) {
            Ok(("{:?}", self.operand_text(operand)?))
        } else {
            Ok(("{}", self.operand_text(operand)?))
        }
    }

    /// Converts a match scrutinee operand to its Rust text representation.
    /// Emit a class-instance test.
    ///
    /// Smelt keeps this as an MIR rvalue instead of folding it in the
    /// TypeScript frontend so future dynamic representations can swap this
    /// implementation for a runtime tag check. With today's concrete class
    /// lowering, the operand type is statically known, so codegen emits a
    /// boolean after the operand has already been evaluated by MIR lowering.
    pub(super) fn instance_of_text(
        &self,
        value: &Operand,
        class: Symbol,
    ) -> Result<String, EmitError> {
        let value_ty = self.operand_ty(value)?;
        let result = match self.mir.types.get(value_ty) {
            Some(Type::Class { name, .. }) => self.class_extends_or_equals(*name, class),
            _ => false,
        };
        Ok(result.to_string())
    }

    /// Return whether `source` is the same as or derives from `target`.
    /// Return whether `source` is the same as or derives from `target`.
    pub(super) fn class_extends_or_equals(&self, source: Symbol, target: Symbol) -> bool {
        let mut current = Some(source);
        while let Some(class_name) = current {
            if class_name == target {
                return true;
            }
            current = self
                .mir
                .classes
                .iter()
                .find(|class| class.name == class_name)
                .and_then(|class| class.base);
        }
        false
    }

    // Finds the type ID for a given type.
}
