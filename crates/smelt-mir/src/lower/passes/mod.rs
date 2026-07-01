//! Whole-program MIR analyses that run after core HIR-to-MIR lowering.
//!
//! These passes operate over already-lowered MIR and are kept separate from
//! expression lowering so ABI and capture invariants have a narrow home.
//!
//! Closure-specific analyses (escape analysis and throwing-closure type
//! widening) live in [`super::closures`], which owns the closure ABI end to
//! end. This module keeps only the general function-throwing propagation in
//! [`throwing`], whose `can_throw` results the closure widening consumes.

pub(super) mod throwing;

use crate::{LocalId, Operand, Place};

/// Return the local behind a plain local operand.
pub(super) const fn operand_local(operand: &Operand) -> Option<LocalId> {
    match operand {
        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => Some(*local),
        Operand::Const(_) | Operand::Copy(_) | Operand::Move(_) => None,
    }
}
