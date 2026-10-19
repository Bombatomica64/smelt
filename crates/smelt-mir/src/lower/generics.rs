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

/// Collect an interface's strict-JSON decode facts, inherited ones included.
///
/// Mirrors [`lower_effective_interface_fields`]: a parent's facts come first
/// and the interface's own facts for the same property replace them, exactly as
/// a redeclared field replaces the inherited one. The facts are keyed by
/// property symbol, so no type substitution applies (a literal or nullability
/// fact does not mention a type parameter).
pub(super) fn lower_effective_interface_decode_facts(
    krate: &smelt_hir::Crate,
    interface: &smelt_hir::Interface,
    seen: &mut HashSet<Symbol>,
) -> Vec<smelt_hir::FieldDecodeFacts> {
    if !seen.insert(interface.name) {
        return Vec::new();
    }
    let mut facts: Vec<smelt_hir::FieldDecodeFacts> = Vec::new();
    for heritage in &interface.extends {
        if let Some(parent) = find_hir_interface(krate, heritage.parent) {
            facts.extend(lower_effective_interface_decode_facts(krate, parent, seen));
        }
    }
    for own in &interface.decode_facts {
        facts.retain(|inherited| inherited.name != own.name);
        facts.push(own.clone());
    }
    seen.remove(&interface.name);
    facts
}

/// The walk state [`lower_effective_interface_fields`] threads through an
/// `extends` chain.
#[derive(Default)]
pub(super) struct InterfaceFlattening {
    /// Interfaces on the current path, which cuts an (invalid) cyclic chain.
    seen: HashSet<Symbol>,
    /// Substitution-free flattenings already computed, by interface.
    flattened: HashMap<Symbol, Vec<smelt_hir::Field>>,
}

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
    state: &mut InterfaceFlattening,
) -> Vec<smelt_hir::Field> {
    // An interface reached with no substitutions in force flattens to the same
    // fields every time, so it is computed once: re-flattening every ancestor
    // for every interface made a long `extends` chain cubic (measured by
    // `tests/compile_scaling.rs` in `smelt-codegen-rust`). Only the
    // substitution-free case is cached; a generic parent instantiated with
    // arguments is flattened per instantiation, as before.
    let memoizable = substitutions.is_empty();
    if memoizable && let Some(fields) = state.flattened.get(&interface.name) {
        return fields.clone();
    }
    if !state.seen.insert(interface.name) {
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
            state,
        ));
    }
    // An override replaces the inherited slot in place. The slot lookup is a
    // map: a linear `find` per field made a long `extends` chain cubic
    // (measured by `tests/compile_scaling.rs` in `smelt-codegen-rust`).
    let mut slots: HashMap<Symbol, usize> = fields
        .iter()
        .enumerate()
        .map(|(slot, field)| (field.name, slot))
        .collect();
    for source_field in &interface.fields {
        let mut lowered_field = source_field.clone();
        lowered_field.ty = substitute_type_id(mir, lowered_field.ty, substitutions);
        if let Some(existing) = slots.get(&lowered_field.name).and_then(|slot| fields.get_mut(*slot)) {
            *existing = lowered_field;
        } else {
            slots.insert(lowered_field.name, fields.len());
            fields.push(lowered_field);
        }
    }
    state.seen.remove(&interface.name);
    // A cycle (`seen` cut the walk short) leaves a partial answer that depends
    // on where the walk entered, so only a walk that started clean is cached.
    if memoizable && state.seen.is_empty() {
        state.flattened.insert(interface.name, fields.clone());
    }
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
        Some(Type::Generator {
            is_async,
            yield_ty,
            return_ty,
            next_ty,
        }) => {
            let substituted_yield = substitute_type_id(mir, yield_ty, substitutions);
            let substituted_return = substitute_type_id(mir, return_ty, substitutions);
            let substituted_next = substitute_type_id(mir, next_ty, substitutions);
            mir.types.intern(Type::Generator {
                is_async,
                yield_ty: substituted_yield,
                return_ty: substituted_return,
                next_ty: substituted_next,
            })
        }
        Some(Type::GeneratorResult {
            yield_ty,
            return_ty,
        }) => {
            let substituted_yield = substitute_type_id(mir, yield_ty, substitutions);
            let substituted_return = substitute_type_id(mir, return_ty, substitutions);
            mir.types.intern(Type::GeneratorResult {
                yield_ty: substituted_yield,
                return_ty: substituted_return,
            })
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
        | None => ty,
    }
}
