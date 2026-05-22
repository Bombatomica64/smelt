//! Pretty-printing utilities for MIR structures.
//!
//! This module provides functions to format MIR in a human-readable compact format,
//! suitable for debugging and testing.

use smelt_hir::{Type, TypeId};

use crate::{
    BuiltinFn, Callee, Constant, LocalId, LocalKind, Mir, Operand, Place, Rvalue, Statement,
    Terminator,
};

/// Formats a MIR module into a human-readable compact string representation.
///
/// # Panics
///
/// Panics if a local index does not fit in `u32`.
#[must_use]
pub fn format_compact(mir: &Mir) -> String {
    let mut out = String::new();
    for class in &mir.classes {
        let name = mir.symbols.get(class.name).unwrap_or("<unknown>");
        let fields = class
            .fields
            .iter()
            .map(|field| {
                format!(
                    "{:?} {}: {}",
                    field.visibility,
                    mir.symbols.get(field.name).unwrap_or("<unknown>"),
                    type_ref(mir, field.ty)
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "class {name} fields [{fields}] constructor {:?} methods {:?}\n",
            class.constructor, class.methods
        ));
    }
    for interface in &mir.interfaces {
        let name = mir.symbols.get(interface.name).unwrap_or("<unknown>");
        out.push_str(&format!("interface {name}\n"));
    }
    if !mir.classes.is_empty() || !mir.interfaces.is_empty() {
        out.push('\n');
    }
    for function in &mir.functions {
        let name = mir.symbols.get(function.name).unwrap_or("<unknown>");
        let async_prefix = if function.is_async { "async " } else { "" };
        out.push_str(&format!(
            "{async_prefix}fn {name} ({:?}) -> {}{}\n",
            function.id,
            type_ref(mir, function.return_ty),
            if function.can_throw { " throws" } else { "" }
        ));

        if !function.locals.is_empty() {
            out.push_str("  locals\n");
            for (idx, local) in function.locals.iter().enumerate() {
                let local_id = index_to_u32(idx, "MIR local index");
                out.push_str(&format!(
                    "    {} {}: {}\n",
                    local_ref(LocalId(local_id)),
                    local_kind_text(mir, local.kind),
                    type_ref(mir, local.ty)
                ));
            }
        }

        for block in &function.blocks {
            out.push_str(&format!("  bb{}:\n", block.id.0));
            for phi in &block.phis {
                out.push_str(&format!(
                    "    {} = phi {}\n",
                    local_ref(phi.dest),
                    type_ref(mir, phi.ty)
                ));
            }
            for statement in &block.statements {
                out.push_str(&format!("    {}\n", statement_text(statement)));
            }
            let terminator = block
                .terminator
                .as_ref()
                .map(terminator_text)
                .unwrap_or_else(|| "<missing terminator>".to_owned());
            out.push_str(&format!("    {terminator}\n"));
        }
        out.push('\n');
    }
    out
}

/// Formats a local kind to human-readable text.
fn local_kind_text(mir: &Mir, kind: LocalKind) -> String {
    match kind {
        LocalKind::Param { symbol } => symbol
            .and_then(|param_symbol| mir.symbols.get(param_symbol))
            .map_or_else(|| "param".to_owned(), |name| format!("param {name}")),
        LocalKind::Temp => "temp".to_owned(),
        LocalKind::UserBinding(symbol) => {
            format!("user {}", mir.symbols.get(symbol).unwrap_or("<unknown>"))
        }
    }
}

/// Formats a statement to human-readable text.
fn statement_text(statement: &Statement) -> String {
    match statement {
        Statement::Assign { dest, value } => {
            format!("{} = {}", local_ref(*dest), rvalue_text(value))
        }
        Statement::AssignPlace { place, value } => {
            format!("{} = {}", place_text(place), rvalue_text(value))
        }
        Statement::StorageLive(local) => format!("StorageLive({})", local_ref(*local)),
        Statement::StorageDead(local) => format!("StorageDead({})", local_ref(*local)),
    }
}

/// Formats an rvalue to human-readable text.
fn rvalue_text(value: &Rvalue) -> String {
    match value {
        Rvalue::Use(operand) => operand_text(operand),
        Rvalue::List(items) => format!(
            "[{}]",
            items
                .iter()
                .map(operand_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Rvalue::Set(items) => format!(
            "set{{{}}}",
            items
                .iter()
                .map(operand_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Rvalue::Dict(entries) => format!(
            "{{{}}}",
            entries
                .iter()
                .map(|(key, entry_value)| {
                    format!("{}: {}", operand_text(key), operand_text(entry_value))
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Rvalue::Tuple(items) => format!(
            "({})",
            items
                .iter()
                .map(operand_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Rvalue::Closure { id, captures } => format!(
            "closure {:?} [{}]",
            id,
            captures
                .iter()
                .map(operand_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Rvalue::ClosureCall { callee, args } => format!(
            "closure_call {}({})",
            operand_text(callee),
            args.iter().map(operand_text).collect::<Vec<_>>().join(", ")
        ),
        Rvalue::Binary { op, lhs, rhs } => {
            format!(
                "{} {} {}",
                operand_text(lhs),
                smelt_hir::bin_op_text(*op),
                operand_text(rhs)
            )
        }
        Rvalue::Unary { op, operand } => {
            let op_text = match op {
                smelt_hir::UnaryOp::Not => "!",
                smelt_hir::UnaryOp::Neg => "-",
            };
            format!("{op_text}{}", operand_text(operand))
        }
        Rvalue::Conditional {
            cond,
            then_operand,
            else_operand,
        } => {
            format!(
                "if {} {{ {} }} else {{ {} }}",
                operand_text(cond),
                operand_text(then_operand),
                operand_text(else_operand)
            )
        }
        Rvalue::OptionalField { receiver, field } => {
            format!("{}?.field{}", operand_text(receiver), field.0)
        }
        Rvalue::OptionalIndex { receiver, index } => {
            format!("{}?.[{}]", operand_text(receiver), operand_text(index))
        }
        Rvalue::OptionalMethod {
            receiver,
            method,
            args,
        } => {
            let args_text = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
            format!(
                "{}?.method{}({args_text})",
                operand_text(receiver),
                method.0
            )
        }
        Rvalue::OptionalCoalesce { optional, fallback } => {
            format!("{} ?? {}", operand_text(optional), operand_text(fallback))
        }
        Rvalue::InstanceOf {
            value: operand,
            class,
        } => {
            format!("instanceof {}, class{}", operand_text(operand), class.0)
        }
        Rvalue::UnknownIs {
            value: unknown_value,
            kind,
        } => {
            format!("unknown_is {kind:?} {}", operand_text(unknown_value))
        }
        Rvalue::UnknownCast {
            value: unknown_value,
            target,
        } => {
            format!(
                "unknown_cast {} as t{}",
                operand_text(unknown_value),
                target.0
            )
        }
        Rvalue::Struct { class, fields } => {
            let field_list = fields
                .iter()
                .map(|(field, field_value)| {
                    format!("field{}: {}", field.0, operand_text(field_value))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("struct{} {{{field_list}}}", class.0)
        }
        Rvalue::ExternalClassInstance { class, args } => {
            let arg_list = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
            format!("external_new{}({arg_list})", class.0)
        }
        Rvalue::Len(operand) => format!("len {}", operand_text(operand)),
        Rvalue::NumericAbs(operand) => format!("numeric_abs {}", operand_text(operand)),
        Rvalue::NumericRound { op, operand } => {
            let op_text = match op {
                smelt_hir::NumericRoundOp::Floor => "floor",
                smelt_hir::NumericRoundOp::Ceil => "ceil",
                smelt_hir::NumericRoundOp::Round => "round",
                smelt_hir::NumericRoundOp::Trunc => "trunc",
            };
            format!("numeric_{op_text} {}", operand_text(operand))
        }
        Rvalue::NumericExtrema { op, args } => {
            let op_text = match op {
                smelt_hir::NumericExtremaOp::Min => "min",
                smelt_hir::NumericExtremaOp::Max => "max",
            };
            let arg_list = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
            format!("numeric_{op_text} {arg_list}")
        }
        Rvalue::NumericHypot { args } => {
            let arg_list = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
            format!("numeric_hypot {arg_list}")
        }
        Rvalue::NumericPredicate { op, operand } => {
            let op_text = match op {
                smelt_hir::NumericPredicateOp::IsFinite => "is_finite",
                smelt_hir::NumericPredicateOp::IsInteger => "is_integer",
                smelt_hir::NumericPredicateOp::IsNaN => "is_nan",
            };
            format!("numeric_{op_text} {}", operand_text(operand))
        }
        Rvalue::NumericUnaryFunc { op, operand } => {
            let op_text = match op {
                smelt_hir::NumericUnaryFuncOp::Sqrt => "sqrt",
                smelt_hir::NumericUnaryFuncOp::Cbrt => "cbrt",
                smelt_hir::NumericUnaryFuncOp::Sign => "sign",
                smelt_hir::NumericUnaryFuncOp::Sin => "sin",
                smelt_hir::NumericUnaryFuncOp::Cos => "cos",
                smelt_hir::NumericUnaryFuncOp::Tan => "tan",
                smelt_hir::NumericUnaryFuncOp::Asin => "asin",
                smelt_hir::NumericUnaryFuncOp::Acos => "acos",
                smelt_hir::NumericUnaryFuncOp::Atan => "atan",
                smelt_hir::NumericUnaryFuncOp::Log => "log",
                smelt_hir::NumericUnaryFuncOp::Log10 => "log10",
                smelt_hir::NumericUnaryFuncOp::Log2 => "log2",
                smelt_hir::NumericUnaryFuncOp::Exp => "exp",
            };
            format!("numeric_{op_text} {}", operand_text(operand))
        }
        Rvalue::NumericPow { base, exponent } => {
            format!(
                "numeric_pow {}, {}",
                operand_text(base),
                operand_text(exponent)
            )
        }
        Rvalue::NumericAtan2 { y, x } => {
            format!("numeric_atan2 {}, {}", operand_text(y), operand_text(x))
        }
        Rvalue::NumericRandom => "numeric_random".to_owned(),
        Rvalue::NumericRandomInt { start, end } => {
            format!(
                "numeric_random_int {}, {}",
                operand_text(start),
                operand_text(end)
            )
        }
        Rvalue::NumericToStringRadix { operand, radix } => {
            format!(
                "numeric_to_string_radix {}, {}",
                operand_text(operand),
                operand_text(radix)
            )
        }
        Rvalue::PrimitiveCast { op, operand } => {
            let op_text = match op {
                smelt_hir::PrimitiveCastOp::ToBool => "bool",
                smelt_hir::PrimitiveCastOp::ToInt => "int",
                smelt_hir::PrimitiveCastOp::ToFloat => "float",
                smelt_hir::PrimitiveCastOp::ToString => "string",
            };
            format!("primitive_cast_{op_text} {}", operand_text(operand))
        }
        Rvalue::StringCase { op, operand } => {
            let op_text = match op {
                smelt_hir::StringCaseOp::Lower => "lower",
                smelt_hir::StringCaseOp::Upper => "upper",
            };
            format!("string_{op_text} {}", operand_text(operand))
        }
        Rvalue::StringNormalize { form, operand } => {
            let form_text = match form {
                smelt_hir::StringNormalizeForm::Nfc => "nfc",
                smelt_hir::StringNormalizeForm::Nfd => "nfd",
                smelt_hir::StringNormalizeForm::Nfkc => "nfkc",
                smelt_hir::StringNormalizeForm::Nfkd => "nfkd",
            };
            format!("string_normalize_{form_text} {}", operand_text(operand))
        }
        Rvalue::StringTrim { side, operand } => {
            let side_text = match side {
                smelt_hir::StringTrimSide::Both => "both",
                smelt_hir::StringTrimSide::Start => "start",
                smelt_hir::StringTrimSide::End => "end",
            };
            format!("string_trim_{side_text} {}", operand_text(operand))
        }
        Rvalue::StringAffix {
            op,
            haystack,
            needle,
        } => {
            let op_text = match op {
                smelt_hir::StringAffixOp::StartsWith => "starts_with",
                smelt_hir::StringAffixOp::EndsWith => "ends_with",
            };
            format!(
                "string_{op_text} {}, {}",
                operand_text(haystack),
                operand_text(needle)
            )
        }
        Rvalue::StringSearch {
            op,
            haystack,
            needle,
        } => {
            let op_text = match op {
                smelt_hir::StringSearchOp::Find => "find",
                smelt_hir::StringSearchOp::RFind => "rfind",
            };
            format!(
                "string_{op_text} {}, {}",
                operand_text(haystack),
                operand_text(needle)
            )
        }
        Rvalue::StringReplace {
            op,
            haystack,
            pattern,
            replacement,
        } => {
            let op_text = match op {
                smelt_hir::StringReplaceOp::First => "replace_first",
                smelt_hir::StringReplaceOp::All => "replace_all",
            };
            format!(
                "string_{op_text} {}, {}, {}",
                operand_text(haystack),
                operand_text(pattern),
                operand_text(replacement)
            )
        }
        Rvalue::StringRemoveAffix {
            op,
            haystack,
            affix,
        } => {
            let op_text = match op {
                smelt_hir::StringAffixOp::StartsWith => "remove_prefix",
                smelt_hir::StringAffixOp::EndsWith => "remove_suffix",
            };
            format!(
                "string_{op_text} {}, {}",
                operand_text(haystack),
                operand_text(affix)
            )
        }
        Rvalue::StringRepeat { operand, count } => {
            format!(
                "string_repeat {}, {}",
                operand_text(operand),
                operand_text(count)
            )
        }
        Rvalue::StringPad {
            op,
            operand,
            target_len,
            pad,
        } => {
            let op_text = match op {
                smelt_hir::StringPadOp::Start => "pad_start",
                smelt_hir::StringPadOp::End => "pad_end",
            };
            format!(
                "string_{op_text} {}, {}, {}",
                operand_text(operand),
                operand_text(target_len),
                operand_text(pad)
            )
        }
        Rvalue::StringPredicate { op, operand } => {
            let op_text = match op {
                smelt_hir::StringPredicateOp::IsDigit => "isdigit",
                smelt_hir::StringPredicateOp::IsAlpha => "isalpha",
                smelt_hir::StringPredicateOp::IsAlnum => "isalnum",
            };
            format!("string_{op_text} {}", operand_text(operand))
        }
        Rvalue::RegexIsMatch {
            op,
            pattern,
            haystack,
        } => {
            let op_text = match op {
                smelt_hir::RegexMatchOp::Search => "search",
                smelt_hir::RegexMatchOp::Match => "match",
                smelt_hir::RegexMatchOp::FullMatch => "fullmatch",
            };
            format!(
                "regex_{op_text} {}, {}",
                operand_text(pattern),
                operand_text(haystack)
            )
        }
        Rvalue::RegexReplace {
            op,
            pattern,
            haystack,
            replacement,
        } => {
            let op_text = match op {
                smelt_hir::StringReplaceOp::First => "replace",
                smelt_hir::StringReplaceOp::All => "replace_all",
            };
            format!(
                "regex_{op_text} {}, {}, {}",
                operand_text(pattern),
                operand_text(haystack),
                operand_text(replacement)
            )
        }
        Rvalue::RegexReplaceCallback {
            op,
            pattern,
            haystack,
            callback,
        } => {
            let op_text = match op {
                smelt_hir::StringReplaceOp::First => "replace_callback",
                smelt_hir::StringReplaceOp::All => "replace_all_callback",
            };
            format!(
                "regex_{op_text} {}, {}, {}",
                operand_text(pattern),
                operand_text(haystack),
                operand_text(callback)
            )
        }
        Rvalue::RegexReplaceFirstMatchUppercase { pattern, haystack } => {
            format!(
                "regex_replace_first_upper {}, {}",
                operand_text(pattern),
                operand_text(haystack)
            )
        }
        Rvalue::RegexSplit { pattern, haystack } => {
            format!(
                "regex_split {}, {}",
                operand_text(pattern),
                operand_text(haystack)
            )
        }
        Rvalue::RegexFind { pattern, haystack } => {
            format!(
                "regex_find {}, {}",
                operand_text(pattern),
                operand_text(haystack)
            )
        }
        Rvalue::RegexExec { regex, haystack } => {
            format!(
                "regex_exec {}, {}",
                operand_text(regex),
                operand_text(haystack)
            )
        }
        Rvalue::StringCharAt { operand, index } => {
            format!(
                "string_char_at {}, {}",
                operand_text(operand),
                operand_text(index)
            )
        }
        Rvalue::StringCharCodeAt { operand, index } => {
            format!(
                "string_char_code_at {}, {}",
                operand_text(operand),
                operand_text(index)
            )
        }
        Rvalue::StringContains { haystack, needle } => {
            format!(
                "string_contains {}, {}",
                operand_text(haystack),
                operand_text(needle)
            )
        }
        Rvalue::StringSlice {
            operand,
            start,
            end,
        } => format!(
            "string_slice {}, {}, {}",
            operand_text(operand),
            optional_operand_text(start.as_ref()),
            optional_operand_text(end.as_ref())
        ),
        Rvalue::ListContains { list, item } => {
            format!(
                "list_contains {}, {}",
                operand_text(list),
                operand_text(item)
            )
        }
        Rvalue::SetContains { set, item } => {
            format!("set_contains {}, {}", operand_text(set), operand_text(item))
        }
        Rvalue::SetDisjoint { left, right } => {
            format!(
                "set_isdisjoint {}, {}",
                operand_text(left),
                operand_text(right)
            )
        }
        Rvalue::SetRelation { op, left, right } => {
            let op_text = match op {
                smelt_hir::SetRelationOp::IsSubset => "issubset",
                smelt_hir::SetRelationOp::IsSuperset => "issuperset",
            };
            format!(
                "set_{op_text} {}, {}",
                operand_text(left),
                operand_text(right)
            )
        }
        Rvalue::SetAdd { set, item } => {
            format!("set_add {}, {}", operand_text(set), operand_text(item))
        }
        Rvalue::SetRemove { op, set, item } => {
            let op_text = match op {
                smelt_hir::SetRemoveOp::Delete => "delete",
                smelt_hir::SetRemoveOp::Discard => "discard",
                smelt_hir::SetRemoveOp::Remove => "remove",
            };
            format!(
                "set_{op_text} {}, {}",
                operand_text(set),
                operand_text(item)
            )
        }
        Rvalue::SetClear { set } => format!("set_clear {}", operand_text(set)),
        Rvalue::SetCopy { set } => format!("set_copy {}", operand_text(set)),
        Rvalue::ListToSet { list } => format!("list_to_set {}", operand_text(list)),
        Rvalue::ListPairsToDict { list } => {
            format!("list_pairs_to_dict {}", operand_text(list))
        }
        Rvalue::SetBinary { op, left, right } => {
            let op_text = match op {
                smelt_hir::SetBinaryOp::Union => "union",
                smelt_hir::SetBinaryOp::Intersection => "intersection",
                smelt_hir::SetBinaryOp::Difference => "difference",
                smelt_hir::SetBinaryOp::SymmetricDifference => "symmetric_difference",
            };
            format!(
                "set_{op_text} {}, {}",
                operand_text(left),
                operand_text(right)
            )
        }
        Rvalue::SetProjection { op, set } => {
            let op_text = match op {
                smelt_hir::SetProjectionOp::Values => "values",
                smelt_hir::SetProjectionOp::Entries => "entries",
            };
            format!("set_{op_text} {}", operand_text(set))
        }
        Rvalue::ListConcat { left, right } => {
            format!(
                "list_concat {}, {}",
                operand_text(left),
                operand_text(right)
            )
        }
        Rvalue::ListSearch { op, list, item } => {
            let op_text = match op {
                smelt_hir::ListSearchOp::Find => "find",
                smelt_hir::ListSearchOp::RFind => "rfind",
            };
            format!(
                "list_{op_text} {}, {}",
                operand_text(list),
                operand_text(item)
            )
        }
        Rvalue::ListCallback { op, list, callback } => {
            let op_text = match op {
                smelt_hir::ListCallbackOp::Map => "map",
                smelt_hir::ListCallbackOp::Filter => "filter",
                smelt_hir::ListCallbackOp::Find => "find_callback",
                smelt_hir::ListCallbackOp::FindIndex => "find_index",
                smelt_hir::ListCallbackOp::FindLast => "find_last_callback",
                smelt_hir::ListCallbackOp::FindLastIndex => "find_last_index",
                smelt_hir::ListCallbackOp::Some => "some",
                smelt_hir::ListCallbackOp::Every => "every",
                smelt_hir::ListCallbackOp::ForEach => "for_each",
                smelt_hir::ListCallbackOp::FlatMap => "flat_map",
            };
            format!(
                "list_{op_text} {}, {}",
                operand_text(list),
                operand_text(callback)
            )
        }
        Rvalue::ListFromLength { length } => {
            format!("list_from_length {}", operand_text(length))
        }
        Rvalue::ListFromLengthMap { length, callback } => format!(
            "list_from_length_map {}, {}",
            operand_text(length),
            operand_text(callback)
        ),
        Rvalue::ListReduce {
            list,
            initial,
            callback,
        } => format!(
            "list_reduce {}, {}, {}",
            operand_text(list),
            optional_operand_text(initial.as_ref()),
            operand_text(callback)
        ),
        Rvalue::ListSlice { list, start, end } => format!(
            "list_slice {}, {}, {}",
            operand_text(list),
            optional_operand_text(start.as_ref()),
            optional_operand_text(end.as_ref())
        ),
        Rvalue::ListSplice {
            list,
            start,
            delete_count,
            items,
            mutate,
        } => format!(
            "list_splice {}, {}, {}, [{}], {}",
            operand_text(list),
            operand_text(start),
            optional_operand_text(delete_count.as_ref()),
            items
                .iter()
                .map(|item| {
                    if item.spread {
                        format!("...{}", operand_text(&item.value))
                    } else {
                        operand_text(&item.value)
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
            mutate
        ),
        Rvalue::ListFill {
            list,
            value: fill_value,
            start,
            end,
        } => format!(
            "list_fill {}, {}, {}, {}",
            operand_text(list),
            operand_text(fill_value),
            optional_operand_text(start.as_ref()),
            optional_operand_text(end.as_ref())
        ),
        Rvalue::ListCopyWithin {
            list,
            target,
            start,
            end,
        } => format!(
            "list_copy_within {}, {}, {}, {}",
            operand_text(list),
            operand_text(target),
            operand_text(start),
            optional_operand_text(end.as_ref())
        ),
        Rvalue::ListWith {
            list,
            index,
            value: replacement,
        } => format!(
            "list_with {}, {}, {}",
            operand_text(list),
            operand_text(index),
            operand_text(replacement)
        ),
        Rvalue::ListFlat { list } => format!("list_flat {}", operand_text(list)),
        Rvalue::ListProjection { op, list } => {
            let op_text = match op {
                smelt_hir::ListProjectionOp::Keys => "keys",
                smelt_hir::ListProjectionOp::Values => "values",
                smelt_hir::ListProjectionOp::Entries => "entries",
            };
            format!("list_{op_text} {}", operand_text(list))
        }
        Rvalue::ListPush { list, item } => {
            format!("list_push {}, {}", operand_text(list), operand_text(item))
        }
        Rvalue::ListExtend { list, other } => {
            format!(
                "list_extend {}, {}",
                operand_text(list),
                operand_text(other)
            )
        }
        Rvalue::ListInsert { list, index, item } => format!(
            "list_insert {}, {}, {}",
            operand_text(list),
            operand_text(index),
            operand_text(item)
        ),
        Rvalue::ListUnshift { list, items } => format!(
            "list_unshift {} [{}]",
            operand_text(list),
            items
                .iter()
                .map(operand_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Rvalue::ListReverse { list } => format!("list_reverse {}", operand_text(list)),
        Rvalue::ListClear { list } => format!("list_clear {}", operand_text(list)),
        Rvalue::ListCopy { list } => format!("list_copy {}", operand_text(list)),
        Rvalue::TupleToList { tuple } => format!("tuple_to_list {}", operand_text(tuple)),
        Rvalue::ListToTuple { list } => format!("list_to_tuple {}", operand_text(list)),
        Rvalue::TupleToSet { tuple } => format!("tuple_to_set {}", operand_text(tuple)),
        Rvalue::ListCount { list, item } => {
            format!("list_count {}, {}", operand_text(list), operand_text(item))
        }
        Rvalue::ListSum { list } => format!("list_sum {}", operand_text(list)),
        Rvalue::ListBoolFold { op, list } => {
            let op_text = match op {
                smelt_hir::BoolFoldOp::All => "all",
                smelt_hir::BoolFoldOp::Any => "any",
            };
            format!("list_{op_text} {}", operand_text(list))
        }
        Rvalue::ListSorted { list } => format!("list_sorted {}", operand_text(list)),
        Rvalue::ListReversed { list } => format!("list_reversed {}", operand_text(list)),
        Rvalue::ListEnumerate { list } => format!("list_enumerate {}", operand_text(list)),
        Rvalue::ListZip { left, right } => {
            format!("list_zip {}, {}", operand_text(left), operand_text(right))
        }
        Rvalue::ListRange { start, end, step } => format!(
            "list_range {}, {}, {}",
            operand_text(start),
            operand_text(end),
            operand_text(step)
        ),
        Rvalue::ListRandomChoice { list } => {
            format!("list_random_choice {}", operand_text(list))
        }
        Rvalue::ListIndex { list, item } => {
            format!("list_index {}, {}", operand_text(list), operand_text(item))
        }
        Rvalue::ListRemove { list, item } => {
            format!("list_remove {}, {}", operand_text(list), operand_text(item))
        }
        Rvalue::ListSort {
            list, comparator, ..
        } => {
            let comparator_text = if comparator.is_some() {
                " <comparator>"
            } else {
                ""
            };
            format!("list_sort {}{}", operand_text(list), comparator_text)
        }
        Rvalue::ListPop { list } => format!("list_pop {}", operand_text(list)),
        Rvalue::ListShift { list } => format!("list_shift {}", operand_text(list)),
        Rvalue::TupleContains { tuple, item } => {
            format!(
                "tuple_contains {}, {}",
                operand_text(tuple),
                operand_text(item)
            )
        }
        Rvalue::TupleIndex { tuple, index } => {
            format!("tuple_index {}, {}", operand_text(tuple), index)
        }
        Rvalue::TupleSlice { tuple, start, end } => {
            format!("tuple_slice {}, {}, {}", operand_text(tuple), start, end)
        }
        Rvalue::DictContainsKey { dict, key } => {
            format!(
                "dict_contains_key {}, {}",
                operand_text(dict),
                operand_text(key)
            )
        }
        Rvalue::DictSet {
            dict,
            key,
            value: dict_value,
        } => {
            format!(
                "dict_set {}, {}, {}",
                operand_text(dict),
                operand_text(key),
                operand_text(dict_value)
            )
        }
        Rvalue::DictRemoveKey { dict, key } => {
            format!(
                "dict_remove_key {}, {}",
                operand_text(dict),
                operand_text(key)
            )
        }
        Rvalue::DictGet { dict, key, default } => format!(
            "dict_get {}, {}, {}",
            operand_text(dict),
            operand_text(key),
            optional_operand_text(default.as_ref())
        ),
        Rvalue::DictSetDefault { dict, key, default } => format!(
            "dict_setdefault {}, {}, {}",
            operand_text(dict),
            operand_text(key),
            operand_text(default)
        ),
        Rvalue::DictClear { dict } => format!("dict_clear {}", operand_text(dict)),
        Rvalue::DictPop { dict, key, default } => format!(
            "dict_pop {}, {}, {}",
            operand_text(dict),
            operand_text(key),
            optional_operand_text(default.as_ref())
        ),
        Rvalue::DictUpdate { dict, other } => {
            format!(
                "dict_update {}, {}",
                operand_text(dict),
                operand_text(other)
            )
        }
        Rvalue::DictAssign { target, sources } => format!(
            "dict_assign {}, [{}]",
            operand_text(target),
            sources
                .iter()
                .map(operand_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Rvalue::CallableObjectAssign { callable, props } => format!(
            "callable_object_assign {}, {{{}}}",
            operand_text(callable),
            props
                .iter()
                .map(|(name, value)| format!("{name:?}: {}", operand_text(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Rvalue::DictCopy { dict } => format!("dict_copy {}", operand_text(dict)),
        Rvalue::DictProjection { op, dict } => {
            let op_text = match op {
                smelt_hir::DictProjectionOp::Keys => "keys",
                smelt_hir::DictProjectionOp::Values => "values",
                smelt_hir::DictProjectionOp::Entries => "entries",
            };
            format!("dict_{op_text} {}", operand_text(dict))
        }
        Rvalue::StringSplit {
            haystack,
            separator,
            limit,
        } => {
            format!(
                "string_split {}, {}, {}",
                operand_text(haystack),
                operand_text(separator),
                optional_operand_text(limit.as_ref())
            )
        }
        Rvalue::StringChars { haystack } => {
            format!("string_chars {}", operand_text(haystack))
        }
        Rvalue::StringJoin { items, separator } => {
            format!(
                "string_join {}, {}",
                operand_text(items),
                operand_text(separator)
            )
        }
        Rvalue::JsonStringify { value: json_value } => {
            format!("json_stringify {}", operand_text(json_value))
        }
        Rvalue::JsonParse { text } => {
            format!("json_parse {}", operand_text(text))
        }
        Rvalue::HttpGetText { url } => format!("http_get_text {}", operand_text(url)),
        Rvalue::DateNow => "date_now".to_owned(),
        Rvalue::DateToIsoString { timestamp_ms } => {
            format!("date_to_iso_string {}", operand_text(timestamp_ms))
        }
        Rvalue::DateFromParts { parts } => format!(
            "date_from_parts [{}]",
            parts
                .iter()
                .map(operand_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Rvalue::DateGetPart { part, timestamp_ms } => {
            format!("date_get_{part:?} {}", operand_text(timestamp_ms))
        }
        Rvalue::DateSetPart {
            part,
            timestamp_ms,
            values,
        } => format!(
            "date_set_{part:?} {}, [{}]",
            operand_text(timestamp_ms),
            values
                .iter()
                .map(operand_text)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Rvalue::UrlField { field, url } => {
            let field_text = match field {
                smelt_hir::UrlField::Href => "href",
                smelt_hir::UrlField::Protocol => "protocol",
                smelt_hir::UrlField::Host => "host",
                smelt_hir::UrlField::Origin => "origin",
                smelt_hir::UrlField::Hostname => "hostname",
                smelt_hir::UrlField::Pathname => "pathname",
                smelt_hir::UrlField::Search => "search",
            };
            format!("url_{field_text} {}", operand_text(url))
        }
        Rvalue::FileReadText { path } => format!("file_read_text {}", operand_text(path)),
        Rvalue::FileWriteText { path, text } => {
            format!(
                "file_write_text {}, {}",
                operand_text(path),
                operand_text(text)
            )
        }
        Rvalue::Await(operand) => format!("await {}", operand_text(operand)),
        Rvalue::AsyncOp { op, args } => {
            let op_text = match op {
                smelt_hir::AsyncOp::All => "async_all",
                smelt_hir::AsyncOp::Race => "async_race",
                smelt_hir::AsyncOp::AllSettled => "async_all_settled",
                smelt_hir::AsyncOp::Sleep => "async_sleep",
                smelt_hir::AsyncOp::CreateTask => "async_create_task",
                smelt_hir::AsyncOp::WaitFor => "async_wait_for",
                smelt_hir::AsyncOp::HttpGetText => "async_http_get_text",
            };
            let arg_list = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
            format!("{op_text}({arg_list})")
        }
    }
}

/// Formats a terminator to human-readable text.
fn terminator_text(terminator: &Terminator) -> String {
    match terminator {
        Terminator::Goto(target) => format!("goto bb{}", target.0),
        Terminator::Call {
            callee,
            args,
            dest,
            target,
            unwind,
        } => {
            let arg_list = args.iter().map(operand_text).collect::<Vec<_>>().join(", ");
            let unwind_text = unwind.map_or_else(String::new, |handler| {
                format!(" unwind bb{}", handler.catch_block.0)
            });
            format!(
                "{} = call {}({}) -> bb{}{}",
                local_ref(*dest),
                callee_text(callee),
                arg_list,
                target.0,
                unwind_text
            )
        }
        Terminator::Switch {
            cond,
            then_block,
            else_block,
        } => format!(
            "switch {} ? bb{} : bb{}",
            operand_text(cond),
            then_block.0,
            else_block.0
        ),
        Terminator::Match {
            scrutinee,
            arms,
            default,
        } => {
            let arm_list = arms
                .iter()
                .map(|arm| format!("{} => bb{}", constant_text(&arm.label), arm.target.0))
                .collect::<Vec<_>>()
                .join(", ");
            let default_suffix = default
                .map(|target| {
                    if arm_list.is_empty() {
                        format!("_ => bb{}", target.0)
                    } else {
                        format!(", _ => bb{}", target.0)
                    }
                })
                .unwrap_or_default();
            format!(
                "match {} {{{}{}}}",
                operand_text(scrutinee),
                arm_list,
                default_suffix
            )
        }
        Terminator::Return(operand) => format!("return {}", operand_text(operand)),
        Terminator::Throw(operand) => format!("throw {}", operand_text(operand)),
        Terminator::Unreachable => "unreachable".to_owned(),
    }
}

/// Formats a callee to human-readable text.
fn callee_text(callee: &Callee) -> String {
    match callee {
        Callee::Static(func) => format!("fn{}", func.0),
        Callee::Indirect(operand) => operand_text(operand),
        Callee::Builtin(BuiltinFn::ConsoleLog) => "@console_log".to_owned(),
    }
}

/// Formats an operand to human-readable text.
fn operand_text(operand: &Operand) -> String {
    match operand {
        Operand::Copy(place) => format!("copy {}", place_text(place)),
        Operand::Move(place) => format!("move {}", place_text(place)),
        Operand::Const(constant) => constant_text(constant),
    }
}

/// Formats an optional operand to human-readable text.
fn optional_operand_text(operand: Option<&Operand>) -> String {
    operand.map_or_else(|| "_".to_owned(), operand_text)
}

/// Formats a place to human-readable text.
fn place_text(place: &Place) -> String {
    match place {
        Place::Local(local) => local_ref(*local),
        Place::Field { base, field } => format!("{}.field{}", local_ref(*base), field.0),
        Place::Index { base, index } => format!("{}[{}]", local_ref(*base), operand_text(index)),
    }
}

/// Formats a constant to human-readable text.
fn constant_text(constant: &Constant) -> String {
    match constant {
        Constant::Bool(value) => value.to_string(),
        Constant::Int(value) => value.to_string(),
        Constant::Float(value) => {
            if value.fract() == 0.0 {
                format!("{value:.1}")
            } else {
                value.to_string()
            }
        }
        Constant::String(value) => format!("\"{value}\""),
        Constant::None => "none".to_owned(),
    }
}

/// Formats a local reference to text.
fn local_ref(local: LocalId) -> String {
    format!("%{}", local.0)
}

/// Formats a type reference to human-readable text.
fn type_ref(mir: &Mir, ty: TypeId) -> String {
    let Some(ty_value) = mir.types.get(ty) else {
        return format!("t{}", ty.0);
    };
    match ty_value {
        Type::Bool => "Bool".to_owned(),
        Type::Int => "Int".to_owned(),
        Type::Float => "Float".to_owned(),
        Type::String => "String".to_owned(),
        Type::Unknown => "Unknown".to_owned(),
        Type::Never => "Never".to_owned(),
        Type::TypeParam { name } => mir.symbols.get(*name).unwrap_or("<unknown>").to_owned(),
        Type::None => "None".to_owned(),
        Type::List(item) => format!("List<{}>", type_ref(mir, *item)),
        Type::Set(item) => format!("Set<{}>", type_ref(mir, *item)),
        Type::Dict(key, value) => {
            format!("Dict<{}, {}>", type_ref(mir, *key), type_ref(mir, *value))
        }
        Type::Tuple(items) => {
            let item_list = items
                .iter()
                .map(|item| type_ref(mir, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({item_list})")
        }
        Type::Optional(item) => format!("Optional<{}>", type_ref(mir, *item)),
        Type::Union(items) => items
            .iter()
            .map(|item| type_ref(mir, *item))
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Class { name, args } => {
            let class_name = mir.symbols.get(*name).unwrap_or("<unknown>");
            if args.is_empty() {
                class_name.to_owned()
            } else {
                let type_args = args
                    .iter()
                    .map(|arg| type_ref(mir, *arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{class_name}<{type_args}>")
            }
        }
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|param| type_ref(mir, *param))
                .collect::<Vec<_>>()
                .join(", ");
            let async_prefix = if function.is_async { "async " } else { "" };
            format!(
                "{async_prefix}fn({params}) -> {}",
                type_ref(mir, function.return_ty)
            )
        }
        Type::Future(item) => format!("Future<{}>", type_ref(mir, *item)),
    }
}

/// Convert an index into a `u32` identifier.
fn index_to_u32(index: usize, label: &str) -> u32 {
    if let Ok(value) = u32::try_from(index) {
        value
    } else {
        let _ = label;
        u32::MAX
    }
}
