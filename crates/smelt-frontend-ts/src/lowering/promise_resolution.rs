//! What an unannotated `new Promise(executor)` resolves to.
//!
//! TypeScript types `new Promise(executor)` from its contextual type (a
//! `Promise<T>` slot, a declared return type) or from an explicit
//! `Promise<T>` type argument. Without either it answers `Promise<unknown>`,
//! but the program still says what the promise carries: the executor calls its
//! `resolve` parameter with the value. A hand-written Rust port would type the
//! future by that value (`SmeltFuture<Response>`), not by `()` and not by an
//! erased tag, so Smelt reads the same evidence:
//!
//! ```ts
//! buildMiddlewareTuple(() =>
//!   new Promise((resolve) => setTimeout(() => resolve(new Response('..')))),
//! )
//! ```
//!
//! resolves to `Response`. The rule, in order:
//!
//! 1. every call of `resolve` with an argument passes the same type `T` → `T`
//!    (`T | undefined` when some call also passes nothing);
//! 2. calls pass arguments of different types → `unknown`, TypeScript's own
//!    answer, since the value really is only known at run time;
//! 3. `resolve` is never called with an argument → `undefined` (unit), the one
//!    case where the promise genuinely carries no value.
//!
//! The evidence is read off the LOWERED executor, so the argument types are the
//! ones the rest of lowering already computed, including through nested
//! closures (`setTimeout(() => resolve(x))`) that capture `resolve`. A
//! `resolve` handed on as a value (`emitter.once('done', resolve)`) is not a
//! call here; its value comes from the slot it is passed to, and it keeps rule 3.

use smelt_hir::{Body, Expr, ExprId, ExprKind, LocalId, TypeId};

/// How deeply nested closures are followed while looking for `resolve` calls.
const MAX_CLOSURE_DEPTH: usize = 32;

/// The calls a Promise executor makes to its `resolve` parameter.
#[derive(Debug, Default)]
pub(super) struct ResolveCalls {
    /// The type of the first argument of every call that passes one.
    pub(super) argument_types: Vec<TypeId>,
    /// Whether some call passes no argument at all (`resolve()`).
    pub(super) has_empty_call: bool,
}

/// Collect the `resolve(..)` calls of a lowered inline Promise executor.
///
/// `executor` is the executor expression in `parent`; `bodies` holds every
/// lowered closure body of the crate. Returns `None` when the executor did not
/// lower to a closure with a `resolve` parameter (a compact callback, a named
/// function value), since then there is no body to read.
pub(super) fn executor_resolve_calls(
    bodies: &[Body],
    parent: &Body,
    executor: ExprId,
) -> Option<ResolveCalls> {
    let ExprKind::Closure(closure) = &expr_at(parent, executor)?.kind else {
        return None;
    };
    let resolve = closure.params.first()?.local;
    let body = bodies.get(usize::try_from(closure.body.0).ok()?)?;
    let mut calls = ResolveCalls::default();
    collect_calls(bodies, body, resolve, 0, &mut calls);
    Some(calls)
}

/// Record every call of `resolve` in `body`, following closures that capture it.
fn collect_calls(
    bodies: &[Body],
    body: &Body,
    resolve: LocalId,
    depth: usize,
    calls: &mut ResolveCalls,
) {
    for expr in &body.exprs {
        match &expr.kind {
            ExprKind::ClosureCall { callee, args } if reads_local(body, *callee, resolve) => {
                match args.first() {
                    Some(argument) => {
                        if let Some(argument) = expr_at(body, *argument) {
                            calls.argument_types.push(argument.ty);
                        }
                    }
                    None => calls.has_empty_call = true,
                }
            }
            ExprKind::Closure(closure) if depth < MAX_CLOSURE_DEPTH => {
                for capture in &closure.captures {
                    if capture.source_local == resolve
                        && let Some(inner_resolve) = capture.body_local
                        && let Some(inner) = usize::try_from(closure.body.0)
                            .ok()
                            .and_then(|index| bodies.get(index))
                    {
                        collect_calls(bodies, inner, inner_resolve, depth + 1, calls);
                    }
                }
            }
            _ => {}
        }
    }
}

/// The expression `id` names in `body`, if it is in range.
fn expr_at(body: &Body, id: ExprId) -> Option<&Expr> {
    body.exprs.get(usize::try_from(id.0).ok()?)
}

/// Whether `expr` in `body` is a plain read of `local`.
fn reads_local(body: &Body, expr: ExprId, local: LocalId) -> bool {
    expr_at(body, expr).is_some_and(|expr| matches!(expr.kind, ExprKind::Local(read) if read == local))
}
