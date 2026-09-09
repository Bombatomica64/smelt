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
fn an_unannotated_non_literal_initializer_is_a_named_blocker() -> Result<(), String> {
    // A non-literal initializer IS lowered (see the test below), but only when
    // the binding's type is known: the classification pass runs before imports
    // and function items resolve, so with neither an annotation nor a literal
    // to infer from there is nothing to type the cell with.
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
        "module-level mutable binding with a non-literal initializer needs an explicit type \
         annotation",
    )
}

#[test]
fn an_annotated_non_literal_initializer_lowers_through_an_initializer_item() -> Result<(), String> {
    // The V1 restrictions were "literal initializer" and "primitive type", and
    // Hono's `router/reg-exp-router/router.ts` breaks both at once with
    // `let cache: Record<string, RegExp> = createNullObject()`. A non-literal
    // initializer now becomes a synthesized nullary function the cell calls
    // lazily, so the global keeps a concrete type and nothing is erased.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const seedCache = (): Record<string, string> => ({});

let cache: Record<string, string> = seedCache();

export function read(key: string): string {
  return cache[key];
}

export function reset(): void {
  cache = seedCache();
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    // The global must carry an `Initializer` item — not a literal, and not the
    // `Pending` placeholder, which reaching MIR would be a compiler bug.
    let inits = ctx
        .krate
        .items
        .iter()
        .filter_map(|item| match item {
            smelt_hir::Item::MutableGlobal(global) => Some(&global.init),
            _ => None,
        })
        .collect::<Vec<_>>();
    ensure!(
        inits.len() == 1
            && matches!(
                inits.first(),
                Some(smelt_hir::MutableGlobalInit::Initializer(_))
            ),
        "expected exactly one mutable global with an initializer item, saw {inits:?}",
    );
    Ok(())
}

#[test]
fn a_non_primitive_type_lowers() -> Result<(), String> {
    // `unknown` used to be rejected by the primitive-type restriction. A
    // non-`Copy` global is now backed by a `RefCell` rather than a `Cell`, so
    // any type works.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
let holder: unknown = 0;

export function stash(value: unknown): void {
  holder = value;
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn writing_through_a_non_primitive_global_lowers() -> Result<(), String> {
    // Hono's `router/reg-exp-router/router.ts` shape. `cache[key] = value`
    // mutates the value the cell HOLDS, which used to be a named blocker
    // because a `GlobalGet` yields a copy and the write would land on it. It
    // now lowers to `Place::Global`, which names the cell as the assignment
    // root so no copy is made. See `blocker-logs/hono-h6-place-global.md`.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
const seedCache = (): Record<string, string> => ({});

let cache: Record<string, string> = seedCache();

export function put(key: string, value: string): void {
  cache[key] = value;
}

export function reset(): void {
  cache = seedCache();
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn a_global_written_through_only_is_still_lifted() -> Result<(), String> {
    // A binding whose ONLY mutation is a write through it is still module state
    // that every function shares. The lift used to require a whole-binding
    // reassignment, which would now leave the write on a module-local copy —
    // the exact silent-loss defect `Place::Global` exists to prevent. There is
    // no `cache = ...` anywhere in this fixture.
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
let cache: Record<string, number> = {};

export function put(key: string, value: number): void {
  cache[key] = value;
}

export function get(key: string): number {
  return cache[key];
}
"),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    let lifted = ctx.krate.items.iter().any(|item| {
        matches!(item, Item::MutableGlobal(global)
            if ctx.krate.symbols.get(global.name) == Some("cache"))
    });
    ensure!(
        lifted,
        "a global that is only written THROUGH must still be lifted to a cell",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn a_nested_write_through_a_non_primitive_global_is_a_named_blocker() -> Result<(), String> {
    // `cache[a][b] = v` still blocks. The inner `cache[a]` has to produce a
    // value, and whether that value shares storage with the cell is the
    // handle-versus-value question `Place::Global` avoids asking — guessing it
    // loses the write for one of the two container representations with no
    // diagnostic. The blocker names the shape rather than the family.
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
let buckets: Record<string, Record<string, string>> = {};

export function put(outer: string, inner: string, value: string): void {
  buckets[outer][inner] = value;
}
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "written through a nested projection")
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
