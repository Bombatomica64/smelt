//! JavaScript `bigint` lowering: its own HIR type, exact literals, and the
//! fallible operations that become `BigIntCall`s.
//!
//! Type-preservation requirement: every bigint value -- a literal at any
//! magnitude, a `bigint` annotation, a `BigInt(x)` result, bigint arithmetic --
//! is typed [`Type::BigInt`], never the bounded `Int` (`i64`) nor `Float`.
//! Behavioral requirement: the operations that can throw a catchable
//! `RangeError` (`/`, `%`, `**`, `BigInt(x)`, `asIntN`/`asUintN`) lower to a
//! fallible [`ExprKind::BigIntCall`], so a `try` can catch them.

use super::*;
use smelt_hir::BigIntOp;

/// Whether some lowered expression is a `BigIntCall` of `op`.
fn has_bigint_call(ctx: &HirCtx, op: BigIntOp) -> bool {
    ctx.krate.bodies.iter().any(|body| {
        body.exprs.iter().any(|expr| {
            matches!(&expr.kind, ExprKind::BigIntCall { op: found, .. } if *found == op)
                && ctx.krate.types.get(expr.ty) == Some(&Type::BigInt)
        })
    })
}

#[test]
fn a_bigint_literal_keeps_its_exact_digits_at_every_magnitude() -> Result<(), String> {
    // 2^73-ish is past both `i64` and an exact `f64`; it used to fall back to
    // an `f64` literal (`1e22`) that saturated to `i64::MAX` downstream.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
const HUGE = 9_999_999_999_999_999_999_999n;
const SMALL = 0x1fn;
"),
        &mut ctx,
    )?;
    let literal = |digits: &str| {
        ctx.krate.bodies.iter().any(|body| {
            body.exprs.iter().any(|expr| {
                matches!(&expr.kind, ExprKind::Literal(Literal::BigInt(found)) if found == digits)
                    && ctx.krate.types.get(expr.ty) == Some(&Type::BigInt)
            })
        })
    };
    ensure!(literal("9999999999999999999999"), "the wide literal lost digits");
    ensure!(literal("31"), "a hex bigint literal is its decimal value");
    ensure!(
        !ctx.krate.bodies.iter().any(|body| body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(_) | Literal::Int(_))))),
        "a bigint literal lowered to a number"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn bigint_annotations_and_arithmetic_carry_the_bigint_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function mix(a: bigint, b: bigint): bigint {
  return ((a << b) + BigInt(7)) & ~a;
}
const index = [1, 2].length;
"),
        &mut ctx,
    )?;
    // Every bigint operator result is a bigint...
    let binary_types = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter(|expr| {
            matches!(
                expr.kind,
                ExprKind::BinOp {
                    op: BinOp::Shl | BinOp::Add | BinOp::BitAnd | BinOp::BitXor,
                    ..
                }
            )
        })
        .map(|expr| ctx.krate.types.get(expr.ty).cloned())
        .collect::<Vec<_>>();
    ensure!(binary_types.len() == 4, "{binary_types:?}");
    ensure!(
        binary_types.iter().all(|ty| *ty == Some(Type::BigInt)),
        "{binary_types:?}"
    );
    // ...and `BigInt(7)` converts through the fallible `ToBigInt`.
    ensure!(has_bigint_call(&ctx, BigIntOp::From));
    // The bounded integer stays the bounded integer: no bigint leaks into the
    // rest of the program's typing.
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn throwing_bigint_operations_lower_to_fallible_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function quotient(a: bigint, b: bigint): bigint { return a / b; }
function remainder(a: bigint, b: bigint): bigint { return a % b; }
function power(a: bigint, b: bigint): bigint { return a ** b; }
function wrap(a: bigint): bigint { return BigInt.asIntN(64, a) + BigInt.asUintN(8, a); }
function halve(a: bigint): bigint {
  a /= 2n;
  a **= 2n;
  return a;
}
"),
        &mut ctx,
    )?;
    for op in [
        BigIntOp::Div,
        BigIntOp::Rem,
        BigIntOp::Pow,
        BigIntOp::AsIntN,
        BigIntOp::AsUintN,
    ] {
        ensure!(has_bigint_call(&ctx, op), "missing {op:?}");
    }
    // A bigint `/` or `%` is never the infallible `BinOp` (which has no
    // unwind edge for its `RangeError`).
    ensure!(
        !ctx.krate.bodies.iter().any(|body| body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::BinOp { op: BinOp::Div | BinOp::Rem, .. })
                && ctx.krate.types.get(expr.ty) == Some(&Type::BigInt)
        })),
        "a bigint division stayed an infallible BinOp"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn a_number_or_bigint_operand_gives_a_number_or_bigint_result() -> Result<(), String> {
    // remeda's `sum`: `out` holds a bigint or a number depending on the data,
    // so `out + value` is either. Typing it `number` dropped the bigint arm.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function total(values: readonly unknown[], big: boolean): number | bigint {
  let out = big ? 0n : 0;
  for (const value of values) {
    out = out + (value as number | bigint);
  }
  return out;
}
"),
        &mut ctx,
    )?;
    let either = |ty: smelt_hir::TypeId| match ctx.krate.types.get(ty) {
        Some(Type::Union(items)) => {
            items.iter().any(|item| ctx.krate.types.get(*item) == Some(&Type::Float))
                && items.iter().any(|item| ctx.krate.types.get(*item) == Some(&Type::BigInt))
        }
        _ => false,
    };
    ensure!(
        ctx.krate.bodies.iter().any(|body| body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::BinOp { op: BinOp::Add, .. })
                && either(expr.ty))),
        "`number | bigint` + `number | bigint` is not `number | bigint`"
    );
    Ok(())
}
