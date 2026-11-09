//! Class methods and statics: method signatures, abstract method signatures,
//! static fields and static functions, and the lowered method items.

use crate::SmeltError;
use crate::lowering::{ModuleBuilder, RestParam};
use super::classes::ClassFieldInit;
use crate::lowering::js_text::js_str_literal;
use crate::lowering::state::fn_ctx::BodyKind;
use crate::lowering::state::local_scope::ScopeGroups;
use crate::lowering::support::visibility;
use oxc::ast::ast::{BindingPattern, ClassElement, Expression, MethodDefinitionKind, PropertyKey};
use smelt_hir::{
    Body, ErasureReason, Expr, ExprKind, Field, Function, FunctionOwner, Item, Literal, LocalDecl,
    MethodSig, Param, ParamSig, Span, Type, Visibility,
};

impl ModuleBuilder<'_> {
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
    pub(super) fn class_method_has_implementation(
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
        self.with_method_type_parameter_scope(method.value.type_parameters.as_deref(), |this, _| {
            this.abstract_class_method_sig_in_scope(method)
        })
    }

    /// [`Self::abstract_class_method_sig`] with the method's type parameters
    /// already in scope.
    fn abstract_class_method_sig_in_scope(
        &mut self,
        method: &oxc::ast::ast::MethodDefinition<'_>,
    ) -> Result<MethodSig, SmeltError> {
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
                .unwrap_or_else(|| self.erase(ErasureReason::MissingAnnotation));
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
        // The rest parameter is the last signature parameter, as it is in the
        // method's own item.
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
                let name_text = match &rest.rest.argument {
                    BindingPattern::BindingIdentifier(binding) => binding.name.as_str().to_owned(),
                    _ => "__rest".to_owned(),
                };
                let span = self.span(rest.span.start, rest.span.end);
                // A fixed-width tuple rest is fixed parameters, named as the
                // method body names them (`lowering::tuple_rest`).
                if let Some(elements) = self.fixed_tuple_rest_elements(declared) {
                    for (index, ty) in elements.into_iter().enumerate() {
                        let name = self.intern_source_name(&format!("{name_text}__{index}"));
                        params.push(ParamSig { name, ty, span });
                    }
                    None
                } else {
                    let (ty, _) = self.rest_param_array_type(declared)?;
                    let name = self.intern_source_name(&name_text);
                    params.push(ParamSig { name, ty, span });
                    Some(params.len() - 1)
                }
            }
            None => None,
        };
        let sig = MethodSig {
            name: self.property_key_symbol(&method.key)?,
            params,
            rest,
            required_params: None,
            return_ty,
            visibility: visibility(method.accessibility),
            is_async: matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_))),
            span: self.span(method.span.start, method.span.end),
        };
        Ok(sig)
    }

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
        // See the instance-field comment above: `#name` is a private *name*
        // (never an own property), while `private`/`protected` is compile-time only.
        let visibility = if matches!(&property.key, PropertyKey::PrivateIdentifier(_)) {
            Visibility::Hidden
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
                js_str_literal(literal.value),
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
        field_initializers: &[(smelt_hir::Symbol, ClassFieldInit<'_>, smelt_hir::TypeId, Span)],
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
        field_initializers: &[(smelt_hir::Symbol, ClassFieldInit<'_>, smelt_hir::TypeId, Span)],
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
                    .intern_synthesized(&smelt_hir::SynthesizedName::new(format!("{prefix}{source_name}")))
            })
        };
        let (method_type_params, return_ty, lowered) = self.with_method_type_parameter_scope(
            method.value.type_parameters.as_deref(),
            |this, method_type_params| {
                let return_ty = if is_constructor {
                    if method.value.return_type.is_some() {
                        return Err(SmeltError::unsupported(
                            this.span(method.span.start, method.span.end),
                            "constructors cannot declare return types",
                        ));
                    }
                    class_ty
                } else if method.kind == MethodDefinitionKind::Set {
                    this.ctx.krate.types.intern(Type::None)
                } else {
                    method
                        .value
                        .return_type
                        .as_ref()
                        .map(|annotation| this.ts_type_to_hir(&annotation.type_annotation))
                        .transpose()?
                        .unwrap_or_else(|| {
                            let unknown = this.erase(ErasureReason::MissingAnnotation);
                            if method.value.r#async && !method.value.generator {
                                this.ctx.krate.types.intern(Type::Future(unknown))
                            } else {
                                unknown
                            }
                        })
                };
                if method.value.r#async
                    && !method.value.generator
                    && !matches!(this.ctx.krate.types.get(return_ty), Some(Type::Future(_)))
                {
                    return Err(SmeltError::unsupported(
                        this.span(method.span.start, method.span.end),
                        "async methods must declare a Promise<T> return type",
                    ));
                }

                let saved_class = this.current_class.replace(class_text.to_owned());
                let ctx = this.nested_fn_ctx(BodyKind::Method, method.value.r#async, Some(return_ty));
                let scope = ScopeGroups::BINDINGS;
                let lowered = this.with_nested_body(ctx, scope, |this, _| {
                    let mut body = Body::new(
                        None,
                        this.span(function_body.span.start, function_body.span.end),
                    );
                    body.set_generator(method.value.generator);
                    let mut params = Vec::new();
                    let this_symbol = this.ctx.krate.symbols.intern("this");
                    let this_local = body.push_local(LocalDecl {
                        name: Some(this_symbol),
                        ty: class_ty,
                        mutable: true,
                        span: this.span(method.span.start, method.span.start),
                    });
                    // A static method has no `this` receiver: it must not bind `this` and
                    // must not carry a receiver parameter, so it lowers to a receiver-free
                    // associated function.
                    if !is_static {
                        this.scope.bind("this".to_owned(), this_local);
                        if !is_constructor {
                            body.params.push(this_local);
                            params.push(Param {
                                name: this_symbol,
                                local: this_local,
                                ty: class_ty,
                                span: this.span(method.span.start, method.span.start),
                            });
                        }
                    }

                    let mut destructured_params = Vec::new();
                    let mut defaulted_params = Vec::new();
                    let mut parameter_property_initializers = Vec::new();
                    for (index, param) in method.value.params.items.iter().enumerate() {
                        let declared_ty = this.function_parameter_type(param)?;
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
                        // Methods and constructors take parameter defaults through exactly the
                        // machinery plain functions use (see `function_declaration_named`): the
                        // ABI parameter widens to `Optional<T>` and a body prelude applies the
                        // declared initializer, so an omitted argument evaluates the source
                        // default instead of the Rust type's zero value. Defaults that already
                        // equal the type's zero value keep the plain ABI, matching the free
                        // function rule.
                        let default_initializer = source_name
                            .is_some()
                            .then_some(param.initializer.as_ref())
                            .flatten()
                            .filter(|initializer| !this.initializer_is_type_zero_default(initializer));
                        let abi_ty = if default_initializer.is_some()
                            && !matches!(
                                this.ctx.krate.types.get(declared_ty),
                                Some(Type::Optional(_))
                            )
                        {
                            this.ctx.krate.types.intern(Type::Optional(declared_ty))
                        } else {
                            declared_ty
                        };
                        let local = body.push_local(LocalDecl {
                            name: Some(param_name),
                            ty: abi_ty,
                            mutable: false,
                            span,
                        });
                        body.params.push(local);
                        if let Some(source_name) = source_name {
                            this.scope.bind(source_name.clone(), local);
                            if let Some(initializer) = default_initializer {
                                defaulted_params
                                    .push((local, abi_ty, &**initializer, param_name, source_name, span));
                            }
                        } else {
                            destructured_params.push((&param.pattern, local, abi_ty));
                        }
                        params.push(Param {
                            name: param_name,
                            local,
                            ty: abi_ty,
                            span,
                        });
                        if is_constructor && (param.accessibility.is_some() || param.readonly) {
                            // The declared (non-`Optional`) type is the field's type: the
                            // widened ABI slot only exists to carry "argument omitted". The
                            // initializing local is resolved after the default prelude runs,
                            // so a defaulted parameter property stores the applied value.
                            let field = Field {
                                name: param_name,
                                ty: declared_ty,
                                visibility: visibility(param.accessibility),
                                optional: false,
                                span,
                            };
                            this.classes.extend_fields(class_text.to_owned(), [field]);
                            parameter_property_initializers.push((
                                param_name,
                                binding_source_name(&param.pattern).map(str::to_owned),
                                declared_ty,
                                span,
                            ));
                        }
                    }
                    // A rest parameter binds the list of the remaining arguments, exactly as
                    // in `function_declaration_named`. Methods used to skip it, so every
                    // read of the rest name in the body was an unresolved identifier.
                    let rest = match method.value.params.rest.as_ref() {
                        Some(rest) => {
                            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                                return Err(SmeltError::unsupported(
                                    this.span(rest.span.start, rest.span.end),
                                    "destructured rest parameters need rest binding lowering",
                                ));
                            };
                            let declared = match rest
                                .type_annotation
                                .as_ref()
                                .map(|annotation| this.ts_type_to_hir(&annotation.type_annotation))
                                .transpose()
                            {
                                Ok(Some(declared)) => declared,
                                Ok(None) => {
                                    let item = this.erase(ErasureReason::MissingAnnotation);
                                    this.ctx.krate.types.intern(Type::List(item))
                                }
                                Err(error) => {
                                    return Err(error);
                                }
                            };
                            // A fixed-width tuple rest is fixed parameters plus a tuple
                            // binding of them (`lowering::tuple_rest`).
                            if let Some(elements) = this.fixed_tuple_rest_elements(declared) {
                                let span = this.span(binding.span.start, binding.span.end);
                                for param in this.bind_tuple_rest_params(
                                    binding.name.as_str(),
                                    declared,
                                    &elements,
                                    span,
                                    &mut body,
                                ) {
                                    params.push(Param {
                                        name: param.name,
                                        local: param.local,
                                        ty: param.ty,
                                        span,
                                    });
                                }
                                None
                            } else {
                            let (ty, item_ty) = match this.rest_param_array_type(declared) {
                                Ok(pair) => pair,
                                Err(error) => {
                                    return Err(error);
                                }
                            };
                            let param_name = this.intern_source_name(binding.name.as_str());
                            let span = this.span(binding.span.start, binding.span.end);
                            let local = body.push_local(LocalDecl {
                                name: Some(param_name),
                                ty,
                                mutable: false,
                                span,
                            });
                            body.params.push(local);
                            this.scope.bind(binding.name.to_string(), local);
                            let index = params.len();
                            params.push(Param {
                                name: param_name,
                                local,
                                ty,
                                span,
                            });
                            Some(RestParam { index, item_ty })
                            }
                        }
                        None => None,
                    };
                    // A DERIVED constructor initializes its own fields and parameter
                    // properties when `super(...)` returns, not on entry: JavaScript runs the
                    // base constructor over the instance first, so a field the derived class
                    // redeclares (`msg = 'hi'` over the base's `msg = 'x'`) must be written
                    // after the base's initialization, never before it.
                    let defer_own_initializers = is_constructor
                        && this.classes.base(class_text).is_some()
                        && function_body
                            .statements
                            .iter()
                            .any(|statement| Self::is_super_call_statement(statement));
                    if is_constructor && !defer_own_initializers {
                        this.emit_class_field_initializers(
                            this_local,
                            class_ty,
                            field_initializers,
                            &mut body,
                        )?;
                    }

                    let mut errors = Vec::new();
                    for (pattern, local, ty) in destructured_params {
                        let root = body.root;
                        let value = body.push_expr(Expr {
                            kind: ExprKind::Local(local),
                            ty,
                            span: this.span(method.span.start, method.span.end),
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
                    // Parameter properties are emitted after the default prelude so a
                    // `constructor(public size = 10)` stores `10`, not the `Optional` slot.
                    let resolved_parameter_properties = parameter_property_initializers
                        .into_iter()
                        .filter_map(|(field, source_name, ty, span)| {
                            let local =
                                source_name.and_then(|source_name| this.scope.lookup(&source_name))?;
                            Some((field, local, ty, span))
                        })
                        .collect::<Vec<_>>();
                    if is_constructor && !defer_own_initializers {
                        Self::emit_parameter_property_initializers(
                            this_local,
                            class_ty,
                            &resolved_parameter_properties,
                            &mut body,
                        );
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
                    let generator_yields = method
                        .value
                        .generator
                        .then(|| this.initialize_generator_yield_accumulator(&method.value, &mut body));
                    this.fn_ctx.generator = generator_yields;
                    for statement in crate::lowering::hoisting::hoisted_statements(&function_body.statements) {
                        // `super(...)` has no callee to defer to once inheritance is flattened
                        // into the derived struct, so it lowers to the base's initialization
                        // applied to `this` instead of running as an ordinary call.
                        if Self::is_super_call_statement(statement) {
                            if let Err(error) = this.lower_super_call_statement(
                                statement,
                                this_local,
                                class_ty,
                                class_text,
                                &mut body,
                            ) {
                                errors.push(error);
                            }
                            if defer_own_initializers {
                                if let Err(error) = this.emit_class_field_initializers(
                                    this_local,
                                    class_ty,
                                    field_initializers,
                                    &mut body,
                                ) {
                                    errors.push(error);
                                }
                                Self::emit_parameter_property_initializers(
                                    this_local,
                                    class_ty,
                                    &resolved_parameter_properties,
                                    &mut body,
                                );
                            }
                            continue;
                        }
                        if let Err(error) = this.statement(statement, &mut body) {
                            errors.push(error);
                        }
                    }
                    if let Some(accumulator) = generator_yields {
                        Self::push_generator_return(accumulator, &method.value, &mut body);
                    }
                    if method.value.r#async {
                        body.finish_async();
                    }
                    Ok((body, params, rest, errors))
                });
                this.current_class = saved_class;
                Ok((method_type_params, return_ty, lowered))
            },
        )?;
        let (body, params, rest, errors) = lowered?;
        if let Some(error) = errors.into_iter().next() {
            return Err(error);
        }
        let body_id = self.ctx.krate.push_body(body);
        let item = self.ctx.krate.push_item(Item::Function(Function {
            name: method_name,
            span: self.span(method.span.start, method.span.end),
            // Class members take generics from the owning class; per-method
            // generics are deferred alongside the generic-class increment.
            // The declared ones are recorded in `erased_item_type_params` below
            // so calls can still tell them apart from the caller's.
            type_params: Vec::new(),
            params,
            rest: rest.map(|rest| rest.index),
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
        }));
        if !method_type_params.is_empty() {
            self.ctx.erased_item_type_params.insert(item, method_type_params);
        }
        Ok(item)
    }
}

/// Return the source identifier a parameter pattern binds, if it binds one.
///
/// Parameter properties (`constructor(public value: T)`) always bind a plain
/// identifier, so this is how their initializing local is looked back up in the
/// local scope after the parameter-default prelude may have shadowed it.
fn binding_source_name<'src>(pattern: &'src BindingPattern<'src>) -> Option<&'src str> {
    match pattern {
        BindingPattern::BindingIdentifier(binding) => Some(binding.name.as_str()),
        _ => None,
    }
}
