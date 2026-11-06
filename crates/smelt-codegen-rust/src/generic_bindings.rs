//! Conservative call-site binding of free-function type parameters.
//!
//! This module is deliberately independent of Rust source rendering. Both the
//! crate-wide generic-safety analysis and the emitter can therefore consume the
//! same directional MIR matcher without growing subtly different inference
//! rules.
#![expect(
    clippy::redundant_pub_crate,
    reason = "the binding analysis is shared with sibling codegen modules"
)]

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;
use smelt_hir::{FunctionType, Symbol, Type, TypeId};
use smelt_mir::{Constant, Mir, MirFunction, Operand, Place};

/// Why a declared type shape cannot safely collect call-site evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BindingUnsupportedReason {
    /// The declared or actual type is a union, whose Rust representation erases.
    Union,
    /// Declared and actual type constructors do not correspond.
    ShapeMismatch,
    /// Function arity or rest-parameter layout does not correspond.
    ///
    /// Also reported for a whole call site whose callee packs a rest parameter:
    /// the emitter packs the trailing arguments into one list before the call,
    /// a transformation positional argument alignment cannot represent.
    FunctionShape,
    /// The operand is a field or index projection whose type is not stored on it.
    ProjectedOperand,
    /// A literal's primitive type is unexpectedly absent from the MIR interner.
    MissingLiteralType,
}

/// Evidence collected for one callee type parameter at one call site.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TypeParamBinding {
    /// No argument supplied evidence for the parameter.
    Unbound,
    /// The parameter is pinned to a concrete MIR type.
    Concrete(TypeId),
    /// Its only observed evidence is emitted through an erased representation.
    Erased,
    /// Independent arguments require incompatible concrete instantiations.
    Conflict {
        /// First concrete type observed.
        first: TypeId,
        /// Incompatible concrete type observed later.
        second: TypeId,
    },
    /// The matcher intentionally declines to infer from this shape.
    Unsupported(BindingUnsupportedReason),
}

impl TypeParamBinding {
    /// Combine another observation without allowing weak evidence to discard a
    /// concrete binding obtained from a direct argument.
    fn merge(self, observation: Self) -> Self {
        match (self, observation) {
            (Self::Conflict { first, second }, _) | (_, Self::Conflict { first, second }) => {
                Self::Conflict { first, second }
            }
            (Self::Concrete(first), Self::Concrete(second)) if first != second => {
                Self::Conflict { first, second }
            }
            (Self::Concrete(ty), _) | (_, Self::Concrete(ty)) => Self::Concrete(ty),
            (Self::Erased, _) | (_, Self::Erased) => Self::Erased,
            (Self::Unsupported(reason), _) | (_, Self::Unsupported(reason)) => {
                Self::Unsupported(reason)
            }
            (Self::Unbound, Self::Unbound) => Self::Unbound,
        }
    }
}

/// Ordered bindings for all type parameters declared by one callee.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CalleeTypeParamBindings {
    /// Bindings in source declaration order for deterministic diagnostics.
    bindings: IndexMap<Symbol, TypeParamBinding>,
}

impl CalleeTypeParamBindings {
    /// Build a binding map declaring `names`, every entry left `Unbound`.
    ///
    /// Production maps are built by the collectors ([`collect_bindings`],
    /// `collect_bindings_from_types`, `bind_class_type_params`) from a real callee
    /// signature. Only the test constructors are test-only: they let consumers of
    /// the binding map (notably [`crate::type_substitution`]) be unit-tested against
    /// every binding state without standing up a whole MIR.
    #[cfg(test)]
    pub(crate) fn unbound(names: impl IntoIterator<Item = Symbol>) -> Self {
        Self {
            bindings: names
                .into_iter()
                .map(|name| (name, TypeParamBinding::Unbound))
                .collect(),
        }
    }

    /// Overwrite one declared parameter's binding outright.
    ///
    /// Test-only, and deliberately not a `merge`: a test wants the state it
    /// names, not the state merging would negotiate.
    #[cfg(test)]
    pub(crate) fn set_for_test(&mut self, name: Symbol, binding: TypeParamBinding) {
        self.bindings.insert(name, binding);
    }

    /// Return the collected binding for `name`.
    pub(crate) fn get(&self, name: Symbol) -> Option<TypeParamBinding> {
        self.bindings.get(&name).copied()
    }

    /// Return whether every declared parameter has one unambiguous concrete binding.
    pub(crate) fn all_concrete(&self) -> bool {
        self.bindings
            .iter()
            .all(|(_, binding)| matches!(binding, TypeParamBinding::Concrete(_)))
    }

    /// Record evidence for one declared parameter.
    fn observe(&mut self, name: Symbol, observation: TypeParamBinding) {
        if let Some(binding) = self.bindings.get_mut(&name) {
            *binding = binding.merge(observation);
        }
    }

    /// Mark every declared type parameter occurring within `ty` with weak evidence.
    fn observe_occurrences(&mut self, mir: &Mir, ty: TypeId, observation: TypeParamBinding) {
        let mut names = HashSet::new();
        collect_type_param_occurrences(mir, ty, &mut names);
        for name in names {
            self.observe(name, observation);
        }
    }
}

/// Collect call-site bindings by matching actual argument types against the
/// callee's declared parameter patterns.
///
/// Resolves each argument's static type with the emitter-free
/// [`operand_type`], which fails closed on field and index projections. The
/// emitter has a richer resolver and passes its answers to
/// [`collect_bindings_from_types`] directly; see that function's docstring for
/// why the two are allowed to differ, and in which direction.
pub(crate) fn collect_bindings(
    mir: &Mir,
    target_function: &MirFunction,
    source_function: &MirFunction,
    args: &[Operand],
) -> CalleeTypeParamBindings {
    let type_params = target_function
        .type_params
        .iter()
        .map(|param| param.name)
        .collect::<Vec<_>>();

    // Positional alignment cannot represent a packed rest parameter: the
    // emitter collects the trailing arguments into one list before the call, so
    // `params[i]` and `args[i]` stop corresponding at the rest position. Fail
    // closed for the whole call site rather than matching a list pattern against
    // a single element type.
    if target_function.rest.is_some() {
        let mut bindings = unbound_bindings(&type_params);
        for (_, binding) in &mut bindings.bindings {
            *binding = binding.merge(TypeParamBinding::Unsupported(
                BindingUnsupportedReason::FunctionShape,
            ));
        }
        return bindings;
    }

    let mut declared = Vec::new();
    let mut actual = Vec::new();
    // Declared positions whose argument type could not be resolved at all. They
    // carry no positional evidence, so they are recorded as weak evidence on
    // every type parameter they mention after the positional pass.
    let mut unresolved = Vec::new();
    for (index, param) in target_function.params.iter().enumerate() {
        let Some(declared_ty) = target_function
            .locals
            .get(usize::try_from(param.0).unwrap_or(usize::MAX))
            .map(|local| local.ty)
        else {
            continue;
        };
        let actual_ty = match args.get(index) {
            None => None,
            Some(argument) => match operand_type(mir, source_function, argument) {
                Ok(resolved) => Some(resolved),
                Err(reason) => {
                    unresolved.push((declared_ty, reason));
                    None
                }
            },
        };
        declared.push(declared_ty);
        actual.push(actual_ty);
    }

    let mut bindings = collect_bindings_from_types(mir, &type_params, &declared, &actual);
    for (declared_ty, reason) in unresolved {
        bindings.observe_occurrences(mir, declared_ty, TypeParamBinding::Unsupported(reason));
    }
    bindings
}

/// Collect call-site bindings from already-resolved declared and actual types.
///
/// This is the positional core of [`collect_bindings`], exposed so the emitter
/// can supply the two things it knows better than the emitter-free path can:
///
/// * the *emitted* parameter types, which are what the generated Rust call has
///   to satisfy. They can differ from the callee's MIR locals (a cross-module
///   overload, an erased signature), and binding against one while rendering
///   against the other would let the two disagree;
/// * argument types resolved through the emitter's own place typing, which
///   handles `Place::Field` and `Place::Index` projections that
///   [`operand_type`] deliberately refuses.
///
/// The two resolvers are allowed to differ only in that direction. The
/// crate-wide generic-safety scan in [`crate::classes`] keeps the stricter one
/// because it decides whether a callee is emitted generically *at all*: failing
/// closed there demotes one function, which is always sound. Failing closed at a
/// single call site would instead force an erased argument into a call the
/// definition already emits generically, so the emitter needs the resolver that
/// answers for more shapes.
///
/// `actual[i] == None` means "no evidence at this position" (an omitted
/// argument, or one whose type the caller declined to resolve) and leaves the
/// mentioned parameters `Unbound`.
pub(crate) fn collect_bindings_from_types(
    mir: &Mir,
    type_params: &[Symbol],
    declared: &[TypeId],
    actual: &[Option<TypeId>],
) -> CalleeTypeParamBindings {
    collect_bindings_from_types_in_scope(mir, type_params, declared, actual, &HashSet::new())
}

/// [`collect_bindings_from_types`] at a call site inside a function whose own
/// real Rust generics are `caller_generics`.
///
/// An argument typed by one of those parameters is a genuine instantiation:
/// `inner(xs)` inside `outer<T>` binds `inner`'s parameter to `outer`'s `T`,
/// which rustc infers exactly. Every other type parameter is from a scope that
/// erased it here and stays `Erased`. Before type parameters carried their
/// declaration's identity this case was answered by accident: the two `T`s
/// were one symbol, so the callee's parameter rendered as the caller's.
pub(crate) fn collect_bindings_from_types_in_scope(
    mir: &Mir,
    type_params: &[Symbol],
    declared: &[TypeId],
    actual: &[Option<TypeId>],
    caller_generics: &HashSet<Symbol>,
) -> CalleeTypeParamBindings {
    let mut bindings = unbound_bindings(type_params);
    let own_params = type_params.iter().copied().collect::<HashSet<_>>();
    let scope = MatchScope { own_params: &own_params, caller_generics };
    for (index, declared_ty) in declared.iter().enumerate() {
        let Some(Some(actual_ty)) = actual.get(index).copied() else {
            continue;
        };
        match_types_in(mir, *declared_ty, actual_ty, &scope, &mut bindings);
    }
    bindings
}

/// Whether `ty` mentions at least one type parameter and every type parameter
/// it mentions is in `scope`.
pub(crate) fn type_mentions_only_params(mir: &Mir, ty: TypeId, scope: &HashSet<Symbol>) -> bool {
    let mut names = HashSet::new();
    collect_type_param_occurrences(mir, ty, &mut names);
    !names.is_empty() && names.iter().all(|name| scope.contains(name))
}

/// Bind a generic class's type parameters from a concrete receiver type.
///
/// A method of `class Box<T>` is emitted inside `impl<T> Box<T>`, so a call on a
/// `Box<f64>` receiver instantiates `T = f64`. The receiver type is the whole
/// evidence: no argument matching is involved, and no interning is required,
/// because every class argument is already a `TypeId` in the receiver's type.
///
/// A receiver that is not a `Class` with the expected argument count (an erased
/// receiver, a union, a structurally different class) pins nothing:
/// every parameter is reported `Unsupported`, which
/// [`CalleeTypeParamBindings::all_concrete`] rejects.
pub(crate) fn bind_class_type_params(
    mir: &Mir,
    class_type_params: &[Symbol],
    receiver_ty: TypeId,
) -> CalleeTypeParamBindings {
    let mut bindings = unbound_bindings(class_type_params);
    let Some(Type::Class { args, .. }) = mir.types.get(receiver_ty) else {
        return unsupported_bindings(class_type_params, BindingUnsupportedReason::ShapeMismatch);
    };
    if args.len() != class_type_params.len() {
        return unsupported_bindings(class_type_params, BindingUnsupportedReason::ShapeMismatch);
    }
    for (name, arg) in class_type_params.iter().zip(args) {
        let observation = if actual_type_is_erased(mir, *arg) {
            TypeParamBinding::Erased
        } else {
            TypeParamBinding::Concrete(*arg)
        };
        bindings.observe(*name, observation);
    }
    bindings
}

/// Build a binding map declaring `type_params`, every entry `Unbound`.
fn unbound_bindings(type_params: &[Symbol]) -> CalleeTypeParamBindings {
    CalleeTypeParamBindings {
        bindings: type_params
            .iter()
            .map(|name| (*name, TypeParamBinding::Unbound))
            .collect(),
    }
}

/// Build a binding map declaring `type_params`, every entry `Unsupported`.
fn unsupported_bindings(
    type_params: &[Symbol],
    reason: BindingUnsupportedReason,
) -> CalleeTypeParamBindings {
    CalleeTypeParamBindings {
        bindings: type_params
            .iter()
            .map(|name| (*name, TypeParamBinding::Unsupported(reason)))
            .collect(),
    }
}

/// Apply `bindings` to a declared callee type and return the interned MIR type
/// the substitution produces.
///
/// This is the recursive return substitution the composite-generic-return work
/// is built on: `List<T>` with `{T -> Float}` resolves to the interned
/// `List<Float>`, which is the type the emitted Rust call really produces at a
/// monomorphized call site.
///
/// The MIR type table is frozen by the time codegen runs, so a substituted type
/// can only be *found*, never minted. That is not a limitation in practice: the
/// substituted type is by construction a type the caller already mentions (it is
/// the type of the value flowing out of the call), and every sub-component of an
/// interned composite is itself interned. When the lookup nevertheless fails,
/// `None` is the honest answer and the call site demotes to erasure.
///
/// Fails closed — `None` — on:
///
/// * a type parameter whose binding is anything but `Concrete`, and on one the
///   callee does not declare at all (an unrelated scope's `T`, which has no
///   instantiation here);
/// * a `Union` anywhere in the pattern: unions erase to `SmeltUnknown` in
///   emitted Rust, so a substituted union is not a type the generated call can
///   be claimed to produce;
/// * a substituted type that is absent from the frozen interner.
///
/// A declared type mentioning no type parameter at all substitutes to itself,
/// which is the honest answer.
pub(crate) fn substituted_type_id(
    mir: &Mir,
    declared: TypeId,
    bindings: &CalleeTypeParamBindings,
) -> Option<TypeId> {
    let resolve = |name| match bindings.get(name) {
        Some(TypeParamBinding::Concrete(bound)) => Some(bound),
        _ => None,
    };
    substitute_type(mir, declared, &resolve, Unrewritable::FailClosed)
}

/// Substitutes `substitutions` through `ty`, keeping the declared type of any
/// layer whose rewrite is not interned.
///
/// Codegen holds `&Mir` and cannot intern, so for a caller that wants a best
/// answer rather than a proof this is the honest fallback.
pub(crate) fn substitute_existing_type(
    mir: &Mir,
    ty: TypeId,
    substitutions: &HashMap<Symbol, TypeId>,
) -> TypeId {
    if substitutions.is_empty() {
        return ty;
    }
    let resolve = |name| substitutions.get(&name).copied();
    substitute_type(mir, ty, &resolve, Unrewritable::KeepDeclared).unwrap_or(ty)
}

/// What a substitution does with a layer it cannot rewrite: a parameter it has
/// no type for, a rewritten type the frozen interner does not hold, and (when
/// failing closed) a union, which erases.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Unrewritable {
    /// The whole substitution answers `None`.
    FailClosed,
    /// That layer stays as declared.
    KeepDeclared,
}

/// The one substitution: replaces each type parameter `resolve` has a type for,
/// reusing `declared` when nothing moved.
///
/// The interner is frozen during codegen, so a rewritten layer is a lookup,
/// never an insert.
fn substitute_type(
    mir: &Mir,
    declared: TypeId,
    resolve: &dyn Fn(Symbol) -> Option<TypeId>,
    unrewritable: Unrewritable,
) -> Option<TypeId> {
    let keep = (unrewritable == Unrewritable::KeepDeclared).then_some(declared);
    let Some(kind) = mir.types.get(declared) else {
        return keep;
    };
    match kind {
        Type::TypeParam { name } => return resolve(*name).or(keep),
        Type::Union(_) if unrewritable == Unrewritable::FailClosed => return None,
        _ => {}
    }
    let rewritten =
        kind.try_map_children(|child| substitute_type(mir, child, resolve, unrewritable))?;
    if rewritten == *kind {
        return Some(declared);
    }
    mir.types.lookup(&rewritten).or(keep)
}

/// Return whether `actual` is exactly `declared` with `bindings` applied.
///
/// The unforgiving, total counterpart of the evidence-collecting
/// [`match_types`]: that walk deliberately tolerates a mismatch landing in a
/// type-parameter-free subtree, because a `Dict` key that disagrees says nothing
/// about `V`. Here the same tolerance would accept a `Dict<String, T>` parameter
/// receiving a `Dict<Int, Float>` argument and emit E0308, so every component
/// must agree.
pub(crate) fn substitution_matches(
    mir: &Mir,
    declared: TypeId,
    actual: TypeId,
    bindings: &CalleeTypeParamBindings,
) -> bool {
    if substituted_type_id(mir, declared, bindings) == Some(actual) {
        return true;
    }
    // A shorter callback is exact on every position it declares: its leading
    // parameters and return are the slot's under `bindings`, and the call
    // site's arity adapter drops the trailing arguments.
    let (Some(Type::Function(slot)), Some(Type::Function(callback))) =
        (mir.types.get(declared), mir.types.get(actual))
    else {
        return false;
    };
    callback_accepts_fewer_params(slot, callback)
        && slot
            .params
            .iter()
            .zip(&callback.params)
            .all(|(slot_param, param)| substituted_type_id(mir, *slot_param, bindings) == Some(*param))
        && substituted_type_id(mir, slot.return_ty, bindings) == Some(callback.return_ty)
}

/// Whether `actual` is a callback declaring a strict prefix of `declared`'s
/// parameters with the same calling convention.
///
/// TypeScript accepts `acc => ..` for a `(value: T, index: number) => T`
/// slot, and JavaScript calls it with both arguments. Neither side may pack a
/// rest parameter (positions would stop corresponding), sync/async and
/// throwing must agree, and a mutable (`&mut`) position must agree on every
/// parameter the callback declares.
pub(crate) fn callback_accepts_fewer_params(declared: &FunctionType, actual: &FunctionType) -> bool {
    actual.params.len() < declared.params.len()
        && declared.rest.is_none()
        && actual.rest.is_none()
        && declared.is_async == actual.is_async
        && declared.may_throw == actual.may_throw
        && actual.mutable_params.iter().all(|index| declared.mutable_params.contains(index))
        && declared
            .mutable_params
            .iter()
            .filter(|index| **index < actual.params.len())
            .all(|index| actual.mutable_params.contains(index))
}

/// Resolve the static type of an operand without emitter state.
///
/// Shared with the crate-wide generic-safety scan in [`crate::classes`] so the
/// analysis and the matcher agree on how an argument's static type is found.
/// Failure is reported rather than guessed: a field or index projection carries
/// no type of its own here, and a caller local that is absent is a shape the
/// caller must decide about explicitly.
pub(crate) fn operand_type(
    mir: &Mir,
    caller: &MirFunction,
    operand: &Operand,
) -> Result<TypeId, BindingUnsupportedReason> {
    match operand {
        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => caller
            .locals
            .get(usize::try_from(local.0).unwrap_or(usize::MAX))
            .map(|decl| decl.ty)
            .ok_or(BindingUnsupportedReason::ShapeMismatch),
        Operand::Copy(Place::Field { .. } | Place::Index { .. } | Place::Global { .. })
        | Operand::Move(Place::Field { .. } | Place::Index { .. } | Place::Global { .. }) => {
            Err(BindingUnsupportedReason::ProjectedOperand)
        }
        Operand::Const(constant) => {
            literal_type(mir, constant).ok_or(BindingUnsupportedReason::MissingLiteralType)
        }
    }
}

/// Find the interned primitive type represented by a MIR literal.
fn literal_type(mir: &Mir, constant: &Constant) -> Option<TypeId> {
    let expected = match constant {
        Constant::Bool(_) => Type::Bool,
        Constant::Int(_) => Type::Int,
        Constant::Float(_) => Type::Float,
        Constant::BigInt(_) => Type::BigInt,
        Constant::String(_) => Type::String,
        Constant::None | Constant::Undefined => Type::None,
        Constant::Symbol(_) => Type::Unknown,
    };
    mir.types
        .all()
        .iter()
        .position(|candidate| *candidate == expected)
        .and_then(|index| u32::try_from(index).ok())
        .map(TypeId)
}

/// Return whether an actual argument type is erased in emitted Rust.
///
/// `Unknown`, `Never` and unions all render as the tagged `SmeltUnknown`
/// carrier, and a type parameter belonging to some other scope is not a
/// concrete instantiation either, so none of them pins a callee type parameter.
/// An absent (uninterned) id fails closed the same way; every `LocalDecl::ty`
/// comes from the same interner, so that arm is unreachable in practice.
///
/// This is the single definition of "erased argument" shared by the matcher
/// below and the crate-wide generic-safety scan in [`crate::classes`], so the
/// two cannot drift apart.
pub(crate) fn actual_type_is_erased(mir: &Mir, actual: TypeId) -> bool {
    matches!(
        mir.types.get(actual),
        Some(Type::Unknown | Type::Never | Type::Union(_) | Type::TypeParam { .. }) | None
    )
}

/// The type parameters one binding pass distinguishes.
struct MatchScope<'a> {
    /// The callee's own parameters: the ones being bound.
    own_params: &'a HashSet<Symbol>,
    /// The caller's real Rust generics: an actual type that is one of them is
    /// a genuine instantiation (`Concrete`), not an erased value.
    caller_generics: &'a HashSet<Symbol>,
}

/// Directionally match a declared callee type against one actual caller type,
/// with no caller generics in scope (see [`match_types_in`]).
#[cfg(test)]
fn match_types(
    mir: &Mir,
    declared: TypeId,
    actual: TypeId,
    own_params: &HashSet<Symbol>,
    bindings: &mut CalleeTypeParamBindings,
) {
    let caller_generics = HashSet::new();
    let scope = MatchScope { own_params, caller_generics: &caller_generics };
    match_types_in(mir, declared, actual, &scope, bindings);
}

/// Directionally match a declared callee type against one actual caller type.
fn match_types_in(
    mir: &Mir,
    declared: TypeId,
    actual: TypeId,
    scope: &MatchScope<'_>,
    bindings: &mut CalleeTypeParamBindings,
) {
    let Some(declared_ty) = mir.types.get(declared) else {
        return;
    };
    if let Type::TypeParam { name } = declared_ty
        && scope.own_params.contains(name)
    {
        let caller_generic = matches!(mir.types.get(actual),
            Some(Type::TypeParam { name: actual_name }) if scope.caller_generics.contains(actual_name));
        let observation = if actual_type_is_erased(mir, actual) && !caller_generic {
            TypeParamBinding::Erased
        } else {
            TypeParamBinding::Concrete(actual)
        };
        bindings.observe(*name, observation);
        return;
    }

    let Some(actual_ty) = mir.types.get(actual) else {
        bindings.observe_occurrences(
            mir,
            declared,
            TypeParamBinding::Unsupported(BindingUnsupportedReason::ShapeMismatch),
        );
        return;
    };
    match (declared_ty, actual_ty) {
        (Type::List(left), Type::List(right))
        | (Type::Set(left), Type::Set(right))
        | (Type::Optional(left), Type::Optional(right))
        | (Type::Future(left), Type::Future(right)) => {
            match_types_in(mir, *left, *right, scope, bindings);
        }
        (Type::Dict(left_key, left_value), Type::Dict(right_key, right_value))
        | (Type::JsMap(left_key, left_value), Type::JsMap(right_key, right_value)) => {
            match_types_in(mir, *left_key, *right_key, scope, bindings);
            match_types_in(mir, *left_value, *right_value, scope, bindings);
        }
        (Type::Tuple(left), Type::Tuple(right)) if left.len() == right.len() => {
            match_type_slices(mir, left, right, scope, bindings);
        }
        (
            Type::Class {
                name: left_name,
                args: left,
            },
            Type::Class {
                name: right_name,
                args: right,
            },
        ) if left_name == right_name && left.len() == right.len() => {
            match_type_slices(mir, left, right, scope, bindings);
        }
        (Type::Function(left), Type::Function(right)) => {
            match_function_types(mir, left, right, scope, bindings);
        }
        (
            Type::Generator {
                is_async: left_async,
                yield_ty: left_yield,
                return_ty: left_return,
                next_ty: left_next,
            },
            Type::Generator {
                is_async: right_async,
                yield_ty: right_yield,
                return_ty: right_return,
                next_ty: right_next,
            },
        ) if left_async == right_async => {
            match_types_in(mir, *left_yield, *right_yield, scope, bindings);
            match_types_in(mir, *left_return, *right_return, scope, bindings);
            match_types_in(mir, *left_next, *right_next, scope, bindings);
        }
        (
            Type::GeneratorResult {
                yield_ty: left_yield,
                return_ty: left_return,
            },
            Type::GeneratorResult {
                yield_ty: right_yield,
                return_ty: right_return,
            },
        ) => {
            match_types_in(mir, *left_yield, *right_yield, scope, bindings);
            match_types_in(mir, *left_return, *right_return, scope, bindings);
        }
        (Type::Union(_), _) | (_, Type::Union(_)) => bindings.observe_occurrences(
            mir,
            declared,
            TypeParamBinding::Unsupported(BindingUnsupportedReason::Union),
        ),
        _ if declared == actual => {}
        _ => bindings.observe_occurrences(
            mir,
            declared,
            TypeParamBinding::Unsupported(BindingUnsupportedReason::ShapeMismatch),
        ),
    }
}

/// Match corresponding elements of equal-shape type constructors.
fn match_type_slices(
    mir: &Mir,
    declared: &[TypeId],
    actual: &[TypeId],
    scope: &MatchScope<'_>,
    bindings: &mut CalleeTypeParamBindings,
) {
    for (declared_item, actual_item) in declared.iter().zip(actual) {
        match_types_in(mir, *declared_item, *actual_item, scope, bindings);
    }
}

/// Match function types only when their arity and rest layout correspond.
///
/// A callback that declares FEWER parameters than the slot also corresponds
/// ([`callback_accepts_fewer_params`]): JavaScript calls it with the slot's
/// arguments and it ignores the trailing ones, so its leading parameters and
/// its return are evidence for the slot's (`acc => acc + 'x'` for
/// `(value: T, i: number) => T` pins `T = string`).
///
/// `required_params` (the JavaScript `Function.length` a closure reports) is
/// not compared: it is runtime metadata, not part of either Rust signature,
/// so `(date: Date, value: V) => boolean` and a method value of the same
/// parameter types still bind `V` positionally.
fn match_function_types(
    mir: &Mir,
    declared: &FunctionType,
    actual: &FunctionType,
    scope: &MatchScope<'_>,
    bindings: &mut CalleeTypeParamBindings,
) {
    if callback_accepts_fewer_params(declared, actual) {
        let used = declared.params.get(..actual.params.len()).unwrap_or_default();
        match_type_slices(mir, used, &actual.params, scope, bindings);
        match_types_in(mir, declared.return_ty, actual.return_ty, scope, bindings);
        return;
    }
    if declared.params.len() != actual.params.len()
        || declared.rest != actual.rest
        || declared.mutable_params != actual.mutable_params
        || declared.is_async != actual.is_async
        || declared.may_throw != actual.may_throw
    {
        for param in &declared.params {
            bindings.observe_occurrences(
                mir,
                *param,
                TypeParamBinding::Unsupported(BindingUnsupportedReason::FunctionShape),
            );
        }
        bindings.observe_occurrences(
            mir,
            declared.return_ty,
            TypeParamBinding::Unsupported(BindingUnsupportedReason::FunctionShape),
        );
        return;
    }
    match_type_slices(mir, &declared.params, &actual.params, scope, bindings);
    match_types_in(mir, declared.return_ty, actual.return_ty, scope, bindings);
}

/// Calls `visit` on each type parameter occurring anywhere inside `ty`, stopping
/// at the first one it answers `true` for.
fn visit_type_params(mir: &Mir, ty: TypeId, visit: &mut dyn FnMut(Symbol) -> bool) -> bool {
    match mir.types.get(ty) {
        Some(Type::TypeParam { name }) => visit(*name),
        Some(other) => other.children().any(|child| visit_type_params(mir, child, visit)),
        None => false,
    }
}

/// Return whether `name` occurs anywhere inside `ty`.
///
/// Answers without building a set: the crate-wide generic-safety scan asks this
/// once per (function, type parameter, parameter type).
pub(crate) fn type_param_occurs(mir: &Mir, ty: TypeId, name: Symbol) -> bool {
    visit_type_params(mir, ty, &mut |param| param == name)
}

/// Collect declared type-parameter names occurring anywhere inside a type.
pub(crate) fn collect_type_param_occurrences(mir: &Mir, ty: TypeId, names: &mut HashSet<Symbol>) {
    visit_type_params(mir, ty, &mut |param| {
        names.insert(param);
        false
    });
}

#[cfg(test)]
mod tests {
    use smelt_hir::{FileId, OriginalNameTable, Span, SymbolInterner, TypeInterner, TypeParamDef};
    use smelt_mir::{BlockId, FuncId, HirOrigin, LocalDecl, LocalId, LocalKind};

    use super::*;

    /// A zero-width span for synthetic test MIR.
    fn span() -> Span {
        Span::new(FileId(0), 0, 0)
    }

    /// Build a function declaring `type_params` and one local per parameter type.
    fn function_with_params(
        id: u32,
        type_params: &[Symbol],
        param_types: &[TypeId],
        return_ty: TypeId,
    ) -> MirFunction {
        MirFunction {
            id: FuncId(id),
            name: Symbol(id),
            type_params: type_params
                .iter()
                .map(|name| TypeParamDef {
                    name: *name,
                    constraint: None,
                    default: None,
                    span: span(),
                })
                .collect(),
            origin: HirOrigin::Body(smelt_hir::BodyId(id)),
            absent: smelt_mir::AbsentSpelling::Undefined,
            is_async: false,
            is_generator: false,
            is_test: false,
            can_throw: false,
            params: (0..param_types.len())
                .map(|index| LocalId(u32::try_from(index).expect("test parameter count")))
                .collect(),
            rest: None,
            return_ty,
            locals: param_types
                .iter()
                .map(|ty| LocalDecl {
                    ty: *ty,
                    kind: LocalKind::Param { symbol: None },
                    span: span(),
                })
                .collect(),
            blocks: Vec::new(),
            entry: BlockId(0),
        }
    }

    /// Read a local as a by-copy argument operand.
    fn local_arg(index: u32) -> Operand {
        Operand::Copy(Place::Local(LocalId(index)))
    }

    /// Construct a MIR shell whose interners are sufficient for type matching.
    fn mir_with_types(types: TypeInterner) -> Mir {
        Mir::new(
            types,
            SymbolInterner::default(),
            OriginalNameTable::default(),
        )
    }

    /// Construct ordered bindings for one type parameter.
    fn one_binding(name: Symbol) -> CalleeTypeParamBindings {
        CalleeTypeParamBindings {
            bindings: IndexMap::from([(name, TypeParamBinding::Unbound)]),
        }
    }

    #[test]
    fn concrete_direct_evidence_survives_erased_callback_evidence() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let unknown = types.intern(Type::Unknown);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let mir = mir_with_types(types);
        let own = HashSet::from([Symbol(0)]);
        let mut bindings = one_binding(Symbol(0));

        match_types(&mir, type_param, float, &own, &mut bindings);
        match_types(&mir, type_param, unknown, &own, &mut bindings);

        assert_eq!(
            bindings.get(Symbol(0)),
            Some(TypeParamBinding::Concrete(float))
        );
        assert!(bindings.all_concrete());
    }

    #[test]
    fn conflicting_composite_arguments_are_distinct_from_erasure() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let string = types.intern(Type::String);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let declared = types.intern(Type::List(type_param));
        let actual_float = types.intern(Type::List(float));
        let actual_string = types.intern(Type::List(string));
        let mir = mir_with_types(types);
        let own = HashSet::from([Symbol(0)]);
        let mut bindings = one_binding(Symbol(0));

        match_types(&mir, declared, actual_float, &own, &mut bindings);
        match_types(&mir, declared, actual_string, &own, &mut bindings);

        assert_eq!(
            bindings.get(Symbol(0)),
            Some(TypeParamBinding::Conflict {
                first: float,
                second: string,
            })
        );
        assert!(!bindings.all_concrete());
    }

    #[test]
    fn callback_return_can_bind_a_type_parameter() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let declared = types.intern(Type::Function(FunctionType {
            params: Vec::new(),
            rest: None,
            required_params: Some(0),
            mutable_params: Vec::new(),
            return_ty: type_param,
            is_async: false,
            may_throw: false,
        }));
        let actual = types.intern(Type::Function(FunctionType {
            params: Vec::new(),
            rest: None,
            required_params: Some(0),
            mutable_params: Vec::new(),
            return_ty: float,
            is_async: false,
            may_throw: false,
        }));
        let mir = mir_with_types(types);
        let own = HashSet::from([Symbol(0)]);
        let mut bindings = one_binding(Symbol(0));

        match_types(&mir, declared, actual, &own, &mut bindings);

        assert_eq!(
            bindings.get(Symbol(0)),
            Some(TypeParamBinding::Concrete(float))
        );
    }

    #[test]
    fn dictionary_binds_key_and_value_parameters() {
        let mut types = TypeInterner::default();
        let string = types.intern(Type::String);
        let float = types.intern(Type::Float);
        let key_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let value_param = types.intern(Type::TypeParam { name: Symbol(1) });
        let declared = types.intern(Type::Dict(key_param, value_param));
        let actual = types.intern(Type::Dict(string, float));
        let mir = mir_with_types(types);
        let own = HashSet::from([Symbol(0), Symbol(1)]);
        let mut bindings = CalleeTypeParamBindings {
            bindings: IndexMap::from([
                (Symbol(0), TypeParamBinding::Unbound),
                (Symbol(1), TypeParamBinding::Unbound),
            ]),
        };

        match_types(&mir, declared, actual, &own, &mut bindings);

        assert_eq!(
            bindings.get(Symbol(0)),
            Some(TypeParamBinding::Concrete(string))
        );
        assert_eq!(
            bindings.get(Symbol(1)),
            Some(TypeParamBinding::Concrete(float))
        );
    }

    #[test]
    fn resolves_literal_types_without_emitter_state() {
        let mut types = TypeInterner::default();
        let bool_ty = types.intern(Type::Bool);
        let int_ty = types.intern(Type::Int);
        let float_ty = types.intern(Type::Float);
        let string_ty = types.intern(Type::String);
        let none_ty = types.intern(Type::None);
        let unknown_ty = types.intern(Type::Unknown);
        let mir = mir_with_types(types);

        assert_eq!(literal_type(&mir, &Constant::Bool(true)), Some(bool_ty));
        assert_eq!(literal_type(&mir, &Constant::Int(1)), Some(int_ty));
        assert_eq!(literal_type(&mir, &Constant::Float(1.0)), Some(float_ty));
        assert_eq!(
            literal_type(&mir, &Constant::String("value".to_owned())),
            Some(string_ty)
        );
        assert_eq!(literal_type(&mir, &Constant::None), Some(none_ty));
        assert_eq!(literal_type(&mir, &Constant::Undefined), Some(none_ty));
        assert_eq!(
            literal_type(&mir, &Constant::Symbol("key".to_owned())),
            Some(unknown_ty)
        );
    }

    #[test]
    fn union_evidence_is_unsupported_instead_of_positionally_zipped() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let string = types.intern(Type::String);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let declared = types.intern(Type::Union(vec![type_param, string]));
        let actual = types.intern(Type::Union(vec![float, string]));
        let mir = mir_with_types(types);
        let own = HashSet::from([Symbol(0)]);
        let mut bindings = one_binding(Symbol(0));

        match_types(&mir, declared, actual, &own, &mut bindings);

        assert_eq!(
            bindings.get(Symbol(0)),
            Some(TypeParamBinding::Unsupported(
                BindingUnsupportedReason::Union
            ))
        );
    }

    #[test]
    fn function_arity_mismatch_is_unsupported() {
        // Requirement history: this used to pin that ANY arity mismatch is
        // `Unsupported`. A callback declaring FEWER parameters than its slot
        // is valid TypeScript (JavaScript drops the surplus arguments), so it
        // now contributes evidence for the positions it declares and leaves
        // the rest `Unbound` (see `shorter_callback_binds_declared_positions`),
        // which still demotes the call site unless another argument pins the
        // parameter. A callback declaring MORE parameters than its slot stays
        // unsupported, which is what this test pins.
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let declared = types.intern(Type::Function(FunctionType {
            params: vec![type_param],
            rest: None,
            required_params: Some(1),
            mutable_params: Vec::new(),
            return_ty: float,
            is_async: false,
            may_throw: false,
        }));
        let actual = types.intern(Type::Function(FunctionType {
            params: vec![float, float],
            rest: None,
            required_params: Some(2),
            mutable_params: Vec::new(),
            return_ty: float,
            is_async: false,
            may_throw: false,
        }));
        let mir = mir_with_types(types);
        let own = HashSet::from([Symbol(0)]);
        let mut bindings = one_binding(Symbol(0));

        match_types(&mir, declared, actual, &own, &mut bindings);

        assert_eq!(
            bindings.get(Symbol(0)),
            Some(TypeParamBinding::Unsupported(
                BindingUnsupportedReason::FunctionShape
            ))
        );
    }

    #[test]
    fn shorter_callback_binds_declared_positions() {
        // `acc => acc + 'x'` against `(value: T, index: number) => T`.
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let string = types.intern(Type::String);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let declared = types.intern(Type::Function(FunctionType {
            params: vec![type_param, float],
            rest: None,
            required_params: Some(2),
            mutable_params: Vec::new(),
            return_ty: type_param,
            is_async: false,
            may_throw: false,
        }));
        let actual = types.intern(Type::Function(FunctionType {
            params: vec![string],
            rest: None,
            required_params: Some(1),
            mutable_params: Vec::new(),
            return_ty: string,
            is_async: false,
            may_throw: false,
        }));
        let mir = mir_with_types(types);
        let own = HashSet::from([Symbol(0)]);
        let mut bindings = one_binding(Symbol(0));

        match_types(&mir, declared, actual, &own, &mut bindings);

        assert_eq!(bindings.get(Symbol(0)), Some(TypeParamBinding::Concrete(string)));
        assert!(substitution_matches(&mir, declared, actual, &bindings));
    }

    #[test]
    fn erased_only_evidence_is_distinct_from_conflict_and_unsupported() {
        let mut types = TypeInterner::default();
        let unknown = types.intern(Type::Unknown);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let declared = types.intern(Type::List(type_param));
        let actual = types.intern(Type::List(unknown));
        let mir = mir_with_types(types);
        let own = HashSet::from([Symbol(0)]);
        let mut bindings = one_binding(Symbol(0));

        match_types(&mir, declared, actual, &own, &mut bindings);

        assert_eq!(bindings.get(Symbol(0)), Some(TypeParamBinding::Erased));
        assert!(!bindings.all_concrete());
    }

    #[test]
    fn shape_mismatch_is_reported_separately_from_erasure() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let declared = types.intern(Type::List(type_param));
        let actual = types.intern(Type::Set(float));
        let mir = mir_with_types(types);
        let own = HashSet::from([Symbol(0)]);
        let mut bindings = one_binding(Symbol(0));

        match_types(&mir, declared, actual, &own, &mut bindings);

        assert_eq!(
            bindings.get(Symbol(0)),
            Some(TypeParamBinding::Unsupported(
                BindingUnsupportedReason::ShapeMismatch
            ))
        );
    }

    #[test]
    fn erased_type_classification_matches_the_safety_scan_truth_table() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let string = types.intern(Type::String);
        let unknown = types.intern(Type::Unknown);
        let never = types.intern(Type::Never);
        let union = types.intern(Type::Union(vec![float, string]));
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let past_end = TypeId(u32::try_from(types.all().len()).expect("test type count"));
        let mir = mir_with_types(types);

        assert!(actual_type_is_erased(&mir, unknown));
        assert!(actual_type_is_erased(&mir, never));
        assert!(actual_type_is_erased(&mir, union));
        assert!(actual_type_is_erased(&mir, type_param));
        assert!(!actual_type_is_erased(&mir, float));
        assert!(!actual_type_is_erased(&mir, string));
        // Fails closed; unreachable for an interned `LocalDecl::ty`.
        assert!(actual_type_is_erased(&mir, past_end));
    }

    #[test]
    fn literal_argument_binds_a_bare_type_parameter() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let mut mir = mir_with_types(types);
        let callee = function_with_params(0, &[Symbol(0)], &[type_param], type_param);
        let call_site = function_with_params(1, &[], &[], float);
        mir.functions.push(callee);

        let bindings = collect_bindings(
            &mir,
            &mir.functions[0],
            &call_site,
            &[Operand::Const(Constant::Float(1.0))],
        );

        assert_eq!(
            bindings.get(Symbol(0)),
            Some(TypeParamBinding::Concrete(float))
        );
        assert!(bindings.all_concrete());
    }

    #[test]
    fn omitted_arguments_contribute_no_evidence() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let callback = types.intern(Type::Function(FunctionType {
            params: vec![type_param],
            rest: None,
            required_params: Some(1),
            mutable_params: Vec::new(),
            return_ty: float,
            is_async: false,
            may_throw: false,
        }));
        let mut mir = mir_with_types(types);
        let callee = function_with_params(0, &[Symbol(0)], &[callback], float);
        let call_site = function_with_params(1, &[], &[], float);
        mir.functions.push(callee);

        let bindings = collect_bindings(&mir, &mir.functions[0], &call_site, &[]);

        assert_eq!(bindings.get(Symbol(0)), Some(TypeParamBinding::Unbound));
        assert!(!bindings.all_concrete());
    }

    #[test]
    fn concrete_direct_argument_outweighs_an_erased_callback_argument() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let unknown = types.intern(Type::Unknown);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let declared_list = types.intern(Type::List(type_param));
        let actual_list = types.intern(Type::List(float));
        let declared_callback = types.intern(Type::Function(FunctionType {
            params: vec![type_param],
            rest: None,
            required_params: Some(1),
            mutable_params: Vec::new(),
            return_ty: float,
            is_async: false,
            may_throw: false,
        }));
        let mut mir = mir_with_types(types);
        let callee = function_with_params(
            0,
            &[Symbol(0)],
            &[declared_list, declared_callback],
            type_param,
        );
        let call_site = function_with_params(1, &[], &[actual_list, unknown], float);
        mir.functions.push(callee);

        let bindings = collect_bindings(
            &mir,
            &mir.functions[0],
            &call_site,
            &[local_arg(0), local_arg(1)],
        );

        assert_eq!(
            bindings.get(Symbol(0)),
            Some(TypeParamBinding::Concrete(float))
        );
    }

    #[test]
    fn a_rest_parameter_fails_the_whole_call_site_closed() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let declared_list = types.intern(Type::List(type_param));
        let mut mir = mir_with_types(types);
        let mut callee =
            function_with_params(0, &[Symbol(0)], &[type_param, declared_list], type_param);
        callee.rest = Some(1);
        let call_site = function_with_params(1, &[], &[float, float], float);
        mir.functions.push(callee);

        let bindings = collect_bindings(
            &mir,
            &mir.functions[0],
            &call_site,
            &[local_arg(0), local_arg(1)],
        );

        assert_eq!(
            bindings.get(Symbol(0)),
            Some(TypeParamBinding::Unsupported(
                BindingUnsupportedReason::FunctionShape
            ))
        );
        assert!(!bindings.all_concrete());
    }

    #[test]
    fn a_projected_argument_is_unsupported_rather_than_guessed() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let mut mir = mir_with_types(types);
        let callee = function_with_params(0, &[Symbol(0)], &[type_param], type_param);
        let call_site = function_with_params(1, &[], &[float], float);
        mir.functions.push(callee);

        let bindings = collect_bindings(
            &mir,
            &mir.functions[0],
            &call_site,
            &[Operand::Copy(Place::Field {
                base: LocalId(0),
                field: Symbol(7),
            })],
        );

        assert_eq!(
            bindings.get(Symbol(0)),
            Some(TypeParamBinding::Unsupported(
                BindingUnsupportedReason::ProjectedOperand
            ))
        );
    }
    /// Bind one type parameter to a concrete type for substitution tests.
    fn concrete_binding(name: Symbol, ty: TypeId) -> CalleeTypeParamBindings {
        CalleeTypeParamBindings {
            bindings: IndexMap::from([(name, TypeParamBinding::Concrete(ty))]),
        }
    }

    #[test]
    /// Every supported constructor substitutes recursively, at any depth.
    fn substitution_walks_every_supported_constructor() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let string = types.intern(Type::String);
        let param = types.intern(Type::TypeParam { name: Symbol(0) });
        // Declared patterns and their expected substitutions, interned in pairs.
        let list_param = types.intern(Type::List(param));
        let list_float = types.intern(Type::List(float));
        let nested_param = types.intern(Type::List(list_param));
        let nested_float = types.intern(Type::List(list_float));
        let set_param = types.intern(Type::Set(param));
        let set_float = types.intern(Type::Set(float));
        let optional_param = types.intern(Type::Optional(param));
        let optional_float = types.intern(Type::Optional(float));
        let future_param = types.intern(Type::Future(param));
        let future_float = types.intern(Type::Future(float));
        let dict_param = types.intern(Type::Dict(string, param));
        let dict_float = types.intern(Type::Dict(string, float));
        let map_param = types.intern(Type::JsMap(string, param));
        let map_float = types.intern(Type::JsMap(string, float));
        let tuple_param = types.intern(Type::Tuple(vec![param, string]));
        let tuple_float = types.intern(Type::Tuple(vec![float, string]));
        let class_param = types.intern(Type::Class {
            name: Symbol(9),
            args: vec![param],
        });
        let class_float = types.intern(Type::Class {
            name: Symbol(9),
            args: vec![float],
        });
        let generator_param = types.intern(Type::Generator {
            is_async: false,
            yield_ty: param,
            return_ty: string,
            next_ty: string,
        });
        let generator_float = types.intern(Type::Generator {
            is_async: false,
            yield_ty: float,
            return_ty: string,
            next_ty: string,
        });
        let result_param = types.intern(Type::GeneratorResult {
            yield_ty: param,
            return_ty: string,
        });
        let result_float = types.intern(Type::GeneratorResult {
            yield_ty: float,
            return_ty: string,
        });
        let function_param = types.intern(Type::Function(FunctionType {
            params: vec![param],
            rest: None,
            required_params: Some(1),
            mutable_params: Vec::new(),
            return_ty: param,
            is_async: false,
            may_throw: false,
        }));
        let function_float = types.intern(Type::Function(FunctionType {
            params: vec![float],
            rest: None,
            required_params: Some(1),
            mutable_params: Vec::new(),
            return_ty: float,
            is_async: false,
            may_throw: false,
        }));
        let mir = mir_with_types(types);
        let bindings = concrete_binding(Symbol(0), float);

        for (declared, expected) in [
            (param, float),
            (list_param, list_float),
            (nested_param, nested_float),
            (set_param, set_float),
            (optional_param, optional_float),
            (future_param, future_float),
            (dict_param, dict_float),
            (map_param, map_float),
            (tuple_param, tuple_float),
            (class_param, class_float),
            (generator_param, generator_float),
            (result_param, result_float),
            (function_param, function_float),
            // A pattern mentioning no type parameter substitutes to itself.
            (string, string),
        ] {
            assert_eq!(
                substituted_type_id(&mir, declared, &bindings),
                Some(expected)
            );
            assert!(substitution_matches(&mir, declared, expected, &bindings));
        }
    }

    #[test]
    /// A mismatched constructor, arity or class name fails closed at any depth.
    fn substitution_rejects_mismatched_shapes() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let string = types.intern(Type::String);
        let param = types.intern(Type::TypeParam { name: Symbol(0) });
        let list_param = types.intern(Type::List(param));
        let set_float = types.intern(Type::Set(float));
        let list_string = types.intern(Type::List(string));
        let class_param = types.intern(Type::Class {
            name: Symbol(9),
            args: vec![param],
        });
        let other_class_float = types.intern(Type::Class {
            name: Symbol(10),
            args: vec![float],
        });
        let tuple_param = types.intern(Type::Tuple(vec![param]));
        let tuple_pair = types.intern(Type::Tuple(vec![float, float]));
        let mir = mir_with_types(types);
        let bindings = concrete_binding(Symbol(0), float);

        // Different constructor, different element, different class, different
        // arity: none of these is `List<T>` with `T = f64`.
        assert!(!substitution_matches(&mir, list_param, set_float, &bindings));
        assert!(!substitution_matches(
            &mir,
            list_param,
            list_string,
            &bindings
        ));
        assert!(!substitution_matches(
            &mir,
            class_param,
            other_class_float,
            &bindings
        ));
        assert!(!substitution_matches(
            &mir,
            tuple_param,
            tuple_pair,
            &bindings
        ));
    }

    #[test]
    /// Substitution is stricter than the evidence matcher in a type-parameter-free
    /// subtree, which is the whole reason it is a separate walk.
    ///
    /// `match_types` deliberately tolerates a mismatch that lands where no type
    /// parameter occurs (a `Dict` key), because such a mismatch says nothing
    /// about `V`. Accepting it at a call site would render a `Dict<Int, Float>`
    /// argument against a `Dict<String, T>` parameter: E0308.
    fn substitution_is_stricter_than_the_evidence_matcher() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let int = types.intern(Type::Int);
        let string = types.intern(Type::String);
        let param = types.intern(Type::TypeParam { name: Symbol(0) });
        let declared = types.intern(Type::Dict(string, param));
        let actual = types.intern(Type::Dict(int, float));
        let mir = mir_with_types(types);

        let own = HashSet::from([Symbol(0)]);
        let mut evidence = one_binding(Symbol(0));
        match_types(&mir, declared, actual, &own, &mut evidence);
        assert_eq!(
            evidence.get(Symbol(0)),
            Some(TypeParamBinding::Concrete(float)),
            "the evidence matcher still binds V through a disagreeing key"
        );

        assert!(
            !substitution_matches(&mir, declared, actual, &evidence),
            "substitution must reject the disagreeing key"
        );
    }

    #[test]
    /// Unions fail closed on either side, at any depth.
    fn substitution_refuses_unions() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let string = types.intern(Type::String);
        let param = types.intern(Type::TypeParam { name: Symbol(0) });
        let declared_union = types.intern(Type::Union(vec![param, string]));
        let actual_union = types.intern(Type::Union(vec![float, string]));
        let declared_nested = types.intern(Type::List(declared_union));
        let actual_nested = types.intern(Type::List(actual_union));
        let mir = mir_with_types(types);
        let bindings = concrete_binding(Symbol(0), float);

        assert_eq!(substituted_type_id(&mir, declared_union, &bindings), None);
        assert_eq!(substituted_type_id(&mir, declared_nested, &bindings), None);
        assert!(!substitution_matches(
            &mir,
            declared_nested,
            actual_nested,
            &bindings
        ));
    }

    #[test]
    /// Every non-concrete binding state, and an undeclared parameter, fail closed.
    fn substitution_fails_closed_on_weak_bindings() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let param = types.intern(Type::TypeParam { name: Symbol(0) });
        let list_param = types.intern(Type::List(param));
        let list_float = types.intern(Type::List(float));
        let mir = mir_with_types(types);

        for state in [
            TypeParamBinding::Unbound,
            TypeParamBinding::Erased,
            TypeParamBinding::Conflict {
                first: float,
                second: param,
            },
            TypeParamBinding::Unsupported(BindingUnsupportedReason::Union),
        ] {
            let bindings = CalleeTypeParamBindings {
                bindings: IndexMap::from([(Symbol(0), state)]),
            };
            assert_eq!(substituted_type_id(&mir, list_param, &bindings), None);
            assert!(!substitution_matches(
                &mir,
                list_param,
                list_float,
                &bindings
            ));
        }

        // A type parameter belonging to some unrelated scope has no
        // instantiation here at all.
        let foreign = concrete_binding(Symbol(1), float);
        assert_eq!(substituted_type_id(&mir, list_param, &foreign), None);
    }

    #[test]
    /// A substituted type absent from the frozen interner fails closed.
    fn substitution_requires_an_interned_result() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let param = types.intern(Type::TypeParam { name: Symbol(0) });
        // `List<T>` is interned; `List<Float>` deliberately is not.
        let list_param = types.intern(Type::List(param));
        let mir = mir_with_types(types);
        let bindings = concrete_binding(Symbol(0), float);

        assert_eq!(substituted_type_id(&mir, list_param, &bindings), None);
    }

    #[test]
    /// A concrete receiver pins the class type parameters; anything else does not.
    fn class_type_params_bind_from_the_receiver() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let unknown = types.intern(Type::Unknown);
        let receiver = types.intern(Type::Class {
            name: Symbol(9),
            args: vec![float],
        });
        let erased_receiver = types.intern(Type::Class {
            name: Symbol(9),
            args: vec![unknown],
        });
        let wrong_arity = types.intern(Type::Class {
            name: Symbol(9),
            args: Vec::new(),
        });
        let mir = mir_with_types(types);

        let bound = bind_class_type_params(&mir, &[Symbol(0)], receiver);
        assert_eq!(bound.get(Symbol(0)), Some(TypeParamBinding::Concrete(float)));
        assert!(bound.all_concrete());

        assert_eq!(
            bind_class_type_params(&mir, &[Symbol(0)], erased_receiver).get(Symbol(0)),
            Some(TypeParamBinding::Erased)
        );
        assert!(!bind_class_type_params(&mir, &[Symbol(0)], wrong_arity).all_concrete());
        assert!(!bind_class_type_params(&mir, &[Symbol(0)], float).all_concrete());
    }

    #[test]
    /// The positional core agrees with the operand-resolving wrapper, and an
    /// absent actual type contributes no evidence.
    fn positional_binding_core_matches_the_operand_wrapper() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let param = types.intern(Type::TypeParam { name: Symbol(0) });
        let list_param = types.intern(Type::List(param));
        let list_float = types.intern(Type::List(float));
        let mir = mir_with_types(types);

        let callee = function_with_params(0, &[Symbol(0)], &[list_param], list_param);
        let call_site = function_with_params(1, &[], &[list_float], list_float);
        let through_operands = collect_bindings(&mir, &callee, &call_site, &[local_arg(0)]);
        let through_types =
            collect_bindings_from_types(&mir, &[Symbol(0)], &[list_param], &[Some(list_float)]);
        assert_eq!(through_operands, through_types);
        assert_eq!(
            through_types.get(Symbol(0)),
            Some(TypeParamBinding::Concrete(float))
        );

        // No supplied argument at that position: no evidence, so `Unbound`.
        let omitted = collect_bindings_from_types(&mir, &[Symbol(0)], &[list_param], &[None]);
        assert_eq!(omitted.get(Symbol(0)), Some(TypeParamBinding::Unbound));
        assert!(!omitted.all_concrete());
    }

}
