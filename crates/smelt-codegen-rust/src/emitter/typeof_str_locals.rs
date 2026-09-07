//! Which locals can hold the `&'static str` a `typeof` produces.
//!
//! JavaScript `typeof` answers one of seven fixed spellings, so the value a
//! `Rvalue::TypeofValue` produces is naturally a `&'static str`
//! ([`FunctionEmitter::typeof_str_text`]). MIR types the destination local
//! `Type::String`, though, because MIR has one string type and no notion of a
//! borrowed one -- so emitting the local as a Rust `String` costs an allocation
//! per evaluation that the value never needed.
//!
//! This analysis finds the locals where the borrowed type is provably enough. It
//! is deliberately a whitelist: a local qualifies only when EVERY definition and
//! EVERY read of it is a shape this module has enumerated and the emitter knows
//! how to spell against a `&'static str`. Anything else -- a read this walk does
//! not recognise, a second definition, a capture, a return -- disqualifies the
//! local and it keeps the owned `String`. A wrong narrowing is a type error in a
//! generated crate, so the analysis errs toward the owned form every time.
//!
//! The recognised shapes, which are what the corpora actually contain:
//!
//! - the scrutinee of a `Terminator::Match` -- the source `switch (typeof x)`.
//!   A `&'static str` has no `.as_str()`, so [`FunctionEmitter::match_scrutinee_text`]
//!   matches the binding directly instead.
//! - either side of an equality `Rvalue::Binary` against a string literal or
//!   against another local this analysis also accepted -- the source
//!   `typeof a !== typeof b`. `&str`/`&str`, `&str`/`String` and `String`/`&str`
//!   comparisons all resolve through `PartialEq` impls in `core`/`alloc`, so the
//!   emitted `lhs == rhs` type-checks whichever side was narrowed.

use std::collections::HashSet;

use smelt_hir::{Type, TypeId};
use smelt_mir::{
    Callee, Constant, LocalId, LocalKind, Mir, MirFunction, Operand, Place, Rvalue, Statement,
    Terminator,
};

/// Locals of `function` whose emitted Rust type can be `&'static str`.
///
/// Computed once per function when the emitter is built. Returns an empty set
/// for a function with no `typeof`, which is the overwhelmingly common case.
pub(super) fn static_typeof_str_locals(mir: &Mir, function: &MirFunction) -> HashSet<LocalId> {
    let mut candidates = definition_candidates(mir, function);
    if candidates.is_empty() {
        return candidates;
    }
    // Equality accepts an operand pair where BOTH sides are candidates, so
    // dropping one local can disqualify another. Shrink to a fixed point rather
    // than trusting a single pass.
    loop {
        let rejected: HashSet<LocalId> = candidates
            .iter()
            .copied()
            .filter(|local| !every_read_is_supported(function, *local, &candidates))
            .collect();
        if rejected.is_empty() {
            return candidates;
        }
        candidates.retain(|local| !rejected.contains(local));
        if candidates.is_empty() {
            return candidates;
        }
    }
}

/// Locals defined exactly once, by a `typeof`, and by nothing else.
///
/// Requiring a SINGLE definition is what keeps the emitted declaration in the
/// two places this change teaches to spell `&'static str`: the function-scoped
/// predeclaration and the `let` at the assignment. A local assigned in both arms
/// of an `if` is hoisted into a `let … = if … else …` by a third emitter, and a
/// call/await destination is declared by a fourth; neither can hold a
/// single-definition local, so neither has to learn the borrowed spelling.
fn definition_candidates(mir: &Mir, function: &MirFunction) -> HashSet<LocalId> {
    let mut typeof_defs: HashSet<LocalId> = HashSet::new();
    let mut disqualified: HashSet<LocalId> = function.params.iter().copied().collect();
    let mut define = |local: LocalId, rejected: &mut HashSet<LocalId>| {
        // A second definition of any shape, `typeof` or not, disqualifies.
        if !typeof_defs.insert(local) {
            rejected.insert(local);
        }
    };
    for block in &function.blocks {
        for phi in &block.phis {
            disqualified.insert(phi.dest);
        }
        for statement in &block.statements {
            match statement {
                Statement::Assign { dest, value } => {
                    if matches!(value, Rvalue::TypeofValue { .. }) {
                        define(*dest, &mut disqualified);
                    } else {
                        disqualified.insert(*dest);
                    }
                    // A local captured by a closure is read from inside another
                    // body, which this walk does not see.
                    if let Rvalue::Closure { id, .. } = value
                        && let Some(closure) = mir
                            .closures
                            .get(usize::try_from(id.0).unwrap_or(usize::MAX))
                    {
                        for capture in &closure.captures {
                            disqualified.insert(capture.source_local);
                        }
                    }
                }
                Statement::AssignPlace { place, .. } => {
                    if let Place::Local(local) = place {
                        disqualified.insert(*local);
                    }
                }
                Statement::DictEntryUpdate { current, .. } => {
                    disqualified.insert(*current);
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }
        if let Some(Terminator::Call { dest, .. } | Terminator::Await { dest, .. }) =
            &block.terminator
        {
            disqualified.insert(*dest);
        }
    }
    typeof_defs.retain(|local| {
        !disqualified.contains(local) && is_plain_string_temp(mir, function, *local)
    });
    typeof_defs
}

/// Whether a local is an ordinary `Type::String` compiler temporary.
///
/// Only a temporary qualifies: a source-named binding can be observed by a
/// debugger, a closure capture, or a later emitter pass that spells its declared
/// type from MIR, and none of those are worth the risk for a value that is read
/// once or twice.
fn is_plain_string_temp(mir: &Mir, function: &MirFunction, local: LocalId) -> bool {
    let Some(decl) = function.locals.get(usize::try_from(local.0).unwrap_or(usize::MAX)) else {
        return false;
    };
    matches!(decl.kind, LocalKind::Temp) && is_string_ty(mir, decl.ty)
}

/// Whether a `TypeId` resolves to the MIR string type.
fn is_string_ty(mir: &Mir, ty: TypeId) -> bool {
    mir.types.get(ty) == Some(&Type::String)
}

/// Whether every read of `local` in `function` sits in a supported position.
///
/// Counts total reads and supported reads through the same walk so an rvalue or
/// terminator shape this module has not considered can only ever lower the
/// supported tally, never pass unnoticed.
fn every_read_is_supported(
    function: &MirFunction,
    local: LocalId,
    candidates: &HashSet<LocalId>,
) -> bool {
    let mut total = 0usize;
    let mut supported = 0usize;
    let count = |reads: &mut usize, operand: &Operand| {
        if reads_local(operand, local) {
            *reads = reads.saturating_add(1);
        }
    };
    for block in &function.blocks {
        for phi in &block.phis {
            for (_, operand) in &phi.incoming {
                count(&mut total, operand);
            }
        }
        for statement in &block.statements {
            match statement {
                Statement::Assign { value, .. } => {
                    value.for_each_operand(|operand| count(&mut total, operand));
                    if let Rvalue::Binary { op, lhs, rhs } = value
                        && is_equality_op(*op)
                    {
                        for (side, other) in [(lhs, rhs), (rhs, lhs)] {
                            if reads_local(side, local) && is_comparable_side(other, candidates) {
                                supported = supported.saturating_add(1);
                            }
                        }
                    }
                }
                Statement::AssignPlace { place, value } => {
                    count_place_reads(place, &mut |operand| count(&mut total, operand));
                    value.for_each_operand(|operand| count(&mut total, operand));
                }
                Statement::DictEntryUpdate {
                    base,
                    index,
                    default,
                    current: _,
                    value,
                } => {
                    if *base == local {
                        total = total.saturating_add(1);
                    }
                    count(&mut total, index);
                    count(&mut total, default);
                    value.for_each_operand(|operand| count(&mut total, operand));
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }
        match &block.terminator {
            None | Some(Terminator::Goto(_) | Terminator::Unreachable) => {}
            Some(Terminator::Call { callee, args, .. }) => {
                if let Callee::Indirect(operand) = callee {
                    count(&mut total, operand);
                }
                for arg in args {
                    count(&mut total, arg);
                }
            }
            Some(
                Terminator::Await {
                    future: operand, ..
                }
                | Terminator::Switch { cond: operand, .. },
            ) => {
                count(&mut total, operand);
            }
            Some(Terminator::Match { scrutinee, .. }) => {
                count(&mut total, scrutinee);
                if reads_local(scrutinee, local) {
                    supported = supported.saturating_add(1);
                }
            }
            Some(Terminator::Return(operand) | Terminator::Throw(operand)) => {
                count(&mut total, operand);
            }
        }
    }
    total == supported
}

/// Whether the other side of an equality can be compared to a `&'static str`.
///
/// A string literal renders as an owned `String` and another accepted local
/// renders as a `&'static str`; `core`/`alloc` provide `PartialEq` both ways, so
/// either pairing type-checks. Any other operand keeps its own (unknown to this
/// module) rendering and is refused.
fn is_comparable_side(operand: &Operand, candidates: &HashSet<LocalId>) -> bool {
    match operand {
        Operand::Const(Constant::String(_)) => true,
        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => {
            candidates.contains(local)
        }
        _ => false,
    }
}

/// Whether an operand is a bare read of `local`.
fn reads_local(operand: &Operand, local: LocalId) -> bool {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => match place {
            Place::Local(candidate) | Place::Field { base: candidate, .. } => *candidate == local,
            Place::Index { base, index, .. } => *base == local || reads_local(index, local),
        },
        Operand::Const(_) => false,
    }
}

/// Count the local reads an assignment place performs before it writes.
fn count_place_reads(place: &Place, count: &mut impl FnMut(&Operand)) {
    match place {
        Place::Local(_) => {}
        Place::Field { base, .. } => count(&Operand::Copy(Place::Local(*base))),
        Place::Index { base, index, .. } => {
            count(&Operand::Copy(Place::Local(*base)));
            count(index);
        }
    }
}

/// Whether a binary operator compares two values for (in)equality.
fn is_equality_op(op: smelt_hir::BinOp) -> bool {
    matches!(
        op,
        smelt_hir::BinOp::Eq
            | smelt_hir::BinOp::NotEq
            | smelt_hir::BinOp::StrictEq
            | smelt_hir::BinOp::StrictNotEq
            | smelt_hir::BinOp::JsStrictEq
            | smelt_hir::BinOp::JsStrictNotEq
    )
}
