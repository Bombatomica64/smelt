//! Regression coverage for the host-module import boundary.
//!
//! Every test here pins one half of the decision documented on
//! `smelt_stdlib::host_modules`: an import whose module resolves to neither a
//! source file nor an implemented host-module export must not degrade into an
//! erased no-op, while the shapes that legitimately erase (relative
//! specifiers, the test tier, modeled exports) must keep lowering.
//!
//! The evidence these tests replace is in `blocker-logs/express-v1-baseline.md`:
//! an app whose whole framework surface was erased at the import boundary
//! probed as "0 blockers" and emitted a crate that did nothing.

use super::*;

/// Using a declared-but-unimplemented host-module value is a named blocker.
#[test]
fn declared_host_module_value_blocks_at_first_use() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
import { DatabaseSync } from 'node:sqlite';

export function open(path: string): DatabaseSync {
  return new DatabaseSync(path);
}
"),
        &mut ctx,
    )?;
    assert_category(
        &errors,
        "node:sqlite",
        smelt_stdlib::DiagnosticCategory::MissingStdlib,
    )
}

/// Importing a declared host-module name without using it stays free.
///
/// The blocker belongs at the use, not at the import: a module that only
/// mentions the name in a type position emits nothing that needs the surface.
#[test]
fn declared_host_module_import_without_use_is_free() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import type { DatabaseSync } from 'node:sqlite';

export function describeHandle(handle: DatabaseSync): string {
  return typeof handle;
}
"),
        &mut ctx,
    )?;
    Ok(())
}

/// A modeled host-module export keeps lowering through its own rule.
///
/// `tz` from `@date-fns/tz` is modeled, so the registry must not turn it into a
/// blocker; the timezone-factory marker is driven by the registry rather than
/// by a package-name test inside import lowering.
#[test]
fn modeled_host_module_export_still_lowers() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { tz } from "@date-fns/tz";

export const zone = tz("America/Santiago");
"#),
        &mut ctx,
    )?;
    Ok(())
}

/// A relative specifier never blocks, even when it resolves to nothing here.
///
/// A relative import names a source file that the manifest resolver owns. A
/// module lowered on its own legitimately sees it unresolved, and that is not a
/// host-module gap.
#[test]
fn relative_specifier_never_blocks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { falsey } from "./falsey";

export function values(): unknown[] {
  return falsey.concat(true, 1, "a");
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

/// The test tier keeps erased interop with libraries Smelt does not model.
///
/// Assertion and fixture libraries only ever flow into matchers that are
/// already erased, and `CLAUDE.md` sanctions the test-function exception.
#[test]
fn test_tier_keeps_unmodeled_package_interop() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { expect, it } from "vitest";
import { UTCDate } from "@date-fns/utc";

it("builds an extension date", () => {
  const result = new UTCDate();
  expect(result).toBeInstanceOf(UTCDate);
});
"#),
        &mut ctx,
    )?;
    Ok(())
}

/// A function whose body cannot lower is reported, never silently dropped.
///
/// `blocker-logs/express-v1-baseline.md` recorded free functions that were
/// called but never emitted. Item lowering pushes a per-item diagnostic, so a
/// module carrying one failing function fails as a whole instead of emitting a
/// crate with a missing definition and a live call site.
#[test]
fn function_with_unlowerable_body_is_reported() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
async function* asyncValues(): AsyncGenerator<number, void, unknown> {
  yield 1;
}

function* syncValues(): Generator<number, void, unknown> {
  yield* asyncValues();
}

export function healthy(value: number): number {
  return value + 1;
}
"),
        &mut ctx,
    )?;
    ensure!(
        errors.iter().any(|error| error
            .message
            .contains("synchronous generator cannot delegate to an AsyncGenerator")),
        "a function whose body fails to lower must be reported: {errors:?}",
    );
    Ok(())
}

/// Using a value imported from an unmodeled bare package is a named blocker.
///
/// This is the second half of the policy documented on
/// `smelt_stdlib::host_modules::unmodeled_package_use_blocks`, enabled after
/// one pass with it off. The framework that drives a program is exactly the
/// surface whose erasure produced the express false green, so a program module
/// that calls into an unmodeled package now fails by name rather than emitting
/// a crate with the framework silently missing.
#[test]
fn unmodeled_package_value_blocks_at_first_use() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
import express from "express";

export const app = express();
"#),
        &mut ctx,
    )?;
    ensure!(
        errors
            .iter()
            .any(|error| error.message.contains("unresolved package `express`")),
        "an unmodeled package used as a value must block by name: {errors:?}",
    );
    assert_category(
        &errors,
        "express",
        smelt_stdlib::DiagnosticCategory::MissingStdlib,
    )
}

/// A named import from an unmodeled package blocks the same way a default does.
///
/// The policy is per *module*, not per import spelling: `import _ from
/// "lodash"` and `import { map } from "lodash"` are the same erasure, and a
/// rule that fired for only one of them would be the special case `CLAUDE.md`
/// forbids.
#[test]
fn unmodeled_package_named_import_blocks_at_first_use() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r#"
import { map } from "lodash";

export const run = (values: number[]) => map(values, (value: number) => value + 1);
"#),
        &mut ctx,
    )?;
    ensure!(
        errors
            .iter()
            .any(|error| error.message.contains("unresolved package `lodash`")),
        "a named import from an unmodeled package must block by name: {errors:?}",
    );
    Ok(())
}

/// Importing an unmodeled package without using its value stays free.
///
/// The blocker is at the use, so a type-only import — the shape a program uses
/// to annotate against a framework whose values it never constructs — must keep
/// lowering.
#[test]
fn unmodeled_package_type_only_import_is_free() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import type { Request } from "express";

export function methodOf(request: Request): string {
  return typeof request;
}
"#),
        &mut ctx,
    )?;
    Ok(())
}

/// Return whether any lowered body contains an expression matching `predicate`.
fn any_host_expr(ctx: &HirCtx, predicate: impl Fn(&smelt_hir::ExprKind) -> bool) -> bool {
    ctx.krate
        .bodies
        .iter()
        .any(|body| body.exprs.iter().any(|expr| predicate(&expr.kind)))
}

/// `node:path` is modeled: every import shape reaches the same host call.
///
/// `path.join`/`path.resolve` once had a lowering rule that returned an empty
/// string literal (`resolve(__dirname, '../key.pub')` became `""`), then were
/// declared blockers. They are now Node's own POSIX algorithms, resolved from
/// the IMPORT the callee names — a default import, a namespace import through
/// `path.posix`, and an aliased named import from `node:path/posix` all lower
/// to `HostModuleCall { Path(..) }`.
#[test]
fn node_path_import_shapes_lower_to_host_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import path from 'path';
import * as ns from 'node:path';
import { join as posixJoin, basename } from 'node:path/posix';

export const a = path.join('/tmp', 'updater.json');
export const b = ns.posix.resolve('/etc', '../key.pub');
export const c = posixJoin('x', 'y');
export const d = basename('/a/b.txt', '.txt');
export const e = path.sep;
"),
        &mut ctx,
    )?;
    for op in [
        smelt_hir::PathOp::Join,
        smelt_hir::PathOp::Resolve,
        smelt_hir::PathOp::Basename,
    ] {
        ensure!(
            any_host_expr(&ctx, |kind| matches!(
                kind,
                smelt_hir::ExprKind::HostModuleCall { op: smelt_hir::HostModuleOp::Path(found), .. }
                    if *found == op
            )),
            "expected a `{}` host call",
            op.name()
        );
    }
    ensure!(
        any_host_expr(&ctx, |kind| matches!(
            kind,
            smelt_hir::ExprKind::Literal(smelt_hir::Literal::String(text)) if text == "/"
        )),
        "`path.sep` must fold to the POSIX separator"
    );
    Ok(())
}

/// A program's OWN `join` is not the host function, whatever it is called.
///
/// Recognition is keyed on the import, so a local `join` (and a `basename`
/// imported from a source module) keeps its own meaning.
#[test]
fn own_join_is_not_the_path_host_call() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function join(...parts: string[]): string {
  return parts.join('+');
}
export const joined = join('a', 'b');
"),
        &mut ctx,
    )?;
    ensure!(
        !any_host_expr(&ctx, |kind| matches!(kind, smelt_hir::ExprKind::HostModuleCall { .. })),
        "a source `join` must not lower to the node:path host call"
    );
    Ok(())
}

/// The record half of `node:path` (`parse`) stays declared and blocks by name.
#[test]
fn node_path_parse_stays_declared() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
import { parse } from 'path';

export const parsed = parse('/etc/key.pub');
"),
        &mut ctx,
    )?;
    ensure!(
        errors.iter().any(|error| error.message.contains("node:path")),
        "node:path parse must block by name: {errors:?}",
    );
    Ok(())
}

/// `createHash(..).update(..).digest(..)` lowers to fallible hasher calls on a
/// concrete `Hash`, never to an erased member chain.
#[test]
fn create_hash_chain_lowers_to_hasher_calls() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { createHash } from 'crypto';

export function hex(data: string): string {
  return createHash('sha256').update(data).digest('hex');
}
"),
        &mut ctx,
    )?;
    for op in [
        smelt_hir::HashOp::Create,
        smelt_hir::HashOp::Update,
        smelt_hir::HashOp::DigestText,
    ] {
        ensure!(
            any_host_expr(&ctx, |kind| matches!(
                kind,
                smelt_hir::ExprKind::HostModuleCall { op: smelt_hir::HostModuleOp::Hash(found), .. }
                    if *found == op
            )),
            "expected a `{}` hasher call",
            op.name()
        );
    }
    ensure!(
        smelt_hir::HostModuleOp::Hash(smelt_hir::HashOp::Create).is_fallible()
            && !smelt_hir::HostModuleOp::Path(smelt_hir::PathOp::Join).is_fallible(),
        "hasher calls throw Node's errors; path functions are total"
    );
    Ok(())
}

/// A run-time `new RegExp(pattern)` compiles fallibly; a literal one does not.
#[test]
fn dynamic_regexp_construction_is_a_fallible_compile() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function compile(source: string): RegExp {
  return new RegExp(`^${source}$`);
}
export const fixed = new RegExp('a+', 'g');
"),
        &mut ctx,
    )?;
    ensure!(
        any_host_expr(&ctx, |kind| matches!(kind, smelt_hir::ExprKind::RegExpCompile { .. })),
        "a template pattern must compile at run time"
    );
    ensure!(
        any_host_expr(&ctx, |kind| matches!(kind, smelt_hir::ExprKind::New { .. })),
        "a literal pattern keeps the infallible construction"
    );
    Ok(())
}

/// `globalThis` is ONE shared object, `Object.assign(global, ..)` writes into
/// it in place, and an ambient `declare const` reads its member back through a
/// checked cast to the declared type.
#[test]
fn global_object_store_backs_ambient_declarations() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
declare const __MANIFEST: string;
Object.assign(global, { __MANIFEST: '{}' });
export function manifest(): string {
  return __MANIFEST;
}
"),
        &mut ctx,
    )?;
    ensure!(
        any_host_expr(&ctx, |kind| matches!(kind, smelt_hir::ExprKind::GlobalObject)),
        "the global object must be the shared global-object node"
    );
    ensure!(
        !any_host_expr(&ctx, |kind| matches!(
            kind,
            smelt_hir::ExprKind::Literal(smelt_hir::Literal::String(text)) if text == "__smelt_global_object"
        )),
        "no per-read marker record may be minted any more"
    );
    ensure!(
        any_host_expr(&ctx, |kind| matches!(kind, smelt_hir::ExprKind::UnknownCast { .. })),
        "the ambient read must narrow the stored member to its declared type"
    );
    Ok(())
}
