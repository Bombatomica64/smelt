//! Regression tests for array literals whose elements are function, `this`, or
//! class expressions.
//!
//! Array literal element lowering historically rejected any element that was not
//! one of an explicit set of expression kinds, so `[function () {}]`,
//! `[this]`, and `[class {}]` all bailed with "array element kind is not lowered
//! yet" / "expression kind is not lowered yet: ClassExpression". These elements
//! now route through the shared expression lowering path, matching how the same
//! constructs lower everywhere else.

use super::*;

/// Return whether any body in the crate contains an expression matching `pred`.
fn crate_has_expr(ctx: &HirCtx, pred: impl Fn(&ExprKind) -> bool) -> bool {
    ctx.krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| pred(&expr.kind)))
}

#[test]
fn lowers_array_element_function_expression_to_closure() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function handlers(): unknown[] {
  return [function () { return 1; }];
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    // The function expression element lowers to a closure value, not an error.
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::Closure(_)
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_element_this_expression() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    // `this` is only meaningful with a receiver, so exercise it inside a method
    // where the current receiver is in scope.
    let module_id = lower_ok(
        ts!(r#"
class Box {
  value: number;
  constructor(value: number) {
    this.value = value;
  }
  selves(): unknown[] {
    return [this];
  }
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_element_named_class_expression() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function classes(): unknown[] {
  return [class Named { value: number = 0; }];
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_element_anonymous_class_expression() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function classes(): unknown[] {
  return [class { value: number = 0; }];
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_class_expression_in_value_position() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    // A class expression bound to a `const` exercises the general expression
    // path (not the array-element path) reaching `class_expression_value`.
    let module_id = lower_ok(
        ts!(r#"
export function make(): unknown {
  const ctor = class { field: number = 1; };
  return ctor;
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn lowers_array_with_mixed_function_this_and_class_elements() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class Registry {
  build(): unknown[] {
    return [function () { return 0; }, this, class Entry { id: number = 0; }];
  }
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(crate_has_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::Closure(_)
    )));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
