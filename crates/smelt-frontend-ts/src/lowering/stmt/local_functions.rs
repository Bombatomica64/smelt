//! Functions declared inside a body: local arrow consts and nested `function`
//! declarations, which become locals holding closures.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use crate::lowering::RestParam;
use crate::lowering::body_arena::BodyArena;
use crate::lowering::erasure::NotNewErasure;
use crate::lowering::state::fn_ctx::BodyKind;
use oxc::ast::ast::BindingPattern;
use smelt_hir::{
    Body, CaptureMode, ClosureCapture, ErasureReason, Expr, ExprKind, FunctionType, LocalDecl,
    Param, Pattern, Span, Stmt, Type,
};
use std::collections::HashSet;

impl ModuleBuilder<'_> {
    /// Lower a local arrow function variable as a non-escaping closure value.
    pub(in crate::lowering) fn local_arrow_callback_declaration(
        &mut self,
        name: &str,
        start: u32,
        end: u32,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        self.push_function_type_parameter_scope(
            arrow.type_parameters.as_deref(),
            crate::lowering::ty::element_params::FunctionSignature::of_arrow(arrow),
        )?;
        let saved_outer_locals = self.scope.snapshot_bindings();
        let result = (|| {
            let contextual_function = self.contextual_function_type(type_hint);
            let mut params =
                self.arrow_callback_param_types_with_hint(arrow, contextual_function.as_ref())?;
            for (param, ty) in arrow.params.items.iter().zip(params.iter_mut()) {
                if param.optional
                    && !matches!(self.ctx.krate.types.get(*ty), Some(Type::Optional(_)))
                {
                    *ty = self.ctx.krate.types.intern(Type::Optional(*ty));
                }
            }
            let rest = arrow.params.rest.as_ref().and_then(|_rest| {
                let index = params.len().checked_sub(1)?;
                let param_ty = params.get(index).copied()?;
                let item_ty = match self.ctx.krate.types.get(param_ty) {
                    Some(Type::List(item_ty)) => *item_ty,
                    _ => param_ty,
                };
                Some(RestParam { index, item_ty })
            });
            let required_params = arrow
                .params
                .items
                .iter()
                .position(|param| param.optional || Self::formal_parameter_has_default(param))
                .unwrap_or(arrow.params.items.len());
            let return_ty = arrow
                .return_type
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            // The binding's annotation is the type every reader of the binding
            // sees, `unknown` included: TypeScript infers a call through
            // `handler` from what it declares, not from the arrow behind it. The
            // body may be narrower, and its value widens to the declared return.
            let declared_return = contextual_function
                .as_ref()
                .map(|function| function.return_ty);
            // With no contextual signature at all, the checker's inferred
            // return stands in for the annotation the source left out
            // (`checker_function_return_type`).
            let checker_return = if return_ty.is_none() && contextual_function.is_none() {
                self.checker_function_return_type(arrow.span.start, arrow.span.end)
            } else {
                None
            };
            let no_return_hint =
                return_ty.is_none() && declared_return.is_none() && checker_return.is_none();
            let mut return_ty = return_ty
                .or(declared_return)
                .or(checker_return)
                // not a new erasure: "no return hint" sentinel; recorded after
                // closure-body lowering if the return stays erased
                .unwrap_or_else(|| self.unknown_type(NotNewErasure::NoHint));
            if arrow.r#async
                && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
            {
                return_ty = self.ctx.krate.types.intern(Type::Future(return_ty));
            }
            let symbol = self.intern_source_name(name);
            let fn_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: params.clone(),
                rest: rest.map(|rest| rest.index),
                required_params: Some(required_params),
                mutable_params: Vec::new(),
                return_ty,
                is_async: arrow.r#async,
                may_throw: false,
            }));
            let value = self.arrow_closure_body_expr(arrow, &params, return_ty, body)?;
            if no_return_hint {
                self.record_unhinted_closure_return(value, body);
            }
            // `fn_ty` was interned before the body was lowered, so its
            // `may_throw` is the syntactic guess `false`. The lowered closure
            // KNOWS whether its body throws
            // (`closure_body_expr_from_parts` derives it from
            // `body_contains_uncaught_throw`), and the binding must carry that:
            // a call that passes this local on is typed from the local's own
            // function type, so a `false` here tells every consumer the callback
            // cannot throw. That is what made a throwing handler passed to an
            // erased promise's `.catch(..)` be adapted DOWN to the non-throwing
            // ABI through the panic route — and a panic route only reaches a
            // source `catch` when the `catch_unwind` is around the INVOCATION,
            // which a continuation invoked later during an `await` does not
            // have, so the throw escaped the program (H70).
            let fn_ty = self.throwing_widened_function_ty(fn_ty, Self::expr_ty(body, value));
            // An arrow with no return annotation and no contextual signature
            // INFERS its return type from its body — that is what `tsc` does,
            // and a `const` with no annotation takes its initializer's type. The
            // signature above was interned before the body was lowered, so its
            // return is still the `Unknown` placeholder; the lowered closure
            // knows the real one, and the binding must carry it.
            //
            // Without this, `const res = (t: string) => this.make(t)` bound
            // `fn(string) -> unknown` while the closure it holds is
            // `fn(string) -> Response`, and every consumer of the binding read
            // the erased spelling: `promise.then(res)` typed itself
            // `Promise<unknown>` over a continuation that resolves a `Response`
            // (E0271 in Hono's `context.rs` slice of `main.rs`).
            let fn_ty = if no_return_hint {
                self.inferred_return_function_ty(fn_ty, Self::expr_ty(body, value))
            } else {
                fn_ty
            };
            let local =
                self.local_arrow_binding_local(name, symbol, fn_ty, self.span(start, end), body);
            self.scope.bind(name.to_owned(), local);
            let pat = body.push_pattern(Pattern::Binding(local));
            body.push_stmt_to_block(
                block,
                Stmt::Let {
                    pat,
                    ty: fn_ty,
                    value: Some(value),
                },
            );
            Ok(())
        })();
        self.pop_type_parameter_scope();
        let declared_local = result.as_ref().ok().and_then(|()| self.scope.lookup(name));
        self.scope.restore_bindings(saved_outer_locals);
        if let Some(local) = declared_local {
            self.scope.bind(name.to_owned(), local);
        }
        result
    }

    /// Re-intern a declared function type with the lowered value's return type.
    ///
    /// `declared` is the signature the binding was interned with before its body
    /// was lowered; `value` is the type the lowered closure actually has. Only
    /// the return type is taken from the value, and only when the declaration
    /// left it erased — an arrow whose return type is annotated, or fixed by a
    /// contextual signature, keeps what the declaration resolved, so this is the
    /// inference step and never an override. An `async` arrow's placeholder is
    /// `Future<unknown>`, which is why both spellings are recognized.
    ///
    /// Returns `declared` unchanged when either type is not a function or the
    /// declaration already names a return type.
    fn inferred_return_function_ty(
        &mut self,
        declared: smelt_hir::TypeId,
        value: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        let Some(Type::Function(declared_ty)) = self.ctx.krate.types.get(declared).cloned() else {
            return declared;
        };
        let declared_is_erased = match self.ctx.krate.types.get(declared_ty.return_ty) {
            Some(Type::Unknown) => true,
            Some(Type::Future(inner)) => {
                matches!(self.ctx.krate.types.get(*inner), Some(Type::Unknown))
            }
            _ => false,
        };
        if !declared_is_erased {
            return declared;
        }
        let Some(Type::Function(value_ty)) = self.ctx.krate.types.get(value).cloned() else {
            return declared;
        };
        if value_ty.return_ty == declared_ty.return_ty {
            return declared;
        }
        let mut inferred = declared_ty;
        inferred.return_ty = value_ty.return_ty;
        self.ctx.krate.types.intern(Type::Function(inferred))
    }

    /// Re-intern a declared function type with the lowered value's `may_throw`.
    ///
    /// `declared` is the type the binding was interned with before its body was
    /// lowered; `value` is the type the lowered closure actually has. Only the
    /// `may_throw` bit is taken from the value, so every other part of the
    /// declared signature (parameter types, arity, return type, asyncness) is
    /// left exactly as the declaration resolved it and emissions for a
    /// non-throwing closure stay byte-identical. Returns `declared` unchanged
    /// when either type is not a function or the declared type already admits a
    /// throw.
    fn throwing_widened_function_ty(
        &mut self,
        declared: smelt_hir::TypeId,
        value: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        let Some(Type::Function(value_ty)) = self.ctx.krate.types.get(value) else {
            return declared;
        };
        if !value_ty.may_throw {
            return declared;
        }
        let Some(Type::Function(declared_ty)) = self.ctx.krate.types.get(declared) else {
            return declared;
        };
        if declared_ty.may_throw {
            return declared;
        }
        let mut widened = declared_ty.clone();
        widened.may_throw = true;
        self.ctx.krate.types.intern(Type::Function(widened))
    }

    /// Reserve the enclosing body's local that a nested function declaration is
    /// bound to, before its own body is lowered.
    ///
    /// [`Self::predeclare_local_function_declarations`] already reserves one for
    /// every declaration at the top of a function body, which is what makes an
    /// earlier sibling able to call a later declaration. A declaration in a
    /// NESTED block (inside an `if`, a loop or a `try`) has no such prepass, so
    /// the local is minted here instead. Either way the local exists before the
    /// declaration's body is lowered, so the body can capture it and recurse.
    ///
    /// The type is provisional: the real function type is only known once the
    /// body has been lowered, and the caller overwrites it there.
    fn reserve_local_function_local(
        &self,
        function_symbol: smelt_hir::Symbol,
        id: &oxc::ast::ast::BindingIdentifier<'_>,
        provisional_fn_ty: smelt_hir::TypeId,
        outer_body: &mut Body,
    ) -> smelt_hir::LocalId {
        // A binding already in scope is this declaration's predeclared local
        // only when it is a local OF THE BODY BEING LOWERED carrying this
        // declaration's name; anything else is an enclosing scope's binding,
        // which this declaration SHADOWS rather than reuses.
        if let Some(existing) = self.scope.lookup(id.name.as_str())
            && outer_body
                .local_at(existing)
                .is_some_and(|decl| decl.name == Some(function_symbol))
        {
            return existing;
        }
        outer_body.push_local(LocalDecl {
            name: Some(function_symbol),
            ty: provisional_fn_ty,
            mutable: false,
            span: self.span(id.span.start, id.span.end),
        })
    }

    /// Lower a nested `function name(...) { ... }` declaration as a local closure.
    pub(in crate::lowering) fn local_function_declaration(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        outer_body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let id = function.id.as_ref().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "anonymous local function declarations are not lowered yet",
            )
        })?;
        // A constructor function recognized during the block prepass was already
        // synthesized into a class; its declaration statement contributes no
        // local closure.
        if self.classes.contains(id.name.as_str()) {
            return Ok(());
        }
        let Some(function_body) = &function.body else {
            return Ok(());
        };
        self.push_function_type_parameter_scope(
            function.type_parameters.as_deref(),
            crate::lowering::ty::element_params::FunctionSignature::of_function(function),
        )?;
        // Everything below lowers this function's OWN body, where its name does
        // not denote a value it can reach (see `defining_local_functions`).
        let defining = id.name.as_str().to_owned();
        let result = self.with_defining_local_function(defining, |this| {
            let mut param_tys = Vec::new();
            let mut closure_body = Body::new(
                None,
                this.span(function_body.span.start, function_body.span.end),
            );
            closure_body.set_generator(function.generator);
            let mut closure_params = Vec::new();
            let mut param_names = HashSet::new();
            let mut saved_locals = Vec::new();
            let mut defaulted_params = Vec::new();

            // A nested `function` that reads its own `arguments` is variadic: the
            // object is the ACTUAL argument list, which a declared-arity signature
            // cannot carry (see `lowering::arguments_forwarding`). Replace the
            // parameter list with one rest list and re-bind each declared name from
            // it. es-toolkit's `rest`/`ary`/`unary` spec helpers are this shape.
            let arguments_forwarding =
                this.arguments_forwarding_params(function, &mut closure_body)?;
            // `Function.prototype.length` is the SOURCE arity — `fn(a, b, c).length`
            // is `3` — and es-toolkit `rest(func)` reads it to choose its default
            // `start` (`func.length - 1`). The variadic rewrite replaces the
            // parameter list with one rest list, so the source arity cannot be
            // recovered from the signature and has to be recorded here.
            // `required_params` is the field `length` is derived from, and it
            // describes the source contract (the parameters before the first
            // optional one) even though the internal ABI is a single list.
            //
            // The same count is what a caller may omit down to, so it is
            // recorded for every nested declaration, not only the variadic one:
            // without it a call that omits a defaulted or optional argument
            // looked short of the declared arity and lost every argument.
            let source_required_params = Some(
                function
                    .params
                    .items
                    .iter()
                    .position(|param| param.optional || Self::formal_parameter_has_default(param))
                    .unwrap_or(function.params.items.len()),
            );
            if let Some(forwarding) = &arguments_forwarding {
                closure_params.extend(forwarding.params.iter().cloned());
                param_tys.extend(forwarding.param_tys.iter().copied());
                for (name, local) in forwarding.binding_pairs() {
                    param_names.insert(name.clone());
                    saved_locals.push((name.clone(), this.scope.bind(name, local)));
                }
            }
            for (index, param) in
                function
                    .params
                    .items
                    .iter()
                    .enumerate()
                    .take(if arguments_forwarding.is_some() {
                        0
                    } else {
                        usize::MAX
                    })
            {
                // A defaulted parameter takes the `Optional<T>` ABI and a body
                // prelude applies its default, exactly like a top-level
                // `function` (see `function_parameter_abi`). Dropping the
                // default here left an omitted argument with nothing to fill it.
                let (ty, default_initializer) = this.function_parameter_abi(param)?;
                let (symbol, param_span, source_name) = match &param.pattern {
                    BindingPattern::BindingIdentifier(binding) => (
                        this.intern_source_name(binding.name.as_str()),
                        this.span(binding.span.start, binding.span.end),
                        Some(binding.name.as_str().to_owned()),
                    ),
                    _ => (
                        this.synthetic_param_symbol(index),
                        this.span(param.span.start, param.span.end),
                        None,
                    ),
                };
                let local = closure_body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty,
                    mutable: false,
                    span: param_span,
                });
                closure_body.params.push(local);
                closure_params.push(Param {
                    name: symbol,
                    local,
                    ty,
                    span: param_span,
                });
                param_tys.push(ty);
                if let Some(source_name) = source_name {
                    param_names.insert(source_name.clone());
                    if let Some(initializer) = default_initializer {
                        defaulted_params.push((
                            local,
                            ty,
                            initializer,
                            symbol,
                            source_name.clone(),
                            param_span,
                        ));
                    }
                    saved_locals.push((source_name.clone(), this.scope.bind(source_name, local)));
                }
            }

            // Lower an optional `...rest` parameter exactly as top-level functions,
            // arrow expressions, and function-expression values do: resolve its
            // array element type, push a packed list local/param into the closure
            // body, bind its source name, and record the rest index so the closure
            // collects the trailing source arguments into one list. A nested
            // `function name(...args) { ... }` is a real local closure (e.g. the
            // curry/curryRight `makeCurry` family), so it must carry rest the same
            // way every other closure form does instead of aborting.
            let rest_index = if let Some(forwarding) = &arguments_forwarding {
                Some(forwarding.rest_index)
            } else if let Some(rest_param) = &function.params.rest {
                let BindingPattern::BindingIdentifier(binding) = &rest_param.rest.argument else {
                    return Err(SmeltError::unsupported(
                        this.span(rest_param.span.start, rest_param.span.end),
                        "nested function destructured rest parameters need rest binding lowering",
                    ));
                };
                let source_ty = rest_param
                    .type_annotation
                    .as_ref()
                    .map(|annotation| this.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .unwrap_or_else(|| {
                        this.erase_with(ErasureReason::MissingAnnotation, "rest parameter")
                    });
                let Ok((ty, _item_ty)) = this.rest_param_array_type(source_ty) else {
                    return Err(SmeltError::unsupported(
                        this.span(rest_param.span.start, rest_param.span.end),
                        "nested function rest parameter type must be an array type",
                    ));
                };
                let rest_index = closure_params.len();
                let symbol = this.intern_source_name(binding.name.as_str());
                let local = closure_body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty,
                    mutable: false,
                    span: this.span(binding.span.start, binding.span.end),
                });
                closure_body.params.push(local);
                closure_params.push(Param {
                    name: symbol,
                    local,
                    ty,
                    span: this.span(binding.span.start, binding.span.end),
                });
                param_tys.push(ty);
                let source_name = binding.name.as_str().to_owned();
                param_names.insert(source_name.clone());
                saved_locals.push((source_name.clone(), this.scope.bind(source_name, local)));
                Some(rest_index)
            } else {
                None
            };

            let declared_return_ty = function
                .return_type
                .as_ref()
                .map(|annotation| this.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?;
            // not a new erasure: provisional return, replaced by the inferred
            // one once the body is lowered.
            let provisional_return_ty = declared_return_ty
                .unwrap_or_else(|| this.unknown_type(NotNewErasure::ErasedUpstream));
            let provisional_fn_ty = this.ctx.krate.types.intern(Type::Function(FunctionType {
                params: param_tys.clone(),
                rest: rest_index,
                required_params: source_required_params,
                mutable_params: Vec::new(),
                return_ty: provisional_return_ty,
                is_async: function.r#async,
                may_throw: false,
            }));
            let function_symbol = this.intern_source_name(id.name.as_str());
            // A function declaration binds its OWN name for the whole scope it
            // is declared in, its own body included, so `function walk(i) { …
            // walk(i + 1) … }` is ordinary recursion. The name therefore has to
            // denote the ENCLOSING body's local that this declaration is
            // assigned to — the same shape as the this-recursive arrow
            // `const walk = (i) => … walk(i + 1) …`, which already lowers to a
            // genuine capture of that local and which codegen already ties with
            // a weak `Rc<RefCell<…>>` knot. Binding the name to a fresh,
            // never-assigned local inside the closure body instead (what this
            // did before) emitted a reference to a name Rust has never seen.
            let outer_local = this.reserve_local_function_local(
                function_symbol,
                id,
                provisional_fn_ty,
                outer_body,
            );
            saved_locals.push((id.name.as_str().to_owned(), Some(outer_local)));
            this.scope.bind(id.name.as_str().to_owned(), outer_local);

            // A local function has its own `this` (its frame takes it), never
            // the enclosing one: it is not a capture, and the enclosing binding
            // is shadowed for its body (`own_this`), restored with the rest.
            param_names.insert("this".to_owned());
            let prior_this = this.shadow_enclosing_this();
            saved_locals.push(("this".to_owned(), prior_this));
            let mut capture_names = Vec::new();
            for statement in &function_body.statements {
                this.collect_statement_capture_names(statement, &param_names, &mut capture_names);
            }
            capture_names.sort();
            capture_names.dedup();

            let mut captures = Vec::new();
            for name in capture_names {
                let Some(source_local) = saved_locals
                    .iter()
                    .find_map(|(saved_name, prior)| {
                        (saved_name == &name).then_some(*prior).flatten()
                    })
                    .or_else(|| this.scope.lookup(name.as_str()))
                else {
                    continue;
                };
                let Some(source_decl) = outer_body.local_at(source_local) else {
                    continue;
                };
                let symbol = source_decl
                    .name
                    .unwrap_or_else(|| this.ctx.krate.symbols.intern(name.as_str()));
                let body_local = closure_body.push_local(LocalDecl {
                    name: Some(symbol),
                    ty: source_decl.ty,
                    mutable: source_decl.mutable,
                    span: source_decl.span,
                });
                saved_locals.push((name.clone(), this.scope.bind(name, body_local)));
                captures.push(ClosureCapture {
                    source_local,
                    body_local: Some(body_local),
                    symbol,
                    ty: source_decl.ty,
                    mode: CaptureMode::ByRef,
                });
            }

            let ctx = this.nested_fn_ctx(BodyKind::Function, function.r#async, declared_return_ty);
            let lowering_result = this.with_fn_ctx(ctx, |this| {
                let generator_yields = function.generator.then(|| {
                    this.initialize_generator_yield_accumulator(function, &mut closure_body)
                });
                this.fn_ctx.generator = generator_yields;
                // With the variadic rewrite the whole argument list is the single rest
                // parameter, so the FIXED arity is zero — that is what tells
                // `arguments_object_expression` to read the list rather than the
                // declared parameters.
                this.fn_ctx.arguments_arity = Some(if arguments_forwarding.is_some() {
                    0
                } else {
                    function.params.items.len()
                });
                // Defaults run first, in callee scope, before any body
                // statement; each rebinds its parameter name to the applied
                // local, which the `saved_locals` restore below undoes.
                let mut lowering_result =
                    this.apply_parameter_defaults(defaulted_params, &mut closure_body);
                for statement in
                    crate::lowering::hoisting::hoisted_statements(&function_body.statements)
                {
                    if lowering_result.is_err() {
                        break;
                    }
                    if let Err(error) = this.statement(statement, &mut closure_body) {
                        lowering_result = Err(error);
                        break;
                    }
                }
                if let Some(accumulator) = generator_yields {
                    Self::push_generator_return(accumulator, function, &mut closure_body);
                }
                if function.r#async {
                    closure_body.finish_async();
                }
                lowering_result
            });
            for (name, prior) in saved_locals.into_iter().rev() {
                if let Some(local) = prior {
                    this.scope.bind(name, local);
                } else {
                    this.scope.unbind(name.as_str());
                }
            }
            lowering_result?;

            let mut return_ty = if function.generator && declared_return_ty.is_none() {
                this.inferred_generator_type(&closure_body, function.r#async)
            } else {
                declared_return_ty
                    .or_else(|| this.last_return_type(&closure_body))
                    .unwrap_or_else(|| this.ctx.krate.types.intern(Type::None))
            };
            // As for a module function: an `async` body's inferred return is the
            // awaited value, and the call yields a Promise of it.
            if function.r#async
                && !function.generator
                && !matches!(this.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
            {
                return_ty = this.ctx.krate.types.intern(Type::Future(return_ty));
            }
            if function.generator && declared_return_ty.is_some() {
                return_ty = this.generator_type_with_fallthrough(&closure_body, return_ty);
            }
            let body_id = this.ctx.krate.push_body(closure_body);
            let fn_ty = this.ctx.krate.types.intern(Type::Function(FunctionType {
                params: param_tys,
                rest: rest_index,
                required_params: source_required_params,
                mutable_params: Vec::new(),
                return_ty,
                is_async: function.r#async,
                may_throw: false,
            }));
            // The local was reserved before the body was lowered (the body may
            // reference it), so only its TYPE is still provisional here.
            let local = outer_local;
            if let Some(decl) = outer_body.local_at_mut(local) {
                decl.ty = fn_ty;
            }
            this.scope.bind(id.name.as_str().to_owned(), local);
            let value = outer_body.push_expr(Expr {
                kind: ExprKind::Closure(smelt_hir::ClosureExpr {
                    params: closure_params,
                    rest: rest_index,
                    required_params: source_required_params,
                    return_ty,
                    captures,
                    body: body_id,
                    function_item: None,
                    this_binding: smelt_hir::ClosureThis::Own,
                    span: this.span(function.span.start, function.span.end),
                }),
                ty: fn_ty,
                span: this.span(function.span.start, function.span.end),
            });
            let pat = outer_body.push_pattern(Pattern::Binding(local));
            outer_body.push_stmt_to_block(
                block,
                Stmt::Let {
                    pat,
                    ty: fn_ty,
                    value: Some(value),
                },
            );
            Ok(())
        });
        self.pop_type_parameter_scope();
        result
    }

    /// Return a contextual function type from an optional type hint.
    pub(in crate::lowering) fn contextual_function_type(
        &mut self,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Option<FunctionType> {
        type_hint.and_then(|hint| {
            let function_hint = self.function_member_type(hint).unwrap_or(hint);
            if let Some(Type::Function(function)) = self.ctx.krate.types.get(function_hint) {
                Some(function.clone())
            } else {
                None
            }
        })
    }

    /// Build the public function type for a local arrow declaration.
    ///
    /// The annotations are lowered in the arrow's own type-parameter scope,
    /// pushed as a closure's ([`Self::push_closure_type_parameter_scope`]):
    /// this is the type of the binding's VALUE, a closure slot that cannot be
    /// generic, so a constrained parameter reads as its constraint exactly as
    /// it does in the closure body. Outside any scope `T[number]` named no
    /// type at all and erased.
    pub(in crate::lowering) fn local_arrow_function_type(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        self.push_closure_type_parameter_scope(arrow.type_parameters.as_deref())?;
        let result = self.local_arrow_function_type_in_scope(arrow, type_hint);
        self.pop_type_parameter_scope();
        result
    }

    /// [`Self::local_arrow_function_type`] once the arrow's scope is pushed.
    fn local_arrow_function_type_in_scope(
        &mut self,
        arrow: &oxc::ast::ast::ArrowFunctionExpression<'_>,
        type_hint: Option<smelt_hir::TypeId>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let contextual_function = self.contextual_function_type(type_hint);
        let mut params =
            self.arrow_callback_param_types_with_hint(arrow, contextual_function.as_ref())?;
        for (param, ty) in arrow.params.items.iter().zip(params.iter_mut()) {
            if param.optional && !matches!(self.ctx.krate.types.get(*ty), Some(Type::Optional(_))) {
                *ty = self.ctx.krate.types.intern(Type::Optional(*ty));
            }
        }
        let return_ty = arrow
            .return_type
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .or_else(|| {
                contextual_function
                    .as_ref()
                    .map(|function| function.return_ty)
            })
            .or_else(|| {
                if contextual_function.is_none() {
                    self.checker_function_return_type(arrow.span.start, arrow.span.end)
                } else {
                    None
                }
            })
            // not a new erasure: provisional pre-pass return, refined from the
            // lowered body
            .unwrap_or_else(|| self.unknown_type(NotNewErasure::ErasedUpstream));
        let mutable_params = self.mutable_params_from_returned_tuple_state(&params, return_ty);
        Ok(self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest: None,
            required_params: Some(
                arrow
                    .params
                    .items
                    .iter()
                    .position(|param| param.optional || Self::formal_parameter_has_default(param))
                    .unwrap_or(arrow.params.items.len()),
            ),
            mutable_params,
            return_ty,
            is_async: arrow.r#async,
            may_throw: false,
        })))
    }

    /// Return a predeclared local arrow binding that belongs to this body.
    pub(in crate::lowering) fn local_arrow_existing_body_local(
        &self,
        name: &str,
        body: &Body,
    ) -> Option<smelt_hir::LocalId> {
        let local = self.scope.lookup(name)?;
        body.local_at(local).map(|_| local)
    }

    /// Return the local slot for a local arrow declaration, updating predeclared slots.
    pub(in crate::lowering) fn local_arrow_binding_local(
        &self,
        name: &str,
        symbol: smelt_hir::Symbol,
        ty: smelt_hir::TypeId,
        span: Span,
        body: &mut Body,
    ) -> smelt_hir::LocalId {
        if let Some(local) = self.local_arrow_existing_body_local(name, body)
            && let Some(local_decl) = body.local_at_mut(local)
        {
            local_decl.name = Some(symbol);
            local_decl.ty = ty;
            local_decl.mutable = false;
            local_decl.span = span;
            return local;
        }
        body.push_local(LocalDecl {
            name: Some(symbol),
            ty,
            mutable: false,
            span,
        })
    }
}
