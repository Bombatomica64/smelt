//! MIR optimization passes.
//!
//! This module provides optimization passes that can improve MIR efficiency,
//! including copy propagation and other transformations.

use std::collections::{HashMap, HashSet};

use crate::{
    LocalId, Mir, MirFunction, Operand, Place, ReadMut, Rvalue, Statement, Terminator,
};
use shared_locals::SharedLocals;
use smelt_hir::Type;

mod dict_default_insert_elision;
mod dict_entry_mutation;
mod dict_entry_update;
mod integer_range;
mod local_use;
pub use local_use::inline_absent_constant_temps;
mod let_temp_elision;
mod move_on_last_use;
mod shared_locals;
mod temp_copy_forwarding;
mod unobserved_receiver_bind;

pub use dict_default_insert_elision::DictDefaultInsertElision;
pub use dict_entry_mutation::DictEntryInPlaceMutation;
pub use dict_entry_update::DictEntryUpdate;
pub use integer_range::IntegerRange;
pub use let_temp_elision::LetTempElision;
pub use move_on_last_use::MoveOnLastUse;
pub use temp_copy_forwarding::TempCopyForwarding;
pub use unobserved_receiver_bind::UnobservedReceiverBind;

/// A MIR optimization pass that transforms the MIR.
///
/// A `Pass` may run any number of times, interleaved with the other fixpoint
/// passes, until none of them changes the MIR: it must be safe to re-run on its
/// own output and must report `true` only when it actually changed something,
/// or the fixpoint in [`optimize`] would not terminate.
pub trait Pass {
    /// Runs the pass on the given MIR. Returns true if the MIR was modified.
    fn run(&self, mir: &mut Mir) -> bool;
}

/// Copy propagation optimization that eliminates alias assignments.
#[derive(Debug, Default)]
pub struct CopyPropagation;

impl Pass for CopyPropagation {
    /// Applies the pass to `mir`, returning true if it was modified.
    fn run(&self, mir: &mut Mir) -> bool {
        let facts = SharedLocals::of(mir);
        let empty = HashSet::new();
        let mut changed = false;
        for (index, function) in mir.functions.iter_mut().enumerate() {
            let shared = facts.of_function(index).unwrap_or(&empty);
            changed |= propagate_function(function, shared, &mir.types);
        }
        changed
    }
}

/// The passes [`optimize`] re-runs until a whole round changes nothing.
fn fixpoint_passes() -> Vec<Box<dyn Pass>> {
    vec![
        Box::<CopyPropagation>::default(),
        Box::<MoveOnLastUse>::default(),
        Box::<LetTempElision>::default(),
        Box::<TempCopyForwarding>::default(),
        Box::<DictEntryInPlaceMutation>::default(),
        Box::<DictEntryUpdate>::default(),
        Box::<DictDefaultInsertElision>::default(),
        Box::<UnobservedReceiverBind>::default(),
    ]
}

/// Optimizes the MIR: the fixpoint passes until a round changes nothing, then
/// [`IntegerRange`] once.
///
/// The ordering is structural: [`IntegerRange`] is not a [`Pass`], so putting it
/// into the fixpoint is a type error. It reasons about the final shape of every
/// body (copy propagation can merge the locals it would otherwise prove
/// separately), and nothing in the fixpoint passes creates new integer-provable
/// `number` locals from a retyped one.
pub fn optimize(mir: &mut Mir) {
    let passes = fixpoint_passes();
    loop {
        let mut changed = false;
        for pass in &passes {
            changed |= pass.run(mir);
        }
        if !changed {
            break;
        }
    }
    IntegerRange.run(mir);
}

/// Propagates copies within a function. Returns true if the function was modified.
///
/// `shared` are the locals a closure can write: a copy of one is a snapshot, not an
/// alias, since the source can change at any later call.
fn propagate_function(
    function: &mut MirFunction,
    shared: &HashSet<LocalId>,
    types: &smelt_hir::TypeInterner,
) -> bool {
    let mut aliases = HashMap::new();
    let mutated = mutated_locals(function);
    let assigned_counts = function.assignment_counts();

    for block in &function.blocks {
        for stmt in &block.statements {
            if let Statement::Assign {
                dest,
                value: Rvalue::Use(Operand::Copy(Place::Local(source))),
            } = stmt
                && !mutated.contains(dest)
                && !mutated.contains(source)
                && !shared.contains(source)
                && assigned_counts.get(dest) == Some(&1)
                && !function.is_temp(*source)
                && !is_function_local(function, types, *dest)
                && !is_function_local(function, types, *source)
                && locals_have_same_type(function, *dest, *source)
            {
                aliases.insert(*dest, resolve_alias(&aliases, *source));
            }
        }
    }

    if aliases.is_empty() {
        return false;
    }

    let mut changed = false;
    for block in &mut function.blocks {
        for stmt in &mut block.statements {
            changed |= rewrite_statement(stmt, &aliases);
        }
        if let Some(terminator) = &mut block.terminator {
            changed |= rewrite_terminator(terminator, &aliases);
        }
    }

    changed
}

/// Return whether copy propagation can preserve the assignment's type semantics.
fn locals_have_same_type(function: &MirFunction, left: LocalId, right: LocalId) -> bool {
    let left_ty = function
        .locals
        .get(usize::try_from(left.0).unwrap_or(usize::MAX))
        .map(|decl| decl.ty);
    let right_ty = function
        .locals
        .get(usize::try_from(right.0).unwrap_or(usize::MAX))
        .map(|decl| decl.ty);
    left_ty.is_some() && left_ty == right_ty
}

/// Return whether a local has a function type that must not be clone-propagated.
fn is_function_local(
    function: &MirFunction,
    types: &smelt_hir::TypeInterner,
    local: LocalId,
) -> bool {
    function
        .locals
        .get(usize::try_from(local.0).unwrap_or(usize::MAX))
        .is_some_and(|decl| matches!(types.get(decl.ty), Some(Type::Function(_))))
}

/// Returns locals that are mutated after initialization.
fn mutated_locals(function: &MirFunction) -> HashSet<LocalId> {
    let mut locals = HashSet::new();
    for block in &function.blocks {
        for stmt in &block.statements {
            match stmt {
                Statement::AssignPlace { place, .. } => match place {
                    Place::Local(local) | Place::Field { base: local, .. } => {
                        locals.insert(*local);
                    }
                    Place::Index { base, .. } => {
                        locals.insert(*base);
                    }
                    // The write target is a `thread_local!` cell, so no local
                    // is written through this place.
                    Place::Global { .. } => {}
                },
                // The fused entry update writes through its container, so the
                // container is mutated and must never become a copy-propagation
                // alias source.
                Statement::DictEntryUpdate { base, .. } => {
                    locals.insert(*base);
                }
                Statement::Assign { .. } => {}
            }
        }
    }
    locals
}

/// Resolves a local to its canonical alias target.
fn resolve_alias(aliases: &HashMap<LocalId, LocalId>, local: LocalId) -> LocalId {
    let mut current = local;
    let mut seen = HashSet::new();
    while let Some(next) = aliases.get(&current).copied() {
        if !seen.insert(current) {
            break;
        }
        current = next;
    }
    current
}

/// Rewrites everything a statement reads using alias mappings. Returns true if
/// modified.
///
/// A statement's own definition needs no exclusion here: an alias key is assigned
/// exactly once, by the copy that created it, so nothing else defines it.
fn rewrite_statement(statement: &mut Statement, aliases: &HashMap<LocalId, LocalId>) -> bool {
    let mut changed = false;
    statement.reads_mut(|read| match read {
        ReadMut::Base(local) => {
            let resolved = resolve_alias(aliases, *local);
            changed |= resolved != *local;
            *local = resolved;
        }
        ReadMut::Operand(operand) => changed |= rewrite_operand(operand, aliases),
    });
    changed
}

/// Rewrites operands in a terminator using alias mappings. Returns true if modified.
fn rewrite_terminator(terminator: &mut Terminator, aliases: &HashMap<LocalId, LocalId>) -> bool {
    let mut changed = false;
    terminator.reads_mut(|operand| changed |= rewrite_operand(operand, aliases));
    changed
}

/// Rewrites an operand using alias mappings. Returns true if modified.
fn rewrite_operand(operand: &mut Operand, aliases: &HashMap<LocalId, LocalId>) -> bool {
    let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand else {
        return false;
    };
    let resolved = resolve_alias(aliases, *local);
    if resolved == *local {
        return false;
    }
    *local = resolved;
    true
}
