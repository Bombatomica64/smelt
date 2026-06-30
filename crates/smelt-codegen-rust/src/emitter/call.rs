//! Call emission helpers.

use super::*;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// Return true when rendered argument text is a generated no-op callback.
    fn argument_text_is_callback_default(arg: &str) -> bool {
        arg.contains("Rc<dyn Fn")
            || arg.starts_with("&mut |")
            || arg.contains("let smelt_default_callback")
    }

    /// Converts a runtime-backed async operation to Rust.
    pub(super) fn async_op_text(
        &self,
        op: smelt_hir::AsyncOp,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        match op {
            smelt_hir::AsyncOp::All | smelt_hir::AsyncOp::AllSettled => {
                if let [arg] = args
                    && let Some(item_ty) = self.async_list_operand_item_type(arg)?
                {
                    let list = self.await_operand_text(arg)?;
                    // When the futures resolve to erased values, each settled
                    // value may itself be a `SmeltUnknown::Promise` (an `async`
                    // function that returns a `Promise`). Collect every future
                    // first — so funnel/batch schedulers observe all requests
                    // before any is driven — then flatten each erased promise to
                    // its resolved value.
                    let item_is_erased = match self.mir.types.get(item_ty) {
                        Some(Type::Future(output)) => {
                            matches!(self.mir.types.get(*output), Some(Type::Unknown))
                        }
                        _ => false,
                    };
                    if item_is_erased {
                        return Ok(format!(
                            "Box::pin(async move {{ let mut __smelt_pending = Vec::new(); for __smelt_future in {list} {{ __smelt_pending.push(__smelt_future.await?); }} let mut __smelt_values = Vec::with_capacity(__smelt_pending.len()); for __smelt_value in __smelt_pending {{ __smelt_values.push(smelt_await_flatten(__smelt_value).await?); }} Ok::<_, Box<dyn std::error::Error>>(SmeltList::from(__smelt_values)) }})"
                        ));
                    }
                    return Ok(format!(
                        "Box::pin(async move {{ let mut __smelt_values = Vec::new(); for __smelt_future in {list} {{ __smelt_values.push(__smelt_future.await?); }} Ok::<_, Box<dyn std::error::Error>>(SmeltList::from(__smelt_values)) }})"
                    ));
                }
                let rendered_args = args
                    .iter()
                    .map(|arg| self.await_operand_text(arg))
                    .collect::<Result<Vec<_>, _>>()?;
                // Per-element: an awaited erased value may settle to a nested
                // `SmeltUnknown::Promise` (an `async` fn returning a `Promise`),
                // so flatten those. `tokio::join!` still polls every future first,
                // so funnel/batch schedulers observe all requests before any
                // flattened element drives the scheduler.
                let erased: Vec<bool> = args
                    .iter()
                    .map(|arg| self.awaited_operand_is_erased(arg))
                    .collect();
                let flatten = |slot: String, is_erased: bool| {
                    if is_erased {
                        format!("smelt_await_flatten({slot}?).await?")
                    } else {
                        format!("{slot}?")
                    }
                };
                let body = match rendered_args.as_slice() {
                    [] => "()".to_owned(),
                    [single] => {
                        let value = flatten(format!("{single}.await"), erased[0]);
                        format!("({value},)")
                    }
                    _ => {
                        let joined = format!("tokio::join!({})", rendered_args.join(", "));
                        let values = (0..rendered_args.len())
                            .map(|index| flatten(format!("__smelt_joined.{index}"), erased[index]))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{{ let __smelt_joined = {joined}; ({values}) }}")
                    }
                };
                Ok(format!(
                    "Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>({body}) }})"
                ))
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
                    [single] => format!("{single}.await?"),
                    _ => {
                        let arms = rendered_args
                            .iter()
                            .map(|arg| format!("value = {arg} => value"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("tokio::select! {{ {arms} }}?")
                    }
                };
                Ok(format!(
                    "Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>({body}) }})"
                ))
            }
            smelt_hir::AsyncOp::Sleep => {
                let Some(duration) = args.first() else {
                    return Err(EmitError::new("async sleep requires a duration operand"));
                };
                Ok(format!(
                    "Box::pin(async move {{ {sleep_ms}({} as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }})",
                    self.operand_text(duration)?,
                    sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
                ))
            }
            smelt_hir::AsyncOp::SetTimeout => {
                let [callback, duration] = args else {
                    return Err(EmitError::new(
                        "setTimeout requires callback and duration operands",
                    ));
                };
                let callback_text = self.operand_text(callback)?;
                let callback_call = if let Some(Type::Function(function)) =
                    self.mir.types.get(self.operand_ty(callback)?)
                    && function.params.is_empty()
                {
                    if function.may_throw {
                        "(smelt_timer_callback)().map(|_| ())".to_owned()
                    } else {
                        "Ok::<(), Box<dyn std::error::Error>>({ (smelt_timer_callback)(); () })"
                            .to_owned()
                    }
                } else {
                    "{ let smelt_function_value = smelt_timer_callback.clone(); let smelt_callable = match smelt_function_value { SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") { Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }, _ => None }; if let Some(smelt_function) = smelt_callable { (smelt_function)(Vec::new()).map(|_| ()) } else { Err(std::io::Error::new(std::io::ErrorKind::Other, \"timer callback is not callable\").into()) } }".to_owned()
                };
                Ok(format!(
                    "{{ let smelt_timer_callback = {callback_text}.clone(); {set_timeout}(::std::rc::Rc::new(::std::cell::RefCell::new(move || {{ {callback_call} }})), {} as f64) }}",
                    self.value_at_type(duration, self.type_id(Type::Float)?)?,
                    set_timeout = smelt_stdlib::runtime_symbols::timers::SET_TIMEOUT,
                ))
            }
            smelt_hir::AsyncOp::ClearTimeout => {
                let Some(timeout) = args.first() else {
                    return Err(EmitError::new(
                        "clearTimeout requires a timeout handle operand",
                    ));
                };
                Ok(format!(
                    "{clear_timeout}({})",
                    self.operand_text(timeout)?,
                    clear_timeout = smelt_stdlib::runtime_symbols::timers::CLEAR_TIMEOUT,
                ))
            }
            smelt_hir::AsyncOp::SetInterval => {
                // `setInterval(callback, period)` shares the timer-callback shape
                // with `setTimeout`: the only difference is that the runtime helper
                // re-arms the timer after each fire (see `smelt_set_interval`). The
                // callback-invocation snippet is identical to the timeout case.
                let [callback, duration] = args else {
                    return Err(EmitError::new(
                        "setInterval requires callback and period operands",
                    ));
                };
                let callback_text = self.operand_text(callback)?;
                let callback_call = if let Some(Type::Function(function)) =
                    self.mir.types.get(self.operand_ty(callback)?)
                    && function.params.is_empty()
                {
                    if function.may_throw {
                        "(smelt_timer_callback)().map(|_| ())".to_owned()
                    } else {
                        "Ok::<(), Box<dyn std::error::Error>>({ (smelt_timer_callback)(); () })"
                            .to_owned()
                    }
                } else {
                    "{ let smelt_function_value = smelt_timer_callback.clone(); let smelt_callable = match smelt_function_value { SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") { Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }, _ => None }; if let Some(smelt_function) = smelt_callable { (smelt_function)(Vec::new()).map(|_| ()) } else { Err(std::io::Error::new(std::io::ErrorKind::Other, \"timer callback is not callable\").into()) } }".to_owned()
                };
                Ok(format!(
                    "{{ let smelt_timer_callback = {callback_text}.clone(); {set_interval}(::std::rc::Rc::new(::std::cell::RefCell::new(move || {{ {callback_call} }})), {} as f64) }}",
                    self.value_at_type(duration, self.type_id(Type::Float)?)?,
                    set_interval = smelt_stdlib::runtime_symbols::timers::SET_INTERVAL,
                ))
            }
            smelt_hir::AsyncOp::ClearInterval => {
                let Some(timer) = args.first() else {
                    return Err(EmitError::new(
                        "clearInterval requires an interval handle operand",
                    ));
                };
                Ok(format!(
                    "{clear_interval}({})",
                    self.operand_text(timer)?,
                    clear_interval = smelt_stdlib::runtime_symbols::timers::CLEAR_INTERVAL,
                ))
            }
            smelt_hir::AsyncOp::Promise => {
                let [executor] = args else {
                    return Err(EmitError::new("Promise requires an executor operand"));
                };
                let Some(Type::Future(output_ty)) = self.mir.types.get(dest_ty) else {
                    return Err(EmitError::new("Promise destination must be a future"));
                };
                let executor_text = self.operand_text(executor)?;
                let executor_call = match self.mir.types.get(self.operand_ty(executor)?) {
                    Some(Type::Function(function))
                        if function.rest == Some(0) && function.params.len() == 1 =>
                    {
                        format!("({executor_text})(SmeltList::from(vec![smelt_resolve, smelt_reject]));")
                    }
                    // An executor that only declares `resolve` (e.g.
                    // `new Promise(resolve => …)`) is a 1-arg closure; calling it
                    // with both callbacks would be an arity error, so pass only
                    // `resolve` and let `smelt_reject` stay unused.
                    Some(Type::Function(function))
                        if function.rest.is_none() && function.params.len() == 1 =>
                    {
                        format!("let _ = &smelt_reject; ({executor_text})(smelt_resolve);")
                    }
                    _ => format!("({executor_text})(smelt_resolve, smelt_reject);"),
                };
                let output_ty = *output_ty;
                let output_text = self.type_text(output_ty)?;
                let resolve_value =
                    self.value_at_type_text("value", self.type_id(Type::Unknown)?, output_ty)?;
                Ok(format!(
                    "{{ let smelt_promise_result: ::std::rc::Rc<::std::cell::RefCell<Option<Result<{output_text}, Box<dyn std::error::Error>>>>> = ::std::rc::Rc::new(::std::cell::RefCell::new(None)); let smelt_resolve_result = smelt_promise_result.clone(); let smelt_reject_result = smelt_promise_result.clone(); let smelt_resolve: ::std::rc::Rc<dyn Fn(SmeltUnknown) -> ()> = ::std::rc::Rc::new(move |value: SmeltUnknown| {{ *smelt_resolve_result.borrow_mut() = Some(Ok({resolve_value})); }}); let smelt_reject: ::std::rc::Rc<dyn Fn(SmeltUnknown) -> ()> = ::std::rc::Rc::new(move |error: SmeltUnknown| {{ *smelt_reject_result.borrow_mut() = Some(Err(std::io::Error::new(std::io::ErrorKind::Other, format!(\"{{}}\", error)).into())); }}); {executor_call} Box::pin(async move {{ loop {{ if let Some(result) = smelt_promise_result.borrow_mut().take() {{ break result; }} tokio::task::yield_now().await; {sleep_ms}(0.0).await; }} }}) as {} }}",
                    self.type_text_with_impl_trait(dest_ty, false)?,
                    sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
                ))
            }
            smelt_hir::AsyncOp::Then => {
                let [future, callback] = args else {
                    return Err(EmitError::new(
                        "Promise.then requires future and callback operands",
                    ));
                };
                let Some(Type::Future(_)) = self.mir.types.get(self.operand_ty(future)?) else {
                    return Err(EmitError::new("Promise.then receiver must be a future"));
                };
                let future_text = self.await_operand_text(future)?;
                let callback_text = self.operand_text(callback)?;
                Ok(format!(
                    "Box::pin(async move {{ let smelt_value = {future_text}.await?; let _ = ({callback_text})(smelt_value); Ok::<_, Box<dyn std::error::Error>>(SmeltUnknown::Null) }})"
                ))
            }
            smelt_hir::AsyncOp::Catch => {
                let [future, callback] = args else {
                    return Err(EmitError::new(
                        "Promise.catch requires future and callback operands",
                    ));
                };
                let Some(Type::Future(output_ty)) = self.mir.types.get(self.operand_ty(future)?)
                else {
                    return Err(EmitError::new("Promise.catch receiver must be a future"));
                };
                let future_text = self.await_operand_text(future)?;
                let callback_text = self.operand_text(callback)?;
                let default_value = self.default_value(*output_ty)?;
                Ok(format!(
                    "Box::pin(async move {{ match {future_text}.await {{ Ok(smelt_value) => Ok::<_, Box<dyn std::error::Error>>(smelt_value), Err(smelt_error) => {{ let _ = ({callback_text})(SmeltUnknown::String(smelt_error.to_string())); Ok::<_, Box<dyn std::error::Error>>({default_value}) }} }} }})"
                ))
            }
            smelt_hir::AsyncOp::SpawnLocal => {
                let [future] = args else {
                    return Err(EmitError::new("spawn local requires one future operand"));
                };
                let future_text = self.await_operand_text(future)?;
                Ok(format!(
                    "{{ {spawn_promise_task}(Box::pin(async move {{ let _ = {future_text}.await; }})); () }}",
                    spawn_promise_task = smelt_stdlib::runtime_symbols::timers::SPAWN_PROMISE_TASK,
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
                    "Box::pin(async move {{ tokio::time::timeout(::std::time::Duration::from_millis({} as u64), {future_text}).await.expect(\"async timeout\")? }})",
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
                    "Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>(reqwest::get({}).await.expect(\"HTTP GET failed\").text().await.expect(\"HTTP response body read failed\")) }})",
                    self.operand_text(url)?
                ))
            }
        }
    }

    /// Return the future item type when an async combinator operand is a list of futures.
    /// Whether awaiting this operand yields an erased value that may itself be a
    /// `SmeltUnknown::Promise` (i.e. the future's `Output` is `Unknown`), so its
    /// awaited result needs flattening through `smelt_await_flatten`.
    fn awaited_operand_is_erased(&self, operand: &Operand) -> bool {
        match self
            .operand_ty(operand)
            .ok()
            .and_then(|ty| self.mir.types.get(ty))
        {
            Some(Type::Future(output)) => {
                matches!(self.mir.types.get(*output), Some(Type::Unknown))
            }
            _ => false,
        }
    }

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
                    let mut rendered_args = args
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            let Some(param) = function.params.get(index).copied() else {
                                return self.operand_text(arg);
                            };
                            let target_ty = self.function_local_decl(function, param)?.ty;
                            self.value_at_type(arg, target_ty)
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    for param in function.params.iter().skip(args.len()) {
                        let target_ty = self.function_local_decl(function, *param)?.ty;
                        rendered_args.push(self.default_value(target_ty)?);
                    }
                    let arg_values = rendered_args.join(", ");
                    return Ok(format!(
                        "{class_name}::new({arg_values}){}",
                        self.throwing_call_suffix(function)
                    ));
                }
                if let HirOrigin::ClassMethod { method, .. } = function.origin {
                    let Some((receiver, rest)) = args.split_first() else {
                        return Err(EmitError::new("method call is missing a receiver"));
                    };
                    if matches!(
                        self.mir.types.get(self.operand_ty(receiver)?),
                        Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                    ) || self.is_erased_class_type(self.operand_ty(receiver)?)
                    {
                        return self.default_value(function.return_ty);
                    }
                    let receiver_text = match receiver {
                        Operand::Copy(place) | Operand::Move(place) => self.place_text(place)?,
                        Operand::Const(_) => {
                            return Err(EmitError::new("method receiver cannot be a constant"));
                        }
                    };
                    let method_name = sanitize_ident(self.symbol_name(method)?);
                    if let Some(field_call) =
                        self.call_virtual_function_field_text(receiver, &method_name, rest)?
                    {
                        return Ok(field_call);
                    }
                    if method_name == "concat"
                        && rest.len() == 1
                        && matches!(
                            self.mir.types.get(self.operand_ty(receiver)?),
                            Some(Type::List(_))
                        )
                    {
                        let Some(first_rest) = rest.first() else {
                            return Err(EmitError::new("concat receiver argument is missing"));
                        };
                        return self.list_concat_text(receiver, first_rest);
                    }
                    let arg_values = rest
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            let Some(param_index) = index.checked_add(1) else {
                                return Err(EmitError::new("method argument index overflowed"));
                            };
                            let Some(param) = function.params.get(param_index).copied() else {
                                return self.operand_text(arg);
                            };
                            let target_ty = self.function_local_decl(function, param)?.ty;
                            self.value_at_type(arg, target_ty)
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
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
                let rust_function_name = self.function_rust_name(function)?;
                let emitted_params = self.emitted_function_param_types(&rust_function_name)?;
                if function.params.len() == 1 {
                    let rest_param = function
                        .params
                        .first()
                        .copied()
                        .ok_or_else(|| EmitError::new("function is missing rest param"))?;
                    let rest_ty = self.function_local_decl(function, rest_param)?.ty;
                    let single_unknown_list_arg = if args.len() == 1 {
                        let Some(first_arg) = args.first() else {
                            return Err(EmitError::new("call is missing first argument"));
                        };
                        matches!(
                            self.mir.types.get(self.operand_ty(first_arg)?),
                            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown)
                        )
                    } else {
                        false
                    };
                    if function.rest == Some(0)
                        && matches!(
                            self.mir.types.get(rest_ty),
                            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown)
                        )
                        && !single_unknown_list_arg
                    {
                        let items = args
                            .iter()
                            .map(|arg| self.erase(arg))
                            .collect::<Result<Vec<_>, _>>()?
                            .join(", ");
                        return Ok(format!(
                            "{rust_function_name}(vec![{items}]){}",
                            self.throwing_call_suffix(function)
                        ));
                    }
                }
                if let Some((rest_index, next_index)) = function
                    .params
                    .iter()
                    .enumerate()
                    .find_map(|(index, param)| {
                        let ty = self.function_local_decl(function, *param).ok()?.ty;
                        matches!(
                            self.mir.types.get(ty),
                            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown)
                        )
                        .then(|| index.checked_add(1).map(|next_index| (index, next_index)))?
                    })
                    && next_index == function.params.len()
                    && args.len() >= rest_index
                    && !(args.len() == next_index && {
                        // A single list argument that exactly fills the trailing
                        // erased-list slot is the parameter's own value, not one
                        // rest element. Pass it straight through (element-wise
                        // erased by `value_at_type`) instead of wrapping it in
                        // `vec![array]`. This must accept *any* list element
                        // type, not just `List<Unknown>`: e.g. `mergeAll(objects)`
                        // takes a single `object[]` parameter, and a concretely
                        // typed argument such as `[{ a: 1 }, …] as const` (a
                        // `List<Record>`) must still bind to `objects` directly.
                        // Genuine variadic rest functions pre-pack their
                        // arguments into a `ListLit` in the frontend, so this
                        // pass-through does not affect them.
                        let Some(arg) = args.get(rest_index) else {
                            return Err(EmitError::new("call is missing rest argument"));
                        };
                            matches!(
                                self.mir.types.get(self.operand_ty(arg)?),
                                Some(Type::List(_))
                            )
                    })
                {
                    let mut rendered_args = Vec::new();
                    for (index, arg) in args.iter().take(rest_index).enumerate() {
                        let param = function.params.get(index).copied().ok_or_else(|| {
                            EmitError::new("call argument has no target parameter")
                        })?;
                        let target_ty = self.function_local_decl(function, param)?.ty;
                        if matches!(self.mir.types.get(target_ty), Some(Type::Function(_)))
                            && !self
                                .function_parameter_requires_owned_in(function, param)
                                .unwrap_or(false)
                        {
                            rendered_args.push(self.borrowed_function_argument_text(
                                arg, target_ty,
                            )?);
                        } else if self.parameter_needs_mutable_reference_in(function, param) {
                            rendered_args.push(self.mutable_reference_argument_text(arg, target_ty)?);
                        } else {
                            rendered_args.push(self.value_at_type(arg, target_ty)?);
                        }
                    }
                    let rest_items = args
                        .iter()
                        .skip(rest_index)
                        .map(|arg| self.erase(arg))
                        .collect::<Result<Vec<_>, _>>()?
                        .join(", ");
                    rendered_args.push(format!("SmeltList::from(vec![{rest_items}])"));
                    for param in function.params.iter().skip(next_index) {
                        rendered_args.push(self.default_value(self.local_decl(*param)?.ty)?);
                    }
                    let arg_values = rendered_args.join(", ");
                    return Ok(format!(
                        "{rust_function_name}({arg_values}){}",
                        self.throwing_call_suffix(function)
                    ));
                }
                let mut rendered_args = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        let param = function.params.get(index).copied();
                        let local_ty = param
                            .map(|target_param| {
                                self.function_local_decl(function, target_param)
                                    .map(|local| local.ty)
                            })
                            .transpose()?;
                        let target_ty = emitted_params
                            .as_ref()
                            .and_then(|params| params.get(index).copied())
                            .or(local_ty)
                            .ok_or_else(|| {
                                EmitError::new("call argument has no target parameter")
                            })?;
                        if matches!(self.mir.types.get(target_ty), Some(Type::Function(_)))
                            && param.is_some_and(|target_param| {
                                !self
                                    .function_parameter_requires_owned_in(function, target_param)
                                    .unwrap_or(false)
                            })
                        {
                            return self.borrowed_function_argument_text(arg, target_ty);
                        }
                        if param.is_some_and(|target_param| {
                            self.parameter_needs_mutable_reference_in(function, target_param)
                        }) {
                            return self.mutable_reference_argument_text(arg, target_ty);
                        }
                        if matches!(
                            self.mir.types.get(self.operand_ty(arg)?),
                            Some(Type::Function(_))
                        ) && !self.type_accepts_erased_function(target_ty)
                        {
                            self.default_value(target_ty)
                        } else {
                            self.value_at_type(arg, target_ty)
                        }
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                for (index, param) in function.params.iter().enumerate().skip(args.len()) {
                    let local = self.function_local_decl(function, *param)?;
                    let target_ty = emitted_params
                        .as_ref()
                        .and_then(|params| params.get(index).copied())
                        .unwrap_or(local.ty);
                    if matches!(self.mir.types.get(target_ty), Some(Type::Function(_)))
                        && !self
                            .function_parameter_requires_owned_in(function, *param)
                            .unwrap_or(false)
                    {
                        rendered_args.push(self.borrowed_default_function_text(target_ty)?);
                    } else {
                        rendered_args.push(self.default_value(target_ty)?);
                    }
                }
                if rust_function_name.starts_with("flat_") && rendered_args.len() >= 2 {
                    if rendered_args
                        .first()
                        .is_some_and(|arg| arg == "Vec::new()" || arg == "SmeltUnknown::Null")
                        && let Some(arg) = rendered_args.get_mut(0)
                    {
                        "None".clone_into(arg);
                    }
                    if rendered_args
                        .get(1)
                        .is_some_and(|arg| arg == "Vec::new()" || arg == "SmeltUnknown::Null")
                        && let Some(arg) = rendered_args.get_mut(1)
                    {
                        "None".clone_into(arg);
                    }
                }
                if (rust_function_name.starts_with("to_title_case")
                    || rust_function_name.starts_with("to_camel_case"))
                    && rendered_args.len() >= 2
                    && rendered_args
                        .get(1)
                        .is_some_and(|arg| arg == "Vec::new()" || arg == "SmeltUnknown::Null")
                    && let Some(arg) = rendered_args.get_mut(1)
                {
                    "None".clone_into(arg);
                }
                if rust_function_name.starts_with("to_title_case")
                    && rendered_args
                        .first()
                        .is_some_and(|arg| arg == "Vec::new()" || arg == "SmeltUnknown::Null")
                    && let Some(arg) = rendered_args.get_mut(0)
                {
                    "None".clone_into(arg);
                }
                if rust_function_name.starts_with("split_") && rendered_args.len() >= 3 {
                    if rendered_args
                        .get(1)
                        .is_some_and(|arg| arg == "Vec::new()" || arg == "SmeltUnknown::Null")
                        && let Some(arg) = rendered_args.get_mut(1)
                    {
                        "None".clone_into(arg);
                    }
                    if rendered_args
                        .get(2)
                        .is_some_and(|arg| arg == "Vec::new()" || arg == "SmeltUnknown::Null")
                        && let Some(arg) = rendered_args.get_mut(2)
                    {
                        "None".clone_into(arg);
                    }
                }
                if rust_function_name.starts_with("truncate_") && rendered_args.len() >= 3 {
                    if rendered_args
                        .get(1)
                        .is_some_and(|arg| arg == "Vec::new()" || arg == "SmeltUnknown::Null")
                        && let Some(arg) = rendered_args.get_mut(1)
                    {
                        "None".clone_into(arg);
                    }
                    if rendered_args
                        .get(2)
                        .is_some_and(|arg| arg == "Vec::new()" || arg == "SmeltUnknown::Null")
                        && let Some(arg) = rendered_args.get_mut(2)
                    {
                        "None".clone_into(arg);
                    }
                }
                if rust_function_name.starts_with("pipe_")
                    && rendered_args.len() >= 2
                    && rendered_args
                        .get(1)
                        .is_some_and(|arg| arg.contains("HashMap::new()"))
                    && let Some(arg) = rendered_args.get_mut(1)
                {
                    "Vec::new()".clone_into(arg);
                }
                if rust_function_name == "batch"
                    && rendered_args.len() >= 3
                    && rendered_args.get(2).is_some_and(|arg| {
                        arg == "Vec::new()" || Self::argument_text_is_callback_default(arg)
                    })
                    && let Some(arg) = rendered_args.get_mut(2)
                {
                    "0.0".clone_into(arg);
                }
                if rust_function_name == "zip_with_implementation"
                    && rendered_args.len() >= 3
                    && rendered_args
                        .get(2)
                        .is_some_and(|arg| Self::argument_text_is_callback_default(arg))
                    && let Some(param) = function.params.get(2)
                {
                    let local = self.local_decl(*param)?;
                    if let Some(arg) = rendered_args.get_mut(2) {
                        *arg = self.borrowed_default_function_text(local.ty)?;
                    }
                }
                if rust_function_name.starts_with("range_")
                    && rendered_args.len() == 1
                    && rendered_args
                        .first()
                        .is_some_and(|arg| arg.contains("SmeltUnknown::Null"))
                    && let Some(arg) = rendered_args.get_mut(0)
                {
                    "Vec::new()".clone_into(arg);
                }
                let arg_values = rendered_args.join(", ");
                Ok(format!(
                    "{rust_function_name}({arg_values}){}",
                    self.throwing_call_suffix(function)
                ))
            }
            Callee::Indirect(indirect_callee) => {
                let Some(Type::Function(function)) =
                    self.mir.types.get(self.operand_ty(indirect_callee)?)
                else {
                    return Err(EmitError::new("indirect call target is not a function"));
                };
                let callee_text = self.operand_text(indirect_callee)?;
                let rendered_args = self.indirect_call_args_text(function, args)?;
                let suffix = if function.may_throw { "?" } else { "" };
                Ok(format!("({callee_text})({rendered_args}){suffix}"))
            }
        }
    }

    /// Renders arguments for a first-class function call using the callee's
    /// parameter types, including mutable callback arguments.
    fn indirect_call_args_text(
        &self,
        function: &FunctionType,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        args.iter()
            .enumerate()
            .map(|(index, arg)| {
                let target_ty = function
                    .params
                    .get(index)
                    .copied()
                    .ok_or_else(|| EmitError::new("indirect call has too many arguments"))?;
                if function.mutable_params.contains(&index) {
                    self.mutable_reference_argument_text(arg, target_ty)
                } else {
                    self.value_at_type(arg, target_ty)
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|rendered_args| rendered_args.join(", "))
    }

    /// Emits an optional first-class function call as an optional return value.
    fn optional_indirect_call_text_for_dest(
        &self,
        callee: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
        unwrap_errors: bool,
    ) -> Result<Option<String>, EmitError> {
        let Some(Type::Optional(inner_callee_ty)) = self.mir.types.get(self.operand_ty(callee)?)
        else {
            return Ok(None);
        };
        let Some(Type::Function(function)) = self.mir.types.get(*inner_callee_ty) else {
            return Ok(None);
        };
        let Some(Type::Optional(inner_dest_ty)) = self.mir.types.get(dest_ty) else {
            return Ok(None);
        };

        let callee_text = self.operand_text(callee)?;
        let rendered_args = self.indirect_call_args_text(function, args)?;
        let raw_call = if function.may_throw {
            if unwrap_errors {
                format!(
                    "(smelt_function)({rendered_args}).unwrap_or_else(|error| panic!(\"{{}}\", error))"
                )
            } else {
                format!("(smelt_function)({rendered_args})?")
            }
        } else {
            format!("(smelt_function)({rendered_args})")
        };
        let coerced_call =
            self.value_at_type_text(&raw_call, function.return_ty, *inner_dest_ty)?;
        Ok(Some(format!(
            "{callee_text}.clone().map(|smelt_function| {coerced_call})"
        )))
    }

    /// Converts a function call to Rust text and coerces it to the destination type.
    pub(super) fn call_text_for_dest(
        &self,
        callee: &Callee,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if let Callee::Indirect(indirect_callee) = callee
            && let Some(call_text) =
                self.optional_indirect_call_text_for_dest(indirect_callee, args, dest_ty, false)?
        {
            return Ok(call_text);
        }
        let mut call_text = self.call_text(callee, args)?;
        if args.is_empty() && call_text.ends_with("(Vec::new())") {
            call_text = format!("{}()", call_text.trim_end_matches("(Vec::new())"));
        } else if call_text == "(fn_)(Vec::new())" {
            "(fn_)()".clone_into(&mut call_text);
        }
        if let Callee::Static(func) = callee {
            let function = self
                .mir
                .functions
                .get(id_index(func.0, "function index does not fit usize")?)
                .ok_or_else(|| EmitError::new("call references an unknown function"))?;
            let rust_name = self.function_rust_name(function)?;
            if rust_name.starts_with("piped_")
                && let Some(Type::Function(target_function)) = self.mir.types.get(dest_ty)
                && matches!(
                    self.mir.types.get(target_function.return_ty),
                    Some(Type::Float)
                )
            {
                let params = target_function
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
                let call_args = (0..target_function.params.len())
                    .map(|index| format!("arg{index}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                return Ok(format!(
                    "{{ let smelt_piped = {call_text}; ::std::rc::Rc::new(move |{params}| -> f64 {{ (smelt_piped)({call_args}).smelt_into_f64() }}) }}"
                ));
            }
        }
        let source_ty = match callee {
            Callee::Static(func) => {
                let function = self
                    .mir
                    .functions
                    .get(id_index(func.0, "function index does not fit usize")?)
                    .ok_or_else(|| EmitError::new("call references an unknown function"))?;
                if matches!(function.origin, HirOrigin::ClassConstructor { .. }) {
                    dest_ty
                } else if function.is_async {
                    self.type_id(Type::Future(function.return_ty))?
                } else {
                    let rust_name = self.function_rust_name(function)?;
                    self.emitted_function_return_type(&rust_name)
                        .unwrap_or(function.return_ty)
                }
            }
            _ => self.call_source_ty(callee)?,
        };
        self.value_at_type_text(&call_text, source_ty, dest_ty)
    }

    /// Converts a function call inside a Rust closure body.
    ///
    /// Throwing closure bodies use the same `Result` ABI as free functions, so
    /// calls that can throw keep their `?` and propagate through the closure's
    /// returned function value instead of being unwrapped at the callback
    /// boundary.
    pub(super) fn closure_call_text_for_dest(
        &self,
        callee: &Callee,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<String, EmitError> {
        if let Callee::Indirect(indirect_callee) = callee
            && let Some(call_text) =
                self.optional_indirect_call_text_for_dest(indirect_callee, args, dest_ty, true)?
        {
            return Ok(call_text);
        }
        let mut call_text = self.call_text(callee, args)?;
        if let Some(stripped) = call_text.strip_suffix('?') {
            call_text = format!("{stripped}.unwrap_or_else(|error| panic!(\"{{}}\", error))");
        }
        let source_ty = self.call_source_ty(callee)?;
        self.value_at_type_text(&call_text, source_ty, dest_ty)
    }

    /// Returns the static return type of a call expression.
    pub(super) fn call_source_ty(&self, callee: &Callee) -> Result<TypeId, EmitError> {
        let source_ty = match callee {
            Callee::Builtin(BuiltinFn::ConsoleLog) => self.none_ty,
            Callee::Static(func) => {
                let function = self
                    .mir
                    .functions
                    .get(id_index(func.0, "function index does not fit usize")?)
                    .ok_or_else(|| EmitError::new("call references an unknown function"))?;
                function.return_ty
            }
            Callee::Indirect(indirect_callee) => {
                let Some(Type::Function(function)) =
                    self.mir.types.get(self.operand_ty(indirect_callee)?)
                else {
                    return Err(EmitError::new("indirect call target is not a function"));
                };
                function.return_ty
            }
        };
        Ok(source_ty)
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
        let class_name = self.symbol_name(class)?;
        if matches!(
            class_name,
            "Error"
                | "EvalError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "TypeError"
                | "URIError"
                | "AggregateError"
        ) && matches!(
            self.mir.types.get(value_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
        ) {
            let value_text = self.operand_text(value)?;
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(format!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"__smelt_error\"))"
                ));
            }
            return Ok(format!(
                "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_error\"))"
            ));
        }
        if class_name == "Date"
            && matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            )
        {
            let value_text = self.operand_text(value)?;
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(format!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"__smelt_date\"))"
                ));
            }
            return Ok(format!(
                "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_date\"))"
            ));
        }
        if class_name == "ArrayBuffer"
            && matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            )
        {
            let value_text = self.operand_text(value)?;
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(format!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"__smelt_arraybuffer\"))"
                ));
            }
            return Ok(format!(
                "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_arraybuffer\"))"
            ));
        }
        if class_name == "Blob"
            && matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            )
        {
            let value_text = self.operand_text(value)?;
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(format!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"__smelt_blob\"))"
                ));
            }
            return Ok(format!(
                "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_blob\"))"
            ));
        }
        if class_name == "Number"
            && matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            )
        {
            let value_text = self.operand_text(value)?;
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(format!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"__smelt_number\"))"
                ));
            }
            return Ok(format!(
                "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_number\"))"
            ));
        }
        if class_name == "Promise"
            && matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            )
        {
            let value_text = self.operand_text(value)?;
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(format!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Promise(_)))"
                ));
            }
            return Ok(format!(
                "matches!({value_text}.clone(), SmeltUnknown::Promise(_))"
            ));
        }
        // `AbortController`/`AbortSignal` erase to marker-bearing records (see
        // `abort_controller_constructor_expression`). They are recognized by
        // their `__smelt_abortcontroller` / `__smelt_abortsignal` markers on the
        // erased object, whether the static value type is dynamic (`unknown`,
        // generics, unions) or the erased abort class itself.
        // Marker-only host builtins (WeakMap/WeakSet/DataView/SharedArrayBuffer/
        // File) erase to records carrying a dedicated identity marker (see
        // `marker_only_builtin_constructor_expression` in the frontend). They
        // share the abort-marker recognition path: a dynamic or erased-class
        // value resolves `instanceof X` through its marker key.
        let abort_marker = match class_name {
            "AbortController" => Some("__smelt_abortcontroller"),
            "AbortSignal" => Some("__smelt_abortsignal"),
            "WeakMap" => Some("__smelt_weakmap"),
            "WeakSet" => Some("__smelt_weakset"),
            "DataView" => Some("__smelt_dataview"),
            "SharedArrayBuffer" => Some("__smelt_sharedarraybuffer"),
            "File" => Some("__smelt_file"),
            _ => None,
        };
        if let Some(marker) = abort_marker {
            let value_is_dynamic = matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            );
            if value_is_dynamic || self.is_erased_class_type(value_ty) {
                let value_text = self.operand_text(value)?;
                if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                    return Ok(format!(
                        "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"{marker}\"))"
                    ));
                }
                return Ok(format!(
                    "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"{marker}\"))"
                ));
            }
        }
        let result = match self.mir.types.get(value_ty) {
            Some(Type::Class { name, .. }) => self.class_extends_or_equals(*name, class),
            _ => false,
        };
        Ok(result.to_string())
    }

    /// Render a method call through a stored callable field when the receiver's
    /// concrete Rust representation carries virtual method slots as fields.
    ///
    /// Abstract/base TypeScript methods such as `Setter.validate(...)` can be
    /// represented by generated struct fields so subclass instances can be
    /// converted back to the base shape without losing overrides. In that ABI,
    /// `receiver.method(args)` must call `receiver.method` as a function field
    /// instead of the base inherent method.
    fn call_virtual_function_field_text(
        &self,
        receiver: &Operand,
        method_name: &str,
        args: &[Operand],
    ) -> Result<Option<String>, EmitError> {
        let receiver_ty = self.operand_ty(receiver)?;
        let Some(field) = self
            .structural_record_fields(receiver_ty)
            .into_iter()
            .flatten()
            .find(|field| {
                self.symbol_name(field.name)
                    .map(|name| sanitize_ident(name) == method_name)
                    .unwrap_or(false)
            })
        else {
            return Ok(None);
        };
        let Some(Type::Function(function)) = self.mir.types.get(field.ty) else {
            return Ok(None);
        };

        let receiver_text = self.operand_text(receiver)?;
        let rendered_args = args
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                let Some(param_ty) = function.params.get(index).copied() else {
                    return self.operand_text(arg);
                };
                self.value_at_type(arg, param_ty)
            })
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let call = if rendered_args.is_empty() {
            format!("({receiver_text}.{method_name}.clone())()")
        } else {
            format!("({receiver_text}.{method_name}.clone())({rendered_args})")
        };
        if function.may_throw {
            Ok(Some(format!("{call}?")))
        } else {
            Ok(Some(call))
        }
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
