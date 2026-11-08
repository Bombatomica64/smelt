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
mod rest_call_fusion;
mod shared_locals;
mod temp_copy_forwarding;
mod unobserved_receiver_bind;

pub use dict_default_insert_elision::DictDefaultInsertElision;
pub use dict_entry_mutation::DictEntryInPlaceMutation;
pub use dict_entry_update::DictEntryUpdate;
pub use integer_range::IntegerRange;
pub use let_temp_elision::LetTempElision;
pub use rest_call_fusion::RestCallFusion;
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

    /// The pass's name, for a validation or convergence diagnostic.
    fn name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// Upper bound on [`optimize`]'s fixpoint rounds.
///
/// Every fixpoint pass only ever removes or fuses (a temporary, a copy, a
/// clone), so a real program settles in a handful of rounds; the cap turns a
/// future pair of mutually inverting passes from a compiler hang into an error.
pub const MAX_OPTIMIZE_ROUNDS: usize = 64;

/// Why [`try_optimize`] stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OptimizeError {
    /// A round still changed the MIR after [`MAX_OPTIMIZE_ROUNDS`] rounds.
    DidNotConverge {
        /// The passes that changed the MIR in the last round.
        changing: Vec<&'static str>,
    },
    /// A pass left MIR that fails validation (checked after every pass in
    /// debug and test builds).
    InvalidAfterPass {
        /// The pass that ran last, or `"lowering"` for the input itself.
        pass: &'static str,
        /// What the validator reported.
        errors: Vec<crate::ValidationError>,
    },
}

impl std::fmt::Display for OptimizeError {
    /// Names the failure and the passes involved.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DidNotConverge { changing } => write!(
                formatter,
                "MIR optimization did not converge in {MAX_OPTIMIZE_ROUNDS} rounds; still changing: {}",
                changing.join(", ")
            ),
            Self::InvalidAfterPass { pass, errors } => {
                write!(formatter, "MIR is invalid after {pass}:")?;
                for error in errors {
                    write!(formatter, "\n  {}", error.message)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for OptimizeError {}

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
        Box::<RestCallFusion>::default(),
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
///
/// After every pass the capture table's `source_local`s are re-derived from
/// their construction sites (a pass may rewrite a captured operand; see
/// [`crate::sync_capture_sources`]) and the name/call index is dropped, so no
/// pass reads facts another pass made stale. In debug and test builds the MIR
/// is also validated after every pass, so a pass that breaks an invariant is
/// named instead of surfacing passes later.
///
/// # Errors
///
/// [`OptimizeError::DidNotConverge`] after [`MAX_OPTIMIZE_ROUNDS`] changing
/// rounds, in every build profile; [`OptimizeError::InvalidAfterPass`] in debug
/// and test builds.
pub fn try_optimize(mir: &mut Mir) -> Result<(), OptimizeError> {
    check_after(mir, "lowering")?;
    run_fixpoint(mir, &fixpoint_passes())?;
    IntegerRange.run(mir);
    after_pass(mir);
    check_after(mir, "IntegerRange")
}

/// Run `passes` in order until a whole round changes nothing, at most
/// [`MAX_OPTIMIZE_ROUNDS`] rounds.
fn run_fixpoint(mir: &mut Mir, passes: &[Box<dyn Pass>]) -> Result<(), OptimizeError> {
    let mut converged = false;
    let mut changing = Vec::new();
    for _ in 0..MAX_OPTIMIZE_ROUNDS {
        changing.clear();
        for pass in passes {
            if pass.run(mir) {
                changing.push(pass.name());
            }
            after_pass(mir);
            check_after(mir, pass.name())?;
        }
        if changing.is_empty() {
            converged = true;
            break;
        }
    }
    if converged { Ok(()) } else { Err(OptimizeError::DidNotConverge { changing }) }
}

/// [`try_optimize`] for callers that treat a failure as a bug: the test
/// harnesses that build MIR by hand.
///
/// # Panics
///
/// When [`try_optimize`] fails. The compiler itself calls [`try_optimize`]
/// through [`crate::compile`], which reports the failure as an error.
#[expect(
    clippy::panic,
    reason = "the harness entry point: a failure here is a compiler bug the test must surface"
)]
pub fn optimize(mir: &mut Mir) {
    if let Err(error) = try_optimize(mir) {
        panic!("{error}");
    }
}

/// Re-establish the facts every pass may have made stale: capture sources
/// derived from their sites, and the lazily built name/call index.
fn after_pass(mir: &mut Mir) {
    crate::sync_capture_sources(mir);
    mir.clear_index();
}

/// Validate `mir` after `pass` in debug and test builds.
///
/// Release builds validate once, after optimization, in [`crate::compile`].
fn check_after(mir: &Mir, pass: &'static str) -> Result<(), OptimizeError> {
    if cfg!(any(debug_assertions, test)) {
        let errors = crate::validate(mir);
        if !errors.is_empty() {
            return Err(OptimizeError::InvalidAfterPass { pass, errors });
        }
    }
    Ok(())
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
    let unwind_written = unwind_written_locals(function);

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
                && !unwind_written.contains(dest)
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

/// The locals an unwind edge writes: the exception local of each call or
/// await handler. Such a local has a definition `assignment_counts` does not
/// see, so a single statement assigning it does not make it an alias.
fn unwind_written_locals(function: &MirFunction) -> HashSet<LocalId> {
    function
        .blocks
        .iter()
        .filter_map(|block| match &block.terminator {
            Some(
                Terminator::Call { unwind: Some(handler), .. }
                | Terminator::Await { unwind: Some(handler), .. },
            ) => handler.exception_local,
            _ => None,
        })
        .collect()
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
///
/// The rewritten operand is always a COPY of the alias target, whatever the
/// original read was. A `move %dest` (made by [`MoveOnLastUse`] in an earlier
/// round) is a last use of `%dest`, not of its source: the source may well be
/// read again later, so rewriting it to `move %source` would move a value that
/// is still live (audit finding C11). A copy is correct at any position, and
/// [`MoveOnLastUse`] re-derives the move from the source's own last use in the
/// next round, so the hazard cannot be produced rather than being checked for.
fn rewrite_operand(operand: &mut Operand, aliases: &HashMap<LocalId, LocalId>) -> bool {
    let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand else {
        return false;
    };
    let resolved = resolve_alias(aliases, *local);
    if resolved == *local {
        return false;
    }
    *operand = Operand::Copy(Place::Local(resolved));
    true
}

#[cfg(test)]
mod fixpoint_tests {
    //! The optimizer's own guarantees: it terminates (C10) and its alias
    //! rewrite never moves a value that is still live (C11).

    use smelt_hir::{BodyId, FileId, OriginalNameTable, Span, Symbol, SymbolInterner, Type, TypeInterner};

    use super::{CopyPropagation, OptimizeError, Pass, run_fixpoint};
    use crate::{
        FuncId, HirOrigin, LocalDecl, LocalId, LocalKind, Mir, MirFunction, Operand, Place,
        Rvalue, Statement, Terminator,
    };

    /// Flips one global's type id between two values: with [`Unflip`], a pair
    /// that never settles.
    struct Flip;
    /// Undoes [`Flip`].
    struct Unflip;

    impl Pass for Flip {
        fn run(&self, mir: &mut Mir) -> bool {
            mir.globals.iter_mut().for_each(|global| global.ty = smelt_hir::TypeId(1));
            true
        }
    }

    impl Pass for Unflip {
        fn run(&self, mir: &mut Mir) -> bool {
            mir.globals.iter_mut().for_each(|global| global.ty = smelt_hir::TypeId(0));
            true
        }
    }

    /// An empty program with one global and two types.
    fn program() -> Mir {
        let mut types = TypeInterner::default();
        types.intern(Type::Float);
        types.intern(Type::String);
        let mut mir = Mir::new(types, SymbolInterner::default(), OriginalNameTable::default());
        mir.globals.push(crate::MirGlobal {
            name: Symbol(0),
            ty: smelt_hir::TypeId(0),
            init: crate::MirGlobalInit::Constant(crate::Constant::Float(0.0)),
        });
        mir
    }

    /// Two mutually inverting passes are an error after the round cap, not a
    /// hang (C10).
    #[test]
    fn mutually_inverting_passes_do_not_hang() {
        let mut mir = program();
        let passes: Vec<Box<dyn Pass>> = vec![Box::new(Flip), Box::new(Unflip)];
        let result = run_fixpoint(&mut mir, &passes);
        assert!(matches!(result, Err(OptimizeError::DidNotConverge { .. })), "{result:?}");
    }

    /// `%1 = copy %0; f(move %1); g(copy %0)`: rewriting the alias must give
    /// `f(copy %0)`, not `f(move %0)`, since `%0` is read again (C11).
    #[test]
    #[expect(clippy::indexing_slicing, clippy::panic, reason = "test fixtures unwrap and index their own known-good shapes")]
    fn an_alias_rewrite_never_moves_its_source() {
        let span = Span::new(FileId(0), 0, 0);
        let float = smelt_hir::TypeId(0);
        let mut mir = program();
        let mut function =
            MirFunction::new(FuncId(0), Symbol(0), HirOrigin::Body(BodyId(0)), float, span);
        for kind in [LocalKind::UserBinding(Symbol(1)), LocalKind::UserBinding(Symbol(2)), LocalKind::Temp] {
            function.locals.push(LocalDecl { ty: float, kind, span });
        }
        let local = |index| Place::Local(LocalId(index));
        function.blocks[0].statements = vec![
            Statement::Assign { dest: LocalId(1), value: Rvalue::Use(Operand::Copy(local(0))) },
            Statement::Assign { dest: LocalId(2), value: Rvalue::List(vec![Operand::Move(local(1)), Operand::Copy(local(0))]) },
        ];
        function.blocks[0].terminator = Some(Terminator::Return(Operand::Copy(local(2))));
        mir.push_function(function);
        assert!(CopyPropagation.run(&mut mir));
        let Some(Statement::Assign { value: Rvalue::List(items), .. }) =
            mir.functions[0].blocks[0].statements.get(1)
        else {
            panic!("the list statement survives");
        };
        assert_eq!(items[0], Operand::Copy(local(0)), "never `move %0`");
    }
}
