//! Shared callback helpers: return-type checks, the call forms of `.call` and
//! `.apply`, and what a receiver's type declares as a method.

use crate::RestParam;
use crate::lowering::{
    BindingPattern, Body, CaptureMode, ClosureCapture, Expr, ExprKind, FunctionType, HashSet,
    Literal, LocalDecl, ModuleBuilder, Param, SmeltError, Span, Type,
};
use crate::lowering::state::fn_ctx::BodyKind;
use crate::lowering::state::local_scope::ScopeGroups;
use smelt_hir::ErasureReason;
impl ModuleBuilder<'_> {
    /// Validate the inferred callback return type for an array method.
    pub(in crate::lowering) fn require_callback_ty(
        &self,
        actual: smelt_hir::TypeId,
        expected: smelt_hir::TypeId,
        call: &oxc::ast::ast::CallExpression<'_>,
        context: &'static str,
    ) -> Result<(), SmeltError> {
        if actual == expected {
            Ok(())
        } else {
            Err(SmeltError::unsupported(
                self.span(call.span.start, call.span.end),
                format!("{context} callback returns an unsupported type"),
            ))
        }
    }

    /// Whether the class named `name` itself (not its bases) declares
    /// `method` as a method; see [`Self::receiver_declares_method`].
    fn class_symbol_declares_method(&self, name: smelt_hir::Symbol, method: smelt_hir::Symbol) -> bool {
        let item_method = self.class_by_symbol(name).is_some_and(|class| {
            class.methods.iter().any(|item| {
                self.ctx
                    .krate
                    .items
                    .get(usize::try_from(item.0).unwrap_or(usize::MAX))
                    .is_some_and(|item| {
                        matches!(item, smelt_hir::Item::Function(function) if function.name == method)
                    })
            })
        });
        let sidecar_method = self
            .ctx
            .krate
            .names
            .get(name)
            .or_else(|| self.ctx.krate.symbols.get(name))
            .and_then(|class_name| self.classes.methods(class_name))
            .is_some_and(|methods| methods.iter().any(|candidate| candidate.name == method));
        // `type_alias_fields` mixes two producers: genuine object-type
        // aliases (structural closure fields) and class method surfaces
        // predeclared by `predeclare_class_method_fields` for barrel
        // cycles. Only the class-backed entries name real methods; an
        // alias-typed receiver (e.g. remeda's `Funnel.flush`) stores a
        // closure field with no class item behind it, so routing it to
        // `ExprKind::Method` makes MIR method resolution fail. A lowered
        // `Item::TypeAlias` with this symbol identifies the alias case.
        let predeclared_method = self.find_type_alias(name).is_none()
            && self
                .types
                .alias_fields(name)
                .and_then(|fields| fields.iter().find(|field| field.name == method))
                .is_some_and(|field| {
                    matches!(self.ctx.krate.types.get(field.ty), Some(Type::Function(_)))
                });
        item_method || sidecar_method || predeclared_method
    }

    /// Return whether every concrete receiver arm declares a source method.
    ///
    /// Callback migration normally treats structural callable fields as
    /// closure calls. A class method—and a method common to every class arm of
    /// a union—must remain `ExprKind::Method` so MIR can dispatch it through the
    /// concrete class or tagged-union representation.
    ///
    /// The ordinary (non-callback) member-call path applies the same rule, so
    /// this is shared rather than duplicated: see
    /// `static_member_is_concrete_class_method`.
    pub(in crate::lowering) fn receiver_declares_method(
        &self,
        receiver_ty: smelt_hir::TypeId,
        method: smelt_hir::Symbol,
    ) -> bool {
        match self.ctx.krate.types.get(receiver_ty) {
            Some(Type::Union(items)) => {
                !items.is_empty()
                    && items
                        .iter()
                        .all(|item| self.receiver_declares_method(*item, method))
            }
            // An inherited method is the receiver's method too: JavaScript
            // finds it on the prototype chain before `Array.prototype` is ever
            // consulted (`class Nums extends Base {}` where `Base` declares
            // `map`), so the walk follows `extends` to the root.
            Some(Type::Class { name, .. }) => {
                let mut current = Some(*name);
                let mut seen = std::collections::HashSet::new();
                while let Some(class_name) = current
                    && seen.insert(class_name)
                {
                    if self.class_symbol_declares_method(class_name, method) {
                        return true;
                    }
                    current = self.class_by_symbol(class_name).and_then(|class| class.base);
                }
                false
            }
            // A class- or interface-bounded type parameter declares what its
            // bound declares (bounded-generics plan, increment 1), so the call
            // stays an `ExprKind::Method` that MIR dispatches through the bound.
            // An interface counts only for a METHOD signature with a typed
            // result: a function-valued property is still a callable field, and
            // an erased result would have `member_call` drop the call.
            Some(Type::TypeParam { .. }) => {
                let Some(bound) = self.type_param_member_bound(receiver_ty) else {
                    return false;
                };
                if self.receiver_declares_method(bound, method) {
                    return true;
                }
                let Some(Type::Class { name, .. }) = self.ctx.krate.types.get(bound) else {
                    return false;
                };
                self.find_interface(*name).is_some_and(|interface| {
                    interface.methods.iter().any(|signature| {
                        signature.name == method
                            && !matches!(
                                self.ctx.krate.types.get(signature.return_ty),
                                Some(Type::Unknown)
                            )
                    })
                })
            }
            _ => false,
        }
    }

    /// Lower a `Function.prototype.apply` call inside a closure body.
    ///
    /// `fn.apply(thisArg, argsArray)` invokes `fn` with the elements of
    /// `argsArray` as positional arguments. A plain function or closure has no
    /// receiver parameter, so the leading `thisArg` is not passed positionally;
    /// it is installed as the callee's `this` for this one call via `BindThis`
    /// (see [`Self::bind_this_receiver`], which elides the bind for a
    /// null/undefined receiver and for a concrete callable-object callee), and
    /// the trailing array is spread through `ClosureCallSpread`, mirroring the
    /// spread branch of [`Self::callback_call_method_to_body_expr`]. This
    /// generalizes the closure-body method table to the `apply` form the same way
    /// `call` is already supported, without special-casing any particular library
    /// function.
    pub(in crate::lowering) fn callback_apply_method_to_body_expr(
        &mut self,
        receiver: smelt_hir::ExprId,
        _receiver_ty: smelt_hir::TypeId,
        args: &[smelt_hir::ExprId],
        ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        // `fn.apply(thisArg, argsArray)` binds `this` from the LEADING operand.
        // The operand is not a positional argument -- a plain function or
        // closure has no receiver parameter -- but it is not nothing either: it
        // is the receiver the callee's `this` resolves to, installed for the
        // duration of this one call by `ExprKind::BindThis`. Dropping it used to
        // be the whole story, which made `func.apply(this, args)` in a wrapper
        // such as es-toolkit's `ary` silently lose the caller's receiver.
        let callee = args.first().copied().map_or(receiver, |this_arg| {
            self.bind_this_receiver(receiver, this_arg, body, span)
        });
        // `fn.apply(thisArg)` (or `fn.apply()`) forwards no positional
        // arguments, so it is an ordinary zero-argument closure call.
        if args.len() <= 1 {
            return Ok(body.push_expr(Expr {
                kind: ExprKind::ClosureCall {
                    callee,
                    args: Vec::new(),
                },
                ty,
                span,
            }));
        }
        // The trailing operand is the array whose elements become the call
        // arguments; spread it through the packed-argument closure-call ABI.
        let arguments = *args.last().ok_or_else(|| {
            SmeltError::unsupported(span, "callback apply call requires an arguments array")
        })?;
        // A `ClosureCallSpread`'s runtime argument list is a `SmeltList<SmeltUnknown>`,
        // and the emitter resolves `List(Unknown)` out of the type table to render it.
        // Intern it here — at the one site that creates such a call from `apply` — so a
        // program whose only erased value is this argument list still declares the
        // carrier it is about to emit, instead of failing at emit time with
        // "type table does not contain literal operand type Unknown".
        // not a new erasure: names the runtime call-ABI carrier type only.
        let unknown_ty = self.unknown_type();
        self.ctx.krate.types.intern(Type::List(unknown_ty));
        Ok(body.push_expr(Expr {
            kind: ExprKind::ClosureCallSpread {
                callee,
                args: arguments,
            },
            ty,
            span,
        }))
    }

    /// Wrap a callable so `receiver` becomes the `this` its body observes.
    ///
    /// The result keeps the callee's own type: binding a receiver does not
    /// change what the value IS, only what `this` resolves to inside one call,
    /// so a typed function stays a typed function and never degrades to
    /// `SmeltUnknown` at the bind.
    pub(in crate::lowering) fn bind_this_receiver(
        &self,
        callee: smelt_hir::ExprId,
        receiver: smelt_hir::ExprId,
        body: &mut Body,
        span: Span,
    ) -> smelt_hir::ExprId {
        let ty = Self::expr_ty(body, callee);
        // Only a value that is CALLED through a function value can carry a
        // receiver: a plain function (`Type::Function`, in either of its two
        // Rust spellings) or a dynamic call surface (`unknown`, a type
        // parameter, a union). A concrete callable OBJECT -- a generated struct
        // such as `DebouncedFunction<F>` -- dispatches through its own generated
        // call member instead, and has no function value to wrap; binding a
        // receiver onto it would have to change its Rust type. Those keep the
        // receiver-less behaviour they already had rather than being silently
        // retyped, which is a known gap rather than a modeled case.
        if !matches!(
            self.ctx.krate.types.get(ty),
            Some(Type::Function(_) | Type::Unknown | Type::TypeParam { .. } | Type::Union(_))
        ) {
            return callee;
        }
        // `fn.apply(null, args)` / `fn.call(undefined, ..)` supply NO receiver.
        // That is what the channel already answers, so there is nothing to
        // install: eliding the bind keeps the callee's own Rust value (and
        // therefore its call ABI) untouched for the single most common `apply`
        // spelling, instead of wrapping it to install a binding that equals the
        // default.
        if matches!(
            usize::try_from(receiver.0)
                .ok()
                .and_then(|index| body.exprs.get(index))
                .map(|expr| &expr.kind),
            Some(ExprKind::Literal(Literal::None))
        ) {
            return callee;
        }
        body.push_expr(Expr {
            kind: ExprKind::BindThis { callee, receiver },
            ty,
            span,
        })
    }

    /// Coerce a callback-body expression to `String` when it is not already one.
    ///
    /// Erased (`unknown`) and type-param receivers reaching string-only methods
    /// (such as `startsWith`/`endsWith`) are routed through a `TypeAssert` to the
    /// string type, mirroring the direct `string_affix_call` coercion.
    pub(in crate::lowering) fn callback_coerce_to_string(
        value: smelt_hir::ExprId,
        string_ty: smelt_hir::TypeId,
        body: &mut Body,
        span: Span,
    ) -> smelt_hir::ExprId {
        if Self::expr_ty(body, value) == string_ty {
            value
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: string_ty,
                span,
            })
        }
    }


    /// Lower a function-valued object property into a closure expression.
    ///
    /// Object tables such as date-fns `formatters` use `key: function (...) {}`
    /// entries. Contextual object types provide the function parameter and
    /// return types when the function expression omits annotations.
    pub(in crate::lowering) fn function_expression_value(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        span: oxc::span::Span,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "function expressions must have a body",
            ));
        };
        // A NAMED function expression binds its own name inside its own body, and
        // that is how JavaScript spells a self-recursive callback:
        //
        //     mergeWith(cloneDeep(target), source, function mergeRecursively(a, b) {
        //       … return mergeWith(clone(a), b, mergeRecursively); …
        //     })
        //
        // The closure path never bound the name, so the self-reference fell through
        // identifier resolution to the forward-callable fallback and lowered to an
        // EMPTY OBJECT — and calling an empty object collapses to a null callback
        // instead of failing, so the recursion silently did nothing. All eight
        // es-toolkit `toMerged` specs are that one defect.
        //
        // An inline Rust closure cannot express it either: the closure would have
        // to capture the very binding it is being assigned to. So lift it to a
        // module-owned function item, which is what a hand port would write —
        // recursion becomes ordinary `fn` recursion and the value handed to the
        // caller is the same item-closure wrapper a named top-level function
        // reference already produces.
        if let Some(id) = &function.id
            && self.function_expression_is_self_recursive(function, id.name.as_str())
            && !self.function_expression_captures_enclosing_scope(function, id.name.as_str())
        {
            return self.lift_self_recursive_function_expression(
                id.name.as_str(),
                function,
                type_hint,
                span,
                outer_body,
            );
        }
        let hint_function = type_hint.and_then(|ty| {
            let ty = self
                .function_member_type(ty)
                .unwrap_or_else(|| self.type_param_constraint_or_self(ty));
            match self.ctx.krate.types.get(ty) {
                Some(Type::Function(function_ty)) => Some((ty, function_ty.clone())),
                _ => None,
            }
        });
        let return_ty = if let Some(return_type) = &function.return_type {
            self.ts_type_to_hir(&return_type.type_annotation)?
        } else if let Some((_, function_ty)) = &hint_function {
            function_ty.return_ty
        } else {
            self.erase_with(ErasureReason::MissingAnnotation, "function expression return")
        };

        // A postfix update inside this body belongs to this body, not to the
        // deferral list of a declaration this expression initializes (`const
        // bound = function () { … a[k++] … };`), and the enclosing statement's
        // block names nothing in this body: `FnCtx::nested` starts both fresh,
        // so neither crosses the function boundary.
        let ctx = self.nested_fn_ctx(BodyKind::Function, function.r#async, Some(return_ty));
        let scope = ScopeGroups::BINDINGS.with(ScopeGroups::NARROWINGS);
        let (body, params, rest, required_params, captures, forwards_arguments, errors) =
            self.with_nested_body(ctx, scope, |this, frame| {
            let mut body = Body::new(
                None,
                this.span(function_body.span.start, function_body.span.end),
            );
            body.set_generator(function.generator);
            let mut params = Vec::new();
            let mut param_names = HashSet::new();
            let mut errors = Vec::new();
            // A `function` expression that reads its own `arguments` is variadic: the
            // object is the ACTUAL argument list, which a declared-arity signature
            // cannot carry (see `lowering::arguments_forwarding`). Replace the parameter
            // list with one rest list and re-bind each declared name from it.
            // es-toolkit's `partial`/`partialRight`/`flow` spec helpers are this shape.
            let arguments_forwarding = match this.arguments_forwarding_params(function, &mut body) {
                Ok(forwarding) => forwarding,
                Err(error) => {
                    errors.push(error);
                    None
                }
            };
            if let Some(forwarding) = &arguments_forwarding {
                params.extend(forwarding.params.iter().cloned());
                for (name, local) in forwarding.binding_pairs() {
                    param_names.insert(name.clone());
                    this.scope.bind(name, local);
                }
            }
            for (index, param) in function
                .params
                .items
                .iter()
                .enumerate()
                .take(if arguments_forwarding.is_some() { 0 } else { usize::MAX })
            {
                let result = (|| {
                    let ty = if let Some(annotation) = &param.type_annotation {
                        this.ts_type_to_hir(&annotation.type_annotation)?
                    } else if let Some((_, function_ty)) = &hint_function {
                        function_ty.params.get(index).copied().ok_or_else(|| {
                            SmeltError::unsupported(
                                this.span(param.span.start, param.span.end),
                                "function expression has more parameters than its type hint",
                            )
                        })?
                    } else {
                        this.erase_with(ErasureReason::MissingAnnotation, "function expression param")
                    };
                    let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                        return Err(SmeltError::unsupported(
                            this.span(param.span.start, param.span.end),
                            "function expression parameters must be identifiers",
                        ));
                    };
                    let param_name = this.intern_source_name(binding.name.as_str());
                    let local = body.push_local(LocalDecl {
                        name: Some(param_name),
                        ty,
                        mutable: false,
                        span: this.span(binding.span.start, binding.span.end),
                    });
                    body.params.push(local);
                    this.scope.bind(binding.name.to_string(), local);
                    param_names.insert(binding.name.to_string());
                    params.push(Param {
                        name: param_name,
                        local,
                        ty,
                        span: this.span(binding.span.start, binding.span.end),
                    });
                    Ok(())
                })();
                if let Err(error) = result {
                    errors.push(error);
                    break;
                }
            }
            // Lower an optional `...rest` parameter the same way top-level functions
            // and arrow expressions do: resolve its array element type, push a packed
            // list local/param, and record the rest index on the closure so codegen
            // collects the trailing source arguments into one list. Function
            // expressions appear as object property values, returned values, and call
            // arguments, so this keeps rest semantics for all of them.
            let mut rest = arguments_forwarding.as_ref().map(|forwarding| RestParam {
                index: forwarding.rest_index,
                item_ty: forwarding.item_ty,
            });
            if rest.is_none()
                && let Some(rest_param) = &function.params.rest
            {
                let result = (|| {
                    let BindingPattern::BindingIdentifier(binding) = &rest_param.rest.argument else {
                        return Err(SmeltError::unsupported(
                            this.span(rest_param.span.start, rest_param.span.end),
                            "function expression destructured rest parameters need rest binding lowering",
                        ));
                    };
                    let annotated_ty = rest_param
                        .type_annotation
                        .as_ref()
                        .map(|annotation| this.ts_type_to_hir(&annotation.type_annotation))
                        .transpose()?;
                    let rest_index = params.len();
                    let hint_rest_ty = hint_function.as_ref().and_then(|(_, function_ty)| {
                        function_ty
                            .rest
                            .filter(|index| *index == rest_index)
                            .and_then(|index| function_ty.params.get(index).copied())
                    });
                    let source_ty = annotated_ty
                        .or(hint_rest_ty)
                        .unwrap_or_else(|| this.erase_with(ErasureReason::MissingAnnotation, "function expression rest"));
                    let Ok((ty, item_ty)) = this.rest_param_array_type(source_ty) else {
                        return Err(SmeltError::unsupported(
                            this.span(rest_param.span.start, rest_param.span.end),
                            "function expression rest parameter type must be an array type",
                        ));
                    };
                    let param_name = this.intern_source_name(binding.name.as_str());
                    let local = body.push_local(LocalDecl {
                        name: Some(param_name),
                        ty,
                        mutable: false,
                        span: this.span(binding.span.start, binding.span.end),
                    });
                    body.params.push(local);
                    this.scope.bind(binding.name.to_string(), local);
                    param_names.insert(binding.name.to_string());
                    params.push(Param {
                        name: param_name,
                        local,
                        ty,
                        span: this.span(binding.span.start, binding.span.end),
                    });
                    rest = Some(RestParam {
                        index: rest_index,
                        item_ty,
                    });
                    Ok(())
                })();
                if let Err(error) = result {
                    errors.push(error);
                }
            }
            let required_params = function
                .params
                .items
                .iter()
                .position(|param| param.optional || Self::formal_parameter_has_default(param))
                .unwrap_or(function.params.items.len());
            let mut captures = Vec::new();
            if errors.is_empty()
                && let Some(saved_locals) = frame.enclosing_bindings()
            {
                let mut capture_names = Vec::new();
                let function_locals = this.scope.snapshot_bindings();
                this.scope.restore_bindings(saved_locals.clone());
                for statement in &function_body.statements {
                    this.collect_statement_capture_names(statement, &param_names, &mut capture_names);
                }
                this.scope.restore_bindings(function_locals);
                capture_names.sort();
                capture_names.dedup();
                for name in capture_names {
                    let Some(source_local) = saved_locals.lookup(name.as_str()) else {
                        continue;
                    };
                    let Some(source_decl) = usize::try_from(source_local.0)
                        .ok()
                        .and_then(|index| outer_body.locals.get(index))
                    else {
                        continue;
                    };
                    let symbol = source_decl
                        .name
                        .unwrap_or_else(|| this.ctx.krate.symbols.intern(name.as_str()));
                    let body_local = body.push_local(LocalDecl {
                        name: Some(symbol),
                        ty: source_decl.ty,
                        mutable: source_decl.mutable,
                        span: source_decl.span,
                    });
                    this.scope.bind(name, body_local);
                    captures.push(ClosureCapture {
                        source_local,
                        body_local: Some(body_local),
                        symbol,
                        ty: source_decl.ty,
                        mode: CaptureMode::ByRef,
                    });
                }
            }
            let generator_yields = function
                .generator
                .then(|| this.initialize_generator_yield_accumulator(function, &mut body));
            this.fn_ctx.generator = generator_yields;
            // A non-arrow `function` expression introduces its own `arguments`
            // binding, so make the array-like `arguments` object available while
            // lowering the body — mirroring the function-declaration and closure
            // lowering paths that also push the argument arity stack.
            // With the variadic rewrite the whole argument list is the single rest
            // parameter, so the FIXED arity is zero — that is what tells
            // `arguments_object_expression` to read the list rather than the declared
            // parameters.
            this.fn_ctx.arguments_arity = Some(if arguments_forwarding.is_some() {
                    0
                } else {
                    function.params.items.len()
                });
            for statement in crate::lowering::hoisting::hoisted_statements(&function_body.statements) {
                if let Err(error) = this.statement(statement, &mut body) {
                    errors.push(error);
                }
            }
            if let Some(accumulator) = generator_yields {
                Self::push_generator_return(accumulator, function, &mut body);
            }
            if function.r#async {
                body.finish_async();
            }
            let forwards_arguments = arguments_forwarding.is_some();
            (body, params, rest, required_params, captures, forwards_arguments, errors)
        });
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let body_id = self.ctx.krate.push_body(body);
        let rest_index = rest.as_ref().map(|rest| rest.index);
        // A contextual type describes the signature the SOURCE wrote. The
        // variadic rewrite above replaced that signature with a single rest list
        // (`lowering::arguments_forwarding`), so a hint taken here would type the
        // closure at an arity its body no longer has: `(function () { return
        // arguments; })()` was typed `() => unknown` while its lowered body takes
        // `__smelt_arguments`, and the call site then invoked a one-parameter
        // closure with no arguments at all (E0057). The rewritten signature is
        // the only one that describes what was lowered.
        let function_ty = match hint_function {
            Some((ty, _)) if !forwards_arguments => ty,
            _ => self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: params.iter().map(|param| param.ty).collect(),
                rest: rest_index,
                required_params: Some(required_params),
                mutable_params: Vec::new(),
                return_ty,
                is_async: function.r#async,
                may_throw: false,
            })),
        };
        Ok(outer_body.push_expr(Expr {
            kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                params,
                rest: rest_index,
                required_params: Some(required_params),
                return_ty,
                captures,
                body: body_id,
                function_item: None,
                span: self.span(function.span.start, function.span.end),
            }),
            ty: function_ty,
            span: self.span(span.start, span.end),
        }))
    }
}
