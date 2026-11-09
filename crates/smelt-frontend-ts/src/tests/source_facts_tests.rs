//! Declaration and read questions are answered by the AST, not by source text.

use super::*;

/// Whether any body of the crate holds an expression satisfying `predicate`.
fn any_expr(ctx: &HirCtx, predicate: impl Fn(&ExprKind) -> bool) -> bool {
    ctx.krate.bodies.iter().flat_map(|body| &body.exprs).any(|expr| predicate(&expr.kind))
}

/// `class URLParser` does not declare `URL`, so `new URL(..)` stays the host
/// constructor. A longer name used to satisfy the prefix probe.
#[test]
fn a_longer_class_name_does_not_declare_the_host_constructor() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
class URLParser {}
export const page = new URL("https://example.com/");
export const parser = new URLParser();
"#),
        &mut ctx,
    )?;
    ensure!(any_expr(&ctx, |kind| matches!(kind, ExprKind::UrlNew { .. })));
    Ok(())
}

/// A comment is no declaration either.
#[test]
fn a_comment_does_not_declare_a_class() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
// class URL is the host one here.
export const page = new URL("https://example.com/");
"#),
        &mut ctx,
    )?;
    ensure!(any_expr(&ctx, |kind| matches!(kind, ExprKind::UrlNew { .. })));
    Ok(())
}

/// `interface ArrayBufferViewLike` does not shadow the ambient `ArrayBufferView`.
#[test]
fn a_longer_type_name_does_not_shadow_an_ambient_type() -> Result<(), String> {
    let source = r"
interface ArrayBufferViewLike { size: number }
export function bytes(view: ArrayBufferView): number { return view.byteLength; }
";
    let mut with_longer = HirCtx::new();
    lower_ok(source, &mut with_longer)?;
    let shadowed = (0..).map_while(|index| with_longer.krate.types.get(smelt_hir::TypeId(index))).any(
        |ty| {
            matches!(ty, Type::Class { name, .. }
                if with_longer.krate.symbols.get(*name) == Some("ArrayBufferView"))
        },
    );
    ensure!(!shadowed, "`ArrayBufferView` must keep its ambient lowering");
    Ok(())
}

/// A later declaration still answers for an earlier read, which is why the facts
/// exist. A commented-out declaration is not one.
#[test]
fn a_comment_does_not_declare_a_module_callable() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
// const later = (): number => 1;
export const first = (): number => later();
"),
        &mut ctx,
    )?;
    ensure!(!errors.is_empty(), "`later` must stay unresolved");
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export const first = (): number => later();
function later(): number { return 1; }
"),
        &mut ctx,
    )?;
    Ok(())
}

/// A named function expression can pass itself along from its own body.
#[test]
fn a_named_function_expression_names_itself_as_a_callback() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function apply(run: (next: unknown) => boolean): boolean { return run(1); }
export function top(): boolean {
  return apply(
    function visit(value: unknown): boolean {
      return apply(visit);
    },
  );
}
"),
        &mut ctx,
    )?;
    ensure_signature(&ctx, "top", "top() -> Bool")?;
    ensure_signature(&ctx, "apply", "apply(run: fn(Unknown) -> Bool) -> Bool")?;
    ensure_exprs!(&ctx, Call, ClosureCall);
    Ok(())
}

/// A binding named like a recognized global is that binding, whichever rule
/// would have claimed the spelling.
#[test]
fn a_binding_shadows_every_recognized_global() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function encode(JSON: { stringify(value: number): string }): string {
  return JSON.stringify(1);
}
export function now(Date: { now(): number }): number {
  return Date.now();
}
export function load(fetch: (url: string) => string): string {
  return fetch("/x");
}
export function pick(Math: { random(): number }): number {
  return Math.random();
}
"#),
        &mut ctx,
    )?;
    ensure!(!any_expr(&ctx, |kind| matches!(
        kind,
        ExprKind::JsonStringify { .. }
            | ExprKind::DateNow
            | ExprKind::NumericRandom
            | ExprKind::AsyncOp { .. }
    )));
    Ok(())
}

/// Importing a host module's export is not a shadow of its global spelling.
#[test]
fn a_host_module_import_keeps_the_rule() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { Buffer } from "node:buffer";
export const bytes = Buffer.from("abc");
"#),
        &mut ctx,
    )?;
    ensure!(smelt_hir::validate(&ctx.krate).is_empty());
    ensure_exprs!(&ctx, HostConstruct);
    Ok(())
}

/// The handlers share one lowering of the receiver: however many of them gate
/// on its type before one claims the call, the parameter is read once.
#[test]
fn a_dispatched_receiver_is_lowered_once() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
export function read(headers: Headers): string | null {
  return headers.get("a");
}
"#),
        &mut ctx,
    )?;
    let reads = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| &body.exprs)
        .filter(|expr| matches!(expr.kind, ExprKind::Local(_)))
        .count();
    ensure_eq!(reads, 1);
    Ok(())
}
