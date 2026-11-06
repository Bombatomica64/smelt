//! Which locals of a body can change without a statement of that body saying so.
//!
//! A closure's write to a capture lands in the cell the creating body reads, so
//! a local of the creating body changes at whatever point the closure runs. The
//! passes that reason about one statement stream need that fact: a read of the
//! local after a call is not the value before it.
//!
//! A body's shared locals are
//!
//! * every capture of a closure body, since any holder of the cell can write it;
//! * every local a closure created in the body captures and writes, directly or
//!   through a closure nested inside it.

use std::collections::HashSet;

use crate::{BasicBlock, ClosureId, LocalId, Mir, MirClosure, Rvalue, Statement};

/// The shared locals of every body of a [`Mir`], see the module docs.
#[derive(Debug, Default)]
pub(super) struct SharedLocals {
    /// By position in [`Mir::functions`].
    functions: Vec<HashSet<LocalId>>,
    /// By position in [`Mir::closures`].
    closures: Vec<HashSet<LocalId>>,
}

impl SharedLocals {
    /// Computes the shared locals of every function and closure body.
    pub(super) fn of(mir: &Mir) -> Self {
        let writes = capture_writes(&mir.closures);
        let functions = mir
            .functions
            .iter()
            .map(|function| written_by_created_closures(&function.blocks, &mir.closures, &writes))
            .collect();
        let closures = mir
            .closures
            .iter()
            .map(|closure| {
                let mut shared = written_by_created_closures(&closure.blocks, &mir.closures, &writes);
                shared.extend(closure.captures.iter().filter_map(|capture| capture.target_local));
                shared
            })
            .collect();
        Self { functions, closures }
    }

    /// The shared locals of the function at `index`.
    pub(super) fn of_function(&self, index: usize) -> Option<&HashSet<LocalId>> {
        self.functions.get(index)
    }

    /// The shared locals of the closure at `index`.
    pub(super) fn of_closure(&self, index: usize) -> Option<&HashSet<LocalId>> {
        self.closures.get(index)
    }
}

/// Per closure, the capture targets that closure's call can write: defined in
/// its own body, or written by a closure it creates. Runs to a fixpoint because
/// a write can sit any number of closures deep.
fn capture_writes(closures: &[MirClosure]) -> Vec<HashSet<LocalId>> {
    let mut writes: Vec<HashSet<LocalId>> = closures
        .iter()
        .map(|closure| {
            let defined = closure.body().assignment_counts();
            closure
                .captures
                .iter()
                .filter_map(|capture| capture.target_local)
                .filter(|target| defined.contains_key(target))
                .collect()
        })
        .collect();
    loop {
        let mut changed = false;
        for (index, closure) in closures.iter().enumerate() {
            let nested = written_by_created_closures(&closure.blocks, closures, &writes);
            let Some(written) = writes.get_mut(index) else {
                continue;
            };
            for capture in &closure.captures {
                if let Some(target) = capture.target_local
                    && nested.contains(&target)
                {
                    changed |= written.insert(target);
                }
            }
        }
        if !changed {
            return writes;
        }
    }
}

/// The locals of the body made of `blocks` that a closure created there writes.
fn written_by_created_closures(
    blocks: &[BasicBlock],
    closures: &[MirClosure],
    writes: &[HashSet<LocalId>],
) -> HashSet<LocalId> {
    let mut shared = HashSet::new();
    for id in created_closures(blocks) {
        let index = usize::try_from(id.0).unwrap_or(usize::MAX);
        let (Some(closure), Some(written)) = (closures.get(index), writes.get(index)) else {
            continue;
        };
        for capture in &closure.captures {
            if capture.target_local.is_some_and(|target| written.contains(&target)) {
                shared.insert(capture.source_local);
            }
        }
    }
    shared
}

/// Every closure the statements of `blocks` construct.
fn created_closures(blocks: &[BasicBlock]) -> impl Iterator<Item = ClosureId> + '_ {
    blocks.iter().flat_map(|block| &block.statements).filter_map(|statement| match statement {
        Statement::Assign { value: Rvalue::Closure { id, .. }, .. }
        | Statement::AssignPlace { value: Rvalue::Closure { id, .. }, .. } => Some(*id),
        Statement::Assign { .. } | Statement::AssignPlace { .. } | Statement::DictEntryUpdate { .. } => None,
    })
}
