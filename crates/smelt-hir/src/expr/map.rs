//! Structural child-expression remapping for HIR expression kinds.
//!
//! [`ExprKind::try_map_child_exprs`] rebuilds an expression kind with every
//! direct child [`ExprId`] passed through a caller-supplied mapping function.
//! It exists so passes that copy expressions between bodies (for example
//! const-item inlining in the frontends) do not need to enumerate every
//! expression variant themselves: the match below is exhaustive, so adding a
//! new [`ExprKind`] variant forces this mapper to be updated alongside it.
//!
//! The mapper is intentionally shallow. It only rewrites the [`ExprId`]s
//! stored directly inside one `ExprKind` value; callers drive recursion by
//! re-invoking the mapper from inside their mapping function. Identifiers
//! that point into crate-global arenas ([`crate::ids::BodyId`],
//! [`crate::ids::ItemId`], [`crate::ids::TypeId`], [`crate::ids::Symbol`])
//! are preserved untouched, as are body-local references
//! ([`crate::ids::LocalId`] in [`ExprKind::Local`] and
//! [`crate::ids::BlockId`] in [`ExprKind::Block`]) — callers that move
//! expressions across bodies must decide separately whether those are valid
//! in the destination body.

use super::kinds::{ExprKind, ListSpliceItem};
use crate::ids::ExprId;

/// Map every element of a child-expression list through `f`.
fn map_vec<E>(
    items: Vec<ExprId>,
    f: &mut impl FnMut(ExprId) -> Result<ExprId, E>,
) -> Result<Vec<ExprId>, E> {
    items.into_iter().map(f).collect()
}

/// Map an optional child expression through `f`, preserving `None`.
fn map_opt<E>(
    item: Option<ExprId>,
    f: &mut impl FnMut(ExprId) -> Result<ExprId, E>,
) -> Result<Option<ExprId>, E> {
    item.map(f).transpose()
}

impl ExprKind {
    /// Rebuild this expression kind with each direct child [`ExprId`] mapped
    /// through `f`, leaving all other payload (operators, symbols, types,
    /// bodies, blocks, locals) unchanged.
    ///
    /// # Errors
    ///
    /// Returns the first error produced by `f`; no partial mapping is
    /// observable because the kind is rebuilt by value.
    #[expect(
        clippy::too_many_lines,
        reason = "exhaustive match over every ExprKind variant is intentionally one function"
    )]
    pub fn try_map_child_exprs<E>(
        self,
        f: &mut impl FnMut(ExprId) -> Result<ExprId, E>,
    ) -> Result<Self, E> {
        Ok(match self {
            // Leaf kinds without direct child expression IDs. `Closure` and
            // `Lambda` reference crate-global bodies; `Local` and `Block`
            // reference body-local arenas that this shallow mapper preserves.
            kind @ (Self::Literal(_)
            | Self::Local(_)
            | Self::Item(_)
            | Self::GlobalGet { .. }
            | Self::Closure(_)
            | Self::NumericRandom
            | Self::DateNow
            | Self::DateResetNow
            | Self::DateTimezoneOffset
            | Self::DateResetTimezoneOffset
            | Self::Block(_)
            | Self::Lambda { .. }) => kind,
            Self::Call { callee, args } => Self::Call {
                callee: f(callee)?,
                args: map_vec(args, f)?,
            },
            Self::GeneratorYield { value } => Self::GeneratorYield { value: f(value)? },
            Self::GeneratorNext {
                generator,
                value,
                kind,
            } => Self::GeneratorNext {
                generator: f(generator)?,
                value: value.map(&mut *f).transpose()?,
                kind,
            },
            Self::GeneratorDone { result } => Self::GeneratorDone { result: f(result)? },
            Self::GeneratorValue { result } => Self::GeneratorValue { result: f(result)? },
            Self::GeneratorDelegate { generator } => Self::GeneratorDelegate {
                generator: f(generator)?,
            },
            Self::ClosureCall { callee, args } => Self::ClosureCall {
                callee: f(callee)?,
                args: map_vec(args, f)?,
            },
            Self::ClosureCallSpread { callee, args } => Self::ClosureCallSpread {
                callee: f(callee)?,
                args: f(args)?,
            },
            Self::Method {
                receiver,
                method,
                args,
            } => Self::Method {
                receiver: f(receiver)?,
                method,
                args: map_vec(args, f)?,
            },
            Self::Field { receiver, field } => Self::Field {
                receiver: f(receiver)?,
                field,
            },
            Self::OptionalField { receiver, field } => Self::OptionalField {
                receiver: f(receiver)?,
                field,
            },
            Self::Index { receiver, index } => Self::Index {
                receiver: f(receiver)?,
                index: f(index)?,
            },
            Self::OptionalIndex { receiver, index } => Self::OptionalIndex {
                receiver: f(receiver)?,
                index: f(index)?,
            },
            Self::OptionalMethod {
                receiver,
                method,
                args,
            } => Self::OptionalMethod {
                receiver: f(receiver)?,
                method,
                args: map_vec(args, f)?,
            },
            Self::OptionalCoalesce { optional, fallback } => Self::OptionalCoalesce {
                optional: f(optional)?,
                fallback: f(fallback)?,
            },
            Self::GlobalSet { item, value } => Self::GlobalSet {
                item,
                value: f(value)?,
            },
            Self::TypeAssert { value } => Self::TypeAssert { value: f(value)? },
            Self::Len { operand } => Self::Len {
                operand: f(operand)?,
            },
            Self::NumericAbs { operand } => Self::NumericAbs {
                operand: f(operand)?,
            },
            Self::NumericRound { op, operand } => Self::NumericRound {
                op,
                operand: f(operand)?,
            },
            Self::NumericExtrema { op, args, spread } => Self::NumericExtrema {
                op,
                args: map_vec(args, f)?,
                spread: map_opt(spread, f)?,
            },
            Self::NumericHypot { args } => Self::NumericHypot {
                args: map_vec(args, f)?,
            },
            Self::NumericPredicate { op, operand } => Self::NumericPredicate {
                op,
                operand: f(operand)?,
            },
            Self::NumericUnaryFunc { op, operand } => Self::NumericUnaryFunc {
                op,
                operand: f(operand)?,
            },
            Self::NumericPow { base, exponent } => Self::NumericPow {
                base: f(base)?,
                exponent: f(exponent)?,
            },
            Self::NumericAtan2 { y, x } => Self::NumericAtan2 { y: f(y)?, x: f(x)? },
            Self::NumericRandomInt { start, end } => Self::NumericRandomInt {
                start: f(start)?,
                end: f(end)?,
            },
            Self::NumericToStringRadix { operand, radix } => Self::NumericToStringRadix {
                operand: f(operand)?,
                radix: f(radix)?,
            },
            Self::NumericToFixed { operand, digits } => Self::NumericToFixed {
                operand: f(operand)?,
                digits: f(digits)?,
            },
            Self::ParseIntRadix { operand, radix } => Self::ParseIntRadix {
                operand: f(operand)?,
                radix: f(radix)?,
            },
            Self::PrimitiveCast { op, operand } => Self::PrimitiveCast {
                op,
                operand: f(operand)?,
            },
            Self::StringCase { op, operand } => Self::StringCase {
                op,
                operand: f(operand)?,
            },
            Self::StringNormalize { form, operand } => Self::StringNormalize {
                form,
                operand: f(operand)?,
            },
            Self::UriEncode { operand } => Self::UriEncode {
                operand: f(operand)?,
            },
            Self::ObjectToStringTag { operand } => Self::ObjectToStringTag {
                operand: f(operand)?,
            },
            Self::StructuredClone { operand } => Self::StructuredClone {
                operand: f(operand)?,
            },
            Self::StringTrim { side, operand } => Self::StringTrim {
                side,
                operand: f(operand)?,
            },
            Self::StringAffix {
                op,
                haystack,
                needle,
            } => Self::StringAffix {
                op,
                haystack: f(haystack)?,
                needle: f(needle)?,
            },
            Self::StringSearch {
                op,
                haystack,
                needle,
                from_index,
            } => Self::StringSearch {
                op,
                haystack: f(haystack)?,
                needle: f(needle)?,
                from_index: map_opt(from_index, f)?,
            },
            Self::StringReplace {
                op,
                haystack,
                pattern,
                replacement,
            } => Self::StringReplace {
                op,
                haystack: f(haystack)?,
                pattern: f(pattern)?,
                replacement: f(replacement)?,
            },
            Self::StringRemoveAffix {
                op,
                haystack,
                affix,
            } => Self::StringRemoveAffix {
                op,
                haystack: f(haystack)?,
                affix: f(affix)?,
            },
            Self::StringRepeat { operand, count } => Self::StringRepeat {
                operand: f(operand)?,
                count: f(count)?,
            },
            Self::StringPad {
                op,
                operand,
                target_len,
                pad,
            } => Self::StringPad {
                op,
                operand: f(operand)?,
                target_len: f(target_len)?,
                pad: f(pad)?,
            },
            Self::StringPredicate { op, operand } => Self::StringPredicate {
                op,
                operand: f(operand)?,
            },
            Self::RegexIsMatch {
                op,
                pattern,
                haystack,
            } => Self::RegexIsMatch {
                op,
                pattern: f(pattern)?,
                haystack: f(haystack)?,
            },
            Self::RegexReplace {
                op,
                pattern,
                haystack,
                replacement,
            } => Self::RegexReplace {
                op,
                pattern: f(pattern)?,
                haystack: f(haystack)?,
                replacement: f(replacement)?,
            },
            Self::RegexReplaceCallback {
                op,
                pattern,
                haystack,
                callback,
            } => Self::RegexReplaceCallback {
                op,
                pattern: f(pattern)?,
                haystack: f(haystack)?,
                callback: f(callback)?,
            },
            Self::RegexReplaceFirstMatchUppercase { pattern, haystack } => {
                Self::RegexReplaceFirstMatchUppercase {
                    pattern: f(pattern)?,
                    haystack: f(haystack)?,
                }
            }
            Self::RegexSplit { pattern, haystack } => Self::RegexSplit {
                pattern: f(pattern)?,
                haystack: f(haystack)?,
            },
            Self::RegexFind { pattern, haystack } => Self::RegexFind {
                pattern: f(pattern)?,
                haystack: f(haystack)?,
            },
            Self::RegexExec { regex, haystack } => Self::RegexExec {
                regex: f(regex)?,
                haystack: f(haystack)?,
            },
            Self::RegexMatchAll { regex, haystack } => Self::RegexMatchAll {
                regex: f(regex)?,
                haystack: f(haystack)?,
            },
            Self::StringCharAt { operand, index } => Self::StringCharAt {
                operand: f(operand)?,
                index: f(index)?,
            },
            Self::StringCharCodeAt { operand, index } => Self::StringCharCodeAt {
                operand: f(operand)?,
                index: f(index)?,
            },
            Self::StringContains {
                haystack,
                needle,
                from_index,
            } => Self::StringContains {
                haystack: f(haystack)?,
                needle: f(needle)?,
                from_index: map_opt(from_index, f)?,
            },
            Self::StringSlice {
                operand,
                start,
                end,
            } => Self::StringSlice {
                operand: f(operand)?,
                start: map_opt(start, f)?,
                end: map_opt(end, f)?,
            },
            Self::ListContains { list, item } => Self::ListContains {
                list: f(list)?,
                item: f(item)?,
            },
            Self::SetContains { set, item } => Self::SetContains {
                set: f(set)?,
                item: f(item)?,
            },
            Self::SetDisjoint { left, right } => Self::SetDisjoint {
                left: f(left)?,
                right: f(right)?,
            },
            Self::SetRelation { op, left, right } => Self::SetRelation {
                op,
                left: f(left)?,
                right: f(right)?,
            },
            Self::SetAdd { set, item } => Self::SetAdd {
                set: f(set)?,
                item: f(item)?,
            },
            Self::SetRemove { op, set, item } => Self::SetRemove {
                op,
                set: f(set)?,
                item: f(item)?,
            },
            Self::SetClear { set } => Self::SetClear { set: f(set)? },
            Self::SetCopy { set } => Self::SetCopy { set: f(set)? },
            Self::SetBinary { op, left, right } => Self::SetBinary {
                op,
                left: f(left)?,
                right: f(right)?,
            },
            Self::SetProjection { op, set } => Self::SetProjection { op, set: f(set)? },
            Self::ListConcat { left, right } => Self::ListConcat {
                left: f(left)?,
                right: f(right)?,
            },
            Self::ListSearch {
                op,
                list,
                item,
                from_index,
            } => Self::ListSearch {
                op,
                list: f(list)?,
                item: f(item)?,
                from_index: map_opt(from_index, f)?,
            },
            Self::ListCallback { op, list, callback } => Self::ListCallback {
                op,
                list: f(list)?,
                callback: f(callback)?,
            },
            Self::ListFromLength { length } => Self::ListFromLength { length: f(length)? },
            Self::ListRepeat { value, count } => Self::ListRepeat {
                value: f(value)?,
                count: f(count)?,
            },
            Self::ListFromLengthMap { length, callback } => Self::ListFromLengthMap {
                length: f(length)?,
                callback: f(callback)?,
            },
            Self::ListReduce {
                list,
                initial,
                callback,
            } => Self::ListReduce {
                list: f(list)?,
                initial: map_opt(initial, f)?,
                callback: f(callback)?,
            },
            Self::ListSlice { list, start, end } => Self::ListSlice {
                list: f(list)?,
                start: map_opt(start, f)?,
                end: map_opt(end, f)?,
            },
            Self::ListSplice {
                list,
                start,
                delete_count,
                items,
                mutate,
            } => Self::ListSplice {
                list: f(list)?,
                start: f(start)?,
                delete_count: map_opt(delete_count, f)?,
                items: items
                    .into_iter()
                    .map(|item| {
                        Ok(ListSpliceItem {
                            value: f(item.value)?,
                            spread: item.spread,
                        })
                    })
                    .collect::<Result<Vec<_>, E>>()?,
                mutate,
            },
            Self::ListFill {
                list,
                value,
                start,
                end,
            } => Self::ListFill {
                list: f(list)?,
                value: f(value)?,
                start: map_opt(start, f)?,
                end: map_opt(end, f)?,
            },
            Self::ListCopyWithin {
                list,
                target,
                start,
                end,
            } => Self::ListCopyWithin {
                list: f(list)?,
                target: f(target)?,
                start: f(start)?,
                end: map_opt(end, f)?,
            },
            Self::ListWith { list, index, value } => Self::ListWith {
                list: f(list)?,
                index: f(index)?,
                value: f(value)?,
            },
            Self::ListFlat { list, depth } => Self::ListFlat {
                list: f(list)?,
                depth: map_opt(depth, f)?,
            },
            Self::ListProjection { op, list } => Self::ListProjection { op, list: f(list)? },
            Self::ListPush { list, item } => Self::ListPush {
                list: f(list)?,
                item: f(item)?,
            },
            Self::ListExtend { list, other } => Self::ListExtend {
                list: f(list)?,
                other: f(other)?,
            },
            Self::ListInsert { list, index, item } => Self::ListInsert {
                list: f(list)?,
                index: f(index)?,
                item: f(item)?,
            },
            Self::ListUnshift { list, items } => Self::ListUnshift {
                list: f(list)?,
                items: map_vec(items, f)?,
            },
            Self::ListReverse { list } => Self::ListReverse { list: f(list)? },
            Self::ListClear { list } => Self::ListClear { list: f(list)? },
            Self::ListCopy { list } => Self::ListCopy { list: f(list)? },
            Self::ListCount { list, item } => Self::ListCount {
                list: f(list)?,
                item: f(item)?,
            },
            Self::ListSum { list } => Self::ListSum { list: f(list)? },
            Self::ListBoolFold { op, list } => Self::ListBoolFold { op, list: f(list)? },
            Self::ListSorted { list, key, reverse } => Self::ListSorted {
                list: f(list)?,
                key: map_opt(key, f)?,
                reverse,
            },
            Self::ListReversed { list } => Self::ListReversed { list: f(list)? },
            Self::ListEnumerate { list } => Self::ListEnumerate { list: f(list)? },
            Self::ListZip { left, right } => Self::ListZip {
                left: f(left)?,
                right: f(right)?,
            },
            Self::ListRange { start, end, step } => Self::ListRange {
                start: f(start)?,
                end: f(end)?,
                step: f(step)?,
            },
            Self::ListRandomChoice { list } => Self::ListRandomChoice { list: f(list)? },
            Self::ListIndex { list, item } => Self::ListIndex {
                list: f(list)?,
                item: f(item)?,
            },
            Self::ListRemove { list, item } => Self::ListRemove {
                list: f(list)?,
                item: f(item)?,
            },
            Self::ListSort {
                list,
                comparator,
                key,
                reverse,
            } => Self::ListSort {
                list: f(list)?,
                comparator: map_opt(comparator, f)?,
                key: map_opt(key, f)?,
                reverse,
            },
            Self::ListPop { list } => Self::ListPop { list: f(list)? },
            Self::ListShift { list } => Self::ListShift { list: f(list)? },
            Self::ListNext { list } => Self::ListNext { list: f(list)? },
            Self::IteratorDone { result } => Self::IteratorDone { result: f(result)? },
            Self::IteratorValue { result } => Self::IteratorValue { result: f(result)? },
            Self::TupleContains { tuple, item } => Self::TupleContains {
                tuple: f(tuple)?,
                item: f(item)?,
            },
            Self::DictContainsKey { dict, key } => Self::DictContainsKey {
                dict: f(dict)?,
                key: f(key)?,
            },
            Self::DictSet { dict, key, value } => Self::DictSet {
                dict: f(dict)?,
                key: f(key)?,
                value: f(value)?,
            },
            Self::DictRemoveKey { dict, key } => Self::DictRemoveKey {
                dict: f(dict)?,
                key: f(key)?,
            },
            Self::DictGet { dict, key, default } => Self::DictGet {
                dict: f(dict)?,
                key: f(key)?,
                default: map_opt(default, f)?,
            },
            Self::DictSetDefault { dict, key, default } => Self::DictSetDefault {
                dict: f(dict)?,
                key: f(key)?,
                default: f(default)?,
            },
            Self::DictClear { dict } => Self::DictClear { dict: f(dict)? },
            Self::DictPop { dict, key, default } => Self::DictPop {
                dict: f(dict)?,
                key: f(key)?,
                default: map_opt(default, f)?,
            },
            Self::DictUpdate { dict, other } => Self::DictUpdate {
                dict: f(dict)?,
                other: f(other)?,
            },
            Self::DictAssign { target, sources } => Self::DictAssign {
                target: f(target)?,
                sources: map_vec(sources, f)?,
            },
            Self::CallableObjectAssign {
                callable,
                props,
                spreads,
            } => Self::CallableObjectAssign {
                callable: f(callable)?,
                props: props
                    .into_iter()
                    .map(|(name, value)| Ok((name, f(value)?)))
                    .collect::<Result<Vec<_>, E>>()?,
                spreads: map_vec(spreads, f)?,
            },
            Self::DictCopy { dict } => Self::DictCopy { dict: f(dict)? },
            Self::DictProjection { op, dict } => Self::DictProjection { op, dict: f(dict)? },
            Self::StringSplit {
                haystack,
                separator,
                limit,
            } => Self::StringSplit {
                haystack: f(haystack)?,
                separator: f(separator)?,
                limit: map_opt(limit, f)?,
            },
            Self::StringChars { haystack } => Self::StringChars {
                haystack: f(haystack)?,
            },
            Self::StringJoin { items, separator } => Self::StringJoin {
                items: f(items)?,
                separator: f(separator)?,
            },
            Self::JsonStringify { value } => Self::JsonStringify { value: f(value)? },
            Self::JsonParse { text } => Self::JsonParse { text: f(text)? },
            Self::HttpGetText { url } => Self::HttpGetText { url: f(url)? },
            Self::DateSetNow { timestamp } => Self::DateSetNow {
                timestamp: f(timestamp)?,
            },
            Self::DateSetTimezoneOffset { offset } => Self::DateSetTimezoneOffset {
                offset: f(offset)?,
            },
            Self::VitestMockFn { implementation } => Self::VitestMockFn {
                implementation: map_opt(implementation, f)?,
            },
            Self::VitestMockCalledTimes { mock, count } => Self::VitestMockCalledTimes {
                mock: f(mock)?,
                count: f(count)?,
            },
            Self::VitestMockCalledWith { mock, args, last } => Self::VitestMockCalledWith {
                mock: f(mock)?,
                args: map_vec(args, f)?,
                last,
            },
            Self::VitestMockLastResolvedWith { mock, expected } => {
                Self::VitestMockLastResolvedWith {
                    mock: f(mock)?,
                    expected: f(expected)?,
                }
            }
            Self::DateTimezoneContext { timezone } => Self::DateTimezoneContext {
                timezone: f(timezone)?,
            },
            Self::DateToIsoString { timestamp_ms } => Self::DateToIsoString {
                timestamp_ms: f(timestamp_ms)?,
            },
            Self::DateToString { timestamp_ms } => Self::DateToString {
                timestamp_ms: f(timestamp_ms)?,
            },
            Self::DateFromParts { parts } => Self::DateFromParts {
                parts: map_vec(parts, f)?,
            },
            Self::DateFromValue { value } => Self::DateFromValue { value: f(value)? },
            Self::DateGetPart { part, timestamp_ms } => Self::DateGetPart {
                part,
                timestamp_ms: f(timestamp_ms)?,
            },
            Self::DateSetPart {
                part,
                timestamp_ms,
                values,
            } => Self::DateSetPart {
                part,
                timestamp_ms: f(timestamp_ms)?,
                values: map_vec(values, f)?,
            },
            Self::UrlField { field, url } => Self::UrlField { field, url: f(url)? },
            Self::FileReadText { path } => Self::FileReadText { path: f(path)? },
            Self::FileWriteText { path, text } => Self::FileWriteText {
                path: f(path)?,
                text: f(text)?,
            },
            Self::BlobFromParts {
                parts,
                blob_type,
                name,
                last_modified,
            } => Self::BlobFromParts {
                parts: f(parts)?,
                blob_type: f(blob_type)?,
                name: map_opt(name, f)?,
                last_modified: map_opt(last_modified, f)?,
            },
            Self::HostGlobalRead { class } => Self::HostGlobalRead { class },
            Self::HostGlobalWrite { class, value } => Self::HostGlobalWrite {
                class,
                value: f(value)?,
            },
            Self::HostGlobalPresent { class } => Self::HostGlobalPresent { class },
            Self::BinOp { op, lhs, rhs } => Self::BinOp {
                op,
                lhs: f(lhs)?,
                rhs: f(rhs)?,
            },
            Self::UnaryOp { op, operand } => Self::UnaryOp {
                op,
                operand: f(operand)?,
            },
            Self::Conditional {
                cond,
                then_expr,
                else_expr,
            } => Self::Conditional {
                cond: f(cond)?,
                then_expr: f(then_expr)?,
                else_expr: f(else_expr)?,
            },
            Self::FunctionTableLookup { key, cases } => Self::FunctionTableLookup {
                key: f(key)?,
                cases: cases
                    .into_iter()
                    .map(|(name, value)| Ok((name, f(value)?)))
                    .collect::<Result<Vec<_>, E>>()?,
            },
            Self::InstanceOf { value, class } => Self::InstanceOf {
                value: f(value)?,
                class,
            },
            Self::UnknownIs { value, kind } => Self::UnknownIs {
                value: f(value)?,
                kind,
            },
            Self::TypeofValue { value } => Self::TypeofValue { value: f(value)? },
            Self::PrototypeSentinel { value } => Self::PrototypeSentinel { value: f(value)? },
            Self::UnknownCast { value, target } => Self::UnknownCast {
                value: f(value)?,
                target,
            },
            Self::ListLit(items) => Self::ListLit(map_vec(items, f)?),
            Self::SetLit(items) => Self::SetLit(map_vec(items, f)?),
            Self::ListToSet { list } => Self::ListToSet { list: f(list)? },
            Self::ListPairsToDict { list } => Self::ListPairsToDict { list: f(list)? },
            Self::DictLit(entries) => Self::DictLit(
                entries
                    .into_iter()
                    .map(|(key, value)| Ok((f(key)?, f(value)?)))
                    .collect::<Result<Vec<_>, E>>()?,
            ),
            Self::TupleLit(items) => Self::TupleLit(map_vec(items, f)?),
            Self::TupleToList { tuple } => Self::TupleToList { tuple: f(tuple)? },
            Self::ListToTuple { list } => Self::ListToTuple { list: f(list)? },
            Self::TupleToSet { tuple } => Self::TupleToSet { tuple: f(tuple)? },
            Self::TupleIndex { tuple, index } => Self::TupleIndex {
                tuple: f(tuple)?,
                index,
            },
            Self::TupleSlice { tuple, start, end } => Self::TupleSlice {
                tuple: f(tuple)?,
                start,
                end,
            },
            Self::New { class, args } => Self::New {
                class,
                args: map_vec(args, f)?,
            },
            Self::Await(value) => Self::Await(f(value)?),
            Self::AsyncOp { op, args } => Self::AsyncOp {
                op,
                args: map_vec(args, f)?,
            },
        })
    }
}
