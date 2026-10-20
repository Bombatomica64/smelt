//! End-to-end tests for typed versions of packages (`.sound/`, `@sound/*`).
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631), typed-package
//! overlays (`packages/bun-sound/README.md`). Each test builds a temporary
//! project that imports a JavaScript-only package from `node_modules` through
//! the `smelt` CLI, and checks what Bun's spec requires: the note or the one
//! warning, the package resolved through the laid typed text (concrete Rust
//! types) or as installed, and the program's behavior against `node` running
//! the installed JavaScript.
//!
//! "As installed" is today's behavior for a JavaScript package: Smelt cannot
//! lower JavaScript, so the import is the `unresolved package` blocker. The
//! mismatch tests assert exactly that, so a typed version that does not fit
//! never changes what the build does without one.

mod common;

use std::{fs, path::Path, process::Command};

use common::{
    TempProject, TestResult, cargo_run_manifest, ensure, ensure_eq, node_runs_typescript,
    node_typescript_stdout, utf8_path,
};
use sha2::{Digest, Sha256};

/// The installed package: JavaScript as npm ships it.
const INDEX_JS: &str = "export function makePoint(x, y) {
  return { x: x, y: y };
}
export function norm2(p) {
  return p.x * p.x + p.y * p.y;
}
export function sumTo(n) {
  let s = 0;
  for (let i = 1; i <= n; i++) {
    s += i;
  }
  return s;
}
";

/// The installed package's declarations, as a JS package with types ships them.
const INDEX_D_TS: &str = "export interface Point { x: number; y: number; }
export declare function makePoint(x: number, y: number): Point;
export declare function norm2(p: Point): number;
export declare function sumTo(n: number): number;
";

/// The typed version: `INDEX_JS` with types added and nothing else changed.
const INDEX_TS: &str = "export interface Point { x: number; y: number; }
export function makePoint(x: number, y: number): Point {
  return { x: x, y: y };
}
export function norm2(p: Point): number {
  return p.x * p.x + p.y * p.y;
}
export function sumTo(n: number): number {
  let s = 0;
  for (let i = 1; i <= n; i++) {
    s += i;
  }
  return s;
}
";

/// The program that uses the package.
const MAIN_TS: &str = "import { makePoint, norm2, sumTo } from \"geom\";

const p = makePoint(3, 4);
console.log(norm2(p));
console.log(sumTo(10));
";

/// The note a fitting `.sound/geom` produces.
const DOT_SOUND_NOTE: &str =
    "note: \"geom\" 1.0.0 is compiled from its typed version in .sound/geom";

/// The blocker an import of a JavaScript package has always produced.
const UNRESOLVED: &str =
    "unresolved package `geom`: not a source file and not a modeled host module";

/// Lowercase hex SHA-256, the form `sound.json` pins files with.
fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Write `text` to `root/relative`, creating parent directories.
fn write(root: &Path, relative: &str, text: &str) -> TestResult {
    let path = root.join(relative);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    Ok(())
}

/// A project importing the JavaScript package `geom@1.0.0`.
///
/// `extra_manifest` is appended to `Smelt.toml`; `project_package_json` is the
/// project's own `package.json`.
fn geom_project(root: &Path, extra_manifest: &str, project_package_json: &str) -> TestResult {
    write(
        root,
        "Smelt.toml",
        &format!(
            "[project]
name = \"typed-geom\"
version = \"0.1.0\"

[sources]
entries = [\"src/main.ts\"]

[output]
target = \"./dist\"
crate-name = \"typed_geom\"

[runtime]
clone-strategy = \"aggressive\"
{extra_manifest}"
        ),
    )?;
    write(root, "package.json", project_package_json)?;
    write(
        root,
        "node_modules/geom/package.json",
        r#"{ "name": "geom", "version": "1.0.0", "type": "module", "main": "index.js", "types": "index.d.ts" }"#,
    )?;
    write(root, "node_modules/geom/index.js", INDEX_JS)?;
    write(root, "node_modules/geom/index.d.ts", INDEX_D_TS)?;
    write(root, "src/main.ts", MAIN_TS)
}

/// The project `package.json` that depends on `geom` only.
const PROJECT_PACKAGE_JSON: &str =
    r#"{ "name": "app", "type": "module", "dependencies": { "geom": "1.0.0" } }"#;

/// Lay a typed version of `geom` at `home` (relative to `root`): `index.ts`
/// is new, `index.js` is its basis. `extra` is spliced into `sound.json`.
fn lay_typed_geom(root: &Path, home: &str, extra: &str) -> TestResult {
    write(root, &format!("{home}/lay/index.ts"), INDEX_TS)?;
    write(
        root,
        &format!("{home}/sound.json"),
        &format!(
            r#"{{
  "sound": 1,
  "name": "geom",
  "version": "1.0.0",{extra}
  "basis": {{ "index.js": "{}" }},
  "lay": {{ "index.ts": "new" }},
  "needs": []
}}"#,
            sha256_hex(INDEX_JS.as_bytes())
        ),
    )
}

/// What one `smelt` run did.
struct Run {
    /// Whether it exited successfully.
    success: bool,
    /// Its standard error.
    stderr: String,
}

/// Run `smelt --manifest-path <root>/Smelt.toml build <extra...>`.
fn build(root: &Path, extra: &[&str]) -> TestResult<Run> {
    let manifest = utf8_path(&root.join("Smelt.toml"))?;
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .args(["--manifest-path", &manifest, "build"])
        .args(extra)
        .output()?;
    Ok(Run {
        success: output.status.success(),
        stderr: String::from_utf8(output.stderr)?,
    })
}

/// The lines of `stderr` that are typed-version notes or warnings.
fn sound_lines(stderr: &str) -> Vec<&str> {
    stderr
        .lines()
        .filter(|line| line.starts_with("note: \"") || line.starts_with("warn: the typed version"))
        .collect()
}

/// `SmeltUnknown` counts of a generated crate, from `smelt-unknown-report`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct UnknownCounts {
    /// Every occurrence.
    total: u64,
    /// Occurrences classified as avoidable erasure.
    avoidable: u64,
    /// Occurrences outside the runtime prelude: the program's own.
    program: u64,
}

/// Measure a generated crate's `SmeltUnknown` usage.
fn unknown_counts(src: &Path) -> TestResult<UnknownCounts> {
    let output = Command::new(env!("CARGO_BIN_EXE_smelt"))
        .args(["smelt-unknown-report", &utf8_path(src)?, "--format", "json"])
        .output()?;
    ensure(output.status.success(), "smelt-unknown-report failed")?;
    let report: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    // A category with no occurrence is absent from `category_totals`.
    let count = |pointer: &str| {
        report
            .pointer(pointer)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    let total = count("/total");
    Ok(UnknownCounts {
        total,
        avoidable: count("/category_totals/avoidable-erasure"),
        program: total.saturating_sub(count("/category_totals/runtime-prelude")),
    })
}

/// Check that a build left the installed package exactly as npm installed it:
/// no file changed and no stub written into it.
fn ensure_installed_package_untouched(root: &Path) -> TestResult {
    let mut installed = fs::read_dir(root.join("node_modules/geom"))?
        .map(|listed| listed.map(|file| file.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()?;
    installed.sort();
    ensure_eq(
        &installed,
        &vec![
            "index.d.ts".to_owned(),
            "index.js".to_owned(),
            "package.json".to_owned(),
        ],
        "installed package files",
    )?;
    ensure_eq(
        &fs::read_to_string(root.join("node_modules/geom/index.d.ts"))?,
        &INDEX_D_TS.to_owned(),
        "the installed declarations are untouched",
    )?;
    ensure_eq(
        &fs::read_to_string(root.join("node_modules/geom/index.js"))?,
        &INDEX_JS.to_owned(),
        "the installed JavaScript is untouched",
    )
}

/// Check that the generated Rust for the typed package carries concrete types.
fn ensure_package_rust_is_concrete(root: &Path) -> TestResult {
    let package_rust = fs::read_to_string(root.join("dist/src/geom_index.rs"))?;
    for signature in [
        "fn make_point(x: f64, y: f64) -> Point",
        "fn norm2(p: Point) -> f64",
        "fn sum_to(n: f64) -> f64",
    ] {
        ensure(
            package_rust.contains(signature),
            format!("missing `{signature}` in:\n{package_rust}"),
        )?;
    }
    ensure(
        !package_rust.contains("SmeltUnknown"),
        format!("the typed package must not erase:\n{package_rust}"),
    )?;
    let main_rust = fs::read_to_string(root.join("dist/src/main.rs"))?;
    ensure(
        main_rust.contains("struct Point {\n    x: f64,\n    y: f64,\n}"),
        "Point must be a concrete struct",
    )
}

/// Build the untyped code of the same package and measure it.
///
/// This is the installed JavaScript as the frontend would see it if it
/// accepted JavaScript: every parameter and result `any`. Smelt cannot lower
/// the JavaScript itself (that build is the `unresolved package` blocker), so
/// this is the closest untyped build of the same package.
fn untyped_counts() -> TestResult<UnknownCounts> {
    let project = TempProject::new()?;
    let root = project.path();
    geom_project(root, "", PROJECT_PACKAGE_JSON)?;
    let any_typed = INDEX_JS
        .replace("makePoint(x, y)", "makePoint(x: any, y: any): any")
        .replace("norm2(p)", "norm2(p: any): any")
        .replace("sumTo(n)", "sumTo(n: any): any");
    write(root, "src/geom.ts", &any_typed)?;
    write(
        root,
        "src/main.ts",
        &MAIN_TS.replace("\"geom\"", "\"./geom.ts\""),
    )?;
    let run = build(root, &[])?;
    ensure(
        run.success,
        format!("untyped build failed:\n{}", run.stderr),
    )?;
    unknown_counts(&root.join("dist/src"))
}

/// Run the generated program and, when `node` is available, compare it with
/// Node running the original program on the installed JavaScript.
fn ensure_generated_program_matches_node(root: &Path) -> TestResult {
    let stdout = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"25\n55\n".to_owned(), "generated program stdout")?;
    if !node_runs_typescript() {
        return Ok(());
    }
    let node = node_typescript_stdout(
        &[
            ("package.json", PROJECT_PACKAGE_JSON),
            (
                "node_modules/geom/package.json",
                r#"{ "name": "geom", "version": "1.0.0", "type": "module", "main": "index.js" }"#,
            ),
            ("node_modules/geom/index.js", INDEX_JS),
            ("src/main.ts", MAIN_TS),
        ],
        "src/main.ts",
    )?;
    ensure_eq(
        &stdout,
        &node,
        "generated program vs node on the installed JavaScript",
    )
}

/// A fitting `.sound/<pkg>` is used: Bun's note, the package lowered from the
/// laid text with concrete types, fewer `SmeltUnknown`s than the untyped code
/// gets, `node_modules` untouched, and the same stdout as `node` running the
/// installed JavaScript.
///
/// Requirements: behavioral — the program prints what Node prints with the
/// installed package; type preservation — `Point` is a struct, `norm2` takes a
/// `Point` and returns `f64`, and the package's Rust has no `SmeltUnknown`.
#[test]
fn dot_sound_typed_version_compiles_the_package_with_concrete_types() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    geom_project(root, "", PROJECT_PACKAGE_JSON)?;
    lay_typed_geom(root, ".sound/geom", "")?;

    let run = build(root, &[])?;
    ensure(run.success, format!("build failed:\n{}", run.stderr))?;
    ensure_eq(&sound_lines(&run.stderr), &vec![DOT_SOUND_NOTE], "the note")?;
    ensure_installed_package_untouched(root)?;
    ensure_package_rust_is_concrete(root)?;

    let typed = unknown_counts(&root.join("dist/src"))?;
    let untyped = untyped_counts()?;
    ensure(
        typed.total < untyped.total
            && typed.avoidable < untyped.avoidable
            && typed.program < untyped.program,
        format!("typed {typed:?} must erase less than untyped {untyped:?}"),
    )?;
    ensure_eq(
        &(typed.avoidable, typed.program),
        &(0, 0),
        "the typed build erases nothing",
    )?;

    ensure_generated_program_matches_node(root)
}

/// `@sound/<pkg>` listed in the project's `devDependencies` is used, and named
/// by its `node_modules` path.
#[test]
fn listed_sound_companion_is_used() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    geom_project(
        root,
        "",
        r#"{ "name": "app", "dependencies": { "geom": "1.0.0" }, "devDependencies": { "@sound/geom": "1.0.0" } }"#,
    )?;
    lay_typed_geom(root, "node_modules/@sound/geom", "")?;
    let run = build(root, &[])?;
    ensure(run.success, format!("build failed:\n{}", run.stderr))?;
    ensure_eq(
        &sound_lines(&run.stderr),
        &vec![
            "note: \"geom\" 1.0.0 is compiled from its typed version in node_modules/@sound/geom",
        ],
        "the note",
    )
}

/// An `@sound/<pkg>` that only a dependency depends on is ignored silently:
/// the package is resolved as installed, which is today's blocker.
#[test]
fn transitive_sound_companion_is_ignored() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    geom_project(root, "", PROJECT_PACKAGE_JSON)?;
    lay_typed_geom(root, "node_modules/@sound/geom", "")?;
    // Some other installed package depends on it; the project does not.
    write(
        root,
        "node_modules/other/package.json",
        r#"{ "name": "other", "version": "1.0.0", "dependencies": { "@sound/geom": "1.0.0" } }"#,
    )?;
    let run = build(root, &[])?;
    ensure(
        !run.success,
        "the untyped JavaScript package must still be unresolved",
    )?;
    ensure(
        run.stderr.contains(UNRESOLVED),
        format!("expected the blocker:\n{}", run.stderr),
    )?;
    ensure_eq(
        &sound_lines(&run.stderr),
        &Vec::<&str>::new(),
        "nothing is said",
    )
}

/// After the installed package is upgraded, the typed version is not used:
/// one warning in Bun's wording, and the build does exactly what it does with
/// no typed version — the JavaScript package's `unresolved package` blocker.
#[test]
fn version_mismatch_warns_and_resolves_the_package_as_installed() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    geom_project(root, "", PROJECT_PACKAGE_JSON)?;
    lay_typed_geom(root, ".sound/geom", "")?;
    write(
        root,
        "node_modules/geom/package.json",
        r#"{ "name": "geom", "version": "1.0.1", "type": "module", "main": "index.js" }"#,
    )?;
    let run = build(root, &[])?;
    ensure_eq(
        &sound_lines(&run.stderr),
        &vec![
            "warn: the typed version of \"geom\" in .sound/geom is not used: it is for version 1.0.0, and 1.0.1 is installed (install the typed version for 1.0.1, or remove this one): the package is compiled untyped",
        ],
        "one warning",
    )?;
    ensure(
        !run.success,
        "the untyped JavaScript package is unresolved, as today",
    )?;
    ensure(
        run.stderr.contains(UNRESOLVED),
        format!("expected the blocker:\n{}", run.stderr),
    )?;

    // Without the typed version the build fails the same way.
    fs::remove_dir_all(root.join(".sound"))?;
    let bare = build(root, &[])?;
    ensure(
        !bare.success && bare.stderr.contains(UNRESOLVED),
        "same failure without it",
    )
}

/// A patched installed file is caught by its `basis` hash.
#[test]
fn patched_basis_file_warns() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    geom_project(root, "", PROJECT_PACKAGE_JSON)?;
    lay_typed_geom(root, ".sound/geom", "")?;
    write(
        root,
        "node_modules/geom/index.js",
        &INDEX_JS.replace("i++", "i += 1"),
    )?;
    let run = build(root, &[])?;
    ensure_eq(
        &sound_lines(&run.stderr),
        &vec![
            "warn: the typed version of \"geom\" in .sound/geom is not used: the installed index.js differs from the published file that this typed version was made from (was the package patched?): the package is compiled untyped",
        ],
        "one warning",
    )
}

/// `when` is matched against defines from `--define` and from `[build] define`.
#[test]
fn when_is_matched_against_cli_and_manifest_defines() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    geom_project(root, "", PROJECT_PACKAGE_JSON)?;
    lay_typed_geom(
        root,
        ".sound/geom",
        r#"
  "when": { "process.env.NODE_ENV": "production" },"#,
    )?;

    let without = build(root, &[])?;
    ensure_eq(
        &sound_lines(&without.stderr),
        &vec![
            "warn: the typed version of \"geom\" in .sound/geom is not used: it is for a build that defines process.env.NODE_ENV as \"production\" (pass --define 'process.env.NODE_ENV=\"production\"'): the package is compiled untyped",
        ],
        "no define",
    )?;

    let with_cli = build(root, &["--define", "process.env.NODE_ENV=\"production\""])?;
    ensure(
        with_cli.success,
        format!("build failed:\n{}", with_cli.stderr),
    )?;
    ensure_eq(
        &sound_lines(&with_cli.stderr),
        &vec![DOT_SOUND_NOTE],
        "--define",
    )?;

    geom_project(
        root,
        "\n[build]\ndefine = { \"process.env.NODE_ENV\" = '\"production\"' }\n",
        PROJECT_PACKAGE_JSON,
    )?;
    let with_manifest = build(root, &[])?;
    ensure(
        with_manifest.success,
        format!("build failed:\n{}", with_manifest.stderr),
    )?;
    ensure_eq(
        &sound_lines(&with_manifest.stderr),
        &vec![DOT_SOUND_NOTE],
        "[build] define",
    )?;

    // The command line overrides the manifest.
    let overridden = build(root, &["--define", "process.env.NODE_ENV=\"development\""])?;
    ensure(
        sound_lines(&overridden.stderr)
            .first()
            .is_some_and(|line| line.starts_with("warn: ")),
        "a --define overrides [build] define",
    )
}

/// In format 2 a laid `.js` holds TypeScript and replaces the installed file
/// at its own path; Smelt lowers it as TypeScript with concrete types.
#[test]
fn format_two_lays_typescript_at_the_installed_js_path() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    geom_project(root, "", PROJECT_PACKAGE_JSON)?;
    write(root, ".sound/geom/lay/index.js", INDEX_TS)?;
    write(
        root,
        ".sound/geom/sound.json",
        &format!(
            r#"{{ "sound": 2, "name": "geom", "version": "1.0.0", "lay": {{ "index.js": "{}" }} }}"#,
            sha256_hex(INDEX_JS.as_bytes())
        ),
    )?;
    let run = build(root, &[])?;
    ensure(run.success, format!("build failed:\n{}", run.stderr))?;
    ensure_eq(&sound_lines(&run.stderr), &vec![DOT_SOUND_NOTE], "the note")?;
    let package_rust = fs::read_to_string(root.join("dist/src/geom_index.rs"))?;
    ensure(
        package_rust.contains("node_modules/geom/index.js")
            && package_rust.contains("fn norm2(p: Point) -> f64"),
        format!("the laid text at the installed path, typed:\n{package_rust}"),
    )?;
    // The installed JavaScript is untouched.
    ensure_eq(
        &fs::read_to_string(root.join("node_modules/geom/index.js"))?,
        &INDEX_JS.to_owned(),
        "node_modules is never modified",
    )
}

/// Laid `"new"` files find each other through ordinary relative imports,
/// although neither exists on disk.
#[test]
fn laid_new_files_import_each_other() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    geom_project(root, "", PROJECT_PACKAGE_JSON)?;
    write(
        root,
        ".sound/geom/lay/index.ts",
        "export { makePoint, norm2, sumTo } from \"./lib/impl.js\";\n",
    )?;
    write(root, ".sound/geom/lay/lib/impl.ts", INDEX_TS)?;
    write(
        root,
        ".sound/geom/sound.json",
        r#"{ "sound": 1, "name": "geom", "version": "1.0.0", "lay": { "index.ts": "new", "lib/impl.ts": "new" } }"#,
    )?;
    let run = build(root, &[])?;
    ensure(run.success, format!("build failed:\n{}", run.stderr))?;
    ensure_eq(&sound_lines(&run.stderr), &vec![DOT_SOUND_NOTE], "the note")?;
    let stdout = cargo_run_manifest(&root.join("dist/Cargo.toml"))?;
    ensure_eq(&stdout, &"25\n55\n".to_owned(), "generated program stdout")
}

/// A typed version whose text Smelt's frontend rejects stops the build with an
/// error that names the typed version (Bun's `TS100021`, adapted).
#[test]
fn rejected_typed_version_stops_the_build_naming_it() -> TestResult {
    let project = TempProject::new()?;
    let root = project.path();
    geom_project(root, "", PROJECT_PACKAGE_JSON)?;
    lay_typed_geom(root, ".sound/geom", "")?;
    // A parameter left without its type: the text is not a typed version.
    write(
        root,
        ".sound/geom/lay/index.ts",
        &INDEX_TS.replace("sumTo(n: number)", "sumTo(n)"),
    )?;
    let run = build(root, &[])?;
    ensure(!run.success, "the build must stop")?;
    ensure(
        run.stderr.contains(
            "error: the typed version of 'geom' in .sound/geom is rejected by Smelt's frontend (",
        ) && run
            .stderr
            .contains("Fix it, or remove it to compile 'geom' untyped."),
        format!("expected the typed-version error:\n{}", run.stderr),
    )
}
