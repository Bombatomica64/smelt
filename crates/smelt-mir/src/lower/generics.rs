//! Interface field flattening and generic type-parameter substitution.
//!
//! MIR needs a concrete field layout for TypeScript interfaces, whose
//! inheritance and generic arguments are erased at runtime. The helpers here
//! walk interface heritage chains, apply generic substitutions to interned
//! [`TypeId`]s, and hand codegen a flattened record-like field list. Keeping
//! this type machinery separate from expression lowering lets the substitution
//! rules evolve without touching the statement/expression builders.

use std::collections::{HashMap, HashSet};

use smelt_hir::{Symbol, Type, TypeId};

use crate::Mir;

/// Return interface fields with inherited parent fields substituted and flattened.
///
/// TypeScript interface inheritance is erased at runtime, but generated Rust
/// storage needs a concrete field layout. MIR expands parent fields while
/// lowering HIR so codegen can treat interfaces as regular records without
/// retaining frontend-only heritage lookup tables.
pub(super) fn lower_effective_interface_fields(
    krate: &smelt_hir::Crate,
    mir: &mut Mir,
    interface: &smelt_hir::Interface,
    substitutions: &HashMap<Symbol, TypeId>,
    seen: &mut HashSet<Symbol>,
) -> Vec<smelt_hir::Field> {
    if !seen.insert(interface.name) {
        return Vec::new();
    }
    let mut fields = Vec::new();
    for heritage in &interface.extends {
        let Some(parent) = find_hir_interface(krate, heritage.parent) else {
            continue;
        };
        let parent_args = heritage
            .args
            .iter()
            .copied()
            .map(|arg| substitute_type_id(mir, arg, substitutions))
            .collect::<Vec<_>>();
        let parent_substitutions = parent
            .type_params
            .iter()
            .zip(parent_args.iter().copied())
            .map(|(param, arg)| (param.name, arg))
            .collect::<HashMap<_, _>>();
        fields.extend(lower_effective_interface_fields(
            krate,
            mir,
            parent,
            &parent_substitutions,
            seen,
        ));
    }
    for source_field in &interface.fields {
        let mut lowered_field = source_field.clone();
        lowered_field.ty = substitute_type_id(mir, lowered_field.ty, substitutions);
        if let Some(existing) = fields
            .iter_mut()
            .find(|candidate: &&mut smelt_hir::Field| candidate.name == lowered_field.name)
        {
            *existing = lowered_field;
        } else {
            fields.push(lowered_field);
        }
    }
    seen.remove(&interface.name);
    fields
}

/// Find a HIR interface item by symbol.
fn find_hir_interface(krate: &smelt_hir::Crate, name: Symbol) -> Option<&smelt_hir::Interface> {
    krate.items.iter().find_map(|item| {
        if let smelt_hir::Item::Interface(interface) = item
            && interface.name == name
        {
            Some(interface)
        } else {
            None
        }
    })
}

/// Apply interface generic substitutions to a type, interning rebuilt shapes.
pub(super) fn substitute_type_id(
    mir: &mut Mir,
    ty: TypeId,
    substitutions: &HashMap<Symbol, TypeId>,
) -> TypeId {
    match mir.types.get(ty).cloned() {
        Some(Type::TypeParam { name }) => substitutions.get(&name).copied().unwrap_or(ty),
        Some(Type::List(item)) => {
            let substituted_item = substitute_type_id(mir, item, substitutions);
            mir.types.intern(Type::List(substituted_item))
        }
        Some(Type::Set(item)) => {
            let substituted_item = substitute_type_id(mir, item, substitutions);
            mir.types.intern(Type::Set(substituted_item))
        }
        Some(Type::Dict(key, value)) => {
            let substituted_key = substitute_type_id(mir, key, substitutions);
            let substituted_value = substitute_type_id(mir, value, substitutions);
            mir.types
                .intern(Type::Dict(substituted_key, substituted_value))
        }
        Some(Type::JsMap(key, value)) => {
            let substituted_key = substitute_type_id(mir, key, substitutions);
            let substituted_value = substitute_type_id(mir, value, substitutions);
            mir.types
                .intern(Type::JsMap(substituted_key, substituted_value))
        }
        Some(Type::Tuple(items)) => {
            let substituted_items = items
                .into_iter()
                .map(|item| substitute_type_id(mir, item, substitutions))
                .collect();
            mir.types.intern(Type::Tuple(substituted_items))
        }
        Some(Type::Optional(item)) => {
            let substituted_item = substitute_type_id(mir, item, substitutions);
            mir.types.intern(Type::Optional(substituted_item))
        }
        Some(Type::Union(items)) => {
            let substituted_items = items
                .into_iter()
                .map(|item| substitute_type_id(mir, item, substitutions))
                .collect();
            mir.types.intern(Type::Union(substituted_items))
        }
        Some(Type::Class { name, args }) => {
            let substituted_args = args
                .into_iter()
                .map(|arg| substitute_type_id(mir, arg, substitutions))
                .collect();
            mir.types.intern(Type::Class {
                name,
                args: substituted_args,
            })
        }
        Some(Type::Function(mut function)) => {
            function.params = function
                .params
                .into_iter()
                .map(|param| substitute_type_id(mir, param, substitutions))
                .collect();
            function.return_ty = substitute_type_id(mir, function.return_ty, substitutions);
            mir.types.intern(Type::Function(function))
        }
        Some(Type::Future(item)) => {
            let substituted_item = substitute_type_id(mir, item, substitutions);
            mir.types.intern(Type::Future(substituted_item))
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
        | None => ty,
    }
}
