//! Frontend coverage for the WHATWG fetch types.
//!
//! The point of modeling `Headers` as a concrete class rather than an erased
//! record is that the *types* survive: `get` is `Optional(String)` because the
//! source says `string | null`, `has` is `Bool`, the projections are lists, and
//! the value itself is `Type::Class { Headers }` — never `Type::Unknown`. These
//! tests pin exactly that, because a regression here would still compile and
//! still run; it would just erase the caller's narrowing.

use super::*;

/// Return the HIR type of the last expression whose kind matches `pred`.
fn last_expr_ty(
    ctx: &HirCtx,
    module_id: ModuleId,
    pred: impl Fn(&ExprKind) -> bool,
) -> Result<smelt_hir::TypeId, String> {
    let lowered_module = module(ctx, module_id)?;
    let body = module_body(ctx, lowered_module)?;
    body.exprs
        .iter()
        .rfind(|expr| pred(&expr.kind))
        .map(|expr| expr.ty)
        .ok_or_else(|| "no expression matched the predicate".to_owned())
}

/// Return the printed form of a lowered type.
fn type_text(ctx: &HirCtx, ty: smelt_hir::TypeId) -> String {
    format!("{:?}", ctx.krate.types.get(ty))
}

/// `new Headers(init)` lowers to a concrete `Headers` value, not an erased record.
#[test]
fn headers_constructor_lowers_to_a_concrete_class_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const headers = new Headers({ "Content-Type": "text/plain" });
"#),
        &mut ctx,
    )?;
    let ty = last_expr_ty(&ctx, module_id, |kind| {
        matches!(kind, ExprKind::HeadersNew { .. })
    })?;
    ensure!(
        matches!(ctx.krate.types.get(ty), Some(Type::Class { .. })),
        "`new Headers(..)` must be a class-typed value, got {}",
        type_text(&ctx, ty),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Every modeled method keeps the exact source result type.
#[test]
fn headers_methods_keep_their_exact_source_types() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const headers = new Headers();
const value = headers.get("accept");
const present = headers.has("accept");
const names = [...headers.keys()];
const pairs = [...headers.entries()];
const cookies = headers.getSetCookie();
"#),
        &mut ctx,
    )?;
    let op_ty = |op: smelt_hir::HeadersOp| {
        last_expr_ty(&ctx, module_id, move |kind| {
            matches!(kind, ExprKind::HeadersOp { op: found, .. } if *found == op)
        })
    };
    let get_ty = op_ty(smelt_hir::HeadersOp::Get)?;
    ensure!(
        matches!(ctx.krate.types.get(get_ty), Some(Type::Optional(inner))
            if matches!(ctx.krate.types.get(*inner), Some(Type::String))),
        "`Headers.get` is `string | null`, got {}",
        type_text(&ctx, get_ty),
    );
    let has_ty = op_ty(smelt_hir::HeadersOp::Has)?;
    ensure!(
        matches!(ctx.krate.types.get(has_ty), Some(Type::Bool)),
        "`Headers.has` is a boolean, got {}",
        type_text(&ctx, has_ty),
    );
    for op in [
        smelt_hir::HeadersOp::Keys,
        smelt_hir::HeadersOp::GetSetCookie,
    ] {
        let ty = op_ty(op)?;
        ensure!(
            matches!(ctx.krate.types.get(ty), Some(Type::List(item))
                if matches!(ctx.krate.types.get(*item), Some(Type::String))),
            "`Headers` projection {op:?} is a string list, got {}",
            type_text(&ctx, ty),
        );
    }
    let entries_ty = op_ty(smelt_hir::HeadersOp::Entries)?;
    ensure!(
        matches!(ctx.krate.types.get(entries_ty), Some(Type::List(item))
            if matches!(ctx.krate.types.get(*item), Some(Type::Tuple(items)) if items.len() == 2)),
        "`Headers.entries` is a list of name/value pairs, got {}",
        type_text(&ctx, entries_ty),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// The mutating methods lower as statements with no value.
#[test]
fn headers_mutations_lower_as_void_operations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const headers = new Headers();
headers.set("accept", "text/html");
headers.append("accept", "application/json");
headers.delete("accept");
"#),
        &mut ctx,
    )?;
    for op in [
        smelt_hir::HeadersOp::Set,
        smelt_hir::HeadersOp::Append,
        smelt_hir::HeadersOp::Delete,
    ] {
        let ty = last_expr_ty(&ctx, module_id, |kind| {
            matches!(kind, ExprKind::HeadersOp { op: found, .. } if *found == op)
        })?;
        ensure!(
            matches!(ctx.krate.types.get(ty), Some(Type::None)),
            "`Headers` mutation {op:?} is void, got {}",
            type_text(&ctx, ty),
        );
    }
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A `Headers` annotation on a parameter keeps the concrete class type.
#[test]
fn headers_annotation_resolves_to_the_modeled_class() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
export function contentType(headers: Headers): string | null {
  return headers.get("content-type");
}
"#),
        &mut ctx,
    )?;
    let _module = module(&ctx, module_id)?;
    let get_typed = ctx.krate.bodies.iter().any(|body| {
        body.exprs.iter().any(|expr| {
            matches!(expr.kind, ExprKind::HeadersOp { op: smelt_hir::HeadersOp::Get, .. })
        })
    });
    ensure!(
        get_typed,
        "a `Headers`-annotated parameter must dispatch `get` as a modeled header read",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A user class named `Headers` shadows the modeled host class.
///
/// The registry models the host *name*; it must not claim a source class that
/// happens to share the spelling, or a program with its own `Headers` would
/// silently get the fetch type's behaviour.
#[test]
fn a_user_class_named_headers_wins() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class Headers {
  constructor(readonly label: string) {}
  get(name: string): string {
    return `${this.label}:${name}`;
  }
}

const headers = new Headers("mine");
const value = headers.get("accept");
"#),
        &mut ctx,
    )?;
    let lowered_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, lowered_module)?;
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::HeadersNew { .. } | ExprKind::HeadersOp { .. })),
        "a user class named `Headers` must not lower to the modeled fetch type",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A `get` call on an unrelated receiver is not a header read.
#[test]
fn map_get_is_not_routed_to_headers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const scores = new Map<string, number>();
scores.set("a", 1);
const score = scores.get("a");
"#),
        &mut ctx,
    )?;
    let lowered_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, lowered_module)?;
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::HeadersOp { .. })),
        "a `Map` receiver must keep the collection lowering",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Too many constructor initializers is a named blocker, not a silent drop.
#[test]
fn headers_constructor_rejects_extra_initializers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
const headers = new Headers({ accept: "text/html" }, { extra: "no" });
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "at most one initializer")
}

/// `new URLSearchParams(init)` lowers to a concrete parameter list.
///
/// It used to fabricate an erased record carrying only a `size` field, so every
/// read answered `undefined`; the value existed but held no parameters.
#[test]
fn url_search_params_constructor_lowers_to_a_concrete_class_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const params = new URLSearchParams("a=1&b=2");
"#),
        &mut ctx,
    )?;
    let ty = last_expr_ty(&ctx, module_id, |kind| {
        matches!(kind, ExprKind::UrlSearchParamsNew { .. })
    })?;
    ensure!(
        matches!(ctx.krate.types.get(ty), Some(Type::Class { .. })),
        "`new URLSearchParams(..)` must be a class-typed value, got {}",
        type_text(&ctx, ty),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Every modeled parameter method keeps the exact source result type.
#[test]
fn url_search_params_methods_keep_their_exact_source_types() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const params = new URLSearchParams("a=1");
const first = params.get("a");
const all = params.getAll("a");
const present = params.has("a");
const text = params.toString();
const count = params.size;
"#),
        &mut ctx,
    )?;
    let op_ty = |op: smelt_hir::UrlSearchParamsOp| {
        last_expr_ty(&ctx, module_id, move |kind| {
            matches!(kind, ExprKind::UrlSearchParamsOp { op: found, .. } if *found == op)
        })
    };
    let get_ty = op_ty(smelt_hir::UrlSearchParamsOp::Get)?;
    ensure!(
        matches!(ctx.krate.types.get(get_ty), Some(Type::Optional(inner))
            if matches!(ctx.krate.types.get(*inner), Some(Type::String))),
        "`URLSearchParams.get` is `string | null`, got {}",
        type_text(&ctx, get_ty),
    );
    let all_ty = op_ty(smelt_hir::UrlSearchParamsOp::GetAll)?;
    ensure!(
        matches!(ctx.krate.types.get(all_ty), Some(Type::List(item))
            if matches!(ctx.krate.types.get(*item), Some(Type::String))),
        "`URLSearchParams.getAll` is a string list, got {}",
        type_text(&ctx, all_ty),
    );
    let has_ty = op_ty(smelt_hir::UrlSearchParamsOp::Has)?;
    ensure!(
        matches!(ctx.krate.types.get(has_ty), Some(Type::Bool)),
        "`URLSearchParams.has` is a boolean, got {}",
        type_text(&ctx, has_ty),
    );
    let text_ty = op_ty(smelt_hir::UrlSearchParamsOp::ToText)?;
    ensure!(
        matches!(ctx.krate.types.get(text_ty), Some(Type::String)),
        "`URLSearchParams.toString` is a string, got {}",
        type_text(&ctx, text_ty),
    );
    let size_ty = last_expr_ty(&ctx, module_id, |kind| {
        matches!(kind, ExprKind::Field { .. })
    })?;
    ensure!(
        matches!(ctx.krate.types.get(size_ty), Some(Type::Float)),
        "`URLSearchParams.size` is a number, got {}",
        type_text(&ctx, size_ty),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// `toString()` on a parameter list is the urlencoded serialization.
///
/// The generic `.toString()` handler accepts any class-typed receiver, so a
/// modeled fetch type has to get first refusal or the call collapses into a
/// `"[object Object]"` string cast.
#[test]
fn url_search_params_to_string_is_not_a_generic_cast() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const params = new URLSearchParams("a=1");
const text = params.toString();
"#),
        &mut ctx,
    )?;
    let lowered_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, lowered_module)?;
    ensure!(
        body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::UrlSearchParamsOp {
                op: smelt_hir::UrlSearchParamsOp::ToText,
                ..
            }
        )),
        "`URLSearchParams.toString` must lower as the modeled serialization",
    );
    ensure!(
        !body
            .exprs
            .iter()
            .any(|expr| matches!(expr.kind, ExprKind::PrimitiveCast { .. })),
        "`URLSearchParams.toString` must not lower as a generic string cast",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A user class named `URLSearchParams` shadows the modeled host class.
#[test]
fn a_user_class_named_url_search_params_wins() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
class URLSearchParams {
  constructor(readonly query: string) {}
  get(name: string): string {
    return `${this.query}:${name}`;
  }
}

const params = new URLSearchParams("a=1");
const value = params.get("a");
"#),
        &mut ctx,
    )?;
    let lowered_module = module(&ctx, module_id)?;
    let body = module_body(&ctx, lowered_module)?;
    ensure!(
        !body.exprs.iter().any(|expr| matches!(
            expr.kind,
            ExprKind::UrlSearchParamsNew { .. } | ExprKind::UrlSearchParamsOp { .. }
        )),
        "a user class named `URLSearchParams` must not lower to the modeled type",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}
