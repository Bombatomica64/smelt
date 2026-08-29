//! Definite-assignment dataflow.
//!
//! A local must be provably assigned on every path that reaches a read of it.
//! This module computes, for each block, the set of locals that are definitely
//! defined on entry (the meet-over-all-paths intersection of predecessor
//! out-sets) via a forward worklist fixpoint, then makes a second linear pass
//! that reports any operand reading a local that is not in the block's
//! definitely-defined set at that program point.
//!
//! The two passes are kept as separate helpers so the fixpoint (which only
//! *accumulates* definitions) and the reporting walk (which *checks* reads)
//! stay independently readable.

use std::collections::{HashSet, VecDeque};

use crate::{Callee, LocalId, MirFunction, Mir, Operand, Place, Rvalue, Statement, Terminator};

use super::structure::validate_local_exists;
use super::{ValidationError, error, block_index, local_index};

/// Definite-assignment set for a single block's entry.
///
/// `None` marks a block the worklist never reached; `Some(set)` is the
/// meet (intersection) of every predecessor's out-set.
type BlockDefinitions = Option<HashSet<LocalId>>;

/// Per-block [`BlockDefinitions`], indexed by block index.
type EntryDefinitions = Vec<BlockDefinitions>;

/// Perform forward dataflow to ensure locals are assigned before use.
pub(super) fn validate_definite_assignment(
    mir: &Mir,
    function: &MirFunction,
    errors: &mut Vec<ValidationError>,
) {
    let entry_idx = block_index(function.entry);
    if function.blocks.get(entry_idx).is_none() {
        return;
    }

    let Some(in_sets) = compute_entry_definitions(function, entry_idx) else {
        return;
    };
    report_undefined_reads(mir, function, &in_sets, errors);
}

/// Run the forward worklist fixpoint and return the per-block entry
/// definition sets.
///
/// `in_sets[i]` is `None` for blocks that are never reached, and otherwise the
/// meet (intersection) of every predecessor's out-set. Returns `None` only when
/// the entry index cannot be seeded, in which case the caller skips reporting.
fn compute_entry_definitions(
    function: &MirFunction,
    entry_idx: usize,
) -> Option<EntryDefinitions> {
    let block_count = function.blocks.len();
    let mut in_sets: EntryDefinitions = vec![None; block_count];

    let mut entry_defs = HashSet::new();
    entry_defs.extend(function.params.iter().copied());
    entry_defs.extend(function.locals.iter().enumerate().filter_map(
        |(idx, local)| match local.kind {
            crate::LocalKind::UserBinding(_) => Some(LocalId(u32::try_from(idx).ok()?)),
            crate::LocalKind::Param { .. } | crate::LocalKind::Temp => None,
        },
    ));

    *in_sets.get_mut(entry_idx)? = Some(entry_defs);

    let mut queue = VecDeque::new();
    queue.push_back(function.entry);

    while let Some(block_id) = queue.pop_front() {
        let block_idx = block_index(block_id);
        let Some(block) = function.blocks.get(block_idx) else {
            continue;
        };
        let Some(definitions_slot) = in_sets.get(block_idx) else {
            continue;
        };
        let Some(mut definitions) = definitions_slot.clone() else {
            continue;
        };

        accumulate_block_definitions(block, &mut definitions);

        if let Some(terminator) = &block.terminator {
            propagate_to_successors(terminator, &definitions, &mut in_sets, &mut queue);
        }
    }

    Some(in_sets)
}

/// Add every local the block itself defines (phis, assignments, call/await
/// destinations) to `definitions`.
fn accumulate_block_definitions(block: &crate::BasicBlock, definitions: &mut HashSet<LocalId>) {
    for phi in &block.phis {
        definitions.insert(phi.dest);
    }

    for stmt in &block.statements {
        match stmt {
            Statement::Assign { dest, .. } => {
                definitions.insert(*dest);
            }
            Statement::AssignPlace { place, .. } => match place {
                Place::Local(local) => {
                    definitions.insert(*local);
                }
                Place::Field { .. } | Place::Index { .. } => {}
            },
            // The fused entry update defines `current`; its container target is
            // written through a place and so defines nothing new, exactly like
            // an `AssignPlace` to `base[index]`.
            Statement::DictEntryUpdate { current, .. } => {
                definitions.insert(*current);
            }
            Statement::StorageLive(_) | Statement::StorageDead(_) => {}
        }
    }

    if let Some(terminator) = &block.terminator {
        match terminator {
            Terminator::Call { dest, .. } | Terminator::Await { dest, .. } => {
                definitions.insert(*dest);
            }
            Terminator::Goto(_)
            | Terminator::Unreachable
            | Terminator::Switch { .. }
            | Terminator::Match { .. }
            | Terminator::Return(_)
            | Terminator::Throw(_) => {}
        }
    }
}

/// Meet `definitions` into each successor's entry set, enqueueing successors
/// whose set shrank so the fixpoint keeps propagating.
fn propagate_to_successors(
    terminator: &Terminator,
    definitions: &HashSet<LocalId>,
    in_sets: &mut [BlockDefinitions],
    queue: &mut VecDeque<crate::BlockId>,
) {
    for successor in terminator.successors() {
        let Some(existing) = in_sets.get_mut(block_index(successor)) else {
            continue;
        };
        let changed = if let Some(existing_defs) = existing {
            let intersection = existing_defs
                .intersection(definitions)
                .copied()
                .collect::<HashSet<_>>();
            let changed = *existing_defs != intersection;
            *existing_defs = intersection;
            changed
        } else {
            *existing = Some(definitions.clone());
            true
        };
        if changed {
            queue.push_back(successor);
        }
    }
}

/// Walk each reachable block in program order and report any operand that reads
/// a local before it is definitely defined at that point.
fn report_undefined_reads(
    mir: &Mir,
    function: &MirFunction,
    in_sets: &[BlockDefinitions],
    errors: &mut Vec<ValidationError>,
) {
    for (block_idx, block) in function.blocks.iter().enumerate() {
        let Some(definitions_slot) = in_sets.get(block_idx) else {
            continue;
        };
        let Some(mut definitions) = definitions_slot.clone() else {
            continue;
        };
        let before_phi = definitions.clone();

        for phi in &block.phis {
            for (_, operand) in &phi.incoming {
                validate_operand(mir, function, &before_phi, operand, errors);
            }
            definitions.insert(phi.dest);
        }

        for stmt in &block.statements {
            report_statement_reads(mir, function, &mut definitions, stmt, errors);
        }

        if let Some(terminator) = &block.terminator {
            report_terminator_reads(mir, function, &mut definitions, terminator, errors);
        }
    }
}

/// Check a statement's read operands against `definitions`, then record the
/// locals it defines.
fn report_statement_reads(
    mir: &Mir,
    function: &MirFunction,
    definitions: &mut HashSet<LocalId>,
    stmt: &Statement,
    errors: &mut Vec<ValidationError>,
) {
    match stmt {
        Statement::Assign { dest, value } => {
            validate_rvalue(mir, function, definitions, value, errors);
            definitions.insert(*dest);
        }
        Statement::AssignPlace { place, value } => {
            validate_rvalue(mir, function, definitions, value, errors);
            match place {
                Place::Local(local) => {
                    definitions.insert(*local);
                }
                Place::Field { .. } | Place::Index { .. } => {
                    validate_place(mir, function, definitions, place, errors);
                }
            }
        }
        // Reads happen in evaluation order: the container, the key and the seed
        // are all read BEFORE `current` is bound, and only then is the stored
        // rvalue evaluated — which is where `current` becomes readable.
        Statement::DictEntryUpdate {
            base,
            index,
            default,
            current,
            value,
        } => {
            validate_place(mir, function, definitions, &Place::Local(*base), errors);
            validate_operand(mir, function, definitions, index, errors);
            validate_operand(mir, function, definitions, default, errors);
            definitions.insert(*current);
            validate_rvalue(mir, function, definitions, value, errors);
        }
        Statement::StorageLive(_) | Statement::StorageDead(_) => {}
    }
}

/// Check a terminator's read operands against `definitions`, then record any
/// call/await destination it defines.
fn report_terminator_reads(
    mir: &Mir,
    function: &MirFunction,
    definitions: &mut HashSet<LocalId>,
    terminator: &Terminator,
    errors: &mut Vec<ValidationError>,
) {
    match terminator {
        Terminator::Goto(_) | Terminator::Unreachable => {}
        Terminator::Call {
            callee,
            args,
            dest,
            unwind,
            ..
        } => {
            validate_callee(mir, function, definitions, callee, errors);
            for arg in args {
                validate_operand(mir, function, definitions, arg, errors);
            }
            if let Some(handler) = unwind
                && let Some(local) = handler.exception_local
            {
                validate_local_exists(function, local, errors);
            }
            definitions.insert(*dest);
        }
        Terminator::Await {
            future,
            dest,
            unwind,
            ..
        } => {
            validate_operand(mir, function, definitions, future, errors);
            if let Some(handler) = unwind
                && let Some(local) = handler.exception_local
            {
                validate_local_exists(function, local, errors);
            }
            definitions.insert(*dest);
        }
        Terminator::Switch { cond, .. } => {
            validate_operand(mir, function, definitions, cond, errors);
        }
        Terminator::Match { scrutinee, .. } => {
            validate_operand(mir, function, definitions, scrutinee, errors);
        }
        Terminator::Return(operand) | Terminator::Throw(operand) => {
            validate_operand(mir, function, definitions, operand, errors);
        }
    }
}

/// Validate definite-assignment constraints for one rvalue.
fn validate_rvalue(
    mir: &Mir,
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    value: &Rvalue,
    errors: &mut Vec<ValidationError>,
) {
    // Operand traversal is the single source of truth in `Rvalue::for_each_operand`;
    // route validation through it so the two cannot drift as variants are added.
    value.for_each_operand(|operand| {
        validate_operand(mir, function, definitions, operand, errors);
    });
}

/// Validate definite-assignment constraints for one operand.
fn validate_operand(
    mir: &Mir,
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    operand: &Operand,
    errors: &mut Vec<ValidationError>,
) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            validate_place(mir, function, definitions, place, errors);
        }
        Operand::Const(_) => {}
    }
}

/// Validate definite-assignment constraints for one place projection chain.
fn validate_place(
    mir: &Mir,
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    place: &Place,
    errors: &mut Vec<ValidationError>,
) {
    match place {
        Place::Local(local) => {
            if !definitions.contains(local) {
                let function_name = mir.symbols.get(function.name).unwrap_or("<unknown>");
                let local_detail = local_detail(mir, function, *local);
                errors.push(error(format!(
                    "function {:?} `{function_name}` reads local {:?} {local_detail} before it is definitely defined",
                    function.id, local
                )));
            }
        }
        Place::Field { base, .. } => {
            validate_place(mir, function, definitions, &Place::Local(*base), errors);
        }
        Place::Index { base, index } => {
            validate_place(mir, function, definitions, &Place::Local(*base), errors);
            validate_operand(mir, function, definitions, index, errors);
        }
    }
}

/// Render a local declaration for definite-assignment diagnostics.
fn local_detail(mir: &Mir, function: &MirFunction, local: LocalId) -> String {
    let Some(decl) = function.locals.get(local_index(local)) else {
        return "<unknown local>".to_owned();
    };
    let kind = match decl.kind {
        crate::LocalKind::Param { symbol } => symbol
            .and_then(|param_symbol| mir.symbols.get(param_symbol))
            .map_or_else(|| "param".to_owned(), |name| format!("param `{name}`")),
        crate::LocalKind::Temp => "temp".to_owned(),
        crate::LocalKind::UserBinding(symbol) => {
            format!(
                "binding `{}`",
                mir.symbols.get(symbol).unwrap_or("<unknown>")
            )
        }
    };
    format!(
        "({kind}, span file {} start {} end {})",
        decl.span.file.0, decl.span.start, decl.span.end
    )
}

/// Validate definite-assignment constraints for one callee.
fn validate_callee(
    mir: &Mir,
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    callee: &Callee,
    errors: &mut Vec<ValidationError>,
) {
    match callee {
        Callee::Static(_) | Callee::Builtin(_) => {}
        Callee::Indirect(operand) => {
            validate_operand(mir, function, definitions, operand, errors);
        }
    }
}
