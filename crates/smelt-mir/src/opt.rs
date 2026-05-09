//! MIR optimization passes.
//!
//! This module provides optimization passes that can improve MIR efficiency,
//! including copy propagation and other transformations.

use std::collections::{HashMap, HashSet};

use crate::{Callee, LocalId, Mir, MirFunction, Operand, Place, Rvalue, Statement, Terminator};

/// A MIR optimization pass that transforms the MIR.
pub trait Pass {
    /// Returns the name of this pass for debugging purposes.
    fn name(&self) -> &'static str;
    /// Runs the pass on the given MIR. Returns true if the MIR was modified.
    fn run(&self, mir: &mut Mir) -> bool;
}

/// Copy propagation optimization that eliminates alias assignments.
#[derive(Debug, Default)]
pub struct CopyPropagation;

impl Pass for CopyPropagation {
    fn name(&self) -> &'static str {
        "copy-propagation"
    }

    fn run(&self, mir: &mut Mir) -> bool {
        let mut changed = false;
        for function in &mut mir.functions {
            changed |= propagate_function(function);
        }
        changed
    }
}

/// Returns the default set of optimization passes.
#[must_use]
pub fn default_passes() -> Vec<Box<dyn Pass>> {
    vec![Box::<CopyPropagation>::default()]
}

/// Applies all default optimization passes to the MIR.
pub fn optimize(mir: &mut Mir) {
    let passes = default_passes();
    loop {
        let mut changed = false;
        for pass in &passes {
            changed |= pass.run(mir);
        }
        if !changed {
            break;
        }
    }
}

/// Propagates copies within a function. Returns true if the function was modified.
fn propagate_function(function: &mut MirFunction) -> bool {
    let mut aliases = HashMap::new();
    let mutated = mutated_locals(function);

    for block in &function.blocks {
        for stmt in &block.statements {
            if let Statement::Assign {
                dest,
                value: Rvalue::Use(Operand::Copy(Place::Local(source))),
            } = stmt
                && !mutated.contains(dest)
                && !mutated.contains(source)
            {
                aliases.insert(*dest, resolve_alias(&aliases, *source));
            }
        }
    }

    if aliases.is_empty() {
        return false;
    }

    let mut changed = false;
    for block in &mut function.blocks {
        for phi in &mut block.phis {
            for (_, operand) in &mut phi.incoming {
                changed |= rewrite_operand(operand, &aliases);
            }
        }
        for stmt in &mut block.statements {
            match stmt {
                Statement::Assign { dest, value } => {
                    changed |= rewrite_rvalue(value, &aliases, Some(*dest));
                }
                Statement::AssignPlace { place, value } => {
                    changed |= rewrite_place(place, &aliases);
                    changed |= rewrite_rvalue(value, &aliases, None);
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }
        if let Some(terminator) = &mut block.terminator {
            changed |= rewrite_terminator(terminator, &aliases);
        }
    }

    changed
}

/// Returns locals that are mutated after initialization.
fn mutated_locals(function: &MirFunction) -> HashSet<LocalId> {
    let mut locals = HashSet::new();
    for block in &function.blocks {
        for stmt in &block.statements {
            if let Statement::AssignPlace { place, .. } = stmt {
                match place {
                    Place::Local(local) | Place::Field { base: local, .. } => {
                        locals.insert(*local);
                    }
                    Place::Index { base, .. } => {
                        locals.insert(*base);
                    }
                }
            }
        }
    }
    locals
}

/// Resolves a local to its canonical alias target.
fn resolve_alias(aliases: &HashMap<LocalId, LocalId>, local: LocalId) -> LocalId {
    let mut current = local;
    let mut seen = HashSet::new();
    while let Some(next) = aliases.get(&current).copied() {
        if !seen.insert(current) {
            break;
        }
        current = next;
    }
    current
}

/// Rewrites operands in an rvalue using alias mappings. Returns true if modified.
fn rewrite_rvalue(
    value: &mut Rvalue,
    aliases: &HashMap<LocalId, LocalId>,
    dest: Option<LocalId>,
) -> bool {
    match value {
        Rvalue::Use(operand) => rewrite_operand_except(operand, aliases, dest),
        Rvalue::List(items) | Rvalue::Tuple(items) => {
            items.iter_mut().fold(false, |changed, item| {
                rewrite_operand_except(item, aliases, dest) | changed
            })
        }
        Rvalue::Dict(entries) => entries.iter_mut().fold(false, |changed, (key, value)| {
            rewrite_operand_except(key, aliases, dest)
                | rewrite_operand_except(value, aliases, dest)
                | changed
        }),
        Rvalue::Binary { lhs, rhs, .. } => {
            rewrite_operand_except(lhs, aliases, dest) | rewrite_operand_except(rhs, aliases, dest)
        }
        Rvalue::StringContains { haystack, needle } => {
            rewrite_operand_except(haystack, aliases, dest)
                | rewrite_operand_except(needle, aliases, dest)
        }
        Rvalue::StringAffix {
            haystack, needle, ..
        }
        | Rvalue::StringSearch {
            haystack, needle, ..
        } => {
            rewrite_operand_except(haystack, aliases, dest)
                | rewrite_operand_except(needle, aliases, dest)
        }
        Rvalue::StringReplace {
            haystack,
            pattern,
            replacement,
            ..
        } => {
            rewrite_operand_except(haystack, aliases, dest)
                | rewrite_operand_except(pattern, aliases, dest)
                | rewrite_operand_except(replacement, aliases, dest)
        }
        Rvalue::StringRemoveAffix {
            haystack, affix, ..
        } => {
            rewrite_operand_except(haystack, aliases, dest)
                | rewrite_operand_except(affix, aliases, dest)
        }
        Rvalue::StringRepeat { operand, count } => {
            rewrite_operand_except(operand, aliases, dest)
                | rewrite_operand_except(count, aliases, dest)
        }
        Rvalue::StringCharAt { operand, index } => {
            rewrite_operand_except(operand, aliases, dest)
                | rewrite_operand_except(index, aliases, dest)
        }
        Rvalue::StringCharCodeAt { operand, index } => {
            rewrite_operand_except(operand, aliases, dest)
                | rewrite_operand_except(index, aliases, dest)
        }
        Rvalue::StringSlice {
            operand,
            start,
            end,
        } => {
            rewrite_operand_except(operand, aliases, dest)
                | rewrite_optional_operand_except(start, aliases, dest)
                | rewrite_optional_operand_except(end, aliases, dest)
        }
        Rvalue::ListContains { list, item } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::ListConcat { left, right } => {
            rewrite_operand_except(left, aliases, dest)
                | rewrite_operand_except(right, aliases, dest)
        }
        Rvalue::ListSearch { list, item, .. } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::ListSlice { list, start, end } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_optional_operand_except(start, aliases, dest)
                | rewrite_optional_operand_except(end, aliases, dest)
        }
        Rvalue::ListPush { list, item } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::ListUnshift { list, items } => {
            let mut changed = rewrite_operand_except(list, aliases, dest);
            for item in items {
                changed |= rewrite_operand_except(item, aliases, dest);
            }
            changed
        }
        Rvalue::ListReverse { list } => rewrite_operand_except(list, aliases, dest),
        Rvalue::ListClear { list } => rewrite_operand_except(list, aliases, dest),
        Rvalue::ListPop { list } => rewrite_operand_except(list, aliases, dest),
        Rvalue::ListShift { list } => rewrite_operand_except(list, aliases, dest),
        Rvalue::TupleContains { tuple, item } => {
            rewrite_operand_except(tuple, aliases, dest)
                | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::DictContainsKey { dict, key } => {
            rewrite_operand_except(dict, aliases, dest) | rewrite_operand_except(key, aliases, dest)
        }
        Rvalue::DictClear { dict } => rewrite_operand_except(dict, aliases, dest),
        Rvalue::DictPop { dict, key, default } => {
            rewrite_operand_except(dict, aliases, dest)
                | rewrite_operand_except(key, aliases, dest)
                | rewrite_optional_operand_except(default, aliases, dest)
        }
        Rvalue::DictUpdate { dict, other } => {
            rewrite_operand_except(dict, aliases, dest)
                | rewrite_operand_except(other, aliases, dest)
        }
        Rvalue::DictCopy { dict } => rewrite_operand_except(dict, aliases, dest),
        Rvalue::DictProjection { dict, .. } => rewrite_operand_except(dict, aliases, dest),
        Rvalue::StringSplit {
            haystack,
            separator,
        } => {
            rewrite_operand_except(haystack, aliases, dest)
                | rewrite_operand_except(separator, aliases, dest)
        }
        Rvalue::StringJoin { items, separator } => {
            rewrite_operand_except(items, aliases, dest)
                | rewrite_operand_except(separator, aliases, dest)
        }
        Rvalue::HttpGetText { url } => rewrite_operand_except(url, aliases, dest),
        Rvalue::NumericExtrema { args, .. } => args.iter_mut().fold(false, |changed, arg| {
            rewrite_operand_except(arg, aliases, dest) || changed
        }),
        Rvalue::NumericHypot { args } => args.iter_mut().fold(false, |changed, arg| {
            rewrite_operand_except(arg, aliases, dest) || changed
        }),
        Rvalue::NumericPow { base, exponent } => {
            rewrite_operand_except(base, aliases, dest)
                | rewrite_operand_except(exponent, aliases, dest)
        }
        Rvalue::Unary { operand, .. } => rewrite_operand_except(operand, aliases, dest),
        Rvalue::Struct { fields, .. } => fields.iter_mut().fold(false, |changed, (_, value)| {
            rewrite_operand_except(value, aliases, dest) | changed
        }),
        Rvalue::Len(operand)
        | Rvalue::NumericAbs(operand)
        | Rvalue::NumericRound { operand, .. }
        | Rvalue::NumericPredicate { operand, .. }
        | Rvalue::NumericUnaryFunc { operand, .. }
        | Rvalue::StringCase { operand, .. }
        | Rvalue::StringTrim { operand, .. }
        | Rvalue::StringPredicate { operand, .. }
        | Rvalue::Await(operand) => rewrite_operand_except(operand, aliases, dest),
        Rvalue::AsyncOp { args, .. } => args.iter_mut().fold(false, |changed, arg| {
            rewrite_operand_except(arg, aliases, dest) || changed
        }),
    }
}

/// Rewrites operands in a terminator using alias mappings. Returns true if modified.
fn rewrite_terminator(terminator: &mut Terminator, aliases: &HashMap<LocalId, LocalId>) -> bool {
    match terminator {
        Terminator::Goto(_) | Terminator::Unreachable => false,
        Terminator::Return(operand) | Terminator::Throw(operand) => {
            rewrite_operand(operand, aliases)
        }
        Terminator::Switch { cond, .. } => rewrite_operand(cond, aliases),
        Terminator::Match { scrutinee, .. } => rewrite_operand(scrutinee, aliases),
        Terminator::Call {
            callee, args, dest, ..
        } => {
            let mut changed = match callee {
                Callee::Indirect(operand) => rewrite_operand(operand, aliases),
                Callee::Static(_) | Callee::Builtin(_) => false,
            };
            for arg in args {
                changed |= rewrite_operand_except(arg, aliases, Some(*dest));
            }
            changed
        }
    }
}

/// Rewrites an operand using alias mappings. Returns true if modified.
fn rewrite_operand(operand: &mut Operand, aliases: &HashMap<LocalId, LocalId>) -> bool {
    rewrite_operand_except(operand, aliases, None)
}

/// Rewrites a place using alias mappings. Returns true if modified.
fn rewrite_place(place: &mut Place, aliases: &HashMap<LocalId, LocalId>) -> bool {
    match place {
        Place::Local(local) | Place::Field { base: local, .. } => {
            let resolved = resolve_alias(aliases, *local);
            let changed = resolved != *local;
            *local = resolved;
            changed
        }
        Place::Index { base, index } => {
            let resolved = resolve_alias(aliases, *base);
            let changed = resolved != *base;
            *base = resolved;
            rewrite_operand(index, aliases) | changed
        }
    }
}

/// Rewrites an operand using alias mappings, except for a specific local ID. Returns true if modified.
fn rewrite_operand_except(
    operand: &mut Operand,
    aliases: &HashMap<LocalId, LocalId>,
    except: Option<LocalId>,
) -> bool {
    let (Operand::Copy(Place::Local(local)) | Operand::Move(Place::Local(local))) = operand else {
        return false;
    };
    if Some(*local) == except {
        return false;
    }
    let resolved = resolve_alias(aliases, *local);
    if resolved == *local {
        return false;
    }
    *local = resolved;
    true
}

/// Rewrites an optional operand using alias mappings, except for a specific local ID.
fn rewrite_optional_operand_except(
    maybe_operand: &mut Option<Operand>,
    aliases: &HashMap<LocalId, LocalId>,
    except: Option<LocalId>,
) -> bool {
    maybe_operand
        .as_mut()
        .is_some_and(|inner| rewrite_operand_except(inner, aliases, except))
}
