//! Path questions over the function's MIR control-flow graph: reachability,
//! dominance, divergence and where two branches rejoin.
//!
//! Pure queries with no emission. The region walker, the loop recognizers and
//! the switch emitter all ask them, so they sit below those emitters and never
//! call back up into them.

use super::*;

impl FunctionEmitter<'_> {
    /// Returns whether a control-flow path can reach `target`.
    pub(in crate::emitter) fn block_reaches_target(
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
        let Some(block) = self.block(block_id).ok() else {
            return false;
        };
        let Some(terminator) = &block.terminator else {
            return false;
        };
        control_flow_successors(terminator)
            .into_iter()
            .any(|successor| self.block_reaches_target(successor, target, visited))
    }

    /// Returns whether `block_id` reaches `target` without crossing avoided blocks.
    pub(in crate::emitter) fn block_reaches_target_avoiding(
        &self,
        block_id: smelt_mir::BlockId,
        target: smelt_mir::BlockId,
        avoid: &[smelt_mir::BlockId],
        visited: &mut BlockIdSet,
    ) -> bool {
        if avoid.contains(&block_id) {
            return false;
        }
        if block_id == target {
            return true;
        }
        if !visited.insert(block_id) {
            return false;
        }
        let Some(block) = self.block(block_id).ok() else {
            return false;
        };
        let Some(terminator) = &block.terminator else {
            return false;
        };
        control_flow_successors(terminator)
            .into_iter()
            .any(|successor| self.block_reaches_target_avoiding(successor, target, avoid, visited))
    }

    /// Returns whether `from` returns to `header` along a genuine BACK EDGE —
    /// a path that stays inside `header`'s own region, never passing through
    /// one of `header`'s strict dominators.
    ///
    /// Plain reachability is not enough: every block inside an OUTER loop
    /// reaches every other one by going around that loop. The `if` inside
    /// `for (..) { if (c) { for (..) {..} } }` reaches itself through the outer
    /// latch and header, so it was mistaken for a `while` header, the inner
    /// loop's preheader was emitted inside a spurious `loop {}`, and the inner
    /// loop's `continue` re-ran that preheader forever (Hono's trie-router
    /// `#pushHandlerSets` hang). Returning through a strict dominator means
    /// the path LEFT the region and came back in through its entry — no loop.
    pub(super) fn is_back_edge_target(
        &self,
        from: smelt_mir::BlockId,
        header: smelt_mir::BlockId,
    ) -> bool {
        let dominators = self.strict_dominators(header);
        self.block_reaches_target_avoiding(from, header, &dominators, &mut BlockIdSet::default())
    }

    /// Returns the blocks that strictly dominate `header`.
    ///
    /// A block `d` strictly dominates `header` when `d != header` and every path
    /// from the function entry to `header` passes through `d`; equivalently, the
    /// entry cannot reach `header` while avoiding `d`. Used by
    /// [`Self::compound_while`] to distinguish a real loop back-edge (which
    /// re-enters the header from inside the loop, avoiding every strict
    /// dominator) from an enclosing loop's edge that merely flows forward into
    /// the header through one of its dominators.
    pub(super) fn strict_dominators(&self, header: smelt_mir::BlockId) -> Vec<smelt_mir::BlockId> {
        self.function
            .blocks
            .iter()
            .map(|candidate| candidate.id)
            .filter(|&candidate| {
                candidate != header
                    && !self.block_reaches_target_avoiding(
                        self.function.entry,
                        header,
                        &[candidate],
                        &mut BlockIdSet::default(),
                    )
            })
            .collect()
    }

    /// Returns whether every straight-line successor from `block_id` ends in a
    /// return, throw, or unreachable terminator before it can fall through.
    pub(in crate::emitter) fn block_eventually_terminates(
        &self,
        block_id: smelt_mir::BlockId,
        visiting: &mut BlockIdSet,
    ) -> Result<bool, EmitError> {
        if let Some(result) = self
            .caches
            .termination_cache
            .borrow()
            .get(&block_id)
            .copied()
        {
            return Ok(result);
        }
        if !visiting.insert(block_id) {
            return Ok(false);
        }

        let block = self.block(block_id)?;
        let result = match &block.terminator {
            Some(Terminator::Return(_) | Terminator::Throw(_)) => true,
            Some(Terminator::Goto(target)) => {
                self.block_eventually_terminates(*target, visiting)?
            }
            Some(Terminator::Call { target, .. } | Terminator::Await { target, .. }) => {
                self.block_eventually_terminates(*target, visiting)?
            }
            Some(Terminator::Switch {
                then_block,
                else_block,
                ..
            }) => {
                self.block_eventually_terminates(*then_block, visiting)?
                    && self.block_eventually_terminates(*else_block, visiting)?
            }
            Some(Terminator::Match { arms, default, .. }) => {
                let default_terminates = if let Some(target) = default {
                    self.block_eventually_terminates(*target, visiting)?
                } else {
                    false
                };
                default_terminates
                    && arms.iter().try_fold(true, |all_terminate, arm| {
                        Ok::<bool, EmitError>(
                            all_terminate
                                && self.block_eventually_terminates(arm.target, visiting)?,
                        )
                    })?
            }
            None => false,
        };

        visiting.remove(&block_id);
        self.caches
            .termination_cache
            .borrow_mut()
            .insert(block_id, result);
        Ok(result)
    }

    /// Find where the arms of a branch inside a forward join region (the two
    /// of a switch, or every target of a `match`) meet again, before (or at)
    /// the region's own `stop`.
    ///
    /// The answer is the branch's immediate post-dominator within the region:
    /// the block `J` every non-diverging path of each arm reaches before
    /// `stop`, chosen as the NEAREST such block (every other candidate is
    /// reached from it). An arm that diverges before `stop` (returns/throws on
    /// every path) places no constraint, so `if (b) return; rest` joins at
    /// `rest`. `stop` itself is always a candidate — the region ends there.
    ///
    /// `None` when both arms diverge (there is nothing to rejoin, the plain
    /// switch emission is right) or when an arm can cycle back to the switch
    /// block itself: that is a loop edge this forward-region rule does not
    /// own, and the existing emission keeps handling it.
    pub(super) fn join_region_branch_join(
        &self,
        switch_block: smelt_mir::BlockId,
        arms: &[smelt_mir::BlockId],
        stop: smelt_mir::BlockId,
    ) -> Result<Option<smelt_mir::BlockId>, EmitError> {
        let mut continuing = Vec::new();
        for arm in arms.iter().copied() {
            if self.block_reaches_target_avoiding(
                arm,
                switch_block,
                &[stop],
                &mut BlockIdSet::default(),
            ) {
                return Ok(None);
            }
            if !self.block_diverges_before(arm, stop, &mut BlockIdSet::default())? {
                continuing.push(arm);
            }
        }
        if continuing.is_empty() {
            return Ok(None);
        }
        // Candidates: every block a continuing arm reaches without passing
        // `stop`, plus `stop`.
        let mut candidates = vec![stop];
        let mut seen = BlockIdSet::default();
        let mut work = continuing.clone();
        while let Some(block_id) = work.pop() {
            if block_id == stop || !seen.insert(block_id) {
                continue;
            }
            candidates.push(block_id);
            if let Some(terminator) = &self.block(block_id)?.terminator {
                work.extend(control_flow_successors(terminator));
            }
        }
        let post_dominates = |candidate: smelt_mir::BlockId| {
            candidate == stop
                || continuing.iter().all(|arm| {
                    self.block_reaches_target_avoiding(
                        *arm,
                        candidate,
                        &[stop],
                        &mut BlockIdSet::default(),
                    ) && !self.block_reaches_target_avoiding(
                        *arm,
                        stop,
                        &[candidate],
                        &mut BlockIdSet::default(),
                    )
                })
        };
        let valid = candidates
            .into_iter()
            .filter(|candidate| post_dominates(*candidate))
            .collect::<Vec<_>>();
        Ok(valid.iter().copied().find(|candidate| {
            valid.iter().all(|other| {
                other == candidate
                    || self.block_reaches_target_avoiding(
                        *candidate,
                        *other,
                        &[],
                        &mut BlockIdSet::default(),
                    )
            })
        }))
    }

    /// Return whether every path from `block_id` returns, throws or is
    /// unreachable WITHOUT first reaching `stop`.
    ///
    /// [`Self::block_eventually_terminates`] asks the same question of the
    /// whole remaining function, so an arm that merely continues to a join
    /// whose own continuation ends in `return` scores as terminating. Inside a
    /// region bounded by `stop` that is the wrong answer: reaching `stop` is
    /// falling through. A cycle is not proven to terminate, so it answers
    /// `false`.
    pub(super) fn block_diverges_before(
        &self,
        block_id: smelt_mir::BlockId,
        stop: smelt_mir::BlockId,
        visiting: &mut BlockIdSet,
    ) -> Result<bool, EmitError> {
        if block_id == stop || !visiting.insert(block_id) {
            return Ok(false);
        }
        let block = self.block(block_id)?;
        let result = match &block.terminator {
            Some(Terminator::Return(_) | Terminator::Throw(_)) => true,
            Some(Terminator::Goto(target)) => {
                self.block_diverges_before(*target, stop, visiting)?
            }
            // A call with an unwind edge forks the region; both successors
            // must diverge.
            Some(terminator @ (Terminator::Call { .. } | Terminator::Await { .. })) => {
                let mut all = true;
                for next in control_flow_successors(terminator) {
                    all = all && self.block_diverges_before(next, stop, visiting)?;
                }
                all
            }
            Some(Terminator::Switch {
                then_block,
                else_block,
                ..
            }) => {
                self.block_diverges_before(*then_block, stop, visiting)?
                    && self.block_diverges_before(*else_block, stop, visiting)?
            }
            Some(Terminator::Match { arms, default, .. }) => {
                let mut all = match default {
                    Some(target) => self.block_diverges_before(*target, stop, visiting)?,
                    None => false,
                };
                for arm in arms {
                    all = all && self.block_diverges_before(arm.target, stop, visiting)?;
                }
                all
            }
            None => false,
        };
        visiting.remove(&block_id);
        Ok(result)
    }

    /// Return true when no path out of `block_id` ever FALLS OUT of the region
    /// it starts — every path either returns, throws, or loops forever.
    ///
    /// This is [`Self::block_eventually_terminates`] with one difference, and
    /// the difference is the whole point: a back edge answers `true` here
    /// instead of `false`. `block_eventually_terminates` asks "does control
    /// reach a `return`?", and for that question a cycle it cannot prove exits
    /// must answer `false`. The question asked HERE is "does control escape this
    /// region and rejoin a continuation?", and a path that goes round a loop has
    /// not escaped — it is still inside the region, so it says nothing either
    /// way and must not veto the answer.
    ///
    /// That distinction matters because `block_eventually_terminates` requires
    /// BOTH arms of a `Switch` to terminate, so a single loop anywhere in a
    /// region — a loop header IS a `Switch` whose body edge cycles back —
    /// poisons the whole region to `false`. A `for` loop that ends in `return`
    /// is exactly that shape, and it is the ordinary way to write an early-exit
    /// branch.
    ///
    /// A region with a genuinely infinite loop also answers `true`, which is
    /// correct for this question: control never reaches the continuation either
    /// way.
    pub(super) fn block_never_falls_through(
        &self,
        block_id: smelt_mir::BlockId,
        visiting: &mut BlockIdSet,
    ) -> Result<bool, EmitError> {
        // A back edge: this path is still inside the region, so it neither
        // escapes nor terminates. Neutral, which for an `&&`-fold is `true`.
        if !visiting.insert(block_id) {
            return Ok(true);
        }
        let block = self.block(block_id)?;
        let result = match &block.terminator {
            Some(Terminator::Return(_) | Terminator::Throw(_)) => true,
            Some(
                Terminator::Goto(target)
                | Terminator::Call { target, .. }
                | Terminator::Await { target, .. },
            ) => self.block_never_falls_through(*target, visiting)?,
            Some(Terminator::Switch {
                then_block,
                else_block,
                ..
            }) => {
                self.block_never_falls_through(*then_block, visiting)?
                    && self.block_never_falls_through(*else_block, visiting)?
            }
            Some(Terminator::Match { arms, default, .. }) => {
                let default_escapes = match default {
                    Some(target) => self.block_never_falls_through(*target, visiting)?,
                    None => false,
                };
                default_escapes
                    && arms.iter().try_fold(true, |all, arm| {
                        Ok::<bool, EmitError>(
                            all && self.block_never_falls_through(arm.target, visiting)?,
                        )
                    })?
            }
            // No terminator: control runs off the end of the block into the
            // continuation, which IS falling through.
            None => false,
        };
        visiting.remove(&block_id);
        Ok(result)
    }
}
