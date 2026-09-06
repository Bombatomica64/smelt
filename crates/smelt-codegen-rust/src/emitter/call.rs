//! Call emission helpers.

use super::*;
use crate::generic_bindings::{
    CalleeTypeParamBindings, bind_class_type_params, collect_bindings_from_types,
    substituted_type_id, substitution_matches, type_param_occurs,
};
use rendered_text_rewrite::shared_capture_cell_name;
use smelt_hir::FunctionType;

/// The concrete instantiation one static call site pins on a generic callee.
///
/// Produced by [`FunctionEmitter::static_call_monomorphization`] and consumed by
/// both halves of the same call: the argument rendering and the type the call
/// expression is claimed to produce. Sharing one decision between them is what
/// keeps the two sides from disagreeing — a monomorphized argument with an
/// erased return claim (or the reverse) is exactly the E0308 this analysis
/// exists to prevent.
// `Debug`/`Eq` exist only for the debug-only seam assertion that compares the
// argument half of a call site against its return half; a release build derives
// nothing, exactly as before the assertions landed.
#[cfg_attr(debug_assertions, derive(Debug, Eq, PartialEq))]
pub(super) struct CallMonomorphization {
    /// Every declared type parameter of the callee, pinned to a concrete type.
    bindings: CalleeTypeParamBindings,
    /// The callee's declared return type with `bindings` applied.
    ///
    /// This is the type the emitted Rust call really evaluates to, which is not
    /// in general the destination local's type: the frontend's destination
    /// carries the type *after* the surrounding conversion (`last<T>(xs): T |
    /// undefined` called on a `number[][]` produces `Option<SmeltList<f64>>`
    /// into a `SmeltList<f64>` destination).
    return_ty: TypeId,
}

/// How a mutable-list argument needs its elements treated at a call site.
///
/// A `&mut SmeltList<..>` argument only type-checks when the caller's rendered
/// element type matches the callee's, because Rust `&mut` references are
/// invariant. There are two ways to reach that state and they are opposites, so
/// the convert-in-place adapter classifies every mutable-list argument into one
/// of them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MutListArgKind {
    /// The callee emits real Rust generics and its parameter is that generic
    /// list shape instantiated by this argument (`arr: &mut SmeltList<T>`
    /// receiving a `SmeltList<f64>`). Rust binds the callee's type parameter
    /// from the argument, so the elements must pass through *unconverted*:
    /// erasing them would monomorphize the callee at `SmeltUnknown` and clash
    /// with the concrete destination.
    Monomorphized,
    /// The callee renders its element at a different concrete type (typically an
    /// erased `SmeltUnknown` monomorphization). `&mut` invariance rules out a
    /// direct reborrow, so each element is converted on the way in and back out.
    Erased,
}

/// How a mutable-list argument's caller-side place is spelled in Rust.
///
/// The adapter both *reads* the place's current contents into a temporary and
/// *writes* the mutated contents back, and `place_text` renders only a read
/// expression whose shape depends on how the binding is stored. The three
/// storages need three different spellings, so mixing them up is a silent
/// miscompile rather than a type error.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MutListPlaceStorage {
    /// A forwarded `&mut` parameter of the current function: `place_text` is the
    /// reference itself (`arr`), so the value lives behind a deref.
    Reference,
    /// A shared closure capture: `place_text` is `(*smelt_capture_x.borrow())`,
    /// and the write needs a fresh `borrow_mut()` of the same cell.
    SharedCapture,
    /// An ordinary owned local: `place_text` is already the value.
    Owned,
}

/// The read expression and assignment target for a mutable-list argument place.
struct MutListPlaceAccess {
    /// Expression producing an owned clone of the list currently in the place.
    read: String,
    /// Place expression the mutated list is assigned back to.
    assign_target: String,
    /// Place expression that may be borrowed mutably *for the whole call*, when
    /// that is safe. `None` for a shared capture, whose `RefCell` guard must not
    /// stay alive across the callee (a callback could re-enter the same cell and
    /// panic with "already borrowed"), so those must go through a temporary.
    mut_borrow: Option<String>,
}

/// The identity markers a `value instanceof <class>` probe must accept.
///
/// Usually one — the class's own registry marker. It is more than one where the
/// platform has a real subclass relation, which the registry already records in
/// `to_string_tag`: Node's `Buffer` *is* a `Uint8Array`, reports
/// `[object Uint8Array]`, and must satisfy `buf instanceof Uint8Array`. Deriving
/// the set from the tag keeps the subclass relation in the registry rather than
/// hard-coded at this call site, and returns `None` for names that are not
/// modeled host objects so the caller falls through to its class dispatch.
fn host_instance_markers(class_name: &str) -> Option<Vec<&'static str>> {
    let markers = smelt_stdlib::HOST_OBJECTS
        .iter()
        .filter(|entry| entry.class_name == class_name || entry.to_string_tag == class_name)
        .map(|entry| entry.marker)
        .collect::<Vec<_>>();
    (!markers.is_empty()).then_some(markers)
}

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
                            "{{ let mut smelt_racers: Vec<::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<_, Box<dyn std::error::Error>>>>>> = Vec::new(); {pushes} {promise_race}(smelt_racers).await? }}",
                            promise_race =
                                smelt_stdlib::runtime_symbols::timers::PROMISE_RACE,
                        )
                    }
                };
                Ok(format!(
                    "SmeltFuture::from_future(Box::pin(async move {{ Ok::<_, Box<dyn std::error::Error>>({body}) }}))"
                ))
            }
            smelt_hir::AsyncOp::ExitDrain => Ok(format!(
                "SmeltFuture::from_future(Box::pin(async move {{ {exit_drain}().await; Ok::<_, Box<dyn std::error::Error>>(()) }}))",
                exit_drain = smelt_stdlib::runtime_symbols::timers::RUN_UNTIL_EXIT,
            )),
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
            smelt_hir::AsyncOp::Resolve => {
                // `Promise.resolve(v)`: defer one microtask, then settle with the
                // operand. The value is rendered at the future's own item type so
                // the emitted `Ok(..)` agrees with the declared `SmeltFuture<T>`;
                // with no operand (`Promise.resolve()`) the item type is unit.
                let Some(duration) = args.first() else {
                    return Err(EmitError::new(
                        "async resolve requires a duration operand",
                    ));
                };
                let duration_text = self.operand_text(duration)?;
                let value_text = match args.get(1) {
                    Some(value) => {
                        let Some(Type::Future(item_ty)) = self.mir.types.get(dest_ty) else {
                            return Err(EmitError::new(
                                "async resolve must produce a future type",
                            ));
                        };
                        self.value_at_type(value, *item_ty)?
                    }
                    None => "()".to_owned(),
                };
                Ok(format!(
                    "SmeltFuture::from_future(Box::pin(async move {{ {sleep_ms}({duration_text} as f64).await; Ok::<_, Box<dyn std::error::Error>>({value_text}) }}))",
                    sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
                ))
            }
            smelt_hir::AsyncOp::Reject => {
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
                let duration_text = self.operand_text(duration)?;
                let payload = match args.get(1) {
                    Some(reason) => self.thrown_payload_text(reason)?,
                    None => self.undefined_thrown_payload_text(),
                };
                Ok(format!(
                    "SmeltFuture::from_future(Box::pin(async move {{ {sleep_ms}({duration_text} as f64).await; Err::<_, Box<dyn std::error::Error>>({payload}) }}))",
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
                // `Promise<void>` still accepts an ignored optional value, but
                // a concrete `Promise<T>` exposes a typed resolver. Keeping the
                // resolver parameter at `T` lets named executors pass through
                // without erasing and reconstructing their argument.
                let resolve_input_ty = if self.mir.types.get(output_ty) == Some(&Type::None) {
                    self.type_id(Type::Unknown)?
                } else {
                    output_ty
                };
                let resolve_input_text = self.type_text(resolve_input_ty)?;
                // The executor's declared `resolve` type goes through
                // `function_type_param_text`, so a list-typed resolver argument is
                // spelled `&SmeltList<..>` there. This synthesized resolver has to
                // match that spelling exactly or the executor call does not type-check,
                // and its body then needs an owned value to store into the result slot.
                let resolve_by_ref =
                    self.synthesized_callback_param_is_shared_reference(resolve_input_ty);
                let resolve_input_text = if resolve_by_ref {
                    format!("&{resolve_input_text}")
                } else {
                    resolve_input_text
                };
                let resolve_value = self.value_at_type_text(
                    if resolve_by_ref { "value.clone()" } else { "value" },
                    resolve_input_ty,
                    output_ty,
                )?;
                // `reject` is synthesized the same way `resolve` is, and its
                // parameter is always the erased rejection reason. Once
                // `SmeltUnknown` is a by-shared-reference callback parameter the
                // executor's declared `reject` is spelled `&SmeltUnknown`, so the
                // synthesized closure has to match it and clone the reason out
                // before handing it to the throw adapter.
                let reject_by_ref = self
                    .synthesized_callback_param_is_shared_reference(self.type_id(Type::Unknown)?);
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
                Ok(format!(
                    "{{ let smelt_promise_result: ::std::rc::Rc<::std::cell::RefCell<Option<Result<{output_text}, Box<dyn std::error::Error>>>>> = ::std::rc::Rc::new(::std::cell::RefCell::new(None)); let smelt_resolve_result = smelt_promise_result.clone(); let smelt_reject_result = smelt_promise_result.clone(); let smelt_resolve: ::std::rc::Rc<dyn Fn({resolve_input_text}) -> ()> = ::std::rc::Rc::new(move |value: {resolve_input_text}| {{ *smelt_resolve_result.borrow_mut() = Some(Ok({resolve_value})); }}); let smelt_reject: ::std::rc::Rc<dyn Fn({reject_input_text}) -> ()> = ::std::rc::Rc::new(move |error: {reject_input_text}| {{ *smelt_reject_result.borrow_mut() = Some(Err({throw_fn}({reject_value}))); }}); {executor_call} SmeltFuture::from_future(Box::pin(async move {{ loop {{ if let Some(result) = smelt_promise_result.borrow_mut().take() {{ break result; }} tokio::task::yield_now().await; {sleep_ms}(0.0).await; }} }})) }}",
                    sleep_ms = smelt_stdlib::runtime_symbols::timers::SLEEP_MS,
                    throw_fn = crate::thrown::THROW_FN,
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
                let Some(Type::Future(output_ty)) = self.mir.types.get(dest_ty) else {
                    return Err(EmitError::new("Promise.then result must be a future"));
                };
                let callback_return_ty = match self.mir.types.get(self.operand_ty(callback)?) {
                    Some(Type::Function(function)) => function.return_ty,
                    _ => self.type_id(Type::Unknown)?,
                };
                let (settle, settled_ty) = match self.mir.types.get(callback_return_ty) {
                    Some(Type::Future(item)) => (
                        format!("let smelt_callback_value = {invocation}.await?;"),
                        *item,
                    ),
                    _ => (
                        format!("let smelt_callback_value = {invocation};"),
                        callback_return_ty,
                    ),
                };
                let output =
                    self.value_at_type_text("smelt_callback_value", settled_ty, *output_ty)?;
                Ok(format!(
                    "{{ {prelude}SmeltFuture::from_future(Box::pin(async move {{ let smelt_value = {future_text}.await?; {settle} Ok::<_, Box<dyn std::error::Error>>({output}) }})) }}"
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
                    "SmeltUnknown::String(smelt_error.to_string().into())",
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
            smelt_hir::AsyncOp::HttpFetch => {
                let Some(url) = args.first() else {
                    return Err(EmitError::new("async fetch requires a URL operand"));
                };
                // `fetch(request)` — which `fetch(url, init)` is defined to be —
                // carries a method, a header list and a body, so it needs the
                // full client rather than the one-line GET.
                if self.is_request_class_type(self.operand_ty(url)?)? {
                    return self.http_fetch_request_text(url);
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
                Ok(format!(
                    "SmeltFuture::from_future(Box::pin(async move {{                      let smelt_http = reqwest::get({}).await.expect(\"HTTP request failed\");                      let smelt_status = f64::from(smelt_http.status().as_u16());                      let smelt_reason = smelt_http.status().canonical_reason().unwrap_or_default().to_owned();                      let smelt_pairs: Vec<(String, String)> = smelt_http.headers().iter().map(|(smelt_name, smelt_value)| (smelt_name.as_str().to_owned(), smelt_value.to_str().unwrap_or_default().to_owned())).collect();                      let smelt_bytes = smelt_http.bytes().await.expect(\"HTTP response body read failed\").to_vec();                      Ok::<_, Box<dyn std::error::Error>>(SmeltResponse::from_parts(smelt_status, smelt_reason, SmeltHeaders::from_pairs(smelt_pairs), SmeltBody::from_bytes(smelt_bytes))) }}))",
                    self.operand_text(url)?
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
            list_read_text(&self.value_at_type(extra_operand, args_ty)?)
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
            Callee::Builtin(BuiltinFn::ConsoleLog { absent }) => {
                let rendered_args = args
                    .iter()
                    .map(|arg| self.console_arg_text(arg, *absent))
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
                // `process.stdout.write` takes a string, so the optional arm is
                // unreachable from here; the TypeScript spelling is the right
                // default for a write that only comes from that surface.
                let (format_spec, value) = args
                    .first()
                    .map(|argument| self.console_arg_text(argument, AbsentSpelling::Undefined))
                    .transpose()?
                    .unwrap_or_else(|| ("{}", "\"\"".to_owned()));
                let macro_name = if matches!(callee, Callee::Builtin(BuiltinFn::ConsoleWrite)) {
                    "print"
                } else {
                    "eprint"
                };
                Ok(format!("{{ {macro_name}!(\"{format_spec}\", {value}); }}"))
            }
            Callee::Builtin(BuiltinFn::JsonParse) => {
                let text = args
                    .first()
                    .ok_or_else(|| EmitError::new("JSON parse takes one string argument"))?;
                if !matches!(
                    self.mir.types.get(self.operand_ty(text)?),
                    Some(Type::String)
                ) {
                    return Err(EmitError::new("JSON parse input must be a string"));
                }
                // The trailing `?` is what marks this call fallible to
                // `emit_throwing_call_terminator`, which then renders the
                // `Ok(Ok(v)) / Ok(Err(e))` shape that binds the caught
                // `SyntaxError` and jumps to the handler's catch block.
                Ok(format!(
                    "{}(&{})?",
                    crate::thrown::JSON_PARSE_FN,
                    self.operand_text(text)?
                ))
            }
            Callee::Builtin(BuiltinFn::UriDecode(op)) => {
                let value = args.first().ok_or_else(|| {
                    EmitError::new("a URI decoder takes one string argument")
                })?;
                let adapter = match op {
                    smelt_hir::UriTranscodeOp::Decode => crate::thrown::DECODE_URI_FN,
                    smelt_hir::UriTranscodeOp::DecodeComponent => {
                        crate::thrown::DECODE_URI_COMPONENT_FN
                    }
                    // Only the decoders are fallible, so only they become a
                    // callee; `is_fallible` in the frontend is what guarantees
                    // it, and this arm exists so a change there is a compile
                    // error here rather than a wrong adapter.
                    smelt_hir::UriTranscodeOp::Encode
                    | smelt_hir::UriTranscodeOp::EncodeComponent => {
                        return Err(EmitError::new(
                            "internal: an infallible URI encoder reached the fallible call path",
                        ));
                    }
                };
                // The trailing `?` is what marks the call fallible to
                // `emit_throwing_call_terminator`, which renders the
                // `Ok(Ok(v)) / Ok(Err(e))` shape binding the caught `URIError`
                // and jumping to the handler's catch block.
                let value_text = self.string_like_operand_text(value, "URI decoder input")?;
                Ok(format!("{adapter}({value_text}.as_str())?"))
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
                    // The rest-parameter pre-pass: the SECOND, shorter argument
                    // ladder in this emitter, and the one that is still NOT a
                    // total function. It has three rungs (borrowed callback,
                    // mutable reference, value) where the main ladder below has
                    // eight, and it deliberately lacks the callee-generic,
                    // monomorphization-passthrough and demoting-erased rungs, so
                    // it cannot share `emitter::static_call_args`'s classifier
                    // without changing emitted bytes. Totalizing it means first
                    // deciding whether those three missing rungs belong here.
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
                                arg, target_ty, None,
                            )?);
                        } else if self.parameter_needs_mutable_reference_in(function, param) {
                            rendered_args.push(self.mutable_reference_argument_text(
                                arg,
                                target_ty,
                                Some(&self.callee_free_function_type_params(function)),
                            )?);
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
                // One decision per call site, shared with the return side in
                // `call_emitted_source_ty`: either this call monomorphizes the
                // callee at the caller's concrete types (arguments pass through,
                // the call's type is the substituted return) or it does not
                // (everything erases, exactly as before).
                let monomorphization = self.static_call_monomorphization(function, args)?;
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
                    // The ladder proper lives in `emitter::static_call_args`:
                    // one classifier decides which of the eight outcomes owns
                    // this argument, and the dispatch below matches that
                    // decision exhaustively. `Surplus` renders nothing, so this
                    // loop can push fewer arguments than it classifies.
                    let callee_bindings =
                        monomorphization.as_ref().map(|pinned| &pinned.bindings);
                    let kind = self.classify_static_call_argument(
                        function,
                        index,
                        arg,
                        param,
                        target_ty,
                        &free_function_type_params,
                        callee_bindings,
                    )?;
                    self.render_static_call_argument(
                        kind,
                        function,
                        arg,
                        &free_function_type_params,
                        callee_bindings,
                        &mut rendered_args,
                    )?;
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
                        rendered_args.push(self.borrowed_default_function_text(
                            target_ty,
                            monomorphization.as_ref().map(|pinned| &pinned.bindings),
                        )?);
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
                // An erased callee (source `unknown`, a union, an erased class)
                // has no static function type, so the concrete callable shape is
                // only known at run time. It is the same boundary `ClosureCall`
                // already routes through `dynamic_callable_dispatch_text`; the
                // two call forms must agree, or moving a call between the
                // statement and terminator forms changes whether it is emittable
                // at all.
                let callee_ty = self.operand_ty(indirect_callee)?;
                if self.callee_is_dynamically_dispatched(callee_ty) {
                    let callee_text = self.operand_text(indirect_callee)?;
                    let rendered_args = args
                        .iter()
                        .map(|arg| self.erase(arg))
                        .collect::<Result<Vec<_>, EmitError>>()?;
                    let args_expr = format!("vec![{}]", rendered_args.join(", "));
                    return Ok(self.dynamic_callable_dispatch_text(&callee_text, &args_expr));
                }
                let Some(Type::Function(function)) = self.mir.types.get(callee_ty) else {
                    return Err(EmitError::new("indirect call target is not a function"));
                };
                let callee_text = self.operand_text(indirect_callee)?;
                // An erased-unknown-rest callable *value* (e.g. an untyped
                // `vi.fn()` spy) lowers to `SmeltErasedFunction`, whose call ABI
                // is the `.call(vec![..])` method over an erased argument vector,
                // not a direct `(f)(args)` invocation. The same MIR type bound to
                // a borrowed callback parameter is emitted as a bare `&dyn Fn`
                // instead, so the question is delegated to the one authority
                // (`callee_uses_erased_call_method`) that `Rvalue::ClosureCall`
                // also consults — otherwise moving a call between the statement
                // and terminator forms would flip its ABI.
                if self.callee_uses_erased_call_method(indirect_callee)? {
                    return Ok(format!(
                        "({callee_text}).call({})",
                        self.erased_call_argument_vector_text(indirect_callee, args)?
                    ));
                }
                // A rest-only callee invoked with positional arguments takes them
                // packed into its single rest `SmeltList` parameter; without this
                // the per-parameter mapping below would report "too many
                // arguments".
                let rendered_args = match self.rest_vector_call_args_text(args, Some(function))? {
                    Some(packed) => packed.join(", "),
                    None => self.indirect_call_args_text(function, args)?,
                };
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
        // A fully erased callback value (`Type::Unknown` — e.g. a stateful
        // `vi.fn()` mock object passed as `then(spy)`) carries its callable
        // behind the `__smelt_call` field, so route through the standard
        // callable-object dispatch; a non-callable value is dropped like an
        // absent handler (the resolved value still evaluates).
        if self.mir.types.get(self.operand_ty(callback)?) == Some(&Type::Unknown) {
            return Ok(format!(
                "{{ let smelt_callable = match ({callback_text}).clone() {{ SmeltUnknown::Function(smelt_function) => Some(smelt_function), SmeltUnknown::Object(smelt_object) => match smelt_object.get(\"__smelt_call\") {{ Some(SmeltUnknown::Function(smelt_function)) => Some(smelt_function), _ => None }}, _ => None }}; match smelt_callable {{ Some(smelt_function) => (smelt_function)(vec![IntoSmeltUnknown::into_smelt_unknown({arg_expr})]).unwrap_or_else(|error| smelt_panic_throw(error)), None => {{ let _ = {arg_expr}; SmeltUnknown::Undefined }} }} }}"
            ));
        }
        if let Some(Type::Function(function)) = self.mir.types.get(self.operand_ty(callback)?) {
            if self.is_erased_unknown_rest_function(function) && !function.may_throw {
                return Ok(format!(
                    "({callback_text}).call(vec![IntoSmeltUnknown::into_smelt_unknown({arg_expr})])"
                ));
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
                let arg_text = self.callback_call_arg_text(
                    function,
                    0,
                    param_ty,
                    arg_expr.to_owned(),
                );
                return Ok(format!("({callback_text})({arg_text})"));
            }
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
    pub(super) fn erased_call_args_text(
        &self,
        args: &[Operand],
    ) -> Result<String, EmitError> {
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
    ///
    /// A source call may supply fewer arguments than the callee's function type
    /// declares whenever the trailing parameters are optional or defaulted
    /// (`const f = (name?: string) => ..` invoked as `f()`). JavaScript fills the
    /// gap with `undefined`, but the Rust value the callee lowered to is a
    /// `dyn Fn(..)` of fixed arity, so every declared parameter must receive an
    /// expression or rustc reports E0057 ("this function takes N arguments but M
    /// arguments were supplied"). Missing trailing parameters are therefore
    /// padded with their parameter type's default value — the same rule the
    /// static-call argument ladder already applies to direct calls of a known
    /// function, applied here to the value-callable form so both call shapes
    /// agree on arity.
    pub(super) fn indirect_call_args_text(
        &self,
        function: &FunctionType,
        args: &[Operand],
    ) -> Result<String, EmitError> {
        let mut rendered_args = args
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                let target_ty = function
                    .params
                    .get(index)
                    .copied()
                    .ok_or_else(|| EmitError::new("indirect call has too many arguments"))?;
                if function.mutable_params.contains(&index) {
                    self.mutable_reference_argument_text(arg, target_ty, None)
                } else if self.callback_param_is_shared_reference(function, index, target_ty)
                {
                    // The parameter is `&T` (see `callback_param_is_shared_reference`).
                    self.shared_reference_argument_text(arg, target_ty)
                } else {
                    self.value_at_type(arg, target_ty)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (index, target_ty) in function
            .params
            .iter()
            .copied()
            .enumerate()
            .skip(args.len())
        {
            // A by-reference (`mutable_params`) parameter lowers to `&mut T`,
            // which has no value-shaped default expression to pad with; such a
            // parameter is only reachable through an explicit argument, so stop
            // padding rather than inventing a temporary place for it.
            if function.mutable_params.contains(&index) {
                break;
            }
            // A padded argument is a fresh temporary, so a by-shared-reference
            // parameter borrows it in place.
            rendered_args.push(self.callback_call_arg_text(
                function,
                index,
                target_ty,
                self.default_value(target_ty)?,
            ));
        }
        Ok(rendered_args.join(", "))
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
    ///
    /// # Relationship to [`crate::generic_bindings`]
    ///
    /// This is *not* dead duplication of the shared matcher, and it is
    /// deliberately not refactored onto it yet. The two answer different
    /// questions and diverge in both directions:
    ///
    /// - **answer shape** — this is a whole-tree conjunction over one
    ///   (source, target) pair, so a mismatch anywhere is fatal;
    ///   `generic_bindings` records evidence per *type parameter* and drops a
    ///   mismatch that lands in a type-parameter-free subtree (a `Dict` key, a
    ///   tuple slot);
    /// - **unions** — this zips equal-length unions positionally and so accepts
    ///   an identical nested union; the shared matcher refuses to bind through
    ///   unions at all, because unions erase in emitted Rust and member order is
    ///   not a sound correspondence;
    /// - **`Class` / `JsMap` / `Function` / generators** — this has no arm for
    ///   them and falls through to identity; the shared matcher descends into
    ///   all of them. Descending here would divert a callback argument off the
    ///   borrowed-callback path and change emitted bytes;
    /// - **erased sources** — a bare `TypeParam` target returns `true` here for
    ///   any source, including `SmeltUnknown`; the shared matcher records that
    ///   as `Erased`, which its only public query reports as not concrete.
    ///
    /// # Remaining scope
    ///
    /// Increment 0b deleted this predicate's *general* consumers: the composite
    /// argument decision and the return-conversion ladder now ask
    /// [`crate::generic_bindings::substitution_matches`], which is exact and
    /// unforgiving, instead of this whole-tree conjunction. What is left is one
    /// caller, [`Self::mut_list_adapter_arg`], so this is now the mutable-list
    /// adapter's private rule rather than a shared one.
    ///
    /// It is kept rather than migrated because the adapter *needs* the
    /// divergences listed above: its `TypeParam`-target-accepts-anything arm is
    /// what classifies a `&mut SmeltList<T>` argument as `Monomorphized`, and
    /// `&mut` invariance makes the adapter's own writeback protocol, not this
    /// predicate, the thing that decides correctness there. Migrating it is a
    /// separate change with its own corpus evidence.
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

    /// Return the generic type parameters declared by the class that owns
    /// `function`, in declaration order.
    ///
    /// [`Self::callee_class_type_params`] answers the membership question and
    /// returns a set; binding needs the order, because the receiver type's class
    /// arguments correspond to the declaration positionally.
    fn callee_class_type_param_names(&self, function: &MirFunction) -> Vec<Symbol> {
        let class_name = match function.origin {
            HirOrigin::ClassConstructor { class, .. } | HirOrigin::ClassMethod { class, .. } => {
                class
            }
            HirOrigin::Body(_) | HirOrigin::ClassStaticMethod { .. } => return Vec::new(),
        };
        self.mir
            .classes
            .iter()
            .find(|class| class.name == class_name)
            .map(|class| class.type_params.iter().map(|param| param.name).collect())
            .unwrap_or_default()
    }

    /// Return whether `ty` mentions any of `names`.
    fn type_mentions_any(&self, ty: TypeId, names: &HashSet<Symbol>) -> bool {
        names
            .iter()
            .any(|name| type_param_occurs(self.mir, ty, *name))
    }

    /// Decide whether one static call really monomorphizes a generic callee at
    /// the caller's concrete types.
    ///
    /// This replaces the former bare-`TypeParam`-return predicates. The return
    /// shape is no longer an input to the decision: what licenses passing an
    /// argument through concretely is that *this call site pins every one of the
    /// callee's type parameters*, not what the callee happens to return. The
    /// substituted return type falls out of the same bindings
    /// ([`CallMonomorphization::return_ty`]), which is how a composite
    /// `T[] -> T[]` callee becomes usable at all.
    ///
    /// Accepts only when every one of the following holds; otherwise this call
    /// site demotes to the ordinary erased path, which is unchanged. Demotion is
    /// per *call site*, never per function: the callee stays generic and other
    /// sites keep instantiating it at `SmeltUnknown`, which its emitted bounds
    /// already satisfy.
    ///
    /// 1. the crate decided to emit this callee with real Rust generics
    ///    ([`Self::callee_free_function_type_params`]). A callee demoted by the
    ///    crate-wide gate — including every callback-bearing one, which the
    ///    callback gate still rejects — never reaches the rest of this check;
    /// 2. the callee packs no rest parameter, which would break the positional
    ///    correspondence between parameters and arguments;
    /// 3. no parameter position that mentions a type parameter is left to the
    ///    trailing default-argument loop, which renders the *erased* default and
    ///    would pin the parameter to `SmeltUnknown`;
    /// 4. every declared type parameter is `Concrete`. An `Unbound` parameter is
    ///    precisely "rustc has nothing to infer from" — passing through anyway
    ///    is E0282/E0283, so the site demotes instead;
    /// 5. every parameter position mentioning a type parameter is *exactly* its
    ///    argument's type under the bindings, so rustc reproduces the binding map
    ///    position by position;
    /// 6. the substituted return type is an interned MIR type, so the call's own
    ///    Rust type can be named rather than guessed.
    pub(super) fn static_call_monomorphization(
        &self,
        function: &MirFunction,
        args: &[Operand],
    ) -> Result<Option<CallMonomorphization>, EmitError> {
        // (1) — the crate-wide generic-emission decision.
        let own_params = self.callee_free_function_type_params(function);
        if own_params.is_empty() {
            return Ok(None);
        }
        // (2) — a packed rest parameter destroys positional alignment.
        if function.rest.is_some() {
            return Ok(None);
        }
        let rust_name = self.function_rust_name(function)?;
        let emitted_params = self.emitted_function_param_types(&rust_name)?;

        // Only the parameters the callee actually LIFTS may be bound here. A
        // parameter that erases is rendered `SmeltUnknown` in the emitted
        // signature, so binding it to the type TypeScript inferred describes a
        // signature that was never emitted. es-toolkit's `countBy<T, K extends
        // PropertyKey>` erases `K`, and binding `K = string` from the call site
        // made the callback adapter declare `-> String` against a callee whose
        // bound reads `Fn(..) -> SmeltUnknown` (E0271), and the result local
        // `SmeltRecord<String, f64>` against a returned `SmeltJsMap<SmeltUnknown,
        // f64>` (E0308).
        let liftable = crate::classes::liftable_type_params(
            self.mir,
            function,
            &self.context.owned_callback_params,
        );
        let type_params = function
            .type_params
            .iter()
            .map(|param| param.name)
            .filter(|name| liftable.contains(name))
            .collect::<Vec<_>>();
        if type_params.is_empty() {
            return Ok(None);
        }
        let mut declared = Vec::new();
        let mut actual = Vec::new();
        for (index, param) in function.params.iter().enumerate() {
            let local_ty = self.function_local_decl(function, *param)?.ty;
            let target_ty = emitted_params
                .as_ref()
                .and_then(|params| params.get(index).copied())
                .unwrap_or(local_ty);
            let Some(arg) = args.get(index) else {
                // (3) — an omitted argument in a generic-mentioning position
                // would be rendered as the erased default.
                if self.type_mentions_any(target_ty, &own_params) {
                    return Ok(None);
                }
                declared.push(target_ty);
                actual.push(None);
                continue;
            };
            declared.push(target_ty);
            actual.push(Some(self.operand_ty(arg)?));
        }

        let bindings = collect_bindings_from_types(self.mir, &type_params, &declared, &actual);
        // (4) — every type parameter pinned, or the site demotes.
        if !bindings.all_concrete() {
            return Ok(None);
        }
        // (5) — each generic-mentioning parameter is exactly its argument.
        for (target_ty, supplied) in declared.iter().zip(&actual) {
            let Some(actual_ty) = *supplied else {
                continue;
            };
            if self.type_mentions_any(*target_ty, &own_params)
                && !substitution_matches(self.mir, *target_ty, actual_ty, &bindings)
            {
                return Ok(None);
            }
        }
        // (6) — the call's own Rust type must be nameable.
        let declared_return = self
            .emitted_function_return_type(&rust_name)
            .unwrap_or(function.return_ty);
        let Some(return_ty) = substituted_type_id(self.mir, declared_return, &bindings) else {
            return Ok(None);
        };
        Ok(Some(CallMonomorphization {
            bindings,
            return_ty,
        }))
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
    pub(super) fn callee_generic_argument_text(
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
                    "(smelt_function)({rendered_args}).unwrap_or_else(|error| smelt_panic_throw(error))"
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
        if function.can_throw && !function.is_generator {
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
        // `callee_generics` is the set of type parameters the callee actually emits
        // as real Rust generics; it is empty for an erased monomorphization, so
        // the same set both renders the callee's parameter types and decides
        // whether an argument monomorphizes them. It is an *emission scope*, not
        // a call-site binding: `crate::generic_bindings` cannot express it, and
        // the post-AST increment must not conflate the two.
        let callee_generics = self.callee_free_function_type_params(function);
        // This adapter renders callback arguments without a call-site binding map
        // (see the `Type::Function` arm of the render loop below), so it can only
        // handle callees whose callback parameters mention none of the type
        // parameters the callee actually emits. When one does, decline the
        // adapter and let the ordinary call path — which computes
        // `static_call_monomorphization` and threads those bindings into every
        // callback renderer — emit the call instead.
        if !callee_generics.is_empty()
            && target_tys.iter().any(|&target_ty| {
                matches!(self.mir.types.get(target_ty), Some(Type::Function(_)))
                    && self.type_mentions_any(target_ty, &callee_generics)
            })
        {
            return Ok(None);
        }
        let caller_scope = self.current_function_type_params();
        let mut needs_adapter = false;
        for (index, arg) in args.iter().enumerate() {
            if self
                .mut_list_adapter_arg(function, index, arg, target_tys[index], &callee_generics)?
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
        // Set when an argument monomorphizes the callee's generics, recording the
        // callee parameter type it instantiated and the concrete type it bound;
        // the return conversion needs both.
        let mut monomorphized_arg: Option<(TypeId, TypeId)> = None;
        for (index, arg) in args.iter().enumerate() {
            let target_ty = target_tys[index];
            if let Some((place, kind)) =
                self.mut_list_adapter_arg(function, index, arg, target_ty, &callee_generics)?
            {
                let access = self.mut_list_place_access(&place)?;
                let place_ty = self.place_ty(&place)?;
                let temp = format!("smelt_mut_arg_{index}");
                match kind {
                    MutListArgKind::Monomorphized => {
                        // The callee's parameter is its own generic list shape, so
                        // Rust binds the callee's type parameter from the caller's
                        // list and no element conversion happens in either
                        // direction. Borrow the place itself where that is safe —
                        // the callee then mutates the caller's list directly, with
                        // no copy and no write-back. A shared capture cannot hold
                        // its borrow guard across the call, so it goes through a
                        // temporary at the caller's own list type instead.
                        if let Some(borrow) = access.mut_borrow {
                            rendered_args.push(format!("&mut {borrow}"));
                        } else {
                            let temp_ty = self.rust_type(
                                place_ty,
                                false,
                                &TypeSubstitution::lexical(&caller_scope),
                            )?;
                            prelude.push_str(&format!(
                                "let mut {temp}: {temp_ty} = {}; ",
                                access.read
                            ));
                            rendered_args.push(format!("&mut {temp}"));
                            writebacks
                                .push_str(&format!("{} = {temp}; ", access.assign_target));
                        }
                        monomorphized_arg = Some((target_ty, place_ty));
                    }
                    MutListArgKind::Erased => {
                        let arg_item = self.list_element_ty(place_ty)?;
                        let caller_elem = self.rust_type(
                            arg_item,
                            false,
                            &TypeSubstitution::lexical(&caller_scope),
                        )?;
                        // The callee renders its erased element as `SmeltUnknown`;
                        // build the temporary at the callee's rendered parameter
                        // type so the `&mut` reborrow is type-correct.
                        // Rendered in the CALLEE's emission scope, which is
                        // empty here precisely because the callee erased this
                        // element. The emptiness is the callee's, not the
                        // caller's; naming it keeps that deliberate.
                        let temp_ty =
                            self.rust_type(target_ty, false, &TypeSubstitution::erased())?;
                        prelude.push_str(&format!(
                            "let mut {temp}: {temp_ty} = {}.into_iter().map(IntoSmeltUnknown::into_smelt_unknown).collect::<{temp_ty}>(); ",
                            access.read
                        ));
                        rendered_args.push(format!("&mut {temp}"));
                        writebacks.push_str(&format!(
                            "{} = {temp}.into_iter().map(|smelt_element| <{caller_elem} as SmeltFromUnknown>::smelt_from_unknown(smelt_element)).collect::<SmeltList<_>>(); ",
                            access.assign_target
                        ));
                    }
                }
            } else if matches!(self.mir.types.get(target_ty), Some(Type::Function(_))) {
                // No call-site bindings on this path: the mutable-list adapter
                // runs its OWN unifier (`mut_list_adapter_arg`) rather than
                // `static_call_monomorphization`. Two unifiers deciding one call
                // site is the §4.1 hazard, so this path never guesses a
                // substitution. The guard above makes that safe rather than
                // merely hopeful: it already declined the adapter for any callee
                // whose callback parameter mentions a type parameter the callee
                // emits, so the callback reaching here has a declared type with
                // nothing to substitute, and the caller's own lexical scope is
                // the right (and unchanged) environment to render it in.
                rendered_args.push(self.borrowed_function_argument_text(arg, target_ty, None)?);
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
        let dest_is_list = matches!(self.mir.types.get(dest_ty), Some(Type::List(_)));
        let result_expr = match monomorphized_arg {
            // The callee's declared return type is literally the parameter type
            // this argument instantiated (`pullAt<T>(arr: T[]): T[]`), so at this
            // call site the returned value already has the argument's own list
            // type. Convert from that concrete type, not from the unbound generic
            // (which renders as `SmeltUnknown` and would emit a bogus extraction
            // against concrete elements). A non-list destination — a discarded
            // (void) call — has nothing to convert to.
            Some((param_ty, arg_ty)) if return_ty == param_ty && dest_is_list => {
                self.value_at_type_text("smelt_mut_call_result", arg_ty, dest_ty)?
            }
            // Any other return position that mentions the callee's generics is
            // likewise monomorphized here, and the destination local already
            // carries the substituted type from the frontend (the same reasoning
            // `call_emitted_source_ty` applies to type-parameter returns), so the
            // value passes through unchanged.
            Some(_) if self.type_mentions_type_params(return_ty, &callee_generics) => {
                "smelt_mut_call_result".to_owned()
            }
            // A fully concrete return position from a generic callee needs the
            // ordinary destination coercion.
            Some(_) => self.value_at_type_text("smelt_mut_call_result", return_ty, dest_ty)?,
            None => self.mut_list_adapter_return_text(
                "smelt_mut_call_result",
                return_ty,
                dest_ty,
                &caller_scope,
            )?,
        };
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
        // Deliberately two different environments in one comparison: the
        // callee's rendering (erased, hence the empty substitution) against the
        // caller's lexical one. Rendering both sides the same way would make the
        // equality below always hold and silently drop the un-erasure.
        let return_render = self
            .rust_type(return_ty, false, &TypeSubstitution::erased())?
            .into_string();
        let dest_render = self
            .rust_type(dest_ty, false, &TypeSubstitution::lexical(caller_scope))?
            .into_string();
        if return_render == dest_render {
            return Ok(value_text.to_owned());
        }
        let dest_elem =
            self.rust_type(*dest_item, false, &TypeSubstitution::lexical(caller_scope))?;
        Ok(format!(
            "{value_text}.into_iter().map(|smelt_element| <{dest_elem} as SmeltFromUnknown>::smelt_from_unknown(smelt_element)).collect::<SmeltList<_>>()"
        ))
    }

    /// Returns the place of the `index`-th argument, and how its elements must be
    /// treated, when it needs the convert-in-place mutable-list adapter.
    ///
    /// The adapter applies when the callee needs a mutable reference to a list
    /// parameter, the argument is a writable local list place, and the caller
    /// renders the list element type differently from the callee. Because `&mut`
    /// references are invariant such a call cannot be emitted as a plain
    /// reborrow, and emitting it as `&mut <converted temporary>` would silently
    /// discard the callee's mutation.
    ///
    /// The two rendered-difference shapes need opposite treatment, so the result
    /// carries a [`MutListArgKind`]: a callee that emits real Rust generics binds
    /// its type parameter from the argument and must see the elements unchanged
    /// (`Monomorphized`), while an erased monomorphization needs each element
    /// converted in and back out (`Erased`).
    fn mut_list_adapter_arg(
        &self,
        function: &MirFunction,
        index: usize,
        arg: &Operand,
        target_ty: TypeId,
        callee_generics: &HashSet<Symbol>,
    ) -> Result<Option<(Place, MutListArgKind)>, EmitError> {
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
        let place = Place::Local(local);
        if !self.mut_list_place_is_writable(local)? {
            return Ok(None);
        }
        let arg_ty = self.place_ty(&place)?;
        let (Some(Type::List(arg_item)), Some(Type::List(target_item))) =
            (self.mir.types.get(arg_ty), self.mir.types.get(target_ty))
        else {
            return Ok(None);
        };
        // Compare rendered element types: the MIR element types can be identical
        // (a shared `TypeParam`) while the caller keeps it generic and the callee
        // erases it. The callee renders its element with generics only when it
        // emits real Rust generics, which is what `callee_generics` encodes.
        let caller_scope = self.current_function_type_params();
        let arg_element_text = self
            .rust_type(*arg_item, false, &TypeSubstitution::lexical(&caller_scope))?
            .into_string();
        let param_element_text = self
            .rust_type(
                *target_item,
                false,
                &TypeSubstitution::callee_emission(callee_generics),
            )?
            .into_string();
        if arg_element_text == param_element_text {
            // Already invariance-compatible: the ordinary call path passes the
            // place (or the forwarded reference) straight through.
            return Ok(None);
        }
        // The callee emits real generics and this argument is exactly its generic
        // list shape instantiated, so Rust infers the type parameter from the
        // caller's concrete list and no element conversion is possible *or*
        // wanted. This reuses the same instantiation rule the ordinary
        // pass-through path uses for by-value generic arguments.
        if !callee_generics.is_empty()
            && self.generic_param_instantiated_by(arg_ty, target_ty, callee_generics)
        {
            return Ok(Some((place, MutListArgKind::Monomorphized)));
        }
        // The erasing adapter converts elements with `IntoSmeltUnknown` on the way
        // in and `SmeltFromUnknown` on the way out, so it can only target a
        // parameter whose element really is the erased `SmeltUnknown`. Any other
        // rendered difference (a callee element that is itself a narrower generated
        // type, such as `SmeltErasedFunction`) has no such element bridge; leave
        // those on the ordinary call path rather than emit conversions that cannot
        // type-check.
        if param_element_text != "SmeltUnknown" {
            return Ok(None);
        }
        Ok(Some((place, MutListArgKind::Erased)))
    }

    /// Returns how a mutable-list argument's local place is stored in Rust.
    ///
    /// See [`MutListPlaceStorage`]; the classification drives both the writable
    /// check and the read/assign spellings.
    fn mut_list_place_storage(&self, local: LocalId) -> Result<MutListPlaceStorage, EmitError> {
        if self.function.id.0 != u32::MAX
            && self.function.params.contains(&local)
            && self.parameter_needs_mutable_reference(local)
        {
            return Ok(MutListPlaceStorage::Reference);
        }
        if shared_capture_cell_name(&self.place_text(&Place::Local(local))?).is_some() {
            return Ok(MutListPlaceStorage::SharedCapture);
        }
        Ok(MutListPlaceStorage::Owned)
    }

    /// Returns whether the adapter may write a new list back through `local`.
    ///
    /// A `&mut` parameter and a shared closure capture are always writable (the
    /// first through its reference, the second through the cell's `borrow_mut()`,
    /// neither of which needs a `mut` binding). An owned local is only writable
    /// when its `let` is actually rendered `mut`; `local_binding_needs_mut` is the
    /// single source of truth for that, and it already forces `mut` for any local
    /// passed to a mutable-reference parameter of a static call — exactly this
    /// shape — so the check is a self-consistency guard rather than a
    /// restriction. Inside a closure body the emitter runs on a synthetic
    /// function whose mutability analysis does not cover the enclosing binding,
    /// so only the shared-capture storage is accepted there.
    fn mut_list_place_is_writable(&self, local: LocalId) -> Result<bool, EmitError> {
        match self.mut_list_place_storage(local)? {
            MutListPlaceStorage::Reference | MutListPlaceStorage::SharedCapture => Ok(true),
            MutListPlaceStorage::Owned => {
                Ok(self.function.id.0 != u32::MAX && self.local_binding_needs_mut(local))
            }
        }
    }

    /// Returns the read expression and assignment target for a mutable-list
    /// argument place.
    ///
    /// `place_text` renders a read whose shape depends on the storage, so the
    /// adapter cannot assume a reference: `(*array).clone()` on an owned
    /// `SmeltList` would deref to a slice and clone that instead of the list, and
    /// `*array = ..` does not compile on a non-reference at all.
    fn mut_list_place_access(&self, place: &Place) -> Result<MutListPlaceAccess, EmitError> {
        let Place::Local(local) = place else {
            return Err(EmitError::new(
                "mutable-list adapter place must be a local",
            ));
        };
        let text = self.place_text(place)?;
        match self.mut_list_place_storage(*local)? {
            MutListPlaceStorage::Reference => Ok(MutListPlaceAccess {
                read: format!("(*{text}).clone()"),
                assign_target: format!("*{text}"),
                mut_borrow: Some(format!("*{text}")),
            }),
            MutListPlaceStorage::SharedCapture => {
                let cell = shared_capture_cell_name(&text).ok_or_else(|| {
                    EmitError::new("shared-capture place lost its cell binding")
                })?;
                Ok(MutListPlaceAccess {
                    // `text` is already `(*smelt_capture_x.borrow())`, so the read
                    // clones through the shared borrow; the write needs its own
                    // short-lived `borrow_mut()` of the same cell.
                    read: format!("{text}.clone()"),
                    assign_target: format!("*{cell}.borrow_mut()"),
                    mut_borrow: None,
                })
            }
            MutListPlaceStorage::Owned => Ok(MutListPlaceAccess {
                read: format!("{text}.clone()"),
                assign_target: text.clone(),
                mut_borrow: Some(text),
            }),
        }
    }

    /// Returns whether `ty` mentions any of `params` anywhere in its structure.
    ///
    /// Used to decide whether a callee's return position is monomorphized at a
    /// call site. The walk is structural and bounded by `depth` so a
    /// pathologically nested type cannot recurse without end.
    fn type_mentions_type_params(&self, ty: TypeId, params: &HashSet<Symbol>) -> bool {
        self.type_mentions_type_params_at_depth(ty, params, 16)
    }

    /// Depth-limited worker for [`Self::type_mentions_type_params`].
    fn type_mentions_type_params_at_depth(
        &self,
        ty: TypeId,
        params: &HashSet<Symbol>,
        depth: usize,
    ) -> bool {
        if depth == 0 || params.is_empty() {
            return false;
        }
        let next = depth.saturating_sub(1);
        match self.mir.types.get(ty) {
            Some(Type::TypeParam { name }) => params.contains(name),
            Some(
                Type::List(item)
                | Type::Set(item)
                | Type::Optional(item)
                | Type::Future(item),
            ) => self.type_mentions_type_params_at_depth(*item, params, next),
            Some(Type::Dict(key, value)) => {
                self.type_mentions_type_params_at_depth(*key, params, next)
                    || self.type_mentions_type_params_at_depth(*value, params, next)
            }
            Some(Type::Tuple(items) | Type::Union(items)) => items
                .iter()
                .any(|item| self.type_mentions_type_params_at_depth(*item, params, next)),
            Some(Type::Function(function)) => {
                self.type_mentions_type_params_at_depth(function.return_ty, params, next)
                    || function
                        .params
                        .iter()
                        .any(|param| {
                            self.type_mentions_type_params_at_depth(*param, params, next)
                        })
            }
            _ => false,
        }
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
        // Seam 2 (`emitter::seam_assertions`): the argument half of this call
        // site is rendered inside `call_text` and the return half inside
        // `call_emitted_source_ty`, each recomputing the monomorphization
        // decision for itself. Sample it on both sides of the argument
        // rendering and compare.
        #[cfg(debug_assertions)]
        let monomorphization_before_arguments = self.sampled_call_monomorphization(callee, args);
        let mut call_text = self.call_text(callee, args)?;
        if args.is_empty() && call_text.ends_with("(Vec::new())") {
            call_text = format!("{}()", call_text.trim_end_matches("(Vec::new())"));
        } else if call_text == "(fn_)(Vec::new())" {
            "(fn_)()".clone_into(&mut call_text);
        }
        #[cfg(debug_assertions)]
        self.debug_assert_call_monomorphization_stable(
            &self.sampled_callee_name(callee),
            monomorphization_before_arguments.as_ref(),
            self.sampled_call_monomorphization(callee, args).as_ref(),
        );
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
                // The wrapper is assigned to `dest_ty`'s `dyn Fn`, so its
                // parameters must carry whatever `&` that spelling does
                // (`callback_param_is_shared_reference`); rendering the bare
                // types here made the two disagree (E0631).
                let scope = self.current_function_type_params();
                let substitution = TypeSubstitution::lexical(&scope);
                let params = self
                    .callback_arg_decls(target_function, &substitution, MutablePrefix::Apply)?
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
        call_text = self.wrap_native_async_call_text(callee, call_text)?;
        let source_ty = self.call_emitted_source_ty(callee, args, dest_ty)?;
        self.value_at_type_text(&call_text, source_ty, dest_ty)
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
    fn wrap_native_async_call_text(
        &self,
        callee: &Callee,
        call_text: String,
    ) -> Result<String, EmitError> {
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
            Ok(format!(
                "SmeltFuture::from_future(Box::pin({call_text}))"
            ))
        } else {
            Ok(call_text)
        }
    }

    /// Returns the Rust type a call *actually* evaluates to in the emitted code.
    ///
    /// Unlike [`Self::call_source_ty`] (which returns the callee's declared MIR
    /// return type), this resolves the type the generated call expression really
    /// produces, accounting for erasure: async functions yield
    /// `Future<return_ty>`, functions whose emitted signature erased a return
    /// type parameter yield the emitted return type, and monomorphized generic
    /// returns yield the callee's declared return with the call site's bindings
    /// applied. Callers coerce the call value from this type to the destination,
    /// so it must match the emitted signature, not the frontend's specialized
    /// view of the call site.
    ///
    /// `args` is needed because a generic callee's real Rust return type is a
    /// property of the *call*, not of the callee: it is whatever the arguments
    /// pinned its type parameters to.
    pub(super) fn call_emitted_source_ty(
        &self,
        callee: &Callee,
        args: &[Operand],
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
                } else if let Some(method_return_ty) =
                    self.class_method_substituted_return_ty(function, args)?
                {
                    // A method of a generic class whose declared return mentions
                    // a class type parameter (`get(): T`, `all(): T[]`) returns
                    // the receiver's concrete instantiation at this call site:
                    // `Box<f64>::all` really evaluates to `SmeltList<f64>`. The
                    // receiver pins the class arguments, so the substituted
                    // return is the honest source type and no `SmeltUnknown`
                    // extraction is emitted against an already-concrete value.
                    method_return_ty
                } else if let Some(monomorphization) =
                    self.monomorphized_free_function_return(function, args)?
                {
                    // A generic free function monomorphized at this call site
                    // (`identity<T>(x: T): T`, `at<T>(xs: T[], ..): T[]`) really
                    // evaluates to its declared return with the call site's
                    // bindings applied. The same decision licensed passing the
                    // arguments through concretely, so the two sides cannot
                    // disagree.
                    if function.is_async && !function.is_generator {
                        self.type_id(Type::Future(monomorphization))?
                    } else {
                        monomorphization
                    }
                } else if function.is_async && !function.is_generator {
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

    /// Return a generic class method's return type with the receiver's class
    /// arguments substituted in, when this call really instantiates them.
    ///
    /// `None` — meaning "keep the ordinary erased path" — when the callee is not
    /// a method of a generic class, when its return mentions no class type
    /// parameter (there is nothing to substitute, and the emitted signature is
    /// already the answer), when the receiver does not pin every class type
    /// parameter concretely, or when the substituted type is not interned.
    fn class_method_substituted_return_ty(
        &self,
        function: &MirFunction,
        args: &[Operand],
    ) -> Result<Option<TypeId>, EmitError> {
        if !matches!(function.origin, HirOrigin::ClassMethod { .. }) {
            return Ok(None);
        }
        let class_params = self.callee_class_type_params(function);
        if class_params.is_empty() || !self.type_mentions_any(function.return_ty, &class_params) {
            return Ok(None);
        }
        // The receiver is `args[0]` for every emitted method call; see the
        // `ClassMethod` arm of `call_text`, which splits it off first.
        let Some(receiver) = args.first() else {
            return Ok(None);
        };
        let ordered = self.callee_class_type_param_names(function);
        let bindings = bind_class_type_params(self.mir, &ordered, self.operand_ty(receiver)?);
        if !bindings.all_concrete() {
            return Ok(None);
        }
        Ok(substituted_type_id(self.mir, function.return_ty, &bindings))
    }

    /// Return a monomorphized generic free function's substituted return type.
    ///
    /// `None` when this call site does not monomorphize the callee, and also
    /// when the callee's return mentions none of its own type parameters: there
    /// the emitted signature already states the return type and the ordinary
    /// ladder below is the right answer, so the argument-side decision must not
    /// perturb it.
    fn monomorphized_free_function_return(
        &self,
        function: &MirFunction,
        args: &[Operand],
    ) -> Result<Option<TypeId>, EmitError> {
        let own_params = self.callee_free_function_type_params(function);
        if own_params.is_empty() || !self.type_mentions_any(function.return_ty, &own_params) {
            return Ok(None);
        }
        Ok(self
            .static_call_monomorphization(function, args)?
            .map(|monomorphization| monomorphization.return_ty))
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
        // A closure body that is itself fallible returns
        // `Result<_, Box<dyn std::error::Error>>`, so a throwing call inside it
        // keeps its `?` and the error stays recoverable for whoever invokes the
        // callback. Only a closure whose Rust signature genuinely cannot carry
        // an error (a non-throwing body, or a generator whose `?` would target
        // the step output) collapses the error into a `panic!`.
        if let Some(stripped) = call_text.strip_suffix('?')
            && !self.body_can_propagate_error()
        {
            call_text = format!("{stripped}.unwrap_or_else(|error| smelt_panic_throw(error))");
        }
        call_text = self.wrap_native_async_call_text(callee, call_text)?;
        // The emitted return, not the declared one. A monomorphized generic
        // callee really evaluates to its declared return with this call site's
        // bindings applied (`shift<T>(SmeltList<T>) -> SmeltList<T>` called with
        // a `SmeltList<f64>` yields `SmeltList<f64>`), while the declared return
        // renders `T` as `SmeltUnknown` in the caller's scope. Asking for the
        // declared type here makes the coercion a no-op whenever the destination
        // is erased, so the concrete value is bound to an erased local and the
        // argument is what rustc reports as mismatched (E0308).
        let source_ty = self.call_emitted_source_ty(callee, args, dest_ty)?;
        self.value_at_type_text(&call_text, source_ty, dest_ty)
    }

    /// The monomorphization decision for a static callee, for seam checking.
    ///
    /// Debug-only. Returns `None` for a non-static callee and for any failure:
    /// a check may not change emission, including by failing differently from
    /// the emission path it observes.
    #[cfg(debug_assertions)]
    fn sampled_call_monomorphization(
        &self,
        callee: &Callee,
        args: &[Operand],
    ) -> Option<CallMonomorphization> {
        let Callee::Static(func) = callee else {
            return None;
        };
        let function = self
            .mir
            .functions
            .get(id_index(func.0, "function index does not fit usize").ok()?)?;
        self.static_call_monomorphization(function, args).ok()?
    }

    /// A callee's Rust name for a seam-assertion message.
    #[cfg(debug_assertions)]
    fn sampled_callee_name(&self, callee: &Callee) -> String {
        let Callee::Static(func) = callee else {
            return "<indirect callee>".to_owned();
        };
        id_index(func.0, "function index does not fit usize")
            .ok()
            .and_then(|index| self.mir.functions.get(index))
            .and_then(|function| self.function_rust_name(function).ok())
            .unwrap_or_else(|| "<unnameable callee>".to_owned())
    }

    /// Returns whether a callee's type carries no static signature, so its call
    /// must go through the run-time callable dispatch.
    ///
    /// Mirrors the `Rvalue::ClosureCall` erased-callee test so both call forms
    /// classify a callee identically.
    pub(super) fn callee_is_dynamically_dispatched(&self, callee_ty: TypeId) -> bool {
        matches!(
            self.mir.types.get(callee_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) || self.is_erased_class_type(callee_ty)
    }

    /// Returns the static return type of a call expression.
    pub(super) fn call_source_ty(&self, callee: &Callee) -> Result<TypeId, EmitError> {
        let source_ty = match callee {
            Callee::Builtin(
                BuiltinFn::ConsoleLog { .. } | BuiltinFn::ConsoleWrite | BuiltinFn::ConsoleErrorWrite,
            ) => self.none_ty,
            // `JSON.parse` yields a dynamic JavaScript value; the destination's
            // own type drives the ordinary coercion from the erased carrier.
            Callee::Builtin(BuiltinFn::JsonParse) => return self.type_id(Type::Unknown),
            // A decoder answers a `String`, not an erased value -- the whole
            // point of keeping the typed runtime helper behind the adapter.
            Callee::Builtin(BuiltinFn::UriDecode(_)) => return self.type_id(Type::String),
            Callee::Static(func) => {
                let function = self
                    .mir
                    .functions
                    .get(id_index(func.0, "function index does not fit usize")?)
                    .ok_or_else(|| EmitError::new("call references an unknown function"))?;
                function.return_ty
            }
            Callee::Indirect(indirect_callee) => {
                let callee_ty = self.operand_ty(indirect_callee)?;
                // A dynamically dispatched callee has no static signature, so
                // its call produces the erased carrier (see the matching arm in
                // `indirect` call emission).
                if self.callee_is_dynamically_dispatched(callee_ty) {
                    return self.type_id(Type::Unknown);
                }
                let Some(Type::Function(function)) = self.mir.types.get(callee_ty) else {
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
        absent: AbsentSpelling,
    ) -> Result<(&'static str, String), EmitError> {
        let ty = self.operand_ty(operand)?;
        if ty == self.none_ty {
            Ok(("{}", "\"null\"".to_owned()))
        } else if matches!(
            self.mir.types.get(ty),
            Some(Type::List(_) | Type::Dict(_, _) | Type::Tuple(_))
        ) {
            Ok(("{:?}", self.operand_text(operand)?))
        } else if matches!(self.mir.types.get(ty), Some(Type::Optional(_))) {
            // An `Optional<T>` prints the value INSIDE it, never the Rust
            // wrapper. `{:?}` on an `Option` put `Some("ada")` / `None` into
            // program output, which is a shape no JavaScript runtime prints.
            Ok(("{}", self.console_optional_text(operand, absent)?))
        } else if matches!(
            self.mir.types.get(ty),
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
            Ok(("{}", self.erase_value_text(&self.operand_text(operand)?, ty)?))
        }
    }

    /// Render an `Optional<T>` console argument as a `String` expression.
    ///
    /// The present arm renders the inner value the way `console.log` renders
    /// that type on its own, so the wrapper is invisible: a `string | undefined`
    /// holding `"ada"` prints `ada`, and a `number[] | undefined` holding
    /// `[1, 2]` prints through the container's `{:?}`.
    ///
    /// # Why the absent arm prints `undefined`
    ///
    /// TypeScript's `null` and `undefined` both intern to `Type::None` (see the
    /// annotation lowering for `TSNullKeyword`/`TSUndefinedKeyword`), so
    /// `string | null` and `string | undefined` are the *same*
    /// `Optional(String)` here. Node prints `null` for the first and
    /// `undefined` for the second, and this layer cannot tell them apart, so
    /// one word has to be chosen and it is wrong for the other spelling.
    ///
    /// `undefined` is chosen because it is what nearly every operation that
    /// *produces* an `Optional` in TypeScript returns: `find`, `pop`,
    /// `Map.get`, an optional property or parameter, an index read, `?.`, and
    /// `process.env.X`. `null` arrives mostly from annotations spelled `T |
    /// null` (and from `headers.get`), and a value annotated as plain `null`
    /// keeps printing `null` through the `Type::None` branch above. The
    /// end-to-end fixture `33_console_optional_value` pins this against Node
    /// 22, whose `find()` miss prints `undefined`.
    ///
    /// Printing the right word for both spellings needs a distinct
    /// `Type::Undefined` carried down from the annotation — a type-table
    /// change, not a console change.
    fn console_optional_text(
        &self,
        operand: &Operand,
        absent: AbsentSpelling,
    ) -> Result<String, EmitError> {
        let ty = self.operand_ty(operand)?;
        let Some(&Type::Optional(inner)) = self.mir.types.get(ty) else {
            return Err(EmitError::new(
                "console optional rendering requires an optional operand",
            ));
        };
        let present = self.console_value_text("value", inner, absent)?;
        Ok(format!(
            "match &{} {{ Some(value) => {present}, None => {:?}.to_owned() }}",
            self.operand_text(operand)?,
            absent.text()
        ))
    }

    /// Render `value_text` of type `ty` as a `String` expression, console-style.
    ///
    /// The same three cases as [`Self::console_arg_text`], but producing an
    /// owned `String` rather than a format-spec pair, so it can be used inside a
    /// match arm. `value_text` names a *reference* to the value.
    fn console_value_text(
        &self,
        value_text: &str,
        ty: TypeId,
        absent: AbsentSpelling,
    ) -> Result<String, EmitError> {
        if ty == self.none_ty {
            return Ok("\"null\".to_owned()".to_owned());
        }
        if matches!(
            self.mir.types.get(ty),
            Some(Type::List(_) | Type::Dict(_, _) | Type::Tuple(_))
        ) {
            return Ok(format!("format!(\"{{:?}}\", {value_text})"));
        }
        if matches!(
            self.mir.types.get(ty),
            Some(Type::Bool | Type::Int | Type::Float | Type::String | Type::Unknown)
        ) {
            return Ok(format!("format!(\"{{}}\", {value_text})"));
        }
        // A nested optional recurses, so `Array<string | undefined>`'s element
        // prints its own inner value rather than a wrapper.
        if let Some(&Type::Optional(inner)) = self.mir.types.get(ty) {
            let present = self.console_value_text(value_text, inner, absent)?;
            return Ok(format!(
                "match {value_text} {{ Some(value) => {present}, None => {:?}.to_owned() }}",
                absent.text()
            ));
        }
        // Every remaining non-`Display` type takes the same erasure route the
        // top-level argument takes.
        let erased = self.erase_value_text(&format!("{value_text}.clone()"), ty)?;
        Ok(format!("format!(\"{{}}\", {erased})"))
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
        // A host constructor this crate REASSIGNS (`globalThis.File = class File
        // extends Blob {}`) lives in an override slot, and `instanceof` reads the
        // binding — so the check has to read the slot too. The static marker
        // probe below answers for the native builtin only, which made
        // `new File(...) instanceof File` false for exactly the override the
        // crate had just installed. Every other spelling of "is the global
        // present / what does it construct" already goes through the slot.
        if let Some(check) = self.host_override_instance_of_text(value, value_ty, class_name)? {
            return Ok(check);
        }
        // One list of builtin error classes, shared with the erasure that stamps
        // `__smelt_error` for a user class whose base chain reaches one
        // (`host_base_markers`), so the probe and the marker cannot disagree.
        if smelt_stdlib::is_error_class_name(class_name)
            && matches!(
            self.mir.types.get(value_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
        ) {
            let value_text = self.operand_text(value)?;
            // `Error` is the base class, so ANY error marker satisfies it. A
            // subclass must match the class name the marker records — every error
            // class used to share one boolean marker, so `new Error('x') instanceof
            // AggregateError` answered `true`. es-toolkit `clone` branches on
            // exactly that and rebuilt a plain Error as
            // `new Ctor(obj.errors, obj.message, ..)`, putting `errors` in the
            // message slot and dropping the message.
            //
            // This models the one level of the built-in hierarchy that exists:
            // every built-in error derives directly from `Error`, so a subclass
            // check is an equality test on the recorded name. A USER class
            // `extends Error` resolves through the class path while it is still
            // typed; once it has crossed an erasure seam into a `SmeltUnknown`
            // (which is exactly what a predicate like `isError(value: unknown)`
            // does) only the markers survive, so its erasure stamps
            // `__smelt_error` with the nearest builtin error base's name — see
            // `host_base_markers` — and answers these same probes.
            let marker_probe = if class_name == "Error" {
                "value.contains_key(\"__smelt_error\")".to_owned()
            } else {
                format!(
                    "matches!(value.get(\"__smelt_error\"), Some(SmeltUnknown::String(smelt_error_class)) if &*smelt_error_class == {class_name:?})"
                )
            };
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(format!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if {marker_probe})"
                ));
            }
            return Ok(format!(
                "matches!({value_text}.clone(), SmeltUnknown::Object(value) if {marker_probe})"
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
        // A concrete `SmeltRegExp` answers `instanceof RegExp` through the typed
        // path; an erased one recovers its identity from the `__smelt_regexp`
        // marker its erasure stamps, exactly like the Date arm above. Without this
        // arm the check was `false` for any `unknown`-typed regex, so es-toolkit
        // `cloneDeepWithImpl` skipped its `valueToClone instanceof RegExp` branch
        // and fell through to the generic `Object.create(getPrototypeOf(x))` path,
        // which produced an object with no `source`/`flags` at all.
        if class_name == "RegExp"
            && matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            )
        {
            let value_text = self.operand_text(value)?;
            if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                return Ok(format!(
                    "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"__smelt_regexp\"))"
                ));
            }
            return Ok(format!(
                "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_regexp\"))"
            ));
        }
        if class_name == "Map" {
            // A concrete source `Map` (`JsMap`) is unconditionally `instanceof
            // Map`. An erased operand recovers Map identity through the
            // `__smelt_map` marker its erasure stamps.
            if matches!(self.mir.types.get(value_ty), Some(Type::JsMap(_, _))) {
                return Ok("true".to_owned());
            }
            if matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            ) {
                let value_text = self.operand_text(value)?;
                if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                    return Ok(format!(
                        "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"__smelt_map\"))"
                    ));
                }
                return Ok(format!(
                    "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_map\"))"
                ));
            }
            // Any other concrete operand carries no Map identity.
            return Ok("false".to_owned());
        }
        if class_name == "Set" {
            // A concrete source `Set` is unconditionally `instanceof Set`. An
            // erased operand recovers Set identity through the `__smelt_set`
            // marker its erasure stamps. Mirrors the `Map` arm above.
            if matches!(self.mir.types.get(value_ty), Some(Type::Set(_))) {
                return Ok("true".to_owned());
            }
            if matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            ) {
                let value_text = self.operand_text(value)?;
                if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                    return Ok(format!(
                        "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if value.contains_key(\"__smelt_set\"))"
                    ));
                }
                return Ok(format!(
                    "matches!({value_text}.clone(), SmeltUnknown::Object(value) if value.contains_key(\"__smelt_set\"))"
                ));
            }
            // Any other concrete operand carries no Set identity.
            return Ok("false".to_owned());
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
            "AbortController" => Some(vec!["__smelt_abortcontroller"]),
            "AbortSignal" => Some(vec!["__smelt_abortsignal"]),
            _ => host_instance_markers(class_name),
        };
        if let Some(markers) = abort_marker {
            let value_is_dynamic = matches!(
                self.mir.types.get(value_ty),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
            );
            if value_is_dynamic || self.is_erased_class_type(value_ty) {
                let value_text = self.operand_text(value)?;
                let probe = markers
                    .iter()
                    .map(|marker| format!("value.contains_key(\"{marker}\")"))
                    .collect::<Vec<_>>()
                    .join(" || ");
                if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
                    return Ok(format!(
                        "matches!({value_text}.clone(), Some(SmeltUnknown::Object(value)) if {probe})"
                    ));
                }
                return Ok(format!(
                    "matches!({value_text}.clone(), SmeltUnknown::Object(value) if {probe})"
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

    /// Slot-aware `instanceof` for a host name this crate reassigns.
    ///
    /// Returns `None` when the class has no override slot in this crate, no
    /// identity marker to recognize its native records by, or a statically
    /// concrete operand that the ordinary class path answers — leaving every
    /// other case exactly as it was.
    fn host_override_instance_of_text(
        &self,
        value: &Operand,
        value_ty: TypeId,
        class_name: &str,
    ) -> Result<Option<String>, EmitError> {
        if !crate::stdlib::host_override_slot_names(self.mir)
            .iter()
            .any(|(_, name)| name == class_name)
        {
            return Ok(None);
        }
        let Some(markers) = host_instance_markers(class_name) else {
            return Ok(None);
        };
        let value_is_dynamic = matches!(
            self.mir.types.get(value_ty),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_) | Type::Optional(_))
        );
        if !(value_is_dynamic || self.is_erased_class_type(value_ty)) {
            return Ok(None);
        }
        let value_text = self.operand_text(value)?;
        let probed = if matches!(self.mir.types.get(value_ty), Some(Type::Optional(_))) {
            format!("{value_text}.clone().unwrap_or(SmeltUnknown::Undefined)")
        } else {
            format!("{value_text}.clone()")
        };
        let marker_list = markers
            .iter()
            .map(|marker| format!("\"{marker}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let slot = format!(
            "{prefix}{suffix}",
            prefix = smelt_stdlib::runtime_symbols::host_override::SLOT_PREFIX,
            suffix = crate::stdlib::host_override_slot_suffix(class_name),
        );
        Ok(Some(format!(
            "{slot}.with(|smelt_slot| smelt_host_override_instance_of(smelt_slot, &{probed}, &[{marker_list}]))"
        )))
    }

    /// Return whether `source` is `target` or derives from it.
    ///
    /// Walks the declared base chain, which is how a statically typed
    /// `instanceof` between two generated classes is answered.
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
