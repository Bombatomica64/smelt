//! Throwing function ABI propagation through local aliases.
//!
//! Invariant: after this pass runs for a local set, every function-typed local
//! that aliases a throwing function-typed source is also typed with the
//! throwing ABI and the source return type.

use smelt_hir::Type;

use crate::{BasicBlock, LocalDecl, LocalId, Rvalue, Statement};

use super::super::local_index;
use super::operand_local;

/// Return local alias assignments from a block list.
pub(super) fn local_alias_assignments(blocks: &[BasicBlock]) -> Vec<(LocalId, LocalId)> {
    blocks
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
        .collect()
}

/// Propagate throwing function ABI through local-to-local aliases in blocks.
pub(super) fn propagate_throwing_function_aliases(
    types: &mut smelt_hir::TypeInterner,
    locals: &mut [LocalDecl],
    assignments: &[(LocalId, LocalId)],
) {
    let mut changed = true;
    while changed {
        changed = false;
        for (dest, source) in assignments.iter().copied() {
            if widen_function_local_from_source(types, locals, dest, source) {
                changed = true;
            }
        }
    }
}

/// Widen a function-typed destination local when it aliases a throwing source.
pub(super) fn widen_function_local_from_source(
    types: &mut smelt_hir::TypeInterner,
    locals: &mut [LocalDecl],
    dest: LocalId,
    source: LocalId,
) -> bool {
    let Some(source_ty) = local_index(source)
        .and_then(|index| locals.get(index))
        .map(|decl| decl.ty)
    else {
        return false;
    };
    let Some(Type::Function(source_fn)) = types.get(source_ty).cloned() else {
        return false;
    };
    if !source_fn.may_throw {
        return false;
    }
    let Some(dest_ty) = local_index(dest)
        .and_then(|index| locals.get(index))
        .map(|decl| decl.ty)
    else {
        return false;
    };
    let Some(Type::Function(mut dest_fn)) = types.get(dest_ty).cloned() else {
        return false;
    };
    if dest_fn.may_throw {
        return false;
    }
    dest_fn.may_throw = true;
    dest_fn.return_ty = source_fn.return_ty;
    let widened = types.intern(Type::Function(dest_fn));
    if let Some(local) = local_index(dest).and_then(|index| locals.get_mut(index)) {
        local.ty = widened;
        true
    } else {
        false
    }
}
