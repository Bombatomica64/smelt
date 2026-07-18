//! Named function-declaration lowering: signatures, overloads, assertion and
//! type-predicate returns, and function bodies.

use std::collections::HashSet;

use crate::lowering::support::visibility;
use crate::lowering::{
    AssertionNarrowing, GeneratorYieldAccumulator,
    ModuleBuilder, RestParam, specialization,
};
use crate::SmeltError;
use oxc::ast::ast::{
    ArrayExpressionElement, BindingPattern, ClassElement, Expression, MethodDefinitionKind, MethodDefinitionType, ObjectPropertyKind,
    PropertyKey, Statement,
};
use oxc::span::GetSpan;
use smelt_hir::{
    Body, CLASS_INDEX_STORE_FIELD, Class, Expr, ExprKind,
    Field, Function, FunctionOwner, FunctionType, Item, Literal, LocalDecl, MethodSig, Param,
    ParamSig, Pattern, Span, Stmt, Type, Visibility,
};

impl ModuleBuilder<'_> {
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
        // of erasing `T` to `SmeltUnknown`. The scope is popped below like any
        // other function scope; the lowered `TypeParamDef`s survive on the item.
        let type_params = self.push_type_parameter_scope(function.type_parameters.as_deref())?;
        let assertion_return = match function
            .return_type
            .as_ref()
            .and_then(|annotation| self.assertion_return_type(&annotation.type_annotation))
            .transpose()
        {
            Ok(value) => value,
            Err(error) => {
                self.pop_type_parameter_scope();
                return Err(error);
            }
        };
        let predicate_return = match function
            .return_type
            .as_ref()
            .and_then(|annotation| self.predicate_return_type(&annotation.type_annotation))
            .transpose()
        {
            Ok(value) => value,
            Err(error) => {
                self.pop_type_parameter_scope();
                return Err(error);
            }
        };
        let declared_return_ty = if assertion_return.is_some() {
            Some(self.ctx.krate.types.intern(Type::None))
        } else {
            match self.function_return_type_annotation_or_overload(function, name_text) {
                Ok(value) => value,
                Err(error) => {
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
            }
        };
        if function.r#async
            && declared_return_ty.is_some()
            && !matches!(
                declared_return_ty.and_then(|ty| self.ctx.krate.types.get(ty)),
                Some(Type::Future(_))
            )
        {
            self.pop_type_parameter_scope();
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "async functions must declare a Promise<T> return type",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_date_value_locals = std::mem::take(&mut self.date_value_locals);
        let saved_callable_local_props = std::mem::take(&mut self.callable_local_props);
        let saved_explicit_any_locals = std::mem::take(&mut self.explicit_any_locals);
        let saved_narrowed_locals = std::mem::take(&mut self.narrowed_locals);
        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        let saved_generator_yields = self.current_generator_yields;
        self.current_async = function.r#async;
        self.current_return_ty = declared_return_ty;
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut params = Vec::new();

        let mut destructured_params = Vec::new();
        let mut defaulted_params = Vec::new();
        for (index, param) in function.params.items.iter().enumerate() {
            let ty = match self.function_parameter_type(param) {
                Ok(value) => value,
                Err(error) => {
                    self.locals = saved_locals;
                    self.date_value_locals = saved_date_value_locals;
                    self.callable_local_props = saved_callable_local_props;
                    self.explicit_any_locals = saved_explicit_any_locals;
                    self.narrowed_locals = saved_narrowed_locals;
                    self.current_async = saved_async;
                    self.current_return_ty = saved_return_ty;
                    self.current_generator_yields = saved_generator_yields;
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
            };
            let (param_name, span, source_name) =
                if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                    (
                        self.intern_source_name(binding.name.as_str()),
                        self.span(binding.span.start, binding.span.end),
                        Some(binding.name.to_string()),
                    )
                } else {
                    let synthetic_name = format!("__param{index}");
                    (
                        self.intern_source_name(&synthetic_name),
                        self.span(param.span.start, param.span.end),
                        None,
                    )
                };
            // A default initializer makes the parameter optional for callers:
            // the ABI parameter is `Optional<T>` and a body prelude applies the
            // default (JavaScript evaluates defaults per call, in callee
            // scope), so omitted arguments lower through the general
            // `Optional` machinery instead of per-function fixups.
            //
            // Defaults that equal the type's zero value (`= 0`, `= ""`,
            // `= []`, `= {}`, `= new Map()`, ...) keep the plain ABI instead:
            // call sites already synthesize exactly that value for omitted
            // arguments, and keeping the parameter un-wrapped preserves
            // mutable-reference threading for state accumulators such as
            // Remeda's `cloneImplementation(value, refFrom = [], refTo = [])`,
            // where recursive callees must observe the caller's array.
            let default_initializer = source_name
                .is_some()
                .then_some(param.initializer.as_ref())
                .flatten()
                .filter(|initializer| !Self::initializer_is_type_zero_default(initializer));
            let ty = if default_initializer.is_some()
                && !matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_)))
            {
                self.ctx.krate.types.intern(Type::Optional(ty))
            } else {
                ty
            };
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span,
            });
            body.params.push(local);
            if self.type_is_known_date_value(ty) {
                self.date_value_locals.insert(local);
            }
            if let Some(source_name) = source_name {
                self.locals.insert(source_name.clone(), local);
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
        let rest = if let Some(rest) = &function.params.rest {
            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                self.locals = saved_locals;
                self.date_value_locals = saved_date_value_locals;
                self.callable_local_props = saved_callable_local_props;
                self.explicit_any_locals = saved_explicit_any_locals;
                self.narrowed_locals = saved_narrowed_locals;
                self.current_async = saved_async;
                self.current_return_ty = saved_return_ty;
                self.current_generator_yields = saved_generator_yields;
                self.pop_type_parameter_scope();
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest parameters need rest binding lowering",
                ));
            };
            let ty = match rest
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()
                .and_then(|value| {
                    value.ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(rest.span.start, rest.span.end),
                            "rest function parameters must have explicit array type annotations",
                        )
                    })
                }) {
                Ok(value) => value,
                Err(error) => {
                    self.locals = saved_locals;
                    self.date_value_locals = saved_date_value_locals;
                    self.callable_local_props = saved_callable_local_props;
                    self.explicit_any_locals = saved_explicit_any_locals;
                    self.narrowed_locals = saved_narrowed_locals;
                    self.current_async = saved_async;
                    self.current_return_ty = saved_return_ty;
                    self.current_generator_yields = saved_generator_yields;
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
            };
            let Ok((ty, item_ty)) = self.rest_param_array_type(ty) else {
                self.locals = saved_locals;
                self.date_value_locals = saved_date_value_locals;
                self.callable_local_props = saved_callable_local_props;
                self.explicit_any_locals = saved_explicit_any_locals;
                self.narrowed_locals = saved_narrowed_locals;
                self.current_async = saved_async;
                self.current_return_ty = saved_return_ty;
                self.current_generator_yields = saved_generator_yields;
                self.pop_type_parameter_scope();
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest function parameter type must be an array type",
                ));
            };
            let param_name = self.intern_source_name(binding.name.as_str());
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span: self.span(binding.span.start, binding.span.end),
            });
            body.params.push(local);
            self.locals.insert(binding.name.to_string(), local);
            let index = params.len();
            params.push(Param {
                name: param_name,
                local,
                ty,
                span: self.span(binding.span.start, binding.span.end),
            });
            Some(RestParam { index, item_ty })
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
                span: self.span(function.span.start, function.span.end),
            });
            if let Err(error) =
                self.binding_declaration(pattern, Some(value), Some(ty), false, &mut body, root)
            {
                errors.push(error);
            }
        }
        // Apply parameter defaults: shadow each defaulted `Optional<T>` ABI
        // parameter with a body-scoped `T` local initialized to
        // `param ?? <default>`. Later defaults may reference earlier
        // parameters, which already resolve to their applied-default locals.
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
            let fallback = match self.expression_with_hint(initializer, &mut body, Some(inner_ty)) {
                Ok(value) => value,
                Err(error) => {
                    errors.push(error);
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
                self.date_value_locals.insert(applied);
            }
            self.locals.insert(source_name, applied);
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
        if let Err(error) =
            self.predeclare_local_arrow_callbacks(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        if let Err(error) =
            self.predeclare_local_function_declarations(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        let generator_yields = function
            .generator
            .then(|| self.initialize_generator_yield_accumulator(function, &mut body));
        self.current_generator_yields = generator_yields;
        self.current_arguments_arities
            .push(function.params.items.len());
        for statement in &function_body.statements {
            if self.is_super_call_statement(statement) {
                continue;
            }
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }
        if let Some(accumulator) = generator_yields {
            self.push_generator_return(accumulator, function, &mut body);
        }
        if function.r#async {
            body.build_async_state_machine();
        }
        self.locals = saved_locals;
        self.date_value_locals = saved_date_value_locals;
        self.callable_local_props = saved_callable_local_props;
        self.explicit_any_locals = saved_explicit_any_locals;
        self.narrowed_locals = saved_narrowed_locals;
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        self.current_generator_yields = saved_generator_yields;
        self.current_arguments_arities.pop();
        self.pop_type_parameter_scope();

        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let mut return_ty = declared_return_ty
            .or_else(|| self.last_return_type(&body))
            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::None));
        if function.r#async && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
        {
            return_ty = self.ctx.krate.types.intern(Type::Future(return_ty));
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
        let item = if let Some(item) = self.local_function_items.get(name_text).copied() {
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
            self.function_rests.insert(name_text.to_owned(), rest);
            self.ctx.function_rests.insert(name_text.to_owned(), rest);
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
            self.assertion_functions.insert(
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
            self.predicate_functions.insert(
                name_text.to_owned(),
                AssertionNarrowing {
                    param_index,
                    target,
                },
            );
        }
        Ok(item)
    }

    /// Create the synthetic list that stores values yielded by a generator body.
    pub(in crate::lowering) fn initialize_generator_yield_accumulator(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        body: &mut Body,
    ) -> GeneratorYieldAccumulator {
        let item_ty = self.ctx.krate.types.intern(Type::Unknown);
        let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
        let local = body.push_local(LocalDecl {
            name: Some(self.intern_source_name("__smelt_yields")),
            ty: list_ty,
            mutable: true,
            span: self.span(function.span.start, function.span.end),
        });
        let value = body.push_expr(Expr {
            kind: ExprKind::ListLit(Vec::new()),
            ty: list_ty,
            span: self.span(function.span.start, function.span.end),
        });
        let pat = body.push_pattern(Pattern::Binding(local));
        body.push_stmt(Stmt::Let {
            pat,
            ty: list_ty,
            value: Some(value),
        });
        GeneratorYieldAccumulator {
            local,
            list_ty,
            item_ty,
        }
    }

    /// Return the materialized generator list through the function's erased boundary.
    pub(in crate::lowering) fn push_generator_return(
        &mut self,
        accumulator: GeneratorYieldAccumulator,
        function: &oxc::ast::ast::Function<'_>,
        body: &mut Body,
    ) {
        let local = body.push_expr(Expr {
            kind: ExprKind::Local(accumulator.local),
            ty: accumulator.list_ty,
            span: self.span(function.span.start, function.span.end),
        });
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let value = body.push_expr(Expr {
            kind: ExprKind::UnknownCast {
                value: local,
                target: unknown_ty,
            },
            ty: unknown_ty,
            span: self.span(function.span.start, function.span.end),
        });
        body.push_stmt(Stmt::Return(Some(value)));
    }

    /// Return whether a TypeScript formal parameter has a default value.
    pub(in crate::lowering) fn formal_parameter_has_default(param: &oxc::ast::ast::FormalParameter<'_>) -> bool {
        param.initializer.is_some() || matches!(param.pattern, BindingPattern::AssignmentPattern(_))
    }

    /// Return whether a parameter default is the type's zero value.
    ///
    /// For such defaults (`= 0`, `= ""`, `= false`, `= []`, `= {}`,
    /// `= new Map()`, `= new Set()`, `= undefined`) omitted call arguments
    /// already synthesize exactly the defaulted value, so the parameter keeps
    /// its plain (non-`Optional`) ABI. This also preserves mutable-reference
    /// threading for parameters that accumulate state across recursive calls.
    fn initializer_is_type_zero_default(initializer: &Expression<'_>) -> bool {
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
                            if callee.name == "Map" || callee.name == "Set"
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
        if param.optional && !matches!(self.ctx.krate.types.get(ty), Some(Type::Optional(_))) {
            Ok(self.ctx.krate.types.intern(Type::Optional(ty)))
        } else {
            Ok(ty)
        }
    }

    /// Lower a function expression into a module-owned HIR function item.
    ///
    /// Exported object namespace constants can contain method syntax, as in
    /// date-fns formatter tables. Each method is represented as a private
    /// module function and referenced from the namespace metadata.
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
        let type_params = self.push_type_parameter_scope(function.type_parameters.as_deref())?;
        let hinted_function =
            type_hint.and_then(|ty| match self.ctx.krate.types.get(ty).cloned() {
                Some(Type::Function(function_ty)) => Some(function_ty),
                _ => None,
            });
        let return_ty = if let Some(return_type) = &function.return_type {
            match self.ts_type_to_hir(&return_type.type_annotation) {
                Ok(value) => value,
                Err(error) => {
                    self.pop_type_parameter_scope();
                    return Err(error);
                }
            }
        } else if let Some(function_ty) = &hinted_function {
            function_ty.return_ty
        } else {
            self.function_return_type_or_overload(function, name_text)
                .unwrap_or_else(|_| self.ctx.krate.types.intern(Type::Unknown))
        };
        if function.r#async && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
        {
            self.pop_type_parameter_scope();
            return Err(SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "async functions must declare a Promise<T> return type",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_narrowed_locals = std::mem::take(&mut self.narrowed_locals);
        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        let saved_preserve_receiver = self.preserve_specialization_receiver;
        self.current_async = function.r#async;
        self.current_return_ty = Some(return_ty);
        self.preserve_specialization_receiver = receiver.is_some();
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut params = Vec::new();
        let mut errors = Vec::new();
        if let Some((_, receiver_ty)) = receiver {
            let this_name = self.intern_source_name("this");
            let this_local = body.push_local(LocalDecl {
                name: Some(this_name),
                ty: receiver_ty,
                mutable: true,
                span: self.span(function.span.start, function.span.start),
            });
            body.params.push(this_local);
            self.locals.insert("this".to_owned(), this_local);
            params.push(Param {
                name: this_name,
                local: this_local,
                ty: receiver_ty,
                span: self.span(function.span.start, function.span.start),
            });
        }

        for (index, param) in function.params.items.iter().enumerate() {
            let result = (|| {
                let ty = if let Some(annotation) = &param.type_annotation {
                    self.ts_type_to_hir(&annotation.type_annotation)?
                } else if let Some(function_ty) = &hinted_function {
                    function_ty.params.get(index).copied().ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(param.span.start, param.span.end),
                            "function expression parameter is missing from contextual callable type",
                        )
                    })?
                } else {
                    return Err(SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "function expression parameters must have explicit type annotations",
                    ));
                };
                let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                    return Err(SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "function expression parameters must be identifiers",
                    ));
                };
                let param_name = self.intern_source_name(binding.name.as_str());
                let local = body.push_local(LocalDecl {
                    name: Some(param_name),
                    ty,
                    mutable: false,
                    span: self.span(binding.span.start, binding.span.end),
                });
                body.params.push(local);
                self.locals.insert(binding.name.to_string(), local);
                params.push(Param {
                    name: param_name,
                    local,
                    ty,
                    span: self.span(binding.span.start, binding.span.end),
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
                let param_name = self.intern_source_name(&format!("__unused{index}"));
                let local = body.push_local(LocalDecl {
                    name: Some(param_name),
                    ty,
                    mutable: false,
                    span: self.span(function.span.start, function.span.end),
                });
                body.params.push(local);
                params.push(Param {
                    name: param_name,
                    local,
                    ty,
                    span: self.span(function.span.start, function.span.end),
                });
            }
        }
        for statement in &function_body.statements {
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }
        if function.r#async {
            body.build_async_state_machine();
        }

        self.locals = saved_locals;
        self.narrowed_locals = saved_narrowed_locals;
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        self.preserve_specialization_receiver = saved_preserve_receiver;
        self.pop_type_parameter_scope();
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }

        let body_id = self.ctx.krate.push_body(body);
        Ok(self.ctx.krate.push_item(Item::Function(Function {
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
        })))
    }

    /// Build a deterministic synthetic name for an anonymous class expression.
    ///
    /// Anonymous classes have no source identifier, but the rest of the class
    /// lowering pipeline keys every class on a name (`self.classes`, the interned
    /// `Type::Class` symbol, field/method metadata). The class span start offset
    /// is unique within a module, so `__smelt_anon_class_<offset>` gives a stable,
    /// collision-free name without threading a mutable counter through lowering.
    fn anonymous_class_name(class: &oxc::ast::ast::Class<'_>) -> String {
        format!("__smelt_anon_class_{}", class.span.start)
    }

    /// Lower a class declaration to HIR.
    ///
    /// Anonymous classes (`class {}` in an expression position) are named with a
    /// deterministic synthetic identifier so they register like a declared class
    /// and can be referenced as a class value; see [`Self::anonymous_class_name`].
    pub(in crate::lowering) fn class_declaration(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        // Named classes use their source identifier; anonymous class expressions
        // fall back to a synthetic name so the class still registers and can be
        // referenced as a value. The name is owned here and borrowed as
        // `class_text` for the rest of lowering.
        let class_source_name = class.id.as_ref().map_or_else(
            || Self::anonymous_class_name(class),
            |id| id.name.to_string(),
        );
        let class_name = self
            .scoped_class_type_names
            .get(&class_source_name)
            .copied()
            .unwrap_or_else(|| self.intern_type_name(&class_source_name));
        let class_name_owned = self
            .ctx
            .krate
            .symbols
            .get(class_name)
            .unwrap_or(&class_source_name)
            .to_owned();
        let class_text = class_name_owned.as_str();
        let class_span = self.span(class.span.start, class.span.end);
        let materialized = self.materialized_class(&class_source_name).cloned();
        if !class.decorators.is_empty() && materialized.is_none() {
            return Err(SmeltError::specialization_required(
                self.span(class.span.start, class.span.end),
                &class_source_name,
            ));
        }
        if materialized.is_some() {
            self.validate_specialization_adapters(self.span(class.span.start, class.span.end))?;
        }
        let type_params = self.push_type_parameter_scope(class.type_parameters.as_deref())?;
        let class_type_args = type_params
            .iter()
            .map(|param| {
                self.ctx
                    .krate
                    .types
                    .intern(Type::TypeParam { name: param.name })
            })
            .collect::<Vec<_>>();
        let class_ty = self.ctx.krate.types.intern(Type::Class {
            name: class_name,
            args: class_type_args,
        });
        let (base, base_args) = self.class_extends_clause(class)?;
        if let Some(base_name) = base {
            self.class_bases
                .insert(class_text.to_owned(), (base_name, base_args.clone()));
        }
        let mut fields = Vec::new();
        let mut field_initializers = Vec::new();
        let mut constructor = None;
        let mut methods = Vec::new();
        let mut static_methods = Vec::new();
        // Static class constants declared directly in source (`static NAME =
        // <literal>`), as opposed to `static_fields` merged from specialization.
        let mut source_static_fields: Vec<smelt_hir::StaticField> = Vec::new();
        let mut abstract_methods = Vec::new();
        let mut descriptor_accessors = Vec::new();
        // Accessor descriptors built directly from plain source getters on a
        // non-materialized class. Materialized classes derive their descriptors
        // from the manifest instead (see `merge_materialized_class_descriptors`).
        let mut source_descriptors: Vec<smelt_hir::Descriptor> = Vec::new();
        // Value type declared by a class string/number index signature, if any.
        // Recorded from the `TSIndexSignature` element below and published to the
        // index-value maps after the class is fully lowered.
        let mut class_index_value_ty = None;

        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(property) => {
                    if !property.decorators.is_empty() && materialized.is_none() {
                        return Err(SmeltError::specialization_required(
                            self.span(property.span.start, property.span.end),
                            class_text,
                        ));
                    }
                    if property.computed && !self.is_resolvable_property_key(&property.key) {
                        return Err(SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "dynamic computed property names are not lowered yet",
                        ));
                    }
                    if property.r#static {
                        if materialized.is_some() {
                            continue;
                        }
                        // A `static NAME: T = <literal>` declares a class-level
                        // constant resolvable via `Class.NAME`. Lower it to a
                        // materialized static field (issue #98). Non-literal or
                        // untyped static initializers are not yet lowered.
                        if let Some(static_field) = self.class_static_field(property)? {
                            source_static_fields.push(static_field);
                            continue;
                        }
                        // A static field initialized to a function or arrow
                        // expression (`static c = function () {};`) is a static
                        // *callable* member, not a data constant. Materializing it
                        // as an associated function needs the static-method
                        // lowering path, which is not wired to property
                        // initializers yet; until then the field is not emitted
                        // rather than blocking the whole class. This keeps classes
                        // that merely *carry* such a member lowerable (its value is
                        // a function, never a data value read back structurally);
                        // an actual `Class.c(...)` use would fail at the call site
                        // rather than silently reading a wrong value.
                        if matches!(
                            property.value,
                            Some(
                                Expression::FunctionExpression(_)
                                    | Expression::ArrowFunctionExpression(_)
                            )
                        ) {
                            continue;
                        }
                        return Err(SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "static fields require a concrete literal initializer",
                        ));
                    }
                    let name = self.property_key_symbol(&property.key)?;
                    let mut ty = if let Some(annotation) = &property.type_annotation {
                        self.ts_type_to_hir(&annotation.type_annotation)?
                    } else if let Some(value) = &property.value {
                        let mut field_body =
                            Body::new(None, self.span(value.span().start, value.span().end));
                        let value = self.expression(value, &mut field_body)?;
                        Self::expr_ty(&field_body, value)
                    } else {
                        return Err(SmeltError::unsupported(
                            self.span(property.span.start, property.span.end),
                            "class fields require explicit type annotations",
                        ));
                    };
                    if property.optional {
                        ty = self.field_type_with_optional(ty, true);
                    }
                    if let Some(value) = &property.value {
                        field_initializers.push((
                            name,
                            value,
                            ty,
                            self.span(property.span.start, property.span.end),
                        ));
                    }
                    let field_visibility =
                        if matches!(&property.key, PropertyKey::PrivateIdentifier(_)) {
                            Visibility::Private
                        } else {
                            visibility(property.accessibility)
                        };
                    fields.push(Field {
                        name,
                        ty,
                        visibility: field_visibility,
                        optional: property.optional,
                        span: self.span(property.span.start, property.span.end),
                    });
                }
                ClassElement::MethodDefinition(method)
                    if method.kind == MethodDefinitionKind::Get =>
                {
                    if !method.decorators.is_empty() {
                        if materialized.is_some() {
                            continue;
                        }
                        return Err(SmeltError::specialization_required(
                            self.span(method.span.start, method.span.end),
                            class_text,
                        ));
                    }
                    if method.computed && !self.is_resolvable_property_key(&method.key) {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "dynamic computed method names are not lowered yet",
                        ));
                    }
                    if method.r#static {
                        if materialized.is_some() {
                            continue;
                        }
                        // Static getters need associated-accessor lowering that
                        // is not implemented yet; static *methods* are handled in
                        // the method-lowering pass below.
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "static accessors are not lowered yet",
                        ));
                    }
                    // A plain source getter (`get x() { … }`) lowers to a `&self`
                    // method plus an accessor descriptor in the second class pass
                    // below, so reads of `obj.x` dispatch to the computed method
                    // instead of a dropped stored field (see the reference-class
                    // spec's finding #3). For a materialized/auto-accessor class
                    // the getter still keeps its backing field.
                    if materialized.is_some() {
                        let name = self.property_key_symbol(&method.key)?;
                        let ty = method
                            .value
                            .return_type
                            .as_ref()
                            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                            .transpose()?
                            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                        fields.push(Field {
                            name,
                            ty,
                            visibility: Visibility::Private,
                            optional: false,
                            span: self.span(method.span.start, method.span.end),
                        });
                    }
                }
                ClassElement::TSIndexSignature(sig) => {
                    // A class string/number index signature `[key: string]: T`
                    // (or `[key: number]: T`) declares a keyed store whose value
                    // type is the statically known `T`. Record `T` here (in the
                    // field-collection pass, before the index-value maps are
                    // published below) so member and computed access can fall
                    // back to it when no declared named field or method matches.
                    // Declared named fields are collected separately and keep
                    // their concrete types; the index value is only the
                    // dynamic-keyed fallback shape, never a replacement for named
                    // members.
                    let value_ty =
                        self.ts_type_to_hir(&sig.type_annotation.type_annotation)?;
                    class_index_value_ty = Some(value_ty);
                }
                _ => {}
            }
        }
        for element in &class.body.body {
            let ClassElement::MethodDefinition(method) = element else {
                continue;
            };
            if method.kind != MethodDefinitionKind::Constructor {
                continue;
            }
            for param in &method.value.params.items {
                // `readonly x` (with no explicit accessibility) is still a
                // parameter property and declares a public instance field, so a
                // constructor arg such as `constructor(readonly innerError: Error)`
                // must produce a field (was E0609 on `.innerError`).
                if param.accessibility.is_none() && !param.readonly {
                    continue;
                }
                let BindingPattern::BindingIdentifier(binding) = &param.pattern else {
                    return Err(SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "destructured constructor parameter properties are not lowered yet",
                    ));
                };
                let name = self.intern_source_name(binding.name.as_str());
                let ty = if let Some(annotation) = &param.type_annotation {
                    self.ts_type_to_hir(&annotation.type_annotation)?
                } else if let Some(default) = &param.initializer {
                    let mut initializer_body =
                        Body::new(None, self.span(default.span().start, default.span().end));
                    let value = self.expression(default, &mut initializer_body)?;
                    Self::expr_ty(&initializer_body, value)
                } else {
                    self.ctx.krate.types.intern(Type::Unknown)
                };
                fields.push(Field {
                    name,
                    ty,
                    visibility: visibility(param.accessibility),
                    optional: false,
                    span: self.span(param.span.start, param.span.end),
                });
            }
        }
        let mut static_fields = materialized
            .as_ref()
            .map_or_else(Vec::new, |materialized_class| {
                self.merge_materialized_class_members(materialized_class, &mut fields, class_span)
            });
        // Source-declared `static NAME = <literal>` constants extend the
        // materialized set; a materialized value takes precedence when a member
        // is named by both, so only append source statics not already present.
        for static_field in source_static_fields {
            if !static_fields
                .iter()
                .any(|existing| existing.name == static_field.name)
            {
                static_fields.push(static_field);
            }
        }
        let method_sigs = self.class_method_signatures(&class.body.body)?;
        let virtual_method_fields =
            self.virtual_method_field_names(&class.body.body, class.r#abstract);
        self.add_virtual_class_method_fields(&mut fields, &method_sigs, &virtual_method_fields);
        // Runtime keyed store for a class index signature (issue #84 / #18).
        //
        // A class declaring `[key: string]: T` (or `[key: number]: T`) needs a
        // real backing store so dynamic keyed writes round-trip at runtime:
        // `x[k] = v; x[k]` must return `v`, and a missing key must read as
        // `undefined`. The store is a synthesized private `Record<string, T>`
        // field that sits ALONGSIDE the declared named fields (which stay
        // concrete). It is added to the class field list here, so it flows
        // through MIR, struct emission, the `Default`/`Clone` derives, and the
        // constructor's default struct literal automatically; codegen only has
        // to route keyed access to it. The field is `Private` so it is excluded
        // from erased object projection. Number-keyed signatures still use a
        // string-keyed store because JavaScript object property keys are strings.
        //
        // Value type `T` stays concrete (`string`, `number`, a class, ...). A
        // union index value type (`[key: string]: string | number`) is subject
        // to the SAME pre-existing limitation as a plain union-typed class field:
        // the generated union enum lacks `Debug`/`Default`, so a struct storing
        // it does not derive them. That is an orthogonal codegen gap tracked
        // separately, not specific to the index store.
        if let Some(value_ty) = class_index_value_ty {
            let store_name = self.intern_source_name(CLASS_INDEX_STORE_FIELD);
            let key_ty = self.ctx.krate.types.intern(Type::String);
            let store_ty = self.ctx.krate.types.intern(Type::Dict(key_ty, value_ty));
            if !fields.iter().any(|field| field.name == store_name) {
                fields.push(Field {
                    name: store_name,
                    ty: store_ty,
                    visibility: Visibility::Private,
                    optional: false,
                    span: class_span,
                });
            }
        }
        self.class_fields
            .insert(class_text.to_owned(), fields.clone());
        self.class_methods
            .insert(class_text.to_owned(), method_sigs.clone());
        // Publish the class index-signature value type (if any) so member and
        // computed access can resolve keyed reads/writes through it while the
        // class's own method bodies are still being lowered, and so later
        // modules see it too. Named fields resolve first; this is the fallback.
        //
        // This drives the *type* level of the index signature (issue #84): the
        // statically known value type `T` types keyed reads as `Optional<T>`
        // (missing key -> undefined) and undeclared member reads as `T`, while
        // declared named fields keep their concrete types. The matching runtime
        // keyed *store* — the synthesized `__smelt_index_store` field added above
        // — makes dynamic writes round-trip: codegen routes `x[k] = v` to
        // `x.__smelt_index_store.insert(k, v)` and `x[k]` to a store lookup.
        if let Some(value_ty) = class_index_value_ty {
            self.class_index_values.insert(class_name, value_ty);
            self.ctx.class_index_values.insert(class_name, value_ty);
        }
        self.add_overridden_base_method_fields(base, &method_sigs);

        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(_) => {}
                ClassElement::MethodDefinition(method) => {
                    // An overload declaration has no JavaScript body. TypeScript
                    // validates overload applicability before Smelt runs; when
                    // the class also contains the matching concrete method, only
                    // that runtime implementation becomes HIR.
                    if method.value.body.is_none()
                        && self.class_method_has_implementation(&class.body.body, method)?
                    {
                        continue;
                    }
                    if !method.decorators.is_empty() && materialized.is_none() {
                        return Err(SmeltError::specialization_required(
                            self.span(method.span.start, method.span.end),
                            class_text,
                        ));
                    }
                    // Plain source getters are lowered as `&self` methods and
                    // registered as accessor descriptors below; setters still need
                    // the write path that is not modeled yet.
                    if materialized.is_none() && method.kind == MethodDefinitionKind::Set {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "setters are not lowered yet",
                        ));
                    }
                    if method.computed && !self.is_resolvable_property_key(&method.key) {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "dynamic computed method names are not lowered yet",
                        ));
                    }
                    if method.r#static {
                        if materialized.is_some() {
                            continue;
                        }
                        // Only plain `static foo(..)` methods are lowered as
                        // receiver-free associated functions (issue #98). Static
                        // getters/setters and computed static keys are deferred.
                        if method.kind != MethodDefinitionKind::Method {
                            return Err(SmeltError::unsupported(
                                self.span(method.span.start, method.span.end),
                                "static accessors are not lowered yet",
                            ));
                        }
                        let item = self.class_static_function(
                            class_text,
                            class_name,
                            method,
                        )?;
                        static_methods.push(item);
                        continue;
                    }
                    if method.r#type == MethodDefinitionType::TSAbstractMethodDefinition {
                        if !matches!(method.kind, MethodDefinitionKind::Method) {
                            return Err(SmeltError::unsupported(
                                self.span(method.span.start, method.span.end),
                                "abstract constructors are not lowered yet",
                            ));
                        }
                        let sig = self.abstract_class_method_sig(method)?;
                        abstract_methods.push(sig);
                        continue;
                    }
                    if !matches!(
                        method.kind,
                        MethodDefinitionKind::Constructor
                            | MethodDefinitionKind::Method
                            | MethodDefinitionKind::Get
                            | MethodDefinitionKind::Set
                    ) {
                        return Err(SmeltError::unsupported(
                            self.span(method.span.start, method.span.end),
                            "getters and setters are not lowered yet",
                        ));
                    }
                    let item = if method.kind == MethodDefinitionKind::Constructor {
                        if constructor.is_some() {
                            return Err(SmeltError::unsupported(
                                self.span(method.span.start, method.span.end),
                                "duplicate constructors are not allowed",
                            ));
                        }
                        let item = self.class_function(
                            class_text,
                            class_name,
                            class_ty,
                            method,
                            true,
                            &field_initializers,
                        )?;
                        constructor = Some(item);
                        item
                    } else {
                        let item = self.class_function(
                            class_text,
                            class_name,
                            class_ty,
                            method,
                            false,
                            &[],
                        )?;
                        if matches!(
                            method.kind,
                            MethodDefinitionKind::Get | MethodDefinitionKind::Set
                        ) {
                            descriptor_accessors.push((
                                self.property_key_symbol(&method.key)?,
                                method.kind,
                                item,
                            ));
                        }
                        // Register a source-level accessor descriptor for a plain
                        // getter so member reads dispatch to the computed method.
                        if materialized.is_none() && method.kind == MethodDefinitionKind::Get {
                            let name = self.property_key_symbol(&method.key)?;
                            let read_ty = method
                                .value
                                .return_type
                                .as_ref()
                                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                                .transpose()?
                                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                            source_descriptors.push(smelt_hir::Descriptor {
                                name,
                                read_ty,
                                write_ty: None,
                                getter: Some(item),
                                setter: None,
                                data_descriptor: false,
                                is_static: false,
                                value_fields: Vec::new(),
                            });
                        }
                        methods.push(item);
                        item
                    };
                    let _ = item;
                }
                ClassElement::AccessorProperty(accessor) => {
                    if materialized.is_some() {
                        continue;
                    }
                    if !accessor.decorators.is_empty() {
                        return Err(SmeltError::specialization_required(
                            self.span(accessor.span.start, accessor.span.end),
                            class_text,
                        ));
                    }
                    return Err(SmeltError::unsupported(
                        self.span(accessor.span.start, accessor.span.end),
                        "accessor properties are not lowered yet",
                    ));
                }
                ClassElement::StaticBlock(block) => {
                    return Err(SmeltError::unsupported(
                        self.span(block.span.start, block.span.end),
                        "static blocks are not lowered yet",
                    ));
                }
                ClassElement::TSIndexSignature(_) => {
                    // The class index signature's value type is collected in the
                    // field-collection pass above and published to the
                    // index-value maps; nothing further is emitted for the
                    // signature member itself here.
                }
            }
        }

        if let Some(materialized_class) = &materialized {
            let lifted_methods = materialized_class
                .methods
                .iter()
                .filter(|method| {
                    method.binding_mode == smelt_specialize::BindingMode::Instance
                        && self.materialized_callable_needs_lift(&method.callable)
                })
                .cloned()
                .collect::<Vec<_>>();
            for materialized_method in lifted_methods {
                let method_name =
                    self.intern_source_name(specialization::materialized_member_name(
                        &materialized_method.name,
                    ));
                let lifted = self.lift_materialized_class_callable(
                    &materialized_method.callable,
                    &materialized_method.signature,
                    specialization::materialized_member_name(&materialized_method.name),
                    class_name,
                    class_ty,
                )?;
                let original_name =
                    self.intern_source_name(&format!("__smelt_original_{}", materialized_method.name));
                let originals = methods
                    .iter()
                    .copied()
                    .filter(|item| {
                        matches!(
                            self.item_ref(*item),
                            Item::Function(function) if function.name == method_name
                        )
                    })
                    .collect::<Vec<_>>();
                for original in originals {
                    let index = usize::try_from(original.0).unwrap_or(usize::MAX);
                    if let Some(Item::Function(function)) =
                        self.ctx.krate.items.get_mut(index)
                    {
                        function.name = original_name;
                        function.owner = FunctionOwner::ClassMethod {
                            class: class_name,
                            method: original_name,
                        };
                    }
                }
                methods.push(lifted);
            }
        }

        if constructor.is_none() {
            constructor = Some(self.synthesize_default_class_constructor(
                class_text,
                class_name,
                class_ty,
                base.is_some(),
                &field_initializers,
                self.span(class.span.start, class.span.end),
            )?);
        }
        let mut instance_initializers = Vec::new();
        if let Some(materialized_class) = &materialized {
            let initializer_signature = smelt_specialize::FunctionSignature {
                parameters: Vec::new(),
                return_type: smelt_specialize::StaticType::Null,
                is_async: false,
                throws: false,
            };
            for initializer in materialized_class.initializers.iter().filter(|initializer| {
                initializer.kind == smelt_specialize::InitializerKind::Instance
            }) {
                let hidden_name = format!("__smelt_initializer_{}", initializer.order);
                let item = self.lift_materialized_class_callable(
                    &initializer.callable,
                    &initializer_signature,
                    &hidden_name,
                    class_name,
                    class_ty,
                )?;
                methods.push(item);
                instance_initializers.push((
                    item,
                    initializer.target_kind.clone(),
                    initializer.target_name.clone(),
                ));
            }
        }
        if let Some(constructor) = constructor {
            self.prepend_materialized_initializer_calls(
                constructor,
                &instance_initializers,
                class_ty,
                class_span,
            )?;
        }
        let descriptors = materialized.as_ref().map_or_else(
            || source_descriptors,
            |materialized_class| {
                self.merge_materialized_class_descriptors(
                    materialized_class,
                    &descriptor_accessors,
                    class_span,
                )
            },
        );

        let implements = class
            .implements
            .iter()
            .filter_map(|imp| self.implements_symbol(imp).transpose())
            .collect::<Result<Vec<_>, _>>()?;
        self.inject_error_marker_fields(base, class_span, &mut fields);
        let item = self.ctx.krate.push_item(Item::Class(Class {
            name: class_name,
            span: self.span(class.span.start, class.span.end),
            kind: if class.r#abstract {
                smelt_hir::ClassKind::Abstract
            } else {
                smelt_hir::ClassKind::Plain
            },
            type_params,
            base,
            base_args,
            fields,
            static_fields,
            descriptors,
            constructor,
            methods,
            static_methods,
            abstract_methods,
            implements,
        }));
        self.pop_type_parameter_scope();
        self.classes.insert(class_source_name, item);
        self.validate_implements(item)?;
        Ok(item)
    }

    /// Lower a class *expression* used in value position into a class value.
    ///
    /// A class expression (`class Foo {}` or anonymous `class {}` used as an
    /// operand, e.g. an array element `[class {}]`) evaluates to the class
    /// constructor. We lower it through the same [`Self::class_declaration`]
    /// pipeline so the class is registered (fields, methods, constructor, and a
    /// `Type::Class` symbol) exactly like a declared class, then materialize the
    /// class value the same way a bare class-name identifier does in
    /// `identifier_expression`: a `Literal::None` placeholder typed as the class
    /// so downstream code carries the class identity through the type.
    pub(in crate::lowering) fn class_expression_value(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let item = self.class_declaration(class)?;
        let Item::Class(lowered) = self.item_ref(item) else {
            return Err(SmeltError::unsupported(
                self.span(class.span.start, class.span.end),
                "class expression did not lower to a class item",
            ));
        };
        let name = lowered.name;
        let ty = self.ctx.krate.types.intern(Type::Class {
            name,
            args: Vec::new(),
        });
        Ok(body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty,
            span: self.span(class.span.start, class.span.end),
        }))
    }

    /// Collect class method signatures before lowering method bodies.
    ///
    /// The final class item is emitted after method bodies are lowered, so
    /// same-class calls need this metadata to resolve return types during the
    /// class lowering pass. The collected signatures are metadata only; method
    /// bodies still lower into regular HIR function items.
    pub(in crate::lowering) fn class_method_signatures(
        &mut self,
        elements: &[ClassElement<'_>],
    ) -> Result<Vec<MethodSig>, SmeltError> {
        let mut methods = Vec::new();
        for element in elements {
            let ClassElement::MethodDefinition(method) = element else {
                continue;
            };
            if method.kind != MethodDefinitionKind::Method || method.r#static {
                continue;
            }
            methods.push(self.abstract_class_method_sig(method)?);
        }
        Ok(methods)
    }

    /// Return whether a bodyless class overload has a matching runtime method.
    ///
    /// Matching includes the static/instance side, method kind, and fully
    /// resolved property-key symbol. An unmatched ambient method remains a
    /// named blocker instead of being silently discarded.
    fn class_method_has_implementation(
        &mut self,
        elements: &[ClassElement<'_>],
        declaration: &oxc::ast::ast::MethodDefinition<'_>,
    ) -> Result<bool, SmeltError> {
        let declaration_name = self.property_key_symbol(&declaration.key)?;
        for element in elements {
            let ClassElement::MethodDefinition(candidate) = element else {
                continue;
            };
            if candidate.value.body.is_none()
                || candidate.r#static != declaration.r#static
                || candidate.kind != declaration.kind
            {
                continue;
            }
            if self.property_key_symbol(&candidate.key)? == declaration_name {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Add callable storage slots for a class's virtual method surface.
    ///
    /// TypeScript lets concrete subclass instances flow through base-class
    /// references such as `Parser<any>`. Generated Rust needs stored callable
    /// slots so structural adapters can bind the concrete implementation
    /// instead of erasing external `baseRef.method()` calls or `this.method`
    /// reads to the abstract/base default.
    pub(in crate::lowering) fn add_virtual_class_method_fields(
        &mut self,
        fields: &mut Vec<Field>,
        methods: &[MethodSig],
        virtual_method_fields: &HashSet<smelt_hir::Symbol>,
    ) {
        let new_fields = self.virtual_class_method_fields(fields, methods, virtual_method_fields);
        fields.extend(new_fields);
    }

    /// Build callable storage slots for selected virtual class methods.
    pub(in crate::lowering) fn virtual_class_method_fields(
        &mut self,
        existing_fields: &[Field],
        methods: &[MethodSig],
        virtual_method_fields: &HashSet<smelt_hir::Symbol>,
    ) -> Vec<Field> {
        let mut fields = Vec::new();
        for method in methods {
            if !virtual_method_fields.contains(&method.name) {
                continue;
            }
            if existing_fields
                .iter()
                .chain(fields.iter())
                .any(|field| field.name == method.name)
            {
                continue;
            }
            let params = method
                .params
                .iter()
                .map(|param| param.ty)
                .collect::<Vec<_>>();
            let mutable_params =
                self.mutable_params_from_returned_tuple_state(&params, method.return_ty);
            let ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
                params,
                rest: method.rest,
                required_params: method.required_params,
                mutable_params,
                return_ty: method.return_ty,
                is_async: method.is_async,
                may_throw: false,
            }));
            fields.push(Field {
                name: method.name,
                ty,
                visibility: method.visibility,
                optional: false,
                span: method.span,
            });
        }
        fields
    }

    /// Add callable slots to a base class when a subclass overrides its method.
    ///
    /// A value stored as the base type must keep the concrete override for
    /// later `baseRef.method()` calls. This pass runs when the subclass is
    /// seen, because that is the point where override usage is known.
    pub(in crate::lowering) fn add_overridden_base_method_fields(
        &mut self,
        base: Option<smelt_hir::Symbol>,
        subclass_methods: &[MethodSig],
    ) {
        let Some(base) = base else {
            return;
        };
        let Some(base_name) = self.ctx.krate.symbols.get(base).map(str::to_owned) else {
            return;
        };
        let Some(base_methods) = self.class_methods.get(&base_name).cloned() else {
            return;
        };
        let base_method_names = base_methods
            .iter()
            .map(|method| method.name)
            .collect::<HashSet<_>>();
        let overridden_methods = subclass_methods
            .iter()
            .filter_map(|method| {
                base_method_names
                    .contains(&method.name)
                    .then_some(method.name)
            })
            .collect::<HashSet<_>>();
        if overridden_methods.is_empty() {
            return;
        }

        let existing_fields = self
            .class_fields
            .get(&base_name)
            .cloned()
            .unwrap_or_default();
        let new_fields =
            self.virtual_class_method_fields(&existing_fields, &base_methods, &overridden_methods);
        if new_fields.is_empty() {
            return;
        }
        self.class_fields
            .entry(base_name)
            .or_default()
            .extend(new_fields.clone());
        if let Some(Item::Class(class)) = self
            .ctx
            .krate
            .items
            .iter_mut()
            .find(|item| matches!(item, Item::Class(class) if class.name == base))
        {
            class.fields.extend(new_fields);
        }
    }

    /// Collect method names that require callable storage on class instances.
    ///
    /// Abstract method declarations are always virtual from the point of view
    /// of erased base-class references. Concrete methods only need storage when
    /// source reads them as values through `this.<method>`.
    pub(in crate::lowering) fn virtual_method_field_names(
        &mut self,
        elements: &[ClassElement<'_>],
        include_abstract_methods: bool,
    ) -> HashSet<smelt_hir::Symbol> {
        let mut names = HashSet::new();
        for element in elements {
            let ClassElement::MethodDefinition(method) = element else {
                continue;
            };
            if include_abstract_methods
                && method.r#type == MethodDefinitionType::TSAbstractMethodDefinition
                && method.kind == MethodDefinitionKind::Method
            {
                let name = match &method.key {
                    PropertyKey::StaticIdentifier(identifier) => Some(identifier.name.as_str()),
                    PropertyKey::PrivateIdentifier(identifier) => Some(identifier.name.as_str()),
                    PropertyKey::StringLiteral(literal) => Some(literal.value.as_str()),
                    _ => None,
                };
                if let Some(name) = name {
                    names.insert(self.intern_source_name(name));
                }
            }
            if include_abstract_methods {
                let Some(body) = &method.value.body else {
                    continue;
                };
                for statement in &body.statements {
                    self.collect_this_member_names_from_statement(statement, &mut names);
                }
            }
        }
        names
    }

    /// Collect `this.<name>` member reads from a statement subtree.
    pub(in crate::lowering) fn collect_this_member_names_from_statement(
        &mut self,
        statement: &Statement<'_>,
        names: &mut HashSet<smelt_hir::Symbol>,
    ) {
        match statement {
            Statement::VariableDeclaration(declaration) => {
                for declarator in &declaration.declarations {
                    if let Some(init) = &declarator.init {
                        self.collect_this_member_names_from_expression(init, names);
                    }
                }
            }
            Statement::ExpressionStatement(statement) => {
                self.collect_this_member_names_from_expression(&statement.expression, names);
            }
            Statement::ReturnStatement(statement) => {
                if let Some(argument) = &statement.argument {
                    self.collect_this_member_names_from_expression(argument, names);
                }
            }
            Statement::IfStatement(statement) => {
                self.collect_this_member_names_from_expression(&statement.test, names);
                self.collect_this_member_names_from_statement(&statement.consequent, names);
                if let Some(alternate) = &statement.alternate {
                    self.collect_this_member_names_from_statement(alternate, names);
                }
            }
            Statement::BlockStatement(block) => {
                for block_statement in &block.body {
                    self.collect_this_member_names_from_statement(block_statement, names);
                }
            }
            _ => {}
        }
    }

    /// Collect `this.<name>` member reads from an expression subtree.
    pub(in crate::lowering) fn collect_this_member_names_from_expression(
        &mut self,
        expression: &Expression<'_>,
        names: &mut HashSet<smelt_hir::Symbol>,
    ) {
        match expression {
            Expression::StaticMemberExpression(member) => {
                if matches!(&member.object, Expression::ThisExpression(_)) {
                    names.insert(self.intern_source_name(member.property.name.as_str()));
                }
                self.collect_this_member_names_from_expression(&member.object, names);
            }
            Expression::CallExpression(call) => {
                self.collect_this_member_names_from_expression(&call.callee, names);
                for argument in &call.arguments {
                    if let Some(argument_expression) = argument.as_expression() {
                        self.collect_this_member_names_from_expression(argument_expression, names);
                    }
                }
            }
            Expression::NewExpression(new_expr) => {
                self.collect_this_member_names_from_expression(&new_expr.callee, names);
                for argument in &new_expr.arguments {
                    if let Some(argument_expression) = argument.as_expression() {
                        self.collect_this_member_names_from_expression(argument_expression, names);
                    }
                }
            }
            Expression::BinaryExpression(binary) => {
                self.collect_this_member_names_from_expression(&binary.left, names);
                self.collect_this_member_names_from_expression(&binary.right, names);
            }
            Expression::LogicalExpression(logical) => {
                self.collect_this_member_names_from_expression(&logical.left, names);
                self.collect_this_member_names_from_expression(&logical.right, names);
            }
            Expression::UnaryExpression(unary) => {
                self.collect_this_member_names_from_expression(&unary.argument, names);
            }
            Expression::ConditionalExpression(conditional) => {
                self.collect_this_member_names_from_expression(&conditional.test, names);
                self.collect_this_member_names_from_expression(&conditional.consequent, names);
                self.collect_this_member_names_from_expression(&conditional.alternate, names);
            }
            Expression::ParenthesizedExpression(parenthesized) => {
                self.collect_this_member_names_from_expression(&parenthesized.expression, names);
            }
            Expression::ObjectExpression(object) => {
                for property in &object.properties {
                    match property {
                        ObjectPropertyKind::ObjectProperty(property) => {
                            self.collect_this_member_names_from_expression(&property.value, names);
                        }
                        ObjectPropertyKind::SpreadProperty(spread) => {
                            self.collect_this_member_names_from_expression(&spread.argument, names);
                        }
                    }
                }
            }
            Expression::ArrayExpression(array) => {
                for element in &array.elements {
                    match element {
                        ArrayExpressionElement::SpreadElement(spread) => {
                            self.collect_this_member_names_from_expression(&spread.argument, names);
                        }
                        other => {
                            if let Some(element_expression) = other.as_expression() {
                                self.collect_this_member_names_from_expression(
                                    element_expression,
                                    names,
                                );
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    /// Synthesize the implicit constructor for a TypeScript class.
    ///
    /// Base classes without an explicit constructor behave like `constructor() {}`.
    /// Derived classes behave like `constructor(...args) { super(...args) }`, so
    /// Smelt accepts one optional erased argument to keep Date-like subclass
    /// construction call-compatible while the base constructor side effect remains
    /// represented by the generated class storage.
    pub(in crate::lowering) fn synthesize_default_class_constructor(
        &mut self,
        class_text: &str,
        class_name: smelt_hir::Symbol,
        class_ty: smelt_hir::TypeId,
        has_base: bool,
        field_initializers: &[(smelt_hir::Symbol, &Expression<'_>, smelt_hir::TypeId, Span)],
        span: Span,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_class = self.current_class.replace(class_text.to_owned());
        let mut body = Body::new(None, span);
        let this_symbol = self.ctx.krate.symbols.intern("this");
        let this_local = body.push_local(LocalDecl {
            name: Some(this_symbol),
            ty: class_ty,
            mutable: true,
            span,
        });
        self.locals.insert("this".to_owned(), this_local);
        let params = if has_base {
            let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
            let arg_ty = self.ctx.krate.types.intern(Type::Optional(unknown_ty));
            let arg_symbol = self.ctx.krate.symbols.intern("_smelt_super_arg");
            let arg_local = body.push_local(LocalDecl {
                name: Some(arg_symbol),
                ty: arg_ty,
                mutable: false,
                span,
            });
            body.params.push(arg_local);
            vec![Param {
                name: arg_symbol,
                local: arg_local,
                ty: arg_ty,
                span,
            }]
        } else {
            Vec::new()
        };
        self.emit_class_field_initializers(this_local, class_ty, field_initializers, &mut body)?;
        self.locals = saved_locals;
        self.current_class = saved_class;
        let body_id = self.ctx.krate.push_body(body);
        let name = self.ctx.krate.symbols.intern("new");
        Ok(self.ctx.krate.push_item(Item::Function(Function {
            name,
            span,
            // Synthetic default constructor takes generics from the class.
            type_params: Vec::new(),
            params,
            rest: None,
            required_params: has_base.then_some(0),
            return_ty: class_ty,
            is_async: false,
            is_test: false,
            body: Some(body_id),
            owner: FunctionOwner::Constructor { class: class_name },
        })))
    }

    /// Emit TypeScript instance field initializers at the start of a constructor.
    ///
    /// JavaScript evaluates public instance field initializers for each new
    /// object before the constructor body runs. Lowering them as assignments to
    /// `this` preserves observable fields for both native field reads and later
    /// erasure through `unknown` object helpers.
    pub(in crate::lowering) fn emit_class_field_initializers(
        &mut self,
        this_local: smelt_hir::LocalId,
        class_ty: smelt_hir::TypeId,
        field_initializers: &[(smelt_hir::Symbol, &Expression<'_>, smelt_hir::TypeId, Span)],
        body: &mut Body,
    ) -> Result<(), SmeltError> {
        for (field, initializer, field_ty, span) in field_initializers {
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(this_local),
                ty: class_ty,
                span: *span,
            });
            let target = body.push_expr(Expr {
                kind: ExprKind::Field {
                    receiver,
                    field: *field,
                },
                ty: *field_ty,
                span: *span,
            });
            // Lower the initializer against the declared field type so empty
            // collection literals (`new Map()`, `[]`, `{}`) and other
            // contextually-typed expressions adopt the field's element/key
            // types instead of defaulting to `Unknown`. Without the hint an
            // empty `new Map()` for a `Map<T, string>` field infers
            // `Map<Unknown, Unknown>`, forcing a lossy key conversion at the
            // assignment that cannot collect into the generic field type (E0277).
            let value = self.expression_with_hint(initializer, body, Some(*field_ty))?;
            body.push_stmt(Stmt::Assign { target, value });
        }
        Ok(())
    }

    /// Emit constructor parameter-property assignments to `this`.
    ///
    /// TypeScript parameter properties such as `constructor(private value: T)`
    /// both declare an instance field and assign the constructor argument into
    /// that field. Class lowering already records the field itself; this helper
    /// emits the constructor-side assignment so later `this.value` reads have a
    /// concrete field in HIR/MIR and generated Rust.
    pub(in crate::lowering) fn emit_parameter_property_initializers(
        this_local: smelt_hir::LocalId,
        class_ty: smelt_hir::TypeId,
        parameter_properties: &[(
            smelt_hir::Symbol,
            smelt_hir::LocalId,
            smelt_hir::TypeId,
            Span,
        )],
        body: &mut Body,
    ) {
        for (field, local, ty, span) in parameter_properties {
            let receiver = body.push_expr(Expr {
                kind: ExprKind::Local(this_local),
                ty: class_ty,
                span: *span,
            });
            let target = body.push_expr(Expr {
                kind: ExprKind::Field {
                    receiver,
                    field: *field,
                },
                ty: *ty,
                span: *span,
            });
            let value = body.push_expr(Expr {
                kind: ExprKind::Local(*local),
                ty: *ty,
                span: *span,
            });
            body.push_stmt(Stmt::Assign { target, value });
        }
    }

    /// Inject the dynamic Error-instance marker fields into a class that extends
    /// the JavaScript `Error` builtin (or one of its standard subclasses).
    ///
    /// The `Error` constructor is a modeled host builtin, not a source-declared
    /// Smelt class, so `effective_class_fields` finds no base layout to inherit.
    /// A subclass constructor nevertheless assigns and reads the inherited
    /// `name`/`message`/`stack`/`cause` slots (`this.name = ...`, `super(msg)`),
    /// which otherwise reference struct fields that were never declared (E0609).
    ///
    /// These four properties have no statically knowable Smelt shape from the
    /// erased host `Error` boundary, so they are declared as `Unknown` — this is
    /// a genuine interop boundary with the JS builtin, matching the `SmeltUnknown`
    /// assignment the constructor body already lowers. Fields the subclass
    /// redeclares itself are left untouched. Only the direct Error-extending class
    /// needs injection; deeper subclasses inherit these through the normal
    /// flattened class layout.
    fn inject_error_marker_fields(
        &mut self,
        base: Option<smelt_hir::Symbol>,
        class_span: Span,
        fields: &mut Vec<Field>,
    ) {
        let Some(base) = base else {
            return;
        };
        let Some(base_name) = self.ctx.krate.symbols.get(base) else {
            return;
        };
        // Error-like host bases carry the same `name`/`message`/`stack`/`cause`
        // slots as `Error`. `DOMException` (and its runtime fallback to `Error`)
        // is not a source-declared class nor a `mir.classes` entry — it is a
        // const alias to a host constructor — so a subclass such as
        // `AbortError extends DOMException` inherits no fields unless we inject the
        // Error marker slots here too (was E0609 on `.message`).
        let extends_error = matches!(
            base_name,
            "Error"
                | "EvalError"
                | "RangeError"
                | "ReferenceError"
                | "SyntaxError"
                | "TypeError"
                | "URIError"
                | "AggregateError"
                | "DOMException"
        );
        if !extends_error {
            return;
        }
        let unknown_ty = self.ctx.krate.types.intern(Type::Unknown);
        let mut injected = Vec::new();
        for marker in ["name", "message", "stack", "cause"] {
            let symbol = self.ctx.krate.symbols.intern(marker);
            if fields.iter().any(|field| field.name == symbol) {
                continue;
            }
            injected.push(Field {
                name: symbol,
                ty: unknown_ty,
                visibility: Visibility::Public,
                optional: false,
                span: class_span,
            });
        }
        // Inherited slots come first so the flattened layout mirrors a real base
        // class (`effective_class_fields` orders base fields before own fields).
        injected.extend(std::mem::take(fields));
        *fields = injected;
    }

    /// Lower the single supported TypeScript `extends` shape for class declarations.
    pub(in crate::lowering) fn class_extends_clause(
        &mut self,
        class: &oxc::ast::ast::Class<'_>,
    ) -> Result<(Option<smelt_hir::Symbol>, Vec<smelt_hir::TypeId>), SmeltError> {
        let Some(super_class) = &class.super_class else {
            return Ok((None, Vec::new()));
        };
        let name = match super_class {
            Expression::Identifier(identifier) => identifier.name.to_string(),
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(object) = &member.object else {
                    return Err(SmeltError::unsupported(
                        self.span(super_class.span().start, super_class.span().end),
                        "class extends currently requires a direct base class identifier or imported namespace member",
                    ));
                };
                if !self.value_imports.contains(object.name.as_str())
                    && !self.namespace_imports.contains(object.name.as_str())
                {
                    return Err(SmeltError::unsupported(
                        self.span(super_class.span().start, super_class.span().end),
                        "class extends currently requires a direct base class identifier or imported namespace member",
                    ));
                }
                format!("{}.{}", object.name, member.property.name)
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(super_class.span().start, super_class.span().end),
                    "class extends currently requires a direct base class identifier or imported namespace member",
                ));
            }
        };
        let name = name.as_str();
        // `class X extends Object {}` names the universal root constructor as its
        // base. Every JavaScript class already descends from `Object`, so an
        // explicit `extends Object` contributes no fields, methods, or distinct
        // constructor behavior: the subclass behaves exactly like a base-less
        // class (its instances are still non-plain objects with their own
        // constructor identity). Model it as no declared base rather than
        // requiring an `Object` class item that Smelt does not synthesize. A
        // user-declared class or value import literally named `Object` shadows
        // the global and is handled through the normal declared-base path below.
        if name == "Object"
            && !self.classes.contains_key(name)
            && !self.value_imports.contains(name)
        {
            return Ok((None, Vec::new()));
        }
        let base = self.intern_type_name(name);
        // A modeled JavaScript host constructor (`Blob`, `File`, `ArrayBuffer`, the
        // boxed primitive wrappers, …) is a legitimate base even though it is not a
        // source-declared class: `smelt_stdlib::host_object` gives it a concrete
        // constructed identity, so `class File extends Blob {}` resolves the same
        // way the error/collection builtins below do. Keying on the shared registry
        // keeps this a general rule rather than a per-name allowlist.
        let allowed_builtin = smelt_stdlib::host_object_by_class(name).is_some()
            || matches!(
                name,
                "Date"
                    | "Error"
                    | "EvalError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "TypeError"
                    | "URIError"
                    | "AggregateError"
                    | "Map"
                    | "ReadonlyMap"
                    | "Set"
                    | "ReadonlySet"
            );
        if !allowed_builtin
            && !self.classes.contains_key(name)
            && !self.value_imports.contains(name)
            && !name.contains('.')
        {
            return Err(SmeltError::unsupported(
                self.span(super_class.span().start, super_class.span().end),
                format!("base class `{name}` is not declared"),
            ));
        }
        let args = class
            .super_type_arguments
            .as_ref()
            .map(|type_args| {
                type_args
                    .params
                    .iter()
                    .map(|arg| self.ts_type_to_hir(arg))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default();
        Ok((Some(base), args))
    }

    /// Lower an abstract TypeScript method declaration to a HIR method signature.
    pub(in crate::lowering) fn abstract_class_method_sig(
        &mut self,
        method: &oxc::ast::ast::MethodDefinition<'_>,
    ) -> Result<MethodSig, SmeltError> {
        if method.value.this_param.is_some() {
            return Err(SmeltError::unsupported(
                self.span(method.span.start, method.span.end),
                "this-parameter abstract methods are not lowered yet",
            ));
        }
        let _type_params =
            self.push_type_parameter_scope(method.value.type_parameters.as_deref())?;
        let return_ty = method
            .value
            .return_type
            .as_ref()
            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
            .transpose()?
            .unwrap_or_else(|| {
                let unknown = self.ctx.krate.types.intern(Type::Unknown);
                if method.value.r#async {
                    self.ctx.krate.types.intern(Type::Future(unknown))
                } else {
                    unknown
                }
            });
        let mut params = Vec::new();
        for (index, param) in method.value.params.items.iter().enumerate() {
            let ty = param
                .type_annotation
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .or_else(|| {
                    param.initializer.as_ref().and_then(|initializer| {
                        self.infer_module_global_initializer_type(initializer).ok()
                    })
                })
                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
            let (name, span) = if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                (
                    self.intern_source_name(binding.name.as_str()),
                    self.span(binding.span.start, binding.span.end),
                )
            } else {
                (
                    self.intern_source_name(&format!("__param{index}")),
                    self.span(param.span.start, param.span.end),
                )
            };
            params.push(ParamSig { name, ty, span });
        }
        let sig = MethodSig {
            name: self.property_key_symbol(&method.key)?,
            params,
            rest: None,
            required_params: None,
            return_ty,
            visibility: visibility(method.accessibility),
            is_async: matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_))),
            span: self.span(method.span.start, method.span.end),
        };
        self.pop_type_parameter_scope();
        Ok(sig)
    }

    /// Lower a class method or constructor to HIR.
    /// Lower a `static NAME: T = <literal>` class field to a static field.
    ///
    /// Returns `None` when the initializer is missing or is not a concrete
    /// literal, so the caller can reject unsupported static-field forms. The
    /// field's type comes from the annotation when present, otherwise it is
    /// inferred from the literal. It is resolvable via `Class.NAME` and lowers to
    /// a receiver-free associated accessor in Rust codegen.
    pub(in crate::lowering) fn class_static_field(
        &mut self,
        property: &oxc::ast::ast::PropertyDefinition<'_>,
    ) -> Result<Option<smelt_hir::StaticField>, SmeltError> {
        let name = self.property_key_symbol(&property.key)?;
        let Some(value_expr) = &property.value else {
            return Ok(None);
        };
        // The field type is taken from the materialized literal so the emitted
        // accessor return type always matches the concrete value's Rust type.
        let Some((literal, ty)) = self.static_field_literal(value_expr)? else {
            return Ok(None);
        };
        let visibility = if matches!(&property.key, PropertyKey::PrivateIdentifier(_)) {
            Visibility::Private
        } else {
            visibility(property.accessibility)
        };
        Ok(Some(smelt_hir::StaticField {
            name,
            ty,
            visibility,
            value: Some(literal),
            span: self.span(property.span.start, property.span.end),
        }))
    }

    /// Convert a static-field initializer expression to a concrete literal and
    /// its inferred type.
    ///
    /// Numeric literals lower to `Float`, mirroring JavaScript's single `number`
    /// type (TypeScript has no `int` spelling), so an annotated `number` static
    /// and its value agree. Returns `None` when the expression is not a
    /// supported literal form.
    fn static_field_literal(
        &mut self,
        expr: &Expression<'_>,
    ) -> Result<Option<(Literal, smelt_hir::TypeId)>, SmeltError> {
        let pair = match expr {
            Expression::StringLiteral(literal) => (
                Literal::String(literal.value.to_string()),
                self.ctx.krate.types.intern(Type::String),
            ),
            Expression::BooleanLiteral(literal) => (
                Literal::Bool(literal.value),
                self.ctx.krate.types.intern(Type::Bool),
            ),
            Expression::NumericLiteral(literal) => (
                Literal::Float(literal.value),
                self.ctx.krate.types.intern(Type::Float),
            ),
            _ => return Ok(None),
        };
        Ok(Some(pair))
    }

    /// Lower a `static foo(..)` class method to a receiver-free HIR function.
    ///
    /// This is the static counterpart to [`Self::class_function`]: the resulting
    /// function has no `this` parameter and is owned by
    /// [`FunctionOwner::ClassStaticMethod`], so codegen emits an associated
    /// function `Class::foo(..)` resolvable via qualified access `Class.foo(..)`.
    pub(in crate::lowering) fn class_static_function(
        &mut self,
        class_text: &str,
        class_name: smelt_hir::Symbol,
        method: &oxc::ast::ast::MethodDefinition<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let class_ty = self.ctx.krate.types.intern(Type::Class {
            name: class_name,
            args: Vec::new(),
        });
        self.class_function_impl(class_text, class_name, class_ty, method, false, true, &[])
    }

    /// Lower an instance method or constructor of a class to a HIR function.
    ///
    /// Non-constructor methods take an implicit leading `this` receiver;
    /// constructors run field and parameter-property initializers. Static
    /// methods use [`Self::class_static_function`] instead.
    pub(in crate::lowering) fn class_function(
        &mut self,
        class_text: &str,
        class_name: smelt_hir::Symbol,
        class_ty: smelt_hir::TypeId,
        method: &oxc::ast::ast::MethodDefinition<'_>,
        is_constructor: bool,
        field_initializers: &[(smelt_hir::Symbol, &Expression<'_>, smelt_hir::TypeId, Span)],
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        self.class_function_impl(
            class_text,
            class_name,
            class_ty,
            method,
            is_constructor,
            false,
            field_initializers,
        )
    }

    /// Shared implementation for instance, constructor, and static class
    /// functions. `is_static` suppresses the implicit `this` receiver and routes
    /// ownership to [`FunctionOwner::ClassStaticMethod`].
    #[expect(
        clippy::too_many_arguments,
        reason = "class function lowering threads the receiver/constructor flags through one shared body"
    )]
    fn class_function_impl(
        &mut self,
        class_text: &str,
        class_name: smelt_hir::Symbol,
        class_ty: smelt_hir::TypeId,
        method: &oxc::ast::ast::MethodDefinition<'_>,
        is_constructor: bool,
        is_static: bool,
        field_initializers: &[(smelt_hir::Symbol, &Expression<'_>, smelt_hir::TypeId, Span)],
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let Some(function_body) = &method.value.body else {
            return Err(SmeltError::unsupported(
                self.span(method.span.start, method.span.end),
                "declare methods are not lowered yet",
            ));
        };
        let method_name = if is_constructor {
            self.ctx.krate.symbols.intern("new")
        } else {
            let source_name = self.property_key_symbol(&method.key)?;
            let prefix = match method.kind {
                MethodDefinitionKind::Get => Some("__smelt_get_"),
                MethodDefinitionKind::Set => Some("__smelt_set_"),
                _ => None,
            };
            prefix.map_or(source_name, |prefix| {
                let source_name = self
                    .ctx
                    .krate
                    .symbols
                    .get(source_name)
                    .unwrap_or("accessor");
                self.ctx
                    .krate
                    .symbols
                    .intern(&format!("{prefix}{source_name}"))
            })
        };
        let _method_type_params =
            self.push_type_parameter_scope(method.value.type_parameters.as_deref())?;
        let return_ty = if is_constructor {
            if method.value.return_type.is_some() {
                return Err(SmeltError::unsupported(
                    self.span(method.span.start, method.span.end),
                    "constructors cannot declare return types",
                ));
            }
            class_ty
        } else if method.kind == MethodDefinitionKind::Set {
            self.ctx.krate.types.intern(Type::None)
        } else {
            method
                .value
                .return_type
                .as_ref()
                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                .transpose()?
                .unwrap_or_else(|| {
                    let unknown = self.ctx.krate.types.intern(Type::Unknown);
                    if method.value.r#async {
                        self.ctx.krate.types.intern(Type::Future(unknown))
                    } else {
                        unknown
                    }
                })
        };
        if method.value.r#async
            && !matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
        {
            return Err(SmeltError::unsupported(
                self.span(method.span.start, method.span.end),
                "async methods must declare a Promise<T> return type",
            ));
        }

        let saved_locals = std::mem::take(&mut self.locals);
        let saved_class = self.current_class.replace(class_text.to_owned());
        let saved_async = self.current_async;
        let saved_return_ty = self.current_return_ty;
        self.current_async = method.value.r#async;
        self.current_return_ty = Some(return_ty);
        let mut body = Body::new(
            None,
            self.span(function_body.span.start, function_body.span.end),
        );
        let mut params = Vec::new();
        let this_symbol = self.ctx.krate.symbols.intern("this");
        let this_local = body.push_local(LocalDecl {
            name: Some(this_symbol),
            ty: class_ty,
            mutable: true,
            span: self.span(method.span.start, method.span.start),
        });
        // A static method has no `this` receiver: it must not bind `this` and
        // must not carry a receiver parameter, so it lowers to a receiver-free
        // associated function.
        if !is_static {
            self.locals.insert("this".to_owned(), this_local);
            if !is_constructor {
                body.params.push(this_local);
                params.push(Param {
                    name: this_symbol,
                    local: this_local,
                    ty: class_ty,
                    span: self.span(method.span.start, method.span.start),
                });
            }
        }

        let mut destructured_params = Vec::new();
        let mut parameter_property_initializers = Vec::new();
        for (index, param) in method.value.params.items.iter().enumerate() {
            let ty = self.function_parameter_type(param)?;
            let (param_name, span, source_name) =
                if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                    (
                        self.intern_source_name(binding.name.as_str()),
                        self.span(binding.span.start, binding.span.end),
                        Some(binding.name.to_string()),
                    )
                } else {
                    let synthetic_name = format!("__param{index}");
                    (
                        self.intern_source_name(&synthetic_name),
                        self.span(param.span.start, param.span.end),
                        None,
                    )
                };
            let local = body.push_local(LocalDecl {
                name: Some(param_name),
                ty,
                mutable: false,
                span,
            });
            body.params.push(local);
            if let Some(source_name) = source_name {
                self.locals.insert(source_name, local);
            } else {
                destructured_params.push((&param.pattern, local, ty));
            }
            params.push(Param {
                name: param_name,
                local,
                ty,
                span,
            });
            if is_constructor && (param.accessibility.is_some() || param.readonly) {
                let field = Field {
                    name: param_name,
                    ty,
                    visibility: visibility(param.accessibility),
                    optional: false,
                    span,
                };
                self.class_fields
                    .entry(class_text.to_owned())
                    .or_default()
                    .push(field);
                parameter_property_initializers.push((param_name, local, ty, span));
            }
        }
        if is_constructor {
            self.emit_class_field_initializers(
                this_local,
                class_ty,
                field_initializers,
                &mut body,
            )?;
            Self::emit_parameter_property_initializers(
                this_local,
                class_ty,
                &parameter_property_initializers,
                &mut body,
            );
        }

        let mut errors = Vec::new();
        for (pattern, local, ty) in destructured_params {
            let root = body.root;
            let value = body.push_expr(Expr {
                kind: ExprKind::Local(local),
                ty,
                span: self.span(method.span.start, method.span.end),
            });
            if let Err(error) =
                self.binding_declaration(pattern, Some(value), Some(ty), false, &mut body, root)
            {
                errors.push(error);
            }
        }
        if let Err(error) =
            self.predeclare_local_arrow_callbacks(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        if let Err(error) =
            self.predeclare_local_function_declarations(&function_body.statements, &mut body)
        {
            errors.push(error);
        }
        for statement in &function_body.statements {
            if self.is_super_call_statement(statement) {
                continue;
            }
            if let Err(error) = self.statement(statement, &mut body) {
                errors.push(error);
            }
        }
        if method.value.r#async {
            body.build_async_state_machine();
        }
        self.locals = saved_locals;
        self.current_class = saved_class;
        self.current_async = saved_async;
        self.current_return_ty = saved_return_ty;
        self.pop_type_parameter_scope();
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        let body_id = self.ctx.krate.push_body(body);
        Ok(self.ctx.krate.push_item(Item::Function(Function {
            name: method_name,
            span: self.span(method.span.start, method.span.end),
            // Class members take generics from the owning class; per-method
            // generics are deferred alongside the generic-class increment.
            type_params: Vec::new(),
            params,
            rest: None,
            required_params: None,
            return_ty,
            is_async: method.value.r#async,
            is_test: false,
            body: Some(body_id),
            owner: if is_constructor {
                FunctionOwner::Constructor { class: class_name }
            } else if is_static {
                FunctionOwner::ClassStaticMethod {
                    class: class_name,
                    method: method_name,
                }
            } else {
                FunctionOwner::ClassMethod {
                    class: class_name,
                    method: method_name,
                }
            },
        })))
    }

    /// Return whether a constructor statement is a bare `super(...)` call.
    pub(in crate::lowering) fn is_super_call_statement(&self, statement: &Statement<'_>) -> bool {
        let Statement::ExpressionStatement(statement) = statement else {
            return false;
        };
        if self
            .source
            .get(
                usize::try_from(statement.span.start).unwrap_or(usize::MAX)
                    ..usize::try_from(statement.span.end).unwrap_or(usize::MAX),
            )
            .is_some_and(|text| text.trim_start().starts_with("super("))
        {
            return true;
        }
        let Expression::CallExpression(call) = &statement.expression else {
            return false;
        };
        matches!(call.callee, Expression::Super(_))
    }

    /// Lower a statement in the current scope.
    pub(in crate::lowering) fn statement(&mut self, statement: &Statement<'_>, body: &mut Body) -> Result<(), SmeltError> {
        self.statement_in_block(statement, body, body.root)
    }

    // Continued in the next split builder file.
}
