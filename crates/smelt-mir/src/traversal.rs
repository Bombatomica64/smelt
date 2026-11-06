//! The IR's own traversal: what each statement, terminator and rvalue reads and
//! defines.
//!
//! Three levels answer the same question at growing width:
//!
//! * [`Rvalue::reads`] / [`Rvalue::reads_mut`] enumerate an rvalue's operands in
//!   evaluation order;
//! * [`Statement::reads`] / [`Statement::reads_mut`] add what the statement's
//!   target reads, and [`Statement::defs`] names the local it defines;
//! * [`Terminator::reads`] / [`Terminator::reads_mut`] and [`Terminator::defs`]
//!   do the same for control flow.
//!
//! The `read_locals` and `reads_local` helpers flatten any of these to the locals
//! involved, including the base and index of a projected place. Optimization
//! passes, the validator and the emitter all ask through this module, so a new
//! variant is described once and every analysis sees it.

use std::collections::HashMap;

use crate::{
    Callee, GlobalProjection, LocalId, LocalKind, MirFunction, Operand, Place, Rvalue, Statement,
    Terminator,
};

/// One thing a [`Statement`] reads.
#[derive(Debug, Clone, Copy)]
pub enum Read<'a> {
    /// A local reached directly as the base of an assignment target.
    Base(LocalId),
    /// An operand, either of the stored rvalue or of the target's index.
    Operand(&'a Operand),
}

/// One thing a [`Statement`] reads, in rewritable form.
#[derive(Debug)]
pub enum ReadMut<'a> {
    /// A local reached directly as the base of an assignment target.
    Base(&'a mut LocalId),
    /// An operand, either of the stored rvalue or of the target's index.
    Operand(&'a mut Operand),
}

/// Expands to the one operand walk over an [`Rvalue`], shared by
/// [`Rvalue::reads`] and [`Rvalue::reads_mut`] so the two cannot disagree.
///
/// `$rvalue` is matched through default binding modes, so the same arms bind
/// shared or mutable references; `$mutability` carries the `mut` for the one arm
/// that has to borrow a field explicitly. There is no wildcard arm: a new variant
/// is a compile error here.
macro_rules! walk_rvalue_operands {
    ($rvalue:expr, $visit:ident, $($mutability:tt)?) => {
    match $rvalue {
        Self::Use(operand) => $visit(operand),
        Self::ThisRead => {}
        Self::BindThis { callee, receiver } => {
            $visit(callee);
            $visit(receiver);
        }
        Self::GeneratorYield { value, .. } => $visit(value),
        Self::GeneratorNext {
            generator, value, ..
        } => {
            $visit(generator);
            if let Some(value) = value {
                $visit(value);
            }
        }
        Self::GeneratorDone { result } | Self::GeneratorValue { result } => $visit(result),
        Self::GeneratorDelegate { generator } => $visit(generator),
        Self::HeadersNew { init } => {
            if let Some(init) = init {
                $visit(init);
            }
        }
        Self::HeadersOp { headers, args, .. } => {
            $visit(headers);
            for arg in args {
                $visit(arg);
            }
        }
        Self::EventEmitterNew | Self::ProcessEnvRead { .. } => {}
        Self::EventEmitterOp { emitter, args, .. } => {
            $visit(emitter);
            for arg in args {
                $visit(arg);
            }
        }
        Self::HttpCreateServer { handler } => $visit(handler),
        Self::HttpCreateFetchServer { fetch } => $visit(fetch),
        Self::SqliteOpen { path } => $visit(path),
        Self::SqliteOp { receiver, args, .. } => {
            $visit(receiver);
            for arg in args {
                $visit(arg);
            }
        }
        Self::HttpServerOp { server, args, .. } => {
            $visit(server);
            for arg in args {
                $visit(arg);
            }
        }
        Self::IncomingMessageOp { message, .. } => $visit(message),
        Self::ServerResponseOp { response, args, .. } => {
            $visit(response);
            for arg in args {
                $visit(arg);
            }
        }
        Self::RequestNew {
            input,
            method,
            headers,
            body,
            signal,
            init_members,
        } => {
            $visit(input);
            for operand in [method, headers, body, signal].into_iter().flatten() {
                $visit(operand);
            }
            for (_, operand) in init_members {
                $visit(operand);
            }
        }
        Self::RequestOp { request, args, .. } => {
            $visit(request);
            for arg in args {
                $visit(arg);
            }
        }
        Self::ResponseNew {
            body,
            status,
            status_text,
            headers,
        } => {
            for operand in [body, status, status_text, headers].into_iter().flatten() {
                $visit(operand);
            }
        }
        Self::ResponseOp { response, args, .. } => {
            $visit(response);
            for arg in args {
                $visit(arg);
            }
        }
        Self::TextEncoderNew | Self::GlobalObject => {}
        Self::TextDecoderNew { label } => {
            if let Some(label) = label {
                $visit(label);
            }
        }
        Self::TextEncoderOp { encoder, args, .. } => {
            $visit(encoder);
            for arg in args {
                $visit(arg);
            }
        }
        Self::TextDecoderOp { decoder, args, .. } => {
            $visit(decoder);
            for arg in args {
                $visit(arg);
            }
        }
        Self::TypedArrayNew { args, .. } => {
            for arg in args {
                $visit(arg);
            }
        }
        Self::ByteArrayOp { bytes, args, .. } => {
            $visit(bytes);
            for arg in args {
                $visit(arg);
            }
        }
        Self::UrlSearchParamsNew { init } => {
            if let Some(init) = init {
                $visit(init);
            }
        }
        Self::UrlOp { url, args, .. } => {
            $visit(url);
            for arg in args {
                $visit(arg);
            }
        }
        Self::UrlSearchParamsOp { params, args, .. } => {
            $visit(params);
            for arg in args {
                $visit(arg);
            }
        }
        Self::AbortSignalOp { args, .. }
        | Self::CryptoOp { args, .. }
        | Self::HostModuleCall { args, .. } => {
            for arg in args {
                $visit(arg);
            }
        }
        Self::FormDataNew => {}
        Self::FormDataOp { form, args, .. } => {
            $visit(form);
            for arg in args {
                $visit(arg);
            }
        }
        Self::List(items) | Self::Set(items) | Self::Tuple(items) => {
            for item in items {
                $visit(item);
            }
        }
        Self::Dict(entries) => {
            for (key, entry_value) in entries {
                $visit(key);
                $visit(entry_value);
            }
        }
        Self::Closure { captures, .. } => {
            for capture in captures {
                $visit(capture);
            }
        }
        Self::ClosureCall { callee, args } => {
            $visit(callee);
            for arg in args {
                $visit(arg);
            }
        }
        Self::Construct { callee, args } => {
            $visit(callee);
            for arg in args {
                $visit(arg);
            }
        }
        Self::ClosureCallSpread { callee, args } => {
            $visit(callee);
            $visit(args);
        }
        Self::Binary { lhs, rhs, .. } => {
            $visit(lhs);
            $visit(rhs);
        }
        Self::FunctionTableLookup { key, cases } => {
            $visit(key);
            for (_, case) in cases {
                $visit(case);
            }
        }
        Self::OptionalField { receiver, .. } | Self::BoundField { receiver, .. } => {
            $visit(receiver);
        }
        Self::OptionalIndex { receiver, index } => {
            $visit(receiver);
            $visit(index);
        }
        Self::OptionalMethod { receiver, args, .. }
        | Self::UnionMethod { receiver, args, .. }
        | Self::BoundMethod { receiver, args, .. } => {
            $visit(receiver);
            for arg in args {
                $visit(arg);
            }
        }
        Self::OptionalCoalesce { optional, fallback } => {
            $visit(optional);
            $visit(fallback);
        }
        Self::InstanceOf { value: operand, .. } => {
            $visit(operand);
        }
        Self::InstanceOfValue { value, target } => {
            $visit(value);
            $visit(target);
        }
        Self::UnknownIs {
            value: unknown_value,
            ..
        } => {
            $visit(unknown_value);
        }
        Self::TypeofValue {
            value: unknown_value,
        } => {
            $visit(unknown_value);
        }
        Self::PrototypeSentinel {
            value: unknown_value,
            ..
        } => {
            $visit(unknown_value);
        }
        Self::BoxPrimitive { value } => {
            $visit(value);
        }
        Self::ObjectFromPrototype { prototype } => {
            $visit(prototype);
        }
        Self::DefineProperties {
            target,
            descriptors,
        } => {
            $visit(target);
            $visit(descriptors);
        }
        Self::UnknownCast {
            value: unknown_value,
            ..
        } => {
            $visit(unknown_value);
        }
        Self::StringLocaleCompare {
            left: haystack,
            right: needle,
        }
        | Self::StringAffix {
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
            $visit(haystack);
            $visit(needle);
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
            $visit(haystack);
            $visit(needle);
            $visit(from_index);
        }
        Self::StringReplace {
            haystack,
            pattern,
            replacement,
            ..
        } => {
            $visit(haystack);
            $visit(pattern);
            $visit(replacement);
        }
        Self::StringRemoveAffix {
            haystack, affix, ..
        } => {
            $visit(haystack);
            $visit(affix);
        }
        Self::StringRepeat { operand, count } => {
            $visit(operand);
            $visit(count);
        }
        Self::StringPad {
            operand,
            target_len,
            pad,
            ..
        } => {
            $visit(operand);
            $visit(target_len);
            $visit(pad);
        }
        Self::StringCharAt { operand, index } => {
            $visit(operand);
            $visit(index);
        }
        Self::StringCharCodeAt { operand, index } => {
            $visit(operand);
            $visit(index);
        }
        Self::StringSlice {
            operand,
            start,
            end,
        } => {
            $visit(operand);
            if let Some(start_operand) = start {
                $visit(start_operand);
            }
            if let Some(end_operand) = end {
                $visit(end_operand);
            }
        }
        Self::ListContains { list, item } => {
            $visit(list);
            $visit(item);
        }
        Self::SetContains { set, item } => {
            $visit(set);
            $visit(item);
        }
        Self::SetDisjoint { left, right } => {
            $visit(left);
            $visit(right);
        }
        Self::SetRelation { left, right, .. } => {
            $visit(left);
            $visit(right);
        }
        Self::SetAdd { set, item } | Self::SetRemove { set, item, .. } => {
            $visit(set);
            $visit(item);
        }
        Self::SetClear { set } | Self::SetCopy { set } => {
            $visit(set);
        }
        Self::ListToSet { list } => {
            $visit(list);
        }
        Self::ListPairsToDict { list } => {
            $visit(list);
        }
        Self::SetBinary { left, right, .. } => {
            $visit(left);
            $visit(right);
        }
        Self::SetProjection { set, .. } => {
            $visit(set);
        }
        Self::ListConcat { left, right } => {
            $visit(left);
            $visit(right);
        }
        Self::ConcatSpread { value } => {
            $visit(value);
        }
        Self::ListSearch {
            list,
            item,
            from_index: None,
            ..
        } => {
            $visit(list);
            $visit(item);
        }
        Self::ListSearch {
            list,
            item,
            from_index: Some(from_index),
            ..
        } => {
            $visit(list);
            $visit(item);
            $visit(from_index);
        }
        Self::ListCallback { list, callback, .. } => {
            $visit(list);
            $visit(callback);
        }
        Self::ListFromLength { length } => {
            $visit(length);
        }
        Self::ListFromLengthMap { length, callback } => {
            $visit(length);
            $visit(callback);
        }
        Self::ListReduce {
            list,
            initial,
            callback,
            ..
        } => {
            $visit(list);
            if let Some(operand) = initial {
                $visit(operand);
            }
            $visit(callback);
        }
        Self::ListSlice { list, start, end } => {
            $visit(list);
            if let Some(operand) = start {
                $visit(operand);
            }
            if let Some(operand) = end {
                $visit(operand);
            }
        }
        Self::ListSplice {
            list,
            start,
            delete_count,
            items,
            ..
        } => {
            $visit(list);
            $visit(start);
            if let Some(operand) = delete_count {
                $visit(operand);
            }
            for item in items {
                $visit(& $($mutability)? item.value);
            }
        }
        Self::ListFill {
            list,
            value: fill_value,
            start,
            end,
        } => {
            $visit(list);
            $visit(fill_value);
            if let Some(operand) = start {
                $visit(operand);
            }
            if let Some(operand) = end {
                $visit(operand);
            }
        }
        Self::ListCopyWithin {
            list,
            target,
            start,
            end,
        } => {
            $visit(list);
            $visit(target);
            $visit(start);
            if let Some(operand) = end {
                $visit(operand);
            }
        }
        Self::ListWith {
            list,
            index,
            value: replacement,
        } => {
            $visit(list);
            $visit(index);
            $visit(replacement);
        }
        Self::ListFlat { list, depth } => {
            $visit(list);
            if let Some(operand) = depth {
                $visit(operand);
            }
        }
        Self::ListProjection { list, .. } => {
            $visit(list);
        }
        Self::ListPush { list, item } => {
            $visit(list);
            $visit(item);
        }
        Self::ListExtend { list, other } => {
            $visit(list);
            $visit(other);
        }
        Self::ListInsert { list, index, item } => {
            $visit(list);
            $visit(index);
            $visit(item);
        }
        Self::ListUnshift { list, items } => {
            $visit(list);
            for item in items {
                $visit(item);
            }
        }
        Self::ListReverse { list } => {
            $visit(list);
        }
        Self::ListClear { list } => {
            $visit(list);
        }
        Self::ListCopy { list } => {
            $visit(list);
        }
        Self::TupleToList { tuple } | Self::TupleToSet { tuple } => {
            $visit(tuple);
        }
        Self::ListToTuple { list } => {
            $visit(list);
        }
        Self::ListCount { list, item } => {
            $visit(list);
            $visit(item);
        }
        Self::ListSum { list }
        | Self::ListBoolFold { list, .. }
        | Self::ListReversed { list }
        | Self::ListEnumerate { list } => {
            $visit(list);
        }
        Self::ListSorted { list, key, .. } => {
            $visit(list);
            if let Some(sort_key) = key {
                $visit(sort_key);
            }
        }
        Self::ListZip { left, right } => {
            $visit(left);
            $visit(right);
        }
        Self::ListRange { start, end, step } => {
            $visit(start);
            $visit(end);
            $visit(step);
        }
        Self::ListRandomChoice { list } => {
            $visit(list);
        }
        Self::ListIndex { list, item } => {
            $visit(list);
            $visit(item);
        }
        Self::ListRemove { list, item } => {
            $visit(list);
            $visit(item);
        }
        Self::ListSort {
            list,
            comparator,
            key,
            ..
        } => {
            $visit(list);
            if let Some(sort_comparator) = comparator {
                $visit(sort_comparator);
            }
            if let Some(sort_key) = key {
                $visit(sort_key);
            }
        }
        Self::ListPop { list } => {
            $visit(list);
        }
        Self::ListShift { list } => {
            $visit(list);
        }
        Self::ListNext { list } => {
            $visit(list);
        }
        Self::IteratorDone { result } | Self::IteratorValue { result } => {
            $visit(result);
        }
        Self::TupleContains { tuple, item } => {
            $visit(tuple);
            $visit(item);
        }
        Self::TupleIndex { tuple, .. } | Self::TupleSlice { tuple, .. } => {
            $visit(tuple);
        }
        Self::DictContainsKey { dict, key, .. } => {
            $visit(dict);
            $visit(key);
        }
        Self::DictSet {
            dict,
            key,
            value: dict_value,
        } => {
            $visit(dict);
            $visit(key);
            $visit(dict_value);
        }
        Self::DictRemoveKey { dict, key } => {
            $visit(dict);
            $visit(key);
        }
        Self::DictGet { dict, key, default } => {
            $visit(dict);
            $visit(key);
            if let Some(operand) = default {
                $visit(operand);
            }
        }
        Self::DictSetDefault { dict, key, default } => {
            $visit(dict);
            $visit(key);
            $visit(default);
        }
        Self::DictClear { dict } => {
            $visit(dict);
        }
        Self::DictPop { dict, key, default } => {
            $visit(dict);
            $visit(key);
            if let Some(operand) = default {
                $visit(operand);
            }
        }
        Self::DictUpdate { dict, other } => {
            $visit(dict);
            $visit(other);
        }
        Self::DictAssign { target, sources } => {
            $visit(target);
            for source in sources {
                $visit(source);
            }
        }
        Self::CallableObjectAssign {
            callable,
            props,
            spreads,
        } => {
            $visit(callable);
            for (_, value) in props {
                $visit(value);
            }
            for value in spreads {
                $visit(value);
            }
        }
        Self::DictCopy { dict } => {
            $visit(dict);
        }
        Self::DictProjection { dict, .. } => {
            $visit(dict);
        }
        Self::StringSplit {
            haystack,
            separator,
            limit,
        } => {
            $visit(haystack);
            $visit(separator);
            if let Some(operand) = limit {
                $visit(operand);
            }
        }
        Self::StringChars { haystack } => {
            $visit(haystack);
        }
        Self::StringJoin { items, separator } => {
            $visit(items);
            $visit(separator);
        }
        Self::JsonStringify { value: json_value } => {
            $visit(json_value);
        }
        Self::RegexIsMatch {
            pattern, haystack, ..
        } => {
            $visit(pattern);
            $visit(haystack);
        }
        Self::RegexReplace {
            pattern,
            haystack,
            replacement,
            ..
        } => {
            $visit(pattern);
            $visit(haystack);
            $visit(replacement);
        }
        Self::RegexReplaceCallback {
            pattern,
            haystack,
            callback,
            ..
        } => {
            $visit(pattern);
            $visit(haystack);
            $visit(callback);
        }
        Self::RegexReplaceFirstMatchUppercase { pattern, haystack } => {
            $visit(pattern);
            $visit(haystack);
        }
        Self::RegexSplit { pattern, haystack } => {
            $visit(pattern);
            $visit(haystack);
        }
        Self::RegexFind { pattern, haystack } => {
            $visit(pattern);
            $visit(haystack);
        }
        Self::RegexExec { regex, haystack } | Self::RegexTest { regex, haystack } => {
            $visit(regex);
            $visit(haystack);
        }
        Self::RegexMatchAll { regex, haystack } => {
            $visit(regex);
            $visit(haystack);
        }
        Self::HttpGetText { url } => {
            $visit(url);
        }
        Self::DateNow => {}
        Self::DateResetNow | Self::VitestRestoreAllMocks => {}
        Self::GlobalGet { .. } => {}
        Self::GlobalSet { value, .. } => {
            $visit(value);
        }
        Self::DateSetNow { timestamp } => {
            $visit(timestamp);
        }
        Self::DateTimezoneOffset | Self::DateResetTimezoneOffset => {}
        Self::DateSetTimezoneOffset { offset } => {
            $visit(offset);
        }
        Self::VitestMockFn { implementation } => {
            if let Some(implementation) = implementation {
                $visit(implementation);
            }
        }
        Self::VitestMockCalledTimes { mock, count } => {
            $visit(mock);
            $visit(count);
        }
        Self::VitestMockCalledWith { mock, args, .. } => {
            $visit(mock);
            for arg in args {
                $visit(arg);
            }
        }
        Self::VitestSpyOn { target, name } => {
            $visit(target);
            $visit(name);
        }
        Self::VitestAsymmetricEqual { actual, expected } => {
            $visit(actual);
            $visit(expected);
        }
        Self::VitestMockLastResolvedWith { mock, expected } => {
            $visit(mock);
            $visit(expected);
        }
        Self::DateTimezoneContext { timezone } => {
            $visit(timezone);
        }
        Self::DateToIsoString { timestamp_ms } => {
            $visit(timestamp_ms);
        }
        Self::DateToString { timestamp_ms } => {
            $visit(timestamp_ms);
        }
        Self::DateToUtcString { timestamp_ms } => {
            $visit(timestamp_ms);
        }
        Self::JsonIndent { text, space } => {
            $visit(text);
            $visit(space);
        }
        Self::DateFromParts { parts } => {
            for part in parts {
                $visit(part);
            }
        }
        Self::DateFromValue { value: date_value } => {
            $visit(date_value);
        }
        Self::DateGetPart { timestamp_ms, .. } => {
            $visit(timestamp_ms);
        }
        Self::DateSetPart {
            timestamp_ms,
            values,
            ..
        } => {
            $visit(timestamp_ms);
            for value in values {
                $visit(value);
            }
        }
        Self::UrlField { url, .. } => $visit(url),
        Self::FileReadText { path } => $visit(path),
        Self::FileWriteText { path, text } => {
            $visit(path);
            $visit(text);
        }
        Self::BlobOp { blob, args, .. } => {
            $visit(blob);
            for arg in args {
                $visit(arg);
            }
        }
        Self::BlobFromParts {
            parts,
            blob_type,
            name,
            last_modified,
        } => {
            $visit(parts);
            $visit(blob_type);
            if let Some(name_operand) = name {
                $visit(name_operand);
            }
            if let Some(last_modified_operand) = last_modified {
                $visit(last_modified_operand);
            }
        }
        Self::HostConstruct { args, .. } => {
            for arg in args {
                $visit(arg);
            }
        }
        Self::ArgumentsObject { fixed, rest } => {
            for value in fixed {
                $visit(value);
            }
            if let Some(rest_operand) = rest {
                $visit(rest_operand);
            }
        }
        Self::BuiltinNamespace { .. } => {}
        Self::HostGlobalRead { .. } | Self::HostGlobalPresent { .. } => {}
        Self::HostGlobalWrite { value, .. } => {
            $visit(value);
        }
        Self::NumericExtrema { args, spread, .. } => {
            for arg in args {
                $visit(arg);
            }
            if let Some(spread_operand) = spread {
                $visit(spread_operand);
            }
        }
        Self::NumericHypot { args } => {
            for arg in args {
                $visit(arg);
            }
        }
        Self::NumericPow { base, exponent } => {
            $visit(base);
            $visit(exponent);
        }
        Self::NumericAtan2 { y, x } => {
            $visit(y);
            $visit(x);
        }
        Self::NumericRandom => {}
        Self::NumericRandomInt { start, end } => {
            $visit(start);
            $visit(end);
        }
        Self::NumericToStringRadix { operand, radix } => {
            $visit(operand);
            $visit(radix);
        }
        Self::NumericToFixed { operand, digits } => {
            $visit(operand);
            $visit(digits);
        }
        Self::ParseIntRadix { operand, radix } => {
            $visit(operand);
            $visit(radix);
        }
        Self::PrimitiveCast { operand, .. } => $visit(operand),
        Self::Unary { operand, .. } => $visit(operand),
        Self::Struct { fields, .. } => {
            for (_, field_value) in fields {
                $visit(field_value);
            }
        }
        Self::ExternalClassInstance { args, .. } => {
            for arg in args {
                $visit(arg);
            }
        }
        Self::Len(operand)
        | Self::NumericAbs(operand)
        | Self::NumericRound { operand, .. }
        | Self::NumericPredicate { operand, .. }
        | Self::NumericUnaryFunc { operand, .. }
        | Self::StringCase { operand, .. }
        | Self::StringNormalize { operand, .. }
        | Self::UriTranscode { operand, .. }
        | Self::ObjectToStringTag { operand }
        | Self::StructuredClone { operand }
        | Self::StringTrim { operand, .. }
        | Self::StringPredicate { operand, .. }
        | Self::Await(operand) => $visit(operand),
        Self::AsyncOp { args, .. } => {
            for arg in args {
                $visit(arg);
            }
        }
    }
    };
}

/// Expands to the one operand walk over a [`Terminator`], shared by
/// [`Terminator::reads`] and [`Terminator::reads_mut`].
macro_rules! walk_terminator_operands {
    ($terminator:expr, $visit:ident) => {
        match $terminator {
            Terminator::Goto(_) => {}
            Terminator::Call { callee, args, .. } => {
                if let Callee::Indirect(operand) = callee {
                    $visit(operand);
                }
                for arg in args {
                    $visit(arg);
                }
            }
            Terminator::Await { future: operand, .. }
            | Terminator::Switch { cond: operand, .. }
            | Terminator::Match {
                scrutinee: operand, ..
            }
            | Terminator::Return(operand)
            | Terminator::Throw(operand) => $visit(operand),
        }
    };
}

impl Operand {
    /// The place this operand reads, if it reads one.
    #[must_use]
    pub const fn place(&self) -> Option<&Place> {
        match self {
            Self::Copy(place) | Self::Move(place) => Some(place),
            Self::Const(_) => None,
        }
    }

    /// The local this operand reads directly, ignoring projections.
    #[must_use]
    pub const fn local(&self) -> Option<LocalId> {
        match self.place() {
            Some(Place::Local(local)) => Some(*local),
            Some(Place::Field { .. } | Place::Index { .. } | Place::Global { .. }) | None => None,
        }
    }

    /// Visit every local this operand reads, through any projection.
    pub fn read_locals(&self, visit: &mut dyn FnMut(LocalId)) {
        if let Some(place) = self.place() {
            place.read_locals(visit);
        }
    }

    /// Whether this operand reads `local`, through any projection.
    #[must_use]
    pub fn reads_local(&self, local: LocalId) -> bool {
        self.place().is_some_and(|place| place.reads_local(local))
    }
}

impl Place {
    /// Visit every local read when this place is read as a value: its base and
    /// any index operand. A global-rooted place has no base local, but its index
    /// operand still reads one.
    pub fn read_locals(&self, visit: &mut dyn FnMut(LocalId)) {
        match self {
            Self::Local(local) | Self::Field { base: local, .. } => visit(*local),
            Self::Index { base, index, .. } => {
                visit(*base);
                index.read_locals(visit);
            }
            Self::Global { projection, .. } => match projection {
                GlobalProjection::Field(_) => {}
                GlobalProjection::Index { index, .. } => index.read_locals(visit),
            },
        }
    }

    /// Whether reading this place as a value reads `local`.
    #[must_use]
    pub fn reads_local(&self, local: LocalId) -> bool {
        let mut found = false;
        self.read_locals(&mut |read| found |= read == local);
        found
    }

    /// Whether assigning to this place reads `local` before writing. A whole
    /// local is a definition and reads nothing; a projection reads its base,
    /// which stays live, and any index operand.
    #[must_use]
    pub fn target_reads_local(&self, local: LocalId) -> bool {
        !matches!(self, Self::Local(_)) && self.reads_local(local)
    }
}

impl Rvalue {
    /// Visit every operand read by this rvalue, in evaluation order.
    pub fn reads(&self, mut visit: impl FnMut(&Operand)) {
        walk_rvalue_operands!(self, visit,);
    }

    /// Visit every operand read by this rvalue mutably, in evaluation order.
    pub fn reads_mut(&mut self, mut visit: impl FnMut(&mut Operand)) {
        walk_rvalue_operands!(self, visit, mut);
    }

    /// The collection operand this rvalue mutates in place, if any.
    ///
    /// This is the one list of receiver-mutating operations; copies and
    /// non-mutating projections (`DictCopy`, `SetCopy`, a non-mutating
    /// `ListSplice`) answer `None`.
    #[must_use]
    pub const fn mutated_receiver(&self) -> Option<&Operand> {
        match self {
            Self::ListPush { list, .. }
            | Self::ListExtend { list, .. }
            | Self::ListInsert { list, .. }
            | Self::ListUnshift { list, .. }
            | Self::ListReverse { list }
            | Self::ListSplice {
                list, mutate: true, ..
            }
            | Self::ListFill { list, .. }
            | Self::ListCopyWithin { list, .. }
            | Self::ListClear { list }
            | Self::ListRemove { list, .. }
            | Self::ListSort { list, .. }
            | Self::ListPop { list }
            | Self::ListShift { list }
            | Self::ListNext { list } => Some(list),
            Self::SetAdd { set, .. } | Self::SetRemove { set, .. } | Self::SetClear { set } => {
                Some(set)
            }
            Self::DictClear { dict }
            | Self::DictPop { dict, .. }
            | Self::DictSet { dict, .. }
            | Self::DictRemoveKey { dict, .. }
            | Self::DictSetDefault { dict, .. }
            | Self::DictUpdate { dict, .. } => Some(dict),
            Self::DictAssign { target, .. } => Some(target),
            _ => None,
        }
    }

    /// Visit every local this rvalue reads, through any projection.
    pub fn read_locals(&self, mut visit: impl FnMut(LocalId)) {
        self.reads(|operand| operand.read_locals(&mut visit));
    }

    /// Whether any operand of this rvalue reads `local`.
    #[must_use]
    pub fn reads_local(&self, local: LocalId) -> bool {
        let mut found = false;
        self.reads(|operand| found |= operand.reads_local(local));
        found
    }
}

impl Read<'_> {
    /// Visit every local this read touches, through any projection.
    pub fn read_locals(self, visit: &mut dyn FnMut(LocalId)) {
        match self {
            Self::Base(local) => visit(local),
            Self::Operand(operand) => operand.read_locals(visit),
        }
    }
}

impl Statement {
    /// Visit everything this statement reads: the target's base and index (a
    /// whole-local target is a definition and contributes nothing), then the
    /// stored rvalue's operands.
    pub fn reads(&self, mut visit: impl FnMut(Read<'_>)) {
        match self {
            Self::Assign { value, .. } => value.reads(|operand| visit(Read::Operand(operand))),
            Self::AssignPlace { place, value } => {
                match place {
                    Place::Local(_) => {}
                    Place::Field { base, .. } => visit(Read::Base(*base)),
                    Place::Index { base, index, .. } => {
                        visit(Read::Base(*base));
                        visit(Read::Operand(index));
                    }
                    Place::Global { projection, .. } => {
                        if let GlobalProjection::Index { index, .. } = projection {
                            visit(Read::Operand(index));
                        }
                    }
                }
                value.reads(|operand| visit(Read::Operand(operand)));
            }
            // The container is mutated in place through its base, exactly like
            // an index target; `current` is the definition and is read only
            // through the stored rvalue.
            Self::DictEntryUpdate {
                base,
                index,
                default,
                current: _,
                value,
            } => {
                visit(Read::Base(*base));
                visit(Read::Operand(index));
                visit(Read::Operand(default));
                value.reads(|operand| visit(Read::Operand(operand)));
            }
        }
    }

    /// Visit everything [`Self::reads`] visits, in rewritable form.
    pub fn reads_mut(&mut self, mut visit: impl FnMut(ReadMut<'_>)) {
        match self {
            Self::Assign { value, .. } => {
                value.reads_mut(|operand| visit(ReadMut::Operand(operand)));
            }
            Self::AssignPlace { place, value } => {
                match place {
                    Place::Local(_) => {}
                    Place::Field { base, .. } => visit(ReadMut::Base(base)),
                    Place::Index { base, index, .. } => {
                        visit(ReadMut::Base(base));
                        visit(ReadMut::Operand(index));
                    }
                    Place::Global { projection, .. } => {
                        if let GlobalProjection::Index { index, .. } = projection {
                            visit(ReadMut::Operand(index));
                        }
                    }
                }
                value.reads_mut(|operand| visit(ReadMut::Operand(operand)));
            }
            Self::DictEntryUpdate {
                base,
                index,
                default,
                current: _,
                value,
            } => {
                visit(ReadMut::Base(base));
                visit(ReadMut::Operand(index));
                visit(ReadMut::Operand(default));
                value.reads_mut(|operand| visit(ReadMut::Operand(operand)));
            }
        }
    }

    /// The rvalue this statement evaluates.
    #[must_use]
    pub const fn value(&self) -> &Rvalue {
        match self {
            Self::Assign { value, .. }
            | Self::AssignPlace { value, .. }
            | Self::DictEntryUpdate { value, .. } => value,
        }
    }

    /// The rvalue this statement evaluates, in rewritable form.
    pub const fn value_mut(&mut self) -> &mut Rvalue {
        match self {
            Self::Assign { value, .. }
            | Self::AssignPlace { value, .. }
            | Self::DictEntryUpdate { value, .. } => value,
        }
    }

    /// The local this statement defines, if any. A projected assignment target
    /// mutates its base without defining it.
    #[must_use]
    pub const fn defs(&self) -> Option<LocalId> {
        match self {
            Self::Assign { dest: local, .. }
            | Self::AssignPlace {
                place: Place::Local(local),
                ..
            }
            | Self::DictEntryUpdate { current: local, .. } => Some(*local),
            Self::AssignPlace { .. } => None,
        }
    }

    /// Visit every local this statement reads, through any projection.
    pub fn read_locals(&self, mut visit: impl FnMut(LocalId)) {
        self.reads(|read| read.read_locals(&mut visit));
    }

    /// Whether this statement reads `local`.
    #[must_use]
    pub fn reads_local(&self, local: LocalId) -> bool {
        let mut found = false;
        self.read_locals(|read| found |= read == local);
        found
    }
}

impl Terminator {
    /// Visit every operand this terminator reads, including an indirect callee.
    pub fn reads(&self, mut visit: impl FnMut(&Operand)) {
        walk_terminator_operands!(self, visit);
    }

    /// Visit every operand [`Self::reads`] visits, in rewritable form.
    pub fn reads_mut(&mut self, mut visit: impl FnMut(&mut Operand)) {
        walk_terminator_operands!(self, visit);
    }

    /// The local this terminator defines: a call's or await's destination.
    #[must_use]
    pub const fn defs(&self) -> Option<LocalId> {
        match self {
            Self::Call { dest, .. } | Self::Await { dest, .. } => Some(*dest),
            Self::Goto(_)
            | Self::Switch { .. }
            | Self::Match { .. }
            | Self::Return(_)
            | Self::Throw(_) => None,
        }
    }

    /// Visit every local this terminator reads, through any projection.
    pub fn read_locals(&self, mut visit: impl FnMut(LocalId)) {
        self.reads(|operand| operand.read_locals(&mut visit));
    }

    /// Whether this terminator reads `local`.
    #[must_use]
    pub fn reads_local(&self, local: LocalId) -> bool {
        let mut found = false;
        self.reads(|operand| found |= operand.reads_local(local));
        found
    }
}

impl MirFunction {
    /// How many times each local is defined anywhere in this body: every
    /// [`Statement::defs`] and [`Terminator::defs`]. This is the one answer to
    /// "is this local assigned once".
    #[must_use]
    pub fn assignment_counts(&self) -> HashMap<LocalId, usize> {
        let mut counts: HashMap<LocalId, usize> = HashMap::new();
        for block in &self.blocks {
            let statements = block.statements.iter().map(Statement::defs);
            let terminator = block.terminator.as_ref().map(Terminator::defs);
            for local in statements.chain(terminator).flatten() {
                let count = counts.entry(local).or_default();
                *count = count.saturating_add(1);
            }
        }
        counts
    }

    /// How many times `local` is defined; see [`Self::assignment_counts`].
    #[must_use]
    pub fn assignment_count(&self, local: LocalId) -> usize {
        self.blocks
            .iter()
            .map(|block| {
                let statements = block.statements.iter().map(Statement::defs);
                let terminator = block.terminator.as_ref().map(Terminator::defs);
                statements
                    .chain(terminator)
                    .filter(|defined| *defined == Some(local))
                    .count()
            })
            .fold(0, usize::saturating_add)
    }

    /// How many times `local` is read anywhere in this body.
    #[must_use]
    pub fn read_count(&self, local: LocalId) -> usize {
        let mut count: usize = 0;
        let mut tally = |read: LocalId| count = count.saturating_add(usize::from(read == local));
        for block in &self.blocks {
            for statement in &block.statements {
                statement.read_locals(&mut tally);
            }
            if let Some(terminator) = &block.terminator {
                terminator.read_locals(&mut tally);
            }
        }
        count
    }

    /// Whether `local` is a compiler temporary.
    #[must_use]
    pub fn is_temp(&self, local: LocalId) -> bool {
        self.locals
            .get(usize::try_from(local.0).unwrap_or(usize::MAX))
            .is_some_and(|decl| decl.kind == LocalKind::Temp)
    }

    /// Whether `local` is a temporary assigned once and read once.
    #[must_use]
    pub fn is_single_use_temp(&self, local: LocalId) -> bool {
        self.is_temp(local) && self.assignment_count(local) == 1 && self.read_count(local) == 1
    }
}

#[cfg(test)]
#[expect(clippy::indexing_slicing, reason = "fixtures index their own one block")]
mod tests {
    //! The read/define contract every analysis relies on, over one function
    //! with locals `%0`..`%2` and a single block.

    use smelt_hir::{BodyId, FileId, Span, Symbol, TypeId};

    use crate::{
        BlockId, Callee, FuncId, HirOrigin, LocalDecl, LocalId, LocalKind, MirFunction, Operand,
        Place, Rvalue, Statement, Terminator,
    };

    const A: LocalId = LocalId(0);
    const B: LocalId = LocalId(1);
    const C: LocalId = LocalId(2);

    /// `copy %n`.
    fn copy(local: LocalId) -> Operand {
        Operand::Copy(Place::Local(local))
    }

    /// A body of temporaries running `statements` and ending in `terminator`.
    fn function(statements: Vec<Statement>, terminator: Terminator) -> MirFunction {
        let span = Span::new(FileId(0), 0, 0);
        let mut function =
            MirFunction::new(FuncId(0), Symbol(0), HirOrigin::Body(BodyId(0)), TypeId(0), span);
        for _ in 0_i32..3_i32 {
            function.push_local(LocalDecl { ty: TypeId(0), kind: LocalKind::Temp, span });
        }
        function.blocks[0].statements = statements;
        function.blocks[0].terminator = Some(terminator);
        function
    }

    /// `%dest = callee(args)` through an indirect callee.
    fn indirect_call(callee: LocalId, args: Vec<Operand>, dest: LocalId) -> Terminator {
        Terminator::Call {
            callee: Callee::Indirect(copy(callee)),
            args,
            dest,
            target: BlockId(0),
            unwind: None,
        }
    }

    /// A whole-local target is a definition; a projected one reads its base.
    #[test]
    fn whole_local_target_defines_and_projected_target_reads() {
        let whole = Statement::AssignPlace {
            place: Place::Local(A),
            value: Rvalue::Use(copy(B)),
        };
        assert_eq!(whole.defs(), Some(A));
        assert!(!whole.reads_local(A), "a whole-local write reads nothing of it");
        assert!(whole.reads_local(B));

        let projected = Statement::AssignPlace {
            place: Place::Index {
                base: A,
                index: Box::new(copy(C)),
                negative: crate::NegativeIndex::FromEnd,
            },
            value: Rvalue::Use(copy(B)),
        };
        assert_eq!(projected.defs(), None, "a projection mutates, it does not define");
        for local in [A, B, C] {
            assert!(projected.reads_local(local), "{local:?} is read");
        }
    }

    /// An indirect callee is a read like any argument.
    #[test]
    fn read_count_includes_an_indirect_callee() {
        let body = function(Vec::new(), indirect_call(A, vec![copy(A), copy(B)], C));
        assert_eq!(body.read_count(A), 2, "once as callee, once as argument");
        assert_eq!(body.read_count(B), 1);
    }

    /// `splice` mutates its receiver; a non-mutating splice is a copy.
    #[test]
    fn mutated_receiver_follows_the_splice_mutate_flag() {
        let splice = |mutate| Rvalue::ListSplice {
            list: copy(A),
            start: copy(B),
            delete_count: None,
            items: Vec::new(),
            mutate,
        };
        assert_eq!(splice(true).mutated_receiver().and_then(Operand::local), Some(A));
        assert!(splice(false).mutated_receiver().is_none());
        let unshift = Rvalue::ListUnshift { list: copy(A), items: Vec::new() };
        assert_eq!(unshift.mutated_receiver().and_then(Operand::local), Some(A));
        assert!(Rvalue::Use(copy(A)).mutated_receiver().is_none());
    }

    /// Every kind of definition counts toward "assigned once".
    #[test]
    fn assignment_count_sees_every_definition() {
        let body = function(
            vec![
                Statement::Assign { dest: A, value: Rvalue::Use(copy(B)) },
                Statement::AssignPlace { place: Place::Local(A), value: Rvalue::Use(copy(B)) },
            ],
            indirect_call(B, Vec::new(), A),
        );
        assert_eq!(body.assignment_count(A), 3);
        assert_eq!(body.assignment_counts().get(&A), Some(&3));
        assert_eq!(body.assignment_count(B), 0);
    }
}
