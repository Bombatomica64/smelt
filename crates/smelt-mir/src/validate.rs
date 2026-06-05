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
    validate_closures(mir, &mut errors);
    for function in &mir.functions {
        validate_function(mir, function, &mut errors);
    }
    errors
}

/// Validate MIR closure table entries and escape-specific capture rules.
fn validate_closures(mir: &Mir, errors: &mut Vec<ValidationError>) {
    for (idx, closure) in mir.closures.iter().enumerate() {
        if usize::try_from(closure.id.0).ok() != Some(idx) {
            errors.push(error(format!(
                "closure index {idx} has mismatched id {:?}",
                closure.id
            )));
        }
        for local in &closure.locals {
            validate_type(mir, local.ty, errors);
        }
        for param in &closure.params {
            if usize::try_from(param.0)
                .ok()
                .and_then(|index| closure.locals.get(index))
                .is_none()
            {
                errors.push(error(format!(
                    "closure {:?} parameter references unknown local {:?}",
                    closure.id, param
                )));
            }
        }
        for capture in &closure.captures {
            validate_type(mir, capture.ty, errors);
            if let Some(target) = capture.target_local
                && usize::try_from(target.0)
                    .ok()
                    .and_then(|index| closure.locals.get(index))
                    .is_none()
            {
                errors.push(error(format!(
                    "closure {:?} capture targets unknown local {:?}",
                    closure.id, target
                )));
            }
            if closure.escapes && capture.mode != smelt_hir::CaptureMode::ByValue {
                errors.push(error(format!(
                    "escaping closure {:?} captures {:?} without owning it",
                    closure.id, capture.source_local
                )));
            }
        }
        validate_type(mir, closure.return_ty, errors);
        if let Some(callback) = &closure.callback_body
            && callback.ty != closure.return_ty
        {
            errors.push(error(format!(
                "closure {:?} callback return type does not match closure return type",
                closure.id
            )));
        }
    }
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

        if let Some(terminator) = &block.terminator {
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
    }

    for local in &function.locals {
        validate_type(mir, local.ty, errors);
    }

    validate_definite_assignment(mir, function, errors);
}

impl Rvalue {
    /// Visit every operand read by this rvalue in evaluation order.
    pub fn for_each_operand(&self, mut visit: impl FnMut(&Operand)) {
        match self {
            Self::Use(operand) => visit(operand),
            Self::List(items) | Self::Set(items) | Self::Tuple(items) => {
                for item in items {
                    visit(item);
                }
            }
            Self::Dict(entries) => {
                for (key, entry_value) in entries {
                    visit(key);
                    visit(entry_value);
                }
            }
            Self::Closure { captures, .. } => {
                for capture in captures {
                    visit(capture);
                }
            }
            Self::ClosureCall { callee, args } => {
                visit(callee);
                for arg in args {
                    visit(arg);
                }
            }
            Self::ClosureCallSpread { callee, args } => {
                visit(callee);
                visit(args);
            }
            Self::Binary { lhs, rhs, .. } => {
                visit(lhs);
                visit(rhs);
            }
            Self::Conditional {
                cond,
                then_operand,
                else_operand,
            } => {
                visit(cond);
                visit(then_operand);
                visit(else_operand);
            }
            Self::FunctionTableLookup { key, cases } => {
                visit(key);
                for (_, case) in cases {
                    visit(case);
                }
            }
            Self::OptionalField { receiver, .. } => {
                visit(receiver);
            }
            Self::OptionalIndex { receiver, index } => {
                visit(receiver);
                visit(index);
            }
            Self::OptionalMethod { receiver, args, .. } => {
                visit(receiver);
                for arg in args {
                    visit(arg);
                }
            }
            Self::OptionalCoalesce { optional, fallback } => {
                visit(optional);
                visit(fallback);
            }
            Self::InstanceOf { value: operand, .. } => {
                visit(operand);
            }
            Self::UnknownIs {
                value: unknown_value,
                ..
            } => {
                visit(unknown_value);
            }
            Self::TypeofValue {
                value: unknown_value,
            } => {
                visit(unknown_value);
            }
            Self::UnknownCast {
                value: unknown_value,
                ..
            } => {
                visit(unknown_value);
            }
            Self::StringContains { haystack, needle } => {
                visit(haystack);
                visit(needle);
            }
            Self::StringAffix {
                haystack, needle, ..
            }
            | Self::StringSearch {
                haystack,
                needle,
                from_index: None,
                ..
            } => {
                visit(haystack);
                visit(needle);
            }
            Self::StringSearch {
                haystack,
                needle,
                from_index: Some(from_index),
                ..
            } => {
                visit(haystack);
                visit(needle);
                visit(from_index);
            }
            Self::StringReplace {
                haystack,
                pattern,
                replacement,
                ..
            } => {
                visit(haystack);
                visit(pattern);
                visit(replacement);
            }
            Self::StringRemoveAffix {
                haystack, affix, ..
            } => {
                visit(haystack);
                visit(affix);
            }
            Self::StringRepeat { operand, count } => {
                visit(operand);
                visit(count);
            }
            Self::StringPad {
                operand,
                target_len,
                pad,
                ..
            } => {
                visit(operand);
                visit(target_len);
                visit(pad);
            }
            Self::StringCharAt { operand, index } => {
                visit(operand);
                visit(index);
            }
            Self::StringCharCodeAt { operand, index } => {
                visit(operand);
                visit(index);
            }
            Self::StringSlice {
                operand,
                start,
                end,
            } => {
                visit(operand);
                if let Some(operand) = start.as_ref() {
                    visit(operand);
                }
                if let Some(operand) = end.as_ref() {
                    visit(operand);
                }
            }
            Self::ListContains { list, item } => {
                visit(list);
                visit(item);
            }
            Self::SetContains { set, item } => {
                visit(set);
                visit(item);
            }
            Self::SetDisjoint { left, right } => {
                visit(left);
                visit(right);
            }
            Self::SetRelation { left, right, .. } => {
                visit(left);
                visit(right);
            }
            Self::SetAdd { set, item } | Self::SetRemove { set, item, .. } => {
                visit(set);
                visit(item);
            }
            Self::SetClear { set } | Self::SetCopy { set } => {
                visit(set);
            }
            Self::ListToSet { list } => {
                visit(list);
            }
            Self::ListPairsToDict { list } => {
                visit(list);
            }
            Self::SetBinary { left, right, .. } => {
                visit(left);
                visit(right);
            }
            Self::SetProjection { set, .. } => {
                visit(set);
            }
            Self::ListConcat { left, right } => {
                visit(left);
                visit(right);
            }
            Self::ListSearch { list, item, .. } => {
                visit(list);
                visit(item);
            }
            Self::ListCallback { list, callback, .. } => {
                visit(list);
                visit(callback);
            }
            Self::ListFromLength { length } => {
                visit(length);
            }
            Self::ListFromLengthMap { length, callback } => {
                visit(length);
                visit(callback);
            }
            Self::ListReduce {
                list,
                initial,
                callback,
            } => {
                visit(list);
                if let Some(operand) = initial.as_ref() {
                    visit(operand);
                }
                visit(callback);
            }
            Self::ListSlice { list, start, end } => {
                visit(list);
                if let Some(operand) = start.as_ref() {
                    visit(operand);
                }
                if let Some(operand) = end.as_ref() {
                    visit(operand);
                }
            }
            Self::ListSplice {
                list,
                start,
                delete_count,
                items,
                ..
            } => {
                visit(list);
                visit(start);
                if let Some(operand) = delete_count.as_ref() {
                    visit(operand);
                }
                for item in items {
                    visit(&item.value);
                }
            }
            Self::ListFill {
                list,
                value: fill_value,
                start,
                end,
            } => {
                visit(list);
                visit(fill_value);
                if let Some(operand) = start.as_ref() {
                    visit(operand);
                }
                if let Some(operand) = end.as_ref() {
                    visit(operand);
                }
            }
            Self::ListCopyWithin {
                list,
                target,
                start,
                end,
            } => {
                visit(list);
                visit(target);
                visit(start);
                if let Some(operand) = end.as_ref() {
                    visit(operand);
                }
            }
            Self::ListWith {
                list,
                index,
                value: replacement,
            } => {
                visit(list);
                visit(index);
                visit(replacement);
            }
            Self::ListFlat { list, depth } => {
                visit(list);
                if let Some(operand) = depth.as_ref() {
                    visit(operand);
                }
            }
            Self::ListProjection { list, .. } => {
                visit(list);
            }
            Self::ListPush { list, item } => {
                visit(list);
                visit(item);
            }
            Self::ListExtend { list, other } => {
                visit(list);
                visit(other);
            }
            Self::ListInsert { list, index, item } => {
                visit(list);
                visit(index);
                visit(item);
            }
            Self::ListUnshift { list, items } => {
                visit(list);
                for item in items {
                    visit(item);
                }
            }
            Self::ListReverse { list } => {
                visit(list);
            }
            Self::ListClear { list } => {
                visit(list);
            }
            Self::ListCopy { list } => {
                visit(list);
            }
            Self::TupleToList { tuple } | Self::TupleToSet { tuple } => {
                visit(tuple);
            }
            Self::ListToTuple { list } => {
                visit(list);
            }
            Self::ListCount { list, item } => {
                visit(list);
                visit(item);
            }
            Self::ListSum { list }
            | Self::ListBoolFold { list, .. }
            | Self::ListSorted { list }
            | Self::ListReversed { list }
            | Self::ListEnumerate { list } => {
                visit(list);
            }
            Self::ListZip { left, right } => {
                visit(left);
                visit(right);
            }
            Self::ListRange { start, end, step } => {
                visit(start);
                visit(end);
                visit(step);
            }
            Self::ListRandomChoice { list } => {
                visit(list);
            }
            Self::ListIndex { list, item } => {
                visit(list);
                visit(item);
            }
            Self::ListRemove { list, item } => {
                visit(list);
                visit(item);
            }
            Self::ListSort { list, .. } => {
                visit(list);
            }
            Self::ListPop { list } => {
                visit(list);
            }
            Self::ListShift { list } => {
                visit(list);
            }
            Self::ListNext { list } => {
                visit(list);
            }
            Self::IteratorDone { result } | Self::IteratorValue { result } => {
                visit(result);
            }
            Self::TupleContains { tuple, item } => {
                visit(tuple);
                visit(item);
            }
            Self::TupleIndex { tuple, .. } | Self::TupleSlice { tuple, .. } => {
                visit(tuple);
            }
            Self::DictContainsKey { dict, key } => {
                visit(dict);
                visit(key);
            }
            Self::DictSet {
                dict,
                key,
                value: dict_value,
            } => {
                visit(dict);
                visit(key);
                visit(dict_value);
            }
            Self::DictRemoveKey { dict, key } => {
                visit(dict);
                visit(key);
            }
            Self::DictGet { dict, key, default } => {
                visit(dict);
                visit(key);
                if let Some(operand) = default.as_ref() {
                    visit(operand);
                }
            }
            Self::DictSetDefault { dict, key, default } => {
                visit(dict);
                visit(key);
                visit(default);
            }
            Self::DictClear { dict } => {
                visit(dict);
            }
            Self::DictPop { dict, key, default } => {
                visit(dict);
                visit(key);
                if let Some(operand) = default.as_ref() {
                    visit(operand);
                }
            }
            Self::DictUpdate { dict, other } => {
                visit(dict);
                visit(other);
            }
            Self::DictAssign { target, sources } => {
                visit(target);
                for source in sources {
                    visit(source);
                }
            }
            Self::CallableObjectAssign { callable, props } => {
                visit(callable);
                for (_, value) in props {
                    visit(value);
                }
            }
            Self::DictCopy { dict } => {
                visit(dict);
            }
            Self::DictProjection { dict, .. } => {
                visit(dict);
            }
            Self::StringSplit {
                haystack,
                separator,
                limit,
            } => {
                visit(haystack);
                visit(separator);
                if let Some(operand) = limit.as_ref() {
                    visit(operand);
                }
            }
            Self::StringChars { haystack } => {
                visit(haystack);
            }
            Self::StringJoin { items, separator } => {
                visit(items);
                visit(separator);
            }
            Self::JsonStringify { value: json_value } => {
                visit(json_value);
            }
            Self::JsonParse { text } => {
                visit(text);
            }
            Self::RegexIsMatch {
                pattern, haystack, ..
            } => {
                visit(pattern);
                visit(haystack);
            }
            Self::RegexReplace {
                pattern,
                haystack,
                replacement,
                ..
            } => {
                visit(pattern);
                visit(haystack);
                visit(replacement);
            }
            Self::RegexReplaceCallback {
                pattern,
                haystack,
                callback,
                ..
            } => {
                visit(pattern);
                visit(haystack);
                visit(callback);
            }
            Self::RegexReplaceFirstMatchUppercase { pattern, haystack } => {
                visit(pattern);
                visit(haystack);
            }
            Self::RegexSplit { pattern, haystack } => {
                visit(pattern);
                visit(haystack);
            }
            Self::RegexFind { pattern, haystack } => {
                visit(pattern);
                visit(haystack);
            }
            Self::RegexExec { regex, haystack } => {
                visit(regex);
                visit(haystack);
            }
            Self::RegexMatchAll { regex, haystack } => {
                visit(regex);
                visit(haystack);
            }
            Self::HttpGetText { url } => {
                visit(url);
            }
            Self::DateNow => {}
            Self::DateResetNow => {}
            Self::DateSetNow { timestamp } => {
                visit(timestamp);
            }
            Self::DateTimezoneOffset | Self::DateResetTimezoneOffset => {}
            Self::DateSetTimezoneOffset { offset } => {
                visit(offset);
            }
            Self::DateTimezoneContext { timezone } => {
                visit(timezone);
            }
            Self::DateToIsoString { timestamp_ms } => {
                visit(timestamp_ms);
            }
            Self::DateToString { timestamp_ms } => {
                visit(timestamp_ms);
            }
            Self::DateFromParts { parts } => {
                for part in parts {
                    visit(part);
                }
            }
            Self::DateFromValue { value: date_value } => {
                visit(date_value);
            }
            Self::DateGetPart { timestamp_ms, .. } => {
                visit(timestamp_ms);
            }
            Self::DateSetPart {
                timestamp_ms,
                values,
                ..
            } => {
                visit(timestamp_ms);
                for value in values {
                    visit(value);
                }
            }
            Self::UrlField { url, .. } => visit(url),
            Self::FileReadText { path } => visit(path),
            Self::FileWriteText { path, text } => {
                visit(path);
                visit(text);
            }
            Self::NumericExtrema { args, .. } => {
                for arg in args {
                    visit(arg);
                }
            }
            Self::NumericHypot { args } => {
                for arg in args {
                    visit(arg);
                }
            }
            Self::NumericPow { base, exponent } => {
                visit(base);
                visit(exponent);
            }
            Self::NumericAtan2 { y, x } => {
                visit(y);
                visit(x);
            }
            Self::NumericRandom => {}
            Self::NumericRandomInt { start, end } => {
                visit(start);
                visit(end);
            }
            Self::NumericToStringRadix { operand, radix } => {
                visit(operand);
                visit(radix);
            }
            Self::PrimitiveCast { operand, .. } => visit(operand),
            Self::Unary { operand, .. } => visit(operand),
            Self::Struct { fields, .. } => {
                for (_, field_value) in fields {
                    visit(field_value);
                }
            }
            Self::ExternalClassInstance { args, .. } => {
                for arg in args {
                    visit(arg);
                }
            }
            Self::Len(operand)
            | Self::NumericAbs(operand)
            | Self::NumericRound { operand, .. }
            | Self::NumericPredicate { operand, .. }
            | Self::NumericUnaryFunc { operand, .. }
            | Self::StringCase { operand, .. }
            | Self::StringNormalize { operand, .. }
            | Self::StringTrim { operand, .. }
            | Self::StringPredicate { operand, .. }
            | Self::Await(operand) => visit(operand),
            Self::AsyncOp { args, .. } => {
                for arg in args {
                    visit(arg);
                }
            }
        }
    }
}

/// Validate that IDs referenced by an rvalue point to existing MIR entities.
fn validate_rvalue_exists(
    mir: &Mir,
    function: &MirFunction,
    value: &Rvalue,
    errors: &mut Vec<ValidationError>,
) {
    value.for_each_operand(|operand| validate_operand_exists(function, operand, errors));
    if let Rvalue::Closure { id, .. } = value
        && mir
            .closures
            .get(usize::try_from(id.0).unwrap_or(usize::MAX))
            .is_none()
    {
        errors.push(ValidationError {
            message: format!("closure rvalue references unknown closure {id:?}"),
        });
    }
    if let Rvalue::UnknownCast { target, .. } = value {
        validate_type(mir, *target, errors);
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
fn validate_definite_assignment(
    mir: &Mir,
    function: &MirFunction,
    errors: &mut Vec<ValidationError>,
) {
    let entry_idx = block_index(function.entry);
    if function.blocks.get(entry_idx).is_none() {
        return;
    }

    let block_count = function.blocks.len();
    let mut in_sets = vec![None::<HashSet<LocalId>>; block_count];
    let mut queue = VecDeque::new();
    let mut entry_defs = HashSet::new();
    entry_defs.extend(function.params.iter().copied());
    entry_defs.extend(function.locals.iter().enumerate().filter_map(
        |(idx, local)| match local.kind {
            crate::LocalKind::UserBinding(_) => Some(LocalId(u32::try_from(idx).ok()?)),
            crate::LocalKind::Param { .. } | crate::LocalKind::Temp => None,
        },
    ));
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
        let Some(mut definitions) = definitions_slot.clone() else {
            continue;
        };

        for phi in &block.phis {
            definitions.insert(phi.dest);
        }

        for stmt in &block.statements {
            match stmt {
                Statement::Assign { dest, value } => {
                    let _ = value;
                    definitions.insert(*dest);
                }
                Statement::AssignPlace { place, value } => {
                    let _ = value;
                    match place {
                        Place::Local(local) => {
                            definitions.insert(*local);
                        }
                        Place::Field { .. } | Place::Index { .. } => {}
                    }
                }
                Statement::StorageLive(_) | Statement::StorageDead(_) => {}
            }
        }

        if let Some(terminator) = &block.terminator {
            match terminator {
                Terminator::Goto(_) | Terminator::Unreachable => {}
                Terminator::Call { dest, .. } | Terminator::Await { dest, .. } => {
                    definitions.insert(*dest);
                }
                Terminator::Switch { .. }
                | Terminator::Match { .. }
                | Terminator::Return(_)
                | Terminator::Throw(_) => {}
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

    for (block_idx, block) in function.blocks.iter().enumerate() {
        let Some(definitions_slot) = in_sets.get(block_idx) else {
            continue;
        };
        let Some(mut definitions) = definitions_slot.clone() else {
            continue;
        };
        let before_phi = definitions.clone();

        for phi in &block.phis {
            for (_, operand) in &phi.incoming {
                validate_operand(mir, function, &before_phi, operand, errors);
            }
            definitions.insert(phi.dest);
        }

        for stmt in &block.statements {
            match stmt {
                Statement::Assign { dest, value } => {
                    validate_rvalue(mir, function, &definitions, value, errors);
                    definitions.insert(*dest);
                }
                Statement::AssignPlace { place, value } => {
                    validate_rvalue(mir, function, &definitions, value, errors);
                    match place {
                        Place::Local(local) => {
                            definitions.insert(*local);
                        }
                        Place::Field { .. } | Place::Index { .. } => {
                            validate_place(mir, function, &definitions, place, errors);
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
                    callee,
                    args,
                    dest,
                    unwind,
                    ..
                } => {
                    validate_callee(mir, function, &definitions, callee, errors);
                    for arg in args {
                        validate_operand(mir, function, &definitions, arg, errors);
                    }
                    if let Some(handler) = unwind
                        && let Some(local) = handler.exception_local
                    {
                        validate_local_exists(function, local, errors);
                    }
                    definitions.insert(*dest);
                }
                Terminator::Await {
                    future,
                    dest,
                    unwind,
                    ..
                } => {
                    validate_operand(mir, function, &definitions, future, errors);
                    if let Some(handler) = unwind
                        && let Some(local) = handler.exception_local
                    {
                        validate_local_exists(function, local, errors);
                    }
                    definitions.insert(*dest);
                }
                Terminator::Switch { cond, .. } => {
                    validate_operand(mir, function, &definitions, cond, errors);
                }
                Terminator::Match { scrutinee, .. } => {
                    validate_operand(mir, function, &definitions, scrutinee, errors);
                }
                Terminator::Return(operand) | Terminator::Throw(operand) => {
                    validate_operand(mir, function, &definitions, operand, errors);
                }
            }
        }
    }
}

/// Return control-flow successors for a block terminator.
fn successors(terminator: &Terminator) -> Vec<crate::BlockId> {
    match terminator {
        Terminator::Goto(target) => vec![*target],
        Terminator::Call { target, unwind, .. } | Terminator::Await { target, unwind, .. } => {
            unwind
                .iter()
                .map(|handler| handler.catch_block)
                .chain(std::iter::once(*target))
                .collect()
        }
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
    mir: &Mir,
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    value: &Rvalue,
    errors: &mut Vec<ValidationError>,
) {
    match value {
        Rvalue::Use(operand) => validate_operand(mir, function, definitions, operand, errors),
        Rvalue::List(items) | Rvalue::Set(items) | Rvalue::Tuple(items) => {
            for item in items {
                validate_operand(mir, function, definitions, item, errors);
            }
        }
        Rvalue::Dict(entries) => {
            for (key, entry_value) in entries {
                validate_operand(mir, function, definitions, key, errors);
                validate_operand(mir, function, definitions, entry_value, errors);
            }
        }
        Rvalue::Closure { .. } => {}
        Rvalue::ClosureCall { callee, args } => {
            validate_operand(mir, function, definitions, callee, errors);
            for arg in args {
                validate_operand(mir, function, definitions, arg, errors);
            }
        }
        Rvalue::ClosureCallSpread { callee, args } => {
            validate_operand(mir, function, definitions, callee, errors);
            validate_operand(mir, function, definitions, args, errors);
        }
        Rvalue::Binary { lhs, rhs, .. } => {
            validate_operand(mir, function, definitions, lhs, errors);
            validate_operand(mir, function, definitions, rhs, errors);
        }
        Rvalue::Conditional {
            cond,
            then_operand,
            else_operand,
        } => {
            validate_operand(mir, function, definitions, cond, errors);
            validate_operand(mir, function, definitions, then_operand, errors);
            validate_operand(mir, function, definitions, else_operand, errors);
        }
        Rvalue::FunctionTableLookup { key, cases } => {
            validate_operand(mir, function, definitions, key, errors);
            for (_, case) in cases {
                validate_operand(mir, function, definitions, case, errors);
            }
        }
        Rvalue::OptionalField { receiver, .. } => {
            validate_operand(mir, function, definitions, receiver, errors);
        }
        Rvalue::OptionalIndex { receiver, index } => {
            validate_operand(mir, function, definitions, receiver, errors);
            validate_operand(mir, function, definitions, index, errors);
        }
        Rvalue::OptionalMethod { receiver, args, .. } => {
            validate_operand(mir, function, definitions, receiver, errors);
            for arg in args {
                validate_operand(mir, function, definitions, arg, errors);
            }
        }
        Rvalue::OptionalCoalesce { optional, fallback } => {
            validate_operand(mir, function, definitions, optional, errors);
            validate_operand(mir, function, definitions, fallback, errors);
        }
        Rvalue::InstanceOf { value: operand, .. } => {
            validate_operand(mir, function, definitions, operand, errors);
        }
        Rvalue::UnknownIs {
            value: unknown_value,
            ..
        }
        | Rvalue::TypeofValue {
            value: unknown_value,
        }
        | Rvalue::UnknownCast {
            value: unknown_value,
            ..
        } => {
            validate_operand(mir, function, definitions, unknown_value, errors);
        }
        Rvalue::StringContains { haystack, needle } => {
            validate_operand(mir, function, definitions, haystack, errors);
            validate_operand(mir, function, definitions, needle, errors);
        }
        Rvalue::StringAffix {
            haystack, needle, ..
        }
        | Rvalue::StringSearch {
            haystack,
            needle,
            from_index: None,
            ..
        } => {
            validate_operand(mir, function, definitions, haystack, errors);
            validate_operand(mir, function, definitions, needle, errors);
        }
        Rvalue::StringSearch {
            haystack,
            needle,
            from_index: Some(from_index),
            ..
        } => {
            validate_operand(mir, function, definitions, haystack, errors);
            validate_operand(mir, function, definitions, needle, errors);
            validate_operand(mir, function, definitions, from_index, errors);
        }
        Rvalue::StringReplace {
            haystack,
            pattern,
            replacement,
            ..
        } => {
            validate_operand(mir, function, definitions, haystack, errors);
            validate_operand(mir, function, definitions, pattern, errors);
            validate_operand(mir, function, definitions, replacement, errors);
        }
        Rvalue::StringRemoveAffix {
            haystack, affix, ..
        } => {
            validate_operand(mir, function, definitions, haystack, errors);
            validate_operand(mir, function, definitions, affix, errors);
        }
        Rvalue::StringRepeat { operand, count } => {
            validate_operand(mir, function, definitions, operand, errors);
            validate_operand(mir, function, definitions, count, errors);
        }
        Rvalue::StringPad {
            operand,
            target_len,
            pad,
            ..
        } => {
            validate_operand(mir, function, definitions, operand, errors);
            validate_operand(mir, function, definitions, target_len, errors);
            validate_operand(mir, function, definitions, pad, errors);
        }
        Rvalue::StringCharAt { operand, index } => {
            validate_operand(mir, function, definitions, operand, errors);
            validate_operand(mir, function, definitions, index, errors);
        }
        Rvalue::StringCharCodeAt { operand, index } => {
            validate_operand(mir, function, definitions, operand, errors);
            validate_operand(mir, function, definitions, index, errors);
        }
        Rvalue::StringSlice {
            operand,
            start,
            end,
        } => {
            validate_operand(mir, function, definitions, operand, errors);
            validate_optional_operand(mir, function, definitions, start.as_ref(), errors);
            validate_optional_operand(mir, function, definitions, end.as_ref(), errors);
        }
        Rvalue::ListContains { list, item } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, item, errors);
        }
        Rvalue::SetContains { set, item } => {
            validate_operand(mir, function, definitions, set, errors);
            validate_operand(mir, function, definitions, item, errors);
        }
        Rvalue::SetDisjoint { left, right } => {
            validate_operand(mir, function, definitions, left, errors);
            validate_operand(mir, function, definitions, right, errors);
        }
        Rvalue::SetRelation { left, right, .. } => {
            validate_operand(mir, function, definitions, left, errors);
            validate_operand(mir, function, definitions, right, errors);
        }
        Rvalue::SetAdd { set, item } | Rvalue::SetRemove { set, item, .. } => {
            validate_operand(mir, function, definitions, set, errors);
            validate_operand(mir, function, definitions, item, errors);
        }
        Rvalue::SetClear { set } | Rvalue::SetCopy { set } => {
            validate_operand(mir, function, definitions, set, errors);
        }
        Rvalue::ListToSet { list } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::ListPairsToDict { list } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::SetBinary { left, right, .. } => {
            validate_operand(mir, function, definitions, left, errors);
            validate_operand(mir, function, definitions, right, errors);
        }
        Rvalue::SetProjection { set, .. } => {
            validate_operand(mir, function, definitions, set, errors);
        }
        Rvalue::ListConcat { left, right } => {
            validate_operand(mir, function, definitions, left, errors);
            validate_operand(mir, function, definitions, right, errors);
        }
        Rvalue::ListSearch { list, item, .. } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, item, errors);
        }
        Rvalue::ListCallback { list, callback, .. } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, callback, errors);
        }
        Rvalue::ListFromLength { length } => {
            validate_operand(mir, function, definitions, length, errors);
        }
        Rvalue::ListFromLengthMap { length, callback } => {
            validate_operand(mir, function, definitions, length, errors);
            validate_operand(mir, function, definitions, callback, errors);
        }
        Rvalue::ListReduce {
            list,
            initial,
            callback,
        } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_optional_operand(mir, function, definitions, initial.as_ref(), errors);
            validate_operand(mir, function, definitions, callback, errors);
        }
        Rvalue::ListSlice { list, start, end } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_optional_operand(mir, function, definitions, start.as_ref(), errors);
            validate_optional_operand(mir, function, definitions, end.as_ref(), errors);
        }
        Rvalue::ListSplice {
            list,
            start,
            delete_count,
            items,
            ..
        } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, start, errors);
            validate_optional_operand(mir, function, definitions, delete_count.as_ref(), errors);
            for item in items {
                validate_operand(mir, function, definitions, &item.value, errors);
            }
        }
        Rvalue::ListFill {
            list,
            value: fill_value,
            start,
            end,
        } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, fill_value, errors);
            validate_optional_operand(mir, function, definitions, start.as_ref(), errors);
            validate_optional_operand(mir, function, definitions, end.as_ref(), errors);
        }
        Rvalue::ListCopyWithin {
            list,
            target,
            start,
            end,
        } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, target, errors);
            validate_operand(mir, function, definitions, start, errors);
            validate_optional_operand(mir, function, definitions, end.as_ref(), errors);
        }
        Rvalue::ListWith {
            list,
            index,
            value: replacement,
        } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, index, errors);
            validate_operand(mir, function, definitions, replacement, errors);
        }
        Rvalue::ListFlat { list, depth } => {
            validate_operand(mir, function, definitions, list, errors);
            if let Some(depth_operand) = depth {
                validate_operand(mir, function, definitions, depth_operand, errors);
            }
        }
        Rvalue::ListProjection { list, .. } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::ListPush { list, item } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, item, errors);
        }
        Rvalue::ListExtend { list, other } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, other, errors);
        }
        Rvalue::ListInsert { list, index, item } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, index, errors);
            validate_operand(mir, function, definitions, item, errors);
        }
        Rvalue::ListUnshift { list, items } => {
            validate_operand(mir, function, definitions, list, errors);
            for item in items {
                validate_operand(mir, function, definitions, item, errors);
            }
        }
        Rvalue::ListReverse { list } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::ListClear { list } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::ListCopy { list } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::TupleToList { tuple } | Rvalue::TupleToSet { tuple } => {
            validate_operand(mir, function, definitions, tuple, errors);
        }
        Rvalue::ListToTuple { list } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::ListCount { list, item } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, item, errors);
        }
        Rvalue::ListSum { list }
        | Rvalue::ListBoolFold { list, .. }
        | Rvalue::ListSorted { list }
        | Rvalue::ListReversed { list }
        | Rvalue::ListEnumerate { list } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::ListZip { left, right } => {
            validate_operand(mir, function, definitions, left, errors);
            validate_operand(mir, function, definitions, right, errors);
        }
        Rvalue::ListRange { start, end, step } => {
            validate_operand(mir, function, definitions, start, errors);
            validate_operand(mir, function, definitions, end, errors);
            validate_operand(mir, function, definitions, step, errors);
        }
        Rvalue::ListRandomChoice { list } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::ListIndex { list, item } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, item, errors);
        }
        Rvalue::ListRemove { list, item } => {
            validate_operand(mir, function, definitions, list, errors);
            validate_operand(mir, function, definitions, item, errors);
        }
        Rvalue::ListSort { list, .. } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::ListPop { list } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::ListShift { list } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::ListNext { list } => {
            validate_operand(mir, function, definitions, list, errors);
        }
        Rvalue::IteratorDone { result } | Rvalue::IteratorValue { result } => {
            validate_operand(mir, function, definitions, result, errors);
        }
        Rvalue::TupleContains { tuple, item } => {
            validate_operand(mir, function, definitions, tuple, errors);
            validate_operand(mir, function, definitions, item, errors);
        }
        Rvalue::TupleIndex { tuple, .. } | Rvalue::TupleSlice { tuple, .. } => {
            validate_operand(mir, function, definitions, tuple, errors);
        }
        Rvalue::DictContainsKey { dict, key } => {
            validate_operand(mir, function, definitions, dict, errors);
            validate_operand(mir, function, definitions, key, errors);
        }
        Rvalue::DictSet {
            dict,
            key,
            value: dict_value,
        } => {
            validate_operand(mir, function, definitions, dict, errors);
            validate_operand(mir, function, definitions, key, errors);
            validate_operand(mir, function, definitions, dict_value, errors);
        }
        Rvalue::DictRemoveKey { dict, key } => {
            validate_operand(mir, function, definitions, dict, errors);
            validate_operand(mir, function, definitions, key, errors);
        }
        Rvalue::DictGet { dict, key, default } => {
            validate_operand(mir, function, definitions, dict, errors);
            validate_operand(mir, function, definitions, key, errors);
            validate_optional_operand(mir, function, definitions, default.as_ref(), errors);
        }
        Rvalue::DictSetDefault { dict, key, default } => {
            validate_operand(mir, function, definitions, dict, errors);
            validate_operand(mir, function, definitions, key, errors);
            validate_operand(mir, function, definitions, default, errors);
        }
        Rvalue::DictClear { dict } => {
            validate_operand(mir, function, definitions, dict, errors);
        }
        Rvalue::DictPop { dict, key, default } => {
            validate_operand(mir, function, definitions, dict, errors);
            validate_operand(mir, function, definitions, key, errors);
            validate_optional_operand(mir, function, definitions, default.as_ref(), errors);
        }
        Rvalue::DictUpdate { dict, other } => {
            validate_operand(mir, function, definitions, dict, errors);
            validate_operand(mir, function, definitions, other, errors);
        }
        Rvalue::DictAssign { target, sources } => {
            validate_operand(mir, function, definitions, target, errors);
            for source in sources {
                validate_operand(mir, function, definitions, source, errors);
            }
        }
        Rvalue::CallableObjectAssign { callable, props } => {
            validate_operand(mir, function, definitions, callable, errors);
            for (_, value) in props {
                validate_operand(mir, function, definitions, value, errors);
            }
        }
        Rvalue::DictCopy { dict } => {
            validate_operand(mir, function, definitions, dict, errors);
        }
        Rvalue::DictProjection { dict, .. } => {
            validate_operand(mir, function, definitions, dict, errors);
        }
        Rvalue::StringSplit {
            haystack,
            separator,
            limit,
        } => {
            validate_operand(mir, function, definitions, haystack, errors);
            validate_operand(mir, function, definitions, separator, errors);
            validate_optional_operand(mir, function, definitions, limit.as_ref(), errors);
        }
        Rvalue::StringChars { haystack } => {
            validate_operand(mir, function, definitions, haystack, errors);
        }
        Rvalue::StringJoin { items, separator } => {
            validate_operand(mir, function, definitions, items, errors);
            validate_operand(mir, function, definitions, separator, errors);
        }
        Rvalue::JsonStringify { value: json_value } => {
            validate_operand(mir, function, definitions, json_value, errors);
        }
        Rvalue::JsonParse { text } => {
            validate_operand(mir, function, definitions, text, errors);
        }
        Rvalue::RegexIsMatch {
            pattern, haystack, ..
        } => {
            validate_operand(mir, function, definitions, pattern, errors);
            validate_operand(mir, function, definitions, haystack, errors);
        }
        Rvalue::RegexReplace {
            pattern,
            haystack,
            replacement,
            ..
        } => {
            validate_operand(mir, function, definitions, pattern, errors);
            validate_operand(mir, function, definitions, haystack, errors);
            validate_operand(mir, function, definitions, replacement, errors);
        }
        Rvalue::RegexReplaceCallback {
            pattern,
            haystack,
            callback,
            ..
        } => {
            validate_operand(mir, function, definitions, pattern, errors);
            validate_operand(mir, function, definitions, haystack, errors);
            validate_operand(mir, function, definitions, callback, errors);
        }
        Rvalue::RegexReplaceFirstMatchUppercase { pattern, haystack } => {
            validate_operand(mir, function, definitions, pattern, errors);
            validate_operand(mir, function, definitions, haystack, errors);
        }
        Rvalue::RegexSplit { pattern, haystack } => {
            validate_operand(mir, function, definitions, pattern, errors);
            validate_operand(mir, function, definitions, haystack, errors);
        }
        Rvalue::RegexFind { pattern, haystack } => {
            validate_operand(mir, function, definitions, pattern, errors);
            validate_operand(mir, function, definitions, haystack, errors);
        }
        Rvalue::RegexExec { regex, haystack } => {
            validate_operand(mir, function, definitions, regex, errors);
            validate_operand(mir, function, definitions, haystack, errors);
        }
        Rvalue::RegexMatchAll { regex, haystack } => {
            validate_operand(mir, function, definitions, regex, errors);
            validate_operand(mir, function, definitions, haystack, errors);
        }
        Rvalue::HttpGetText { url } => {
            validate_operand(mir, function, definitions, url, errors);
        }
        Rvalue::DateNow => {}
        Rvalue::DateResetNow => {}
        Rvalue::DateSetNow { timestamp } => {
            validate_operand(mir, function, definitions, timestamp, errors);
        }
        Rvalue::DateTimezoneOffset | Rvalue::DateResetTimezoneOffset => {}
        Rvalue::DateSetTimezoneOffset { offset } => {
            validate_operand(mir, function, definitions, offset, errors);
        }
        Rvalue::DateTimezoneContext { timezone } => {
            validate_operand(mir, function, definitions, timezone, errors);
        }
        Rvalue::DateToIsoString { timestamp_ms } => {
            validate_operand(mir, function, definitions, timestamp_ms, errors);
        }
        Rvalue::DateToString { timestamp_ms } => {
            validate_operand(mir, function, definitions, timestamp_ms, errors);
        }
        Rvalue::DateFromParts { parts } => {
            for part in parts {
                validate_operand(mir, function, definitions, part, errors);
            }
        }
        Rvalue::DateFromValue { value: date_value } => {
            validate_operand(mir, function, definitions, date_value, errors);
        }
        Rvalue::DateGetPart { timestamp_ms, .. } => {
            validate_operand(mir, function, definitions, timestamp_ms, errors);
        }
        Rvalue::DateSetPart {
            timestamp_ms,
            values,
            ..
        } => {
            validate_operand(mir, function, definitions, timestamp_ms, errors);
            for value in values {
                validate_operand(mir, function, definitions, value, errors);
            }
        }
        Rvalue::UrlField { url, .. } => validate_operand(mir, function, definitions, url, errors),
        Rvalue::FileReadText { path } => validate_operand(mir, function, definitions, path, errors),
        Rvalue::FileWriteText { path, text } => {
            validate_operand(mir, function, definitions, path, errors);
            validate_operand(mir, function, definitions, text, errors);
        }
        Rvalue::NumericExtrema { args, .. } => {
            for arg in args {
                validate_operand(mir, function, definitions, arg, errors);
            }
        }
        Rvalue::NumericHypot { args } => {
            for arg in args {
                validate_operand(mir, function, definitions, arg, errors);
            }
        }
        Rvalue::NumericPow { base, exponent } => {
            validate_operand(mir, function, definitions, base, errors);
            validate_operand(mir, function, definitions, exponent, errors);
        }
        Rvalue::NumericAtan2 { y, x } => {
            validate_operand(mir, function, definitions, y, errors);
            validate_operand(mir, function, definitions, x, errors);
        }
        Rvalue::NumericRandom => {}
        Rvalue::NumericRandomInt { start, end } => {
            validate_operand(mir, function, definitions, start, errors);
            validate_operand(mir, function, definitions, end, errors);
        }
        Rvalue::NumericToStringRadix { operand, radix } => {
            validate_operand(mir, function, definitions, operand, errors);
            validate_operand(mir, function, definitions, radix, errors);
        }
        Rvalue::PrimitiveCast { operand, .. } => {
            validate_operand(mir, function, definitions, operand, errors);
        }
        Rvalue::Unary { operand, .. } => {
            validate_operand(mir, function, definitions, operand, errors)
        }
        Rvalue::Struct { fields, .. } => {
            for (_, field_value) in fields {
                validate_operand(mir, function, definitions, field_value, errors);
            }
        }
        Rvalue::ExternalClassInstance { args, .. } => {
            for arg in args {
                validate_operand(mir, function, definitions, arg, errors);
            }
        }
        Rvalue::Len(operand)
        | Rvalue::NumericAbs(operand)
        | Rvalue::NumericRound { operand, .. }
        | Rvalue::NumericPredicate { operand, .. }
        | Rvalue::NumericUnaryFunc { operand, .. }
        | Rvalue::StringCase { operand, .. }
        | Rvalue::StringNormalize { operand, .. }
        | Rvalue::StringTrim { operand, .. }
        | Rvalue::StringPredicate { operand, .. }
        | Rvalue::Await(operand) => validate_operand(mir, function, definitions, operand, errors),
        Rvalue::AsyncOp { args, .. } => {
            for arg in args {
                validate_operand(mir, function, definitions, arg, errors);
            }
        }
    }
}

/// Validate type constraints for one operand.
fn validate_operand(
    mir: &Mir,
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    operand: &Operand,
    errors: &mut Vec<ValidationError>,
) {
    match operand {
        Operand::Copy(place) | Operand::Move(place) => {
            validate_place(mir, function, definitions, place, errors);
        }
        Operand::Const(_) => {}
    }
}

/// Validate type constraints for one optional operand.
fn validate_optional_operand(
    mir: &Mir,
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    maybe_operand: Option<&Operand>,
    errors: &mut Vec<ValidationError>,
) {
    if let Some(inner) = maybe_operand {
        validate_operand(mir, function, definitions, inner, errors);
    }
}

/// Validate type constraints for one place projection chain.
fn validate_place(
    mir: &Mir,
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    place: &Place,
    errors: &mut Vec<ValidationError>,
) {
    match place {
        Place::Local(local) => {
            if !definitions.contains(local) {
                let function_name = mir.symbols.get(function.name).unwrap_or("<unknown>");
                let local_detail = local_detail(mir, function, *local);
                errors.push(error(format!(
                    "function {:?} `{function_name}` reads local {:?} {local_detail} before it is definitely defined",
                    function.id, local
                )));
            }
        }
        Place::Field { base, .. } => {
            validate_place(mir, function, definitions, &Place::Local(*base), errors)
        }
        Place::Index { base, index } => {
            validate_place(mir, function, definitions, &Place::Local(*base), errors);
            validate_operand(mir, function, definitions, index, errors);
        }
    }
}

/// Render a local declaration for definite-assignment diagnostics.
fn local_detail(mir: &Mir, function: &MirFunction, local: LocalId) -> String {
    let Some(decl) = function.locals.get(local_index(local)) else {
        return "<unknown local>".to_owned();
    };
    let kind = match decl.kind {
        crate::LocalKind::Param { symbol } => symbol
            .and_then(|param_symbol| mir.symbols.get(param_symbol))
            .map_or_else(|| "param".to_owned(), |name| format!("param `{name}`")),
        crate::LocalKind::Temp => "temp".to_owned(),
        crate::LocalKind::UserBinding(symbol) => {
            format!(
                "binding `{}`",
                mir.symbols.get(symbol).unwrap_or("<unknown>")
            )
        }
    };
    format!(
        "({kind}, span file {} start {} end {})",
        decl.span.file.0, decl.span.start, decl.span.end
    )
}

/// Validate type constraints for one callee.
fn validate_callee(
    mir: &Mir,
    function: &MirFunction,
    definitions: &HashSet<LocalId>,
    callee: &Callee,
    errors: &mut Vec<ValidationError>,
) {
    match callee {
        Callee::Static(_) | Callee::Builtin(_) => {}
        Callee::Indirect(operand) => {
            validate_operand(mir, function, definitions, operand, errors);
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
fn u32_to_usize(value: u32, label: &str) -> usize {
    if let Ok(index) = usize::try_from(value) {
        index
    } else {
        let _ = label;
        usize::MAX
    }
}

/// Build a validation error with no source span.
fn error(message: String) -> ValidationError {
    ValidationError { message }
}
