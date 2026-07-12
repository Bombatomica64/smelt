//! Call emission helpers.

use super::*;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
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
                            "SmeltFuture::from_future(Box::pin(async move {{ let mut __smelt_pending = Vec::new(); for __smelt_future in {list} {{ __smelt_pending.push(__smelt_future.await?); }} let mut __smelt_values = Vec::with_capacity(__smelt_pending.len()); for __smelt_value in __smelt_pending {{ __smelt_values.push(smelt_await_flatten(__smelt_value).await?); }} Ok::<_, Box<dyn std::error::Error>>(SmeltList::from(__smelt_values)) }}))"
                        ));
                    }
                    return Ok(format!(
                        "SmeltFuture::from_future(Box::pin(async move {{ let mut __smelt_values = Vec::new(); for __smelt_future in {list} {{ __smelt_values.push(__smelt_future.await?); }} Ok::<_, Box<dyn std::error::Error>>(SmeltList::from(__smelt_values)) }}))"
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
                        let value = flatten(
                            format!("{single}.await"),
                            erased.first().copied().unwrap_or(false),
                        );
                        format!("({value},)")
                    }
                    _ => {
                        let joined = format!("tokio::join!({})", rendered_args.join(", "));
                        let values = (0..rendered_args.len())
                            .map(|index| {
                                flatten(
                                    format!("__smelt_joined.{index}"),
                                    erased.get(index).copied().unwrap_or(false),
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{{ let __smelt_joined = {joined}; ({values}) }}")
                    }
                };
                Ok(format!(
                    "SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>({body}) }}))"
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
                    "SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>({body}) }}))"
                ))
            }
            smelt_hir::AsyncOp::Sleep => {
                let Some(duration) = args.first() else {
                    return Err(EmitError::new("async sleep requires a duration operand"));
                };
                Ok(format!(
                    "SmeltFuture::from_future(Box::pin(async move {{ {sleep_ms}({} as f64).await; Ok::<_, Box<dyn std::error::Error>>(()) }}))",
                    self.operand_text(duration)?,
                    sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
                ))
            }
            smelt_hir::AsyncOp::SetTimeout => {
                let [callback, duration, extra @ ..] = args else {
                    return Err(EmitError::new(
                        "setTimeout requires callback and duration operands",
                    ));
                };
                let callback_text = self.operand_text(callback)?;
                let callback_call = self.timer_callback_call_text(callback, extra.first())?;
                let extra_binding = self.timer_extra_args_binding(extra.first())?;
                Ok(format!(
                    "{{ let smelt_timer_callback = {callback_text}.clone(); {extra_binding}{set_timeout}(::std::rc::Rc::new(::std::cell::RefCell::new(move || {{ {callback_call} }})), {} as f64) }}",
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
                let [callback, duration, extra @ ..] = args else {
                    return Err(EmitError::new(
                        "setInterval requires callback and period operands",
                    ));
                };
                let callback_text = self.operand_text(callback)?;
                let callback_call = self.timer_callback_call_text(callback, extra.first())?;
                let extra_binding = self.timer_extra_args_binding(extra.first())?;
                Ok(format!(
                    "{{ let smelt_timer_callback = {callback_text}.clone(); {extra_binding}{set_interval}(::std::rc::Rc::new(::std::cell::RefCell::new(move || {{ {callback_call} }})), {} as f64) }}",
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
                let Some(&Type::Future(output_ty)) = self.mir.types.get(dest_ty) else {
                    return Err(EmitError::new("Promise destination must be a future"));
                };
                let executor_text = self.operand_text(executor)?;
                let executor_call = match self.mir.types.get(self.operand_ty(executor)?) {
                    Some(Type::Function(function))
                        if function.rest == Some(0) && function.params.len() == 1 =>
                    {
                        format!(
                            "({executor_text})(SmeltList::from(vec![smelt_resolve, smelt_reject]));"
                        )
                    }
                    // An executor that declares no parameters (e.g.
                    // `new Promise(() => {})`) ignores both callbacks; calling it
                    // with either would be an arity error, so invoke it with no
                    // arguments and let both callbacks stay unused.
                    Some(Type::Function(function))
                        if function.rest.is_none() && function.params.is_empty() =>
                    {
                        format!(
                            "let _ = &smelt_resolve; let _ = &smelt_reject; ({executor_text})();"
                        )
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
                let output_text = self.type_text(output_ty)?;
                let resolve_value =
                    self.value_at_type_text("value", self.type_id(Type::Unknown)?, output_ty)?;
                Ok(format!(
                    "{{ let smelt_promise_result: ::std::rc::Rc<::std::cell::RefCell<Option<Result<{output_text}, Box<dyn std::error::Error>>>>> = ::std::rc::Rc::new(::std::cell::RefCell::new(None)); let smelt_resolve_result = smelt_promise_result.clone(); let smelt_reject_result = smelt_promise_result.clone(); let smelt_resolve: ::std::rc::Rc<dyn Fn(SmeltUnknown) -> ()> = ::std::rc::Rc::new(move |value: SmeltUnknown| {{ *smelt_resolve_result.borrow_mut() = Some(Ok({resolve_value})); }}); let smelt_reject: ::std::rc::Rc<dyn Fn(SmeltUnknown) -> ()> = ::std::rc::Rc::new(move |error: SmeltUnknown| {{ *smelt_reject_result.borrow_mut() = Some(Err(std::io::Error::new(std::io::ErrorKind::Other, format!(\"{{}}\", error)).into())); }}); {executor_call} SmeltFuture::from_future(Box::pin(async move {{ loop {{ if let Some(result) = smelt_promise_result.borrow_mut().take() {{ break result; }} tokio::task::yield_now().await; {sleep_ms}(0.0).await; }} }})) }}",
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
                let (prelude, callback_expr) = self.promise_callback_hoist(callback)?;
                let invocation =
                    self.promise_callback_invocation_with(callback, &callback_expr, "smelt_value")?;
                Ok(format!(
                    "{{ {prelude}SmeltFuture::from_future(Box::pin(async move {{ let smelt_value = {future_text}.await?; let _ = {invocation}; Ok::<_, Box<dyn std::error::Error>>(SmeltUnknown::Null) }})) }}"
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
                let (prelude, callback_expr) = self.promise_callback_hoist(callback)?;
                let invocation = self.promise_callback_invocation_with(
                    callback,
                    &callback_expr,
                    "SmeltUnknown::String(smelt_error.to_string())",
                )?;
                let default_value = self.default_value(*output_ty)?;
                Ok(format!(
                    "{{ {prelude}SmeltFuture::from_future(Box::pin(async move {{ match {future_text}.await {{ Ok(smelt_value) => Ok::<_, Box<dyn std::error::Error>>(smelt_value), Err(smelt_error) => {{ let _ = {invocation}; Ok::<_, Box<dyn std::error::Error>>({default_value}) }} }} }})) }}"
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
                    "SmeltFuture::from_future(Box::pin(async move {{ tokio::spawn(async move {{ {future_text}.await }}).await.expect(\"async task panicked\") }}))"
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
                    "SmeltFuture::from_future(Box::pin(async move {{ tokio::time::timeout(::std::time::Duration::from_millis({} as u64), async move {{ {future_text}.await }}).await.expect(\"async timeout\")? }}))",
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
                    "SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>(reqwest::get({}).await.expect(\"HTTP GET failed\").text().await.expect(\"HTTP response body read failed\")) }}))",
                    self.operand_text(url)?
                ))
            }
        }
    }

    /// Render the timer-callback invocation shared by setTimeout/setInterval.
    ///
    /// A statically-typed zero-parameter closure is invoked directly (only
    /// possible without forwarded arguments); every other callback dispatches
    /// through the erased `SmeltUnknown` callable ABI, receiving the packed
    /// extra-argument vector (the `setTimeout(cb, ms, ...args)` tail) bound by
    /// [`Self::timer_extra_args_binding`] when present.
    fn timer_callback_call_text(
        &self,
        callback: &Operand,
        extra: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let callback_ty = self.operand_ty(callback)?;
        if extra.is_none()
            && let Some(Type::Function(function)) = self.mir.types.get(callback_ty)
            && function.params.is_empty()
        {
            return Ok(if function.may_throw {
                "(smelt_timer_callback)().map(|_| ())".to_owned()
            } else {
                "Ok::<(), Box<dyn std::error::Error>>({ (smelt_timer_callback)(); () })"
                    .to_owned()
            });
        }
        // A callback with a statically-known function type (e.g. `delay`'s
        // `(...args: any[]) => any`) is a concrete `dyn Fn`, not a
        // `SmeltUnknown`, so the erased callable probe below does not apply.
        // Call it directly, adapting the forwarded argument vector to the
        // declared parameters.
        if let Some(Type::Function(function)) = self.mir.types.get(callback_ty).cloned() {
            return self.timer_typed_callback_call_text(&function, extra);
        }
        let call_args = if extra.is_some() {
            "smelt_timer_args.clone()"
        } else {
            "Vec::new()"
        };
        // The erased callable probe matches on `SmeltUnknown`. The object arm
        // binds through `get`, which yields a reference, so it must clone to
        // match the owned handle produced by the function arm.
        Ok(format!(
            "{{ let smelt_function_value = smelt_timer_callback.clone(); let smelt_callable = match smelt_function_value {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function.clone()), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_callable {{ (smelt_function)({call_args}).map(|_| ()) }} else {{ Err(std::io::Error::new(std::io::ErrorKind::Other, \"timer callback is not callable\").into()) }} }}"
        ))
    }

    /// Render a direct call to a statically-typed timer callback.
    ///
    /// The forwarded argument vector (`smelt_timer_args`, present only when the
    /// timer call has trailing `...args`) is adapted to the callback's declared
    /// parameters: a rest parameter receives the remaining arguments as a list,
    /// and each fixed parameter receives one positional argument (defaulting to
    /// `undefined` when absent). The callback result is discarded and mapped to
    /// the `Result<(), _>` shape the timer runtime expects.
    fn timer_typed_callback_call_text(
        &self,
        function: &FunctionType,
        extra: Option<&Operand>,
    ) -> Result<String, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let unknown_list_ty = self.type_id(Type::List(unknown_ty))?;
        let args_source = if extra.is_some() {
            "smelt_timer_args"
        } else {
            "smelt_timer_args_empty"
        };
        let prelude = if extra.is_some() {
            String::new()
        } else {
            "let smelt_timer_args_empty: Vec<SmeltUnknown> = Vec::new(); ".to_owned()
        };
        // The fully-erased `(...args: unknown[]) => unknown` callback lowers to a
        // `SmeltErasedFunction` struct rather than a bare `dyn Fn`, and is
        // invoked through its inherent `.call(...)` method (see the runtime
        // prelude). The forwarded argument vector is passed straight through.
        if self.is_erased_unknown_rest_function(function) && !function.may_throw {
            let call = format!("smelt_timer_callback.call({args_source}.clone())");
            return Ok(format!(
                "{{ {prelude}let _ = {call}; Ok::<(), Box<dyn std::error::Error>>(()) }}"
            ));
        }
        let mut call_args = Vec::new();
        for (index, param) in function.params.iter().enumerate() {
            if function.rest == Some(index) {
                let rest_text = format!(
                    "SmeltList::from({args_source}.iter().skip({index}).cloned().collect::<Vec<_>>())"
                );
                call_args.push(self.value_at_type_text(&rest_text, unknown_list_ty, *param)?);
            } else {
                let arg_text = format!(
                    "{args_source}.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined)"
                );
                call_args.push(self.value_at_type_text(&arg_text, unknown_ty, *param)?);
            }
        }
        let call = format!("(smelt_timer_callback)({})", call_args.join(", "));
        if function.may_throw {
            Ok(format!("{{ {prelude}({call}).map(|_| ()) }}"))
        } else {
            Ok(format!(
                "{{ {prelude}let _ = {call}; Ok::<(), Box<dyn std::error::Error>>(()) }}"
            ))
        }
    }

    /// Render the `let smelt_timer_args = ...;` prelude for forwarded timer
    /// arguments, or an empty string when the timer call has no extras.
    fn timer_extra_args_binding(&self, extra: Option<&Operand>) -> Result<String, EmitError> {
        let Some(extra_operand) = extra else {
            return Ok(String::new());
        };
        let unknown_ty = self.type_id(Type::Unknown)?;
        let args_ty = self.type_id(Type::List(unknown_ty))?;
        Ok(format!(
            "let smelt_timer_args: Vec<SmeltUnknown> = {}.iter().cloned().collect(); ",
            self.value_at_type(extra_operand, args_ty)?
        ))
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

    /// Return the future item type when the operand is a list of futures.
    ///
    /// `Promise.all`-style async ops accept a `List<Future<_>>` operand; this
    /// resolves that operand's element type so callers can inspect the awaited
    /// output shape. Returns `None` when the operand is not a list of futures.
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
            Callee::Builtin(BuiltinFn::ConsoleWrite | BuiltinFn::ConsoleErrorWrite) => {
                let (format_spec, value) = args
                    .first()
                    .map(|argument| self.console_arg_text(argument))
                    .transpose()?
                    .unwrap_or_else(|| ("{}", "\"\"".to_owned()));
                let macro_name = if matches!(callee, Callee::Builtin(BuiltinFn::ConsoleWrite)) {
                    "print"
                } else {
                    "eprint"
                };
                Ok(format!("{{ {macro_name}!(\"{format_spec}\", {value}); }}"))
            }
            Callee::Static(func) => {
                let function = self
                    .mir
                    .functions
                    .get(id_index(func.0, "function index does not fit usize")?)
                    .ok_or_else(|| EmitError::new("call references an unknown function"))?;
                if let HirOrigin::ClassConstructor { class, .. } = function.origin {
                    let class_name = sanitize_ident(self.symbol_name(class)?);
                    let class_type_params = self.callee_class_type_params(function);
                    let mut rendered_args = args
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            let Some(param) = function.params.get(index).copied() else {
                                return self.operand_text(arg);
                            };
                            let target_ty = self.function_local_decl(function, param)?.ty;
                            self.callee_generic_argument_text(
                                arg,
                                function,
                                target_ty,
                                &class_type_params,
                            )
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
                    // Route the built-in `Array.prototype.concat` to the list-concat
                    // helper. This is gated on the receiver's *static* type being a
                    // `List` (not on argument text), so it is a general rule keyed by
                    // the concrete receiver type rather than a name-based fixup: any
                    // list receiver calling single-argument `concat` lowers here.
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
                    let class_type_params = self.callee_class_type_params(function);
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
                            self.callee_generic_argument_text(
                                arg,
                                function,
                                target_ty,
                                &class_type_params,
                            )
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
                if let HirOrigin::ClassStaticMethod { class, method, .. } = function.origin {
                    // `Class.staticMethod(args)` lowers to the receiver-free
                    // associated function `Class::staticMethod(args)`. Arguments
                    // are coerced to the emitted parameter types like any other
                    // static call, and missing trailing parameters default.
                    let class_name = sanitize_ident(self.symbol_name(class)?);
                    let method_name = sanitize_ident(self.symbol_name(method)?);
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
                        "{class_name}::{method_name}({arg_values}){}",
                        self.throwing_call_suffix(function)
                    ));
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
                let free_function_type_params =
                    self.callee_free_function_type_params(function);
                let mut rendered_args = Vec::new();
                for (index, arg) in args.iter().enumerate() {
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
                        .or(local_ty);
                    let Some(target_ty) = target_ty else {
                        // Extra trailing argument beyond the callee's fixed arity
                        // (no rest parameter absorbs it). JavaScript silently
                        // ignores surplus positional arguments; the operand has
                        // already been evaluated into a temporary before the call,
                        // so its side effects are preserved and it is simply not
                        // forwarded. `tsc` only admits this shape when the callee
                        // is erased/`any`-typed, so a genuine over-application is
                        // already rejected upstream.
                        continue;
                    };
                    // A concrete argument bound to one of the callee's own
                    // generic type parameters (`identity(3)` against
                    // `x: T`) is passed through at its own type so Rust
                    // monomorphizes `identity::<f64>`; erasing it against the
                    // bare `T` target would mismatch the generic parameter.
                    if matches!(
                        self.mir.types.get(target_ty),
                        Some(Type::TypeParam { name })
                            if free_function_type_params.contains(name)
                    ) {
                        rendered_args.push(self.callee_generic_argument_text(
                            arg,
                            function,
                            target_ty,
                            &free_function_type_params,
                        )?);
                        continue;
                    }
                    // A composite parameter that mentions one of the callee's own
                    // generics (`arr: T[]` -> `SmeltList<T>`) is monomorphized at
                    // this call site to the concrete argument shape. This is only
                    // taken for callees that return one of their own type
                    // parameters (e.g. `sample<T>(arr: T[]): T`): there the return
                    // dest pins `T` to a concrete type, so the argument must be
                    // rendered at its own concrete shape (`SmeltList<f64>`) to bind
                    // `T = f64`; coercing to the bare `SmeltList<T>` target would
                    // erase elements to `SmeltUnknown` and clash with the
                    // monomorphization (E0308). Restricting to type-param-returning
                    // callees keeps functions that merely consume a generic list
                    // (and are emitted with erased or independently-inferred
                    // parameters) on the ordinary coercion path.
                    if !free_function_type_params.is_empty()
                        && self.free_function_returns_own_type_param(function)
                    {
                        let source_ty = self.operand_ty(arg)?;
                        if source_ty != target_ty
                            && self.generic_param_instantiated_by(
                                source_ty,
                                target_ty,
                                &free_function_type_params,
                            )
                        {
                            rendered_args.push(self.value_at_type(arg, source_ty)?);
                            continue;
                        }
                    }
                    // A composite parameter that mentions one of the callee's own
                    // generics (`arr: T[]` -> `SmeltList<T>`) is monomorphized at
                    // this call site to the concrete argument shape. Render the
                    // argument at its own type so Rust binds the type parameter
                    // (`SmeltList<f64>` -> `T = f64`); coercing to the bare
                    // `SmeltList<T>` target would erase elements to `SmeltUnknown`
                    // and clash with the monomorphization (E0308).
                    if matches!(self.mir.types.get(target_ty), Some(Type::Function(_)))
                        && param.is_some_and(|target_param| {
                            !self
                                .function_parameter_requires_owned_in(function, target_param)
                                .unwrap_or(false)
                        })
                    {
                        rendered_args.push(self.borrowed_function_argument_text(arg, target_ty)?);
                        continue;
                    }
                    if param.is_some_and(|target_param| {
                        self.parameter_needs_mutable_reference_in(function, target_param)
                    }) {
                        rendered_args.push(self.mutable_reference_argument_text(arg, target_ty)?);
                        continue;
                    }
                    if matches!(
                        self.mir.types.get(self.operand_ty(arg)?),
                        Some(Type::Function(_))
                    ) && !self.type_accepts_erased_function(target_ty)
                    {
                        rendered_args.push(self.default_value(target_ty)?);
                    } else {
                        rendered_args.push(self.value_at_type(arg, target_ty)?);
                    }
                }
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
                // An erased-unknown-rest callable value (e.g. an untyped
                // `vi.fn()` spy) lowers to `SmeltErasedFunction`, whose call ABI
                // is the `.call(vec![..])` method over an erased argument vector,
                // not a direct `(f)(args)` invocation. Route the call through that
                // method and erase every argument to `SmeltUnknown`.
                if self.is_erased_unknown_rest_function(function) && !function.may_throw {
                    return Ok(format!(
                        "({callee_text}).call({})",
                        self.erased_call_args_text(args)?
                    ));
                }
                let rendered_args = self.indirect_call_args_text(function, args)?;
                let suffix = if function.may_throw { "?" } else { "" };
                Ok(format!("({callee_text})({rendered_args}){suffix}"))
            }
        }
    }

    /// Render the invocation of a promise-continuation callback (`.then`/`.catch`
    /// handler) applied to a single already-erased `SmeltUnknown` argument
    /// expression.
    ///
    /// An untyped callback (e.g. a `vi.fn()` spy passed as `then(spy)`) lowers to
    /// `SmeltErasedFunction`, whose call ABI is `.call(vec![..])` rather than a
    /// direct `(f)(arg)` invocation. Detect that shape and route accordingly; all
    /// other callbacks keep the direct call.
    /// Render a `.then`/`.catch` callback invocation using an explicit callback
    /// expression `callback_text` (e.g. a hoisted clone binding) rather than the
    /// operand's own place text. The operand is still consulted for its function
    /// type so the erased-`SmeltErasedFunction` calling convention is preserved.
    fn promise_callback_invocation_with(
        &self,
        callback: &Operand,
        callback_text: &str,
        arg_expr: &str,
    ) -> Result<String, EmitError> {
        if let Some(Type::Function(function)) = self.mir.types.get(self.operand_ty(callback)?)
            && self.is_erased_unknown_rest_function(function)
            && !function.may_throw
        {
            return Ok(format!(
                "({callback_text}).call(vec![IntoSmeltUnknown::into_smelt_unknown({arg_expr})])"
            ));
        }
        Ok(format!("({callback_text})({arg_expr})"))
    }

    /// Hoist a `.then`/`.catch` callback out of the `Box::pin(async move { .. })`
    /// block it is invoked in.
    ///
    /// An `async move` block captures every referenced outer binding by move, so
    /// a callback that names a live outer variable would be consumed by the
    /// future even though JavaScript's `.then(cb)`/`.catch(cb)` do not consume
    /// `cb` (was E0382). Returns a `(prelude, callback_expr)` pair: for a
    /// place-based callback the prelude binds a clone in an enclosing block and
    /// the future moves that clone; an inline callback (a literal closure) is not
    /// live afterward and is left in place with an empty prelude.
    fn promise_callback_hoist(
        &self,
        callback: &Operand,
    ) -> Result<(String, String), EmitError> {
        let callback_text = self.operand_text(callback)?;
        if matches!(callback, Operand::Copy(_) | Operand::Move(_)) {
            let binding = "smelt_promise_callback";
            Ok((
                format!("let {binding} = ({callback_text}).clone(); "),
                binding.to_owned(),
            ))
        } else {
            Ok((String::new(), callback_text))
        }
    }

    /// Render arguments for a call into an erased `SmeltErasedFunction`, erasing
    /// each to `SmeltUnknown` and packing them into the `vec![..]` the runtime
    /// `.call(..)` method consumes.
    fn erased_call_args_text(&self, args: &[Operand]) -> Result<String, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let rendered = args
            .iter()
            .map(|arg| self.value_at_type(arg, unknown_ty))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        Ok(format!("vec![{rendered}]"))
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

    /// Return the generic type parameters declared by the class that owns
    /// `function`, when `function` is a class method or constructor.
    ///
    /// Methods and constructors of a generic class are emitted inside an
    /// `impl<T> Class<T>` block, so their signatures reference the class type
    /// parameters `T` directly. Callers use this to detect arguments whose
    /// target parameter is one of those class-level type parameters.
    fn callee_class_type_params(&self, function: &MirFunction) -> HashSet<Symbol> {
        let class_name = match function.origin {
            HirOrigin::ClassConstructor { class, .. } | HirOrigin::ClassMethod { class, .. } => {
                class
            }
            // Static methods are emitted as receiver-free associated functions and
            // do not participate in the receiver's generic instantiation (static
            // members on generic classes are deferred, see #98), so they carry no
            // class type parameters relevant to argument pass-through here.
            HirOrigin::Body(_) | HirOrigin::ClassStaticMethod { .. } => return HashSet::new(),
        };
        self.mir
            .classes
            .iter()
            .find(|class| class.name == class_name)
            .map(|class| class.type_params.iter().map(|param| param.name).collect())
            .unwrap_or_default()
    }

    /// Return the generic type parameters declared by a generic free function.
    ///
    /// A generic free function (`function identity<T>(x: T): T`) is emitted as
    /// `fn identity<T: ..>(x: T) -> T`, so its parameter and return positions
    /// reference `T` directly. Callers use this to detect arguments whose target
    /// parameter is one of the callee's own type parameters, which must be
    /// passed through concretely so Rust monomorphizes the call.
    fn callee_free_function_type_params(&self, function: &MirFunction) -> HashSet<Symbol> {
        // Only functions the crate decided to emit as real generics participate
        // in argument pass-through. Using the crate-wide decision (rather than
        // the signature-only predicate) keeps the call site in agreement with
        // the emitted definition: a function whose body forced a fall back to
        // erasure has erased (`SmeltUnknown`) parameters, so its arguments must
        // be erased here too, not passed through concretely.
        if !matches!(function.origin, HirOrigin::Body(_))
            || !self.context.is_generic_function(function.id)
        {
            return HashSet::new();
        }
        function
            .type_params
            .iter()
            .map(|param| param.name)
            .collect()
    }

    /// Returns whether a concrete `source` argument type is exactly the callee
    /// parameter `target` with its own generic `params` instantiated.
    ///
    /// A bare `TypeParam` argument (`x: T`) is passed through concretely so Rust
    /// monomorphizes the callee (see the call-site pass-through). A composite
    /// parameter that *contains* a type parameter (`arr: T[]` lowering to
    /// `SmeltList<T>`) is instantiated at the concrete argument shape at the same
    /// call site, so its argument must likewise pass through at its own concrete
    /// element type rather than be erased to `SmeltList<SmeltUnknown>` — erasing
    /// would clash with the monomorphized `SmeltList<f64>` the callee expects.
    ///
    /// The match is exact so pass-through only fires when Rust can actually bind
    /// the parameters from the argument: a `TypeParam` position accepts any
    /// source subtree (it is the binding site), every other constructor must
    /// agree in shape, and a concrete target must equal the source. When the
    /// argument has a different or erased shape (`SmeltUnknown` against
    /// `SmeltList<T>`), this returns `false` and the caller falls back to the
    /// ordinary coercion so the value is erased as before.
    fn generic_param_instantiated_by(
        &self,
        source: TypeId,
        target: TypeId,
        params: &HashSet<Symbol>,
    ) -> bool {
        match (self.mir.types.get(target), self.mir.types.get(source)) {
            (Some(Type::TypeParam { name }), _) if params.contains(name) => true,
            (Some(Type::List(target_item)), Some(Type::List(source_item)))
            | (Some(Type::Set(target_item)), Some(Type::Set(source_item)))
            | (Some(Type::Optional(target_item)), Some(Type::Optional(source_item)))
            | (Some(Type::Future(target_item)), Some(Type::Future(source_item))) => {
                self.generic_param_instantiated_by(*source_item, *target_item, params)
            }
            (Some(Type::Dict(target_key, target_value)), Some(Type::Dict(source_key, source_value))) => {
                self.generic_param_instantiated_by(*source_key, *target_key, params)
                    && self.generic_param_instantiated_by(*source_value, *target_value, params)
            }
            (Some(Type::Tuple(target_items)), Some(Type::Tuple(source_items)))
            | (Some(Type::Union(target_items)), Some(Type::Union(source_items)))
                if target_items.len() == source_items.len() =>
            {
                target_items
                    .iter()
                    .zip(source_items.iter())
                    .all(|(target_item, source_item)| {
                        self.generic_param_instantiated_by(*source_item, *target_item, params)
                    })
            }
            _ => source == target,
        }
    }

    /// Return whether a generic free function's declared return type is one of
    /// its own type parameters.
    ///
    /// A call such as `identity(3)` monomorphizes `fn identity<T>(x: T) -> T` to
    /// return `f64`, so the call value is already concrete at the site. The dest
    /// local carries that concrete type from the frontend, so the return needs
    /// no `SmeltUnknown` extraction. As with the argument pass-through, only a
    /// bare `TypeParam` return is handled; nested shapes (`T[]`) are excluded to
    /// keep the increment narrow and correct.
    fn free_function_returns_own_type_param(&self, function: &MirFunction) -> bool {
        matches!(
            self.mir.types.get(function.return_ty),
            Some(Type::TypeParam { name })
                if self.callee_free_function_type_params(function).contains(name)
        )
    }

    /// Return whether `function`'s declared return type is a generic type
    /// parameter of the class that owns it.
    ///
    /// Used to detect method calls such as `Box<number>::get(): T`, whose result
    /// is the receiver's concrete class argument at the call site rather than an
    /// erased `SmeltUnknown`. Nested shapes built from the parameter (for example
    /// `T[]`) are intentionally excluded here; only a bare `TypeParam` return is
    /// handled, matching the argument pass-through rule and keeping the concrete
    /// increment narrow and correct.
    fn method_returns_class_type_param(&self, function: &MirFunction) -> bool {
        matches!(
            self.mir.types.get(function.return_ty),
            Some(Type::TypeParam { name }) if self.callee_class_type_params(function).contains(name)
        )
    }

    /// Render a constructor or method argument against a callee parameter type,
    /// keeping the value concrete when the parameter is one of the callee
    /// class's own generic type parameters.
    ///
    /// A call such as `new Box<number>(3)` monomorphizes the emitted
    /// `impl<T> Box<T>` to `Box<f64>`, so the constructor parameter `value: T`
    /// resolves to `f64` at this site. Coercing the argument against the bare
    /// `TypeParam` target would erase it to `SmeltUnknown` (the calling function
    /// has no `T` in scope), producing a type mismatch against the generic
    /// parameter. Instead the argument is passed through at its own concrete
    /// type and Rust infers the class type argument from the value, which is the
    /// point of emitting real generics rather than erasing them.
    fn callee_generic_argument_text(
        &self,
        arg: &Operand,
        function: &MirFunction,
        target_ty: TypeId,
        class_type_params: &HashSet<Symbol>,
    ) -> Result<String, EmitError> {
        if let Some(Type::TypeParam { name }) = self.mir.types.get(target_ty)
            && class_type_params.contains(name)
        {
            let source_ty = self.operand_ty(arg)?;
            // A `TypeParam` source is already the class generic itself (e.g. one
            // generic method forwarding to another); pass it straight through.
            // Any concrete source is bound to the class type argument by Rust
            // inference, so rendering it at its own type keeps it concrete.
            let _ = function;
            return self.value_at_type(arg, source_ty);
        }
        self.value_at_type(arg, target_ty)
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
    /// Builds a convert-in-place adapter block for a static call that forwards
    /// a `&mut` list argument whose element type differs from the callee's
    /// emitted parameter element type.
    ///
    /// A generic caller such as `pull<T>(arr: &mut SmeltList<T>)` may delegate
    /// to an erased monomorphization `pull_127(arr: &mut SmeltList<SmeltUnknown>)`.
    /// Rust `&mut` references are invariant in their element type, so
    /// `&mut SmeltList<T>` cannot be passed where `&mut SmeltList<SmeltUnknown>`
    /// is expected. This emits a block that:
    ///   1. builds an erased temporary list from the argument's current
    ///      contents (each element coerced to the callee's element type),
    ///   2. passes `&mut temp` to the callee,
    ///   3. writes the (possibly mutated) temp elements back through the
    ///      original `&mut` argument, converting each element back to the
    ///      caller's element type, and
    ///   4. converts the callee's returned value to the destination type.
    ///
    /// Returns `None` when the call has no `&mut` list argument that is both a
    /// forwarded mutable-reference parameter of the current function and needs
    /// element conversion, leaving the ordinary call path untouched.
    pub(super) fn static_call_mut_list_adapter_text(
        &self,
        func: FuncId,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let function = self
            .mir
            .functions
            .get(id_index(func.0, "function index does not fit usize")?)
            .ok_or_else(|| EmitError::new("call references an unknown function"))?;
        // Throwing calls are emitted through a dedicated terminator path; keep
        // this adapter to the ordinary (non-throwing) statement path.
        if function.can_throw {
            return Ok(None);
        }
        if args.len() != function.params.len() {
            return Ok(None);
        }
        let rust_function_name = self.function_rust_name(function)?;
        let emitted_params = self.emitted_function_param_types(&rust_function_name)?;
        // Resolve each argument's effective (emitted) target parameter type.
        let target_tys = function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                let emitted = emitted_params
                    .as_ref()
                    .and_then(|params| params.get(index).copied());
                match emitted {
                    Some(ty) => Ok(ty),
                    None => Ok(self.function_local_decl(function, *param)?.ty),
                }
            })
            .collect::<Result<Vec<_>, EmitError>>()?;
        // Detect which arguments need the convert-in-place mutable-list adapter.
        let callee_emits_generics = self.context.is_generic_function(func);
        let caller_scope = self.current_function_type_params();
        let mut needs_adapter = false;
        for (index, arg) in args.iter().enumerate() {
            if self
                .mut_list_adapter_arg(function, index, arg, target_tys[index], callee_emits_generics)?
                .is_some()
            {
                needs_adapter = true;
            }
        }
        if !needs_adapter {
            return Ok(None);
        }
        let mut prelude = String::new();
        let mut rendered_args = Vec::with_capacity(args.len());
        let mut writebacks = String::new();
        for (index, arg) in args.iter().enumerate() {
            let target_ty = target_tys[index];
            if let Some(place) =
                self.mut_list_adapter_arg(function, index, arg, target_ty, callee_emits_generics)?
            {
                let place_text = self.place_text(&place)?;
                let arg_item = self.list_element_ty(self.place_ty(&place)?)?;
                let caller_elem = self.type_text_with_scoped_type_params(
                    arg_item,
                    false,
                    &caller_scope,
                )?;
                // The callee renders its erased element as `SmeltUnknown`; build
                // the temporary at the callee's rendered parameter type so the
                // `&mut` reborrow is type-correct.
                let temp_ty = self.type_text_with_scoped_type_params(
                    target_ty,
                    false,
                    &HashSet::new(),
                )?;
                let temp = format!("smelt_mut_arg_{index}");
                prelude.push_str(&format!(
                    "let mut {temp}: {temp_ty} = (*{place_text}).clone().into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<{temp_ty}>(); "
                ));
                rendered_args.push(format!("&mut {temp}"));
                writebacks.push_str(&format!(
                    "*{place_text} = {temp}.into_iter().map(|smelt_element| <{caller_elem} as SmeltFromUnknown>::smelt_from_unknown(smelt_element)).collect::<SmeltList<_>>(); "
                ));
            } else if matches!(self.mir.types.get(target_ty), Some(Type::Function(_))) {
                rendered_args.push(self.borrowed_function_argument_text(arg, target_ty)?);
            } else {
                rendered_args.push(self.value_at_type(arg, target_ty)?);
            }
        }
        let arg_values = rendered_args.join(", ");
        // Convert the callee's returned value to the destination type. The callee
        // erases its list element to `SmeltUnknown` while the destination keeps
        // the caller's element type, so a matching per-element un-erasure is
        // applied when the returned value is a list whose rendered element type
        // differs from the destination's.
        let return_ty = self
            .emitted_function_return_type(&rust_function_name)
            .unwrap_or(function.return_ty);
        let result_expr = self.mut_list_adapter_return_text(
            "smelt_mut_call_result",
            return_ty,
            dest_ty,
            &caller_scope,
        )?;
        Ok(Some(format!(
            "{{ {prelude}let smelt_mut_call_result = {rust_function_name}({arg_values}); {writebacks}{result_expr} }}"
        )))
    }

    /// Converts the value returned by a mutable-list adapter call to the
    /// destination type. When both the callee's rendered return type and the
    /// destination are lists whose rendered element types differ (the callee
    /// erased its element to `SmeltUnknown` while the destination keeps the
    /// caller's generic element), each element is un-erased through
    /// `SmeltFromUnknown`. Otherwise the value is returned unchanged.
    fn mut_list_adapter_return_text(
        &self,
        value_text: &str,
        return_ty: TypeId,
        dest_ty: TypeId,
        caller_scope: &HashSet<Symbol>,
    ) -> Result<String, EmitError> {
        let (Some(Type::List(_)), Some(Type::List(dest_item))) =
            (self.mir.types.get(return_ty), self.mir.types.get(dest_ty))
        else {
            return Ok(value_text.to_owned());
        };
        let return_render = self.type_text_with_scoped_type_params(return_ty, false, &HashSet::new())?;
        let dest_render = self.type_text_with_scoped_type_params(dest_ty, false, caller_scope)?;
        if return_render == dest_render {
            return Ok(value_text.to_owned());
        }
        let dest_elem = self.type_text_with_scoped_type_params(*dest_item, false, caller_scope)?;
        Ok(format!(
            "{value_text}.into_iter().map(|smelt_element| <{dest_elem} as SmeltFromUnknown>::smelt_from_unknown(smelt_element)).collect::<SmeltList<_>>()"
        ))
    }

    /// Returns the place of the `index`-th argument when it needs the
    /// convert-in-place mutable-list adapter.
    ///
    /// The adapter applies when the argument is a forwarded `&mut` list
    /// parameter of the current function, the callee needs a mutable reference
    /// to that parameter, and the caller renders the list element type
    /// differently from the callee (typically a generic `T` in a generic caller
    /// versus `SmeltUnknown` in an erased callee). Because `&mut` references are
    /// invariant, such a reborrow cannot type-check without element conversion.
    fn mut_list_adapter_arg(
        &self,
        function: &MirFunction,
        index: usize,
        arg: &Operand,
        target_ty: TypeId,
        callee_emits_generics: bool,
    ) -> Result<Option<Place>, EmitError> {
        let Some(param) = function.params.get(index).copied() else {
            return Ok(None);
        };
        if !self.parameter_needs_mutable_reference_in(function, param) {
            return Ok(None);
        }
        let local = match arg {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => *local,
            _ => return Ok(None),
        };
        // The argument must itself be a `&mut` list parameter of the current
        // function; only then does the emitted argument text reborrow through a
        // reference whose element type must match invariantly.
        if self.function.id.0 == u32::MAX
            || !self.function.params.contains(&local)
            || !self.parameter_needs_mutable_reference(local)
        {
            return Ok(None);
        }
        let arg_ty = self.place_ty(&Place::Local(local))?;
        let (Some(Type::List(arg_item)), Some(Type::List(target_item))) =
            (self.mir.types.get(arg_ty), self.mir.types.get(target_ty))
        else {
            return Ok(None);
        };
        // Compare rendered element types: the MIR element types can be identical
        // (a shared `TypeParam`) while the caller keeps it generic and the callee
        // erases it. The callee renders its element with generics only when it
        // emits real Rust generics.
        let arg_element_text =
            self.type_text_with_scoped_type_params(*arg_item, false, &self.current_function_type_params())?;
        let callee_scope: HashSet<Symbol> = if callee_emits_generics {
            function
                .type_params
                .iter()
                .map(|type_param| type_param.name)
                .collect()
        } else {
            HashSet::new()
        };
        let param_element_text =
            self.type_text_with_scoped_type_params(*target_item, false, &callee_scope)?;
        if arg_element_text == param_element_text {
            return Ok(None);
        }
        Ok(Some(Place::Local(local)))
    }

    /// Returns the element type of a list type, or an error when the type is
    /// not a list.
    fn list_element_ty(&self, list_ty: TypeId) -> Result<TypeId, EmitError> {
        match self.mir.types.get(list_ty) {
            Some(Type::List(item)) => Ok(*item),
            _ => Err(EmitError::new("expected a list type")),
        }
    }

    /// Converts a call expression to Rust, coercing the result to `dest_ty`.
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
        if let Callee::Static(func) = callee
            && let Some(adapter) = self.static_call_mut_list_adapter_text(*func, args, dest_ty)?
        {
            return Ok(adapter);
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
        let source_ty = self.call_emitted_source_ty(callee, dest_ty)?;
        self.value_at_type_text(&call_text, source_ty, dest_ty)
    }

    /// Returns the Rust type a call *actually* evaluates to in the emitted code.
    ///
    /// Unlike [`Self::call_source_ty`] (which returns the callee's declared MIR
    /// return type), this resolves the type the generated call expression really
    /// produces, accounting for erasure: async functions yield
    /// `Future<return_ty>`, functions whose emitted signature erased a return
    /// type parameter yield the emitted return type, and monomorphized generic
    /// returns pass through at the concrete `dest_ty`. Callers coerce the call
    /// value from this type to the destination, so it must match the emitted
    /// signature, not the frontend's specialized view of the call site.
    pub(super) fn call_emitted_source_ty(
        &self,
        callee: &Callee,
        dest_ty: TypeId,
    ) -> Result<TypeId, EmitError> {
        let source_ty = match callee {
            Callee::Static(func) => {
                let function = self
                    .mir
                    .functions
                    .get(id_index(func.0, "function index does not fit usize")?)
                    .ok_or_else(|| EmitError::new("call references an unknown function"))?;
                if matches!(function.origin, HirOrigin::ClassConstructor { .. }) {
                    dest_ty
                } else if matches!(function.origin, HirOrigin::ClassMethod { .. })
                    && self.method_returns_class_type_param(function)
                {
                    // A method of a generic class whose declared return type is
                    // one of the class type parameters (e.g. `get(): T`) returns
                    // the receiver's concrete instantiation at this call site
                    // (`Box<f64>::get` yields `f64`). The dest local already
                    // carries that substituted type from the frontend, so use it
                    // as the source type: the call value is concrete and needs no
                    // `SmeltUnknown` extraction. Erasing here would emit an
                    // extraction match against a value that is not `SmeltUnknown`.
                    dest_ty
                } else if self.free_function_returns_own_type_param(function) {
                    // A generic free function whose declared return type is one
                    // of its own type parameters (`identity<T>(x: T): T`) is
                    // monomorphized at this call site, so `identity(3)` yields a
                    // concrete `f64`. The dest local carries that concrete type
                    // from the frontend; use it as the source type so the value
                    // passes through without a spurious `SmeltUnknown`
                    // extraction against an already-concrete value.
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
        Ok(source_ty)
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
            Callee::Builtin(
                BuiltinFn::ConsoleLog | BuiltinFn::ConsoleWrite | BuiltinFn::ConsoleErrorWrite,
            ) => self.none_ty,
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
        } else if matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::Unknown)
        ) {
            // These render through their own `Display` impl (`SmeltUnknown`
            // implements `Display` via the JS `String()` coercion).
            Ok(("{}", self.operand_text(operand)?))
        } else {
            // A class instance, generic type parameter, union, function, or set
            // has no Rust `Display` impl, so `{}` would fail to compile (E0277).
            // Erase to the runtime `SmeltUnknown` form, which does implement
            // `Display`, matching how the same values stringify everywhere else.
            let ty = self.operand_ty(operand)?;
            Ok(("{}", self.erase_value_text(&self.operand_text(operand)?, ty)?))
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
        if let Some(check) =
            self.concrete_union_class_check(&self.operand_text(value)?, value_ty, class)
        {
            return Ok(check);
        }
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
        // Boxed primitive wrappers (`Boolean`/`String`) share the same marker
        // recognition path: a primitive `true`/`"a"` erases to `SmeltUnknown::Bool`
        // / `SmeltUnknown::String` (never an `Object`), so the marker check is the
        // correct `false`, while a boxed wrapper object carrying the dedicated
        // marker resolves to `true`.
        // AbortController/AbortSignal keep their own markers (they are runtime
        // primitives, not host-object registry entries). Every other modeled host
        // object and boxed primitive wrapper resolves its marker through the
        // shared `smelt_stdlib::host_object` registry, so this `instanceof` path
        // cannot drift from the frontend construction path or the runtime for-in
        // filter. (`ArrayBuffer`/`Blob`/`Number` are already handled by dedicated
        // branches above; sourcing them here too is harmless since those
        // short-circuit first.)
        let abort_marker = match class_name {
            "AbortController" => Some("__smelt_abortcontroller"),
            "AbortSignal" => Some("__smelt_abortsignal"),
            _ => smelt_stdlib::host_object_marker(class_name),
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
                    .is_ok_and(|name| sanitize_ident(name) == method_name)
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
