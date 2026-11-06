//! The callable member of a type: picking the function arm of a union, and the call
//! signature of an interface, that a call's arguments select.

use crate::lowering::ModuleBuilder;
use oxc::ast::ast::{Argument, Expression};
use smelt_hir::{Body, ErasureReason, FunctionType, Type};
use std::collections::HashMap;

impl ModuleBuilder<'_> {
    /// Extract a callable member from a union or function type.
    pub(in crate::lowering) fn function_member_type(
        &mut self,
        ty: smelt_hir::TypeId,
    ) -> Option<smelt_hir::TypeId> {
        self.function_member_type_for_arg_count(ty, None)
    }

    /// Extract a callable member matching a supplied argument count when available.
    pub(in crate::lowering) fn function_member_type_for_arg_count(
        &mut self,
        ty: smelt_hir::TypeId,
        arg_count: Option<usize>,
    ) -> Option<smelt_hir::TypeId> {
        self.function_member_type_for_args(ty, arg_count, &[])
    }

    /// Extract a callable member for a call site, honouring argument types.
    ///
    /// `arg_tys` carries one entry per supplied argument: `Some(ty)` when the
    /// argument's type could be read without lowering it (see
    /// [`Self::probe_argument_type`]) and `None` when it could not, meaning
    /// "this position constrains nothing". Overload selection uses those types
    /// to discard call signatures the arguments provably cannot satisfy, so an
    /// interface whose overloads differ only in parameter *types* resolves to
    /// the signature the call actually matches instead of the first one that
    /// happens to share its arity.
    pub(in crate::lowering) fn function_member_type_for_args(
        &mut self,
        ty: smelt_hir::TypeId,
        arg_count: Option<usize>,
        arg_tys: &[Option<smelt_hir::TypeId>],
    ) -> Option<smelt_hir::TypeId> {
        let resolved_ty = self.type_param_constraint_or_self(ty);
        match self.ctx.krate.types.get(resolved_ty).cloned() {
            Some(Type::Function(_)) => Some(resolved_ty),
            Some(Type::Optional(item)) => {
                self.function_member_type_for_args(item, arg_count, arg_tys)
            }
            Some(Type::Union(items)) => items
                .iter()
                .copied()
                .find_map(|item| self.function_member_type_for_args(item, arg_count, arg_tys)),
            Some(Type::Class { name, args }) => {
                self.interface_call_signature_type_for_args(name, &args, arg_count, arg_tys)
            }
            _ => None,
        }
    }

    /// Read an argument's type without lowering it into the body.
    ///
    /// Overload selection runs *before* the arguments are lowered, because the
    /// selected signature supplies the parameter-type hints the arguments are
    /// lowered against. Lowering an argument twice would duplicate its side
    /// effects, so this probe stays purely syntactic: it answers for the forms
    /// whose type is knowable from the source spelling alone (literals and
    /// plain identifiers already in scope) and answers `None` — "unconstrained"
    /// — for everything else. A `None` never rejects a candidate signature, so
    /// the probe can only ever make selection more precise, never wrong.
    pub(in crate::lowering) fn probe_argument_type(
        &mut self,
        argument: &Argument<'_>,
        body: &Body,
    ) -> Option<smelt_hir::TypeId> {
        let expression = argument.as_expression()?;
        match expression {
            Expression::NumericLiteral(_) => Some(self.ctx.krate.types.intern(Type::Float)),
            Expression::StringLiteral(_) => Some(self.ctx.krate.types.intern(Type::String)),
            Expression::TemplateLiteral(_) => Some(self.ctx.krate.types.intern(Type::String)),
            Expression::BooleanLiteral(_) => Some(self.ctx.krate.types.intern(Type::Bool)),
            Expression::Identifier(identifier) => {
                let name = identifier.name.as_str();
                let local = self.scope.lookup(name)?;
                Some(
                    self.narrowed_type(name)
                        .unwrap_or_else(|| Self::local_ty(body, local)),
                )
            }
            _ => None,
        }
    }

    /// Probe every argument of a call for overload selection.
    pub(in crate::lowering) fn probe_argument_types(
        &mut self,
        arguments: &[Argument<'_>],
        body: &Body,
    ) -> Vec<Option<smelt_hir::TypeId>> {
        arguments
            .iter()
            .map(|argument| self.probe_argument_type(argument, body))
            .collect()
    }

    /// Instantiate the call signature a call site selects from a callable interface.
    ///
    /// A callable interface may declare several overloads. Selection proceeds in
    /// the order TypeScript itself uses:
    ///
    /// 1. keep the overloads whose declared arity can accept `arg_count`
    ///    (optional and rest parameters relax the exact match);
    /// 2. discard those whose parameter types the *arguments* provably cannot
    ///    satisfy, using the side-effect-free [`Self::probe_argument_type`]
    ///    readings in `arg_tys`;
    /// 3. take the first survivor, in declaration order.
    ///
    /// Two cases have no single answer and are handled by
    /// [`Self::ambiguous_interface_call_type`] and
    /// [`Self::variadic_interface_call_type`] instead: several survivors that
    /// the argument types could not separate, and no survivor at all because
    /// the call passes more arguments than any overload declares. Both used to
    /// silently fall back to the *first* declared signature, which reported a
    /// return type from an overload the call does not run and — when that
    /// signature was shorter than the call — dropped the surplus arguments
    /// before they reached the callee.
    pub(in crate::lowering) fn interface_call_signature_type_for_args(
        &mut self,
        name: smelt_hir::Symbol,
        args: &[smelt_hir::TypeId],
        arg_count: Option<usize>,
        arg_tys: &[Option<smelt_hir::TypeId>],
    ) -> Option<smelt_hir::TypeId> {
        let signatures = self.interfaces.call_signatures(name).cloned()?;
        let interface = self.find_interface(name).cloned();
        let type_params = interface
            .map(|interface| interface.type_params)
            .unwrap_or_default();
        let substitutions = self
            .type_argument_substitution(&type_params, args, self.span(0, 0))
            .ok()?;
        // An overload set that shares one result is stored as ONE concrete
        // slot typed by the merged signature (`merged_overload_call_signature`),
        // so every call — and every read of the callable as a value — uses that
        // signature directly: the arguments are coerced into the merged
        // parameters and no per-overload adapter is built around the slot.
        if let Some(merged) = self.merged_overload_call_signature(&signatures) {
            return Some(self.instantiate_signature(&merged, &substitutions));
        }
        let Some(count) = arg_count else {
            // No call site to select against (`function_member_type`): the
            // first declared signature stands in for the callable's shape.
            let signature = signatures.first()?.clone();
            return Some(self.instantiate_signature(&signature, &substitutions));
        };
        let candidates = signatures
            .iter()
            .filter(|signature| Self::signature_accepts_arg_count(signature, count))
            .map(|signature| {
                let params = signature
                    .params
                    .iter()
                    .map(|param| self.substitute_type_params(*param, &substitutions))
                    .collect::<Vec<_>>();
                let return_ty = self.substitute_type_params(signature.return_ty, &substitutions);
                FunctionType {
                    params,
                    rest: signature.rest,
                    required_params: signature.required_params,
                    mutable_params: Vec::new(),
                    return_ty,
                    is_async: signature.is_async,
                    may_throw: false,
                }
            })
            .collect::<Vec<_>>();
        let matching = candidates
            .iter()
            .filter(|signature| self.signature_accepts_arg_types(signature, arg_tys))
            .cloned()
            .collect::<Vec<_>>();
        let selected = if matching.is_empty() {
            candidates
        } else {
            matching
        };
        match selected.len() {
            0 => Some(self.variadic_interface_call_type(count)),
            1 => {
                let signature = selected.into_iter().next()?;
                Some(self.finish_interface_call_type(signature))
            }
            _ => Some(self.ambiguous_interface_call_type(&selected, count)),
        }
    }

    /// Intern one already-substituted call signature as a HIR function type.
    fn finish_interface_call_type(&mut self, signature: FunctionType) -> smelt_hir::TypeId {
        let mutable_params =
            self.mutable_params_from_returned_tuple_state(&signature.params, signature.return_ty);
        self.ctx.krate.types.intern(Type::Function(FunctionType {
            mutable_params,
            ..signature
        }))
    }

    /// Instantiate a declared call signature under an interface's type arguments.
    fn instantiate_signature(
        &mut self,
        signature: &FunctionType,
        substitutions: &HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    ) -> smelt_hir::TypeId {
        let params = signature
            .params
            .iter()
            .map(|param| self.substitute_type_params(*param, substitutions))
            .collect::<Vec<_>>();
        let return_ty = self.substitute_type_params(signature.return_ty, substitutions);
        let mutable_params = self.mutable_params_from_returned_tuple_state(&params, return_ty);
        self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest: signature.rest,
            required_params: signature.required_params,
            mutable_params,
            return_ty,
            is_async: signature.is_async,
            may_throw: false,
        }))
    }

    /// Return whether the probed argument types can satisfy a call signature.
    ///
    /// Only positions whose type the probe could actually read take part; an
    /// unread position (`None`) constrains nothing, so a signature is rejected
    /// only on evidence.
    fn signature_accepts_arg_types(
        &self,
        signature: &FunctionType,
        arg_tys: &[Option<smelt_hir::TypeId>],
    ) -> bool {
        arg_tys.iter().enumerate().all(|(index, arg_ty)| {
            let Some(arg_ty) = *arg_ty else {
                return true;
            };
            // A rest slot absorbs every argument from its index on, and its
            // declared type is the *list*: each absorbed argument is checked
            // against the list's ELEMENT type. Without that check a leading
            // `(...handlers: Handler[])` overload claimed `use('/p', h)` ahead
            // of the `(path: string, handler: Handler)` one the call really
            // runs, and the string was packed into the handler list (E0308).
            // A rest slot of any other shape (a tuple rest) is not a
            // per-argument constraint this probe can check.
            if let Some(rest) = signature.rest
                && index >= rest
            {
                let Some(rest_ty) = signature.params.get(rest) else {
                    return true;
                };
                return match self.ctx.krate.types.get(*rest_ty) {
                    Some(Type::List(element)) => self.type_assignable_to(arg_ty, *element),
                    _ => true,
                };
            }
            let Some(param_ty) = signature.params.get(index) else {
                return true;
            };
            self.type_assignable_to(arg_ty, *param_ty)
        })
    }

    /// Build the call type for a call no declared overload can accept.
    ///
    /// JavaScript passes every supplied argument regardless of the declared
    /// arity, and an overloaded callable interface stores its implementation in
    /// one erased variadic `__smelt_call` slot (see
    /// `ModuleBuilder::overloaded_call_signature_slot_type`), so the call is
    /// still executable — es-toolkit's `curry` specs deliberately call a
    /// `CurriedFunction1` with a placeholder plus a value. Describing it with a
    /// shorter declared signature truncated the argument list at the adapter
    /// and called the callee with nothing. Which overload the callee then runs
    /// is decided by inspecting the runtime argument values, a genuine dynamic
    /// boundary, so the result is `unknown` — narrowed again by whatever the
    /// caller does with it.
    fn variadic_interface_call_type(&mut self, arg_count: usize) -> smelt_hir::TypeId {
        let unknown_ty = self.erase_with(ErasureReason::TypeLevel, "variadic interface call");
        self.ctx.krate.types.intern(Type::Function(FunctionType {
            params: vec![unknown_ty; arg_count],
            rest: None,
            required_params: Some(arg_count),
            mutable_params: Vec::new(),
            return_ty: unknown_ty,
            is_async: false,
            may_throw: false,
        }))
    }

    /// Build the call type for a call several overloads could still run.
    ///
    /// Overloads that share an arity are separated by their parameter *types*,
    /// and TypeScript separates es-toolkit's
    /// `(t1: __, t2: T2): CurriedFunction1<T1, R>` from
    /// `(t1: T1, t2: T2): R` by the `unique symbol` type of the placeholder.
    /// Smelt carries symbols as opaque runtime values, so both parameter
    /// positions read as `unknown` here and no static rule can pick between
    /// them — the callee picks at runtime by comparing the argument against its
    /// placeholder sentinel. Rather than guess one overload's return type (the
    /// old behaviour: `curried(2, 3)` claimed to return a `CurriedFunction1`,
    /// so the comparison against `6` const-folded to `false`), the call keeps
    /// each type only where every survivor agrees on it — the shared return
    /// type when they all return the same thing, and `unknown` otherwise, which
    /// is exactly the erased type the interface's single `__smelt_call` slot
    /// already carries. This is a real dynamic boundary, not erasure of a known
    /// shape: the callee inspects the argument values to decide, and the caller
    /// narrows the result back with the checks it already writes.
    fn ambiguous_interface_call_type(
        &mut self,
        candidates: &[FunctionType],
        arg_count: usize,
    ) -> smelt_hir::TypeId {
        let unknown_ty = self.erase_with(ErasureReason::TypeLevel, "ambiguous interface overload");
        let params = (0..arg_count)
            .map(|index| {
                let mut positional = candidates.iter().map(|candidate| {
                    if candidate.rest.is_some_and(|rest| index >= rest) {
                        return unknown_ty;
                    }
                    candidate.params.get(index).copied().unwrap_or(unknown_ty)
                });
                let first = positional.next().unwrap_or(unknown_ty);
                if positional.all(|param| param == first) {
                    first
                } else {
                    unknown_ty
                }
            })
            .collect::<Vec<_>>();
        let mut return_tys = candidates
            .iter()
            .map(|candidate| candidate.return_ty)
            .collect::<Vec<_>>();
        return_tys.sort_unstable_by_key(|ty| ty.0);
        return_tys.dedup();
        let return_ty = if let [single] = return_tys.as_slice() {
            *single
        } else {
            unknown_ty
        };
        let mutable_params = self.mutable_params_from_returned_tuple_state(&params, return_ty);
        self.ctx.krate.types.intern(Type::Function(FunctionType {
            params,
            rest: None,
            required_params: Some(arg_count),
            mutable_params,
            return_ty,
            is_async: candidates.iter().all(|candidate| candidate.is_async),
            may_throw: false,
        }))
    }

    /// Return whether a call-signature overload can accept `arg_count` arguments.
    ///
    /// The requested arity is acceptable when it supplies at least the required
    /// parameters and does not overflow the declared parameters — unless the
    /// signature has a rest parameter, which absorbs any surplus. This mirrors
    /// [`Self::function_arity_assignable`] at the call site so overload
    /// selection honours optional/rest arity instead of demanding an exact
    /// `params.len()` match.
    fn signature_accepts_arg_count(signature: &FunctionType, arg_count: usize) -> bool {
        let required = signature.required_params.unwrap_or(signature.params.len());
        if arg_count < required {
            return false;
        }
        if signature.rest.is_some() {
            return true;
        }
        arg_count <= signature.params.len()
    }
}
