//! The block where a loop's early exits rejoin its normal exit.
//!
//! A loop body can leave the loop other than through the loop's own exit
//! block `E`: a throwing call in the body whose `catch` (or `finally`'s
//! exceptional entry) lies after the loop. `for (const x of gen()) { .. }` is
//! `try { while (!step.done) { ..; step = it.next() } } finally { close }`: the
//! `next()` call's handler raises the `finally`'s pending flag and jumps to the
//! clause, which the loop's normal exit `E` (an empty forward block) also
//! reaches. A walker that only knows `continue` and `break` emits such an edge
//! by inlining its target, the clause and everything after it, in each arm
//! that reaches it (the `Err` and the panic arm of the call's `match`), so
//! every further loop in the function multiplied the rest of the function: `n`
//! sequential `for..of` loops over generators rendered `4^n` copies of the
//! tail and took exponential time.
//!
//! The rejoin point `J` is the nearest block on `E`'s post-dominator chain
//! (over the paths that do not `throw`, see `post_dominance`) that every early
//! exit's own post-dominator chain reaches. When it lies past `E`, the loop is
//! emitted inside a labelled block that also holds the region from `E` up to
//! `J`:
//!
//! ```text
//! 'smelt_exit_N: {
//!     loop { ..; match next() { Ok(..) => continue, Err(..) => { ..; break 'smelt_exit_N; } } }
//!     <E up to J>
//! }
//! <J>
//! ```
//!
//! An edge from inside the loop to `J` leaves the block (`LoopJump::BreakToExitJoin`),
//! the loop's normal exit runs `E..J` and falls out of the block, and `J` is
//! emitted once. Behavior is unchanged: each path runs the same blocks in the
//! same order as before, only the textual copies of `J` onward are shared.

use super::*;

impl FunctionEmitter<'_> {
    /// The natural loop headed by `header`: the blocks that `header` reaches
    /// and that reach `header` again without passing through it.
    ///
    /// Neither walk crosses `enclosing` (the open enclosing loops' headers,
    /// continue targets and exits, and the region's end): going round an
    /// enclosing loop, every block of its body reaches `header`.
    fn natural_loop_blocks(
        &self,
        header: smelt_mir::BlockId,
        enclosing: &[smelt_mir::BlockId],
    ) -> BlockIdSet {
        let mut forward = BlockIdSet::default();
        let mut pending = vec![header];
        let mut predecessors: rustc_hash::FxHashMap<smelt_mir::BlockId, Vec<smelt_mir::BlockId>> =
            rustc_hash::FxHashMap::default();
        while let Some(block_id) = pending.pop() {
            if !forward.insert(block_id) {
                continue;
            }
            let Some(terminator) = self
                .block(block_id)
                .ok()
                .and_then(|b| b.terminator.as_ref())
            else {
                continue;
            };
            for successor in control_flow_successors(terminator) {
                if enclosing.contains(&successor) {
                    continue;
                }
                predecessors.entry(successor).or_default().push(block_id);
                pending.push(successor);
            }
        }
        let mut body = BlockIdSet::default();
        body.insert(header);
        let mut backward = predecessors.get(&header).cloned().unwrap_or_default();
        while let Some(block_id) = backward.pop() {
            if !body.insert(block_id) {
                continue;
            }
            if let Some(preds) = predecessors.get(&block_id) {
                backward.extend(preds.iter().copied());
            }
        }
        body
    }

    /// The exit join of the loop headed by `header` whose structural exit is
    /// `exit_block`, emitted in a region that ends at `region_stop` (`None`
    /// at function level); see the module docs.
    ///
    /// `None` keeps the plain loop: no early exit, every early exit already
    /// rejoins at `exit_block` itself (where it renders `break`), no common
    /// rejoin point inside the region, or a call result that the code from
    /// the join on would read outside the block that binds it.
    pub(super) fn loop_exit_join(
        &self,
        header: smelt_mir::BlockId,
        exit_block: smelt_mir::BlockId,
        region_stop: Option<smelt_mir::BlockId>,
    ) -> Option<smelt_mir::BlockId> {
        // Blocks the walkers already render as jumps; the join must lie
        // before them, and an exit to one of them is that jump, not an arm.
        let mut boundaries = self.flow.open_loop_boundaries();
        boundaries.extend(region_stop);
        let body = self.natural_loop_blocks(header, &boundaries);
        if body.contains(&exit_block) {
            return None;
        }
        boundaries.push(header);
        let exits = self.loop_exits(&body, &boundaries);
        if !exits.iter().any(|exit| *exit != exit_block) {
            return None;
        }
        let (join, index, _) = self.common_exit_join(exit_block, &exits, &body, &boundaries)?;
        (index > 0 && !self.region_result_read_after(&[header], join, &boundaries)).then_some(join)
    }

    /// The exit join of a loop in a closure body, headed by `header` and
    /// emitted on the path `active` up to `region_stop`.
    ///
    /// A closure loop has no `break` target: each exit, its normal one
    /// included, is emitted inside the loop up to the region's end. With two
    /// or more exits that rejoin, each stops at the join with a labelled
    /// `break` of the loop instead, and the join follows the loop once.
    pub(in crate::emitter) fn closure_loop_exit_join(
        &self,
        header: smelt_mir::BlockId,
        active: &[smelt_mir::BlockId],
        region_stop: Option<smelt_mir::BlockId>,
    ) -> Option<smelt_mir::BlockId> {
        let mut boundaries = active.to_vec();
        boundaries.extend(region_stop);
        let body = self.natural_loop_blocks(header, &boundaries);
        boundaries.push(header);
        let exits = self.loop_exits(&body, &boundaries);
        // Any exit's chain will do as the base, as long as it is one the
        // others rejoin: an exit that returns on every path has a chain no
        // other exit meets.
        let join = exits.iter().find_map(|base| {
            self.common_exit_join(*base, &exits, &body, &boundaries)
                .filter(|(_, _, rejoined)| *rejoined >= 2)
                .map(|(join, _, _)| join)
        })?;
        (!self.region_result_read_after(&[header], join, &boundaries)).then_some(join)
    }

    /// The blocks outside `body` that its edges lead to, minus `boundaries`
    /// (edges the walkers render as jumps), in block order.
    fn loop_exits(
        &self,
        body: &BlockIdSet,
        boundaries: &[smelt_mir::BlockId],
    ) -> Vec<smelt_mir::BlockId> {
        let mut exits: Vec<smelt_mir::BlockId> = Vec::new();
        for block_id in body {
            let Some(terminator) = self
                .block(*block_id)
                .ok()
                .and_then(|b| b.terminator.as_ref())
            else {
                continue;
            };
            for successor in control_flow_successors(terminator) {
                if !body.contains(&successor)
                    && !boundaries.contains(&successor)
                    && !exits.contains(&successor)
                {
                    exits.push(successor);
                }
            }
        }
        exits.sort_by_key(|block| block.0);
        exits
    }

    /// The nearest block on `base`'s post-dominator chain (cut at the first
    /// boundary or loop block) that the chain of every exit meets, with its
    /// index on that chain and the number of exits that meet it.
    ///
    /// An exit that never meets the chain is ignored when it cannot reach it
    /// at all (it returns or throws on every path, and is emitted in full
    /// where it is reached); one that meets it only on some paths has no
    /// rejoin point, and neither has the loop.
    fn common_exit_join(
        &self,
        base: smelt_mir::BlockId,
        exits: &[smelt_mir::BlockId],
        body: &BlockIdSet,
        boundaries: &[smelt_mir::BlockId],
    ) -> Option<(smelt_mir::BlockId, usize, usize)> {
        let mut chain = vec![base];
        let mut next = self.immediate_post_dominator_past_throws(base);
        while let Some(block_id) = next {
            if boundaries.contains(&block_id)
                || body.contains(&block_id)
                || chain.contains(&block_id)
            {
                break;
            }
            chain.push(block_id);
            next = self.immediate_post_dominator_past_throws(block_id);
        }
        let mut furthest = 0;
        let mut rejoined = 0_usize;
        for exit in exits {
            let mut seen = BlockIdSet::default();
            let mut step = Some(*exit);
            let mut hit = None;
            while let Some(block_id) = step {
                if let Some(index) = chain.iter().position(|member| *member == block_id) {
                    hit = Some(index);
                    break;
                }
                if !seen.insert(block_id) {
                    break;
                }
                step = self.immediate_post_dominator_past_throws(block_id);
            }
            match hit {
                Some(index) => {
                    furthest = furthest.max(index);
                    rejoined = rejoined.saturating_add(1);
                }
                None => {
                    if chain.iter().any(|member| {
                        self.block_reaches_target_avoiding(
                            *exit,
                            *member,
                            boundaries,
                            &mut BlockIdSet::default(),
                        )
                    }) {
                        return None;
                    }
                }
            }
        }
        chain.get(furthest).map(|join| (*join, furthest, rejoined))
    }

    /// Whether the code from `join` on reads the result of a call or `await`,
    /// or the `catch` binding of one, in the region from `starts` up to `join`
    /// (a loop and its exit region, or the arms of a branch).
    ///
    /// Such a result is bound (`let`) inside the labelled block or arm where
    /// its call is emitted, and would be out of scope at the join after it.
    pub(in crate::emitter) fn region_result_read_after(
        &self,
        starts: &[smelt_mir::BlockId],
        join: smelt_mir::BlockId,
        boundaries: &[smelt_mir::BlockId],
    ) -> bool {
        let mut results = Vec::new();
        let mut pending = starts.to_vec();
        let mut visited = BlockIdSet::default();
        while let Some(block_id) = pending.pop() {
            if block_id == join || !visited.insert(block_id) {
                continue;
            }
            let Some(terminator) = self
                .block(block_id)
                .ok()
                .and_then(|b| b.terminator.as_ref())
            else {
                continue;
            };
            if let Terminator::Call { dest, unwind, .. } | Terminator::Await { dest, unwind, .. } =
                terminator
            {
                results.push(*dest);
                // A `catch` binding is bound (`let`) in the call's `Err` arm.
                results.extend(unwind.and_then(|handler| handler.exception_local));
            }
            pending.extend(
                control_flow_successors(terminator)
                    .into_iter()
                    .filter(|successor| *successor == join || !boundaries.contains(successor)),
            );
        }
        if results.is_empty() {
            return false;
        }
        let mut after = vec![join];
        let mut read_visited = BlockIdSet::default();
        while let Some(block_id) = after.pop() {
            if boundaries.contains(&block_id) || !read_visited.insert(block_id) {
                continue;
            }
            let Ok(block) = self.block(block_id) else {
                continue;
            };
            let mut reads = false;
            for statement in &block.statements {
                statement.read_locals(|read| reads |= results.contains(&read));
            }
            if let Some(terminator) = &block.terminator {
                terminator.read_locals(|read| reads |= results.contains(&read));
                after.extend(control_flow_successors(terminator));
            }
            if reads {
                return true;
            }
        }
        false
    }

    /// After a loop whose [`OpenLoopScope::close`](super::flow_state::OpenLoopScope::close)
    /// answered `used_join`: emit the region from the loop's `exit_block` up to the
    /// join inside the still-open labelled block, close the block, and answer
    /// the block emission continues at (`join`, or `exit_block` for a loop
    /// whose exit join no edge used).
    pub(super) fn emit_loop_exit_region(
        &self,
        used_join: Option<smelt_mir::BlockId>,
        exit_block: smelt_mir::BlockId,
        out: &mut String,
    ) -> Result<smelt_mir::BlockId, EmitError> {
        let Some(join) = used_join else {
            return Ok(exit_block);
        };
        let region_declared = self.declared_scope();
        self.emit_block_until_goto(self.block(exit_block)?, join, RegionExit::Join, out)?;
        out.push_str("    }\n");
        region_declared.close();
        Ok(join)
    }
}
