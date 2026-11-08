//! Generic call inference: binding a callee's type parameters from its argument
//! types and instantiating the declared return with those bindings.

use crate::SmeltError;
use crate::lowering::ModuleBuilder;
use smelt_hir::{Body, FunctionType, Type, TypeParamDef};
use std::collections::HashMap;

impl ModuleBuilder<'_> {
    /// Instantiate a generic METHOD's declared result type for one call.
    ///
    /// Used by the class-method and static-method call paths, which resolve a
    /// concrete method item but previously typed the call with its declared
    /// return verbatim, so a method-level type parameter (`list<U = number>():
    /// U[]`, `static bag<T>(): Record<string, T>`) leaked into the caller as a
    /// raw `TypeParam` — aliasing any same-named caller parameter.
    ///
    /// `type_params` are the method's OWN type parameters (recorded in
    /// `HirCtx::erased_item_type_params`). Only these are instantiated:
    /// `return_ty` may still mention the enclosing class's parameters (already
    /// substituted from the receiver's arguments by `resolve_method`, or
    /// deliberately left as the caller's own `this` parameters), and those must
    /// never be rebound here.
    ///
    /// DYNAMIC BOUNDARY: every own parameter becomes `unknown`, without
    /// argument inference. Codegen emits a method's own type parameters as
    /// `SmeltUnknown` (methods are never generic over them in Rust), so typing
    /// the result from the arguments would promise a concrete container the
    /// erased definition does not return; `unknown` is exactly what the
    /// leaked parameter used to render as.
    ///
    /// Returns `return_ty` unchanged for a non-generic method, and keeps the
    /// declared return where instantiation would turn it into a function that
    /// renders as the erased `SmeltErasedFunction` (same rule as free calls).
    pub(in crate::lowering) fn instantiate_generic_method_result(
        &mut self,
        type_params: &[TypeParamDef],
        return_ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        if type_params.is_empty()
            || !self.overload_constraint_contains_unresolved_type_param(return_ty)
        {
            return return_ty;
        }
        let mut substitutions = HashMap::new();
        self.complete_callee_type_substitution(type_params, &mut substitutions);
        let substituted = self.substitute_type_params(return_ty, &substitutions);
        if self.renders_as_erased_function(substituted) {
            return_ty
        } else {
            substituted
        }
    }

    /// The result type of calling a generic function ITEM through its value
    /// (`(flatten as any)(array, depth)`), instantiated for these arguments.
    ///
    /// The value's type is the item's declared signature, spelled with the
    /// item's own type parameters; those are in scope only inside the item,
    /// so the result is instantiated as a direct call's is
    /// (`call_expression_lowering`'s item arm): bound from the arguments,
    /// then from a type assertion around the call
    /// (`bind_from_asserted_return`), and a parameter neither binds becomes
    /// `unknown`, the type the emitted definition yields for it. Without this
    /// the caller would hold a value of the callee's `T[]`, a type that names
    /// nothing at the call site.
    pub(in crate::lowering) fn instantiated_item_value_call_result(
        &mut self,
        item: smelt_hir::ItemId,
        function: &FunctionType,
        args: &[smelt_hir::ExprId],
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &Body,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        let type_params = self.callee_own_type_params(item);
        self.instantiated_call_return(&type_params, &function.params, args, function.return_ty, call, body)
    }

    /// The result type of one call of a generic callee: `return_ty` with the
    /// callee's own type parameters instantiated for this call.
    ///
    /// A direct call and a call through the item's value share this rule,
    /// because the instantiation does not depend on how the callee was named:
    ///
    /// 1. TypeScript's inference applies, approximated from the lowered
    ///    arguments ([`Self::infer_call_type_bindings`]): every position is a
    ///    candidate, read off the checker's answer where it has one;
    /// 2. an explicit type argument wider than that replaces it. A narrower one
    ///    does not: Rust binds the callee from its arguments alone, so the
    ///    arguments decide what the emitted call returns;
    /// 3. a type assertion around the call may bind what the arguments left
    ///    unbound;
    /// 4. a parameter nothing bound instantiates to `unknown`, the type the
    ///    emitted definition returns for it, so the result never carries a raw
    ///    callee `TypeParam` that would alias a same-named caller parameter;
    /// 5. the checker resolved the very same call, so the result may not
    ///    promise more than it did ([`Self::widened_call_type`]).
    ///
    /// A returned function with erased-unknown-rest parameters
    /// (`(...args: unknown[]) => T`, e.g. `constant`) lowers to the
    /// fully-erased `SmeltErasedFunction`, whose calls yield `unknown`
    /// regardless of the declared payload. Concretizing its return from the
    /// argument types would desync the value's runtime shape from the concrete
    /// type its call sites would then expect, so such a curried generic keeps
    /// its declared erased signature.
    pub(in crate::lowering) fn instantiated_call_return(
        &mut self,
        type_params: &[TypeParamDef],
        params: &[smelt_hir::TypeId],
        args: &[smelt_hir::ExprId],
        return_ty: smelt_hir::TypeId,
        call: &oxc::ast::ast::CallExpression<'_>,
        body: &Body,
    ) -> Result<smelt_hir::TypeId, SmeltError> {
        if type_params.is_empty() || !self.overload_constraint_contains_unresolved_type_param(return_ty) {
            return Ok(return_ty);
        }
        let resolved = self.checker_call_type(call.span.start, call.span.end);
        let mut substitutions = self.infer_call_type_bindings(type_params, params, args, body);
        // An `unknown` among the lowered arguments may be what Smelt lost and
        // not what the source says, so only without the checker's answer does
        // it stand for a candidate.
        if resolved.is_none() {
            self.widen_call_bindings(type_params, params, args, body, &mut substitutions);
        }
        let mut explicit_bindings: Vec<_> =
            self.explicit_call_type_bindings(type_params, call).unwrap_or_default().into_iter().collect();
        explicit_bindings.sort_unstable_by_key(|(name, _)| name.0);
        for (name, explicit) in explicit_bindings {
            if substitutions.get(&name).is_some_and(|bound| self.type_accepts_all_of(explicit, *bound)) {
                substitutions.insert(name, explicit);
            }
        }
        self.bind_from_asserted_return(type_params, return_ty, call.span, &mut substitutions);
        // A parameter reachable only through a union has no Rust generic to carry
        // it, so the definition yields it erased, and a generator's yield type
        // has no conversion at the call: the result keeps what the definition
        // returns.
        if matches!(self.ctx.krate.types.get(return_ty), Some(Type::Generator { .. })) {
            substitutions.retain(|name, _| {
                params.iter().any(|param| self.mentions_outside_a_union(*param, *name))
            });
        }
        self.complete_callee_type_substitution(type_params, &mut substitutions);
        let substituted = self.substitute_type_params(return_ty, &substitutions);
        if self.renders_as_erased_function(substituted) {
            return Ok(return_ty);
        }
        let result = self.widened_call_type(resolved, substituted);
        let span = self.span(call.span.start, call.span.end);
        self.call_result_shaped_by_checker(result, call.span.start, call.span.end, span)
    }

    /// Whether type parameter `name` occurs in `ty` at a position no union encloses.
    fn mentions_outside_a_union(&self, ty: smelt_hir::TypeId, name: smelt_hir::Symbol) -> bool {
        match self.ctx.krate.types.get(ty) {
            Some(Type::Union(_)) | None => false,
            Some(Type::TypeParam { name: found, .. }) => *found == name,
            Some(other) => other.children().any(|child| self.mentions_outside_a_union(child, name)),
        }
    }

    /// The bindings a call's explicit type arguments spell, if it has any that
    /// fit `type_params`. A selected overload may declare more parameters than
    /// the implementation these name, and then the spelling says nothing here.
    pub(in crate::lowering) fn explicit_call_type_bindings(
        &mut self,
        type_params: &[TypeParamDef],
        call: &oxc::ast::ast::CallExpression<'_>,
    ) -> Option<HashMap<smelt_hir::Symbol, smelt_hir::TypeId>> {
        let explicit = call.type_arguments.as_ref()?;
        if explicit.params.len() > type_params.len() {
            return None;
        }
        let explicit = explicit
            .params
            .iter()
            .map(|argument| self.ts_type_to_hir(argument))
            .collect::<Result<Vec<_>, _>>()
            .ok()?;
        self.type_argument_substitution(type_params, &explicit, self.span(call.span.start, call.span.end))
            .ok()
    }

    /// Bind a callee's own type parameters from one call's lowered arguments,
    /// the way TypeScript's inference does: every position is a candidate.
    ///
    /// Naked type-parameter arguments go first. A direct value such as
    /// `initValue: T` is a stronger constraint than a callback body whose
    /// contextual `T` is still unresolved; processing the callback first can
    /// prematurely bind `T` from an incidental operation and then reject the
    /// concrete initializer.
    ///
    /// The first candidate only opens the binding. A later wider one replaces it
    /// ([`Self::widen_call_bindings`]): `T` in `iterate(5, handler, 0)` is
    /// `unknown` when `handler` returns `unknown`, although `0` came first.
    fn infer_call_type_bindings(
        &mut self,
        type_params: &[TypeParamDef],
        params: &[smelt_hir::TypeId],
        args: &[smelt_hir::ExprId],
        body: &Body,
    ) -> HashMap<smelt_hir::Symbol, smelt_hir::TypeId> {
        let mut substitutions = HashMap::new();
        let mut inference_inputs = params.iter().zip(args).collect::<Vec<_>>();
        inference_inputs.sort_by_key(|(param, _)| {
            !matches!(self.ctx.krate.types.get(**param), Some(Type::TypeParam { .. }))
        });
        for (param, arg) in inference_inputs {
            let arg_ty = Self::expr_ty(body, *arg);
            let _ = self.infer_callee_type_bindings(type_params, *param, arg_ty, &mut substitutions);
        }
        substitutions
    }

    /// Replace bindings with the wider candidates of a call's arguments
    /// ([`Self::widen_call_binding`]).
    pub(in crate::lowering) fn widen_call_bindings(
        &mut self,
        type_params: &[TypeParamDef],
        params: &[smelt_hir::TypeId],
        args: &[smelt_hir::ExprId],
        body: &Body,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) {
        for (param, arg) in params.iter().zip(args) {
            self.widen_call_binding(type_params, *param, Self::expr_ty(body, *arg), substitutions);
        }
    }

    /// Replace a type parameter's binding with a wider candidate at one
    /// `expected`/`actual` position, walking the covariant positions: a
    /// callback's return is a candidate, and its parameters are not, because
    /// TypeScript prefers covariant candidates (`(acc: unknown) => number`
    /// with an init of `0` is `T = number`).
    ///
    /// A candidate is wider when it is `unknown`, or a union or optional holding
    /// the binding. An unrelated candidate (`both(numbers, strings)`, which tsc
    /// rejects) keeps the first binding here, and that is not a miscompile:
    /// codegen's call-site analysis classifies the two bindings as a `Conflict`
    /// and demotes the call to the erased form instead of emitting a signature
    /// rustc would reject (`generic_free_function_demotes_on_conflicting_bindings`
    /// and `callback_only_type_param_demotes_on_conflicting_call_site_evidence`
    /// in `smelt-codegen-rust` pin it). So this path needs no checked program.
    fn widen_call_binding(
        &mut self,
        type_params: &[TypeParamDef],
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) {
        let (Some(expected_ty), Some(actual_ty)) = (
            self.ctx.krate.types.get(expected).cloned(),
            self.ctx.krate.types.get(actual).cloned(),
        ) else {
            return;
        };
        match (expected_ty, actual_ty) {
            (Type::TypeParam { name }, actual_ty) => {
                let Some(bound) = substitutions.get(&name).copied() else {
                    return;
                };
                let wider = match actual_ty {
                    Type::Unknown => true,
                    Type::Union(members) => members.contains(&bound),
                    Type::Optional(item) => item == bound,
                    _ => false,
                };
                if wider && type_params.iter().any(|param| param.name == name) {
                    substitutions.insert(name, actual);
                }
            }
            (Type::Function(declared), Type::Function(given)) => {
                self.widen_call_binding(type_params, declared.return_ty, given.return_ty, substitutions);
            }
            (Type::List(declared), Type::List(given))
            | (Type::Set(declared), Type::Set(given))
            | (Type::Future(declared), Type::Future(given)) => {
                self.widen_call_binding(type_params, declared, given, substitutions);
            }
            (Type::Optional(declared), Type::Optional(given)) => {
                self.widen_call_binding(type_params, declared, given, substitutions);
            }
            (Type::Tuple(declared), Type::Tuple(given)) if declared.len() == given.len() => {
                for (declared, given) in declared.into_iter().zip(given) {
                    self.widen_call_binding(type_params, declared, given, substitutions);
                }
            }
            _ => {}
        }
    }

    /// Return whether `ty` is a function that Rust codegen renders as the
    /// fully-erased `SmeltErasedFunction`.
    ///
    /// Mirrors the codegen predicate: a non-async, non-throwing function whose
    /// single parameter is a rest list of erased items (`(...args: unknown[])
    /// => …`). Such a value carries no static parameter or return shape at the
    /// Rust ABI — every call returns `unknown` — so its declared return type
    /// must stay erased to match the runtime representation.
    fn renders_as_erased_function(&self, ty: smelt_hir::TypeId) -> bool {
        let Some(Type::Function(function)) = self.ctx.krate.types.get(ty) else {
            return false;
        };
        if function.is_async || function.may_throw || function.rest != Some(0) {
            return false;
        }
        if matches!(self.ctx.krate.types.get(function.return_ty), Some(Type::Future(_))) {
            return false;
        }
        let [param] = function.params.as_slice() else {
            return false;
        };
        let Some(Type::List(item)) = self.ctx.krate.types.get(*param) else {
            return false;
        };
        matches!(
            self.ctx.krate.types.get(*item),
            Some(Type::Unknown | Type::TypeParam { .. } | Type::Never)
        )
    }

    /// Infer generic overload substitutions while checking argument compatibility.
    pub(in crate::lowering) fn infer_overload_type(
        &mut self,
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        let expected = self.substitute_type_params(expected, substitutions);
        if expected == actual {
            return true;
        }
        match (
            self.ctx.krate.types.get(expected).cloned(),
            self.ctx.krate.types.get(actual).cloned(),
        ) {
            (Some(Type::TypeParam { name }), _) => {
                substitutions.insert(name, actual);
                true
            }
            (Some(Type::Unknown), _)
            | (_, Some(Type::Unknown | Type::TypeParam { .. }))
            | (Some(Type::Float), Some(Type::Int))
            | (Some(Type::Int), Some(Type::Float)) => true,
            _ if self.is_date_constructor_arg_type(expected)
                && self.is_date_constructor_arg_type(actual) =>
            {
                true
            }
            (Some(Type::Optional(expected_inner)), Some(Type::Optional(actual_inner))) => {
                (self.function_member_type(expected_inner).is_some()
                    && self.function_member_type(actual_inner).is_some())
                    || self.infer_overload_type(expected_inner, actual_inner, substitutions)
            }
            (Some(Type::Optional(_)), Some(Type::None)) => true,
            (Some(Type::Optional(inner)), _) if inner == actual => true,
            (Some(Type::Optional(inner)), _) => {
                self.infer_overload_type(inner, actual, substitutions)
            }
            // The actual argument may keep an `Optional` wrapper that
            // source-level narrowing (a `typeof` switch or truthiness guard)
            // already peeled — Smelt's erased locals are not re-typed across
            // narrowing. tsc validated the call, so match the payload type.
            (_, Some(Type::Optional(actual_inner))) => {
                self.infer_overload_type(expected, actual_inner, substitutions)
            }
            (Some(Type::List(expected_item)), Some(Type::List(actual_item)))
            | (Some(Type::Set(expected_item)), Some(Type::Set(actual_item)))
            | (Some(Type::Future(expected_item)), Some(Type::Future(actual_item))) => {
                self.infer_overload_type(expected_item, actual_item, substitutions)
            }
            (
                Some(Type::Dict(expected_key, expected_value)),
                Some(Type::Dict(actual_key, actual_value)),
            ) => {
                self.infer_overload_type(expected_key, actual_key, substitutions)
                    && self.infer_overload_type(expected_value, actual_value, substitutions)
            }
            (Some(Type::Tuple(expected_items)), Some(Type::Tuple(actual_items))) => {
                expected_items.len() == actual_items.len()
                    && expected_items.into_iter().zip(actual_items).all(
                        |(expected_item, actual_item)| {
                            self.infer_overload_type(expected_item, actual_item, substitutions)
                        },
                    )
            }
            (Some(Type::Tuple(expected_items)), Some(Type::List(actual_item))) => {
                expected_items.into_iter().all(|expected_item| {
                    self.infer_overload_type(expected_item, actual_item, substitutions)
                })
            }
            (
                Some(Type::Class {
                    name: expected_name,
                    args: expected_args,
                }),
                Some(Type::Class {
                    name: actual_name,
                    args: actual_args,
                }),
            ) => {
                expected_name == actual_name
                    && expected_args.len() == actual_args.len()
                    && expected_args.into_iter().zip(actual_args).all(
                        |(expected_arg, actual_arg)| {
                            self.infer_overload_type(expected_arg, actual_arg, substitutions)
                        },
                    )
            }
            (Some(Type::Function(expected_function)), Some(Type::Function(actual_function))) => {
                self.infer_overload_function_type(
                    &expected_function,
                    &actual_function,
                    substitutions,
                )
            }
            (Some(Type::Union(expected_items)), _) => {
                self.infer_overload_union_branch(expected_items, actual, substitutions)
            }
            (_, Some(Type::Union(actual_items))) => {
                self.infer_overload_actual_union_branch(expected, actual_items, substitutions)
            }
            _ => false,
        }
    }

    /// Infer against the first compatible expected union member and keep its substitutions.
    ///
    /// Structural members are tried before a naked type-parameter arm. For a
    /// signature like `readonly U[] | U` (remeda's `flatMap` callback return),
    /// inferring from a `number[]` argument must destructure the `U[]` arm to
    /// bind `U = number`, not greedily bind the bare `U` arm to the whole
    /// `number[]`. This mirrors tsc, where inference from a naked type parameter
    /// has lower priority than structural inference, and it is order-independent:
    /// union members are interned in a canonical (sorted) order, so relying on
    /// source spelling order here would be unstable.
    pub(in crate::lowering) fn infer_overload_union_branch(
        &mut self,
        expected_items: Vec<smelt_hir::TypeId>,
        actual: smelt_hir::TypeId,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        let (bare_params, structural): (Vec<_>, Vec<_>) = expected_items
            .into_iter()
            .partition(|item| matches!(self.ctx.krate.types.get(*item), Some(Type::TypeParam { .. })));
        for item in structural.into_iter().chain(bare_params) {
            let mut branch_substitutions = substitutions.clone();
            if self.infer_overload_type(item, actual, &mut branch_substitutions) {
                *substitutions = branch_substitutions;
                return true;
            }
        }
        false
    }

    /// Infer against the first compatible actual union member and keep its substitutions.
    pub(in crate::lowering) fn infer_overload_actual_union_branch(
        &mut self,
        expected: smelt_hir::TypeId,
        actual_items: Vec<smelt_hir::TypeId>,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        for item in actual_items {
            let mut branch_substitutions = substitutions.clone();
            if self.infer_overload_type(expected, item, &mut branch_substitutions) {
                *substitutions = branch_substitutions;
                return true;
            }
        }
        false
    }

    /// Infer compatibility for function-typed overload parameters.
    pub(in crate::lowering) fn infer_overload_function_type(
        &mut self,
        expected: &FunctionType,
        actual: &FunctionType,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        // An `async` callback and a sync-spelled `(...) => Promise<U>` slot are
        // the same TypeScript type; the async bit only differs in spelling. The
        // mismatch is bridged when the sync-spelled side's return is
        // Promise-shaped (or erased), and the return comparison below aligns
        // the Future wrapping.
        if expected.is_async != actual.is_async {
            let sync_return = if expected.is_async {
                actual.return_ty
            } else {
                expected.return_ty
            };
            if !self.future_shaped_return(sync_return) {
                return false;
            }
        }
        if actual.params.len() > expected.params.len()
            && (actual.rest != actual.params.len().checked_sub(1)
                || actual.params.len() != expected.params.len() + 1)
        {
            return false;
        }
        let mut actual_substitutions = HashMap::new();
        for (expected_param, actual_param) in expected.params.iter().zip(&actual.params) {
            let expected_param = self.substitute_type_params(*expected_param, substitutions);
            if !self.infer_callable_parameter_type(
                expected_param,
                *actual_param,
                &mut actual_substitutions,
            ) {
                return false;
            }
        }
        let actual_return_ty = self.substitute_type_params(actual.return_ty, &actual_substitutions);
        if self.infer_overload_type(expected.return_ty, actual_return_ty, substitutions) {
            return true;
        }
        // Async-vs-sync spelling can leave exactly one return Future-wrapped
        // (`Promise<U>` vs the async body's bare `U`); compare the unwrapped
        // payloads in that case.
        if expected.is_async != actual.is_async {
            match (
                self.future_payload_type(expected.return_ty),
                self.future_payload_type(actual_return_ty),
            ) {
                (Some(expected_inner), None) => {
                    return self.infer_overload_type(expected_inner, actual_return_ty, substitutions);
                }
                (None, Some(actual_inner)) => {
                    return self.infer_overload_type(expected.return_ty, actual_inner, substitutions);
                }
                _ => {}
            }
        }
        false
    }

    /// Return whether a sync-spelled callback return type is Promise-shaped
    /// (or erased enough to stand in for one) for async-spelling bridging.
    fn future_shaped_return(&self, return_ty: smelt_hir::TypeId) -> bool {
        matches!(
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(return_ty)),
            Some(Type::Future(_) | Type::Unknown | Type::TypeParam { .. })
        )
    }

    /// Return the payload type of a `Future`-shaped type, if it is one.
    fn future_payload_type(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::TypeId> {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(Type::Future(inner)) => Some(*inner),
            _ => None,
        }
    }

    /// Check that an actual callback can accept the input required by an overload parameter.
    pub(in crate::lowering) fn infer_callable_parameter_type(
        &mut self,
        required_input: smelt_hir::TypeId,
        actual_param: smelt_hir::TypeId,
        actual_substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        let actual_param = self.substitute_type_params(actual_param, actual_substitutions);
        if actual_param == required_input {
            return true;
        }
        match self.ctx.krate.types.get(actual_param).cloned() {
            Some(Type::TypeParam { name }) => {
                actual_substitutions.insert(name, required_input);
                true
            }
            Some(Type::Unknown) => true,
            _ => self.infer_overload_type(actual_param, required_input, actual_substitutions),
        }
    }
}
