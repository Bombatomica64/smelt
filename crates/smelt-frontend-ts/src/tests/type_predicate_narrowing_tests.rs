//! Regression tests for user-defined type predicate narrowing.
//!
//! A `value is T` return type proves `T` for the named argument where the call
//! answered `true`, and proves "the union minus `T`" where it answered `false`
//! — whichever callable carries the signature. Before this, only a function
//! DECLARATION's positive branch narrowed: an else branch, a negated guard's
//! then-branch, an arrow const, a class method and a block-local arrow all left
//! the union un-narrowed, so a member only one arm declares (`request.url`)
//! was read through the erased runtime field lookup.
//!
//! Each test asserts on the narrowing casts the guarded function's body
//! carries: a narrowed union local is read through `ExprKind::UnknownCast` to
//! the member type, which the emitter lowers to a `match` unwrapping that arm.
//! The end-to-end fixture `140_user_type_guard_narrowing` covers the runtime
//! behaviour.

use super::*;

/// Shared declarations every fixture below narrows over.
const PRELUDE: &str = r#"
interface Req {
  kind: "req";
  url: string;
}

interface RawReq {
  kind: "raw";
  raw: number;
}
"#;

/// Lower `PRELUDE` followed by `source` into a fresh context.
fn lower_with_prelude(source: &str) -> Result<HirCtx, String> {
    let mut ctx = HirCtx::new();
    lower_ok(&format!("{PRELUDE}{source}"), &mut ctx)?;
    Ok(ctx)
}

/// Find a lowered function (free function or method) by its lowered
/// (snake-case) name.
fn any_function<'a>(ctx: &'a HirCtx, name: &str) -> Result<&'a Function, String> {
    ctx.krate
        .items
        .iter()
        .find_map(|item| match item {
            Item::Function(function) if ctx.krate.symbols.get(function.name) == Some(name) => {
                Some(function)
            }
            _ => None,
        })
        .ok_or_else(|| format!("missing function named `{name}`"))
}

/// Render the target of every narrowing cast in `name`'s body, in order.
///
/// A class/interface target renders as its name; any other type renders by its
/// `Debug` shape, which is enough to tell `Float` from `String`. Consecutive
/// repeats collapse, so a branch that reads its narrowed local twice still
/// reports one entry per branch.
fn narrowing_cast_targets(ctx: &HirCtx, name: &str) -> Result<Vec<String>, String> {
    let body = function_body(ctx, any_function(ctx, name)?)?;
    Ok(body
        .exprs
        .iter()
        .filter_map(|expr| match expr.kind {
            ExprKind::UnknownCast { target, .. } => Some(target),
            _ => None,
        })
        .map(|target| match ctx.krate.types.get(target) {
            Some(Type::Class { name, .. }) => ctx
                .krate
                .symbols
                .get(*name)
                .unwrap_or("<unnamed>")
                .to_owned(),
            Some(other) => format!("{other:?}"),
            None => "<missing type>".to_owned(),
        })
        .fold(Vec::new(), |mut targets: Vec<String>, target| {
            if targets.last() != Some(&target) {
                targets.push(target);
            }
            targets
        }))
}

/// The else branch of a predicate sees the union's REMAINING member.
#[test]
fn predicate_else_branch_narrows_to_remaining_member() -> Result<(), String> {
    let ctx = lower_with_prelude(ts!(r#"
function isRaw(r: Req | RawReq): r is RawReq {
  return r.kind === "raw";
}
export function describe(request: Req | RawReq): string {
  if (isRaw(request)) {
    return "raw " + request.raw;
  } else {
    return "req " + request.url;
  }
}
"#))?;
    ensure_eq!(narrowing_cast_targets(&ctx, "describe")?, ["RawReq", "Req"]);
    Ok(())
}

/// `!guard` narrows the then-branch to the remaining member and the
/// fall-through after an early return to the proven one.
#[test]
fn negated_predicate_narrows_then_branch_and_fall_through() -> Result<(), String> {
    let ctx = lower_with_prelude(ts!(r#"
function isRaw(r: Req | RawReq): r is RawReq {
  return r.kind === "raw";
}
export function early(request: Req | RawReq): string {
  if (!isRaw(request)) {
    return "req " + request.url;
  }
  return "raw " + request.raw;
}
"#))?;
    ensure_eq!(narrowing_cast_targets(&ctx, "early")?, ["Req", "RawReq"]);
    Ok(())
}

/// An arrow bound to a const carries its predicate like a declaration does,
/// including when a hoisted function body calling it is lowered first.
#[test]
fn arrow_const_predicate_narrows_both_branches() -> Result<(), String> {
    let ctx = lower_with_prelude(ts!(r#"
export function describeArrow(request: Req | RawReq): string {
  if (isReq(request)) {
    return "req " + request.url;
  }
  return "raw " + request.raw;
}
const isReq = (r: Req | RawReq): r is Req => r.kind === "req";
"#))?;
    ensure_eq!(narrowing_cast_targets(&ctx, "describe_arrow")?, ["Req", "RawReq"]);
    Ok(())
}

/// A block-local arrow predicate narrows its callers in the same block.
#[test]
fn block_local_arrow_predicate_narrows() -> Result<(), String> {
    let ctx = lower_with_prelude(ts!(r#"
export function describeLocal(request: Req | RawReq): string {
  const localIsRaw = (candidate: Req | RawReq): candidate is RawReq =>
    candidate.kind === "raw";
  if (localIsRaw(request)) {
    return "raw " + request.raw;
  }
  return "req " + request.url;
}
"#))?;
    ensure_eq!(narrowing_cast_targets(&ctx, "describe_local")?, ["RawReq", "Req"]);
    Ok(())
}

/// Class-method predicates narrow through an instance receiver, through
/// `this`, and through the class name for a static method.
#[test]
fn method_predicates_narrow_through_instance_this_and_static() -> Result<(), String> {
    let ctx = lower_with_prelude(ts!(r#"
export class Checker {
  isRaw(r: Req | RawReq): r is RawReq {
    return r.kind === "raw";
  }
  static isReq(r: Req | RawReq): r is Req {
    return r.kind === "req";
  }
  label(request: Req | RawReq): string {
    if (this.isRaw(request)) {
      return "raw " + request.raw;
    }
    return "req " + request.url;
  }
}
export function viaInstance(checker: Checker, request: Req | RawReq): string {
  if (checker.isRaw(request)) {
    return "raw " + request.raw;
  }
  return "req " + request.url;
}
export function viaStatic(request: Req | RawReq): string {
  if (Checker.isReq(request)) {
    return "req " + request.url;
  }
  return "raw " + request.raw;
}
"#))?;
    ensure_eq!(narrowing_cast_targets(&ctx, "label")?, ["RawReq", "Req"]);
    ensure_eq!(narrowing_cast_targets(&ctx, "via_instance")?, ["RawReq", "Req"]);
    ensure_eq!(narrowing_cast_targets(&ctx, "via_static")?, ["Req", "RawReq"]);
    Ok(())
}

/// A predicate proving a UNION removes every proven member from the false
/// branch, and `&&` carries the positive fact into the rest of the condition.
#[test]
fn union_target_predicate_leaves_the_complement() -> Result<(), String> {
    let ctx = lower_with_prelude(ts!(r#"
function isText(value: string | number | boolean): value is string | boolean {
  return typeof value !== "number";
}
function isString(value: string | number): value is string {
  return typeof value === "string";
}
export function complement(value: string | number | boolean): number {
  if (isText(value)) {
    return 0;
  }
  return value + 1;
}
export function conjunction(value: string | number, big: boolean): number {
  if (isString(value) && big && value.length > 2) {
    return 1;
  }
  return 0;
}
"#))?;
    ensure_eq!(narrowing_cast_targets(&ctx, "complement")?, ["Float"]);
    ensure_eq!(narrowing_cast_targets(&ctx, "conjunction")?, ["String"]);
    Ok(())
}

/// An assertion arrow (`asserts r is T`, spelled through the binding's
/// function type as TypeScript requires) narrows after the call statement.
#[test]
fn annotated_assertion_arrow_narrows_after_the_call() -> Result<(), String> {
    let ctx = lower_with_prelude(ts!(r#"
const assertRaw: (r: Req | RawReq) => asserts r is RawReq = (r) => {
  if (r.kind !== "raw") {
    throw new Error("not raw");
  }
};
export function asserted(request: Req | RawReq): number {
  assertRaw(request);
  return request.raw;
}
"#))?;
    ensure_eq!(narrowing_cast_targets(&ctx, "asserted")?, ["RawReq"]);
    Ok(())
}

/// A predicate over a non-union local proves nothing for its false branch:
/// there is no member to remove, so the local keeps its declared type.
#[test]
fn predicate_on_non_union_local_leaves_false_branch_alone() -> Result<(), String> {
    let ctx = lower_with_prelude(ts!(r#"
function isRaw(r: Req): r is Req {
  return r.kind === "req";
}
export function plain(request: Req): string {
  if (isRaw(request)) {
    return "a";
  }
  return request.url;
}
"#))?;
    ensure!(
        !narrowing_cast_targets(&ctx, "plain")?
            .iter()
            .any(|target| target != "Req"),
        "a non-union local must not gain a false-branch narrowing"
    );
    Ok(())
}
