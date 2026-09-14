//! Regression tests for a callback whose NAME is written under a type
//! assertion.
//!
//! `as`, `satisfies`, `!` and parentheses claim a type for a value; none of
//! them changes which function a callback names, so callback selection has to
//! see through them. It did not, so `xs.filter(Boolean as any)` — Hono's
//! spelling in `src/utils/html.ts`, and the usual way to satisfy `filter<T>`'s
//! narrowing overload — reported "array callback methods currently require
//! arrow function callbacks" while the identical `xs.filter(Boolean)` lowered
//! to the real builtin.
//!
//! The end-to-end fixture `69_asserted_callback_name` covers the runtime
//! behaviour for named items and arrow literals. The GLOBAL BUILTIN arm lives
//! here instead: those builtins lower to a closure over `SmeltUnknown` whichever
//! way they are spelled (see
//! `blocker-logs/hono-h49-builtin-callback-operand.md`), so a fixture carrying
//! one would add avoidable erasure to a corpus that holds zero. What has to be
//! asserted is that the asserted spelling lowers at all, and to the same
//! callback the bare spelling picks.

use super::*;

/// Whether any body in the crate holds an expression matching `pred`.
fn crate_has_expr(ctx: &HirCtx, pred: impl Fn(&ExprKind) -> bool) -> bool {
    ctx.krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| pred(&expr.kind)))
}

/// Whether the crate performs the given primitive cast anywhere.
fn crate_has_cast(ctx: &HirCtx, op: PrimitiveCastOp) -> bool {
    crate_has_expr(ctx, |kind| {
        matches!(kind, ExprKind::PrimitiveCast { op: found, .. } if *found == op)
    })
}

#[test]
fn an_asserted_builtin_callback_lowers_like_the_bare_one() -> Result<(), String> {
    let mut bare = HirCtx::new();
    lower_ok(
        ts!(r"
const values: (string | undefined)[] = ['a', undefined];
export const kept = values.filter(Boolean);
"),
        &mut bare,
    )?;
    // The bare spelling runs the builtin's real truthiness coercion rather than
    // a placeholder callback.
    ensure!(crate_has_cast(&bare, PrimitiveCastOp::ToBool));

    let mut asserted = HirCtx::new();
    lower_ok(
        ts!(r"
const values: (string | undefined)[] = ['a', undefined];
export const kept = values.filter<string>(Boolean as any);
"),
        &mut asserted,
    )?;
    // Before the fix this source did not lower at all. It must now select the
    // same callback: the same coercion, not an opaque stand-in.
    ensure!(crate_has_cast(&asserted, PrimitiveCastOp::ToBool));
    ensure!(smelt_hir::validate(&asserted.krate).is_empty());
    Ok(())
}

#[test]
fn an_asserted_mapping_builtin_keeps_its_real_conversion() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
const texts = ['1', '2'];
export const parsed = texts.map(Number as any);
"),
        &mut ctx,
    )?;
    ensure!(crate_has_cast(&ctx, PrimitiveCastOp::ToJsNumber));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn every_assertion_spelling_selects_the_named_item() -> Result<(), String> {
    // `as`, `!`, `satisfies` and parentheses all have to be transparent; a
    // single spelling passing would not show that.
    for callback in [
        "isLong",
        "isLong as Predicate",
        "isLong!",
        "isLong satisfies Predicate",
        "(isLong as Predicate)",
    ] {
        let source = format!(
            "type Predicate = (value: string) => boolean;\n\
             function isLong(value: string): boolean {{ return value.length > 2; }}\n\
             const words = ['a', 'abc'];\n\
             export const kept = words.filter({callback});\n"
        );
        let mut ctx = HirCtx::new();
        lower_ok(&source, &mut ctx).map_err(|error| format!("`{callback}`: {error}"))?;
        ensure!(
            smelt_hir::validate(&ctx.krate).is_empty(),
            "`{callback}` produced an invalid crate"
        );
    }
    Ok(())
}
