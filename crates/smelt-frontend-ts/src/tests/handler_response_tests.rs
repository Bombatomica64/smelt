//! Regression coverage for calls whose callee or receiver was only known after
//! lowering: a computed-member read of a callable union or an erased value,
//! and `.test(..)` on an expression typed `RegExp`. Each used to lower to a
//! value other than the call's result, with no diagnostic.

use super::*;

/// Whether the named function's body contains an expression matching `pred`.
fn function_has_expr(
    ctx: &HirCtx,
    module_id: ModuleId,
    name: &str,
    pred: impl Fn(&ExprKind) -> bool,
) -> Result<bool, String> {
    let module = module(ctx, module_id)?;
    let item = named_function_item(ctx, module, name)?;
    let body = function_body(ctx, item)?;
    Ok(body.exprs.iter().any(|expr| pred(&expr.kind)))
}

#[test]
fn computed_member_call_on_callable_union_calls_the_value() -> Result<(), String> {
    // `xs[0][0](b, next)` over `Handler | Middleware` used to lower to its
    // FIRST ARGUMENT (`b`), so the handler never ran.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
type Next = () => Promise<void>
type A<R = any> = (b: string, next: Next) => R
type B<R = string> = (b: string, next: Next) => Promise<R | void>
type H = A | B
function pick(xs: [H, string][], b: string) {
  return xs[0][0](b, async () => {})
}
console.log(pick([[(b: string) => b + '!', 'r']], 'x'))
"),
        &mut ctx,
    )?;
    ensure!(function_has_expr(&ctx, module_id, "pick", |kind| matches!(
        kind,
        ExprKind::ClosureCall { args, .. } if args.len() == 2
    ))?);
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn computed_member_call_on_erased_value_calls_the_value() -> Result<(), String> {
    // An `any` element used to lower to a literal `undefined`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function first(xs: any[]): unknown {
  return xs[0](1, 2)
}
console.log(first([(a: number, b: number) => a + b]))
"),
        &mut ctx,
    )?;
    ensure!(function_has_expr(&ctx, module_id, "first", |kind| matches!(
        kind,
        ExprKind::ClosureCall { args, .. } if args.len() == 2
    ))?);
    ensure!(!function_has_expr(&ctx, module_id, "first", |kind| matches!(
        kind,
        ExprKind::Literal(Literal::None)
    ))?);
    Ok(())
}

#[test]
fn regexp_test_on_a_call_receiver_runs_the_regex() -> Result<(), String> {
    // `build(k).test(path)` read the host class's `test` slot as an erased
    // field and called it, answering `null` (false) for every path.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
function build(path: string): RegExp {
  return new RegExp('^' + path + '$')
}
function matches(k: string, path: string): boolean {
  return build(k).test(path)
}
console.log(matches('a.*', 'abc'))
"),
        &mut ctx,
    )?;
    ensure!(function_has_expr(&ctx, module_id, "matches", |kind| matches!(
        kind,
        ExprKind::RegexTest { .. }
    ))?);
    Ok(())
}
