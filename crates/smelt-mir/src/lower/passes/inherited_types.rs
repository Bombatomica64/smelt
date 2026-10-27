//! Interning of the types an inherited member copy is emitted at.
//!
//! Smelt flattens inheritance: a base class's methods and constructor body are
//! re-emitted into every subclass's `impl` block (codegen's
//! `effective_class_methods` and `base_init`). Inside that copy the base's type
//! parameters are not declared; they stand for the subclass's arguments
//! (`class YearParser extends Parser<number>` copies `Parser::run` with `T =
//! f64`). Codegen emits the copy from a clone of the function whose types are
//! substituted, and it holds `&Mir`, so it can only LOOK UP the substituted
//! types. This pass interns them, once, in the pass that owns the type table.
//!
//! It adds types and never rewrites one, so no existing id changes meaning.

use super::generic_records::substitute_and_intern;
use crate::{FuncId, Mir};
use smelt_hir::{Symbol, TypeId};
use std::collections::{HashMap, HashSet};

/// Intern every type an inherited method or base-constructor copy is emitted
/// at, for every class that extends a generic base.
pub(in crate::lower) fn intern_inherited_member_types(mir: &mut Mir) {
    for class_index in 0..mir.classes.len() {
        for (members, substitutions) in ancestor_substitutions(mir, class_index) {
            for member in members {
                let Some(function) = usize::try_from(member.0)
                    .ok()
                    .and_then(|index| mir.functions.get(index))
                else {
                    continue;
                };
                let mut types = Vec::new();
                crate::for_each_function_type(function, |ty| types.push(ty));
                for ty in types {
                    substitute_and_intern(mir, ty, &substitutions);
                }
            }
        }
    }
}

/// One ancestor's members (methods and constructor) paired with the map from
/// its type parameters to their arguments in the subclass's scope.
type AncestorMembers = (Vec<FuncId>, HashMap<Symbol, TypeId>);

/// For each ancestor of `mir.classes[class_index]` whose type parameters are
/// bound by the chain, its members and their substitution (interning each
/// composed argument).
fn ancestor_substitutions(mir: &mut Mir, class_index: usize) -> Vec<AncestorMembers> {
    let mut result = Vec::new();
    let mut outer: HashMap<Symbol, TypeId> = HashMap::new();
    let mut current = class_index;
    let mut seen = HashSet::new();
    while seen.insert(current) {
        let Some(class) = mir.classes.get(current) else {
            break;
        };
        let Some(base_name) = class.base else {
            break;
        };
        let base_args = class.base_args.clone();
        let Some(base_index) = mir.classes.iter().position(|item| item.name == base_name) else {
            break;
        };
        let mut map: HashMap<Symbol, TypeId> = HashMap::new();
        for (position, arg) in base_args.into_iter().enumerate() {
            let Some(param) = mir
                .classes
                .get(base_index)
                .and_then(|base| base.type_params.get(position))
                .map(|param| param.name)
            else {
                break;
            };
            let bound = substitute_and_intern(mir, arg, &outer);
            map.insert(param, bound);
        }
        if !map.is_empty()
            && let Some(base) = mir.classes.get(base_index)
        {
            let members = base.methods.iter().copied().chain(base.constructor).collect();
            result.push((members, map.clone()));
        }
        outer = map;
        current = base_index;
    }
    result
}
