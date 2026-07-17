//! Regression coverage for module-level mutable state (mutable globals).
//!
//! A module-level `let`/`var` binding mutated anywhere in the module is lifted
//! to an `Item::MutableGlobal`: reads lower to `ExprKind::GlobalGet` and writes
//! desugar to `ExprKind::GlobalSet` (whose value is the stored value, so
//! `++`/`+=` compose). Non-mutated module `let`s keep the existing inline
//! paths, and the V1 constraints (literal initializer, primitive type) are
//! explicit named blockers.

use super::*;

/// Return whether any body in the crate contains an expression matching `pred`.
fn crate_has(ctx: &HirCtx, pred: impl Fn(&ExprKind) -> bool) -> bool {
    ctx.krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| pred(&expr.kind)))
}

/// Return whether the crate contains a lifted mutable-global item.
fn crate_has_mutable_global(ctx: &HirCtx) -> bool {
    ctx.krate
        .items
        .iter()
        .any(|item| matches!(item, Item::MutableGlobal(_)))
}

#[test]
fn mutated_module_let_lowers_reads_and_writes_through_globals() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
let idCounter = 0;

export function uniqueId(): number {
  return ++idCounter;
}

export function current(): number {
  return idCounter;
}
"),
        &mut ctx,
    )?;
    ensure!(
        crate_has_mutable_global(&ctx),
        "mutated module let should lift to an Item::MutableGlobal",
    );
    ensure!(
        crate_has(&ctx, |kind| matches!(kind, ExprKind::GlobalGet { .. })),
        "reads of a lifted binding should lower to GlobalGet",
    );
    ensure!(
        crate_has(&ctx, |kind| matches!(kind, ExprKind::GlobalSet { .. })),
        "`++` of a lifted binding should lower to GlobalSet",
    );
    Ok(())
}

#[test]
fn module_map_const_rematerializes_entries_inside_functions() -> Result<(), String> {
    // A module-level literal `new Map([...])` const is a string-keyed dictionary.
    // A function that reads it must re-materialize the full dictionary (like the
    // object-literal const path), not inline an empty default whose real
    // construction only lives in the never-called module body.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const table = new Map([["a", "x"], ["b", "y"]]);

export function lookup(key: string): string {
  return table.get(key) ?? key;
}
"#),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "lookup")?;
    let body = function_body(&ctx, function)?;
    let dict_entries = body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            ExprKind::DictLit(entries) => Some(entries.len()),
            _ => None,
        })
        .ok_or_else(|| "expected a re-materialized dict literal in the function body".to_owned())?;
    ensure!(
        dict_entries == 2,
        "function reading a module Map const should re-materialize all entries, got {dict_entries}",
    );
    Ok(())
}

#[test]
fn prefix_increment_returns_global_set_result() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
let counter = 0;

export function bump(): number {
  return ++counter;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "bump")?;
    let body = function_body(&ctx, function)?;
    // The returned expression is the GlobalSet itself (new value).
    let returned = body
        .stmts
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Return(Some(expr)) => Some(*expr),
            _ => None,
        })
        .ok_or_else(|| "expected a return statement".to_owned())?;
    let returned_kind = &body.exprs[returned.0 as usize].kind;
    ensure!(
        matches!(returned_kind, ExprKind::GlobalSet { .. }),
        "prefix increment should return the GlobalSet (new value), got {returned_kind:?}",
    );
    Ok(())
}

#[test]
fn postfix_increment_returns_old_value_temp() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
let counter = 0;

export function bumpAfter(): number {
  return counter++;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "bump_after")?;
    let body = function_body(&ctx, function)?;
    // The returned expression is a local temp holding the pre-increment value,
    // while a GlobalSet statement performs the store.
    let returned = body
        .stmts
        .iter()
        .find_map(|stmt| match stmt {
            Stmt::Return(Some(expr)) => Some(*expr),
            _ => None,
        })
        .ok_or_else(|| "expected a return statement".to_owned())?;
    let returned_kind = &body.exprs[returned.0 as usize].kind;
    ensure!(
        matches!(returned_kind, ExprKind::Local(_)),
        "postfix increment should return the old-value temp local, got {returned_kind:?}",
    );
    ensure!(
        body.exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::GlobalSet { .. })),
        "postfix increment should still store through GlobalSet",
    );
    Ok(())
}

#[test]
fn compound_assignment_reads_current_value_through_global_get() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
let total = 0;

export function add(amount: number): number {
  total += amount;
  return total;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let function = named_function_item(&ctx, module, "add")?;
    let body = function_body(&ctx, function)?;
    // `total += amount` desugars to GlobalSet(total, GlobalGet(total) + amount).
    let has_set_of_binop_over_get = body.exprs.iter().any(|expr| {
        let ExprKind::GlobalSet { value, .. } = &expr.kind else {
            return false;
        };
        let ExprKind::BinOp { op: BinOp::Add, lhs, .. } = &body.exprs[value.0 as usize].kind else {
            return false;
        };
        matches!(body.exprs[lhs.0 as usize].kind, ExprKind::GlobalGet { .. })
    });
    ensure!(
        has_set_of_binop_over_get,
        "`+=` should desugar to GlobalSet over GlobalGet + rhs",
    );
    Ok(())
}

#[test]
fn var_binding_is_treated_like_let() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
var flag = false;

export function toggle(): boolean {
  flag = !flag;
  return flag;
}
"),
        &mut ctx,
    )?;
    ensure!(
        crate_has_mutable_global(&ctx),
        "mutated module var should lift like let",
    );
    ensure!(
        crate_has(&ctx, |kind| matches!(kind, ExprKind::GlobalSet { .. })),
        "assignment to a lifted var should lower to GlobalSet",
    );
    Ok(())
}

#[test]
fn non_mutated_module_let_keeps_inline_path() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
let greeting = 'hello';

export function greet(): string {
  return greeting;
}
"),
        &mut ctx,
    )?;
    ensure!(
        !crate_has_mutable_global(&ctx),
        "non-mutated module let must not lift to a mutable global",
    );
    ensure!(
        !crate_has(&ctx, |kind| matches!(
            kind,
            ExprKind::GlobalGet { .. } | ExprKind::GlobalSet { .. }
        )),
        "non-mutated module let reads must keep the existing inline path",
    );
    Ok(())
}

#[test]
fn function_local_binding_shadows_module_global() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
let counter = 0;

export function bump(): number {
  counter += 1;
  return counter;
}

export function shadowed(): number {
  let counter = 10;
  counter += 1;
  return counter;
}
"),
        &mut ctx,
    )?;
    let module = ctx.krate.modules.last().ok_or("missing module")?;
    let shadowed = named_function_item(&ctx, module, "shadowed")?;
    let body = function_body(&ctx, shadowed)?;
    ensure!(
        !body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::GlobalGet { .. } | ExprKind::GlobalSet { .. }
        )),
        "a same-named function local must shadow the module global",
    );
    Ok(())
}

#[test]
fn non_literal_initializer_is_a_named_blocker() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
function seed(): number {
  return 41;
}

let counter = seed();

export function bump(): number {
  counter = counter + 1;
  return counter;
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(
        &errors,
        "module-level mutable binding initializer must be a literal for now",
    )
}

#[test]
fn non_primitive_type_is_a_named_blocker() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
let holder: unknown = 0;

export function stash(value: unknown): void {
  holder = value;
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(
        &errors,
        "module-level mutable bindings support primitive types for now",
    )
}

#[test]
fn narrowing_facts_do_not_leak_across_sibling_tests() -> Result<(), String> {
    // Regression: an observed-type narrowing recorded in one test body
    // (`array1 = /c/.exec(...)` observing `Optional<SmeltMatch>`) leaked into a
    // sibling test that declares its own `array1`, turning its indexed write
    // target into a non-assignable optional projection (`OptionalIndex`).
    let mut ctx = HirCtx::new();
    lower_path_ok(
        ts!(r"
import { describe, expect, it } from 'vitest';

describe('narrowing scope', () => {
  it('records a match narrowing', () => {
    let array1: any = [1, 2, 3];
    array1 = /c/.exec('abcde');
    expect(array1).toBe(array1);
  });

  it('declares its own array and writes by index', () => {
    let array1: any[] = [];
    array1 = ['a', 'b', 'c'];
    array1[1] = array1;
    expect(array1).toBe(array1);
  });
});
"),
        "src/narrowing-scope.spec.ts",
        &mut ctx,
    )?;
    // No assignment target in any body may be an optional projection.
    for body in &ctx.krate.bodies {
        for stmt in &body.stmts {
            if let Stmt::Assign { target, .. } = stmt {
                let kind = &body.exprs[target.0 as usize].kind;
                ensure!(
                    !matches!(
                        kind,
                        ExprKind::OptionalIndex { .. } | ExprKind::OptionalField { .. }
                    ),
                    "assignment target must not be an optional projection, got {kind:?}",
                );
            }
        }
    }
    Ok(())
}
