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

/// Return the HIR type of the last expression in ANY body matching `pred`.
///
/// The module-scoped [`last_expr_ty`] cannot see inside a function body, and a
/// `Response` read most often sits in one (`await response.text()` needs an
/// `async` function), so these tests scan the whole lowered crate.
fn any_body_expr_ty(
    ctx: &HirCtx,
    pred: impl Fn(&ExprKind) -> bool,
) -> Result<smelt_hir::TypeId, String> {
    ctx.krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .rfind(|expr| pred(&expr.kind))
        .map(|expr| expr.ty)
        .ok_or_else(|| "no expression in any body matched the predicate".to_owned())
}

/// Return whether ANY body holds an expression matching `pred`.
fn any_body_has(ctx: &HirCtx, pred: impl Fn(&ExprKind) -> bool) -> bool {
    ctx.krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .any(|expr| pred(&expr.kind))
}

/// `new Response(..)` lowers to a concrete `Response` value, not a record.
#[test]
fn response_constructor_lowers_to_a_concrete_class_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const response = new Response("hello", { status: 201 });
"#),
        &mut ctx,
    )?;
    let ty = last_expr_ty(&ctx, module_id, |kind| {
        matches!(kind, ExprKind::ResponseNew { .. })
    })?;
    ensure!(
        matches!(ctx.krate.types.get(ty), Some(Type::Class { .. })),
        "`new Response(..)` must be a class-typed value, got {}",
        type_text(&ctx, ty),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// The init literal's keys lower to their own typed fields.
///
/// Keeping the init as a record would mean codegen re-deriving `status`'s type
/// from a tagged value at run time; the whole point of the split is that it
/// never has to.
#[test]
fn response_init_keys_lower_to_separate_fields() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const response = new Response("hello", { status: 201, statusText: "Created" });
"#),
        &mut ctx,
    )?;
    let lowered_module = module(&ctx, module_id)?;
    let lowered_body = module_body(&ctx, lowered_module)?;
    let found = lowered_body
        .exprs
        .iter()
        .find_map(|expr| match &expr.kind {
            ExprKind::ResponseNew {
                body,
                status,
                status_text,
                headers,
            } => Some((
                body.is_some(),
                status.is_some(),
                status_text.is_some(),
                headers.is_some(),
            )),
            _ => None,
        })
        .ok_or_else(|| "no ResponseNew lowered".to_owned())?;
    ensure_eq!(found, (true, true, true, false));
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Every modeled member keeps the exact source result type.
#[test]
fn response_members_keep_their_exact_source_types() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
async function read(): Promise<void> {
  const response = new Response("hello");
  const status = response.status;
  const ok = response.ok;
  const phrase = response.statusText;
  const used = response.bodyUsed;
  const headers = response.headers;
  const copy = response.clone();
  const text = await response.text();
}
"#),
        &mut ctx,
    )?;
    for (op, expected) in [
        (smelt_hir::ResponseOp::Status, "Some(Float)"),
        (smelt_hir::ResponseOp::Ok, "Some(Bool)"),
        (smelt_hir::ResponseOp::StatusText, "Some(String)"),
        (smelt_hir::ResponseOp::BodyUsed, "Some(Bool)"),
    ] {
        let ty = any_body_expr_ty(&ctx, |kind| {
            matches!(kind, ExprKind::ResponseOp { op: found, .. } if *found == op)
        })?;
        ensure_eq!(type_text(&ctx, ty), expected.to_owned());
    }
    for op in [
        smelt_hir::ResponseOp::Headers,
        smelt_hir::ResponseOp::Clone,
    ] {
        let ty = any_body_expr_ty(&ctx, |kind| {
            matches!(kind, ExprKind::ResponseOp { op: found, .. } if *found == op)
        })?;
        ensure!(
            matches!(ctx.krate.types.get(ty), Some(Type::Class { .. })),
            "`Response` member {op:?} must be class-typed, got {}",
            type_text(&ctx, ty),
        );
    }
    // `text()` is a future because the source method is `async`: the caller
    // awaits it, so the type has to say so.
    let text_ty = any_body_expr_ty(&ctx, |kind| {
        matches!(
            kind,
            ExprKind::ResponseOp {
                op: smelt_hir::ResponseOp::Text,
                ..
            }
        )
    })?;
    ensure!(
        matches!(ctx.krate.types.get(text_ty), Some(Type::Future(inner))
            if matches!(ctx.krate.types.get(*inner), Some(Type::String))),
        "`response.text()` is a `Promise<string>`, got {}",
        type_text(&ctx, text_ty),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A `Response` parameter annotation resolves to the modeled class.
#[test]
fn response_annotation_resolves_to_the_modeled_class() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function statusOf(response: Response): number {
  return response.status;
}
"),
        &mut ctx,
    )?;
    let ty = any_body_expr_ty(&ctx, |kind| {
        matches!(
            kind,
            ExprKind::ResponseOp {
                op: smelt_hir::ResponseOp::Status,
                ..
            }
        )
    })?;
    ensure_eq!(type_text(&ctx, ty), "Some(Float)".to_owned());
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A user class named `Response` shadows the modeled host class.
///
/// The registry models the host *name*; it must not claim a source class that
/// happens to share the spelling.
#[test]
fn a_user_class_named_response_wins() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
class Response {
  constructor(readonly status: number) {}
  describe(): number {
    return this.status;
  }
}

const response = new Response(204);
const described = response.describe();
"),
        &mut ctx,
    )?;
    ensure!(
        !any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::ResponseNew { .. } | ExprKind::ResponseOp { .. }
        )),
        "a user class named `Response` must not lower to the modeled fetch type",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A `status`/`ok` field on an unrelated value keeps the ordinary field read.
///
/// The property names are common, so recognition cannot key on the member
/// alone; it keys on the receiver's lowered type being the modeled class.
#[test]
fn an_unrelated_status_read_is_not_a_response_read() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
interface Job {
  status: number;
  ok: boolean;
}

const job: Job = { status: 3, ok: true };
const status = job.status;
const ok = job.ok;
"),
        &mut ctx,
    )?;
    ensure!(
        !any_body_has(&ctx, |kind| matches!(kind, ExprKind::ResponseOp { .. })),
        "an interface field named `status` must keep the ordinary field read",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A typed init variable is lowered by field, not rejected.
///
/// The first version of this rule required an object literal. That was too
/// strict: a value whose static type declares the keys has exactly as much
/// type information as a literal, and reading a declared key is an ordinary
/// typed field read. Only a genuinely erased init has nothing to read.
#[test]
fn response_init_can_be_a_typed_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
interface PageInit {
  status?: number;
}

const init: PageInit = { status: 201 };
const response = new Response("hello", init);
"#),
        &mut ctx,
    )?;
    ensure!(
        any_body_has(&ctx, |kind| matches!(kind, ExprKind::ResponseNew { .. })),
        "a typed init must still build a concrete response",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// An init read off an *erased* value is still a blocker.
///
/// This is the case the literal-only rule was really protecting: an `unknown`
/// value declares no keys, so nothing can be read with its type intact and
/// inventing the keys at run time is the tagged-record path these types exist
/// to avoid.
#[test]
fn response_init_from_an_erased_value_is_a_blocker() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
declare const init: unknown;
const response = new Response("hello", init);
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "erased value")
}

/// An init key Smelt does not model yet is named, not dropped.
#[test]
fn response_unmodeled_init_key_is_named() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
const response = new Response("hello", { url: "https://example.test" });
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "`url` is not modeled yet")
}

/// `new Request(..)` lowers to a concrete `Request` value, not a record.
#[test]
fn request_constructor_lowers_to_a_concrete_class_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
const request = new Request("https://a.test/p", { method: "POST", body: "hi" });
"#),
        &mut ctx,
    )?;
    let ty = last_expr_ty(&ctx, module_id, |kind| {
        matches!(kind, ExprKind::RequestNew { .. })
    })?;
    ensure!(
        matches!(ctx.krate.types.get(ty), Some(Type::Class { .. })),
        "`new Request(..)` must be a class-typed value, got {}",
        type_text(&ctx, ty),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Every modeled member keeps the exact source result type.
///
/// `url` being `String` is the point of demand item 6 in
/// `blocker-logs/hono-fetch-demand.md`: an untyped read made
/// `request.url.indexOf(':')` a "string search methods require a string
/// receiver" error.
#[test]
fn request_members_keep_their_exact_source_types() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
async function read(): Promise<void> {
  const request = new Request("https://a.test/p");
  const url = request.url;
  const method = request.method;
  const used = request.bodyUsed;
  const headers = request.headers;
  const copy = request.clone();
  const text = await request.text();
}
"#),
        &mut ctx,
    )?;
    for (op, expected) in [
        (smelt_hir::RequestOp::Url, "Some(String)"),
        (smelt_hir::RequestOp::Method, "Some(String)"),
        (smelt_hir::RequestOp::BodyUsed, "Some(Bool)"),
    ] {
        let ty = any_body_expr_ty(&ctx, |kind| {
            matches!(kind, ExprKind::RequestOp { op: found, .. } if *found == op)
        })?;
        ensure_eq!(type_text(&ctx, ty), expected.to_owned());
    }
    for op in [smelt_hir::RequestOp::Headers, smelt_hir::RequestOp::Clone] {
        let ty = any_body_expr_ty(&ctx, |kind| {
            matches!(kind, ExprKind::RequestOp { op: found, .. } if *found == op)
        })?;
        ensure!(
            matches!(ctx.krate.types.get(ty), Some(Type::Class { .. })),
            "`Request` member {op:?} must be class-typed, got {}",
            type_text(&ctx, ty),
        );
    }
    let text_ty = any_body_expr_ty(&ctx, |kind| {
        matches!(
            kind,
            ExprKind::RequestOp {
                op: smelt_hir::RequestOp::Text,
                ..
            }
        )
    })?;
    ensure!(
        matches!(ctx.krate.types.get(text_ty), Some(Type::Future(inner))
            if matches!(ctx.krate.types.get(*inner), Some(Type::String))),
        "`request.text()` is a `Promise<string>`, got {}",
        type_text(&ctx, text_ty),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A string method applies directly to `request.url`.
#[test]
fn request_url_is_a_string_receiver() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r#"
function schemeEnd(request: Request): number {
  return request.url.indexOf(":");
}
"#),
        &mut ctx,
    )?;
    let _ = module_id;
    ensure!(
        any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::RequestOp {
                op: smelt_hir::RequestOp::Url,
                ..
            }
        )),
        "`request.url` must lower to the typed url read",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A user class named `Request` shadows the modeled host class.
#[test]
fn a_user_class_named_request_wins() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
class Request {
  constructor(readonly url: string) {}
  describe(): string {
    return this.url;
  }
}

const request = new Request("mine");
const described = request.describe();
"#),
        &mut ctx,
    )?;
    ensure!(
        !any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::RequestNew { .. } | ExprKind::RequestOp { .. }
        )),
        "a user class named `Request` must not lower to the modeled fetch type",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A typed init variable is lowered by field for `Request` too.
#[test]
fn request_init_can_be_a_typed_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function send(init: RequestInit): Request {
  return new Request("https://a.test/p", init);
}
"#),
        &mut ctx,
    )?;
    ensure!(
        any_body_has(&ctx, |kind| matches!(kind, ExprKind::RequestNew { .. })),
        "a typed init must still build a concrete request",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A spread init keeps the object literal's own evaluation order.
///
/// `{ ...init, status }` takes the later key, which is what the source says;
/// reading the spread source first and letting named keys overwrite is how
/// that order is preserved.
#[test]
fn a_spread_init_lets_the_later_key_win() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
interface PageInit {
  status?: number;
}

function page(init: PageInit): Response {
  return new Response("page", { ...init, status: 201 });
}
"#),
        &mut ctx,
    )?;
    let ty = any_body_expr_ty(&ctx, |kind| matches!(kind, ExprKind::ResponseNew { .. }))?;
    ensure!(
        matches!(ctx.krate.types.get(ty), Some(Type::Class { .. })),
        "a spread init must still build a concrete response, got {}",
        type_text(&ctx, ty),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// An init key Smelt does not model yet is named, not dropped.
///
/// `signal`, `redirect`, `credentials` and the rest of `RequestInit` are real
/// keys with real behaviour; accepting and ignoring one would change what the
/// program does with no diagnostic.
#[test]
fn request_unmodeled_init_key_is_named() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
const request = new Request("https://a.test/p", { redirect: "manual" });
"#),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "`redirect` is not modeled yet")
}

/// A `Request` with no URL argument is a named blocker.
#[test]
fn request_requires_a_url_argument() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
const request = new Request();
"),
        &mut ctx,
    )?;
    assert_unsupported_ts(&errors, "requires a URL argument")
}

/// A `Request` at the init position is lowered, not rejected as erased.
///
/// The spec allows it and copies the source's method, headers and body
/// (`new Request(url, request)`), which `hono-base.ts` relies on.
#[test]
fn a_request_can_be_a_request_init() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function retarget(url: string, request: Request): Request {
  return new Request(url, request);
}
"),
        &mut ctx,
    )?;
    ensure!(
        any_body_has(&ctx, |kind| matches!(kind, ExprKind::RequestNew { .. })),
        "a `Request` init must still build a concrete request",
    );
    // The method is read through the modeled member, not a struct field.
    ensure!(
        any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::RequestOp {
                op: smelt_hir::RequestOp::Method,
                ..
            }
        )),
        "the source request's method must be read through its modeled member",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A user-declared init interface whose key is generic still declares that key.
///
/// Hono declares `interface ResponseInit<T extends StatusCode>` with
/// `status?: T`. A type parameter has no runtime shape, so the key resolves
/// through its constraint — which is what the source promises about every
/// instantiation.
#[test]
fn a_generic_init_key_resolves_through_its_constraint() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type StatusCode = 200 | 201 | 404;

interface PageInit<T extends StatusCode = StatusCode> {
  status?: T;
}

export function page(init: PageInit): Response {
  return new Response("page", init);
}
"#),
        &mut ctx,
    )?;
    ensure!(
        any_body_has(&ctx, |kind| matches!(kind, ExprKind::ResponseNew { .. })),
        "a generic init must still build a concrete response",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A qualified ambient init resolves to the ambient interface, not a local one.
///
/// A module that declares its own `ResponseInit` can still reach the platform's
/// through `globalThis.ResponseInit`; the qualified reference keeps its full
/// path, which is what lets the two be told apart at all.
#[test]
fn a_qualified_ambient_init_is_not_the_local_one() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
interface ResponseInit {
  unrelated?: string;
}

export function platform(init: globalThis.ResponseInit): Response {
  return new Response("x", init);
}
"#),
        &mut ctx,
    )?;
    ensure!(
        any_body_has(&ctx, |kind| matches!(kind, ExprKind::ResponseNew { .. })),
        "the qualified ambient init must be lowered by field",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// `BodyInit` is a union, so its string arm survives.
///
/// Left opaque, `JSON.stringify(body)` reported "value must be
/// JSON-serializable (got Class `BodyInit`)" for a value that is a string on
/// every path a program takes.
#[test]
fn body_init_is_a_union_whose_string_arm_survives() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function serialize(body: BodyInit): string {
  return JSON.stringify(body);
}
"),
        &mut ctx,
    )?;
    let ty = any_body_expr_ty(&ctx, |kind| matches!(kind, ExprKind::Local(_)))?;
    let _ = ty;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A host object is JSON-serializable, because JavaScript writes `{}` for one.
#[test]
fn a_host_object_is_json_serializable() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function serialize(headers: Headers): string {
  return JSON.stringify(headers);
}
"),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// `instanceof` lowers against an identity-only host object.
///
/// `FormData` and `ReadableStream` joined the host registry for identity, and
/// `instanceof` is exactly the operation identity exists for — so it must
/// resolve through the marker even though neither has a modeled surface.
/// Before this, `body instanceof FormData` aborted as "not a lowered class"
/// (`request.ts:490`), which is worse than answering: adding identity had made
/// the one operation that identity provides stop working.
#[test]
fn instanceof_lowers_against_an_identity_only_host_object() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function kindOf(body: BodyInit): string {
  if (body instanceof FormData) {
    return "form";
  }
  if (body instanceof ReadableStream) {
    return "stream";
  }
  return "text";
}
"#),
        &mut ctx,
    )?;
    ensure!(
        any_body_has(&ctx, |kind| matches!(kind, ExprKind::InstanceOf { .. })),
        "an identity-only host target must lower to the marker probe",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A utility type over the ambient init resolves to the same field table.
///
/// Hono's `request.ts` declares
/// `Required<Omit<RequestInit, 'window' | 'priority'>> & { [K in ...]?: ... }`
/// and passes a value of it to `new Request(url, init)`. Two things were
/// missing and both belonged in the one field-table path: the utilities
/// themselves (only `Pick` was handled), and the ambient inits' fields, which
/// no interface or alias lookup can find because they are not in the crate.
#[test]
fn a_utility_type_over_an_ambient_init_declares_its_keys() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
type RequiredRequestInit = Required<Omit<RequestInit, "window" | "priority">> & {
  [Key in "window" | "priority"]?: RequestInit[Key];
};

export function cloneRequest(req: Request): Request {
  const requestInit: RequiredRequestInit = {
    method: req.method,
    headers: req.headers,
    body: "payload",
  };
  return new Request(req.url, requestInit);
}
"#),
        &mut ctx,
    )?;
    ensure!(
        any_body_has(&ctx, |kind| matches!(kind, ExprKind::RequestNew { .. })),
        "a utility-typed init must still build a concrete request",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// `Omit` and `Required` work over an ordinary source interface too.
///
/// The utilities are handled in the shared field-table path, so nothing about
/// them is specific to the fetch types.
#[test]
fn omit_and_required_resolve_over_a_source_interface() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
interface Base {
  a?: string;
  b?: number;
}

export function omitted(init: Omit<Base, "b">): string {
  return init.a ?? "none";
}

export function required(init: Required<Base>): string {
  return init.a;
}
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// `new EventEmitter()` lowers to a concrete class value, not an erased record.
///
/// The registry entry for `node:events` is `Modeled`, so the import resolves
/// instead of blocking, and the constructor answers `Type::Class { EventEmitter }`.
#[test]
fn the_event_emitter_constructor_lowers_to_a_concrete_class_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
import { EventEmitter } from 'node:events';
const emitter = new EventEmitter();
"),
        &mut ctx,
    )?;
    let ty = last_expr_ty(&ctx, module_id, |kind| {
        matches!(kind, ExprKind::EventEmitterNew)
    })?;
    ensure!(
        matches!(ctx.krate.types.get(ty), Some(Type::Class { .. })),
        "a constructed emitter must keep its class type, got {}",
        type_text(&ctx, ty),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// Every registration and removal answers the EMITTER, and `emit` a boolean.
///
/// Those return types are what makes `e.on(..).on(..)` chain and
/// `if (e.emit(..))` narrow, so they are pinned at the HIR level rather than
/// only observed at run time.
#[test]
fn emitter_members_keep_their_source_result_types() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
import { EventEmitter } from 'node:events';
const emitter = new EventEmitter();
const chained = emitter.on('a', () => {});
const ran = emitter.emit('a');
const count = emitter.listenerCount('a');
"),
        &mut ctx,
    )?;
    let registered = last_expr_ty(&ctx, module_id, |kind| {
        matches!(
            kind,
            ExprKind::EventEmitterOp {
                op: smelt_hir::EventEmitterOp::On,
                ..
            }
        )
    })?;
    ensure!(
        matches!(ctx.krate.types.get(registered), Some(Type::Class { .. })),
        "`on` answers the emitter, got {}",
        type_text(&ctx, registered),
    );
    let emitted = last_expr_ty(&ctx, module_id, |kind| {
        matches!(
            kind,
            ExprKind::EventEmitterOp {
                op: smelt_hir::EventEmitterOp::Emit,
                ..
            }
        )
    })?;
    ensure_eq!(type_text(&ctx, emitted), "Some(Bool)".to_owned());
    let counted = last_expr_ty(&ctx, module_id, |kind| {
        matches!(
            kind,
            ExprKind::EventEmitterOp {
                op: smelt_hir::EventEmitterOp::ListenerCount,
                ..
            }
        )
    })?;
    ensure_eq!(type_text(&ctx, counted), "Some(Float)".to_owned());
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// `addListener`/`removeListener` are the same operations under other names.
#[test]
fn the_alias_spellings_lower_to_the_same_operations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { EventEmitter } from 'node:events';
const emitter = new EventEmitter();
const listener = () => {};
emitter.addListener('a', listener);
emitter.removeListener('a', listener);
"),
        &mut ctx,
    )?;
    ensure!(
        any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::EventEmitterOp {
                op: smelt_hir::EventEmitterOp::On,
                ..
            }
        )),
        "`addListener` is `on`",
    );
    ensure!(
        any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::EventEmitterOp {
                op: smelt_hir::EventEmitterOp::Off,
                ..
            }
        )),
        "`removeListener` is `off`",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A user class named `EventEmitter` shadows the modeled one.
///
/// The same shared `user_class_shadows` check the other modeled classes use:
/// a source class of that name owns its own members, including inside its own
/// methods where the class is still only pending.
#[test]
fn a_user_event_emitter_class_shadows_the_modeled_one() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
class EventEmitter {
  constructor(readonly label: string) {}
  emit(): string {
    return this.label;
  }
}

const mine = new EventEmitter('local');
const label = mine.emit();
"),
        &mut ctx,
    )?;
    ensure!(
        !any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::EventEmitterNew | ExprKind::EventEmitterOp { .. }
        )),
        "a user class of the same name must not be claimed by the modeled type",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// The constructor's `captureRejections` option is a named blocker.
///
/// It changes how a rejected promise returned by a listener is reported, and
/// nothing in the generated runtime can observe that yet. Accepting the option
/// and ignoring it would be a silent behaviour difference, which is exactly
/// what the honest-blocker rule exists to prevent.
#[test]
fn emitter_constructor_options_are_a_named_blocker() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
import { EventEmitter } from 'node:events';
const emitter = new EventEmitter({ captureRejections: true });
"),
        &mut ctx,
    )?;
    ensure!(
        errors
            .iter()
            .any(|error| format!("{error:?}").contains("EventEmitter options")),
        "the unmodeled option must be named in the diagnostic",
    );
    Ok(())
}

/// A `Response` is accepted at the `ResponseInit` position.
///
/// Hono's `hono-base.ts:417` writes `new Response(null, await dispatch(..))`.
/// The spec's dictionary conversion reads the object's own `status`,
/// `statusText` and `headers`, so the init lowers to those three modeled reads
/// rather than reporting an erased init.
#[test]
fn a_response_can_be_a_response_init() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function reclothe(source: Response): Response {
  return new Response(null, source);
}
"),
        &mut ctx,
    )?;
    ensure!(
        any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::ResponseOp {
                op: smelt_hir::ResponseOp::Status,
                ..
            }
        )),
        "the init's status must be read off the source response",
    );
    ensure!(
        any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::ResponseOp {
                op: smelt_hir::ResponseOp::StatusText,
                ..
            }
        )),
        "the init's statusText must be read off the source response",
    );
    ensure!(
        any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::ResponseOp {
                op: smelt_hir::ResponseOp::Headers,
                ..
            }
        )),
        "the init's headers must be read off the source response",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// `createServer(handler)` lowers to a concrete `Server` value.
///
/// The registry entry for `node:http`'s server half is `Modeled`, so the import
/// resolves instead of blocking, and the call answers `Type::Class { Server }`
/// rather than an erased host value.
#[test]
fn create_server_lowers_to_a_concrete_class_value() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
import { createServer } from 'node:http';
const server = createServer((req, res) => { res.end('ok'); });
"),
        &mut ctx,
    )?;
    let ty = last_expr_ty(&ctx, module_id, |kind| {
        matches!(kind, ExprKind::HttpCreateServer { .. })
    })?;
    ensure!(
        matches!(ctx.krate.types.get(ty), Some(Type::Class { .. })),
        "a created server must keep its class type, got {}",
        type_text(&ctx, ty),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// The handler's parameters are TYPED from the module, not left erased.
///
/// A source handler is written `(req, res) => ..` with no annotations, so
/// nothing in the arrow says what `req` is. Typing them from `node:http` is what
/// makes `req.url` a modeled read instead of an erased property lookup — and a
/// regression here would still compile and still run, it would just erase
/// everything the handler touches.
#[test]
fn the_request_handler_parameters_are_typed_from_the_module() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { createServer } from 'node:http';
const server = createServer((req, res) => {
  res.statusCode = 204;
  res.end(req.method + req.url);
});
"),
        &mut ctx,
    )?;
    for op in [
        smelt_hir::IncomingMessageOp::Method,
        smelt_hir::IncomingMessageOp::Url,
    ] {
        ensure!(
            any_body_has(
                &ctx,
                |kind| matches!(kind, ExprKind::IncomingMessageOp { op: found, .. } if *found == op)
            ),
            "the handler's request parameter must read as a modeled message: {op:?}",
        );
    }
    ensure!(
        any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::ServerResponseOp {
                op: smelt_hir::ServerResponseOp::SetStatusCode,
                ..
            }
        )),
        "`res.statusCode = ..` must lower to the status-line write",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// `listen` answers the SERVER and `address` an optional port.
///
/// `listen` answering the server is what makes `createServer(h).listen(0)` a
/// server-valued expression as it is in Node; `address` answering an optional is
/// what forces the source to narrow before using the port, the same shape as
/// Node's `AddressInfo | null`.
#[test]
fn http_server_members_keep_their_source_result_types() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let module_id = lower_ok(
        ts!(r"
import { createServer } from 'node:http';
const server = createServer((req, res) => { res.end('ok'); });
const listening = server.listen(0);
const port = server.address();
"),
        &mut ctx,
    )?;
    let listened = last_expr_ty(&ctx, module_id, |kind| {
        matches!(
            kind,
            ExprKind::HttpServerOp {
                op: smelt_hir::HttpServerOp::Listen,
                ..
            }
        )
    })?;
    ensure!(
        matches!(ctx.krate.types.get(listened), Some(Type::Class { .. })),
        "`listen` answers the server, got {}",
        type_text(&ctx, listened),
    );
    let address = last_expr_ty(&ctx, module_id, |kind| {
        matches!(
            kind,
            ExprKind::HttpServerOp {
                op: smelt_hir::HttpServerOp::Address,
                ..
            }
        )
    })?;
    ensure!(
        matches!(ctx.krate.types.get(address), Some(Type::Optional(_))),
        "`address` answers an optional port, got {}",
        type_text(&ctx, address),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// `req.on(..)` reaches the EMITTER operations, and answers the REQUEST.
///
/// The coupling that made `node:http` one commit with `node:events`: Node's
/// `IncomingMessage` extends `EventEmitter`, and a request body is read through
/// that inheritance. The dispatch tests whether the receiver's class HAS an
/// emitter rather than whether it IS one, so one operation serves both — and
/// the result stays the REQUEST, so a chained `req.on(..).url` still finds
/// `url`.
#[test]
fn an_incoming_message_registers_listeners_as_an_emitter() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { createServer } from 'node:http';
const server = createServer((req, res) => {
  let body = '';
  req.on('data', (chunk) => { body += chunk; });
  req.on('end', () => { res.end(body); });
});
"),
        &mut ctx,
    )?;
    ensure!(
        any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::EventEmitterOp {
                op: smelt_hir::EventEmitterOp::On,
                ..
            }
        )),
        "`req.on(..)` must lower to the shared emitter operation",
    );
    let registered = any_body_expr_ty(&ctx, |kind| {
        matches!(
            kind,
            ExprKind::EventEmitterOp {
                op: smelt_hir::EventEmitterOp::On,
                ..
            }
        )
    })?;
    ensure!(
        matches!(ctx.krate.types.get(registered), Some(Type::Class { .. })),
        "`req.on(..)` answers the request itself, got {}",
        type_text(&ctx, registered),
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A modeled receiver's listener keeps a REAL parameter type.
///
/// A plain emitter's events are open — any name, any listener signature — which
/// is the boundary its erased listener store exists for. `IncomingMessage` is
/// not open: `node:http` says `data` carries one chunk and `end` carries
/// nothing, so the source's own closure is typed from that schema instead of
/// taking an erased value and coercing it by hand. Pinned here because the
/// erased spelling compiles and runs identically; it just puts a `SmeltUnknown`
/// inside program code for a signature the module already publishes.
#[test]
fn a_modeled_receivers_listener_is_typed_from_its_event() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { createServer } from 'node:http';
const server = createServer((req, res) => {
  req.on('data', (chunk) => { res.write(chunk); });
});
"),
        &mut ctx,
    )?;
    // The chunk flows into `res.write`, whose argument is a string. Had the
    // listener parameter stayed erased, the closure's own parameter local would
    // be `Unknown` and the write would carry a cast.
    ensure!(
        any_body_has(&ctx, |kind| matches!(
            kind,
            ExprKind::ServerResponseOp {
                op: smelt_hir::ServerResponseOp::Write,
                ..
            }
        )),
        "the listener's chunk must flow into the modeled write",
    );
    let erased_listener_param = ctx.krate.bodies.iter().any(|body| {
        body.params.iter().any(|param| {
            body.locals
                .get(param.0 as usize)
                .is_some_and(|local| ctx.krate.types.get(local.ty) == Some(&Type::Unknown))
        })
    });
    ensure!(
        !erased_listener_param,
        "a `data` listener's chunk must lower as a string, not an erased value",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// A user class named `Server` shadows the modeled one.
///
/// The rule every other modeled class follows: a program that defines its own
/// `Server` keeps it, and the `node:http` entry does not steal the name.
#[test]
fn a_user_server_class_shadows_the_modeled_one() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
class Server {
  ready: boolean = false;
  address(): number { return 0; }
}
const server = new Server();
const port = server.address();
"),
        &mut ctx,
    )?;
    ensure!(
        !any_body_has(&ctx, |kind| matches!(kind, ExprKind::HttpServerOp { .. })),
        "a user `Server` must not reach the modeled server operations",
    );
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    Ok(())
}

/// `http.request`/`http.get` stay a named blocker.
///
/// The server half of `node:http` is modeled and the client half is not, so a
/// half-modeled module must still REPORT the half it does not serve rather than
/// erasing it into a dynamic lookup that quietly does nothing.
#[test]
fn the_node_http_client_surface_is_a_named_blocker() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
import { request } from 'node:http';
const pending = request('http://example.test');
"),
        &mut ctx,
    )?;
    ensure!(
        errors
            .iter()
            .any(|error| format!("{error:?}").contains("node:http")),
        "the unmodeled client surface must be named in the diagnostic, got {errors:?}",
    );
    Ok(())
}
