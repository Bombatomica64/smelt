//! `ModuleBuilder` lowering methods (part 04): type-declaration name qualification
//! and related HIR construction helpers split out of `lowering.rs`.

use crate::SmeltError;
use crate::lowering::support::statement_terminates;
use crate::lowering::state::interface_registry::LoweredInterface;
use crate::lowering::{InterfaceHeritageRef, ModuleBuilder};
use oxc::ast::ast::{
    Argument, AssignmentTarget, BindingPattern, ChainElement, Declaration, Expression, Statement,
    TSModuleDeclarationBody, TSModuleDeclarationName, TSSignature,
};
use oxc::span::GetSpan;
use oxc::syntax::operator::{AssignmentOperator, BinaryOperator, UnaryOperator};
use smelt_hir::{
    AsyncOp, BinOp, Body, DictProjectionOp, Expr, ExprKind, Field, FunctionType, Interface, Item,
    Literal, LocalDecl, MatchArm, MethodSig, ParamSig, Pattern, SetProjectionOp, Stmt, Type,
    UnaryOp, Visibility,
};

impl ModuleBuilder<'_> {
    /// Prefix a local type declaration with the active TypeScript namespace path.
    pub(in crate::lowering) fn qualified_type_declaration_name(&self, name: &str) -> String {
        self.types.qualify(name)
    }

    /// Lower a TypeScript type alias declaration to HIR.
    pub(in crate::lowering) fn type_alias_declaration(
        &mut self,
        alias: &oxc::ast::ast::TSTypeAliasDeclaration<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let local_name_text = alias.id.name.as_str();
        let name_text = self.qualified_type_declaration_name(local_name_text);
        let name = self.intern_type_name(&name_text);
        let type_params = self.push_type_parameter_scope(alias.type_parameters.as_deref())?;
        let result = self.ts_type_to_hir(&alias.type_annotation);
        let fields = self.type_fields_from_ts(&alias.type_annotation).ok();
        let is_callable_object = Self::ts_type_is_callable_object_surface(&alias.type_annotation);
        self.pop_type_parameter_scope();
        let ty = result?;
        if is_callable_object {
            self.types.mark_callable_object_alias(name);
            self.ctx.callable_object_aliases.insert(name);
        }
        if let Some(fields) = fields
            && !fields.is_empty()
        {
            self.types.set_alias_fields(name, fields.clone());
            self.ctx.type_alias_fields.insert(name, fields);
        }
        let item = self
            .ctx
            .krate
            .push_item(Item::TypeAlias(smelt_hir::TypeAlias {
                name,
                type_params,
                ty,
                span: self.span(alias.span.start, alias.span.end),
            }));
        self.items.insert(name_text, item);
        Ok(item)
    }

    /// Lower a TypeScript interface declaration to HIR.
    pub(in crate::lowering) fn interface_declaration(
        &mut self,
        interface: &oxc::ast::ast::TSInterfaceDeclaration<'_>,
    ) -> Result<smelt_hir::ItemId, SmeltError> {
        let local_name_text = interface.id.name.as_str();
        let name_text = self.qualified_type_declaration_name(local_name_text);
        let name = self.intern_type_name(&name_text);
        let type_params = self.push_type_parameter_scope(interface.type_parameters.as_deref())?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        let mut call_signatures = Vec::new();
        let mut construct_signatures = Vec::new();
        let mut index_value_ty = None;

        let mut heritage_refs = Vec::new();
        let result = (|| {
            for heritage in &interface.extends {
                let (parent_name, parent_args) = self.interface_heritage(heritage)?;
                if self.ctx.krate.symbols.get(parent_name) == Some("Date") {
                    continue;
                }
                heritage_refs.push(InterfaceHeritageRef {
                    parent: parent_name,
                    args: parent_args.clone(),
                });
                let Some(parent) = self.find_interface(parent_name).cloned() else {
                    // An extended name that is not a lowerable user interface
                    // resolves instead to a type alias, a `typeof`/namespace or
                    // value import, a dotted/qualified library type, or a global
                    // ambient lib type such as `Array`/`ArrayLike`. None of these
                    // can contribute structural fields here, but TypeScript has
                    // already validated the heritage, so the child interface
                    // keeps its own members and the parent is treated as an
                    // opaque base rather than blocking the whole file.
                    continue;
                };
                let substitutions = self.type_argument_substitution(
                    &parent.type_params,
                    &parent_args,
                    self.span(heritage.span.start, heritage.span.end),
                )?;
                fields.extend(self.substituted_fields(&parent.fields, &substitutions));
                methods.extend(self.substituted_methods(&parent.methods, &substitutions));
            }

            for sig in &interface.body.body {
                match sig {
                    TSSignature::TSPropertySignature(prop) => {
                        // A computed property signature keeps its declared field
                        // when the key statically resolves (`[K]`, `[E.Member]`,
                        // `[Symbol.iterator]`); a genuinely dynamic key has no
                        // named field to record and is skipped.
                        if prop.computed && !self.is_resolvable_property_key(&prop.key) {
                            continue;
                        }
                        let ty = prop
                            .type_annotation
                            .as_ref()
                            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                            .transpose()?
                            .ok_or_else(|| {
                                SmeltError::unsupported(
                                    self.span(prop.span.start, prop.span.end),
                                    "interface fields require explicit type annotations",
                                )
                            })?;
                        let field_ty = if prop.optional {
                            self.ctx.krate.types.intern(Type::Optional(ty))
                        } else {
                            ty
                        };
                        fields.push(Field {
                            name: self.property_key_symbol(&prop.key)?,
                            ty: field_ty,
                            visibility: Visibility::Public,
                            optional: prop.optional,
                            span: self.span(prop.span.start, prop.span.end),
                        });
                    }
                    TSSignature::TSMethodSignature(method) => {
                        if (method.computed && !self.is_resolvable_property_key(&method.key))
                            || method.this_param.is_some()
                        {
                            return Err(SmeltError::unsupported(
                                self.span(method.span.start, method.span.end),
                                "dynamic computed and this-parameter interface methods are not lowered yet",
                            ));
                        }
                        let _method_type_params =
                            self.push_type_parameter_scope(method.type_parameters.as_deref())?;
                        let result = (|| {
                            let return_ty = method
                                .return_type
                                .as_ref()
                                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                                .transpose()?
                                .ok_or_else(|| {
                                    SmeltError::unsupported(
                                        self.span(method.span.start, method.span.end),
                                        "interface methods require explicit return types",
                                    )
                                })?;
                            let mut params = Vec::new();
                            for (index, param) in method.params.items.iter().enumerate() {
                                let ty = param
                                    .type_annotation
                                    .as_ref()
                                    .map(|annotation| {
                                        self.ts_type_to_hir(&annotation.type_annotation)
                                    })
                                    .transpose()?
                                    .ok_or_else(|| {
                                        SmeltError::unsupported(
                                            self.span(param.span.start, param.span.end),
                                            "interface method parameters require explicit types",
                                        )
                                    })?;
                                // An optional parameter (`x?: T`) has the same
                                // `Optional<T>` ABI as an optional function-type
                                // parameter so under-application can pass a typed
                                // `None`. Recording it here keeps the callable
                                // method field's arity in sync with the
                                // `required_params` count below.
                                let ty = if param.optional {
                                    self.ctx.krate.types.intern(Type::Optional(ty))
                                } else {
                                    ty
                                };
                                let (param_name, param_span) =
                                    if let BindingPattern::BindingIdentifier(binding) =
                                        &param.pattern
                                    {
                                        (
                                            self.intern_source_name(binding.name.as_str()),
                                            self.span(binding.span.start, binding.span.end),
                                        )
                                    } else {
                                        (
                                            self.synthetic_param_symbol(index),
                                            self.span(param.span.start, param.span.end),
                                        )
                                    };
                                params.push(ParamSig {
                                    name: param_name,
                                    ty,
                                    span: param_span,
                                });
                            }
                            // A trailing rest parameter (`...args: T[]`) becomes
                            // the final `List<T>` slot; its index feeds the
                            // `rest` metadata so call lowering packs the tail
                            // instead of mistaking the array for a fixed param.
                            let mut rest_index = None;
                            if let Some(rest) = &method.params.rest {
                                let rest_ty = rest
                                    .type_annotation
                                    .as_ref()
                                    .map(|annotation| {
                                        self.function_type_rest_param_to_hir(
                                            &annotation.type_annotation,
                                        )
                                    })
                                    .transpose()?
                                    .ok_or_else(|| {
                                        SmeltError::unsupported(
                                            self.span(rest.span.start, rest.span.end),
                                            "interface method rest parameters require explicit array types",
                                        )
                                    })?;
                                // The rest binding's own identifier is purely
                                // type-level in an interface method signature, so
                                // a synthetic slot name is sufficient for the
                                // generated callable field.
                                rest_index = Some(params.len());
                                params.push(ParamSig {
                                    name: self.synthetic_param_symbol(params.len()),
                                    ty: rest_ty,
                                    span: self.span(rest.span.start, rest.span.end),
                                });
                            }
                            let required_params =
                                Self::formal_parameters_required_count(&method.params);
                            Ok((return_ty, params, rest_index, required_params))
                        })();
                        self.pop_type_parameter_scope();
                        let (return_ty, params, rest_index, required_params) = result?;
                        if method.optional {
                            let param_tys = params.iter().map(|param| param.ty).collect::<Vec<_>>();
                            let mutable_params = self
                                .mutable_params_from_returned_tuple_state(&param_tys, return_ty);
                            let function_ty =
                                self.ctx.krate.types.intern(Type::Function(FunctionType {
                                    params: param_tys,
                                    rest: rest_index,
                                    required_params: Some(required_params),
                                    mutable_params,
                                    return_ty,
                                    is_async: matches!(
                                        self.ctx.krate.types.get(return_ty),
                                        Some(Type::Future(_))
                                    ),
                                    may_throw: false,
                                }));
                            fields.push(Field {
                                name: self.property_key_symbol(&method.key)?,
                                ty: function_ty,
                                visibility: Visibility::Public,
                                optional: true,
                                span: self.span(method.span.start, method.span.end),
                            });
                            continue;
                        }
                        methods.push(MethodSig {
                            name: self.property_key_symbol(&method.key)?,
                            params,
                            rest: rest_index,
                            required_params: Some(required_params),
                            return_ty,
                            visibility: Visibility::Public,
                            is_async: matches!(
                                self.ctx.krate.types.get(return_ty),
                                Some(Type::Future(_))
                            ),
                            span: self.span(method.span.start, method.span.end),
                        });
                    }
                    TSSignature::TSCallSignatureDeclaration(signature) => {
                        let return_ty = signature
                            .return_type
                            .as_ref()
                            .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                            .transpose()?
                            .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                        let mut params = Vec::new();
                        for param in &signature.params.items {
                            let ty = param
                                .type_annotation
                                .as_ref()
                                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                                .transpose()?
                                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                            // Optional call-signature parameters keep the
                            // `Optional<T>` ABI so under-application can supply a
                            // typed `None`, matching the `required_params` count.
                            let ty = if param.optional {
                                self.ctx.krate.types.intern(Type::Optional(ty))
                            } else {
                                ty
                            };
                            params.push(ty);
                        }
                        // Preserve a trailing rest parameter so the call
                        // signature's runtime arity survives instead of the
                        // rest slot being lowered as a fixed array parameter.
                        let mut rest_index = None;
                        if let Some(rest) = &signature.params.rest {
                            let rest_ty = rest
                                .type_annotation
                                .as_ref()
                                .map(|annotation| {
                                    self.function_type_rest_param_to_hir(
                                        &annotation.type_annotation,
                                    )
                                })
                                .transpose()?
                                .ok_or_else(|| {
                                    SmeltError::unsupported(
                                        self.span(rest.span.start, rest.span.end),
                                        "call signature rest parameters require explicit array types",
                                    )
                                })?;
                            rest_index = Some(params.len());
                            params.push(rest_ty);
                        }
                        let required_params =
                            Self::formal_parameters_required_count(&signature.params);
                        call_signatures.push(FunctionType {
                            mutable_params: self
                                .mutable_params_from_returned_tuple_state(&params, return_ty),
                            params,
                            rest: rest_index,
                            required_params: Some(required_params),
                            return_ty,
                            is_async: matches!(
                                self.ctx.krate.types.get(return_ty),
                                Some(Type::Future(_))
                            ),
                            may_throw: false,
                        });
                    }
                    TSSignature::TSIndexSignature(index) => {
                        index_value_ty =
                            Some(self.ts_type_to_hir(&index.type_annotation.type_annotation)?);
                    }
                    TSSignature::TSConstructSignatureDeclaration(signature) => {
                        // A construct signature `new (args): T` is, at runtime,
                        // an ordinary callable value: `new value(args)` invokes
                        // it to produce a `T`. Lower it to the same
                        // `FunctionType` a `new (args) => T` constructor-type
                        // annotation produces, so a reference to this interface
                        // can resolve to a typed constructor slot (a
                        // `Type::Function`) instead of an erased dictionary. Its
                        // own type parameters are scoped so generic construct
                        // signatures resolve their parameters.
                        let _construct_type_params =
                            self.push_type_parameter_scope(signature.type_parameters.as_deref())?;
                        let result = (|| {
                            let return_ty = signature
                                .return_type
                                .as_ref()
                                .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                                .transpose()?
                                .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                            let mut params = Vec::new();
                            for param in &signature.params.items {
                                let ty = param
                                    .type_annotation
                                    .as_ref()
                                    .map(|annotation| {
                                        self.ts_type_to_hir(&annotation.type_annotation)
                                    })
                                    .transpose()?
                                    .unwrap_or_else(|| self.ctx.krate.types.intern(Type::Unknown));
                                params.push(ty);
                            }
                            Ok::<_, SmeltError>((return_ty, params))
                        })();
                        self.pop_type_parameter_scope();
                        let (return_ty, params) = result?;
                        construct_signatures.push(FunctionType {
                            mutable_params: self
                                .mutable_params_from_returned_tuple_state(&params, return_ty),
                            params,
                            rest: None,
                            required_params: None,
                            return_ty,
                            is_async: false,
                            may_throw: false,
                        });
                    }
                }
            }
            Ok(())
        })();
        self.pop_type_parameter_scope();
        result?;
        // Method signatures describe callable members. Generated Rust interface
        // structs only carry data fields, so each declared method is also given
        // a function-typed storage field of the same name. This lets a value
        // typed as the interface be invoked through the ordinary field-call
        // machinery (`receiver.method` is a function value; `receiver.method()`
        // is a closure call) exactly like a class virtual-method field, and lets
        // an object literal satisfy the interface by supplying the method as a
        // property. The `methods` list is retained so class `implements`
        // validation continues to match a class method against the requirement.
        self.add_interface_method_fields(&mut fields, &methods);
        // A callable interface (`interface F { (x: T): R; prop: … }`) is, at
        // runtime, a function value that also carries own properties. The
        // generated Rust interface struct stores the ordinary data/method
        // fields, but it also needs a slot for the underlying callable so a
        // value typed as this interface can be invoked. Append a synthetic
        // `__smelt_call` storage field typed from the (first) call signature,
        // so the struct def, `Default`, `into_smelt_unknown`, and struct-literal
        // construction all pick it up through the existing field machinery. The
        // erased callable is what `into_smelt_unknown` writes under the matching
        // `"__smelt_call"` object key that call routing later extracts.
        self.add_interface_call_signature_field(&mut fields, &call_signatures);
        let item = self.ctx.krate.push_item(Item::Interface(Interface {
            name,
            span: self.span(interface.span.start, interface.span.end),
            type_params,
            extends: heritage_refs
                .iter()
                .map(|heritage| smelt_hir::InterfaceHeritage {
                    parent: heritage.parent,
                    args: heritage.args.clone(),
                })
                .collect(),
            fields,
            methods,
        }));
        // One call records the item, the locally-lowered mark and every sidecar
        // together; see `InterfaceRegistry` for why they cannot be split.
        self.interfaces.register_lowered(LoweredInterface {
            name,
            name_text,
            item,
            extends: heritage_refs.clone(),
            call_signatures: call_signatures.clone(),
            construct_signatures: construct_signatures.clone(),
            index_value_ty,
        });
        // Mirror the same facts into the shared context for later modules.
        self.ctx.interface_extends.insert(name, heritage_refs);
        self.ctx
            .interface_call_signatures
            .insert(name, call_signatures);
        if !construct_signatures.is_empty() {
            self.ctx
                .interface_construct_signatures
                .insert(name, construct_signatures);
        }
        if let Some(index_value_ty) = index_value_ty {
            self.ctx.interface_index_values.insert(name, index_value_ty);
        }
        Ok(item)
    }

    /// Add function-typed storage fields for an interface's method signatures.
    ///
    /// A TypeScript interface method such as `count(): number` is a callable
    /// member, but the generated Rust interface struct only stores data fields.
    /// This mirrors the class virtual-method-field lowering
    /// (`add_virtual_class_method_fields`): each method becomes a field of the
    /// same name whose type is the corresponding `Type::Function`, so
    /// `receiver.count` reads a callable value and `receiver.count()` lowers
    /// through the shared field-call path instead of the class-only
    /// `resolve_method` machinery. An existing data field of the same name (for
    /// example a method also declared as a property signature) is left
    /// untouched.
    pub(in crate::lowering) fn add_interface_method_fields(
        &mut self,
        fields: &mut Vec<Field>,
        methods: &[MethodSig],
    ) {
        for method in methods {
            if fields.iter().any(|field| field.name == method.name) {
                continue;
            }
            let params = method
                .params
                .iter()
                .map(|param| param.ty)
                .collect::<Vec<_>>();
            let mutable_params =
                self.mutable_params_from_returned_tuple_state(&params, method.return_ty);
            let function_ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
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
                ty: function_ty,
                visibility: method.visibility,
                optional: false,
                span: method.span,
            });
        }
    }

    /// Add the synthetic `__smelt_call` storage field for a callable interface.
    ///
    /// A TypeScript interface with one or more call signatures
    /// (`interface F { (x: T): R; … }`) describes a value that is invoked like a
    /// function while still carrying its declared data/method fields. The
    /// generated Rust interface struct cannot itself be a closure, so the
    /// underlying callable is kept in a dedicated `__smelt_call` field typed
    /// from the first call signature. Call routing invokes a value of this
    /// interface by erasing it (`into_smelt_unknown`, which writes this field
    /// under the `"__smelt_call"` object key) and extracting that callable.
    ///
    /// The name matches the object key produced by `CallableObjectAssign`
    /// construction (`callable_object_assign_text`) and read by the erased-call
    /// coercion. A pre-existing field named `__smelt_call` (which user source
    /// cannot legally declare) is left untouched. Interfaces with no call
    /// signature are unchanged.
    pub(in crate::lowering) fn add_interface_call_signature_field(
        &mut self,
        fields: &mut Vec<Field>,
        call_signatures: &[FunctionType],
    ) {
        let Some(signature) = call_signatures.first() else {
            return;
        };
        let name = self.ctx.krate.symbols.intern("__smelt_call");
        if fields.iter().any(|field| field.name == name) {
            return;
        }
        let function_ty = self
            .ctx
            .krate
            .types
            .intern(Type::Function(signature.clone()));
        let span = self.span(0, 0);
        fields.push(Field {
            name,
            ty: function_ty,
            visibility: Visibility::Public,
            optional: false,
            span,
        });
    }

    /// Lower TypeScript namespace declarations that contain exported type declarations.
    pub(in crate::lowering) fn type_namespace_declaration(
        &mut self,
        module_decl: &oxc::ast::ast::TSModuleDeclaration<'_>,
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let Some(namespace_name) = Self::type_namespace_name(&module_decl.id) else {
            return Ok(Vec::new());
        };
        self.types.push_namespace(namespace_name);
        let result = self.type_namespace_body(module_decl.body.as_ref());
        self.types.pop_namespace();
        result
    }

    /// Return the source namespace identifier for namespace declarations.
    pub(in crate::lowering) fn type_namespace_name(
        name: &TSModuleDeclarationName<'_>,
    ) -> Option<String> {
        match name {
            TSModuleDeclarationName::Identifier(ident) => Some(ident.name.to_string()),
            TSModuleDeclarationName::StringLiteral(_) => None,
        }
    }

    /// Lower exported type declarations from a namespace body.
    pub(in crate::lowering) fn type_namespace_body(
        &mut self,
        body: Option<&TSModuleDeclarationBody<'_>>,
    ) -> Result<Vec<smelt_hir::ItemId>, SmeltError> {
        let Some(body) = body else {
            return Ok(Vec::new());
        };
        match body {
            TSModuleDeclarationBody::TSModuleDeclaration(module_decl) => {
                self.type_namespace_declaration(module_decl)
            }
            TSModuleDeclarationBody::TSModuleBlock(block) => {
                let mut items = Vec::new();
                for statement in &block.body {
                    match statement {
                        Statement::TSTypeAliasDeclaration(alias) => {
                            items.push(self.type_alias_declaration(alias)?);
                        }
                        Statement::TSInterfaceDeclaration(interface) => {
                            items.push(self.interface_declaration(interface)?);
                        }
                        Statement::TSModuleDeclaration(module_decl) => {
                            items.extend(self.type_namespace_declaration(module_decl)?);
                        }
                        Statement::ExportNamedDeclaration(export) => {
                            let Some(decl) = &export.declaration else {
                                continue;
                            };
                            match decl {
                                Declaration::TSTypeAliasDeclaration(alias) => {
                                    items.push(self.type_alias_declaration(alias)?);
                                }
                                Declaration::TSInterfaceDeclaration(interface) => {
                                    items.push(self.interface_declaration(interface)?);
                                }
                                Declaration::TSModuleDeclaration(module_decl) => {
                                    items.extend(self.type_namespace_declaration(module_decl)?);
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
                Ok(items)
            }
        }
    }

    /// Type hint for the value of a `return` statement in the current function.
    ///
    /// For a synchronous function this is simply the declared return type. For
    /// an `async` function the declared return type is `Promise<Inner>`
    /// (`Type::Future`), yet the value produced by a `return X` statement is the
    /// resolved `Inner`, because the async lowering itself is responsible for
    /// wrapping the body into the promise. Hinting the raw `Future` type here
    /// makes literal returns (tuples, arrays, objects) get coerced into a
    /// promise around a non-future value; unwrapping one `Future` layer keeps
    /// the returned expression at the value type it actually has.
    pub(in crate::lowering) fn return_statement_value_hint(&self) -> Option<smelt_hir::TypeId> {
        let return_ty = self.current_return_ty?;
        if self.current_async
            && let Some(inner) = self.future_inner_type(return_ty)
        {
            return Some(inner);
        }
        if let Some(Type::Generator { return_ty, .. }) = self.ctx.krate.types.get(return_ty) {
            return Some(*return_ty);
        }
        Some(return_ty)
    }

    /// Lower a statement within a specific block.
    pub(in crate::lowering) fn statement_in_block(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let previous_statement_block = self.current_statement_block.replace(block);
        let result = match statement {
            Statement::EmptyStatement(_) => Ok(()),
            Statement::VariableDeclaration(decl) => self.variable_declaration(decl, body, block),
            Statement::FunctionDeclaration(function) => {
                self.local_function_declaration(function, body, block)
            }
            Statement::ClassDeclaration(class) => {
                self.class_declaration(class)?;
                Ok(())
            }
            Statement::TSTypeAliasDeclaration(alias) => {
                self.type_alias_declaration(alias)?;
                Ok(())
            }
            Statement::TSInterfaceDeclaration(interface) => {
                self.interface_declaration(interface)?;
                Ok(())
            }
            Statement::TSModuleDeclaration(_) => Ok(()),
            Statement::ExpressionStatement(expr_stmt) => {
                if let Expression::SequenceExpression(sequence) =
                    Self::unparenthesized_expression(&expr_stmt.expression)
                {
                    for expression in &sequence.expressions {
                        self.sequence_expression_statement(
                            expression,
                            expression.span(),
                            body,
                            block,
                        )?;
                    }
                    return Ok(());
                }
                // `Foo.prototype.x = …` assignments for a constructor function
                // were folded into the synthesized class during the block
                // prepass, so the assignment statement itself is dropped.
                if self.is_synthesized_prototype_assignment(&expr_stmt.expression) {
                    return Ok(());
                }
                if self.inline_runtime_lifecycle_setup(&expr_stmt.expression, body, block)? {
                    return Ok(());
                }
                if self.is_test_framework_statement(&expr_stmt.expression) {
                    return Ok(());
                }
                if Self::is_vitest_mock_statement(&expr_stmt.expression)
                    || Self::is_top_level_dynamic_import_await(&expr_stmt.expression)
                {
                    return Ok(());
                }
                if let Expression::CallExpression(call) = &expr_stmt.expression
                    && self.for_each_statement(call, body, block)?
                {
                    return Ok(());
                }
                if let Expression::CallExpression(call) = &expr_stmt.expression
                    && self.expect_matcher_statement(call, body)?
                {
                    return Ok(());
                }
                if let Expression::CallExpression(call) = &expr_stmt.expression
                    && self.node_assert_statement(call, body)?
                {
                    return Ok(());
                }
                if let Expression::AssignmentExpression(assign) = &expr_stmt.expression {
                    // A write to a lifted mutable global desugars to a
                    // `GlobalSet` expression statement; it must intercept before
                    // the discard-only module-global path below.
                    if let Some(set) = self.try_global_assignment_expression(assign, body)? {
                        body.push_stmt_to_block(block, Stmt::Expr(set));
                        return Ok(());
                    }
                    if block == body.root
                        && self.module_global_assignment_statement(assign, body, block)?
                    {
                        return Ok(());
                    }
                    if self.try_lower_negative_bracket_write_statement(assign, body, block)? {
                        return Ok(());
                    }
                    if self.try_lower_list_length_assignment_statement(assign, body, block)? {
                        return Ok(());
                    }
                    if self.array_destructuring_assignment_statement(assign, body, block)? {
                        return Ok(());
                    }
                    if self.try_collect_callable_local_prop(assign, body, block)? {
                        return Ok(());
                    }
                    let (target, value) = self.assignment_parts(assign, body)?;
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                    return Ok(());
                }
                if let Expression::UpdateExpression(update) = &expr_stmt.expression {
                    // Statement-position `++`/`--` of a lifted mutable global
                    // discards its result, so the old-value temp is skipped.
                    if let Some(set) = self.try_global_update_expression(update, body, false)? {
                        body.push_stmt_to_block(block, Stmt::Expr(set));
                        return Ok(());
                    }
                    let (target, value) = self.update_parts(update, body)?;
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                    return Ok(());
                }
                if let Expression::YieldExpression(yield_expr) = &expr_stmt.expression
                    && self.generator_yield_statement(yield_expr, body, block)?
                {
                    return Ok(());
                }
                let assertion_narrowing = self.assertion_call_narrowing(&expr_stmt.expression);
                let expr = self.expression(&expr_stmt.expression, body)?;
                let expr = if matches!(
                    self.ctx.krate.types.get(Self::expr_ty(body, expr)),
                    Some(Type::Future(_))
                ) {
                    let none_ty = self.ctx.krate.types.intern(Type::None);
                    body.push_expr(Expr {
                        kind: ExprKind::AsyncOp {
                            op: AsyncOp::SpawnLocal,
                            args: vec![expr],
                        },
                        ty: none_ty,
                        span: self.span(expr_stmt.span.start, expr_stmt.span.end),
                    })
                } else {
                    expr
                };
                body.push_stmt_to_block(block, Stmt::Expr(expr));
                if let Some((name, target)) = assertion_narrowing {
                    self.apply_narrowing(name, target);
                }
                Ok(())
            }
            Statement::ReturnStatement(return_stmt) => {
                // Inside an `async` function the declared return type is
                // `Promise<Inner>` (a `Type::Future`), but a `return X` statement
                // yields the *resolved* value `Inner`, not the promise itself:
                // the async lowering wraps the whole body into the future. Hint
                // the returned expression with the unwrapped inner type so a
                // tuple/array/object literal lowers to `Inner` directly instead
                // of being coerced into a `SmeltPromise::from_future(..)` around
                // a non-future value.
                let return_hint = self.return_statement_value_hint();
                let value = return_stmt
                    .argument
                    .as_ref()
                    .map(|argument| self.expression_with_hint(argument, body, return_hint))
                    .transpose()?;
                body.push_stmt_to_block(block, Stmt::Return(value));
                Ok(())
            }
            Statement::IfStatement(if_stmt) => {
                let cond = self.condition_expression(&if_stmt.test, body)?;
                let then_narrowing = self.guard_narrowing(&if_stmt.test, body);
                // Assignments performed only within this branch are flow facts
                // for the branch, not for statements reached from either path.
                self.scope
                    .push_narrowing_scope(then_narrowing.unwrap_or_default());
                let then_block = self.block_from_statement(&if_stmt.consequent, body)?;
                self.scope.pop_narrowing_scope();
                let else_narrowing = self.inverse_guard_narrowing(&if_stmt.test, body);
                let else_block = if let Some(alternate) = &if_stmt.alternate {
                    self.scope
                        .push_narrowing_scope(else_narrowing.unwrap_or_default());
                    let else_block = self.block_from_statement(alternate, body)?;
                    self.scope.pop_narrowing_scope();
                    Some(else_block)
                } else {
                    None
                };
                body.push_stmt_to_block(
                    block,
                    Stmt::If {
                        cond,
                        then_block,
                        else_block,
                    },
                );
                if if_stmt.alternate.is_none()
                    && Self::statement_must_exit(&if_stmt.consequent)
                    && let Some(narrowing) = self.inverse_guard_narrowing(&if_stmt.test, body)
                {
                    for (name, target) in narrowing {
                        self.apply_narrowing(name, target);
                    }
                }
                // Default-initialization idiom `if (x == null) { x = <value>; }`:
                // both paths leave `x` non-null (the not-taken path by the nullish
                // guard's inverse, the taken path by the reassignment), so narrow
                // `x` to its non-null type after the `if`. This runs alongside the
                // must-exit case above (a reassigning branch does not exit) and lets
                // later reads/writes such as `x[i] = ...` see the concrete list.
                if if_stmt.alternate.is_none()
                    && let Some((name, non_null_ty)) =
                        self.optional_none_inverse_guard(&if_stmt.test, body)
                    && Self::branch_reassigns_to_nonnull(&if_stmt.consequent, &name)
                {
                    self.apply_narrowing(name, non_null_ty);
                }
                // Branch-join narrowing for the two-armed form
                // `if (cond) { x = <nonnull>; } else { x = <nonnull>; }`: both
                // arms leave `x` non-null on their respective paths, so `x` is
                // non-null after the join regardless of which arm ran. Narrow
                // every optional local that is reassigned to a non-null value at
                // the top level of *both* arms to its non-null type; this lets
                // later reads/writes see the concrete type instead of the
                // declared `Optional<T>` (e.g. `fromIndex?: number` reassigned in
                // both arms then indexed as `f64`).
                if let Some(alternate) = &if_stmt.alternate {
                    for name in Self::branch_top_level_assigned_names(&if_stmt.consequent) {
                        if Self::branch_reassigns_to_nonnull(&if_stmt.consequent, &name)
                            && Self::branch_reassigns_to_nonnull(alternate, &name)
                            && let Some(non_null_ty) = self.optional_local_nonnull_type(&name, body)
                        {
                            self.apply_narrowing(name, non_null_ty);
                        }
                    }
                }
                Ok(())
            }
            Statement::WhileStatement(while_stmt) => {
                if let Some((cond, loop_body, update_target, update_value)) =
                    self.while_assignment_condition_body(while_stmt, body, block)?
                {
                    body.push_stmt_to_block(
                        block,
                        Stmt::WhileUpdate {
                            cond,
                            body: loop_body,
                            update_target,
                            update_value,
                        },
                    );
                    return Ok(());
                }
                let cond = self.condition_expression(&while_stmt.test, body)?;
                let loop_narrowing = self.guard_narrowing(&while_stmt.test, body);
                self.scope
                    .push_narrowing_scope(loop_narrowing.unwrap_or_default());
                let loop_body = self.block_from_statement(&while_stmt.body, body)?;
                self.scope.pop_narrowing_scope();
                body.push_stmt_to_block(
                    block,
                    Stmt::While {
                        cond,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::DoWhileStatement(do_while) => {
                let loop_body = self.block_from_statement(&do_while.body, body)?;
                let cond = self.condition_expression(&do_while.test, body)?;
                let negated = body.push_expr(Expr {
                    kind: ExprKind::UnaryOp {
                        op: UnaryOp::Not,
                        operand: cond,
                    },
                    ty: self.ctx.krate.types.intern(Type::Bool),
                    span: self.span(do_while.test.span().start, do_while.test.span().end),
                });
                let break_block =
                    body.push_block(self.span(do_while.span.start, do_while.span.end));
                body.push_stmt_to_block(break_block, Stmt::Break);
                body.push_stmt_to_block(
                    loop_body,
                    Stmt::If {
                        cond: negated,
                        then_block: break_block,
                        else_block: None,
                    },
                );
                let true_expr = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Bool(true)),
                    ty: self.ctx.krate.types.intern(Type::Bool),
                    span: self.span(do_while.span.start, do_while.span.end),
                });
                body.push_stmt_to_block(
                    block,
                    Stmt::While {
                        cond: true_expr,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::ForOfStatement(for_stmt) => {
                let saved_locals = self.scope.snapshot_bindings();
                let mut iter = self.expression(&for_stmt.right, body)?;
                if for_stmt.r#await
                    && let Some(Type::Future(inner)) =
                        self.ctx.krate.types.get(Self::expr_ty(body, iter)).cloned()
                {
                    iter = body.push_expr(Expr {
                        kind: ExprKind::Await(iter),
                        ty: inner,
                        span: self.span(for_stmt.right.span().start, for_stmt.right.span().end),
                    });
                }
                // A synchronous generator receiver has no indexable Rust
                // representation, so it cannot flow through the index-based
                // `Stmt::For` lowering. Drain it through the resume protocol
                // instead. Async generators keep the existing path (their
                // resume returns a future).
                if let Some(Type::Generator {
                    is_async: false,
                    yield_ty,
                    return_ty,
                    ..
                }) = self.ctx.krate.types.get(Self::expr_ty(body, iter)).cloned()
                {
                    let result = self.lower_generator_for_of(
                        for_stmt, iter, yield_ty, return_ty, body, block,
                    );
                    self.scope.restore_bindings(saved_locals);
                    return result;
                }
                let iter = self.for_of_iterable(iter, &for_stmt.right, body);
                let destructured =
                    self.for_left_destructuring(&for_stmt.left, Self::expr_ty(body, iter), body)?;
                let (pat, loop_body) =
                    if let Some((pat, value, binding, annotated_ty, mutable)) = destructured {
                        let loop_body = body.push_block(self.statement_span(&for_stmt.body));
                        self.binding_declaration(
                            binding,
                            Some(value),
                            annotated_ty,
                            mutable,
                            body,
                            loop_body,
                        )?;
                        if let Statement::BlockStatement(block_stmt) = &for_stmt.body {
                            for nested_statement in &block_stmt.body {
                                self.statement_in_block(nested_statement, body, loop_body)?;
                            }
                        } else {
                            self.statement_in_block(&for_stmt.body, body, loop_body)?;
                        }
                        (pat, loop_body)
                    } else {
                        (
                            self.for_left_pattern(&for_stmt.left, Self::expr_ty(body, iter), body)?,
                            self.block_from_statement(&for_stmt.body, body)?,
                        )
                    };
                self.scope.restore_bindings(saved_locals);
                body.push_stmt_to_block(
                    block,
                    Stmt::For {
                        pat,
                        iter,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::ForInStatement(for_stmt) => {
                let saved_locals = self.scope.snapshot_bindings();
                let iter = self.for_in_iterable(&for_stmt.right, body)?;
                let destructured =
                    self.for_left_destructuring(&for_stmt.left, Self::expr_ty(body, iter), body)?;
                let (pat, loop_body) =
                    if let Some((pat, value, binding, annotated_ty, mutable)) = destructured {
                        let loop_body = body.push_block(self.statement_span(&for_stmt.body));
                        self.binding_declaration(
                            binding,
                            Some(value),
                            annotated_ty,
                            mutable,
                            body,
                            loop_body,
                        )?;
                        if let Statement::BlockStatement(block_stmt) = &for_stmt.body {
                            for nested_statement in &block_stmt.body {
                                self.statement_in_block(nested_statement, body, loop_body)?;
                            }
                        } else {
                            self.statement_in_block(&for_stmt.body, body, loop_body)?;
                        }
                        (pat, loop_body)
                    } else {
                        (
                            self.for_left_pattern(&for_stmt.left, Self::expr_ty(body, iter), body)?,
                            self.block_from_statement(&for_stmt.body, body)?,
                        )
                    };
                self.scope.restore_bindings(saved_locals);
                body.push_stmt_to_block(
                    block,
                    Stmt::For {
                        pat,
                        iter,
                        body: loop_body,
                    },
                );
                Ok(())
            }
            Statement::ForStatement(for_stmt) => self.c_for_statement(for_stmt, body, block),
            Statement::SwitchStatement(switch_stmt) => {
                // A switch with real fallthrough (a case body that can reach
                // the next case) cannot lower to a HIR `Match`; route it
                // through the general single-iteration-loop lowering where JS
                // `break` maps to the loop break.
                if Self::switch_has_fallthrough(switch_stmt) {
                    return self.switch_fallthrough_statement(switch_stmt, body, block);
                }
                let scrutinee = self.expression(&switch_stmt.discriminant, body)?;
                // A switch on a discriminant property (`switch (x.kind)`) proves,
                // inside every non-default arm, that `x` carries that property.
                // The narrowing is only recorded for arms with a case label; the
                // `default` arm is reached when no label matched and cannot rely
                // on the discriminant being present. Nullish/dynamic boundaries
                // are left untouched: the fact only projects concrete union arms.
                let discriminant_narrowing =
                    self.switch_discriminant_narrowing(&switch_stmt.discriminant, body);
                // A `switch (typeof x)` discriminates a union the way a chain of
                // `if (typeof x === 'k')` guards would: each labeled arm proves
                // `x` is the member(s) whose runtime `typeof` matches that arm's
                // label. Unlike the field-discriminant fact above, this narrowing
                // is per-arm, so the arm's own label(s) drive it. `pending_kinds`
                // accumulates the string labels of grouped empty cases
                // (`case 'a': case 'b':`) sharing the next arm's body; a `None`
                // entry marks a label we could not read as a `typeof` kind, which
                // disables narrowing for that group.
                let typeof_switch_local = Self::typeof_identifier_name(&switch_stmt.discriminant);
                let mut pending_kinds: Vec<Option<String>> = Vec::new();
                let mut arms = Vec::new();
                let mut default = None;
                let mut pending_empty_labels = Vec::new();

                let case_count = switch_stmt.cases.len();
                for (case_index, case) in switch_stmt.cases.iter().enumerate() {
                    if case.consequent.is_empty() {
                        if let Some(test) = &case.test {
                            if typeof_switch_local.is_some() {
                                pending_kinds.push(Self::string_literal_value(test));
                            }
                            pending_empty_labels.push(self.literal_case_label(test)?);
                            continue;
                        }
                    }
                    // Discriminant facts apply to labeled arms only; the default
                    // arm handles the "no label matched" path and must not assume
                    // the discriminant property is present.
                    let narrowing_pushed = if case.test.is_some() {
                        // Prefer the per-arm `typeof` fact when the discriminant is
                        // `typeof x`; otherwise project the shared field-discriminant
                        // fact. Grouped labels for this arm (`pending_kinds`) union
                        // with the arm's own label, and a group we could not read as
                        // kinds records no fact.
                        let typeof_fact = typeof_switch_local.as_ref().and_then(|name| {
                            let group = std::mem::take(&mut pending_kinds);
                            let current = case.test.as_ref().and_then(Self::string_literal_value);
                            match current {
                                Some(current) if group.iter().all(Option::is_some) => {
                                    let mut kinds: Vec<String> =
                                        group.into_iter().flatten().collect();
                                    kinds.push(current);
                                    self.switch_typeof_case_narrowing(name, &kinds, body)
                                }
                                _ => None,
                            }
                        });
                        let fact = typeof_fact.or_else(|| discriminant_narrowing.clone());
                        if let Some((name, target)) = fact {
                            self.apply_narrowing_scope(name, target);
                            true
                        } else {
                            false
                        }
                    } else {
                        // The `default` arm proves no discriminant fact; drop any
                        // pending `typeof` group kinds that fell through to it.
                        pending_kinds.clear();
                        false
                    };
                    let case_block = body.push_block(self.span(case.span.start, case.span.end));
                    let mut saw_break = false;
                    for case_statement in &case.consequent {
                        if let Statement::BlockStatement(block_stmt) = case_statement {
                            for nested_statement in &block_stmt.body {
                                if matches!(nested_statement, Statement::ContinueStatement(_)) {
                                    return Err(SmeltError::unsupported(
                                        self.statement_span(nested_statement),
                                        "switch continue lowering is not implemented yet",
                                    ));
                                }
                                if matches!(nested_statement, Statement::BreakStatement(_)) {
                                    saw_break = true;
                                    break;
                                }
                                self.statement_in_block(nested_statement, body, case_block)?;
                            }
                            if saw_break {
                                break;
                            }
                            continue;
                        }
                        if matches!(case_statement, Statement::ContinueStatement(_)) {
                            return Err(SmeltError::unsupported(
                                self.statement_span(case_statement),
                                "switch continue lowering is not implemented yet",
                            ));
                        }
                        if matches!(case_statement, Statement::BreakStatement(_)) {
                            saw_break = true;
                            break;
                        }
                        self.statement_in_block(case_statement, body, case_block)?;
                    }
                    if narrowing_pushed {
                        self.scope.pop_narrowing_scope();
                    }
                    let is_last_case = case_index + 1 == case_count;
                    if !saw_break
                        && !is_last_case
                        && !case.consequent.iter().any(statement_terminates)
                    {
                        return Err(SmeltError::unsupported(
                            self.span(case.span.start, case.span.end),
                            "switch fallthrough is not lowered yet; each case must break, return, or throw",
                        ));
                    }

                    if let Some(test) = &case.test {
                        for label in std::mem::take(&mut pending_empty_labels) {
                            arms.push(MatchArm {
                                label,
                                body: case_block,
                            });
                        }
                        arms.push(MatchArm {
                            label: self.literal_case_label(test)?,
                            body: case_block,
                        });
                    } else if default.replace(case_block).is_some() {
                        return Err(SmeltError::unsupported(
                            self.span(case.span.start, case.span.end),
                            "switch statements can only have one default case",
                        ));
                    } else {
                        for label in std::mem::take(&mut pending_empty_labels) {
                            arms.push(MatchArm {
                                label,
                                body: case_block,
                            });
                        }
                    }
                }
                if !pending_empty_labels.is_empty() {
                    return Err(SmeltError::unsupported(
                        self.span(switch_stmt.span.start, switch_stmt.span.end),
                        "switch fallthrough is not lowered yet; each case must break, return, or throw",
                    ));
                }

                body.push_stmt_to_block(
                    block,
                    Stmt::Match {
                        scrutinee,
                        arms,
                        default,
                    },
                );
                Ok(())
            }
            Statement::BreakStatement(break_stmt) => {
                if break_stmt.label.is_some() {
                    return Err(SmeltError::unsupported(
                        self.span(break_stmt.span.start, break_stmt.span.end),
                        "labeled break is not lowered yet",
                    ));
                }
                body.push_stmt_to_block(block, Stmt::Break);
                Ok(())
            }
            Statement::ContinueStatement(continue_stmt) => {
                if continue_stmt.label.is_some() {
                    return Err(SmeltError::unsupported(
                        self.span(continue_stmt.span.start, continue_stmt.span.end),
                        "labeled continue is not lowered yet",
                    ));
                }
                body.push_stmt_to_block(block, Stmt::Continue);
                Ok(())
            }
            Statement::ThrowStatement(throw_stmt) => {
                let expr = self.throw_operand_expression(&throw_stmt.argument, body)?;
                body.push_stmt_to_block(block, Stmt::Throw(expr));
                Ok(())
            }
            Statement::TryStatement(try_stmt) => {
                let try_body = self.block_from_block_statement(&try_stmt.block, body)?;
                let (catch_binding, catch_body) = if let Some(handler) = &try_stmt.handler {
                    let previous_locals = self.scope.snapshot_bindings();
                    let catch_binding = handler
                        .param
                        .as_ref()
                        .map(|param| self.catch_binding(param, body))
                        .transpose()?;
                    let catch_body = self.block_from_block_statement(&handler.body, body)?;
                    self.scope.restore_bindings(previous_locals);
                    (catch_binding, Some(catch_body))
                } else {
                    (None, None)
                };
                let finally_body = try_stmt
                    .finalizer
                    .as_ref()
                    .map(|finalizer| self.block_from_block_statement(finalizer, body))
                    .transpose()?;

                body.push_stmt_to_block(
                    block,
                    Stmt::TryCatch {
                        body: try_body,
                        catch_binding,
                        catch_body,
                        finally_body,
                    },
                );
                Ok(())
            }
            Statement::BlockStatement(block_stmt) => {
                for child in &block_stmt.body {
                    self.statement_in_block(child, body, block)?;
                }
                Ok(())
            }
            _ => Err(SmeltError::unsupported(
                self.statement_span(statement),
                format!("statement kind is not lowered yet: {statement:?}"),
            )),
        };
        self.current_statement_block = previous_statement_block;
        result
    }

    /// Lower one element of a statement-position comma expression.
    ///
    /// Every element is a complete JavaScript expression statement for side-effect
    /// purposes. Route it through the same interceptors as an ordinary expression
    /// statement so assignments, updates, test assertions, async spawning, and
    /// narrowing retain their normal semantics and source order.
    fn sequence_expression_statement(
        &mut self,
        expression: &Expression<'_>,
        expression_span: oxc::span::Span,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let expression = Self::unparenthesized_expression(expression);
        if let Expression::SequenceExpression(sequence) = expression {
            for child in &sequence.expressions {
                self.sequence_expression_statement(child, child.span(), body, block)?;
            }
            return Ok(());
        }
        if self.is_synthesized_prototype_assignment(expression)
            || self.inline_runtime_lifecycle_setup(expression, body, block)?
            || self.is_test_framework_statement(expression)
            || Self::is_vitest_mock_statement(expression)
            || Self::is_top_level_dynamic_import_await(expression)
        {
            return Ok(());
        }
        if let Expression::CallExpression(call) = expression {
            if self.for_each_statement(call, body, block)?
                || self.expect_matcher_statement(call, body)?
                || self.node_assert_statement(call, body)?
            {
                return Ok(());
            }
        }
        if let Expression::AssignmentExpression(assign) = expression {
            if let Some(set) = self.try_global_assignment_expression(assign, body)? {
                body.push_stmt_to_block(block, Stmt::Expr(set));
                return Ok(());
            }
            if block == body.root && self.module_global_assignment_statement(assign, body, block)? {
                return Ok(());
            }
            if self.try_lower_negative_bracket_write_statement(assign, body, block)?
                || self.try_lower_list_length_assignment_statement(assign, body, block)?
                || self.array_destructuring_assignment_statement(assign, body, block)?
                || self.try_collect_callable_local_prop(assign, body, block)?
            {
                return Ok(());
            }
            let (target, value) = self.assignment_parts(assign, body)?;
            body.push_stmt_to_block(block, Stmt::Assign { target, value });
            return Ok(());
        }
        if let Expression::UpdateExpression(update) = expression {
            if let Some(set) = self.try_global_update_expression(update, body, false)? {
                body.push_stmt_to_block(block, Stmt::Expr(set));
                return Ok(());
            }
            let (target, value) = self.update_parts(update, body)?;
            body.push_stmt_to_block(block, Stmt::Assign { target, value });
            return Ok(());
        }
        if let Expression::YieldExpression(yield_expr) = expression
            && self.generator_yield_statement(yield_expr, body, block)?
        {
            return Ok(());
        }
        let assertion_narrowing = self.assertion_call_narrowing(expression);
        let value = self.expression(expression, body)?;
        let value = if matches!(
            self.ctx.krate.types.get(Self::expr_ty(body, value)),
            Some(Type::Future(_))
        ) {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            body.push_expr(Expr {
                kind: ExprKind::AsyncOp {
                    op: AsyncOp::SpawnLocal,
                    args: vec![value],
                },
                ty: none_ty,
                span: self.span(expression_span.start, expression_span.end),
            })
        } else {
            value
        };
        body.push_stmt_to_block(block, Stmt::Expr(value));
        if let Some((name, target)) = assertion_narrowing {
            self.apply_narrowing(name, target);
        }
        Ok(())
    }

    /// Lower side-effecting `array.forEach((item) => { ... })` as a normal loop.
    pub(in crate::lowering) fn for_each_statement(
        &mut self,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return Ok(false);
        };
        if member.property.name != "forEach" {
            return Ok(false);
        }
        let [Argument::ArrowFunctionExpression(arrow)] = call.arguments.as_slice() else {
            return Ok(false);
        };
        let Some(item_param) = arrow.params.items.first() else {
            // A `forEach` whose callback has no fixed item parameter — a bare
            // `() => ...` side effect or a rest-only `(...args) => ...` collector
            // — is not modeled by this statement-loop shortcut. Decline so the
            // general callback lowering (which supports rest parameters through
            // the closure-body path) handles it instead of failing here.
            return Ok(false);
        };
        let mut iter = self.expression(&member.object, body)?;
        let iter_ty = Self::expr_ty(body, iter);
        // `map.forEach((value, key) => …)` receives the *key* as its second
        // argument, not a numeric index: iterate the `[key, value]` entries
        // list and bind both callback parameters from each entry tuple.
        if let Some(Type::Dict(key_ty, value_ty) | Type::JsMap(key_ty, value_ty)) =
            self.ctx.krate.types.get(iter_ty).cloned()
            && arrow.params.items.len() >= 2
        {
            let span = self.span(call.span.start, call.span.end);
            let entry_ty = self
                .ctx
                .krate
                .types
                .intern(Type::Tuple(vec![key_ty, value_ty]));
            let entries_ty = self.ctx.krate.types.intern(Type::List(entry_ty));
            let entries = body.push_expr(Expr {
                kind: ExprKind::DictProjection {
                    op: DictProjectionOp::Entries,
                    dict: iter,
                },
                ty: entries_ty,
                span,
            });
            let entry_symbol = self.ctx.krate.symbols.intern("__for_each_entry");
            let entry_local = body.push_local(LocalDecl {
                name: Some(entry_symbol),
                ty: entry_ty,
                mutable: false,
                span,
            });
            let entry_pat = body.push_pattern(Pattern::Binding(entry_local));
            let loop_body = body.push_block(self.span(arrow.body.span.start, arrow.body.span.end));
            let mut param_names = Vec::new();
            for param in arrow.params.items.iter().take(2) {
                Self::binding_pattern_names(&param.pattern, &mut param_names);
            }
            let saved_locals = param_names
                .iter()
                .map(|name| (name.clone(), self.scope.lookup(name)))
                .collect::<Vec<_>>();
            for (param_index, param) in arrow.params.items.iter().take(2).enumerate() {
                // Callback order is `(value, key)`; entries are `[key, value]`.
                let (tuple_index, ty) = if param_index == 0 {
                    (1_usize, value_ty)
                } else {
                    (0_usize, key_ty)
                };
                let entry_read = body.push_expr(Expr {
                    kind: ExprKind::Local(entry_local),
                    ty: entry_ty,
                    span,
                });
                let extracted = body.push_expr(Expr {
                    kind: ExprKind::TupleIndex {
                        tuple: entry_read,
                        index: tuple_index,
                    },
                    ty,
                    span,
                });
                self.binding_declaration(
                    &param.pattern,
                    Some(extracted),
                    Some(ty),
                    false,
                    body,
                    loop_body,
                )?;
            }
            for statement in &arrow.body.statements {
                self.for_each_callback_statement(statement, body, loop_body)?;
            }
            for (name, prior) in saved_locals {
                match prior {
                    Some(local) => {
                        self.scope.bind(name, local);
                    }
                    None => {
                        self.scope.unbind(name.as_str());
                    }
                }
            }
            body.push_stmt_to_block(
                block,
                Stmt::For {
                    pat: entry_pat,
                    iter: entries,
                    body: loop_body,
                },
            );
            return Ok(true);
        }
        let item_ty = match self.ctx.krate.types.get(iter_ty).cloned() {
            Some(Type::List(item_ty)) => item_ty,
            // `set.forEach(value => …)` iterates the set's values in
            // insertion order; project them into a list and reuse the array
            // loop below.
            Some(Type::Set(item_ty)) => {
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::SetProjection {
                        op: SetProjectionOp::Values,
                        set: iter,
                    },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                item_ty
            }
            Some(Type::Dict(_, value_ty)) => {
                let list_ty = self.ctx.krate.types.intern(Type::List(value_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::DictProjection {
                        op: DictProjectionOp::Values,
                        dict: iter,
                    },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                value_ty
            }
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Class { .. }) => {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: iter },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                item_ty
            }
            Some(Type::Union(items))
                if items.iter().any(|item| {
                    matches!(
                        self.ctx.krate.types.get(*item),
                        Some(
                            Type::List(_)
                                | Type::Unknown
                                | Type::TypeParam { .. }
                                | Type::Class { .. }
                        )
                    )
                }) =>
            {
                let item_ty = self.ctx.krate.types.intern(Type::Unknown);
                let list_ty = self.ctx.krate.types.intern(Type::List(item_ty));
                iter = body.push_expr(Expr {
                    kind: ExprKind::TypeAssert { value: iter },
                    ty: list_ty,
                    span: self.span(member.object.span().start, member.object.span().end),
                });
                item_ty
            }
            _ => {
                return Err(SmeltError::unsupported(
                    self.span(member.object.span().start, member.object.span().end),
                    "array forEach statement receiver must be an array",
                ));
            }
        };
        let item_binding = match &item_param.pattern {
            BindingPattern::BindingIdentifier(binding) => Some(binding),
            _ => None,
        };
        let item_symbol = if let Some(binding) = item_binding {
            self.intern_source_name(binding.name.as_str())
        } else {
            self.ctx.krate.symbols.intern("__for_each_item")
        };
        let item_local = body.push_local(LocalDecl {
            name: Some(item_symbol),
            ty: item_ty,
            mutable: false,
            span: self.span(item_param.span.start, item_param.span.end),
        });
        let saved_item_local =
            item_binding.map(|binding| self.scope.bind(binding.name.to_string(), item_local));
        let item_pat = body.push_pattern(Pattern::Binding(item_local));
        let index_binding = arrow
            .params
            .items
            .get(1)
            .map(|index_param| {
                let BindingPattern::BindingIdentifier(index_binding) = &index_param.pattern else {
                    return Err(SmeltError::unsupported(
                        self.span(index_param.span.start, index_param.span.end),
                        "array forEach statement index parameters must be identifiers",
                    ));
                };
                Ok(index_binding)
            })
            .transpose()?;
        let index_ty = self.ctx.krate.types.intern(Type::Float);
        let index_counter = index_binding.is_some().then(|| {
            let counter_symbol = self.ctx.krate.symbols.intern("__for_each_index");
            let counter_local = body.push_local(LocalDecl {
                name: Some(counter_symbol),
                ty: index_ty,
                mutable: true,
                span: self.span(call.span.start, call.span.end),
            });
            let zero = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Float(0.0)),
                ty: index_ty,
                span: self.span(call.span.start, call.span.end),
            });
            let counter_pat = body.push_pattern(Pattern::Binding(counter_local));
            body.push_stmt_to_block(
                block,
                Stmt::Let {
                    pat: counter_pat,
                    ty: index_ty,
                    value: Some(zero),
                },
            );
            counter_local
        });
        let loop_body = body.push_block(self.span(arrow.body.span.start, arrow.body.span.end));
        if item_binding.is_none() {
            let item_value = body.push_expr(Expr {
                kind: ExprKind::Local(item_local),
                ty: item_ty,
                span: self.span(item_param.span.start, item_param.span.end),
            });
            self.binding_declaration(
                &item_param.pattern,
                Some(item_value),
                Some(item_ty),
                false,
                body,
                loop_body,
            )?;
        }
        let saved_index_local =
            if let (Some(index_binding), Some(counter)) = (index_binding, index_counter) {
                let index_symbol = self.intern_source_name(index_binding.name.as_str());
                let index_local = body.push_local(LocalDecl {
                    name: Some(index_symbol),
                    ty: index_ty,
                    mutable: false,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let counter_value = body.push_expr(Expr {
                    kind: ExprKind::Local(counter),
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let index_pat = body.push_pattern(Pattern::Binding(index_local));
                body.push_stmt_to_block(
                    loop_body,
                    Stmt::Let {
                        pat: index_pat,
                        ty: index_ty,
                        value: Some(counter_value),
                    },
                );
                let one = body.push_expr(Expr {
                    kind: ExprKind::Literal(Literal::Float(1.0)),
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let next = body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Add,
                        lhs: counter_value,
                        rhs: one,
                    },
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                let target = body.push_expr(Expr {
                    kind: ExprKind::Local(counter),
                    ty: index_ty,
                    span: self.span(index_binding.span.start, index_binding.span.end),
                });
                body.push_stmt_to_block(
                    loop_body,
                    Stmt::Assign {
                        target,
                        value: next,
                    },
                );
                self.scope.bind(index_binding.name.to_string(), index_local)
            } else {
                None
            };
        for statement in &arrow.body.statements {
            self.for_each_callback_statement(statement, body, loop_body)?;
        }
        if let Some(index_binding) = index_binding {
            if let Some(prior) = saved_index_local {
                self.scope.bind(index_binding.name.to_string(), prior);
            } else {
                self.scope.unbind(index_binding.name.as_str());
            }
        }
        if let Some(item_binding) = item_binding {
            if let Some(Some(prior)) = saved_item_local {
                self.scope.bind(item_binding.name.to_string(), prior);
            } else {
                self.scope.unbind(item_binding.name.as_str());
            }
        }
        body.push_stmt_to_block(
            block,
            Stmt::For {
                pat: item_pat,
                iter,
                body: loop_body,
            },
        );
        Ok(true)
    }

    /// Lower a statement inside a `forEach` callback body.
    pub(in crate::lowering) fn for_each_callback_statement(
        &mut self,
        statement: &Statement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let previous_statement_block = self.current_statement_block.replace(block);
        let result = match statement {
            Statement::ExpressionStatement(expr_stmt) => {
                if let Expression::AssignmentExpression(assign) = &expr_stmt.expression {
                    if let Some(set) = self.try_global_assignment_expression(assign, body)? {
                        body.push_stmt_to_block(block, Stmt::Expr(set));
                        return Ok(());
                    }
                    if self.try_lower_negative_bracket_write_statement(assign, body, block)? {
                        return Ok(());
                    }
                    if self.try_lower_list_length_assignment_statement(assign, body, block)? {
                        return Ok(());
                    }
                    if self.array_destructuring_assignment_statement(assign, body, block)? {
                        return Ok(());
                    }
                    let (target, value) = self.assignment_parts(assign, body)?;
                    if let Some(local_decl) = usize::try_from(target.0)
                        .ok()
                        .and_then(|index| body.exprs.get(index))
                        .and_then(|expr| match expr.kind {
                            ExprKind::Local(local) => usize::try_from(local.0)
                                .ok()
                                .and_then(|index| body.locals.get(index)),
                            _ => None,
                        })
                        && !local_decl.mutable
                    {
                        return Err(SmeltError::unsupported(
                            self.span(assign.span.start, assign.span.end),
                            "callback assignment to captured const local is not supported",
                        ));
                    }
                    body.push_stmt_to_block(block, Stmt::Assign { target, value });
                    Ok(())
                } else {
                    self.statement_in_block(statement, body, block)
                }
            }
            Statement::ReturnStatement(_) => {
                body.push_stmt_to_block(block, Stmt::Continue);
                Ok(())
            }
            Statement::BlockStatement(block_stmt) => {
                for child in &block_stmt.body {
                    self.for_each_callback_statement(child, body, block)?;
                }
                Ok(())
            }
            Statement::IfStatement(if_stmt) => {
                let cond = self.expression(&if_stmt.test, body)?;
                let then_block = body.push_block(self.statement_span(&if_stmt.consequent));
                self.for_each_callback_statement(&if_stmt.consequent, body, then_block)?;
                let else_block = if_stmt
                    .alternate
                    .as_ref()
                    .map(|alternate| {
                        let else_block = body.push_block(self.statement_span(alternate));
                        self.for_each_callback_statement(alternate, body, else_block)?;
                        Ok(else_block)
                    })
                    .transpose()?;
                body.push_stmt_to_block(
                    block,
                    Stmt::If {
                        cond,
                        then_block,
                        else_block,
                    },
                );
                Ok(())
            }
            _ => self.statement_in_block(statement, body, block),
        };
        self.current_statement_block = previous_statement_block;
        result
    }

    /// Lower a generator suspension point while preserving its concrete yield type.
    pub(in crate::lowering) fn generator_yield_statement(
        &mut self,
        yield_expr: &oxc::ast::ast::YieldExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Some(generator) = self.current_generator_yields else {
            return Ok(false);
        };
        if yield_expr.delegate {
            let delegate = self.generator_delegate_expression(yield_expr, body)?;
            body.push_stmt_to_block(block, Stmt::Expr(delegate));
            return Ok(true);
        }
        let value = if let Some(argument) = &yield_expr.argument {
            self.expression(argument, body)?
        } else {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: none_ty,
                span: self.span(yield_expr.span.start, yield_expr.span.end),
            })
        };
        let yielded = if matches!(
            self.ctx.krate.types.get(generator.yield_ty),
            Some(Type::Unknown)
        ) {
            // Preserve the concrete expression while an unannotated generator
            // signature is being inferred after body lowering.
            value
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: generator.yield_ty,
                span: self.span(yield_expr.span.start, yield_expr.span.end),
            })
        };
        let suspend = body.push_expr(Expr {
            kind: ExprKind::GeneratorYield { value: yielded },
            // The MIR temporary retains `next_ty` even when the source statement
            // discards it, allowing every suspension in one producer to share
            // genawaiter's single typed resume channel.
            ty: generator.next_ty,
            span: self.span(yield_expr.span.start, yield_expr.span.end),
        });
        body.push_stmt_to_block(block, Stmt::Expr(suspend));
        Ok(true)
    }

    /// Lower a yield used as an expression, retaining the caller-provided resume type.
    pub(in crate::lowering) fn generator_yield_expression(
        &mut self,
        yield_expr: &oxc::ast::ast::YieldExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let generator = self.current_generator_yields.ok_or_else(|| {
            SmeltError::unsupported(
                self.span(yield_expr.span.start, yield_expr.span.end),
                "yield is only valid inside a generator",
            )
        })?;
        let value = if let Some(argument) = &yield_expr.argument {
            self.expression(argument, body)?
        } else {
            let none_ty = self.ctx.krate.types.intern(Type::None);
            body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::None),
                ty: none_ty,
                span: self.span(yield_expr.span.start, yield_expr.span.end),
            })
        };
        let yielded = if matches!(
            self.ctx.krate.types.get(generator.yield_ty),
            Some(Type::Unknown)
        ) {
            value
        } else {
            body.push_expr(Expr {
                kind: ExprKind::TypeAssert { value },
                ty: generator.yield_ty,
                span: self.span(yield_expr.span.start, yield_expr.span.end),
            })
        };
        Ok(body.push_expr(Expr {
            kind: ExprKind::GeneratorYield { value: yielded },
            ty: generator.next_ty,
            span: self.span(yield_expr.span.start, yield_expr.span.end),
        }))
    }

    /// Lower expression-position `yield*` as a typed resume/forward/complete operation.
    pub(in crate::lowering) fn generator_delegate_expression(
        &mut self,
        yield_expr: &oxc::ast::ast::YieldExpression<'_>,
        body: &mut Body,
    ) -> Result<smelt_hir::ExprId, SmeltError> {
        let Some(argument) = &yield_expr.argument else {
            return Err(SmeltError::unsupported(
                self.span(yield_expr.span.start, yield_expr.span.end),
                "yield* requires a delegated generator value",
            ));
        };
        let value = self.expression(argument, body)?;
        let value_ty = Self::expr_ty(body, value);
        let span = self.span(yield_expr.span.start, yield_expr.span.end);
        let outer_is_async = self
            .current_generator_yields
            .is_some_and(|generator| generator.is_async);
        let outer_yield_ty = self
            .current_generator_yields
            .map(|generator| generator.yield_ty)
            .ok_or_else(|| {
                SmeltError::unsupported(span, "yield* is only valid inside a generator")
            })?;
        let (generator, return_ty) =
            if let Some(Type::Generator {
                is_async,
                yield_ty,
                return_ty,
                ..
            }) =
                self.ctx.krate.types.get(value_ty).cloned()
            {
                if is_async && !outer_is_async {
                    return Err(SmeltError::unsupported(
                        span,
                        "a synchronous generator cannot delegate to an AsyncGenerator",
                    ));
                }
                if !self.type_assignable_to(yield_ty, outer_yield_ty) {
                    return Err(SmeltError::unsupported(
                        span,
                        "yield* item type is not assignable to the outer yield type",
                    ));
                }
                (value, return_ty)
            } else if let Some(members) = match self.ctx.krate.types.get(value_ty).cloned() {
                Some(Type::Union(members)) => Some(members),
                _ => None,
            } {
                let mut return_types = Vec::with_capacity(members.len());
                for member in members {
                    let (is_async, item_ty, return_ty) = match self.ctx.krate.types.get(member) {
                        Some(Type::Generator {
                            is_async,
                            yield_ty,
                            return_ty,
                            ..
                        }) => (*is_async, *yield_ty, *return_ty),
                        Some(Type::List(item_ty) | Type::Set(item_ty)) => (
                            false,
                            *item_ty,
                            self.ctx.krate.types.intern(Type::None),
                        ),
                        Some(Type::String) => {
                            let string_ty = self.ctx.krate.types.intern(Type::String);
                            (false, string_ty, self.ctx.krate.types.intern(Type::None))
                        }
                        _ => {
                            return Err(SmeltError::unsupported(
                                span,
                                "yield* union member is not a typed iterable carrier",
                            ));
                        }
                    };
                    if is_async && !outer_is_async {
                        return Err(SmeltError::unsupported(
                            span,
                            "a synchronous generator cannot delegate to an async union member",
                        ));
                    }
                    if !self.type_assignable_to(item_ty, outer_yield_ty) {
                        return Err(SmeltError::unsupported(
                            span,
                            "yield* union item type is not assignable to the outer yield type",
                        ));
                    }
                    if !return_types.contains(&return_ty) {
                        return_types.push(return_ty);
                    }
                }
                (value, self.reconciled_inferred_types(return_types))
            } else if let Some(item_ty) = match self.ctx.krate.types.get(value_ty) {
                Some(Type::List(item_ty) | Type::Set(item_ty)) => Some(*item_ty),
                Some(Type::String) => Some(self.ctx.krate.types.intern(Type::String)),
                Some(Type::Tuple(items)) if items
                    .iter()
                    .all(|item| self.type_assignable_to(*item, outer_yield_ty)) =>
                {
                    Some(outer_yield_ty)
                }
                _ => None,
            } {
                if !self.type_assignable_to(item_ty, outer_yield_ty) {
                    return Err(SmeltError::unsupported(
                        span,
                        "yield* iterable item type is not assignable to the outer yield type",
                    ));
                }
                // Built-in iterable completion is JavaScript `undefined`, which
                // is represented by the frontend's void/none result type. The
                // operand remains a collection rather than masquerading as a
                // generator carrier; MIR/codegen adapt its concrete protocol.
                (value, self.ctx.krate.types.intern(Type::None))
            } else {
                // TypeScript's iterable protocol delegates through the
                // well-known `Symbol.iterator` member. Computed-key lowering
                // gives that member a stable synthetic name, so resolve it
                // through the same general class/interface member lookup used
                // by ordinary method calls rather than recognizing a source
                // library or concrete class.
                let mut candidates = Vec::new();
                if outer_is_async {
                    candidates.push(self.intern_source_name("__smelt_symbol_async_iterator"));
                }
                candidates.push(self.intern_source_name("__smelt_symbol_iterator"));
                let mut resolved = None;
                for iterator in candidates {
                    let Ok(iterator_ty) = self.class_field_type(value_ty, iterator) else {
                        continue;
                    };
                    let Some(Type::Function(iterator_fn)) =
                        self.ctx.krate.types.get(iterator_ty).cloned()
                    else {
                        continue;
                    };
                    let generator_ty = iterator_fn.return_ty;
                    let Some(Type::Generator {
                        is_async,
                        return_ty,
                        ..
                    }) = self.ctx.krate.types.get(generator_ty).cloned()
                    else {
                        continue;
                    };
                    if !is_async || outer_is_async {
                        resolved = Some((iterator, generator_ty, return_ty));
                        break;
                    }
                }
                let Some((iterator, generator_ty, return_ty)) = resolved else {
                    return Err(SmeltError::unsupported(
                        span,
                        format!(
                            "yield* requires a typed sync or async iterator method (received {:?})",
                            self.ctx.krate.types.get(value_ty),
                        ),
                    ));
                };
                let generator = body.push_expr(Expr {
                    kind: ExprKind::Method {
                        receiver: value,
                        method: iterator,
                        args: Vec::new(),
                    },
                    ty: generator_ty,
                    span,
                });
                (generator, return_ty)
            };
        Ok(body.push_expr(Expr {
            kind: ExprKind::GeneratorDelegate { generator },
            ty: return_ty,
            span,
        }))
    }

    /// Lower writes to known module-level variables without requiring a local target.
    pub(in crate::lowering) fn module_global_assignment_statement(
        &mut self,
        assign: &oxc::ast::ast::AssignmentExpression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let AssignmentTarget::AssignmentTargetIdentifier(target) = &assign.left else {
            return Ok(false);
        };
        if assign.operator != AssignmentOperator::Assign {
            return Ok(false);
        }
        if !self.module_globals.contains_key(target.name.as_str()) {
            return Ok(false);
        }
        let value = self.expression(&assign.right, body)?;
        body.push_stmt_to_block(block, Stmt::Expr(value));
        Ok(true)
    }

    /// Lower `while ((target = value) !== null)` without dropping the assignment.
    pub(in crate::lowering) fn while_assignment_condition_body(
        &mut self,
        while_stmt: &oxc::ast::ast::WhileStatement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<
        Option<(
            smelt_hir::ExprId,
            smelt_hir::BlockId,
            smelt_hir::ExprId,
            smelt_hir::ExprId,
        )>,
        SmeltError,
    > {
        let test = Self::unparenthesized_expression(&while_stmt.test);
        let Expression::BinaryExpression(binary) = test else {
            return Ok(None);
        };
        if !matches!(
            binary.operator,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality
        ) || !matches!(&binary.right, Expression::NullLiteral(_))
        {
            return Ok(None);
        }
        let left = Self::unparenthesized_expression(&binary.left);
        let Expression::AssignmentExpression(assign) = left else {
            return Ok(None);
        };
        let (target, value) = self.assignment_parts(assign, body)?;
        body.push_stmt_to_block(block, Stmt::Assign { target, value });
        let null_expr = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::None),
            ty: self.ctx.krate.types.intern(Type::None),
            span: self.span(binary.right.span().start, binary.right.span().end),
        });
        let cond = body.push_expr(Expr {
            kind: ExprKind::BinOp {
                op: BinOp::NotEq,
                lhs: target,
                rhs: null_expr,
            },
            ty: self.ctx.krate.types.intern(Type::Bool),
            span: self.span(binary.span.start, binary.span.end),
        });
        let loop_body = body.push_block(self.statement_span(&while_stmt.body));
        if let Statement::BlockStatement(block_stmt) = &while_stmt.body {
            for statement in &block_stmt.body {
                self.statement_in_block(statement, body, loop_body)?;
            }
        } else {
            self.statement_in_block(&while_stmt.body, body, loop_body)?;
        }
        let (update_target, update_value) = self.assignment_parts(assign, body)?;
        Ok(Some((cond, loop_body, update_target, update_value)))
    }

    /// Strip transparent parentheses from a TypeScript expression.
    pub(in crate::lowering) fn unparenthesized_expression<'a>(
        expression: &'a Expression<'a>,
    ) -> &'a Expression<'a> {
        let mut current = expression;
        while let Expression::ParenthesizedExpression(parenthesized) = current {
            current = &parenthesized.expression;
        }
        current
    }

    /// Return whether an expression is a top-level Vitest organization call.
    pub(in crate::lowering) fn is_test_framework_statement(
        &self,
        expression: &Expression<'_>,
    ) -> bool {
        if self.table_test_call(expression).is_some() {
            return true;
        }
        if self.property_test_call(expression).is_some() {
            return true;
        }
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        self.is_test_framework_callee(&call.callee)
    }

    /// Return whether a switch contains a case body that can reach the next
    /// case (genuine fallthrough), which the strict `Match` lowering rejects.
    pub(in crate::lowering) fn switch_has_fallthrough(
        switch_stmt: &oxc::ast::ast::SwitchStatement<'_>,
    ) -> bool {
        let case_count = switch_stmt.cases.len();
        for (case_index, case) in switch_stmt.cases.iter().enumerate() {
            if case.consequent.is_empty() || case_index + 1 == case_count {
                continue;
            }
            let has_top_level_break = case.consequent.iter().any(|statement| match statement {
                Statement::BreakStatement(_) => true,
                Statement::BlockStatement(block_stmt) => block_stmt
                    .body
                    .iter()
                    .any(|nested| matches!(nested, Statement::BreakStatement(_))),
                _ => false,
            });
            if !has_top_level_break && !case.consequent.iter().any(statement_terminates) {
                return true;
            }
        }
        false
    }

    /// Lower a switch statement with genuine fallthrough.
    ///
    /// The switch becomes a single-iteration `while` loop: each case lowers to
    /// `if matched || scrutinee === label { matched = true; <body> }`, a JS
    /// `break` inside a case body maps to the loop break, and a trailing
    /// `default` body runs whenever control reaches the end of the chain —
    /// exactly when JavaScript reaches it (no label matched, or an earlier
    /// case fell through without breaking). A loop-tail `break` makes the
    /// loop run once.
    fn switch_fallthrough_statement(
        &mut self,
        switch_stmt: &oxc::ast::ast::SwitchStatement<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<(), SmeltError> {
        let span = self.span(switch_stmt.span.start, switch_stmt.span.end);
        let case_count = switch_stmt.cases.len();
        for (case_index, case) in switch_stmt.cases.iter().enumerate() {
            if case.test.is_none() && case_index + 1 != case_count {
                return Err(SmeltError::unsupported(
                    span,
                    "switch fallthrough lowering requires the default case to be last",
                ));
            }
            // A top-level `continue` would bind to the synthetic loop instead
            // of the enclosing one; keep the strict path's rejection.
            let has_top_level_continue = case.consequent.iter().any(|statement| match statement {
                Statement::ContinueStatement(_) => true,
                Statement::BlockStatement(block_stmt) => block_stmt
                    .body
                    .iter()
                    .any(|nested| matches!(nested, Statement::ContinueStatement(_))),
                _ => false,
            });
            if has_top_level_continue {
                return Err(SmeltError::unsupported(
                    span,
                    "switch continue lowering is not implemented yet",
                ));
            }
        }
        let scrutinee = self.expression(&switch_stmt.discriminant, body)?;
        let scrutinee_ty = Self::expr_ty(body, scrutinee);
        let scrutinee_name = self.intern_source_name("__switch_value");
        let scrutinee_local = body.push_local(LocalDecl {
            name: Some(scrutinee_name),
            ty: scrutinee_ty,
            mutable: false,
            span,
        });
        let scrutinee_pat = body.push_pattern(Pattern::Binding(scrutinee_local));
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat: scrutinee_pat,
                ty: scrutinee_ty,
                value: Some(scrutinee),
            },
        );
        let bool_ty = self.ctx.krate.types.intern(Type::Bool);
        let matched_name = self.intern_source_name("__switch_matched");
        let matched_local = body.push_local(LocalDecl {
            name: Some(matched_name),
            ty: bool_ty,
            mutable: true,
            span,
        });
        let matched_pat = body.push_pattern(Pattern::Binding(matched_local));
        let false_expr = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(false)),
            ty: bool_ty,
            span,
        });
        body.push_stmt_to_block(
            block,
            Stmt::Let {
                pat: matched_pat,
                ty: bool_ty,
                value: Some(false_expr),
            },
        );
        let loop_block = body.push_block(span);
        let mut pending_label_conds: Vec<smelt_hir::ExprId> = Vec::new();
        for case in &switch_stmt.cases {
            let Some(test) = &case.test else {
                // Trailing default: runs whenever the chain reaches it.
                for case_statement in &case.consequent {
                    self.statement_in_block(case_statement, body, loop_block)?;
                }
                continue;
            };
            let label = self.literal_case_label(test)?;
            let label_ty = self.ctx.krate.types.intern(match &label {
                Literal::String(_) => Type::String,
                Literal::Bool(_) => Type::Bool,
                Literal::None => Type::None,
                _ => Type::Float,
            });
            let label_expr = body.push_expr(Expr {
                kind: ExprKind::Literal(label),
                ty: label_ty,
                span,
            });
            let scrutinee_read = body.push_expr(Expr {
                kind: ExprKind::Local(scrutinee_local),
                ty: scrutinee_ty,
                span,
            });
            let cmp = body.push_expr(Expr {
                kind: ExprKind::BinOp {
                    op: BinOp::JsStrictEq,
                    lhs: scrutinee_read,
                    rhs: label_expr,
                },
                ty: bool_ty,
                span,
            });
            if case.consequent.is_empty() {
                pending_label_conds.push(cmp);
                continue;
            }
            let mut cond = body.push_expr(Expr {
                kind: ExprKind::Local(matched_local),
                ty: bool_ty,
                span,
            });
            for pending in std::mem::take(&mut pending_label_conds)
                .into_iter()
                .chain(std::iter::once(cmp))
            {
                cond = body.push_expr(Expr {
                    kind: ExprKind::BinOp {
                        op: BinOp::Or,
                        lhs: cond,
                        rhs: pending,
                    },
                    ty: bool_ty,
                    span,
                });
            }
            let case_block = body.push_block(span);
            let matched_target = body.push_expr(Expr {
                kind: ExprKind::Local(matched_local),
                ty: bool_ty,
                span,
            });
            let true_expr = body.push_expr(Expr {
                kind: ExprKind::Literal(Literal::Bool(true)),
                ty: bool_ty,
                span,
            });
            body.push_stmt_to_block(
                case_block,
                Stmt::Assign {
                    target: matched_target,
                    value: true_expr,
                },
            );
            for case_statement in &case.consequent {
                self.statement_in_block(case_statement, body, case_block)?;
            }
            body.push_stmt_to_block(
                loop_block,
                Stmt::If {
                    cond,
                    then_block: case_block,
                    else_block: None,
                },
            );
        }
        body.push_stmt_to_block(loop_block, Stmt::Break);
        let loop_cond = body.push_expr(Expr {
            kind: ExprKind::Literal(Literal::Bool(true)),
            ty: bool_ty,
            span,
        });
        body.push_stmt_to_block(
            block,
            Stmt::While {
                cond: loop_cond,
                body: loop_block,
            },
        );
        Ok(())
    }

    /// Return whether this is a top-level `vi.mock(...)` registration.
    pub(in crate::lowering) fn is_vitest_mock_statement(expression: &Expression<'_>) -> bool {
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return false;
        };
        member.property.name == "mock"
            && matches!(&member.object, Expression::Identifier(object) if object.name == "vi")
    }

    /// Return whether this is a top-level `await import("...")` side-effect load.
    pub(in crate::lowering) fn is_top_level_dynamic_import_await(
        expression: &Expression<'_>,
    ) -> bool {
        let Expression::AwaitExpression(await_expr) = expression else {
            return false;
        };
        matches!(&await_expr.argument, Expression::ImportExpression(_))
    }

    /// Return a supported top-level test case call, if this expression is one.
    pub(in crate::lowering) fn test_case_call<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return None;
        };
        let name = callee.name.as_str();
        (self.imports.is_test_builtin(name) && matches!(name, "it" | "test")).then_some(call)
    }

    /// Return whether an expression is a skipped Vitest test case.
    pub(in crate::lowering) fn skipped_test_case_call(&self, expression: &Expression<'_>) -> bool {
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return false;
        };
        if member.property.name != "skip" {
            return false;
        }
        matches!(
            &member.object,
            Expression::Identifier(object)
                if self.imports.is_test_builtin(object.name.as_str())
                    && matches!(object.name.as_str(), "it" | "test")
        )
    }

    /// Return whether a suite-level condition is known false for native Rust tests.
    pub(in crate::lowering) fn describe_condition_is_native_false(
        expression: &Expression<'_>,
    ) -> bool {
        Self::typeof_window_undefined_comparison(expression, false)
    }

    /// Return whether a suite-level condition is known true for native Rust tests.
    pub(in crate::lowering) fn describe_condition_is_native_true(
        expression: &Expression<'_>,
    ) -> bool {
        Self::typeof_window_undefined_comparison(expression, true)
    }

    /// Evaluate `typeof window ===/!== "undefined"` for the Rust test target.
    pub(in crate::lowering) fn typeof_window_undefined_comparison(
        expression: &Expression<'_>,
        want_equal: bool,
    ) -> bool {
        let Expression::BinaryExpression(binary) = expression else {
            return false;
        };
        let is_equal = match binary.operator {
            BinaryOperator::StrictEquality | BinaryOperator::Equality => true,
            BinaryOperator::StrictInequality | BinaryOperator::Inequality => false,
            _ => return false,
        };
        if is_equal != want_equal {
            return false;
        }
        let left_typeof_window = Self::is_typeof_window(&binary.left);
        let right_typeof_window = Self::is_typeof_window(&binary.right);
        matches!(
            (left_typeof_window, &binary.right, right_typeof_window, &binary.left),
            (true, Expression::StringLiteral(value), _, _) if value.value == "undefined"
        ) || matches!(
            (left_typeof_window, &binary.right, right_typeof_window, &binary.left),
            (_, _, true, Expression::StringLiteral(value)) if value.value == "undefined"
        )
    }

    /// Return whether an expression is `typeof window`.
    pub(in crate::lowering) fn is_typeof_window(expression: &Expression<'_>) -> bool {
        let Expression::UnaryExpression(unary) = expression else {
            return false;
        };
        if unary.operator != UnaryOperator::Typeof {
            return false;
        }
        matches!(&unary.argument, Expression::Identifier(identifier) if identifier.name == "window")
    }

    /// Return a supported `test.each(...)` or `describe.each(...)` outer call.
    pub(in crate::lowering) fn table_test_call<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        let call = match expression {
            Expression::CallExpression(call) => call,
            Expression::ChainExpression(chain) => match &chain.expression {
                ChainElement::CallExpression(call) => call,
                _ => return None,
            },
            _ => return None,
        };
        self.table_each_callee(&call.callee).then_some(call)
    }

    /// Return whether a callee is the invoked result of `.each(...)`.
    pub(in crate::lowering) fn table_each_callee(&self, callee: &Expression<'_>) -> bool {
        let Expression::CallExpression(each_call) = callee else {
            return false;
        };
        let Expression::StaticMemberExpression(member) = &each_call.callee else {
            return false;
        };
        if member.property.name != "each" {
            return false;
        }
        matches!(
            &member.object,
            Expression::Identifier(object)
                if self.imports.is_test_builtin(object.name.as_str())
                    && matches!(object.name.as_str(), "test" | "it" | "describe")
        )
    }

    /// Return a supported `test.prop(...)` or `it.prop(...)` property-test call.
    pub(in crate::lowering) fn property_test_call<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        let Expression::CallExpression(prop_call) = &call.callee else {
            return None;
        };
        let Expression::StaticMemberExpression(member) = &prop_call.callee else {
            return None;
        };
        if member.property.name != "prop" {
            return None;
        }
        matches!(
            &member.object,
            Expression::Identifier(object)
                if self.imports.is_test_builtin(object.name.as_str())
                    && matches!(object.name.as_str(), "test" | "it")
        )
        .then_some(call)
    }

    /// Return a supported top-level `describe` call, if this expression is one.
    pub(in crate::lowering) fn describe_call<'a>(
        &self,
        expression: &'a Expression<'a>,
    ) -> Option<&'a oxc::ast::ast::CallExpression<'a>> {
        let Expression::CallExpression(call) = expression else {
            return None;
        };
        self.is_describe_callee(&call.callee).then_some(call)
    }

    /// Collect a test lifecycle callback that should wrap flattened tests.
    ///
    /// Smelt emits one Rust test per supported Vitest test case, so suite-level
    /// hooks are inherited into each flattened test body in declaration order.
    pub(in crate::lowering) fn collect_lifecycle_hook<'a>(
        &self,
        expression: &'a Expression<'a>,
        before_each: &mut Vec<&'a oxc::ast::ast::ArrowFunctionExpression<'a>>,
        after_each: &mut Vec<&'a oxc::ast::ast::ArrowFunctionExpression<'a>>,
    ) -> bool {
        let Expression::CallExpression(call) = expression else {
            return false;
        };
        let Expression::Identifier(callee) = &call.callee else {
            return false;
        };
        let name = callee.name.as_str();
        if !self.imports.is_test_builtin(name)
            || !matches!(name, "beforeAll" | "beforeEach" | "afterAll" | "afterEach")
        {
            return false;
        }
        let Some(callback_arg) = call.arguments.first() else {
            return false;
        };
        let Ok(callback) = self.test_arrow_callback(callback_arg, "lifecycle callbacks") else {
            return false;
        };
        if matches!(name, "beforeAll" | "beforeEach") {
            before_each.push(callback);
        } else {
            after_each.push(callback);
        }
        true
    }

    /// Inline setup hooks that are invoked from suite setup helpers.
    ///
    /// Suite expressions are copied into every native Rust test body. If such a
    /// helper registers `beforeEach` or `beforeAll`, executing its callback at
    /// that copied call site gives the same setup ordering for the current test.
    /// Teardown hooks remain handled by ordinary suite collection or by the
    /// per-test reset emitted for mutable Vitest runtime state.
    pub(in crate::lowering) fn inline_runtime_lifecycle_setup(
        &mut self,
        expression: &Expression<'_>,
        body: &mut Body,
        block: smelt_hir::BlockId,
    ) -> Result<bool, SmeltError> {
        let Expression::CallExpression(call) = expression else {
            return Ok(false);
        };
        let Expression::Identifier(callee) = &call.callee else {
            return Ok(false);
        };
        if !self.imports.is_test_builtin(callee.name.as_str())
            || !matches!(callee.name.as_str(), "beforeAll" | "beforeEach")
        {
            return Ok(false);
        }
        let Some(callback_arg) = call.arguments.first() else {
            return Ok(false);
        };
        let callback = self.test_arrow_callback(callback_arg, "lifecycle callbacks")?;
        for statement in &callback.body.statements {
            self.statement_in_block(statement, body, block)?;
        }
        Ok(true)
    }

    /// Return whether a callee belongs to an imported test-framework API.
    pub(in crate::lowering) fn is_test_framework_callee(&self, callee: &Expression<'_>) -> bool {
        match callee {
            Expression::Identifier(ident) => self.imports.is_test_builtin(ident.name.as_str()),
            Expression::CallExpression(call) => self.is_test_framework_callee(&call.callee),
            Expression::StaticMemberExpression(member)
                if member.property.name == "concurrent"
                    || member.property.name == "each"
                    || member.property.name == "skip"
                    || member.property.name == "skipIf"
                    || member.property.name == "only" =>
            {
                matches!(
                    &member.object,
                    Expression::Identifier(object)
                        if self.imports.is_test_builtin(object.name.as_str())
                )
            }
            _ => false,
        }
    }

    /// Return whether a callee is `describe` or `describe.concurrent`.
    pub(in crate::lowering) fn is_describe_callee(&self, callee: &Expression<'_>) -> bool {
        match callee {
            Expression::Identifier(ident) => {
                ident.name == "describe" && self.imports.is_test_builtin("describe")
            }
            Expression::StaticMemberExpression(member) if member.property.name == "concurrent" => {
                matches!(
                    &member.object,
                    Expression::Identifier(object)
                        if object.name == "describe" && self.imports.is_test_builtin("describe")
                )
            }
            _ => false,
        }
    }

    // Continued in the next split builder file.
}
