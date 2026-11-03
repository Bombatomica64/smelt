//! Function-scope isolation and `async def` coverage for the Python frontend.
//!
//! The module builder lowers every item and records an error per failing item
//! instead of stopping, so a function that fails half-way must not leave its
//! scope behind: the items and module statements lowered after it would see
//! its locals (and its async-ness) as their own. These tests pin that a failed
//! body's state is gone, and that every kind of `async def` body -- a plain
//! function, a parametrized test row, a method -- may `await`.
use super::*;

/// The messages of every diagnostic, for containment checks.
fn messages(errors: &[SmeltError]) -> Vec<String> {
    errors.iter().map(|error| error.message.clone()).collect()
}

/// The async-ness and return type of the function item named `name`.
fn function_named<'a>(ctx: &'a HirCtx, module_id: ModuleId, name: &str) -> Result<&'a smelt_hir::Function, String> {
    let module = module(ctx, module_id)?;
    for item_id in &module.items {
        if let Item::Function(function) = item(ctx, *item_id)?
            && symbol(ctx, function.name)? == name
        {
            return Ok(function);
        }
    }
    Err(format!("no function item named `{name}`"))
}

#[test]
fn a_failed_function_does_not_leak_its_locals_into_module_statements() -> TestResult {
    // `x` is bound as `bad`'s parameter before `y`'s annotation fails. Before
    // the scope was restored on that early exit, the module-level `print(x)`
    // resolved `x` to `bad`'s local id and the frontend panicked indexing the
    // module body with it.
    let source = py!(r"
def bad(x: int, y: 1 + 2) -> int:
    return x


print(x)
");
    let mut ctx = HirCtx::new();
    let errors = messages(&lower_errors(source, &mut ctx)?);
    ensure(
        errors.iter().any(|message| message.contains("unsupported type annotation form")),
        format!("expected the annotation error, got {errors:?}"),
    )?;
    ensure(
        errors.iter().any(|message| message.contains("unresolved name 'x'")),
        format!("expected `x` to be unresolved at module level, got {errors:?}"),
    )
}

#[test]
fn a_failed_method_does_not_leak_its_receiver_into_module_statements() -> TestResult {
    let source = py!(r"
class Counter:
    def bump(self, step: 1 + 2) -> int:
        return 1


print(self)
");
    let mut ctx = HirCtx::new();
    let errors = messages(&lower_errors(source, &mut ctx)?);
    ensure(
        errors.iter().any(|message| message.contains("unresolved name 'self'")),
        format!("expected `self` to be unresolved at module level, got {errors:?}"),
    )
}

#[test]
fn a_failed_async_function_does_not_make_the_module_async() -> TestResult {
    // Module-level `await` is not valid Python, so this only pins that the
    // failed body's async-ness does not survive it: the `await` must still be
    // reported as outside an async function.
    let source = py!(r"
async def bad(x: int, y: 1 + 2) -> int:
    return x


async def lift(value: int) -> int:
    return value


await lift(1)
");
    let mut ctx = HirCtx::new();
    let errors = messages(&lower_errors(source, &mut ctx)?);
    ensure(
        errors.iter().any(|message| message.contains("only supported inside async functions")),
        format!("expected the module-level await to be rejected, got {errors:?}"),
    )
}

#[test]
fn a_parametrized_async_test_may_await() -> TestResult {
    // Each parametrize row became a test lowered with `await` forbidden, so a
    // perfectly ordinary async pytest test was rejected.
    let source = py!(r#"
import pytest


async def lift(value: int) -> int:
    return value


@pytest.mark.parametrize("value", [1, 2])
async def test_lift(value: int) -> None:
    assert await lift(value) == value
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_module(source, "tests/test_async_rows.py", &mut ctx)?;
    let row = function_named(&ctx, module_id, "test_lift__case_0")?;
    ensure(row.is_async, "an `async def` row lowers to an async test")?;
    ensure(
        matches!(ctx.krate.types.get(row.return_ty), Some(Type::Future(_))),
        "an async row returns a future",
    )
}

#[test]
fn an_async_method_may_await() -> TestResult {
    // A method body never entered an async scope, so `await` inside an
    // `async def` method was rejected, and the method itself lowered as a
    // synchronous function returning the bare annotation.
    let source = py!(r"
class Source:
    async def read(self) -> int:
        return 1

    async def twice(self) -> int:
        return await self.read() * 2


async def run() -> int:
    return await Source().twice()
");
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let mut async_methods = 0;
    for item in &ctx.krate.items {
        if let Item::Function(function) = item
            && matches!(function.owner, smelt_hir::FunctionOwner::ClassMethod { .. })
        {
            ensure(function.is_async, "an `async def` method lowers async")?;
            ensure(
                matches!(ctx.krate.types.get(function.return_ty), Some(Type::Future(_))),
                "an async method returns a future",
            )?;
            async_methods += 1;
        }
    }
    ensure_eq(&async_methods, &2, "async method count")?;
    ensure(function_named(&ctx, module_id, "run")?.is_async, "run stays async")
}
