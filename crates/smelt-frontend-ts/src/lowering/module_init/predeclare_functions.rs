//! Function signature predeclaration: overload signatures, forward function
//! types and predeclared function items, so a body can call a function declared
//! further down.

use std::collections::HashSet;
use crate::{OverloadSignature, SmeltError};
use oxc::ast::ast::{BindingPattern, Declaration, Expression, Program, Statement};
use smelt_hir::{ErasureReason, Function, FunctionOwner, FunctionType, Item, Param, Type};
use super::super::{AssertionNarrowing, ModuleBuilder, RestParam};
use super::super::support::is_implemented_overload_signature;

impl ModuleBuilder<'_> {
    /// Drop imported overloads shadowed by implementations in the current module.
    ///
    /// Overload signatures are valid only for their concrete implementation.
    /// Because the builder carries visible items across files, a local helper
    /// with the same name as an imported overloaded function must not inherit
    /// that imported function's return surface.
    pub(in crate::lowering) fn shadow_cross_module_overloads(&mut self, implemented_functions: &HashSet<String>) {
        self.functions.shadow_implementations(implemented_functions);
    }

    /// Collect TypeScript overload signatures for concrete implementations.
    pub(in crate::lowering) fn collect_overload_signatures(
        &mut self,
        program: &Program<'_>,
        implemented_functions: &HashSet<String>,
    ) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function)
                    if is_implemented_overload_signature(function, implemented_functions) =>
                {
                    self.collect_overload_signature(function);
                }
                Statement::ExportDeclaration(export) => {
                    if let Declaration::FunctionDeclaration(function) = &export.declaration
                        && is_implemented_overload_signature(function, implemented_functions)
                    {
                        self.collect_overload_signature(function);
                    }
                }
                _ => {}
            }
        }
    }

    /// Collect one overload signature, skipping signatures outside the current type surface.
    pub(in crate::lowering) fn collect_overload_signature(&mut self, function: &oxc::ast::ast::Function<'_>) {
        let Some(id) = &function.id else {
            return;
        };
        if let Ok(signature) = self.overload_signature(function) {
            self.functions
                .push_overload(id.name.to_string(), signature.clone());
            let key = self.export_key(id.name.as_str());
            self.ctx
                .overloads
                .write().entry(key)
                .or_default()
                .push(signature);
        }
    }

    /// Lower a TypeScript overload declaration into callable metadata.
    pub(in crate::lowering) fn overload_signature(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> Result<OverloadSignature, SmeltError> {
        let type_params = self.push_type_parameter_scope(function.type_parameters.as_deref())?;
        let result = (|| {
            let mut params = Vec::new();
            // Recorded alongside the lowered type because the lowering erases
            // it: `[A, B]`, `[T, ...T[]]` and `T[]` are all `Type::List`.
            let mut param_lengths = Vec::new();
            for param in &function.params.items {
                let annotation = param.type_annotation.as_ref().ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(param.span.start, param.span.end),
                        "overload parameters must have explicit type annotations",
                    )
                })?;
                param_lengths.push(Self::parameter_length_requirement(
                    &annotation.type_annotation,
                ));
                params.push(self.ts_type_to_hir(&annotation.type_annotation)?);
            }
            let mut min_rest = 0;
            if let Some(rest) = &function.params.rest {
                let annotation = rest.type_annotation.as_ref().ok_or_else(|| {
                    SmeltError::unsupported(
                        self.span(rest.span.start, rest.span.end),
                        "overload rest parameters must have explicit type annotations",
                    )
                })?;
                min_rest = Self::rest_parameter_min_arity(&annotation.type_annotation);
                let rest_ty = self.ts_type_to_hir(&annotation.type_annotation)?;
                params.push(self.type_param_constraint_or_self(rest_ty));
                // A rest parameter's length lives in `min_rest`, so the
                // parallel vector carries no separate requirement for it.
                param_lengths.push(None);
            }
            // An ASSERTION overload (`asserts condition`) returns void at runtime,
            // exactly as `function_declaration` records for the implementation
            // signature. Lowering the annotation structurally instead yields `Bool`
            // (a `TSTypePredicate` is boolean-shaped), and the selected overload's
            // return type is what types the call's destination — so the call site
            // ended up with a `Bool` destination for a `None`-returning function and
            // the emitter dropped the call altogether, silently evaluating only its
            // arguments. es-toolkit `invariant` is exactly that shape: two `asserts
            // condition` overloads plus an implementation, and all four of its specs
            // saw a call that never happened.
            //
            // A `value is T` PREDICATE overload keeps `Bool`: a type predicate really
            // does return a boolean, and its callers read that boolean.
            let assertion_return = function
                .return_type
                .as_ref()
                .and_then(|annotation| self.assertion_return_type(&annotation.type_annotation))
                .transpose()?;
            let return_ty = if assertion_return.is_some() {
                self.ctx.krate.types.intern(Type::None)
            } else {
                function
                    .return_type
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(function.span.start, function.span.end),
                            "overload declarations must have explicit return types",
                        )
                    })?
            };
            Ok(OverloadSignature {
                type_params,
                params,
                param_lengths,
                rest: function
                    .params
                    .rest
                    .as_ref()
                    .map(|_| function.params.items.len()),
                min_rest,
                required_params: Some(
                    function
                        .params
                        .items
                        .iter()
                        .position(|param| {
                            param.optional || Self::formal_parameter_has_default(param)
                        })
                        .unwrap_or(function.params.items.len()),
                ),
                return_ty,
                is_async: function.r#async,
            })
        })();
        self.pop_type_parameter_scope();
        result
    }

    /// Collect top-level function signatures before lowering function bodies.
    pub(in crate::lowering) fn collect_forward_function_types(
        &mut self,
        program: &Program<'_>,
        implemented_functions: &HashSet<String>,
    ) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    if function.declare
                        || is_implemented_overload_signature(function, implemented_functions)
                    {
                        continue;
                    }
                    self.collect_forward_function_type(function);
                }
                Statement::ExportDeclaration(export) => {
                    let Declaration::FunctionDeclaration(function) = &export.declaration
                    else {
                        continue;
                    };
                    if function.declare
                        || is_implemented_overload_signature(function, implemented_functions)
                    {
                        continue;
                    }
                    self.collect_forward_function_type(function);
                }
                _ => {}
            }
        }
    }

    /// Collect one function declaration signature for hoisted callback references.
    pub(in crate::lowering) fn collect_forward_function_type(&mut self, function: &oxc::ast::ast::Function<'_>) {
        let Some(id) = &function.id else {
            return;
        };
        let Ok(_type_params) = self.push_type_parameter_scope(function.type_parameters.as_deref())
        else {
            return;
        };
        let result = self.forward_function_type(function, id.name.as_str());
        self.pop_type_parameter_scope();
        if let Ok((symbol, ty, rest)) = result {
            self.functions
                .set_forward_type(id.name.to_string(), symbol, ty);
            if let Some(rest) = rest {
                self.functions.set_rest(id.name.to_string(), rest);
                self.publish_export(|ctx| &mut ctx.function_rests, id.name.to_string(), rest);
            }
        }
    }

    /// Build the callable type for a forward function declaration.
    pub(in crate::lowering) fn forward_function_type(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        name_text: &str,
    ) -> Result<(smelt_hir::Symbol, smelt_hir::TypeId, Option<RestParam>), SmeltError> {
        let name = self.intern_source_name(name_text);
        let mut params = Vec::new();
        for (index, param) in function.params.items.iter().enumerate() {
            let ty = self.function_parameter_type(param)?;
            params.push(self.overload_omitted_param_type(name_text, index, param, ty));
        }
        let rest = if let Some(rest) = &function.params.rest {
            let Some(annotation) = &rest.type_annotation else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest function parameters must have explicit array type annotations",
                ));
            };
            let ty = self.ts_type_to_hir(&annotation.type_annotation)?;
            // A fixed-width tuple rest is fixed parameters (`lowering::tuple_rest`).
            if let Some(elements) = self.fixed_tuple_rest_elements(ty) {
                params.extend(elements);
                None
            } else {
            let (ty, item_ty) = self.rest_param_array_type(ty).map_err(|_error| {
                SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest function parameter type must be an array type",
                )
            })?;
            let index = params.len();
            params.push(ty);
            Some(RestParam { index, item_ty })
            }
        } else {
            None
        };
        let return_ty = self
            .function_return_type_or_overload(function, name_text)
            .unwrap_or_else(|_| {
                let span = self.span(function.span.start, function.span.end);
                self.erase_at(ErasureReason::MissingAnnotation, span, "function return")
            });
        let mutable_params = self.mutable_params_from_returned_tuple_state(&params, return_ty);
        let ty = self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest: rest.map(|rest| rest.index),
            required_params: None,
            mutable_params,
            return_ty,
            is_async: function.r#async,
            may_throw: false,
        }));
        Ok((name, ty, rest))
    }

    /// Resolve a function return type from its annotation or implementation overloads.
    pub(in crate::lowering) fn function_return_type_or_overload(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        name_text: &str,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if let Some(return_ty) =
            self.function_return_type_annotation_or_overload(function, name_text)?
        {
            return Ok(return_ty);
        }
        Err(SmeltError::unsupported(
            self.span(function.span.start, function.span.end),
            "function declarations must have an explicit return type",
        ))
    }

    /// The return an unannotated overload implementation is lowered at.
    ///
    /// One Rust return type cannot be both an array and an object map, so when
    /// the overloads disagree between those shapes the value is a real dynamic
    /// boundary: which one a call produces depends on its run-time argument
    /// (`all(promises)` over an array or a record). Otherwise the implementation
    /// keeps the last overload's return.
    fn overload_implementation_return(
        &mut self,
        name_text: &str,
        last_return: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        let shape = |builder: &Self, ty: smelt_hir::TypeId| {
            let settled = match builder.ctx.krate.types.get(ty) {
                Some(Type::Future(inner)) => *inner,
                _ => ty,
            };
            match builder.ctx.krate.types.get(settled) {
                Some(Type::List(_) | Type::Tuple(_)) => Some(false),
                Some(Type::Dict(..)) => Some(true),
                _ => None,
            }
        };
        let returns = self
            .functions
            .overloads(name_text)
            .map(|signatures| signatures.iter().map(|signature| signature.return_ty).collect::<Vec<_>>())
            .unwrap_or_default();
        let is_async = matches!(self.ctx.krate.types.get(last_return), Some(Type::Future(_)));
        let shapes = returns.iter().filter_map(|ty| shape(self, *ty)).collect::<Vec<_>>();
        if !(shapes.contains(&true) && shapes.contains(&false)) {
            return last_return;
        }
        let erased = self.erase_with(ErasureReason::OpenLayout, "overloads return an array or an object");
        if is_async {
            self.ctx.krate.types.intern(Type::Future(erased))
        } else {
            erased
        }
    }

    /// Resolve an optional function return annotation or implementation overload.
    pub(in crate::lowering) fn function_return_type_annotation_or_overload(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        name_text: &str,
    ) -> Result<Option<smelt_hir::TypeId>, SmeltError> {
        if let Some(return_type) = &function.return_type {
            return self.ts_type_to_hir(&return_type.type_annotation).map(Some);
        }
        if let Some(signature) = self.functions.last_overload(name_text) {
            let last_return = signature.return_ty;
            return Ok(Some(self.overload_implementation_return(name_text, last_return)));
        }
        Ok(None)
    }

    /// Reserve HIR item slots for hoisted top-level function declarations.
    pub(in crate::lowering) fn predeclare_function_items(
        &mut self,
        program: &Program<'_>,
        implemented_functions: &HashSet<String>,
        errors: &mut Vec<SmeltError>,
    ) {
        for statement in &program.body {
            match statement {
                Statement::FunctionDeclaration(function) => {
                    if function.declare
                        || is_implemented_overload_signature(function, implemented_functions)
                    {
                        continue;
                    }
                    if let Err(error) = self.predeclare_function_item(function) {
                        errors.push(error);
                    }
                }
                Statement::ExportDeclaration(export) => {
                    let Declaration::FunctionDeclaration(function) = &export.declaration
                    else {
                        continue;
                    };
                    if function.declare
                        || is_implemented_overload_signature(function, implemented_functions)
                    {
                        continue;
                    }
                    if let Err(error) = self.predeclare_function_item(function) {
                        errors.push(error);
                    }
                }
                _ => {}
            }
        }
    }

    /// Reserve one function item with its callable signature and no body yet.
    pub(in crate::lowering) fn predeclare_function_item(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
    ) -> Result<(), SmeltError> {
        let id = function.id.as_ref().ok_or_else(|| {
            SmeltError::unsupported(
                self.span(function.span.start, function.span.end),
                "anonymous function declarations are not lowered yet",
            )
        })?;
        if self.scope.has_function_item(id.name.as_str()) {
            return Ok(());
        }
        // A constructor function becomes a synthesized class, so it gets no
        // callable function item to shadow the class binding.
        if self.classes.is_constructor_function(id.name.as_str()) {
            return Ok(());
        }
        let type_params = self.push_type_parameter_scope(function.type_parameters.as_deref())?;
        let predicate_return = function
            .return_type
            .as_ref()
            .and_then(|annotation| self.predicate_return_type(&annotation.type_annotation))
            .transpose();
        let result = self.predeclared_function(function, id.name.as_str(), type_params);
        let returns_date = result
            .as_ref()
            .is_ok_and(|predeclared| self.type_is_known_date_value(predeclared.return_ty));
        self.pop_type_parameter_scope();
        let item = self.ctx.krate.push_item(Item::Function(result?));
        if returns_date {
            self.ctx.date_returning_functions.insert(item);
        }
        if let Ok(Some((parameter_name, target))) = predicate_return
            && let Some(param_index) = function.params.items.iter().position(|param| {
                matches!(
                    &param.pattern,
                    BindingPattern::BindingIdentifier(binding)
                        if binding.name.as_str() == parameter_name
                )
            })
        {
            self.functions.set_predicate(
                id.name.to_string(),
                AssertionNarrowing {
                    param_index,
                    target,
                },
            );
        }
        self.items.insert(id.name.to_string(), item);
        self.scope.register_function_item(id.name.to_string(), item);
        Ok(())
    }

    /// Build the body-less function item used by declaration hoisting.
    ///
    /// A later parameter's default initializer may reference an earlier
    /// parameter (`function f(array, value, end = array.length)`), so each
    /// parameter binding is registered as a local before the next parameter's
    /// type (and any default initializer it lowers) is inferred. The local
    /// scope is saved and restored around the loop because this prepass runs
    /// before the function body is lowered and must not leak parameter bindings
    /// into the surrounding module scope.
    pub(in crate::lowering) fn predeclared_function(
        &mut self,
        function: &oxc::ast::ast::Function<'_>,
        name_text: &str,
        type_params: Vec<smelt_hir::TypeParamDef>,
    ) -> Result<Function, SmeltError> {
        let name = self.intern_source_name(name_text);
        let mut params = Vec::new();
        self.with_empty_lexical_scope(|this| {
            for (index, param) in function.params.items.iter().enumerate() {
                let ty = this.function_parameter_type(param)?;
                let ty = this.overload_omitted_param_type(name_text, index, param, ty);
                let local = smelt_hir::LocalId(u32::try_from(index).unwrap_or(u32::MAX));
                let (param_name, span) =
                    if let BindingPattern::BindingIdentifier(binding) = &param.pattern {
                        this.scope.bind(binding.name.to_string(), local);
                        (
                            this.intern_source_name(binding.name.as_str()),
                            this.span(binding.span.start, binding.span.end),
                        )
                    } else {
                        (
                            this.intern_source_name(&format!("__param{index}")),
                            this.span(param.span.start, param.span.end),
                        )
                    };
                params.push(Param {
                    name: param_name,
                    local,
                    ty,
                    span,
                });
            }
            Ok::<_, SmeltError>(())
        })?;
        let mut rest_index = None;
        if let Some(rest) = &function.params.rest {
            let BindingPattern::BindingIdentifier(binding) = &rest.rest.argument else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "destructured rest parameters need rest binding lowering",
                ));
            };
            let Some(annotation) = &rest.type_annotation else {
                return Err(SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest function parameters must have explicit array type annotations",
                ));
            };
            let ty = self.ts_type_to_hir(&annotation.type_annotation)?;
            // A fixed-width tuple rest is fixed parameters, named as the
            // function body names them (`lowering::tuple_rest`).
            if let Some(elements) = self.fixed_tuple_rest_elements(ty) {
                for (offset, element) in elements.into_iter().enumerate() {
                    let index = params.len();
                    params.push(Param {
                        name: self.intern_source_name(&format!("{}__{offset}", binding.name)),
                        local: smelt_hir::LocalId(u32::try_from(index).unwrap_or(u32::MAX)),
                        ty: element,
                        span: self.span(binding.span.start, binding.span.end),
                    });
                }
            } else {
            let (ty, item_ty) = self.rest_param_array_type(ty).map_err(|_error| {
                SmeltError::unsupported(
                    self.span(rest.span.start, rest.span.end),
                    "rest function parameter type must be an array type",
                )
            })?;
            let index = params.len();
            rest_index = Some(index);
            let rest_param = RestParam { index, item_ty };
            self.functions.set_rest(name_text.to_owned(), rest_param);
            self.publish_export(|ctx| &mut ctx.function_rests, name_text.to_owned(), rest_param);
            params.push(Param {
                name: self.intern_source_name(binding.name.as_str()),
                local: smelt_hir::LocalId(u32::try_from(index).unwrap_or(u32::MAX)),
                ty,
                span: self.span(binding.span.start, binding.span.end),
            });
            }
        }
        let return_ty = self
            .function_return_type_or_overload(function, name_text)
            .unwrap_or_else(|_| {
                let span = self.span(function.span.start, function.span.end);
                self.erase_at(ErasureReason::MissingAnnotation, span, "function return")
            });
        let required_params = function
            .params
            .items
            .iter()
            .position(|param| param.optional || Self::formal_parameter_has_default(param))
            .unwrap_or(function.params.items.len());
        Ok(Function {
            name,
            span: self.span(function.span.start, function.span.end),
            type_params,
            params,
            rest: rest_index,
            required_params: Some(required_params),
            return_ty,
            is_async: function.r#async,
            is_test: false,
            body: None,
            owner: FunctionOwner::Module,
        })
    }

    /// Export `export const alias = fn` (optionally `fn as Sig`) as the item
    /// `fn` itself, exactly like `export { fn as alias }`.
    ///
    /// The ordinary exported-const path makes an importer INLINE the const's
    /// initializer, and that initializer names a binding private to this
    /// module (`const _getQueryParam = (..) => ..; export const getQueryParam =
    /// _getQueryParam as ..` in Hono's `utils/url`), which the importing module
    /// cannot resolve — the call lowered to `undefined`. When the initializer
    /// is (a type-level wrapper around) a bare name bound to a function item,
    /// the const IS that function under a second name, so the alias is
    /// published as the item.
    pub(super) fn alias_exported_function_consts(&mut self, variable: &oxc::ast::ast::VariableDeclaration<'_>) {
        if !matches!(variable.kind, oxc::ast::ast::VariableDeclarationKind::Const) {
            return;
        }
        for declarator in &variable.declarations {
            let BindingPattern::BindingIdentifier(binding) = &declarator.id else {
                continue;
            };
            let Some(mut init) = declarator.init.as_ref() else {
                continue;
            };
            loop {
                init = match init {
                    Expression::ParenthesizedExpression(inner) => &inner.expression,
                    Expression::TSAsExpression(inner) => &inner.expression,
                    Expression::TSSatisfiesExpression(inner) => &inner.expression,
                    Expression::TSNonNullExpression(inner) => &inner.expression,
                    _ => break,
                };
            }
            let Expression::Identifier(target) = init else {
                continue;
            };
            let Some(item) = self.items.get(target.name.as_str()).copied() else {
                continue;
            };
            let is_function = self
                .ctx
                .krate
                .items
                .get(usize::try_from(item.0).unwrap_or(usize::MAX))
                .is_some_and(|candidate| matches!(candidate, Item::Function(_)));
            if !is_function {
                continue;
            }
            let exported = binding.name.as_str().to_owned();
            self.items.insert(exported.clone(), item);
            self.publish_export_alias(exported, item);
        }
    }
}
