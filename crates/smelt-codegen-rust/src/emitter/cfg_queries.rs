//! Control-flow-graph reachability queries over a function's MIR blocks: whether a block can re-execute, sits under a repeating region, or can reach a target block.

use super::*;
use super::core::terminator_successors;

impl FunctionEmitter<'_> {
    /// Return whether control flow from `block_id` can reach `block_id` again.
    pub(super) fn block_can_repeat(
        &self,
        block_id: smelt_mir::BlockId,
        visited: &mut BlockIdSet,
    ) -> bool {
        let Some(block) = self.block(block_id).ok() else {
            return true;
        };
        let Some(terminator) = &block.terminator else {
            return false;
        };
        visited.insert(block_id);
        terminator_successors(terminator)
            .into_iter()
            .any(|successor| {
                successor == block_id
                    || self.block_can_reach(successor, block_id, &mut visited.clone())
            })
    }

    /// Return whether `block_id` is emitted under a structured Rust loop.
    ///
    /// MIR control flow can prove that a branch-local assignment is followed by
    /// a return on every semantic path, but the structured Rust emitter may
    /// still place that assignment textually inside a `loop { ... }`. Rust's
    /// definite-assignment rules reject assigning to an immutable local from a
    /// loop body even when later control flow always exits, so locals assigned
    /// in blocks reached from repeatable regions need mutable bindings.
    pub(super) fn block_is_reached_from_repeating_region(&self, block_id: smelt_mir::BlockId) -> bool {
        self.function
            .blocks
            .iter()
            .filter(|candidate| candidate.id != block_id)
            .any(|candidate| {
                self.block_can_repeat(candidate.id, &mut BlockIdSet::default())
                    && self.block_can_reach(candidate.id, block_id, &mut BlockIdSet::default())
            })
    }

    /// Return whether a successor path can reach `target`.
    pub(super) fn block_can_reach(
        &self,
        block_id: smelt_mir::BlockId,
        target: smelt_mir::BlockId,
        visited: &mut BlockIdSet,
    ) -> bool {
        if block_id == target {
            return true;
        }
        if !visited.insert(block_id) {
            return false;
        }
        self.block(block_id)
            .ok()
            .and_then(|block| block.terminator.as_ref())
            .is_some_and(|terminator| {
                terminator_successors(terminator)
                    .into_iter()
                    .any(|next| self.block_can_reach(next, target, visited))
            })
    }

}
