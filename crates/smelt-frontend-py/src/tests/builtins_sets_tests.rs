use super::*;

#[test]
fn dict_projection_methods_lower() -> TestResult {
    let source = py!(r#"
mapping: dict[str, int] = {"a": 1, "b": 2}
keys: list[str] = mapping.keys()
values: list[int] = mapping.values()
items: list[tuple[str, int]] = mapping.items()
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    for expected in [
        DictProjectionOp::Keys,
        DictProjectionOp::Values,
        DictProjectionOp::Entries,
    ] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::DictProjection { op, .. } if op == expected),
            ),
            "expected dict projection lowering",
        )?;
    }
    Ok(())
}

#[test]
fn math_numeric_functions_lower() -> TestResult {
    let source = py!(r#"
import math
value: float = 4.0
root: float = math.sqrt(value)
sin_value: float = math.sin(value)
cos_value: float = math.cos(value)
tan_value: float = math.tan(value)
asin_value: float = math.asin(value)
acos_value: float = math.acos(value)
atan_value: float = math.atan(value)
atan2_value: float = math.atan2(value, 2.0)
log_value: float = math.log(value)
log10_value: float = math.log10(value)
log2_value: float = math.log2(value)
exp_value: float = math.exp(value)
pi_value: float = math.pi
e_value: float = math.e
tau_value: float = math.tau
raised: float = math.pow(value, 2.0)
floored: int = math.floor(value)
ceiled: int = math.ceil(value)
whole: int = math.trunc(value)
finite: bool = math.isfinite(value)
nan_value: bool = math.isnan(value)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    for expected in [
        NumericUnaryFuncOp::Sqrt,
        NumericUnaryFuncOp::Sin,
        NumericUnaryFuncOp::Cos,
        NumericUnaryFuncOp::Tan,
        NumericUnaryFuncOp::Asin,
        NumericUnaryFuncOp::Acos,
        NumericUnaryFuncOp::Atan,
        NumericUnaryFuncOp::Log,
        NumericUnaryFuncOp::Log10,
        NumericUnaryFuncOp::Log2,
        NumericUnaryFuncOp::Exp,
    ] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::NumericUnaryFunc { op, .. } if op == expected),
            ),
            "expected math unary lowering",
        )?;
    }
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericPow { .. })),
        "expected math.pow lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericAtan2 { .. })),
        "expected math.atan2 lowering",
    )?;
    for expected in [
        NumericRoundOp::Floor,
        NumericRoundOp::Ceil,
        NumericRoundOp::Trunc,
    ] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::NumericRound { op, .. } if op == expected),
            ),
            "expected math rounding lowering",
        )?;
    }
    for expected in [NumericPredicateOp::IsFinite, NumericPredicateOp::IsNaN] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::NumericPredicate { op, .. } if op == expected),
            ),
            "expected math predicate lowering",
        )?;
    }
    for expected in [
        std::f64::consts::PI,
        std::f64::consts::E,
        std::f64::consts::TAU,
    ] {
        ensure(
            body.exprs.iter().any(|expr| {
                matches!(expr.kind, ExprKind::Literal(Literal::Float(value)) if value == expected)
            }),
            "expected math constant lowering",
        )?;
    }
    Ok(())
}

#[test]
fn random_module_functions_lower() -> TestResult {
    let source = py!(r#"
import random
sample: float = random.random()
roll: int = random.randint(1, 6)
values: list[str] = ["a", "b"]
picked: str = random.choice(values)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericRandom)),
        "expected random.random lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::NumericRandomInt { .. })),
        "expected random.randint lowering",
    )?;
    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListRandomChoice { .. })),
        "expected random.choice lowering",
    )?;
    Ok(())
}

#[test]
fn builtin_min_max_lower() -> TestResult {
    let source = py!(r#"
first: int = 1
second: int = 2
highest: int = max(first, second)
lowest: int = min(first, second)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    for expected in [NumericExtremaOp::Max, NumericExtremaOp::Min] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::NumericExtrema { op, .. } if op == expected),
            ),
            "expected Python min/max lowering",
        )?;
    }
    Ok(())
}

#[test]
fn builtin_sum_lower() -> TestResult {
    let source = py!(r#"
ints: list[int] = [1, 2]
int_total: int = sum(ints)
floats: list[float] = [1.0, 2.0]
float_total: float = sum(floats)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;
    let sums = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListSum { .. }))
        .count();
    ensure_eq(&sums, &2, "list sum count")
}

#[test]
fn builtin_primitive_conversions_lower() -> TestResult {
    let source = py!(r#"
as_text: str = str(True)
as_int: int = int(3.8)
as_float: float = float(7)
as_bool: bool = bool("")
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    for expected in [
        PrimitiveCastOp::ToString,
        PrimitiveCastOp::ToInt,
        PrimitiveCastOp::ToFloat,
        PrimitiveCastOp::ToBool,
    ] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::PrimitiveCast { op, .. } if op == expected),
            ),
            "expected primitive conversion lowering",
        )?;
    }
    Ok(())
}

#[test]
fn string_contains_comparison_lowers() -> TestResult {
    let source = py!(r#"
word: str = "Smelt"
has: bool = "mel" in word
missing: bool = "xyz" not in word
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body_id = module
        .body
        .ok_or_else(|| "expected module body".to_owned())?;
    let body = body(&ctx, body_id)?;

    let contains_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::StringContains { .. }))
        .count();
    ensure_eq(&contains_count, &2, "string contains count")?;
    Ok(())
}

#[test]
fn builtin_all_any_lower() -> TestResult {
    let source = py!(r#"
values: list[bool] = [True, False]
all_values: bool = all(values)
any_values: bool = any(values)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    for expected in [BoolFoldOp::All, BoolFoldOp::Any] {
        ensure(
            body.exprs.iter().any(
                |expr| matches!(expr.kind, ExprKind::ListBoolFold { op, .. } if op == expected),
            ),
            "expected bool fold lowering",
        )?;
    }
    Ok(())
}

#[test]
fn builtin_sorted_lower() -> TestResult {
    let source = py!(r#"
values: list[int] = [2, 1]
ordered: list[int] = sorted(values)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListSorted { .. })),
        "expected sorted lowering",
    )?;
    Ok(())
}

#[test]
fn builtin_map_filter_lambda_callbacks_lower() -> TestResult {
    let source = py!(r#"
factor: int = 2
values: list[int] = [1, 2, 3]
scaled: list[int] = list(map(lambda value: value * factor, values))
filtered: list[int] = list(filter(lambda value: value > factor, values))
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    ensure(
        body.exprs.iter().any(|expr| {
            matches!(
                &expr.kind,
                ExprKind::ListCallback {
                    op: smelt_hir::ListCallbackOp::Map,
                    callback,
                    ..
                } if closure_cfg_has_capture(&ctx, body, *callback)
            )
        }),
        "expected map lambda callback lowering",
    )?;
    ensure(
        body.exprs.iter().any(|expr| {
            matches!(
                &expr.kind,
                ExprKind::ListCallback {
                    op: smelt_hir::ListCallbackOp::Filter,
                    callback,
                    ..
                } if closure_cfg_has_capture(&ctx, body, *callback)
            )
        }),
        "expected filter lambda callback lowering",
    )?;
    Ok(())
}

#[test]
fn builtin_map_local_lambda_callback_lowers() -> TestResult {
    let source = py!(r#"
from typing import Callable
factor: int = 2
values: list[int] = [1, 2, 3]
scale: Callable[[int], int] = lambda value: value * factor
scaled: list[int] = list(map(scale, values))
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    ensure(
        body.exprs.iter().any(|expr| {
            matches!(
                &expr.kind,
                ExprKind::ListCallback {
                    op: smelt_hir::ListCallbackOp::Map,
                    callback,
                    ..
                } if closure_cfg_has_capture(&ctx, body, *callback)
            )
        }),
        "expected local map lambda callback lowering",
    )?;
    Ok(())
}

#[test]
fn builtin_reversed_lower() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2]
flipped: list[int] = reversed(values)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::ListReversed { .. })),
        "expected reversed lowering",
    )?;
    Ok(())
}

#[test]
fn builtin_enumerate_lower() -> TestResult {
    let source = py!(r#"
values: list[str] = ["a", "b"]
pairs: list[tuple[int, str]] = enumerate(values)
names: dict[str, int] = {"Ada": 1}
name_pairs: list[tuple[int, str]] = enumerate(names)
items: set[int] = {1, 2}
item_pairs: list[tuple[int, int]] = enumerate(items)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;
    ensure(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::ListEnumerate { .. }))
            .count()
            == 3,
        "expected enumerate() lowering for list, dict keys, and set values",
    )
}

#[test]
fn builtin_zip_lower() -> TestResult {
    let source = py!(r#"
names: list[str] = ["Ada", "Linus"]
scores: list[int] = [1, 2]
pairs: list[tuple[str, int]] = zip(names, scores)
lookup: dict[str, int] = {"Ada": 1}
items: set[int] = {1, 2}
mixed: list[tuple[str, int]] = zip(lookup, items)
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;
    ensure(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::ListZip { .. }))
            .count()
            == 2,
        "expected zip() lowering for list/list and dict/set inputs",
    )
}

#[test]
fn range_builtin_lowers() -> TestResult {
    let source = py!(r#"
first: list[int] = range(3)
middle: list[int] = range(1, 4)
stepped: list[int] = range(5, 1, -2)
total: int = 0
for value in range(3):
    total = total + value
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;
    ensure(
        body.exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::ListRange { .. }))
            .count()
            >= 4,
        "expected range lowering",
    )?;
    ensure(
        body.stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::For { .. })),
        "expected for range lowering",
    )
}

#[test]
fn set_and_dict_for_loops_lower_to_projections() -> TestResult {
    let source = py!(r#"
items: set[int] = {1, 2}
total: int = 0
for item in items:
    total = total + item
names: dict[str, int] = {"Ada": 1}
last: str = ""
for name in names:
    last = name
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    ensure(
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::SetProjection {
                    op: SetProjectionOp::Values,
                    ..
                }
            )
        }),
        "expected set for-loop to lower through values projection",
    )?;
    ensure(
        body.exprs.iter().any(|expr| {
            matches!(
                expr.kind,
                ExprKind::DictProjection {
                    op: DictProjectionOp::Keys,
                    ..
                }
            )
        }),
        "expected dict for-loop to lower through keys projection",
    )?;
    ensure(
        body.stmts
            .iter()
            .filter(|stmt| matches!(stmt, Stmt::For { .. }))
            .count()
            >= 2,
        "expected both for loops to lower",
    )
}

#[test]
fn list_contains_comparison_lowers() -> TestResult {
    let source = py!(r#"
values: list[int] = [1, 2, 3]
has: bool = 2 in values
missing: bool = 4 not in values
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body_id = module
        .body
        .ok_or_else(|| "expected module body".to_owned())?;
    let body = body(&ctx, body_id)?;

    let contains_count = body
        .exprs
        .iter()
        .filter(|expr| matches!(expr.kind, ExprKind::ListContains { .. }))
        .count();
    ensure_eq(&contains_count, &2, "list contains count")?;
    Ok(())
}

#[test]
fn set_literal_and_contains_comparison_lowers() -> TestResult {
    let source = py!(r#"
values: set[int] = {1, 2, 3}
has: bool = 2 in values
missing: bool = 4 not in values
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_module(source, &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let body = body(
        &ctx,
        module
            .body
            .ok_or_else(|| "expected module body".to_owned())?,
    )?;

    ensure(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::SetLit(_))),
        "expected set literal lowering",
    )?;
    ensure_eq(
        &body
            .exprs
            .iter()
            .filter(|expr| matches!(expr.kind, ExprKind::SetContains { .. }))
            .count(),
        &2,
        "set contains count",
    )?;
    Ok(())
}
