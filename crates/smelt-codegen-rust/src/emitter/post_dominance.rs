//! Immediate post-dominators of the MIR function being emitted.
//!
//! A two-way branch `switch c ? A : B` rejoins at the block every path out of
//! it must pass through next: its immediate post-dominator. Structured
//! emission renders `if c { A..J } else { B..J }` and then `J` ONCE; an
//! emitter that instead follows each arm to the end of the function renders
//! `J` (and everything after it) once per path, which is exponential in the
//! number of sequential branches. The closure emitter used to do exactly that
//! (see `emit_closure_block_inner`).
//!
//! The analysis is the textbook dataflow over the reverse CFG with one
//! virtual exit node that every `return`/`throw`/`unreachable` block (and any
//! block without a terminator) flows into: `PD(exit) = {exit}` and
//! `PD(n) = {n} ∪ ⋂ PD(s)` over the successors `s` of `n`, iterated to the
//! greatest fixpoint. Post-dominator sets form a chain, so the immediate
//! post-dominator of `n` is the member of `PD(n) \ {n}` with the largest set.
//! A block that cannot reach the exit (an infinite loop) has no
//! post-dominator.
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
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "every index is a node number below `nodes`, the length of both tables"
)]
pub(super) fn immediate_post_dominators(function: &MirFunction) -> Vec<Option<smelt_mir::BlockId>> {
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
            if next.is_empty() {
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
    (0..blocks)
        .map(|node| {
            // A block that cannot reach the exit keeps the full set.
            if !post[node].contains(exit) {
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
    /// The immediate post-dominator of `block` in the function being emitted
    /// (see the module docs); computed once per function and cached.
    pub(super) fn immediate_post_dominator(
        &self,
        block: smelt_mir::BlockId,
    ) -> Option<smelt_mir::BlockId> {
        let mut cache = self.post_dominators.borrow_mut();
        let table = cache.get_or_insert_with(|| immediate_post_dominators(self.function));
        usize::try_from(block.0)
            .ok()
            .and_then(|index| table.get(index).copied().flatten())
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
