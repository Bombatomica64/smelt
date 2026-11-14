//! Array-constrained type parameters are carried as their element
//! (`lowering/ty/element_params.rs`).
//!
//! Each test lowers a small valid program and pins the lowered signature: a
//! carried parameter `T` makes the name `T` a `List<T>` and `T[number]` the
//! parameter itself, with no `type-level` erasure. A parameter used as a
//! positional tuple (a rest-parameter list, a `[...T]` spread) is NOT carried.

use super::*;
use smelt_hir::ErasureReason;

/// Whether `symbol` spells `text`, in either the interned or the original name table.
fn names(ctx: &HirCtx, symbol: smelt_hir::Symbol, text: &str) -> bool {
    ctx.krate.symbols.get(symbol) == Some(text) || ctx.krate.names.get(symbol) == Some(text)
}

/// The function item named `name`, rendered as `(param, ..) -> return`.
fn signature(ctx: &HirCtx, name: &str) -> Result<String, String> {
    ctx.krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function) if names(ctx, function.name, name) => {
                let params = function
                    .params
                    .iter()
                    .map(|param| smelt_hir::type_display(&ctx.krate, param.ty))
                    .collect::<Vec<_>>()
                    .join(", ");
                let return_ty = smelt_hir::type_display(&ctx.krate, function.return_ty);
                Some(format!("({params}) -> {return_ty}"))
            }
            _ => None,
        })
        .ok_or_else(|| format!("no function `{name}`"))
}

/// The rendered type of the local named `name` in any body.
fn local_type(ctx: &HirCtx, name: &str) -> Result<String, String> {
    ctx.krate
        .bodies
        .iter()
        .flat_map(|body| body.locals.iter())
        .find(|local| local.name.is_some_and(|symbol| names(ctx, symbol, name)))
        .map(|local| smelt_hir::type_display(&ctx.krate, local.ty))
        .ok_or_else(|| format!("no local `{name}`"))
}

/// Whether lowering recorded a `type-level` erasure.
fn type_level_recorded(ctx: &HirCtx) -> bool {
    ctx.krate
        .erasures
        .iter()
        .any(|record| record.reason == ErasureReason::TypeLevel)
}

/// `T[number]` over an array-constrained `T` is the element, and `T` a list of it.
#[test]
fn array_constrained_parameter_is_its_element() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(
            "type Container = ReadonlyArray<unknown> | readonly [];\nfunction dropWhile<T extends Container>(data: T, predicate: (item: T[number], index: number) => boolean): T[number][] {\n  for (const [index, item] of data.entries()) {\n    if (!predicate(item, index)) {\n      return data.slice(index);\n    }\n  }\n  return [];\n}\nconst kept = dropWhile([1, 2, 3], (x) => x < 2);\nconsole.log(kept.length);\n"
        ),
        &mut ctx,
    )?;
    let lowered = signature(&ctx, "dropWhile")?;
    ensure!(
        lowered.starts_with("(List<T>, ") && lowered.ends_with("-> List<T>"),
        "expected `T` carried as `List<T>` and `T[number][]` as `List<T>`, got {lowered}"
    );
    ensure!(
        !type_level_recorded(&ctx),
        "`T[number]` must not erase, recorded {:?}",
        ctx.krate.erasures
    );
    let kept = local_type(&ctx, "kept")?;
    ensure_eq!(kept, "List<Float>");
    Ok(())
}

/// An explicit type argument names the whole list; the carried parameter
/// binds its element, and a generic alias instantiated by a tuple binds the
/// union of the tuple's items (TypeScript's `[A, B][number]`).
#[test]
fn explicit_arguments_bind_the_element() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(
            "type Item<T extends readonly unknown[]> = T[number];\nfunction first<T extends readonly unknown[]>(items: T): Item<T> {\n  return items[0];\n}\nconst one = first<number[]>([4, 5]);\nconst tagged: Item<[string, string]> = \"a\";\nconsole.log(one, tagged);\n"
        ),
        &mut ctx,
    )?;
    ensure_eq!(signature(&ctx, "first")?, "(List<T>) -> T");
    ensure_eq!(local_type(&ctx, "one")?, "Float");
    ensure_eq!(local_type(&ctx, "tagged")?, "String");
    Ok(())
}

/// A parameter typing a rest-parameter list is an argument TUPLE, whose
/// positions differ in type: it is not carried, so `A` keeps its own meaning.
#[test]
fn rest_parameter_list_is_not_carried() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(
            "function apply<A extends unknown[], R>(fn: (...args: A) => R, ...args: A): R {\n  return fn(...args);\n}\nconsole.log(apply((a: number, b: string) => b.repeat(a), 2, \"x\"));\n"
        ),
        &mut ctx,
    )?;
    let lowered = signature(&ctx, "apply")?;
    ensure!(
        !lowered.contains("List<A>"),
        "a rest-list parameter must not be carried as its element, got {lowered}"
    );
    Ok(())
}

/// A parameter spread into a tuple (`[...T]`, a heterogeneous row) is a
/// positional tuple too and is not carried.
#[test]
fn tuple_spread_parameter_is_not_carried() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(
            "function rows<T extends unknown[]>(zipped: ReadonlyArray<[...T]>): number {\n  return zipped.length;\n}\nconsole.log(rows([[1, \"a\"], [2, \"b\"]]));\n"
        ),
        &mut ctx,
    )?;
    let lowered = signature(&ctx, "rows")?;
    ensure!(
        !lowered.contains("List<List<T>>"),
        "a `[...T]` parameter must not be carried as its element, got {lowered}"
    );
    Ok(())
}
