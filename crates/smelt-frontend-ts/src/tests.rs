//! Tests for TypeScript frontend lowering.

use super::*;
use smelt_hir::{
    DictProjectionOp, ExprKind, FileId, Function, Item, ListSearchOp, Literal, ModuleId,
    NumericExtremaOp, NumericPredicateOp, NumericRoundOp, NumericUnaryFuncOp, Stmt, StringAffixOp,
    StringCaseOp, StringReplaceOp, StringSearchOp, StringTrimSide, Type,
};

/// Fail the current test with a formatted message when `cond` is false.
macro_rules! ensure {
    ($cond:expr $(,)?) => {{
        if !$cond {
            return Err(format!("assertion failed: {}", stringify!($cond)));
        }
    }};
    ($cond:expr, $($arg:tt)+) => {{
        if !$cond {
            return Err(format!($($arg)+));
        }
    }};
}

/// Fail the current test with a formatted message when the values differ.
macro_rules! ensure_eq {
    ($left:expr, $right:expr $(,)?) => {{
        let left = &$left;
        let right = &$right;
        if left != right {
            return Err(format!(
                "assertion failed: left != right\n  left: {left:?}\n right: {right:?}"
            ));
        }
    }};
}

/// Marks a test fixture string as TypeScript source code.
macro_rules! ts {
    ($source:literal $(,)?) => {
        $source
    };
}

/// Lower TypeScript source and fail the test with a readable message on error.
fn lower_ok(source: &str, ctx: &mut HirCtx) -> Result<ModuleId, String> {
    to_hir(source, FileId(0), ctx)
        .map_err(|errors| format!("unexpected lowering failure: {errors:?}"))
}

/// Lower TypeScript source and return the diagnostics when lowering fails.
fn lowering_errors(source: &str, ctx: &mut HirCtx) -> Result<Vec<SmeltError>, String> {
    match to_hir(source, FileId(0), ctx) {
        Ok(module_id) => Err(format!(
            "expected lowering failure, got module {module_id:?}"
        )),
        Err(errors) => Ok(errors),
    }
}

/// Get the lowered module for a module ID.
fn module(ctx: &HirCtx, module_id: ModuleId) -> Result<&smelt_hir::Module, String> {
    let module_index = usize::try_from(module_id.0)
        .map_err(|err| format!("module id {module_id:?} does not fit in usize: {err}"))?;
    ctx.krate
        .modules
        .get(module_index)
        .ok_or_else(|| format!("missing module {module_id:?} in lowered crate"))
}

/// Get the module body for a lowered module.
fn module_body<'a>(
    ctx: &'a HirCtx,
    module: &'a smelt_hir::Module,
) -> Result<&'a smelt_hir::Body, String> {
    let body_id = module
        .body
        .ok_or_else(|| format!("module {} has no body", module.name))?;
    let body_index = usize::try_from(body_id.0)
        .map_err(|err| format!("body id {body_id:?} does not fit in usize: {err}"))?;
    ctx.krate
        .bodies
        .get(body_index)
        .ok_or_else(|| format!("missing body {body_id:?} in lowered crate"))
}

/// Get a function item at the provided module item index.
fn function_item<'a>(
    ctx: &'a HirCtx,
    module: &'a smelt_hir::Module,
    index: usize,
) -> Result<&'a Function, String> {
    let item_id = *module
        .items
        .get(index)
        .ok_or_else(|| format!("missing module item at index {index}"))?;
    let item_index = usize::try_from(item_id.0)
        .map_err(|err| format!("item id {item_id:?} does not fit in usize: {err}"))?;
    let item = ctx
        .krate
        .items
        .get(item_index)
        .ok_or_else(|| format!("missing item {item_id:?} in lowered crate"))?;
    let Item::Function(function) = item else {
        return Err(format!(
            "expected function item at index {index}, got {item:?}"
        ));
    };
    Ok(function)
}

/// Get the body owned by a function.
fn function_body<'a>(
    ctx: &'a HirCtx,
    function: &'a Function,
) -> Result<&'a smelt_hir::Body, String> {
    let body_id = function
        .body
        .ok_or_else(|| format!("function {:?} has no body", function.name))?;
    let body_index = usize::try_from(body_id.0)
        .map_err(|err| format!("body id {body_id:?} does not fit in usize: {err}"))?;
    ctx.krate
        .bodies
        .get(body_index)
        .ok_or_else(|| format!("missing body {body_id:?} in lowered crate"))
}

/// Assert that the first lowering error is an unsupported TS diagnostic containing `needle`.
fn assert_unsupported_ts(errors: &[SmeltError], needle: &str) -> Result<(), String> {
    let error = errors
        .first()
        .ok_or_else(|| "expected at least one lowering error".to_owned())?;
    ensure_eq!(error.code, "smelt::unsupported-ts");
    ensure!(error.span.end >= error.span.start);
    ensure!(error.message.contains(needle));
    Ok(())
}

#[test]
fn converts_top_level_let_and_console_log() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("let x = 6;
console.log(x);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure_eq!(body.locals.len(), 1);
    ensure_eq!(body.stmts.len(), 2);
    ensure_eq!(body.exprs.len(), 4);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_stdlib_length_properties() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3];
const count = values.length;
const word = "smelt";
const letters = word.length;
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let len_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::Len { .. }))
        .count();
    ensure_eq!(len_count, 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_index_and_for_of() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "abc";
const first = word[0];
let joined = "";
for (let ch: string of word) {
  joined = joined + ch;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Index { .. }))
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_math_abs_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = -5;
const positive = Math.abs(value);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericAbs { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_math_rounding_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = 5.5;
const floor = Math.floor(value);
const ceil = Math.ceil(value);
const round = Math.round(value);
const trunc = Math.trunc(value);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        NumericRoundOp::Floor,
        NumericRoundOp::Ceil,
        NumericRoundOp::Round,
        NumericRoundOp::Trunc,
    ] {
        ensure!(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::NumericRound { op, .. } if op == expected)
            )
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_math_extrema_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const first = 1;
const second = 2;
const highest = Math.max(first, second, 3);
const lowest = Math.min(first, second, -1);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [NumericExtremaOp::Max, NumericExtremaOp::Min] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::NumericExtrema { op, .. } if op == expected)
        ));
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_case_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const lower = word.toLowerCase();
const upper = word.toUpperCase();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::StringCase {
            op: StringCaseOp::Lower,
            ..
        }
    )));
    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::StringCase {
            op: StringCaseOp::Upper,
            ..
        }
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_trim_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = " Smelt ";
const trimmed = word.trim();
const left = word.trimStart();
const right = word.trimEnd();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        StringTrimSide::Both,
        StringTrimSide::Start,
        StringTrimSide::End,
    ] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::StringTrim { side, .. } if side == expected)
        ));
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_string_prefix_suffix_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const starts = word.startsWith("Sm");
const ends = word.endsWith("lt");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [StringAffixOp::StartsWith, StringAffixOp::EndsWith] {
        ensure!(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::StringAffix { op, .. } if op == expected)
            )
        );
    }
    Ok(())
}

#[test]
fn lowers_string_search_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const first = word.indexOf("m");
const last = word.lastIndexOf("t");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [StringSearchOp::Find, StringSearchOp::RFind] {
        ensure!(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::StringSearch { op, .. } if op == expected)
            )
        );
    }
    Ok(())
}

#[test]
fn lowers_string_replace_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "hello hello";
const replaced = word.replace("hello", "hi");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(body.exprs.iter().any(|expr| matches!(
        expr.kind,
        ExprKind::StringReplace {
            op: StringReplaceOp::First,
            ..
        }
    )));
    Ok(())
}

#[test]
fn lowers_string_repeat_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "ha";
const repeated = word.repeat(3);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringRepeat { .. }))
    );
    Ok(())
}

#[test]
fn lowers_string_char_at_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const char = word.charAt(1);
const code = word.charCodeAt(2);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringCharAt { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringCharCodeAt { .. }))
    );
    Ok(())
}

#[test]
fn lowers_array_join_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const words: string[] = ["a", "b", "c"];
const joined = words.join("-");
const comma = words.join();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let join_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::StringJoin { .. }))
        .count();
    ensure_eq!(join_count, 2);
    Ok(())
}

#[test]
fn lowers_array_concat_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const left: number[] = [1, 2];
const right: number[] = [3, 4];
const merged = left.concat(right);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListConcat { .. }))
    );
    Ok(())
}

#[test]
fn lowers_array_search_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3, 2];
const first = values.indexOf(2);
const last = values.lastIndexOf(2);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [ListSearchOp::Find, ListSearchOp::RFind] {
        ensure!(
            body.exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::ListSearch { op, .. } if op == expected))
        );
    }
    Ok(())
}

#[test]
fn lowers_array_and_string_slice_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3, 4];
const allValues = values.slice();
const tailValues = values.slice(1);
const midValues = values.slice(1, 3);
const word = "smelting";
const allText = word.slice();
const tailText = word.slice(1);
const midText = word.slice(1, 4);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let list_slices = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListSlice { .. }))
        .count();
    let string_slices = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::StringSlice { .. }))
        .count();
    ensure_eq!(list_slices, 3);
    ensure_eq!(string_slices, 3);
    Ok(())
}

#[test]
fn lowers_array_push_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
let values: number[] = [1, 2];
values.push(3);
const length = values.push(4);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let pushes = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListPush { .. }))
        .count();
    ensure_eq!(pushes, 2);
    Ok(())
}

#[test]
fn lowers_array_reverse_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
let values: number[] = [1, 2];
values.reverse();
const reversed = values.reverse();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let reverses = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListReverse { .. }))
        .count();
    ensure_eq!(reverses, 2);
    Ok(())
}

#[test]
fn lowers_array_pop_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
let values: number[] = [1, 2];
values.pop();
const item = values.pop();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let pops = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListPop { .. }))
        .count();
    ensure_eq!(pops, 2);
    Ok(())
}

#[test]
fn lowers_array_shift_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
let values: string[] = ["a", "b"];
values.shift();
const item = values.shift();
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    let shifts = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListShift { .. }))
        .count();
    ensure_eq!(shifts, 2);
    Ok(())
}

#[test]
fn rejects_unsupported_array_push_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let wrong_type = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.push("x");
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&wrong_type, "argument must match")?;

    let mut ctx = HirCtx::new();
    let too_many = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.push(3, 4);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&too_many, "exactly one item argument")
}

#[test]
fn rejects_unsupported_array_pop_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.pop(0);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "requires no arguments")
}

#[test]
fn rejects_unsupported_array_shift_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
let values: number[] = [1, 2];
values.shift(0);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "requires no arguments")
}

#[test]
fn rejects_unsupported_slice_argument_forms() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let negative = lowering_errors(
        ts!(r#"
const values: number[] = [1, 2, 3];
const bad = values.slice(-1);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&negative, "negative indexes")?;

    let mut ctx = HirCtx::new();
    let too_many = lowering_errors(
        ts!(r#"
const values: number[] = [1, 2, 3];
const bad = values.slice(0, 1, 2);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&too_many, "omitted, start, and end arguments")
}

#[test]
fn lowers_array_is_array_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3];
const yes = Array.isArray(values);
const no = Array.isArray(1);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(true))))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Bool(false))))
    );
    Ok(())
}

#[test]
fn lowers_math_sqrt_pow_sign() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = 4;
const root = Math.sqrt(value);
const cubeRoot = Math.cbrt(value);
const signed = Math.sign(value);
const raised = Math.pow(value, 2);
const distance = Math.hypot(value, 3);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        NumericUnaryFuncOp::Sqrt,
        NumericUnaryFuncOp::Cbrt,
        NumericUnaryFuncOp::Sign,
    ] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::NumericUnaryFunc { op, .. } if op == expected)
        ));
    }
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericPow { .. }))
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericHypot { .. }))
    );
    Ok(())
}

#[test]
fn lowers_number_predicate_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const value = 4;
const finite = Number.isFinite(value);
const nan = Number.isNaN(value);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [NumericPredicateOp::IsFinite, NumericPredicateOp::IsNaN] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::NumericPredicate { op, .. } if op == expected)
        ));
    }
    Ok(())
}

#[test]
fn lowers_object_projection_methods() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const mapping: Record<string, number> = { a: 1, b: 2 };
const keys = Object.keys(mapping);
const values = Object.values(mapping);
const entries = Object.entries(mapping);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    for expected in [
        DictProjectionOp::Keys,
        DictProjectionOp::Values,
        DictProjectionOp::Entries,
    ] {
        ensure!(body.exprs.iter().any(
            |expr| matches!(expr.kind, ExprKind::DictProjection { op, .. } if op == expected)
        ));
    }
    Ok(())
}

#[test]
fn lowers_string_includes_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "Smelt";
const has = word.includes("mel");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringContains { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_includes_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const values: number[] = [1, 2, 3];
const has = values.includes(2);
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListContains { .. })),
        "array includes did not lower to ListContains"
    );
    Ok(())
}

#[test]
fn lowers_string_split_method() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const word = "a,b,c";
const parts = word.split(",");
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::StringSplit { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn rejects_unknown_identifier() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(ts!("console.log(x);"), &mut ctx)?;
    assert_unsupported_ts(&errors, "unresolved identifier")
}

#[test]
fn formats_compact_hir() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("let count = 42;
console.log(count);
"),
        &mut ctx,
    )?;

    let output = smelt_hir::format_compact(&ctx.krate, &[("sample.ts".to_owned(), module_id)]);

    ensure_eq!(
        output,
        "module sample.ts (ModuleId(0))\n  body BodyId(0)\n  locals\n    %0 let count: Float\n  exprs\n    #0: Float = 42.0\n    #1: Float = %0\n    #2: None = @0(console_log)\n    #3: None = call #2(#1)\n  stmts\n    s0: let %0: Float = #0\n    s1: #3\n\ninterned types\n  t0 = Float\n  t1 = None\n"
    );
    Ok(())
}

#[test]
fn normalizes_camel_case() -> Result<(), String> {
    ensure_eq!(camel_to_snake("myFunction"), "my_function");
    ensure_eq!(camel_to_snake("URLParser"), "url_parser");
    ensure_eq!(camel_to_snake("IPAddr"), "ip_addr");
    ensure_eq!(camel_to_snake("_internal"), "_internal");
    Ok(())
}

#[test]
fn lowers_function_declaration_and_direct_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("function add(a: number, b: number): number {
  return a + b;
}
const result = add(2, 3);
console.log(result);
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;

    ensure_eq!(module.items.len(), 1);
    ensure_eq!(ctx.krate.items.len(), 2);
    ensure_eq!(ctx.krate.bodies.len(), 2);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_if_else_while_and_for_of_to_hir() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("let count = 0;
if (count < 10) {
  console.log(count);
} else {
  console.log(count);
}
while (count < 10) {
  break;
}
for (let item: number of count) {
  continue;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::If { .. }))
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::While { .. }))
    );
    ensure!(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. }))
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_try_catch_finally_to_hir() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("try {
  throw 'x';
} catch (error) {
  console.log(error);
} finally {
  console.log('done');
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;

    let Some(Stmt::TryCatch {
        body: try_body,
        catch_binding: Some(_),
        catch_body: Some(catch_body),
        finally_body: Some(finally_body),
    }) = body
        .stmts
        .iter()
        .find(|stmt| matches!(stmt, Stmt::TryCatch { .. }))
    else {
        return Err("expected try/catch/finally to lower to HIR".to_owned());
    };
    let try_block = body
        .blocks
        .get(
            usize::try_from(try_body.0)
                .map_err(|err| format!("try block id {try_body:?} does not fit in usize: {err}"))?,
        )
        .ok_or_else(|| format!("missing try block {try_body:?}"))?;
    let catch_block =
        body.blocks
            .get(usize::try_from(catch_body.0).map_err(|err| {
                format!("catch block id {catch_body:?} does not fit in usize: {err}")
            })?)
            .ok_or_else(|| format!("missing catch block {catch_body:?}"))?;
    let finally_block = body
        .blocks
        .get(usize::try_from(finally_body.0).map_err(|err| {
            format!("finally block id {finally_body:?} does not fit in usize: {err}")
        })?)
        .ok_or_else(|| format!("missing finally block {finally_body:?}"))?;
    ensure!(try_block.stmts.iter().any(|stmt| {
        usize::try_from(stmt.0)
            .is_ok_and(|stmt_index| matches!(body.stmts.get(stmt_index), Some(Stmt::Throw(_))))
    }));
    ensure!(!catch_block.stmts.is_empty());
    ensure!(!finally_block.stmts.is_empty());
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn rejects_missing_implemented_interface_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("interface Named { name: string; }
class User implements Named {
  constructor() {}
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "field `name`")
}

#[test]
fn rejects_implemented_method_signature_mismatch() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("interface Named { label(prefix: string): string; }
class User implements Named {
  label(prefix: number): string { return \"x\"; }
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "mismatched signature")
}

#[test]
fn lowers_interface_inheritance_into_shape_requirements() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("interface Entity { id: string; }
interface Named extends Entity { name: string; }
class User implements Named {
  id: string;
  name: string;
  constructor(id: string, name: string) {
    this.id = id;
    this.name = name;
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn rejects_missing_inherited_interface_field() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("interface Entity { id: string; }
interface Named extends Entity { name: string; }
class User implements Named {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "field `id`")
}

#[test]
fn lowers_literal_computed_property_names() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("interface Entity { [\"id\"]: string; }
class User implements Entity {
  [\"id\"]: string;
  constructor(id: string) {
    this.id = id;
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn rejects_dynamic_computed_property_names() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("const key = \"id\";
class User {
  [key]: string;
  constructor() {}
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "dynamic computed property")
}

#[test]
fn optional_interface_fields_may_be_absent() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("interface Named { name?: string; }
class User implements Named {
  constructor() {}
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn required_fields_satisfy_optional_interface_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("interface Named { name?: string; }
class User implements Named {
  name: string;
  constructor(name: string) {
    this.name = name;
  }
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn rejects_optional_class_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("class User {
  name?: string;
  constructor() {}
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "optional class fields")
}

#[test]
fn rejects_generic_classes_and_interfaces() -> Result<(), String> {
    let mut class_ctx = HirCtx::new();
    let class_errors = lowering_errors(
        ts!("class Box<T> {
  value: T;
  constructor(value: T) {
    this.value = value;
  }
}
"),
        &mut class_ctx,
    )?;
    assert_unsupported_ts(&class_errors, "generic classes")?;

    let mut interface_ctx = HirCtx::new();
    let interface_errors = lowering_errors(
        ts!("interface Box<T> {
  value: T;
}
"),
        &mut interface_ctx,
    )?;
    assert_unsupported_ts(&interface_errors, "generic interfaces")
}

#[test]
fn rejects_static_members() -> Result<(), String> {
    let mut field_ctx = HirCtx::new();
    let field_errors = lowering_errors(
        ts!("class User {
  static role: string;
  constructor() {}
}
"),
        &mut field_ctx,
    )?;
    assert_unsupported_ts(&field_errors, "static fields")?;

    let mut method_ctx = HirCtx::new();
    let method_errors = lowering_errors(
        ts!("class User {
  static role(): string { return \"admin\"; }
}
"),
        &mut method_ctx,
    )?;
    assert_unsupported_ts(&method_errors, "static methods")
}

#[test]
fn rejects_getters_setters_decorators_and_abstract_classes() -> Result<(), String> {
    let mut getter_ctx = HirCtx::new();
    let getter_errors = lowering_errors(
        ts!("class User {
  get name(): string { return \"Ada\"; }
}
"),
        &mut getter_ctx,
    )?;
    assert_unsupported_ts(&getter_errors, "getters and setters")?;

    let mut decorator_ctx = HirCtx::new();
    let decorator_errors = lowering_errors(
        ts!("@sealed
class User {
  constructor() {}
}
"),
        &mut decorator_ctx,
    )?;
    assert_unsupported_ts(&decorator_errors, "decorators")?;

    let mut abstract_ctx = HirCtx::new();
    let abstract_errors = lowering_errors(
        ts!("abstract class User {
  abstract name(): string;
}
"),
        &mut abstract_ctx,
    )?;
    assert_unsupported_ts(&abstract_errors, "abstract classes")
}

#[test]
fn lowers_literal_switch_to_hir_match() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(
            "function label(status: \"pending\" | \"approved\" | \"rejected\"): string {
  switch (status) {
    case \"pending\":
      return \"Waiting\";
    case \"approved\":
      return \"Approved\";
    case \"rejected\":
      return \"Rejected\";
  }
}
const result = label(\"approved\");
console.log(result);
"
        ),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;

    let Some(Stmt::Match { arms, default, .. }) = body
        .stmts
        .iter()
        .find(|stmt| matches!(stmt, Stmt::Match { .. }))
    else {
        return Err("expected switch to lower to HIR match".to_owned());
    };
    ensure_eq!(arms.len(), 3);
    ensure!(default.is_none());
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn rejects_coercive_equality() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("function same(a: number, b: number): boolean {
  return a == b;
}
"),
        &mut ctx,
    )?;

    assert_unsupported_ts(&errors, "coercive equality")
}

#[test]
fn rejects_untyped_for_of_binding() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("let values = 1;
for (let item of values) {
  continue;
}
"),
        &mut ctx,
    )?;

    assert_unsupported_ts(&errors, "explicit type annotations")
}

#[test]
fn lowers_async_functions_and_await_to_hir() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function load(value: number): Promise<number> {
  return value;
}

async function main(): Promise<number> {
  return await load(1);
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;

    ensure_eq!(module.items.len(), 2);
    let load = function_item(&ctx, module, 0)?;
    ensure!(load.is_async);
    ensure!(matches!(
        ctx.krate.types.get(load.return_ty),
        Some(Type::Future(_))
    ));

    let main = function_item(&ctx, module, 1)?;
    let body = function_body(&ctx, main)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Await(_)))
    );
    let machine = body
        .async_state_machine
        .as_ref()
        .ok_or_else(|| "async body should have state-machine metadata".to_owned())?;
    ensure_eq!(machine.states.len(), 2);
    ensure_eq!(machine.suspensions.len(), 1);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_promise_all_to_async_runtime_op() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function lift(value: number): Promise<number> {
  return value;
}

async function main(): Promise<[number, number]> {
  return await Promise.all([lift(1), lift(2)]);
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let main = function_item(&ctx, module, 1)?;
    let body = function_body(&ctx, main)?;

    ensure!(body.exprs.iter().any(|expr| {
        matches!(
            expr.kind,
            ExprKind::AsyncOp {
                op: smelt_hir::AsyncOp::All,
                ..
            }
        )
    }));
    Ok(())
}

#[test]
fn lowers_promise_race_all_settled_and_timer_shim() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("async function lift(value: number): Promise<number> {
  await setTimeout(0);
  return value;
}

async function race(): Promise<number> {
  return await Promise.race([lift(1), lift(2)]);
}

async function settled(): Promise<[number, number]> {
  return await Promise.allSettled([lift(1), lift(2)]);
}
"),
        &mut ctx,
    )?;
    Ok(())
}

#[test]
fn lowers_fetch_to_async_http_get_text() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!("async function load(): Promise<string> {
  return await fetch(\"https://example.com\");
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let load = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, load)?;

    ensure!(body.exprs.iter().any(|expr| {
        matches!(
            expr.kind,
            ExprKind::AsyncOp {
                op: smelt_hir::AsyncOp::HttpGetText,
                ..
            }
        )
    }));
    Ok(())
}

#[test]
fn rejects_await_outside_async_function() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("function read(): number {
  return await 1;
}
"),
        &mut ctx,
    )?;

    let error = errors
        .first()
        .ok_or_else(|| "expected at least one lowering error".to_owned())?;
    ensure_eq!(error.code, "smelt::parse-error");
    ensure!(error.message.contains("await"));
    Ok(())
}

#[test]
fn rejects_async_functions_without_promise_return_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!("async function load(): number {
  return 1;
}
"),
        &mut ctx,
    )?;

    assert_unsupported_ts(&errors, "Promise<T>")
}

#[test]
fn rejects_switch_fallthrough_until_it_is_modeled() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(
            "function label(status: \"pending\" | \"approved\"): string {
  switch (status) {
    case \"pending\":
      const waiting = \"waiting\";
    case \"approved\":
      return \"Approved\";
  }
}
"
        ),
        &mut ctx,
    )?;

    ensure!(
        errors
            .iter()
            .any(|error| error.message.contains("switch fallthrough")),
        "expected switch fallthrough error, got {errors:?}"
    );
    Ok(())
}

#[test]
fn lowers_template_literal_to_string_concat() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let _module_id = lower_ok(
        ts!("const name: string = \"world\";\nconst msg: string = `Hello ${name}!`;"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn accepts_import_and_export_declarations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let _module_id = lower_ok(
        ts!("import { foo } from './foo';\nexport function bar(): number { return 1; }"),
        &mut ctx,
    )?;
    Ok(())
}
