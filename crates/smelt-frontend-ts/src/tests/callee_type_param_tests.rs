//! Regression tests: a generic call instantiates EVERY callee type parameter.
//!
//! HIR type parameters are identified by name, so a callee parameter that
//! argument inference left unbound used to reach the caller as a raw
//! `TypeParam` — indistinguishable from the caller's own same-named parameter
//! (`class Router<T> { m = createNullObject() }` typed `m` with `Router`'s `T`)
//! or simply unbound. The rule these tests pin
//! (`ModuleBuilder::complete_callee_type_substitution`): an unbound callee
//! parameter takes the contextual result type, else its declared default, else
//! its constraint, else `unknown`. Callees whose generics the definition erases
//! (methods, static methods, lifted arrows) take `unknown`, the type their
//! emitted definition actually returns.

use super::*;

/// Render a HIR type the way `dump-hir` does, so assertions read as source.
fn type_text(ctx: &HirCtx, ty: smelt_hir::TypeId) -> String {
    smelt_hir::type_display(&ctx.krate, ty)
}

/// Whether `symbol` spells `text`, in either the interned or the original
/// (pre-sanitization) name table.
fn names(ctx: &HirCtx, symbol: smelt_hir::Symbol, text: &str) -> bool {
    ctx.krate.symbols.get(symbol) == Some(text) || ctx.krate.names.get(symbol) == Some(text)
}

/// The rendered type of the value named `name`: a local in any body, or a
/// module-level const item (an exported `const` lowers to one).
fn value_type(ctx: &HirCtx, name: &str) -> Result<String, String> {
    let local = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.locals.iter())
        .find(|local| local.name.is_some_and(|symbol| names(ctx, symbol, name)))
        .map(|local| local.ty);
    let item = || {
        ctx.krate.items.iter().find_map(|item| match item {
            Item::Const(const_item) if names(ctx, const_item.name, name) => Some(const_item.ty),
            _ => None,
        })
    };
    local
        .or_else(item)
        .map(|ty| type_text(ctx, ty))
        .ok_or_else(|| format!("no local or const named `{name}`"))
}

/// The rendered type of field `field` on the class named `class`.
fn field_type(ctx: &HirCtx, class: &str, field: &str) -> Result<String, String> {
    ctx.krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Class(item) if names(ctx, item.name, class) => item
                .fields
                .iter()
                .find(|candidate| names(ctx, candidate.name, field))
                .map(|candidate| type_text(ctx, candidate.ty)),
            _ => None,
        })
        .ok_or_else(|| format!("no field `{class}.{field}`"))
}

#[test]
fn an_unbound_callee_parameter_in_a_generic_class_field_takes_its_default()
-> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function makeBag<T = string>(): Record<string, T> {
  const r: Record<string, T> = {};
  return r;
}
class Router<T> {
  m = makeBag();
  items: T[] = [];
}
export const router = new Router<number>();
"),
        &mut ctx,
    )?;
    // Before the fix this was `Dict<String, T>` — the CLASS's `T`, which the
    // receiver `Router<number>` then rewrote to `Dict<String, Float>`.
    ensure_eq!(field_type(&ctx, "Router", "m")?, "Dict<String, String>");
    ensure_eq!(field_type(&ctx, "Router", "items")?, "List<T>");
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn an_any_default_instantiates_to_unknown_not_the_callers_parameter() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function createNullObject<T = any>(): Record<string, T> {
  return Object.create(null);
}
class Router<T> {
  m = createNullObject();
  items: T[] = [];
}
"),
        &mut ctx,
    )?;
    // DYNAMIC BOUNDARY: `= any` is source `any`, the declaration's own "unchecked"
    // default; `unknown` is what it means. What must never happen is `Dict<String, T>`.
    ensure_eq!(field_type(&ctx, "Router", "m")?, "Dict<String, Unknown>");
    Ok(())
}

#[test]
fn the_contextual_type_wins_over_the_default() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function makeBag<T = string>(): Record<string, T> {
  const r: Record<string, T> = {};
  return r;
}
export function total(): number {
  const counts: Record<string, number> = makeBag();
  counts['x'] = 1;
  return counts['x'];
}
class Router<T> {
  handlers: Record<string, T> = makeBag();
}
"),
        &mut ctx,
    )?;
    let call_tys = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter(|expr| matches!(expr.kind, ExprKind::Call { .. }))
        .map(|expr| type_text(&ctx, expr.ty))
        .collect::<Vec<_>>();
    // The CALLS themselves are typed from their context, not merely converted
    // to it: the local's annotation, and the field's declared type in the
    // constructor's initializer.
    ensure!(
        call_tys.iter().any(|ty| ty == "Dict<String, Float>"),
        "annotated local call should be `Dict<String, Float>`, got {call_tys:?}"
    );
    ensure!(
        call_tys.iter().any(|ty| ty == "Dict<String, T>"),
        "field initializer call should be `Dict<String, T>`, got {call_tys:?}"
    );
    ensure!(
        !call_tys.iter().any(|ty| ty == "Dict<String, String>"),
        "no call here should fall back to the default, got {call_tys:?}"
    );
    // The caller's own `T` is the contextual type here, so it is the right answer.
    ensure_eq!(field_type(&ctx, "Router", "handlers")?, "Dict<String, T>");
    Ok(())
}

#[test]
fn a_generic_caller_does_not_alias_a_same_named_callee_parameter() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function makeBag<T = string>(): Record<string, T> {
  const r: Record<string, T> = {};
  return r;
}
export function tally<T>(first: T): T {
  const seen = makeBag();
  seen['a'] = 'b';
  return first;
}
"),
        &mut ctx,
    )?;
    ensure_eq!(value_type(&ctx, "seen")?, "Dict<String, String>");
    Ok(())
}

#[test]
fn a_callers_parameter_flowing_into_a_same_named_callee_parameter_binds_it()
-> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function ident<T = string>(x: T): T {
  return x;
}
function wrap<T>(x: T): T[] {
  return [x];
}
export function tally<T>(first: T): T[] {
  const same = ident(first);
  const listed = wrap(first);
  return listed;
}
"),
        &mut ctx,
    )?;
    // The argument's type IS the caller's `T`, interned identically to the
    // callee's `T`; that must still count as the binding `T := caller's T`
    // rather than as "no evidence" (which would take the default `string`).
    ensure_eq!(value_type(&ctx, "same")?, "T");
    ensure_eq!(value_type(&ctx, "listed")?, "List<T>");
    Ok(())
}

#[test]
fn an_outer_callees_parameter_in_the_hint_is_not_adopted() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function makeBag<T = string>(): Record<string, T> {
  const r: Record<string, T> = {};
  return r;
}
function count<U>(bag: Record<string, U>): number {
  return Object.keys(bag).length;
}
export function run(): number {
  return count(makeBag());
}
"),
        &mut ctx,
    )?;
    // In argument position the contextual type is `count`'s declared
    // `Record<string, U>` — `U` is `count`'s own raw parameter, not a type
    // `run` can name, so it must not become `makeBag`'s instantiation.
    let call_tys = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter(|expr| matches!(expr.kind, ExprKind::Call { .. }))
        .map(|expr| type_text(&ctx, expr.ty))
        .collect::<Vec<_>>();
    ensure!(
        call_tys.iter().any(|ty| ty == "Dict<String, String>"),
        "the inner call should take the default, got {call_tys:?}"
    );
    ensure!(
        !call_tys.iter().any(|ty| ty == "Dict<String, U>"),
        "no call may carry `count`'s `U`, got {call_tys:?}"
    );
    Ok(())
}

#[test]
fn defaults_and_constraints_instantiate_left_to_right() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function pairOf<A, B = A[]>(a: A): [A, B] {
  return [a, [] as unknown as B];
}
function zeros<T extends number>(): T[] {
  return [];
}
function blank<T>(): T[] {
  return [];
}
export const pair = pairOf('x');
export const scores = zeros();
export const nothing = blank();
"),
        &mut ctx,
    )?;
    // `B`'s default names the earlier `A`, which the argument bound.
    ensure_eq!(value_type(&ctx, "pair")?, "(String, List<String>)");
    // No default: the `extends` constraint is what `tsc` infers.
    ensure_eq!(value_type(&ctx, "scores")?, "List<Float>");
    // Neither: `unknown`, never the raw callee `T`.
    ensure_eq!(value_type(&ctx, "nothing")?, "List<Unknown>");
    Ok(())
}

#[test]
fn erased_method_and_arrow_generics_instantiate_to_their_erased_abi() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
class Factory {
  static bag<T = boolean>(): Record<string, T> {
    const r: Record<string, T> = {};
    return r;
  }
  list<U = number>(): U[] {
    return [];
  }
}
const arrowList = <T = string>(): T[] => [];
export function useIt<T, U>(v: T, w: U): number {
  const s = Factory.bag();
  const l = new Factory().list();
  const a = arrowList();
  return l.length + a.length + Object.keys(s).length;
}
"),
        &mut ctx,
    )?;
    // The definitions of these callees render their own parameters as
    // `SmeltUnknown` (method and arrow generics are not materialized), so the
    // call result is the erased type the definition returns. Before the fix
    // all three leaked the raw parameter, which in `useIt` named ITS `T`/`U`.
    ensure_eq!(value_type(&ctx, "s")?, "Dict<String, Unknown>");
    ensure_eq!(value_type(&ctx, "l")?, "List<Unknown>");
    ensure_eq!(value_type(&ctx, "a")?, "List<Unknown>");
    Ok(())
}

#[test]
fn argument_inference_is_unchanged() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function ident<T = string>(x: T): T {
  return x;
}
export const n = ident(3);
"),
        &mut ctx,
    )?;
    // An argument binding always outranks the default.
    ensure_eq!(value_type(&ctx, "n")?, "Float");
    Ok(())
}
