//! Closure function-type synchronization after throwing propagation.
//!
//! Invariant: closure values whose bodies can throw are represented by throwing
//! function types everywhere they are stored, aliased, or captured.

use smelt_hir::Type;

use crate::{BasicBlock, ClosureId, LocalDecl, Mir, MirClosure, Rvalue, Statement};

use super::super::{closure_id_index, local_index};
use super::aliases::{
    local_alias_assignments, propagate_throwing_function_aliases, widen_function_local_from_source,
};
use super::operand_local;

/// Widen MIR locals that hold throwing closures to the throwing function ABI.
///
/// HIR closure expressions initially carry the syntactic function type. MIR can
/// later discover additional throwing behavior through lowered calls inside the
/// closure body, so the local receiving `Rvalue::Closure` must be updated after
/// throw propagation. Keeping this in MIR avoids frontend guesses about which
/// callees will eventually use a `Result` ABI.
pub(in crate::lower) fn synchronize_throwing_closure_types(mir: &mut Mir) {
    for function_index in 0..mir.functions.len() {
        let mut changed = true;
        while changed {
            changed = false;
            let assignments = {
                let Some(function) = mir.functions.get(function_index) else {
                    continue;
                };
                local_alias_assignments(&function.blocks)
            };
            for (dest, source) in assignments {
                let Some(function) = mir.functions.get_mut(function_index) else {
                    continue;
                };
                if widen_function_local_from_source(
                    &mut mir.types,
                    &mut function.locals,
                    dest,
                    source,
                ) {
                    changed = true;
                }
            }
        }
        let updates = {
            let Some(function) = mir.functions.get(function_index) else {
                continue;
            };
            let mut updates = Vec::new();
            for block in &function.blocks {
                for statement in &block.statements {
                    let Statement::Assign {
                        dest,
                        value: Rvalue::Closure { id, .. },
                    } = statement
                    else {
                        continue;
                    };
                    let Some(closure) =
                        closure_id_index(*id).and_then(|index| mir.closures.get(index))
                    else {
                        continue;
                    };
                    if !closure.can_throw {
                        continue;
                    }
                    updates.push((*dest, closure.return_ty));
                }
            }
            updates
        };
        for (local, return_ty) in updates {
            let Some(local_decl) = local_index(local)
                .and_then(|index| {
                    mir.functions
                        .get(function_index)
                        .and_then(|function| function.locals.get(index))
                })
                .cloned()
            else {
                continue;
            };
            let Some(Type::Function(mut function_ty)) = mir.types.get(local_decl.ty).cloned()
            else {
                continue;
            };
            if function_ty.may_throw {
                continue;
            }
            function_ty.may_throw = true;
            function_ty.return_ty = return_ty;
            let widened = mir.types.intern(Type::Function(function_ty));
            if let Some(local_decl) = local_index(local).and_then(|index| {
                mir.functions
                    .get_mut(function_index)
                    .and_then(|function| function.locals.get_mut(index))
            }) {
                local_decl.ty = widened;
            }
        }
        let Some(function) = mir.functions.get_mut(function_index) else {
            continue;
        };
        let assignments = local_alias_assignments(&function.blocks);
        propagate_throwing_function_aliases(&mut mir.types, &mut function.locals, &assignments);
        let owner_locals = function.locals.clone();
        let closure_ids = closure_ids_in_blocks(&function.blocks);
        synchronize_closure_capture_types_from_locals(
            &mut mir.closures,
            &owner_locals,
            closure_ids,
        );
    }
    for closure_index in 0..mir.closures.len() {
        let mut changed = true;
        while changed {
            changed = false;
            let assignments = {
                let Some(closure) = mir.closures.get(closure_index) else {
                    continue;
                };
                closure
                    .blocks
                    .iter()
                    .flat_map(|block| block.statements.iter())
                    .filter_map(|statement| {
                        let Statement::Assign {
                            dest,
                            value: Rvalue::Use(operand),
                        } = statement
                        else {
                            return None;
                        };
                        let source = operand_local(operand)?;
                        Some((*dest, source))
                    })
                    .collect::<Vec<_>>()
            };
            for (dest, source) in assignments {
                let Some(closure) = mir.closures.get_mut(closure_index) else {
                    continue;
                };
                if widen_function_local_from_source(
                    &mut mir.types,
                    &mut closure.locals,
                    dest,
                    source,
                ) {
                    changed = true;
                }
            }
        }
        let updates = {
            let Some(closure) = mir.closures.get(closure_index) else {
                continue;
            };
            let mut updates = Vec::new();
            for block in &closure.blocks {
                for statement in &block.statements {
                    let Statement::Assign {
                        dest,
                        value: Rvalue::Closure { id, .. },
                    } = statement
                    else {
                        continue;
                    };
                    let Some(nested) =
                        closure_id_index(*id).and_then(|index| mir.closures.get(index))
                    else {
                        continue;
                    };
                    if nested.can_throw {
                        updates.push((*dest, nested.return_ty));
                    }
                }
            }
            updates
        };
        for (local, return_ty) in updates {
            let Some(local_decl) = local_index(local)
                .and_then(|index| {
                    mir.closures
                        .get(closure_index)
                        .and_then(|closure| closure.locals.get(index))
                })
                .cloned()
            else {
                continue;
            };
            let Some(Type::Function(mut function_ty)) = mir.types.get(local_decl.ty).cloned()
            else {
                continue;
            };
            if function_ty.may_throw {
                continue;
            }
            function_ty.may_throw = true;
            function_ty.return_ty = return_ty;
            let widened = mir.types.intern(Type::Function(function_ty));
            if let Some(local_decl) = local_index(local).and_then(|index| {
                mir.closures
                    .get_mut(closure_index)
                    .and_then(|closure| closure.locals.get_mut(index))
            }) {
                local_decl.ty = widened;
            }
        }
        let Some(closure) = mir.closures.get_mut(closure_index) else {
            continue;
        };
        let assignments = local_alias_assignments(&closure.blocks);
        propagate_throwing_function_aliases(&mut mir.types, &mut closure.locals, &assignments);
        let owner_locals = closure.locals.clone();
        let closure_ids = closure_ids_in_blocks(&closure.blocks);
        synchronize_closure_capture_types_from_locals(
            &mut mir.closures,
            &owner_locals,
            closure_ids,
        );
    }
}

/// Return closure IDs constructed by assignments in a block list.
fn closure_ids_in_blocks(blocks: &[BasicBlock]) -> Vec<ClosureId> {
    blocks
        .iter()
        .flat_map(|block| block.statements.iter())
        .filter_map(|statement| {
            let Statement::Assign {
                value: Rvalue::Closure { id, .. },
                ..
            } = statement
            else {
                return None;
            };
            Some(*id)
        })
        .collect()
}

/// Keep capture metadata and captured locals aligned with widened source locals.
fn synchronize_closure_capture_types_from_locals(
    closures: &mut [MirClosure],
    owner_locals: &[LocalDecl],
    closure_ids: Vec<ClosureId>,
) {
    for closure_id in closure_ids {
        let Some(closure) = closure_id_index(closure_id).and_then(|index| closures.get_mut(index))
        else {
            continue;
        };
        for capture in &mut closure.captures {
            let Some(source_ty) = local_index(capture.source_local)
                .and_then(|index| owner_locals.get(index))
                .map(|decl| decl.ty)
            else {
                continue;
            };
            capture.ty = source_ty;
            if let Some(target_local) = capture.target_local
                && let Some(local) =
                    local_index(target_local).and_then(|index| closure.locals.get_mut(index))
            {
                local.ty = source_ty;
            }
        }
    }
}
