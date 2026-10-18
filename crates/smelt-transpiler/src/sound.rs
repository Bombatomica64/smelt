//! Typed versions of npm packages: `.sound/<pkg>/`, `@sound/<pkg>` and `sound.json`.
//!
//! Design adapted from Bun's `--aot` (oven-sh/bun#45631), typed-package
//! overlays: the spec is the "Typed versions of packages" section of
//! `docs/bundler/executables.mdx` and `packages/bun-sound/README.md` ("Using
//! one", "When the warning comes", the `sound.json` field table); the check
//! order and every message are Bun's `src/sema/driver/typed_packages.rs`.
//!
//! # What a typed version is
//!
//! Most npm packages ship JavaScript, which Smelt cannot lower: a JavaScript
//! function's parameters carry no types, so the TypeScript frontend rejects it,
//! and an import of such a package stays the `unresolved package` blocker it
//! has always been. A *typed version* is the package's own published code with
//! TypeScript types added and nothing else changed. When the project has one
//! that fits the installed files exactly, the dependency collector resolves the
//! package through the typed text instead of the installed JavaScript, so the
//! package is lowered with concrete types. Nothing in `node_modules` is
//! modified: the laid files exist only in memory, for this build (see
//! `sound_overlay` for the resolution seam).
//!
//! # Lookup
//!
//! For a package named `pkg`, the first of these that has a `sound.json` wins:
//!
//! 1. `.sound/pkg/` in the directory the build runs from. For Smelt that is the
//!    **manifest directory** (the directory holding `Smelt.toml`), not the
//!    process's working directory: Smelt's project root is the manifest, and
//!    `--manifest-path` builds from anywhere.
//! 2. `node_modules/@sound/pkg` (`@sound/scope__pkg` for `@scope/pkg`) in the
//!    manifest directory or any ancestor, but only if the project's own
//!    `package.json` — the nearest one at or above the manifest directory —
//!    lists that companion in `dependencies` or `devDependencies`. A companion
//!    that is only there transitively is ignored, silently.
//!
//! Bun's third source, a package that ships its own typed text behind a
//! `// @sound` marker in its `index.d.ts`, is not implemented: the task's spec
//! names only the two lookups above.
//!
//! # All or nothing
//!
//! A typed version is used only when everything matches, in Bun's order:
//! format, name, version, `when` defines, `basis` hashes, `lay` entries, then
//! `needs`. The first mismatch produces ONE warning, in Bun's wording, and the
//! package is resolved as installed — which in Smelt today means the import
//! keeps its `unresolved package` blocker, exactly as without a typed version.
//! On success Smelt prints Bun's note.
//!
//! Every mismatch message is Bun's own text, with two deliberate departures:
//!
//! - the unknown-format reason says "this Smelt" where Bun says "this Bun",
//!   because the remedy (update the tool) names the tool doing the build;
//! - a laid file that is not UTF-8 is refused with "its lay/F is not UTF-8
//!   text". Bun has no such message (it reads bytes; Smelt's frontend reads
//!   `String`), so the wording follows Bun's own "it has no lay/F".
//!
//! # `when` and defines
//!
//! `when` maps a define key to the string value the build must give it. Smelt
//! reads defines from `[build] define` in `Smelt.toml` and from
//! `smelt build --define KEY=VALUE`, spelled like Bun's `--define`: the value
//! is JSON text, so `process.env.NODE_ENV` defined as `"production"` (with the
//! quotes) matches `"when": { "process.env.NODE_ENV": "production" }`. Smelt
//! consults defines ONLY for this matching: it does not substitute them into
//! the program text.
//!
//! # Format 2
//!
//! In format 2 a laid `.js`, `.cjs` or `.mjs` file holds TypeScript text and
//! replaces the installed file at its own path. Smelt has a single frontend
//! that parses every source as TypeScript, so a laid file is always handed to
//! it as TypeScript; in format 1 a laid `.js` holds JavaScript, which is
//! accepted wherever it needs no annotation (an entry that only re-exports the
//! typed text, the common case) and otherwise reports the frontend's usual
//! blocker.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;
use sha2::{Digest, Sha256};

/// The npm scope companion typed versions are published under.
const SCOPE: &str = "@sound";
/// The per-project folder that holds typed versions of patched packages.
const FOLDER: &str = ".sound";
/// The manifest every typed version carries.
const MANIFEST: &str = "sound.json";
/// The folder, inside a typed version, holding the laid files.
const REPLACEMENTS: &str = "lay";
/// How deep `needs` may recurse before it counts as unmet (Bun's bound).
const MAX_DEPTH: usize = 8;

/// The inputs a typed-version lookup depends on, fixed for one build.
#[derive(Debug, Clone)]
pub(crate) struct SoundSettings {
    /// Directory the build runs from: the manifest directory.
    pub(crate) root: PathBuf,
    /// Build-time defines as `(key, JSON text)` pairs, consulted for `when`.
    pub(crate) defines: Vec<(String, String)>,
}

/// One file a typed version lays over (or beside) the installed package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LaidFile {
    /// Absolute path inside the installed package the text takes the place of.
    pub(crate) path: PathBuf,
    /// The laid text, read from `lay/<same path>`.
    pub(crate) text: String,
}

/// What looking for a package's typed version found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TypedPackage {
    /// The package name as imported.
    pub(crate) name: String,
    /// Where the typed version lives, relative to the build root when inside it.
    pub(crate) shown: String,
    /// The laid files; empty when the typed version is not used.
    pub(crate) files: Vec<LaidFile>,
    /// The line to print: Bun's `note:` on success or `warn:` on a mismatch.
    pub(crate) message: String,
}

/// Look up and validate the typed version of package `name` installed at
/// `directory`.
///
/// Returns `None` when the project has no typed version for the package (the
/// package is then compiled as always, and nothing is said). Otherwise returns
/// the laid files when every check passes, or no files and a warning naming
/// the first mismatch.
pub(crate) fn typed_package(
    settings: &SoundSettings,
    directory: &Path,
    name: &str,
) -> Option<TypedPackage> {
    let home = typed_package_home(&settings.root, name)?;
    let shown = shown_path(&settings.root, &home);
    let (files, message) = match files_of(settings, &home, directory, name, 0) {
        Ok((files, version)) => (
            files,
            format!("note: \"{name}\" {version} is compiled from its typed version in {shown}"),
        ),
        Err(why) => (
            Vec::new(),
            format!(
                "warn: the typed version of \"{name}\" in {shown} is not used: {why}: the package is compiled untyped"
            ),
        ),
    };
    // Bun replaces control characters so a hostile manifest cannot drive the
    // terminal through the one line the build prints about it.
    let sanitized = message
        .chars()
        .map(|character| {
            if character.is_control() {
                '?'
            } else {
                character
            }
        })
        .collect();
    Some(TypedPackage {
        name: name.to_owned(),
        shown,
        files,
        message: sanitized,
    })
}

/// Name a typed version's home the way messages show it: relative to the build
/// root with `/` separators when inside it, absolute otherwise.
fn shown_path(root: &Path, home: &Path) -> String {
    home.strip_prefix(root).map_or_else(
        |_| home.display().to_string(),
        |inside| {
            inside
                .components()
                .map(|component| component.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/")
        },
    )
}

/// The companion package name a typed version is published under:
/// `@sound/pkg`, or `@sound/scope__pkg` for `@scope/pkg`.
pub(crate) fn companion_package_name(name: &str) -> String {
    let flat = name.strip_prefix('@').map_or_else(
        || name.to_owned(),
        |scoped| scoped.split('/').collect::<Vec<_>>().join("__"),
    );
    format!("{SCOPE}/{flat}")
}

/// Find where the typed version of `name` lives, if the project has one.
///
/// `.sound/<name>/` under the root wins; otherwise the nearest
/// `node_modules/@sound/<flat name>` at or above the root, provided the
/// project's own `package.json` (the nearest at or above the root) lists that
/// companion in `dependencies` or `devDependencies`.
fn typed_package_home(root: &Path, name: &str) -> Option<PathBuf> {
    let own = root.join(FOLDER).join(name);
    if own.join(MANIFEST).is_file() {
        return Some(own);
    }
    let companion = companion_package_name(name);
    let home = root
        .ancestors()
        .map(|at| at.join("node_modules").join(&companion))
        .find(|home| home.join(MANIFEST).is_file())?;
    let project_bytes = root
        .ancestors()
        .find_map(|at| fs::read(at.join("package.json")).ok())?;
    let project: Value = serde_json::from_slice(strip_bom(&project_bytes)).ok()?;
    let lists_it = |field: &str| {
        project
            .get(field)
            .is_some_and(|listed| listed.get(&companion).is_some())
    };
    (lists_it("dependencies") || lists_it("devDependencies")).then_some(home)
}

/// Strip a UTF-8 byte-order mark, which Bun's JSON reader also tolerates.
fn strip_bom(bytes: &[u8]) -> &[u8] {
    bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)
}

/// Read and parse a JSON file.
fn read_json(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(strip_bom(&bytes)).ok()
}

/// Whether a `sound.json` path stays inside the package: relative, `/`
/// separated, no empty, `.` or `..` segment, and not `package.json`, which a
/// typed version may never replace.
fn is_relative_inner_path(path: &str) -> bool {
    path != "package.json"
        && !path.contains(['\\', '\0'])
        && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

/// Whether a `needs` entry is a package name: one segment, or two for a scope.
fn is_package_name(name: &str) -> bool {
    is_relative_inner_path(name)
        && name.split('/').count() == if name.starts_with('@') { 2 } else { 1 }
}

/// The string field `field` of a JSON object, or `""` when absent or not a string.
fn text_of<'v>(json: &'v Value, field: &str) -> &'v str {
    json.get(field).and_then(Value::as_str).unwrap_or_default()
}

/// The entries of an object-of-strings field, in document order.
///
/// An absent field is empty. Order matters: the first mismatch is the one the
/// warning names, and Bun reports them in the order the manifest writes them.
/// `serde_json`'s `preserve_order` feature (enabled in this crate's manifest)
/// keeps that order.
fn entries<'m>(manifest: &'m Value, field: &str) -> Result<Vec<(&'m str, &'m str)>, String> {
    let Some(listed) = manifest.get(field) else {
        return Ok(Vec::new());
    };
    let refused = || format!("\"{field}\" of its sound.json is not an object of strings");
    listed
        .as_object()
        .ok_or_else(refused)?
        .iter()
        .map(|(path, value)| Ok((path.as_str(), value.as_str().ok_or_else(refused)?)))
        .collect()
}

/// Lowercase hex SHA-256 of `bytes`, the form `sound.json` pins files with.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Check that the installed file at `path` is the one the typed text was
/// written for.
fn verify_typed_against(directory: &Path, path: &str, wanted: &str) -> Result<(), String> {
    if !is_relative_inner_path(path) {
        return Err(format!(
            "\"{path}\" in its sound.json is no path inside the package"
        ));
    }
    let Ok(installed) = fs::read(directory.join(path)) else {
        return Err(format!("{path} is not installed"));
    };
    if sha256_hex(&installed) == wanted {
        Ok(())
    } else {
        Err(format!(
            "the installed {path} differs from the published file that this typed version was made from (was the package patched?)"
        ))
    }
}

/// Validate the typed version at `home` against the package installed at
/// `directory`, returning its laid files and version, or the first mismatch.
///
/// The order of the checks is Bun's, because the warning names only the first
/// mismatch and a user fixing one should meet the next in the same order Bun
/// would show it.
fn files_of(
    settings: &SoundSettings,
    home: &Path,
    directory: &Path,
    name: &str,
    depth: usize,
) -> Result<(Vec<LaidFile>, String), String> {
    let manifest = read_json(&home.join(MANIFEST)).ok_or("its sound.json is not JSON")?;
    let format = manifest.get("sound").and_then(Value::as_f64);
    if format != Some(1.0_f64) && format != Some(2.0_f64) {
        return Err("its sound.json is of a format that this Smelt does not know".into());
    }
    let installed = read_json(&directory.join("package.json"))
        .ok_or("the installed package.json is not JSON")?;
    if text_of(&manifest, "name") != name || text_of(&installed, "name") != name {
        return Err(format!(
            "it is for \"{}\", and \"{}\" is installed",
            text_of(&manifest, "name"),
            text_of(&installed, "name")
        ));
    }
    let found = text_of(&installed, "version");
    if text_of(&manifest, "version") != found {
        return Err(format!(
            "it is for version {}, and {found} is installed (install the typed version for {found}, or remove this one)",
            text_of(&manifest, "version"),
        ));
    }
    for (key, wanted) in entries(&manifest, "when")? {
        let defined = settings
            .defines
            .iter()
            .find(|(defined, _)| defined == key)
            .and_then(|(_, value)| serde_json::from_str::<Value>(value).ok());
        if defined.as_ref().and_then(Value::as_str) != Some(wanted) {
            return Err(format!(
                "it is for a build that defines {key} as \"{wanted}\" (pass --define '{key}=\"{wanted}\"')"
            ));
        }
    }
    for (path, wanted) in entries(&manifest, "basis")? {
        verify_typed_against(directory, path, wanted)?;
    }
    let mut files = Vec::new();
    for (path, wanted) in entries(&manifest, "lay")? {
        let at = directory.join(path);
        match wanted {
            "new" if !is_relative_inner_path(path) => {
                return Err(format!(
                    "\"{path}\" in its sound.json is no path inside the package"
                ));
            }
            "new" if at.exists() => {
                return Err(format!(
                    "{path} is installed, and it was not when the package was typed"
                ));
            }
            "new" => {}
            _ => verify_typed_against(directory, path, wanted)?,
        }
        let Ok(bytes) = fs::read(home.join(REPLACEMENTS).join(path)) else {
            return Err(format!("it has no lay/{path}"));
        };
        // Smelt-only: Bun reads bytes, Smelt's frontend reads `String`.
        let Ok(text) = String::from_utf8(bytes) else {
            return Err(format!("its lay/{path} is not UTF-8 text"));
        };
        files.push(LaidFile { path: at, text });
    }
    if files.is_empty() {
        return Err("its sound.json lays nothing".into());
    }
    let needs = manifest.get("needs").and_then(Value::as_array);
    for entry in needs.into_iter().flatten() {
        let needed = entry
            .as_str()
            .filter(|candidate| is_package_name(candidate))
            .ok_or("\"needs\" of its sound.json is not a list of packages' names")?;
        let fits = |there: PathBuf| {
            typed_package_home(&settings.root, needed).is_some_and(|needed_home| {
                files_of(
                    settings,
                    &needed_home,
                    &there,
                    needed,
                    depth.saturating_add(1),
                )
                .is_ok()
            })
        };
        let there = installed_package_dir(directory, needed);
        if depth == MAX_DEPTH || !there.is_some_and(fits) {
            return Err(format!(
                "it needs the typed version of \"{needed}\", which is not used"
            ));
        }
    }
    Ok((files, text_of(&manifest, "version").to_owned()))
}

/// Node's lookup of an installed package: the nearest
/// `node_modules/<name>/package.json` at or above `from`.
pub(crate) fn installed_package_dir(from: &Path, name: &str) -> Option<PathBuf> {
    from.ancestors()
        .map(|at| at.join("node_modules").join(name))
        .find(|there| there.join("package.json").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fail the test with both values when they differ (a `Result`-returning
    /// test may not panic).
    macro_rules! ensure_eq {
        ($left:expr, $right:expr $(,)?) => {
            ensure_eq!($left, $right, "values differ")
        };
        ($left:expr, $right:expr, $($context:tt)+) => {{
            let (left, right) = (&$left, &$right);
            if left != right {
                return Err(format!(
                    "{}\n  left: {left:?}\n right: {right:?}",
                    format!($($context)+)
                )
                .into());
            }
        }};
    }

    /// Fail the test when `condition` is false.
    macro_rules! ensure {
        ($condition:expr $(,)?) => {
            ensure!($condition, "{}", stringify!($condition))
        };
        ($condition:expr, $($context:tt)+) => {{
            if !$condition {
                return Err(format!($($context)+).into());
            }
        }};
    }

    /// Result type for tests that touch the filesystem.
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    /// A temporary project with one installed package and a typed version.
    struct Fixture {
        /// Keeps the temporary directory alive.
        dir: tempfile::TempDir,
    }

    /// The installed package's only module.
    const INDEX_JS: &str = "export function twice(x) {\n  return x * 2;\n}\n";
    /// The installed package's helper, typed against but not replaced.
    const UTIL_JS: &str = "export const unit = 1;\n";
    /// The typed text laid beside `index.js`.
    const INDEX_TS: &str = "export function twice(x: number): number {\n  return x * 2;\n}\n";

    impl Fixture {
        /// Project root (the manifest directory).
        fn root(&self) -> &Path {
            self.dir.path()
        }

        /// Installed package directory.
        fn package(&self) -> PathBuf {
            self.root().join("node_modules/pkg")
        }

        /// Home of the project-local typed version.
        fn home(&self) -> PathBuf {
            self.root().join(".sound/pkg")
        }

        /// A project with `pkg@1.0.0` installed and a fitting `.sound/pkg`.
        fn new() -> Result<Self, Box<dyn std::error::Error>> {
            let fixture = Self {
                dir: tempfile::tempdir()?,
            };
            fs::create_dir_all(fixture.package())?;
            fs::write(
                fixture.package().join("package.json"),
                r#"{ "name": "pkg", "version": "1.0.0", "main": "index.js" }"#,
            )?;
            fs::write(fixture.package().join("index.js"), INDEX_JS)?;
            fs::write(fixture.package().join("util.js"), UTIL_JS)?;
            fs::create_dir_all(fixture.home().join("lay"))?;
            fs::write(fixture.home().join("lay/index.ts"), INDEX_TS)?;
            fixture.write_manifest(&serde_json::json!({
                "sound": 1_i32,
                "name": "pkg",
                "version": "1.0.0",
                "when": { "process.env.NODE_ENV": "production" },
                "basis": { "util.js": sha256_hex(UTIL_JS.as_bytes()) },
                "lay": { "index.ts": "new" },
                "needs": []
            }))?;
            Ok(fixture)
        }

        /// Overwrite the typed version's `sound.json`.
        fn write_manifest(&self, manifest: &Value) -> TestResult {
            fs::write(
                self.home().join("sound.json"),
                serde_json::to_string(manifest)?,
            )?;
            Ok(())
        }

        /// Edit one field of the typed version's `sound.json`.
        fn set(&self, field: &str, value: Value) -> TestResult {
            let mut manifest =
                read_json(&self.home().join("sound.json")).ok_or("fixture manifest")?;
            manifest
                .as_object_mut()
                .ok_or("fixture manifest is an object")?
                .insert(field.to_owned(), value);
            self.write_manifest(&manifest)
        }

        /// Settings with the production define the fixture's `when` asks for.
        fn settings(&self) -> SoundSettings {
            SoundSettings {
                root: self.root().to_path_buf(),
                defines: vec![(
                    "process.env.NODE_ENV".to_owned(),
                    "\"production\"".to_owned(),
                )],
            }
        }

        /// The line the build would print for `pkg`.
        fn message(&self) -> Result<String, Box<dyn std::error::Error>> {
            self.message_with(&self.settings())
        }

        /// The line the build would print for `pkg` under `settings`.
        fn message_with(
            &self,
            settings: &SoundSettings,
        ) -> Result<String, Box<dyn std::error::Error>> {
            Ok(typed_package(settings, &self.package(), "pkg")
                .ok_or("a typed version must be found")?
                .message)
        }
    }

    /// The warning prefix and suffix around a mismatch reason.
    fn warning(why: &str) -> String {
        format!(
            "warn: the typed version of \"pkg\" in .sound/pkg is not used: {why}: the package is compiled untyped"
        )
    }

    /// A fitting typed version is used, says Bun's note, and lays its file at
    /// the installed package's path.
    #[test]
    fn fitting_typed_version_is_used_with_bun_note() -> TestResult {
        let fixture = Fixture::new()?;
        let found =
            typed_package(&fixture.settings(), &fixture.package(), "pkg").ok_or("must be found")?;
        ensure_eq!(
            found.message,
            "note: \"pkg\" 1.0.0 is compiled from its typed version in .sound/pkg"
        );
        ensure_eq!(
            found.files,
            vec![LaidFile {
                path: fixture.package().join("index.ts"),
                text: INDEX_TS.to_owned(),
            }]
        );
        Ok(())
    }

    /// No typed version: nothing is found and nothing is said.
    #[test]
    fn package_without_typed_version_is_silent() -> TestResult {
        let fixture = Fixture::new()?;
        fs::remove_dir_all(fixture.home())?;
        ensure!(typed_package(&fixture.settings(), &fixture.package(), "pkg").is_none());
        Ok(())
    }

    /// Bun's version-mismatch wording, exactly as the docs print it.
    #[test]
    fn version_mismatch_names_both_versions() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.set("version", Value::from("0.9.0"))?;
        ensure_eq!(
            fixture.message()?,
            warning(
                "it is for version 0.9.0, and 1.0.0 is installed (install the typed version for 1.0.0, or remove this one)"
            )
        );
        Ok(())
    }

    /// A manifest for another package is refused before its version is read.
    #[test]
    fn name_mismatch_names_both_packages() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.set("name", Value::from("other"))?;
        ensure_eq!(
            fixture.message()?,
            warning("it is for \"other\", and \"pkg\" is installed")
        );
        Ok(())
    }

    /// An unknown format compiles the package untyped; 2 is known.
    #[test]
    fn unknown_format_is_refused_and_format_two_is_known() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.set("sound", Value::from(3_i32))?;
        ensure_eq!(
            fixture.message()?,
            warning("its sound.json is of a format that this Smelt does not know")
        );
        fixture.set("sound", Value::from(2_i32))?;
        ensure!(fixture.message()?.starts_with("note: "));
        Ok(())
    }

    /// A missing or different `when` define names the define to pass.
    #[test]
    fn missing_when_define_says_what_to_pass() -> TestResult {
        let fixture = Fixture::new()?;
        let expected = warning(
            "it is for a build that defines process.env.NODE_ENV as \"production\" (pass --define 'process.env.NODE_ENV=\"production\"')",
        );
        let mut settings = fixture.settings();
        settings.defines.clear();
        ensure_eq!(fixture.message_with(&settings)?, expected);
        settings.defines = vec![(
            "process.env.NODE_ENV".to_owned(),
            "\"development\"".to_owned(),
        )];
        ensure_eq!(fixture.message_with(&settings)?, expected);
        // The define's value is JSON text: an unquoted word is not the string.
        settings.defines = vec![("process.env.NODE_ENV".to_owned(), "production".to_owned())];
        ensure_eq!(fixture.message_with(&settings)?, expected);
        Ok(())
    }

    /// A patched basis file is detected by its hash.
    #[test]
    fn modified_basis_file_is_detected() -> TestResult {
        let fixture = Fixture::new()?;
        fs::write(
            fixture.package().join("util.js"),
            "export const unit = 2;\n",
        )?;
        ensure_eq!(
            fixture.message()?,
            warning(
                "the installed util.js differs from the published file that this typed version was made from (was the package patched?)"
            )
        );
        Ok(())
    }

    /// A replaced file is hash-checked too, and its replacement is laid at the
    /// installed path.
    #[test]
    fn modified_replaced_file_is_detected() -> TestResult {
        let fixture = Fixture::new()?;
        fs::write(fixture.home().join("lay/index.js"), INDEX_TS)?;
        fixture.set(
            "lay",
            serde_json::json!({ "index.js": sha256_hex(INDEX_JS.as_bytes()) }),
        )?;
        ensure!(fixture.message()?.starts_with("note: "));
        fs::write(fixture.package().join("index.js"), "export {};\n")?;
        ensure_eq!(
            fixture.message()?,
            warning(
                "the installed index.js differs from the published file that this typed version was made from (was the package patched?)"
            )
        );
        Ok(())
    }

    /// A `"new"` file that the installed package now has is another version.
    #[test]
    fn new_file_that_is_installed_is_refused() -> TestResult {
        let fixture = Fixture::new()?;
        fs::write(fixture.package().join("index.ts"), INDEX_TS)?;
        ensure_eq!(
            fixture.message()?,
            warning("index.ts is installed, and it was not when the package was typed")
        );
        Ok(())
    }

    /// Paths must stay inside the package, and `package.json` cannot be laid.
    #[test]
    fn invalid_paths_are_refused() -> TestResult {
        let fixture = Fixture::new()?;
        for path in [
            "../escape.ts",
            "/abs.ts",
            "a//b.ts",
            "./index.ts",
            "package.json",
            "a\\b.ts",
        ] {
            fixture.set("lay", serde_json::json!({ path: "new" }))?;
            ensure_eq!(
                fixture.message()?,
                warning(&format!(
                    "\"{path}\" in its sound.json is no path inside the package"
                )),
                "lay path {path}"
            );
        }
        fixture.set("lay", serde_json::json!({ "index.ts": "new" }))?;
        fixture.set("basis", serde_json::json!({ "../x.js": "00" }))?;
        ensure_eq!(
            fixture.message()?,
            warning("\"../x.js\" in its sound.json is no path inside the package")
        );
        Ok(())
    }

    /// A basis file the package does not have is named.
    #[test]
    fn missing_basis_file_is_named() -> TestResult {
        let fixture = Fixture::new()?;
        fs::remove_file(fixture.package().join("util.js"))?;
        ensure_eq!(fixture.message()?, warning("util.js is not installed"));
        Ok(())
    }

    /// A lay entry without its replacement file, and a manifest that lays
    /// nothing, are refused.
    #[test]
    fn missing_lay_file_and_empty_lay_are_refused() -> TestResult {
        let fixture = Fixture::new()?;
        fs::remove_file(fixture.home().join("lay/index.ts"))?;
        ensure_eq!(fixture.message()?, warning("it has no lay/index.ts"));
        fixture.set("lay", serde_json::json!({}))?;
        ensure_eq!(fixture.message()?, warning("its sound.json lays nothing"));
        Ok(())
    }

    /// Malformed fields are named.
    #[test]
    fn malformed_fields_are_named() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.set("basis", serde_json::json!({ "util.js": 1_i32 }))?;
        ensure_eq!(
            fixture.message()?,
            warning("\"basis\" of its sound.json is not an object of strings")
        );
        fixture.set("basis", serde_json::json!({}))?;
        fixture.set("needs", serde_json::json!(["../x"]))?;
        ensure_eq!(
            fixture.message()?,
            warning("\"needs\" of its sound.json is not a list of packages' names")
        );
        fs::write(fixture.home().join("sound.json"), "{ not json")?;
        ensure_eq!(fixture.message()?, warning("its sound.json is not JSON"));
        Ok(())
    }

    /// `needs` requires the other package's typed version to fit too.
    #[test]
    fn unmet_needs_is_refused_and_met_needs_passes() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.set("needs", serde_json::json!(["dep"]))?;
        let expected = warning("it needs the typed version of \"dep\", which is not used");
        // Not installed.
        ensure_eq!(fixture.message()?, expected);
        // Installed, but no typed version.
        let dep = fixture.root().join("node_modules/dep");
        fs::create_dir_all(&dep)?;
        fs::write(
            dep.join("package.json"),
            r#"{ "name": "dep", "version": "2.0.0" }"#,
        )?;
        ensure_eq!(fixture.message()?, expected);
        // Installed with a fitting typed version.
        let home = fixture.root().join(".sound/dep");
        fs::create_dir_all(home.join("lay"))?;
        fs::write(home.join("lay/index.ts"), "export const d: number = 1;\n")?;
        let manifest = serde_json::json!({
            "sound": 1_i32, "name": "dep", "version": "2.0.0", "lay": { "index.ts": "new" }
        });
        fs::write(home.join("sound.json"), serde_json::to_string(&manifest)?)?;
        ensure!(fixture.message()?.starts_with("note: "));
        // A typed version that no longer fits is "not used".
        fs::write(
            dep.join("package.json"),
            r#"{ "name": "dep", "version": "2.0.1" }"#,
        )?;
        ensure_eq!(fixture.message()?, expected);
        Ok(())
    }

    /// The first mismatch in document order is the one reported.
    #[test]
    fn first_mismatch_in_document_order_is_reported() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.set(
            "basis",
            serde_json::json!({ "zeta.js": "00", "alpha.js": "00" }),
        )?;
        ensure_eq!(fixture.message()?, warning("zeta.js is not installed"));
        Ok(())
    }

    /// `@sound/pkg` is used only when the project's own package.json lists it.
    #[test]
    fn companion_is_used_only_when_listed_by_the_project() -> TestResult {
        let fixture = Fixture::new()?;
        let companion = fixture.root().join("node_modules/@sound/pkg");
        fs::create_dir_all(companion.parent().ok_or("parent")?)?;
        fs::rename(fixture.home(), &companion)?;
        // Transitive only: the project does not list it, so it is ignored.
        fs::write(
            fixture.root().join("package.json"),
            r#"{ "name": "app", "dependencies": { "pkg": "1.0.0" } }"#,
        )?;
        ensure!(typed_package(&fixture.settings(), &fixture.package(), "pkg").is_none());
        fs::write(
            fixture.root().join("package.json"),
            r#"{ "name": "app", "devDependencies": { "@sound/pkg": "1.0.0" } }"#,
        )?;
        ensure_eq!(
            fixture.message()?,
            "note: \"pkg\" 1.0.0 is compiled from its typed version in node_modules/@sound/pkg"
        );
        Ok(())
    }

    /// Scoped packages flatten into the `@sound` scope.
    #[test]
    fn scoped_companion_name_is_flattened() -> TestResult {
        ensure_eq!(companion_package_name("@scope/pkg"), "@sound/scope__pkg");
        ensure_eq!(companion_package_name("react-dom"), "@sound/react-dom");
        Ok(())
    }

    /// Control characters in the manifest cannot reach the terminal.
    #[test]
    fn control_characters_are_replaced() -> TestResult {
        let fixture = Fixture::new()?;
        fixture.set("version", Value::from("1.0\u{1b}[31m"))?;
        let message = fixture.message()?;
        ensure!(!message.contains('\u{1b}'), "{message}");
        ensure!(message.contains("1.0?[31m"), "{message}");
        Ok(())
    }
}
