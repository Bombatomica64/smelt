//! Structural reference validation for a single MIR body, a function's or a
//! closure's.
//!
//! These checks confirm that a body's skeleton is internally consistent:
//! the entry block exists, block indices match their ids, every block has a
//! terminator, and each statement/terminator only references locals, blocks,
//! and types that are actually declared. Operand-level existence checks are
//! delegated to [`super::operands`]; definite-assignment dataflow is delegated
//! to [`super::assignment`].

use crate::{MirBody, Mir, Statement, Terminator};

use super::operands::{
    validate_callee_exists, validate_operand_exists, validate_place_exists, validate_rvalue_exists,
};
use super::{ValidationError, assignment, block_index, error, local_index};

/// Validate one MIR body and append any discovered errors.
///
/// Runs for every body [`Mir::bodies`] yields, so closure bodies get the same
/// terminator, id and definite-assignment checks as functions.
pub(super) fn validate_body(
    mir: &Mir,
    body: MirBody<'_>,
    errors: &mut Vec<ValidationError>,
) {
    validate_entry(body, errors);
    for (block_idx, block) in body.blocks.iter().enumerate() {
        validate_block(mir, body, block_idx, block, errors);
    }
    for &param in body.params {
        validate_local_exists(body, param, errors);
    }
    assignment::validate_definite_assignment(mir, body, errors);
}

/// Check that the function's declared entry block exists.
fn validate_entry(body: MirBody<'_>, errors: &mut Vec<ValidationError>) {
    let entry_idx = block_index(body.entry);
    if body.blocks.get(entry_idx).is_none() {
        errors.push(error(format!(
            "{:?} has an unknown entry block {:?}",
            body.owner, body.entry
        )));
    }
}

/// Check one basic block's id, terminator presence, statements, and
/// terminator references.
fn validate_block(
    mir: &Mir,
    body: MirBody<'_>,
    block_idx: usize,
    block: &crate::BasicBlock,
    errors: &mut Vec<ValidationError>,
) {
    if block_index(block.id) != block_idx {
        errors.push(error(format!(
            "{:?} block index {block_idx} has mismatched id {:?}",
            body.owner, block.id
        )));
    }
    if block.terminator.is_none() {
        errors.push(error(format!(
            "{:?} block {:?} is missing a terminator",
            body.owner, block.id
        )));
    }


    for stmt in &block.statements {
        validate_statement(mir, body, stmt, errors);
    }

    if let Some(terminator) = &block.terminator {
        validate_terminator(mir, body, terminator, errors);
    }
}

/// Check that a statement's destinations and read operands reference valid
/// entities.
fn validate_statement(
    mir: &Mir,
    body: MirBody<'_>,
    stmt: &Statement,
    errors: &mut Vec<ValidationError>,
) {
    match stmt {
        Statement::Assign { dest, value } => {
            validate_rvalue_exists(mir, body, value, errors);
            validate_local_exists(body, *dest, errors);
        }
        Statement::AssignPlace { place, value } => {
            validate_place_exists(mir, body, place, errors);
            validate_rvalue_exists(mir, body, value, errors);
        }
        Statement::DictEntryUpdate {
            base,
            index,
            default,
            current,
            value,
        } => {
            validate_place_exists(
                mir,
                body,
                &crate::Place::Index {
                    base: *base,
                    index: Box::new(index.clone()),
                    // `validate_place_exists` only resolves the base local and
                    // the index operand, so the negative-index rule carried by
                    // a real place is not consulted here.
                    negative: crate::NegativeIndex::OutOfRange,
                },
                errors,
            );
            validate_operand_exists(mir, body, default, errors);
            validate_local_exists(body, *current, errors);
            validate_rvalue_exists(mir, body, value, errors);
        }
    }
}

/// Check that a terminator's operands, destinations, and successor blocks
/// reference valid entities.
fn validate_terminator(
    mir: &Mir,
    body: MirBody<'_>,
    terminator: &Terminator,
    errors: &mut Vec<ValidationError>,
) {
    match terminator {
        Terminator::Goto(target) => validate_block_exists(body, *target, errors),
        Terminator::Call {
            callee,
            args,
            dest,
            target,
            unwind,
        } => {
            validate_callee_exists(mir, body, callee, errors);
            for arg in args {
                validate_operand_exists(mir, body, arg, errors);
            }
            validate_local_exists(body, *dest, errors);
            validate_block_exists(body, *target, errors);
            if let Some(handler) = unwind {
                validate_block_exists(body, handler.catch_block, errors);
                if let Some(local) = handler.exception_local {
                    validate_local_exists(body, local, errors);
                }
            }
        }
        Terminator::Await {
            future,
            dest,
            target,
            unwind,
        } => {
            validate_operand_exists(mir, body, future, errors);
            validate_local_exists(body, *dest, errors);
            validate_block_exists(body, *target, errors);
            if let Some(handler) = unwind {
                validate_block_exists(body, handler.catch_block, errors);
                if let Some(local) = handler.exception_local {
                    validate_local_exists(body, local, errors);
                }
            }
        }
        Terminator::Switch {
            cond,
            then_block,
            else_block,
        } => {
            validate_operand_exists(mir, body, cond, errors);
            validate_block_exists(body, *then_block, errors);
            validate_block_exists(body, *else_block, errors);
        }
        Terminator::Match {
            scrutinee,
            arms,
            default,
        } => {
            validate_operand_exists(mir, body, scrutinee, errors);
            for arm in arms {
                validate_block_exists(body, arm.target, errors);
            }
            if let Some(default_target) = default {
                validate_block_exists(body, *default_target, errors);
            }
        }
        Terminator::Return(operand) | Terminator::Throw(operand) => {
            validate_operand_exists(mir, body, operand, errors);
        }
    }
}

/// Ensure a local ID points to an existing local declaration.
pub(super) fn validate_local_exists(
    body: MirBody<'_>,
    local: crate::LocalId,
    errors: &mut Vec<ValidationError>,
) {
    if body.locals.get(local_index(local)).is_none() {
        errors.push(error(format!(
            "{:?} references unknown local {:?}",
            body.owner, local
        )));
    }
}

/// Ensure a block ID points to an existing basic block.
pub(super) fn validate_block_exists(
    body: MirBody<'_>,
    block: crate::BlockId,
    errors: &mut Vec<ValidationError>,
) {
    if body.blocks.get(block_index(block)).is_none() {
        errors.push(error(format!(
            "{:?} references unknown block {:?}",
            body.owner, block
        )));
    }
}
