//! An abstract class that declares nothing of its own is the union of its
//! concrete subclasses.
//!
//! `abstract class Parent {}` with `class Child1 extends Parent` and
//! `class Child2 extends Parent` has no instances of its own (it is abstract),
//! and a value typed `Parent` offers nothing to read through that type (it
//! declares no field and no method). What such a value IS, then, is one of the
//! concrete classes below it, and code using it asks which one (`x instanceof
//! Child1`, a pattern match). A hand-written Rust port spells that type as an
//! enum of the subclasses, which is what a union of classes already lowers to:
//! its generated enum carries each arm's own struct, answers `instanceof` per
//! arm and dispatches member reads over the arms.
//!
//! Before, `Parent` stayed its own (empty) struct, so passing a `Child1` where
//! a `Parent` is expected either did not compile (E0308) or was rebuilt
//! member-wise as the base struct, which keeps nothing: not the subclass's
//! fields, not its methods and not the class `instanceof` asks about.
//!
//! An abstract class that does declare members keeps its struct and the
//! virtual-slot adapters that bind each subclass's implementation into it.
//! The class's own declaration and its own constructor keep its struct type;
//! every other position is rewritten.

use crate::{HirOrigin, Mir};
use smelt_hir::{Symbol, Type, TypeId};
use std::collections::HashMap;

/// Rewrite every use of a member-less abstract class type to the union of
/// its concrete descendants.
pub(in crate::lower) fn abstract_classes_as_unions(mir: &mut Mir) {
    let replacements = member_less_abstract_classes(mir);
    if replacements.is_empty() {
        return;
    }
    let mut cache: HashMap<TypeId, TypeId> = HashMap::new();
    let mut functions = std::mem::take(&mut mir.functions);
    let mut closures = std::mem::take(&mut mir.closures);
    let mut classes = std::mem::take(&mut mir.classes);
    let mut interfaces = std::mem::take(&mut mir.interfaces);
    let mut globals = std::mem::take(&mut mir.globals);
    let types = &mut mir.types;
    let mut map = |ty: TypeId| rewrite(types, ty, &replacements, &mut cache);
    for function in &mut functions {
        let own = match function.origin {
            HirOrigin::ClassConstructor { class, .. } | HirOrigin::ClassMethod { class, .. } => {
                replacements.contains_key(&class)
            }
            HirOrigin::Body(_) | HirOrigin::ClassStaticMethod { .. } => false,
        };
        if !own {
            function.map_types(&mut map);
        }
    }
    for closure in &mut closures {
        closure.map_types(&mut map);
    }
    for class in &mut classes {
        if !replacements.contains_key(&class.name) {
            class.map_types(&mut map);
        }
    }
    for interface in &mut interfaces {
        interface.map_types(&mut map);
    }
    for global in &mut globals {
        global.map_types(&mut map);
    }
    mir.functions = functions;
    mir.closures = closures;
    mir.classes = classes;
    mir.interfaces = interfaces;
    mir.globals = globals;
}

/// Each non-generic abstract class with no fields, methods or abstract
/// methods, mapped to its concrete descendants (in declaration order). A class
/// with no concrete descendant is left alone.
fn member_less_abstract_classes(mir: &Mir) -> HashMap<Symbol, Vec<Symbol>> {
    let mut replacements = HashMap::new();
    for class in &mir.classes {
        if !class.is_abstract
            || !class.type_params.is_empty()
            || !class.fields.is_empty()
            || !class.methods.is_empty()
            || !class.abstract_methods.is_empty()
            || !class.static_fields.is_empty()
            || !class.static_methods.is_empty()
        {
            continue;
        }
        let descendants = mir
            .classes
            .iter()
            .filter(|candidate| !candidate.is_abstract && candidate.type_params.is_empty())
            .filter(|candidate| descends_from(mir, candidate.name, class.name))
            .map(|candidate| candidate.name)
            .collect::<Vec<_>>();
        if !descendants.is_empty() {
            replacements.insert(class.name, descendants);
        }
    }
    replacements
}

/// Whether `class` (transitively) extends `ancestor`.
fn descends_from(mir: &Mir, class: Symbol, ancestor: Symbol) -> bool {
    let mut current = mir
        .classes
        .iter()
        .find(|candidate| candidate.name == class)
        .and_then(|found| found.base);
    let mut steps = 0_usize;
    while let Some(base) = current {
        if base == ancestor {
            return true;
        }
        steps = steps.saturating_add(1);
        if steps > mir.classes.len() {
            return false;
        }
        current = mir
            .classes
            .iter()
            .find(|candidate| candidate.name == base)
            .and_then(|found| found.base);
    }
    false
}

/// `ty` with every member-less abstract class replaced by its descendants'
/// union, at any depth.
fn rewrite(
    types: &mut smelt_hir::TypeInterner,
    ty: TypeId,
    replacements: &HashMap<Symbol, Vec<Symbol>>,
    cache: &mut HashMap<TypeId, TypeId>,
) -> TypeId {
    if let Some(done) = cache.get(&ty) {
        return *done;
    }
    let Some(kind) = types.get(ty).cloned() else {
        return ty;
    };
    let replaced = if let Type::Class { name, args } = &kind
        && args.is_empty()
    {
        replacements.get(name)
    } else {
        None
    };
    let rewritten = match replaced {
        Some(descendants) => {
            let members = descendants
                .iter()
                .map(|member| {
                    types.intern(Type::Class {
                        name: *member,
                        args: Vec::new(),
                    })
                })
                .collect::<Vec<_>>();
            match members.as_slice() {
                [only] => *only,
                _ => types.intern(Type::Union(members)),
            }
        }
        None => {
            match kind.try_map_children(|child| Some(rewrite(types, child, replacements, cache))) {
                Some(mapped) if mapped != kind => types.intern(mapped),
                _ => ty,
            }
        }
    };
    cache.insert(ty, rewritten);
    rewritten
}
