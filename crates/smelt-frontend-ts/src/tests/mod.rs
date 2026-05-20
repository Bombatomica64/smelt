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

/// Return whether a lowered callback references a parameter index.
fn callback_has_param(callback: &smelt_hir::CallbackExpr, target: usize) -> bool {
    match &callback.kind {
        smelt_hir::CallbackExprKind::Param(index) => *index == target,
        smelt_hir::CallbackExprKind::Capture(_) => false,
        smelt_hir::CallbackExprKind::Function(_) => false,
        smelt_hir::CallbackExprKind::FunctionTableLookup { key, .. } => {
            callback_has_param(key, target)
        }
        smelt_hir::CallbackExprKind::AssignCapture { value, .. } => {
            callback_has_param(value, target)
        }
        smelt_hir::CallbackExprKind::Literal(_) => false,
        smelt_hir::CallbackExprKind::ListLit(items) => {
            items.iter().any(|item| callback_has_param(item, target))
        }
        smelt_hir::CallbackExprKind::Sequence { effects, result } => {
            effects
                .iter()
                .any(|effect| callback_has_param(effect, target))
                || callback_has_param(result, target)
        }
        smelt_hir::CallbackExprKind::DictLit(entries) => entries
            .iter()
            .any(|(_, value)| callback_has_param(value, target)),
        smelt_hir::CallbackExprKind::Throw { message } => message
            .as_deref()
            .is_some_and(|message| callback_has_param(message, target)),
        smelt_hir::CallbackExprKind::Index { receiver, .. }
        | smelt_hir::CallbackExprKind::Field { receiver, .. }
        | smelt_hir::CallbackExprKind::HasField { receiver, .. }
        | smelt_hir::CallbackExprKind::FieldTruthy { receiver, .. } => {
            callback_has_param(receiver, target)
        }
        smelt_hir::CallbackExprKind::DynamicIndex { receiver, index } => {
            callback_has_param(receiver, target) || callback_has_param(index, target)
        }
        smelt_hir::CallbackExprKind::HasDynamicField { receiver, field } => {
            callback_has_param(receiver, target) || callback_has_param(field, target)
        }
        smelt_hir::CallbackExprKind::Unary { operand, .. } => callback_has_param(operand, target),
        smelt_hir::CallbackExprKind::Binary { lhs, rhs, .. } => {
            callback_has_param(lhs, target) || callback_has_param(rhs, target)
        }
        smelt_hir::CallbackExprKind::UnknownIs { value, .. } => callback_has_param(value, target),
        smelt_hir::CallbackExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => {
            callback_has_param(cond, target)
                || callback_has_param(then_expr, target)
                || callback_has_param(else_expr, target)
        }
        smelt_hir::CallbackExprKind::Call { callee, args } => {
            callback_has_param(callee, target)
                || args.iter().any(|arg| callback_has_param(&arg.expr, target))
        }
        smelt_hir::CallbackExprKind::MethodCall { receiver, args, .. } => {
            callback_has_param(receiver, target)
                || args.iter().any(|arg| callback_has_param(&arg.expr, target))
        }
    }
}

/// Return whether a closure expression's callback body references a parameter index.
fn closure_callback_has_param(
    body: &smelt_hir::Body,
    callback: smelt_hir::ExprId,
    target: usize,
) -> bool {
    closure_callback_body(body, callback)
        .is_some_and(|callback| callback_has_param(callback, target))
}

/// Return whether a lowered callback captures an enclosing local.
fn callback_has_capture(callback: &smelt_hir::CallbackExpr) -> bool {
    match &callback.kind {
        smelt_hir::CallbackExprKind::Capture(_) => true,
        smelt_hir::CallbackExprKind::AssignCapture { .. } => true,
        smelt_hir::CallbackExprKind::Param(_)
        | smelt_hir::CallbackExprKind::Function(_)
        | smelt_hir::CallbackExprKind::Literal(_) => false,
        smelt_hir::CallbackExprKind::FunctionTableLookup { key, .. } => callback_has_capture(key),
        smelt_hir::CallbackExprKind::ListLit(items) => items.iter().any(callback_has_capture),
        smelt_hir::CallbackExprKind::Sequence { effects, result } => {
            effects.iter().any(callback_has_capture) || callback_has_capture(result)
        }
        smelt_hir::CallbackExprKind::DictLit(entries) => {
            entries.iter().any(|(_, value)| callback_has_capture(value))
        }
        smelt_hir::CallbackExprKind::Throw { message } => {
            message.as_deref().is_some_and(callback_has_capture)
        }
        smelt_hir::CallbackExprKind::Index { receiver, .. }
        | smelt_hir::CallbackExprKind::Field { receiver, .. }
        | smelt_hir::CallbackExprKind::HasField { receiver, .. }
        | smelt_hir::CallbackExprKind::FieldTruthy { receiver, .. } => {
            callback_has_capture(receiver)
        }
        smelt_hir::CallbackExprKind::DynamicIndex { receiver, index } => {
            callback_has_capture(receiver) || callback_has_capture(index)
        }
        smelt_hir::CallbackExprKind::HasDynamicField { receiver, field } => {
            callback_has_capture(receiver) || callback_has_capture(field)
        }
        smelt_hir::CallbackExprKind::Unary { operand, .. } => callback_has_capture(operand),
        smelt_hir::CallbackExprKind::Binary { lhs, rhs, .. } => {
            callback_has_capture(lhs) || callback_has_capture(rhs)
        }
        smelt_hir::CallbackExprKind::UnknownIs { value, .. } => callback_has_capture(value),
        smelt_hir::CallbackExprKind::Conditional {
            cond,
            then_expr,
            else_expr,
        } => {
            callback_has_capture(cond)
                || callback_has_capture(then_expr)
                || callback_has_capture(else_expr)
        }
        smelt_hir::CallbackExprKind::Call { callee, args } => {
            callback_has_capture(callee) || args.iter().any(|arg| callback_has_capture(&arg.expr))
        }
        smelt_hir::CallbackExprKind::MethodCall { receiver, args, .. } => {
            callback_has_capture(receiver) || args.iter().any(|arg| callback_has_capture(&arg.expr))
        }
    }
}

/// Return whether a closure expression's callback body captures an enclosing local.
fn closure_callback_has_capture(body: &smelt_hir::Body, callback: smelt_hir::ExprId) -> bool {
    closure_callback_body(body, callback).is_some_and(callback_has_capture)
}

/// Resolve the temporary callback-expression bridge from a closure expression.
fn closure_callback_body(
    body: &smelt_hir::Body,
    callback: smelt_hir::ExprId,
) -> Option<&smelt_hir::CallbackExpr> {
    let expr = body.exprs.get(callback.0 as usize)?;
    let ExprKind::Closure(closure) = &expr.kind else {
        return None;
    };
    closure.callback_body.as_ref()
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
