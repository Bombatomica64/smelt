//! Operand traversal and existence checks.
//!
//! This module owns the canonical operand walk for [`Rvalue`]
//! ([`Rvalue::for_each_operand`] and its mutable mirror), which the rest of the
//! crate reuses so operand enumeration cannot drift as variants are added. It
//! also holds the "does this reference exist" checks for rvalues, operands,
//! places, and callees used by structural validation.

use crate::{Callee, MirFunction, Mir, Operand, Place, Rvalue};

use super::structure::validate_local_exists;
use super::{ValidationError, error, function_index, validate_type};

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
            Self::PrototypeSentinel {
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
            Self::StringAffix {
                haystack, needle, ..
            }
            | Self::StringSearch {
                haystack,
                needle,
                from_index: None,
                ..
            }
            | Self::StringContains {
                haystack,
                needle,
                from_index: None,
            } => {
                visit(haystack);
                visit(needle);
            }
            Self::StringSearch {
                haystack,
                needle,
                from_index: Some(from_index),
                ..
            }
            | Self::StringContains {
                haystack,
                needle,
                from_index: Some(from_index),
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
                if let Some(start_operand) = start.as_ref() {
                    visit(start_operand);
                }
                if let Some(end_operand) = end.as_ref() {
                    visit(end_operand);
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
            Self::ListSearch {
                list,
                item,
                from_index: None,
                ..
            } => {
                visit(list);
                visit(item);
            }
            Self::ListSearch {
                list,
                item,
                from_index: Some(from_index),
                ..
            } => {
                visit(list);
                visit(item);
                visit(from_index);
            }
            Self::ListCallback { list, callback, .. } => {
                visit(list);
                visit(callback);
            }
            Self::ListFromLength { length } => {
                visit(length);
            }
            Self::ListRepeat { value, count } => {
                visit(value);
                visit(count);
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
            | Self::ListReversed { list }
            | Self::ListEnumerate { list } => {
                visit(list);
            }
            Self::ListSorted { list, key, .. } => {
                visit(list);
                if let Some(sort_key) = key {
                    visit(sort_key);
                }
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
            Self::ListSort {
                list,
                comparator,
                key,
                ..
            } => {
                visit(list);
                if let Some(sort_comparator) = comparator {
                    visit(sort_comparator);
                }
                if let Some(sort_key) = key {
                    visit(sort_key);
                }
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
            Self::CallableObjectAssign {
                callable,
                props,
                spreads,
            } => {
                visit(callable);
                for (_, value) in props {
                    visit(value);
                }
                for value in spreads {
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
            Self::GlobalGet { .. } => {}
            Self::GlobalSet { value, .. } => {
                visit(value);
            }
            Self::DateSetNow { timestamp } => {
                visit(timestamp);
            }
            Self::DateTimezoneOffset | Self::DateResetTimezoneOffset => {}
            Self::DateSetTimezoneOffset { offset } => {
                visit(offset);
            }
            Self::VitestMockFn { implementation } => {
                if let Some(implementation) = implementation {
                    visit(implementation);
                }
            }
            Self::VitestMockCalledTimes { mock, count } => {
                visit(mock);
                visit(count);
            }
            Self::VitestMockCalledWith { mock, args, .. } => {
                visit(mock);
                for arg in args {
                    visit(arg);
                }
            }
            Self::VitestMockLastResolvedWith { mock, expected } => {
                visit(mock);
                visit(expected);
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
            Self::BlobFromParts {
                parts,
                blob_type,
                name,
                last_modified,
            } => {
                visit(parts);
                visit(blob_type);
                if let Some(name_operand) = name {
                    visit(name_operand);
                }
                if let Some(last_modified_operand) = last_modified {
                    visit(last_modified_operand);
                }
            }
            Self::HostGlobalRead { .. } | Self::HostGlobalPresent { .. } => {}
            Self::HostGlobalWrite { value, .. } => {
                visit(value);
            }
            Self::NumericExtrema { args, spread, .. } => {
                for arg in args {
                    visit(arg);
                }
                if let Some(spread_operand) = spread {
                    visit(spread_operand);
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
            Self::NumericToFixed { operand, digits } => {
                visit(operand);
                visit(digits);
            }
            Self::ParseIntRadix { operand, radix } => {
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
            | Self::UriEncode { operand }
            | Self::ObjectToStringTag { operand }
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

    /// Visit every operand read by this rvalue mutably, in evaluation order.
    ///
    /// This is the exact mutable mirror of [`Rvalue::for_each_operand`]; the two
    /// must enumerate the same operands so analyses and rewrites cannot drift as
    /// variants are added. Exhaustiveness is enforced by the compiler (no
    /// wildcard arm), so a new `Rvalue` variant forces both to be updated.
    pub fn for_each_operand_mut(&mut self, mut visit: impl FnMut(&mut Operand)) {
        match self {
            Self::Use(operand) => visit(operand),
            Self::List(items) | Self::Set(items) | Self::Tuple(items) => {
                for item in items.iter_mut() {
                    visit(item);
                }
            }
            Self::Dict(entries) => {
                for (key, entry_value) in entries.iter_mut() {
                    visit(key);
                    visit(entry_value);
                }
            }
            Self::Closure { captures, .. } => {
                for capture in captures.iter_mut() {
                    visit(capture);
                }
            }
            Self::ClosureCall { callee, args } => {
                visit(callee);
                for arg in args.iter_mut() {
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
                for (_, case) in cases.iter_mut() {
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
                for arg in args.iter_mut() {
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
            Self::PrototypeSentinel { value } => {
                visit(value);
            }
            Self::UnknownCast {
                value: unknown_value,
                ..
            } => {
                visit(unknown_value);
            }
            Self::StringAffix {
                haystack, needle, ..
            }
            | Self::StringSearch {
                haystack,
                needle,
                from_index: None,
                ..
            }
            | Self::StringContains {
                haystack,
                needle,
                from_index: None,
            } => {
                visit(haystack);
                visit(needle);
            }
            Self::StringSearch {
                haystack,
                needle,
                from_index: Some(from_index),
                ..
            }
            | Self::StringContains {
                haystack,
                needle,
                from_index: Some(from_index),
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
                if let Some(start_operand) = start.as_mut() {
                    visit(start_operand);
                }
                if let Some(end_operand) = end.as_mut() {
                    visit(end_operand);
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
            Self::ListSearch {
                list,
                item,
                from_index: None,
                ..
            } => {
                visit(list);
                visit(item);
            }
            Self::ListSearch {
                list,
                item,
                from_index: Some(from_index),
                ..
            } => {
                visit(list);
                visit(item);
                visit(from_index);
            }
            Self::ListCallback { list, callback, .. } => {
                visit(list);
                visit(callback);
            }
            Self::ListFromLength { length } => {
                visit(length);
            }
            Self::ListRepeat { value, count } => {
                visit(value);
                visit(count);
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
                if let Some(operand) = initial.as_mut() {
                    visit(operand);
                }
                visit(callback);
            }
            Self::ListSlice { list, start, end } => {
                visit(list);
                if let Some(operand) = start.as_mut() {
                    visit(operand);
                }
                if let Some(operand) = end.as_mut() {
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
                if let Some(operand) = delete_count.as_mut() {
                    visit(operand);
                }
                for item in items.iter_mut() {
                    visit(&mut item.value);
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
                if let Some(operand) = start.as_mut() {
                    visit(operand);
                }
                if let Some(operand) = end.as_mut() {
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
                if let Some(operand) = end.as_mut() {
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
                if let Some(operand) = depth.as_mut() {
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
                for item in items.iter_mut() {
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
            | Self::ListReversed { list }
            | Self::ListEnumerate { list } => {
                visit(list);
            }
            Self::ListSorted { list, key, .. } => {
                visit(list);
                if let Some(sort_key) = key {
                    visit(sort_key);
                }
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
            Self::ListSort {
                list,
                comparator,
                key,
                ..
            } => {
                visit(list);
                if let Some(sort_comparator) = comparator {
                    visit(sort_comparator);
                }
                if let Some(sort_key) = key {
                    visit(sort_key);
                }
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
                if let Some(operand) = default.as_mut() {
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
                if let Some(operand) = default.as_mut() {
                    visit(operand);
                }
            }
            Self::DictUpdate { dict, other } => {
                visit(dict);
                visit(other);
            }
            Self::DictAssign { target, sources } => {
                visit(target);
                for source in sources.iter_mut() {
                    visit(source);
                }
            }
            Self::CallableObjectAssign {
                callable,
                props,
                spreads,
            } => {
                visit(callable);
                for (_, value) in props.iter_mut() {
                    visit(value);
                }
                for value in spreads.iter_mut() {
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
                if let Some(operand) = limit.as_mut() {
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
            Self::GlobalGet { .. } => {}
            Self::GlobalSet { value, .. } => {
                visit(value);
            }
            Self::DateSetNow { timestamp } => {
                visit(timestamp);
            }
            Self::DateTimezoneOffset | Self::DateResetTimezoneOffset => {}
            Self::DateSetTimezoneOffset { offset } => {
                visit(offset);
            }
            Self::VitestMockFn { implementation } => {
                if let Some(implementation) = implementation {
                    visit(implementation);
                }
            }
            Self::VitestMockCalledTimes { mock, count } => {
                visit(mock);
                visit(count);
            }
            Self::VitestMockCalledWith { mock, args, .. } => {
                visit(mock);
                for arg in args {
                    visit(arg);
                }
            }
            Self::VitestMockLastResolvedWith { mock, expected } => {
                visit(mock);
                visit(expected);
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
                for part in parts.iter_mut() {
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
                for value in values.iter_mut() {
                    visit(value);
                }
            }
            Self::UrlField { url, .. } => visit(url),
            Self::FileReadText { path } => visit(path),
            Self::FileWriteText { path, text } => {
                visit(path);
                visit(text);
            }
            Self::BlobFromParts {
                parts,
                blob_type,
                name,
                last_modified,
            } => {
                visit(parts);
                visit(blob_type);
                if let Some(name_operand) = name {
                    visit(name_operand);
                }
                if let Some(last_modified_operand) = last_modified {
                    visit(last_modified_operand);
                }
            }
            Self::HostGlobalRead { .. } | Self::HostGlobalPresent { .. } => {}
            Self::HostGlobalWrite { value, .. } => {
                visit(value);
            }
            Self::NumericExtrema { args, spread, .. } => {
                for arg in args.iter_mut() {
                    visit(arg);
                }
                if let Some(spread_operand) = spread {
                    visit(spread_operand);
                }
            }
            Self::NumericHypot { args } => {
                for arg in args.iter_mut() {
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
            Self::NumericToFixed { operand, digits } => {
                visit(operand);
                visit(digits);
            }
            Self::ParseIntRadix { operand, radix } => {
                visit(operand);
                visit(radix);
            }
            Self::PrimitiveCast { operand, .. } => visit(operand),
            Self::Unary { operand, .. } => visit(operand),
            Self::Struct { fields, .. } => {
                for (_, field_value) in fields.iter_mut() {
                    visit(field_value);
                }
            }
            Self::ExternalClassInstance { args, .. } => {
                for arg in args.iter_mut() {
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
            | Self::UriEncode { operand }
            | Self::ObjectToStringTag { operand }
            | Self::StringTrim { operand, .. }
            | Self::StringPredicate { operand, .. }
            | Self::Await(operand) => visit(operand),
            Self::AsyncOp { args, .. } => {
                for arg in args.iter_mut() {
                    visit(arg);
                }
            }
        }
    }
}

/// Validate that IDs referenced by an rvalue point to existing MIR entities.
pub(super) fn validate_rvalue_exists(
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
        errors.push(error(format!(
            "closure rvalue references unknown closure {id:?}"
        )));
    }
    if let Rvalue::UnknownCast { target, .. } = value {
        validate_type(mir, *target, errors);
    }
}

/// Validate that IDs referenced by an operand point to existing MIR entities.
pub(super) fn validate_operand_exists(
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
pub(super) fn validate_place_exists(
    function: &MirFunction,
    place: &Place,
    errors: &mut Vec<ValidationError>,
) {
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
pub(super) fn validate_callee_exists(
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
