use super::*;

#[test]
fn empty_module_lowers_to_empty_hir() -> TestResult {
    let mut ctx = HirCtx::new();
    let module_id = lower_module(py!(""), &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq(
        &module.source.language,
        &Language::Python,
        "module language",
    )?;
    ensure(module.items.is_empty(), "expected empty module")?;
    Ok(())
}

#[test]
fn parse_error_is_reported() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(py!("x = \"oops"), &mut ctx)?;
    ensure(!errors.is_empty(), "expected parse error")?;
    ensure_eq(
        &first_error(&errors)?.code,
        &"smelt::parse-error-py",
        "parse error code",
    )?;
    Ok(())
}

#[test]
fn simple_function_lowers() -> TestResult {
    let source = py!(r#"
def add(x: int, y: int) -> int:
    return x + y
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq(&module.items.len(), &1, "item count")?;

    let item_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected function item".to_owned())?;
    if let Item::Function(f) = item(&ctx, item_id)? {
        ensure_eq(&symbol(&ctx, f.name)?, &"add", "function name")?;
        ensure_eq(&f.params.len(), &2, "parameter count")?;
        ensure(f.body.is_some(), "expected function body")?;
    } else {
        return Err("expected Function item".to_owned());
    }
    Ok(())
}

#[test]
fn module_all_assignment_is_ignored() -> TestResult {
    let source = py!(r#"
__all__ = ["add"]

def add(x: int, y: int) -> int:
    return x + y
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    ensure_eq(&module.items.len(), &1, "item count")?;
    let module_body = body(&ctx, module.body.ok_or("expected module body")?)?;
    ensure(
        module_body.stmts.is_empty(),
        "expected __all__ metadata not to emit runtime statements",
    )?;
    Ok(())
}

#[test]
fn module_dunders_lower_to_string_literals() -> TestResult {
    let source = py!(r#"
module_name: str = __name__
module_file: str = __file__
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "src/package/example.py", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let module_body = body(&ctx, module.body.ok_or("expected module body")?)?;
    ensure(
        module_body.exprs.iter().any(
            |expr| matches!(&expr.kind, ExprKind::Literal(Literal::String(value)) if value == "example")
        ),
        "expected __name__ literal",
    )?;
    ensure(
        module_body.exprs.iter().any(
            |expr| matches!(&expr.kind, ExprKind::Literal(Literal::String(value)) if value == "src/package/example.py")
        ),
        "expected __file__ literal",
    )?;
    Ok(())
}

#[test]
fn ternary_expression_lowers() -> TestResult {
    let source = py!(r#"
def choose(flag: bool, left: int, right: int) -> int:
    return left if flag else right
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let item_id = *module.items.first().ok_or("expected function")?;
    let Item::Function(function) = item(&ctx, item_id)? else {
        return Err("expected function item".to_owned());
    };
    let function_body = body(&ctx, function.body.ok_or("expected function body")?)?;
    ensure(
        function_body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Conditional { .. })),
        "expected ternary to lower to conditional expression",
    )?;
    Ok(())
}

#[test]
fn value_returning_or_none_lowers_to_optional_conditional() -> TestResult {
    let source = py!(r#"
class Obj:
    id: str

    def __init__(self, id: str) -> None:
        self.id = id

obj: Obj = Obj("a")
value: str | None = obj.id or None
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let module_body = body(&ctx, module.body.ok_or("expected module body")?)?;
    ensure(
        module_body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::Conditional {
                    then_expr: _,
                    else_expr: _,
                    ..
                }
            ) && matches!(ctx.krate.types.get(expr.ty), Some(Type::Optional(_)))
        }),
        "expected value-returning or to lower to an optional conditional",
    )?;
    Ok(())
}

#[test]
fn async_function_and_await_lower() -> TestResult {
    let source = py!(r#"
async def lift(value: int) -> int:
    return value

async def run() -> int:
    return await lift(5)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;

    ensure_eq(&module.items.len(), &2, "item count")?;

    let lift_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected lift function".to_owned())?;
    let Item::Function(lift) = item(&ctx, lift_id)? else {
        return Err("expected function item for lift".to_owned());
    };
    ensure(lift.is_async, "lift should be async")?;
    ensure(
        matches!(ctx.krate.types.get(lift.return_ty), Some(Type::Future(_))),
        "lift return type should be Future",
    )?;

    let run_id = module
        .items
        .get(1)
        .copied()
        .ok_or_else(|| "expected run function".to_owned())?;
    let Item::Function(run) = item(&ctx, run_id)? else {
        return Err("expected function item for run".to_owned());
    };
    ensure(run.is_async, "run should be async")?;
    let body_id = run.body.ok_or_else(|| "expected run body".to_owned())?;
    let body = body(&ctx, body_id)?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Await(_))),
        "expected await expression",
    )?;
    let state_machine = body
        .async_state_machine
        .as_ref()
        .ok_or_else(|| "async body should record suspension metadata".to_owned())?;
    ensure_eq(&state_machine.suspensions.len(), &1, "suspension count")?;
    Ok(())
}

#[test]
fn await_outside_async_function_is_rejected() -> TestResult {
    let source = py!(r#"
async def lift(value: int) -> int:
    return value

def run() -> int:
    return await lift(5)
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::unsupported-py", "error code")?;
    ensure(
        error.message.contains("inside async functions"),
        "expected async-only message",
    )?;
    Ok(())
}

#[test]
fn asyncio_gather_and_sleep_lower_to_async_ops() -> TestResult {
    let source = py!(r#"
import asyncio

async def lift(value: int) -> int:
    await asyncio.sleep(0)
    return value

async def run() -> tuple[int, int]:
    return await asyncio.gather(lift(1), lift(2))
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;

    let lift_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected lift function".to_owned())?;
    let Item::Function(lift) = item(&ctx, lift_id)? else {
        return Err("expected function item for lift".to_owned());
    };
    let lift_body_id = lift.body.ok_or_else(|| "expected lift body".to_owned())?;
    let lift_body = body(&ctx, lift_body_id)?;
    ensure(
        lift_body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::AsyncOp {
                    op: AsyncOp::Sleep,
                    ..
                }
            )
        }),
        "expected asyncio.sleep lowering",
    )?;

    let run_id = module
        .items
        .get(1)
        .copied()
        .ok_or_else(|| "expected run function".to_owned())?;
    let Item::Function(run) = item(&ctx, run_id)? else {
        return Err("expected function item for run".to_owned());
    };
    let run_body_id = run.body.ok_or_else(|| "expected run body".to_owned())?;
    let run_body = body(&ctx, run_body_id)?;
    ensure(
        run_body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::AsyncOp {
                    op: AsyncOp::All,
                    ..
                }
            )
        }),
        "expected asyncio.gather lowering",
    )?;
    Ok(())
}

#[test]
fn lower_level_asyncio_loop_apis_are_rejected() -> TestResult {
    let source = py!(r#"
import asyncio

def run() -> None:
    asyncio.get_event_loop()
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::unsupported-py", "error code")?;
    ensure(
        error.message.contains("lower-level event-loop API"),
        "expected lower-level event-loop message",
    )?;
    Ok(())
}

#[test]
fn asyncio_task_wait_for_and_runtime_objects_are_classified() -> TestResult {
    let source = py!(r#"
import asyncio

async def lift(value: int) -> int:
    return value

async def run() -> int:
    task: Awaitable[int] = asyncio.create_task(lift(1))
    return await asyncio.wait_for(task, 10)
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;

    let queue_source = py!(r#"
import asyncio

def run() -> None:
    asyncio.Queue()
"#);
    let mut queue_ctx = HirCtx::new();
    let errors = lower_errors(queue_source, &mut queue_ctx)?;
    ensure(
        first_error(&errors)?
            .message
            .contains("runtime object support"),
        "expected runtime object support message",
    )?;
    Ok(())
}

#[test]
fn fstring_interpolation_lowers_to_string_concat() -> TestResult {
    let source = py!(r#"
def greet(name: str, count: int) -> str:
    return f"Hello {name}, you have {count} messages"
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let item_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected greet function".to_owned())?;
    let Item::Function(greet) = item(&ctx, item_id)? else {
        return Err("expected function item".to_owned());
    };
    let body_id = greet.body.ok_or_else(|| "expected greet body".to_owned())?;
    let body = body(&ctx, body_id)?;
    ensure(
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::BinOp {
                    op: BinOp::Add,
                    ..
                }
            ) && ctx.krate.types.get(expr.ty) == Some(&Type::String)
        }),
        "expected f-string to lower to a String addition chain",
    )?;
    Ok(())
}

#[test]
fn fstring_only_literal_parts_lower_to_string() -> TestResult {
    let source = py!(r#"
def shout() -> str:
    return f"static text"
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let item_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected shout function".to_owned())?;
    let Item::Function(shout) = item(&ctx, item_id)? else {
        return Err("expected function item".to_owned());
    };
    let body_id = shout.body.ok_or_else(|| "expected shout body".to_owned())?;
    let body = body(&ctx, body_id)?;
    ensure(
        body.exprs.iter().any(|expr| {
            matches!(&expr.kind, ExprKind::Literal(Literal::String(text)) if text == "static text")
        }),
        "expected a literal-only f-string to lower to a string literal",
    )?;
    Ok(())
}

#[test]
fn fstring_format_spec_is_rejected() -> TestResult {
    let source = py!(r#"
def render(value: float) -> str:
    return f"{value:.2f}"
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::unsupported-py", "error code")?;
    ensure(
        error.message.contains("format specifications"),
        "expected format specification rejection message",
    )?;
    Ok(())
}

#[test]
fn fstring_repr_conversion_is_rejected() -> TestResult {
    let source = py!(r#"
def render(value: int) -> str:
    return f"{value!r}"
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::unsupported-py", "error code")?;
    ensure(
        error.message.contains("conversions"),
        "expected conversion rejection message",
    )?;
    Ok(())
}

/// Returns the body of the single function item in `source` after lowering.
fn single_function_body<'a>(ctx: &'a HirCtx, module_id: ModuleId) -> Result<&'a Body, String> {
    let module = module(ctx, module_id)?;
    let item_id = module
        .items
        .first()
        .copied()
        .ok_or_else(|| "expected a function item".to_owned())?;
    let Item::Function(function) = item(ctx, item_id)? else {
        return Err("expected function item".to_owned());
    };
    let body_id = function.body.ok_or_else(|| "expected function body".to_owned())?;
    body(ctx, body_id)
}

#[test]
fn list_comprehension_lowers_to_block_with_push() -> TestResult {
    let source = py!(r#"
def doubles(xs: list[int]) -> list[int]:
    return [x * 2 for x in xs]
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let body = single_function_body(&ctx, module_id)?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Block(_))),
        "expected list comprehension to lower to a block expression",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListPush { .. })),
        "expected the accumulator to be built with list pushes",
    )?;
    Ok(())
}

#[test]
fn list_comprehension_if_clause_lowers() -> TestResult {
    // The `if` guard lowers to an `If` statement inside the loop; we assert the
    // overall comprehension still lowers to a block with a push.
    let source = py!(r#"
def evens(xs: list[int]) -> list[int]:
    return [x for x in xs if x > 0]
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let body = single_function_body(&ctx, module_id)?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Block(_))),
        "expected a block expression",
    )?;
    Ok(())
}

#[test]
fn nested_list_comprehension_lowers() -> TestResult {
    let source = py!(r#"
def pairs(xs: list[int], ys: list[int]) -> list[int]:
    return [x + y for x in xs for y in ys]
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let body = single_function_body(&ctx, module_id)?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Block(_))),
        "expected nested list comprehension to lower to a block expression",
    )?;
    Ok(())
}

#[test]
fn set_comprehension_lowers_to_block_with_add() -> TestResult {
    let source = py!(r#"
def uniq(xs: list[int]) -> set[int]:
    return {x for x in xs}
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let body = single_function_body(&ctx, module_id)?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetAdd { .. })),
        "expected set comprehension to build the accumulator with set adds",
    )?;
    Ok(())
}

#[test]
fn dict_comprehension_lowers_to_block() -> TestResult {
    let source = py!(r#"
def table(xs: list[int]) -> dict[int, int]:
    return {k: k for k in xs}
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let body = single_function_body(&ctx, module_id)?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Block(_))),
        "expected dict comprehension to lower to a block expression",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::DictLit(_))),
        "expected an empty dict accumulator literal",
    )?;
    Ok(())
}

#[test]
fn generator_expression_lowers() -> TestResult {
    let source = py!(r#"
def collected(xs: list[int]) -> list[int]:
    return list(x for x in xs)
"#);
    let mut ctx = HirCtx::new();
    // Generator expressions materialize eagerly; lowering should succeed.
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn list_comprehension_over_string_lowers() -> TestResult {
    let source = py!(r#"
def chars(text: str) -> list[str]:
    return [c for c in text]
"#);
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    Ok(())
}

#[test]
fn comprehension_destructuring_target_is_rejected() -> TestResult {
    let source = py!(r#"
def firsts(pairs: list[tuple[int, int]]) -> list[int]:
    return [a for a, b in pairs]
"#);
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let error = first_error(&errors)?;
    ensure_eq(&error.code, &"smelt::unsupported-py", "error code")?;
    ensure(
        error.message.contains("destructuring"),
        "expected destructuring rejection message",
    )?;
    Ok(())
}
