//! An inline object type whose fields have different types lowers to a struct.
//!
//! Regression (radash `retry`): `options: { times?: number; delay?: number |
//! null; backoff?: (count: number) => number }` lowered to ONE
//! `Dict<String, Optional<Float | fn(Float) -> Float>>`, so `options.times`
//! read as a number-or-function and the generated crate failed to type-check
//! where that value met `range(1, times)`. See `decls::anonymous_record`.

use super::*;

/// The lowered type of parameter `index` of the module function named `name`.
fn param_type(ctx: &HirCtx, name: &str, index: usize) -> Result<smelt_hir::TypeId, String> {
    ctx.krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function)
                if ctx.krate.symbols.get(function.name) == Some(name) =>
            {
                function.params.get(index).map(|param| param.ty)
            }
            _ => None,
        })
        .ok_or_else(|| format!("no function `{name}` with parameter {index}"))
}

/// The fields of the interface item a `Type::Class` names, by field name.
fn interface_fields(
    ctx: &HirCtx,
    ty: smelt_hir::TypeId,
) -> Result<Vec<(String, smelt_hir::TypeId, bool)>, String> {
    let Some(Type::Class { name, args }) = ctx.krate.types.get(ty) else {
        return Err(format!("expected a struct type, got {:?}", ctx.krate.types.get(ty)));
    };
    if !args.is_empty() {
        return Err(format!("a synthesized record takes no type arguments: {args:?}"));
    }
    let interface = ctx
        .krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Interface(interface) if interface.name == *name => Some(interface),
            _ => None,
        })
        .ok_or_else(|| "the record type names no interface item".to_owned())?;
    Ok(interface
        .fields
        .iter()
        .map(|field| {
            (
                ctx.krate.symbols.get(field.name).unwrap_or("").to_owned(),
                field.ty,
                field.optional,
            )
        })
        .collect())
}

#[test]
fn a_heterogeneous_inline_object_type_lowers_to_a_struct_with_typed_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(
            "export function retry(options: { times?: number; delay?: number | null; backoff?: (count: number) => number }): number {
               const times = options?.times ?? 3
               return times
             }
             export function again(other: { backoff?: (count: number) => number; delay?: number | null; times?: number }): number {
               return other.times ?? 1
             }"
        ),
        &mut ctx,
    )?;
    let options_ty = param_type(&ctx, "retry", 0)?;
    let fields = interface_fields(&ctx, options_ty)?;
    let float = ctx.krate.types.intern(Type::Float);
    let optional_float = ctx.krate.types.intern(Type::Optional(float));
    let field = |name: &str| {
        fields
            .iter()
            .find(|(field_name, _, _)| field_name == name)
            .map(|(_, ty, optional)| (ctx.krate.types.get(*ty).cloned(), *optional))
    };
    ensure_eq!(field("times"), Some((Some(Type::Optional(float)), true)));
    // `number | null` and the optional marker both collapse onto one `Option`.
    ensure_eq!(field("delay"), Some((Some(Type::Optional(float)), true)));
    ensure!(
        matches!(
            field("backoff"),
            Some((Some(Type::Optional(inner)), true))
                if matches!(ctx.krate.types.get(inner), Some(Type::Function(_)))
        ),
        "backoff should be an optional function: {:?}",
        field("backoff")
    );
    // A second spelling (fields in another order) is also a struct whose
    // `times` keeps its own type.
    let other_ty = param_type(&ctx, "again", 0)?;
    let other_fields = interface_fields(&ctx, other_ty)?;
    ensure!(
        other_fields
            .iter()
            .any(|(name, ty, _)| name == "times" && *ty == optional_float),
        "second spelling keeps `times: Option<f64>`: {other_fields:?}"
    );
    Ok(())
}

#[test]
fn a_homogeneous_inline_object_type_stays_a_string_dict() -> Result<(), String> {
    // One value type describes every field, so the `Dict` layout is faithful
    // and the rule leaves it alone.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(
            "export function pick(options: { times?: number; delay?: number | null }): number {
               return options.times ?? 3
             }"
        ),
        &mut ctx,
    )?;
    let options_ty = param_type(&ctx, "pick", 0)?;
    ensure!(
        matches!(ctx.krate.types.get(options_ty), Some(Type::Dict(_, _))),
        "expected a dict, got {:?}",
        ctx.krate.types.get(options_ty)
    );
    Ok(())
}

#[test]
fn a_heterogeneous_inline_object_type_naming_a_type_parameter_stays_a_dict() -> Result<(), String> {
    // A synthesized struct has no binder for `T`, so the literal keeps its
    // previous lowering rather than leaking an unbound parameter.
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(
            "export function first<T>(options: { value: T; count: number }): number {
               return options.count as number
             }"
        ),
        &mut ctx,
    )?;
    let options_ty = param_type(&ctx, "first", 0)?;
    ensure!(
        matches!(ctx.krate.types.get(options_ty), Some(Type::Dict(_, _))),
        "expected a dict, got {:?}",
        ctx.krate.types.get(options_ty)
    );
    Ok(())
}
