//! Unit tests for the Python frontend.

use crate::{FrontendOptions, HirCtx, SmeltError, to_hir, to_hir_with_options, to_hir_with_path};
use smelt_hir::{
    AsyncOp, BinOp, Body, BodyId, BoolFoldOp, ClassProtocol, DictProjectionOp, ExprKind, FileId,
    Item, ItemId, Language, Literal, Module, ModuleId, NumericExtremaOp, NumericPredicateOp,
    NumericRoundOp, NumericUnaryFuncOp, Pattern, PatternId, PrimitiveCastOp, RegexMatchOp,
    SetBinaryOp, SetProjectionOp, SetRelationOp, SetRemoveOp, Stmt, StringAffixOp, StringCaseOp,
    StringPredicateOp, StringReplaceOp, StringSearchOp, StringTrimSide, Symbol, Type,
};

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

/// Fails unless the function named `name` lowered to exactly `expected`, the
/// text of [`smelt_hir::function_signature`].
fn ensure_signature(ctx: &HirCtx, name: &str, expected: &str) -> TestResult {
    let actual = smelt_hir::function_signature(&ctx.krate, name)
        .ok_or_else(|| format!("missing function item named `{name}`"))?;
    if actual == expected {
        Ok(())
    } else {
        Err(format!("signature of `{name}`:\n  actual: {actual}\nexpected: {expected}"))
    }
}

/// Fails unless class `name` has exactly the shape `expected`, written
/// `Name<T>: Base { field: Type, .. }` with fields in declaration order.
fn ensure_class_shape(ctx: &HirCtx, name: &str, expected: &str) -> TestResult {
    let class = class_named(ctx, name)?;
    let render = |sym| symbol(ctx, sym).map(str::to_owned);
    let params = class
        .type_params
        .iter()
        .map(|param| render(param.name))
        .collect::<Result<Vec<_>, _>>()?;
    let generics = if params.is_empty() {
        String::new()
    } else {
        format!("<{}>", params.join(", "))
    };
    let base = class
        .base
        .map(|base| render(base).map(|rendered| format!(": {rendered}")))
        .transpose()?
        .unwrap_or_default();
    let fields = class
        .fields
        .iter()
        .map(|field| {
            render(field.name).map(|field_name| {
                format!("{field_name}: {}", smelt_hir::type_display(&ctx.krate, field.ty))
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let actual = format!("{name}{generics}{base} {{ {} }}", fields.join(", "));
    ensure_eq(&actual.as_str(), &expected, "class shape")
}

/// The class item named `name`.
fn class_named<'a>(ctx: &'a HirCtx, name: &str) -> Result<&'a smelt_hir::Class, String> {
    ctx.krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Class(class) if ctx.krate.symbols.get(class.name) == Some(name) => Some(class),
            _ => None,
        })
        .ok_or_else(|| format!("missing class `{name}`"))
}

/// The function item named `name`.
fn function_named<'a>(ctx: &'a HirCtx, name: &str) -> Result<&'a smelt_hir::Function, String> {
    ctx.krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function) if ctx.krate.symbols.get(function.name) == Some(name) => {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| format!("missing function `{name}`"))
}

/// Fails unless the abstract methods of class `name` are exactly `expected`.
fn ensure_abstract_methods(ctx: &HirCtx, name: &str, expected: &[&str]) -> TestResult {
    let actual = class_named(ctx, name)?
        .abstract_methods
        .iter()
        .map(|method| symbol(ctx, method.name))
        .collect::<Result<Vec<_>, _>>()?;
    ensure_eq(&actual.as_slice(), &expected, "abstract methods")
}

/// Fails unless function `name` is owned by a class as a static method.
fn ensure_owner_is_static(ctx: &HirCtx, name: &str) -> TestResult {
    ensure(
        matches!(
            function_named(ctx, name)?.owner,
            smelt_hir::FunctionOwner::ClassStaticMethod { .. }
        ),
        format!("`{name}` is not a static method"),
    )
}

/// Whether some module has a global binding called `name`.
fn has_module_binding(ctx: &HirCtx, name: &str) -> bool {
    ctx.krate.items.iter().any(|item| match item {
        Item::MutableGlobal(global) => ctx.krate.symbols.get(global.name) == Some(name),
        _ => false,
    })
}

/// The rendered receiver types of the method calls in function `name`, in
/// expression order.
fn method_receiver_types(ctx: &HirCtx, name: &str) -> Result<Vec<String>, String> {
    let function = function_named(ctx, name)?;
    let body = body(ctx, function.body.ok_or("expected a body")?)?;
    body.exprs
        .iter()
        .filter_map(|expr| match expr.kind {
            ExprKind::Method { receiver, .. } => Some(receiver),
            _ => None,
        })
        .map(|receiver| {
            let index = usize::try_from(receiver.0).map_err(|error| error.to_string())?;
            let receiver_expr = body.exprs.get(index).ok_or("missing receiver expression")?;
            Ok(smelt_hir::type_display(&ctx.krate, receiver_expr.ty))
        })
        .collect()
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

/// Every body holding a module's top-level code, in order: the module body,
/// then the initializer of each module global.
///
/// A typed top-level binding (`total: int = sum(xs)`) is a module global
/// (`Item::MutableGlobal`) because functions and importers read it; its value
/// lowers into the global's nullary initializer, not into the module body.
/// A test asking whether the module's code lowered an expression asks it of
/// all of these bodies.
struct ModuleCode<'a> {
    /// The module body first, then the initializer bodies.
    bodies: Vec<&'a Body>,
}

impl<'a> ModuleCode<'a> {
    /// Every expression of the module's code.
    fn exprs(&self) -> impl Iterator<Item = &'a smelt_hir::Expr> + '_ {
        self.bodies.iter().flat_map(|body| body.exprs.iter())
    }

    /// Every statement of the module's code.
    fn stmts(&self) -> impl Iterator<Item = &'a Stmt> + '_ {
        self.bodies.iter().flat_map(|body| body.stmts.iter())
    }
}

/// The [`ModuleCode`] of `module`.
fn module_code<'a>(ctx: &'a HirCtx, module: &Module) -> Result<ModuleCode<'a>, String> {
    let mut bodies = vec![body(ctx, module.body.ok_or("expected module body")?)?];
    for item_id in &module.items {
        let Item::MutableGlobal(global) = item(ctx, *item_id)? else {
            continue;
        };
        let smelt_hir::MutableGlobalInit::Initializer(init) = global.init else {
            continue;
        };
        if let Item::Function(function) = item(ctx, init)?
            && let Some(init_body) = function.body
        {
            bodies.push(body(ctx, init_body)?);
        }
    }
    Ok(ModuleCode { bodies })
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
mod category_tests;
mod class_tests;
mod collections_a_tests;
mod collections_b_tests;
mod collections_reject_a_tests;
mod collections_reject_b_tests;
mod lang_features_tests;
mod name_resolution_tests;
mod packages_tests;
mod fn_scope_tests;
mod pytest_tests;
mod specialization_tests;
#[cfg(feature = "ty")]
mod ty_resolution_tests;
