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
