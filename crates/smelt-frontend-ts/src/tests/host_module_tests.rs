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

/// `node:path` is declared, not modeled, and blocks by name.
///
/// `path.join`/`path.resolve` previously had a lowering rule that returned an
/// empty string literal, so `resolve(__dirname, '../key.pub')` became `""` and
/// the program went on to use it as a filename. That is a worse false green
/// than erasure — a wrong value with no diagnostic — so the rule is gone and
/// the surface is declared until it is implemented against `std::path`.
#[test]
fn node_path_surface_is_declared_not_faked() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
import path from 'path';

export const configPath = path.join('/tmp', 'updater.json');
"),
        &mut ctx,
    )?;
    let blocker = errors
        .iter()
        .find(|error| error.message.contains("node:path"))
        .ok_or_else(|| format!("node:path must block by name: {errors:?}"))?;
    ensure_eq!(
        blocker.category,
        smelt_stdlib::DiagnosticCategory::MissingStdlib
    );
    Ok(())
}

/// The named-import spelling of `node:path` blocks with the same reason.
#[test]
fn node_path_named_import_blocks() -> Result<(), String> {
    let mut ctx = HirCtx::new();
    let errors = lowering_errors(
        ts!(r"
import { resolve } from 'path';

export const keyPath = resolve('/etc', '../key.pub');
"),
        &mut ctx,
    )?;
    ensure!(
        errors.iter().any(|error| error.message.contains("node:path")),
        "node:path must block by name: {errors:?}",
    );
    Ok(())
}
