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

use std::collections::HashSet;

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
pub(crate) fn collect_bindings(
    mir: &Mir,
    target_function: &MirFunction,
    source_function: &MirFunction,
    args: &[Operand],
) -> CalleeTypeParamBindings {
    let mut bindings = CalleeTypeParamBindings {
        bindings: target_function
            .type_params
            .iter()
            .map(|param| (param.name, TypeParamBinding::Unbound))
            .collect(),
    };
    let own_params = target_function
        .type_params
        .iter()
        .map(|param| param.name)
        .collect::<HashSet<_>>();

    // Positional alignment cannot represent a packed rest parameter: the
    // emitter collects the trailing arguments into one list before the call, so
    // `params[i]` and `args[i]` stop corresponding at the rest position. Fail
    // closed for the whole call site rather than matching a list pattern against
    // a single element type.
    if target_function.rest.is_some() {
        for (_, binding) in &mut bindings.bindings {
            *binding = binding.merge(TypeParamBinding::Unsupported(
                BindingUnsupportedReason::FunctionShape,
            ));
        }
        return bindings;
    }

    for (index, param) in target_function.params.iter().enumerate() {
        let Some(declared) = target_function
            .locals
            .get(usize::try_from(param.0).unwrap_or(usize::MAX))
            .map(|local| local.ty)
        else {
            continue;
        };
        let Some(argument) = args.get(index) else {
            continue;
        };
        let actual = match operand_type(mir, source_function, argument) {
            Ok(actual) => actual,
            Err(reason) => {
                bindings.observe_occurrences(mir, declared, TypeParamBinding::Unsupported(reason));
                continue;
            }
        };
        match_types(mir, declared, actual, &own_params, &mut bindings);
    }

    bindings
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
        Operand::Copy(Place::Field { .. } | Place::Index { .. })
        | Operand::Move(Place::Field { .. } | Place::Index { .. }) => {
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

/// Directionally match a declared callee type against one actual caller type.
fn match_types(
    mir: &Mir,
    declared: TypeId,
    actual: TypeId,
    own_params: &HashSet<Symbol>,
    bindings: &mut CalleeTypeParamBindings,
) {
    let Some(declared_ty) = mir.types.get(declared) else {
        return;
    };
    if let Type::TypeParam { name } = declared_ty
        && own_params.contains(name)
    {
        let observation = if actual_type_is_erased(mir, actual) {
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
            match_types(mir, *left, *right, own_params, bindings);
        }
        (Type::Dict(left_key, left_value), Type::Dict(right_key, right_value))
        | (Type::JsMap(left_key, left_value), Type::JsMap(right_key, right_value)) => {
            match_types(mir, *left_key, *right_key, own_params, bindings);
            match_types(mir, *left_value, *right_value, own_params, bindings);
        }
        (Type::Tuple(left), Type::Tuple(right)) if left.len() == right.len() => {
            match_type_slices(mir, left, right, own_params, bindings);
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
            match_type_slices(mir, left, right, own_params, bindings);
        }
        (Type::Function(left), Type::Function(right)) => {
            match_function_types(mir, left, right, own_params, bindings);
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
            match_types(mir, *left_yield, *right_yield, own_params, bindings);
            match_types(mir, *left_return, *right_return, own_params, bindings);
            match_types(mir, *left_next, *right_next, own_params, bindings);
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
            match_types(mir, *left_yield, *right_yield, own_params, bindings);
            match_types(mir, *left_return, *right_return, own_params, bindings);
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
    own_params: &HashSet<Symbol>,
    bindings: &mut CalleeTypeParamBindings,
) {
    for (declared_item, actual_item) in declared.iter().zip(actual) {
        match_types(mir, *declared_item, *actual_item, own_params, bindings);
    }
}

/// Match function types only when their arity and rest layout correspond.
fn match_function_types(
    mir: &Mir,
    declared: &FunctionType,
    actual: &FunctionType,
    own_params: &HashSet<Symbol>,
    bindings: &mut CalleeTypeParamBindings,
) {
    if declared.params.len() != actual.params.len()
        || declared.rest != actual.rest
        || declared.required_params != actual.required_params
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
    match_type_slices(mir, &declared.params, &actual.params, own_params, bindings);
    match_types(
        mir,
        declared.return_ty,
        actual.return_ty,
        own_params,
        bindings,
    );
}

/// Return whether `name` occurs anywhere inside `ty`.
///
/// Descends through every shape, function types and unions included, and stops
/// at the first hit. This is the short-circuiting sibling of
/// [`collect_type_param_occurrences`] over exactly the same grammar; the two are
/// pinned to agree by a unit test. Kept separate because the crate-wide
/// generic-safety scan asks this question once per (function, type parameter,
/// parameter type) and must not allocate a set to answer it.
pub(crate) fn type_param_occurs(mir: &Mir, ty: TypeId, name: Symbol) -> bool {
    match mir.types.get(ty) {
        Some(Type::TypeParam { name: param_name }) => *param_name == name,
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
            type_param_occurs(mir, *item, name)
        }
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            type_param_occurs(mir, *key, name) || type_param_occurs(mir, *value, name)
        }
        Some(Type::Tuple(items) | Type::Union(items)) => {
            items.iter().any(|item| type_param_occurs(mir, *item, name))
        }
        Some(Type::Class { args, .. }) => args.iter().any(|arg| type_param_occurs(mir, *arg, name)),
        Some(Type::Function(function)) => {
            function
                .params
                .iter()
                .any(|param| type_param_occurs(mir, *param, name))
                || type_param_occurs(mir, function.return_ty, name)
        }
        Some(Type::Generator {
            yield_ty,
            return_ty,
            next_ty,
            ..
        }) => {
            type_param_occurs(mir, *yield_ty, name)
                || type_param_occurs(mir, *return_ty, name)
                || type_param_occurs(mir, *next_ty, name)
        }
        Some(Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) => type_param_occurs(mir, *yield_ty, name) || type_param_occurs(mir, *return_ty, name),
        Some(
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::Unknown
            | Type::Never
            | Type::None,
        )
        | None => false,
    }
}

/// Collect declared type-parameter names occurring anywhere inside a type.
fn collect_type_param_occurrences(mir: &Mir, ty: TypeId, names: &mut HashSet<Symbol>) {
    match mir.types.get(ty) {
        Some(Type::TypeParam { name }) => {
            names.insert(*name);
        }
        Some(Type::List(item) | Type::Set(item) | Type::Optional(item) | Type::Future(item)) => {
            collect_type_param_occurrences(mir, *item, names);
        }
        Some(Type::Dict(key, value) | Type::JsMap(key, value)) => {
            collect_type_param_occurrences(mir, *key, names);
            collect_type_param_occurrences(mir, *value, names);
        }
        Some(Type::Tuple(items) | Type::Union(items)) => {
            for item in items {
                collect_type_param_occurrences(mir, *item, names);
            }
        }
        Some(Type::Class { args, .. }) => {
            for arg in args {
                collect_type_param_occurrences(mir, *arg, names);
            }
        }
        Some(Type::Function(function)) => {
            for param in &function.params {
                collect_type_param_occurrences(mir, *param, names);
            }
            collect_type_param_occurrences(mir, function.return_ty, names);
        }
        Some(Type::Generator {
            yield_ty,
            return_ty,
            next_ty,
            ..
        }) => {
            collect_type_param_occurrences(mir, *yield_ty, names);
            collect_type_param_occurrences(mir, *return_ty, names);
            collect_type_param_occurrences(mir, *next_ty, names);
        }
        Some(Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) => {
            collect_type_param_occurrences(mir, *yield_ty, names);
            collect_type_param_occurrences(mir, *return_ty, names);
        }
        Some(
            Type::Bool
            | Type::Int
            | Type::Float
            | Type::String
            | Type::Unknown
            | Type::Never
            | Type::None,
        )
        | None => {}
    }
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
            Some(TypeParamBinding::Unsupported(
                BindingUnsupportedReason::FunctionShape
            ))
        );
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
    fn occurrence_walks_agree_arm_for_arm() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let outer = types.intern(Type::TypeParam { name: Symbol(0) });
        let inner = types.intern(Type::TypeParam { name: Symbol(1) });
        let absent = Symbol(2);
        let callback = types.intern(Type::Function(FunctionType {
            params: vec![inner],
            rest: None,
            required_params: Some(1),
            mutable_params: Vec::new(),
            return_ty: float,
            is_async: false,
            may_throw: false,
        }));
        let union = types.intern(Type::Union(vec![outer, float]));
        let candidates = [
            float,
            outer,
            inner,
            callback,
            union,
            types.intern(Type::List(callback)),
            types.intern(Type::Dict(outer, callback)),
            types.intern(Type::Tuple(vec![union, inner])),
            types.intern(Type::Class {
                name: Symbol(9),
                args: vec![outer],
            }),
            types.intern(Type::Generator {
                is_async: false,
                yield_ty: inner,
                return_ty: float,
                next_ty: outer,
            }),
            types.intern(Type::GeneratorResult {
                yield_ty: union,
                return_ty: callback,
            }),
        ];
        let past_end = TypeId(u32::try_from(types.all().len()).expect("test type count"));
        let mir = mir_with_types(types);

        for ty in candidates.into_iter().chain([past_end]) {
            let mut names = HashSet::new();
            collect_type_param_occurrences(&mir, ty, &mut names);
            for name in [Symbol(0), Symbol(1), absent] {
                assert_eq!(
                    type_param_occurs(&mir, ty, name),
                    names.contains(&name),
                    "walks disagree for {ty:?} and {name:?}"
                );
            }
        }
    }

    #[test]
    fn literal_argument_binds_a_bare_type_parameter() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let type_param = types.intern(Type::TypeParam { name: Symbol(0) });
        let mut mir = mir_with_types(types);
        let callee = function_with_params(0, &[Symbol(0)], &[type_param], type_param);
        let caller = function_with_params(1, &[], &[], float);
        mir.functions.push(callee);

        let bindings = collect_bindings(
            &mir,
            &mir.functions[0],
            &caller,
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
        let caller = function_with_params(1, &[], &[], float);
        mir.functions.push(callee);

        let bindings = collect_bindings(&mir, &mir.functions[0], &caller, &[]);

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
        let caller = function_with_params(1, &[], &[actual_list, unknown], float);
        mir.functions.push(callee);

        let bindings = collect_bindings(
            &mir,
            &mir.functions[0],
            &caller,
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
        let caller = function_with_params(1, &[], &[float, float], float);
        mir.functions.push(callee);

        let bindings = collect_bindings(
            &mir,
            &mir.functions[0],
            &caller,
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
        let caller = function_with_params(1, &[], &[float], float);
        mir.functions.push(callee);

        let bindings = collect_bindings(
            &mir,
            &mir.functions[0],
            &caller,
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
}
