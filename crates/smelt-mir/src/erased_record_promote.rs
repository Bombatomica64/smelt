//! Promote erased-and-mutated record locals to an erased value representation.
//!
//! JavaScript objects are reference values: passing an object somewhere and then
//! mutating the original binding must be observable through the alias. Smelt
//! represents a statically-typed object literal as a `SmeltRecord<K, V>` with a
//! concrete value type `V`. Erasing such a record to `SmeltUnknown::Object`
//! (which happens when it crosses a generic/`unknown` boundary) *copies* its
//! entries, because the typed `V` cannot share storage with the erased
//! `SmeltUnknown` map. A later mutation of the original binding is then invisible
//! through the erased alias — breaking reference identity (see remeda
//! `constant` "returns identity (doesn't clone)").
//!
//! When a record local is *both* mutated and erased-by-reference, lowering its
//! value type to `Unknown` lets the erasure share the underlying
//! `Rc<RefCell<..>>` (via `SmeltObject::from_unknown_record`), so the alias
//! observes the mutation. This is exactly the dynamic-boundary case where a
//! tagged `SmeltUnknown` representation is appropriate.

use std::collections::{HashMap, HashSet};

use smelt_hir::{Type, TypeId};

use crate::{
    BasicBlock, Callee, FuncId, LocalDecl, LocalId, Mir, Operand, Place, Rvalue, Statement,
    Terminator,
};

/// Rewrite erased-and-mutated record locals to use an erased value type.
pub fn promote_erased_mutated_records(mir: &mut Mir) {
    // Parameter value types for each statically-known function, used to detect
    // arguments that are erased to `unknown`/a generic type parameter.
    let func_param_tys: HashMap<FuncId, Vec<TypeId>> = mir
        .functions
        .iter()
        .map(|function| {
            let tys = function
                .params
                .iter()
                .filter_map(|local| function.locals.get(local.0 as usize).map(|decl| decl.ty))
                .collect::<Vec<_>>();
            (function.id, tys)
        })
        .collect();

    let mut functions = std::mem::take(&mut mir.functions);
    for function in &mut functions {
        promote_body(
            &mut mir.types,
            &func_param_tys,
            &function.params,
            &mut function.locals,
            &function.blocks,
        );
    }
    mir.functions = functions;

    let mut closures = std::mem::take(&mut mir.closures);
    for closure in &mut closures {
        promote_body(
            &mut mir.types,
            &func_param_tys,
            &closure.params,
            &mut closure.locals,
            &closure.blocks,
        );
    }
    mir.closures = closures;
}

/// Analyze one function/closure body and promote qualifying record locals.
fn promote_body(
    types: &mut smelt_hir::TypeInterner,
    func_param_tys: &HashMap<FuncId, Vec<TypeId>>,
    params: &[LocalId],
    locals: &mut [LocalDecl],
    blocks: &[BasicBlock],
) {
    let mut mutated: HashSet<LocalId> = HashSet::new();
    let mut erased: HashSet<LocalId> = HashSet::new();
    // Undirected copy edges between locals (`dest = copy/move src`). Erasure or
    // mutation of either endpoint should promote the whole alias component so a
    // record literal feeding a binding is promoted alongside the binding.
    let mut copy_edges: Vec<(LocalId, LocalId)> = Vec::new();

    let note_erased_operand = |erased: &mut HashSet<LocalId>, operand: &Operand| {
        if let Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) = operand {
            erased.insert(*local);
        }
    };

    for block in blocks {
        for statement in &block.statements {
            match statement {
                Statement::AssignPlace { place, value } => {
                    match place {
                        Place::Field { base, .. } | Place::Index { base, .. } => {
                            mutated.insert(*base);
                        }
                        Place::Local(_) => {}
                    }
                    // A value erased through an explicit cast escapes by reference.
                    if let Rvalue::UnknownCast { value, .. } = value {
                        note_erased_operand(&mut erased, value);
                    }
                }
                Statement::Assign { dest, value } => {
                    match value {
                        Rvalue::Use(Operand::Copy(Place::Local(src)))
                        | Rvalue::Use(Operand::Move(Place::Local(src))) => {
                            copy_edges.push((*dest, *src));
                        }
                        Rvalue::UnknownCast { value, .. } => {
                            note_erased_operand(&mut erased, value);
                        }
                        _ => {}
                    }
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }
        if let Some(Terminator::Call { callee, args, .. }) = &block.terminator
            && let Callee::Static(func_id) = callee
            && let Some(param_tys) = func_param_tys.get(func_id)
        {
            for (index, arg) in args.iter().enumerate() {
                let Some(param_ty) = param_tys.get(index) else {
                    continue;
                };
                if matches!(
                    types.get(*param_ty),
                    Some(Type::Unknown | Type::TypeParam { .. })
                ) {
                    note_erased_operand(&mut erased, arg);
                }
            }
        }
    }

    // Seed promotion with record locals that are both mutated and erased, then
    // expand across copy edges so their defining literals are promoted too.
    let param_set: HashSet<LocalId> = params.iter().copied().collect();
    let mut to_promote: HashSet<LocalId> = mutated.intersection(&erased).copied().collect();
    if to_promote.is_empty() {
        return;
    }
    expand_copy_component(&mut to_promote, &copy_edges);

    for local in to_promote {
        // Promoting a parameter would change the function's ABI; leave it alone.
        if param_set.contains(&local) {
            continue;
        }
        let Some(decl) = locals.get_mut(local.0 as usize) else {
            continue;
        };
        if let Some(Type::Dict(key_ty, value_ty)) = types.get(decl.ty).cloned() {
            if matches!(types.get(value_ty), Some(Type::Unknown)) {
                continue;
            }
            let unknown_ty = types.intern(Type::Unknown);
            decl.ty = types.intern(Type::Dict(key_ty, unknown_ty));
        }
    }
}

/// Grow `seed` to include every local reachable through undirected copy edges.
fn expand_copy_component(seed: &mut HashSet<LocalId>, copy_edges: &[(LocalId, LocalId)]) {
    let mut adjacency: HashMap<LocalId, Vec<LocalId>> = HashMap::new();
    for (dest, src) in copy_edges {
        adjacency.entry(*dest).or_default().push(*src);
        adjacency.entry(*src).or_default().push(*dest);
    }
    let mut stack: Vec<LocalId> = seed.iter().copied().collect();
    while let Some(local) = stack.pop() {
        let Some(neighbors) = adjacency.get(&local) else {
            continue;
        };
        for neighbor in neighbors {
            if seed.insert(*neighbor) {
                stack.push(*neighbor);
            }
        }
    }
}
