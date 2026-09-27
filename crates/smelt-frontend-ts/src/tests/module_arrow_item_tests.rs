//! Regression coverage for module-level arrow consts called from item bodies.
//!
//! A never-reassigned module `const f = (..) => ..` read from any item body —
//! a class member, its own body, another function — is lowered as a function
//! item and every such call targets that item. The failure these guard is a
//! call that silently bound `module_global_function_expression`'s
//! default-returning stub instead (`0`, `""`, an empty record).

use super::*;

/// Find the function item whose Rust-facing name is `name` or the
/// module-qualified private spelling `name__module_<identity>`.
fn arrow_item(ctx: &HirCtx, name: &str) -> Result<smelt_hir::ItemId, String> {
    let qualified = format!("{name}__module_");
    ctx.krate
        .items
        .iter()
        .enumerate()
        .find_map(|(index, item)| {
            let Item::Function(function) = item else {
                return None;
            };
            let symbol = ctx.krate.symbols.get(function.name)?;
            (symbol == name || symbol.starts_with(&qualified))
                .then(|| smelt_hir::ItemId(u32::try_from(index).unwrap_or(u32::MAX)))
        })
        .ok_or_else(|| format!("no function item lifted for arrow `{name}`"))
}

/// Return whether `body` calls `item` directly (`call Item(item)(..)`).
fn body_calls_item(body: &smelt_hir::Body, item: smelt_hir::ItemId) -> bool {
    body.exprs.iter().any(|expr| {
        let ExprKind::Call { callee, .. } = &expr.kind else {
            return false;
        };
        body.exprs
            .get(callee.0 as usize)
            .is_some_and(|callee| matches!(callee.kind, ExprKind::Item(target) if target == item))
    })
}

/// The body owned by function item `item`.
fn item_body(ctx: &HirCtx, item: smelt_hir::ItemId) -> Result<&smelt_hir::Body, String> {
    let Some(Item::Function(function)) = ctx.krate.items.get(item.0 as usize) else {
        return Err(format!("item {item:?} is not a function"));
    };
    function_body(ctx, function)
}

#[test]
fn class_method_calls_module_arrow_item() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
const add = (a: number, b?: number): number => a + (b ?? 1);

class Counter {
  count(): number {
    return add(41);
  }
}

console.log(new Counter().count());
"),
        &mut ctx,
    )?;
    let add = arrow_item(&ctx, "add")?;
    // Some method body calls the lifted item itself.
    let called = ctx.krate.bodies.iter().any(|body| body_calls_item(body, add));
    ensure!(called, "the class method must call the lifted `add` item");
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn self_recursive_arrow_calls_its_own_item() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
const fact = (n: number): number => (n <= 1 ? 1 : n * fact(n - 1));
export const join: (...parts: string[]) => string = (head: string, ...rest: string[]): string =>
  rest.length ? head + '/' + join(...rest) : head;
console.log(fact(5), join('a', 'b'));
"),
        &mut ctx,
    )?;
    for name in ["fact", "join"] {
        let item = arrow_item(&ctx, name)?;
        let body = item_body(&ctx, item)?;
        ensure!(
            body_calls_item(body, item),
            "the recursive call inside `{name}` must target `{name}`'s own item"
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

#[test]
fn modeled_host_class_intersection_keeps_the_class() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
type Brand<T> = { _data: T };
export function wrap(text: string): Response & Brand<string> {
  return new Response(text) as Response & Brand<string>;
}
"),
        &mut ctx,
    )?;
    let module = module(&ctx, module_id)?;
    let wrap = named_function_item(&ctx, module, "wrap")?;
    ensure!(
        matches!(
            ctx.krate.types.get(wrap.return_ty),
            Some(Type::Class { name, .. })
                if ctx.krate.names.get(*name).or_else(|| ctx.krate.symbols.get(*name))
                    == Some("Response")
        ),
        "`Response & Brand` must lower to the host `Response` class, got {:?}",
        ctx.krate.types.get(wrap.return_ty)
    );
    Ok(())
}
