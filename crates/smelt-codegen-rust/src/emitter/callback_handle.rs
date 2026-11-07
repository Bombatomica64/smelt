//! How a callable value is held and invoked: erased `SmeltErasedFunction`
//! values, borrowed `&dyn Fn` handles and monomorphized generic handles.

use super::*;
use crate::emitter::local_binding::LocalBinding;
use crate::rust::RustExpr;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// Return whether a function type is represented as an erased JS rest callable.
    pub(super) fn is_erased_unknown_rest_function(&self, function: &FunctionType) -> bool {
        is_erased_unknown_rest_function_in(&self.mir.types, function)
    }

    /// Return whether the currently emitted Rust body can propagate a `?`.
    ///
    /// A fallible body returns `Result<_, Box<dyn std::error::Error>>`, so a
    /// throwing call inside it can forward the error with `?` instead of
    /// aborting the process with `panic!`. Generators are excluded: their `?`
    /// would target the generator step's output type rather than the body's.
    pub(super) fn body_can_propagate_error(&self) -> bool {
        self.function.can_throw && !self.function.is_generator
    }

    /// Return whether the *emitted Rust value* for `callee` carries the inherent
    /// `SmeltErasedFunction::call` method, rather than being a bare `dyn Fn`
    /// handle that is invoked with direct call syntax.
    ///
    /// This is the single authority for that question. Two call-emitting sites
    /// used to answer it independently — `Rvalue::ClosureCall` with an explicit
    /// precedence ladder, and `rendered_call`'s `Callee::Indirect` with none — so
    /// routing one call from the statement form to the terminator form silently
    /// flipped its ABI (E0658 `fn_traits` plus E0308).
    ///
    /// The precedence matters and is encoded here once: a function *parameter*
    /// and a borrowed callback *capture* are emitted as borrowed `&dyn Fn`
    /// handles (see `param_type`) even when their MIR type has the
    /// erased-unknown-rest shape, so they must be invoked directly. Emitting
    /// `.call(..)` on them would resolve to the unstable `Fn::call` trait
    /// method. Only a value whose emitted Rust type really is the
    /// `SmeltErasedFunction` struct answers `true`.
    pub(super) fn callee_uses_erased_call_method(
        &self,
        callee: &Operand,
    ) -> Result<bool, EmitError> {
        Ok(matches!(
            self.callback_handle_kind(callee)?,
            Some(CallbackHandleKind::ErasedCall)
        ))
    }

    /// Classify how the emitted Rust value for `callee` must be invoked.
    ///
    /// The one authority behind [`Self::callee_uses_erased_call_method`] and
    /// [`Self::callee_is_borrowed_function_handle`], which are thin `matches!`
    /// wrappers over it so the three call-shape ladders keep their exact
    /// current text.
    ///
    /// `None` means the callee is an ordinary callable *value* with no special
    /// handle shape; the ladders render it parenthesized (`({callee})({args})`).
    ///
    /// Precedence is load-bearing and unchanged: borrowed handles win over the
    /// erased-rest shape. A callback parameter whose MIR type is the
    /// erased-unknown-rest shape (`Fn(SmeltList<SmeltUnknown>) -> _`) is still
    /// emitted as a borrowed handle — and is still `F{n}`-eligible, because
    /// §4.4's rest test reads the *enclosing* function's packed rest parameter,
    /// not the callback's own — so calling `.call(..)` on it would resolve to
    /// the unstable `Fn::call` trait method (E0658 `fn_traits` plus E0308).
    pub(super) fn callback_handle_kind(
        &self,
        callee: &Operand,
    ) -> Result<Option<CallbackHandleKind>, EmitError> {
        // `operand_ty` is resolved first so this helper keeps the exact error
        // behaviour of the `callee_uses_erased_call_method` it replaces.
        let erased_shape = match self.mir.types.get(self.operand_ty(callee)?) {
            Some(Type::Function(function)) => {
                self.is_erased_unknown_rest_function(function) && !function.may_throw
            }
            _ => false,
        };
        if let Operand::Copy(place) | Operand::Move(place) = callee
            && self.is_function_parameter_place(place)?
        {
            return Ok(Some(self.borrowed_handle_kind(place)?));
        }
        let callee_text = self.operand_expr(callee)?;
        if self.is_function_parameter_name(&callee_text)?
            || self.is_borrowed_callback_capture_name(&callee_text)
        {
            let kind = match callee {
                Operand::Copy(place) | Operand::Move(place) => self.borrowed_handle_kind(place)?,
                Operand::Const(_) => CallbackHandleKind::BorrowedDyn,
            };
            return Ok(Some(kind));
        }
        if erased_shape {
            return Ok(Some(CallbackHandleKind::ErasedCall));
        }
        Ok(None)
    }

    /// Which borrowed representation a known-borrowed callback place carries.
    ///
    /// Purely descriptive: both variants are invoked with the same bare direct
    /// call syntax, because `impl<A, F: ?Sized + Fn<A>> Fn<A> for &F` makes a
    /// `&F{n}` callable exactly like a `&dyn Fn(..)`.
    fn borrowed_handle_kind(&self, place: &Place) -> Result<CallbackHandleKind, EmitError> {
        if let Place::Local(local) = place
            && self.callback_generic_name(*local)?.is_some()
        {
            return Ok(CallbackHandleKind::MonomorphizedGeneric);
        }
        Ok(CallbackHandleKind::BorrowedDyn)
    }

    /// Render the argument vector for a call through the erased
    /// `SmeltErasedFunction::call` ABI.
    ///
    /// The companion to `callee_uses_erased_call_method`: the one authority that
    /// answers "how do I call this value" also answers "how do I shape its
    /// arguments", because the two answers have to agree.
    ///
    /// The two callable ABIs disagree about what a rest argument list *is*.
    /// `&dyn Fn(SmeltList<SmeltUnknown>) -> _` takes the packed list as its one
    /// parameter, so lowering hands this emitter a single `SmeltList` operand
    /// standing for all N source arguments (`g(3, 4)` packs to `[3, 4]`;
    /// `g(arr)` packs to `[arr]`; `g(...arr)` passes `arr` itself). But
    /// `SmeltErasedFunction::call(impl Into<Vec<SmeltUnknown>>)` takes the
    /// *argument vector*, where each element is one argument. Erasing the packed
    /// list into a single vector element (`vec![SmeltUnknown::Array([3, 4])]`)
    /// therefore calls the callback with ONE array argument instead of two
    /// arguments — it compiles, and silently passes the wrong arity.
    ///
    /// So when the argument list is already the rest-packed list, hand it over as
    /// the argument vector (`From<SmeltList<T>> for Vec<T>` converts it) rather
    /// than nesting it. Only genuinely separate per-argument operands are erased
    /// element-by-element.
    pub(super) fn erased_call_argument_vector_expr(
        &self,
        callee: &Operand,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        if let Some(Type::Function(function)) = self.mir.types.get(self.operand_ty(callee)?)
            && function.rest == Some(0)
            && let [rest_param] = function.params.as_slice()
            && let [packed] = args
            && self.operand_ty(packed)? == *rest_param
        {
            return self.value_at_type(packed, *rest_param);
        }
        self.erased_call_args_expr(args)
    }

    /// Return whether the emitted Rust value for `callee` is a borrowed
    /// `&dyn Fn` handle, which is invoked with bare direct call syntax.
    ///
    /// Companion to `callee_uses_erased_call_method`; the two together are the
    /// full precedence ladder for choosing an indirect call's shape.
    pub(super) fn callee_is_borrowed_function_handle(
        &self,
        callee: &Operand,
    ) -> Result<bool, EmitError> {
        Ok(matches!(
            self.callback_handle_kind(callee)?,
            Some(CallbackHandleKind::BorrowedDyn | CallbackHandleKind::MonomorphizedGeneric)
        ))
    }

    /// Adapt a concrete callable to an erased JS rest callable while preserving
    /// the source `Function.length` metadata.
    pub(super) fn erased_rest_function_value(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(Type::Function(target_function)) = self.mir.types.get(target) else {
            return Ok(None);
        };
        if !self.is_erased_unknown_rest_function(target_function) || target_function.may_throw {
            return Ok(None);
        }
        // The source is ALREADY lowered to `SmeltErasedFunction` (the exact
        // predicate `types.rs` uses to choose that Rust type). Re-wrapping it
        // into a fresh `SmeltErasedFunction` would mint a new callback `Rc`,
        // breaking function reference identity (`Rc::ptr_eq`) for singletons
        // like `doNothing()`. Pass it through unchanged: the caller falls to
        // `{text}.clone()`, which shares the inner `Rc`.
        if let Some(Type::Function(source)) = self.mir.types.get(self.operand_ty(operand)?)
            && self.is_erased_unknown_rest_function(source)
            && !source.may_throw
        {
            return Ok(None);
        }
        let Some(callback) = self.smelt_erased_function_callback_expr(operand, target)? else {
            return Ok(None);
        };
        let length = self.operand_function_length(operand)?;
        Ok(Some(RustExpr::loose(format!(
            "SmeltErasedFunction {{ callback: {callback}, length: {length}.0, object: None }}"
        ))))
    }

    /// Adapt a typed function operand to the runtime callback stored by
    /// `SmeltErasedFunction`.
    ///
    /// The erased callable ABI always stores a `Vec<SmeltUnknown> ->
    /// SmeltUnknown` callback, even when the static function type has a more
    /// precise return. This keeps function values first-class while ensuring the
    /// erased representation can be called uniformly by JavaScript-style helper
    /// code.
    fn smelt_erased_function_callback_expr(
        &self,
        operand: &Operand,
        target: TypeId,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(Type::Function(source)) = self.mir.types.get(self.operand_ty(operand)?) else {
            return Ok(None);
        };
        let Some(Type::Function(target_function)) = self.mir.types.get(target) else {
            return Ok(None);
        };
        if !self.is_erased_unknown_rest_function(target_function) || target_function.may_throw {
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
        let source_is_erased = self.is_erased_unknown_rest_function(source) && !source.may_throw;
        let call = if source_is_erased {
            RustExpr::postfix_on(&callback_text, format!("{callback_text}.call({args})"))
        } else if is_borrowed_param {
            RustExpr::postfix_on(&callback_text, format!("{callback_text}({args})"))
        } else {
            RustExpr::atom(format!("({callback_text})({args})"))
        };
        // An async callback carries its throw path inside the returned future's
        // `Result`; invoking the callback yields the future directly. Only a
        // synchronous throwing callback returns a `Result` at this call site.
        let source_returns_future = matches!(
            self.mir.types.get(source.return_ty),
            Some(Type::Future(_))
        );
        let call_value = if source.may_throw && !source_returns_future {
            RustExpr::postfix_on(&call, format!("{call}.unwrap_or_else(|error| smelt_panic_throw(error))"))
        } else {
            call
        };
        let unknown_ty = self.type_id(Type::Unknown)?;
        // A `never`-returning callback (one that always throws) is still CALLED:
        // its throw is the whole point of invoking it, and erasing its "result"
        // must not replace the call with a constant. It therefore takes the
        // statement shape of a `void` callback, whose value is never observed.
        let return_text = if matches!(self.mir.types.get(source.return_ty), Some(Type::None | Type::Never)) {
            let null_text = self.null_value();
            format!("{{ {call_value}; {null_text} }}")
        } else {
            self.rendered_at_type(&call_value, source.return_ty, unknown_ty, &self.render_scope())?.into_string()
        };
        let closure = self.carrier_format(format_args!("move |smelt_args: Vec<SmeltUnknown>| {return_text}"));
        Ok(Some(if is_borrowed_param {
            RustExpr::loose(format!("::std::rc::Rc::new({closure})"))
        } else {
            RustExpr::atom(format!(
                "{{ let smelt_callback = {function_text}.clone(); ::std::rc::Rc::new({closure}) }}"
            ))
        }))
    }

    /// Return the JavaScript `Function.length` represented by a function operand.
    pub(super) fn operand_function_length(&self, operand: &Operand) -> Result<usize, EmitError> {
        if let Some(local) = operand.local()
            && let Some(closure_id) = closure_definitions(self.function)?.get(&local).copied()
            && let Some(closure) = self
                .mir
                .closures
                .get(id_index(closure_id.0, "closure index does not fit usize")?)
        {
            return Ok(closure
                .required_params
                .unwrap_or_else(|| closure.rest.unwrap_or(closure.params.len())));
        }

        Ok(match self.mir.types.get(self.operand_ty(operand)?) {
            Some(Type::Function(function)) => function
                .required_params
                .unwrap_or_else(|| function.rest.unwrap_or(function.params.len())),
            _ => 1,
        })
    }

    /// Return true when a place names a borrowed function parameter.
    pub(super) fn is_function_parameter_place(&self, place: &Place) -> Result<bool, EmitError> {
        let Place::Local(local_id) = place else {
            return Ok(false);
        };
        let local_decl = self.local_decl(*local_id)?;
        if matches!(self.local_binding(*local_id)?, LocalBinding::ClosureArg(_)) {
            return Ok(false);
        }
        Ok(matches!(local_decl.kind, LocalKind::Param { .. })
            && matches!(self.mir.types.get(local_decl.ty), Some(Type::Function(_)))
            && !self.function_parameter_requires_owned(*local_id)?)
    }

    /// Return whether a function-typed parameter needs an owned callback handle.
    pub(super) fn function_parameter_requires_owned(
        &self,
        local: LocalId,
    ) -> Result<bool, EmitError> {
        self.function_parameter_requires_owned_in(self.function, local)
    }

    /// Return whether a function-typed parameter in `function` needs ownership.
    pub(super) fn function_parameter_requires_owned_in(
        &self,
        function: &MirFunction,
        local: LocalId,
    ) -> Result<bool, EmitError> {
        let Some(local_decl) = function
            .locals
            .get(id_index(local.0, "local index does not fit usize")?)
        else {
            return Ok(false);
        };
        if !matches!(local_decl.kind, LocalKind::Param { .. })
            || !matches!(self.mir.types.get(local_decl.ty), Some(Type::Function(_)))
        {
            return Ok(false);
        }
        if matches!(function.origin, HirOrigin::ClassConstructor { .. }) {
            return Ok(true);
        }
        Ok(self
            .context
            .owned_callback_params
            .contains(&(function.id, local)))
    }

    /// Return true when emitted text names a source function parameter.
    ///
    /// Callback bodies can reference captured source parameters through MIR
    /// locals that are not themselves in the nested closure parameter list. The
    /// emitted Rust type is still `&dyn Fn`, so calls must use direct
    /// callable syntax instead of the `Rc<RefCell<_>>` handle path.
    pub(super) fn is_function_parameter_name(&self, name: &str) -> Result<bool, EmitError> {
        Ok(self.function.params.iter().any(|param| {
            self.local_binding(*param).is_ok_and(|binding| {
                !matches!(binding, LocalBinding::ClosureArg(_)) && binding.text() == name
            }) && self.local_decl(*param).is_ok_and(|local| {
                    matches!(self.mir.types.get(local.ty), Some(Type::Function(_)))
                })
                && !self
                    .function_parameter_requires_owned(*param)
                    .unwrap_or(false)
        }))
    }

    /// Return true for captured callback parameters that keep borrowed Rust type.
    ///
    /// Closure MIR currently records captured outer callback parameters as
    /// ordinary function-typed locals, so name shape is the remaining signal
    /// that the emitted Rust value is a borrowed `&dyn Fn` rather than
    /// an owned callable handle.
    pub(super) fn is_borrowed_callback_capture_name(&self, name: &str) -> bool {
        self.borrowed_callback_names.contains(name)
    }

    /// Return true when a captured local is a borrowed callback parameter.
    ///
    /// Such captures must not force a `move` closure: moving the borrowed
    /// callback prevents later direct uses in the same generated function.
    pub(super) fn capture_is_borrowed_callback_param(
        &self,
        local: LocalId,
    ) -> Result<bool, EmitError> {
        let decl = self.local_decl(local)?;
        let local_name = self.local_binding(local)?.text();
        Ok(
            matches!(self.mir.types.get(decl.ty), Some(Type::Function(_)))
                && self.function.params.iter().any(|param| {
                    self.local_binding(*param)
                        .is_ok_and(|binding| binding.text() == local_name)
                        && self
                            .function_parameter_requires_owned(*param)
                            .is_ok_and(|requires_owned| !requires_owned)
                }),
        )
    }

    /// Return true when a closure capture symbol names a borrowed callback parameter.
    pub(super) fn capture_symbol_is_borrowed_callback_param(
        &self,
        symbol: Symbol,
        ty: TypeId,
    ) -> Result<bool, EmitError> {
        if !matches!(self.mir.types.get(ty), Some(Type::Function(_))) {
            return Ok(false);
        }
        let capture_name = self.symbol_name(symbol)?;
        Ok(self.function.params.iter().any(|param| {
            self.local_binding(*param)
                .is_ok_and(|binding| binding.text() == capture_name)
                && self
                    .function_parameter_requires_owned(*param)
                    .is_ok_and(|requires_owned| !requires_owned)
        }))
    }

    /// Render a callback argument for a borrowed function parameter.
    ///
    /// Borrowed callback params are used only when crate-level callback ABI
    /// analysis proves the callee does not retain the callback. Owned callback
    /// values are borrowed from their reentrant `Rc<dyn Fn>` handle for the
    /// duration of the call; already-borrowed callback parameters are reborrowed.
    /// `callee_bindings` is the concrete instantiation this call site pinned for
    /// the callee's own type parameters, or `None` when the site did not
    /// monomorphize (see
    /// `crate::emitter::call::FunctionEmitter::static_call_monomorphization`).
    /// It selects the environment the callee's declared callback type renders
    /// in, and it must be the SAME decision the sibling value arguments used: a
    /// site that passes `SmeltList<f64>` through concretely instantiates the
    /// callee at `T = f64`, so its callback adapter has to be declared
    /// `|arg0: f64|` and not `|arg0: SmeltUnknown|` (`E0631`).
    pub(super) fn borrowed_function_argument_expr(
        &self,
        operand: &Operand,
        target: TypeId,
        callee_bindings: Option<&CalleeTypeParamBindings>,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(self.operand_ty(operand)?),
            Some(Type::Function(_))
        ) {
            // A dynamically-typed source (e.g. a `SmeltUnknown` element spread
            // from a `...args` rest array) still carries a callable at runtime.
            // Recover the owned `Rc<dyn Fn>` through checked extraction and
            // reborrow it for the duration of the call instead of substituting a
            // no-op default callback, which would silently swap the caller's
            // predicate/mapper for one that always returns the type's default.
            // The erased-unknown-rest shape extracts to a `SmeltErasedFunction`
            // (not an `Rc<dyn Fn>`) so it cannot be reborrowed this way; leave it
            // to the existing default fallback.
            if (matches!(
                self.mir.types.get(self.operand_ty(operand)?),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
            ) || self.is_erased_class_type(self.operand_ty(operand)?))
                && let Some(Type::Function(function)) = self.mir.types.get(target)
                && !self.is_erased_unknown_rest_function(function)
            {
                let owned = self.extract(operand, target, &self.render_scope())?;
                return Ok(RustExpr::loose(format!("&*({owned})")));
            }
            return self.borrowed_default_function_expr(target, callee_bindings);
        }
        if let Some(adapter) = self.rest_vector_function_adapter(operand, target, true)? {
            return Ok(adapter);
        }
        if let Some(adapter) =
            self.function_shape_adapter(operand, target, true, callee_bindings)?
        {
            return Ok(adapter);
        }
        match operand {
            Operand::Copy(place) | Operand::Move(place) => {
                let place_expr = self.place_expr(place)?;
                Ok(RustExpr::loose(format!("&*{place_expr}")))
            }
            Operand::Const(_) => self.borrowed_default_function_expr(target, callee_bindings),
        }
    }

    /// Render an inline typed no-op callback for a borrowed function parameter.
    pub(super) fn borrowed_default_function_expr(
        &self,
        target: TypeId,
        callee_bindings: Option<&CalleeTypeParamBindings>,
    ) -> Result<RustExpr, EmitError> {
        let Some(Type::Function(function)) = self.mir.types.get(target) else {
            return Err(EmitError::new(
                "borrowed default callback target must be a function",
            ));
        };
        // `target` is always a CALLEE-declared parameter type here, so its type
        // parameters belong to the callee. Rendered without the `&mut ` prefix
        // `function_type_param` would add, which is preserved verbatim.
        let caller_scope = self.current_function_type_params();
        let substitution = callee_substitution(&caller_scope, callee_bindings);
        let params = self
            .callback_arg_decls(function, &substitution, MutablePrefix::Ignore)?
            .join(", ");
        let return_value = self.default_value(function.return_ty)?;
        let return_text = if function.may_throw {
            format!("Ok::<_, {ERROR_BOX}>({return_value})")
        } else {
            return_value.into_string()
        };
        // Increment 3: this is the renderer that produces the literal
        // `&|| Default::default()` of §1.3. With no closure argument to carry
        // the type, a callee type parameter appearing only in this callback's
        // return has no inference source at all, so the annotation is what makes
        // it definite. `callback_only_params_are_pinned_at_every_call_site`
        // demotes the callee crate-wide for the same shape; the redundancy is
        // deliberate, since the valve is crate-wide and this is local.
        if let Some(annotation) = self.callback_return_annotation(function, callee_bindings)? {
            return Ok(RustExpr::loose(format!("&|{params}| -> {annotation} {{ {return_text} }}")));
        }
        Ok(RustExpr::loose(format!("&|{params}| {return_text}")))
    }

    /// Wrap a borrowed callback parameter in a cloneable owned callable handle.
    pub(super) fn borrowed_function_handle_expr(
        &self,
        function_text: &RustExpr,
        target: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let Some(Type::Function(function)) = self.mir.types.get(target) else {
            return Err(EmitError::new(
                "borrowed callback handle target must be a function",
            ));
        };
        // Near-verbatim twin of `borrowed_default_function_expr`: same caller
        // scope, same omitted `&mut ` prefix, preserved for the same reason.
        let caller_scope = self.current_function_type_params();
        let params = self.callback_arg_decls(
            function,
            &TypeSubstitution::lexical(&caller_scope),
            MutablePrefix::Ignore,
        )?;
        let args = (0..function.params.len())
            .map(|index| format!("arg{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        // The wrapped callable is CAPTURED by the `move` closure, so naming it
        // in the body moves it out of whatever binding holds it. When that
        // binding is a parameter of an enclosing closure — `closure_arg_1`,
        // bound by value as an owned `Rc<dyn Fn(..)>` — the enclosing closure is
        // an `Fn` and cannot give a captured variable away (E0507: "cannot move
        // out of `closure_arg_1`, a captured variable in an `Fn` closure").
        //
        // Bind a clone in front of the closure, which is the clone discipline
        // every sibling adapter already uses: `_smelt_adapted_callback` in
        // `function_shape_adapter` and `smelt_callback` in
        // `rendered_function_shape_adapter` are both `{place}.clone()`
        // preludes for exactly this reason. The clone is one `Rc` bump per
        // evaluation of this expression, not per invocation of the handle it
        // builds. `&dyn Fn` — the borrowed-parameter shape this helper is named
        // for — is `Copy`, so `.clone()` on it yields the same shared reference
        // and that emission is unchanged.
        Ok(RustExpr::atom(format!(
            "{{ let smelt_handle_callback = {function_text}.clone(); \
             ::std::rc::Rc::new(move |{}| (smelt_handle_callback)({args})) }}",
            params.join(", ")
        )))
    }
}

/// How the emitted Rust value for a callee is invoked.
///
/// The three-valued generalisation of the `callee_uses_erased_call_method` /
/// `callee_is_borrowed_function_handle` pair (Increment 2 of
/// `blocker-logs/estk-callback-generics-plan.md`, §3).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CallbackHandleKind {
    /// A `SmeltErasedFunction` value: invoked as `value.call(vec![..])`.
    ErasedCall,
    /// A borrowed `&dyn Fn(..)` handle: bare direct call syntax.
    BorrowedDyn,
    /// A borrowed `&F{n}` monomorphized generic handle: bare direct call
    /// syntax, exactly as `BorrowedDyn`. Distinguished for honesty about the
    /// emitted representation, not for dispatch — no caller branches on the
    /// difference, because `impl<A, F: ?Sized + Fn<A>> Fn<A> for &F` makes the
    /// two spellings callable identically.
    MonomorphizedGeneric,
}

/// Return whether a function type is represented as an erased JS rest callable.
///
/// Free-function form of `FunctionEmitter::is_erased_unknown_rest_function`, so the
/// pre-emission ownership analysis in `emitter::mod` (which runs before any
/// `Emitter` exists) asks the exact same question the type renderer does. The
/// method delegates here; the predicate has one definition.
pub(crate) fn is_erased_unknown_rest_function_in(
    types: &smelt_hir::TypeInterner,
    function: &FunctionType,
) -> bool {
    if function.is_async || matches!(types.get(function.return_ty), Some(Type::Future(_))) {
        return false;
    }
    let Some(0) = function.rest else {
        return false;
    };
    let [param] = function.params.as_slice() else {
        return false;
    };
    matches!(
        types.get(*param),
        Some(Type::List(item))
            if matches!(
                types.get(*item),
                Some(Type::Unknown | Type::TypeParam { .. } | Type::Never)
            )
    )
}
