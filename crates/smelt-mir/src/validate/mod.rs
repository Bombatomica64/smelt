//! MIR validation and diagnostics.
//!
//! Validation is a read-only pass over a [`Mir`] value that reports structural
//! and dataflow problems as a flat list of [`ValidationError`]s. It never
//! mutates the IR and never bails early: every body is checked so callers see
//! the full set of problems in one run. It runs in release builds as well
//! (`compile` fails on any error), and after every optimization pass in debug
//! and test builds.
//!
//! The checks are split across focused submodules:
//!
//! - [`tables`] checks the crate-level tables: function ids equal their
//!   positions, class member and global initializer ids are in range, and no
//!   two classes (or two interfaces) share a name.
//! - [`structure`] verifies that block, local, and terminator references point
//!   at entities that actually exist in a body.
//! - [`operands`] walks each statement/rvalue and confirms the operands it
//!   reads reference valid places and globals.
//! - [`assignment`] runs the forward definite-assignment dataflow that catches
//!   reads of locals before they are provably defined.
//! - [`closures`] enforces the closure-table and capture ABI invariants.
//!
//! Every body check runs over [`Mir::bodies`], functions and closures alike,
//! and every type check over [`Mir::visit_types`], the one list of type
//! positions.

mod assignment;
mod closures;
mod operands;
mod structure;
mod tables;

use smelt_hir::TypeId;

use crate::{BodyOwner, Mir};

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
    tables::validate_tables(mir, &mut errors);
    closures::validate_closures(mir, &mut errors);
    for body in mir.bodies() {
        structure::validate_body(mir, body, &mut errors);
    }
    mir.visit_types(&mut |ty| validate_type(mir, ty, &mut errors));
    errors
}

/// Ensure a type ID points to an existing MIR type entry.
fn validate_type(mir: &Mir, ty: TypeId, errors: &mut Vec<ValidationError>) {
    if mir.types.get(ty).is_none() {
        errors.push(error(format!("MIR references unknown type {ty:?}")));
    }
}

/// Name a body for a diagnostic: the function's id and name, or the
/// closure's id.
fn owner_name(mir: &Mir, owner: BodyOwner) -> String {
    match owner {
        BodyOwner::Function(id) => {
            let name = mir
                .function(id)
                .and_then(|function| mir.symbols.get(function.name))
                .unwrap_or("<unknown>");
            format!("function {id:?} `{name}`")
        }
        BodyOwner::Closure(id) => format!("closure {id:?}"),
    }
}

/// Convert a local identifier into a vector index.
fn local_index(local: crate::LocalId) -> usize {
    local.index()
}

/// Convert a block identifier into a vector index.
fn block_index(block: crate::BlockId) -> usize {
    block.index()
}

/// Build a validation error with no source span.
const fn error(message: String) -> ValidationError {
    ValidationError { message }
}

#[cfg(test)]
mod tests;
