//! Local read/write accounting shared by the MIR optimization passes.
//!
//! Several passes are only sound when a local they are about to delete is a
//! compiler temporary with exactly one definition and a known number of readers.
//! Answering that question means walking every phi, statement and terminator of
//! a function, so the walk lives here once instead of being re-derived (and
//! re-diverged) inside each pass.

use crate::{
    GlobalProjection, LocalDecl, LocalId, LocalKind, MirFunction, Operand, Place, Rvalue,
    Statement, Terminator,
};

/// The place an operand reads, if it reads one.
pub(super) const fn operand_place(operand: &Operand) -> Option<&Place> {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => Some(place),
        Operand::Const(_) => None,
    }
}

/// The local an operand reads directly, ignoring projections.
pub(super) const fn operand_local(operand: &Operand) -> Option<LocalId> {
    match operand_place(operand) {
        Some(Place::Local(local)) => Some(*local),
        Some(Place::Field { .. } | Place::Index { .. } | Place::Global { .. }) | None => None,
    }
}

/// Whether a local is a compiler temporary.
pub(super) fn local_is_temp(function: &MirFunction, local: LocalId) -> bool {
    local_decl(function, local).is_some_and(|decl| decl.kind == LocalKind::Temp)
}

/// The declaration of a local, if the index is in range.
pub(super) fn local_decl(function: &MirFunction, local: LocalId) -> Option<&LocalDecl> {
    function
        .locals
        .get(usize::try_from(local.0).unwrap_or(usize::MAX))
}

/// Count direct assignments to a local.
pub(super) fn local_assignment_count(function: &MirFunction, local: LocalId) -> usize {
    function
        .blocks
        .iter()
        .map(|block| {
            let statements = block
                .statements
                .iter()
                .filter(|statement| match statement {
                    Statement::Assign { dest, .. } => *dest == local,
                    // The fused entry update binds the entry's value to
                    // `current`; that binding is a definition, so a pass asking
                    // "is this local assigned exactly once?" must see it.
                    Statement::DictEntryUpdate { current, .. } => *current == local,
                    Statement::AssignPlace { .. }
                    | Statement::StorageLive(_)
                    | Statement::StorageDead(_) => false,
                })
                .count();
            let phis = block.phis.iter().filter(|phi| phi.dest == local).count();
            let terminator = usize::from(matches!(
                &block.terminator,
                Some(Terminator::Call { dest, .. } | Terminator::Await { dest, .. }) if *dest == local
            ));
            statements.saturating_add(phis).saturating_add(terminator)
        })
        .fold(0, usize::saturating_add)
}

/// Count reads of a local anywhere in the function body.
pub(super) fn local_read_count(function: &MirFunction, local: LocalId) -> usize {
    function
        .blocks
        .iter()
        .map(|block| {
            let phis = block
                .phis
                .iter()
                .flat_map(|phi| phi.incoming.iter())
                .filter(|(_, operand)| operand_reads_local(operand, local))
                .count();
            let statements = block
                .statements
                .iter()
                .map(|statement| statement_read_count(statement, local))
                .fold(0, usize::saturating_add);
            let terminator = terminator_read_count(block.terminator.as_ref(), local);
            phis.saturating_add(statements).saturating_add(terminator)
        })
        .fold(0, usize::saturating_add)
}

/// Count reads of a local in one statement.
fn statement_read_count(statement: &Statement, local: LocalId) -> usize {
    match statement {
        Statement::Assign { value, .. } => rvalue_read_count(value, local),
        Statement::AssignPlace { place, value } => usize::from(place_reads_local(place, local))
            .saturating_add(rvalue_read_count(value, local)),
        // Counted exactly like the `AssignPlace { place: base[index], .. }` it
        // replaces: the container local is read (the entry is reached through
        // it), so are the key and the seed, plus every operand of the stored
        // rvalue — which is where `current` is read. Missing any of these would
        // let the sibling dict passes believe a local has no readers and delete
        // a value this statement still consumes.
        Statement::DictEntryUpdate {
            base,
            index,
            default,
            current: _,
            value,
        } => usize::from(*base == local)
            .saturating_add(usize::from(operand_reads_local(index, local)))
            .saturating_add(usize::from(operand_reads_local(default, local)))
            .saturating_add(rvalue_read_count(value, local)),
        Statement::StorageLive(_) | Statement::StorageDead(_) => 0,
    }
}

/// Count reads of a local across every operand of an rvalue.
fn rvalue_read_count(value: &Rvalue, local: LocalId) -> usize {
    let mut count: usize = 0;
    value.for_each_operand(|operand| {
        count = count.saturating_add(usize::from(operand_reads_local(operand, local)));
    });
    count
}

/// Count reads of a local in a terminator.
fn terminator_read_count(terminator: Option<&Terminator>, local: LocalId) -> usize {
    let Some(found) = terminator else {
        return 0;
    };
    match found {
        Terminator::Call { args, .. } => args
            .iter()
            .filter(|arg| operand_reads_local(arg, local))
            .count(),
        Terminator::Await { future: operand, .. }
        | Terminator::Switch { cond: operand, .. }
        | Terminator::Match {
            scrutinee: operand, ..
        }
        | Terminator::Return(operand)
        | Terminator::Throw(operand) => usize::from(operand_reads_local(operand, local)),
        Terminator::Goto(_) | Terminator::Unreachable => 0,
    }
}

/// Whether an operand reads a local, including through a place projection.
pub(super) fn operand_reads_local(operand: &Operand, local: LocalId) -> bool {
    operand_place(operand).is_some_and(|place| place_reads_local(place, local))
}

/// Whether a place reads a local as its base or inside its index operand.
pub(super) fn place_reads_local(place: &Place, local: LocalId) -> bool {
    match place {
        Place::Local(candidate) | Place::Field { base: candidate, .. } => *candidate == local,
        Place::Index { base, index, .. } => *base == local || operand_reads_local(index, local),
        // No base local (the base is a `thread_local!` cell), but the INDEX
        // operand still reads one. Answering `false` here would let an
        // optimisation retire a local that the global write depends on.
        Place::Global { projection, .. } => match projection {
            GlobalProjection::Field(_) => false,
            GlobalProjection::Index { index, .. } => operand_reads_local(index, local),
        },
    }
}
