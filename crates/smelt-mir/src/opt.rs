//! MIR optimization passes.
//!
//! This module provides optimization passes that can improve MIR efficiency,
//! including copy propagation and other transformations.

use std::collections::{HashMap, HashSet};

use crate::{Callee, LocalId, Mir, MirFunction, Operand, Place, Rvalue, Statement, Terminator};
use smelt_hir::Type;

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
            changed |= propagate_function(function, &mir.types);
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
fn propagate_function(function: &mut MirFunction, types: &smelt_hir::TypeInterner) -> bool {
    let mut aliases = HashMap::new();
    let mutated = mutated_locals(function);
    let assigned_counts = assigned_local_counts(function);

    for block in &function.blocks {
        for stmt in &block.statements {
            if let Statement::Assign {
                dest,
                value: Rvalue::Use(Operand::Copy(Place::Local(source))),
            } = stmt
                && !mutated.contains(dest)
                && !mutated.contains(source)
                && assigned_counts.get(dest) == Some(&1)
                && !is_temp_local(function, *source)
                && !is_function_local(function, types, *dest)
                && !is_function_local(function, types, *source)
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

/// Count direct assignments to each local across a function.
fn assigned_local_counts(function: &MirFunction) -> HashMap<LocalId, usize> {
    let mut counts = HashMap::new();
    for block in &function.blocks {
        for stmt in &block.statements {
            match stmt {
                Statement::Assign { dest, .. } => {
                    let count = counts.entry(*dest).or_insert(0usize);
                    *count = count.saturating_add(1);
                }
                Statement::AssignPlace {
                    place: Place::Local(local),
                    ..
                } => {
                    let count = counts.entry(*local).or_insert(0usize);
                    *count = count.saturating_add(1);
                }
                Statement::AssignPlace { .. }
                | Statement::StorageLive(_)
                | Statement::StorageDead(_) => {}
            }
        }
    }
    counts
}

/// Return whether a local is a compiler-generated temporary.
fn is_temp_local(function: &MirFunction, local: LocalId) -> bool {
    function
        .locals
        .get(usize::try_from(local.0).unwrap_or(usize::MAX))
        .is_some_and(|decl| matches!(decl.kind, crate::LocalKind::Temp))
}

/// Return whether a local has a function type that must not be clone-propagated.
fn is_function_local(
    function: &MirFunction,
    types: &smelt_hir::TypeInterner,
    local: LocalId,
) -> bool {
    function
        .locals
        .get(usize::try_from(local.0).unwrap_or(usize::MAX))
        .is_some_and(|decl| matches!(types.get(decl.ty), Some(Type::Function(_))))
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
        Rvalue::List(items) | Rvalue::Set(items) | Rvalue::Tuple(items) => {
            items.iter_mut().fold(false, |changed, item| {
                rewrite_operand_except(item, aliases, dest) | changed
            })
        }
        Rvalue::Dict(entries) => entries.iter_mut().fold(false, |changed, (key, value)| {
            rewrite_operand_except(key, aliases, dest)
                | rewrite_operand_except(value, aliases, dest)
                | changed
        }),
        Rvalue::Closure { captures, .. } => captures.iter_mut().fold(false, |changed, capture| {
            rewrite_operand_except(capture, aliases, dest) | changed
        }),
        Rvalue::ClosureCall { callee, args } => args.iter_mut().fold(
            rewrite_operand_except(callee, aliases, dest),
            |changed, arg| rewrite_operand_except(arg, aliases, dest) | changed,
        ),
        Rvalue::Binary { lhs, rhs, .. } => {
            rewrite_operand_except(lhs, aliases, dest) | rewrite_operand_except(rhs, aliases, dest)
        }
        Rvalue::Conditional {
            cond,
            then_operand,
            else_operand,
        } => {
            rewrite_operand_except(cond, aliases, dest)
                | rewrite_operand_except(then_operand, aliases, dest)
                | rewrite_operand_except(else_operand, aliases, dest)
        }
        Rvalue::OptionalField { receiver, .. } => rewrite_operand_except(receiver, aliases, dest),
        Rvalue::OptionalIndex { receiver, index } => {
            rewrite_operand_except(receiver, aliases, dest)
                | rewrite_operand_except(index, aliases, dest)
        }
        Rvalue::OptionalMethod { receiver, args, .. } => args.iter_mut().fold(
            rewrite_operand_except(receiver, aliases, dest),
            |changed, arg| rewrite_operand_except(arg, aliases, dest) | changed,
        ),
        Rvalue::OptionalCoalesce { optional, fallback } => {
            rewrite_operand_except(optional, aliases, dest)
                | rewrite_operand_except(fallback, aliases, dest)
        }
        Rvalue::InstanceOf { value: operand, .. }
        | Rvalue::UnknownIs { value: operand, .. }
        | Rvalue::UnknownCast { value: operand, .. } => {
            rewrite_operand_except(operand, aliases, dest)
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
        }
        | Rvalue::RegexReplace {
            haystack,
            pattern,
            replacement,
            ..
        } => {
            rewrite_operand_except(haystack, aliases, dest)
                | rewrite_operand_except(pattern, aliases, dest)
                | rewrite_operand_except(replacement, aliases, dest)
        }
        Rvalue::RegexReplaceCallback {
            haystack,
            pattern,
            callback,
            ..
        } => {
            rewrite_operand_except(haystack, aliases, dest)
                | rewrite_operand_except(pattern, aliases, dest)
                | rewrite_operand_except(callback, aliases, dest)
        }
        Rvalue::RegexReplaceFirstMatchUppercase { pattern, haystack } => {
            rewrite_operand_except(pattern, aliases, dest)
                | rewrite_operand_except(haystack, aliases, dest)
        }
        Rvalue::RegexSplit { pattern, haystack } => {
            rewrite_operand_except(pattern, aliases, dest)
                | rewrite_operand_except(haystack, aliases, dest)
        }
        Rvalue::RegexFind { pattern, haystack } => {
            rewrite_operand_except(pattern, aliases, dest)
                | rewrite_operand_except(haystack, aliases, dest)
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
        Rvalue::StringPad {
            operand,
            target_len,
            pad,
            ..
        } => {
            rewrite_operand_except(operand, aliases, dest)
                | rewrite_operand_except(target_len, aliases, dest)
                | rewrite_operand_except(pad, aliases, dest)
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
        Rvalue::SetContains { set, item } => {
            rewrite_operand_except(set, aliases, dest) | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::SetDisjoint { left, right } => {
            rewrite_operand_except(left, aliases, dest)
                | rewrite_operand_except(right, aliases, dest)
        }
        Rvalue::SetRelation { left, right, .. } => {
            rewrite_operand_except(left, aliases, dest)
                | rewrite_operand_except(right, aliases, dest)
        }
        Rvalue::SetAdd { set, item } | Rvalue::SetRemove { set, item, .. } => {
            rewrite_operand_except(set, aliases, dest) | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::SetClear { set } | Rvalue::SetCopy { set } => {
            rewrite_operand_except(set, aliases, dest)
        }
        Rvalue::ListToSet { list } | Rvalue::ListPairsToDict { list } => {
            rewrite_operand_except(list, aliases, dest)
        }
        Rvalue::SetBinary { left, right, .. } => {
            rewrite_operand_except(left, aliases, dest)
                | rewrite_operand_except(right, aliases, dest)
        }
        Rvalue::SetProjection { set, .. } => rewrite_operand_except(set, aliases, dest),
        Rvalue::ListConcat { left, right } => {
            rewrite_operand_except(left, aliases, dest)
                | rewrite_operand_except(right, aliases, dest)
        }
        Rvalue::ListSearch { list, item, .. } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::ListCallback { list, callback, .. } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(callback, aliases, dest)
        }
        Rvalue::ListFromLength { length } => rewrite_operand_except(length, aliases, dest),
        Rvalue::ListFromLengthMap { length, callback } => {
            rewrite_operand_except(length, aliases, dest)
                | rewrite_operand_except(callback, aliases, dest)
        }
        Rvalue::ListReduce {
            list,
            initial,
            callback,
        } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_optional_operand_except(initial, aliases, dest)
                | rewrite_operand_except(callback, aliases, dest)
        }
        Rvalue::ListSlice { list, start, end } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_optional_operand_except(start, aliases, dest)
                | rewrite_optional_operand_except(end, aliases, dest)
        }
        Rvalue::ListSplice {
            list,
            start,
            delete_count,
            items,
            ..
        } => {
            let mut changed = rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(start, aliases, dest)
                | rewrite_optional_operand_except(delete_count, aliases, dest);
            for item in items {
                changed |= rewrite_operand_except(&mut item.value, aliases, dest);
            }
            changed
        }
        Rvalue::ListFill {
            list,
            value: fill_value,
            start,
            end,
        } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(fill_value, aliases, dest)
                | rewrite_optional_operand_except(start, aliases, dest)
                | rewrite_optional_operand_except(end, aliases, dest)
        }
        Rvalue::ListCopyWithin {
            list,
            target,
            start,
            end,
        } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(target, aliases, dest)
                | rewrite_operand_except(start, aliases, dest)
                | rewrite_optional_operand_except(end, aliases, dest)
        }
        Rvalue::ListWith {
            list,
            index,
            value: replacement,
        } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(index, aliases, dest)
                | rewrite_operand_except(replacement, aliases, dest)
        }
        Rvalue::ListFlat { list } | Rvalue::ListProjection { list, .. } => {
            rewrite_operand_except(list, aliases, dest)
        }
        Rvalue::ListPush { list, item } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::ListExtend { list, other } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(other, aliases, dest)
        }
        Rvalue::ListInsert { list, index, item } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(index, aliases, dest)
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
        Rvalue::ListCopy { list } => rewrite_operand_except(list, aliases, dest),
        Rvalue::TupleToList { tuple } | Rvalue::TupleToSet { tuple } => {
            rewrite_operand_except(tuple, aliases, dest)
        }
        Rvalue::ListToTuple { list } => rewrite_operand_except(list, aliases, dest),
        Rvalue::ListCount { list, item } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::ListSum { list }
        | Rvalue::ListBoolFold { list, .. }
        | Rvalue::ListSorted { list }
        | Rvalue::ListReversed { list }
        | Rvalue::ListEnumerate { list } => rewrite_operand_except(list, aliases, dest),
        Rvalue::ListZip { left, right } => {
            rewrite_operand_except(left, aliases, dest)
                | rewrite_operand_except(right, aliases, dest)
        }
        Rvalue::ListRange { start, end, step } => {
            rewrite_operand_except(start, aliases, dest)
                | rewrite_operand_except(end, aliases, dest)
                | rewrite_operand_except(step, aliases, dest)
        }
        Rvalue::ListRandomChoice { list } => rewrite_operand_except(list, aliases, dest),
        Rvalue::ListIndex { list, item } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::ListRemove { list, item } => {
            rewrite_operand_except(list, aliases, dest)
                | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::ListSort { list, .. } => rewrite_operand_except(list, aliases, dest),
        Rvalue::ListPop { list } => rewrite_operand_except(list, aliases, dest),
        Rvalue::ListShift { list } => rewrite_operand_except(list, aliases, dest),
        Rvalue::TupleContains { tuple, item } => {
            rewrite_operand_except(tuple, aliases, dest)
                | rewrite_operand_except(item, aliases, dest)
        }
        Rvalue::TupleIndex { tuple, .. } | Rvalue::TupleSlice { tuple, .. } => {
            rewrite_operand_except(tuple, aliases, dest)
        }
        Rvalue::DictContainsKey { dict, key } => {
            rewrite_operand_except(dict, aliases, dest) | rewrite_operand_except(key, aliases, dest)
        }
        Rvalue::DictSet {
            dict,
            key,
            value: dict_value,
        } => {
            rewrite_operand_except(dict, aliases, dest)
                | rewrite_operand_except(key, aliases, dest)
                | rewrite_operand_except(dict_value, aliases, dest)
        }
        Rvalue::DictRemoveKey { dict, key } => {
            rewrite_operand_except(dict, aliases, dest) | rewrite_operand_except(key, aliases, dest)
        }
        Rvalue::DictGet { dict, key, default } => {
            rewrite_operand_except(dict, aliases, dest)
                | rewrite_operand_except(key, aliases, dest)
                | rewrite_optional_operand_except(default, aliases, dest)
        }
        Rvalue::DictSetDefault { dict, key, default } => {
            rewrite_operand_except(dict, aliases, dest)
                | rewrite_operand_except(key, aliases, dest)
                | rewrite_operand_except(default, aliases, dest)
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
        Rvalue::DictAssign { target, sources } => {
            let mut changed = rewrite_operand_except(target, aliases, dest);
            for source in sources {
                changed |= rewrite_operand_except(source, aliases, dest);
            }
            changed
        }
        Rvalue::CallableObjectAssign { callable, props } => {
            let mut changed = rewrite_operand_except(callable, aliases, dest);
            for (_, value) in props {
                changed |= rewrite_operand_except(value, aliases, dest);
            }
            changed
        }
        Rvalue::DictCopy { dict } => rewrite_operand_except(dict, aliases, dest),
        Rvalue::DictProjection { dict, .. } => rewrite_operand_except(dict, aliases, dest),
        Rvalue::StringSplit {
            haystack,
            separator,
            limit,
        } => {
            rewrite_operand_except(haystack, aliases, dest)
                | rewrite_operand_except(separator, aliases, dest)
                | rewrite_optional_operand_except(limit, aliases, dest)
        }
        Rvalue::StringChars { haystack } => rewrite_operand_except(haystack, aliases, dest),
        Rvalue::StringJoin { items, separator } => {
            rewrite_operand_except(items, aliases, dest)
                | rewrite_operand_except(separator, aliases, dest)
        }
        Rvalue::JsonStringify { value: json_value } => {
            rewrite_operand_except(json_value, aliases, dest)
        }
        Rvalue::JsonParse { text } => rewrite_operand_except(text, aliases, dest),
        Rvalue::RegexIsMatch {
            pattern, haystack, ..
        } => {
            rewrite_operand_except(pattern, aliases, dest)
                | rewrite_operand_except(haystack, aliases, dest)
        }
        Rvalue::HttpGetText { url } => rewrite_operand_except(url, aliases, dest),
        Rvalue::DateNow => false,
        Rvalue::DateToIsoString { timestamp_ms } => {
            rewrite_operand_except(timestamp_ms, aliases, dest)
        }
        Rvalue::DateFromParts { parts } => {
            let mut changed = false;
            for part in parts {
                changed |= rewrite_operand_except(part, aliases, dest);
            }
            changed
        }
        Rvalue::DateGetPart { timestamp_ms, .. } => {
            rewrite_operand_except(timestamp_ms, aliases, dest)
        }
        Rvalue::DateSetPart {
            timestamp_ms,
            values,
            ..
        } => {
            let mut changed = rewrite_operand_except(timestamp_ms, aliases, dest);
            for value in values {
                changed |= rewrite_operand_except(value, aliases, dest);
            }
            changed
        }
        Rvalue::UrlField { url, .. } => rewrite_operand_except(url, aliases, dest),
        Rvalue::FileReadText { path } => rewrite_operand_except(path, aliases, dest),
        Rvalue::FileWriteText { path, text } => {
            rewrite_operand_except(path, aliases, dest)
                | rewrite_operand_except(text, aliases, dest)
        }
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
        Rvalue::NumericAtan2 { y, x } => {
            rewrite_operand_except(y, aliases, dest) | rewrite_operand_except(x, aliases, dest)
        }
        Rvalue::NumericRandom => false,
        Rvalue::NumericRandomInt { start, end } => {
            rewrite_operand_except(start, aliases, dest)
                | rewrite_operand_except(end, aliases, dest)
        }
        Rvalue::NumericToStringRadix { operand, radix } => {
            rewrite_operand_except(operand, aliases, dest)
                | rewrite_operand_except(radix, aliases, dest)
        }
        Rvalue::PrimitiveCast { operand, .. } => rewrite_operand_except(operand, aliases, dest),
        Rvalue::Unary { operand, .. } => rewrite_operand_except(operand, aliases, dest),
        Rvalue::Struct { fields, .. } => fields.iter_mut().fold(false, |changed, (_, value)| {
            rewrite_operand_except(value, aliases, dest) | changed
        }),
        Rvalue::ExternalClassInstance { args, .. } => {
            args.iter_mut().fold(false, |changed, arg| {
                rewrite_operand_except(arg, aliases, dest) | changed
            })
        }
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
