//! MIR validation and diagnostics.
//!
//! Validation is a read-only pass over a [`Mir`] value that reports structural
//! and dataflow problems as a flat list of [`ValidationError`]s. It never
//! mutates the IR and never bails early: every function is checked so callers
//! see the full set of problems in one run.
//!
//! The checks are split across focused submodules:
//!
//! - [`structure`] verifies that block, local, and terminator references point
//!   at entities that actually exist in the function.
//! - [`operands`] walks each statement/rvalue and confirms the operands it
//!   reads reference valid places, plus the canonical operand traversal used by
//!   the rest of the crate.
//! - [`assignment`] runs the forward definite-assignment dataflow that catches
//!   reads of locals before they are provably defined.
//! - [`closures`] enforces the closure-table and capture ABI invariants.
//!
//! Shared index/type helpers live in this module so every submodule can reach
//! them through `super`.

mod assignment;
mod closures;
mod operands;
mod structure;

use smelt_hir::TypeId;

use crate::Mir;

/// A validation error discovered while checking MIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Human-readable description of the problem.
    pub message: String,
}

/// Validates MIR and returns any structural errors.
#[must_use]
pub fn validate(mir: &Mir) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    closures::validate_closures(mir, &mut errors);
    for function in &mir.functions {
        structure::validate_function(mir, function, &mut errors);
    }
    errors
}

/// Ensure a type ID points to an existing MIR type entry.
fn validate_type(mir: &Mir, ty: TypeId, errors: &mut Vec<ValidationError>) {
    if mir.types.get(ty).is_none() {
        errors.push(error(format!("MIR references unknown type {ty:?}")));
    }
}

/// Convert a local identifier into a vector index.
fn local_index(local: crate::LocalId) -> usize {
    u32_to_usize(local.0)
}

/// Convert a block identifier into a vector index.
fn block_index(block: crate::BlockId) -> usize {
    u32_to_usize(block.0)
}

/// Convert a function identifier into a vector index.
fn function_index(function: crate::FuncId) -> usize {
    u32_to_usize(function.0)
}

/// Convert a `u32` identifier into a vector index.
///
/// MIR identifiers are always small enough to fit in `usize` on supported
/// targets; the saturating fallback keeps the validators total on the
/// (unreachable) 16-bit case so an out-of-range id simply fails the existence
/// check rather than panicking.
fn u32_to_usize(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// Build a validation error with no source span.
const fn error(message: String) -> ValidationError {
    ValidationError { message }
}
