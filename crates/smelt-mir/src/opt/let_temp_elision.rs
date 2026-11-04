//! Let-temporary elision.
//!
//! HIR-to-MIR lowering evaluates every initializer into a fresh temporary and
//! then moves it into the declared local:
//!
//! ```text
//! %5 = list_flat copy %1, _
//! %2 = move %5          // %2 is `transforms`
//! ```
//!
//! A hand-written port has no such temporary (`let transforms =
//! _transforms.flat();`). [`CopyPropagation`](super::CopyPropagation) never
//! removes it on purpose: aliasing the user local to the temporary would
//! replace the source name with a synthetic one. This pass removes it in the
//! other direction: the defining statement writes straight into the user local
//! and the move disappears, so the source name is kept.
//!
//! # Correctness
//!
//! A pair `t = rv; u = move|copy t` in one block, as adjacent statements, is
//! rewritten to `u = rv` only when every guard holds:
//!
//! * `t` is a compiler temporary defined exactly once and read exactly once
//!   (by the move), so no other statement observes it.
//! * `u` is defined exactly once (the move), so `u` is a single-assignment
//!   binding and writing it one statement earlier changes no other read: the
//!   only statement in between is the move itself.
//! * `rv` does not read `u`. A self-referencing initializer (a closure that
//!   captures its own binding) must keep reading the binding's prior state.
//! * `t` and `u` have the same type, so no conversion was implied by the move.
//! * Neither is function-typed: callable locals carry ABI decisions
//!   (`Rc<dyn Fn>` vs. borrowed callbacks) that copy propagation also leaves
//!   alone.
//! * `t` appears in no `StorageLive`/`StorageDead` marker.
//!
//! Behaviour requirement: unchanged (the same value reaches the same local).
//! Type-preservation requirement: unchanged types; the generated Rust has one
//! fewer `let` per such binding.

use smelt_hir::Type;

use super::local_use::{local_assignment_count, local_decl, local_is_temp, local_read_count, operand_local};
use super::Pass;
use crate::{Mir, MirFunction, Operand, Rvalue, Statement};

/// The let-temporary elision pass; see the module docs.
#[derive(Debug, Default)]
pub struct LetTempElision;

impl Pass for LetTempElision {
    /// Returns the stable identifier of this pass.
    fn name(&self) -> &'static str {
        "let-temp-elision"
    }

    /// Applies the pass to `mir`, returning true if it was modified.
    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= elide_function(function, &mir.types);
        }
        changed
    }
}

/// Elides every qualifying temporary in one function; true if modified.
fn elide_function(function: &mut MirFunction, types: &smelt_hir::TypeInterner) -> bool {
    let mut changed = false;
    for block_index in 0..function.blocks.len() {
        let mut index: usize = 0;
        while index.saturating_add(1) < function.blocks[block_index].statements.len() {
            if let Some(user) = elidable_pair(function, types, block_index, index) {
                let block = &mut function.blocks[block_index];
                if let Statement::Assign { dest, .. } = &mut block.statements[index] {
                    *dest = user;
                }
                block.statements.remove(index.saturating_add(1));
                changed = true;
            }
            index = index.saturating_add(1);
        }
    }
    changed
}

/// The user local `u` when statements `index` and `index + 1` of the block
/// are an elidable `t = rv; u = move t` pair.
fn elidable_pair(
    function: &MirFunction,
    types: &smelt_hir::TypeInterner,
    block_index: usize,
    index: usize,
) -> Option<crate::LocalId> {
    let statements = &function.blocks.get(block_index)?.statements;
    let Statement::Assign { dest: temp, value } = statements.get(index)? else {
        return None;
    };
    let Statement::Assign { dest: user, value: Rvalue::Use(operand @ (Operand::Copy(_) | Operand::Move(_))) } =
        statements.get(index.saturating_add(1))?
    else {
        return None;
    };
    let (temp, user) = (*temp, *user);
    if operand_local(operand) != Some(temp) || temp == user {
        return None;
    }
    let temp_decl = local_decl(function, temp)?;
    let user_decl = local_decl(function, user)?;
    let reads_user = {
        let mut reads = false;
        value.for_each_operand(|operand| reads |= super::local_use::operand_reads_local(operand, user));
        reads
    };
    let has_storage_marker = function.blocks.iter().flat_map(|block| &block.statements).any(|statement| {
        matches!(statement, Statement::StorageLive(local) | Statement::StorageDead(local) if *local == temp)
    });
    let is_function = |ty| matches!(types.get(ty), Some(Type::Function(_)));
    (local_is_temp(function, temp)
        && local_assignment_count(function, temp) == 1
        && local_read_count(function, temp) == 1
        && local_assignment_count(function, user) == 1
        && !reads_user
        && temp_decl.ty == user_decl.ty
        && !is_function(temp_decl.ty)
        && !has_storage_marker)
        .then_some(user)
}
