//! Immediate post-dominators of the MIR function being emitted.
//!
//! A two-way branch `switch c ? A : B` rejoins at the block every path out of
//! it must pass through next: its immediate post-dominator. Structured
//! emission renders `if c { A..J } else { B..J }` and then `J` ONCE; an
//! emitter that instead follows each arm to the end of the function renders
//! `J` (and everything after it) once per path, which is exponential in the
//! number of sequential branches.
//!
//! The analysis is the textbook dataflow over the reverse CFG with one
//! virtual exit node that every `return`/`unreachable` block (and any block
//! without a terminator) flows into; a `throw` block has no successor at all,
//! so it constrains no join (an arm that throws leaves through its own
//! `return Err(..)`): `PD(exit) = {exit}` and
//! `PD(n) = {n} ∪ ⋂ PD(s)` over the successors `s` of `n`, iterated to the
//! greatest fixpoint. Post-dominator sets form a chain, so the immediate
//! post-dominator of `n` is the member of `PD(n) \ {n}` with the largest set.
//! A block that cannot reach the exit (an infinite loop, or one that throws
//! on every path) has no post-dominator.
//!
//! Successors are the emitter's own (`control_flow_successors`), so a call's
//! unwind edge into its `catch` block counts: a `try` rejoins after the
//! handler, not after the call.

use super::*;

/// Dense bitset over the blocks of one function plus the virtual exit.
#[derive(Clone, PartialEq, Eq)]
struct BlockSet(Vec<u64>);

#[expect(
    clippy::arithmetic_side_effects,
    clippy::integer_division,
    reason = "word/bit addressing of a bitset: `index / 64` and `index % 64` are in range by construction"
)]
impl BlockSet {
    /// The empty set over `len` nodes.
    fn empty(len: usize) -> Self {
        Self(vec![0; len.div_ceil(64)])
    }

    /// The set of all `len` nodes (bits past `len` stay clear).
    fn full(len: usize) -> Self {
        let mut set = Self(vec![u64::MAX; len.div_ceil(64)]);
        let tail = len % 64;
        if tail != 0
            && let Some(last) = set.0.last_mut()
        {
            *last = (1_u64 << tail) - 1;
        }
        set
    }

    /// Add node `index`.
    fn insert(&mut self, index: usize) {
        if let Some(word) = self.0.get_mut(index / 64) {
            *word |= 1_u64 << (index % 64);
        }
    }

    /// Whether node `index` is a member.
    fn contains(&self, index: usize) -> bool {
        self.0
            .get(index / 64)
            .is_some_and(|word| word & (1_u64 << (index % 64)) != 0)
    }

    /// Keep only the members `other` also has.
    fn intersect(&mut self, other: &Self) {
        for (word, kept) in self.0.iter_mut().zip(&other.0) {
            *word &= *kept;
        }
    }

    /// The number of members.
    fn count(&self) -> u32 {
        self.0.iter().map(|word| word.count_ones()).sum()
    }
}

/// The immediate post-dominator of every block of `function`, indexed by block
/// id (`None`: the virtual exit, or no path to it).
///
/// A `throw` block does not flow into the exit: it
/// has no successor at all, so it constrains no predecessor, and a block whose
/// non-throwing paths all meet at `J` is post-dominated by `J`. That is where a
/// `catch` handler whose unmatched case rethrows rejoins its `try`.
pub(super) fn immediate_post_dominators(function: &MirFunction) -> Vec<Option<smelt_mir::BlockId>> {
    immediate_post_dominators_with(function, |block| {
        matches!(block.terminator, Some(Terminator::Throw(_)))
    })
}

/// The immediate post-dominator of every block of `function`, where a block
/// without successors flows into the virtual exit unless `neutral` holds for
/// it: a neutral block has no successor at all, so it constrains no
/// predecessor (see [`immediate_post_dominators`] for `throw` blocks).
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "every index is a node number below `nodes`, the length of both tables"
)]
fn immediate_post_dominators_with(
    function: &MirFunction,
    neutral: impl Fn(&BasicBlock) -> bool,
) -> Vec<Option<smelt_mir::BlockId>> {
    let blocks = function.blocks.len();
    let exit = blocks;
    let nodes = blocks + 1;
    let successors = function
        .blocks
        .iter()
        .map(|block| {
            let mut next = block
                .terminator
                .as_ref()
                .map(control_flow::control_flow_successors)
                .unwrap_or_default()
                .into_iter()
                .filter_map(|id| usize::try_from(id.0).ok().filter(|index| *index < blocks))
                .collect::<Vec<_>>();
            if next.is_empty() && !neutral(block) {
                next.push(exit);
            }
            next
        })
        .collect::<Vec<_>>();
    let mut post = (0..nodes)
        .map(|node| {
            if node == exit {
                let mut set = BlockSet::empty(nodes);
                set.insert(exit);
                set
            } else {
                BlockSet::full(nodes)
            }
        })
        .collect::<Vec<_>>();
    let mut changed = true;
    while changed {
        changed = false;
        // Reverse block order converges fastest for a forward-ordered CFG.
        for node in (0..blocks).rev() {
            let mut meet = BlockSet::full(nodes);
            for successor in &successors[node] {
                meet.intersect(&post[*successor]);
            }
            meet.insert(node);
            if meet != post[node] {
                post[node] = meet;
                changed = true;
            }
        }
    }
    // The blocks with a path to the exit. One without (an infinite loop, or
    // every path ends in a neutral block) keeps the full set, which holds the
    // exit too, so the set cannot answer this.
    let mut reaches_exit = vec![false; nodes];
    reaches_exit[exit] = true;
    let mut grew = true;
    while grew {
        grew = false;
        for node in 0..blocks {
            if !reaches_exit[node] && successors[node].iter().any(|next| reaches_exit[*next]) {
                reaches_exit[node] = true;
                grew = true;
            }
        }
    }
    (0..blocks)
        .map(|node| {
            if !reaches_exit[node] {
                return None;
            }
            (0..blocks)
                .filter(|candidate| *candidate != node && post[node].contains(*candidate))
                .max_by_key(|candidate| post[*candidate].count())
                .and_then(|candidate| u32::try_from(candidate).ok())
                .map(smelt_mir::BlockId)
        })
        .collect()
}

impl FunctionEmitter<'_> {
    /// The immediate post-dominator of `block` over the paths that do not
    /// `throw` (see [`immediate_post_dominators`]); computed once per
    /// function and cached.
    pub(super) fn immediate_post_dominator_past_throws(
        &self,
        block: smelt_mir::BlockId,
    ) -> Option<smelt_mir::BlockId> {
        let mut cache = self.caches.post_dominators_past_throws.borrow_mut();
        let table = cache.get_or_insert_with(|| immediate_post_dominators(self.function));
        usize::try_from(block.0)
            .ok()
            .and_then(|index| table.get(index).copied().flatten())
    }
}

impl FunctionEmitter<'_> {
    /// Where the arms `arms` of the branch at `branch` (an `if`'s two, or a
    /// `match`'s targets) rejoin: the branch's immediate post-dominator over
    /// the paths that neither `throw` nor end in a `return` only one arm
    /// reaches.
    ///
    /// A `case` that returns (`switch (k) { case 0: return -1; default: .. }`)
    /// leaves through its own `return` the way a throwing one does, so it must
    /// not stop the other arms from rejoining; the function's final `return`,
    /// which every arm reaches after the join, must still end the paths. A
    /// terminating block reachable from a single arm (walking from each arm
    /// without crossing `boundaries` or the branch itself) is emitted only
    /// inside that arm, so treating it as neutral never duplicates it. The
    /// usual table (`immediate_post_dominator_past_throws`) answers first;
    /// the per-branch table is computed only when an arm returns early.
    pub(super) fn branch_rejoin_post_dominator(
        &self,
        branch: smelt_mir::BlockId,
        arms: &[smelt_mir::BlockId],
        boundaries: &[smelt_mir::BlockId],
    ) -> Option<smelt_mir::BlockId> {
        if let Some(join) = self.immediate_post_dominator_past_throws(branch) {
            return Some(join);
        }
        let exclusive = self.arm_exclusive_terminators(Some(branch), arms, boundaries);
        if exclusive.is_empty() {
            return None;
        }
        let table = immediate_post_dominators_with(self.function, |block| {
            matches!(block.terminator, Some(Terminator::Throw(_))) || exclusive.contains(&block.id)
        });
        let join = usize::try_from(branch.0)
            .ok()
            .and_then(|index| table.get(index).copied().flatten())?;
        // A single continuing arm post-dominates the others once they are
        // neutral: nothing would be emitted twice, so there is nothing to
        // rejoin.
        let reaching = arms
            .iter()
            .filter(|arm| {
                **arm == join
                    || self.block_reaches_target_avoiding(
                        **arm,
                        join,
                        boundaries,
                        &mut BlockIdSet::default(),
                    )
            })
            .count();
        (reaching >= 2).then_some(join)
    }

    /// The `return`/`throw` blocks that exactly one of `arms` reaches, walking
    /// forward without crossing `boundaries` or `branch`.
    pub(super) fn arm_exclusive_terminators(
        &self,
        branch: Option<smelt_mir::BlockId>,
        arms: &[smelt_mir::BlockId],
        boundaries: &[smelt_mir::BlockId],
    ) -> BlockIdSet {
        let mut reached_by: rustc_hash::FxHashMap<smelt_mir::BlockId, usize> =
            rustc_hash::FxHashMap::default();
        let mut distinct = arms.to_vec();
        distinct.sort_by_key(|arm| arm.0);
        distinct.dedup();
        for arm in &distinct {
            let mut seen = BlockIdSet::default();
            let mut pending = vec![*arm];
            while let Some(block_id) = pending.pop() {
                if Some(block_id) == branch
                    || boundaries.contains(&block_id)
                    || !seen.insert(block_id)
                {
                    continue;
                }
                let Some(terminator) = self
                    .block(block_id)
                    .ok()
                    .and_then(|block| block.terminator.as_ref())
                else {
                    continue;
                };
                if matches!(terminator, Terminator::Return(_) | Terminator::Throw(_)) {
                    let count = reached_by.entry(block_id).or_default();
                    *count = count.saturating_add(1);
                }
                pending.extend(control_flow::control_flow_successors(terminator));
            }
        }
        reached_by
            .into_iter()
            .filter(|(_, count)| *count == 1)
            .map(|(block, _)| block)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The full set masks off the bits past the last node, and intersection
    /// keeps exactly the common members (the post-dominator dataflow relies on
    /// both: a stray tail bit would be a phantom post-dominator).
    #[test]
    fn block_set_full_masks_its_tail_and_intersects() {
        let set = |len: usize, members: &[usize]| {
            let mut set = BlockSet::empty(len);
            for member in members {
                set.insert(*member);
            }
            set
        };
        let mut full = BlockSet::full(70);
        assert!(full.contains(69) && !full.contains(70));
        full.intersect(&set(70, &[3, 64]));
        assert_eq!(full.count(), 2);
        assert!(full.contains(64));
    }
}
