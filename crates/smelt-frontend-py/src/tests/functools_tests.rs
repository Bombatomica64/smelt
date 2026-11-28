//! `functools`: `reduce`, `partial`, `lru_cache` / `cache` lowering and refusals.
use super::*;

/// The type of the module global named `name`.
fn global_type<'ctx>(ctx: &'ctx HirCtx, name: &str) -> Result<&'ctx Type, String> {
    ctx.krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::MutableGlobal(global) if ctx.krate.symbols.get(global.name) == Some(name) => {
                ctx.krate.types.get(global.ty)
            }
            _ => None,
        })
        .ok_or_else(|| format!("missing module global `{name}`"))
}

/// The first diagnostic's message, which must mention `needle`.
fn ensure_refused(source: &str, needle: &str) -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    let message = &first_error(&errors)?.message;
    ensure(
        message.contains(needle),
        format!("expected a diagnostic mentioning `{needle}`, got: {message}"),
    )
}

/// `@lru_cache` keeps the function's name and signature on a wrapper, moves
/// the body to a hidden function, and caches in a module-global dict keyed
/// by the argument tuple and valued by the declared return type.
#[test]
fn lru_cache_lowers_to_a_typed_cache_and_wrapper() -> TestResult {
    let mut ctx = HirCtx::new();
    lower_module(
        py!(r#"
import functools as ft


@ft.lru_cache(maxsize=None)
def cost(word: str, n: int) -> int:
    if n == 0:
        return len(word)
    return cost(word, n - 1) + 1
"#),
        &mut ctx,
    )?;
    let int = ctx.krate.types.intern(Type::PyInt);
    let wrapper =
        smelt_hir::function_signature(&ctx.krate, "cost").ok_or("missing the `cost` wrapper")?;
    let body = smelt_hir::function_signature(&ctx.krate, "__smelt_uncached_cost")
        .ok_or("missing the uncached body")?;
    ensure(
        wrapper.replace("cost", "") == body.replace("__smelt_uncached_cost", ""),
        format!("wrapper `{wrapper}` and body `{body}` signatures differ"),
    )?;
    match global_type(&ctx, "__smelt_lru_cache_cost")? {
        Type::Dict(key, value) => {
            let Some(Type::Tuple(members)) = ctx.krate.types.get(*key) else {
                return Err(format!(
                    "cache key is not a tuple: {:?}",
                    ctx.krate.types.get(*key)
                ));
            };
            ensure(
                members.len() == 2 && members[1] == int,
                "cache key is (str, int)",
            )?;
            ensure(*value == int, "cache value is the declared return type")
        }
        other => Err(format!("cache is not a dict: {other:?}")),
    }
}

/// One parameter keys the cache by itself, not a 1-tuple.
#[test]
fn single_parameter_cache_is_keyed_by_the_parameter() -> TestResult {
    let mut ctx = HirCtx::new();
    lower_module(
        py!(r#"
from functools import cache


@cache
def fib(n: int) -> int:
    return n if n < 2 else fib(n - 1) + fib(n - 2)
"#),
        &mut ctx,
    )?;
    let int = ctx.krate.types.intern(Type::PyInt);
    ensure(
        matches!(global_type(&ctx, "__smelt_lru_cache_fib")?, Type::Dict(key, value) if *key == int && *value == int),
        "fib's cache is dict[int, int]",
    )
}

/// What a memoizing decorator denotes is decided by binding: a program's own
/// `lru_cache` is a user decorator (which needs the build-time run), not
/// `functools.lru_cache`.
#[test]
fn a_program_defined_lru_cache_is_not_memoization() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(
        py!(r#"
def lru_cache(function):
    return function


@lru_cache
def double(n: int) -> int:
    return n * 2
"#),
        &mut ctx,
    )?;
    ensure(
        errors
            .iter()
            .all(|error| !error.message.contains("memoized")),
        "a local `lru_cache` must not be treated as functools'",
    )
}

/// A parameter whose values do not key a shared dict by value is refused.
#[test]
fn unhashable_or_float_parameters_are_refused() -> TestResult {
    ensure_refused(
        py!(r#"
from functools import cache


@cache
def total(values: list[int]) -> int:
    return sum(values)
"#),
        "cannot key the cache",
    )?;
    ensure_refused(
        py!(r#"
from functools import cache


@cache
def half(x: float) -> float:
    return x / 2
"#),
        "cannot key the cache",
    )
}

/// The memoizing decorators' unsupported shapes are refused with a reason.
#[test]
fn unsupported_memoization_shapes_are_refused() -> TestResult {
    ensure_refused(
        py!(r#"
import functools


@functools.lru_cache
@functools.lru_cache
def f(n: int) -> int:
    return n
"#),
        "memoized twice",
    )?;
    ensure_refused(
        py!(r#"
from functools import lru_cache

SIZE = 4


@lru_cache(maxsize=SIZE)
def f(n: int) -> int:
    return n
"#),
        "integer literal or None",
    )?;
    ensure_refused(
        py!(r#"
from functools import cache


@cache
def f(n: int):
    return n
"#),
        "return annotation",
    )?;
    ensure_refused(
        py!(r#"
from functools import cache


@cache
def f(*values: int) -> int:
    return len(values)
"#),
        "*args",
    )
}

/// `reduce` is typed by its initial value: a function returning another
/// type is refused rather than widened.
#[test]
fn reduce_accumulator_must_keep_its_type() -> TestResult {
    ensure_refused(
        py!(r#"
from functools import reduce


def halve(acc: int, item: int) -> float:
    return (acc + item) / 2


def main() -> None:
    print(reduce(halve, [1, 2, 3], 0))
"#),
        "accumulator",
    )
}

/// `reduce` lowers to a typed fold over the list.
#[test]
fn reduce_lowers_to_a_typed_fold() -> TestResult {
    let mut ctx = HirCtx::new();
    lower_module(
        py!(r#"
from functools import reduce


def join(acc: str, item: int) -> str:
    return acc + str(item)


def digits(values: list[int]) -> str:
    return reduce(join, values, "")
"#),
        &mut ctx,
    )?;
    let has_fold = ctx.krate.bodies.iter().any(|body| {
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::ListReduce {
                    initial: Some(_),
                    ..
                }
            )
        })
    });
    ensure(has_fold, "expected a ListReduce with an initial value")
}

/// A partial whose remaining required parameter follows a keyword-bound one
/// can only be called with that parameter by keyword: calling it works, and
/// using it as a value is refused.
#[test]
fn keyword_only_partial_is_callable_but_not_a_value() -> TestResult {
    let mut ctx = HirCtx::new();
    lower_module(
        py!(r#"
from functools import partial


def box(width: int, height: int, depth: int) -> int:
    return width * height * depth


def main() -> None:
    flat = partial(box, height=1)
    print(flat(2, depth=3))
"#),
        &mut ctx,
    )?;
    ensure_refused(
        py!(r#"
from functools import partial
from typing import Callable


def box(width: int, height: int, depth: int) -> int:
    return width * height * depth


def main() -> None:
    flat = partial(box, height=1)
    sizes = list(map(flat, [1, 2]))
"#),
        "not used as a value",
    )
}
