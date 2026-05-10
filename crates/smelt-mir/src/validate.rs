use std::collections::{HashSet, VecDeque};

use smelt_hir::TypeId;

use crate::{Callee, LocalId, Mir, MirFunction, Operand, Place, Rvalue, Statement, Terminator};

/// A validation error discovered while checking MIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Human-readable description of the problem.
    pub message: String,
}

#[must_use]
/// Validates MIR and returns any structural errors.
pub fn validate(mir: &Mir) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for function in &mir.functions {
        validate_function(mir, function, &mut errors);
    }
    errors
}

/// Validate one MIR function and append any discovered errors.
fn validate_function(mir: &Mir, function: &MirFunction, errors: &mut Vec<ValidationError>) {
    let entry_idx = block_index(function.entry);
    if function.blocks.get(entry_idx).is_none() {
        errors.push(error(format!(
            "function {:?} has an unknown entry block {:?}",
            function.id, function.entry
        )));
    }

    for (block_idx, block) in function.blocks.iter().enumerate() {
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
    }

    for local in &function.locals {
        validate_type(mir, local.ty, errors);
    }

    validate_definite_assignment(function, errors);
}

/// Validate that IDs referenced by an rvalue point to existing MIR entities.
fn validate_rvalue_exists(
    function: &MirFunction,
    value: &Rvalue,
    errors: &mut Vec<ValidationError>,
) {
    match value {
        Rvalue::Use(operand) => validate_operand_exists(function, operand, errors),
        Rvalue::List(items) | Rvalue::Set(items) | Rvalue::Tuple(items) => {
            for item in items {
                validate_operand_exists(function, item, errors);
            }
        }
        Rvalue::Dict(entries) => {
            for (key, entry_value) in entries {
                validate_operand_exists(function, key, errors);
                validate_operand_exists(function, entry_value, errors);
            }
        }
        Rvalue::Binary { lhs, rhs, .. } => {
            validate_operand_exists(function, lhs, errors);
            validate_operand_exists(function, rhs, errors);
        }
        Rvalue::StringContains { haystack, needle } => {
            validate_operand_exists(function, haystack, errors);
            validate_operand_exists(function, needle, errors);
        }
        Rvalue::StringAffix {
            haystack, needle, ..
        }
        | Rvalue::StringSearch {
            haystack, needle, ..
        } => {
            validate_operand_exists(function, haystack, errors);
            validate_operand_exists(function, needle, errors);
        }
        Rvalue::StringReplace {
            haystack,
            pattern,
            replacement,
            ..
        } => {
            validate_operand_exists(function, haystack, errors);
            validate_operand_exists(function, pattern, errors);
            validate_operand_exists(function, replacement, errors);
        }
        Rvalue::StringRemoveAffix {
            haystack, affix, ..
        } => {
            validate_operand_exists(function, haystack, errors);
            validate_operand_exists(function, affix, errors);
        }
        Rvalue::StringRepeat { operand, count } => {
            validate_operand_exists(function, operand, errors);
            validate_operand_exists(function, count, errors);
        }
        Rvalue::StringPad {
            operand,
            target_len,
            pad,
            ..
        } => {
            validate_operand_exists(function, operand, errors);
            validate_operand_exists(function, target_len, errors);
            validate_operand_exists(function, pad, errors);
        }
        Rvalue::StringCharAt { operand, index } => {
            validate_operand_exists(function, operand, errors);
            validate_operand_exists(function, index, errors);
        }
        Rvalue::StringCharCodeAt { operand, index } => {
            validate_operand_exists(function, operand, errors);
            validate_operand_exists(function, index, errors);
        }
        Rvalue::StringSlice {
            operand,
            start,
            end,
        } => {
            validate_operand_exists(function, operand, errors);
            validate_optional_operand_exists(function, start.as_ref(), errors);
            validate_optional_operand_exists(function, end.as_ref(), errors);
        }
        Rvalue::ListContains { list, item } => {
            validate_operand_exists(function, list, errors);
            validate_operand_exists(function, item, errors);
        }
        Rvalue::SetContains { set, item } => {
            validate_operand_exists(function, set, errors);
            validate_operand_exists(function, item, errors);
        }
        Rvalue::ListConcat { left, right } => {
            validate_operand_exists(function, left, errors);
            validate_operand_exists(function, right, errors);
        }
        Rvalue::ListSearch { list, item, .. } => {
            validate_operand_exists(function, list, errors);
            validate_operand_exists(function, item, errors);
        }
        Rvalue::ListCallback { list, .. } => {
            validate_operand_exists(function, list, errors);
        }
        Rvalue::ListReduce { list, initial, .. } => {
            validate_operand_exists(function, list, errors);
            validate_operand_exists(function, initial, errors);
        }
        Rvalue::ListSlice { list, start, end } => {
            validate_operand_exists(function, list, errors);
            validate_optional_operand_exists(function, start.as_ref(), errors);
            validate_optional_operand_exists(function, end.as_ref(), errors);
        }
        Rvalue::ListPush { list, item } => {
            validate_operand_exists(function, list, errors);
            validate_operand_exists(function, item, errors);
        }
        Rvalue::ListExtend { list, other } => {
            validate_operand_exists(function, list, errors);
            validate_operand_exists(function, other, errors);
        }
        Rvalue::ListInsert { list, index, item } => {
            validate_operand_exists(function, list, errors);
            validate_operand_exists(function, index, errors);
            validate_operand_exists(function, item, errors);
        }
        Rvalue::ListUnshift { list, items } => {
            validate_operand_exists(function, list, errors);
            for item in items {
                validate_operand_exists(function, item, errors);
            }
        }
        Rvalue::ListReverse { list } => {
            validate_operand_exists(function, list, errors);
        }
        Rvalue::ListClear { list } => {
            validate_operand_exists(function, list, errors);
        }
        Rvalue::ListCopy { list } => {
            validate_operand_exists(function, list, errors);
        }
        Rvalue::ListCount { list, item } => {
            validate_operand_exists(function, list, errors);
            validate_operand_exists(function, item, errors);
        }
        Rvalue::ListSum { list }
        | Rvalue::ListBoolFold { list, .. }
        | Rvalue::ListSorted { list } => {
            validate_operand_exists(function, list, errors);
        }
        Rvalue::ListIndex { list, item } => {
            validate_operand_exists(function, list, errors);
            validate_operand_exists(function, item, errors);
        }
        Rvalue::ListRemove { list, item } => {
            validate_operand_exists(function, list, errors);
            validate_operand_exists(function, item, errors);
        }
        Rvalue::ListSort { list } => {
            validate_operand_exists(function, list, errors);
        }
        Rvalue::ListPop { list } => {
            validate_operand_exists(function, list, errors);
        }
        Rvalue::ListShift { list } => {
            validate_operand_exists(function, list, errors);
        }
        Rvalue::TupleContains { tuple, item } => {
            validate_operand_exists(function, tuple, errors);
            validate_operand_exists(function, item, errors);
        }
        Rvalue::DictContainsKey { dict, key } => {
            validate_operand_exists(function, dict, errors);
            validate_operand_exists(function, key, errors);
        }
        Rvalue::DictGet { dict, key, default } => {
            validate_operand_exists(function, dict, errors);
            validate_operand_exists(function, key, errors);
            validate_optional_operand_exists(function, default.as_ref(), errors);
        }
        Rvalue::DictSetDefault { dict, key, default } => {
            validate_operand_exists(function, dict, errors);
            validate_operand_exists(function, key, errors);
            validate_operand_exists(function, default, errors);
        }
        Rvalue::DictClear { dict } => {
            validate_operand_exists(function, dict, errors);
        }
        Rvalue::DictPop { dict, key, default } => {
            validate_operand_exists(function, dict, errors);
            validate_operand_exists(function, key, errors);
            validate_optional_operand_exists(function, default.as_ref(), errors);
        }
        Rvalue::DictUpdate { dict, other } => {
            validate_operand_exists(function, dict, errors);
            validate_operand_exists(function, other, errors);
        }
        Rvalue::DictCopy { dict } => {
            validate_operand_exists(function, dict, errors);
        }
        Rvalue::DictProjection { dict, .. } => {
            validate_operand_exists(function, dict, errors);
        }
        Rvalue::StringSplit {
            haystack,
            separator,
        } => {
            validate_operand_exists(function, haystack, errors);
            validate_operand_exists(function, separator, errors);
        }
        Rvalue::StringJoin { items, separator } => {
            validate_operand_exists(function, items, errors);
            validate_operand_exists(function, separator, errors);
        }
        Rvalue::JsonStringify { value: json_value } => {
            validate_operand_exists(function, json_value, errors);
        }
        Rvalue::JsonParse { text } => {
            validate_operand_exists(function, text, errors);
        }
        Rvalue::RegexIsMatch {
            pattern, haystack, ..
        } => {
            validate_operand_exists(function, pattern, errors);
            validate_operand_exists(function, haystack, errors);
        }
        Rvalue::HttpGetText { url } => {
            validate_operand_exists(function, url, errors);
        }
        Rvalue::NumericExtrema { args, .. } => {
            for arg in args {
                validate_operand_exists(function, arg, errors);
            }
        }
        Rvalue::NumericHypot { args } => {
            for arg in args {
                validate_operand_exists(function, arg, errors);
            }
        }
        Rvalue::NumericPow { base, exponent } => {
            validate_operand_exists(function, base, errors);
            validate_operand_exists(function, exponent, errors);
        }
        Rvalue::NumericAtan2 { y, x } => {
            validate_operand_exists(function, y, errors);
            validate_operand_exists(function, x, errors);
        }
        Rvalue::NumericRandom => {}
        Rvalue::Unary { operand, .. } => validate_operand_exists(function, operand, errors),
        Rvalue::Struct { fields, .. } => {
            for (_, field_value) in fields {
                validate_operand_exists(function, field_value, errors);
            }
        }
        Rvalue::Len(operand)
        | Rvalue::NumericAbs(operand)
        | Rvalue::NumericRound { operand, .. }
        | Rvalue::NumericPredicate { operand, .. }
        | Rvalue::NumericUnaryFunc { operand, .. }
        | Rvalue::StringCase { operand, .. }
        | Rvalue::StringTrim { operand, .. }
        | Rvalue::StringPredicate { operand, .. }
        | Rvalue::Await(operand) => validate_operand_exists(function, operand, errors),
        Rvalue::AsyncOp { args, .. } => {
            for arg in args {
                validate_operand_exists(function, arg, errors);
            }
        }
    }
}

/// Validate that IDs referenced by an operand point to existing MIR entities.
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

/// Validate that an optional operand points to existing MIR entities when present.
fn validate_optional_operand_exists(
    function: &MirFunction,
    maybe_operand: Option<&Operand>,
    errors: &mut Vec<ValidationError>,
) {
    if let Some(inner) = maybe_operand {
        validate_operand_exists(function, inner, errors);
    }
}

/// Validate that a place references valid locals and projected fields.
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

/// Validate that a callee target exists and references valid receiver places.
fn validate_callee_exists(
    mir: &Mir,
    function: &MirFunction,
    callee: &Callee,
    errors: &mut Vec<ValidationError>,
) {
    match callee {
        Callee::Static(func) => {
            if mir.functions.get(function_index(*func)).is_none() {
                errors.push(error(format!("call references unknown function {func:?}")));
            }
        }
        Callee::Indirect(operand) => {
            validate_operand_exists(function, operand, errors);
        }
        Callee::Builtin(_) => {}
    }
}

/// Perform forward dataflow to ensure locals are assigned before use.
fn validate_definite_assignment(function: &MirFunction, errors: &mut Vec<ValidationError>) {
    let entry_idx = block_index(function.entry);
    if function.blocks.get(entry_idx).is_none() {
        return;
    }

    let block_count = function.blocks.len();
    let mut in_sets = vec![None::<HashSet<LocalId>>; block_count];
    let mut queue = VecDeque::new();
    let mut entry_defs = HashSet::new();
    entry_defs.extend(function.params.iter().copied());
    if let Some(slot) = in_sets.get_mut(entry_idx) {
        *slot = Some(entry_defs);
    } else {
        return;
    }
    queue.push_back(function.entry);

    while let Some(block_id) = queue.pop_front() {
        let block_idx = block_index(block_id);
        let Some(block) = function.blocks.get(block_idx) else {
            continue;
        };
        let Some(definitions_slot) = in_sets.get(block_idx) else {
            continue;
        };
        let mut definitions = definitions_slot.clone().unwrap_or_default();
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
                Terminator::Return(operand) | Terminator::Throw(operand) => {
                    validate_operand(function, &definitions, operand, errors);
                }
            }

            for successor in successors(terminator) {
                let Some(existing) = in_sets.get_mut(block_index(successor)) else {
                    continue;
                };
                let changed = if let Some(existing_defs) = existing {
                    let intersection = existing_defs
                        .intersection(&definitions)
                        .copied()
                        .collect::<HashSet<_>>();
                    let changed = *existing_defs != intersection;
                    *existing_defs = intersection;
                    changed
                } else {
                    *existing = Some(definitions.clone());
                    true
                };
                if changed {
                    queue.push_back(successor);
                }
            }
        }
    }
}

/// Return control-flow successors for a block terminator.
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
        Terminator::Return(_) | Terminator::Throw(_) | Terminator::Unreachable => Vec::new(),
    }
}

/// Validate type constraints for one rvalue.
fn validate_rvalue(
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    value: &Rvalue,
    errors: &mut Vec<ValidationError>,
) {
    match value {
        Rvalue::Use(operand) => validate_operand(function, definitions, operand, errors),
        Rvalue::List(items) | Rvalue::Set(items) | Rvalue::Tuple(items) => {
            for item in items {
                validate_operand(function, definitions, item, errors);
            }
        }
        Rvalue::Dict(entries) => {
            for (key, entry_value) in entries {
                validate_operand(function, definitions, key, errors);
                validate_operand(function, definitions, entry_value, errors);
            }
        }
        Rvalue::Binary { lhs, rhs, .. } => {
            validate_operand(function, definitions, lhs, errors);
            validate_operand(function, definitions, rhs, errors);
        }
        Rvalue::StringContains { haystack, needle } => {
            validate_operand(function, definitions, haystack, errors);
            validate_operand(function, definitions, needle, errors);
        }
        Rvalue::StringAffix {
            haystack, needle, ..
        }
        | Rvalue::StringSearch {
            haystack, needle, ..
        } => {
            validate_operand(function, definitions, haystack, errors);
            validate_operand(function, definitions, needle, errors);
        }
        Rvalue::StringReplace {
            haystack,
            pattern,
            replacement,
            ..
        } => {
            validate_operand(function, definitions, haystack, errors);
            validate_operand(function, definitions, pattern, errors);
            validate_operand(function, definitions, replacement, errors);
        }
        Rvalue::StringRemoveAffix {
            haystack, affix, ..
        } => {
            validate_operand(function, definitions, haystack, errors);
            validate_operand(function, definitions, affix, errors);
        }
        Rvalue::StringRepeat { operand, count } => {
            validate_operand(function, definitions, operand, errors);
            validate_operand(function, definitions, count, errors);
        }
        Rvalue::StringPad {
            operand,
            target_len,
            pad,
            ..
        } => {
            validate_operand(function, definitions, operand, errors);
            validate_operand(function, definitions, target_len, errors);
            validate_operand(function, definitions, pad, errors);
        }
        Rvalue::StringCharAt { operand, index } => {
            validate_operand(function, definitions, operand, errors);
            validate_operand(function, definitions, index, errors);
        }
        Rvalue::StringCharCodeAt { operand, index } => {
            validate_operand(function, definitions, operand, errors);
            validate_operand(function, definitions, index, errors);
        }
        Rvalue::StringSlice {
            operand,
            start,
            end,
        } => {
            validate_operand(function, definitions, operand, errors);
            validate_optional_operand(function, definitions, start.as_ref(), errors);
            validate_optional_operand(function, definitions, end.as_ref(), errors);
        }
        Rvalue::ListContains { list, item } => {
            validate_operand(function, definitions, list, errors);
            validate_operand(function, definitions, item, errors);
        }
        Rvalue::SetContains { set, item } => {
            validate_operand(function, definitions, set, errors);
            validate_operand(function, definitions, item, errors);
        }
        Rvalue::ListConcat { left, right } => {
            validate_operand(function, definitions, left, errors);
            validate_operand(function, definitions, right, errors);
        }
        Rvalue::ListSearch { list, item, .. } => {
            validate_operand(function, definitions, list, errors);
            validate_operand(function, definitions, item, errors);
        }
        Rvalue::ListCallback { list, .. } => {
            validate_operand(function, definitions, list, errors);
        }
        Rvalue::ListReduce { list, initial, .. } => {
            validate_operand(function, definitions, list, errors);
            validate_operand(function, definitions, initial, errors);
        }
        Rvalue::ListSlice { list, start, end } => {
            validate_operand(function, definitions, list, errors);
            validate_optional_operand(function, definitions, start.as_ref(), errors);
            validate_optional_operand(function, definitions, end.as_ref(), errors);
        }
        Rvalue::ListPush { list, item } => {
            validate_operand(function, definitions, list, errors);
            validate_operand(function, definitions, item, errors);
        }
        Rvalue::ListExtend { list, other } => {
            validate_operand(function, definitions, list, errors);
            validate_operand(function, definitions, other, errors);
        }
        Rvalue::ListInsert { list, index, item } => {
            validate_operand(function, definitions, list, errors);
            validate_operand(function, definitions, index, errors);
            validate_operand(function, definitions, item, errors);
        }
        Rvalue::ListUnshift { list, items } => {
            validate_operand(function, definitions, list, errors);
            for item in items {
                validate_operand(function, definitions, item, errors);
            }
        }
        Rvalue::ListReverse { list } => {
            validate_operand(function, definitions, list, errors);
        }
        Rvalue::ListClear { list } => {
            validate_operand(function, definitions, list, errors);
        }
        Rvalue::ListCopy { list } => {
            validate_operand(function, definitions, list, errors);
        }
        Rvalue::ListCount { list, item } => {
            validate_operand(function, definitions, list, errors);
            validate_operand(function, definitions, item, errors);
        }
        Rvalue::ListSum { list }
        | Rvalue::ListBoolFold { list, .. }
        | Rvalue::ListSorted { list } => {
            validate_operand(function, definitions, list, errors);
        }
        Rvalue::ListIndex { list, item } => {
            validate_operand(function, definitions, list, errors);
            validate_operand(function, definitions, item, errors);
        }
        Rvalue::ListRemove { list, item } => {
            validate_operand(function, definitions, list, errors);
            validate_operand(function, definitions, item, errors);
        }
        Rvalue::ListSort { list } => {
            validate_operand(function, definitions, list, errors);
        }
        Rvalue::ListPop { list } => {
            validate_operand(function, definitions, list, errors);
        }
        Rvalue::ListShift { list } => {
            validate_operand(function, definitions, list, errors);
        }
        Rvalue::TupleContains { tuple, item } => {
            validate_operand(function, definitions, tuple, errors);
            validate_operand(function, definitions, item, errors);
        }
        Rvalue::DictContainsKey { dict, key } => {
            validate_operand(function, definitions, dict, errors);
            validate_operand(function, definitions, key, errors);
        }
        Rvalue::DictGet { dict, key, default } => {
            validate_operand(function, definitions, dict, errors);
            validate_operand(function, definitions, key, errors);
            validate_optional_operand(function, definitions, default.as_ref(), errors);
        }
        Rvalue::DictSetDefault { dict, key, default } => {
            validate_operand(function, definitions, dict, errors);
            validate_operand(function, definitions, key, errors);
            validate_operand(function, definitions, default, errors);
        }
        Rvalue::DictClear { dict } => {
            validate_operand(function, definitions, dict, errors);
        }
        Rvalue::DictPop { dict, key, default } => {
            validate_operand(function, definitions, dict, errors);
            validate_operand(function, definitions, key, errors);
            validate_optional_operand(function, definitions, default.as_ref(), errors);
        }
        Rvalue::DictUpdate { dict, other } => {
            validate_operand(function, definitions, dict, errors);
            validate_operand(function, definitions, other, errors);
        }
        Rvalue::DictCopy { dict } => {
            validate_operand(function, definitions, dict, errors);
        }
        Rvalue::DictProjection { dict, .. } => {
            validate_operand(function, definitions, dict, errors);
        }
        Rvalue::StringSplit {
            haystack,
            separator,
        } => {
            validate_operand(function, definitions, haystack, errors);
            validate_operand(function, definitions, separator, errors);
        }
        Rvalue::StringJoin { items, separator } => {
            validate_operand(function, definitions, items, errors);
            validate_operand(function, definitions, separator, errors);
        }
        Rvalue::JsonStringify { value: json_value } => {
            validate_operand(function, definitions, json_value, errors);
        }
        Rvalue::JsonParse { text } => {
            validate_operand(function, definitions, text, errors);
        }
        Rvalue::RegexIsMatch {
            pattern, haystack, ..
        } => {
            validate_operand(function, definitions, pattern, errors);
            validate_operand(function, definitions, haystack, errors);
        }
        Rvalue::HttpGetText { url } => {
            validate_operand(function, definitions, url, errors);
        }
        Rvalue::NumericExtrema { args, .. } => {
            for arg in args {
                validate_operand(function, definitions, arg, errors);
            }
        }
        Rvalue::NumericHypot { args } => {
            for arg in args {
                validate_operand(function, definitions, arg, errors);
            }
        }
        Rvalue::NumericPow { base, exponent } => {
            validate_operand(function, definitions, base, errors);
            validate_operand(function, definitions, exponent, errors);
        }
        Rvalue::NumericAtan2 { y, x } => {
            validate_operand(function, definitions, y, errors);
            validate_operand(function, definitions, x, errors);
        }
        Rvalue::NumericRandom => {}
        Rvalue::Unary { operand, .. } => validate_operand(function, definitions, operand, errors),
        Rvalue::Struct { fields, .. } => {
            for (_, field_value) in fields {
                validate_operand(function, definitions, field_value, errors);
            }
        }
        Rvalue::Len(operand)
        | Rvalue::NumericAbs(operand)
        | Rvalue::NumericRound { operand, .. }
        | Rvalue::NumericPredicate { operand, .. }
        | Rvalue::NumericUnaryFunc { operand, .. }
        | Rvalue::StringCase { operand, .. }
        | Rvalue::StringTrim { operand, .. }
        | Rvalue::StringPredicate { operand, .. }
        | Rvalue::Await(operand) => validate_operand(function, definitions, operand, errors),
        Rvalue::AsyncOp { args, .. } => {
            for arg in args {
                validate_operand(function, definitions, arg, errors);
            }
        }
    }
}

/// Validate type constraints for one operand.
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

/// Validate type constraints for one optional operand.
fn validate_optional_operand(
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    maybe_operand: Option<&Operand>,
    errors: &mut Vec<ValidationError>,
) {
    if let Some(inner) = maybe_operand {
        validate_operand(function, definitions, inner, errors);
    }
}

/// Validate type constraints for one place projection chain.
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

/// Validate type constraints for one callee.
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

/// Ensure a local ID points to an existing local declaration.
fn validate_local_exists(
    function: &MirFunction,
    local: LocalId,
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
fn validate_block_exists(
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

/// Ensure a type ID points to an existing MIR type entry.
fn validate_type(mir: &Mir, ty: TypeId, errors: &mut Vec<ValidationError>) {
    if mir.types.get(ty).is_none() {
        errors.push(error(format!("MIR references unknown type {ty:?}")));
    }
}

/// Convert a local identifier into a vector index.
fn local_index(local: LocalId) -> usize {
    u32_to_usize(local.0, "MIR local id")
}

/// Convert a block identifier into a vector index.
fn block_index(block: crate::BlockId) -> usize {
    u32_to_usize(block.0, "MIR block id")
}

/// Convert a function identifier into a vector index.
fn function_index(function: crate::FuncId) -> usize {
    u32_to_usize(function.0, "MIR function id")
}

/// Convert a `u32` identifier into a vector index.
///
/// # Panics
///
/// Panics if the value does not fit in `usize`.
fn u32_to_usize(value: u32, label: &str) -> usize {
    match usize::try_from(value) {
        Ok(index) => index,
        Err(error) => panic!("{label} does not fit in usize: {error}"),
    }
}

/// Build a validation error with no source span.
fn error(message: String) -> ValidationError {
    ValidationError { message }
}
