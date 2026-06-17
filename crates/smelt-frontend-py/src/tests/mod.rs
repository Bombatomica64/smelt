//! Unit tests for the Python frontend.

use crate::{HirCtx, SmeltError, to_hir, to_hir_with_path};
use smelt_hir::{
    AsyncOp, BinOp, Body, BodyId, BoolFoldOp, DictProjectionOp, ExprKind, FileId, Item, ItemId,
    Language, ListCallbackOp,
    Literal, Module, ModuleId, NumericExtremaOp, NumericPredicateOp, NumericRoundOp,
    NumericUnaryFuncOp, Pattern, PatternId, PrimitiveCastOp, RegexMatchOp, SetBinaryOp,
    SetProjectionOp, SetRelationOp, SetRemoveOp, Stmt, StringAffixOp, StringCaseOp,
    StringPredicateOp, StringReplaceOp, StringSearchOp, StringTrimSide, Symbol, Type,
};
use std::convert::TryFrom;

type TestResult = Result<(), String>;

/// Marks a test fixture string as Python source code.
macro_rules! py {
    ($source:literal $(,)?) => {
        $source
    };
}

/// Lowers `source` into HIR and returns the module ID.
fn lower_module(source: &str, ctx: &mut HirCtx) -> Result<ModuleId, String> {
    to_hir(source, FileId(0), ctx)
        .map_err(|errors| format!("expected successful lowering, got {errors:?}"))
}

/// Lowers `source` and returns the diagnostics produced by the frontend.
fn lower_errors(source: &str, ctx: &mut HirCtx) -> Result<Vec<SmeltError>, String> {
    match to_hir(source, FileId(0), ctx) {
        Ok(module_id) => Err(format!(
            "expected lowering to fail, got module {module_id:?}"
        )),
        Err(errors) => Ok(errors),
    }
}

/// Lowers `source` from a concrete path and returns the module ID.
fn lower_path_module(source: &str, path: &str, ctx: &mut HirCtx) -> Result<ModuleId, String> {
    to_hir_with_path(source, FileId(0), path, ctx)
        .map_err(|errors| format!("expected successful lowering, got {errors:?}"))
}

/// Returns the first diagnostic from `errors`.
fn first_error(errors: &[SmeltError]) -> Result<&SmeltError, String> {
    errors
        .first()
        .ok_or_else(|| "expected at least one diagnostic".to_owned())
}

/// Fails the test if `condition` is false.
fn ensure(condition: bool, message: impl Into<String>) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

/// Fails the test if `left` and `right` are not equal.
fn ensure_eq<T>(left: &T, right: &T, context: &str) -> Result<(), String>
where
    T: PartialEq + std::fmt::Debug,
{
    if left == right {
        Ok(())
    } else {
        Err(format!("{context}: left={left:?}, right={right:?}"))
    }
}

/// Looks up a module by ID.
fn module(ctx: &HirCtx, module_id: ModuleId) -> Result<&Module, String> {
    let idx = usize::try_from(module_id.0)
        .map_err(|error| format!("missing module {module_id:?}: {error}"))?;
    ctx.krate
        .modules
        .get(idx)
        .ok_or_else(|| format!("missing module {module_id:?}"))
}

/// Looks up an item by ID.
fn item(ctx: &HirCtx, item_id: ItemId) -> Result<&Item, String> {
    let idx =
        usize::try_from(item_id.0).map_err(|error| format!("missing item {item_id:?}: {error}"))?;
    ctx.krate
        .items
        .get(idx)
        .ok_or_else(|| format!("missing item {item_id:?}"))
}

/// Looks up a body by ID.
fn body(ctx: &HirCtx, body_id: BodyId) -> Result<&Body, String> {
    let idx =
        usize::try_from(body_id.0).map_err(|error| format!("missing body {body_id:?}: {error}"))?;
    ctx.krate
        .bodies
        .get(idx)
        .ok_or_else(|| format!("missing body {body_id:?}"))
}

/// Looks up a pattern by ID within `body`.
fn pattern(body: &Body, pattern_id: PatternId) -> Result<&Pattern, String> {
    let idx = usize::try_from(pattern_id.0)
        .map_err(|error| format!("missing pattern {pattern_id:?}: {error}"))?;
    body.patterns
        .get(idx)
        .ok_or_else(|| format!("missing pattern {pattern_id:?}"))
}

/// Resolves a symbol back to its interned name.
fn symbol(ctx: &HirCtx, symbol: Symbol) -> Result<&str, String> {
    ctx.krate
        .symbols
        .get(symbol)
        .ok_or_else(|| format!("missing symbol {symbol:?}"))
}

/// Return whether a closure callback uses a captured local through its normal body CFG.
fn closure_cfg_has_capture(ctx: &HirCtx, body: &Body, callback: smelt_hir::ExprId) -> bool {
    let Ok(callback_index) = usize::try_from(callback.0) else {
        return false;
    };
    let Some(expr) = body.exprs.get(callback_index) else {
        return false;
    };
    let ExprKind::Closure(closure) = &expr.kind else {
        return false;
    };
    let Ok(body_index) = usize::try_from(closure.body.0) else {
        return false;
    };
    let Some(closure_body) = ctx.krate.bodies.get(body_index) else {
        return false;
    };
    closure
        .captures
        .iter()
        .all(|capture| capture.body_local.is_some())
        && !closure.captures.is_empty()
        && closure_body
            .blocks
            .first()
            .is_some_and(|root| root.tail.is_some())
}

mod basic_tests;
mod builtins_sets_tests;
mod class_tests;
mod collections_a_tests;
mod collections_b_tests;
mod collections_reject_a_tests;
mod collections_reject_b_tests;
mod packages_tests;
mod pytest_tests;
