//! MIR optimization passes.
//!
//! This module provides optimization passes that can improve MIR efficiency,
//! including copy propagation and other transformations.

use std::collections::{HashMap, HashSet};

use crate::{
    Callee, LocalId, Mir, MirFunction, Operand, Place, ReadMut, Rvalue, Statement, Terminator,
};
use smelt_hir::Type;

mod dict_default_insert_elision;
mod dict_entry_mutation;
mod dict_entry_update;
mod integer_range;
mod local_use;
pub use local_use::inline_absent_constant_temps;
mod let_temp_elision;
mod move_on_last_use;
mod unobserved_receiver_bind;

pub use dict_default_insert_elision::DictDefaultInsertElision;
pub use dict_entry_mutation::DictEntryInPlaceMutation;
pub use dict_entry_update::DictEntryUpdate;
pub use integer_range::IntegerRange;
pub use let_temp_elision::LetTempElision;
pub use move_on_last_use::MoveOnLastUse;
pub use unobserved_receiver_bind::UnobservedReceiverBind;

/// A MIR optimization pass that transforms the MIR.
///
/// A `Pass` may run any number of times, interleaved with the other fixpoint
/// passes, until none of them changes the MIR: it must be safe to re-run on its
/// own output and must report `true` only when it actually changed something,
/// or the fixpoint in [`Pipeline::run`] would not terminate.
pub trait Pass {
    /// Returns the name of this pass for debugging purposes.
    fn name(&self) -> &'static str;
    /// Runs the pass on the given MIR. Returns true if the MIR was modified.
    fn run(&self, mir: &mut Mir) -> bool;
}

/// A MIR pass that runs exactly once, after the fixpoint has settled.
///
/// A separate trait (rather than a [`Pass`] the pipeline is careful to place
/// last) so that putting such a pass into the fixpoint is a type error. A final
/// pass relies on the *final* shape of every body -- e.g. [`IntegerRange`]
/// proves ranges over locals that copy propagation might otherwise still merge
/// -- and may produce output the fixpoint passes would rewrite again.
pub trait FinalPass {
    /// Returns the name of this pass for debugging purposes.
    fn name(&self) -> &'static str;
    /// Runs the pass on the given MIR. Returns true if the MIR was modified.
    fn run(&self, mir: &mut Mir) -> bool;
}

/// The MIR optimization pipeline: a fixpoint phase, then a final phase.
///
/// The ordering invariant is structural: `fixpoint` holds only [`Pass`]es and
/// `post` only [`FinalPass`]es, and [`Self::run`] always finishes the first
/// before starting the second.
pub struct Pipeline {
    /// Passes re-run, in order, until a whole round changes nothing.
    pub fixpoint: Vec<Box<dyn Pass>>,
    /// Passes run once each, in order, on the settled MIR.
    pub post: Vec<Box<dyn FinalPass>>,
}

impl Pipeline {
    /// The default pipeline used by [`optimize`].
    #[must_use]
    pub fn default_pipeline() -> Self {
        Self {
            fixpoint: default_passes(),
            post: vec![Box::<IntegerRange>::default()],
        }
    }

    /// Runs the fixpoint passes until a round changes nothing, then each final
    /// pass once.
    pub fn run(&self, mir: &mut Mir) {
        loop {
            let mut changed = false;
            for pass in &self.fixpoint {
                changed |= pass.run(mir);
            }
            if !changed {
                break;
            }
        }
        for pass in &self.post {
            pass.run(mir);
        }
    }
}

/// Copy propagation optimization that eliminates alias assignments.
#[derive(Debug, Default)]
pub struct CopyPropagation;

impl Pass for CopyPropagation {
    /// Returns the stable identifier of this pass.
    fn name(&self) -> &'static str {
        "copy-propagation"
    }

    /// Applies the pass to `mir`, returning true if it was modified.
    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= propagate_function(function, &mir.types);
        }
        changed
    }
}

/// Returns the default fixpoint passes of [`Pipeline::default_pipeline`].
#[must_use]
pub fn default_passes() -> Vec<Box<dyn Pass>> {
    vec![
        Box::<CopyPropagation>::default(),
        Box::<MoveOnLastUse>::default(),
        Box::<LetTempElision>::default(),
        Box::<DictEntryInPlaceMutation>::default(),
        Box::<DictEntryUpdate>::default(),
        Box::<DictDefaultInsertElision>::default(),
        Box::<UnobservedReceiverBind>::default(),
    ]
}

/// Applies the default optimization [`Pipeline`] to the MIR.
///
/// [`IntegerRange`] is a [`FinalPass`]: it runs once, after the fixpoint. It
/// reasons about the final shape of every body (copy propagation can merge the
/// locals it would otherwise prove separately), and nothing in the fixpoint
/// passes creates new integer-provable `number` locals from a retyped one.
pub fn optimize(mir: &mut Mir) {
    Pipeline::default_pipeline().run(mir);
}

/// Propagates copies within a function. Returns true if the function was modified.
fn propagate_function(function: &mut MirFunction, types: &smelt_hir::TypeInterner) -> bool {
    let mut aliases = HashMap::new();
    let mutated = mutated_locals(function);
    let assigned_counts = assigned_local_counts(function);

    for block in &function.blocks {
        for stmt in &block.statements {
            if let Statement::Assign {
                dest,
                value: Rvalue::Use(Operand::Copy(Place::Local(source))),
            } = stmt
                && !mutated.contains(dest)
                && !mutated.contains(source)
                && assigned_counts.get(dest) == Some(&1)
                && !is_temp_local(function, *source)
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

/// Count direct assignments to each local across a function.
fn assigned_local_counts(function: &MirFunction) -> HashMap<LocalId, usize> {
    let mut counts = HashMap::new();
    for block in &function.blocks {
        for stmt in &block.statements {
            match stmt {
                Statement::Assign { dest, .. } => {
                    let count = counts.entry(*dest).or_insert(0usize);
                    *count = count.saturating_add(1);
                }
                Statement::AssignPlace {
                    place: Place::Local(local),
                    ..
                } => {
                    let count = counts.entry(*local).or_insert(0usize);
                    *count = count.saturating_add(1);
                }
                // Binding the entry's value to `current` is a definition.
                Statement::DictEntryUpdate { current, .. } => {
                    let count = counts.entry(*current).or_insert(0usize);
                    *count = count.saturating_add(1);
                }
                Statement::AssignPlace { .. } => {}
            }
        }
    }
    counts
}

/// Return whether a local is a compiler-generated temporary.
fn is_temp_local(function: &MirFunction, local: LocalId) -> bool {
    function
        .locals
        .get(usize::try_from(local.0).unwrap_or(usize::MAX))
        .is_some_and(|decl| matches!(decl.kind, crate::LocalKind::Temp))
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
/// exactly once, by the copy that created it, so no other statement defines it.
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
    match terminator {
        Terminator::Goto(_) => false,
        Terminator::Return(operand) | Terminator::Throw(operand) => {
            rewrite_operand(operand, aliases)
        }
        Terminator::Switch { cond, .. } => rewrite_operand(cond, aliases),
        Terminator::Match { scrutinee, .. } => rewrite_operand(scrutinee, aliases),
        Terminator::Call {
            callee, args, dest, ..
        } => {
            let mut changed = match callee {
                Callee::Indirect(operand) => rewrite_operand(operand, aliases),
                Callee::Static(_) | Callee::Builtin(_) | Callee::BaseInit(_) => false,
            };
            for arg in args {
                changed |= rewrite_operand_except(arg, aliases, Some(*dest));
            }
            changed
        }
        Terminator::Await { future, dest, .. } => {
            rewrite_operand_except(future, aliases, Some(*dest))
        }
    }
}

/// Rewrites an operand using alias mappings. Returns true if modified.
fn rewrite_operand(operand: &mut Operand, aliases: &HashMap<LocalId, LocalId>) -> bool {
    rewrite_operand_except(operand, aliases, None)
}

/// Rewrites an operand using alias mappings, except for a specific local ID. Returns true if modified.
fn rewrite_operand_except(
    operand: &mut Operand,
    aliases: &HashMap<LocalId, LocalId>,
    except: Option<LocalId>,
) -> bool {
    let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand else {
        return false;
    };
    if Some(*local) == except {
        return false;
    }
    let resolved = resolve_alias(aliases, *local);
    if resolved == *local {
        return false;
    }
    *local = resolved;
    true
}
