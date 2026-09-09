//! A type imported from a project source the crate does not have.
//!
//! The rule under test is deliberately one case wide. A type reference that
//! resolves to no declaration used to become a nominal `Type::Class`, which is
//! erased — and an erased union member makes the WHOLE union non-concrete, so
//! the union reached the emitter as `SmeltUnknown` far from the reference with
//! nothing naming the alias. Hono's five-arm `ResponseHeadersInit` erased that
//! way for weeks because `src/utils/mime.ts` was missing from the dependency
//! closure (see `blocker-logs/standards-generic-arm-and-typeof-indexed-alias.md`).
//!
//! So the blocker fires only for that gap: a RELATIVE specifier resolving to a
//! file the manifest's own source roots contain and the closure did not reach.
//! The three shapes that look similar and must keep the nominal fallback each
//! have a test below, because each one is a program `tsc` accepts.

use super::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Create a temp directory holding `other.ts` and return the directory.
///
/// The file has to exist on disk: both the seeded set and the frontend's module
/// keys are canonical paths, and canonicalization only works for real files.
fn project_dir(label: &str) -> Result<PathBuf, String> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("clock before unix epoch: {error}"))?
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("smelt_type_import_{label}_{unique}"));
    fs::create_dir_all(&dir).map_err(|error| format!("create temp dir: {error}"))?;
    fs::write(dir.join("other.ts"), "export type Other = string;\n")
        .map_err(|error| format!("write other.ts: {error}"))?;
    Ok(dir)
}

/// The canonical path string of a file inside the project directory.
fn canonical(dir: &Path, name: &str) -> Result<String, String> {
    fs::canonicalize(dir.join(name))
        .map(|path| path.display().to_string())
        .map_err(|error| format!("canonicalize {name}: {error}"))
}

/// A type imported from a project source outside the crate is a named blocker.
#[test]
fn a_type_from_a_project_source_outside_the_crate_is_named() -> Result<(), String> {
    let dir = project_dir("gap")?;
    let mut ctx = HirCtx::new();
    ctx.project_sources_outside_crate
        .insert(canonical(&dir, "other.ts")?);
    let main = dir.join("main.ts").display().to_string();

    let result = to_hir_with_path(
        ts!(r"
import type { Other } from './other';

export function describe(value: Other): string {
  return typeof value;
}
"),
        FileId(0),
        &main,
        &mut ctx,
    );

    let Err(errors) = result else {
        return Err("expected the unresolvable type import to be a blocker".to_owned());
    };
    ensure!(
        errors.iter().any(|error| {
            error.message.contains("`Other`") && error.message.contains("./other")
        }),
        "the blocker must name both the type and the module: {errors:?}"
    );
    Ok(())
}

/// A type from a module the crate DOES have keeps its existing lowering.
///
/// This is the cyclic type-only import: Hono's `types.ts` names `Context` from
/// `./context` while `context.ts` imports `types.ts`, so whichever lowers first
/// sees the other's declarations missing even though they exist. The module is
/// in the crate, so the reference must lower exactly as it did before — the
/// declaration arrives later.
#[test]
fn a_type_from_an_in_crate_module_is_not_blocked() -> Result<(), String> {
    let dir = project_dir("cycle")?;
    let mut ctx = HirCtx::new();
    // Something else in the project is outside the crate, so the check is live
    // rather than short-circuited by an empty set.
    fs::write(dir.join("elsewhere.ts"), "export type Elsewhere = number;\n")
        .map_err(|error| format!("write elsewhere.ts: {error}"))?;
    ctx.project_sources_outside_crate
        .insert(canonical(&dir, "elsewhere.ts")?);
    let main = dir.join("main.ts").display().to_string();

    to_hir_with_path(
        ts!(r"
import type { Other } from './other';

export function describe(value: Other): string {
  return typeof value;
}
"),
        FileId(0),
        &main,
        &mut ctx,
    )
    .map_err(|errors| format!("an in-crate module must not block: {errors:?}"))?;
    Ok(())
}

/// A type from a types-only external package keeps its existing lowering.
///
/// remeda imports `Simplify` from `type-fest`: a bare specifier, not a crate
/// module, and never will be. Only relative specifiers can name a project
/// source, so this returns to the nominal fallback.
#[test]
fn a_type_from_an_external_package_is_not_blocked() -> Result<(), String> {
    let dir = project_dir("package")?;
    let mut ctx = HirCtx::new();
    ctx.project_sources_outside_crate
        .insert(canonical(&dir, "other.ts")?);
    let main = dir.join("main.ts").display().to_string();

    to_hir_with_path(
        ts!(r"
import type { Simplify } from 'type-fest';

export function describe(value: Simplify): string {
  return typeof value;
}
"),
        FileId(0),
        &main,
        &mut ctx,
    )
    .map_err(|errors| format!("an external package type must not block: {errors:?}"))?;
    Ok(())
}

/// A type from an EXCLUDED module keeps its existing lowering.
///
/// `[sources] exclude` promises that excluding a module removes its
/// implementation, not its type surface. An excluded file is dropped from the
/// source-root set the check consults, so it is not in the outside-crate set
/// either and the reference keeps the nominal fallback — which is exactly the
/// freedom the overlays document.
#[test]
fn a_type_from_an_excluded_module_is_not_blocked() -> Result<(), String> {
    let dir = project_dir("excluded")?;
    let mut ctx = HirCtx::new();
    fs::write(dir.join("excluded.ts"), "export type Gone = string;\n")
        .map_err(|error| format!("write excluded.ts: {error}"))?;
    // `other.ts` stands in for some unrelated project source outside the crate;
    // `excluded.ts` is absent from the set the way an excluded file is.
    ctx.project_sources_outside_crate
        .insert(canonical(&dir, "other.ts")?);
    let main = dir.join("main.ts").display().to_string();

    to_hir_with_path(
        ts!(r"
import type { Gone } from './excluded';

export function describe(value: Gone): string {
  return typeof value;
}
"),
        FileId(0),
        &main,
        &mut ctx,
    )
    .map_err(|errors| format!("an excluded module's type must not block: {errors:?}"))?;
    Ok(())
}
