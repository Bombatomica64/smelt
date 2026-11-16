//! Shared HIR type normalization helpers.
//!
//! Frontends can preserve source-language spelling while these helpers build
//! the canonical operational shapes that later MIR and Rust codegen expect.

use crate::{Absence, FunctionType, Type, TypeId, TypeInterner};

/// Options controlling how source-language type shapes are normalized.
#[derive(Debug, Clone, Copy)]
pub struct NormalizeOptions {
    /// Collapse nested optional wrappers into one optional wrapper.
    pub flatten_optional: bool,
    /// Collapse unions that only add `None` into `Optional<T>`.
    pub flatten_union_none: bool,
    /// Keep optional layers that may encode observable key/property presence.
    pub preserve_observable_absence: bool,
}

impl Default for NormalizeOptions {
    /// The default options: flatten optional layers and `None` unions, and do not preserve observable absence.
    fn default() -> Self {
        Self {
            flatten_optional: true,
            flatten_union_none: true,
            preserve_observable_absence: false,
        }
    }
}

/// Intern an optional type after applying canonical optional flattening.
///
/// The absent value is JavaScript's `undefined` (Python's `None`); see
/// [`optional_with`] for a `null`.
pub fn optional_of(types: &mut TypeInterner, inner: TypeId) -> TypeId {
    optional_with(types, inner, Absence::Undefined)
}

/// Intern `inner` or `absence`, in canonical form.
///
/// An optional layer of the same absence is absorbed (`(T | null) | null` is
/// `T | null`). Layers of both absences nest in one fixed order, `undefined`
/// outside: `Optional(Optional(T, Null), Undefined)`, a Rust
/// `Option<Option<T>>` whose outer `None` is `undefined` and whose
/// `Some(None)` is `null`. So `T | null | undefined` has one spelling
/// whichever way it was assembled, which is what makes a generic `V | null`
/// at `V = number | undefined` the same type as the source's
/// `number | null | undefined`.
pub fn optional_with(types: &mut TypeInterner, inner: TypeId, absence: Absence) -> TypeId {
    let normalized_inner = normalize_type(types, inner, NormalizeOptions::default());
    let (base, undefined, null) = peel_optional(types, normalized_inner);
    let undefined = undefined || absence == Absence::Undefined;
    let null = null || absence == Absence::Null;
    rebuild_optional(types, base, undefined, null)
}

/// Wrap `inner` in one optional layer of `absence`, keeping mixed layers in
/// canonical order.
///
/// Unlike [`optional_with`], a layer of the same absence is not absorbed: a
/// substitution that rebuilds `Optional(V, a)` keeps exactly the layers it
/// had. Only a `null` layer around an `undefined` one moves inside it, so
/// `V | null` at `V = T | undefined` is the canonical
/// `Optional(Optional(T, Null), Undefined)`.
pub fn wrap_optional(types: &mut TypeInterner, inner: TypeId, absence: Absence) -> TypeId {
    if absence == Absence::Null
        && let Some(Type::Optional(nested, Absence::Undefined)) = types.get(inner).cloned()
    {
        let moved = wrap_optional(types, nested, Absence::Null);
        return types.intern(Type::Optional(moved, Absence::Undefined));
    }
    types.intern(Type::Optional(inner, absence))
}

/// `ty` widened to admit `undefined`, as an optional slot (`x?: T`) is.
///
/// A type that admits it already is returned as it is; a `T | null` gains
/// the outer `undefined` layer (`Optional(Optional(T, Null), Undefined)`).
pub fn admit_undefined(types: &mut TypeInterner, ty: TypeId) -> TypeId {
    let (_, undefined, _) = peel_optional(types, ty);
    if undefined {
        ty
    } else {
        wrap_optional(types, ty, Absence::Undefined)
    }
}

/// Split `ty` into its first non-optional type and the absences of the
/// optional layers around it: `(base, admits undefined, admits null)`.
#[must_use]
pub fn peel_optional(types: &TypeInterner, ty: TypeId) -> (TypeId, bool, bool) {
    let mut base = ty;
    let (mut undefined, mut null) = (false, false);
    while let Some(Type::Optional(inner, absence)) = types.get(base) {
        match absence {
            Absence::Undefined => undefined = true,
            Absence::Null => null = true,
        }
        base = *inner;
    }
    (base, undefined, null)
}

/// Wrap `base` in the canonical optional layers for the given absences (see
/// [`optional_with`]); `base` itself when it admits neither.
pub fn rebuild_optional(
    types: &mut TypeInterner,
    base: TypeId,
    undefined: bool,
    null: bool,
) -> TypeId {
    let mut ty = base;
    if null {
        ty = types.intern(Type::Optional(ty, Absence::Null));
    }
    if undefined {
        ty = types.intern(Type::Optional(ty, Absence::Undefined));
    }
    ty
}

/// The single absence `ty` admits as a one-layer optional, or `None` for a
/// non-optional type and for one that admits both absences (whose two layers
/// each have their own).
#[must_use]
pub fn sole_absence(types: &TypeInterner, ty: TypeId) -> Option<Absence> {
    match types.get(ty) {
        Some(Type::Optional(inner, absence)) if !matches!(types.get(*inner), Some(Type::Optional(_, other)) if other != absence) => {
            Some(*absence)
        }
        _ => None,
    }
}

/// Return whether `ty` is represented as an optional/nullish value.
#[must_use]
pub fn is_optional_like(types: &TypeInterner, ty: TypeId) -> bool {
    matches!(types.get(ty), Some(Type::Optional(..)))
}

/// Return the non-nullish part of an optional or `None`-containing union.
pub fn non_nullish_type(types: &mut TypeInterner, ty: TypeId) -> Option<TypeId> {
    match types.get(ty).cloned() {
        Some(Type::Optional(..)) => {
            let (base, _, _) = peel_optional(types, ty);
            Some(normalize_type(types, base, NormalizeOptions::default()))
        }
        Some(Type::Union(items)) => {
            let none_ty = types.intern(Type::None);
            let mut remaining = Vec::new();
            for item in items {
                if item == none_ty {
                    continue;
                }
                let normalized = normalize_type(types, item, NormalizeOptions::default());
                let (flattened, _, _) = peel_optional(types, normalized);
                if !remaining.contains(&flattened) {
                    remaining.push(flattened);
                }
            }
            match remaining.as_slice() {
                [] => None,
                [single] => Some(*single),
                _ => Some(types.intern(Type::Union(remaining))),
            }
        }
        Some(Type::None) | None => None,
        Some(_) => Some(normalize_type(types, ty, NormalizeOptions::default())),
    }
}

/// Normalize a type into the canonical operational shape used downstream.
pub fn normalize_type(types: &mut TypeInterner, ty: TypeId, options: NormalizeOptions) -> TypeId {
    let Some(original) = types.get(ty).cloned() else {
        return ty;
    };
    match original {
        Type::List(item) => {
            let item = normalize_type(types, item, options);
            types.intern(Type::List(item))
        }
        Type::Set(item) => {
            let item = normalize_type(types, item, options);
            types.intern(Type::Set(item))
        }
        Type::Dict(key, value) => {
            let key = normalize_type(types, key, options);
            let value = normalize_type(types, value, options);
            types.intern(Type::Dict(key, value))
        }
        Type::JsMap(key, value) => {
            let key = normalize_type(types, key, options);
            let value = normalize_type(types, value, options);
            types.intern(Type::JsMap(key, value))
        }
        Type::Tuple(items) => {
            let items = items
                .into_iter()
                .map(|item| normalize_type(types, item, options))
                .collect();
            types.intern(Type::Tuple(items))
        }
        Type::Optional(inner, absence) => normalize_optional_type(types, inner, absence, options),
        Type::Union(items) => normalize_union_type(types, items, options),
        Type::Class { name, args } => {
            let args = args
                .into_iter()
                .map(|arg| normalize_type(types, arg, options))
                .collect();
            types.intern(Type::Class { name, args })
        }
        Type::Function(function) => {
            let params = function
                .params
                .into_iter()
                .map(|param| normalize_type(types, param, options))
                .collect();
            let return_ty = normalize_type(types, function.return_ty, options);
            types.intern(Type::Function(FunctionType {
                params,
                rest: function.rest,
                required_params: function.required_params,
                mutable_params: function.mutable_params,
                return_ty,
                is_async: function.is_async,
                may_throw: function.may_throw,
            }))
        }
        Type::Future(item) => {
            let item = normalize_type(types, item, options);
            types.intern(Type::Future(item))
        }
        Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            next_ty,
        } => {
            let yield_ty = normalize_type(types, yield_ty, options);
            let return_ty = normalize_type(types, return_ty, options);
            let next_ty = normalize_type(types, next_ty, options);
            types.intern(Type::Generator {
                is_async,
                yield_ty,
                return_ty,
                next_ty,
            })
        }
        Type::GeneratorResult {
            yield_ty,
            return_ty,
        } => {
            let yield_ty = normalize_type(types, yield_ty, options);
            let return_ty = normalize_type(types, return_ty, options);
            types.intern(Type::GeneratorResult {
                yield_ty,
                return_ty,
            })
        }
        // Not evaluated here: only a call that binds the object can do that.
        Type::IndexedAccess { object, index } => {
            let object = normalize_type(types, object, options);
            let index = normalize_type(types, index, options);
            types.intern(Type::IndexedAccess { object, index })
        }
        Type::Bool
        | Type::Int
        | Type::PyInt
        | Type::FixedInt(_)
        | Type::BigInt
        | Type::Float
        | Type::String
        | Type::PyStr
        | Type::Symbol
        | Type::PropKey
        | Type::Unknown
        | Type::Never
        | Type::TypeParam { .. }
        | Type::None => ty,
    }
}

/// Normalize an optional wrapper while preserving one operational `Option`.
fn normalize_optional_type(
    types: &mut TypeInterner,
    inner: TypeId,
    absence: Absence,
    options: NormalizeOptions,
) -> TypeId {
    let normalized_inner = normalize_type(types, inner, options);
    if options.flatten_optional
        && !options.preserve_observable_absence
        && matches!(types.get(normalized_inner), Some(Type::Optional(..)))
    {
        let (base, undefined, null) = peel_optional(types, normalized_inner);
        return rebuild_optional(
            types,
            base,
            undefined || absence == Absence::Undefined,
            null || absence == Absence::Null,
        );
    }
    if options.flatten_union_none
        && !options.preserve_observable_absence
        && let Some(Type::Union(items)) = types.get(normalized_inner).cloned()
    {
        let union_ty = types.intern(Type::Union(items));
        let Some(non_nullish) = non_nullish_type(types, union_ty) else {
            return types.intern(Type::Optional(normalized_inner, absence));
        };
        return optional_with(types, non_nullish, absence);
    }
    types.intern(Type::Optional(normalized_inner, absence))
}

/// Normalize unions, including `T | None` into canonical optional values.
fn normalize_union_type(
    types: &mut TypeInterner,
    items: Vec<TypeId>,
    options: NormalizeOptions,
) -> TypeId {
    let none_ty = types.intern(Type::None);
    let mut normalized = Vec::new();
    let mut has_none = false;
    let mut has_null = false;
    for item in items {
        let item = normalize_type(types, item, options);
        if item == none_ty {
            has_none = true;
            continue;
        }
        let item = if matches!(types.get(item), Some(Type::Optional(..)))
            && options.flatten_union_none
            && !options.preserve_observable_absence
        {
            let (base, undefined, null) = peel_optional(types, item);
            has_none |= undefined;
            has_null |= null;
            base
        } else {
            item
        };
        // A union is never an arm of a union: `PropertyKey | PropertyKey[]`
        // composes the alias's own union with a list, and codegen reads only a
        // union's top-level arms (its tagged enum, `typeof` routing, arm
        // projection). The nested arms are already normalized.
        let arms = match types.get(item) {
            Some(Type::Union(nested)) => nested.clone(),
            _ => vec![item],
        };
        for arm in arms {
            if arm == none_ty {
                has_none = true;
            } else if !normalized.contains(&arm) {
                normalized.push(arm);
            }
        }
    }
    // A bare `None` arm is the language's own absent value (`undefined`); a
    // `null` arrives only inside an optional arm that says so.
    match (
        normalized.as_slice(),
        (has_none || has_null) && options.flatten_union_none,
    ) {
        ([], _) => none_ty,
        ([single], true) => rebuild_optional(types, *single, has_none, has_null),
        ([single], false) => *single,
        (_, true) if !options.preserve_observable_absence => {
            if has_null {
                let union = types.intern(Type::Union(normalized));
                return rebuild_optional(types, union, has_none, true);
            }
            normalized.push(none_ty);
            types.intern(Type::Union(normalized))
        }
        _ => types.intern(Type::Union(normalized)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flattens_nested_optional_types() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let optional = types.intern(Type::Optional(float, Absence::Undefined));
        let nested = types.intern(Type::Optional(optional, Absence::Undefined));

        let normalized = normalize_type(&mut types, nested, NormalizeOptions::default());

        assert_eq!(
            types.get(normalized),
            Some(&Type::Optional(float, Absence::Undefined))
        );
    }

    #[test]
    fn flattens_union_none_into_optional() {
        let mut types = TypeInterner::default();
        let string = types.intern(Type::String);
        let none = types.intern(Type::None);
        let union = types.intern(Type::Union(vec![string, none]));

        let normalized = normalize_type(&mut types, union, NormalizeOptions::default());

        assert_eq!(
            types.get(normalized),
            Some(&Type::Optional(string, Absence::Undefined))
        );
    }

    #[test]
    fn normalizes_contained_function_types() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let string = types.intern(Type::String);
        let optional_float = types.intern(Type::Optional(float, Absence::Undefined));
        let nested_param = types.intern(Type::Optional(optional_float, Absence::Undefined));
        let optional_string = types.intern(Type::Optional(string, Absence::Undefined));
        let nested_return = types.intern(Type::Optional(optional_string, Absence::Undefined));
        let function = types.intern(Type::Function(FunctionType {
            params: vec![nested_param],
            rest: None,
            required_params: None,
            mutable_params: Vec::new(),
            return_ty: nested_return,
            is_async: false,
            may_throw: false,
        }));

        let normalized = normalize_type(&mut types, function, NormalizeOptions::default());

        let Some(Type::Function(function)) = types.get(normalized) else {
            panic!("expected normalized function type");
        };
        assert_eq!(
            types.get(function.params[0]),
            Some(&Type::Optional(float, Absence::Undefined))
        );
        assert_eq!(
            types.get(function.return_ty),
            Some(&Type::Optional(string, Absence::Undefined))
        );
    }
}
