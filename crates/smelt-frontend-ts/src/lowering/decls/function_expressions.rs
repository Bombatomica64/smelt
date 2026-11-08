//! Function expressions lowered to items: the item for a `function () {}`
//! value, its free names and captures, and self-recursive function expressions
//! lifted to a reserved slot.

use std::collections::HashSet;
use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::state::fn_ctx::BodyKind;
use crate::lowering::state::local_scope::ScopeGroups;
use oxc::ast::ast::BindingPattern;
use smelt_hir::{Body, ErasureReason, Function, FunctionOwner, Item, LocalDecl, Param, Type};

impl ModuleBuilder<'_> {
    /// Lower a synthetic object-table function while preserving exact key spelling.
    pub(in crate::lowering) fn function_expression_item_with_source_name(
        &mut self,
        name_text: &str,
        function: &oxc::ast::ast::Function<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        self.function_expression_item_with_receiver(name_text, function, type_hint, None)
    }

    /// Lift a function expression, optionally binding it as a class method.
    pub(in crate::lowering) fn function_expression_item_with_receiver(
        &mut self,
        name_text: &str,
        function: &oxc::ast::ast::Function<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        receiver: Option<(smelt_hir::Symbol, smelt_hir::TypeId)>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        self.function_expression_item_into_slot(name_text, function, type_hint, receiver, None)
    }

    /// Lift a function expression, writing it into an already-reserved item slot.
    ///
    /// `slot` is the reservation made *before* the body is lowered so that the
    /// body can already reference the item — the mechanism a self-recursive named
    /// function expression needs, and the same one `function_declaration` uses for
    /// hoisted local declarations (`LocalScope::function_item`). With `slot` as
    /// `None` this pushes a fresh item, which is the ordinary lift.
    pub(in crate::lowering) fn function_expression_item_into_slot(
        &mut self,
        name_text: &str,
        function: &oxc::ast::ast::Function<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        receiver: Option<(smelt_hir::Symbol, smelt_hir::TypeId)>,
        slot: Option<smelt_hir::ItemId>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "function expressions must have a body",
            ));
        };
        if function.params.rest.is_some() {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "object namespace method rest parameters are not lowered yet",
            ));
        }

        let name = self.intern_exact_source_name(name_text);
        let (type_params, hinted_function, mut return_ty, body, params, errors) = self.with_type_parameter_scope(function.type_parameters.as_deref(), |this, type_params| {
            let hinted_function =
                type_hint.and_then(|ty| match this.ctx.krate.types.get(ty).cloned() {
                    Some(Type::Function(function_ty)) => Some(function_ty),
                    _ => None,
                });
            let return_ty = if let Some(return_type) = &function.return_type {
                match this.ts_type_to_hir(&return_type.type_annotation) {
                    Ok(value) => value,
                    Err(error) => {
                        return Err(error);
                    }
                }
            } else if let Some(function_ty) = &hinted_function {
                function_ty.return_ty
            } else {
                this.function_return_type_or_overload(function, name_text)
                    .unwrap_or_else(|_| this.erase(ErasureReason::MissingAnnotation))
            };
            if function.r#async
                && !function.generator
                && !matches!(this.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
            {
                return Err(SmeltError::unsupported(
                    this.span(function.span.start, function.span.end),
                    "async functions must declare a Promise<T> return type",
                ));
            }

            let mut ctx = this.nested_fn_ctx(BodyKind::Function, function.r#async, Some(return_ty));
            // A lifted specialization keeps its concrete `this` through `this as T`.
            ctx.preserve_receiver = receiver.is_some();
            let scope = ScopeGroups::BINDINGS.with(ScopeGroups::NARROWINGS);
            let (body, params, errors) = this.with_nested_body(ctx, scope, |this, _| {
                let mut body = Body::new(
                    None,
                    this.span(function_body.span.start, function_body.span.end),
                );
                body.set_generator(function.generator);
                let mut params = Vec::new();
                let mut errors = Vec::new();
                if let Some((_, receiver_ty)) = receiver {
                    let this_name = this.intern_source_name("this");
                    let this_local = body.push_local(LocalDecl {
                        name: Some(this_name),
                        ty: receiver_ty,
                        mutable: true,
                        span: this.span(function.span.start, function.span.start),
                    });
                    body.params.push(this_local);
                    this.scope.bind("this".to_owned(), this_local);
                    params.push(Param {
                        name: this_name,
                        local: this_local,
                        ty: receiver_ty,
                        span: this.span(function.span.start, function.span.start),
                    });
                }

                for (index, param) in function.params.items.iter().enumerate() {
                    let result = (|| {
                        let ty = if let Some(annotation) = &param.type_annotation {
                            this.ts_type_to_hir(&annotation.type_annotation)?
                        } else if let Some(function_ty) = &hinted_function {
                            function_ty.params.get(index).copied().ok_or_else(|| {
                                SmeltError::unsupported(
                                    this.span(param.span.start, param.span.end),
                                    "function expression parameter is missing from contextual callable type",
                                )
                            })?
                        } else {
                            return Err(SmeltError::unsupported(
                                this.span(param.span.start, param.span.end),
                                "function expression parameters must have explicit type annotations",
                            ));
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
                        params.push(Param {
                            name: param_name,
                            local,
                            ty,
                            span: this.span(binding.span.start, binding.span.end),
                        });
                        debug_assert_eq!(
                            usize::try_from(local.0).unwrap_or(usize::MAX),
                            index + usize::from(receiver.is_some()),
                            "function expression parameters should be added in source order",
                        );
                        Ok(())
                    })();
                    if let Err(error) = result {
                        errors.push(error);
                        break;
                    }
                }
                let receiver_count = usize::from(receiver.is_some());
                let source_param_count = params.len().saturating_sub(receiver_count);
                if errors.is_empty()
                    && let Some(function_ty) = &hinted_function
                    && function_ty.params.len() > source_param_count
                {
                    for (index, ty) in function_ty
                        .params
                        .iter()
                        .copied()
                        .enumerate()
                        .skip(source_param_count)
                    {
                        let param_name = this.intern_source_name(&format!("__unused{index}"));
                        let local = body.push_local(LocalDecl {
                            name: Some(param_name),
                            ty,
                            mutable: false,
                            span: this.span(function.span.start, function.span.end),
                        });
                        body.params.push(local);
                        params.push(Param {
                            name: param_name,
                            local,
                            ty,
                            span: this.span(function.span.start, function.span.end),
                        });
                    }
                }
                let generator_yields = function
                    .generator
                    .then(|| this.initialize_generator_yield_accumulator(function, &mut body));
                this.fn_ctx.generator = generator_yields;
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

                (body, params, errors)
            });
            Ok((type_params, hinted_function, return_ty, body, params, errors))
        })?;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        if function.generator && function.return_type.is_none() && hinted_function.is_none() {
            return_ty = self.inferred_generator_type(&body, function.r#async);
        }
        if function.generator && (function.return_type.is_some() || hinted_function.is_some()) {
            return_ty = self.generator_type_with_fallthrough(&body, return_ty);
        }

        let body_id = self.ctx.krate.push_body(body);
        let lifted = Item::Function(Function {
            name,
            span: self.span(function.span.start, function.span.end),
            type_params,
            params,
            rest: None,
            required_params: None,
            return_ty,
            is_async: function.r#async,
            is_test: false,
            body: Some(body_id),
            owner: receiver.map_or(FunctionOwner::Module, |(class, _)| {
                FunctionOwner::ClassMethod {
                    class,
                    method: name,
                }
            }),
        });
        // A reserved slot is overwritten in place so that references lowered
        // during the body above — a self-recursive call resolving through the
        // reservation — keep pointing at this item rather than at a second copy.
        if let Some(slot) = slot {
            let index = usize::try_from(slot.0).unwrap_or(usize::MAX);
            if let Some(existing) = self.ctx.krate.items.get_mut(index) {
                *existing = lifted;
                return Ok(slot);
            }
        }
        Ok(self.ctx.krate.push_item(lifted))
    }

    /// Collect the free (unbound-by-this-function) names referenced in a function
    /// expression's body.
    ///
    /// Purely an AST walk over `collect_statement_capture_names`, so it can run
    /// *before* the body is lowered — which is what the self-recursion decision
    /// needs. Parameter names (including the rest parameter) are excluded, so what
    /// comes back is exactly the set of names the body expects its surroundings to
    /// supply, plus the function's own name when it recurses.
    fn function_expression_free_names(
        &self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> Vec<String> {
        let mut param_names = HashSet::new();
        for param in &function.params.items {
            if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                param_names.insert(binding.name.as_str().to_owned());
            }
        }
        if let Some(rest) = &function.params.rest
            && let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument
        {
            param_names.insert(binding.name.as_str().to_owned());
        }
        let mut free_names = Vec::new();
        if let Some(body) = &function.body {
            for statement in &body.statements {
                self.collect_statement_capture_names(statement, &param_names, &mut free_names);
            }
        }
        free_names.sort();
        free_names.dedup();
        free_names
    }

    /// Return whether a named function expression's body references its own name.
    ///
    /// `collect_statement_capture_names` reports only names that are ALREADY bound
    /// in the enclosing scope — it is a capture collector, not a free-variable
    /// collector — and a named function expression's own name is by definition not
    /// bound out there. So bind it to a probe for the duration of the scan and ask
    /// whether the walk reports it. That reuses the one visitor that already knows
    /// every expression and statement shape instead of adding a second one that
    /// would drift from it.
    pub(in crate::lowering) fn function_expression_is_self_recursive(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        name_text: &str,
    ) -> bool {
        let saved_bindings = self.scope.snapshot_bindings();
        self.scope
            .bind(name_text.to_owned(), smelt_hir::LocalId(u32::MAX));
        let free_names = self.function_expression_free_names(function);
        self.scope.restore_bindings(saved_bindings);
        free_names.iter().any(|name| name == name_text)
    }

    /// Return whether a function expression's body reads any binding that lives in
    /// the *enclosing* body rather than at module scope.
    ///
    /// The lift target is a module-owned item, which has no access to the
    /// surrounding body's locals, so a capturing self-recursive function expression
    /// cannot be lifted as-is and keeps the closure path. Its own name never counts
    /// as a capture — resolving that name is the point of the lift.
    pub(in crate::lowering) fn function_expression_captures_enclosing_scope(
        &self,
        function: &oxc::ast::ast::Function<'_>,
        name_text: &str,
    ) -> bool {
        self.function_expression_free_names(function)
            .iter()
            .any(|name| name != name_text && self.scope.lookup(name.as_str()).is_some())
    }

    /// Lift a self-recursive named function expression to a module function item.
    ///
    /// The item slot is **reserved before the body is lowered** and the source name
    /// is bound to it for exactly the duration of that lowering, so the body's
    /// self-references resolve through the ordinary
    /// `items` -> `Item::Function` -> `item_function_closure_expression` path while
    /// the name stays invisible to the rest of the module — which is precisely
    /// JavaScript's scoping rule for a named function expression.
    ///
    /// The reservation carries the signature the real lowering will derive
    /// (annotation, else the contextual callable hint, else `unknown`), because a
    /// self-reference lowered against the reservation reads its parameter types.
    /// Getting that wrong would coerce the recursive call's arguments against a
    /// signature the finished item does not have.
    pub(in crate::lowering) fn lift_self_recursive_function_expression(
        &mut self,
        name_text: &str,
        function: &oxc::ast::ast::Function<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        span: oxc::span::Span,
        outer_body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let slot = self.reserve_self_recursive_function_slot(name_text, function, type_hint);
        let previous = self.items.insert(name_text.to_owned(), slot);
        let lifted = self.function_expression_item_into_slot(
            name_text,
            function,
            type_hint,
            None,
            Some(slot),
        );
        match previous {
            Some(item) => {
                self.items.insert(name_text.to_owned(), item);
            }
            None => {
                self.items.remove(name_text);
            }
        }
        let item = lifted?;
        self.item_function_closure_expression(item, span.start, span.end, outer_body)
    }

    /// Reserve the item slot a self-recursive function expression will occupy.
    ///
    /// The reservation is body-less and is overwritten by
    /// `function_expression_item_into_slot`; only its *signature* is read, by
    /// self-references lowered inside the body. Parameter and return types are
    /// resolved by the same precedence the real lift uses, and a parameter that
    /// resolves to neither an annotation nor a hint gets `unknown` here — the real
    /// lift raises the named error for that shape, at which point this reservation
    /// is discarded along with the lift.
    fn reserve_self_recursive_function_slot(
        &mut self,
        name_text: &str,
        function: &oxc::ast::ast::Function<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> smelt_hir::ItemId {
        let hinted = type_hint.and_then(|ty| match self.ctx.krate.types.get(ty).cloned() {
            Some(Type::Function(function_ty)) => Some(function_ty),
            _ => None,
        });
        // not a new erasure: a provisional, body-less slot signature that the
        // real lift overwrites; the lift decides (and records) these types.
        let unknown = self.unknown_type();
        let span = self.span(function.span.start, function.span.end);
        let mut params = Vec::new();
        for (index, param) in function.params.items.iter().enumerate() {
            let ty = param
                .type_annotation
                .as_ref()
                .and_then(|annotation| self.ts_type_to_hir(&annotation.type_annotation).ok())
                .or_else(|| {
                    hinted
                        .as_ref()
                        .and_then(|function_ty| function_ty.params.get(index).copied())
                })
                .unwrap_or(unknown);
            let name = match &param.pattern {
                BindingPattern::BindingIdentifier(binding) => {
                    self.intern_source_name(binding.name.as_str())
                }
                _ => self.intern_source_name(&format!("__unused{index}")),
            };
            params.push(Param {
                name,
                local: smelt_hir::LocalId(u32::try_from(index).unwrap_or(0)),
                ty,
                span,
            });
        }
        let return_ty = function
            .return_type
            .as_ref()
            .and_then(|annotation| self.ts_type_to_hir(&annotation.type_annotation).ok())
            .or_else(|| hinted.as_ref().map(|function_ty| function_ty.return_ty))
            .unwrap_or(unknown);
        let name = self.intern_exact_source_name(name_text);
        self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            type_params: Vec::new(),
            params,
            rest: None,
            required_params: None,
            return_ty,
            is_async: function.r#async,
            is_test: false,
            body: None,
            owner: FunctionOwner::Module,
        }))
    }
}
