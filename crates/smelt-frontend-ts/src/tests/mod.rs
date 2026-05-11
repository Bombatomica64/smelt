//! Tests for TypeScript frontend lowering.

use super::*;
use smelt_hir::{
    BinOp, DictProjectionOp, ExprKind, FileId, Function, Item, ListCallbackOp, ListSearchOp,
    Literal, ModuleId, NumericExtremaOp, NumericPredicateOp, NumericRoundOp, NumericUnaryFuncOp,
    PrimitiveCastOp, RegexMatchOp, SetProjectionOp, SetRemoveOp, Stmt, StringAffixOp, StringCaseOp,
    StringPadOp, StringReplaceOp, StringSearchOp, StringTrimSide, Type,
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

/// Lower TypeScript source with a source path and fail readably on error.
fn lower_path_ok(source: &str, path: &str, ctx: &mut HirCtx) -> Result<ModuleId, String> {
    to_hir_with_path(source, FileId(0), path, ctx)
        .map_err(|errors| format!("unexpected lowering failure: {errors:?}"))
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

/// Return whether a lowered capture-free callback references a parameter index.
fn callback_has_param(callback: &smelt_hir::CallbackExpr, target: usize) -> bool {
    match &callback.kind {
        smelt_hir::CallbackExprKind::Param(index) => *index == target,
        smelt_hir::CallbackExprKind::Literal(_) => false,
        smelt_hir::CallbackExprKind::Unary { operand, .. } => callback_has_param(operand, target),
        smelt_hir::CallbackExprKind::Binary { lhs, rhs, .. } => {
            callback_has_param(lhs, target) || callback_has_param(rhs, target)
        }
    }
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

mod part01_tests;
mod part02_tests;
mod part03_tests;
mod part04_tests;
mod part05_tests;
mod part06_tests;
