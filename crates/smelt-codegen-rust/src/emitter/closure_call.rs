//! Calling and building function values: closure construction, closure calls
//! with fixed or spread arguments, and `this` binding.

use super::*;
use crate::rust::RustExpr;
use smelt_hir::FunctionType;

impl FunctionEmitter<'_> {
    /// Install a receiver on a callable value.
    pub(super) fn bind_this_expr(
        &self,
        callee: &Operand,
        receiver: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let receiver_text = self.erase(receiver)?;
        // A callee that still has its `Type::Function` keeps it: the
        // typed `smelt_bind_this` method returns a `SmeltErasedFunction`
        // so binding a receiver never costs the callable its static
        // shape. Only an already-erased callee goes through the free
        // function, which also handles a callable OBJECT receiver.
        let callee_ty = self.operand_ty(callee)?;
        if let Some(Type::Function(function)) = self.mir.types.get(callee_ty).cloned() {
            // A `Type::Function` has two Rust spellings (see `rust_type`):
            // the erased `SmeltErasedFunction` struct, which carries the
            // bind as a method, and a typed `Rc<dyn Fn(..)>` closure,
            // which does not. The typed form is rebound by wrapping it in
            // an arity-matched closure of the SAME type; the explicit
            // annotation is what lets the parameter types be inferred
            // from the coercion target instead of respelled here.
            if self.is_erased_unknown_rest_function(&function) && !function.may_throw {
                // The MIR type says "erased-rest function", but that is
                // not the same question as "the emitted Rust VALUE is an
                // owned `SmeltErasedFunction`". A borrowed callback
                // handle (`&dyn Fn(..)`, how a function PARAMETER is
                // rendered) and an erased field read on an erased
                // receiver (a `SmeltUnknown`) both carry this type while
                // being something else in Rust, and neither can host the
                // binding: a borrow cannot move into the `'static`
                // callback a bound callable needs, and a `SmeltUnknown`
                // is not the struct the destination expects.
                // `callee_uses_erased_call_method` is the single
                // authority for that distinction, so it decides here
                // too. When it says no, the callee is emitted unchanged
                // and the receiver is not installed -- the behavior
                // every member call had before the channel existed. It
                // is a known gap, recorded rather than turned into a
                // hard error, because erroring would reject programs
                // that transpile today.
                if !self.callee_uses_erased_call_method(callee)? {
                    return self.value_at_type(callee, dest_ty);
                }
                return Ok({ let receiver = self.operand_expr(callee)?; RustExpr::postfix_on(&receiver, format!("{receiver}.smelt_bind_this({receiver_text})")) });
            }
            let params = (0..function.params.len())
                .map(|index| format!("smelt_bound_arg_{index}"))
                .collect::<Vec<_>>()
                .join(", ");
            // A receiver that is still a concrete value (a class
            // instance at an ordinary `obj.field(..)` call) is erased
            // only if the callee actually reads `this`: the wrapper
            // installs a THUNK, and `smelt_this()` forces it. Erasing
            // it eagerly would rebuild the instance's whole erased
            // view — every prototype member included — on every call
            // through a function-valued field.
            let receiver_ty = self.operand_ty(receiver)?;
            let (bind_receiver, install) =
                if matches!(self.mir.types.get(receiver_ty), Some(Type::Unknown)) {
                    (
                        format!("let smelt_bound_this = {receiver_text};"),
                        "smelt_push_this(smelt_bound_this.clone())".to_owned(),
                    )
                } else {
                    let lazy_erase =
                        self.erase_rendered(&RustExpr::chain("smelt_bound_receiver.clone()").ending_in_clone(), receiver_ty)?;
                    (
                        format!(
                            "let smelt_bound_receiver = {}.clone();",
                            self.operand_expr(receiver)?
                        ),
                        format!(
                            "smelt_push_this_lazy({{ let smelt_bound_receiver = smelt_bound_receiver.clone(); ::std::rc::Rc::new(move || {lazy_erase}) }})"
                        ),
                    )
                };
            // A generic class's function-valued field is STORED with
            // its erased declared return (a union mentioning `T` has no
            // concrete spelling at the declaration), while the read is
            // typed with the receiver's arguments substituted. The
            // wrapper has the read's type, so it converts the stored
            // callable's result the way a direct call through the field
            // does (see `declared_slot_return_is_erased`).
            let direct_call = RustExpr::atom(format!("(smelt_bound_callee)({params})"));
            let forwarded = if self.declared_slot_return_is_erased(callee, &function) {
                let unknown_ty = self.type_id(Type::Unknown)?;
                self.rendered_at_type(
                    &direct_call,
                    unknown_ty,
                    function.return_ty,
                    &self.render_scope(),
                )?
            } else {
                direct_call
            };
            return Ok(RustExpr::atom(format!(
                "{{ let smelt_bound_callee = {callee}.clone();                          {bind_receiver}                          let smelt_bound: {ty} = ::std::rc::Rc::new(move |{params}| {{                          let _smelt_this_guard = {install};                          {forwarded} }}); smelt_bound }}",
                callee = self.operand_expr(callee)?,
                // `impl Trait` is illegal in a `let` annotation, and the
                // annotation is precisely what drives parameter inference
                // here, so the boxed `dyn` spelling is required.
                ty = self.rendered_type(callee_ty)?,
            )));
        }
        {
            Ok(RustExpr::chain(format!(
                "smelt_bind_this({}, {receiver_text})",
                self.erase(callee)?
            )))
        }
    }

    /// A closure value at its destination.
    pub(super) fn closure_value_expr(
        &self,
        id: smelt_mir::ClosureId,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if !matches!(
            self.mir.types.get(dest_ty),
            Some(
                Type::Function(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_)
            )
        ) && !self.is_erased_class_type(dest_ty)
        {
            return self.default_value(dest_ty);
        }
        if let Some(closure) = self
            .mir
            .closures
            .get(id_index(id.0, "closure id does not fit usize")?)
            && closure.captures.iter().any(|capture| {
                self.capture_is_borrowed_callback_param(capture.source_local)
                    .unwrap_or(false)
                    || self
                        .capture_symbol_is_borrowed_callback_param(
                            capture.symbol,
                            capture.ty,
                        )
                        .unwrap_or(false)
            })
            && (closure.escapes
                || !matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))))
        {
            return self.default_value(dest_ty);
        }
        // Emit the closure value directly at its destination type.
        //
        // A bare function-item-as-value wrapper carries a stable
        // `function_item_key`. Its erased form keeps reference identity
        // through the ERASE site's per-item accessor (`coercion::erase`);
        // its typed form needs the same, because typed function values
        // are compared with `===` too (`smelt_same_function_identity`).
        if let Some(accessor) = self.function_item_typed_accessor(id, dest_ty)? {
            return Ok(RustExpr::from(accessor));
        }
        Ok(self.closure_expr_for_type(id, dest_ty)?)
    }

    /// A call of a closure value.
    pub(super) fn closure_call_rvalue_expr(
        &self,
        callee: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let callee_ty = self.operand_ty(callee)?;
        if self.has_dynamic_shape(callee_ty)
        {
            // The dynamic dispatch below matches on the ERASED carrier.
            // A union with concrete members renders as its generated
            // enum, not as `SmeltUnknown`, so it crosses the boundary
            // adapter first (`match SmeltUnionN { SmeltUnknown::.. }`
            // was E0308).
            let callee_text = if matches!(self.mir.types.get(callee_ty), Some(Type::Union(_)))
                && self.concrete_union_members(callee_ty).is_some()
            {
                self.erase(callee)?
            } else {
                self.operand_expr(callee)?
            };
            let rendered_args =
                args.iter()
                    .map(|arg| self.erase(arg))
                    .collect::<Result<Vec<_>, EmitError>>()?;
            // A `ClosureCall` carries the exact argument list; spread calls
            // are represented separately as `ClosureCallSpread`, which packs
            // and flattens the runtime vector. Pass these arguments verbatim
            // so a single array argument is delivered as one value instead of
            // being mistaken for a spread and flattened into its elements.
            let smelt_call_args = RustExpr::atom(format!("vec![{}]", rendered_args.join(", ")));
            let call_text =
                self.dynamic_callable_dispatch_expr(&callee_text, &smelt_call_args, self.prototype_method_not_callable(callee).as_deref());
            let unknown_ty = self.type_id(Type::Unknown)?;
            if matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))) {
                return Ok(call_text);
            }
            return self.rendered_at_type(&call_text, unknown_ty, dest_ty, &self.render_scope());
        }
        // An optional call `f?.(args)` whose callee is an absent-able
        // `Option<Rc<dyn Fn(..)>>` must short-circuit to `None`
        // (JavaScript `undefined`) when the callee is missing, rather
        // than substitute a null-returning default callback and call it
        // unconditionally. Render it as `callee.map(|f| f(args))` so an
        // absent callee yields `None` and a present one yields
        // `Some(result)`. The non-throwing case is handled here; a
        // throwing inner function is routed through the terminator call
        // path (see MIR `ClosureCall` lowering) where
        // `optional_indirect_call_expr_for_dest` applies the same rule.
        if let Some(Type::Optional(inner_callee_ty)) = self.mir.types.get(callee_ty)
            && let Some(Type::Function(function)) =
                self.mir.types.get(*inner_callee_ty).cloned()
        {
            let dest_is_optional =
                matches!(self.mir.types.get(dest_ty), Some(Type::Optional(_)));
            let inner_dest_ty = match self.mir.types.get(dest_ty) {
                Some(Type::Optional(inner)) => *inner,
                _ => dest_ty,
            };
            let callee_text = self.operand_expr(callee)?;
            let rendered_args = self.indirect_call_args_expr(&function, args)?;
            let raw_call = if function.may_throw {
                RustExpr::atom(format!(
                    "(smelt_function)({rendered_args}).unwrap_or_else(|error| smelt_panic_throw(error))"
                ))
            } else {
                RustExpr::atom(format!("(smelt_function)({rendered_args})"))
            };
            let coerced_call =
                self.rendered_at_type(&raw_call, function.return_ty, inner_dest_ty, &self.render_scope())?;
            let map_expr =
                RustExpr::postfix_on(&callee_text, format!("{callee_text}.clone().map(|smelt_function| {coerced_call})"));
            if dest_is_optional {
                return Ok(map_expr);
            }
            // The destination is not an `Option` (an erased
            // `SmeltUnknown` seam): an absent callee lowers to
            // `undefined` at the destination type.
            let unknown_ty = self.type_id(Type::Unknown)?;
            let undefined_text =
                self.rendered_at_type(&self.carrier_expr(&RustExpr::undefined()), unknown_ty, dest_ty, &self.render_scope())?;
            return Ok(RustExpr::postfix_on(&map_expr, format!("{map_expr}.unwrap_or({undefined_text})")));
        }
        // A callable-object record IS callable: its synthetic
        // `__smelt_call` slot holds the underlying function. Falling
        // through to `default_value` below silently replaced the whole
        // call with `null` — es-toolkit's `memoize` (whose result type is
        // `F & { cache }`) lost every `memoized(value)` inside a callback
        // that way, with no diagnostic. Route the call through the slot.
        if let Some(call_text) = self.callable_object_slot_call_expr(callee, args, dest_ty)?
        {
            return Ok(call_text);
        }
        if !matches!(self.mir.types.get(callee_ty), Some(Type::Function(_))) {
            return self.default_value(dest_ty);
        }
        self.typed_closure_call_expr(callee, args, dest_ty, callee_ty)
    }

    /// A call of a value whose static type is a function.
    fn typed_closure_call_expr(
        &self,
        callee: &Operand,
        args: &[Operand],
        dest_ty: TypeId,
        callee_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let callee_text = self.operand_expr(callee)?;
        if callee_text.is_unit() {
            return self.default_value(dest_ty);
        }
        let emitted_params = self.emitted_function_param_types(&callee_text)?;
        let local_function = match callee {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
                match self.mir.types.get(self.local_decl(*local)?.ty) {
                    Some(Type::Function(function)) => Some(function),
                    _ => None,
                }
            }
            _ => None,
        };
        let local_params = local_function.map(|function| function.params.as_slice());
        let inferred_function = match self.mir.types.get(callee_ty) {
            Some(Type::Function(function)) => Some(function),
            _ => None,
        };
        let inferred_params = match self.mir.types.get(callee_ty) {
            Some(Type::Function(function)) => function.params.as_slice(),
            _ => &[],
        };
        let params = local_params
            .or(emitted_params.as_deref())
            .unwrap_or(inferred_params);
        let rest_function = local_function.or(inferred_function);
        // The by-reference ABI is the callee's, and a local that received
        // the result of a generic call holds a value whose ABI was decided
        // by the callee's DECLARED signature, not by the instantiated type
        // MIR gave the local. See `emitted_call_result_function_type`.
        // A class's function-typed FIELD answers the same way, from its
        // declaration rather than from the instantiated field type
        // MIR resolved: see `class_field_declared_function_type`.
        let abi_function = match callee {
            Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
                self.emitted_call_result_function_type(*local)
            }
            _ => None,
        }
        .or_else(|| self.class_field_declared_function_type(callee));
        let mut rendered_args = {
            let mut rendered_args = args
                .iter()
                .zip(params.iter())
                .enumerate()
                .map(|(index, (arg, param))| {
                    let text = if rest_function.is_some_and(|function| {
                        function.mutable_params.contains(&index)
                    }) {
                        self.mutable_reference_argument_expr(arg, *param, None)?
                    } else if abi_function.as_ref().map_or_else(
                        || {
                            rest_function.is_some_and(|function| {
                                self.callback_param_is_shared_reference(
                                    function, index, *param,
                                )
                            })
                        },
                        |function| {
                            function.params.get(index).is_some_and(|declared| {
                                self.callback_param_is_shared_reference(
                                    function, index, *declared,
                                )
                            })
                        },
                    ) {
                        // The parameter is `&T`, so pass a reference. Borrowing
                        // the place is the point: this is the per-element
                        // whole-list copy that made array callbacks quadratic.
                        self.shared_reference_argument_expr(arg, *param)?
                    } else {
                        self.closure_value_argument_expr(arg, *param)?
                    };
                    Ok(text)
                })
                .collect::<Result<Vec<_>, _>>()?;
            // A padded argument is a fresh temporary, so a
            // by-shared-reference parameter borrows it in place. Without
            // this the padding is the one argument in the ladder that
            // ignores the callee's ABI (E0308) — es-toolkit's
            // `isEqualWith` calls a six-parameter comparator with two
            // required arguments and four padded ones.
            for (index, param) in params.iter().enumerate().skip(rendered_args.len()) {
                let default_text = self.closure_value_padding_expr(*param)?;
                rendered_args.push(match (abi_function.as_ref(), rest_function) {
                    (Some(function), _) => match function.params.get(index) {
                        Some(declared) => self.callback_call_arg_expr(
                            function,
                            index,
                            *declared,
                            default_text.into_string(),
                        ),
                        None => default_text,
                    },
                    (None, Some(function)) => {
                        self.callback_call_arg_expr(function, index, *param, default_text.into_string())
                    }
                    (None, None) => default_text,
                });
            }
            rendered_args
        };
        if args.is_empty()
            && rendered_args.len() == 1
            && matches!(
                self.mir.types.get(callee_ty),
                Some(Type::Function(function)) if function.params.is_empty()
            )
        {
            rendered_args.clear();
        }
        let args_text = rendered_args.join(", ");
        let callee_is_erased_rest = matches!(
            self.mir.types.get(callee_ty),
            Some(Type::Function(function))
                if self.is_erased_unknown_rest_function(function) && !function.may_throw
        );
        // The choice between the erased `.call(..)` ABI and a direct
        // invocation is answered once, by
        // `callee_uses_erased_call_method` (see its docs for the
        // precedence and why it matters); `rendered_call`'s
        // `Callee::Indirect` arm consults the same helper.
        let uses_erased_call_method = self.callee_uses_erased_call_method(callee)?;
        let call_text = if uses_erased_call_method {
            RustExpr::postfix_on(&callee_text, format!("{callee_text}.call({args_text})"))
        } else if self.callee_is_borrowed_function_handle(callee)? {
            RustExpr::postfix_on(&callee_text, format!("{callee_text}({args_text})"))
        } else if self.operand_reads_through_ref_cell(callee) {
            // The callee was read out of a `RefCell` (a reference
            // class's function-typed field, or a shared closure
            // capture), and that read's guard lives to the end of the
            // enclosing statement. Calling it in place therefore runs
            // the callee while the cell is borrowed, and a class-field
            // arrow's body mutates that same cell ("already borrowed").
            // Binding the callable drops the guard before the call, and
            // it is CLONED rather than moved: a shared capture renders
            // as `(*cell.borrow())`, and an `Rc<dyn Fn ..>` moved out of
            // a `Ref` deref is E0507. Twin of the same binding in
            // `call.rs`.
            RustExpr::atom(format!(
                "{{ let smelt_callable = ::std::clone::Clone::clone(&{callee_text}); (smelt_callable)({args_text}) }}"
            ))
        } else {
            RustExpr::atom(format!("({callee_text})({args_text})"))
        };
        self.closure_call_result_expr(callee, dest_ty, callee_ty, call_text, callee_is_erased_rest, uses_erased_call_method)
    }

    /// The rendered call brought to the destination type.
    fn closure_call_result_expr(
        &self,
        callee: &Operand,
        dest_ty: TypeId,
        callee_ty: TypeId,
        call_text: RustExpr,
        callee_is_erased_rest: bool,
        uses_erased_call_method: bool,
    ) -> Result<RustExpr, EmitError> {
        let (source_ty, rendered_call_text) = match self.mir.types.get(callee_ty) {
            Some(Type::Function(function)) => {
                let returns_future = matches!(
                    self.mir.types.get(function.return_ty),
                    Some(Type::Future(_))
                );
                // A throwing callee invoked in statement position
                // propagates its error with `?` when the enclosing
                // emitted body itself returns a `Result` — a recoverable
                // JavaScript exception must stay recoverable. The
                // `panic!` is kept only where the surrounding Rust
                // signature genuinely cannot carry an error (a
                // non-throwing body, or a generator whose `?` would
                // target the wrong output type).
                let throwing_call_text = if function.may_throw && !returns_future {
                    if self.body_can_propagate_error() {
                        RustExpr::atom_on(&call_text, format!("{call_text}?"))
                    } else {
                        RustExpr::postfix_on(&call_text, format!(
                            "{call_text}.unwrap_or_else(|error| smelt_panic_throw(error))"
                        ))
                    }
                } else {
                    call_text
                };
                // The fully-erased `SmeltErasedFunction::call` ABI always
                // yields a bare `SmeltUnknown`, regardless of the callee's
                // declared return type. Coercing the call result from the
                // declared `return_ty` (e.g. `T | undefined`, which lowers
                // to `Option<SmeltUnknown>`) would suppress the wrap the
                // destination needs, so treat the erased-rest call's source
                // type as `Unknown` and let `rendered_at_type` inject the
                // correct `Some(..)`/extraction at the assignment seam.
                // Only that `.call(..)` ABI erases the result: a callee
                // of the same shape held as a borrowed `&dyn Fn` handle
                // (a callback parameter, a borrowed capture) is invoked
                // directly and returns its declared `return_ty`, so
                // `(...args: any[]) => number` / `Callable[..., int]`
                // yields the `f64`/`i64` itself, which must not be
                // re-extracted from a `SmeltUnknown` (E0308).
                let source_ty = if (callee_is_erased_rest && uses_erased_call_method)
                    || self.declared_slot_return_is_erased(callee, function)
                {
                    self.type_id(Type::Unknown)?
                } else {
                    function.return_ty
                };
                (source_ty, throwing_call_text)
            }
            _ => (dest_ty, call_text),
        };
        // An erased call result bound to a function-typed local is
        // extracted to that function like any other typed destination:
        // every later use (a call, `Object.assign(fn, ..)`, an erasure)
        // renders the local as the typed `Rc<dyn Fn>` its MIR type
        // says. Leaving the bare `SmeltUnknown` in it was E0308/E0618
        // at each of those uses (remeda `const fn = lazy(...lazyArgs)`).
        if callee_is_erased_rest && self.mir.types.get(dest_ty) == Some(&Type::None) {
            return Ok(RustExpr::atom(format!("{{ {rendered_call_text}; () }}")));
        }
        self.rendered_at_type(&rendered_call_text, source_ty, dest_ty, &self.render_scope())
    }

    /// A call of a closure value with spread arguments.
    pub(super) fn closure_call_spread_expr(
        &self,
        callee: &Operand,
        args: &Operand,
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let callee_text = self.operand_expr(callee)?;
        let unknown_ty = self.type_id(Type::Unknown)?;
        let args_ty = self.type_id(Type::List(unknown_ty))?;
        let args_text = self
            .inline_list_concat_operand_expr(args)?
            .unwrap_or(self.value_at_type(args, args_ty)?);
        if let Some(Type::Function(function)) = self.mir.types.get(self.operand_ty(callee)?)
        {
            let callee_is_erased_rest =
                self.is_erased_unknown_rest_function(function) && !function.may_throw;
            // A typed callee with leading positional parameters before its
            // rest cannot receive the packed spread list as a single
            // argument; redistribute the list into `(positionals…,
            // rest_list)` read from an in-scope `smelt_spread_args`.
            let split_args = if callee_is_erased_rest {
                None
            } else {
                self.spread_leading_positional_call_args_expr(function)?
            };
            // The argument list handed to the callee: either the split
            // positional+rest text (reads `smelt_spread_args`) or the
            // packed list verbatim.
            let inner_args = split_args.as_deref().unwrap_or(&args_text);
            // See the `ClosureCall` arm: the ABI question is answered by
            // the shared `callee_uses_erased_call_method` helper.
            let uses_erased_call_method = self.callee_uses_erased_call_method(callee)?;
            let inner_call = if uses_erased_call_method {
                RustExpr::postfix_on(&callee_text, format!("{callee_text}.call({inner_args})"))
            } else if self.callee_is_borrowed_function_handle(callee)? {
                RustExpr::postfix_on(&callee_text, format!("{callee_text}({inner_args})"))
            } else {
                RustExpr::atom(format!("({callee_text})({inner_args})"))
            };
            // When the arguments were split, bind the packed list once so
            // the positional/rest reads above resolve; otherwise the call
            // stands alone.
            let call_text = if split_args.is_some() {
                RustExpr::atom(format!("{{ let smelt_spread_args = {args_text}; {inner_call} }}"))
            } else {
                inner_call
            };
            if callee_is_erased_rest && self.mir.types.get(dest_ty) == Some(&Type::None) {
                return Ok(RustExpr::atom(format!("{{ {call_text}; () }}")));
            }
            // Same rule as the `ClosureCall` arm above, for the same
            // reason: the erased ABI -- `SmeltErasedFunction::call`, or the
            // borrowed `dyn Fn(SmeltList<SmeltUnknown>) -> SmeltUnknown`
            // handle it is emitted as -- always yields a bare
            // `SmeltUnknown`, whatever the callee's declared return type
            // says. Coercing from the declared `return_ty` emits an
            // identity conversion and leaves a `SmeltUnknown` assigned to,
            // say, a `bool` destination: an E0308 the moment a spread call
            // reaches such a callee with a non-`unknown` return, as
            // es-toolkit's `cond` does with `predicate.apply(this, args)`
            // on a `(...args: any[]) => boolean`.
            //
            // The condition keys on the ABI actually chosen for
            // `inner_call` above, not just on the erased-rest shape, so the
            // two cannot drift: ANY call rendered as `.call(..)` returns the
            // erased carrier and needs the same correction.
            let source_ty = if uses_erased_call_method || callee_is_erased_rest {
                unknown_ty
            } else {
                function.return_ty
            };
            return self.rendered_at_type(&call_text, source_ty, dest_ty, &self.render_scope());
        }
        // The runtime dispatch snippet matches the callee over
        // `SmeltUnknown` discriminants, so the callee value must be the
        // bare erased carrier. A callee typed as `Optional<..>` or a
        // concrete union renders as `Option<SmeltUnknown>` /
        // `SmeltUnion..`, not `SmeltUnknown`; erase it to the runtime
        // carrier first (flow can narrow an optional callable to a
        // definitely-present function, and this unwraps it) so the match
        // sees the shape it expects instead of failing to type-check
        // (E0308). An already-`Unknown` callee coerces to itself.
        let erased_callee =
            self.rendered_at_type(&callee_text, self.operand_ty(callee)?, unknown_ty, &self.render_scope())?;
        let call_text = self.dynamic_callable_dispatch_expr(&erased_callee, &args_text, self.prototype_method_not_callable(callee).as_deref());
        if matches!(self.mir.types.get(dest_ty), Some(Type::Function(_))) {
            return Ok(call_text);
        }
        self.rendered_at_type(&call_text, unknown_ty, dest_ty, &self.render_scope())
    }

    /// Converts an awaited future operand without cloning it.
    pub(super) fn await_operand_expr(&self, operand: &Operand) -> Result<RustExpr, EmitError> {
        match operand {
            Operand::Copy(place) | Operand::Move(place) => self.place_expr(place),
            Operand::Const(_) => self.operand_expr(operand),
        }
    }

    /// Emits Rust for selecting a function value from a static string-keyed table.
    pub(super) fn function_table_lookup_expr(
        &self,
        key: &Operand,
        cases: &[(String, Operand)],
        dest_ty: TypeId,
    ) -> Result<RustExpr, EmitError> {
        let key_text = self.operand_expr(key)?;
        let cases_text = cases
            .iter()
            .map(|(case_key, case)| {
                let case_text = self.value_at_type(case, dest_ty)?;
                Ok(format!("{case_key:?} => {case_text}"))
            })
            .collect::<Result<Vec<_>, EmitError>>()?
            .join(", ");
        Ok(RustExpr::atom(format!(
            "{{ let __smelt_function_key = {key_text}; match __smelt_function_key.as_str() {{ {cases_text}, _ => panic!(\"unknown function table key: {{}}\", __smelt_function_key) }} }}"
        )))
    }

    /// Render call-argument text for a spread across a leading-positional + rest
    /// signature, binding the packed list to `smelt_spread_args`.
    ///
    /// A JavaScript call like `callee(data, ...extraArgs)` lowers to a
    /// `ClosureCallSpread` whose runtime argument list is the concatenation
    /// `[data, ...extraArgs]`. When the typed callee declares leading positional
    /// parameters before its rest (`(data: T, ...rest: U[]) => …`, i.e. `rest`
    /// starts after index 0), the packed list must be redistributed: the first
    /// `rest` elements fill the positional parameters and the remainder becomes
    /// the rest `SmeltList`. Returns `None` when the callee has no leading
    /// positional before the rest (index-0 rest is handled by the plain spread
    /// path) or when the shape does not match `[positionals…, rest_list]`.
    ///
    /// The returned string is the comma-separated argument list that reads from
    /// an in-scope `smelt_spread_args` binding; the caller wraps the whole call
    /// in a block that first binds `smelt_spread_args` to the packed list.
    fn spread_leading_positional_call_args_expr(
        &self,
        function: &FunctionType,
    ) -> Result<Option<RustExpr>, EmitError> {
        let Some(rest_index) = function.rest else {
            return Ok(None);
        };
        if rest_index == 0 {
            return Ok(None);
        }
        // The rest parameter is the last declared parameter; everything before it
        // is a leading positional the packed list must supply by index.
        let Some(expected_params_len) = rest_index.checked_add(1) else {
            return Ok(None);
        };
        if function.params.len() != expected_params_len {
            return Ok(None);
        }
        let Some((rest_param, positional_params)) = function.params.split_last() else {
            return Ok(None);
        };
        let Some(Type::List(rest_item)) = self.mir.types.get(*rest_param) else {
            return Ok(None);
        };
        let unknown_ty = self.type_id(Type::Unknown)?;
        let rendered_capacity = positional_params
            .len()
            .checked_add(1)
            .ok_or_else(|| EmitError::new("spread call argument count overflowed usize"))?;
        let mut rendered = Vec::with_capacity(rendered_capacity);
        for (index, param) in positional_params.iter().enumerate() {
            // Each positional reads the erased element at its index (absent
            // arguments become `undefined`, matching JS) and coerces to the
            // declared parameter type.
            let element = RustExpr::chain(self.carrier_format(format_args!(
                "smelt_spread_args.get({index}).cloned().unwrap_or(SmeltUnknown::Undefined)"
            )));
            let coerced = self.rendered_at_type(&element, unknown_ty, *param, &self.render_scope())?;
            rendered.push(self.callback_call_arg_expr(function, index, *param, coerced.into_string()));
        }
        // The rest parameter collects the remaining elements as a fresh
        // `SmeltList`; coerce each element to the rest item type when needed.
        let rest_text = if self.mir.types.get(*rest_item) == Some(&Type::Unknown) {
            RustExpr::atom(format!(
                "SmeltList::from(smelt_spread_args.iter().skip({rest_index}).cloned().collect::<Vec<_>>())"
            ))
        } else {
            let item_text = self.rendered_at_type(&RustExpr::path("value"), unknown_ty, *rest_item, &self.render_scope())?;
            RustExpr::atom(format!(
                "SmeltList::from(smelt_spread_args.iter().skip({rest_index}).cloned().map(|value| {item_text}).collect::<Vec<_>>())"
            ))
        };
        rendered.push(rest_text);
        Ok(Some(RustExpr::from(rendered.join(", "))))
    }

    /// Emits a field read against a named in-scope receiver value.
    /// Render `arg` passed to a closure VALUE's `param`.
    ///
    /// A closure value (`Rc<dyn Fn ..>`) is never generic, so its own signature
    /// fixes every argument's type, and an absent argument (`undefined` passed
    /// or padded for an optional parameter) is a bare `None` — what a hand port
    /// writes. Spelling `None::<T>` from the CALLER's view of the signature
    /// breaks whenever that view is less precise than the callee's: a callable
    /// interface's signature predeclared for an import cycle is lowered before
    /// an alias it names can resolve (`headers?: HeaderRecord`, whose
    /// `BaseMime` is computed from a const object), while the slot's real type
    /// is the precise one.
    pub(super) fn closure_value_argument_expr(
        &self,
        arg: &Operand,
        param: TypeId,
    ) -> Result<RustExpr, EmitError> {
        if matches!(arg, Operand::Const(Constant::Undefined | Constant::None))
            && matches!(self.mir.types.get(param), Some(Type::Optional(_)))
        {
            return Ok(RustExpr::path("None"));
        }
        self.value_at_type(arg, param)
    }

    /// The padding for an omitted argument to a closure value's `param`; see
    /// [`Self::closure_value_argument_expr`].
    pub(super) fn closure_value_padding_expr(&self, param: TypeId) -> Result<RustExpr, EmitError> {
        if matches!(self.mir.types.get(param), Some(Type::Optional(_))) {
            return Ok(RustExpr::path("None"));
        }
        self.default_value(param)
    }
}
