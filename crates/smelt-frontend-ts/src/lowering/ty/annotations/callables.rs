//! Function, constructor and call-signature annotations, and the assertion and
//! predicate return types of type guards.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::TSType;
use smelt_hir::{FunctionType, Type};

impl ModuleBuilder<'_> {
    /// Extract assertion-signature metadata from a TypeScript return annotation.
    ///
    /// Two assertion forms are recognized:
    /// - `asserts value is T` carries a narrowing target `T` and yields
    ///   `Some(Ok((value, Some(T))))`.
    /// - `asserts value` (a bare condition assertion, e.g. `invariant`) has no
    ///   structural narrowing target and yields `Some(Ok((value, None)))`.
    ///
    /// In both cases the function's runtime return type is void; the `Option`
    /// target only drives optional parameter narrowing at call sites.
    pub(in crate::lowering) fn assertion_return_type(
        &mut self,
        ty: &TSType<'_>,
    ) -> Option<Result<(String, Option<smelt_hir::TypeId>), SmeltError>> {
        let TSType::TSTypePredicate(predicate) = ty else {
            return None;
        };
        if !predicate.asserts {
            return None;
        }
        let oxc::ast::ast::TSTypePredicateName::Identifier(parameter) = &predicate.parameter_name
        else {
            return Some(Err(SmeltError::unsupported(
                self.span(predicate.span.start, predicate.span.end),
                "assertion functions on `this` are not lowered yet",
            )));
        };
        let Some(annotation) = &predicate.type_annotation else {
            return Some(Ok((parameter.name.to_string(), None)));
        };
        Some(
            self.ts_type_to_hir(&annotation.type_annotation)
                .map(|target| (parameter.name.to_string(), Some(target))),
        )
    }

    /// Extract `value is T` metadata from a TypeScript return annotation.
    pub(in crate::lowering) fn predicate_return_type(
        &mut self,
        ty: &TSType<'_>,
    ) -> Option<Result<(String, smelt_hir::TypeId), SmeltError>> {
        let TSType::TSTypePredicate(predicate) = ty else {
            return None;
        };
        if predicate.asserts {
            return None;
        }
        let oxc::ast::ast::TSTypePredicateName::Identifier(parameter) = &predicate.parameter_name
        else {
            return Some(Err(SmeltError::unsupported(
                self.span(predicate.span.start, predicate.span.end),
                "predicate functions on `this` are not lowered yet",
            )));
        };
        let Some(annotation) = &predicate.type_annotation else {
            return Some(Err(SmeltError::unsupported(
                self.span(predicate.span.start, predicate.span.end),
                "predicate functions must use `value is T`",
            )));
        };
        Some(
            self.ts_type_to_hir(&annotation.type_annotation)
                .map(|target| (parameter.name.to_string(), target)),
        )
    }

    /// Convert a TypeScript function type node into HIR callable metadata.
    pub(in crate::lowering) fn function_type_to_hir(
        &mut self,
        function: &oxc::ast::ast::TSFunctionType<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        // An explicit `this` parameter in a function *type* is purely type-level:
        // JavaScript drops the bound receiver at call sites, so the runtime signature
        // is just the declared `params`. oxc keeps `this_param` separate from
        // `params.items`, so erasing it here needs no further filtering.
        let _ = &function.this_param;
        self.callable_type_to_hir(
            function.type_parameters.as_deref(),
            &function.params,
            &function.return_type,
        )
    }

    /// Convert a TypeScript constructor type node into HIR callable metadata.
    ///
    /// A `new (args) => T` constructor type is, at runtime, an ordinary
    /// callable value: JavaScript classes *are* constructor functions, and
    /// `new ctor(args)` invokes that function to produce a `T`. So the honest
    /// lowering is the same `Type::Function` a plain `(args) => T` produces —
    /// its parameters are the constructor parameters and its return type is the
    /// constructed type. This lets a value typed with a constructor type flow
    /// through the existing closure / indirect-call machinery (see the
    /// value-callee path in `new_expr.rs`). The `abstract` marker is purely
    /// type-level (it forbids `new` on the *type-name* itself, not on values of
    /// this callable type) and carries no runtime signature, so it is erased.
    pub(in crate::lowering) fn constructor_type_to_hir(
        &mut self,
        constructor: &oxc::ast::ast::TSConstructorType<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        self.callable_type_to_hir(
            constructor.type_parameters.as_deref(),
            &constructor.params,
            &constructor.return_type,
        )
    }

    /// Lower the shared parts of a callable type annotation (function type or
    /// constructor type) into an HIR [`FunctionType`].
    ///
    /// `TSFunctionType` and `TSConstructorType` differ only in surface spelling
    /// (`(args) => T` vs `new (args) => T`) and a couple of type-level-only
    /// fields (`this_param`, `abstract`) that do not affect the runtime
    /// signature. Both reduce to the same ordinary callable value, so their
    /// parameter/return lowering is factored here to keep the two entry points
    /// in lockstep. A caller-supplied type-parameter scope wraps the lowering so
    /// generic callable types resolve their own parameters.
    ///
    /// A callable TYPE's own type parameters (`<K extends string>(key: K) => K`)
    /// describe a value that is stored and passed as one `Rc<dyn Fn(..)>`, which
    /// cannot be generic, so a constrained one resolves to its constraint — the
    /// rule `push_closure_type_parameter_scope` documents, shared with generic
    /// arrow closures and callable-interface call signatures so the three agree.
    pub(super) fn callable_type_to_hir(
        &mut self,
        type_parameters: Option<&oxc::ast::ast::TSTypeParameterDeclaration<'_>>,
        params: &oxc::ast::ast::FormalParameters<'_>,
        return_type: &oxc::ast::ast::TSTypeAnnotation<'_>,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        self.push_closure_type_parameter_scope(type_parameters)?;
        let mut lowered_params = Vec::new();
        let result = (|| {
            for param in &params.items {
                let param_ty = param
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.ts_type_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(param.span.start, param.span.end),
                            "function type parameters require explicit type annotations",
                        )
                    })?;
                let param_ty = if param.optional {
                    self.ctx.krate.types.intern(Type::Optional(param_ty))
                } else {
                    param_ty
                };
                lowered_params.push(param_ty);
            }
            let rest_index = if let Some(rest) = &params.rest {
                let rest_ty = rest
                    .type_annotation
                    .as_ref()
                    .map(|annotation| self.function_type_rest_param_to_hir(&annotation.type_annotation))
                    .transpose()?
                    .ok_or_else(|| {
                        SmeltError::unsupported(
                            self.span(rest.span.start, rest.span.end),
                            "rest function type parameters require explicit array types",
                        )
                    })?;
                self.push_rest_param_type(&mut lowered_params, rest_ty)?
            } else {
                None
            };
            let return_ty = self.ts_type_to_hir(&return_type.type_annotation)?;
            let mutable_params =
                self.mutable_params_from_returned_tuple_state(&lowered_params, return_ty);
            let required_params = Some(Self::formal_parameters_required_count(params));
            Ok(self.ctx.krate.types.intern(Type::Function(FunctionType {
                params: lowered_params,
                rest: rest_index,
                required_params,
                mutable_params,
                return_ty,
                is_async: false,
                may_throw: false,
            })))
        })();
        self.pop_type_parameter_scope();
        result
    }

    /// Count the leading required parameters of an oxc callable signature.
    ///
    /// TypeScript's `Function.length` (the source-level required arity) stops at
    /// the first parameter that is optional (`x?`) or carries a default value,
    /// and never counts a trailing rest parameter. This mirrors the same
    /// computation performed for concrete function declarations in
    /// [`Self::formal_parameter_has_default`], so callable *type* annotations
    /// (`(a: T, b?: U) => V`, aliases, callable fields) preserve the exact
    /// under-application semantics that assignability and call lowering depend
    /// on instead of falling back to `params.len()`.
    pub(in crate::lowering) fn formal_parameters_required_count(
        params: &oxc::ast::ast::FormalParameters<'_>,
    ) -> usize {
        params
            .items
            .iter()
            .position(|param| param.optional || Self::formal_parameter_has_default(param))
            .unwrap_or(params.items.len())
    }
}
