//! Function-shape adapters, behind the coercion seam: a callable of one
//! signature wrapped to answer another, including rest-vector and erased forms.

use super::*;

impl FunctionEmitter<'_> {
    /// The body of a callback adapter around `call_value`, once its value has
    /// been converted to `converted_text`: what the target's return type asks of
    /// a void, future or fallible result.
    ///
    /// `forward_thrown_future` hands a fallible source's future straight through
    /// to a fallible future target, for an adapter whose call value is that future.
    #[expect(clippy::too_many_arguments, reason = "the adapter's own facts, named")]
    fn adapter_return_expr(
        &self,
        source: &FunctionType,
        target_function: &FunctionType,
        target_return_ty: TypeId,
        call_value: &str,
        default_adjusted_return_text: String,
        adapter_source: AdapterSource,
        forward_thrown_future: bool,
    ) -> Result<RustExpr, EmitError> {
        let AdapterSource {
            returns_future: source_returns_future,
            output_may_throw: source_async_output_may_throw,
        } = adapter_source;
        Ok(if self.mir.types.get(target_return_ty) == Some(&Type::None)
            && !source_returns_future
        {
            if target_function.may_throw {
                RustExpr::atom(format!("{{ {call_value}; Ok::<(), {ERROR_BOX}>(()) }}"))
            } else {
                RustExpr::atom(format!("{{ {call_value}; () }}"))
            }
        } else if self.mir.types.get(target_return_ty) == Some(&Type::None)
            && source_returns_future
        {
            if target_function.may_throw {
                RustExpr::atom(format!(
                    "{{ {spawn_promise_task}(Box::pin(async move {{ let _ = {call_value}.await; }})); Ok::<(), {ERROR_BOX}>(()) }}",
                    spawn_promise_task = smelt_stdlib::runtime_symbols::timers::SPAWN_PROMISE_TASK,
                ))
            } else {
                RustExpr::atom(format!(
                    "{{ {spawn_promise_task}(Box::pin(async move {{ let _ = {call_value}.await; }})); () }}",
                    spawn_promise_task = smelt_stdlib::runtime_symbols::timers::SPAWN_PROMISE_TASK,
                ))
            }
        } else if target_function.may_throw
            && source_returns_future
            && !source_async_output_may_throw
            && let Some(Type::Future(item)) = self.mir.types.get(source.return_ty)
        {
            let item_text = self.rendered_type(*item)?;
            RustExpr::atom(format!(
                "SmeltFuture::from_future(Box::pin(async move {{ Ok::<{item_text}, {ERROR_BOX}>({default_adjusted_return_text}.await?) }}))"
            ))
        } else if target_function.may_throw
            && matches!(self.mir.types.get(target_return_ty), Some(Type::Future(_)))
            && source.return_ty != target_return_ty
        {
            RustExpr::from(default_adjusted_return_text)
        } else if target_function.may_throw
            && let Some(Type::Future(item)) = self.mir.types.get(target_return_ty)
            && !(source.may_throw && source_returns_future)
        {
            let item_text = self.rendered_type(*item)?;
            RustExpr::atom(format!(
                "SmeltFuture::from_future(Box::pin(async move {{ Ok::<{item_text}, {ERROR_BOX}>({default_adjusted_return_text}.await?) }}))"
            ))
        } else if forward_thrown_future
            && target_function.may_throw
            && matches!(
                self.mir.types.get(target_return_ty),
                Some(Type::Future(_))
            )
            && source.may_throw
            && source_returns_future
        {
            // A callback that returns a future renders its Rust type as
            // `-> SmeltFuture<..>` with NO outer `Result` — the possible throw is
            // carried inside the future's `Result` output (see the `Type::Function`
            // arm in the default-value emitter). So when both the source and the
            // fallible target return a future, forward the future value directly
            // rather than re-wrapping it in `Ok(..)`. Wrapping here would make the
            // adapter closure return `Result<Future, _>`, and a chain of such
            // adapters (or a later erasure into a promise) would then observe a
            // nested `Result<Result<Future, _>, _>` at its await seam (was E0277).
            RustExpr::from(default_adjusted_return_text)
        } else if target_function.may_throw {
            RustExpr::loose(format!("Ok::<_, {ERROR_BOX}>({default_adjusted_return_text})"))
        } else {
            RustExpr::from(default_adjusted_return_text)
        })
    }

    /// Returns whether a callback map value can be wrapped for a typed field.
    ///
    /// The wrapper may discard arguments, as TypeScript permits, but it cannot
    /// invent a different return value or convert incompatible argument types.
    pub(super) fn can_adapt_rendered_function_value(&self, source: TypeId, target: TypeId) -> bool {
        let (Some(Type::Function(source_function)), Some(Type::Function(target_function))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        else {
            return false;
        };
        // The adapter closure takes the *target* parameters and forwards them to
        // the source callback. The source may legally declare more parameters than
        // the target when the surplus are optional (their values are filled with
        // defaults at the forwarding site); this is what lets a `Promise` `resolve`
        // typed `(value?) => void` flow into a `() => void` slot. We still require
        // every target parameter the source actually consumes to be renderable into
        // the matching source parameter type.
        if source_function.is_async || target_function.is_async {
            return false;
        }
        // An erased-rest source is the `SmeltErasedFunction` callable ABI: the
        // adapter does not forward the target's arguments *into* the source's
        // declared rest-list parameter, it packs them into a
        // `Vec<SmeltUnknown>` and calls `.call(..)` (see the `packed` branch in
        // `rendered_function_shape_adapter`). So the per-parameter
        // renderability test below asks the wrong question — `f64` is not
        // renderable as `unknown[]` yet erases to `SmeltUnknown` perfectly
        // well. What the packed call really needs is a return value that can
        // come back out of `SmeltUnknown`, which every erased slot answers.
        // Without this an overloaded callable interface whose overloads differ
        // only by parameter *type* (`(v: string): string; (v: number): number;`)
        // failed to adapt its erased `__smelt_call` slot to the overload the
        // call site selected, emitting a raw field read and an E0308.
        if self.is_erased_unknown_rest_function(source_function) {
            let unknown_ty = self.mir.types.intern_late(Type::Unknown);
            return self
                .can_render_non_function_dict_value_as(unknown_ty, target_function.return_ty);
        }
        // A trailing rest parameter in the SOURCE collects every target
        // argument from its position on (`(...args) => ..` stored in a
        // fixed-arity slot), so each of those must render as the list's
        // element rather than as the list.
        if let Some((rest, element)) = self.trailing_rest_element(source_function) {
            return self.can_render_dict_value_as(
                source_function.return_ty,
                target_function.return_ty,
            ) && target_function.params.iter().enumerate().all(|(index, target_param)| {
                let into = if index < rest { source_function.params[index] } else { element };
                self.can_render_dict_value_as(*target_param, into)
            });
        }
        // A parameter or return that is itself a callable converts through
        // this same adapter, parameters contravariantly: `ts-pattern`'s
        // `otherwise: (handler: (value: i) => R) => R` read off a
        // `Match<unknown>` and stored in a `Match<number[]>` wraps `handler`
        // back into `(value: unknown) => R` before calling the stored slot.
        let shared = source_function.params.len().min(target_function.params.len());
        self.can_render_dict_value_as(
            source_function.return_ty,
            target_function.return_ty,
        ) && source_function
            .params
            .iter()
            .take(shared)
            .zip(target_function.params.iter().take(shared))
            .all(|(source_param, target_param)| {
                self.can_render_dict_value_as(*target_param, *source_param)
            })
    }

    /// The position and element type of a function's trailing rest parameter,
    /// when it has one whose type is a list.
    fn trailing_rest_element(&self, function: &FunctionType) -> Option<(usize, TypeId)> {
        let rest = function.rest?;
        if rest + 1 != function.params.len() {
            return None;
        }
        match self.mir.types.get(*function.params.get(rest)?) {
            Some(Type::List(element)) => Some((rest, *element)),
            _ => None,
        }
    }

    /// Adapt an extracted callback value to a compatible callback field shape.
    ///
    /// Structural record construction emits map values from rendered Rust text,
    /// rather than MIR operands. JavaScript permits object callback fields to
    /// omit arguments supplied by their consumer, so map-extracted callbacks
    /// require the same wrapper semantics as ordinary callback operands.
    pub(in crate::emitter) fn rendered_function_shape_adapter(
        &self,
        value_text: &RustExpr,
        source: TypeId,
        target: TypeId,
        scope: &RenderScope,
    ) -> Result<Option<RustExpr>, EmitError> {
        let (Some(Type::Function(source_function)), Some(Type::Function(target_function))) =
            (self.mir.types.get(source), self.mir.types.get(target))
        else {
            return Ok(None);
        };
        // `SmeltErasedFunction` already erases static return differences at the ABI.
        if self.is_erased_unknown_rest_function(source_function)
            && !source_function.may_throw
            && self.is_erased_unknown_rest_function(target_function)
            && !target_function.may_throw
        {
            return Ok(None);
        }
        if source == target
            || !self.can_adapt_rendered_function_value(source, target)
            || matches!(
                self.mir.types.get(source_function.return_ty),
                Some(Type::Future(_))
            )
            || matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Future(_))
            )
        {
            return Ok(None);
        }

        // Note this adapter disagrees with the other three shape adapters: it
        // renders the target's parameters in an explicitly empty environment
        // while its own binding type (below) uses the caller's lexical scope.
        // Preserved rather than reconciled — reconciling it moves bytes.
        let arg_decls = self.callback_arg_decls(
            target_function,
            &TypeSubstitution::erased(),
            MutablePrefix::Apply,
        )?;
        // Forward one argument per *source* parameter. Where the target supplies a
        // matching argument (`arg{index}`), coerce it into the source parameter
        // type; where the source declares more (optional) parameters than the
        // target provides, fill the surplus with the source parameter's default
        // value so the source callback is still called with full arity.
        let source_rest = self.trailing_rest_element(source_function);
        let forwarded = source_function
            .params
            .iter()
            .enumerate()
            .map(|(index, source_param)| self.rendered_adapter_argument(source_function, target_function, source_rest, index, *source_param, scope))
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        // An erased-rest source is a `SmeltErasedFunction` value, which is not a
        // Rust `Fn` and cannot be invoked with call syntax. Route it through the
        // erased callable ABI (`.call(..)`) instead of `(callback)(..)`.
        let source_uses_erased_abi = self.is_erased_unknown_rest_function(source_function);
        // `SmeltErasedFunction::call` answers `SmeltUnknown` whatever the source's
        // DECLARED return type is — `is_erased_unknown_rest_function` keys only on
        // the parameter shape — so the call's value is already erased and its
        // declared type must not be applied to it a second time. A rest-only
        // function with a concrete return (`function f(...): unknown[]`, which is
        // what an `arguments`-reading function becomes) otherwise produced
        // `SmeltUnknown::Array(<already SmeltUnknown>.into())`, an E0277 in the
        // generated crate.
        let call_return_ty = if source_uses_erased_abi {
            self.type_id(Type::Unknown)?
        } else {
            source_function.return_ty
        };
        let call = if source_uses_erased_abi {
            if self.is_erased_unknown_rest_function(target_function) {
                // The target is itself an erased-rest callable: its single
                // argument already *is* the packed argument list, so forward it
                // unchanged.
                RustExpr::chain("smelt_callback.call(arg0)")
            } else {
                // The target has fixed arity. Pack every positional argument
                // into the erased rest list. The per-source-parameter mapping in
                // `forwarded` would instead coerce `arg0` *into* the list —
                // spreading its elements and dropping `arg1..` — which panics on
                // non-iterable values and corrupts multi-argument adapters (see
                // `partial`/`partialRight` two-argument callbacks).
                //
                // A target's rest parameter holds zero or more arguments, so it
                // spreads into the vector rather than arriving as one array.
                let rest_slot = target_function.rest;
                let packed = target_function
                    .params
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| rest_slot != Some(*index))
                    .map(|(index, target_param)| {
                        // The adapter's own parameters are declared with the
                        // by-reference ABI (`function_type_param` renders
                        // `&T`/`&mut T` for a shared-reference or mutable slot),
                        // but the packed argument vector is `Vec<SmeltUnknown>` and
                        // owns its elements. Take an owned copy before erasing;
                        // without it a single reference-typed parameter fixed the
                        // vector's element type to `&SmeltUnknown` and every other
                        // element became an E0308.
                        let base = if target_function.mutable_params.contains(&index)
                            || self.callback_param_is_shared_reference(
                                target_function,
                                index,
                                *target_param,
                            ) {
                            RustExpr::chain(format!("arg{index}.clone()")).ending_in_clone()
                        } else {
                            RustExpr::path(format!("arg{index}"))
                        };
                        self.erase_rendered(&base, *target_param)
                    })
                    .collect::<Result<Vec<_>, EmitError>>()?
                    .join(", ");
                match self.rest_spread_expr(target_function)? {
                    Some(spread) => RustExpr::chain(format!(
                        "smelt_callback.call({{ let mut smelt_packed: Vec<SmeltUnknown> = vec![{packed}]; {spread} smelt_packed }})"
                    )),
                    None => RustExpr::chain(format!("smelt_callback.call(vec![{packed}])")),
                }
            }
        } else {
            RustExpr::atom(format!("(smelt_callback)({forwarded})"))
        };
        let call_value = if source_function.may_throw && target_function.may_throw {
            RustExpr::atom(format!("{call}?"))
        } else if source_function.may_throw {
            RustExpr::postfix_on(&call, format!("{call}.unwrap_or_else(|error| smelt_panic_throw(error))"))
        } else {
            call
        };
        let converted = if self.mir.types.get(call_return_ty) == Some(&Type::None)
            && matches!(
                self.mir.types.get(target_function.return_ty),
                Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
            ) {
            // A `void`-returning source callback adapted into an erased return
            // slot yields JavaScript `undefined`, not `null`. Invoke the
            // callback for its side effects, then materialize
            // `SmeltUnknown::Undefined` so downstream `!== undefined` guards
            // treat the result as "no value".
            self.carrier_format(format_args!("{{ {call_value}; SmeltUnknown::Undefined }}"))
        } else {
            self.rendered_at_type(&call_value, call_return_ty, target_function.return_ty, scope)?.into_string()
        };
        let returned = if target_function.may_throw && !source_function.may_throw {
            RustExpr::loose(format!("Ok::<_, {ERROR_BOX}>({converted})"))
        } else {
            RustExpr::from(converted)
        };
        // An erased-rest, non-throwing destination is NOT a bare `Rc<dyn Fn(..)>`
        // — `types.rs` renders it as the concrete `SmeltErasedFunction` struct,
        // whose `callback` field takes the packed `Vec<SmeltUnknown>` argument
        // list and returns `SmeltUnknown`. Binding the adapter closure directly
        // as that type is an E0308. Build the struct instead, unpacking the
        // argument vector into the single rest parameter the adapter body reads
        // and erasing the adapted return value to the callable ABI. (The
        // operand-based path reaches `erased_rest_function_value` before
        // this adapter, which is why only rendered-text coercions — such as a
        // callable object's `__smelt_call` slot — hit this shape.)
        if self.is_erased_unknown_rest_function(target_function)
            && !target_function.may_throw
            && let [rest_param] = target_function.params.as_slice()
        {
            let rest_text = self.callback_param_type(
                target_function,
                0,
                *rest_param,
                &TypeSubstitution::erased(),
                MutablePrefix::Apply,
            )?;
            let erased_return =
                self.erase_rendered(&returned, target_function.return_ty)?;
            let length = source_function.params.len();
            return Ok(Some(RustExpr::atom(self.carrier_format(format_args!(
                "{{ let smelt_callback = {value_text}.clone(); SmeltErasedFunction {{ callback: ::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {{ let arg0: {rest_text} = SmeltList::from(smelt_args); {erased_return} }}), length: {length}.0, object: None }} }}"
            )))));
        }
        let target_text = self.rendered_type(target)?;
        Ok(Some(RustExpr::atom(format!(
            "{{ let smelt_callback = {value_text}.clone(); let smelt_adapted: {target_text} = ::std::rc::Rc::new(move |{}| {returned}); smelt_adapted }}",
            arg_decls.join(", ")
        ))))
    }

    /// The argument a rendered adapter forwards to source parameter `index`.
    fn rendered_adapter_argument(
        &self,
        source_function: &FunctionType,
        target_function: &FunctionType,
        source_rest: Option<(usize, TypeId)>,
        index: usize,
        source_param: TypeId,
        scope: &RenderScope,
    ) -> Result<RustExpr, EmitError> {
        // A source rest parameter receives every target argument from
        // its position on, as JavaScript binds `...args`: the list of
        // the remaining arguments, each converted to the element type.
        // A fixed-arity target cannot say which trailing optional
        // arguments were omitted, so an omitted one is an `undefined`
        // element.
        if let Some((rest, element)) = source_rest
            && index == rest
        {
            let elements = target_function
                .params
                .iter()
                .enumerate()
                .skip(rest)
                .map(|(target_index, target_param)| {
                    // The list owns its elements, so a parameter the
                    // adapter receives by reference is cloned first,
                    // as the packed-vector branch below does.
                    let base = if target_function.mutable_params.contains(&target_index)
                        || self.callback_param_is_shared_reference(
                            target_function,
                            target_index,
                            *target_param,
                        ) {
                        RustExpr::chain(format!("arg{target_index}.clone()")).ending_in_clone()
                    } else {
                        RustExpr::path(format!("arg{target_index}"))
                    };
                    self.rendered_at_type(&base, *target_param, element, scope)
                })
                .collect::<Result<Vec<_>, EmitError>>()?
                .join(", ");
            let text = format!("SmeltList::from(vec![{elements}])");
            return Ok(self.callback_call_arg_expr(source_function, index, source_param, text));
        }
        // The wrapped callback's own parameters carry the by-reference ABI
        // (`callback_param_is_shared_reference`) just as the declarations
        // above do for the target's. This adapter forwards a value into
        // each of them, so each forwarded value is borrowed where the
        // wrapped signature says `&`; Rust extends the temporary's lifetime
        // to the end of the statement, so `&(expr)` is valid for a value
        // with no name.
        let text = match target_function.params.get(index) {
            // A conversion consumes its value, and the adapter receives a
            // by-shared-reference parameter as `&T`: it converts an owned
            // copy. An unconverted parameter is forwarded as it came.
            Some(target_param)
                if *target_param != source_param
                    && self.callback_param_is_shared_reference(
                        target_function,
                        index,
                        *target_param,
                    ) =>
            {
                self.rendered_at_type(
                    &RustExpr::chain(format!("arg{index}.clone()")).ending_in_clone(),
                    *target_param,
                    source_param,
                    scope,
                )?
            }
            Some(target_param) => self.rendered_at_type(
                &RustExpr::path(format!("arg{index}")),
                *target_param,
                source_param,
                scope,
            )?,
            None => self.default_value(source_param)?,
        };
        Ok(self.callback_call_arg_expr(source_function, index, source_param, text.into_string()))
    }

    /// The statement appending an adapter's rest parameter, element by element and
    /// erased, to the packed vector `smelt_packed`. `None` when `function` declares
    /// no list-typed rest parameter.
    fn rest_spread_expr(&self, function: &FunctionType) -> Result<Option<RustExpr>, EmitError> {
        let Some(index) = function.rest else {
            return Ok(None);
        };
        let Some(param) = function.params.get(index).copied() else {
            return Ok(None);
        };
        let Some(Type::List(item)) = self.mir.types.get(param) else {
            return Ok(None);
        };
        let by_reference = function.mutable_params.contains(&index)
            || self.callback_param_is_shared_reference(function, index, param);
        let list = if by_reference { format!("arg{index}.clone()") } else { format!("arg{index}") };
        let erased = self.erase_rendered(&RustExpr::path("smelt_item"), *item)?;
        Ok(Some(RustExpr::loose(format!(
            "let smelt_rest: Vec<_> = {list}.into(); smelt_packed.extend(smelt_rest.into_iter().map(|smelt_item| {erased}));"
        ))))
    }

    /// Adapts a concrete callback to a single `Vec<SmeltUnknown>` rest callback.
    pub(in crate::emitter) fn rest_vector_function_adapter(
        &self,
        operand: &Operand,
        target: TypeId,
        borrowed: bool,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(Type::Function(source)) = self.mir.types.get(self.operand_ty(operand)?) else {
            return Ok(None);
        };
        let Some(Type::Function(target_function)) = self.mir.types.get(target) else {
            return Ok(None);
        };
        let Some(0) = target_function.rest else {
            return Ok(None);
        };
        let [rest_param] = target_function.params.as_slice() else {
            return Ok(None);
        };
        let Some(Type::List(rest_item)) = self.mir.types.get(*rest_param) else {
            return Ok(None);
        };
        if !matches!(
            self.mir.types.get(*rest_item),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Never)
        ) {
            return Ok(None);
        }
        // When producing an owned value (not a borrowed `&` argument) and the
        // source is ALREADY a `SmeltErasedFunction` of the same erased-rest
        // shape as the target, re-wrapping would mint a fresh callback `Rc` and
        // break function reference identity. Fall through so the caller emits
        // `{text}.clone()` (shares the inner `Rc`). The borrowed path keeps its
        // existing adapter behaviour. Mirrors the guard in
        // `erased_rest_function_value`.
        if !borrowed
            && self.is_erased_unknown_rest_function(source)
            && !source.may_throw
        {
            return Ok(None);
        }
        let function_text = self.operand_expr(operand)?;
        let is_borrowed_param = match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                self.is_function_parameter_place(place)?
            }
            Operand::Const(_) => false,
        };
        let args = self.function_args_from_smelt_args_text(source, ErasedCallTargetAbi::Declared)?;
        let callback_text = if is_borrowed_param {
            function_text.clone()
        } else {
            RustExpr::path("smelt_callback")
        };
        // Which invocation syntax this adapter body may use is a question about
        // the source value's emitted Rust REPRESENTATION, so it goes through the
        // one authority that answers it (`callback_handle_kind`) instead of
        // re-deriving it from the MIR shape. A callback *parameter* whose MIR type
        // has the erased-unknown-rest shape is still emitted as a borrowed
        // `&dyn Fn` handle (see `param_type`), and `.call(..)` on one of
        // those resolves to the nightly-only `Fn::call` — E0658 `fn_traits`, plus
        // an E0308 because that trait method takes an argument *tuple*. That is
        // exactly the drift `callback_handle_kind`'s doc comment warns about, and
        // es-toolkit `curry`/`curryRight` hit it: they hand their `func` parameter
        // to `makeCurry` through this adapter.
        let source_is_erased = self.callee_uses_erased_call_method(operand)? && !source.may_throw;
        let call = if source_is_erased {
            RustExpr::postfix_on(&callback_text, format!("{callback_text}.call({args})"))
        } else if is_borrowed_param {
            RustExpr::postfix_on(&callback_text, format!("{callback_text}({args})"))
        } else {
            RustExpr::atom(format!("({callback_text})({args})"))
        };
        let source_returns_future = source.is_async
            || matches!(self.mir.types.get(source.return_ty), Some(Type::Future(_)));
        let source_async_output_may_throw =
            source.may_throw || self.operand_closure_can_throw(operand)?;
        let call_value = if source_async_output_may_throw
            && source_returns_future
            && !target_function.may_throw
            && let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) = (
                self.mir.types.get(source.return_ty),
                self.mir.types.get(target_function.return_ty),
            ) {
            let awaited =
                self.rendered_at_type(&RustExpr::path("smelt_async_output"), *source_item, *target_item, &self.render_scope())?;
            if is_borrowed_param {
                async_adapter_future_expr(&call, &awaited)
            } else {
                let async_call = if source_is_erased {
                    RustExpr::chain(format!("smelt_async_callback.call({args})"))
                } else {
                    RustExpr::atom(format!("(smelt_async_callback)({args})"))
                };
                RustExpr::atom(format!(
                    "{{ let smelt_async_callback = {function_text}.clone(); {} }}",
                    async_adapter_future_expr(&async_call, &awaited)
                ))
            }
        } else if source.may_throw && !source_returns_future && target_function.may_throw {
            RustExpr::atom(format!("{call}?"))
        } else if source.may_throw && !source_returns_future {
            RustExpr::postfix_on(&call, format!("{call}.unwrap_or_else(|error| smelt_panic_throw(error))"))
        } else {
            call
        };
        // `SmeltErasedFunction::call` answers `SmeltUnknown` whatever the source's
        // DECLARED return type is — `is_erased_unknown_rest_function` keys only on
        // the parameter shape — so the call's value is already erased and its
        // declared type must not be applied to it a second time. A rest-only
        // function with a concrete return (`function f(...): unknown[]`, which is
        // what an `arguments`-reading function becomes) otherwise produced
        // `SmeltUnknown::Array(<already SmeltUnknown>.into())`, an E0277 in the
        // generated crate. Mirrors the same correction in
        // `rest_vector_unknown_adapter` and
        // `erased_rest_forwarding_closure`.
        let call_return_ty = if source_is_erased {
            self.type_id(Type::Unknown)?
        } else {
            source.return_ty
        };
        let converted_return_text =
            self.rendered_at_type(&call_value, call_return_ty, target_function.return_ty, &self.render_scope())?;
        let default_adjusted_return_text = converted_return_text;
        let return_text = self.adapter_return_expr(
            source,
            target_function,
            target_function.return_ty,
            &call_value,
            default_adjusted_return_text.into_string(),
            AdapterSource { returns_future: source_returns_future, output_may_throw: source_async_output_may_throw },
            true,
        )?;
        // Match the adapter's parameter to its cast target: a pure-rest callback
        // whose first param is a list is `Fn(SmeltList<..>)`, not `Fn(Vec<..>)`.
        // The body reads `smelt_args` only through `.iter()`/`.get()`/`.skip()`,
        // which are `Vec` methods. A `SmeltList` keeps its elements in a shared
        // `Rc<RefCell<Vec<_>>>` and so has no `Deref` to hand those out, so that
        // branch rebinds the arguments to a snapshot under the same name and the
        // body reads one type either way. The snapshot is one copy of the
        // argument vector per call, which is what erasing the callback costs
        // anyway.
        let args_is_list = matches!(
            target_function
                .params
                .first()
                .map(|param| self.mir.types.get(*param)),
            Some(Some(Type::List(_)))
        );
        self.context.note_carrier_use();
        let (smelt_args_ty, args_prelude) = if args_is_list {
            ("SmeltList<SmeltUnknown>", "let smelt_args = smelt_args.to_vec(); ")
        } else {
            ("Vec<SmeltUnknown>", "")
        };
        // Increment 3 deliberately emits NO return annotation here. This
        // renderer threads no call-site bindings, so it can never be at a
        // monomorphizing site -- the precondition
        // `callback_return_annotation` requires -- and the only type
        // environment available to it is the CALLER's lexical scope, under
        // which rendering the callee's declared return captures the caller's
        // unrelated same-named type parameter rather than pinning anything.
        // With no bindings there is also nothing unsolved: the callee's
        // declared rest-callback return is a known expected type.
        let closure =
            format!("move |smelt_args: {smelt_args_ty}| {{ {args_prelude}{return_text} }}");
        // When the source callback is NOT itself a function parameter, the
        // adapter body references it through a `smelt_callback` binding (see
        // `callback_text`), so that binding must be introduced in the emitted
        // scope. The borrowed (`&mut`) path also needs it: without the enclosing
        // `let smelt_callback = ..`, the moved closure references an unbound
        // `smelt_callback` (E0425). A function-parameter source binds nothing
        // because the closure captures the parameter directly.
        Ok(Some(if borrowed {
            // An owned callback closure refers to a `smelt_callback` binding; a
            // borrowed function parameter is called by its own name and needs no
            // binding. When the source is owned, introduce the `smelt_callback`
            // binding inside the borrowed temporary so the closure body resolves
            // (mirrors the owned `::std::rc::Rc::new` branch below).
            if is_borrowed_param {
                RustExpr::loose(format!("&mut {closure}"))
            } else {
                RustExpr::loose(format!("&mut {{ let smelt_callback = {function_text}.clone(); {closure} }}"))
            }
        } else if is_borrowed_param {
            RustExpr::loose(format!("::std::rc::Rc::new({closure})"))
        } else {
            RustExpr::atom(format!(
                "{{ let smelt_callback = {function_text}.clone(); ::std::rc::Rc::new({closure}) }}"
            ))
        }))
    }

    /// Render one erased argument at a callee parameter's declared type.
    ///
    /// The ordinary answer is [`Self::extract_rendered`] at the ambient render
    /// scope. The exception is a parameter whose declared type is a
    /// `Type::TypeParam` this position CANNOT spell: the erasure arm there
    /// answers `SmeltUnknown`, which is a claim about the callee's Rust
    /// signature that this position is in no state to make. The callee is
    /// reached through a value whose type arguments were fixed somewhere else --
    /// a method of `Router<[unknown, RouterRoute]>` erased into a callable,
    /// whose `add(.., handler: T)` really takes `&(SmeltUnknown, RouterRoute)` --
    /// so asserting `SmeltUnknown` is wrong wherever the instantiation was not
    /// itself erased (E0308, 8 of them in Hono's router).
    ///
    /// An ARGUMENT position is exactly where the right answer needs no scope:
    /// the callee's signature IS the expected type, so
    /// `SmeltFromUnknown::smelt_from_unknown(..)` with an INFERRED target lets
    /// rustc solve it from the position, and the identity impl on `SmeltUnknown`
    /// makes a genuinely erased parameter render the value unchanged. This is
    /// the H42 rule -- the render position decides what a type parameter means --
    /// at the one position whose decision is not the emitter's to make.
    ///
    /// This is NOT a general licence for inference: it applies only where the
    /// declared type is a bare type parameter with no Rust name in scope. Every
    /// other shape keeps its checked extraction.
    ///
    /// The `.into_smelt_unknown()` on the source mirrors the SPELLED-target arm
    /// of `extract_rendered` character for character
    /// (`<T as SmeltFromUnknown>::smelt_from_unknown((..).into_smelt_unknown())`).
    /// It is identity on a value that is already erased, and keeping the two
    /// spellings identical is deliberate: they are the same boundary crossing
    /// with and without a name for the target, so `unknown_report`'s
    /// `into_smelt_unknown` boundary marker classifies them the same way and a
    /// baseline diff stays comparable across the change.
    fn erased_argument_at_param_expr(
        &self,
        item: &RustExpr,
        param_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let scope = self.render_scope();
        if let Some(Type::TypeParam { name }) = self.mir.types.get(param_ty)
            && !scope.spells(*name)
        {
            return Ok(RustExpr::atom(self.carrier_format(format_args!(
                "SmeltFromUnknown::smelt_from_unknown(({item}).into_smelt_unknown())"
            ))));
        }
        self.extract_rendered(item, param_ty, &scope)
    }

    /// Render concrete callback arguments extracted from an erased JS argument vector.
    ///
    /// `target_abi` says which ABI the callee behind `smelt_callback` was
    /// rendered with, and it is not always the one `function` declares. A
    /// callback reached through a DECLARED function type follows
    /// [`Self::callback_param_is_shared_reference`], so a by-shared-reference
    /// parameter is handed `&(..)`. A callback that is a closure this emitter
    /// just rendered against a different contextual type — the erased-rest
    /// wrapper in `closures.rs` synthesizes a `FunctionType` purely to describe
    /// the closure's own parameter list — was rendered by value, and packing
    /// `&(..)` for it would not type-check (E0308). The two sites disagree about
    /// the callee, not about the rule, so the caller states which callee it has.
    pub(in crate::emitter) fn function_args_from_smelt_args_text(
        &self,
        function: &FunctionType,
        target_abi: ErasedCallTargetAbi,
    ) -> Result<String, EmitError> {
        function
            .params
            .iter()
            .enumerate()
            .map(|(index, param_ty)| {
                if function.rest == Some(index)
                    && let Some(Type::List(item_ty)) = self.mir.types.get(*param_ty)
                {
                    let by_ref = target_abi == ErasedCallTargetAbi::Declared
                        && self.callback_param_is_shared_reference(function, index, *param_ty);
                    let (open, close) = if by_ref { ("&(", ")") } else { ("", "") };
                    if self.mir.types.get(*item_ty) == Some(&Type::Unknown) {
                        return Ok(format!(
                            "{open}smelt_args.iter().skip({index}).cloned().collect::<SmeltList<_>>(){close}"
                        ));
                    }
                    let item_text = self.extract_rendered(&RustExpr::path("value"), *item_ty, &self.render_scope())?;
                    return Ok(format!(
                        "{open}smelt_args.iter().skip({index}).cloned().map(|value| {item_text}).collect::<SmeltList<_>>(){close}"
                    ));
                }
                let item = RustExpr::chain(self.carrier_format(format_args!("smelt_args.get({index}).cloned().unwrap_or(SmeltUnknown::Null)")));
                let value = self.erased_argument_at_param_expr(&item, *param_ty)?;
                let arg = if function
                    .required_params
                    .is_some_and(|required_params| index >= required_params)
                {
                    let default = self.default_value(*param_ty)?;
                    format!(
                        "if smelt_args.get({index}).is_some() {{ {value} }} else {{ {default} }}"
                    )
                } else {
                    value.into_string()
                };
                if function.mutable_params.contains(&index) {
                    Ok(format!("&mut ({arg})"))
                } else if target_abi == ErasedCallTargetAbi::Declared
                    && self.callback_param_is_shared_reference(function, index, *param_ty)
                {
                    Ok(format!("&({arg})"))
                } else {
                    Ok(arg)
                }
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|args| args.join(", "))
    }

    /// Resolve a callee parameter type through this call site's bindings.
    ///
    /// The adapter renders each parameter twice: once as a declaration
    /// (`arg0: f64`, via `callback_arg_decls` under a substitution carrying
    /// these bindings) and once as the source of the conversion into the wrapped
    /// callback's own parameter type. Both must start from the SAME type. The
    /// declaration side goes through `TypeSubstitution`, which resolves a bound
    /// name to its concrete type; this is the MIR-level equivalent for the body
    /// side, so the two cannot disagree.
    ///
    /// Falls back to the declared type when there are no bindings, or when the
    /// substituted type is not interned -- both mean this site did not
    /// monomorphize, and the declaration falls back the same way.
    pub(in crate::emitter) fn substituted_param_ty(
        &self,
        declared: TypeId,
        callee_bindings: Option<&CalleeTypeParamBindings>,
    ) -> TypeId {
        callee_bindings
            .and_then(|bindings| {
                crate::generic_bindings::substituted_type_id(self.mir, declared, bindings)
            })
            .unwrap_or(declared)
    }

    /// The explicit `-> R` return annotation a call-site adapter closure needs,
    /// or `None` when its return type is already determined.
    ///
    /// Increment 3 of `blocker-logs/estk-callback-generics-plan.md`. Callback
    /// PARAMETER positions were never at risk — `callback_arg_decls` has always
    /// annotated them — but a closure's RETURN type is inferred from its body,
    /// and at a callee that infers a type parameter ONLY through this callback
    /// there is nothing else to solve it from (`E0282`/`E0283`, §1.3).
    ///
    /// An inference variable exists only where the call site MONOMORPHIZED the
    /// callee, so `callee_bindings` is the whole precondition: `Some` means the
    /// callee is generic and this site pinned its type parameters, which is the
    /// only situation in which the closure's return is genuinely unsolved. When
    /// it is `None` the callee's signature carries no free variable at all — it
    /// either is not generic or was not instantiated here — and its declared
    /// callback return is a known expected type rustc coerces the closure to.
    ///
    /// That precondition also makes a whole class of miscompile structurally
    /// unreachable. `Symbol` is name-interned, so rendering a callee's declared
    /// type under the CALLER's lexical scope does not merely lose the binding,
    /// it silently captures the caller's unrelated `T` (see the
    /// `type_substitution` module docstring). Deriving the substitution here
    /// from the bindings alone — the `Some` arm of [`callee_substitution`]
    /// verbatim — means no caller scope can ever reach this rendering, so the
    /// annotation cannot spell a name that is not the callee's own.
    ///
    /// The remaining `substituted != erased` test is what keeps it from being
    /// churn: a binding that renders the same text the erasure would carries
    /// nothing, so nothing is emitted.
    ///
    /// Rendered by [`Self::function_value_return_type`], the same helper
    /// [`Self::callback_fn_trait_text`] uses to build the `F{n}: Fn(..) -> R`
    /// bound, so the annotation and the bound cannot drift; it also composes
    /// `may_throw` into `Result<T, Box<dyn Error>>` and a `Future` return into
    /// `SmeltFuture<T>`.
    pub(in crate::emitter) fn callback_return_annotation(
        &self,
        target_function: &FunctionType,
        callee_bindings: Option<&CalleeTypeParamBindings>,
    ) -> Result<Option<String>, EmitError> {
        let Some(bindings) = callee_bindings else {
            return Ok(None);
        };
        let substitution = TypeSubstitution::erased().with_bindings(bindings);
        let substituted = self.function_value_return_type(target_function, &substitution)?;
        let erased =
            self.function_value_return_type(target_function, &TypeSubstitution::erased())?;
        Ok((substituted != erased).then_some(substituted.into_string()))
    }

    /// Adapt a callback to a compatible target callback shape.
    pub(in crate::emitter) fn function_shape_adapter(
        &self,
        operand: &Operand,
        target: TypeId,
        borrowed: bool,
        callee_bindings: Option<&CalleeTypeParamBindings>,
    ) -> Result<Option<RustExpr>, EmitError> {
        let (Some(Type::Function(source)), Some(Type::Function(target_function))) = (
            self.mir.types.get(self.operand_ty(operand)?),
            self.mir.types.get(target),
        ) else {
            return Ok(None);
        };
        // `SmeltErasedFunction` already erases static return differences at the ABI.
        if self.is_erased_unknown_rest_function(source)
            && !source.may_throw
            && self.is_erased_unknown_rest_function(target_function)
            && !target_function.may_throw
        {
            return Ok(None);
        }
        // A callee parameter `T` bound at this call site to one of the CALLER's
        // own generics is that generic: the callback forwards unchanged, as it
        // did when the two `T`s were (by accident of spelling) one symbol.
        let caller_generics = self.current_function_type_params();
        let parameter_mismatch = source.params.len() != target_function.params.len()
            || source
                .params
                .iter()
                .zip(target_function.params.iter())
                .any(|(source_param, target_param)| {
                    let bound = self.substituted_param_ty(*target_param, callee_bindings);
                    if source_param == target_param {
                        // The closure still names the callee's own parameter,
                        // which is out of scope here and renders erased, while
                        // this site instantiates that parameter (`bound`): the
                        // two callback shapes differ although their ids agree.
                        return bound != *target_param;
                    }
                    !(bound == *source_param
                        && crate::generic_bindings::type_mentions_only_params(
                            self.mir,
                            bound,
                            &caller_generics,
                        ))
                });
        // The adapter's return is converted into the callee's declared callback
        // return type, which must be resolved through this call site's bindings
        // for the same reason its parameters are (see `substituted_param_ty`).
        // Leaving it unsubstituted converts into the erased `T` -- wrapping an
        // `f64` back into `SmeltUnknown::Number(..)` against an `Fn(f64) -> f64`
        // target (E0308). The mismatch test below must use it too, or an adapter
        // that is needed for the parameters alone would compare a substituted
        // return against an unsubstituted one and report a spurious mismatch.
        let target_return_ty = self.substituted_param_ty(target_function.return_ty, callee_bindings);
        let return_mismatch = source.return_ty != target_return_ty;
        let throws_mismatch = (source.may_throw || self.operand_closure_can_throw(operand)?)
            != target_function.may_throw;
        if !parameter_mismatch && !return_mismatch && !throws_mismatch {
            return Ok(None);
        }
        let (Operand::Copy(place) | Operand::Move(place)) = operand else {
            return Ok(None);
        };
        // A borrowed callback parameter is bound as an immutable `&dyn Fn`
        // (see `param_type`), so the adapter reborrows it immutably. A
        // `&mut *` reborrow through the shared reference would fail to compile
        // (E0596); the fresh `move` wrapper closure below supplies whatever
        // `FnMut` shape the target expects on its own.
        let function_text = if self.is_function_parameter_place(place)? {
            format!("&*{}", self.place_expr(place)?)
        } else {
            self.place_expr(place)?.into_string()
        };
        // The adapter's declarations spell the TARGET's parameter types, which
        // at a static call site belong to the callee. When this site pinned the
        // callee's type parameters they are spelled at those concrete types;
        // otherwise the caller's lexical scope stands in, which is the
        // environment the sibling value arguments are coerced in, so the two
        // agree. See `callee_substitution`.
        let caller_scope = self.current_function_type_params();
        let substitution = callee_substitution(&caller_scope, callee_bindings);
        let arg_decls =
            self.callback_arg_decls(target_function, &substitution, MutablePrefix::Ignore)?;
        // Increment 3 of the callback-generics plan: see
        // `callback_return_annotation` for when this is `Some` and why. It is
        // handed the same `callee_bindings` the `substitution` above is built
        // from, so the annotation cannot disagree with the argument
        // declarations it sits beside; when this site did not monomorphize it
        // reads `None` and the caller's lexical scope never reaches a rendered
        // return type.
        //
        // Measured: this is inert on all three compat corpora — every adapter
        // in es-toolkit, remeda and radash is byte-identical.
        let return_annotation = self.callback_return_annotation(target_function, callee_bindings)?;
        // Seam 1b (`emitter::seam_assertions`): the return conversion below uses
        // `target_return_ty`, computed before the declarations above existed.
        // Both are in scope here, so compare them rather than trusting that the
        // two were written from the same bindings.
        #[cfg(debug_assertions)]
        self.debug_assert_adapter_return_agrees(
            target_function.return_ty,
            target_return_ty,
            &substitution,
        );
        // An adapter whose body is placed inside `Box::pin(async move ..)`
        // rebinds every by-reference parameter to an owned clone before the body
        // runs (see `owned_arg_bindings` below, and the E0521 it avoids). That
        // rebinding changes what `argN` NAMES inside the body -- an owned value,
        // not a borrow -- so the forwarding built below has to be written against
        // that effective form. Computing it after the forwarding, as this used to,
        // left the zero-copy arm handing an owned value to a callback declared to
        // take a reference (E0308).
        let body_is_static_future = source.is_async
            || matches!(self.mir.types.get(source.return_ty), Some(Type::Future(_)))
            || matches!(self.mir.types.get(target_return_ty), Some(Type::Future(_)));
        let forwarded = source
            .params
            .iter()
            .enumerate()
            .map(|(index, source_param)| self.forwarded_adapter_argument(source, target_function, index, *source_param, callee_bindings, &substitution, body_is_static_future))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        let source_is_erased = self.is_erased_unknown_rest_function(source) && !source.may_throw;
        let (call_text, callback_prelude) = if source_is_erased {
            (
                RustExpr::chain(format!("_smelt_adapted_callback.call({forwarded})")),
                Some(format!(
                    "let _smelt_adapted_callback = {function_text}.clone();"
                )),
            )
        } else if self.is_function_parameter_place(place)? {
            (RustExpr::atom(format!("({function_text})({forwarded})")), None)
        } else {
            // The adapted callback is a cloned, shareable callback value
            // (`Rc<dyn Fn(..)>`-style) that is only *called* and *cloned* here —
            // never reassigned or mutably borrowed — exactly like the erased
            // `.call` path above, which binds it without `mut`. The async
            // rewrite (below) also only `.clone()`s it, so no `mut` is required.
            (
                RustExpr::atom(format!("(_smelt_adapted_callback)({forwarded})")),
                Some(format!(
                    "let _smelt_adapted_callback = {function_text}.clone();"
                )),
            )
        };
        let uses_adapted_callback = callback_prelude.is_some();
        // The same call through the binding an async body owns.
        let async_callback_call = if source_is_erased {
            RustExpr::chain(format!("smelt_async_callback.call({forwarded})"))
        } else {
            RustExpr::atom(format!("(smelt_async_callback)({forwarded})"))
        };
        let source_returns_future = source.is_async
            || matches!(self.mir.types.get(source.return_ty), Some(Type::Future(_)));
        let source_async_output_may_throw =
            source.may_throw || self.operand_closure_can_throw(operand)?;
        let call_value = if source_async_output_may_throw
            && source_returns_future
            && !target_function.may_throw
            && let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) = (
                self.mir.types.get(source.return_ty),
                self.mir.types.get(target_return_ty),
            ) {
            let awaited =
                self.rendered_at_type(&RustExpr::path("smelt_async_output"), *source_item, *target_item, &self.render_scope())?;
            if uses_adapted_callback {
                RustExpr::atom(format!(
                    "{{ let smelt_async_callback = _smelt_adapted_callback.clone(); {} }}",
                    async_adapter_future_expr(&async_callback_call, &awaited)
                ))
            } else {
                async_adapter_future_expr(&call_text, &awaited)
            }
        } else if source_returns_future
            && uses_adapted_callback
            && let (Some(Type::Future(_)), Some(Type::Future(_))) = (
                self.mir.types.get(source.return_ty),
                self.mir.types.get(target_return_ty),
            )
        {
            let (Some(Type::Future(source_item)), Some(Type::Future(target_item))) = (
                self.mir.types.get(source.return_ty),
                self.mir.types.get(target_return_ty),
            ) else {
                return Err(EmitError::new(
                    "async callback adapter requires future types",
                ));
            };
            let awaited =
                self.rendered_at_type(&RustExpr::path("smelt_async_output"), *source_item, *target_item, &self.render_scope())?;
            RustExpr::atom(format!(
                "{{ let smelt_async_callback = _smelt_adapted_callback.clone(); {} }}",
                async_adapter_future_expr(&async_callback_call, &awaited)
            ))
        } else if source.may_throw && !source_returns_future && target_function.may_throw {
            RustExpr::atom(format!("{call_text}?"))
        } else if source.may_throw && !source_returns_future {
            RustExpr::postfix_on(&call_text, format!("{call_text}.unwrap_or_else(|error| smelt_panic_throw(error))"))
        } else {
            call_text
        };
        let render_return = |call_value: &RustExpr| -> Result<RustExpr, EmitError> {
            let converted_return_text = if source_returns_future
                && uses_adapted_callback
                && matches!(
                    self.mir.types.get(target_return_ty),
                    Some(Type::Future(_))
                ) {
                // Both sides are promise values (`SmeltFuture<..>`): the future was
                // already rebuilt at the target output type when `call_value` was
                // constructed, so no further coercion is needed.
                call_value.clone()
            } else if source_returns_future {
                // The source returns a promise value but the target return is erased
                // (or otherwise not a future), so erase the `SmeltFuture<T>` to a
                // `SmeltUnknown::Promise` boundary value via the normal coercion.
                self.rendered_at_type(call_value, source.return_ty, target_return_ty, &self.render_scope())?
            } else if source_is_erased {
                // An erased callable is invoked through `SmeltErasedFunction::call`,
                // which yields a bare `SmeltUnknown` at runtime regardless of the
                // source function's declared return type. Coerce the call result
                // from `Unknown` so the target coercion emits a checked adapter
                // (e.g. `SmeltUnknown` -> `Option<...>` via a Null/Undefined guard)
                // rather than treating the value as if it already had the source
                // return type and calling `Option` methods on a `SmeltUnknown`.
                let unknown_ty = self.type_id(Type::Unknown)?;
                self.rendered_at_type(call_value, unknown_ty, target_return_ty, &self.render_scope())?
            } else if self.mir.types.get(source.return_ty) == Some(&Type::None)
                && matches!(
                    self.mir.types.get(target_return_ty),
                    Some(Type::Unknown | Type::Union(_) | Type::TypeParam { .. })
                ) {
                // A `void`-returning source callback adapted into an erased return
                // slot produces JavaScript `undefined`, not `null`. Invoke the
                // callback for its side effects, then materialize
                // `SmeltUnknown::Undefined` so downstream `!== undefined` guards
                // (e.g. cloneDeepWith's customizer wrapper) correctly treat the
                // result as "no value" rather than a real `null` clone.
                RustExpr::atom(self.carrier_format(format_args!("{{ {call_value}; SmeltUnknown::Undefined }}")))
            } else {
                self.rendered_at_type(call_value, source.return_ty, target_return_ty, &self.render_scope())?
            };
            let field_adjusted_return_text = if !source_returns_future
                && matches!(
                    self.mir.types.get(target_return_ty),
                    Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
                )
                && self.is_erased_class_type(source.return_ty)
            {
                call_value.clone()
            } else {
                converted_return_text
            };
            let default_adjusted_return_text = field_adjusted_return_text;
            self.adapter_return_expr(
                source,
                target_function,
                target_return_ty,
                call_value,
                default_adjusted_return_text.into_string(),
                AdapterSource { returns_future: source_returns_future, output_may_throw: source_async_output_may_throw },
                false,
            )
        };
        let return_text = render_return(&call_value)?;
        // Invariant: an adapter body that does not mention the wrapped callback
        // has silently discarded it. Such a closure still type-checks and still
        // returns a plausible value, so the defect is invisible in the generated
        // Rust and only shows up as a test that never runs its callback.
        // The body drops the call exactly when it renders the same with any other
        // value in the call's place.
        let drops_call = render_return(&RustExpr::path("smelt_adapter_probe"))?.as_str() == return_text.as_str();
        // One general repair for every coercion arm that renders a constant.
        //
        // A coercion answers with a bare constant — `SmeltUnknown::Null`,
        // `None::<bool>`, `Default::default()` — precisely when the source has
        // NO VALUE to convert: its return type is `void` (`Type::None`) or
        // uninhabited (`Type::Never`, an arrow whose body only throws). Such an
        // arm ignores the value text it was handed, so the adapter body loses
        // the call and the callback is never invoked. Two shapes hit this in
        // es-toolkit: `attempt(() => { throw new Error('test') })` (`never` ->
        // `unknown`, emitted `move || SmeltUnknown::Null`) and `isMatch`'s
        // `void` customizer adapted into a `boolean | undefined` slot (`void`
        // -> `Optional(Bool)`, emitted `move || None::<bool>`).
        //
        // In both cases the constant is the CORRECT result — a valueless source
        // really does produce "no value", and a diverging one never reaches the
        // constant at all — so the repair is not to change it but to evaluate
        // the call for its effects in front of it. `let _ =` accepts the call's
        // own Rust type whatever it is, including `!`.
        //
        // The rule is deliberately keyed on the CAUSE (a source with no value)
        // rather than on any one arm's constant, so a new arm rendering a new
        // constant is repaired without another entry here. It stays out of the
        // future paths: `call_value` for a future source is a `SmeltFuture`
        // expression that always mentions the call already, and `let _ =` on a
        // future would construct it without polling.
        let source_has_no_value = matches!(
            self.mir.types.get(source.return_ty),
            Some(Type::None | Type::Never)
        );
        let return_text = if source_has_no_value
            && !source_returns_future
            && drops_call
        {
            format!("{{ let _ = {call_value}; {return_text} }}")
        } else {
            return_text.into_string()
        };
        // Anything still missing the call had a real value to convert and lost
        // it, which is a coercion defect rather than a valueless source. Fail
        // the emit instead of shipping a closure that silently returns a
        // constant.
        if drops_call && (!source_has_no_value || source_returns_future) {
            // Name the site: a backstop that cannot say WHERE it fired forces a
            // bisect over the whole crate. The enclosing function, the adapted
            // callback's place, and both return types are what identify the
            // coercion arm that dropped the value.
            let enclosing = self.symbol_name(self.function.name).unwrap_or("<unnamed>");
            let callback = self
                .place_expr(place)
                .unwrap_or_else(|_| RustExpr::loose("<unrenderable place>"));
            let source_return = self.mir.types.get(source.return_ty);
            let target_return = self.mir.types.get(target_return_ty);
            return Err(EmitError::new(format!(
                "callback adapter would discard the wrapped callback: \
                 in `{enclosing}`, adapting `{callback}` \
                 (source return {source_return:?} -> target return {target_return:?}) \
                 emitted body `{return_text}`, which never invokes it"
            )));
        }
        // An adapter parameter the destination `dyn Fn` passes by shared reference
        // (`callback_param_is_shared_reference`) cannot cross into a `'static`
        // future: the branches above place the body inside `Box::pin(async move ..)`
        // whenever either side of the adapter returns a promise, and a borrow of the
        // caller's argument does not live that long (E0521). Copy each such
        // parameter into an owned binding of the same name first — the value the
        // by-VALUE ABI used to hand the body — which happens per CALL and only for
        // the async adapters, not for the synchronous per-element ones the
        // by-reference ABI exists to speed up.
        let owned_arg_bindings: Vec<String> = if body_is_static_future {
            target_function
                .params
                .iter()
                .enumerate()
                .filter(|(index, param)| {
                    self.callback_param_is_shared_reference(target_function, *index, **param)
                })
                .map(|(index, _)| format!("let arg{index} = arg{index}.clone();"))
                .collect()
        } else {
            Vec::new()
        };
        let owned_arg_prelude = if owned_arg_bindings.is_empty() {
            String::new()
        } else {
            format!("{} ", owned_arg_bindings.join(" "))
        };
        // Rust requires a block body once a closure annotates its return type.
        let closure_tail = match &return_annotation {
            Some(annotation) => format!("-> {annotation} {{ {owned_arg_prelude}{return_text} }}"),
            None if owned_arg_prelude.is_empty() => return_text,
            None => format!("{{ {owned_arg_prelude}{return_text} }}"),
        };
        let closure = if let Some(prelude) = callback_prelude {
            format!(
                "{{ {prelude} move |{}| {closure_tail} }}",
                arg_decls.join(", ")
            )
        } else {
            format!("move |{}| {closure_tail}", arg_decls.join(", "))
        };
        if borrowed {
            return Ok(Some(RustExpr::loose(format!("&mut {closure}"))));
        }
        let adapter = RustExpr::loose(format!("::std::rc::Rc::new({closure})"));
        if uses_adapted_callback
            && !source_is_erased
            && let Some(wide_erasure) =
                self.arity_narrowing_erasure_expr(place, source, target_function)?
        {
            return Ok(Some(RustExpr::atom(format!(
                "{{ let smelt_narrowed_callable = {adapter}; smelt_register_narrowed_callable(&smelt_narrowed_callable, {wide_erasure}); smelt_narrowed_callable }}"
            ))));
        }
        Ok(Some(adapter))
    }

    /// The argument an adapter forwards to source parameter `index`: its own parameter converted, the rest list, or the default for a surplus parameter.
    fn forwarded_adapter_argument(
        &self,
        source: &FunctionType,
        target_function: &FunctionType,
        index: usize,
        source_param: TypeId,
        callee_bindings: Option<&CalleeTypeParamBindings>,
        #[cfg_attr(
            not(debug_assertions),
            expect(unused_variables, reason = "read only by the debug-build adapter assertion")
        )]
        substitution: &TypeSubstitution<'_>,
        body_is_static_future: bool,
    ) -> Result<String, EmitError> {
        if source.rest == Some(index)
            && target_function.params.len() > source.params.len()
            && let Some(Type::List(source_item)) = self.mir.types.get(source_param)
            && target_function.params.get(index).is_some()
        {
            let mut text = String::from("{ let mut smelt_forwarded_args = Vec::new(); ");
            for (target_index, target_param) in
                target_function.params.iter().enumerate().skip(index)
            {
                // The rest list this builds OWNS its elements, so a
                // parameter the adapter declares by shared reference
                // (`callback_param_is_shared_reference`) is copied out
                // before it is packed. Without the copy the vector infers
                // `SmeltList<&SmeltUnknown>` and the `Into<Vec<_>>` at the
                // erased call fails to resolve (E0277).
                let arg_text = if self.callback_param_is_shared_reference(
                    target_function,
                    target_index,
                    *target_param,
                ) {
                    RustExpr::chain(format!("arg{target_index}.clone()")).ending_in_clone()
                } else {
                    RustExpr::path(format!("arg{target_index}"))
                };
                if target_index > index
                    && let Some(Type::List(target_item)) = self.mir.types.get(*target_param)
                {
                    let item_text = if matches!(
                        self.mir.types.get(*source_item),
                        Some(Type::Unknown | Type::Never | Type::None)
                    ) && self.mir.types.get(*target_item) == Some(&Type::Unknown)
                    {
                        "value".to_owned()
                    } else {
                        self.rendered_at_type(
                            &RustExpr::path("value"),
                            *target_item,
                            *source_item,
                            &self.render_scope(),
                        )?.into_string()
                    };
                    text.push_str(&format!(
                        "smelt_forwarded_args.extend({arg_text}.into_iter().map(|value| {item_text})); "
                    ));
                } else {
                    let item_text =
                        self.rendered_at_type(&arg_text, *target_param, *source_item, &self.render_scope())?;
                    text.push_str(&format!("smelt_forwarded_args.push({item_text}); "));
                }
            }
            text.push_str("Into::<SmeltList<_>>::into(smelt_forwarded_args) }");
            return Ok(text);
        }
        // The mirror case: the SLOT packs its trailing arguments into a
        // rest list (`extractor(response, index, ...params)`), and the
        // callback names them one by one (`(response, _, word) => ..`).
        // JavaScript spreads the list into those parameters, so each one
        // reads its element of the rest list (`undefined` past its end)
        // instead of receiving the whole list converted to its type.
        if let Some(rest_index) = target_function.rest
            && index >= rest_index
            // A callback parameter is positional unless it is itself a
            // list-typed rest; a contextually typed closure can carry
            // the slot's rest index on a scalar parameter.
            && !(source.rest == Some(index)
                && matches!(self.mir.types.get(source_param), Some(Type::List(_))))
            && let Some(target_rest) = target_function.params.get(rest_index)
            && let Some(Type::List(rest_item)) = self.mir.types.get(*target_rest)
        {
            let offset = index.saturating_sub(rest_index);
            let missing = self.element_missing_value(*rest_item)?;
            let element = RustExpr::chain(format!(
                "arg{rest_index}.borrow().get({offset}).cloned().unwrap_or_else(|| {missing})"
            ));
            let arg_text =
                self.rendered_at_type(&element, *rest_item, source_param, &self.render_scope())?;
            if self.callback_param_is_shared_reference(source, index, source_param) {
                return Ok(format!("&({arg_text})"));
            }
            return Ok(arg_text.into_string());
        }
        if let Some(target_param) = target_function.params.get(index) {
            // The adapter's parameter is DECLARED at the substituted
            // type (`arg0: f64`, rendered from the same bindings in
            // `callback_arg_decls`), so the conversion into the wrapped
            // callback's parameter must start from that same type. Using
            // the unsubstituted declared `T` here computes the ladder for
            // an erased argument -- `match arg0 { SmeltUnknown::Number(v)
            // => v, .. }` against an `f64` (E0308). One substitution has
            // to drive the declaration and the body alike.
            let declared = self.substituted_param_ty(*target_param, callee_bindings);
            // Seam 1 (`emitter::seam_assertions`): `declared` is where
            // this conversion starts; the declaration it must match was
            // rendered above under `substitution`.
            #[cfg(debug_assertions)]
            self.debug_assert_adapter_param_agrees(
                index,
                *target_param,
                declared,
                substitution,
            );
            // Both sides of the adapter carry their own by-reference axis
            // (`callback_param_is_shared_reference`): the parameter the
            // adapter DECLARES, and the parameter of the callback it wraps.
            let declares_reference = self.callback_param_is_shared_reference(
                target_function,
                index,
                *target_param,
            );
            let wrapped_takes_reference =
                self.callback_param_is_shared_reference(source, index, source_param);
            // A declared reference an async adapter has already rebound to
            // an owned clone is, inside the body, an owned value.
            let declared_reference_is_owned_in_body =
                declares_reference && body_is_static_future;
            // The zero-copy path, and the reason the ABI exists: the same
            // by-reference type on both sides forwards the borrow untouched --
            // re-borrowing the owned rebinding where there is one.
            if declares_reference && wrapped_takes_reference && declared == source_param {
                return Ok(if declared_reference_is_owned_in_body {
                    format!("&arg{index}")
                } else {
                    format!("arg{index}")
                });
            }
            // Otherwise a conversion runs, and every conversion arm is written
            // against an owned value — so a declared reference is copied back
            // into one first. That copy is per CALL of the adapter, and only
            // where the two sides genuinely disagree about the type; a
            // parameter the async rebinding already owns needs no second copy.
            let source_text = if declares_reference && !declared_reference_is_owned_in_body
            {
                RustExpr::chain(format!("arg{index}.clone()")).ending_in_clone()
            } else {
                RustExpr::path(format!("arg{index}"))
            };
            let arg_text =
                self.rendered_at_type(&source_text, declared, source_param, &self.render_scope())?;
            // The converted value is a temporary, and Rust extends a borrowed
            // temporary's lifetime to the end of the statement, so `&(expr)` is
            // valid even though the value is unnamed.
            if wrapped_takes_reference {
                return Ok(format!("&({arg_text})"));
            }
            Ok(arg_text.into_string())
        } else {
            let default_text = self.default_value(source_param)?;
            if self.callback_param_is_shared_reference(source, index, source_param) {
                return Ok(format!("&({default_text})"));
            }
            Ok(default_text.into_string())
        }
    }

    /// Erased full-arity view of a callback an adapter narrows to fewer parameters.
    ///
    /// A shape adapter whose SOURCE declares more parameters than its TARGET
    /// slot supplies (`((ctx, next) => ..) as Next` with `Next = () => ..`, or a
    /// callback with optional trailing parameters passed where a shorter
    /// callback is expected) fills the missing trailing arguments with
    /// defaults when it is called through the target's typed signature. That
    /// is right for a TYPED call — the target signature is all the caller can
    /// supply — but the adapter is still the same JavaScript function as its
    /// source: an assertion or a wider-to-narrower assignment changes nothing
    /// at runtime, and a call that reaches the value through an erased
    /// `Function` (`handler(context, next)`) passes every argument it has.
    ///
    /// Returns the erased (`SmeltUnknown::Function`) rendering of the source
    /// callback, which the caller records against the adapter allocation with
    /// `smelt_register_narrowed_callable`; every erasure of the adapter then
    /// answers that full-arity function instead of a wrapper that can only
    /// forward the target's parameters. `None` when the adapter does not narrow
    /// the arity (same or more target parameters, or a target rest parameter
    /// that already forwards every argument).
    fn arity_narrowing_erasure_expr(
        &self,
        place: &Place,
        source: &FunctionType,
        target_function: &FunctionType,
    ) -> Result<Option<RustExpr>, EmitError> {
        if target_function.rest.is_some() || source.params.len() <= target_function.params.len() {
            return Ok(None);
        }
        let source_ty = self.place_ty(place)?;
        let source_text = { let receiver = self.place_expr(place)?; RustExpr::postfix_on(&receiver, format!("{receiver}.clone()")) }.ending_in_clone();
        self.erase_rendered(&source_text, source_ty).map(Some)
    }

    /// If `operand` is a bare function-item-as-value wrapper, return its crate
    /// unique item cache key and the self-contained erased `SmeltUnknown::Function`
    /// accessor body for that item.
    ///
    /// The body forwards to the function item directly: the wrapper closure has
    /// no captures and references the free function by name, so the resulting
    /// expression captures no outer local and can be lifted verbatim into a
    /// module-level `__smelt_fn_value_<key>()` accessor that caches one shared
    /// erased value. Returns `None` for any operand that is not a bare function
    /// item value (e.g. user arrows), so those keep their fresh per-reference
    /// identity through the ordinary erase path.
    pub(in crate::emitter) fn function_item_erased_accessor(
        &self,
        operand: &Operand,
    ) -> Result<Option<(usize, String)>, EmitError> {
        let Some(local) = operand.local() else {
            return Ok(None);
        };
        let Some(closure_id) = closure_definitions(self.function)?.get(&local).copied() else {
            return Ok(None);
        };
        let Some(closure) = self
            .mir
            .closures
            .get(id_index(closure_id.0, "closure index does not fit usize")?)
        else {
            return Ok(None);
        };
        let Some(key) = closure.function_item_key else {
            return Ok(None);
        };
        let source_ty = self.operand_ty(operand)?;
        let Some(Type::Function(_)) = self.mir.types.get(source_ty) else {
            return Ok(None);
        };
        // Everything below is rendered into a MODULE-LEVEL accessor, so it is
        // rendered with no type parameters in scope: the enclosing function may
        // be generic, but `fn __smelt_fn_value_<key>()` is not, and spelling its
        // `T` there is `E0412`. A generic function item's erased value is its
        // `T = SmeltUnknown` instantiation, which is what the empty scope emits.
        let hoisted = HoistedModuleItemScope::enter(self);
        // Self-contained typed wrapper that forwards to the function item by name
        // (no captures, no local reference): `::std::rc::Rc::new(move |..| func1(..))`.
        let wrapper_text = self.closure_expr_for_type(closure_id, source_ty)?;
        // Erased forwarding closure using `smelt_callback` as the bound source.
        let inner = self.erased_rest_forwarding_closure(source_ty)?;
        // Register the source arity on the cached callable: an `Rc<dyn Fn>` cannot
        // carry `Function.prototype.length`, and this accessor is the only place a
        // function *item*'s erased value is built.
        let body = self.carrier_format(format_args!(
            "SmeltUnknown::Function({{ let smelt_callback = {wrapper_text}; let smelt_erased_fn: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, {ERROR_BOX}>> = {inner}; {register}(&smelt_erased_fn, {length}.0); smelt_erased_fn }})",
            register = smelt_stdlib::runtime_symbols::function_length::REGISTER,
            length = self.operand_function_length(operand)?,
        ));
        drop(hoisted);
        Ok(Some((key, body)))
    }

    /// Route a typed function-item-as-value wrapper through a cached accessor.
    ///
    /// Returns `__smelt_fn_typed_<key>_<ordinal>()` for a closure that is a bare
    /// function item referenced as a value (`function_item_key`, no captures)
    /// at a `Type::Function` destination whose Rust spelling needs no type
    /// parameter, registering the accessor's wrapper body on first use. See
    /// [`super::EmitContext::function_item_typed_accessors`] for why: every
    /// reference must share one `Rc` so `===` answers as in JavaScript.
    ///
    /// Returns `None` — keep the inline fresh wrapper — for a user closure
    /// (each arrow expression IS a distinct function), a non-function
    /// destination, an erased-rest callable (it has its own per-item accessor),
    /// or a callable type that mentions a type parameter in scope:
    /// the accessor is a module-level `fn` with no generics, so a type it
    /// cannot spell has no single cached value.
    pub(in crate::emitter) fn function_item_typed_accessor(
        &self,
        id: smelt_mir::ClosureId,
        dest_ty: TypeId,
    ) -> Result<Option<String>, EmitError> {
        let closure = self.mir_closure(id)?;
        let Some(key) = closure.function_item_key else {
            return Ok(None);
        };
        let Some(Type::Function(function)) = self.mir.types.get(dest_ty) else {
            return Ok(None);
        };
        // An erased-rest callable renders as `SmeltErasedFunction`, whose
        // per-item `__smelt_fn_erased_<key>()` accessor (`closure_expr_for_type`)
        // already gives every reference one shared identity.
        if !closure.captures.is_empty() || self.is_erased_unknown_rest_function(function) {
            return Ok(None);
        }
        let scoped_text = self.rendered_type(dest_ty)?;
        let hoisted = HoistedModuleItemScope::enter(self);
        let module_text = self.rendered_type(dest_ty)?;
        if module_text != scoped_text {
            return Ok(None);
        }
        let wrapper = self.closure_expr_for_type(id, dest_ty)?;
        drop(hoisted);
        let mut accessors = self.context.minted.function_item_typed_accessors.borrow_mut();
        let next_ordinal = accessors
            .keys()
            .filter(|(existing, _)| *existing == key)
            .count();
        let (ordinal, _) = accessors
            .entry((key, module_text.into_string()))
            .or_insert_with(|| (next_ordinal, wrapper.into_string()));
        Ok(Some(format!("__smelt_fn_typed_{key}_{ordinal}()")))
    }

    /// Build the erased rest-forwarding closure for a function type.
    ///
    /// Emits `::std::rc::Rc::new(move |smelt_args| { .. })` that forwards the
    /// erased `Vec<SmeltUnknown>` argument vector to a callback the caller has
    /// bound as `smelt_callback: Rc<dyn Fn..>`, then erases the result back into
    /// `SmeltUnknown`. The closure is self-contained: it references only
    /// `smelt_callback` and `smelt_args`, never the original operand local, so it
    /// can be reused both by `rest_vector_unknown_adapter` (owned callback
    /// path) and by the function-item value accessor.
    ///
    /// This mirrors the `needs_owned_callback` branch of
    /// `rest_vector_unknown_adapter`: the call is `smelt_callback.call(..)`
    /// when the source already exposes the erased rest ABI and cannot throw, and
    /// `(smelt_callback)(..)` otherwise. The `return_text` branches (None-return,
    /// fieldless class, may-throw, ordinary erase) match that function exactly.
    fn erased_rest_forwarding_closure(
        &self,
        source_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let Some(Type::Function(source)) = self.mir.types.get(source_ty) else {
            return Err(EmitError::new(
                "erased rest forwarding closure requires a function type",
            ));
        };
        let args = self.function_args_from_smelt_args_text(source, ErasedCallTargetAbi::Declared)?;
        let source_is_erased = self.is_erased_unknown_rest_function(source) && !source.may_throw;
        let call = if source_is_erased {
            RustExpr::chain(format!("smelt_callback.call({args})"))
        } else {
            RustExpr::atom(format!("(smelt_callback)({args})"))
        };
        // `SmeltErasedFunction::call` answers `SmeltUnknown` whatever the source's
        // DECLARED return type is, so the value is already erased and must not be
        // erased again. Mirrors the same correction in
        // `rest_vector_unknown_adapter`.
        let source = &FunctionType {
            return_ty: if source_is_erased {
                self.type_id(Type::Unknown)?
            } else {
                source.return_ty
            },
            ..source.clone()
        };
        // A `never`-returning callback (one that always throws) is still CALLED:
        // its throw is the whole point of invoking it, and erasing its "result"
        // must not replace the call with a constant. It therefore takes the
        // statement shape of a `void` callback, whose value is never observed.
        let return_text = if matches!(self.mir.types.get(source.return_ty), Some(Type::None | Type::Never)) {
            // A `void`-returning callback erased to a callable value returns
            // JavaScript `undefined`, not `null`.
            if source.may_throw {
                self.carrier_format(format_args!(
                    "{{ {call}?; Ok::<SmeltUnknown, {ERROR_BOX}>(SmeltUnknown::Undefined) }}"
                ))
            } else {
                self.carrier_format(format_args!("{{ {call}; Ok::<SmeltUnknown, {ERROR_BOX}>(SmeltUnknown::Undefined) }}"))
            }
        } else if matches!(self.mir.types.get(source.return_ty), Some(Type::Future(_))) {
            let value = self.erase_rendered(&call, source.return_ty)?;
            self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({value})"))
        } else if self.is_erased_class_type(source.return_ty) {
            // A return value skips erasure only when its Rust
            // representation already IS `SmeltUnknown`
            // (`is_erased_class_type`), not merely when the class declares no
            // fields: a MODELED host class such as `Request` or `ArrayBuffer`
            // declares none here yet renders as its own concrete struct. See
            // the same seam in `coercion::erase_rendered`.
            if source.may_throw {
                call.into_string()
            } else {
                self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({call})"))
            }
        } else if source.may_throw {
            let value = self.erase_rendered(&RustExpr::atom(format!("{call}?")), source.return_ty)?;
            self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({value})"))
        } else {
            let value = self.erase_rendered(&call, source.return_ty)?;
            self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({value})"))
        };
        Ok(RustExpr::loose(self.carrier_format(format_args!(
            "::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {return_text})"
        ))))
    }

    /// Registration text linking an erased callable to the class it constructs.
    ///
    /// Erasing `class X extends Host {}` to a callable value drops the one thing
    /// `instanceof` needs: which class the callable constructs. The instances
    /// record their own class (`__smelt_class`), so the callable records the
    /// class names whose instances belong to it — the class itself plus every
    /// subclass declared in this crate, resolved here where the hierarchy is
    /// known rather than by a runtime class-graph walk.
    ///
    /// Empty text unless the callable returns a class AND this crate has host
    /// override slots, since the registry only exists (and is only consulted)
    /// there: a crate that never reassigns a host global emits byte-identical
    /// output.
    fn constructed_class_registration_expr(&self, return_ty: TypeId) -> RustExpr {
        let Some(Type::Class { name, .. }) = self.mir.types.get(return_ty) else {
            return RustExpr::opaque("");
        };
        if crate::stdlib::host_override_slot_names(self.mir).is_empty() {
            return RustExpr::opaque("");
        }
        let Ok(class_name) = self.symbol_source_name(*name) else {
            return RustExpr::opaque("");
        };
        let mut classes = vec![format!("{class_name:?}")];
        for class in &self.mir.classes {
            if class.name != *name
                && self.class_extends_or_equals(class.name, *name)
                && let Ok(subclass_name) = self.symbol_source_name(class.name)
            {
                classes.push(format!("{subclass_name:?}"));
            }
        }
        RustExpr::loose(format!(
            "smelt_register_function_classes(&smelt_erased_fn, &[{}]); ",
            classes.join(", ")
        ))
    }

    /// Adapts a concrete callback to the erased JavaScript callback surface.
    pub(super) fn rest_vector_unknown_adapter(
        &self,
        operand: &Operand,
    ) -> Result<Option<RustExpr>, EmitError> {
        let source_ty = self.operand_ty(operand)?;
        let Some(Type::Function(source)) = self.mir.types.get(source_ty) else {
            return Ok(None);
        };
        let function_text = self.operand_expr(operand)?;
        let needs_owned_callback = matches!(
            operand,
            Operand::Copy(place) | Operand::Move(place) if !self.is_function_parameter_place(place)?
        );
        if needs_owned_callback {
            // Bind the callback as `smelt_callback` and reuse the shared erased
            // forwarding closure builder. The bound name and call shapes match
            // the original owned-callback code, so the emitted text is identical.
            let inner = self.erased_rest_forwarding_closure(source_ty)?;
            return Ok(Some(RustExpr::atom(format!(
                // Bind the source ONCE, then record which callable the adapter
                // forwards to so JavaScript `===` can see through the wrapper
                // (`smelt_same_erased_function`). Two erasures of one closure
                // build two adapters; without this they compare unequal.
                // Register the source arity alongside the identity link: an
                // `Rc<dyn Fn>` cannot carry `Function.prototype.length`, so this is
                // the last point that knows both the arity and the allocation it
                // belongs to. es-toolkit `rest(func)` reads `func.length` off
                // exactly this adapter.
                // A callback built by an arity-NARROWING adapter
                // (`function_shape_adapter`) already recorded the
                // full-arity erased function it forwards to; that function, not
                // a fresh wrapper limited to the narrowed parameters, is the
                // value's erased form.
                "{{ let smelt_source_fn = {function_text}.clone(); let smelt_callback = smelt_source_fn.clone(); let smelt_erased_fn: ::std::rc::Rc<dyn Fn(Vec<SmeltUnknown>) -> Result<SmeltUnknown, Box<dyn std::error::Error>>> = if let Some(SmeltUnknown::Function(smelt_narrowed_fn)) = smelt_lookup_callable_object(&smelt_source_fn) {{ smelt_narrowed_fn }} else {{ {inner} }}; smelt_link_function_identity(&smelt_erased_fn, &smelt_source_fn); {register}(&smelt_erased_fn, {length}.0); {class_registration}smelt_erased_fn }}",
                register = smelt_stdlib::runtime_symbols::function_length::REGISTER,
                length = self.operand_function_length(operand)?,
                class_registration = self.constructed_class_registration_expr(source.return_ty),
            ))));
        }
        // Non-owned (function-parameter) path: invoke the callback by its operand
        // text directly, with no binding or extra parentheses. Kept inline so the
        // emitted text remains byte-identical to the previous implementation.
        let args = self.function_args_from_smelt_args_text(source, ErasedCallTargetAbi::Declared)?;
        let source_is_erased = self.is_erased_unknown_rest_function(source) && !source.may_throw;
        let call = if source_is_erased {
            RustExpr::postfix_on(&function_text, format!("{function_text}.call({args})"))
        } else {
            RustExpr::postfix_on(&function_text, format!("{function_text}({args})"))
        };
        // A `never`-returning callback (one that always throws) is still CALLED:
        // its throw is the whole point of invoking it, and erasing its "result"
        // must not replace the call with a constant. It therefore takes the
        // statement shape of a `void` callback, whose value is never observed.
        let return_text = if matches!(self.mir.types.get(source.return_ty), Some(Type::None | Type::Never)) {
            // A `void`-returning callback erased to a callable value returns
            // JavaScript `undefined`, not `null`.
            if source.may_throw {
                self.carrier_format(format_args!(
                    "{{ {call}?; Ok::<SmeltUnknown, {ERROR_BOX}>(SmeltUnknown::Undefined) }}"
                ))
            } else {
                self.carrier_format(format_args!("{{ {call}; Ok::<SmeltUnknown, {ERROR_BOX}>(SmeltUnknown::Undefined) }}"))
            }
        } else if matches!(self.mir.types.get(source.return_ty), Some(Type::Future(_))) {
            let value = self.erase_rendered(&call, source.return_ty)?;
            self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({value})"))
        } else if self.is_erased_class_type(source.return_ty) {
            // A return value skips erasure only when its Rust
            // representation already IS `SmeltUnknown`
            // (`is_erased_class_type`), not merely when the class declares no
            // fields: a MODELED host class such as `Request` or `ArrayBuffer`
            // declares none here yet renders as its own concrete struct. See
            // the same seam in `coercion::erase_rendered`.
            if source.may_throw {
                call.into_string()
            } else {
                self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({call})"))
            }
        } else if source.may_throw {
            let value = self.erase_rendered(&RustExpr::atom(format!("{call}?")), source.return_ty)?;
            self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({value})"))
        } else {
            let value = self.erase_rendered(&call, source.return_ty)?;
            self.carrier_format(format_args!("Ok::<SmeltUnknown, {ERROR_BOX}>({value})"))
        };
        Ok(Some(RustExpr::loose(self.carrier_format(format_args!(
            "::std::rc::Rc::new(move |smelt_args: Vec<SmeltUnknown>| {return_text})"
        )))))
    }
}

/// What an adapter knows about the callable it wraps.
struct AdapterSource {
    /// Whether the source's call yields a future.
    returns_future: bool,
    /// Whether that future's output can be an error.
    output_may_throw: bool,
}

/// Renders an async callback adapter: an inner async call whose settled output is
/// re-typed for the adapter's own return type.
///
/// The inner call is emitted *outside* the returned future's body, on purpose.
/// Calling a JavaScript async function starts it — the promise it hands back is
/// already running — and Smelt models that with `SmeltFuture::from_future_primed`,
/// whose eager first poll runs the body up to its first suspension. That only
/// happens if the call itself happens. Emitting the call *inside* a lazy
/// `from_future` body defers it until the adapter's own future is polled, so a
/// batch of adapted callbacks (`Promise.all(array.map(cb))`, where every element
/// goes through an arity/type adapter) starts one at a time as the combinator
/// awaits them and the source's concurrency silently disappears. Hoisting the
/// call keeps the start eager and defers only the output conversion.
///
/// `rendered_call` is the inner call expression; `awaited_text` converts the binding
/// `smelt_async_output` to the adapter's output type.
fn async_adapter_future_expr(call_text: &RustExpr, awaited_text: &RustExpr) -> RustExpr {
    RustExpr::atom(format!(
        "{{ let smelt_async_source = {call_text}; SmeltFuture::from_future(Box::pin(async move {{ let smelt_async_output = smelt_async_source.await?; Ok::<_, {ERROR_BOX}>({awaited_text}) }})) }}"
    ))
}
