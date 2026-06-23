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
//!   borrow pass and receivers may be references), not involved in any `Phi`
//!   (edge-precise phi liveness is deferred), and not captured by any closure
//!   (a by-reference capture keeps the local live past its textual use).
//!
//! These guards are intentionally conservative; later passes relax them.

use std::collections::{HashMap, HashSet};

use crate::{
    BasicBlock, Callee, LocalId, LocalKind, Mir, MirFunction, Operand, Place, Rvalue, Statement,
    Terminator,
};

use super::Pass;

/// Rewrites the last `Copy` use of an owned local into a `Move`.
#[derive(Debug, Default)]
pub struct MoveOnLastUse;

impl Pass for MoveOnLastUse {
    fn name(&self) -> &'static str {
        "move-on-last-use"
    }

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
        if let Some(value) = statement_value_mut(stmt) {
            let mut local_changed = false;
            value.for_each_operand_mut(|operand| {
                if try_convert(operand, live_after, &counts, convertible) {
                    local_changed = true;
                }
            });
            changed |= local_changed;
        }
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
fn rewrite_terminator(
    terminator: &mut Terminator,
    live_after: &HashSet<LocalId>,
    read_counts: &HashMap<LocalId, usize>,
    convertible: &HashSet<LocalId>,
) -> bool {
    match terminator {
        Terminator::Goto(_) | Terminator::Unreachable => false,
        Terminator::Return(operand) | Terminator::Throw(operand) => {
            try_convert(operand, live_after, read_counts, convertible)
        }
        // `Switch`/`Match` scrutinees can move on last use. The Rust emitter's
        // structured-loop reconstruction accepts both `switch copy %local` and
        // `switch move %local` headers (and re-emits the condition via an
        // explicit `Copy` when generating the loop), so a moved scrutinee no
        // longer flattens nested loops.
        Terminator::Switch { cond, .. } => {
            try_convert(cond, live_after, read_counts, convertible)
        }
        Terminator::Match { scrutinee, .. } => {
            try_convert(scrutinee, live_after, read_counts, convertible)
        }
        Terminator::Await { future, .. } => {
            try_convert(future, live_after, read_counts, convertible)
        }
        Terminator::Call { callee, args, .. } => {
            let mut changed = match callee {
                Callee::Indirect(operand) => {
                    try_convert(operand, live_after, read_counts, convertible)
                }
                Callee::Static(_) | Callee::Builtin(_) => false,
            };
            for arg in args.iter_mut() {
                changed |= try_convert(arg, live_after, read_counts, convertible);
            }
            changed
        }
    }
}

/// Locals eligible for move conversion in this function.
///
/// Excludes parameters, anything touched by a `Phi`, and any closure capture
/// source — see the module docs for why each guard is required.
fn convertible_locals(function: &MirFunction) -> HashSet<LocalId> {
    let mut excluded = HashSet::new();

    for (index, decl) in function.locals.iter().enumerate() {
        if matches!(decl.kind, LocalKind::Param { .. }) {
            excluded.insert(local_id(index));
        }
    }

    let mut scratch = Vec::new();
    for block in &function.blocks {
        for phi in &block.phis {
            excluded.insert(phi.dest);
            for (_, operand) in &phi.incoming {
                scratch.clear();
                collect_operand_reads(operand, &mut scratch);
                excluded.extend(scratch.iter().copied());
            }
        }
        for stmt in &block.statements {
            if let Some(Rvalue::Closure { captures, .. }) = statement_value(stmt) {
                for capture in captures {
                    scratch.clear();
                    collect_operand_reads(capture, &mut scratch);
                    excluded.extend(scratch.iter().copied());
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

    // Phi edges are intentionally ignored: every phi-related local is excluded
    // from conversion, and omitting their edge-uses only ever *adds* liveness
    // elsewhere, which keeps the analysis sound (conservative) for the locals
    // we do convert.
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
    if let Some(def) = statement_def(stmt) {
        live.remove(&def);
    }
    let mut reads = Vec::new();
    statement_reads(stmt, &mut reads);
    live.extend(reads);
}

/// Transfer a live set backward across the terminator: `live = (live - def) + uses`.
fn apply_terminator_transfer(terminator: &Terminator, live: &mut HashSet<LocalId>) {
    if let Some(def) = terminator_def(terminator) {
        live.remove(&def);
    }
    let mut reads = Vec::new();
    terminator_reads(terminator, &mut reads);
    live.extend(reads);
}

/// The local defined (overwritten) by a statement, if any.
fn statement_def(stmt: &Statement) -> Option<LocalId> {
    match stmt {
        Statement::Assign { dest, .. } => Some(*dest),
        Statement::AssignPlace {
            place: Place::Local(local),
            ..
        } => Some(*local),
        Statement::AssignPlace { .. }
        | Statement::StorageLive(_)
        | Statement::StorageDead(_) => None,
    }
}

/// The local defined by a terminator (the destination of a call/await), if any.
fn terminator_def(terminator: &Terminator) -> Option<LocalId> {
    match terminator {
        Terminator::Call { dest, .. } | Terminator::Await { dest, .. } => Some(*dest),
        Terminator::Goto(_)
        | Terminator::Switch { .. }
        | Terminator::Match { .. }
        | Terminator::Return(_)
        | Terminator::Throw(_)
        | Terminator::Unreachable => None,
    }
}

/// Collect every local read by a statement (place projections included).
///
/// A `Field`/`Index` assignment target is a *read* of its base (the value is
/// mutated in place and stays live), whereas a bare `Local` target is a pure
/// definition and contributes no read.
fn statement_reads(stmt: &Statement, out: &mut Vec<LocalId>) {
    match stmt {
        Statement::Assign { value, .. } => {
            value.for_each_operand(|operand| collect_operand_reads(operand, out));
        }
        Statement::AssignPlace { place, value } => {
            match place {
                Place::Local(_) => {}
                Place::Field { base, .. } => out.push(*base),
                Place::Index { base, index } => {
                    out.push(*base);
                    collect_operand_reads(index, out);
                }
            }
            value.for_each_operand(|operand| collect_operand_reads(operand, out));
        }
        Statement::StorageLive(_) | Statement::StorageDead(_) => {}
    }
}

/// Collect every local read by a terminator.
fn terminator_reads(terminator: &Terminator, out: &mut Vec<LocalId>) {
    match terminator {
        Terminator::Goto(_) | Terminator::Unreachable => {}
        Terminator::Return(operand) | Terminator::Throw(operand) => {
            collect_operand_reads(operand, out);
        }
        Terminator::Switch { cond, .. } => collect_operand_reads(cond, out),
        Terminator::Match { scrutinee, .. } => collect_operand_reads(scrutinee, out),
        Terminator::Await { future, .. } => collect_operand_reads(future, out),
        Terminator::Call { callee, args, .. } => {
            if let Callee::Indirect(operand) = callee {
                collect_operand_reads(operand, out);
            }
            for arg in args {
                collect_operand_reads(arg, out);
            }
        }
    }
}

/// Decompose an operand into the locals it reads, recursing through places.
fn collect_operand_reads(operand: &Operand, out: &mut Vec<LocalId>) {
    match operand {
        Operand::Const(_) => {}
        Operand::Copy(place) | Operand::Move(place) => match place {
            Place::Local(local) => out.push(*local),
            Place::Field { base, .. } => out.push(*base),
            Place::Index { base, index } => {
                out.push(*base);
                collect_operand_reads(index, out);
            }
        },
    }
}

/// Count reads of each local within a statement.
fn statement_read_counts(stmt: &Statement) -> HashMap<LocalId, usize> {
    let mut reads = Vec::new();
    statement_reads(stmt, &mut reads);
    count(reads)
}

/// Count reads of each local within a terminator.
fn terminator_read_counts(terminator: &Terminator) -> HashMap<LocalId, usize> {
    let mut reads = Vec::new();
    terminator_reads(terminator, &mut reads);
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

/// The rvalue assigned by a statement, if it assigns one.
fn statement_value(stmt: &Statement) -> Option<&Rvalue> {
    match stmt {
        Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } => Some(value),
        Statement::StorageLive(_) | Statement::StorageDead(_) => None,
    }
}

/// The mutable rvalue assigned by a statement, if it assigns one.
fn statement_value_mut(stmt: &mut Statement) -> Option<&mut Rvalue> {
    match stmt {
        Statement::Assign { value, .. } | Statement::AssignPlace { value, .. } => Some(value),
        Statement::StorageLive(_) | Statement::StorageDead(_) => None,
    }
}

/// Build a [`LocalId`] from a positional index.
fn local_id(index: usize) -> LocalId {
    LocalId(u32::try_from(index).unwrap_or(u32::MAX))
}
