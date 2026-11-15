//! Value reads of module-level objects, object namespaces and switches the
//! literal-label `Match` cannot express.
//!
//! - A module object read as a VALUE is one allocation: every read is a
//!   `GlobalGet` of the same slot (`lowering::module_init::identity_slots`).
//! - An object namespace of methods read as a value is a dictionary of its
//!   members, typed when they share a type.
//! - A `switch` over an erased discriminant, or with a label that is no
//!   literal, is a chain of `===` tests, never a `Match` on literal labels.

use super::*;

/// The `GlobalGet` items a function body reads.
fn global_reads(body: &smelt_hir::Body) -> Vec<smelt_hir::ItemId> {
    body.exprs
        .iter()
        .filter_map(|expr| match expr.kind {
            ExprKind::GlobalGet { item } => Some(item),
            _ => None,
        })
        .collect()
}

#[test]
fn module_object_value_reads_share_one_slot() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const defaults = { weekStartsOn: 0, locale: "en" };
let current: { weekStartsOn: number; locale: string } | undefined = undefined;

export function remember(): void {
  current = defaults;
}

export function same(): boolean {
  return current === defaults;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let remember = global_reads(function_body(
        &ctx,
        named_function_item(&ctx, module, "remember")?,
    )?);
    let same = global_reads(function_body(
        &ctx,
        named_function_item(&ctx, module, "same")?,
    )?);
    let shared = remember.iter().find(|item| same.contains(item));
    ensure!(
        shared.is_some(),
        "both functions should read `defaults` through one slot, got {remember:?} and {same:?}",
    );
    Ok(())
}

#[test]
fn uniform_object_namespace_value_is_a_typed_dict() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export const formatters = {
  y(value: number): string { return String(value); },
  M(value: number): string { return String(value + 1); },
};

export function pick(token: string): ((value: number) => string) | undefined {
  return formatters[token as keyof typeof formatters];
}
"),
        &mut ctx,
    )?;
    let typed_dict = ctx.krate.bodies.iter().flat_map(|body| body.exprs.iter()).any(|expr| {
        matches!(expr.kind, ExprKind::DictLit(_))
            && matches!(
                ctx.krate.types.get(expr.ty),
                Some(Type::Dict(_, value)) if matches!(ctx.krate.types.get(*value), Some(Type::Function(_)))
            )
    });
    ensure!(
        typed_dict,
        "the namespace value should be a Dict<String, Function>"
    );
    Ok(())
}

#[test]
fn switch_over_erased_discriminant_is_an_equality_chain() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function erased(value: unknown): number {
  switch (value) {
    case "a": return 1;
    case undefined: return 2;
    default: return 3;
  }
}

export function text(value: string): number {
  switch (value) {
    case "a": return 1;
    default: return 3;
  }
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let has_match = |name: &str| -> Result<bool, String> {
        let body = function_body(&ctx, named_function_item(&ctx, module, name)?)?;
        Ok(body
            .stmts
            .iter()
            .any(|stmt| matches!(stmt, Stmt::Match { .. })))
    };
    ensure!(
        !has_match("erased")?,
        "an erased discriminant should not lower to a label Match"
    );
    ensure!(
        has_match("text")?,
        "a string discriminant with literal labels keeps its Match"
    );
    Ok(())
}
