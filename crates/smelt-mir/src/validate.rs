use std::collections::{HashSet, VecDeque};

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
            validate_local_exists(function, phi.dest, errors);
            for (_, operand) in &phi.incoming {
                validate_operand_exists(function, operand, errors);
            }
        }

        for stmt in &block.statements {
            match stmt {
                Statement::Assign { dest, value } => {
                    validate_rvalue_exists(function, value, errors);
                    validate_local_exists(function, *dest, errors);
                }
                Statement::AssignPlace { place, value } => {
                    validate_place_exists(function, place, errors);
                    validate_rvalue_exists(function, value, errors);
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
                    validate_callee_exists(mir, function, callee, errors);
                    for arg in args {
                        validate_operand_exists(function, arg, errors);
                    }
                    validate_local_exists(function, *dest, errors);
                    validate_block_exists(function, *target, errors);
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
                    if let Some(default) = default {
                        validate_block_exists(function, *default, errors);
                    }
                }
                Terminator::Return(operand) => {
                    validate_operand_exists(function, operand, errors);
                }
                Terminator::Unreachable => {}
            }
        }
    }

    for local in &function.locals {
        validate_type(mir, local.ty, errors);
    }

    validate_definite_assignment(function, errors);
}

fn validate_rvalue_exists(
    function: &MirFunction,
    value: &Rvalue,
    errors: &mut Vec<ValidationError>,
) {
    match value {
        Rvalue::Use(operand) => validate_operand_exists(function, operand, errors),
        Rvalue::List(items) | Rvalue::Tuple(items) => {
            for item in items {
                validate_operand_exists(function, item, errors);
            }
        }
        Rvalue::Dict(entries) => {
            for (key, value) in entries {
                validate_operand_exists(function, key, errors);
                validate_operand_exists(function, value, errors);
            }
        }
        Rvalue::Binary { lhs, rhs, .. } => {
            validate_operand_exists(function, lhs, errors);
            validate_operand_exists(function, rhs, errors);
        }
        Rvalue::Unary { operand, .. } => validate_operand_exists(function, operand, errors),
    }
}

fn validate_operand_exists(
    function: &MirFunction,
    operand: &Operand,
    errors: &mut Vec<ValidationError>,
) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            validate_place_exists(function, place, errors);
        }
        Operand::Const(_) => {}
    }
}

fn validate_place_exists(function: &MirFunction, place: &Place, errors: &mut Vec<ValidationError>) {
    match place {
        Place::Local(local) => {
            validate_local_exists(function, *local, errors);
        }
        Place::Field { base, .. } => validate_local_exists(function, *base, errors),
        Place::Index { base, index } => {
            validate_local_exists(function, *base, errors);
            validate_operand_exists(function, index, errors);
        }
    }
}

fn validate_callee_exists(
    mir: &Mir,
    function: &MirFunction,
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
            validate_operand_exists(function, operand, errors);
        }
        Callee::Builtin(_) => {}
    }
}

fn validate_definite_assignment(function: &MirFunction, errors: &mut Vec<ValidationError>) {
    if function.blocks.get(function.entry.0 as usize).is_none() {
        return;
    }

    let block_count = function.blocks.len();
    let mut in_sets = vec![None::<HashSet<LocalId>>; block_count];
    let mut queue = VecDeque::new();
    let mut entry_defs = HashSet::new();
    entry_defs.extend(function.params.iter().copied());
    in_sets[function.entry.0 as usize] = Some(entry_defs);
    queue.push_back(function.entry);

    while let Some(block_id) = queue.pop_front() {
        let Some(block) = function.blocks.get(block_id.0 as usize) else {
            continue;
        };
        let mut definitions = in_sets[block_id.0 as usize].clone().unwrap_or_default();
        let before_phi = definitions.clone();

        for phi in &block.phis {
            for (_, operand) in &phi.incoming {
                validate_operand(function, &before_phi, operand, errors);
            }
            definitions.insert(phi.dest);
        }

        for stmt in &block.statements {
            match stmt {
                Statement::Assign { dest, value } => {
                    validate_rvalue(function, &definitions, value, errors);
                    definitions.insert(*dest);
                }
                Statement::AssignPlace { place, value } => {
                    validate_rvalue(function, &definitions, value, errors);
                    match place {
                        Place::Local(local) => {
                            definitions.insert(*local);
                        }
                        Place::Field { .. } | Place::Index { .. } => {
                            validate_place(function, &definitions, place, errors);
                        }
                    }
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }

        if let Some(terminator) = &block.terminator {
            match terminator {
                Terminator::Goto(_) | Terminator::Unreachable => {}
                Terminator::Call {
                    callee, args, dest, ..
                } => {
                    validate_callee(function, &definitions, callee, errors);
                    for arg in args {
                        validate_operand(function, &definitions, arg, errors);
                    }
                    definitions.insert(*dest);
                }
                Terminator::Switch { cond, .. } => {
                    validate_operand(function, &definitions, cond, errors);
                }
                Terminator::Match { scrutinee, .. } => {
                    validate_operand(function, &definitions, scrutinee, errors);
                }
                Terminator::Return(operand) => {
                    validate_operand(function, &definitions, operand, errors);
                }
            }

            for successor in successors(terminator) {
                let idx = successor.0 as usize;
                if idx >= block_count {
                    continue;
                }
                let changed = if let Some(existing) = &mut in_sets[idx] {
                    let intersection = existing
                        .intersection(&definitions)
                        .copied()
                        .collect::<HashSet<_>>();
                    let changed = *existing != intersection;
                    *existing = intersection;
                    changed
                } else {
                    in_sets[idx] = Some(definitions.clone());
                    true
                };
                if changed {
                    queue.push_back(successor);
                }
            }
        }
    }
}

fn successors(terminator: &Terminator) -> Vec<crate::BlockId> {
    match terminator {
        Terminator::Goto(target) => vec![*target],
        Terminator::Call { target, .. } => vec![*target],
        Terminator::Switch {
            then_block,
            else_block,
            ..
        } => vec![*then_block, *else_block],
        Terminator::Match { arms, default, .. } => {
            arms.iter().map(|arm| arm.target).chain(*default).collect()
        }
        Terminator::Return(_) | Terminator::Unreachable => Vec::new(),
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
        Rvalue::List(items) | Rvalue::Tuple(items) => {
            for item in items {
                validate_operand(function, definitions, item, errors);
            }
        }
        Rvalue::Dict(entries) => {
            for (key, value) in entries {
                validate_operand(function, definitions, key, errors);
                validate_operand(function, definitions, value, errors);
            }
        }
        Rvalue::Binary { lhs, rhs, .. } => {
            validate_operand(function, definitions, lhs, errors);
            validate_operand(function, definitions, rhs, errors);
        }
        Rvalue::Unary { operand, .. } => validate_operand(function, definitions, operand, errors),
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
            if !definitions.contains(local) {
                errors.push(error(format!(
                    "function {:?} reads local {:?} before it is definitely defined",
                    function.id, local
                )));
            }
        }
        Place::Field { base, .. } => {
            validate_place(function, definitions, &Place::Local(*base), errors)
        }
        Place::Index { base, index } => {
            validate_place(function, definitions, &Place::Local(*base), errors);
            validate_operand(function, definitions, index, errors);
        }
    }
}

fn validate_callee(
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    callee: &Callee,
    errors: &mut Vec<ValidationError>,
) {
    match callee {
        Callee::Static(_) | Callee::Builtin(_) => {}
        Callee::Indirect(operand) => {
            validate_operand(function, definitions, operand, errors);
        }
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
