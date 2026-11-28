//! `itertools` lowering: each adapter answers a typed generator (its values'
//! types are the source's, its tuples' arity static), and the shapes whose
//! types are not static are refused with a named diagnostic.
use super::*;

/// The type every `itertools` call of `source` answers, in source order.
fn itertools_call_types(source: &str) -> Result<Vec<String>, String> {
    let mut ctx = HirCtx::new();
    lower_module(source, &mut ctx)?;
    // The call lowers to a block whose tail is the adapter's generator; a
    // generator-typed block is that call.
    let mut types = Vec::new();
    for body in &ctx.krate.bodies {
        for expr in &body.exprs {
            if matches!(expr.kind, ExprKind::Block(_))
                && matches!(
                    ctx.krate.types.get(expr.ty),
                    Some(smelt_hir::Type::Generator { .. })
                )
            {
                types.push(smelt_hir::type_display(&ctx.krate, expr.ty));
            }
        }
    }
    Ok(types)
}

/// Each function's annotated return type is exactly what its `itertools`
/// call answers (a mismatch fails lowering), for both import spellings.
#[test]
fn itertools_results_keep_their_element_types() -> TestResult {
    let source = py!(r#"
import itertools
from itertools import accumulate, chain, combinations, groupby, islice, permutations, product
from typing import Iterator


def flat(xs: list[int], ys: list[int]) -> Iterator[int]:
    return chain(xs, ys)


def window(words: list[str]) -> Iterator[str]:
    return itertools.islice(words, 1, None, 2)


def totals(xs: list[float]) -> Iterator[float]:
    return accumulate(xs)


def grid(xs: list[int], names: str) -> Iterator[tuple[int, str]]:
    return product(xs, names)


def pairs(xs: list[int]) -> Iterator[tuple[int, int]]:
    return combinations(xs, 2)


def orders(xs: list[str]) -> Iterator[tuple[str, str, str]]:
    return permutations(xs, 3)


def runs(words: list[str]) -> Iterator[tuple[int, Iterator[str]]]:
    return groupby(words, key=len)
"#);
    let types = itertools_call_types(source)?;
    ensure(
        types.len() >= 7,
        format!("expected every adapter to answer a generator, got {types:?}"),
    )?;
    ensure(
        types.iter().all(|ty| !ty.contains("Unknown")),
        format!("an itertools result erased its element type: {types:?}"),
    )
}

/// `permutations()` without `r` yields tuples as long as its source, a
/// static arity only for a display or a fixed-size tuple: any other source is
/// refused, naming the fix.
#[test]
fn permutations_without_r_needs_a_static_length() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(
        py!(r#"
from itertools import permutations


def orders(xs: list[int]) -> int:
    return len(list(permutations(xs)))
"#),
        &mut ctx,
    )?;
    let error = first_error(&errors)?;
    ensure(
        error.message.contains("permutations() without r"),
        format!("unexpected message: {}", error.message),
    )
}

/// A combinatoric arity is static: an `r` computed at run time is refused.
#[test]
fn combinations_r_must_be_a_literal() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(
        py!(r#"
from itertools import combinations


def picks(xs: list[int], r: int) -> int:
    return len(list(combinations(xs, r)))
"#),
        &mut ctx,
    )?;
    let error = first_error(&errors)?;
    ensure(
        error.message.contains("non-negative int literal"),
        format!("unexpected message: {}", error.message),
    )
}

/// `chain` of iterables of different element types has no single element
/// type to yield.
#[test]
fn chain_of_mixed_element_types_is_refused() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(
        py!(r#"
from itertools import chain


def mixed(xs: list[int], ys: list[str]) -> int:
    return len(list(chain(xs, ys)))
"#),
        &mut ctx,
    )?;
    let error = first_error(&errors)?;
    ensure(
        error.message.contains("share one element type"),
        format!("unexpected message: {}", error.message),
    )
}

/// An `itertools` callable Smelt does not map names the API.
#[test]
fn unmapped_itertools_callable_names_the_api() -> TestResult {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(
        py!(r#"
import itertools


def pairs(xs: list[int]) -> int:
    return len(list(itertools.tee(xs)))
"#),
        &mut ctx,
    )?;
    let error = first_error(&errors)?;
    ensure(
        error.message.contains("itertools.tee"),
        format!("unexpected message: {}", error.message),
    )
}
