//! Overload resolution: which declared signature of an overloaded function a call
//! selects, from arity, the shape of the source arguments and their probed types.
//!
//! The selected signature supplies parameter hints before the arguments are
//! lowered, so selection reads arguments without lowering them.

use crate::{OverloadSignature, SmeltError};
use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Argument, ArrayExpressionElement};
use smelt_hir::{Body, Field, FunctionType, Type};
use std::collections::HashMap;

impl ModuleBuilder<'_> {
    /// Keep an implementation's dynamic-arity function return over an overload's
    /// fixed-arity claim about it.
    ///
    /// TypeScript overloads are a *caller-side* argument-checking device: the
    /// value a call produces is whatever the single implementation body
    /// returns. When that body returns a variadic callable (`(...args) => R`)
    /// while the matched overload declares a fixed-arity result
    /// (`(t2: T2, t3: T3) => R`), adopting the overload's type forces codegen to
    /// materialize a fixed-arity Rust closure around a dynamic-arity runtime
    /// value. That adapter is lossy in two ways no later pass can undo: calls
    /// carrying more arguments than the declared arity silently drop the
    /// surplus, and `Function.length` answers the declared parameter count
    /// instead of the callable's own (0 for a rest-only function).
    ///
    /// So when the implementation's return type is a variadic function and the
    /// overload's fixed-arity replacement carries *no static information beyond
    /// its arity* — every parameter and the return type erased to `unknown` —
    /// the implementation's representation wins: the trade is a real runtime
    /// arity for an unenforceable claim about one. An overload that genuinely
    /// refines the shape (concrete parameter or return types, a curried
    /// interface, a non-function return) still wins, so this never costs
    /// precision that the declaration actually supplied.
    pub(in crate::lowering) fn representable_overload_return_ty(
        &mut self,
        implementation_return_ty: smelt_hir::TypeId,
        overload_return_ty: smelt_hir::TypeId,
    ) -> smelt_hir::TypeId {
        if implementation_return_ty == overload_return_ty {
            return overload_return_ty;
        }
        // The callee's LOWERED return is what its Rust body actually produces.
        // When that can be absent -- an `Option<T>`, or an erased value that may
        // carry `undefined` -- and the declared overload return cannot, the
        // declaration is a lie about a value the call site will observe. Keeping
        // the declared type there makes the emitter insert a coercion that
        // MANUFACTURES a value for absence (`map_or(Default::default(), ..)` for
        // an `Option`, a JS `ToNumber` whose nullish arm is `f64::NAN` for an
        // erased one) and then const-folds `result === undefined` to `false`.
        // The presence information wins, exactly as it already does below for a
        // callee that lowers to the fully-erased function shape.
        if self.lowered_return_admits_absence(implementation_return_ty, overload_return_ty) {
            // The lowered return is the implementation's own GENERIC return
            // (`T | undefined` for remeda/es-toolkit `maxBy<T>`), so it is
            // instantiated at the call site's evidence, which the selected
            // overload's instantiated return carries (`T = number`): matching
            // the two binds the implementation's parameters. Returning it
            // unsubstituted typed the call `Optional<T>`, which erased the
            // result the backend had just monomorphized (`Option<f64>` read
            // back as `Option<SmeltUnknown>`).
            let mut bindings = HashMap::new();
            if self.bind_return_through_absence(implementation_return_ty, overload_return_ty, &mut bindings)
                && !bindings.is_empty()
            {
                return self.substitute_type_params(implementation_return_ty, &bindings);
            }
            return implementation_return_ty;
        }
        let (
            Some(Type::Function(implementation_function)),
            Some(Type::Function(overload_function)),
        ) = (
            self.ctx.krate.types.get(implementation_return_ty),
            self.ctx.krate.types.get(overload_return_ty),
        ) else {
            return overload_return_ty;
        };
        if implementation_function.rest.is_some()
            && overload_function.rest.is_none()
            && self.function_type_is_bare_erased_shape(overload_function)
        {
            return implementation_return_ty;
        }
        overload_return_ty
    }

    /// Bind the implementation's return type parameters from the overload's
    /// instantiated return, looking through the absence the implementation
    /// adds (`T | undefined` against `number` binds `T = number`) and through
    /// the wrappers `lowered_return_admits_absence` walks.
    fn bind_return_through_absence(
        &mut self,
        lowered: smelt_hir::TypeId,
        declared: smelt_hir::TypeId,
        bindings: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        match (
            self.ctx.krate.types.get(lowered).cloned(),
            self.ctx.krate.types.get(declared).cloned(),
        ) {
            (Some(Type::Future(lowered_inner)), Some(Type::Future(declared_inner)))
            | (Some(Type::List(lowered_inner)), Some(Type::List(declared_inner))) => {
                self.bind_return_through_absence(lowered_inner, declared_inner, bindings)
            }
            (Some(Type::Optional(lowered_inner)), Some(declared_kind))
                if !matches!(declared_kind, Type::Optional(_)) =>
            {
                self.infer_overload_type(lowered_inner, declared, bindings)
            }
            _ => self.infer_overload_type(lowered, declared, bindings),
        }
    }

    /// Whether the lowered return type can be absent where the declared
    /// overload return type cannot.
    ///
    /// Walks both types together through the wrappers that do not themselves
    /// decide presence (`Promise`/`Future`, `List`), and reports the first
    /// position where the lowered side admits absence and the declared side
    /// does not:
    ///
    /// lowered `Optional(T)` against a non-optional declaration — the callee
    /// returns `None` for some input, as `maxBy` does on an empty array.
    ///
    /// A declaration that is itself optional or erased asserts nothing extra,
    /// so it keeps winning; this only ever *removes* a claim the callee cannot
    /// honour.
    ///
    /// A lowered `Unknown` is deliberately NOT treated as absence here. The
    /// data-last / purry idiom writes the implementation as
    /// `f(...args: unknown[]): unknown` precisely so the overloads carry the
    /// contract, and there the declaration is the more truthful of the two.
    fn lowered_return_admits_absence(
        &self,
        lowered: smelt_hir::TypeId,
        declared: smelt_hir::TypeId,
    ) -> bool {
        let lowered_ty = self.ctx.krate.types.get(lowered);
        let declared_ty = self.ctx.krate.types.get(declared);
        match (lowered_ty, declared_ty) {
            (Some(Type::Future(lowered_inner)), Some(Type::Future(declared_inner)))
            | (Some(Type::List(lowered_inner)), Some(Type::List(declared_inner))) => {
                self.lowered_return_admits_absence(*lowered_inner, *declared_inner)
            }
            (Some(Type::Optional(_)), Some(declared_kind)) => !matches!(
                declared_kind,
                Type::Optional(_) | Type::Unknown | Type::None | Type::TypeParam { .. }
            ),
            _ => false,
        }
    }

    /// Whether a function type says nothing about its values beyond its arity.
    ///
    /// Every parameter and the return type is either `unknown` or a type
    /// parameter that overload instantiation left unbound — a slot the call
    /// site supplied no evidence for, which codegen renders as `SmeltUnknown`
    /// exactly like `unknown` itself. Such a signature constrains no argument
    /// and promises no result: it is a bare callable shape carrying only an
    /// arity.
    fn function_type_is_bare_erased_shape(&self, function: &FunctionType) -> bool {
        let carries_no_type = |ty: &smelt_hir::TypeId| {
            matches!(
                self.ctx.krate.types.get(*ty),
                Some(Type::Unknown | Type::TypeParam { .. })
            )
        };
        function.params.iter().all(carries_no_type) && carries_no_type(&function.return_ty)
    }

    /// Select the TypeScript overload signature that matches a call site.
    pub(in crate::lowering) fn selected_overload_signature(
        &mut self,
        name: &str,
        arguments: &[Argument<'_>],
        span: oxc::span::Span,
        body: &mut Body,
    ) -> Result<Option<OverloadSignature>, SmeltError> {
        let Some(signatures) = (if let Some(signatures) = self.functions.overloads(name) {
            Some(signatures.clone())
        } else {
            self.ctx.overloads.get(name).cloned()
        }) else {
            return Ok(None);
        };
        if signatures.is_empty() {
            return Ok(None);
        }
        let mut lowered_arg_tys = Vec::new();
        for argument in arguments {
            let arg = self.argument(argument, body)?;
            lowered_arg_tys.push(Self::expr_ty(body, arg));
        }
        let mut selected: Option<(
            (usize, usize, usize, usize),
            OverloadSignature,
            HashMap<_, _>,
        )> = None;
        for (order, signature) in signatures.iter().enumerate() {
            let mut substitutions = HashMap::new();
            let constraint_scope = Self::overload_type_param_constraint_scope(signature);
            self.types.push_constraint_scope(constraint_scope);
            let matches = self.overload_signature_matches_args(
                signature,
                arguments,
                &lowered_arg_tys,
                &mut substitutions,
            ) && self.overload_accepts_spread_shape(signature, arguments, &lowered_arg_tys)
                && self.overload_substitutions_satisfy_constraints(signature, &substitutions);
            self.types.pop_constraint_scope();
            if matches {
                let score =
                    self.overload_signature_specificity_score(signature, lowered_arg_tys.len());
                let literal_score =
                    usize::MAX - self.overload_source_arg_mismatch_count(signature, arguments);
                let none_collapse_score =
                    self.overload_substitution_none_collapse_score(&substitutions);
                let candidate = (
                    (
                        literal_score,
                        score,
                        none_collapse_score,
                        usize::MAX - order,
                    ),
                    signature.clone(),
                    substitutions,
                );
                if selected
                    .as_ref()
                    .is_none_or(|current| candidate.0 > current.0)
                {
                    selected = Some(candidate);
                }
            }
        }
        if let Some((_, signature, mut substitutions)) = selected {
            // What the arguments left unbound, a type assertion around the
            // call may bind; see `bind_from_asserted_return`.
            let type_params = signature.type_params.clone();
            self.bind_from_asserted_return(
                &type_params,
                signature.return_ty,
                span,
                &mut substitutions,
            );
            return Ok(Some(
                self.instantiate_overload_signature(signature, &substitutions),
            ));
        }
        // The author explicitly suppressed TypeScript's own rejection of this
        // call (`@ts-expect-error` / `@ts-ignore` on the preceding line), so no
        // declared overload *can* describe it — the call is a deliberate
        // runtime-behavior probe (lodash-compat specs call `range()`,
        // `clamp(5)`, `fill(1, 'a')`, ... this way). Fall back to the
        // implementation signature exactly like a sibling implementation call:
        // the caller lowers the supplied arguments against the implementation
        // parameter types and missing trailing parameters take their padded
        // defaults (`undefined` for erased/optional slots).
        if self.has_ts_error_suppression_before(span.start) {
            return Ok(None);
        }
        if self.allows_unknown_index_access() {
            let mut loose_selected: Option<(
                (usize, usize, usize, usize),
                OverloadSignature,
                HashMap<_, _>,
            )> = None;
            for (order, signature) in signatures.iter().enumerate() {
                if !Self::overload_signature_arity_matches(signature, arguments.len()) {
                    continue;
                }
                let mut substitutions = HashMap::new();
                let constraint_scope = Self::overload_type_param_constraint_scope(signature);
                self.types.push_constraint_scope(constraint_scope);
                let matches = self.loose_overload_signature_matches_args(
                    signature,
                    arguments,
                    &lowered_arg_tys,
                    &mut substitutions,
                ) && self.overload_accepts_spread_shape(signature, arguments, &lowered_arg_tys)
                    && self.overload_substitutions_satisfy_constraints(signature, &substitutions);
                self.types.pop_constraint_scope();
                if !matches {
                    continue;
                }
                let score =
                    self.overload_signature_specificity_score(signature, lowered_arg_tys.len());
                let literal_score =
                    usize::MAX - self.overload_source_arg_mismatch_count(signature, arguments);
                let none_collapse_score =
                    self.overload_substitution_none_collapse_score(&substitutions);
                let candidate = (
                    (
                        literal_score,
                        score,
                        none_collapse_score,
                        usize::MAX - order,
                    ),
                    signature.clone(),
                    substitutions,
                );
                if loose_selected
                    .as_ref()
                    .is_none_or(|current| candidate.0 > current.0)
                {
                    loose_selected = Some(candidate);
                }
            }
            if let Some((_, signature, substitutions)) = loose_selected {
                return Ok(Some(
                    self.instantiate_overload_signature(signature, &substitutions),
                ));
            }
            return Ok(None);
        }
        Err(SmeltError::unsupported(
            self.span(span.start, span.end),
            format!(
                "no overload of `{name}` matches this call (args: {:?}, overloads: {:?})",
                lowered_arg_tys
                    .iter()
                    .map(|ty| self.ctx.krate.types.get(*ty))
                    .collect::<Vec<_>>(),
                signatures
                    .iter()
                    .map(|signature| signature
                        .params
                        .iter()
                        .map(|ty| self.ctx.krate.types.get(*ty))
                        .collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            ),
        ))
    }

    /// Score how free a matching overload's inferences are of `undefined` collapse.
    ///
    /// Smelt interns TypeScript `void`, `undefined`, and `null` as one `None`
    /// type, so a candidate can bind a type parameter to `None` (for example
    /// `R := void` when a `noop` callback argument matches a
    /// `customizer: (...) => R` parameter) and still pass constraint checks
    /// that `tsc` would have failed on the distinct `void` type. When another
    /// candidate matches the same call *without* collapsing any type parameter
    /// to `None` — its signature absorbs the `undefined` through an explicit
    /// `R | undefined` or optional slot — that candidate mirrors the checker's
    /// real selection (e.g. `cloneWith(value, noop)` picks the
    /// `customizer: (...) => R | undefined` overload returning `R | T`).
    /// Returns `usize::MAX` minus the number of `None` substitutions so a
    /// larger score is better; used as a tie-break after literal and
    /// specificity scores in [`Self::selected_overload_signature`].
    fn overload_substitution_none_collapse_score(
        &self,
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> usize {
        usize::MAX
            - substitutions
                .values()
                .filter(|ty| matches!(self.ctx.krate.types.get(**ty), Some(Type::None)))
                .count()
    }

    /// Build the active constraint scope needed while matching a stored overload signature.
    pub(in crate::lowering) fn overload_type_param_constraint_scope(
        signature: &OverloadSignature,
    ) -> HashMap<smelt_hir::Symbol, smelt_hir::TypeId> {
        signature
            .type_params
            .iter()
            .filter_map(|param| param.constraint.map(|constraint| (param.name, constraint)))
            .collect()
    }

    /// Count obvious source literal conflicts with an overload parameter surface.
    pub(in crate::lowering) fn overload_source_arg_mismatch_count(
        &mut self,
        signature: &OverloadSignature,
        arguments: &[Argument<'_>],
    ) -> usize {
        if arguments.len() < 2 {
            return 0;
        }
        let constraints = Self::overload_type_param_constraint_scope(signature);
        signature
            .params
            .iter()
            .take(arguments.len())
            .zip(arguments)
            .filter(|(expected, argument)| {
                !self.overload_source_arg_matches_param(**expected, argument, &constraints)
            })
            .count()
    }

    /// Return whether one source literal can inhabit an overload parameter surface.
    pub(in crate::lowering) fn overload_source_arg_matches_param(
        &mut self,
        expected: smelt_hir::TypeId,
        argument: &Argument<'_>,
        constraints: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        let expected = match self.ctx.krate.types.get(expected).cloned() {
            Some(Type::TypeParam { name }) => constraints.get(&name).copied().unwrap_or(expected),
            _ => expected,
        };
        match self.ctx.krate.types.get(expected).cloned() {
            Some(Type::Unknown | Type::TypeParam { .. }) => true,
            Some(Type::Optional(inner)) => {
                matches!(argument, Argument::NullLiteral(_))
                    || self.overload_source_arg_matches_param(inner, argument, constraints)
            }
            Some(Type::Union(items)) => items
                .into_iter()
                .any(|item| self.overload_source_arg_matches_param(item, argument, constraints)),
            Some(Type::String) => matches!(
                argument,
                Argument::StringLiteral(_)
                    | Argument::TemplateLiteral(_)
                    | Argument::Identifier(_)
                    | Argument::CallExpression(_)
                    | Argument::StaticMemberExpression(_)
                    | Argument::ComputedMemberExpression(_)
            ),
            Some(Type::Int | Type::Float) => matches!(
                argument,
                Argument::NumericLiteral(_)
                    | Argument::BigIntLiteral(_)
                    | Argument::Identifier(_)
                    | Argument::CallExpression(_)
                    | Argument::StaticMemberExpression(_)
                    | Argument::ComputedMemberExpression(_)
            ),
            Some(Type::Bool) => matches!(
                argument,
                Argument::BooleanLiteral(_)
                    | Argument::Identifier(_)
                    | Argument::CallExpression(_)
                    | Argument::StaticMemberExpression(_)
                    | Argument::ComputedMemberExpression(_)
            ),
            Some(Type::None) => matches!(argument, Argument::NullLiteral(_)),
            Some(Type::Class { name, .. }) if self.stdlib_class_of_symbol(name) == Some(smelt_stdlib::StdlibClass::RegExp) => {
                matches!(
                    argument,
                    Argument::RegExpLiteral(_)
                        | Argument::Identifier(_)
                        | Argument::CallExpression(_)
                        | Argument::StaticMemberExpression(_)
                        | Argument::ComputedMemberExpression(_)
                )
            }
            _ => true,
        }
    }

    /// Return whether an overload accepts the call under permissive top-level shape checks.
    ///
    /// This is used only as the unknown-index fallback after strict inference has
    /// failed. It still rejects source-shape mismatches such as a primitive value
    /// being passed where an overload expects a tuple/case value, which keeps
    /// broad Remeda data-last signatures from shadowing data-first calls.
    pub(in crate::lowering) fn loose_overload_signature_matches_args(
        &mut self,
        signature: &OverloadSignature,
        arguments: &[Argument<'_>],
        arg_tys: &[smelt_hir::TypeId],
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        if Self::overload_signature_arity_matches(signature, arg_tys.len())
            && signature.rest.is_none()
            && signature
                .params
                .iter()
                .take(arg_tys.len())
                .zip(arg_tys)
                .all(|(expected, actual)| {
                    self.loose_overload_type_matches(*expected, *actual, substitutions)
                })
        {
            return true;
        }
        let Some(rest_index) = signature.rest else {
            return false;
        };
        let Some((&rest_ty, fixed_params)) = signature.params.split_last() else {
            return arg_tys.is_empty();
        };
        if rest_index != fixed_params.len() || arg_tys.len() < fixed_params.len() {
            return false;
        }
        let mut rest_substitutions = substitutions.clone();
        let Some(fixed_args) = arg_tys.get(..fixed_params.len()) else {
            return false;
        };
        if !fixed_params
            .iter()
            .zip(fixed_args)
            .all(|(expected, actual)| {
                self.loose_overload_type_matches(*expected, *actual, &mut rest_substitutions)
            })
        {
            return false;
        }
        let Some(rest_args) = arg_tys.get(fixed_params.len()..) else {
            return false;
        };
        let Some(rest_arguments) = arguments.get(fixed_params.len()..) else {
            return false;
        };
        let rest_matches = match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(rest_ty))
            .cloned()
        {
            Some(Type::Tuple(_))
                if rest_args.len() == 1
                    && matches!(rest_arguments.first(), Some(Argument::SpreadElement(_))) =>
            {
                rest_args.first().is_some_and(|actual| {
                    self.loose_overload_type_matches(
                        rest_ty,
                        *actual,
                        &mut rest_substitutions,
                    )
                })
            }
            Some(Type::Tuple(items)) if items.len() == rest_args.len() => {
                items.iter().zip(rest_args).all(|(expected, actual)| {
                    self.loose_overload_type_matches(*expected, *actual, &mut rest_substitutions)
                })
            }
            Some(Type::List(item_ty)) => rest_args.iter().zip(rest_arguments).all(
                |(actual, argument)| {
                    let expected = if matches!(argument, Argument::SpreadElement(_)) {
                        rest_ty
                    } else {
                        item_ty
                    };
                    self.loose_overload_type_matches(
                        expected,
                        *actual,
                        &mut rest_substitutions,
                    )
                },
            ),
            _ => false,
        };
        if rest_matches {
            *substitutions = rest_substitutions;
        }
        rest_matches
    }

    /// Return whether an argument has a plausible top-level shape for an overload parameter.
    pub(in crate::lowering) fn loose_overload_type_matches(
        &mut self,
        expected: smelt_hir::TypeId,
        actual: smelt_hir::TypeId,
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        let expected = self.substitute_type_params(expected, substitutions);
        if expected == actual || self.infer_overload_type(expected, actual, substitutions) {
            return true;
        }
        match (
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(expected))
                .cloned(),
            self.ctx.krate.types.get(actual).cloned(),
        ) {
            (Some(Type::TypeParam { name }), _) => {
                substitutions.insert(name, actual);
                true
            }
            (Some(Type::Unknown), _) | (_, Some(Type::Unknown | Type::TypeParam { .. })) => true,
            (Some(Type::Optional(_)), Some(Type::None)) => true,
            (Some(Type::Optional(inner)), _) => {
                self.loose_overload_type_matches(inner, actual, substitutions)
            }
            (Some(Type::Union(items)), _) => items.into_iter().any(|item| {
                self.loose_overload_type_matches(item, actual, &mut substitutions.clone())
            }),
            (_, Some(Type::Union(items))) => items.into_iter().any(|item| {
                self.loose_overload_type_matches(expected, item, &mut substitutions.clone())
            }),
            (Some(Type::Tuple(_)), Some(Type::Tuple(_) | Type::List(_))) => true,
            (Some(Type::List(_)), Some(Type::List(_) | Type::Tuple(_))) => true,
            (Some(Type::Function(_)), Some(Type::Function(_))) => true,
            (
                Some(Type::Class {
                    name: expected_name,
                    ..
                }),
                Some(Type::Class {
                    name: actual_name, ..
                }),
            ) => expected_name == actual_name,
            (Some(Type::Dict(_, _)), Some(Type::Dict(_, _))) => true,
            (Some(Type::Set(_)), Some(Type::Set(_))) => true,
            (Some(Type::Future(_)), Some(Type::Future(_))) => true,
            (Some(Type::Bool), Some(Type::Bool))
            | (Some(Type::String), Some(Type::String))
            | (Some(Type::Int | Type::Float), Some(Type::Int | Type::Float))
            | (Some(Type::None), Some(Type::None))
            | (Some(Type::Never), _) => true,
            _ => false,
        }
    }

    /// Return whether an overload has the same source-level argument count as a call.
    pub(in crate::lowering) fn overload_signature_arity_matches(
        signature: &OverloadSignature,
        argument_count: usize,
    ) -> bool {
        let minimum = signature.required_params.unwrap_or(signature.params.len());
        argument_count >= minimum
            && signature
                .rest
                .map_or(argument_count <= signature.params.len(), |_| true)
    }

    /// Score a matching overload by source-shape specificity.
    ///
    /// Broad purry overloads often have a generic data-first parameter that can
    /// accept an order-rule tuple. Counting concrete fixed/rest positions keeps
    /// the intended data-last overload from being shadowed for single-rule calls
    /// while still preferring data-first when it has an actual leading data
    /// argument plus rule arguments.
    pub(in crate::lowering) fn overload_signature_specificity_score(
        &self,
        signature: &OverloadSignature,
        argument_count: usize,
    ) -> usize {
        let fixed_arity = signature.rest.unwrap_or(signature.params.len());
        let fixed_score = signature
            .params
            .iter()
            .take(fixed_arity)
            .filter(|ty| self.overload_param_is_specific(**ty))
            .count();
        let rest_score = signature.rest.map_or(0, |rest_index| {
            let rest_ty = signature.params.get(rest_index).copied();
            let rest_count = argument_count.saturating_sub(rest_index);
            if rest_ty.is_some_and(|ty| self.overload_param_is_specific(ty)) {
                rest_count
            } else {
                0
            }
        });
        // Specificity rewards parameters whose declared type actually constrains
        // the argument, not the mere existence of a fixed slot. A leading generic
        // fixed parameter (e.g. data-first `sortBy(array: T, ...rules)`) is no more
        // specific than a variadic of a concrete union (data-last `sortBy(...rules)`),
        // so weight by the count of *specific* fixed params. This keeps TypeScript's
        // first-declared-overload-wins behaviour for purry data-last/data-first pairs
        // while still preferring overloads with genuinely concrete fixed parameters.
        fixed_score * 100 + rest_score
    }

    /// Reject fixed tuple-rest overloads for variable-length spread tails.
    ///
    /// A call such as `fn(head, ...values)` cannot select an overload whose
    /// rest parameter is a fixed tuple unless the spread expression itself has
    /// a fixed tuple shape. Otherwise runtime values beyond the tuple width
    /// would be discarded by later destructuring.
    pub(in crate::lowering) fn overload_accepts_spread_shape(
        &self,
        signature: &OverloadSignature,
        arguments: &[Argument<'_>],
        argument_tys: &[smelt_hir::TypeId],
    ) -> bool {
        let Some(rest_index) = signature.rest else {
            return true;
        };
        let Some(rest_ty) = signature.params.get(rest_index).copied() else {
            return false;
        };
        if !matches!(
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(rest_ty)),
            Some(Type::Tuple(_))
        ) {
            return true;
        }
        arguments
            .iter()
            .enumerate()
            .skip(rest_index)
            .filter(|(_, argument)| matches!(argument, Argument::SpreadElement(_)))
            .all(|(index, _)| {
                argument_tys.get(index).is_some_and(|ty| {
                    matches!(
                        self.ctx
                            .krate
                            .types
                            .get(self.type_param_constraint_or_self(*ty)),
                        Some(Type::Tuple(_))
                    )
                })
            })
    }

    /// Return whether a call argument is a syntactically empty array literal `[]`.
    pub(in crate::lowering) fn argument_is_empty_array_literal(argument: &Argument<'_>) -> bool {
        matches!(argument, Argument::ArrayExpression(array) if array.elements.is_empty())
    }

    /// Return whether every length-constrained parameter of `signature` has an
    /// argument that *proves* the length its annotation demands.
    ///
    /// A tuple parameter (`[A, B]`) or a non-empty-array parameter
    /// (`readonly [T, ...T[]]`, `NonEmptyArray<T>`) is a claim about the
    /// argument's length, and only the call site can settle it: a spread-free
    /// array literal with the right element count. A variable of type `T[]`, a
    /// call result, or a literal containing a spread proves nothing about its
    /// runtime length, so such a signature is inapplicable — otherwise the
    /// earliest-declared tuple overload swallows plain-array calls TypeScript
    /// routes to a later one, and its (often much narrower) return type becomes
    /// the call's type.
    ///
    /// A missing argument is not a failure here: `overload_signature_arity_matches`
    /// already ruled on arity, and an optional parameter may legally have none.
    fn overload_param_lengths_are_proven(
        signature: &OverloadSignature,
        arguments: &[Argument<'_>],
    ) -> bool {
        signature
            .param_lengths
            .iter()
            .enumerate()
            .all(|(index, requirement)| {
                let Some(requirement) = *requirement else {
                    return true;
                };
                let Some(argument) = arguments.get(index) else {
                    return true;
                };
                Self::argument_proven_array_length(argument)
                    .is_some_and(|len| requirement.is_satisfied_by(len))
            })
    }

    /// Return the statically known element count of an argument, if it has one.
    ///
    /// Only a spread-free array literal does. A spread element contributes an
    /// unknown number of elements, so a literal containing one has no proven
    /// length at all.
    fn argument_proven_array_length(argument: &Argument<'_>) -> Option<usize> {
        let Argument::ArrayExpression(array) = argument else {
            return None;
        };
        if array
            .elements
            .iter()
            .any(|element| matches!(element, ArrayExpressionElement::SpreadElement(_)))
        {
            return None;
        }
        Some(array.elements.len())
    }

    /// Return whether an empty array literal `[]` is assignable to `ty`.
    ///
    /// `[]` inhabits collection-shaped types (arrays/lists, sets, dictionaries)
    /// and fully-erased/generic positions, but never a function, a non-empty
    /// fixed tuple, or a union that lacks a collection member. This mirrors
    /// TypeScript assignability and prevents an empty literal from vacuously
    /// matching an element type whose only structural member is a tuple.
    pub(in crate::lowering) fn type_accepts_empty_array_literal(&self, ty: smelt_hir::TypeId) -> bool {
        match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(ty))
        {
            Some(
                Type::Unknown
                | Type::TypeParam { .. }
                | Type::List(_)
                | Type::Set(_)
                | Type::Dict(..),
            ) => true,
            Some(Type::Tuple(items)) => items.is_empty(),
            Some(Type::Optional(inner)) => self.type_accepts_empty_array_literal(*inner),
            Some(Type::Union(items)) => items
                .clone()
                .into_iter()
                .any(|item| self.type_accepts_empty_array_literal(item)),
            _ => false,
        }
    }

    /// Return whether an overload parameter carries a concrete enough type to
    /// prefer over generic or unknown catch-all signatures.
    pub(in crate::lowering) fn overload_param_is_specific(&self, ty: smelt_hir::TypeId) -> bool {
        !matches!(
            self.ctx
                .krate
                .types
                .get(self.type_param_constraint_or_self(ty)),
            Some(Type::Unknown | Type::TypeParam { .. })
        )
    }

    /// Return whether an overload signature accepts the lowered call argument types.
    ///
    /// TypeScript overload declarations represent `...rest: [A, B]` as a
    /// single tuple-typed parameter in HIR. Call sites still pass those tuple
    /// elements as individual source arguments, so overload matching expands a
    /// trailing tuple/list parameter when exact arity matching fails.
    pub(in crate::lowering) fn overload_signature_matches_args(
        &mut self,
        signature: &OverloadSignature,
        arguments: &[Argument<'_>],
        arg_tys: &[smelt_hir::TypeId],
        substitutions: &mut HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        // A spread argument (`fn(...values)`) has an unknown runtime length, so
        // it can only be absorbed by a rest parameter. A fixed-arity (rest-less)
        // overload must not claim it: e.g. `cartesianProduct(...inputs)` where
        // `inputs: number[][]` would otherwise match the 1-array overload
        // `(arr1: T[]): Array<[T]>` with `T = number[]`, mis-typing the result
        // as a list of 1-tuples-of-lists instead of routing to the variadic
        // implementation overload.
        let has_spread_argument = arguments
            .iter()
            .any(|argument| matches!(argument, Argument::SpreadElement(_)));
        if Self::overload_signature_arity_matches(signature, arg_tys.len())
            && signature.rest.is_none()
            && !has_spread_argument
            && Self::overload_param_lengths_are_proven(signature, arguments)
            && signature
                .params
                .iter()
                .take(arg_tys.len())
                .zip(arg_tys)
                .all(|(expected, actual)| {
                    self.infer_overload_type(*expected, *actual, substitutions)
                })
        {
            return true;
        }
        let Some(rest_index) = signature.rest else {
            return false;
        };
        let Some((&rest_ty, fixed_params)) = signature.params.split_last() else {
            return arg_tys.is_empty();
        };
        if rest_index != fixed_params.len() {
            return false;
        }
        if !Self::overload_param_lengths_are_proven(signature, arguments) {
            return false;
        }
        if arg_tys.len() < fixed_params.len() {
            return false;
        }
        let mut rest_substitutions = substitutions.clone();
        let Some(fixed_args) = arg_tys.get(..fixed_params.len()) else {
            return false;
        };
        if !fixed_params
            .iter()
            .zip(fixed_args)
            .all(|(expected, actual)| {
                self.infer_overload_type(*expected, *actual, &mut rest_substitutions)
            })
        {
            return false;
        }
        let Some(rest_args) = arg_tys.get(fixed_params.len()..) else {
            return false;
        };
        let Some(rest_arguments) = arguments.get(fixed_params.len()..) else {
            return false;
        };
        let rest_matches = match self
            .ctx
            .krate
            .types
            .get(self.type_param_constraint_or_self(rest_ty))
            .cloned()
        {
            Some(Type::Tuple(_))
                if rest_args.len() == 1
                    && matches!(rest_arguments.first(), Some(Argument::SpreadElement(_))) =>
            {
                rest_args.first().is_some_and(|actual| {
                    self.infer_overload_type(rest_ty, *actual, &mut rest_substitutions)
                })
            }
            Some(Type::Tuple(items)) if items.len() == rest_args.len() => {
                items.iter().zip(rest_args).all(|(expected, actual)| {
                    self.infer_overload_type(*expected, *actual, &mut rest_substitutions)
                })
            }
            // A `Type::List` rest models both `...rest: T[]` (accepts an empty
            // tail) and `...rest: NonEmptyArray<T>` (`[T, ...T[]]`, requires at
            // least one element). `min_rest` carries the required-prefix arity
            // erased during type lowering, so a required-prefix rest must see at
            // least that many rest arguments instead of matching an empty tail
            // vacuously. A leading spread argument can stand in for the required
            // prefix (its runtime length is unknown), so it bypasses the count.
            Some(Type::List(item_ty)) => {
                let rest_supplied_by_spread =
                    matches!(rest_arguments.first(), Some(Argument::SpreadElement(_)));
                (rest_supplied_by_spread || rest_args.len() >= signature.min_rest)
                    && rest_args.iter().zip(rest_arguments).all(|(actual, argument)| {
                        let expected = if matches!(argument, Argument::SpreadElement(_)) {
                            rest_ty
                        } else {
                            item_ty
                        };
                        // An empty array literal (`[]`) is only assignable to a
                        // collection-shaped element type. Without this guard its
                        // `Unknown` element type lets it vacuously satisfy a
                        // tuple/union element (e.g. an `OrderRule` rest), which
                        // would steal a genuine data-first call (`firstBy([], rule)`)
                        // into the variadic data-last overload.
                        if Self::argument_is_empty_array_literal(argument)
                            && !self.type_accepts_empty_array_literal(expected)
                        {
                            return false;
                        }
                        self.infer_overload_type(expected, *actual, &mut rest_substitutions)
                    })
            }
            _ => false,
        };
        if rest_matches {
            *substitutions = rest_substitutions;
        }
        rest_matches
    }

    /// Instantiate a selected overload signature with inferred generic types.
    pub(in crate::lowering) fn instantiate_overload_signature(
        &mut self,
        signature: OverloadSignature,
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> OverloadSignature {
        let params = signature
            .params
            .into_iter()
            .map(|param| self.substitute_type_params(param, substitutions))
            .collect();
        let return_ty = self.substitute_type_params(signature.return_ty, substitutions);
        OverloadSignature {
            type_params: signature.type_params,
            params,
            param_lengths: signature.param_lengths,
            rest: signature.rest,
            min_rest: signature.min_rest,
            required_params: signature.required_params,
            return_ty,
            is_async: signature.is_async,
        }
    }

    /// Return whether inferred generic overload arguments satisfy their declared bounds.
    ///
    /// Applicability of a TypeScript overload includes each generic `extends`
    /// constraint. Checking this after inference retains substitutions learned
    /// from nested parameter shapes while preventing broad purry overloads such
    /// as `<Options extends OptionsShape>(options?: Options)` from accepting a
    /// primitive data argument.
    pub(in crate::lowering) fn overload_substitutions_satisfy_constraints(
        &mut self,
        signature: &OverloadSignature,
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> bool {
        signature.type_params.iter().all(|param| {
            let Some(constraint) = param.constraint else {
                return true;
            };
            let Some(actual) = substitutions.get(&param.name).copied() else {
                return true;
            };
            let constraint = self.substitute_type_params(constraint, substitutions);
            if self.overload_constraint_contains_unresolved_type_param(constraint) {
                return true;
            }
            self.overload_constraint_accepts(actual, constraint)
        })
    }

    /// Peel the wrappers that do not decide a return's identity, then report
    /// the type parameter the leaf names, if it is one.
    pub(in crate::lowering) fn return_leaf_type_param(&self, ty: smelt_hir::TypeId) -> Option<smelt_hir::Symbol> {
        match self.ctx.krate.types.get(ty) {
            Some(Type::TypeParam { name }) => Some(*name),
            Some(Type::Future(inner) | Type::Optional(inner)) => {
                self.return_leaf_type_param(*inner)
            }
            _ => None,
        }
    }

    /// Return whether a generic bound still depends on contextual type inference.
    ///
    /// TypeScript can select a data-last overload such as
    /// `<T, K extends Keys<T>>(key: K) => (data: T) => ...` before `T` is
    /// known; the later callable context provides it. Enforcing `K`'s bound
    /// while `T` is still present rejects valid curried calls.
    pub(in crate::lowering) fn overload_constraint_contains_unresolved_type_param(
        &self,
        constraint: smelt_hir::TypeId,
    ) -> bool {
        match self.ctx.krate.types.get(constraint) {
            Some(Type::TypeParam { .. }) => true,
            Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
                self.overload_constraint_contains_unresolved_type_param(*item)
            }
            Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
                self.overload_constraint_contains_unresolved_type_param(*key)
                    || self.overload_constraint_contains_unresolved_type_param(*value)
            }
            Some(Type::Tuple(items) | Type::Union(items)) => items
                .iter()
                .copied()
                .any(|item| self.overload_constraint_contains_unresolved_type_param(item)),
            Some(Type::Class { args, .. }) => args
                .iter()
                .copied()
                .any(|arg| self.overload_constraint_contains_unresolved_type_param(arg)),
            Some(Type::Function(function)) => {
                function.params.iter().copied().any(|param| {
                    self.overload_constraint_contains_unresolved_type_param(param)
                }) || self.overload_constraint_contains_unresolved_type_param(function.return_ty)
            }
            Some(Type::Generator {
                yield_ty,
                return_ty,
                next_ty,
                ..
            }) => {
                self.overload_constraint_contains_unresolved_type_param(*yield_ty)
                    || self.overload_constraint_contains_unresolved_type_param(*return_ty)
                    || self.overload_constraint_contains_unresolved_type_param(*next_ty)
            }
            Some(Type::GeneratorResult {
                yield_ty,
                return_ty,
            }) => {
                self.overload_constraint_contains_unresolved_type_param(*yield_ty)
                    || self.overload_constraint_contains_unresolved_type_param(*return_ty)
            }
            Some(
                Type::Bool
                | Type::Int
                | Type::FixedInt(_)
                | Type::BigInt
                | Type::Float
                | Type::String
                | Type::Unknown
                | Type::Never
                | Type::None,
            )
            | None => false,
        }
    }

    /// Return whether an inferred overload argument satisfies its `extends` surface.
    ///
    /// Lowered object literals use `Dict` storage even when their TypeScript
    /// annotation is a named structural object. Constraint applicability must
    /// bridge that erased representation while primitive values still fail
    /// object constraints.
    pub(in crate::lowering) fn overload_constraint_accepts(
        &mut self,
        actual: smelt_hir::TypeId,
        constraint: smelt_hir::TypeId,
    ) -> bool {
        if self.type_assignable_to(actual, constraint) {
            return true;
        }
        if let (Some(Type::Function(expected)), Some(Type::Function(actual))) = (
            self.ctx.krate.types.get(constraint).cloned(),
            self.ctx.krate.types.get(actual).cloned(),
        ) {
            let mut inferred = HashMap::new();
            if self.infer_overload_function_type(&expected, &actual, &mut inferred) {
                return true;
            }
        }
        let (
            Some(Type::Dict(actual_key, actual_value)),
            Some(Type::Class { name, args }),
        ) = (
            self.ctx.krate.types.get(actual).cloned(),
            self.ctx.krate.types.get(constraint).cloned(),
        ) else {
            return false;
        };
        if !matches!(self.ctx.krate.types.get(actual_key), Some(Type::String)) {
            return false;
        }
        let Some(fields) = self.overload_structural_constraint_fields(name, &args) else {
            return false;
        };
        if fields.is_empty() {
            return true;
        }
        fields.into_iter().any(|field| {
            self.type_assignable_to(actual_value, field.ty)
                || (field.optional
                    && self.ctx.krate.types.get(field.ty).is_some_and(|ty| {
                        if let Type::Optional(inner) = ty {
                            self.type_assignable_to(actual_value, *inner)
                        } else {
                            false
                        }
                    }))
        })
    }

    /// Return declared fields for a named structural constraint with generic arguments applied.
    pub(in crate::lowering) fn overload_structural_constraint_fields(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
    ) -> Option<Vec<Field>> {
        if let Some(class) = self.class_by_symbol(name).cloned() {
            let substitutions = self
                .type_argument_substitution(&class.type_params, args, self.span(0, 0))
                .ok()?;
            return Some(self.substituted_fields(&class.fields, &substitutions));
        }
        if let Some(interface) = self.find_interface(name).cloned() {
            let substitutions = self
                .type_argument_substitution(&interface.type_params, args, self.span(0, 0))
                .ok()?;
            return Some(self.substituted_fields(&interface.fields, &substitutions));
        }
        let alias = self.find_type_alias(name).cloned()?;
        let substitutions = self
            .type_argument_substitution(&alias.type_params, args, self.span(0, 0))
            .ok()?;
        self.types.alias_fields(name)
            .cloned()
            .map(|fields| self.substituted_fields(&fields, &substitutions))
    }
}
