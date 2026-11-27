//! Round-3 Python sweep: truth tests, value-returning `and`/`or`, starred
//! targets, `str.format`, dataclass `field(...)`: the HIR shapes the rules
//! produce and the source shapes they refuse.
use super::*;

/// The first diagnostic lowering `source` produces.
fn refusal(source: &str) -> Result<String, String> {
    let mut ctx = HirCtx::new();
    let errors = lower_errors(source, &mut ctx)?;
    Ok(first_error(&errors)?.message.clone())
}

/// Fails unless lowering `source` is refused with a message containing `needle`.
fn ensure_refused(source: &str, needle: &str) -> TestResult {
    let message = refusal(source)?;
    ensure(
        message.contains(needle),
        format!("expected `{needle}`, got: {message}"),
    )
}

#[test]
fn a_container_condition_is_a_length_test_not_a_cast() -> TestResult {
    let source = py!(r#"
values: list[int] = []
if values:
    print("non-empty")
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let code = module_code(&ctx, module)?;
    ensure(
        code.exprs()
            .any(|expr| matches!(expr.kind, ExprKind::Len { .. })),
        "`if values:` tests `len(values) != 0`",
    )?;
    ensure(
        !code.exprs().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToBool,
                    ..
                }
            )
        }),
        "a list condition never goes through the JavaScript `ToBool`",
    )
}

#[test]
fn a_union_holding_a_container_is_not_truth_tested() -> TestResult {
    ensure_refused(
        py!(r#"
def f(value: int | list[int]) -> None:
    if value:
        print("yes")
"#),
        "truth test of a union holding a container",
    )
}

#[test]
fn and_or_over_operands_of_different_types_is_refused() -> TestResult {
    ensure_refused(
        py!(r#"
def f(items: list[int], name: str) -> None:
    print(items or name)
"#),
        "returns one of its operands, and these have different types",
    )
}

#[test]
fn a_starred_target_needs_a_list_value() -> TestResult {
    ensure_refused(
        py!(r#"
def f(pair: tuple[int, str, str]) -> None:
    first, *rest = pair
    print(first, rest)
"#),
        "a starred assignment needs a list value",
    )
}

#[test]
fn str_format_fields_it_cannot_resolve_are_refused() -> TestResult {
    ensure_refused(
        py!(r#"print("{0.x}".format(1))"#),
        "naming an attribute or an item",
    )?;
    ensure_refused(
        py!(r#"print("{} {}".format(1))"#),
        "replacement index 1 out of range",
    )?;
    ensure_refused(
        py!(r#"print("{name}".format(other=1))"#),
        "names no keyword argument",
    )
}

#[test]
fn dataclass_field_with_other_options_is_refused() -> TestResult {
    ensure_refused(
        py!(r#"
from dataclasses import dataclass, field

@dataclass
class Item:
    name: str
    tags: list[str] = field(default_factory=list, repr=False)
"#),
        "exactly one of `default=` or `default_factory=`",
    )
}
