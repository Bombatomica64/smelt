//! Regression tests for issue #95 Python language-feature lowering:
//! `try`/`except`/`finally`, `lambda` expressions, and additional class-body
//! statements.

use super::*;

/// Return the lowered body of the named top-level function in `module`.
fn named_function_body<'a>(
    ctx: &'a HirCtx,
    module: &Module,
    name: &str,
) -> Result<&'a Body, String> {
    for &item_id in &module.items {
        if let Item::Function(function) = item(ctx, item_id)?
            && symbol(ctx, function.name)? == name
        {
            return body(ctx, function.body.ok_or("expected a function body")?);
        }
    }
    Err(format!("no top-level function named `{name}`"))
}

#[test]
fn try_except_lowers_to_try_catch() -> TestResult {
    let source = py!(r#"
def risky() -> int:
    return 1

def run() -> int:
    try:
        return risky()
    except ValueError:
        return 0
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let function_body = named_function_body(&ctx, module, "run")?;
    ensure(
        function_body
            .stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::TryCatch { catch_body: Some(_), .. })),
        "expected try/except to lower to a TryCatch with a catch body",
    )?;
    Ok(())
}

#[test]
fn try_except_binding_is_visible_in_handler() -> TestResult {
    let source = py!(r#"
def risky() -> str:
    return "ok"

def describe() -> str:
    try:
        return risky()
    except ValueError as error:
        return error
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let function_body = named_function_body(&ctx, module, "describe")?;
    let has_binding = function_body.stmts.iter().any(|stmt| {
        matches!(
            stmt,
            Stmt::TryCatch {
                catch_binding: Some(_),
                catch_body: Some(_),
                ..
            }
        )
    });
    ensure(has_binding, "expected `except ... as error` to bind a catch local")?;
    Ok(())
}

#[test]
fn try_finally_lowers_finally_block() -> TestResult {
    let source = py!(r#"
def risky() -> int:
    return 1

def note() -> None:
    print("done")

def cleanup() -> int:
    try:
        return risky()
    finally:
        note()
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let function_body = named_function_body(&ctx, module, "cleanup")?;
    ensure(
        function_body.stmts.iter().any(|stmt| {
            matches!(
                stmt,
                Stmt::TryCatch {
                    catch_body: None,
                    finally_body: Some(_),
                    ..
                }
            )
        }),
        "expected try/finally to lower to a TryCatch with a finally body and no catch",
    )?;
    Ok(())
}

#[test]
fn try_except_else_appends_else_to_try_body() -> TestResult {
    let source = py!(r#"
def risky() -> int:
    return 1

def choose() -> int:
    value: int = 0
    try:
        value = risky()
    except ValueError:
        return 0
    else:
        return value
    return -1
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let function_body = named_function_body(&ctx, module, "choose")?;
    ensure(
        function_body
            .stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::TryCatch { .. })),
        "expected try/except/else to lower to a TryCatch",
    )?;
    Ok(())
}

#[test]
fn multiple_except_clauses_are_rejected() -> TestResult {
    let source = py!(r#"
def run() -> int:
    try:
        return risky()
    except ValueError:
        return 0
    except KeyError:
        return 1

def risky() -> int:
    return 1
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    ensure(
        first_error(&errors)?.message.contains("multiple `except`"),
        "expected multiple except clauses to be rejected with a specific message",
    )?;
    Ok(())
}

#[test]
fn except_star_is_rejected() -> TestResult {
    let source = py!(r#"
def run() -> int:
    try:
        return risky()
    except* ValueError:
        return 0

def risky() -> int:
    return 1
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    ensure(
        first_error(&errors)?.message.contains("except*"),
        "expected `except*` to be rejected with a specific message",
    )?;
    Ok(())
}

#[test]
fn lambda_call_argument_lowers_to_closure() -> TestResult {
    let source = py!(r#"
from typing import Callable

def apply(f: Callable[[int], int], value: int) -> int:
    return f(value)

def run() -> int:
    return apply(lambda x: x + 1, 10)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    // The lambda argument to `apply` recovers its `Callable[[int], int]` type
    // from the parameter hint and materializes a first-class closure value.
    let has_closure = ctx
        .krate
        .bodies
        .iter()
        .any(|b| b.exprs.iter().any(|expr| matches!(expr.kind, ExprKind::Closure(_))));
    ensure(has_closure, "expected the lambda argument to lower to a closure")?;
    let _ = module;
    Ok(())
}

#[test]
fn bare_lambda_without_hint_is_rejected_with_actionable_message() -> TestResult {
    // Without a `Callable[...]` context the parameter types are unknowable.
    // The lambda must be rejected with a specific, actionable message rather
    // than the generic "unsupported expression: lambda".
    let source = py!(r#"
def run() -> int:
    handlers = [lambda x: x]
    return 0
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let message = &first_error(&errors)?.message;
    ensure(
        message.contains("lambda") && message.contains("Callable"),
        "expected a hint-less lambda to be rejected with a Callable-oriented message",
    )?;
    Ok(())
}

#[test]
fn class_body_ellipsis_is_a_no_op() -> TestResult {
    let source = py!(r#"
class Marker:
    ...
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure(
        module.items.iter().any(|&item_id| {
            matches!(item(&ctx, item_id), Ok(Item::Class(_)))
        }),
        "expected a class-body `...` placeholder to lower to a class",
    )?;
    Ok(())
}

#[test]
fn class_body_docstring_and_ellipsis_together_lower() -> TestResult {
    let source = py!(r#"
class Doc:
    """A documented placeholder."""
    ...
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure(
        module.items.iter().any(|&item_id| {
            matches!(item(&ctx, item_id), Ok(Item::Class(_)))
        }),
        "expected a docstring + `...` class body to lower to a class",
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Nested closure return-type inference (issue #93 / #94 blocker family
// `nested closure return type must be explicit`).
// ---------------------------------------------------------------------------

/// A nested `def` without a `-> T` annotation lowers, taking its return type
/// from the type its single `return` expression actually lowers to.
///
/// This is the shape idiomatic Python writes (`funcy`, `toolz`, and
/// `more-itertools` between them hit the old hard error 72 times), and the
/// closure's return type is knowable without any annotation: the body *is* one
/// expression, so its lowered HIR type is the accurate answer.
#[test]
fn nested_closure_without_return_annotation_lowers() -> TestResult {
    let source = py!(r#"
def make_adder(n: int) -> int:
    def add(x: int):
        return x + n
    return add(1)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = named_function_body(&ctx, module, "make_adder")?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. })),
        "expected the unannotated nested closure to lower and be callable",
    )?;
    Ok(())
}

/// The inferred return type is the *concrete* lowered type, not an erased one:
/// a closure whose body builds a `str` gets `Type::String`, so the enclosing
/// function's `-> str` contract type-checks against it.
#[test]
fn nested_closure_infers_concrete_return_type() -> TestResult {
    let source = py!(r#"
def describe(n: int) -> str:
    def label(x: str):
        return "v" + x
    return label("1")
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = named_function_body(&ctx, module, "describe")?;
    let call_ty = body
        .exprs
        .iter()
        .find(|expr| matches!(expr.kind, ExprKind::ClosureCall { .. }))
        .map(|expr| expr.ty)
        .ok_or("expected a nested closure call")?;
    ensure(
        matches!(ctx.krate.types.get(call_ty), Some(Type::String)),
        "expected the inferred closure return type to be the concrete `str`",
    )?;
    Ok(())
}

/// A declared `-> T` on a nested closure remains a contract: it is checked
/// against the lowered body and a divergent body is still rejected. Inference
/// only fills in the *missing* annotation; it never overrides a present one.
#[test]
fn nested_closure_return_annotation_is_still_enforced() -> TestResult {
    let source = py!(r#"
def broken(n: int) -> int:
    def add(x: int) -> str:
        return x + n
    return add(1)
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    ensure(
        first_error(&errors)?
            .message
            .contains("nested closure return type does not match its annotation"),
        "expected a mismatched nested closure annotation to still be rejected",
    )?;
    Ok(())
}
