//! Structural reference validation for a single MIR function.
//!
//! These checks confirm that a function's skeleton is internally consistent:
//! the entry block exists, block indices match their ids, every block has a
//! terminator, and each statement/terminator only references locals, blocks,
//! and types that are actually declared. Operand-level existence checks are
//! delegated to [`super::operands`]; definite-assignment dataflow is delegated
//! to [`super::assignment`].

use crate::{MirFunction, Mir, Statement, Terminator};

use super::operands::{
    validate_callee_exists, validate_operand_exists, validate_place_exists, validate_rvalue_exists,
};
use super::{ValidationError, assignment, block_index, error, local_index, validate_type};

/// Validate one MIR function and append any discovered errors.
pub(super) fn validate_function(
    mir: &Mir,
    function: &MirFunction,
    errors: &mut Vec<ValidationError>,
) {
    validate_entry(function, errors);
    for (block_idx, block) in function.blocks.iter().enumerate() {
        validate_block(mir, function, block_idx, block, errors);
    }
    for local in &function.locals {
        validate_type(mir, local.ty, errors);
    }
    assignment::validate_definite_assignment(mir, function, errors);
}

/// Check that the function's declared entry block exists.
fn validate_entry(function: &MirFunction, errors: &mut Vec<ValidationError>) {
    let entry_idx = block_index(function.entry);
    if function.blocks.get(entry_idx).is_none() {
        errors.push(error(format!(
            "function {:?} has an unknown entry block {:?}",
            function.id, function.entry
        )));
    }
}

/// Check one basic block's id, terminator presence, phis, statements, and
/// terminator references.
fn validate_block(
    mir: &Mir,
    function: &MirFunction,
    block_idx: usize,
    block: &crate::BasicBlock,
    errors: &mut Vec<ValidationError>,
) {
    if block_index(block.id) != block_idx {
        errors.push(error(format!(
            "function {:?} block index {block_idx} has mismatched id {:?}",
            function.id, block.id
        )));
    }
    if block.terminator.is_none() {
        errors.push(error(format!(
            "function {:?} block {:?} is missing a terminator",
            function.id, block.id
        )));
    }

    for phi in &block.phis {
        validate_type(mir, phi.ty, errors);
        validate_local_exists(function, phi.dest, errors);
        for (_, operand) in &phi.incoming {
            validate_operand_exists(function, operand, errors);
        }
    }

    for stmt in &block.statements {
        validate_statement(mir, function, stmt, errors);
    }

    if let Some(terminator) = &block.terminator {
        validate_terminator(mir, function, terminator, errors);
    }
}

/// Check that a statement's destinations and read operands reference valid
/// entities.
fn validate_statement(
    mir: &Mir,
    function: &MirFunction,
    stmt: &Statement,
    errors: &mut Vec<ValidationError>,
) {
    match stmt {
        Statement::Assign { dest, value } => {
            validate_rvalue_exists(mir, function, value, errors);
            validate_local_exists(function, *dest, errors);
        }
        Statement::AssignPlace { place, value } => {
            validate_place_exists(function, place, errors);
            validate_rvalue_exists(mir, function, value, errors);
        }
        Statement::StorageLive(local) | Statement::StorageDead(local) => {
            validate_local_exists(function, *local, errors);
        }
    }
}

/// Check that a terminator's operands, destinations, and successor blocks
/// reference valid entities.
fn validate_terminator(
    mir: &Mir,
    function: &MirFunction,
    terminator: &Terminator,
    errors: &mut Vec<ValidationError>,
) {
    match terminator {
        Terminator::Goto(target) => validate_block_exists(function, *target, errors),
        Terminator::Call {
            callee,
            args,
            dest,
            target,
            unwind,
        } => {
            validate_callee_exists(mir, function, callee, errors);
            for arg in args {
                validate_operand_exists(function, arg, errors);
            }
            validate_local_exists(function, *dest, errors);
            validate_block_exists(function, *target, errors);
            if let Some(handler) = unwind {
                validate_block_exists(function, handler.catch_block, errors);
                if let Some(local) = handler.exception_local {
                    validate_local_exists(function, local, errors);
                }
            }
        }
        Terminator::Await {
            future,
            dest,
            target,
            unwind,
        } => {
            validate_operand_exists(function, future, errors);
            validate_local_exists(function, *dest, errors);
            validate_block_exists(function, *target, errors);
            if let Some(handler) = unwind {
                validate_block_exists(function, handler.catch_block, errors);
                if let Some(local) = handler.exception_local {
                    validate_local_exists(function, local, errors);
                }
            }
        }
        Terminator::Switch {
            cond,
            then_block,
            else_block,
        } => {
            validate_operand_exists(function, cond, errors);
            validate_block_exists(function, *then_block, errors);
            validate_block_exists(function, *else_block, errors);
        }
        Terminator::Match {
            scrutinee,
            arms,
            default,
        } => {
            validate_operand_exists(function, scrutinee, errors);
            for arm in arms {
                validate_block_exists(function, arm.target, errors);
            }
            if let Some(default_target) = default {
                validate_block_exists(function, *default_target, errors);
            }
        }
        Terminator::Return(operand) | Terminator::Throw(operand) => {
            validate_operand_exists(function, operand, errors);
        }
        Terminator::Unreachable => {}
    }
}

/// Ensure a local ID points to an existing local declaration.
pub(super) fn validate_local_exists(
    function: &MirFunction,
    local: crate::LocalId,
    errors: &mut Vec<ValidationError>,
) {
    if function.locals.get(local_index(local)).is_none() {
        errors.push(error(format!(
            "function {:?} references unknown local {:?}",
            function.id, local
        )));
    }
}

/// Ensure a block ID points to an existing basic block.
pub(super) fn validate_block_exists(
    function: &MirFunction,
    block: crate::BlockId,
    errors: &mut Vec<ValidationError>,
) {
    if function.blocks.get(block_index(block)).is_none() {
        errors.push(error(format!(
            "function {:?} references unknown block {:?}",
            function.id, block
        )));
    }
}
