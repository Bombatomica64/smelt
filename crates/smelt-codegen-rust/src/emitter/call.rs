//! Rendering a call: the callee shape decides the call form and its arguments.

use crate::rust::RustExpr;
use super::*;
use smelt_hir::FunctionType;

/// A rendered call, and whether it propagates the callee's error.
///
/// `rendered_call` keeps the `?` apart from the call: a caller that cannot propagate
/// (a body with no error channel, a `try` block with a catch arm) handles the
/// `Result` itself.
#[derive(Clone, Debug)]
pub(super) struct RenderedCall {
    /// The call expression, without any `?`.
    call: RustExpr,
    /// Whether the call yields a `Result` this body propagates with `?`.
    propagates: bool,
}

impl From<RustExpr> for RenderedCall {
    /// A call that yields its value directly.
    fn from(call: RustExpr) -> Self {
        Self::plain(call)
    }
}

impl RenderedCall {
    /// A call that yields its value directly.
    pub(super) fn plain(call: impl Into<RustExpr>) -> Self {
        Self {
            call: call.into(),
            propagates: false,
        }
    }

    /// A call that yields a `Result` whose error propagates.
    pub(super) fn propagating(call: impl Into<RustExpr>) -> Self {
        Self {
            call: call.into(),
            propagates: true,
        }
    }

    /// `call`, propagating exactly when the callee can throw.
    fn propagating_if(call: impl Into<RustExpr>, propagates: bool) -> Self {
        Self {
            call: call.into(),
            propagates,
        }
    }

    /// The call as an expression of its value.
    pub(super) fn into_expression(self) -> RustExpr {
        if self.propagates {
            RustExpr::atom(format!("{}?", self.call))
        } else {
            self.call
        }
    }

    /// Whether the call propagates the callee's error.
    pub(super) const fn propagates(&self) -> bool {
        self.propagates
    }

    /// The call expression, without any `?`.
    pub(super) fn into_call(self) -> RustExpr {
        self.call
    }

    /// This call with its text rewritten by `rewrite`, keeping whether it
    /// propagates (the text is a `Result` exactly when it does).
    pub(super) fn map_call(self, rewrite: impl FnOnce(&RustExpr) -> String) -> Self {
        Self { call: RustExpr::chain(rewrite(&self.call)), propagates: self.propagates }
    }
}

impl From<String> for RenderedCall {
    fn from(call: String) -> Self {
        Self::plain(call)
    }
}

impl FunctionEmitter<'_> {
    /// Converts a function call to its Rust text representation.
    pub(super) fn rendered_call(&self, callee: &Callee, args: &[Operand]) -> Result<RenderedCall, EmitError> {
        match callee {
            Callee::Builtin(BuiltinFn::ConsoleLog { absent }) => self.console_log_call(*absent, args),
            Callee::Builtin(BuiltinFn::ConsoleWrite | BuiltinFn::ConsoleErrorWrite) => {
                // `process.stdout.write` takes a string, so the optional arm is
                // unreachable from here; the TypeScript spelling is the right
                // default for a write that only comes from that surface.
                let (format_spec, value) = args
                    .first()
                    .map(|argument| self.console_arg(argument, AbsentSpelling::Undefined))
                    .transpose()?
                    .unwrap_or_else(|| ("{}", "\"\"".to_owned()));
                let macro_name = if matches!(callee, Callee::Builtin(BuiltinFn::ConsoleWrite)) {
                    "print"
                } else {
                    "eprint"
                };
                Ok(RustExpr::atom(format!("{{ {macro_name}!(\"{format_spec}\", {value}); }}")).into())
            }
            Callee::Op(op, Some(ty)) => Ok(self.rvalue_expr_for_dest(op, *ty)?.into()),
            Callee::Op(op, None) => Ok(self.fallible_op_call(op)?.text),
            Callee::BaseInit(func) => self.base_initializer_call(*func, args),
            Callee::Static(func) => self.static_call(*func, args),
            Callee::Indirect(indirect_callee) => self.indirect_call(indirect_callee, args),
        }
    }

    /// Render arguments for a call into an erased `SmeltErasedFunction`, erasing
    /// each to `SmeltUnknown` and packing them into the `vec![..]` the runtime
    /// `.call(..)` method consumes.
    pub(super) fn erased_call_args_expr(
        &self,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        let unknown_ty = self.type_id(Type::Unknown)?;
        let rendered = args
            .iter()
            .map(|arg| self.value_at_type(arg, unknown_ty))
            .collect::<Result<Vec<_>, _>>()?
            .join(", ");
        Ok(RustExpr::atom(format!("vec![{rendered}]")))
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
    pub(super) fn indirect_call_args_expr(
        &self,
        function: &FunctionType,
        args: &[Operand],
    ) -> Result<RustExpr, EmitError> {
        self.indirect_call_args_text_with_abi(function, args, None).map(RustExpr::from)
    }

    /// `indirect_call_args_expr`, with the DECLARED parameter types that decide
    /// the callee's ABI supplied separately from the substituted ones.
    ///
    /// The two differ exactly where a generic declaration has been instantiated.
    /// A callable slot declared `(value: T) => ..` is emitted as
    /// `Rc<dyn Fn(&T) -> ..>`, because `param_type_is_by_shared_reference`
    /// answers TRUE for a bare type parameter — the ABI has to be the callee's,
    /// and the callee's own declaration is the only place that knows it. Rust
    /// then instantiates that slot at `Fn(&f64)` for a `Chain<number>`, while
    /// MIR hands this call site the SUBSTITUTED parameter type `f64`, which the
    /// same predicate answers FALSE for. The argument was packed by value
    /// against a slot spelled `&`, which is E0308 and is the
    /// `&(SmeltUnknown, RouterRoute)` family in `hono-round30-types.md`.
    ///
    /// So the ABI is asked of `abi_params` — the declaration's parameter list —
    /// while the VALUE is still rendered at the substituted type, which is what
    /// it has to be coerced to. Substituting a type parameter at a call site
    /// must not change how the callee is called.
    fn indirect_call_args_text_with_abi(
        &self,
        function: &FunctionType,
        args: &[Operand],
        abi_params: Option<&[TypeId]>,
    ) -> Result<String, EmitError> {
        let abi_param = |index: usize, substituted: TypeId| {
            abi_params
                .and_then(|params| params.get(index).copied())
                .unwrap_or(substituted)
        };
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
                    self.mutable_reference_argument_expr(arg, target_ty, None)
                } else if self.callback_param_is_shared_reference(
                    function,
                    index,
                    abi_param(index, target_ty),
                ) {
                    // The parameter is `&T` (see `callback_param_is_shared_reference`).
                    self.shared_reference_argument_expr(arg, target_ty)
                } else {
                    self.closure_value_argument_expr(arg, target_ty)
                }
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (index, target_ty) in function
            .params
            .iter()
            .copied()
            .enumerate()
            .skip(rendered_args.len())
        {
            // A by-reference (`mutable_params`) parameter lowers to `&mut T`,
            // which has no value-shaped default expression to pad with; such a
            // parameter is only reachable through an explicit argument, so stop
            // padding rather than inventing a temporary place for it.
            if function.mutable_params.contains(&index) {
                break;
            }
            // A padded argument is a fresh temporary, so a by-shared-reference
            // parameter borrows it in place. The ABI question takes the
            // declaration's parameter here too, for the reason above.
            rendered_args.push(self.callback_call_arg_expr(
                function,
                index,
                abi_param(index, target_ty),
                self.closure_value_padding_expr(target_ty)?.into_string(),
            ));
        }
        Ok(rendered_args.join(", "))
    }

    /// Emits an optional first-class function call as an optional return value.
    pub(super) fn optional_indirect_call_expr_for_dest(
        &self,
        callee: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
        unwrap_errors: bool,
    ) -> Result<Option<RustExpr>, EmitError> {
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

        let callee_text = self.operand_expr(callee)?;
        let rendered_args = self.indirect_call_args_expr(function, args)?;
        let raw_call = if self.function_value_call_is_fallible(function) {
            if unwrap_errors {
                RustExpr::atom(format!(
                    "(smelt_function)({rendered_args}).unwrap_or_else(|error| smelt_panic_throw(error))"
                ))
            } else {
                RustExpr::loose(format!("(smelt_function)({rendered_args})?"))
            }
        } else {
            RustExpr::atom(format!("(smelt_function)({rendered_args})"))
        };
        let coerced_call =
            self.rendered_at_type(&raw_call, function.return_ty, *inner_dest_ty, &self.render_scope())?;
        Ok(Some(RustExpr::postfix_on(&callee_text, format!(
            "{callee_text}.clone().map(|smelt_function| {coerced_call})"
        ))))
    }

    /// Converts a call expression to Rust, coercing the result to `dest_ty`.
    pub(super) fn call_expr_for_dest(
        &self,
        callee: &Callee,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if let Callee::Indirect(indirect_callee) = callee
            && let Some(call_text) =
                self.optional_indirect_call_expr_for_dest(indirect_callee, args, dest_ty, false)?
        {
            return Ok(call_text);
        }
        if let Callee::Static(func) = callee
            && let Some(adapter) = self.static_call_mut_list_adapter(*func, args, dest_ty)?
        {
            return Ok(adapter);
        }
        // Seam 2 (`emitter::seam_assertions`): the argument half of this call
        // site is rendered inside `rendered_call` and the return half inside
        // `call_emitted_source_ty`, each recomputing the monomorphization
        // decision for itself. Sample it on both sides of the argument
        // rendering and compare.
        #[cfg(debug_assertions)]
        let monomorphization_before_arguments = self.sampled_call_monomorphization(callee, args);
        let mut call_text = self.rendered_call(callee, args)?.into_expression();
        #[cfg(debug_assertions)]
        self.debug_assert_call_monomorphization_stable(
            &self.sampled_callee_name(callee),
            monomorphization_before_arguments.as_ref(),
            self.sampled_call_monomorphization(callee, args).as_ref(),
        );
        call_text = self.wrap_native_async_call_expr(callee, call_text)?;
        let source_ty = self.call_emitted_source_ty(callee, args, dest_ty)?;
        self.rendered_at_type(&call_text, source_ty, dest_ty, &self.render_scope())
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
            // The initializer answers the receiver it was handed, which is the
            // derived instance the destination holds.
            Callee::BaseInit(_) => dest_ty,
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
                    self.emitted_function_return_type(&self.emitted_signature_key(function)?)
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
    pub(super) fn closure_call_expr_for_dest(
        &self,
        callee: &Callee,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if let Callee::Indirect(indirect_callee) = callee
            && let Some(call_text) =
                self.optional_indirect_call_expr_for_dest(indirect_callee, args, dest_ty, true)?
        {
            return Ok(call_text);
        }
        let rendered = self.rendered_call(callee, args)?;
        // A closure body that is itself fallible returns
        // `Result<_, Box<dyn std::error::Error>>`, so a throwing call inside it
        // keeps its `?` and the error stays recoverable for whoever invokes the
        // callback. Only a closure whose Rust signature genuinely cannot carry
        // an error (a non-throwing body, or a generator whose `?` would target
        // the step output) collapses the error into a `panic!`.
        let mut call_text = if rendered.propagates() && !self.body_can_propagate_error() {
            { let receiver = rendered.into_call(); RustExpr::postfix_on(&receiver, format!("{receiver}.unwrap_or_else(|error| smelt_panic_throw(error))")) }
        } else {
            rendered.into_expression()
        };
        call_text = self.wrap_native_async_call_expr(callee, call_text)?;
        // The emitted return, not the declared one. A monomorphized generic
        // callee really evaluates to its declared return with this call site's
        // bindings applied (`shift<T>(SmeltList<T>) -> SmeltList<T>` called with
        // a `SmeltList<f64>` yields `SmeltList<f64>`), while the declared return
        // renders `T` as `SmeltUnknown` in the caller's scope. Asking for the
        // declared type here makes the coercion a no-op whenever the destination
        // is erased, so the concrete value is bound to an erased local and the
        // argument is what rustc reports as mismatched (E0308).
        let source_ty = self.call_emitted_source_ty(callee, args, dest_ty)?;
        self.rendered_at_type(&call_text, source_ty, dest_ty, &self.render_scope())
    }

    /// Bind every argument that reads through a `RefCell` borrow to a local.
    ///
    /// `args` are the call's argument operands, positionally paired with
    /// `rendered_args` (their rendered text, rewritten in place to the bound
    /// name). Returns the `let` bindings to emit before the call, in argument
    /// order, so evaluation order is unchanged. See
    /// [`Self::operand_reads_through_ref_cell`] for which reads hold a guard.
    fn prebind_ref_cell_arguments(
        &self,
        function: &MirFunction,
        args: &[Operand],
        rendered_args: &mut [RustExpr],
    ) -> Result<Vec<String>, EmitError> {
        let mut bindings = Vec::new();
        for (index, (arg, rendered)) in args.iter().zip(rendered_args.iter_mut()).enumerate() {
            // A by-reference argument (`&mut` in-place ABI, borrowed callback)
            // borrows INTO the read; binding it would outlive its temporary.
            let param = function.params.get(index.saturating_add(1)).copied();
            let by_reference = match param {
                Some(param) => {
                    self.parameter_needs_mutable_reference_in(function, param)
                        || (matches!(
                            self.mir.types.get(self.function_local_decl(function, param)?.ty),
                            Some(Type::Function(_))
                        ) && !self.function_parameter_requires_owned_in(function, param)?)
                }
                None => false,
            };
            if !self.operand_reads_through_ref_cell(arg) || by_reference {
                continue;
            }
            let name = RustExpr::opaque(format!("smelt_call_arg_{index}"));
            bindings.push(format!("let {name} = {rendered};"));
            *rendered = name;
        }
        Ok(bindings)
    }

    /// Render a derived `super(..)`: `Self::__smelt_init_Base(this, args..)`.
    ///
    /// The first argument is the receiver (the derived `this`), handed to the
    /// base constructor body re-emitted into this impl as an initializer (see
    /// `crate::base_init`); the rest are the base constructor's own arguments,
    /// each coerced to its parameter's type, and an omitted trailing argument
    /// takes that type's default, as a `Base::new(..)` call does. A throwing
    /// base constructor makes the initializer fallible, so the call carries the
    /// same `?` suffix a throwing `new` does.
    fn base_initializer_call(&self, func: FuncId, args: &[Operand]) -> Result<RenderedCall, EmitError> {
        let function = self
            .mir
            .functions
            .get(id_index(func.0, "function index does not fit usize")?)
            .ok_or_else(|| EmitError::new("base initializer references an unknown function"))?;
        let class = crate::base_init::constructor_class(function)
            .ok_or_else(|| EmitError::new("base initializer callee is not a constructor"))?;
        let Some((receiver, rest)) = args.split_first() else {
            return Err(EmitError::new("base initializer call is missing its receiver"));
        };
        // The initializer is emitted into THIS impl, so its parameters are
        // rendered in the same type-parameter scope as this call: coercing each
        // argument to the declared parameter type here is exactly what the
        // initializer's signature spells (a base type parameter the derived
        // layout erases is erased on both sides). No callee-generic binding
        // applies, unlike a `Base::new(..)` call into the base's own impl.
        let mut rendered_args = vec![self.operand_expr(receiver)?];
        for (index, arg) in rest.iter().enumerate() {
            let Some(param) = function.params.get(index).copied() else {
                rendered_args.push(self.operand_expr(arg)?);
                continue;
            };
            let target_ty = self.function_local_decl(function, param)?.ty;
            rendered_args.push(self.value_at_type(arg, target_ty)?);
        }
        for param in function.params.iter().skip(rest.len()) {
            let target_ty = self.function_local_decl(function, *param)?.ty;
            rendered_args.push(self.default_value(target_ty)?);
        }
        Ok(RenderedCall::propagating_if(
            RustExpr::atom(format!(
                "Self::{}({})",
                crate::base_init::base_initializer_name(self.mir, class)?,
                rendered_args.join(", "),
            )),
            self.call_is_throwing(function),
        ))
    }

    /// Returns the static return type of a call expression.
    pub(super) fn call_source_ty(&self, callee: &Callee) -> Result<TypeId, EmitError> {
        let source_ty = match callee {
            Callee::Builtin(
                BuiltinFn::ConsoleLog { .. } | BuiltinFn::ConsoleWrite | BuiltinFn::ConsoleErrorWrite,
            ) => self.none_ty,
            Callee::Op(_, Some(ty)) => return Ok(*ty),
            Callee::Op(op, None) => return Ok(self.fallible_op_call(op)?.source_ty),
            Callee::Static(func) | Callee::BaseInit(func) => {
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
                if self.has_dynamic_shape(callee_ty) {
                    return self.type_id(Type::Unknown);
                }
                // An erased-rest callable (`(...args: any) => LazyFn`) is the
                // `SmeltErasedFunction` carrier, and `.call(..)` on it answers
                // `SmeltUnknown` whatever its declared return: the call's value
                // is erased and converts to the destination from there
                // (remeda `const fn = lazy(...lazyArgs)`, E0308/E0618).
                if self.callee_uses_erased_call_method(indirect_callee)? {
                    return self.type_id(Type::Unknown);
                }
                let Some(Type::Function(function)) = self.mir.types.get(callee_ty) else {
                    return Err(EmitError::new("indirect call target is not a function"));
                };
                if self.declared_slot_return_is_erased(indirect_callee, function) {
                    return self.type_id(Type::Unknown);
                }
                function.return_ty
            }
        };
        Ok(source_ty)
    }

    /// Returns whether a class/interface callable slot's DECLARED return renders
    /// as the erased carrier while MIR types this call at a concrete return.
    ///
    /// A slot keeps the ABI its declaration gave it. A declared return that is a
    /// union over the class's type parameters (`match: .. => Result<T>`, a union
    /// of tuples of `T`) has no generated enum and renders `SmeltUnknown` in the
    /// slot, even though MIR hands the call the receiver-substituted union
    /// (`Result<string>`, a concrete generated enum). The call really produces
    /// the erased carrier, so callers treat its source type as `Unknown` and the
    /// destination extracts from it instead of receiving it unconverted (E0308
    /// `expected SmeltUnionN, found SmeltUnknown`, Hono's router `match`).
    pub(super) fn declared_slot_return_is_erased(
        &self,
        callee: &Operand,
        function: &FunctionType,
    ) -> bool {
        self.class_field_declared_function_type(callee)
            .is_some_and(|declared| {
                declared.return_ty != function.return_ty
                    && matches!(self.mir.types.get(declared.return_ty), Some(Type::Union(_)))
                    && self.concrete_union_members(declared.return_ty).is_none()
            })
    }


    /// `console.log`.
    fn console_log_call(
        &self,
        absent: AbsentSpelling,
        args: &[Operand],
    ) -> Result<RenderedCall, EmitError> {
        let rendered_args = args
            .iter()
            .map(|arg| self.console_arg(arg, absent))
            .collect::<Result<Vec<_>, _>>()?;
        if rendered_args.is_empty() {
            Ok("{ println!(); }".to_owned().into())
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
            Ok(RustExpr::atom(format!("{{ println!(\"{format}\", {arg_values}); }}")).into())
        }
    }

    /// A direct call of a function this program defines.
    fn static_call(&self, func: FuncId, args: &[Operand]) -> Result<RenderedCall, EmitError> {
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
                        return self.operand_expr(arg);
                    };
                    let target_ty = self.function_local_decl(function, param)?.ty;
                    self.callee_generic_argument_expr(
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
            return Ok(RenderedCall::propagating_if(
                RustExpr::atom(format!("{class_name}::new({arg_values})")),
                self.call_is_throwing(function),
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
                return self.default_value(function.return_ty).map(RenderedCall::from);
            }
            let receiver_text = match receiver {
                Operand::Copy(place) | Operand::Move(place) => self.place_expr(place)?,
                Operand::Const(_) => {
                    return Err(EmitError::new("method receiver cannot be a constant"));
                }
            };
            // `super.method()` reaches the ancestor's copy, not the override
            // the receiver's own method and slot hold.
            let super_copy = match self.mir.types.get(self.operand_ty(receiver)?) {
                Some(Type::Class { name, .. }) if crate::classes::is_super_call(self.mir, *name, func) => {
                    crate::classes::super_copy_name(self.mir, function)
                }
                _ => None,
            };
            let method_name = match &super_copy {
                Some(name) => name.clone(),
                None => sanitize_ident(self.symbol_name(method)?),
            };
            if super_copy.is_none()
                && let Some(field_call) =
                    self.call_virtual_function_field_expr(receiver, &method_name, rest)?
            {
                return Ok(field_call.into());
            }
            let class_type_params = self.callee_class_type_params(function);
            let mut rendered_args = rest
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    let Some(param_index) = index.checked_add(1) else {
                        return Err(EmitError::new("method argument index overflowed"));
                    };
                    let Some(param) = function.params.get(param_index).copied() else {
                        return self.operand_expr(arg);
                    };
                    let target_ty = self.function_local_decl(function, param)?.ty;
                    // A method's parameters carry the SAME ABI as every
                    // other callee's. A parameter the callee mutates in
                    // place is emitted `&mut T`
                    // (`parameter_needs_mutable_reference_in`, the very
                    // predicate that decided the signature), so the
                    // argument is a mutable borrow -- passing a value
                    // was `expected &mut SmeltList<_>, found
                    // SmeltList<_>` at every call to Hono's
                    // `#pushHandlerSets`, 400 of the router slice's 498
                    // errors. Every other callee kind already asks this
                    // question; only this arm never did.
                    if self.parameter_needs_mutable_reference_in(function, param) {
                        return self.mutable_reference_argument_expr(
                            arg,
                            target_ty,
                            Some(&class_type_params),
                        );
                    }
                    // A callback parameter the method does not retain is
                    // emitted `&dyn Fn(..)` (`param_type`), the same
                    // borrowed-callback ABI a free function gets, so the
                    // argument is the same reborrow the free-function
                    // ladders render. Passing the owned `Rc<dyn Fn>` was
                    // `expected &dyn Fn(f64, f64) -> f64, found Rc<..>` at
                    // every call of a method taking a callback it only
                    // invokes (`bag.fold((a, x) => a + x, 0)`).
                    if matches!(self.mir.types.get(target_ty), Some(Type::Function(_)))
                        && !self
                            .function_parameter_requires_owned_in(function, param)
                            .unwrap_or(true)
                    {
                        let callback_ty = self.method_argument_substituted_target(
                            arg,
                            target_ty,
                            &class_type_params,
                        )?;
                        return self.borrowed_function_argument_expr(arg, callback_ty, None);
                    }
                    if let Some(bound_text) = self
                        .receiver_bound_class_param_argument_expr(
                            function, receiver, arg, target_ty,
                        )?
                    {
                        return Ok(bound_text);
                    }
                    self.callee_generic_argument_expr(
                        arg,
                        function,
                        target_ty,
                        &class_type_params,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            // A parameter the call OMITS is padded with its default, the
            // way the static-method and free-function arms already pad
            // theirs: `#pushHandlerSets(sets, node, method, params)`
            // leaves the trailing `params?: Record<string, string>`
            // unwritten, and TypeScript's optional parameter is
            // `Option<T>` here, whose default is `None`. Without this
            // the call was short by one argument (`takes 5 arguments but
            // 4 were supplied`, 19 more of the slice's errors).
            //
            // A by-reference parameter is skipped rather than padded:
            // `&mut T` has no value-shaped default to invent, and such a
            // parameter is only reachable through an explicit argument.
            // Same rule, same reason, as `indirect_call_args_expr`.
            for (index, param) in function
                .params
                .iter()
                .enumerate()
                .skip(rest.len().saturating_add(1))
            {
                if self.parameter_needs_mutable_reference_in(function, *param) {
                    break;
                }
                let target_ty = self.function_local_decl(function, *param)?.ty;
                let _ = index;
                rendered_args.push(self.default_value(target_ty)?);
            }
            // An argument read through a reference class's cell
            // (`this.0.borrow()._path.clone()`) holds its `Ref` guard to
            // the end of the statement, so a method on a reference
            // receiver that writes its own cell panicked "already
            // borrowed" — `this.#addRoute(m, this.#path, h)` in Hono's
            // base constructor. Such arguments are bound first, which
            // drops each guard before the call runs (JavaScript
            // evaluates arguments before the call anyway).
            let prebound = if self.is_reference_class_type(self.operand_ty(receiver)?) {
                self.prebind_ref_cell_arguments(function, rest, &mut rendered_args)?
            } else {
                Vec::new()
            };
            let arg_values = rendered_args.join(", ");
            let call = RustExpr::postfix_on(&receiver_text, format!("{receiver_text}.{method_name}({arg_values})"));
            let call = if prebound.is_empty() {
                call
            } else {
                RustExpr::atom(format!("{{ {} {call} }}", prebound.join(" ")))
            };
            return Ok(RenderedCall::propagating_if(call, self.call_is_throwing(function)));
        }
        if let HirOrigin::ClassStaticMethod { class, method, .. } = function.origin {
            // `Class.staticMethod(args)` lowers to the receiver-free
            // associated function `Class::staticMethod(args)`. Arguments
            // are coerced to the emitted parameter types like any other
            // static call, and missing trailing parameters default.
            let class_name = sanitize_ident(self.symbol_name(class)?);
            let method_name =
                function_item::static_method_rust_name(self.context, function.id, self.symbol_name(method)?);
            let mut rendered_args = args
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    let Some(param) = function.params.get(index).copied() else {
                        return self.operand_expr(arg);
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
            return Ok(RenderedCall::propagating_if(
                RustExpr::atom(format!("{class_name}::{method_name}({arg_values})")),
                self.call_is_throwing(function),
            ));
        }
        self.static_call_positional(args, function)
    }

    /// A call whose callee takes its arguments by position: rest and default parameters filled in.
    fn static_call_positional(
        &self,
        args: &[Operand],
        function: &MirFunction,
    ) -> Result<RenderedCall, EmitError> {
        let rust_function_name = self.function_rust_name(function)?;
        let emitted_params =
            self.emitted_function_param_types(&self.emitted_signature_key(function)?)?;
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
                self.is_unknown_list_type(self.operand_ty(first_arg)?)
            } else {
                false
            };
            if function.rest == Some(0)
                && self.is_unknown_list_type(rest_ty)
                && !single_unknown_list_arg
            {
                let items = args
                    .iter()
                    .map(|arg| self.erase(arg))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ");
                return Ok(RenderedCall::propagating_if(
                    RustExpr::chain(format!("{rust_function_name}(vec![{items}])")),
                    self.call_is_throwing(function),
                ));
            }
        }
        if let Some((rest_index, next_index)) = function
            .params
            .iter()
            .enumerate()
            .find_map(|(index, param)| {
                let ty = self.function_local_decl(function, *param).ok()?.ty;
                self.is_unknown_list_type(ty)
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
                    rendered_args.push(self.borrowed_function_argument_expr(
                        arg, target_ty, None,
                    )?);
                } else if self.parameter_needs_mutable_reference_in(function, param) {
                    rendered_args.push(self.mutable_reference_argument_expr(
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
            rendered_args.push(RustExpr::atom(format!("SmeltList::from(vec![{rest_items}])")));
            for param in function.params.iter().skip(next_index) {
                rendered_args.push(self.default_value(self.local_decl(*param)?.ty)?);
            }
            let arg_values = rendered_args.join(", ");
            return Ok(RenderedCall::propagating_if(
                RustExpr::chain(format!("{rust_function_name}({arg_values})")),
                self.call_is_throwing(function),
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
                rendered_args.push(self.borrowed_default_function_expr(
                    target_ty,
                    monomorphization.as_ref().map(|pinned| &pinned.bindings),
                )?.into_string());
            } else {
                rendered_args.push(self.default_value(target_ty)?.into_string());
            }
        }
        let arg_values = rendered_args.join(", ");
        Ok(RenderedCall::propagating_if(
            RustExpr::chain(format!("{rust_function_name}({arg_values})")),
            self.call_is_throwing(function),
        ))
    }

    /// A call through a function value.
    fn indirect_call(
        &self,
        indirect_callee: &Operand,
        args: &[Operand],
    ) -> Result<RenderedCall, EmitError> {
        // An erased callee (source `unknown`, a union, an erased class)
        // has no static function type, so the concrete callable shape is
        // only known at run time. It is the same boundary `ClosureCall`
        // already routes through `dynamic_callable_dispatch_expr`; the
        // two call forms must agree, or moving a call between the
        // statement and terminator forms changes whether it is emittable
        // at all.
        let callee_ty = self.operand_ty(indirect_callee)?;
        if self.has_dynamic_shape(callee_ty) {
            let callee_text = self.operand_expr(indirect_callee)?;
            let rendered_args = args
                .iter()
                .map(|arg| self.erase(arg))
                .collect::<Result<Vec<_>, EmitError>>()?;
            let args_expr = RustExpr::atom(format!("vec![{}]", rendered_args.join(", ")));
            return Ok(self.dynamic_callable_dispatch_expr(&callee_text, &args_expr, self.prototype_method_not_callable(indirect_callee).as_deref()).into());
        }
        let Some(Type::Function(function)) = self.mir.types.get(callee_ty) else {
            return Err(EmitError::new("indirect call target is not a function"));
        };
        let callee_text = self.operand_expr(indirect_callee)?;
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
            return Ok(RustExpr::atom(format!(
                "({callee_text}).call({})",
                self.erased_call_argument_vector_expr(indirect_callee, args)?
            )).into());
        }
        let declared_abi = self
            .class_field_declared_function_type(indirect_callee)
            .map(|declared| declared.params);
        let rendered_args = self.indirect_call_args_text_with_abi(
            function,
            args,
            declared_abi.as_deref(),
        )?;
        let suffix = if self.function_value_call_is_fallible(function) {
            "?"
        } else {
            ""
        };
        // A callee read out of a `RefCell` — a reference class's
        // function-typed field, or a shared closure capture — renders as
        // `recv.0.borrow().f.clone()`. The `Ref` guard that read creates
        // lives to the END of the enclosing statement, so calling it in
        // place holds the borrow while the callee runs; a class-field
        // arrow's whole purpose is to `borrow_mut()` that same cell, so
        // the call panicked with "already borrowed". Binding the callable
        // in its own `let` drops the guard before the call.
        if self.operand_reads_through_ref_cell(indirect_callee) {
            // CLONED out of the guard, not moved out of it. A shared
            // closure capture renders as `(*cell.borrow())`, whose
            // `Rc<dyn Fn ..>` is not `Copy`, so binding it directly was
            // a move out of a `Ref` deref — E0507, and the es-toolkit
            // probe crate did not compile at all (`flatten`'s recursive
            // capture). Cloning an `Rc` bumps a refcount, the `let`
            // still drops the guard before the call, and a callee text
            // that is already an owned clone (a reference class's
            // function-typed field, `recv.0.borrow().f.clone()`) only
            // pays one more refcount.
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_callable = ::std::clone::Clone::clone(&{callee_text}); (smelt_callable)({rendered_args}){suffix} }}"
            )).into());
        }
        Ok(RenderedCall::propagating_if(
            RustExpr::atom(format!("({callee_text})({rendered_args})")),
            self.function_value_call_is_fallible(function),
        ))
    }
}
