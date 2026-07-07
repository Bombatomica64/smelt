//! Regression tests for the HIR-lowering fixes that cleared the first
//! es-toolkit whole-crate MIR-lowering gates.
//!
//! Each test pins the observable HIR shape a later MIR-lowering pass depends on:
//! a `String.raw` tagged template becoming string concatenation, an
//! `arr.length =` write becoming a truncating splice, a `let` arrow binding a
//! closure value rather than lifting to an immutable item, a `.rejects.toThrow`
//! test callback being marked async, and `new Request(...)` becoming a
//! marker-bearing host record.

use super::*;

/// `String.raw`…`` lowers to concatenation of the template's *raw* quasis
/// (backslash escapes verbatim) interleaved with the substitutions, producing a
/// `string`. Tagged templates were previously rejected outright.
#[test]
fn string_raw_tagged_template_lowers_to_raw_string() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function raw(name: string): string {
  return String.raw`a\n${name}`;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "raw")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "a\\n"
        )),
        "expected the raw (unescaped) quasi `a\\n` to be preserved verbatim",
    );
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            (&expr.kind, ctx.krate.types.get(expr.ty)),
            (ExprKind::BinOp { op: BinOp::Add, .. }, Some(Type::String))
        )),
        "expected String.raw to lower to string concatenation",
    );
    Ok(())
}

/// `arr.length = n` (the array-resize idiom) lowers to an in-place truncating
/// splice rather than an assignment to a `Len` expression, which MIR rejects.
#[test]
fn array_length_assignment_lowers_to_splice() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function truncate<T>(arr: T[], n: number): T[] {
  arr.length = n;
  return arr;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "truncate")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::ListSplice {
                delete_count: None,
                mutate: true,
                ..
            }
        )),
        "expected `arr.length = n` to lower to a truncating in-place splice",
    );
    Ok(())
}

/// A `let` variable initialized to an arrow and later reassigned binds a
/// closure-valued local (an `ExprKind::Closure` in the body) rather than lifting
/// the arrow to an immutable function item, so the reassignment has a real place
/// to write to.
#[test]
fn let_arrow_reassignment_binds_closure_local() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function pick<T>(values: Array<(a: T, b: T) => boolean>): (a: T, b: T) => boolean {
  let comparator = (a: T, b: T) => a === b;
  const last = values[0];
  if (typeof last === 'function') {
    comparator = last;
  }
  return comparator;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "pick")?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::Closure(_))),
        "expected the `let` arrow to bind a closure value, not lift to an item",
    );
    Ok(())
}

/// A non-`async` test callback that returns `expect(promise).rejects.toThrow()`
/// desugars into an inlined `await`, so the synthesized test function must be
/// marked async (emitting `#[tokio::test] async fn`), matching JavaScript's
/// "the framework awaits the returned promise" semantics.
#[test]
fn rejects_matcher_marks_test_function_async() -> Result<(), String> {
    let source = ts!(r#"
import { test, expect } from "vitest";

async function fails(): Promise<number> {
  throw new Error("boom");
}

test("rejects", () => {
  return expect(fails()).rejects.toThrow("boom");
});
"#);
    let mut ctx = HirCtx::new();
    let module_id = lower_path_ok(source, "src/rejects.test.ts", &mut ctx)?;
    let module = module(&ctx, module_id)?;
    let test_fn = named_function_item(&ctx, module, "test_rejects")?;
    ensure!(test_fn.is_test);
    ensure!(
        test_fn.is_async,
        "a `.rejects.toThrow` callback inlines an await, so the test must be async",
    );
    let body = function_body(&ctx, test_fn)?;
    ensure!(
        body.async_state_machine.is_some(),
        "an async test function must carry an async state machine",
    );
    Ok(())
}

/// `new Request(...)` lowers to a marker-bearing host record carrying the
/// `__smelt_request` identity key (the marker-only host-object model), so
/// `isPlainObject(new Request(...))` folds to `false` via the marker.
#[test]
fn new_request_lowers_to_marker_record() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"const r = new Request("http://localhost");"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let body = module_body(&ctx, module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            &expr.kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_request"
        )),
        "expected `new Request(...)` to carry the `__smelt_request` marker key",
    );
    Ok(())
}

/// After an `if (array == null) { array = ... }` default initialization, a bare
/// `array[i] = value` write lowers to an assignable `Index` target because the
/// nullish guard's inverse plus the branch reassignment narrow `array` to a
/// concrete list (rather than leaving it `Optional`, which produces a
/// non-assignable `OptionalIndex`).
#[test]
fn index_write_after_default_init_uses_index_target() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function copyArray<T>(source: T[], array?: T[]): T[] {
  const length = source.length;
  if (array == null) {
    array = new Array(length);
  }
  for (let i = 0; i < length; i++) {
    array[i] = source[i];
  }
  return array;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = function_item(&ctx, module, 0)?;
    let body = function_body(&ctx, function)?;
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::Index { .. })),
        "expected the narrowed `array[i] = ...` write to lower to an Index place",
    );
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(&expr.kind, ExprKind::OptionalIndex { .. })),
        "the write target must not remain an OptionalIndex after null narrowing",
    );
    Ok(())
}
