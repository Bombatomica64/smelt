//! Shared HIR type normalization helpers.
//!
//! Frontends can preserve source-language spelling while these helpers build
//! the canonical operational shapes that later MIR and Rust codegen expect.

use crate::{FunctionType, Type, TypeId, TypeInterner};

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
    fn default() -> Self {
        Self {
            flatten_optional: true,
            flatten_union_none: true,
            preserve_observable_absence: false,
        }
    }
}

/// Intern an optional type after applying canonical optional flattening.
pub fn optional_of(types: &mut TypeInterner, inner: TypeId) -> TypeId {
    let normalized_inner = normalize_type(types, inner, NormalizeOptions::default());
    if let Some(Type::Optional(nested)) = types.get(normalized_inner).cloned() {
        return types.intern(Type::Optional(nested));
    }
    types.intern(Type::Optional(normalized_inner))
}

/// Return whether `ty` is represented as an optional/nullish value.
#[must_use]
pub fn is_optional_like(types: &TypeInterner, ty: TypeId) -> bool {
    matches!(types.get(ty), Some(Type::Optional(_)))
}

/// Return the non-nullish part of an optional or `None`-containing union.
pub fn non_nullish_type(types: &mut TypeInterner, ty: TypeId) -> Option<TypeId> {
    match types.get(ty).cloned() {
        Some(Type::Optional(inner)) => {
            Some(normalize_type(types, inner, NormalizeOptions::default()))
        }
        Some(Type::Union(items)) => {
            let none_ty = types.intern(Type::None);
            let mut remaining = Vec::new();
            for item in items {
                if item == none_ty {
                    continue;
                }
                let normalized = normalize_type(types, item, NormalizeOptions::default());
                let flattened = match types.get(normalized).cloned() {
                    Some(Type::Optional(inner)) => inner,
                    _ => normalized,
                };
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
        Type::Optional(inner) => normalize_optional_type(types, inner, options),
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
        Type::Bool
        | Type::Int
        | Type::Float
        | Type::String
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
    options: NormalizeOptions,
) -> TypeId {
    let normalized_inner = normalize_type(types, inner, options);
    if options.flatten_optional
        && !options.preserve_observable_absence
        && let Some(Type::Optional(nested)) = types.get(normalized_inner).cloned()
    {
        return types.intern(Type::Optional(nested));
    }
    if options.flatten_union_none
        && !options.preserve_observable_absence
        && let Some(Type::Union(items)) = types.get(normalized_inner).cloned()
    {
        let union_ty = types.intern(Type::Union(items));
        let Some(non_nullish) = non_nullish_type(types, union_ty) else {
            return types.intern(Type::Optional(normalized_inner));
        };
        return types.intern(Type::Optional(non_nullish));
    }
    types.intern(Type::Optional(normalized_inner))
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
    for item in items {
        let item = normalize_type(types, item, options);
        if item == none_ty {
            has_none = true;
            continue;
        }
        if let Some(Type::Optional(inner)) = types.get(item).cloned()
            && options.flatten_union_none
            && !options.preserve_observable_absence
        {
            has_none = true;
            if !normalized.contains(&inner) {
                normalized.push(inner);
            }
            continue;
        }
        if !normalized.contains(&item) {
            normalized.push(item);
        }
    }
    match (
        normalized.as_slice(),
        has_none && options.flatten_union_none,
    ) {
        ([], _) => none_ty,
        ([single], true) => types.intern(Type::Optional(*single)),
        ([single], false) => *single,
        (_, true) if !options.preserve_observable_absence => {
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
        let optional = types.intern(Type::Optional(float));
        let nested = types.intern(Type::Optional(optional));

        let normalized = normalize_type(&mut types, nested, NormalizeOptions::default());

        assert_eq!(types.get(normalized), Some(&Type::Optional(float)));
    }

    #[test]
    fn flattens_union_none_into_optional() {
        let mut types = TypeInterner::default();
        let string = types.intern(Type::String);
        let none = types.intern(Type::None);
        let union = types.intern(Type::Union(vec![string, none]));

        let normalized = normalize_type(&mut types, union, NormalizeOptions::default());

        assert_eq!(types.get(normalized), Some(&Type::Optional(string)));
    }

    #[test]
    fn normalizes_contained_function_types() {
        let mut types = TypeInterner::default();
        let float = types.intern(Type::Float);
        let string = types.intern(Type::String);
        let optional_float = types.intern(Type::Optional(float));
        let nested_param = types.intern(Type::Optional(optional_float));
        let optional_string = types.intern(Type::Optional(string));
        let nested_return = types.intern(Type::Optional(optional_string));
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
        assert_eq!(types.get(function.params[0]), Some(&Type::Optional(float)));
        assert_eq!(types.get(function.return_ty), Some(&Type::Optional(string)));
    }
}
