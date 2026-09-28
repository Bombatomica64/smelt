//! Regression tests for the value type of an unannotated `new Promise(executor)`.
//!
//! With no `Promise<T>` type argument and no contextual type, the promise's
//! value comes from what the executor passes to `resolve` (see
//! `lowering::promise_resolution`). These pin each arm of that rule, and that a
//! contextual type still wins.

use super::*;
use smelt_hir::AsyncOp;

/// The HIR types of every `AsyncOp::Promise` expression, in lowering order.
fn promise_types(ctx: &HirCtx) -> Vec<Type> {
    ctx.krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter(|expr| {
            matches!(
                expr.kind,
                ExprKind::AsyncOp {
                    op: AsyncOp::Promise,
                    ..
                }
            )
        })
        .filter_map(|expr| ctx.krate.types.get(expr.ty).cloned())
        .collect()
}

/// The resolved value type of the only `new Promise(..)` in `source`.
fn only_promise_output(source: &str) -> Result<Type, String> {
    let mut ctx = HirCtx::new();
    lower_ok(source, &mut ctx)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    let types = promise_types(&ctx);
    let [Type::Future(output)] = types.as_slice() else {
        return Err(format!("expected exactly one promise future, found {types:?}"));
    };
    ctx.krate
        .types
        .get(*output)
        .cloned()
        .ok_or_else(|| "promise output type is not interned".to_owned())
}

#[test]
fn resolve_through_a_nested_timer_callback_types_the_promise() -> Result<(), String> {
    let output = only_promise_output(ts!(r"
export function later() {
  return new Promise((resolve) => setTimeout(() => resolve(42)));
}
"))?;
    ensure!(output == Type::Float, "expected Future<Float>, got Future<{output:?}>");
    Ok(())
}

#[test]
fn resolve_with_one_type_on_every_branch_types_the_promise() -> Result<(), String> {
    let output = only_promise_output(ts!(r"
export function pick(flag: boolean) {
  return new Promise((resolve) => {
    if (flag) {
      resolve('yes');
    } else {
      resolve('no');
    }
  });
}
"))?;
    ensure!(output == Type::String, "expected Future<String>, got Future<{output:?}>");
    Ok(())
}

#[test]
fn resolve_with_disagreeing_types_is_unknown() -> Result<(), String> {
    let output = only_promise_output(ts!(r"
export function mixed(flag: boolean) {
  return new Promise((resolve) => (flag ? resolve('text') : resolve(7)));
}
"))?;
    ensure!(output == Type::Unknown, "expected Future<Unknown>, got Future<{output:?}>");
    Ok(())
}

#[test]
fn resolve_called_without_a_value_stays_valueless() -> Result<(), String> {
    let output = only_promise_output(ts!(r"
export function settle() {
  return new Promise((resolve) => setTimeout(() => resolve()));
}
"))?;
    ensure!(output == Type::None, "expected Future<None>, got Future<{output:?}>");
    Ok(())
}

#[test]
fn resolve_with_and_without_a_value_is_optional() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function maybe(flag: boolean) {
  return new Promise((resolve) => (flag ? resolve(1) : resolve()));
}
"),
        &mut ctx,
    )?;
    let types = promise_types(&ctx);
    let [Type::Future(output)] = types.as_slice() else {
        return Err(format!("expected exactly one promise future, found {types:?}"));
    };
    let Some(Type::Optional(inner)) = ctx.krate.types.get(*output) else {
        return Err(format!("expected Future<Optional<Float>>, got {types:?}"));
    };
    ensure!(ctx.krate.types.get(*inner) == Some(&Type::Float));
    Ok(())
}

#[test]
fn a_declared_return_type_still_decides_the_promise_value() -> Result<(), String> {
    let output = only_promise_output(ts!(r"
export function text(): Promise<string> {
  return new Promise((resolve) => setTimeout(() => resolve('x')));
}
"))?;
    ensure!(output == Type::String, "expected Future<String>, got Future<{output:?}>");
    Ok(())
}
