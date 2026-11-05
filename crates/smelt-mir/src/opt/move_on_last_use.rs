//! Move-on-last-use optimization.
//!
//! Source languages (TypeScript / Python) have reference semantics and a
//! garbage collector, so HIR-to-MIR lowering reads every value through
//! [`Operand::Copy`], which codegen turns into a defensive `.clone()`. A human
//! porting the same code to Rust would instead *move* a value at its final use.
//! This pass performs that transformation: it rewrites
//! `Operand::Copy(Place::Local(x))` into `Operand::Move` wherever `x` is
//! provably dead immediately afterwards.
//!
//! The Rust compiler cannot recover this for us — once codegen has emitted a
//! `.clone()`, the allocation is committed. Performing the move here removes
//! real `String`/`Vec`/struct allocations and, as a side effect, clears the
//! no-op clones on `Copy` types that would otherwise be clippy noise.
//!
//! # Correctness
//!
//! A use of local `x` is rewritten to a move only when every guard holds:
//!
//! * The operand is a bare [`Place::Local`]. Moving out of a `Field` or `Index`
//!   place is rejected by Rust (`cannot move out of ...`), so those keep their
//!   `Copy`.
//! * `x` is dead in the live-out set at that program point (real backward
//!   liveness over the CFG, so loop back-edges keep loop-carried values alive).
//! * `x` is read exactly once in the containing statement/terminator. This
//!   sidesteps within-statement use ordering (e.g. `x + x`) conservatively.
//! * `x` is convertible: not a parameter (their ABI is handled by a later
//!   borrow pass and receivers may be references), and not captured by any
//!   closure (a by-reference capture keeps the local live past its textual use).
//!
//! These guards are intentionally conservative; later passes relax them.

use std::collections::{HashMap, HashSet};

use crate::{
    BasicBlock, LocalId, LocalKind, Mir, MirFunction, Operand, Place, Rvalue, Statement, Terminator,
};

use super::Pass;

/// Rewrites the last `Copy` use of an owned local into a `Move`.
#[derive(Debug, Default)]
pub struct MoveOnLastUse;

impl Pass for MoveOnLastUse {
    /// Returns the stable identifier of this pass.
    fn name(&self) -> &'static str {
        "move-on-last-use"
    }

    /// Applies the pass to `mir`, returning true if it was modified.
    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= run_function(function);
        }
        changed
    }
}

/// Apply move-on-last-use to a single function. Returns true if modified.
fn run_function(function: &mut MirFunction) -> bool {
    let convertible = convertible_locals(function);
    if convertible.is_empty() {
        return false;
    }

    let live_out = compute_block_live_out(function);

    let mut changed = false;
    for (block, block_live_out) in function.blocks.iter_mut().zip(&live_out) {
        let live_after = block_program_point_liveness(block, block_live_out);
        changed |= rewrite_block(block, &live_after, &convertible);
    }
    changed
}

/// Per-program-point live-after sets within a single block.
struct BlockLiveness {
    /// Live locals immediately after statement `i` executes.
    after_statement: Vec<HashSet<LocalId>>,
    /// Live locals immediately after the terminator (the block live-out set).
    after_terminator: HashSet<LocalId>,
}

/// Compute live-after sets for every statement and the terminator of a block.
fn block_program_point_liveness(block: &BasicBlock, live_out: &HashSet<LocalId>) -> BlockLiveness {
    let after_terminator = live_out.clone();

    let mut live = live_out.clone();
    if let Some(terminator) = &block.terminator {
        apply_terminator_transfer(terminator, &mut live);
    }

    let mut after_statement = Vec::with_capacity(block.statements.len());
    for stmt in block.statements.iter().rev() {
        after_statement.push(live.clone());
        apply_statement_transfer(stmt, &mut live);
    }
    after_statement.reverse();

    BlockLiveness {
        after_statement,
        after_terminator,
    }
}

/// Rewrite convertible operands of a block in place. Returns true if modified.
fn rewrite_block(
    block: &mut BasicBlock,
    live: &BlockLiveness,
    convertible: &HashSet<LocalId>,
) -> bool {
    let mut changed = false;

    for (stmt, live_after) in block.statements.iter_mut().zip(&live.after_statement) {
        let counts = statement_read_counts(stmt);
        stmt.value_mut().reads_mut(|operand| {
            changed |= try_convert(operand, live_after, &counts, convertible);
        });
    }

    if let Some(terminator) = &mut block.terminator {
        let counts = terminator_read_counts(terminator);
        changed |= rewrite_terminator(terminator, &live.after_terminator, &counts, convertible);
    }

    changed
}

/// Convert a single operand to a move when all guards hold. Returns true if modified.
fn try_convert(
    operand: &mut Operand,
    live_after: &HashSet<LocalId>,
    read_counts: &HashMap<LocalId, usize>,
    convertible: &HashSet<LocalId>,
) -> bool {
    let Operand::Copy(Place::Local(local)) = operand else {
        return false;
    };
    let target = *local;
    if convertible.contains(&target)
        && read_counts.get(&target).copied() == Some(1)
        && !live_after.contains(&target)
    {
        *operand = Operand::Move(Place::Local(target));
        return true;
    }
    false
}

/// Rewrite the operands directly held by a terminator. Returns true if modified.
///
/// `Switch`/`Match` scrutinees can move on last use: the Rust emitter's
/// structured-loop reconstruction accepts both `switch copy %local` and
/// `switch move %local` headers, and re-emits the condition via an explicit
/// `Copy` when generating the loop.
fn rewrite_terminator(
    terminator: &mut Terminator,
    live_after: &HashSet<LocalId>,
    read_counts: &HashMap<LocalId, usize>,
    convertible: &HashSet<LocalId>,
) -> bool {
    let mut changed = false;
    terminator.reads_mut(|operand| {
        changed |= try_convert(operand, live_after, read_counts, convertible);
    });
    changed
}

/// Locals eligible for move conversion in this function.
///
/// Excludes parameters and any closure capture source — see the module docs for why each guard is required.
fn convertible_locals(function: &MirFunction) -> HashSet<LocalId> {
    let mut excluded = HashSet::new();

    for (index, decl) in function.locals.iter().enumerate() {
        if matches!(decl.kind, LocalKind::Param { .. }) {
            excluded.insert(local_id(index));
        }
    }

    for block in &function.blocks {
        for stmt in &block.statements {
            if let Rvalue::Closure { captures, .. } = stmt.value() {
                for capture in captures {
                    capture.read_locals(&mut |local| {
                        excluded.insert(local);
                    });
                }
            }
        }
    }

    (0..function.locals.len())
        .map(local_id)
        .filter(|id| !excluded.contains(id))
        .collect()
}

/// Whole-function backward liveness producing a live-out set per block.
fn compute_block_live_out(function: &MirFunction) -> Vec<HashSet<LocalId>> {
    let block_count = function.blocks.len();
    let mut index_of = HashMap::with_capacity(block_count);
    for (index, block) in function.blocks.iter().enumerate() {
        index_of.insert(block.id, index);
    }

    let mut live_in = vec![HashSet::new(); block_count];
    let mut live_out = vec![HashSet::new(); block_count];

    let mut changed = true;
    while changed {
        changed = false;
        for (index, block) in function.blocks.iter().enumerate().rev() {
            let mut out = HashSet::new();
            if let Some(terminator) = &block.terminator {
                for successor in terminator.successors() {
                    if let Some(&successor_index) = index_of.get(&successor)
                        && let Some(successor_live_in) = live_in.get(successor_index)
                    {
                        out.extend(successor_live_in.iter().copied());
                    }
                }
            }

            let mut entry = out.clone();
            transfer_block_backward(block, &mut entry);

            if live_out.get(index) != Some(&out) {
                if let Some(slot) = live_out.get_mut(index) {
                    *slot = out;
                }
                changed = true;
            }
            if live_in.get(index) != Some(&entry) {
                if let Some(slot) = live_in.get_mut(index) {
                    *slot = entry;
                }
                changed = true;
            }
        }
    }

    live_out
}

/// Transfer a live set backward across an entire block (live-out -> live-in).
fn transfer_block_backward(block: &BasicBlock, live: &mut HashSet<LocalId>) {
    if let Some(terminator) = &block.terminator {
        apply_terminator_transfer(terminator, live);
    }
    for stmt in block.statements.iter().rev() {
        apply_statement_transfer(stmt, live);
    }
}

/// Transfer a live set backward across one statement: `live = (live - def) + uses`.
fn apply_statement_transfer(stmt: &Statement, live: &mut HashSet<LocalId>) {
    if let Some(def) = stmt.defs() {
        live.remove(&def);
    }
    stmt.read_locals(|local| {
        live.insert(local);
    });
}

/// Transfer a live set backward across the terminator: `live = (live - def) + uses`.
fn apply_terminator_transfer(terminator: &Terminator, live: &mut HashSet<LocalId>) {
    if let Some(def) = terminator.defs() {
        live.remove(&def);
    }
    terminator.read_locals(|local| {
        live.insert(local);
    });
}

/// Count reads of each local within a statement.
fn statement_read_counts(stmt: &Statement) -> HashMap<LocalId, usize> {
    let mut reads = Vec::new();
    stmt.read_locals(|local| reads.push(local));
    count(reads)
}

/// Count reads of each local within a terminator.
fn terminator_read_counts(terminator: &Terminator) -> HashMap<LocalId, usize> {
    let mut reads = Vec::new();
    terminator.read_locals(|local| reads.push(local));
    count(reads)
}

/// Build an occurrence histogram from a list of locals.
fn count(reads: Vec<LocalId>) -> HashMap<LocalId, usize> {
    let mut counts = HashMap::new();
    for local in reads {
        let entry = counts.entry(local).or_insert(0usize);
        *entry = entry.saturating_add(1);
    }
    counts
}

/// Build a [`LocalId`] from a positional index.
fn local_id(index: usize) -> LocalId {
    LocalId(u32::try_from(index).unwrap_or(u32::MAX))
}
