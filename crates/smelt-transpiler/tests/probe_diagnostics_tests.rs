//! Integration tests for recoverable, categorized diagnostics
//! (`smelt check --message-format json`).

mod common;

use std::fs;

use common::{TempProject, TestResult, ensure, smelt, utf8_path};

/// `check --message-format json` keeps lowering past the first failing file and
/// reports every module's diagnostics, each tagged with a structured category.
#[test]
fn check_json_recovers_and_categorizes_across_files() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "diag-recovery"
version = "0.1.0"

[sources]
entries = ["src/a.ts", "src/b.ts"]

[output]
target = "./dist"
crate-name = "diag_recovery"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    // First file references an unknown user symbol; second references a known
    // builtin Smelt does not model. A fail-fast pass would only report the first.
    // (`Reflect`/`Math`/`JSON` now resolve as namespace values, so this uses
    // `structuredClone`, which has no runtime implementation yet.)
    fs::write(project_path.join("src/a.ts"), "console.log(missingAlpha);\n")?;
    fs::write(project_path.join("src/b.ts"), "const f = structuredClone;\n")?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    let json = smelt(&[
        "--manifest-path",
        &manifest_arg,
        "check",
        "--message-format",
        "json",
    ])?;

    // Both files surface (recovery past the first failure).
    ensure(json.contains("missingAlpha"), "missing first-file diagnostic")?;
    ensure(
        json.contains("structuredClone"),
        "missing second-file diagnostic",
    )?;
    // Categories are decided in the frontend and serialized in kebab-case.
    ensure(
        json.contains("\"unresolved-reference\""),
        "expected unresolved-reference category for unknown symbol",
    )?;
    ensure(
        json.contains("\"missing-stdlib\""),
        "expected missing-stdlib category for known builtin",
    )?;
    Ok(())
}

/// `[sources] exclude` prunes the dependency CLOSURE, not only the root set, and
/// a *value* imported from an excluded module reports the exclusion by name.
///
/// This is the shape root-only filtering cannot express: `src/main.ts` reaches
/// the excluded module both directly and through a barrel that re-exports its
/// types. Before closure pruning the excluded module was pulled in
/// transitively and lowered as if it were in scope, which is why an exclusion
/// could be written in the manifest and silently have no effect.
#[test]
fn exclude_prunes_the_dependency_closure_and_names_value_imports() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/client"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "exclude-closure"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]
exclude = ["src/client/**"]

[output]
target = "./dist"
crate-name = "exclude_closure"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    // The excluded module. `makeClient` is a value; `ClientOptions` a type.
    fs::write(
        project_path.join("src/client/index.ts"),
        "export type ClientOptions = { base: string };\nexport const makeClient = (base: string): string => base;\n",
    )?;
    // A type-only re-export from the excluded module must stay free: this is
    // how a barrel keeps publishing an out-of-scope module's type surface.
    fs::write(
        project_path.join("src/types.ts"),
        "export type { ClientOptions } from './client';\n",
    )?;
    // Using the excluded module's VALUE must block, and name the exclusion.
    fs::write(
        project_path.join("src/main.ts"),
        "import { makeClient } from './client';\nexport const base = makeClient('x');\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    let json = smelt(&[
        "--manifest-path",
        &manifest_arg,
        "check",
        "--message-format",
        "json",
    ])?;

    ensure(
        json.contains("makeClient") && json.contains("which the manifest excludes"),
        &format!("expected a named exclusion blocker for the value import, got {json}"),
    )?;
    // The excluded file must not have been lowered as a module of the crate.
    ensure(
        !json.contains("client/index.ts"),
        &format!("excluded module should be pruned from the closure, got {json}"),
    )?;

    Ok(())
}

/// A type-only import from an excluded module is free, and a type-only
/// re-export through a barrel is too.
///
/// This is the half that must NOT block: excluding a module removes its
/// implementation from the crate, not its type surface, and a barrel that
/// re-exports `export type { X } from './excluded'` stays usable. Hono's
/// `src/index.ts` is exactly this shape.
#[test]
fn exclude_keeps_type_only_imports_and_reexports_free() -> TestResult {
    let project = TempProject::new()?;
    let project_path = project.path();
    fs::create_dir_all(project_path.join("src/client"))?;
    fs::write(
        project_path.join("Smelt.toml"),
        r#"[project]
name = "exclude-type-only"
version = "0.1.0"

[sources]
entries = ["src/main.ts"]
exclude = ["src/client/**"]

[output]
target = "./dist"
crate-name = "exclude_type_only"
build = false

[runtime]
clone-strategy = "aggressive"
"#,
    )?;
    fs::write(
        project_path.join("src/client/index.ts"),
        "export type ClientOptions = { base: string };\nexport const makeClient = (base: string): string => base;\n",
    )?;
    fs::write(
        project_path.join("src/types.ts"),
        "export type { ClientOptions } from './client';\n",
    )?;
    fs::write(
        project_path.join("src/main.ts"),
        "import type { ClientOptions } from './types';\nexport const base = (options: ClientOptions): string => options.base;\n",
    )?;

    let manifest_arg = utf8_path(&project_path.join("Smelt.toml"))?;
    let json = smelt(&[
        "--manifest-path",
        &manifest_arg,
        "check",
        "--message-format",
        "json",
    ])?;

    ensure(
        !json.contains("which the manifest excludes"),
        &format!("a type-only import from an excluded module must not block, got {json}"),
    )?;

    Ok(())
}
