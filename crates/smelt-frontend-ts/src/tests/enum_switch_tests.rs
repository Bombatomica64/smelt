//! TypeScript `enum` lowering tests (issue #78).
//!
//! Covers const-folding of enum members into literals, the enum name resolving
//! to its underlying primitive type, and `switch` statements whose case labels
//! reference enum members lowering to a `Match` over the folded values.

use super::*;

/// Collect every `Match`-arm label across all bodies in the crate.
fn match_arm_labels(ctx: &HirCtx) -> Vec<Literal> {
    let mut labels = Vec::new();
    for body in &ctx.krate.bodies {
        for stmt in &body.stmts {
            if let Stmt::Match { arms, .. } = stmt {
                for arm in arms {
                    labels.push(arm.label.clone());
                }
            }
        }
    }
    labels
}

/// Return whether any body has a `Match` arm whose label equals `literal`.
fn has_match_arm_label(ctx: &HirCtx, literal: &Literal) -> bool {
    match_arm_labels(ctx).iter().any(|label| label == literal)
}

#[test]
fn switch_over_numeric_enum_members_folds_labels() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
enum Color {
  Red = 0,
  Green = 1,
  Blue = 2,
}

export function colorName(c: Color): string {
  switch (c) {
    case Color.Red:
      return "red";
    case Color.Green:
      return "green";
    case Color.Blue:
      return "blue";
    default:
      return "unknown";
  }
}
"#),
        &mut ctx,
    )?;
    // Case labels resolve to the folded numeric member values.
    for value in [0.0, 1.0, 2.0] {
        ensure!(
            has_match_arm_label(&ctx, &Literal::Float(value)),
            "missing match arm for enum member value {value}"
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn numeric_enum_param_type_lowers_to_float() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
enum Color {
  Red,
  Green,
  Blue,
}

export function colorRank(c: Color): number {
  switch (c) {
    case Color.Red:
      return 10;
    case Color.Green:
      return 20;
    default:
      return 0;
  }
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    // Function names are interned in snake_case.
    let function = named_function_item(&ctx, module, "color_rank")?;
    let param = function
        .params
        .first()
        .ok_or_else(|| "color_rank has no parameter".to_owned())?;
    ensure_eq!(ctx.krate.types.get(param.ty), Some(&Type::Float));
    // Implicit members auto-increment from 0.
    for value in [0.0, 1.0] {
        ensure!(has_match_arm_label(&ctx, &Literal::Float(value)));
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn switch_over_string_enum_members_folds_labels() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
enum Level {
  Low = "low",
  High = "high",
}

export function levelLabel(l: Level): string {
  switch (l) {
    case Level.Low:
      return "lo";
    case Level.High:
      return "hi";
    default:
      return "x";
  }
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "level_label")?;
    let param = function
        .params
        .first()
        .ok_or_else(|| "level_label has no parameter".to_owned())?;
    // A string enum's underlying type is `String`.
    ensure_eq!(ctx.krate.types.get(param.ty), Some(&Type::String));
    for value in ["low", "high"] {
        ensure!(
            has_match_arm_label(&ctx, &Literal::String(value.to_owned())),
            "missing match arm for string enum member `{value}`"
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn enum_member_read_folds_to_member_literal() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
enum Status {
  Ok = 200,
  NotFound = 404,
}

export function okStatus(): number {
  return Status.Ok;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "ok_status")?;
    let body = function_body(&ctx, function)?;
    // `Status.Ok` inlines its folded numeric value instead of a field read.
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(v)) if v == 200.0)),
        "enum member read did not fold to its literal"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn enum_member_initializer_references_earlier_member() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
enum Flag {
  First = 1,
  Alias = First,
}

export function aliasValue(): number {
  return Flag.Alias;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "alias_value")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::Literal(Literal::Float(v)) if v == 1.0)),
        "enum member referencing an earlier member did not fold"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn exported_enum_is_consumed_without_error() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export enum Direction {
  Up,
  Down,
}

export function isUp(d: Direction): boolean {
  switch (d) {
    case Direction.Up:
      return true;
    default:
      return false;
  }
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
