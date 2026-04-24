use std::collections::HashSet;

use smelt_hir::TypeId;

use crate::{Callee, LocalId, Mir, MirFunction, Operand, Place, Rvalue, Statement, Terminator};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub message: String,
}

#[must_use]
pub fn validate(mir: &Mir) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for function in &mir.functions {
        validate_function(mir, function, &mut errors);
    }
    errors
}

fn validate_function(mir: &Mir, function: &MirFunction, errors: &mut Vec<ValidationError>) {
    if function.blocks.get(function.entry.0 as usize).is_none() {
        errors.push(error(format!(
            "function {:?} has an unknown entry block {:?}",
            function.id, function.entry
        )));
    }

    let mut definitions = HashSet::new();
    for param in &function.params {
        definitions.insert(*param);
    }

    for (block_idx, block) in function.blocks.iter().enumerate() {
        if block.id.0 as usize != block_idx {
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
            for (_, operand) in &phi.incoming {
                validate_operand(function, &definitions, operand, errors);
            }
            define_local(function, &mut definitions, phi.dest, errors);
        }

        for stmt in &block.statements {
            match stmt {
                Statement::Assign { dest, value } => {
                    validate_rvalue(function, &definitions, value, errors);
                    define_local(function, &mut definitions, *dest, errors);
                }
                Statement::StorageLive(local) | Statement::StorageDead(local) => {
                    validate_local_exists(function, *local, errors);
                }
            }
        }

        if let Some(terminator) = &block.terminator {
            match terminator {
                Terminator::Goto(target) => validate_block_exists(function, *target, errors),
                Terminator::Call {
                    callee,
                    args,
                    dest,
                    target,
                } => {
                    validate_callee(mir, function, &definitions, callee, errors);
                    for arg in args {
                        validate_operand(function, &definitions, arg, errors);
                    }
                    define_local(function, &mut definitions, *dest, errors);
                    validate_block_exists(function, *target, errors);
                }
                Terminator::Return(operand) => {
                    validate_operand(function, &definitions, operand, errors);
                }
                Terminator::Unreachable => {}
            }
        }
    }

    for local in &function.locals {
        validate_type(mir, local.ty, errors);
    }
}

fn define_local(
    function: &MirFunction,
    definitions: &mut HashSet<LocalId>,
    local: LocalId,
    errors: &mut Vec<ValidationError>,
) {
    validate_local_exists(function, local, errors);
    if !definitions.insert(local) {
        errors.push(error(format!(
            "function {:?} local {:?} is defined more than once",
            function.id, local
        )));
    }
}

fn validate_rvalue(
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    value: &Rvalue,
    errors: &mut Vec<ValidationError>,
) {
    match value {
        Rvalue::Use(operand) => validate_operand(function, definitions, operand, errors),
    }
}

fn validate_operand(
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    operand: &Operand,
    errors: &mut Vec<ValidationError>,
) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            validate_place(function, definitions, place, errors);
        }
        Operand::Const(_) => {}
    }
}

fn validate_place(
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    place: &Place,
    errors: &mut Vec<ValidationError>,
) {
    match place {
        Place::Local(local) => {
            validate_local_exists(function, *local, errors);
            if !definitions.contains(local) {
                errors.push(error(format!(
                    "function {:?} reads local {:?} before it is defined",
                    function.id, local
                )));
            }
        }
    }
}

fn validate_callee(
    mir: &Mir,
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    callee: &Callee,
    errors: &mut Vec<ValidationError>,
) {
    match callee {
        Callee::Static(func) => {
            if mir.functions.get(func.0 as usize).is_none() {
                errors.push(error(format!("call references unknown function {func:?}")));
            }
        }
        Callee::Indirect(operand) => {
            validate_operand(function, definitions, operand, errors);
        }
        Callee::Builtin(_) => {}
    }
}

fn validate_local_exists(
    function: &MirFunction,
    local: LocalId,
    errors: &mut Vec<ValidationError>,
) {
    if function.locals.get(local.0 as usize).is_none() {
        errors.push(error(format!(
            "function {:?} references unknown local {:?}",
            function.id, local
        )));
    }
}

fn validate_block_exists(
    function: &MirFunction,
    block: crate::BlockId,
    errors: &mut Vec<ValidationError>,
) {
    if function.blocks.get(block.0 as usize).is_none() {
        errors.push(error(format!(
            "function {:?} references unknown block {:?}",
            function.id, block
        )));
    }
}

fn validate_type(mir: &Mir, ty: TypeId, errors: &mut Vec<ValidationError>) {
    if mir.types.get(ty).is_none() {
        errors.push(error(format!("MIR references unknown type {ty:?}")));
    }
}

fn error(message: String) -> ValidationError {
    ValidationError { message }
}
