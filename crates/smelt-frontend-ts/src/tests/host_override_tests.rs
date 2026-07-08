//! Tests for host-global override-slot lowering (`globalThis.<Name> = ...`).

use super::*;
use smelt_hir::ExprKind;

/// Return whether any lowered body in the crate contains an expression matching
/// `predicate`. Host-override lowering emits its rvalues into whichever body
/// owns the write/read/guard, so tests scan every body.
fn any_expr(ctx: &HirCtx, predicate: impl Fn(&ExprKind) -> bool) -> bool {
    ctx.krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| predicate(&expr.kind)))
}

/// Seed the crate-level written-host-global set exactly as the pre-pass would.
fn seed(ctx: &mut HirCtx, names: &[&str]) {
    for name in names {
        ctx.written_host_globals.insert((*name).to_owned());
    }
}

/// A `globalThis.<Name> = class ... extends <Host>` write lowers to a
/// `HostGlobalWrite` whose value is a constructor closure, and a
/// `globalThis.<Name> = undefined` write lowers to a `HostGlobalWrite` too.
#[test]
fn write_lowers_class_expression_and_undefined() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    seed(&mut ctx, &["File", "Blob"]);
    lower_ok(
        ts!(
            "export function setup() {\n\
             globalThis.File = class File extends Blob {\n\
               name: string;\n\
               constructor(chunks: any[], filename: string) { super(chunks); this.name = filename; }\n\
             };\n\
             globalThis.Blob = undefined;\n\
             }\n"
        ),
        &mut ctx,
    )?;
    ensure!(
        any_expr(&ctx, |kind| matches!(kind, ExprKind::HostGlobalWrite { .. })),
        "expected a HostGlobalWrite for the globalThis writes"
    );
    // The class-expression write stores a constructor closure so `new File(...)`
    // dispatch can tell a `Ctor` override from a `Native` slot at runtime.
    ensure!(
        any_expr(&ctx, |kind| matches!(kind, ExprKind::Closure(_))),
        "expected the class-expression override to lower to a constructor closure"
    );
    Ok(())
}

/// A presence guard (`typeof Blob === 'undefined'`) becomes a dynamic
/// `HostGlobalPresent` probe ONLY when the name is written somewhere; an
/// unwritten name keeps the byte-identical constant fold (no probe emitted).
#[test]
fn presence_guard_dynamic_only_for_written_names() -> Result<(), String> {
    const SOURCE: &str =
        ts!("export function supported(): boolean { return typeof Blob !== 'undefined'; }\n");

    // Written: the guard lowers to a dynamic slot probe.
    let mut written = HirCtx::new();
    seed(&mut written, &["Blob"]);
    lower_ok(SOURCE, &mut written)?;
    ensure!(
        any_expr(&written, |kind| matches!(
            kind,
            ExprKind::HostGlobalPresent { .. }
        )),
        "a written host global must lower its presence guard to HostGlobalPresent"
    );

    // Unwritten: no probe, and the guard folds to a constant `true` (`Blob` is
    // always present in the default profile) — byte-identical to before.
    let mut unwritten = HirCtx::new();
    lower_ok(SOURCE, &mut unwritten)?;
    ensure!(
        !any_expr(&unwritten, |kind| matches!(
            kind,
            ExprKind::HostGlobalPresent { .. }
        )),
        "an unwritten host global must NOT emit a HostGlobalPresent probe"
    );
    ensure!(
        any_expr(&unwritten, |kind| matches!(
            kind,
            ExprKind::Literal(Literal::Bool(true))
        )),
        "an unwritten host global's presence guard must fold to a constant"
    );
    Ok(())
}

/// The save/restore round trip lowers the read to `HostGlobalRead` and the
/// restore write to `HostGlobalWrite`, so the native handle can round-trip the
/// slot back to `Native`.
#[test]
fn save_restore_round_trip_lowers_read_and_write() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    seed(&mut ctx, &["File"]);
    lower_ok(
        ts!(
            "export function roundTrip() {\n\
             const original = globalThis.File;\n\
             globalThis.File = original;\n\
             }\n"
        ),
        &mut ctx,
    )?;
    ensure!(
        any_expr(&ctx, |kind| matches!(kind, ExprKind::HostGlobalRead { .. })),
        "the saved read must lower to HostGlobalRead"
    );
    ensure!(
        any_expr(&ctx, |kind| matches!(kind, ExprKind::HostGlobalWrite { .. })),
        "the restore must lower to HostGlobalWrite"
    );
    Ok(())
}

/// An unwritten host name keeps native `new X(...)` construction: no override
/// slot dispatch (no `HostGlobalRead`) is emitted around the construction.
#[test]
fn unwritten_new_keeps_native_construction() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!("export function make(): unknown { return new Blob(['x']); }\n"),
        &mut ctx,
    )?;
    ensure!(
        any_expr(&ctx, |kind| matches!(kind, ExprKind::BlobFromParts { .. })),
        "an unwritten `new Blob(...)` must lower to native BlobFromParts"
    );
    ensure!(
        !any_expr(&ctx, |kind| matches!(kind, ExprKind::HostGlobalRead { .. })),
        "an unwritten `new Blob(...)` must not emit override-slot dispatch"
    );
    Ok(())
}
