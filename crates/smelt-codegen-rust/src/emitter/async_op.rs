//! Built-in async operations: `Promise` combinators and their callbacks, timers
//! and awaited values, and the erased forms their results take.

use super::*;
use crate::rust::RustExpr;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// Converts a runtime-backed async operation to Rust.
    fn async_op_expr(
        &self,
        op: smelt_hir::AsyncOp,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        match op {
            smelt_hir::AsyncOp::All | smelt_hir::AsyncOp::AllSettled => self.promise_all_expr(args),
            smelt_hir::AsyncOp::Race => self.promise_race_expr(args),
            smelt_hir::AsyncOp::ExitDrain => Ok(RustExpr::atom(format!(
                "SmeltFuture::from_future(Box::pin(async move {{ {exit_drain}().await; Ok::<_, {ERROR_BOX}>(()) }}))",
                exit_drain = smelt_stdlib::runtime_symbols::timers::RUN_UNTIL_EXIT,
            ))),
            smelt_hir::AsyncOp::Sleep => self.sleep_expr(args),
            smelt_hir::AsyncOp::Resolve => self.promise_resolve_expr(args, dest_ty),
            smelt_hir::AsyncOp::Reject => self.promise_reject_expr(args),
            smelt_hir::AsyncOp::SetTimeout => self.set_timeout_expr(args),
            smelt_hir::AsyncOp::ClearTimeout => {
                let Some(timeout) = args.first() else {
                    return Err(EmitError::new(
                        "clearTimeout requires a timeout handle operand",
                    ));
                };
                Ok(RustExpr::chain(format!(
                    "{clear_timeout}({})",
                    self.operand_expr(timeout)?,
                    clear_timeout = smelt_stdlib::runtime_symbols::timers::CLEAR_TIMEOUT,
                )))
            }
            smelt_hir::AsyncOp::SetInterval => self.set_interval_expr(args),
            smelt_hir::AsyncOp::ClearInterval => {
                let Some(timer) = args.first() else {
                    return Err(EmitError::new(
                        "clearInterval requires an interval handle operand",
                    ));
                };
                Ok(RustExpr::chain(format!(
                    "{clear_interval}({})",
                    self.operand_expr(timer)?,
                    clear_interval = smelt_stdlib::runtime_symbols::timers::CLEAR_INTERVAL,
                )))
            }
            smelt_hir::AsyncOp::Promise => self.promise_constructor_expr(args, dest_ty),
            smelt_hir::AsyncOp::Then => self.promise_then_expr(args, dest_ty),
            smelt_hir::AsyncOp::Catch => self.promise_catch_expr(args),
            smelt_hir::AsyncOp::Finally => self.promise_finally_expr(args),
            smelt_hir::AsyncOp::SpawnLocal => {
                let [future] = args else {
                    return Err(EmitError::new("spawn local requires one future operand"));
                };
                let future_text = self.await_operand_expr(future)?;
                Ok(RustExpr::atom(format!(
                    "{{ {spawn_promise_task}(Box::pin(async move {{ let _ = {future_text}.await; }})); () }}",
                    spawn_promise_task = smelt_stdlib::runtime_symbols::timers::SPAWN_PROMISE_TASK,
                )))
            }
            smelt_hir::AsyncOp::CreateTask => {
                let Some(future) = args.first() else {
                    return Err(EmitError::new(
                        "async task creation requires a future operand",
                    ));
                };
                let future_text = self.await_operand_expr(future)?;
                Ok(RustExpr::atom(format!(
                    "SmeltFuture::from_future(Box::pin(async move {{ tokio::spawn(async move {{ {future_text}.await }}).await.expect(\"async task panicked\") }}))"
                )))
            }
            smelt_hir::AsyncOp::WaitFor => {
                let [future, timeout, ..] = args else {
                    return Err(EmitError::new(
                        "async wait_for requires a future and timeout operand",
                    ));
                };
                let future_text = self.await_operand_expr(future)?;
                Ok(RustExpr::atom(format!(
                    "SmeltFuture::from_future(Box::pin(async move {{ tokio::time::timeout(::std::time::Duration::from_millis({} as u64), async move {{ {future_text}.await }}).await.expect(\"async timeout\")? }}))",
                    self.operand_expr(timeout)?
                )))
            }
            smelt_hir::AsyncOp::HttpFetch => self.http_fetch_async_expr(args),
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
                Ok(RustExpr::atom(format!(
                    "SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, {ERROR_BOX}>(reqwest::get({}).await.expect(\"HTTP GET failed\").text().await.expect(\"HTTP response body read failed\")) }}))",
                    self.operand_expr(url)?
                )))
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
    fn timer_callback_call_expr(
        &self,
        callback: &Operand,
        extra: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let callback_ty = self.operand_ty(callback)?;
        if extra.is_none()
            && let Some(Type::Function(function)) = self.mir.types.get(callback_ty)
            && function.params.is_empty()
        {
            return Ok(if self.function_value_call_is_fallible(function) {
                RustExpr::atom("(smelt_timer_callback)().map(|_| ())")
            } else {
                RustExpr::loose(format!("Ok::<(), {ERROR_BOX}>({{ (smelt_timer_callback)(); () }})"))
            });
        }
        // A callback with a statically-known function type (e.g. `delay`'s
        // `(...args: any[]) => any`) is a concrete `dyn Fn`, not a
        // `SmeltUnknown`, so the erased callable probe below does not apply.
        // Call it directly, adapting the forwarded argument vector to the
        // declared parameters.
        if let Some(Type::Function(function)) = self.mir.types.get(callback_ty).cloned() {
            return self.timer_typed_callback_call_expr(&function, extra);
        }
        let call_args = if extra.is_some() {
            "smelt_timer_args.clone()"
        } else {
            "Vec::new()"
        };
        // The erased callable probe matches on `SmeltUnknown`. The object arm
        // binds through `get`, which yields a reference, so it must clone to
        // match the owned handle produced by the function arm.
        Ok(RustExpr::atom(self.carrier_format(format_args!(
            "{{ let smelt_function_value = smelt_timer_callback.clone(); let smelt_callable = match smelt_function_value {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"{SMELT_CALL}\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function.clone()), _ => None }}, _ => None }}; if let Some(smelt_function) = smelt_callable {{ (smelt_function)({call_args}).map(|_| ()) }} else {{ Err(std::io::Error::new(std::io::ErrorKind::Other, \"timer callback is not callable\").into()) }} }}"
        ))))
    }

    /// Render a direct call to a statically-typed timer callback.
    ///
    /// The forwarded argument vector (`smelt_timer_args`, present only when the
    /// timer call has trailing `...args`) is adapted to the callback's declared
    /// parameters: a rest parameter receives the remaining arguments as a list,
    /// and each fixed parameter receives one positional argument (defaulting to
    /// `undefined` when absent). The callback result is discarded and mapped to
    /// the `Result<(), _>` shape the timer runtime expects.
    fn timer_typed_callback_call_expr(
        &self,
        function: &FunctionType,
        extra: Option<&Operand>,
    ) -> Result<RustExpr, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let unknown_list_ty = self.type_id(Type::List(unknown_ty))?;
        let args_source = if extra.is_some() {
            RustExpr::path("smelt_timer_args")
        } else {
            RustExpr::path("smelt_timer_args_empty")
        };
        let prelude = if extra.is_some() {
            String::new()
        } else {
            self.carrier_expr(&RustExpr::loose("let smelt_timer_args_empty: Vec<SmeltUnknown> = Vec::new(); ")).into_string()
        };
        // The fully-erased `(...args: unknown[]) => unknown` callback lowers to a
        // `SmeltErasedFunction` struct rather than a bare `dyn Fn`, and is
        // invoked through its inherent `.call(...)` method (see the runtime
        // prelude). The forwarded argument vector is passed straight through.
        if self.is_erased_unknown_rest_function(function) && !function.may_throw {
            let call = format!("smelt_timer_callback.call({args_source}.clone())");
            return Ok(RustExpr::atom(format!(
                "{{ {prelude}let _ = {call}; Ok::<(), {ERROR_BOX}>(()) }}"
            )));
        }
        let mut call_args = Vec::new();
        for (index, param) in function.params.iter().enumerate() {
            if function.rest == Some(index) {
                let rest_text = RustExpr::atom(format!(
                    "SmeltList::from({args_source}.iter().skip({index}).cloned().collect::<Vec<_>>())"
                ));
                call_args.push(self.rendered_at_type(&rest_text, unknown_list_ty, *param, &self.render_scope())?);
            } else {
                let arg_text = RustExpr::postfix_on(&args_source, self.carrier_format(format_args!(
                    "{args_source}.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined)"
                )));
                call_args.push(self.rendered_at_type(&arg_text, unknown_ty, *param, &self.render_scope())?);
            }
        }
        let call = format!("(smelt_timer_callback)({})", call_args.join(", "));
        if self.function_value_call_is_fallible(function) {
            Ok(RustExpr::atom(format!("{{ {prelude}({call}).map(|_| ()) }}")))
        } else {
            Ok(RustExpr::atom(format!(
                "{{ {prelude}let _ = {call}; Ok::<(), {ERROR_BOX}>(()) }}"
            )))
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
        Ok(self.carrier_format(format_args!(
            "let smelt_timer_args: Vec<SmeltUnknown> = {}.iter().cloned().collect(); ",
            list_read_expr(&self.value_at_type(extra_operand, args_ty)?)
        )))
    }

    /// Whether awaiting this operand yields an erased value that may itself be a
    /// `SmeltUnknown::Promise` (i.e. the future's `Output` is `Unknown`), so its
    /// awaited result needs flattening through `smelt_await_flatten`.
    fn awaited_operand_is_erased(&self, operand: &Operand) -> bool {
        self.operand_ty(operand)
            .is_ok_and(|ty| self.is_erased_future_type(ty))
    }

    /// Whether `ty` is a future whose awaited value is `unknown`.
    fn is_erased_future_type(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::Future(output)) if self.mir.types.get(*output) == Some(&Type::Unknown)
        )
    }

    /// Whether `ty` is a list of `unknown` elements.
    pub(super) fn is_unknown_list_type(&self, ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(ty),
            Some(Type::List(item)) if self.mir.types.get(*item) == Some(&Type::Unknown)
        )
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

    /// Render a `.then`/`.catch` callback invocation using an explicit callback
    /// expression `callback_text` (e.g. a hoisted clone binding) rather than the
    /// operand's own place text. The operand is still consulted for its function
    /// type so the erased-`SmeltErasedFunction` calling convention is preserved.
    fn promise_callback_invocation_with(
        &self,
        callback: &Operand,
        callback_text: &RustExpr,
        arg_expr: &RustExpr,
    ) -> Result<String, EmitError> {
        // A fully erased callback value (`Type::Unknown` — e.g. a stateful
        // `vi.fn()` mock object passed as `then(spy)`) carries its callable
        // behind the `__smelt_call` field, so route through the standard
        // callable-object dispatch; a non-callable value is dropped like an
        // absent handler (the resolved value still evaluates).
        if self.mir.types.get(self.operand_ty(callback)?) == Some(&Type::Unknown) {
            return Ok(self.carrier_format(format_args!(
                "{{ let smelt_callable = match ({callback_text}).clone() {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"{SMELT_CALL}\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; match smelt_callable {{ Some(smelt_function) => (smelt_function)(vec![IntoSmeltUnknown::into_smelt_unknown({arg_expr})]).unwrap_or_else(|error| smelt_panic_throw(error)), None => {{ let _ = {arg_expr}; SmeltUnknown::Undefined }} }} }}"
            )));
        }
        if let Some(Type::Function(function)) = self.mir.types.get(self.operand_ty(callback)?) {
            if self.is_erased_unknown_rest_function(function) && !function.may_throw {
                return Ok(self.carrier_format(format_args!(
                    "({callback_text}).call(vec![IntoSmeltUnknown::into_smelt_unknown({arg_expr})])"
                )));
            }
            // JavaScript `.then(cb)`/`.catch(cb)` always invoke the continuation
            // with the resolved value (or rejection reason), but a 0-arity source
            // callback declares no parameter to receive it. Adapt to the callback's
            // declared arity: a statically-typed closure with no parameters and no
            // rest must be called with no arguments, dropping the value, or Rust
            // reports E0057 (too many arguments).
            if function.rest.is_none() && function.params.is_empty() {
                return Ok(format!("({{ let _ = {arg_expr}; ({callback_text})() }})"));
            }
            // The continuation's declared parameter may be by shared reference
            // (`callback_param_is_shared_reference`); `arg_expr` is an owned
            // erased value, so borrow the temporary at the call.
            if let Some(param_ty) = function.params.first().copied() {
                let arg_text = self.callback_call_arg_expr(
                    function,
                    0,
                    param_ty,
                    arg_expr.to_owned().into_string(),
                );
                return Ok(format!("({callback_text})({arg_text})"));
            }
        }
        Ok(format!("({callback_text})({arg_expr})"))
    }

    /// Bind a `.then`/`.catch` continuation's settled value as
    /// `smelt_callback_value`, returning that statement and the value's type.
    ///
    /// The statement runs inside the `async move` block the continuation's
    /// promise is built from, whose error channel IS that promise's rejection.
    /// Two continuation shapes need unwrapping there:
    ///
    /// * A handler returning a promise is flattened (`.await?`), so neither
    ///   method answers `Promise<Promise<T>>`.
    /// * A THROWING handler (`may_throw`, emitted as a function returning
    ///   `Result<T, Box<dyn Error>>`) propagates its error with `?`: in
    ///   JavaScript a `then`/`catch` handler that throws rejects the promise the
    ///   call returns (`p.catch(e => { if (!ok(e)) throw e; ... })` rethrows).
    ///   Binding the bare `Result` instead settled the promise with a
    ///   `Result<Result<T, _>, _>`, which does not type-check.
    fn promise_callback_settle(
        &self,
        callback: &Operand,
        invocation: &str,
    ) -> Result<(String, TypeId), EmitError> {
        let (callback_return_ty, may_throw) = match self.mir.types.get(self.operand_ty(callback)?)
        {
            Some(Type::Function(function)) => (function.return_ty, function.may_throw),
            _ => (self.type_id(Type::Unknown)?, false),
        };
        Ok(match self.mir.types.get(callback_return_ty) {
            Some(Type::Future(item)) => (
                format!("let smelt_callback_value = {invocation}.await?;"),
                *item,
            ),
            _ if may_throw => (
                format!("let smelt_callback_value = {invocation}?;"),
                callback_return_ty,
            ),
            _ => (
                format!("let smelt_callback_value = {invocation};"),
                callback_return_ty,
            ),
        })
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
        let callback_text = self.operand_expr(callback)?;
        if matches!(callback, Operand::Copy(_) | Operand::Move(_)) {
            let binding = "smelt_promise_callback";
            Ok((
                format!("let {binding} = ({callback_text}).clone(); "),
                binding.to_owned(),
            ))
        } else {
            Ok((String::new(), callback_text.into_string()))
        }
    }

    /// Wrap a generated native `async fn` call in Smelt's stable future ABI.
    ///
    /// Rust evaluates an `async fn` call as an anonymous `impl Future`, while
    /// MIR `Type::Future(T)` is emitted as `SmeltFuture<T>`. Materializing the
    /// wrapper at call destinations keeps free async functions compatible with
    /// closure temporaries, adapters, and values that escape before an await.
    ///
    /// Only callees that are themselves emitted as a real Rust `async fn` yield
    /// an `impl Future` needing this wrapper: free functions (`HirOrigin::Body`)
    /// and static methods (`ClassStaticMethod`). An async *instance* method is
    /// emitted as a synchronous `fn(&self, ..) -> SmeltFuture<T>` whose body
    /// runs inside a moved `async` block (see `emit_async_method_owned_self_body`),
    /// so its call already produces a `SmeltFuture<T>`. Wrapping that again in
    /// `SmeltFuture::from_future(Box::pin(..))` fails to compile because
    /// `SmeltFuture<T>` is not a `Future` (E0277), so instance methods are
    /// excluded here and their call value passes through unchanged.
    pub(super) fn wrap_native_async_call_expr(
        &self,
        callee: &Callee,
        call_text: RustExpr,
    ) -> Result<RustExpr, EmitError> {
        let Callee::Static(func) = callee else {
            return Ok(call_text);
        };
        let function = self
            .mir
            .functions
            .get(id_index(func.0, "function index does not fit usize")?)
            .ok_or_else(|| EmitError::new("call references an unknown function"))?;
        if function.is_async
            && !function.is_generator
            && !matches!(function.origin, HirOrigin::ClassMethod { .. })
        {
            Ok(RustExpr::atom(format!(
                "SmeltFuture::from_future(Box::pin({call_text}))"
            )))
        } else {
            Ok(call_text)
        }
    }

    /// `await` of a value.
    pub(super) fn await_rvalue_expr(
        &self,
        operand: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if self.mir.types.get(self.operand_ty(operand)?) == Some(&Type::None) {
            return self.default_value(dest_ty);
        }
        Ok(RustExpr::loose(format!("{}.await?", self.await_operand_expr(operand)?)))
    }

    /// A built-in async operation at its destination.
    pub(super) fn async_rvalue_expr(
        &self,
        op: smelt_hir::AsyncOp,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let text = self.async_op_expr(op, args, dest_ty)?;
        if matches!(op, smelt_hir::AsyncOp::Sleep)
            && let Some(Type::Future(item)) = self.mir.types.get(dest_ty)
            && self.mir.types.get(*item) != Some(&Type::None)
        {
            return Ok(RustExpr::atom(format!(
                "SmeltFuture::from_future(Box::pin(async move {{ {text}.await?; Ok::<_, {ERROR_BOX}>({}) }}))",
                self.default_value(*item)?,
            )));
        }
        Ok(text)
    }

    /// `Promise.all` and `Promise.allSettled`.
    fn promise_all_expr(
        &self,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        if let [arg] = args
            && let Some(item_ty) = self.async_list_operand_item_type(arg)?
        {
            let list = self.await_operand_expr(arg)?;
            // When the futures resolve to erased values, each settled
            // value may itself be a `SmeltUnknown::Promise` (an `async`
            // function that returns a `Promise`). Collect every future
            // first — so funnel/batch schedulers observe all requests
            // before any is driven — then flatten each erased promise to
            // its resolved value.
            let item_is_erased = self.is_erased_future_type(item_ty);
            if item_is_erased {
                return Ok(RustExpr::atom(format!(
                    "SmeltFuture::from_future(Box::pin(async move {{ let mut __smelt_pending = Vec::new(); for __smelt_future in {list} {{ __smelt_pending.push(__smelt_future.await?); }} let mut __smelt_values = Vec::with_capacity(__smelt_pending.len()); for __smelt_value in __smelt_pending {{ __smelt_values.push(smelt_await_flatten(__smelt_value).await?); }} Ok::<_, {ERROR_BOX}>(SmeltList::from(__smelt_values)) }}))"
                )));
            }
            return Ok(RustExpr::atom(format!(
                "SmeltFuture::from_future(Box::pin(async move {{ let mut __smelt_values = Vec::new(); for __smelt_future in {list} {{ __smelt_values.push(__smelt_future.await?); }} Ok::<_, {ERROR_BOX}>(SmeltList::from(__smelt_values)) }}))"
            )));
        }
        let rendered_args = args
            .iter()
            .map(|arg| self.await_operand_expr(arg))
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
                // `biased;`: each wake polls the branches in argument order.
                // The default rotates the first branch per poll for fairness,
                // so two tasks resuming at the same `await` ran in a
                // different order than Python's FIFO event loop (and than
                // JavaScript's microtask queue) runs them.
                let joined = format!("tokio::join!(biased; {})", rendered_args.join(", "));
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
        Ok(RustExpr::atom(format!(
            "SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, {ERROR_BOX}>({body}) }}))"
        )))
    }

    /// `Promise.race`.
    fn promise_race_expr(&self, args: &[Operand]) -> Result<RustExpr, EmitError> {
        let rendered_args = args
            .iter()
            .map(|arg| self.await_operand_expr(arg))
            .collect::<Result<Vec<_>, _>>()?;
        let body = match rendered_args.as_slice() {
            [] => {
                return Err(EmitError::new(
                    "async race requires at least one future operand",
                ));
            }
            [single] => format!("{single}.await?"),
            _ => {
                // `tokio::select!` polls its branches in a randomized
                // order and returns the first branch that reports
                // `Ready` in a poll round. On the virtual clock every
                // racer can settle within the same round (each poll of a
                // promise spin-loop advances time by one timer step), so
                // the winner was a coin flip rather than the racer that
                // settled first. `smelt_promise_race` polls in source
                // order instead — see its prelude docs.
                let pushes = rendered_args
                    .iter()
                    .map(|arg| {
                        format!(
                            "smelt_racers.push(Box::pin(::std::future::IntoFuture::into_future({arg})));"
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                format!(
                    "{{ let mut smelt_racers: Vec<::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<_, {ERROR_BOX}>>>>> = Vec::new(); {pushes} {promise_race}(smelt_racers).await? }}",
                    promise_race =
                        smelt_stdlib::runtime_symbols::timers::PROMISE_RACE,
                )
            }
        };
        Ok(RustExpr::atom(format!(
            "SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, {ERROR_BOX}>({body}) }}))"
        )))
    }

    /// A timed suspension.
    fn sleep_expr(&self, args: &[Operand]) -> Result<RustExpr, EmitError> {
        let Some(duration) = args.first() else {
            return Err(EmitError::new("async sleep requires a duration operand"));
        };
        Ok(RustExpr::atom(format!(
            "SmeltFuture::from_future(Box::pin(async move {{ {sleep_ms}({} as f64).await; Ok::<_, {ERROR_BOX}>(()) }}))",
            self.operand_expr(duration)?,
            sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
        )))
    }

    /// `Promise.resolve`.
    fn promise_resolve_expr(
        &self,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        // `Promise.resolve(v)`: defer one microtask, then settle with the
        // operand. The value is rendered at the future's own item type so
        // the emitted `Ok(..)` agrees with the declared `SmeltFuture<T>`;
        // with no operand (`Promise.resolve()`) the item type is unit.
        let Some(duration) = args.first() else {
            return Err(EmitError::new(
                "async resolve requires a duration operand",
            ));
        };
        let duration_text = self.operand_expr(duration)?;
        let value_text = match args.get(1) {
            Some(value) => {
                let Some(Type::Future(item_ty)) = self.mir.types.get(dest_ty) else {
                    return Err(EmitError::new(
                        "async resolve must produce a future type",
                    ));
                };
                self.value_at_type(value, *item_ty)?
            }
            None => RustExpr::unit(),
        };
        Ok(RustExpr::atom(format!(
            "SmeltFuture::from_future(Box::pin(async move {{ {sleep_ms}({duration_text} as f64).await; Ok::<_, {ERROR_BOX}>({value_text}) }}))",
            sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
        )))
    }

    /// `Promise.reject`.
    fn promise_reject_expr(
        &self,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        // `Promise.reject(reason)`: defer one microtask, then settle in
        // the error channel with the reason unchanged. The reason takes
        // the same `throw` path a `throw` statement takes, so a non-Error
        // reason keeps its own properties (JavaScript rejects with any
        // value) and a program with no erased values keeps the plain
        // string error form.
        let Some(duration) = args.first() else {
            return Err(EmitError::new(
                "async reject requires a duration operand",
            ));
        };
        let duration_text = self.operand_expr(duration)?;
        let payload = match args.get(1) {
            Some(reason) => self.thrown_payload_expr(reason)?,
            None => self.undefined_thrown_payload_expr(),
        };
        Ok(RustExpr::atom(format!(
            "SmeltFuture::from_future(Box::pin(async move {{ {sleep_ms}({duration_text} as f64).await; Err::<_, {ERROR_BOX}>({payload}) }}))",
            sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
        )))
    }

    /// `setTimeout`.
    fn set_timeout_expr(&self, args: &[Operand]) -> Result<RustExpr, EmitError> {
        let [callback, duration, extra @ ..] = args else {
            return Err(EmitError::new(
                "setTimeout requires callback and duration operands",
            ));
        };
        let callback_text = self.operand_expr(callback)?;
        let callback_call = self.timer_callback_call_expr(callback, extra.first())?;
        let extra_binding = self.timer_extra_args_binding(extra.first())?;
        Ok(RustExpr::atom(format!(
            "{{ let smelt_timer_callback = {callback_text}.clone(); {extra_binding}{set_timeout}(::std::rc::Rc::new(::std::cell::RefCell::new(move || {{ {callback_call} }})), {} as f64) }}",
            self.value_at_type(duration, self.type_id(Type::Float)?)?,
            set_timeout = smelt_stdlib::runtime_symbols::timers::SET_TIMEOUT,
        )))
    }

    /// `setInterval`.
    fn set_interval_expr(&self, args: &[Operand]) -> Result<RustExpr, EmitError> {
        // `setInterval(callback, period)` shares the timer-callback shape
        // with `setTimeout`: the only difference is that the runtime helper
        // re-arms the timer after each fire (see `smelt_set_interval`). The
        // callback-invocation snippet is identical to the timeout case.
        let [callback, duration, extra @ ..] = args else {
            return Err(EmitError::new(
                "setInterval requires callback and period operands",
            ));
        };
        let callback_text = self.operand_expr(callback)?;
        let callback_call = self.timer_callback_call_expr(callback, extra.first())?;
        let extra_binding = self.timer_extra_args_binding(extra.first())?;
        Ok(RustExpr::atom(format!(
            "{{ let smelt_timer_callback = {callback_text}.clone(); {extra_binding}{set_interval}(::std::rc::Rc::new(::std::cell::RefCell::new(move || {{ {callback_call} }})), {} as f64) }}",
            self.value_at_type(duration, self.type_id(Type::Float)?)?,
            set_interval = smelt_stdlib::runtime_symbols::timers::SET_INTERVAL,
        )))
    }

    /// `new Promise(executor)`.
    fn promise_constructor_expr(
        &self,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let [executor] = args else {
            return Err(EmitError::new("Promise requires an executor operand"));
        };
        let Some(&Type::Future(output_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("Promise destination must be a future"));
        };
        let executor_text = self.operand_expr(executor)?;
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
        let output_text = self.signature_type(output_ty)?;
        // `Promise<void>` still accepts an ignored optional value, but
        // a concrete `Promise<T>` exposes a typed resolver. Keeping the
        // resolver parameter at `T` lets named executors pass through
        // without erasing and reconstructing their argument.
        let resolve_input_ty = if self.mir.types.get(output_ty) == Some(&Type::None) {
            self.type_id(Type::Unknown)?
        } else {
            output_ty
        };
        let resolve_input_text = self.signature_type(resolve_input_ty)?;
        // The executor's declared `resolve` type goes through
        // `function_type_param`, so a list-typed resolver argument is
        // spelled `&SmeltList<..>` there. This synthesized resolver has to
        // match that spelling exactly or the executor call does not type-check,
        // and its body then needs an owned value to store into the result slot.
        let resolve_by_ref =
            self.synthesized_callback_param_is_shared_reference(resolve_input_ty);
        let resolve_input_text = if resolve_by_ref {
            format!("&{resolve_input_text}")
        } else {
            resolve_input_text.into_string()
        };
        let resolve_source = if resolve_by_ref {
            RustExpr::chain("value.clone()").ending_in_clone()
        } else {
            RustExpr::path("value")
        };
        let resolve_value = self.rendered_at_type(
            &resolve_source,
            resolve_input_ty,
            output_ty,
            &self.render_scope(),
        )?;
        // `reject` is synthesized the same way `resolve` is, and its
        // parameter is always the erased rejection reason. Once
        // `SmeltUnknown` is a by-shared-reference callback parameter the
        // executor's declared `reject` is spelled `&SmeltUnknown`, so the
        // synthesized closure has to match it and clone the reason out
        // before handing it to the throw adapter.
        let reject_by_ref = self
            .synthesized_callback_param_is_shared_reference(self.type_id(Type::Unknown)?);
        self.context.note_carrier_use();
        let (reject_input_text, reject_value) = if reject_by_ref {
            ("&SmeltUnknown", "error.clone()")
        } else {
            ("SmeltUnknown", "error")
        };
        // `reject(value)` is a `throw` that crosses a future boundary, so
        // it enters the error channel through the same payload-preserving
        // adapter as `Terminator::Throw` (see `crate::thrown`). The
        // rejection reason keeps its class, `name`, `message`, `cause` and
        // custom fields, and `SmeltThrown`'s `Display` still projects
        // `message`, so a string-typed `catch` observes the same text this
        // site used to build by hand.
        Ok(RustExpr::atom(format!(
            "{{ let smelt_promise_result: ::std::rc::Rc<::std::cell::RefCell<Option<Result<{output_text}, {ERROR_BOX}>>>> = ::std::rc::Rc::new(::std::cell::RefCell::new(None)); let smelt_resolve_result = smelt_promise_result.clone(); let smelt_reject_result = smelt_promise_result.clone(); let smelt_resolve: ::std::rc::Rc<dyn Fn({resolve_input_text}) -> ()> = ::std::rc::Rc::new(move |value: {resolve_input_text}| {{ *smelt_resolve_result.borrow_mut() = Some(Ok({resolve_value})); }}); let smelt_reject: ::std::rc::Rc<dyn Fn({reject_input_text}) -> ()> = ::std::rc::Rc::new(move |error: {reject_input_text}| {{ *smelt_reject_result.borrow_mut() = Some(Err({throw_fn}({reject_value}))); }}); {executor_call} SmeltFuture::from_future(Box::pin(async move {{ loop {{ if let Some(result) = smelt_promise_result.borrow_mut().take() {{ break result; }} tokio::task::yield_now().await; {sleep_ms}(0.0).await; }} }})) }}",
            sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
            throw_fn = crate::thrown::THROW_FN,
        )))
    }

    /// `.then`.
    fn promise_then_expr(&self, args: &[Operand], dest_ty: TypeId) -> Result<RustExpr, EmitError> {
        let [future, callback] = args else {
            return Err(EmitError::new(
                "Promise.then requires future and callback operands",
            ));
        };
        let Some(Type::Future(_)) = self.mir.types.get(self.operand_ty(future)?) else {
            return Err(EmitError::new("Promise.then receiver must be a future"));
        };
        let future_text = self.await_operand_expr(future)?;
        let (prelude, callback_expr) = self.promise_callback_hoist(callback)?;
        let invocation =
            self.promise_callback_invocation_with(callback, &RustExpr::opaque(callback_expr), &RustExpr::path("smelt_value"))?;
        let Some(Type::Future(output_ty)) = self.mir.types.get(dest_ty) else {
            return Err(EmitError::new("Promise.then result must be a future"));
        };
        let (settle, settled_ty) = self.promise_callback_settle(callback, &invocation)?;
        let output =
            self.rendered_at_type(&RustExpr::path("smelt_callback_value"), settled_ty, *output_ty, &self.render_scope())?;
        Ok(RustExpr::atom(format!(
            "{{ {prelude}SmeltFuture::from_future(Box::pin(async move {{ let smelt_value = {future_text}.await?; {settle} Ok::<_, {ERROR_BOX}>({output}) }})) }}"
        )))
    }

    /// `.catch`.
    fn promise_catch_expr(
        &self,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let [future, callback] = args else {
            return Err(EmitError::new(
                "Promise.catch requires future and callback operands",
            ));
        };
        let Some(Type::Future(output_ty)) = self.mir.types.get(self.operand_ty(future)?)
        else {
            return Err(EmitError::new("Promise.catch receiver must be a future"));
        };
        let future_text = self.await_operand_expr(future)?;
        let (prelude, callback_expr) = self.promise_callback_hoist(callback)?;
        // The handler receives the THROWN VALUE, recovered from the
        // error channel — not the rejection's message text. A `catch`
        // binding sees the value JavaScript threw, so `error instanceof
        // Error`, `error.name` and a non-`Error` rejection reason all
        // answer as they do in the source.
        let invocation = self.promise_callback_invocation_with(
            callback,
            &RustExpr::opaque(callback_expr),
            &crate::thrown::thrown_value_expr("smelt_error"),
        )?;
        // `catch` RECOVERS: the promise it answers settles with the
        // handler's value, exactly as `then`'s does. Discarding it and
        // answering the output type's default (`let _ = {invocation};`)
        // made `await p.catch(() => 'fallback')` evaluate to `""` — a
        // silent wrong value on the standard recovery idiom.
        //
        // A handler that returns a promise is flattened, so `catch`
        // never answers `Promise<Promise<T>>`; this is the same
        // settle/flatten pair the `Then` arm above uses.
        let (settle, settled_ty) = self.promise_callback_settle(callback, &invocation)?;
        let recovered =
            self.rendered_at_type(&RustExpr::path("smelt_callback_value"), settled_ty, *output_ty, &self.render_scope())?;
        Ok(RustExpr::atom(format!(
            "{{ {prelude}SmeltFuture::from_future(Box::pin(async move {{ match {future_text}.await {{ Ok(smelt_value) => Ok::<_, {ERROR_BOX}>(smelt_value), Err(smelt_error) => {{ {settle} Ok::<_, {ERROR_BOX}>({recovered}) }} }} }})) }}"
        )))
    }

    /// `.finally`.
    fn promise_finally_expr(
        &self,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let [future, callback] = args else {
            return Err(EmitError::new(
                "Promise.finally requires future and callback operands",
            ));
        };
        let Some(Type::Future(_)) = self.mir.types.get(self.operand_ty(future)?) else {
            return Err(EmitError::new("Promise.finally receiver must be a future"));
        };
        let future_text = self.await_operand_expr(future)?;
        let (prelude, callback_expr) = self.promise_callback_hoist(callback)?;
        // `onFinally` takes no argument; the invocation helper drops the
        // placeholder for a zero-parameter callback.
        let invocation =
            self.promise_callback_invocation_with(callback, &RustExpr::opaque(callback_expr), &RustExpr::unit())?;
        // The settle step awaits a returned promise and propagates a
        // throw or rejection with `?` — which is exactly the case where
        // `finally`'s own rejection replaces the original outcome.
        // Otherwise the callback's value is discarded and the ORIGINAL
        // result (value or rejection) is the settled one.
        let (settle, _) = self.promise_callback_settle(callback, &invocation)?;
        Ok(RustExpr::atom(format!(
            "{{ {prelude}SmeltFuture::from_future(Box::pin(async move {{ let smelt_outcome = {future_text}.await; {settle} let _ = smelt_callback_value; smelt_outcome }})) }}"
        )))
    }

    /// `fetch`.
    fn http_fetch_async_expr(
        &self,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let Some(url) = args.first() else {
            return Err(EmitError::new("async fetch requires a URL operand"));
        };
        // `fetch(request)` — which `fetch(url, init)` is defined to be —
        // carries a method, a header list and a body, so it needs the
        // full client rather than the one-line GET.
        if self.is_request_class_type(self.operand_ty(url)?)? {
            return self.http_fetch_request_expr(url);
        }
        if !matches!(
            self.mir.types.get(self.operand_ty(url)?),
            Some(Type::String)
        ) {
            return Err(EmitError::new("async fetch URL must be a string"));
        }
        // The response is assembled from the parts the transport
        // actually reports, so nothing is invented: the status and its
        // canonical reason phrase, every response header in order, and
        // the body as RAW BYTES. Bytes rather than text is deliberate —
        // `SmeltBody::from_text` would stamp an implied
        // `text/plain;charset=UTF-8`, and a fetched response's content
        // type belongs to the server, not to how Smelt read it.
        Ok(RustExpr::atom(format!(
            "SmeltFuture::from_future(Box::pin(async move {{                      let smelt_http = reqwest::get({}).await.expect(\"HTTP request failed\");                      let smelt_status = f64::from(smelt_http.status().as_u16());                      let smelt_reason = smelt_http.status().canonical_reason().unwrap_or_default().to_owned();                      let smelt_pairs: Vec<(String, String)> = smelt_http.headers().iter().map(|(smelt_name, smelt_value)| (smelt_name.as_str().to_owned(), smelt_value.to_str().unwrap_or_default().to_owned())).collect();                      let smelt_bytes = smelt_http.bytes().await.expect(\"HTTP response body read failed\").to_vec();                      Ok::<_, {ERROR_BOX}>(SmeltResponse::from_parts(smelt_status, smelt_reason, SmeltHeaders::from_pairs(smelt_pairs), SmeltBody::from_bytes(smelt_bytes))) }}))",
            self.operand_expr(url)?
        )))
    }
}
