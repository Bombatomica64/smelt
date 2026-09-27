//! Regression tests: a generic call instantiates EVERY callee type parameter.
//!
//! HIR type parameters are identified by name, so a callee parameter that
//! argument inference left unbound used to reach the caller as a raw
//! `TypeParam` — indistinguishable from the caller's own same-named parameter
//! (`class Router<T> { m = createNullObject() }` typed `m` with `Router`'s `T`)
//! or simply unbound. The rule these tests pin
//! (`ModuleBuilder::complete_callee_type_substitution`): a callee parameter no
//! argument binds instantiates to `unknown` — the type the emitted Rust
//! definition returns for it, since codegen binds callee generics from
//! arguments only — even when the source declares a default, a constraint or
//! the call has a contextual type. Methods', static methods' and lifted
//! arrows' own parameters are erased in their definitions outright, so they
//! always take `unknown`.

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

/// The rendered type of every call expression in the crate.
fn call_types(ctx: &HirCtx) -> Vec<String> {
    ctx.krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .filter(|expr| matches!(expr.kind, ExprKind::Call { .. }))
        .map(|expr| type_text(ctx, expr.ty))
        .collect()
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
fn an_unbound_callee_parameter_in_a_generic_class_field_is_not_the_class_parameter()
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
    // receiver `Router<number>` then rewrote to `Dict<String, Float>`. The
    // default `string` is not adopted: `makeBag`'s emitted definition erases
    // its return-only `T`, and the call must type as what it returns.
    ensure_eq!(field_type(&ctx, "Router", "m")?, "Dict<String, Unknown>");
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
fn a_contextual_type_does_not_instantiate_the_call() -> Result<(), String> {
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
    let call_tys = call_types(&ctx);
    // The annotation types the LOCAL / FIELD (converted from the call); the
    // call itself is what the erased definition returns.
    ensure!(
        !call_tys.is_empty() && call_tys.iter().all(|ty| ty == "Dict<String, Unknown>"),
        "every `makeBag()` call should be `Dict<String, Unknown>`, got {call_tys:?}"
    );
    ensure_eq!(value_type(&ctx, "counts")?, "Dict<String, Float>");
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
    // Never `Dict<String, T>` with `tally`'s `T`.
    ensure_eq!(value_type(&ctx, "seen")?, "Dict<String, Unknown>");
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
fn defaults_and_constraints_do_not_instantiate_an_unbound_parameter() -> Result<(), String> {
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
    // `A` is argument-bound and kept; `B` has only a default, which the
    // emitted definition does not honour.
    ensure_eq!(value_type(&ctx, "pair")?, "(String, Unknown)");
    ensure_eq!(value_type(&ctx, "scores")?, "List<Unknown>");
    // Never the raw callee `T`.
    ensure_eq!(value_type(&ctx, "nothing")?, "List<Unknown>");
    Ok(())
}

#[test]
fn a_parameter_only_reachable_through_an_omitted_argument_is_unknown() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function* range<T = number>(
  length: number,
  valueOrMapper?: T | ((i: number) => T),
): Generator<T> {
  for (let i = 0; i < length; i++) {
    yield valueOrMapper as T;
  }
}
export function firstThree(): number {
  let total = 0;
  for (const value of range(3)) {
    total += 1;
  }
  return total;
}
"),
        &mut ctx,
    )?;
    // radash's `range`: codegen erases `T` (its only inference source is the
    // optional function-union argument), so its generator yields
    // `SmeltUnknown`. Typing `range(3)` from the default `number` produced a
    // `Generator<number>` local that the returned generator cannot fill
    // (E0308); the call must carry the erased element type instead.
    let call_tys = call_types(&ctx);
    ensure!(
        call_tys.iter().any(|ty| ty.contains("Unknown")),
        "`range(3)` should yield `Unknown`, got {call_tys:?}"
    );
    ensure!(
        !call_tys.iter().any(|ty| ty.contains("Generator<Float")),
        "`range(3)` must not be instantiated from the default, got {call_tys:?}"
    );
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
