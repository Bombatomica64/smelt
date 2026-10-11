//! Regression tests for drifted duplicate helpers in the TypeScript frontend
//! (haiku audit §2.4, `blocker-logs/haiku-audit-followup.md`).
//!
//! These pin the HIR a construct lowers to on the path that used to take a
//! divergent copy of a helper. The emitted-Rust and runtime checks for the same
//! rows live in `smelt-codegen-rust` (`tests/drifted_helper_tests.rs` and the
//! runtime tiers named there).

use super::*;

/// The index operand of every `OptionalIndex` (`.at(i)`) in the crate.
fn at_index_kinds(ctx: &HirCtx) -> Vec<&ExprKind> {
    ctx.krate
        .bodies
        .iter()
        .flat_map(|body| {
            body.exprs.iter().filter_map(move |expr| match &expr.kind {
                ExprKind::OptionalIndex { index, .. } => usize::try_from(index.0)
                    .ok()
                    .and_then(|index| body.exprs.get(index))
                    .map(|index| &index.kind),
                _ => None,
            })
        })
        .collect()
}

/// F0611. `.at(i)` inside an inline callback coerces its index exactly like
/// `.at(i)` outside one: through the shared `optional_at_index_expr`, which
/// casts a non-`number` index with `Number(...)`. The callback copy skipped
/// the coercion, so an erased index reached the emitter raw and was read with a
/// different string-to-number rule: with `i: any = " 1 "`,
/// `[0].map(() => ys.at(i))` answered `ys[0]` and `ys.at(i)` answered `ys[1]`.
#[test]
fn a_callback_at_index_is_coerced_like_a_direct_one() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
const ys = ["a", "b", "c"];
const padded: any = " 1 ";
function direct(xs: string[], i: any): string | undefined {
  return xs.at(i);
}
function viaCallback(xs: string[], i: any): (string | undefined)[] {
  return [0].map(() => xs.at(i));
}
console.log(direct(ys, padded), viaCallback(ys, padded));
"#),
        &mut ctx,
    )?;
    let kinds = at_index_kinds(&ctx);
    ensure!(kinds.len() == 2, "expected two `.at` calls, found {}", kinds.len());
    for kind in kinds {
        ensure!(
            matches!(
                kind,
                ExprKind::PrimitiveCast {
                    op: PrimitiveCastOp::ToJsNumber,
                    ..
                }
            ),
            "an `.at` index must be the `Number(...)` cast, got {kind:?}"
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
