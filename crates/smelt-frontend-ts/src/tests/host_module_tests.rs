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
import { backup } from 'node:sqlite';

export const copy = backup;
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
import type { Session } from 'node:sqlite';

export function describeHandle(handle: Session): string {
  return typeof handle;
}
"),
        &mut ctx,
    )?;
    ensure_signature(
        &ctx,
        "describe_handle",
        "describe_handle(handle: Session) -> String",
    )?;
    Ok(())
}

/// A modeled host-module export keeps lowering through its own rule.
///
/// `tz` from `@date-fns/tz` is modeled, so the registry must not turn it into a
/// blocker; the call resolves through the registry, not through its spelling.
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
    ensure_exprs!(&ctx, DateTimezoneContext);
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
    ensure_signature(&ctx, "values", "values() -> List<Unknown>")?;
    ensure_exprs!(&ctx, BindThis);
    ensure!(crate_has_field(&ctx, "concat"), "no read of field `concat`");
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
    ensure_signature(
        &ctx,
        "test_builds_an_extension_date",
        "test_builds_an_extension_date() -> None",
    )?;
    ensure_exprs!(&ctx, InstanceOf);
    ensure!(
        crate_has_new(&ctx, "UTCDate"),
        "no construction of `UTCDate`"
    );
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
/// The framework that drives a program is exactly the surface whose erasure
/// produced the express false green, so a program module that calls into an
/// unmodeled package fails by name rather than emitting a crate with the
/// framework silently missing.
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
    ensure_signature(&ctx, "method_of", "method_of(request: Request) -> String")?;
    Ok(())
}

/// Return whether any lowered body contains an expression matching `predicate`.
fn any_host_expr(ctx: &HirCtx, predicate: impl Fn(&ExprKind) -> bool) -> bool {
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
                ExprKind::HostModuleCall { op: smelt_hir::HostModuleOp::Path(found), .. }
                    if *found == op
            )),
            "expected a `{}` host call",
            op.name()
        );
    }
    ensure!(
        any_host_expr(&ctx, |kind| matches!(
            kind,
            ExprKind::Literal(Literal::String(text)) if text == "/"
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
        !any_host_expr(&ctx, |kind| matches!(kind, ExprKind::HostModuleCall { .. })),
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
        errors
            .iter()
            .any(|error| error.message.contains("node:path")),
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
                ExprKind::HostModuleCall { op: smelt_hir::HostModuleOp::Hash(found), .. }
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
        any_host_expr(&ctx, |kind| matches!(kind, ExprKind::RegExpCompile { .. })),
        "a template pattern must compile at run time"
    );
    ensure!(
        any_host_expr(&ctx, |kind| matches!(kind, ExprKind::New { .. })),
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
        any_host_expr(&ctx, |kind| matches!(kind, ExprKind::GlobalObject)),
        "the global object must be the shared global-object node"
    );
    ensure!(
        !any_host_expr(&ctx, |kind| matches!(
            kind,
            ExprKind::Literal(Literal::String(text)) if text == "__smelt_global_object"
        )),
        "no per-read marker record may be minted any more"
    );
    ensure!(
        any_host_expr(&ctx, |kind| matches!(kind, ExprKind::UnknownCast { .. })),
        "the ambient read must narrow the stored member to its declared type"
    );
    Ok(())
}

/// `serve` from `@hono/node-server` lowers to a fetch server that listens.
#[test]
fn hono_node_server_serve_lowers_to_a_listening_fetch_server() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
import { serve } from '@hono/node-server';

const fetch = (request: Request): Response => new Response(request.url);
serve({ fetch, port: 8080 }, (): void => {
  console.log('up');
});
"),
        &mut ctx,
    )?;
    let kinds = ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .map(|expr| &expr.kind)
        .collect::<Vec<_>>();
    if !kinds
        .iter()
        .any(|kind| matches!(kind, ExprKind::HttpCreateFetchServer { .. }))
    {
        return Err("serve did not build a fetch server".to_owned());
    }
    if !kinds.iter().any(|kind| {
        matches!(kind, ExprKind::HttpServerOp { op: smelt_hir::HttpServerOp::Listen, args, .. } if args.len() == 3)
    }) {
        return Err("serve did not listen with port, host and callback".to_owned());
    }
    Ok(())
}

/// An option the adapter has but Smelt does not model is a named blocker,
/// never silently ignored.
#[test]
fn hono_node_server_serve_rejects_an_unmodeled_option() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
import { serve } from '@hono/node-server';

const fetch = (request: Request): Response => new Response(request.url);
serve({ fetch, port: 8080, overrideGlobalObjects: false });
"),
        &mut ctx,
    )?;
    if errors
        .iter()
        .any(|error| format!("{error:?}").contains("overrideGlobalObjects"))
    {
        Ok(())
    } else {
        Err(format!(
            "expected a blocker naming the option, got {errors:?}"
        ))
    }
}

/// A locally declared `serve` is the program's own function, not the adapter.
#[test]
fn a_local_serve_is_not_the_hono_adapter() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
function serve(options: { port: number }): number {
  return options.port;
}
console.log(serve({ port: 1 }));
"),
        &mut ctx,
    )?;
    if ctx
        .krate
        .bodies
        .iter()
        .flat_map(|body| body.exprs.iter())
        .any(|expr| matches!(expr.kind, ExprKind::HttpCreateFetchServer { .. }))
    {
        return Err("a local serve was lowered as the adapter".to_owned());
    }
    Ok(())
}

/// Whether any body of the crate holds an expression satisfying `predicate`.
fn any_expr(ctx: &HirCtx, predicate: impl Fn(&smelt_hir::Expr) -> bool) -> bool {
    ctx.krate
        .bodies
        .iter()
        .flat_map(|body| &body.exprs)
        .any(predicate)
}

/// The diagnostics for `source`, which must not lower.
fn blockers(source: &str) -> Result<Vec<String>, String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(source, &mut ctx)?;
    Ok(errors.into_iter().map(|error| error.message).collect())
}

/// Library helpers have no rule of their own: an unmodeled package blocks at the
/// use, whatever the helper is called and however it reaches the use.
#[test]
fn unmodeled_package_helpers_block_instead_of_being_stubbed() -> Result<(), String> {
    for source in [
        ts!(r#"
import _ from "lodash";
export const run = (value: { a: number }): boolean => _.has(value, "a");
"#),
        ts!(r#"
import _ from "lodash";
export function run(values: number[]): void { _.forEach(values, (value) => { console.log(value); }); }
"#),
        ts!(r#"
import fp from "lodash/fp";
export const run = (values: number[]) => fp.map((value: number) => value, values);
"#),
        ts!(r#"
import { async } from "@strapi/utils";
export const run = (values: number[]) => async.map(values, async (value: number) => value);
"#),
        ts!(r#"
import { isEmpty } from "lodash";
export const run = (values: string[]): string[] => values.filter(isEmpty);
"#),
        ts!(r#"
import { has } from "lodash/fp";
export const run = (values: { a?: number }[]) => values.filter(has("a"));
"#),
        ts!(r#"
import { omit } from "lodash/fp";
export const run = (values: { a: number }[]) => values.map(omit("a"));
"#),
    ] {
        let messages = blockers(source)?;
        ensure!(
            messages
                .iter()
                .any(|message| message.contains("unresolved package")),
            "expected a package blocker for {source}, got {messages:?}",
        );
    }
    Ok(())
}

/// `process` and `Intl` members nothing models are blockers, not reads of an
/// undefined field of the erased global.
#[test]
fn unmodeled_host_namespace_members_block() -> Result<(), String> {
    for (source, expected) in [
        (
            "export const dir = process.cwd();",
            "`process.cwd` is not modeled",
        ),
        (
            "export const zone = Intl.DateTimeFormat().resolvedOptions().timeZone;",
            "`Intl.DateTimeFormat` is not modeled",
        ),
        (
            "export const format = (date: Date) => new Intl.DateTimeFormat('en').format(date);",
            "`Intl.DateTimeFormat` is not modeled",
        ),
        (
            "export const format = (n: number) => new Intl.RelativeTimeFormat('en').format(n, 'day');",
            "`Intl.RelativeTimeFormat` is not modeled",
        ),
    ] {
        let messages = blockers(source)?;
        ensure!(
            messages.iter().any(|message| message.contains(expected)),
            "expected {expected} for {source}, got {messages:?}",
        );
    }
    Ok(())
}

/// A binding named like the host namespace is the binding, not the namespace.
#[test]
fn shadowed_host_namespace_call_is_not_blocked() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
export function dir(process: { cwd(): string }): string {
  return process.cwd();
}
"),
        &mut ctx,
    )?;
    ensure_exprs!(&ctx, BindThis);
    ensure!(crate_has_field(&ctx, "cwd"), "no read of field `cwd`");
    Ok(())
}

/// `strapi` is no ambient global: an undeclared name stays unresolved.
#[test]
fn undeclared_framework_global_does_not_resolve() -> Result<(), String> {
    let messages = blockers(ts!("export const config = () => strapi.config;\n"))?;
    ensure!(!messages.is_empty(), "expected `strapi` to stay unresolved");
    Ok(())
}

/// A library's own type names lower through their declarations: the tuple alias
/// of the corpus spelling keeps no name-keyed arity.
#[test]
fn library_type_names_have_no_rule_of_their_own() -> Result<(), String> {
    let messages = blockers(ts!(
        "export const head = (...xs: NonEmptyArray<number>): number => xs[0];\n"
    ))?;
    ensure!(
        !messages.is_empty(),
        "expected undeclared `NonEmptyArray` to stay unresolved"
    );
    Ok(())
}

/// The renamed `tz` import resolves through the registry, and a local `tz` that
/// shadows nothing imported is an ordinary function.
#[test]
fn timezone_factory_is_recognized_by_binding_not_spelling() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
import { tz as zone } from "@date-fns/tz";

export const named = zone("Europe/Rome");
"#),
        &mut ctx,
    )?;
    ensure!(
        any_expr(&ctx, |expr| matches!(
            expr.kind,
            ExprKind::DateTimezoneContext { .. }
        )),
        "expected the renamed import to reach the timezone rule",
    );
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r#"
function tz(name: string): string { return name; }

export const named = tz("Europe/Rome");
"#),
        &mut ctx,
    )?;
    ensure!(
        !any_expr(&ctx, |expr| matches!(
            expr.kind,
            ExprKind::DateTimezoneContext { .. }
        )),
        "a local `tz` must not reach the timezone rule",
    );
    Ok(())
}

/// A local object named `sinon` is not the test library.
#[test]
fn sinon_rule_requires_the_import() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    lower_ok(
        ts!(r"
const sinon = { useFakeTimers: (): number => 1 };
export const clock = sinon.useFakeTimers();
"),
        &mut ctx,
    )?;
    let is_fake_timers = |expr: &smelt_hir::Expr| {
        matches!(
            ctx.krate.types.get(expr.ty),
            Some(Type::Class { name, .. }) if ctx.krate.symbols.get(*name) == Some("SinonFakeTimers")
        )
    };
    ensure!(
        !any_expr(&ctx, is_fake_timers),
        "a local `sinon` must not be claimed by the fake-timers rule",
    );
    Ok(())
}
