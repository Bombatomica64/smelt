//! Named function-declaration lowering: signatures, overloads, assertion and
//! type-predicate returns, parameter types and defaults, and function bodies.
//!
//! Function expressions, inferred result types and classes are sibling
//! modules of `decls`.


use crate::SmeltError;
use crate::lowering::{AssertionNarrowing, ModuleBuilder, RestParam};
use crate::lowering::state::fn_ctx::BodyKind;
use crate::lowering::state::local_scope::ScopeGroups;
use oxc::ast::ast::{BindingPattern, Expression, Statement};
use smelt_hir::{
    Body, ErasureReason, Expr, ExprKind, Function, FunctionOwner, FunctionType, Item, LocalDecl,
    Param, Pattern, Span, Stmt, Type,
};

impl ModuleBuilder<'_> {
    /// Build the HIR function type describing a method's own signature.
    ///
    /// Used when a reassigned method is carried as a function-typed field (see
    /// [`methods_assigned_on_this`]): the field's declared type has to be the
    /// signature the method itself declares, so a call through the field keeps
    /// the parameter and return types the source wrote rather than widening to
    /// the runtime carrier. Parameters and the return follow the same
    /// annotation-or-`Unknown` rules as an abstract method signature.
    ///
    /// The method's own type parameters are in scope only while its signature
    /// lowers; this used to push that scope and never pop it, so every later
    /// item of the module resolved a same-named type to the method's
    /// parameter.
    pub(super) fn method_signature_function_type(
        &mut self,
        method: &oxc::ast::ast::MethodDefinition<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        self.with_method_type_parameter_scope(method.value.type_parameters.as_deref(), |this, _| {
            this.method_signature_function_type_in_scope(method)
        })
    }

    /// [`Self::method_signature_function_type`] with the method's type
    /// parameters already in scope.
    fn method_signature_function_type_in_scope(
        &mut self,
        method: &oxc::ast::ast::MethodDefinition<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let return_ty = method
            .value
            .return_type
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .unwrap_or_else(|| {
                let unknown = self.erase(ErasureReason::MissingAnnotation);
                if method.value.r#async {
                    self.ctx.krate.types.intern(Type::Future(unknown))
                } else {
                    unknown
                }
            });
        let mut params = Vec::new();
        for param in &method.value.params.items {
            let ty = param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .unwrap_or_else(|| self.erase(ErasureReason::MissingAnnotation));
            params.push(ty);
        }
        // `Function.length` counts the leading parameters before the first
        // optional or defaulted one, the same rule named-function lowering uses.
        let required_params = method
            .value
            .params
            .items
            .iter()
            .position(|param| param.optional || Self::formal_parameter_has_default(param))
            .unwrap_or(method.value.params.items.len());
        // The rest parameter is the LAST entry of `params`, at index
        // `items.len()`, as in every other function type. It used to be left
        // out, with `rest` pointing at the last FIXED parameter, so a method
        // `f(...args: T[])` had no slot for its list.
        let rest = match method.value.params.rest.as_ref() {
            Some(rest) => {
                let declared = rest
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .unwrap_or_else(|| {
                        let item = self.erase(ErasureReason::MissingAnnotation);
                        self.ctx.krate.types.intern(Type::List(item))
                    });
                self.push_rest_param_type(&mut params, declared)?
            }
            None => None,
        };
        Ok(self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest,
            required_params: Some(required_params),
            mutable_params: Vec::new(),
            return_ty,
            is_async: method.value.r#async,
            may_throw: false,
        })))
    }

    /// Lower a TypeScript function declaration into a HIR function item.
    pub(in crate::lowering) fn function_declaration(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let id = function.id.as_ref().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "anonymous function declarations are not lowered yet",
            )
        })?;
        self.function_declaration_named(function, id.name.as_str())
    }

    /// Lower a TypeScript `Function` AST node into a HIR function item under an
    /// externally supplied name.
    ///
    /// A `function` *declaration* carries its own identifier, but an exported
    /// `const stub = function () { ... }` binds an otherwise-anonymous
    /// `FunctionExpression` to the const's name. Both forms are semantically a
    /// named module function, so they share this body; the only difference is
    /// where the name comes from.
    pub(in crate::lowering) fn function_declaration_named(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        name_text: &str,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let Some(function_body) = &function.body else {
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "declare functions are not lowered yet",
            ));
        };
        let name = self.intern_source_name(name_text);
        // Retain the declared type parameters so a generic free function such as
        // `function identity<T>(x: T): T` lowers to real Rust generics instead
        // of erasing `T` to `SmeltUnknown`. The scope is popped on every exit
        // path of the signature and body lowering; the lowered `TypeParamDef`s
        // survive on the item.
        let (type_params, assertion_return, predicate_return, declared_return_ty, lowered) = self
            .with_type_parameter_scope(function.type_parameters.as_deref(), |this, type_params| {
            let assertion_return = match function
                .return_type
                .as_ref()
                .and_then(|annotation| this.assertion_return_type(&annotation.type_annotation))
                .transpose()
            {
                Ok(value) => value,
                Err(error) => {
                    return Err(error);
                }
            };
            let predicate_return = match function
                .return_type
                .as_ref()
                .and_then(|annotation| this.predicate_return_type(&annotation.type_annotation))
                .transpose()
            {
                Ok(value) => value,
                Err(error) => {
                    return Err(error);
                }
            };
            let declared_return_ty = if assertion_return.is_some() {
                Some(this.ctx.krate.types.intern(Type::None))
            } else {
                match this.function_return_type_annotation_or_overload(function, name_text) {
                    Ok(value) => value,
                    Err(error) => {
                        return Err(error);
                    }
                }
            };
            if function.r#async
                && !function.generator
                && declared_return_ty.is_some()
                && !matches!(
                    declared_return_ty.and_then(|ty| this.ctx.krate.types.get(ty)),
                    Some(Type::Future(_))
                )
            {
                return Err(SmeltError::unsupported(
                    this.span(function.span.start, function.span.end),
                    "async functions must declare a Promise<T> return type",
                ));
            }

            let ctx = this.nested_fn_ctx(BodyKind::Function, function.r#async, declared_return_ty);
            let lowered = this.with_nested_body(ctx, ScopeGroups::ALL, |this, _| {
                let mut body = Body::new(
                    None,
                    this.span(function_body.span.start, function_body.span.end),
                );
                body.set_generator(function.generator);
                let mut params = Vec::new();

                let mut destructured_params = Vec::new();
                let mut defaulted_params = Vec::new();
                // A `function` that reads its own `arguments` is variadic: the object is the
                // ACTUAL argument list, which a declared-arity signature cannot carry (see
                // `lowering::arguments_forwarding`). Replace the parameter list with one rest
                // list and re-bind each declared name from it. Every other function keeps its
                // declared parameters untouched.
                let arguments_forwarding = match this.arguments_forwarding_params(function, &mut body) {
                    Ok(forwarding) => forwarding,
                    Err(error) => {
                        return Err(error);
                    }
                };
                if let Some(forwarding) = &arguments_forwarding {
                    params.extend(forwarding.params.iter().cloned());
                    for (name, local) in forwarding.binding_pairs() {
                        this.scope.bind(name, local);
                    }
                }
                for (index, param) in function
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
                    // The `Optional<T>` ABI of a defaulted parameter and its body
                    // prelude follow `function_parameter_abi`; see there.
                    let (ty, default_initializer) = this.function_parameter_abi(param)?;
                    let ty = this.overload_omitted_param_type(name_text, index, param, ty);
                    let (param_name, span, source_name) =
                        if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                            (
                                this.intern_source_name(binding.name.as_str()),
                                this.span(binding.span.start, binding.span.end),
                                Some(binding.name.to_string()),
                            )
                        } else {
                            let synthetic_name = format!("__param{index}");
                            (
                                this.intern_source_name(&synthetic_name),
                                this.span(param.span.start, param.span.end),
                                None,
                            )
                        };
                    // Defaults that equal the type's zero value keep the plain ABI,
                    // which also preserves mutable-reference threading for state
                    // accumulators such as Remeda's
                    // `cloneImplementation(value, refFrom = [], refTo = [])`, where
                    // recursive callees must observe the caller's array.
                    let local = body.push_local(LocalDecl {
                        name: Some(param_name),
                        ty,
                        mutable: false,
                        span,
                    });
                    body.params.push(local);
                    if this.type_is_known_date_value(ty) {
                        this.scope.mark_date_value(local);
                    }
                    if let Some(source_name) = source_name {
                        this.scope.bind(source_name.clone(), local);
                        if let Some(initializer) = default_initializer {
                            defaulted_params.push((local, ty, initializer, param_name, source_name, span));
                        }
                    } else {
                        destructured_params.push((&param.pattern, local, ty));
                    }
                    params.push(Param {
                        name: param_name,
                        local,
                        ty,
                        span,
                    });
                }
                let rest = if let Some(forwarding) = &arguments_forwarding {
                    Some(RestParam {
                        index: forwarding.rest_index,
                        item_ty: forwarding.item_ty,
                    })
                } else if let Some(rest) = &function.params.rest {
                    let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                        return Err(SmeltError::unsupported(
                            this.span(rest.span.start, rest.span.end),
                            "destructured rest parameters need rest binding lowering",
                        ));
                    };
                    let ty = match rest
                        .type_annotation
                        .as_ref()
                        .map(|annotation| this.ts_type_to_hir(&annotation.type_annotation))
                        .transpose()
                        .and_then(|value| {
                            value.ok_or_else(|| {
                                SmeltError::unsupported(
                                    this.span(rest.span.start, rest.span.end),
                                    "rest function parameters must have explicit array type annotations",
                                )
                            })
                        }) {
                        Ok(value) => value,
                        Err(error) => {
                            return Err(error);
                        }
                    };
                    // A fixed-width tuple rest is fixed parameters plus a tuple binding
                    // of them (`lowering::tuple_rest`).
                    if let Some(elements) = this.fixed_tuple_rest_elements(ty) {
                        let span = this.span(binding.span.start, binding.span.end);
                        for param in
                            this.bind_tuple_rest_params(binding.name.as_str(), ty, &elements, span, &mut body)
                        {
                            params.push(Param {
                                name: param.name,
                                local: param.local,
                                ty: param.ty,
                                span,
                            });
                        }
                        None
                    } else {
                    let Ok((ty, item_ty)) = this.rest_param_array_type(ty) else {
                        return Err(SmeltError::unsupported(
                            this.span(rest.span.start, rest.span.end),
                            "rest function parameter type must be an array type",
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
                    let index = params.len();
                    params.push(Param {
                        name: param_name,
                        local,
                        ty,
                        span: this.span(binding.span.start, binding.span.end),
                    });
                    Some(RestParam { index, item_ty })
                    }
                } else {
                    None
                };
                let required_params = function
                    .params
                    .items
                    .iter()
                    .position(|param| param.optional || Self::formal_parameter_has_default(param))
                    .unwrap_or(function.params.items.len());

                let mut errors = Vec::new();
                for (pattern, local, ty) in destructured_params {
                    let root = body.root;
                    let value = body.push_expr(Expr {
                        kind: ExprKind::Local(local),
                        ty,
                        span: this.span(function.span.start, function.span.end),
                    });
                    if let Err(error) =
                        this.binding_declaration(pattern, Some(value), Some(ty), false, &mut body, root)
                    {
                        errors.push(error);
                    }
                }
                if let Err(error) = this.apply_parameter_defaults(defaulted_params, &mut body) {
                    errors.push(error);
                }
                if let Err(error) =
                    this.predeclare_forward_referenced_locals(&function_body.statements, &mut body)
                {
                    errors.push(error);
                }
                if let Err(error) =
                    this.predeclare_local_function_declarations(&function_body.statements, &mut body)
                {
                    errors.push(error);
                }
                let generator_yields = function
                    .generator
                    .then(|| this.initialize_generator_yield_accumulator(function, &mut body));
                this.fn_ctx.generator = generator_yields;
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
                    if Self::is_super_call_statement(statement) {
                        continue;
                    }
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
                // Any `fn.prop = value` write this body collected must have been rebuilt
                // into a callable-interface struct by now; a leftover is a dropped write.
                this.report_unconsumed_callable_props(&mut errors);
                Ok((body, params, rest, required_params, errors))
            });
            Ok((type_params, assertion_return, predicate_return, declared_return_ty, lowered))
        })?;
        let (body, params, rest, required_params, errors) = lowered?;

        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let mut return_ty = if function.generator && declared_return_ty.is_none() {
            self.inferred_generator_type(&body, function.r#async)
        } else {
            declared_return_ty
                .or_else(|| self.last_return_type(&body))
                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::None))
        };
        if function.r#async
            && !function.generator
            && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
        {
            return_ty = self.ctx.krate.types.intern(Type::Future(return_ty));
        }
        if function.generator && declared_return_ty.is_some() {
            return_ty = self.generator_type_with_fallthrough(&body, return_ty);
        }
        let body_id = self.ctx.krate.push_body(body);
        let function_item = Function {
            name,
            span: self.span(function.span.start, function.span.end),
            type_params,
            params,
            rest: rest.map(|rest| rest.index),
            required_params: Some(required_params),
            return_ty,
            is_async: function.r#async,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Module,
        };
        let item = if let Some(item) = self.scope.function_item(name_text) {
            let index = usize::try_from(item.0).unwrap_or(usize::MAX);
            if let Some(slot) = self.ctx.krate.items.get_mut(index) {
                *slot = Item::Function(function_item);
            }
            item
        } else {
            self.ctx.krate.push_item(Item::Function(function_item))
        };
        self.items.insert(name_text.to_owned(), item);
        if let Some(rest) = rest {
            self.functions.set_rest(name_text.to_owned(), rest);
            self.publish_export(|ctx| &mut ctx.function_rests, name_text.to_owned(), rest);
        }
        if let Some((parameter_name, Some(target))) = assertion_return
            && let Some(param_index) = function.params.items.iter().position(|param| {
                matches!(
                    &param.pattern,
                    BindingPattern::BindingIdentifier(binding)
                        if binding.name.as_str() == parameter_name
                )
            })
        {
            self.functions.set_assertion(
                name_text.to_owned(),
                AssertionNarrowing {
                    param_index,
                    target,
                },
            );
        }
        if let Some((parameter_name, target)) = predicate_return
            && let Some(param_index) = function.params.items.iter().position(|param| {
                matches!(
                    &param.pattern,
                    BindingPattern::BindingIdentifier(binding)
                        if binding.name.as_str() == parameter_name
                )
            })
        {
            self.functions.set_predicate(
                name_text.to_owned(),
                AssertionNarrowing {
                    param_index,
                    target,
                },
            );
        }
        Ok(item)
    }

    /// Return whether a TypeScript formal parameter has a default value.
    pub(in crate::lowering) fn formal_parameter_has_default(param: &oxc::ast::ast::FormalParameter<'_>) -> bool {
        param.initializer.is_some() || matches!(param.pattern, BindingPattern::AssignmentPattern(_))
    }

    /// Emit the body prelude that applies declared parameter defaults.
    ///
    /// Each defaulted parameter arrives on the ABI as `Optional<T>` and is
    /// shadowed by a body-scoped `T` local initialized to `param ?? <default>`,
    /// mirroring JavaScript's per-call evaluation of defaults in callee scope.
    /// Later defaults may reference earlier parameters, which by then already
    /// resolve to their applied-default locals.
    ///
    /// Shared by free functions, methods, and constructors so all three agree on
    /// the ABI and on when a default is applied.
    pub(in crate::lowering) fn apply_parameter_defaults<'src>(
        &mut self,
        defaulted_params: Vec<(
            smelt_hir::LocalId,
            smelt_hir::TypeId,
            &'src Expression<'src>,
            smelt_hir::Symbol,
            String,
            Span,
        )>,
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        let mut first_error = None;
        for (abi_local, abi_ty, initializer, param_name, source_name, span) in defaulted_params {
            let inner_ty = match self.ctx.krate.types.get(abi_ty) {
                Some(Type::Optional(inner)) => *inner,
                _ => abi_ty,
            };
            let optional = body.push_expr(Expr {
                kind: ExprKind::Local(abi_local),
                ty: abi_ty,
                span,
            });
            let fallback = match self.expression_with_hint(initializer, body, Some(inner_ty)) {
                Ok(value) => value,
                Err(error) => {
                    first_error = first_error.or(Some(error));
                    continue;
                }
            };
            let value = body.push_expr(Expr {
                kind: ExprKind::OptionalCoalesce { optional, fallback },
                ty: inner_ty,
                span,
            });
            let applied = body.push_local(LocalDecl {
                name: Some(param_name),
                ty: inner_ty,
                mutable: true,
                span,
            });
            if self.type_is_known_date_value(inner_ty) {
                self.scope.mark_date_value(applied);
            }
            self.scope.bind(source_name, applied);
            let pat = body.push_pattern(Pattern::Binding(applied));
            let root = body.root;
            body.push_stmt_to_block(
                root,
                Stmt::Let {
                    pat,
                    ty: inner_ty,
                    value: Some(value),
                },
            );
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Return whether a parameter default is the type's zero value.
    ///
    /// For such defaults (`= 0`, `= ""`, `= false`, `= []`, `= {}`,
    /// `= new Map()`, `= new Set()`, `= undefined`) omitted call arguments
    /// already synthesize exactly the defaulted value, so the parameter keeps
    /// its plain (non-`Optional`) ABI. This also preserves mutable-reference
    /// threading for parameters that accumulate state across recursive calls.
    pub(in crate::lowering) fn initializer_is_type_zero_default(&self, initializer: &Expression<'_>) -> bool {
        match initializer {
            Expression::NumericLiteral(literal) => literal.value == 0.0,
            Expression::StringLiteral(literal) => literal.value.is_empty(),
            Expression::BooleanLiteral(literal) => !literal.value,
            Expression::NullLiteral(_) => true,
            Expression::Identifier(identifier) => identifier.name == "undefined",
            Expression::ArrayExpression(array) => array.elements.is_empty(),
            Expression::ObjectExpression(object) => object.properties.is_empty(),
            Expression::NewExpression(new_expr) => {
                new_expr.arguments.is_empty()
                    && matches!(
                        &new_expr.callee,
                        Expression::Identifier(callee)
                            if self.names_host(callee.name.as_str(), "Map") || self.names_host(callee.name.as_str(), "Set")
                    )
            }
            _ => false,
        }
    }

    /// Resolve the HIR type for a function declaration parameter.
    ///
    /// Explicit TypeScript annotations remain the primary source of parameter
    /// types. For unannotated parameters with default initializers, TypeScript
    /// infers the in-body parameter type from the default expression, so Smelt
    /// mirrors that narrow case without weakening arbitrary untyped functions.
    pub(in crate::lowering) fn function_parameter_type(
        &mut self,
        param: &oxc::ast::ast::FormalParameter<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let ty = if let Some(annotation) = &param.type_annotation {
            self.ts_type_to_hir(&annotation.type_annotation)?
        } else if let Some(initializer) = &param.initializer {
            self.infer_module_global_initializer_type(initializer)?
        } else {
            return Err(SmeltError::unsupported(
                self.span(param.span.start, param.span.end),
                "function parameters must have explicit type annotations or default initializers",
            ));
        };
        let widened = self.presence_widened_param_type(param, ty);
        if param.optional && !matches!(self.ctx.krate.types.get(widened), Some(Type::Optional(_))) {
            Ok(self.ctx.krate.types.intern(Type::Optional(widened)))
        } else {
            Ok(widened)
        }
    }

    /// Whether the checker reads `param`'s annotation as an object type with
    /// named properties.
    fn annotation_is_named_object_type(&self, param: &oxc::ast::ast::FormalParameter<'_>) -> bool {
        let (Some(annotation), Some(facts)) = (&param.type_annotation, self.ts_facts) else {
            return false;
        };
        let span = oxc::span::GetSpan::span(&annotation.type_annotation);
        facts
            .type_node_at(&self.path, span.start, span.end)
            .is_some_and(|ty| matches!(&ty.kind, smelt_ts_model::TsTypeKind::Object(object) if !object.properties.is_empty()))
    }

    /// `ty`, the annotated record type of `param`, as `Option` when the body
    /// tests the parameter for presence (`presence_tested_locals`): such a
    /// function accepts an absent value whatever its annotation says, and a
    /// record struct has no way to hold one.
    pub(in crate::lowering) fn presence_widened_param_type(
        &mut self,
        param: &oxc::ast::ast::FormalParameter<'_>,
        ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        let tested = matches!(
            &param.pattern,
            BindingPattern::BindingIdentifier(binding)
                if self.presence_widened_bindings.contains_key(&binding.span.start)
        );
        // A `Dict` here is either a record struct (an object type with named
        // properties, spelled inline or through an alias) or a real `Record`,
        // which holds no fields to miss and keeps its plain ABI. The name of an
        // alias that already admits `null` is not a record item either.
        let record = match self.ctx.krate.types.get(ty) {
            Some(Type::Class { name, .. }) => {
                let item = *name;
                self.find_interface(item).is_some() || self.class_by_symbol(item).is_some()
            }
            Some(Type::Dict(..)) => self.annotation_is_named_object_type(param),
            _ => false,
        };
        if tested && record {
            self.ctx.krate.types.intern(Type::Optional(ty))
        } else {
            ty
        }
    }

    /// The ABI type of a `function` parameter together with the default
    /// initializer its body prelude must apply, if any.
    ///
    /// This is the one rule every `function` form (top-level item, nested
    /// local declaration, and the hoisting prepass that reserves the nested
    /// declaration's local) shares, so a caller sees the same signature
    /// wherever the callee is declared. A default initializer makes the
    /// parameter optional for callers: the ABI parameter is `Optional<T>` and
    /// [`Self::apply_parameter_defaults`] applies the default in the callee
    /// (JavaScript evaluates defaults per call, in callee scope). A default
    /// that equals the type's zero value keeps the plain ABI, because call
    /// sites already synthesize exactly that value for an omitted argument
    /// (see [`Self::initializer_is_type_zero_default`]). Only an identifier
    /// parameter carries a prelude default; a destructured one is bound by
    /// its own pattern lowering.
    /// The implementation parameter type at `index` of the overloaded
    /// function `name_text`, made optional when some overload omits it.
    ///
    /// `function toCamelCase(options?: Options): (data: string) => string`
    /// next to `function toCamelCase(dataOrOptions: Options | string,
    /// options?: Options)` lets a caller write `toCamelCase()`. JavaScript
    /// then runs the implementation with `dataOrOptions === undefined`, so the
    /// implementation's parameter admits `undefined` even though its own
    /// annotation does not say so (TypeScript only checks callers against the
    /// overloads). Without this the omitted argument was filled with the
    /// parameter type's default (`""`), and the implementation took the
    /// data-first branch. A parameter some overload leaves to a rest list is
    /// not counted: the rest decides its own arity. A parameter with a default
    /// initializer already answers its omission with that default.
    pub(in crate::lowering) fn overload_omitted_param_type(
        &mut self,
        name_text: &str,
        index: usize,
        param: &oxc::ast::ast::FormalParameter<'_>,
        ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        // An erased parameter already carries `undefined` in its runtime tag.
        if param.initializer.is_some()
            || matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_) | Type::Unknown))
        {
            return ty;
        }
        let omitted = self.functions.overloads(name_text).is_some_and(|overloads| {
            overloads.iter().any(|overload| {
                overload.rest.is_none()
                    && overload.required_params.unwrap_or(overload.params.len()) <= index
            })
        });
        if omitted {
            self.ctx.krate.types.intern(Type::Optional(ty))
        } else {
            ty
        }
    }

    pub(in crate::lowering) fn function_parameter_abi<'src>(
        &mut self,
        param: &'src oxc::ast::ast::FormalParameter<'src>,
    ) -> Result<(smelt_hir::TypeId, Option<&'src Expression<'src>>), SmeltError> {
        let ty = self.function_parameter_type(param)?;
        let default_initializer = matches!(param.pattern, BindingPattern::BindingIdentifier(_))
            .then_some(param.initializer.as_deref())
            .flatten()
            .filter(|initializer| !self.initializer_is_type_zero_default(initializer));
        let ty = if default_initializer.is_some()
            && !matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_)))
        {
            self.ctx.krate.types.intern(Type::Optional(ty))
        } else {
            ty
        };
        Ok((ty, default_initializer))
    }

    /// Return whether a constructor statement is a bare `super(...)` call.
    pub(in crate::lowering) fn is_super_call_statement(statement: &Statement<'_>) -> bool {
        let Statement::ExpressionStatement(statement) = statement else {
            return false;
        };
        let Expression::CallExpression(call) = statement.expression.without_parentheses() else {
            return false;
        };
        matches!(call.callee, Expression::Super(_))
    }
}
