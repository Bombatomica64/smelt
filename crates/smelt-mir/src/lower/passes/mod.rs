//! Whole-program MIR analyses that run after core HIR-to-MIR lowering.
//!
//! These passes operate over already-lowered MIR and are kept separate from
//! expression lowering so ABI and capture invariants have a narrow home.

pub(super) mod aliases;
pub(super) mod closure_types;
pub(super) mod escaping;
pub(super) mod throwing;

use crate::{LocalId, Operand, Place};

/// Return the local behind a plain local operand.
pub(super) fn operand_local(operand: &Operand) -> Option<LocalId> {
    match operand {
        Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local)) => Some(*local),
        Operand::Const(_) | Operand::Copy(_) | Operand::Move(_) => None,
    }
}
