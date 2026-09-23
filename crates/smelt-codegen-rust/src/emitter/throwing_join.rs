//! Shared-join analysis for throwing terminators (`try`/`catch` around a call or `await`).
//!
//! A throwing `Call`/`Await` terminator is emitted as a Rust `match` with one
//! arm per outcome (`Ok`, the Smelt error channel, and the panic route). Each
//! arm used to emit its successor through the enclosing continuation, which
//! re-emits EVERYTHING after the `try`/`catch` inside every arm. Sequential
//! `try`/`catch` statements (or sequential `expect(() => ..).toThrow()`
//! assertions, which lower to one) therefore multiplied: N of them emitted
//! 3^N copies of the tail — Hono's `utils/cookie.test.ts` (12 assertions)
//! reached a 616 MB module that OOM-killed rustc.
//!
//! The general rule implemented here: when the normal and the catch successor
//! converge on one block that every path out of the `try` region reaches (or
//! diverges before reaching), that block is a JOIN. The arms are emitted as
//! forward regions ending at the join, and the join — the rest of the body —
//! is emitted exactly once after the `match`. This is the same rule the
//! `if`/`else` and `match` emitters already follow for their own arms.

use super::control_flow::Continuation;
use super::core::{block_reads_local, terminator_successors};
use super::*;

/// Upper bound on join candidates examined for one throwing terminator.
///
/// Candidates are visited nearest-first, so the real join is found within the
/// first few; the cap only bounds the quadratic worst case on a huge function
/// with no join at all (the terminator then keeps the per-arm emission).
const MAX_JOIN_CANDIDATES: usize = 64;

impl FunctionEmitter<'_> {
    /// Finds the block where the forked paths starting at `starts` rejoin.
    ///
    /// `starts` are the successors of one fork: a throwing terminator's catch
    /// block and normal target, or a branch's arms. `defined` are the locals
    /// the fork itself binds inside its arms (a call destination, a catch
    /// binding). `exits` are blocks that end the enclosing structured region
    /// (the region's stop block, a loop's header and exit): a path that
    /// reaches one of them before the join leaves the region the arms are
    /// emitted in, so that candidate is rejected.
    ///
    /// Returns `Some(join)` only when
    /// * `join` is reachable from every start,
    /// * every path from each start reaches `join`, or diverges
    ///   (`return`/`throw`/`unreachable`) at a block one of the starts
    ///   DOMINATES — a divergence inside an arm, such as a catch arm that
    ///   rethrows or a failed assertion's `throw` (the normal arm reaches the
    ///   catch block too, through nested unwinds, so "an arm" is any start), and
    /// * no local bound inside the arms (the fork's own bindings and every
    ///   call/`await` destination or catch binding in the region) is read from
    ///   the join onwards — such a binding is scoped to its Rust arm and would
    ///   not be visible after it.
    ///
    /// The dominance condition is what keeps the function's own tail from
    /// fooling the search. Every path eventually diverges at the final
    /// `return`, so "reaches or diverges" alone accepts nearly any candidate:
    /// a `try`'s own catch block (leaving the whole tail inside the normal
    /// arm), or a failure `throw` leaf past the real join (leaving the join
    /// and everything after it inside both arms). The shared tail is reachable
    /// from the function entry without passing through either arm, so its
    /// `return`/`throw` is dominated by neither start and does not count.
    ///
    /// Candidates are visited breadth-first from the first start, so the
    /// nearest join wins. `None` keeps per-arm emission of the whole tail.
    pub(super) fn forked_region_join(
        &self,
        starts: &[smelt_mir::BlockId],
        defined: &[LocalId],
        exits: &[smelt_mir::BlockId],
    ) -> Result<Option<smelt_mir::BlockId>, EmitError> {
        let Some((&first, rest)) = starts.split_first() else {
            return Ok(None);
        };
        let reach = rest
            .iter()
            .map(|start| self.forward_reachable(*start))
            .collect::<Result<Vec<_>, _>>()?;
        let bypasses = starts
            .iter()
            .map(|start| self.reachable_from_entry_avoiding(*start))
            .collect::<Result<Vec<_>, _>>()?;
        let mut queue = std::collections::VecDeque::from([first]);
        let mut seen = BlockIdSet::default();
        let mut examined = 0usize;
        while let Some(candidate) = queue.pop_front() {
            if !seen.insert(candidate) || exits.contains(&candidate) {
                continue;
            }
            if reach.iter().all(|set| set.contains(&candidate)) {
                examined = examined.saturating_add(1);
                if examined > MAX_JOIN_CANDIDATES {
                    return Ok(None);
                }
                let mut all_reach = true;
                for start in starts {
                    if !self.all_paths_reach_or_diverge(
                        *start,
                        candidate,
                        exits,
                        &bypasses,
                        &mut BlockIdSet::default(),
                    )? {
                        all_reach = false;
                        break;
                    }
                }
                if all_reach {
                    return Ok(self
                        .arm_bindings_stay_in_arms(starts, candidate, defined)?
                        .then_some(candidate));
                }
            }
            if let Some(terminator) = &self.block(candidate)?.terminator {
                queue.extend(terminator_successors(terminator));
            }
        }
        Ok(None)
    }

    /// Returns the blocks reachable from the function entry without entering `avoid`.
    ///
    /// A block missing from this set is dominated by `avoid`: every path from
    /// the entry to it passes through `avoid`.
    fn reachable_from_entry_avoiding(
        &self,
        avoid: smelt_mir::BlockId,
    ) -> Result<BlockIdSet, EmitError> {
        let mut seen = BlockIdSet::default();
        let mut stack = vec![self.function.entry];
        while let Some(block_id) = stack.pop() {
            if block_id == avoid || !seen.insert(block_id) {
                continue;
            }
            if let Some(terminator) = &self.block(block_id)?.terminator {
                stack.extend(terminator_successors(terminator));
            }
        }
        Ok(seen)
    }

    /// Returns every block reachable from `start`, `start` included.
    fn forward_reachable(&self, start: smelt_mir::BlockId) -> Result<BlockIdSet, EmitError> {
        let mut seen = BlockIdSet::default();
        let mut stack = vec![start];
        while let Some(block_id) = stack.pop() {
            if !seen.insert(block_id) {
                continue;
            }
            if let Some(terminator) = &self.block(block_id)?.terminator {
                stack.extend(terminator_successors(terminator));
            }
        }
        Ok(seen)
    }

    /// Returns whether every path from `block_id` reaches `join`, or diverges
    /// at a block missing from one of `bypasses` (a block some start
    /// dominates; see [`Self::reachable_from_entry_avoiding`]).
    ///
    /// A revisited block is neutral (`true`): either it is a back edge inside
    /// the region, which has not left it, or it was already proven on another
    /// path — the first failing path short-circuits the whole query, so a block
    /// seen earlier never hides a failure. Reaching one of `exits`, or a block
    /// with no terminator, is a path that escapes without meeting `join`.
    fn all_paths_reach_or_diverge(
        &self,
        block_id: smelt_mir::BlockId,
        join: smelt_mir::BlockId,
        exits: &[smelt_mir::BlockId],
        bypasses: &[BlockIdSet],
        visited: &mut BlockIdSet,
    ) -> Result<bool, EmitError> {
        if block_id == join {
            return Ok(true);
        }
        if exits.contains(&block_id) {
            return Ok(false);
        }
        if !visited.insert(block_id) {
            return Ok(true);
        }
        let Some(terminator) = &self.block(block_id)?.terminator else {
            return Ok(false);
        };
        if matches!(
            terminator,
            Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable
        ) {
            return Ok(bypasses.iter().any(|bypass| !bypass.contains(&block_id)));
        }
        for successor in terminator_successors(terminator) {
            if !self.all_paths_reach_or_diverge(successor, join, exits, bypasses, visited)? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Returns whether every local bound inside the arms is dead from `join` on.
    ///
    /// The arms bind the terminator's own `defined` locals and, inside the
    /// region between the successors and `join`, every nested call/`await`
    /// destination and catch binding with an arm-scoped `let`. Statement
    /// destinations are not listed: in a multi-block function they are
    /// predeclared at function scope (see `predeclared_locals_for_function`),
    /// so they stay visible after the `match`.
    fn arm_bindings_stay_in_arms(
        &self,
        starts: &[smelt_mir::BlockId],
        join: smelt_mir::BlockId,
        defined: &[LocalId],
    ) -> Result<bool, EmitError> {
        let mut bound = defined.to_vec();
        let mut region = BlockIdSet::default();
        let mut stack = starts.to_vec();
        while let Some(block_id) = stack.pop() {
            if block_id == join || !region.insert(block_id) {
                continue;
            }
            let block = self.block(block_id)?;
            if let Some(
                Terminator::Call { dest, unwind, .. } | Terminator::Await { dest, unwind, .. },
            ) = &block.terminator
            {
                bound.push(*dest);
                if let Some(local) = unwind.as_ref().and_then(|handler| handler.exception_local) {
                    bound.push(local);
                }
            }
            for statement in &block.statements {
                if let Statement::Assign { dest, .. } = statement
                    && !self.predeclared_locals.contains(dest)
                {
                    bound.push(*dest);
                }
            }
            if let Some(terminator) = &block.terminator {
                stack.extend(terminator_successors(terminator));
            }
        }
        let after = self.forward_reachable(join)?;
        for block_id in after {
            let block = self.block(block_id)?;
            if bound.iter().any(|local| block_reads_local(block, *local)) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Chooses the join for a throwing terminator emitted under `continuation`.
    ///
    /// Translates the enclosing continuation into the blocks that end its
    /// region (see [`Self::forked_region_join`]): nothing for a plain
    /// block, the stop block for a forward region, the header and exit for a
    /// loop body. Closure bodies keep per-arm emission: their blocks are
    /// rendered by the closure emitter, whose tail-expression `return`
    /// handling a forward region would not reproduce.
    pub(super) fn throwing_join_for(
        &self,
        target: smelt_mir::BlockId,
        handler: smelt_mir::ExceptionHandler,
        dest: LocalId,
        continuation: &Continuation<'_>,
    ) -> Result<Option<smelt_mir::BlockId>, EmitError> {
        let exits: Vec<smelt_mir::BlockId> = match continuation {
            Continuation::Block => Vec::new(),
            Continuation::Region { stop, .. } => vec![*stop],
            Continuation::InLoop {
                continue_target,
                break_target,
                ..
            } => std::iter::once(*continue_target).chain(*break_target).collect(),
            Continuation::Closure { .. } => return Ok(None),
        };
        let defined: Vec<LocalId> = std::iter::once(dest).chain(handler.exception_local).collect();
        self.forked_region_join(&[handler.catch_block, target], &defined, &exits)
    }
}
