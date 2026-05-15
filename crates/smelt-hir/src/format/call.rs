//! Expression formatting helpers, including call-like nodes.

use crate::expr::{AsyncOp, Expr, ExprKind};
use crate::ids::ExprId;
use crate::krate::Crate;

use super::{
    collection_text, dict_lit_text, expr_list_text, expr_ref, item_ref, literal_text, local_ref,
    optional_expr_ref, type_ref,
};

/// Formats an expression as text.
pub(super) fn expr_text(krate: &Crate, expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Literal(literal) => literal_text(literal),
        ExprKind::Local(local) => local_ref(*local),
        ExprKind::Item(item) => item_ref(krate, *item),
        ExprKind::Call { .. }
        | ExprKind::ClosureCall { .. }
        | ExprKind::Method { .. }
        | ExprKind::New { .. } => call_like_expr_text(krate, expr),
        ExprKind::Field { receiver, field } => {
            let field_name = krate.symbols.get(*field).unwrap_or("<unknown>");
            format!("{}.{}", expr_ref(*receiver), field_name)
        }
        ExprKind::OptionalField { receiver, field } => {
            let field_name = krate.symbols.get(*field).unwrap_or("<unknown>");
            format!("{}?.{}", expr_ref(*receiver), field_name)
        }
        ExprKind::Index { receiver, index } => {
            format!("{}[{}]", expr_ref(*receiver), expr_ref(*index))
        }
        ExprKind::OptionalIndex { receiver, index } => {
            format!("{}?.[{}]", expr_ref(*receiver), expr_ref(*index))
        }
        ExprKind::OptionalMethod {
            receiver,
            method,
            args,
        } => {
            let method_name = krate.symbols.get(*method).unwrap_or("<unknown>");
            format!(
                "{}?.{}({})",
                expr_ref(*receiver),
                method_name,
                expr_list_text(args)
            )
        }
        ExprKind::OptionalCoalesce { optional, fallback } => {
            format!("{} ?? {}", expr_ref(*optional), expr_ref(*fallback))
        }
        ExprKind::TypeAssert { value } => format!("assert_type {}", expr_ref(*value)),
        ExprKind::Len { operand } => format!("len {}", expr_ref(*operand)),
        ExprKind::NumericAbs { operand } => format!("numeric_abs {}", expr_ref(*operand)),
        ExprKind::NumericRound { op, operand } => {
            let op_name = match op {
                crate::expr::NumericRoundOp::Floor => "floor",
                crate::expr::NumericRoundOp::Ceil => "ceil",
                crate::expr::NumericRoundOp::Round => "round",
                crate::expr::NumericRoundOp::Trunc => "trunc",
            };
            format!("numeric_{op_name} {}", expr_ref(*operand))
        }
        ExprKind::NumericExtrema { op, args } => {
            let op_name = match op {
                crate::expr::NumericExtremaOp::Min => "min",
                crate::expr::NumericExtremaOp::Max => "max",
            };
            format!("numeric_{op_name} {}", expr_list_text(args))
        }
        ExprKind::NumericHypot { args } => format!("numeric_hypot {}", expr_list_text(args)),
        ExprKind::NumericPredicate { op, operand } => {
            let op_name = match op {
                crate::expr::NumericPredicateOp::IsFinite => "is_finite",
                crate::expr::NumericPredicateOp::IsInteger => "is_integer",
                crate::expr::NumericPredicateOp::IsNaN => "is_nan",
            };
            format!("numeric_{op_name} {}", expr_ref(*operand))
        }
        ExprKind::NumericUnaryFunc { op, operand } => {
            let op_name = match op {
                crate::expr::NumericUnaryFuncOp::Sqrt => "sqrt",
                crate::expr::NumericUnaryFuncOp::Cbrt => "cbrt",
                crate::expr::NumericUnaryFuncOp::Sign => "sign",
                crate::expr::NumericUnaryFuncOp::Sin => "sin",
                crate::expr::NumericUnaryFuncOp::Cos => "cos",
                crate::expr::NumericUnaryFuncOp::Tan => "tan",
                crate::expr::NumericUnaryFuncOp::Asin => "asin",
                crate::expr::NumericUnaryFuncOp::Acos => "acos",
                crate::expr::NumericUnaryFuncOp::Atan => "atan",
                crate::expr::NumericUnaryFuncOp::Log => "log",
                crate::expr::NumericUnaryFuncOp::Log10 => "log10",
                crate::expr::NumericUnaryFuncOp::Log2 => "log2",
                crate::expr::NumericUnaryFuncOp::Exp => "exp",
            };
            format!("numeric_{op_name} {}", expr_ref(*operand))
        }
        ExprKind::NumericPow { base, exponent } => {
            format!("numeric_pow {}, {}", expr_ref(*base), expr_ref(*exponent))
        }
        ExprKind::NumericAtan2 { y, x } => {
            format!("numeric_atan2 {}, {}", expr_ref(*y), expr_ref(*x))
        }
        ExprKind::NumericRandom => "numeric_random".to_owned(),
        ExprKind::NumericRandomInt { start, end } => {
            format!(
                "numeric_random_int {}, {}",
                expr_ref(*start),
                expr_ref(*end)
            )
        }
        ExprKind::NumericToStringRadix { operand, radix } => {
            format!(
                "numeric_to_string_radix {}, {}",
                expr_ref(*operand),
                expr_ref(*radix)
            )
        }
        ExprKind::PrimitiveCast { op, operand } => {
            let op_name = match op {
                crate::expr::PrimitiveCastOp::ToBool => "bool",
                crate::expr::PrimitiveCastOp::ToInt => "int",
                crate::expr::PrimitiveCastOp::ToFloat => "float",
                crate::expr::PrimitiveCastOp::ToString => "string",
            };
            format!("primitive_cast_{op_name} {}", expr_ref(*operand))
        }
        ExprKind::StringCase { op, operand } => {
            let op_name = match op {
                crate::expr::StringCaseOp::Lower => "lower",
                crate::expr::StringCaseOp::Upper => "upper",
            };
            format!("string_{op_name} {}", expr_ref(*operand))
        }
        ExprKind::StringTrim { side, operand } => {
            let side_name = match side {
                crate::expr::StringTrimSide::Both => "both",
                crate::expr::StringTrimSide::Start => "start",
                crate::expr::StringTrimSide::End => "end",
            };
            format!("string_trim_{side_name} {}", expr_ref(*operand))
        }
        ExprKind::StringAffix {
            op,
            haystack,
            needle,
        } => {
            let op_name = match op {
                crate::expr::StringAffixOp::StartsWith => "starts_with",
                crate::expr::StringAffixOp::EndsWith => "ends_with",
            };
            format!(
                "string_{op_name} {}, {}",
                expr_ref(*haystack),
                expr_ref(*needle)
            )
        }
        ExprKind::StringSearch {
            op,
            haystack,
            needle,
        } => {
            let op_name = match op {
                crate::expr::StringSearchOp::Find => "find",
                crate::expr::StringSearchOp::RFind => "rfind",
            };
            format!(
                "string_{op_name} {}, {}",
                expr_ref(*haystack),
                expr_ref(*needle)
            )
        }
        ExprKind::StringReplace {
            op,
            haystack,
            pattern,
            replacement,
        } => {
            let op_name = match op {
                crate::expr::StringReplaceOp::First => "replace_first",
                crate::expr::StringReplaceOp::All => "replace_all",
            };
            format!(
                "string_{op_name} {}, {}, {}",
                expr_ref(*haystack),
                expr_ref(*pattern),
                expr_ref(*replacement)
            )
        }
        ExprKind::StringRemoveAffix {
            op,
            haystack,
            affix,
        } => {
            let op_name = match op {
                crate::expr::StringAffixOp::StartsWith => "remove_prefix",
                crate::expr::StringAffixOp::EndsWith => "remove_suffix",
            };
            format!(
                "string_{op_name} {}, {}",
                expr_ref(*haystack),
                expr_ref(*affix)
            )
        }
        ExprKind::StringRepeat { operand, count } => {
            format!("string_repeat {}, {}", expr_ref(*operand), expr_ref(*count))
        }
        ExprKind::StringPad {
            op,
            operand,
            target_len,
            pad,
        } => {
            let op_text = match op {
                crate::expr::StringPadOp::Start => "pad_start",
                crate::expr::StringPadOp::End => "pad_end",
            };
            format!(
                "string_{op_text} {}, {}, {}",
                expr_ref(*operand),
                expr_ref(*target_len),
                expr_ref(*pad)
            )
        }
        ExprKind::StringPredicate { op, operand } => {
            let op_name = match op {
                crate::expr::StringPredicateOp::IsDigit => "isdigit",
                crate::expr::StringPredicateOp::IsAlpha => "isalpha",
                crate::expr::StringPredicateOp::IsAlnum => "isalnum",
            };
            format!("string_{op_name} {}", expr_ref(*operand))
        }
        ExprKind::RegexIsMatch {
            op,
            pattern,
            haystack,
        } => {
            let op_name = match op {
                crate::expr::RegexMatchOp::Search => "search",
                crate::expr::RegexMatchOp::Match => "match",
                crate::expr::RegexMatchOp::FullMatch => "fullmatch",
            };
            format!(
                "regex_{op_name} {}, {}",
                expr_ref(*pattern),
                expr_ref(*haystack)
            )
        }
        ExprKind::RegexReplace {
            op,
            pattern,
            haystack,
            replacement,
        } => {
            let op_name = match op {
                crate::expr::StringReplaceOp::First => "replace_first",
                crate::expr::StringReplaceOp::All => "replace_all",
            };
            format!(
                "regex_{op_name} {}, {}, {}",
                expr_ref(*pattern),
                expr_ref(*haystack),
                expr_ref(*replacement)
            )
        }
        ExprKind::RegexSplit { pattern, haystack } => {
            format!(
                "regex_split {}, {}",
                expr_ref(*pattern),
                expr_ref(*haystack)
            )
        }
        ExprKind::RegexFind { pattern, haystack } => {
            format!("regex_find {}, {}", expr_ref(*pattern), expr_ref(*haystack))
        }
        ExprKind::StringCharAt { operand, index } => {
            format!(
                "string_char_at {}, {}",
                expr_ref(*operand),
                expr_ref(*index)
            )
        }
        ExprKind::StringCharCodeAt { operand, index } => {
            format!(
                "string_char_code_at {}, {}",
                expr_ref(*operand),
                expr_ref(*index)
            )
        }
        ExprKind::StringContains { haystack, needle } => {
            format!(
                "string_contains {}, {}",
                expr_ref(*haystack),
                expr_ref(*needle)
            )
        }
        ExprKind::StringSlice {
            operand,
            start,
            end,
        } => format!(
            "string_slice {}, {}, {}",
            expr_ref(*operand),
            optional_expr_ref(*start),
            optional_expr_ref(*end)
        ),
        ExprKind::ListContains { list, item } => {
            format!("list_contains {}, {}", expr_ref(*list), expr_ref(*item))
        }
        ExprKind::SetContains { set, item } => {
            format!("set_contains {}, {}", expr_ref(*set), expr_ref(*item))
        }
        ExprKind::SetDisjoint { left, right } => {
            format!("set_isdisjoint {}, {}", expr_ref(*left), expr_ref(*right))
        }
        ExprKind::SetRelation { op, left, right } => {
            let op_text = match op {
                crate::expr::SetRelationOp::IsSubset => "issubset",
                crate::expr::SetRelationOp::IsSuperset => "issuperset",
            };
            format!("set_{op_text} {}, {}", expr_ref(*left), expr_ref(*right))
        }
        ExprKind::SetAdd { set, item } => {
            format!("set_add {}, {}", expr_ref(*set), expr_ref(*item))
        }
        ExprKind::SetRemove { op, set, item } => {
            let op_name = match op {
                crate::expr::SetRemoveOp::Delete => "delete",
                crate::expr::SetRemoveOp::Discard => "discard",
                crate::expr::SetRemoveOp::Remove => "remove",
            };
            format!("set_{op_name} {}, {}", expr_ref(*set), expr_ref(*item))
        }
        ExprKind::SetClear { set } => format!("set_clear {}", expr_ref(*set)),
        ExprKind::SetCopy { set } => format!("set_copy {}", expr_ref(*set)),
        ExprKind::ListToSet { list } => format!("list_to_set {}", expr_ref(*list)),
        ExprKind::ListPairsToDict { list } => {
            format!("list_pairs_to_dict {}", expr_ref(*list))
        }
        ExprKind::SetBinary { op, left, right } => {
            let op_name = match op {
                crate::expr::SetBinaryOp::Union => "union",
                crate::expr::SetBinaryOp::Intersection => "intersection",
                crate::expr::SetBinaryOp::Difference => "difference",
                crate::expr::SetBinaryOp::SymmetricDifference => "symmetric_difference",
            };
            format!("set_{op_name} {}, {}", expr_ref(*left), expr_ref(*right))
        }
        ExprKind::SetProjection { op, set } => {
            let op_name = match op {
                crate::expr::SetProjectionOp::Values => "values",
                crate::expr::SetProjectionOp::Entries => "entries",
            };
            format!("set_{op_name} {}", expr_ref(*set))
        }
        ExprKind::ListConcat { left, right } => {
            format!("list_concat {}, {}", expr_ref(*left), expr_ref(*right))
        }
        ExprKind::ListSearch { op, list, item } => {
            let op_name = match op {
                crate::expr::ListSearchOp::Find => "find",
                crate::expr::ListSearchOp::RFind => "rfind",
            };
            format!("list_{op_name} {}, {}", expr_ref(*list), expr_ref(*item))
        }
        ExprKind::ListCallback { op, list, callback } => {
            let op_name = match op {
                crate::expr::ListCallbackOp::Map => "map",
                crate::expr::ListCallbackOp::Filter => "filter",
                crate::expr::ListCallbackOp::Find => "find_callback",
                crate::expr::ListCallbackOp::FindIndex => "find_index",
                crate::expr::ListCallbackOp::FindLast => "find_last_callback",
                crate::expr::ListCallbackOp::FindLastIndex => "find_last_index",
                crate::expr::ListCallbackOp::Some => "some",
                crate::expr::ListCallbackOp::Every => "every",
                crate::expr::ListCallbackOp::ForEach => "for_each",
                crate::expr::ListCallbackOp::FlatMap => "flat_map",
            };
            format!(
                "list_{op_name} {}, {}",
                expr_ref(*list),
                expr_ref(*callback)
            )
        }
        ExprKind::ListFromLength { length } => {
            format!("list_from_length {}", expr_ref(*length))
        }
        ExprKind::ListFromLengthMap { length, callback } => format!(
            "list_from_length_map {}, {}",
            expr_ref(*length),
            expr_ref(*callback)
        ),
        ExprKind::ListReduce {
            list,
            initial,
            callback,
        } => {
            format!(
                "list_reduce {}, {}, {}",
                expr_ref(*list),
                initial.map(expr_ref).unwrap_or_else(|| "_".to_owned()),
                expr_ref(*callback)
            )
        }
        ExprKind::ListSlice { list, start, end } => format!(
            "list_slice {}, {}, {}",
            expr_ref(*list),
            optional_expr_ref(*start),
            optional_expr_ref(*end)
        ),
        ExprKind::ListSplice {
            list,
            start,
            delete_count,
            items,
            mutate,
        } => format!(
            "list_splice {}, {}, {}, [{}], {}",
            expr_ref(*list),
            expr_ref(*start),
            optional_expr_ref(*delete_count),
            items
                .iter()
                .map(|item| {
                    if item.spread {
                        format!("...{}", expr_ref(item.value))
                    } else {
                        expr_ref(item.value)
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
            mutate
        ),
        ExprKind::ListFill {
            list,
            value,
            start,
            end,
        } => format!(
            "list_fill {}, {}, {}, {}",
            expr_ref(*list),
            expr_ref(*value),
            optional_expr_ref(*start),
            optional_expr_ref(*end)
        ),
        ExprKind::ListCopyWithin {
            list,
            target,
            start,
            end,
        } => format!(
            "list_copy_within {}, {}, {}, {}",
            expr_ref(*list),
            expr_ref(*target),
            expr_ref(*start),
            optional_expr_ref(*end)
        ),
        ExprKind::ListWith { list, index, value } => format!(
            "list_with {}, {}, {}",
            expr_ref(*list),
            expr_ref(*index),
            expr_ref(*value)
        ),
        ExprKind::ListFlat { list } => format!("list_flat {}", expr_ref(*list)),
        ExprKind::ListProjection { op, list } => {
            let op_name = match op {
                crate::expr::ListProjectionOp::Keys => "keys",
                crate::expr::ListProjectionOp::Values => "values",
                crate::expr::ListProjectionOp::Entries => "entries",
            };
            format!("list_{op_name} {}", expr_ref(*list))
        }
        ExprKind::ListPush { list, item } => {
            format!("list_push {}, {}", expr_ref(*list), expr_ref(*item))
        }
        ExprKind::ListExtend { list, other } => {
            format!("list_extend {}, {}", expr_ref(*list), expr_ref(*other))
        }
        ExprKind::ListInsert { list, index, item } => format!(
            "list_insert {}, {}, {}",
            expr_ref(*list),
            expr_ref(*index),
            expr_ref(*item)
        ),
        ExprKind::ListUnshift { list, items } => format!(
            "list_unshift {} [{}]",
            expr_ref(*list),
            items
                .iter()
                .map(|item| expr_ref(*item))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::ListReverse { list } => format!("list_reverse {}", expr_ref(*list)),
        ExprKind::ListClear { list } => format!("list_clear {}", expr_ref(*list)),
        ExprKind::ListCopy { list } => format!("list_copy {}", expr_ref(*list)),
        ExprKind::TupleToList { tuple } => format!("tuple_to_list {}", expr_ref(*tuple)),
        ExprKind::ListToTuple { list } => format!("list_to_tuple {}", expr_ref(*list)),
        ExprKind::TupleToSet { tuple } => format!("tuple_to_set {}", expr_ref(*tuple)),
        ExprKind::ListCount { list, item } => {
            format!("list_count {}, {}", expr_ref(*list), expr_ref(*item))
        }
        ExprKind::ListSum { list } => format!("list_sum {}", expr_ref(*list)),
        ExprKind::ListBoolFold { op, list } => {
            let op_text = match op {
                crate::expr::BoolFoldOp::All => "all",
                crate::expr::BoolFoldOp::Any => "any",
            };
            format!("list_{op_text} {}", expr_ref(*list))
        }
        ExprKind::ListSorted { list } => format!("list_sorted {}", expr_ref(*list)),
        ExprKind::ListReversed { list } => format!("list_reversed {}", expr_ref(*list)),
        ExprKind::ListEnumerate { list } => format!("list_enumerate {}", expr_ref(*list)),
        ExprKind::ListZip { left, right } => {
            format!("list_zip {}, {}", expr_ref(*left), expr_ref(*right))
        }
        ExprKind::ListRange { start, end, step } => format!(
            "list_range {}, {}, {}",
            expr_ref(*start),
            expr_ref(*end),
            expr_ref(*step)
        ),
        ExprKind::ListRandomChoice { list } => {
            format!("list_random_choice {}", expr_ref(*list))
        }
        ExprKind::ListIndex { list, item } => {
            format!("list_index {}, {}", expr_ref(*list), expr_ref(*item))
        }
        ExprKind::ListRemove { list, item } => {
            format!("list_remove {}, {}", expr_ref(*list), expr_ref(*item))
        }
        ExprKind::ListSort {
            list, comparator, ..
        } => {
            let comparator_text = if comparator.is_some() {
                " <comparator>"
            } else {
                ""
            };
            format!("list_sort {}{}", expr_ref(*list), comparator_text)
        }
        ExprKind::ListPop { list } => format!("list_pop {}", expr_ref(*list)),
        ExprKind::ListShift { list } => format!("list_shift {}", expr_ref(*list)),
        ExprKind::TupleContains { tuple, item } => {
            format!("tuple_contains {}, {}", expr_ref(*tuple), expr_ref(*item))
        }
        ExprKind::DictContainsKey { dict, key } => {
            format!("dict_contains_key {}, {}", expr_ref(*dict), expr_ref(*key))
        }
        ExprKind::DictSet { dict, key, value } => {
            format!(
                "dict_set {}, {}, {}",
                expr_ref(*dict),
                expr_ref(*key),
                expr_ref(*value)
            )
        }
        ExprKind::DictRemoveKey { dict, key } => {
            format!("dict_remove_key {}, {}", expr_ref(*dict), expr_ref(*key))
        }
        ExprKind::DictGet { dict, key, default } => format!(
            "dict_get {}, {}, {}",
            expr_ref(*dict),
            expr_ref(*key),
            optional_expr_ref(*default)
        ),
        ExprKind::DictSetDefault { dict, key, default } => format!(
            "dict_setdefault {}, {}, {}",
            expr_ref(*dict),
            expr_ref(*key),
            expr_ref(*default)
        ),
        ExprKind::DictClear { dict } => format!("dict_clear {}", expr_ref(*dict)),
        ExprKind::DictPop { dict, key, default } => format!(
            "dict_pop {}, {}, {}",
            expr_ref(*dict),
            expr_ref(*key),
            optional_expr_ref(*default)
        ),
        ExprKind::DictUpdate { dict, other } => {
            format!("dict_update {}, {}", expr_ref(*dict), expr_ref(*other))
        }
        ExprKind::DictAssign { target, sources } => format!(
            "dict_assign {}, [{}]",
            expr_ref(*target),
            sources
                .iter()
                .map(|source| expr_ref(*source))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::CallableObjectAssign { callable, props } => format!(
            "callable_object_assign {}, {{{}}}",
            expr_ref(*callable),
            props
                .iter()
                .map(|(name, value)| {
                    let name = krate.symbols.get(*name).unwrap_or("<unknown>");
                    format!("{name}: {}", expr_ref(*value))
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::DictCopy { dict } => format!("dict_copy {}", expr_ref(*dict)),
        ExprKind::DictProjection { op, dict } => {
            let op_name = match op {
                crate::expr::DictProjectionOp::Keys => "keys",
                crate::expr::DictProjectionOp::Values => "values",
                crate::expr::DictProjectionOp::Entries => "entries",
            };
            format!("dict_{op_name} {}", expr_ref(*dict))
        }
        ExprKind::StringSplit {
            haystack,
            separator,
            limit,
        } => {
            format!(
                "string_split {}, {}, {}",
                expr_ref(*haystack),
                expr_ref(*separator),
                optional_expr_ref(*limit)
            )
        }
        ExprKind::StringChars { haystack } => {
            format!("string_chars {}", expr_ref(*haystack))
        }
        ExprKind::StringJoin { items, separator } => {
            format!("string_join {}, {}", expr_ref(*items), expr_ref(*separator))
        }
        ExprKind::JsonStringify { value } => format!("json_stringify {}", expr_ref(*value)),
        ExprKind::JsonParse { text } => format!("json_parse {}", expr_ref(*text)),
        ExprKind::HttpGetText { url } => format!("http_get_text {}", expr_ref(*url)),
        ExprKind::DateNow => "date_now".to_owned(),
        ExprKind::DateToIsoString { timestamp_ms } => {
            format!("date_to_iso {}", expr_ref(*timestamp_ms))
        }
        ExprKind::DateFromParts { parts } => format!(
            "date_from_parts [{}]",
            parts
                .iter()
                .map(|part| expr_ref(*part))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::DateGetPart { part, timestamp_ms } => {
            format!("date_get_{part:?} {}", expr_ref(*timestamp_ms))
        }
        ExprKind::DateSetPart {
            part,
            timestamp_ms,
            values,
        } => format!(
            "date_set_{part:?} {}, [{}]",
            expr_ref(*timestamp_ms),
            values
                .iter()
                .map(|value| expr_ref(*value))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::UrlField { field, url } => format!("url_{field:?} {}", expr_ref(*url)),
        ExprKind::FileReadText { path } => format!("file_read_text {}", expr_ref(*path)),
        ExprKind::FileWriteText { path, text } => {
            format!("file_write_text {}, {}", expr_ref(*path), expr_ref(*text))
        }
        ExprKind::BinOp { op, lhs, rhs } => {
            format!("{op:?} {}, {}", expr_ref(*lhs), expr_ref(*rhs))
        }
        ExprKind::UnaryOp { op, operand } => format!("{op:?} {}", expr_ref(*operand)),
        ExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => {
            format!(
                "if {} then {} else {}",
                expr_ref(*cond),
                expr_ref(*then_expr),
                expr_ref(*else_expr)
            )
        }
        ExprKind::InstanceOf { value, class } => {
            format!("instanceof {}, class{}", expr_ref(*value), class.0)
        }
        ExprKind::UnknownIs { value, kind } => {
            format!("unknown_is {kind:?} {}", expr_ref(*value))
        }
        ExprKind::UnknownCast { value, target } => {
            format!(
                "unknown_cast {} as {}",
                expr_ref(*value),
                type_ref(krate, *target)
            )
        }
        ExprKind::Block(block) => format!("block {block:?}"),
        ExprKind::Lambda { body, return_ty } => {
            format!("lambda {body:?} -> {}", type_ref(krate, *return_ty))
        }
        ExprKind::Closure(closure) => {
            format!(
                "closure {:?} captures {} -> {}",
                closure.body,
                closure.captures.len(),
                type_ref(krate, closure.return_ty)
            )
        }
        ExprKind::ListLit(items) => collection_text("[", "]", items),
        ExprKind::SetLit(items) => collection_text("set{", "}", items),
        ExprKind::DictLit(items) => dict_lit_text(items),
        ExprKind::TupleLit(items) => collection_text("(", ")", items),
        ExprKind::TupleIndex { tuple, index } => {
            format!("tuple_index {}, {}", expr_ref(*tuple), index)
        }
        ExprKind::TupleSlice { tuple, start, end } => {
            format!("tuple_slice {}, {}, {}", expr_ref(*tuple), start, end)
        }
        ExprKind::Await(await_expr) => format!("await {}", expr_ref(*await_expr)),
        ExprKind::AsyncOp { op, args } => async_op_text(*op, args),
    }
}

/// Formats a runtime-backed async operation.
fn async_op_text(op: AsyncOp, args: &[ExprId]) -> String {
    let op_name = match op {
        AsyncOp::All => "async_all",
        AsyncOp::Race => "async_race",
        AsyncOp::AllSettled => "async_all_settled",
        AsyncOp::Sleep => "async_sleep",
        AsyncOp::CreateTask => "async_create_task",
        AsyncOp::WaitFor => "async_wait_for",
        AsyncOp::HttpGetText => "async_http_get_text",
    };
    let args_text = args
        .iter()
        .map(|arg| expr_ref(*arg))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{op_name}({args_text})")
}

/// Formats call-like expressions as text.
#[expect(
    clippy::wildcard_enum_match_arm,
    reason = "Fallback arm preserves previous invalid-call behavior while keeping formatter split concise"
)]
fn call_like_expr_text(krate: &Crate, expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            let arg_text = expr_list_text(args);
            format!("call {}({arg_text})", expr_ref(*callee))
        }
        ExprKind::ClosureCall { callee, args } => {
            let arg_text = expr_list_text(args);
            format!("closure_call {}({arg_text})", expr_ref(*callee))
        }
        ExprKind::Method {
            receiver,
            method,
            args,
        } => {
            let method_name = krate.symbols.get(*method).unwrap_or("<unknown>");
            let arg_text = expr_list_text(args);
            format!("{}.{}({arg_text})", expr_ref(*receiver), method_name)
        }
        ExprKind::New { class, args } => {
            let class_name = krate.symbols.get(*class).unwrap_or("<unknown>");
            let arg_text = expr_list_text(args);
            format!("new {class_name}({arg_text})")
        }
        _ => "invalid call".to_owned(),
    }
}
