//! What counts as a source file, and how Smelt finds one.
//!
//! Owns extension classification ([`SourceLang`]), the directory walks, the
//! narrow glob syntax of `[sources] exclude` and `test-globs`, and the
//! manifest-relative path helpers those comparisons go through. It depends on
//! nothing else in this crate but configuration, so `lowering`, `manifest` and
//! the reports can all ask it the same question.

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::CliResult;

/// Extensions the TypeScript resolver tries for an extensionless specifier.
pub(crate) const RESOLVER_EXTENSIONS: [&str; 6] = [".ts", ".mts", ".cts", ".d.ts", ".py", ".pyi"];

/// Source language inferred from a file path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SourceLang {
    /// TypeScript source file.
    TypeScript,
    /// TypeScript declaration file.
    TypeScriptDeclaration,
    /// Python source file.
    Python,
    /// Python stub declaration file.
    PythonDeclaration,
}

impl SourceLang {
    /// Infers the source language from a path extension.
    pub(crate) fn from_path(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if is_typescript_declaration_path(path) {
            return Ok(Self::TypeScriptDeclaration);
        }
        if Path::new(path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pyi"))
        {
            return Ok(Self::PythonDeclaration);
        }
        match Path::new(path).extension().and_then(|e| e.to_str()) {
            // `.mts`/`.cts` are the ESM/CommonJS-pinned TypeScript inputs a
            // NodeNext project writes; they parse exactly like `.ts`.
            Some("ts" | "mts" | "cts") => Ok(Self::TypeScript),
            Some("py") => Ok(Self::Python),
            _ => Err(format!("unsupported source extension: {path}").into()),
        }
    }

    /// Returns whether this path kind is lowered by the TypeScript frontend.
    pub(crate) const fn is_typescript(self) -> bool {
        matches!(self, Self::TypeScript | Self::TypeScriptDeclaration)
    }
}

/// Return whether a token of a diagnostic names a `.ts`/`.py` source file.
pub(crate) fn names_source_file(token: &str) -> bool {
    matches!(
        Path::new(token).extension().and_then(|extension| extension.to_str()),
        Some("ts" | "py")
    )
}

/// Return whether a path names a TypeScript declaration file.
fn is_typescript_declaration_path(path: &str) -> bool {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|file_name| {
            let lower_name = file_name.to_ascii_lowercase();
            lower_name.ends_with(".d.ts")
                || lower_name.ends_with(".d.mts")
                || lower_name.ends_with(".d.cts")
        })
}

/// Recursively collect every TypeScript/Python source file under the manifest
/// source roots, skipping declaration files and vendored/output directories.
///
/// The skipped output directory is the manifest's `[output] target`, compared by
/// identity, so a source directory that merely shares a prefix with it stays.
/// Files whose manifest-relative path matches a `[sources] exclude` glob are
/// dropped so probe discovery honors the same exclusions as build discovery.
pub(crate) fn discover_source_files(
    config: &crate::config::Config,
    manifest_dir: &Path,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let roots = config
        .source_roots()
        .map_or_else(|| vec![PathBuf::from(".")], <[_]>::to_vec);
    let output_dir = resolve_manifest_path(manifest_dir, config.output_target())
        .canonicalize()
        .ok();
    let mut paths = Vec::new();
    for root in roots {
        let absolute_root = resolve_manifest_path(manifest_dir, &root);
        collect_source_files(&absolute_root, output_dir.as_deref(), &mut paths)?;
    }
    retain_included_paths(&mut paths, config.source_excludes(), manifest_dir);
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Recursively gather probe-eligible source files below `dir`.
fn collect_source_files(
    dir: &Path,
    output_dir: Option<&Path>,
    paths: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    if !dir.is_dir() {
        return Ok(());
    }
    for read_result in fs::read_dir(dir)? {
        let dir_entry = read_result?;
        let path = dir_entry.path();
        let file_type = dir_entry.file_type()?;
        if file_type.is_dir() {
            // Skip vendored dependencies and generated output directories.
            let vendored = path
                .file_name()
                .is_some_and(|name| name == "node_modules" || name == "__pycache__");
            if vendored || is_output_dir(&path, output_dir) {
                continue;
            }
            collect_source_files(&path, output_dir, paths)?;
        } else if file_type.is_file() && is_probe_source_file(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

/// Returns whether `dir` is the manifest's output directory.
fn is_output_dir(dir: &Path, output_dir: Option<&Path>) -> bool {
    output_dir.is_some_and(|output| dir.canonicalize().is_ok_and(|canonical| canonical == output))
}

/// Returns whether `path` is a `.ts`/`.py` source file the probe should lower.
fn is_probe_source_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name.ends_with(".d.ts") {
        return false;
    }
    names_source_file(name)
}

/// Drops paths whose manifest-relative form matches any `exclude` glob.
///
/// Exclusion patterns are matched against each path relative to the manifest
/// directory (for example `src/object/clone.spec.ts`), using the same narrow
/// glob syntax as [`path_matches_glob`]. Paths outside `manifest_dir` and paths
/// with no exclude patterns are always retained.
pub(crate) fn retain_included_paths(paths: &mut Vec<PathBuf>, excludes: &[String], manifest_dir: &Path) {
    if excludes.is_empty() {
        return;
    }
    paths.retain(|path| !is_excluded_source(path, excludes, manifest_dir));
}

/// Returns whether `path` matches any `exclude` glob relative to `manifest_dir`.
///
/// The comparison goes through [`exclusion_base`] / [`manifest_relative_path`]
/// so a manifest named with no directory component, or reached through a
/// symlink, matches the same globs as an absolute one. The ROOT set and the
/// dependency closure ask the same question and must not answer it differently.
fn is_excluded_source(path: &Path, excludes: &[String], manifest_dir: &Path) -> bool {
    let relative = manifest_relative_path(path, &exclusion_base(manifest_dir));
    excludes
        .iter()
        .any(|pattern| path_matches_glob(&relative, pattern))
}

/// Discovers test files matching configured source-root-relative glob patterns.
///
/// Test globs are matched against paths relative to EACH source root, so a
/// glob that repeats the root's own name (`src/**/*.test.ts` under
/// `roots = ["src"]`) can never match. That silent empty test set is
/// indistinguishable from "this project has no tests", so every glob that
/// matched nothing under any root is reported as a warning on stderr by both
/// `smelt build` and `smelt probe`.
pub(crate) fn discover_test_paths(
    config: &crate::config::Config,
    manifest_dir: &Path,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let patterns = config.test_globs();
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    let roots = config
        .source_roots()
        .map_or_else(|| vec![PathBuf::from(".")], <[_]>::to_vec);
    let mut paths = Vec::new();
    let mut matched = vec![false; patterns.len()];
    for root in &roots {
        let absolute_root = resolve_manifest_path(manifest_dir, root);
        collect_matching_test_paths(
            &absolute_root,
            &absolute_root,
            patterns,
            &mut matched,
            &mut paths,
        )?;
    }
    if let Some(warning) = empty_test_glob_warning(patterns, &matched, &roots) {
        eprintln!("{warning}");
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Formats the warning for `test-prefix` globs that matched no file at all.
///
/// Returns `None` when every configured glob matched at least one file under
/// some source root. The message names each dead glob and the roots the globs
/// were matched against, so the fix is readable straight from the warning.
fn empty_test_glob_warning(
    patterns: &[String],
    matched: &[bool],
    roots: &[PathBuf],
) -> Option<String> {
    let dead = patterns
        .iter()
        .enumerate()
        .filter(|(index, _)| !matched.get(*index).copied().unwrap_or(false))
        .map(|(_, pattern)| format!("`{pattern}`"))
        .collect::<Vec<_>>();
    if dead.is_empty() {
        return None;
    }
    let root_list = roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "warning: [sources] test-prefix glob{plural} {globs} matched no files under {where_}; globs are matched against paths relative to each source root, so a glob must not repeat the root's own path",
        plural = if dead.len() == 1 { "" } else { "s" },
        globs = dead.join(", "),
        where_ = if root_list.is_empty() {
            "any source root".to_owned()
        } else {
            format!("source root(s) {root_list}")
        },
    ))
}

/// Recursively collects files below `dir` whose root-relative path matches a glob.
///
/// `matched[i]` is set once `patterns[i]` has matched at least one file, which
/// is what [`empty_test_glob_warning`] reports on. Every pattern is tested for
/// each file rather than short-circuiting on the first hit, so the flags stay
/// accurate.
fn collect_matching_test_paths(
    root: &Path,
    dir: &Path,
    patterns: &[String],
    matched: &mut [bool],
    paths: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    for read_result in fs::read_dir(dir)? {
        let dir_entry = read_result?;
        let path = dir_entry.path();
        let file_type = dir_entry.file_type()?;
        if file_type.is_dir() {
            collect_matching_test_paths(root, &path, patterns, matched, paths)?;
        } else if file_type.is_file()
            && let Ok(relative) = path.strip_prefix(root)
        {
            let mut hit = false;
            for (index, pattern) in patterns.iter().enumerate() {
                if path_matches_glob(relative, pattern) {
                    hit = true;
                    if let Some(flag) = matched.get_mut(index) {
                        *flag = true;
                    }
                }
            }
            if hit {
                paths.push(path);
            }
        }
    }
    Ok(())
}

/// Returns whether a relative path matches a small glob syntax.
///
/// Supported syntax is intentionally narrow: `*` matches any characters inside
/// one path segment and `**` matches zero or more path segments. Patterns are
/// evaluated with `/` separators regardless of platform.
pub(crate) fn path_matches_glob(path: &Path, pattern: &str) -> bool {
    let path_segments = path
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    let pattern_segments = pattern.split('/').collect::<Vec<_>>();
    glob_segments_match(&path_segments, &pattern_segments)
}

/// Matches root-relative path segments against parsed glob pattern segments.
fn glob_segments_match(path: &[&str], pattern: &[&str]) -> bool {
    match pattern {
        [] => path.is_empty(),
        ["**", rest @ ..] => {
            glob_segments_match(path, rest)
                || path
                    .split_first()
                    .is_some_and(|(_, remaining)| glob_segments_match(remaining, pattern))
        }
        [head, rest @ ..] => path.split_first().is_some_and(|(segment, remaining)| {
            glob_segment_match(segment, head) && glob_segments_match(remaining, rest)
        }),
    }
}

/// Matches one path segment against `*` wildcards.
fn glob_segment_match(path: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let parts = pattern.split('*').collect::<Vec<_>>();
    if parts.len() == 1 {
        return path == pattern;
    }
    if !pattern.starts_with('*') && parts.first().is_some_and(|part| !path.starts_with(part)) {
        return false;
    }
    if !pattern.ends_with('*') && parts.last().is_some_and(|part| !path.ends_with(part)) {
        return false;
    }
    let mut cursor = 0usize;
    for part in parts.into_iter().filter(|part| !part.is_empty()) {
        let Some(found) = path[cursor..].find(part) else {
            return false;
        };
        cursor = cursor.saturating_add(found).saturating_add(part.len());
    }
    true
}

/// The manifest directory in the form `[sources] exclude` globs are matched
/// against: absolute and canonical when the directory exists.
///
/// An exclude glob is written relative to the MANIFEST, and the paths it is
/// matched against are produced by the module resolver, which canonicalizes
/// them. Making the two comparable is this function's whole job, and it closes
/// two traps that each silently disabled every exclude:
///
/// * `Path::new("Smelt.toml").parent()` is `Some("")`, not `None`, so the usual
///   `.unwrap_or(Path::new("."))` never fired for a manifest named with no
///   directory component — `smelt build` in a project root, or
///   `--manifest-path Smelt.toml`. An empty prefix makes `strip_prefix` succeed
///   while stripping nothing, so a canonical dependency path stayed ABSOLUTE
///   and no manifest-relative glob could match it. Hono then lowered the
///   `src/client/**` its manifest excludes.
/// * A relative directory is relative to the process's working directory, which
///   is not the manifest's directory in general.
///
/// It deliberately does NOT replace the manifest directory used to RESOLVE
/// paths: a root or entry keeps the spelling the manifest gave it, because that
/// spelling becomes the lowered module's path and is visible in HIR dumps, in
/// generated file names and in diagnostics. Only the comparison is normalized.
///
/// Canonicalization is best-effort: a directory that does not exist on disk
/// keeps its literal form rather than failing the build.
pub(crate) fn exclusion_base(manifest_dir: &Path) -> PathBuf {
    let base = if manifest_dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        manifest_dir
    };
    base.canonicalize().unwrap_or_else(|_| base.to_path_buf())
}

/// Strip `base` off `path`, trying the canonical form of each first.
///
/// The two sides reach this from different directions — a manifest root is
/// joined onto the manifest directory as written, while a resolved dependency
/// is canonical — so neither form alone strips both. Returns the path unchanged
/// when it lies outside the manifest directory, which is what
/// `path_matches_glob` then declines to match.
pub(crate) fn manifest_relative_path(path: &Path, base: &Path) -> PathBuf {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    canonical
        .strip_prefix(base)
        .map(Path::to_path_buf)
        .or_else(|_| path.strip_prefix(base).map(Path::to_path_buf))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Resolves a path relative to the manifest directory, or returns it if absolute.
pub(crate) fn resolve_manifest_path(manifest_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        manifest_dir.join(path)
    }
}

/// Expand the requested roots into a sorted, de-duplicated list of `.rs` files.
///
/// Directories are walked recursively. Sorting makes the first-seen example for
/// each shape deterministic across runs and platforms.
pub(crate) fn discover_rust_files(roots: &[PathBuf]) -> CliResult<Vec<PathBuf>> {
    let mut files = Vec::new();
    for root in roots {
        collect_rust_files(root, &mut files)?;
    }
    files.sort();
    files.dedup();
    Ok(files)
}

/// Recursively collect `.rs` files under `root` (or `root` itself when it is a
/// single `.rs` file) into `out`.
fn collect_rust_files(root: &Path, out: &mut Vec<PathBuf>) -> CliResult<()> {
    let metadata = fs::metadata(root)
        .map_err(|error| format!("failed to stat `{}`: {error}", root.display()))?;
    if metadata.is_file() {
        if root.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            out.push(root.to_path_buf());
        }
        return Ok(());
    }
    // Read directory entries in a stable order so the walk is deterministic.
    let mut entries = fs::read_dir(root)
        .map_err(|error| format!("failed to read directory `{}`: {error}", root.display()))?
        .map(|dir_entry| dir_entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    entries.sort();
    for entry in entries {
        // Skip Cargo build output; it duplicates generated sources and would
        // double-count while making the scan non-deterministic across machines.
        if entry.file_name().and_then(|name| name.to_str()) == Some("target") {
            continue;
        }
        collect_rust_files(&entry, out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Unit tests for discovery, glob matching and `[sources] exclude` filtering.

    use super::{
        discover_source_files, empty_test_glob_warning, is_excluded_source, path_matches_glob,
        retain_included_paths,
    };
    use crate::config::Config;
    use std::{
        fs,
        path::{Path, PathBuf},
    };

    /// Discover sources under a manifest whose `[output] target` is `target`,
    /// in a tree holding `files`, as manifest-relative `/`-joined strings.
    fn discovered(target: &str, files: &[&str]) -> Vec<String> {
        let root = tempfile::tempdir().expect("temp dir");
        for file in files {
            let path = root.path().join(file);
            fs::create_dir_all(path.parent().expect("file has a parent")).expect("dir");
            fs::write(path, "").expect("source file");
        }
        let manifest = format!(
            "[project]\nname = \"ex\"\nversion = \"0.1.0\"\n\n\
             [sources]\nentries = [\"main.ts\"]\n\n\
             [output]\ntarget = \"{target}\"\ncrate-name = \"ex\"\n\n\
             [runtime]\nclone-strategy = \"aggressive\"\n"
        );
        let config: Config = toml::from_str(&manifest).expect("manifest");
        discover_source_files(&config, root.path())
            .expect("discovery")
            .iter()
            .map(|path| {
                path.strip_prefix(root.path())
                    .expect("under the manifest")
                    .to_string_lossy()
                    .replace('\\', "/")
                    .trim_start_matches("./")
                    .to_owned()
            })
            .collect()
    }

    /// A directory that only shares a prefix with `dist` holds real sources.
    #[test]
    fn discovery_keeps_directories_that_only_look_like_output() {
        let files = ["distance/haversine.ts", "dist/generated.ts", "main.ts"];
        assert_eq!(discovered("./dist", &files), ["distance/haversine.ts", "main.ts"]);
    }

    /// The skipped directory is the configured target, whatever it is called.
    #[test]
    fn discovery_skips_the_configured_output_target() {
        let files = ["build/rust/generated.ts", "dist/kept.ts", "main.ts"];
        assert_eq!(discovered("build/rust", &files), ["dist/kept.ts", "main.ts"]);
    }

    /// Vendored directories and declaration files stay out regardless.
    #[test]
    fn discovery_skips_vendored_directories_and_declarations() {
        let files = ["node_modules/p/i.ts", "__pycache__/m.py", "types.d.ts", "m.py"];
        assert_eq!(discovered("./dist", &files), ["m.py"]);
    }

    #[test]
    fn glob_double_star_spans_zero_or_more_segments() {
        assert!(path_matches_glob(
            Path::new("object/clone.spec.ts"),
            "object/**/*.spec.ts"
        ));
        assert!(path_matches_glob(
            Path::new("object/nested/deep/clone.spec.ts"),
            "object/**/*.spec.ts"
        ));
        assert!(path_matches_glob(
            Path::new("src/object/clone.spec.ts"),
            "**/*.spec.ts"
        ));
    }

    #[test]
    fn glob_single_star_stays_within_one_segment() {
        assert!(path_matches_glob(Path::new("clone.spec.ts"), "*.spec.ts"));
        // `*` does not cross a `/`, so a nested path needs `**`.
        assert!(!path_matches_glob(
            Path::new("object/clone.spec.ts"),
            "*.spec.ts"
        ));
    }

    #[test]
    fn glob_rejects_non_matching_extensions() {
        assert!(!path_matches_glob(
            Path::new("src/object/clone.ts"),
            "src/object/*.spec.ts"
        ));
    }

    #[test]
    fn exclude_matches_manifest_relative_path() {
        let manifest_dir = Path::new("/repo/es-toolkit");
        let excludes = vec!["src/object/clone.spec.ts".to_owned()];
        assert!(is_excluded_source(
            Path::new("/repo/es-toolkit/src/object/clone.spec.ts"),
            &excludes,
            manifest_dir,
        ));
        assert!(!is_excluded_source(
            Path::new("/repo/es-toolkit/src/object/clone.ts"),
            &excludes,
            manifest_dir,
        ));
    }

    #[test]
    fn exclude_supports_directory_wide_globs() {
        let manifest_dir = Path::new("/repo/es-toolkit");
        let excludes = vec!["src/**/*.spec.ts".to_owned()];
        assert!(is_excluded_source(
            Path::new("/repo/es-toolkit/src/array/chunk.spec.ts"),
            &excludes,
            manifest_dir,
        ));
        assert!(!is_excluded_source(
            Path::new("/repo/es-toolkit/src/array/chunk.ts"),
            &excludes,
            manifest_dir,
        ));
    }

    #[test]
    fn retain_drops_only_excluded_paths() {
        let manifest_dir = Path::new("/repo/es-toolkit");
        let mut paths = vec![
            PathBuf::from("/repo/es-toolkit/src/index.ts"),
            PathBuf::from("/repo/es-toolkit/src/object/clone.spec.ts"),
            PathBuf::from("/repo/es-toolkit/src/object/clone.ts"),
        ];
        let excludes = vec!["src/object/clone.spec.ts".to_owned()];
        retain_included_paths(&mut paths, &excludes, manifest_dir);
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/repo/es-toolkit/src/index.ts"),
                PathBuf::from("/repo/es-toolkit/src/object/clone.ts"),
            ]
        );
    }

    #[test]
    fn retain_with_no_excludes_keeps_everything() {
        let manifest_dir = Path::new("/repo/es-toolkit");
        let mut paths = vec![
            PathBuf::from("/repo/es-toolkit/src/index.ts"),
            PathBuf::from("/repo/es-toolkit/src/object/clone.spec.ts"),
        ];
        let original = paths.clone();
        retain_included_paths(&mut paths, &[], manifest_dir);
        assert_eq!(paths, original);
    }

    #[test]
    fn empty_test_glob_warning_is_silent_when_every_glob_matched() {
        assert!(
            empty_test_glob_warning(
                &["**/*.test.ts".to_owned()],
                &[true],
                &[PathBuf::from("src")],
            )
            .is_none()
        );
    }

    #[test]
    fn empty_test_glob_warning_names_the_dead_glob_and_the_roots() {
        let warning = empty_test_glob_warning(
            &["src/**/*.test.ts".to_owned(), "**/*.spec.ts".to_owned()],
            &[false, true],
            &[PathBuf::from("src")],
        )
        .expect("a glob that matched nothing must warn");
        assert!(warning.contains("`src/**/*.test.ts`"), "{warning}");
        assert!(!warning.contains("`**/*.spec.ts`"), "{warning}");
        assert!(warning.contains("src"), "{warning}");
        assert!(warning.starts_with("warning:"), "{warning}");
    }

    #[test]
    fn empty_test_glob_warning_lists_every_dead_glob() {
        let warning = empty_test_glob_warning(
            &["a/**/*.test.ts".to_owned(), "b/**/*.test.ts".to_owned()],
            &[false, false],
            &[PathBuf::from("src"), PathBuf::from("lib")],
        )
        .expect("two dead globs must warn");
        assert!(warning.contains("globs"), "{warning}");
        assert!(warning.contains("`a/**/*.test.ts`"), "{warning}");
        assert!(warning.contains("`b/**/*.test.ts`"), "{warning}");
        assert!(warning.contains("src, lib"), "{warning}");
    }
}
