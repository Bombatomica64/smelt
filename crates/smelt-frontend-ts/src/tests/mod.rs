//! Tests for TypeScript frontend lowering.

use super::*;
use smelt_hir::{
    BinOp, DatePart, DictProjectionOp, ExprKind, FileId, Function, Item, ListCallbackOp,
    ListProjectionOp, ListSearchOp, Literal, ModuleId, NumericExtremaOp, NumericPredicateOp,
    NumericRoundOp, NumericUnaryFuncOp, PrimitiveCastOp, SetProjectionOp, SetRemoveOp, Stmt,
    StringAffixOp, StringCaseOp, StringPadOp, StringReplaceOp, StringSearchOp, StringTrimSide,
    Type,
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

/// Return whether a closure expression's normal body CFG references a parameter index.
fn closure_callback_has_param(
    ctx: &HirCtx,
    body: &smelt_hir::Body,
    callback: smelt_hir::ExprId,
    target: usize,
) -> bool {
    let Some(ExprKind::Closure(closure)) =
        body.exprs.get(callback.0 as usize).map(|expr| &expr.kind)
    else {
        return false;
    };
    let Some(param) = closure.params.get(target) else {
        return false;
    };
    ctx.krate
        .bodies
        .get(closure.body.0 as usize)
        .is_some_and(|closure_body| {
            closure_body
                .exprs
                .iter()
                .any(|expr| matches!(expr.kind, ExprKind::Local(local) if local == param.local))
        })
}

/// Return whether a closure expression points at a populated normal body CFG.
fn closure_has_cfg_body(ctx: &HirCtx, closure: &smelt_hir::ClosureExpr) -> bool {
    ctx.krate
        .bodies
        .get(closure.body.0 as usize)
        .is_some_and(|body| !body.blocks.is_empty())
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

/// Find a module's function item by its source name.
///
/// Modules that declare types (interfaces, aliases) before functions cannot be
/// indexed positionally for the function, so this resolves by interned name.
fn named_function_item<'a>(
    ctx: &'a HirCtx,
    module: &'a smelt_hir::Module,
    name: &str,
) -> Result<&'a Function, String> {
    for item_id in &module.items {
        let item_index = usize::try_from(item_id.0)
            .map_err(|err| format!("item id {item_id:?} does not fit in usize: {err}"))?;
        if let Some(Item::Function(function)) = ctx.krate.items.get(item_index)
            && ctx.krate.symbols.get(function.name) == Some(name)
        {
            return Ok(function);
        }
    }
    Err(format!("missing function item named `{name}`"))
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

/// Assert that the first error contains `needle` and carries the given category.
fn assert_category(
    errors: &[SmeltError],
    needle: &str,
    category: smelt_stdlib::DiagnosticCategory,
) -> Result<(), String> {
    let error = errors
        .first()
        .ok_or_else(|| "expected at least one lowering error".to_owned())?;
    ensure!(error.message.contains(needle));
    ensure_eq!(error.category, category);
    Ok(())
}

mod category_tests;
mod part01_tests;
mod part02_tests;
mod part03_tests;
mod constructor_function_tests;
mod part04_tests;
mod part05_tests;
mod part06_tests;
mod specialization_tests;
mod part07_tests;
mod estk6_coverage_tests;
mod estk_transpile_gate_tests;
mod class_module_tests;
mod enum_switch_tests;
mod array_literal_expr_tests;
mod estk_mir_gate_tests;
mod module_globals_tests;
mod fetch_types_tests;
mod host_module_tests;
mod host_override_tests;
