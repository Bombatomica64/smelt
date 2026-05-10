//! Pretty-printing utilities for HIR.

use crate::body::{Body, Pattern, Stmt};
use crate::expr::{AsyncOp, Expr, ExprKind, Literal};
use crate::ids::{ExprId, ItemId, LocalId, ModuleId, PatternId, TypeId, id_index};
use crate::item::Item;
use crate::krate::Crate;
use crate::ty::Type;
use std::fmt::Write as _;

/// Append formatted text to the output buffer.
fn push_fmt(out: &mut String, args: std::fmt::Arguments<'_>) {
    let _ignored = out.write_fmt(args);
}

/// Formats the HIR of the given modules in a compact, human-readable form.
#[must_use]
pub fn format_compact(krate: &Crate, modules: &[(String, ModuleId)]) -> String {
    let mut out = String::new();

    for (path, module_id) in modules {
        let Some(module_idx) = usize::try_from(module_id.0).ok() else {
            push_fmt(
                &mut out,
                format_args!("module {path} (ModuleId({}))\n", module_id.0),
            );
            out.push_str("  <invalid module>\n\n");
            continue;
        };
        let Some(module) = krate.modules.get(module_idx) else {
            push_fmt(
                &mut out,
                format_args!("module {path} (ModuleId({}))\n", module_id.0),
            );
            out.push_str("  <missing module>\n\n");
            continue;
        };
        push_fmt(
            &mut out,
            format_args!("module {path} (ModuleId({}))\n", module_id.0),
        );

        let Some(body_id) = module.body else {
            out.push_str("  <no body>\n\n");
            continue;
        };

        push_fmt(&mut out, format_args!("  body BodyId({})\n", body_id.0));

        if !module.items.is_empty() {
            out.push_str("  items\n");
            for item in &module.items {
                out.push_str("    ");
                out.push_str(&item_text(krate, *item));
                out.push('\n');
            }
        }

        let Some(body_idx) = usize::try_from(body_id.0).ok() else {
            out.push_str("  <invalid body>\n\n");
            continue;
        };
        let Some(body) = krate.bodies.get(body_idx) else {
            out.push_str("  <missing body>\n\n");
            continue;
        };
        format_body_sections(krate, body, &mut out);
        out.push('\n');
    }

    out.push_str("interned types\n");
    for (idx, ty) in krate.types.all().iter().enumerate() {
        push_fmt(
            &mut out,
            format_args!("  t{} = {}\n", idx, type_text(krate, ty)),
        );
    }

    out
}

/// Appends the locals/exprs/stmts sections of a body to the output string.
fn format_body_sections(krate: &Crate, body: &Body, out: &mut String) {
    if !body.locals.is_empty() {
        out.push_str("  locals\n");
        for (idx, local) in body.locals.iter().enumerate() {
            let local_id = LocalId(id_index(idx));
            let mutability = if local.mutable { "let" } else { "const" };
            let name = local
                .name
                .and_then(|sym| krate.names.get(sym).or_else(|| krate.symbols.get(sym)))
                .unwrap_or("_");
            push_fmt(
                out,
                format_args!(
                    "    {} {} {}: {}\n",
                    local_ref(local_id),
                    mutability,
                    name,
                    type_ref(krate, local.ty)
                ),
            );
        }
    }

    if !body.exprs.is_empty() {
        out.push_str("  exprs\n");
        for (idx, expr) in body.exprs.iter().enumerate() {
            let expr_id = ExprId(id_index(idx));
            push_fmt(
                out,
                format_args!(
                    "    {}: {} = {}\n",
                    expr_ref(expr_id),
                    type_ref(krate, expr.ty),
                    expr_text(krate, expr)
                ),
            );
        }
    }

    if !body.stmts.is_empty() {
        out.push_str("  stmts\n");
        for (idx, stmt) in body.stmts.iter().enumerate() {
            push_fmt(
                out,
                format_args!("    s{}: {}\n", idx, stmt_text(krate, body, stmt)),
            );
        }
    }

    if let Some(machine) = &body.async_state_machine {
        out.push_str("  async state machine\n");
        let states = machine
            .states
            .iter()
            .map(|state| format!("state{}", state.id.0))
            .collect::<Vec<_>>()
            .join(", ");
        push_fmt(out, format_args!("    states [{states}]\n"));
        for suspension in &machine.suspensions {
            push_fmt(
                out,
                format_args!(
                    "    suspend {} on {} -> state{}\n",
                    expr_ref(suspension.await_expr),
                    expr_ref(suspension.future),
                    suspension.resume_state.0
                ),
            );
        }
    }
}

/// Formats a statement as text.
fn stmt_text(krate: &Crate, body: &Body, stmt: &Stmt) -> String {
    match stmt {
        Stmt::Let { pat, ty, value } => {
            let value_suffix = value
                .map(|expr_id| format!(" = {}", expr_ref(expr_id)))
                .unwrap_or_default();
            format!(
                "let {}: {}{}",
                pattern_text(body, *pat),
                type_ref(krate, *ty),
                value_suffix
            )
        }
        Stmt::Assign { target, value } => {
            format!("{} = {}", expr_ref(*target), expr_ref(*value))
        }
        Stmt::Expr(expr) => expr_ref(*expr),
        Stmt::Return(Some(expr)) => format!("return {}", expr_ref(*expr)),
        Stmt::Return(None) => "return".to_owned(),
        Stmt::Break => "break".to_owned(),
        Stmt::Continue => "continue".to_owned(),
        Stmt::If { .. }
        | Stmt::While { .. }
        | Stmt::For { .. }
        | Stmt::Match { .. }
        | Stmt::Throw(_)
        | Stmt::TryCatch { .. } => control_stmt_text(body, stmt),
    }
}

/// Formats control-flow statements as text.
fn control_stmt_text(body: &Body, stmt: &Stmt) -> String {
    match stmt {
        Stmt::If {
            cond,
            then_block,
            else_block,
        } => {
            let else_text = else_block
                .map(|block| format!(" else {block:?}"))
                .unwrap_or_default();
            format!("if {} then {:?}{}", expr_ref(*cond), then_block, else_text)
        }
        Stmt::While {
            cond,
            body: loop_body,
        } => format!("while {} {:?}", expr_ref(*cond), loop_body),
        Stmt::For {
            pat,
            iter,
            body: loop_body,
        } => {
            format!(
                "for {} in {} {:?}",
                pattern_text(body, *pat),
                expr_ref(*iter),
                loop_body
            )
        }
        Stmt::Match { .. } => match_stmt_text(stmt),
        Stmt::Throw(expr) => format!("throw {}", expr_ref(*expr)),
        Stmt::TryCatch { .. } => try_catch_stmt_text(stmt),
        Stmt::Let { .. }
        | Stmt::Assign { .. }
        | Stmt::Expr(_)
        | Stmt::Return(_)
        | Stmt::Break
        | Stmt::Continue => "invalid statement".to_owned(),
    }
}

/// Formats a match statement as text.
fn match_stmt_text(stmt: &Stmt) -> String {
    let Stmt::Match {
        scrutinee,
        arms,
        default,
    } = stmt
    else {
        return "invalid match".to_owned();
    };
    let arm_text = arms
        .iter()
        .map(|arm| format!("{} => {:?}", literal_text(&arm.label), arm.body))
        .collect::<Vec<_>>()
        .join(", ");
    let default_text = default
        .map(|block| format!(" default {block:?}"))
        .unwrap_or_default();
    format!(
        "match {} {{{}}}{}",
        expr_ref(*scrutinee),
        arm_text,
        default_text
    )
}

/// Formats a try/catch/finally statement as text.
fn try_catch_stmt_text(stmt: &Stmt) -> String {
    let Stmt::TryCatch {
        body,
        catch_binding,
        catch_body,
        finally_body,
    } = stmt
    else {
        return "invalid try/catch".to_owned();
    };
    let catch = catch_body
        .map(|block| {
            let binding = catch_binding
                .map(|local| format!(" {}", local_ref(local)))
                .unwrap_or_default();
            format!(" catch{binding} {block:?}")
        })
        .unwrap_or_default();
    let finally = finally_body
        .map(|block| format!(" finally {block:?}"))
        .unwrap_or_default();
    format!("try {body:?}{catch}{finally}")
}

/// Formats an expression as text.
fn expr_text(krate: &Crate, expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Literal(literal) => literal_text(literal),
        ExprKind::Local(local) => local_ref(*local),
        ExprKind::Item(item) => item_ref(krate, *item),
        ExprKind::Call { .. } | ExprKind::Method { .. } | ExprKind::New { .. } => {
            call_like_expr_text(krate, expr)
        }
        ExprKind::Field { receiver, field } => {
            let field_name = krate.symbols.get(*field).unwrap_or("<unknown>");
            format!("{}.{}", expr_ref(*receiver), field_name)
        }
        ExprKind::Index { receiver, index } => {
            format!("{}[{}]", expr_ref(*receiver), expr_ref(*index))
        }
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
        ExprKind::SetBinary { op, left, right } => {
            let op_name = match op {
                crate::expr::SetBinaryOp::Union => "union",
                crate::expr::SetBinaryOp::Intersection => "intersection",
                crate::expr::SetBinaryOp::Difference => "difference",
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
        ExprKind::ListCallback { op, list, .. } => {
            let op_name = match op {
                crate::expr::ListCallbackOp::Map => "map",
                crate::expr::ListCallbackOp::Filter => "filter",
                crate::expr::ListCallbackOp::Find => "find_callback",
                crate::expr::ListCallbackOp::FindIndex => "find_index",
                crate::expr::ListCallbackOp::Some => "some",
                crate::expr::ListCallbackOp::Every => "every",
                crate::expr::ListCallbackOp::ForEach => "for_each",
            };
            format!("list_{op_name} {} <callback>", expr_ref(*list))
        }
        ExprKind::ListReduce { list, initial, .. } => {
            format!(
                "list_reduce {}, {} <callback>",
                expr_ref(*list),
                expr_ref(*initial)
            )
        }
        ExprKind::ListSlice { list, start, end } => format!(
            "list_slice {}, {}, {}",
            expr_ref(*list),
            optional_expr_ref(*start),
            optional_expr_ref(*end)
        ),
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
        ExprKind::ListRange { start, end, step } => format!(
            "list_range {}, {}, {}",
            expr_ref(*start),
            expr_ref(*end),
            expr_ref(*step)
        ),
        ExprKind::ListIndex { list, item } => {
            format!("list_index {}, {}", expr_ref(*list), expr_ref(*item))
        }
        ExprKind::ListRemove { list, item } => {
            format!("list_remove {}, {}", expr_ref(*list), expr_ref(*item))
        }
        ExprKind::ListSort { list } => format!("list_sort {}", expr_ref(*list)),
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
        } => {
            format!(
                "string_split {}, {}",
                expr_ref(*haystack),
                expr_ref(*separator)
            )
        }
        ExprKind::StringJoin { items, separator } => {
            format!("string_join {}, {}", expr_ref(*items), expr_ref(*separator))
        }
        ExprKind::JsonStringify { value } => format!("json_stringify {}", expr_ref(*value)),
        ExprKind::JsonParse { text } => format!("json_parse {}", expr_ref(*text)),
        ExprKind::HttpGetText { url } => format!("http_get_text {}", expr_ref(*url)),
        ExprKind::BinOp { op, lhs, rhs } => {
            format!("{op:?} {}, {}", expr_ref(*lhs), expr_ref(*rhs))
        }
        ExprKind::UnaryOp { op, operand } => format!("{op:?} {}", expr_ref(*operand)),
        ExprKind::Block(block) => format!("block {block:?}"),
        ExprKind::Lambda { body, return_ty } => {
            format!("lambda {body:?} -> {}", type_ref(krate, *return_ty))
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
fn call_like_expr_text(krate: &Crate, expr: &Expr) -> String {
    match &expr.kind {
        ExprKind::Call { callee, args } => {
            let arg_text = expr_list_text(args);
            format!("call {}({arg_text})", expr_ref(*callee))
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
        ExprKind::Literal(_)
        | ExprKind::Local(_)
        | ExprKind::Item(_)
        | ExprKind::Field { .. }
        | ExprKind::Index { .. }
        | ExprKind::Len { .. }
        | ExprKind::NumericAbs { .. }
        | ExprKind::NumericRound { .. }
        | ExprKind::NumericExtrema { .. }
        | ExprKind::NumericHypot { .. }
        | ExprKind::NumericPredicate { .. }
        | ExprKind::NumericUnaryFunc { .. }
        | ExprKind::NumericPow { .. }
        | ExprKind::NumericAtan2 { .. }
        | ExprKind::NumericRandom
        | ExprKind::StringCase { .. }
        | ExprKind::StringTrim { .. }
        | ExprKind::StringAffix { .. }
        | ExprKind::StringSearch { .. }
        | ExprKind::StringReplace { .. }
        | ExprKind::StringRemoveAffix { .. }
        | ExprKind::StringRepeat { .. }
        | ExprKind::StringPad { .. }
        | ExprKind::StringPredicate { .. }
        | ExprKind::RegexIsMatch { .. }
        | ExprKind::StringCharAt { .. }
        | ExprKind::StringCharCodeAt { .. }
        | ExprKind::StringContains { .. }
        | ExprKind::StringSlice { .. }
        | ExprKind::ListContains { .. }
        | ExprKind::SetContains { .. }
        | ExprKind::SetAdd { .. }
        | ExprKind::SetRemove { .. }
        | ExprKind::SetClear { .. }
        | ExprKind::SetCopy { .. }
        | ExprKind::SetBinary { .. }
        | ExprKind::SetProjection { .. }
        | ExprKind::ListConcat { .. }
        | ExprKind::ListSearch { .. }
        | ExprKind::ListCallback { .. }
        | ExprKind::ListReduce { .. }
        | ExprKind::ListSlice { .. }
        | ExprKind::ListPush { .. }
        | ExprKind::ListExtend { .. }
        | ExprKind::ListInsert { .. }
        | ExprKind::ListUnshift { .. }
        | ExprKind::ListReverse { .. }
        | ExprKind::ListClear { .. }
        | ExprKind::ListCopy { .. }
        | ExprKind::ListCount { .. }
        | ExprKind::ListSum { .. }
        | ExprKind::ListBoolFold { .. }
        | ExprKind::ListSorted { .. }
        | ExprKind::ListRange { .. }
        | ExprKind::ListIndex { .. }
        | ExprKind::ListRemove { .. }
        | ExprKind::ListSort { .. }
        | ExprKind::ListPop { .. }
        | ExprKind::ListShift { .. }
        | ExprKind::TupleContains { .. }
        | ExprKind::DictContainsKey { .. }
        | ExprKind::DictSet { .. }
        | ExprKind::DictRemoveKey { .. }
        | ExprKind::DictGet { .. }
        | ExprKind::DictSetDefault { .. }
        | ExprKind::DictClear { .. }
        | ExprKind::DictPop { .. }
        | ExprKind::DictUpdate { .. }
        | ExprKind::DictCopy { .. }
        | ExprKind::DictProjection { .. }
        | ExprKind::StringSplit { .. }
        | ExprKind::StringJoin { .. }
        | ExprKind::JsonStringify { .. }
        | ExprKind::JsonParse { .. }
        | ExprKind::HttpGetText { .. }
        | ExprKind::BinOp { .. }
        | ExprKind::UnaryOp { .. }
        | ExprKind::Block(_)
        | ExprKind::Lambda { .. }
        | ExprKind::ListLit(_)
        | ExprKind::SetLit(_)
        | ExprKind::DictLit(_)
        | ExprKind::TupleLit(_)
        | ExprKind::TupleIndex { .. }
        | ExprKind::TupleSlice { .. }
        | ExprKind::Await(_)
        | ExprKind::AsyncOp { .. } => "invalid call".to_owned(),
    }
}

/// Formats a comma-separated expression ID list.
fn expr_list_text(items: &[ExprId]) -> String {
    items
        .iter()
        .map(|arg| expr_ref(*arg))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Formats a dictionary literal as text.
fn dict_lit_text(items: &[(ExprId, ExprId)]) -> String {
    let item_text = items
        .iter()
        .map(|(key, value)| format!("{}: {}", expr_ref(*key), expr_ref(*value)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{{item_text}}}")
}

/// Formats a collection literal as text.
fn collection_text(open: &str, close: &str, items: &[ExprId]) -> String {
    let item_text = items
        .iter()
        .map(|item| expr_ref(*item))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{open}{item_text}{close}")
}

/// Formats a pattern as text.
fn pattern_text(body: &Body, pattern: PatternId) -> String {
    let Some(pattern_idx) = usize::try_from(pattern.0).ok() else {
        return "<invalid-pattern>".to_owned();
    };
    let Some(pattern_value) = body.patterns.get(pattern_idx) else {
        return "<missing-pattern>".to_owned();
    };
    match pattern_value {
        Pattern::Wildcard => "_".to_owned(),
        Pattern::Binding(local) => local_ref(*local),
        Pattern::Tuple(items) => {
            let tuple_items = items
                .iter()
                .map(|item| pattern_text(body, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({tuple_items})")
        }
        Pattern::Literal(literal) => literal_text(literal),
    }
}

/// Formats a literal as text.
fn literal_text(literal: &Literal) -> String {
    match literal {
        Literal::Bool(value) => value.to_string(),
        Literal::Int(value) => value.to_string(),
        Literal::Float(value) => {
            if value.fract() == 0.0 {
                format!("{value:.1}")
            } else {
                value.to_string()
            }
        }
        Literal::String(value) => format!("\"{value}\""),
        Literal::None => "none".to_owned(),
    }
}

/// Formats an item reference as text.
fn item_ref(krate: &Crate, item: ItemId) -> String {
    let Some(item_value) = krate.items.get(item.0 as usize) else {
        return format!("item{}", item.0);
    };

    let name = match item_value {
        Item::Function(function) => krate.symbols.get(function.name),
        Item::Class(class) => krate.symbols.get(class.name),
        Item::Interface(interface) => krate.symbols.get(interface.name),
        Item::TypeAlias(alias) => krate.symbols.get(alias.name),
        Item::Const(item) => krate.symbols.get(item.name),
    }
    .unwrap_or("<unknown>");

    format!("@{}({})", item.0, name)
}

/// Formats an item as text.
fn item_text(krate: &Crate, item: ItemId) -> String {
    let Some(item_idx) = usize::try_from(item.0).ok() else {
        return format!("invalid-item-{}", item.0);
    };
    let Some(item_value) = krate.items.get(item_idx) else {
        return format!("missing-item-{}", item.0);
    };
    match item_value {
        Item::Function(function) => {
            let name = krate.symbols.get(function.name).unwrap_or("<unknown>");
            format!("fn {name} owner {:?}", function.owner)
        }
        Item::Class(class) => class_item_text(krate, class),
        Item::Interface(interface) => interface_item_text(krate, interface),
        Item::TypeAlias(alias) => {
            let name = krate.symbols.get(alias.name).unwrap_or("<unknown>");
            format!("type {name} = {}", type_ref(krate, alias.ty))
        }
        Item::Const(const_item) => {
            let name = krate.symbols.get(const_item.name).unwrap_or("<unknown>");
            format!("const {name}: {}", type_ref(krate, const_item.ty))
        }
    }
}

/// Formats a list of fields as text.
fn fields_text(krate: &Crate, fields: &[crate::item::Field]) -> String {
    fields
        .iter()
        .map(|field| {
            format!(
                "{:?} {}{}: {}",
                field.visibility,
                krate.symbols.get(field.name).unwrap_or("<unknown>"),
                if field.optional { "?" } else { "" },
                type_ref(krate, field.ty)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Formats a class item as text.
fn class_item_text(krate: &Crate, class: &crate::item::Class) -> String {
    let name = krate.symbols.get(class.name).unwrap_or("<unknown>");
    let fields = fields_text(krate, &class.fields);
    let implements = class
        .implements
        .iter()
        .map(|sym| krate.symbols.get(*sym).unwrap_or("<unknown>").to_owned())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "class {name} fields [{fields}] constructor {:?} methods {:?} implements [{implements}]",
        class.constructor, class.methods
    )
}

/// Formats an interface item as text.
fn interface_item_text(krate: &Crate, interface: &crate::item::Interface) -> String {
    let name = krate.symbols.get(interface.name).unwrap_or("<unknown>");
    let fields = fields_text(krate, &interface.fields);
    let methods = interface
        .methods
        .iter()
        .map(|method| method_sig_text(krate, method))
        .collect::<Vec<_>>()
        .join(", ");
    format!("interface {name} fields [{fields}] methods [{methods}]")
}

/// Formats a method signature as text.
fn method_sig_text(krate: &Crate, method: &crate::item::MethodSig) -> String {
    let params = method
        .params
        .iter()
        .map(|param| {
            format!(
                "{}: {}",
                krate.symbols.get(param.name).unwrap_or("<unknown>"),
                type_ref(krate, param.ty)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{:?} {}({params}) -> {}",
        method.visibility,
        krate.symbols.get(method.name).unwrap_or("<unknown>"),
        type_ref(krate, method.return_ty)
    )
}

/// Formats a type reference as text.
fn type_ref(krate: &Crate, ty: TypeId) -> String {
    let Some(ty_value) = krate.types.get(ty) else {
        return format!("t{}", ty.0);
    };
    type_text(krate, ty_value)
}

/// Formats a type as text.
fn type_text(krate: &Crate, ty: &Type) -> String {
    match ty {
        Type::Bool => "Bool".to_owned(),
        Type::Int => "Int".to_owned(),
        Type::Float => "Float".to_owned(),
        Type::String => "String".to_owned(),
        Type::None => "None".to_owned(),
        Type::List(item) => format!("List<{}>", type_ref(krate, *item)),
        Type::Set(item) => format!("Set<{}>", type_ref(krate, *item)),
        Type::Dict(key, value) => {
            format!(
                "Dict<{}, {}>",
                type_ref(krate, *key),
                type_ref(krate, *value)
            )
        }
        Type::Tuple(items) => {
            let items = items
                .iter()
                .map(|item| type_ref(krate, *item))
                .collect::<Vec<_>>()
                .join(", ");
            format!("({items})")
        }
        Type::Optional(item) => format!("Optional<{}>", type_ref(krate, *item)),
        Type::Union(items) => items
            .iter()
            .map(|item| type_ref(krate, *item))
            .collect::<Vec<_>>()
            .join(" | "),
        Type::Class { name, args } => {
            let name = krate.symbols.get(*name).unwrap_or("<unknown>");
            if args.is_empty() {
                name.to_owned()
            } else {
                let args = args
                    .iter()
                    .map(|arg| type_ref(krate, *arg))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{name}<{args}>")
            }
        }
        Type::Function(function) => {
            let params = function
                .params
                .iter()
                .map(|param| type_ref(krate, *param))
                .collect::<Vec<_>>()
                .join(", ");
            let async_prefix = if function.is_async { "async " } else { "" };
            format!(
                "{async_prefix}fn({params}) -> {}",
                type_ref(krate, function.return_ty)
            )
        }
        Type::Future(item) => format!("Future<{}>", type_ref(krate, *item)),
    }
}

/// Formats a local variable reference as text.
fn local_ref(local: LocalId) -> String {
    format!("%{}", local.0)
}

/// Formats an expression reference as text.
fn expr_ref(expr: ExprId) -> String {
    format!("#{}", expr.0)
}

/// Formats an optional expression reference as text.
fn optional_expr_ref(expr: Option<ExprId>) -> String {
    expr.map_or_else(|| "_".to_owned(), expr_ref)
}
