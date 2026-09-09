//! Regression tests for the type hint a closure call gives its arguments.
//!
//! A call to a const-bound arrow (or any local callable) collects its arguments
//! in `local_callable_call`. That loop lowered every argument with NO hint,
//! while the named-function, `new`-expression and IIFE paths all pass the
//! callee's parameter type down, so an object literal written inline against an
//! arrow's parameter was built as a bare `Dict` of erased values and only then
//! converted to the struct. These tests pin both halves of the rule that
//! replaced it: hint when the parameter type is a shape to lower into, and do
//! NOT hint when the parameter type is itself erased, where hinting would move
//! the erasure out of the boundary adapter and into ordinary data flow.

use super::*;

/// The HIR type of the first expression whose kind matches `pred`.
fn first_expr_ty(
    ctx: &HirCtx,
    pred: impl Fn(&ExprKind) -> bool + Copy,
) -> Result<Type, String> {
    ctx.krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .find(|expr| pred(&expr.kind))
        .and_then(|expr| ctx.krate.types.get(expr.ty).cloned())
        .ok_or_else(|| "no expression matched the predicate".to_owned())
}

#[test]
fn an_object_literal_against_a_const_bound_arrow_takes_the_parameter_struct()
-> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
interface Opts {
  status?: number;
  label?: string;
}
const showRequired = (opts: Opts): string => `${opts.status}`;
export const shown = showRequired({ status: 8, label: 'x' });
"),
        &mut ctx,
    )?;

    // The literal is the struct itself, not a record that is converted after
    // the fact. `Dict` here would mean every value was erased on the way in.
    let literal_ty = first_expr_ty(&ctx, |kind| matches!(kind, ExprKind::DictLit(_)))?;
    ensure!(
        matches!(&literal_ty, Type::Class { name, .. } if ctx.krate.symbols.get(*name) == Some("Opts")),
        "inline literal should lower at the parameter's struct type, got {literal_ty:?}"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn an_optional_parameter_still_hints_through_the_option() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
interface Opts {
  status?: number;
}
const showOptional = (opts?: Opts): string => `${opts?.status}`;
export const shown = showOptional({ status: 7 });
"),
        &mut ctx,
    )?;

    // The hint arrives as `Optional<Opts>`; the literal has to see through the
    // option to the struct rather than falling back to a record.
    let literal_ty = first_expr_ty(&ctx, |kind| matches!(kind, ExprKind::DictLit(_)))?;
    ensure!(
        matches!(&literal_ty, Type::Class { name, .. } if ctx.krate.symbols.get(*name) == Some("Opts")),
        "literal at an optional parameter should still lower as the struct, got {literal_ty:?}"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn an_array_literal_against_an_erased_parameter_keeps_its_element_type() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
const spreadFn = (values: unknown[]): number => values.length;
export const size = spreadFn([1, 2]);
"),
        &mut ctx,
    )?;

    // `unknown[]` is not a shape to lower into: hinting it would build the
    // list AS `List<Unknown>`, erasing each element at construction, where the
    // unhinted lowering builds `List<Float>` and erases at the call through the
    // boundary adapter codegen already emits. es-toolkit's `spread.spec.ts`
    // measured that difference as +14 avoidable erasure.
    let literal_ty = first_expr_ty(&ctx, |kind| matches!(kind, ExprKind::ListLit(_)))?;
    let Type::List(item) = literal_ty else {
        return Err(format!("expected a list literal type, got {literal_ty:?}"));
    };
    ensure_eq!(ctx.krate.types.get(item), Some(&Type::Float));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn a_typed_array_parameter_does_hint_its_elements() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
interface Opts {
  status?: number;
}
const total = (items: Opts[]): number => items.length;
export const size = total([{ status: 1 }]);
"),
        &mut ctx,
    )?;

    // The complement of the test above: a list of a real struct IS a shape to
    // lower into, so the elements are structs rather than records.
    let literal_ty = first_expr_ty(&ctx, |kind| matches!(kind, ExprKind::ListLit(_)))?;
    let Type::List(item) = literal_ty else {
        return Err(format!("expected a list literal type, got {literal_ty:?}"));
    };
    let item_ty = ctx
        .krate
        .types
        .get(item)
        .cloned()
        .ok_or_else(|| "missing element type".to_owned())?;
    ensure!(
        matches!(&item_ty, Type::Class { name, .. } if ctx.krate.symbols.get(*name) == Some("Opts")),
        "elements should lower at the parameter's element struct, got {item_ty:?}"
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
