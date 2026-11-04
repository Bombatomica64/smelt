//! Which array-literal elements may fill a generic callee's open `T[]` hint.
//!
//! Regression (es-toolkit `uniq.spec.ts`, since #298): `uniq([null,
//! undefined, null, undefined])` filled `T` from the elements' `Type::None`, so
//! the call was instantiated at Rust `()`. That lost which element was `null`
//! and which `undefined` (JavaScript answers `[null, undefined]`) and failed to
//! compile (E0277: `(): SmeltFromUnknown`). See `fills_open_slot`.

use super::*;

/// Types of every expression in the body of the module function `name`.
fn function_expr_types(ctx: &HirCtx, name: &str) -> Result<Vec<Option<Type>>, String> {
    let body_id = ctx
        .krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function) if ctx.krate.symbols.get(function.name) == Some(name) => {
                function.body
            }
            _ => None,
        })
        .ok_or_else(|| format!("no function `{name}` with a body"))?;
    let body = ctx
        .krate
        .bodies
        .get(body_id.0 as usize)
        .ok_or_else(|| format!("missing body {body_id:?}"))?;
    Ok(body
        .exprs
        .iter()
        .map(|expr| ctx.krate.types.get(expr.ty).cloned())
        .collect())
}

#[test]
fn nullish_elements_do_not_bind_an_open_list_hint() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(
            "export function uniq<T>(arr: readonly T[]): T[] { return [...new Set(arr)] }
             export function nullish() { return uniq([null, undefined, null, undefined]) }
             export function numbers() { return uniq([1, 2, 1]) }
             export function maybe(a: number | null) { return uniq([a, a]) }"
        ),
        &mut ctx,
    )?;
    let none = ctx.krate.types.intern(Type::None);
    let float = ctx.krate.types.intern(Type::Float);
    let optional_float = ctx.krate.types.intern(Type::Optional(float));

    // `null`/`undefined` keep the open hint: no value of the call is a list of
    // the indistinguishable unit.
    let nullish = function_expr_types(&ctx, "nullish")?;
    ensure!(
        !nullish.contains(&Some(Type::List(none))),
        "a literal of `null`/`undefined` must not instantiate `T` at `None`: {nullish:?}"
    );

    // The rule is not wider than the nullish unit: concrete elements, and
    // optional ones whose absence `Option` represents, still fill the slot.
    let numbers = function_expr_types(&ctx, "numbers")?;
    ensure!(
        numbers.contains(&Some(Type::List(float))),
        "`[1, 2, 1]` still binds `T = number`: {numbers:?}"
    );
    let maybe = function_expr_types(&ctx, "maybe")?;
    ensure!(
        maybe.contains(&Some(Type::List(optional_float))),
        "`[a, a]` with `a: number | null` still binds `T = number | null`: {maybe:?}"
    );
    Ok(())
}
