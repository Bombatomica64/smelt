//! Which blocks of a body run at all, and removing the ones that cannot.
//!
//! A block is reachable when an edge leads to it from the body's entry: a
//! terminator edge, or a generator-command edge out of a `yield` (see
//! [`crate::definite_assignment`]). Lowering leaves unreachable blocks behind,
//! and a pass that settles a branch can strand one; [`remove_unreachable_blocks`]
//! runs after lowering and after every optimization pass, so the validator can
//! check every block it is given and the emitter is never handed one that reads
//! locals no path assigned.

use std::collections::{HashMap, HashSet};

use crate::{BasicBlock, BlockId, Mir, Rvalue, Terminator};

/// Every block reachable from `entry`: through terminator edges and through
/// the generator-command edges of a `yield` (see the module docs).
#[must_use]
pub fn reachable_blocks(blocks: &[BasicBlock], entry: BlockId) -> HashSet<BlockId> {
    let mut seen = HashSet::from([entry]);
    let mut stack = vec![entry];
    while let Some(block_id) = stack.pop() {
        let Some(block) = blocks.get(block_id.index()) else {
            continue;
        };
        let mut targets: Vec<BlockId> = Vec::new();
        for statement in &block.statements {
            targets.extend(crate::definite_assignment::yield_edges(statement).into_iter().map(|(target, _)| target));
        }
        if let Some(terminator) = &block.terminator {
            targets.extend(terminator.successors());
        }
        for target in targets {
            if seen.insert(target) {
                stack.push(target);
            }
        }
    }
    seen
}

/// Removes every block no edge reaches from its body's entry, renumbering the
/// rest.
///
/// Lowering leaves such blocks behind (the copy of a `finally` after a jump
/// that always leaves, the join after two returning branches), and they read
/// temporaries assigned only on paths that do not exist. Removing them is what
/// lets the validator check every block it is given: after this pass a block
/// no edge reaches is a bug, not a leftover. The edges are the ones the
/// definite-assignment analysis follows, a `yield`'s generator-command edges
/// included, so a block only a generator command reaches is kept.
pub fn remove_unreachable_blocks(mir: &mut Mir) {
    for function in &mut mir.functions {
        remove_unreachable(&mut function.blocks, &mut function.entry);
    }
    for closure in &mut mir.closures {
        remove_unreachable(&mut closure.blocks, &mut closure.entry);
    }
}

/// [`remove_unreachable_blocks`] for one body's blocks and entry.
fn remove_unreachable(blocks: &mut Vec<BasicBlock>, entry: &mut BlockId) {
    let reachable = reachable_blocks(blocks, *entry);
    if reachable.len() == blocks.len() {
        return;
    }
    let mut renumbered: HashMap<BlockId, BlockId> = HashMap::new();
    let mut kept: Vec<BasicBlock> = Vec::with_capacity(reachable.len());
    for block in std::mem::take(blocks) {
        if !reachable.contains(&block.id) {
            continue;
        }
        let new_id = BlockId(u32::try_from(kept.len()).unwrap_or(u32::MAX));
        renumbered.insert(block.id, new_id);
        kept.push(block);
    }
    let map = |block: &mut BlockId| {
        if let Some(new_id) = renumbered.get(block) {
            *block = *new_id;
        }
    };
    for block in &mut kept {
        map(&mut block.id);
        for statement in &mut block.statements {
            if let Rvalue::GeneratorYield { unwind, cleanup, .. } = statement.value_mut() {
                if let Some(handler) = unwind {
                    map(&mut handler.catch_block);
                }
                if let Some(finally) = cleanup {
                    map(&mut finally.block);
                    map(&mut finally.after);
                }
            }
        }
        if let Some(terminator) = &mut block.terminator {
            remap_terminator(terminator, &map);
        }
    }
    map(entry);
    *blocks = kept;
}

/// Rewrites one block reference to its renumbered id.
type BlockMap<'m> = dyn Fn(&mut BlockId) + 'm;

/// Rewrites every block a terminator names through `map`.
fn remap_terminator(terminator: &mut Terminator, map: &BlockMap<'_>) {
    match terminator {
        Terminator::Goto(target) => map(target),
        Terminator::Call { target, unwind, .. } | Terminator::Await { target, unwind, .. } => {
            map(target);
            if let Some(handler) = unwind {
                map(&mut handler.catch_block);
            }
        }
        Terminator::Switch { then_block, else_block, .. } => {
            map(then_block);
            map(else_block);
        }
        Terminator::Match { arms, default, .. } => {
            for arm in arms {
                map(&mut arm.target);
            }
            if let Some(fallback) = default {
                map(fallback);
            }
        }
        Terminator::Return(_) | Terminator::Throw(_) => {}
    }
}
