//! Lowering of one call signature (`(x: T): R` / `<K extends C>(k: K): R`)
//! declared inside an interface or an object type literal.
//!
//! A call signature describes how a *value* of the surrounding type is called.
//! Smelt stores that value in the callable-interface struct's `__smelt_call`
//! slot, and a stored callable is a Rust `Rc<dyn Fn(..)>`: a trait object
//! cannot be generic over its own parameters. So a generic call signature is
//! lowered the way a hand-written port would store it — each of the
//! signature's OWN type parameters that has a constraint resolves to that
//! constraint (`<K extends string>(key: K): K` is stored as `Fn(String) ->
//! String`). This is the rule `push_closure_type_parameter_scope` implements
//! for every callable stored as a trait object: generic arrow closures and generic
//! function-type annotations follow it too, so a slot and the closure stored
//! into it agree on one concrete signature.
//!
//! An UNCONSTRAINED own parameter (`<T>(value: T): T`) has no single Rust type
//! to stand for it, so it stays a [`Type::TypeParam`]; codegen renders a free
//! parameter as the tagged runtime value, which is the genuine boundary for a
//! value callable at every type. Scoping the parameters at all is also what
//! stops `K` from resolving to an undeclared nominal type named `K`, which is
//! what it did before this module existed.
//!
//! # Overload sets
//!
//! An interface may declare several call signatures. When they all return the
//! same type, a hand-written port stores ONE closure whose parameters accept
//! every overload's arguments — exactly what the TypeScript implementation
//! signature does (`set: Setter = (key: string, value: number | string) => …`
//! behind `(key: string, value: number): void; (key: string, value: string):
//! void`). [`ModuleBuilder::merged_overload_call_signature`] builds that
//! signature position by position: a position's type is the union of the
//! overloads' types there, and it is optional when any overload makes it
//! optional or omits it. Overloads whose RESULT differs cannot share one
//! concrete signature — which result a call yields is decided by its runtime
//! arguments — so they keep the erased variadic slot
//! (`overloaded_call_signature_slot_type`).

use oxc::ast::ast::TSCallSignatureDeclaration;
use smelt_hir::{FunctionType, Type, TypeId};

use crate::SmeltError;
use crate::lowering::ModuleBuilder;

/// The lowered pieces of a call signature: parameter types, the index of a
/// trailing rest parameter, and the return type.
type CallSignatureParts = (Vec<TypeId>, Option<usize>, TypeId);

impl ModuleBuilder<'_> {
    /// Lower one call signature to the [`FunctionType`] its callable slot stores.
    ///
    /// Parameters without an annotation (and a missing return annotation) are
    /// `unknown`, matching TypeScript's implicit `any`. An optional parameter
    /// keeps the `Optional<T>` ABI so under-application can pass a typed
    /// `None`, and a trailing rest parameter keeps its index so the runtime
    /// arity survives. See the module docs for how the signature's own type
    /// parameters are treated.
    pub(in crate::lowering) fn call_signature_to_hir(
        &mut self,
        signature: &TSCallSignatureDeclaration<'_>,
    ) -> Result<FunctionType, SmeltError> {
        // The signature's own parameters resolve to their constraints while it
        // is lowered; see the module docs.
        self.push_closure_type_parameter_scope(signature.type_parameters.as_deref())?;
        let lowered = self.call_signature_parts_to_hir(signature);
        self.pop_type_parameter_scope();
        let (params, rest, return_ty) = lowered?;
        let required_params = Self::formal_parameters_required_count(&signature.params);
        Ok(FunctionType {
            mutable_params: self.mutable_params_from_returned_tuple_state(&params, return_ty),
            params,
            rest,
            required_params: Some(required_params),
            return_ty,
            is_async: matches!(self.ctx.krate.types.get(return_ty), Some(Type::Future(_))),
            may_throw: false,
        })
    }

    /// Lower a call signature's parameter list and return type, in the scope
    /// of its own type parameters (pushed by [`Self::call_signature_to_hir`]).
    fn call_signature_parts_to_hir(
        &mut self,
        signature: &TSCallSignatureDeclaration<'_>,
    ) -> Result<CallSignatureParts, SmeltError> {
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
            let ty = if param.optional {
                self.ctx.krate.types.intern(Type::Optional(ty))
            } else {
                ty
            };
            params.push(ty);
        }
        // A trailing rest parameter keeps its index so the signature's runtime
        // arity survives instead of the rest slot becoming a fixed array.
        let mut rest_index = None;
        if let Some(rest) = &signature.params.rest {
            let rest_ty = rest
                .type_annotation
                .as_ref()
                .map(|annotation| self.function_type_rest_param_to_hir(&annotation.type_annotation))
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
        Ok((params, rest_index, return_ty))
    }

    /// Merge an overload set whose signatures share one result into the single
    /// concrete signature that accepts every overload's arguments.
    ///
    /// Returns `None` — keep the erased slot — when there are fewer than two
    /// signatures, when any of them has a rest parameter (a rest slot does not
    /// line up positionally), or when their return types or `async`-ness
    /// differ (see the module docs). Parameter types are merged per position
    /// with [`Self::merged_overload_param`]; the merged signature requires only
    /// the arguments EVERY overload requires.
    pub(in crate::lowering) fn merged_overload_call_signature(
        &mut self,
        signatures: &[FunctionType],
    ) -> Option<FunctionType> {
        let (first, rest) = signatures.split_first()?;
        if rest.is_empty()
            || signatures.iter().any(|signature| signature.rest.is_some())
            || rest.iter().any(|signature| {
                signature.return_ty != first.return_ty || signature.is_async != first.is_async
            })
        {
            return None;
        }
        let arity = signatures.iter().map(|signature| signature.params.len()).max()?;
        let params = (0..arity)
            .map(|index| self.merged_overload_param(signatures, index))
            .collect::<Vec<_>>();
        let required_params = signatures
            .iter()
            .map(|signature| signature.required_params.unwrap_or(signature.params.len()))
            .min();
        Some(FunctionType {
            mutable_params: self.mutable_params_from_returned_tuple_state(&params, first.return_ty),
            params,
            rest: None,
            required_params,
            return_ty: first.return_ty,
            is_async: first.is_async,
            may_throw: false,
        })
    }

    /// The merged type of parameter `index` across an overload set.
    ///
    /// The distinct types the overloads declare at that position form a union
    /// (a single type when they agree, `unknown` when any of them is), in
    /// declaration order. The position is
    /// `Optional` when any overload spells it optional (`x?: T`, already
    /// lowered to `Optional<T>`), leaves it past its required count, or does
    /// not declare it at all — a call that matches that overload passes nothing
    /// there.
    fn merged_overload_param(&mut self, signatures: &[FunctionType], index: usize) -> TypeId {
        let mut members = Vec::new();
        let mut optional = false;
        for signature in signatures {
            let Some(param) = signature.params.get(index).copied() else {
                optional = true;
                continue;
            };
            if index >= signature.required_params.unwrap_or(signature.params.len()) {
                optional = true;
            }
            let param = match self.ctx.krate.types.get(param) {
                Some(Type::Optional(inner)) => {
                    optional = true;
                    *inner
                }
                _ => param,
            };
            if !members.contains(&param) {
                members.push(param);
            }
        }
        // `unknown | T` is `unknown` in TypeScript, so an overload that accepts
        // anything at this position makes the merged position accept anything.
        let unknown = self.ctx.krate.types.intern(Type::Unknown);
        let merged = match members.as_slice() {
            _ if members.contains(&unknown) => unknown,
            [single] => *single,
            _ => self.ctx.krate.types.intern(Type::Union(members)),
        };
        if optional {
            self.ctx.krate.types.intern(Type::Optional(merged))
        } else {
            merged
        }
    }
}
