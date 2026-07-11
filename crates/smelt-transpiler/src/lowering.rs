//! Frontend lowering helpers for single files and mixed-language manifests.
//!
//! This module owns source-language detection plus HIR lowering entrypoints
//! used by CLI commands and manifest-driven builds.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use smelt_hir::{FileId, ModuleId};
use smelt_stdlib::DiagnosticCategory;

use crate::manifest::{
    ManifestSource, dependency_closure, order_manifest_sources, read_manifest_source,
    resolve_manifest_path,
};
use crate::timing;

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
            Some("ts") => Ok(Self::TypeScript),
            Some("py") => Ok(Self::Python),
            _ => Err(format!("unsupported source extension: {path}").into()),
        }
    }

    /// Returns whether this path kind is lowered by the TypeScript frontend.
    pub(crate) const fn is_typescript(self) -> bool {
        matches!(self, Self::TypeScript | Self::TypeScriptDeclaration)
    }
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

/// Represents a lowered crate with its modules.
pub(crate) type LoweredCrate = (smelt_hir::Crate, Vec<(String, ModuleId)>);

/// Cross-file frontend metadata that is not represented directly in the shared HIR crate.
#[derive(Default)]
struct FrontendLoweringState {
    /// TypeScript re-export aliases visible to later manifest entries.
    ts_export_aliases: HashMap<String, smelt_hir::ItemId>,
    /// TypeScript exports grouped by source module path.
    ts_module_exports: HashMap<String, HashMap<String, smelt_hir::ItemId>>,
    /// TypeScript exported object constants used as namespace-like APIs.
    ts_object_namespaces: HashMap<String, HashMap<String, smelt_hir::ItemId>>,
    /// TypeScript exported object constants with static literal values.
    ts_object_consts: HashMap<String, smelt_frontend_ts::ObjectConst>,
    /// TypeScript exported object constants whose values can be projected.
    ts_object_value_collections: HashMap<String, smelt_frontend_ts::ConstCollection>,
    /// TypeScript exported array/set constants visible across manifest entries.
    ts_const_collections: HashMap<String, smelt_frontend_ts::ConstCollection>,
    /// TypeScript const-folded `enum` member values visible across manifest entries.
    ts_enum_members: HashMap<String, HashMap<String, smelt_frontend_ts::ConstLiteral>>,
    /// TypeScript overload signatures visible across manifest entries.
    ts_overloads: HashMap<String, Vec<smelt_frontend_ts::OverloadSignature>>,
    /// TypeScript rest-parameter metadata visible across manifest entries.
    ts_function_rests: HashMap<String, smelt_frontend_ts::RestParam>,
    /// TypeScript functions whose source return contract is a JavaScript `Date`.
    ts_date_returning_functions: std::collections::HashSet<smelt_hir::ItemId>,
    /// TypeScript structural type-alias fields visible across manifest entries.
    ts_type_alias_fields: HashMap<smelt_hir::Symbol, Vec<smelt_hir::Field>>,
    /// TypeScript interface heritage edges visible across manifest entries.
    ts_interface_extends: HashMap<smelt_hir::Symbol, Vec<smelt_frontend_ts::InterfaceHeritageRef>>,
    /// TypeScript interface string index signature value types visible across manifest entries.
    ts_interface_index_values: HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    /// TypeScript class string index signature value types visible across manifest entries.
    ts_class_index_values: HashMap<smelt_hir::Symbol, smelt_hir::TypeId>,
    /// TypeScript interface call signatures visible across manifest entries.
    ts_interface_call_signatures: HashMap<smelt_hir::Symbol, Vec<smelt_hir::FunctionType>>,
    /// TypeScript interface construct signatures visible across manifest entries.
    ts_interface_construct_signatures: HashMap<smelt_hir::Symbol, Vec<smelt_hir::FunctionType>>,
    /// TypeScript callable intersection fields visible across manifest entries.
    ts_callable_fields: HashMap<smelt_hir::TypeId, Vec<smelt_hir::Field>>,
    /// TypeScript aliases whose source surface is a callable object intersection.
    ts_callable_object_aliases: std::collections::HashSet<smelt_hir::Symbol>,
    /// Modeled host constructor names reassigned via `globalThis.<Name> =`
    /// anywhere in the crate (the crate-level host-global override pre-pass).
    ///
    /// Seeded once before lowering begins by scanning every source, so a write
    /// in a spec file activates the dynamic override machinery in the predicate
    /// module even though that module lowers first.
    ts_written_host_globals: std::collections::HashSet<String>,
    /// Python module/package namespaces visible through `import package`.
    py_module_namespaces: HashMap<String, HashMap<String, smelt_hir::ItemId>>,
    /// Python `IntEnum` member values visible to later manifest entries.
    py_enum_members: HashMap<String, HashMap<String, i64>>,
}

/// Lowers TypeScript source files to HIR.
pub(crate) fn lower_typescript_files(
    files: &[String],
) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let mut ctx = smelt_frontend_ts::HirCtx::new();
    let mut modules = Vec::new();

    // Crate-level host-global override pre-pass: scan every source for
    // `globalThis.<Name> =` writes before any file lowers.
    for file in files {
        if let Ok(source) = fs::read_to_string(file) {
            ctx.written_host_globals
                .extend(smelt_frontend_ts::scan_written_host_globals(&source, file));
        }
    }

    for (idx, file) in files.iter().enumerate() {
        let source = fs::read_to_string(file)?;
        let file_id = u32::try_from(idx).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("too many source files: {error}"),
            )
        })?;
        let module = smelt_frontend_ts::to_hir_with_path(&source, FileId(file_id), file, &mut ctx)
            .map_err(|errors| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("{}:\n{errors:#?}", Path::new(file).display()),
                )
            })?;
        modules.push((file.clone(), module));
    }

    Ok((ctx.krate, modules))
}

/// Dispatches one source file to the matching frontend.
///
/// TypeScript targets are lowered together with their local relative-import
/// dependencies (see [`lower_typescript_file_with_dependencies`]) so cross-file
/// constants, classes, and interfaces resolve exactly as they do in a full
/// manifest build. This keeps `dump-hir`/`dump-mir` from reporting isolation-only
/// blockers — e.g. a `switch case <importedTag>:` label whose value lives in a
/// sibling module — while still focusing the printed output on the target file.
pub(crate) fn lower_single_file(file: &str) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    match SourceLang::from_path(file)? {
        SourceLang::TypeScript | SourceLang::TypeScriptDeclaration => {
            lower_typescript_file_with_dependencies(file)
        }
        SourceLang::Python | SourceLang::PythonDeclaration => {
            crate::python::lower_files(&[file.to_owned()])
        }
    }
}

/// Lower a single TypeScript file after first lowering its local dependencies.
///
/// Single-file commands (`dump-hir`, `dump-mir`) historically lowered the target
/// in isolation, so any value imported from a sibling module — a folded string
/// tag used as a `switch` case label, an exported base class, an interface — was
/// unresolvable and surfaced as a spurious lowering blocker. This resolves the
/// target's relative-import closure (reusing the same manifest graph helpers a
/// real build uses), lowers every dependency in dependency-first order into one
/// shared [`smelt_frontend_ts::HirCtx`], and lowers the target last so its
/// imports fold against the already-lowered modules.
///
/// Dependencies are lowered best-effort: a dependency that itself fails to lower
/// (for example because of an unrelated unsupported feature) is skipped rather
/// than aborting, since the target only needs the exported metadata a partially
/// lowered dependency still records. Only the target file's own lowering errors
/// are reported, and only the target module is returned so the printed HIR/MIR
/// stays focused on the requested file while the shared crate still carries the
/// dependency items the target references.
fn lower_typescript_file_with_dependencies(
    file: &str,
) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let target_path = PathBuf::from(file);
    let target_key = target_path.canonicalize().unwrap_or(target_path.clone());

    // Build the dependency-first order of the target and its local imports. Any
    // failure to scan/resolve dependencies degrades gracefully to lowering the
    // target alone, preserving the previous isolated behavior for odd inputs.
    let ordered_paths = ordered_dependency_paths(target_path.clone()).unwrap_or_default();

    let mut ctx = smelt_frontend_ts::HirCtx::new();
    let mut target_module: Option<(String, ModuleId)> = None;

    // Crate-level host-global override pre-pass over the target's dependency
    // closure, so single-file `dump-hir`/`dump-mir` see the same written-name
    // set a full build does.
    for path in &ordered_paths {
        let path_string = path.display().to_string();
        if SourceLang::from_path(&path_string).is_ok_and(SourceLang::is_typescript)
            && let Ok(source) = fs::read_to_string(path)
        {
            ctx.written_host_globals.extend(
                smelt_frontend_ts::scan_written_host_globals(&source, &path_string),
            );
        }
    }

    for (idx, path) in ordered_paths.iter().enumerate() {
        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        let path_string = path.display().to_string();
        // A dependency file that is not TypeScript (a Python sibling reached
        // through a mixed graph) is not lowered by this frontend path.
        if !SourceLang::from_path(&path_string).is_ok_and(SourceLang::is_typescript) {
            continue;
        }
        let file_id = FileId(u32::try_from(idx).unwrap_or(u32::MAX));
        let is_target = path.canonicalize().unwrap_or_else(|_| path.clone()) == target_key
            || path == &target_path;
        let outcome =
            smelt_frontend_ts::to_hir_with_path(&source, file_id, &path_string, &mut ctx);
        match outcome {
            Ok(module) => {
                if is_target {
                    target_module = Some((file.to_owned(), module));
                }
            }
            Err(errors) => {
                // Surface the target's own errors; a dependency's unrelated
                // failure is swallowed so it cannot mask the target result.
                if is_target {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("{}:\n{errors:#?}", target_path.display()),
                    )
                    .into());
                }
            }
        }
    }

    // Fall back to lowering the target directly when the dependency walk never
    // produced it (empty order, canonicalization mismatch, or a non-TypeScript
    // classification of the target itself).
    if let Some(module) = target_module {
        Ok((ctx.krate, vec![module]))
    } else {
        lower_typescript_files(&[file.to_owned()])
    }
}

/// Return the target file and its local dependency closure in dependency-first
/// order (dependencies before importers, target last).
///
/// Reuses the manifest module-graph helpers so single-file lowering resolves the
/// same relative imports a full build does. Returns `None` if the target cannot
/// be read or its import graph cannot be ordered, letting the caller fall back to
/// isolated lowering.
fn ordered_dependency_paths(target: PathBuf) -> Option<Vec<PathBuf>> {
    let root = read_manifest_source(target).ok()?;
    let sources = dependency_closure(vec![root]).ok()?;
    let ordered = order_manifest_sources(&sources).ok()?;
    Some(
        ordered
            .into_iter()
            .filter_map(|idx| sources.get(idx).map(|source| source.path.clone()))
            .collect(),
    )
}

/// One categorized frontend diagnostic from lowering a single file.
///
/// Flattens the per-frontend `SmeltError` types into a shared shape so probe
/// reporting can group diagnostics without depending on either frontend's
/// concrete error type.
#[derive(Debug, Clone)]
pub(crate) struct FileDiagnostic {
    /// Coarse classification used for grouping.
    pub(crate) category: DiagnosticCategory,
    /// Stable per-site diagnostic code.
    pub(crate) code: &'static str,
    /// Human-readable message.
    pub(crate) message: String,
}

/// Lower one source file in isolation and return its categorized diagnostics.
///
/// Single-file lowering does not resolve cross-file imports, so unresolved
/// references to other project modules surface here; probe reporting accounts
/// for that by reading each diagnostic's [`DiagnosticCategory`]. Returns an
/// empty vector when the file lowers cleanly.
pub(crate) fn collect_file_diagnostics(
    file: &str,
) -> Result<Vec<FileDiagnostic>, Box<dyn std::error::Error>> {
    let source = fs::read_to_string(file)?;
    let lang = SourceLang::from_path(file)?;
    // A frontend panic on one file must not abort the whole probe; treat it as
    // its own reportable blocker. `catch_unwind` is safe here because the HIR
    // context is created and dropped entirely inside the closure.
    let lowered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match lang {
        SourceLang::TypeScript | SourceLang::TypeScriptDeclaration => {
            let mut ctx = smelt_frontend_ts::HirCtx::new();
            smelt_frontend_ts::to_hir_with_path(&source, FileId(0), file, &mut ctx)
                .err()
                .map(|errors| {
                    errors
                        .into_iter()
                        .map(|error| FileDiagnostic {
                            category: error.category,
                            code: error.code,
                            message: error.message,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        }
        SourceLang::Python | SourceLang::PythonDeclaration => {
            crate::python::diagnostics_for(&source, file)
        }
    }));
    Ok(lowered.unwrap_or_else(|_| {
        vec![FileDiagnostic {
            category: DiagnosticCategory::Internal,
            code: "smelt::frontend-panic",
            message: "frontend panicked while lowering this file".to_owned(),
        }]
    }))
}

/// One categorized diagnostic from a whole-crate recoverable lowering, tagged
/// with the file it came from. Serializes directly for `--message-format json`.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ManifestDiagnostic {
    /// Source file the diagnostic refers to.
    pub(crate) file: String,
    /// Coarse classification used for grouping.
    pub(crate) category: DiagnosticCategory,
    /// Stable per-site diagnostic code.
    pub(crate) code: &'static str,
    /// Human-readable message.
    pub(crate) message: String,
}

/// RAII guard that silences panic backtraces and restores the prior hook on drop.
///
/// Used by recoverable/probing lowering where individual-file panics are caught
/// and reported rather than aborting the process.
pub(crate) struct QuietPanics {
    /// The panic hook to restore on drop.
    previous: Option<Box<dyn Fn(&std::panic::PanicHookInfo<'_>) + Sync + Send + 'static>>,
}

impl QuietPanics {
    /// Install a no-op panic hook, remembering the previous one.
    pub(crate) fn install() -> Self {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_info| {}));
        Self {
            previous: Some(previous),
        }
    }
}

impl Drop for QuietPanics {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            std::panic::set_hook(previous);
        }
    }
}

/// Recursively collect every TypeScript/Python source file under the manifest
/// source roots, skipping declaration files and vendored/output directories.
///
/// Files whose manifest-relative path matches a `[sources] exclude` glob are
/// dropped so probe discovery honors the same exclusions as build discovery.
pub(crate) fn discover_source_files(
    config: &crate::config::Config,
    manifest_dir: &Path,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let roots = config
        .source_roots()
        .map_or_else(|| vec![PathBuf::from(".")], <[_]>::to_vec);
    let mut paths = Vec::new();
    for root in roots {
        let absolute_root = resolve_manifest_path(manifest_dir, &root);
        collect_source_files(&absolute_root, &mut paths)?;
    }
    retain_included_paths(&mut paths, config.source_excludes(), manifest_dir);
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Recursively gather probe-eligible source files below `dir`.
fn collect_source_files(
    dir: &Path,
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
            if let Some(name) = path.file_name().and_then(|name| name.to_str())
                && (name == "node_modules" || name == "__pycache__" || name.starts_with("dist"))
            {
                continue;
            }
            collect_source_files(&path, paths)?;
        } else if file_type.is_file() && is_probe_source_file(&path) {
            paths.push(path);
        }
    }
    Ok(())
}

/// Returns whether `path` is a `.ts`/`.py` source file the probe should lower.
fn is_probe_source_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name.ends_with(".d.ts") {
        return false;
    }
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("ts" | "py")
    )
}

/// Lowers manifest entries in dependency order using one shared HIR crate.
pub(crate) fn lower_manifest_entries(
    config: &crate::config::Config,
    manifest_path: &Path,
) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let root_sources = timing::measure("manifest.read_roots", || {
        manifest_root_paths(config, manifest_dir)?
            .into_iter()
            .map(read_manifest_source)
            .collect::<Result<Vec<_>, _>>()
    })?;
    let sources = timing::measure("manifest.dependency_closure", || {
        dependency_closure(root_sources)
    })?;

    let ordered_sources = timing::measure("manifest.order_sources", || {
        order_manifest_sources(&sources)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))
            .map(|ordered| {
                ordered
                    .into_iter()
                    .filter_map(|idx| sources.get(idx))
                    .collect::<Vec<_>>()
            })
    })?;

    if ordered_sources.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "manifest has no source entries to lower",
        )
        .into());
    }

    let specialization = timing::measure("manifest.specialize", || {
        crate::specialization::prepare(config, manifest_path, &ordered_sources)
    })?;
    timing::measure("manifest.frontend_lower", || {
        lower_ordered_manifest_sources(&ordered_sources, &specialization)
    })
}

/// Returns explicitly listed entries plus test files discovered by glob.
///
/// Any root source whose manifest-relative path matches a `[sources] exclude`
/// glob is removed before dependency resolution. Because excluded specs are
/// leaf test files that nothing imports, dropping them from the root set keeps
/// them out of the whole-crate build entirely.
fn manifest_root_paths(
    config: &crate::config::Config,
    manifest_dir: &Path,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut paths = config
        .entries()
        .iter()
        .map(|path| resolve_manifest_path(manifest_dir, path))
        .collect::<Vec<_>>();
    paths.extend(discover_test_paths(config, manifest_dir)?);
    retain_included_paths(&mut paths, config.source_excludes(), manifest_dir);
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Drops paths whose manifest-relative form matches any `exclude` glob.
///
/// Exclusion patterns are matched against each path relative to the manifest
/// directory (for example `src/object/clone.spec.ts`), using the same narrow
/// glob syntax as [`path_matches_glob`]. Paths outside `manifest_dir` and paths
/// with no exclude patterns are always retained.
fn retain_included_paths(paths: &mut Vec<PathBuf>, excludes: &[String], manifest_dir: &Path) {
    if excludes.is_empty() {
        return;
    }
    paths.retain(|path| !is_excluded_source(path, excludes, manifest_dir));
}

/// Returns whether `path` matches any `exclude` glob relative to `manifest_dir`.
fn is_excluded_source(path: &Path, excludes: &[String], manifest_dir: &Path) -> bool {
    let relative = path.strip_prefix(manifest_dir).unwrap_or(path);
    excludes
        .iter()
        .any(|pattern| path_matches_glob(relative, pattern))
}

/// Discovers test files matching configured source-root-relative glob patterns.
fn discover_test_paths(
    config: &crate::config::Config,
    manifest_dir: &Path,
) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    if config.test_globs().is_empty() {
        return Ok(Vec::new());
    }
    let roots = config
        .source_roots()
        .map_or_else(|| vec![PathBuf::from(".")], <[_]>::to_vec);
    let mut paths = Vec::new();
    for root in roots {
        let absolute_root = resolve_manifest_path(manifest_dir, &root);
        collect_matching_test_paths(
            &absolute_root,
            &absolute_root,
            config.test_globs(),
            &mut paths,
        )?;
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

/// Recursively collects files below `dir` whose root-relative path matches a glob.
fn collect_matching_test_paths(
    root: &Path,
    dir: &Path,
    patterns: &[String],
    paths: &mut Vec<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    for read_result in fs::read_dir(dir)? {
        let dir_entry = read_result?;
        let path = dir_entry.path();
        let file_type = dir_entry.file_type()?;
        if file_type.is_dir() {
            collect_matching_test_paths(root, &path, patterns, paths)?;
        } else if file_type.is_file()
            && let Ok(relative) = path.strip_prefix(root)
            && patterns
                .iter()
                .any(|pattern| path_matches_glob(relative, pattern))
        {
            paths.push(path);
        }
    }
    Ok(())
}

/// Returns whether a relative path matches a small glob syntax.
///
/// Supported syntax is intentionally narrow: `*` matches any characters inside
/// one path segment and `**` matches zero or more path segments. Patterns are
/// evaluated with `/` separators regardless of platform.
fn path_matches_glob(path: &Path, pattern: &str) -> bool {
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

/// Lowers already ordered manifest files into one shared HIR crate.
/// Seed the crate-level host-global override set from every TypeScript source.
///
/// The scan runs once before per-file lowering so a `globalThis.<Name> =` write
/// in any module (typically a spec file) activates the dynamic override
/// machinery — presence guards, `new` dispatch, reads — in the module that
/// defines the predicate, even though that module lowers first.
fn seed_written_host_globals(sources: &[&ManifestSource], state: &mut FrontendLoweringState) {
    for source in sources {
        let path = source.path.display().to_string();
        if SourceLang::from_path(&path).is_ok_and(SourceLang::is_typescript) {
            state.ts_written_host_globals.extend(
                smelt_frontend_ts::scan_written_host_globals(&source.source, &path),
            );
        }
    }
}

/// Lowers the manifest sources, in manifest order, into one HIR crate, seeding
/// host-global write scanning and applying prepared specialization results.
fn lower_ordered_manifest_sources(
    sources: &[&ManifestSource],
    specialization: &crate::specialization::PreparedSpecialization,
) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let mut krate = smelt_hir::Crate::new();
    let mut state = FrontendLoweringState::default();
    seed_written_host_globals(sources, &mut state);
    let mut modules = Vec::new();
    let module_names = manifest_module_names(sources);

    for (idx, source) in sources.iter().enumerate() {
        let (next_krate, next_state, outcome) =
            lower_manifest_source(krate, state, source, idx, specialization)?;
        krate = next_krate;
        state = next_state;
        let module = outcome.map_err(|diagnostics| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}:\n{diagnostics:#?}", source.path.display()),
            )
        })?;
        let file = &source.path;
        if let Ok(module_idx) = usize::try_from(module.0)
            && let Some(module_value) = krate.modules.get_mut(module_idx)
        {
            module_value.name = module_names
                .get(idx)
                .cloned()
                .unwrap_or_else(|| manifest_module_name(file));
        }
        modules.push((file.display().to_string(), module));
    }

    Ok((krate, modules))
}

/// Lower all manifest sources in dependency order, recovering from per-file
/// lowering failures so one pass enumerates blockers across the whole crate.
///
/// Unlike [`lower_manifest_entries`], a file that fails to lower does not abort
/// the run: its categorized diagnostics are collected and lowering continues
/// with the remaining modules (imports into already-lowered modules still
/// resolve). A frontend panic on a file is reported as an `internal` diagnostic
/// and stops the pass, since the shared crate cannot be recovered past a panic.
pub(crate) fn collect_manifest_diagnostics(
    config: &crate::config::Config,
    manifest_path: &Path,
) -> Result<Vec<ManifestDiagnostic>, Box<dyn std::error::Error>> {
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let root_sources = manifest_root_paths(config, manifest_dir)?
        .into_iter()
        .map(read_manifest_source)
        .collect::<Result<Vec<_>, _>>()?;
    let sources = dependency_closure(root_sources)?;
    let ordered_sources = order_manifest_sources(&sources)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
        .into_iter()
        .filter_map(|idx| sources.get(idx))
        .collect::<Vec<_>>();
    let specialization = crate::specialization::prepare(config, manifest_path, &ordered_sources)?;

    let _quiet = QuietPanics::install();
    let mut krate = smelt_hir::Crate::new();
    let mut state = FrontendLoweringState::default();
    seed_written_host_globals(&ordered_sources, &mut state);
    let mut diagnostics = Vec::new();
    for (idx, source) in ordered_sources.iter().enumerate() {
        let lowered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            lower_manifest_source(krate, state, source, idx, &specialization)
        }));
        match lowered {
            Ok(Ok((next_krate, next_state, outcome))) => {
                krate = next_krate;
                state = next_state;
                if let Err(file_diagnostics) = outcome {
                    diagnostics.extend(file_diagnostics);
                }
            }
            Ok(Err(hard_error)) => return Err(hard_error),
            Err(_panic) => {
                diagnostics.push(ManifestDiagnostic {
                    file: source.path.display().to_string(),
                    category: DiagnosticCategory::Internal,
                    code: "smelt::frontend-panic",
                    message: "frontend panicked while lowering this file; \
                              remaining modules were not scanned"
                        .to_owned(),
                });
                break;
            }
        }
    }
    Ok(diagnostics)
}

/// Lowers one manifest file, returning the (possibly mutated) crate and state
/// plus either the lowered module or its categorized diagnostics.
///
/// The crate and state are returned even when the file has lowering errors, so
/// a recoverable driver can keep lowering the remaining modules with the
/// already-lowered modules still resolvable. The outer error is reserved for
/// unrecoverable failures such as an unknown source extension.
fn lower_manifest_source(
    krate: smelt_hir::Crate,
    state: FrontendLoweringState,
    source: &ManifestSource,
    idx: usize,
    specialization: &crate::specialization::PreparedSpecialization,
) -> Result<
    (
        smelt_hir::Crate,
        FrontendLoweringState,
        Result<ModuleId, Vec<ManifestDiagnostic>>,
    ),
    Box<dyn std::error::Error>,
> {
    let file = &source.path;
    let file_string = file.display().to_string();
    let file_id = FileId(u32::try_from(idx).unwrap_or(u32::MAX));
    match SourceLang::from_path(&file_string)? {
        SourceLang::TypeScript | SourceLang::TypeScriptDeclaration => {
            let mut ctx = smelt_frontend_ts::HirCtx {
                krate,
                export_aliases: state.ts_export_aliases,
                module_exports: state.ts_module_exports,
                object_namespaces: state.ts_object_namespaces,
                object_consts: state.ts_object_consts,
                object_value_collections: state.ts_object_value_collections,
                const_collections: state.ts_const_collections,
                enum_members: state.ts_enum_members,
                overloads: state.ts_overloads,
                function_rests: state.ts_function_rests,
                date_returning_functions: state.ts_date_returning_functions,
                type_alias_fields: state.ts_type_alias_fields,
                interface_extends: state.ts_interface_extends,
                interface_index_values: state.ts_interface_index_values,
                class_index_values: state.ts_class_index_values,
                interface_call_signatures: state.ts_interface_call_signatures,
                interface_construct_signatures: state.ts_interface_construct_signatures,
                callable_fields: state.ts_callable_fields,
                callable_object_aliases: state.ts_callable_object_aliases,
                written_host_globals: state.ts_written_host_globals,
            };
            let outcome = smelt_frontend_ts::to_hir_with_options(
                &source.source,
                file_id,
                &file_string,
                &mut ctx,
                smelt_frontend_ts::FrontendOptions {
                    specialization: specialization.typescript.as_ref(),
                },
            )
            .map_err(|errors| {
                manifest_diagnostics(
                    &file_string,
                    errors
                        .iter()
                        .map(|e| (e.category, e.code, e.message.clone())),
                )
            });
            let next_state = FrontendLoweringState {
                ts_export_aliases: ctx.export_aliases,
                ts_module_exports: ctx.module_exports,
                ts_object_namespaces: ctx.object_namespaces,
                ts_object_consts: ctx.object_consts,
                ts_object_value_collections: ctx.object_value_collections,
                ts_const_collections: ctx.const_collections,
                ts_enum_members: ctx.enum_members,
                ts_overloads: ctx.overloads,
                ts_function_rests: ctx.function_rests,
                ts_date_returning_functions: ctx.date_returning_functions,
                ts_type_alias_fields: ctx.type_alias_fields,
                ts_interface_extends: ctx.interface_extends,
                ts_interface_index_values: ctx.interface_index_values,
                ts_class_index_values: ctx.class_index_values,
                ts_interface_call_signatures: ctx.interface_call_signatures,
                ts_interface_construct_signatures: ctx.interface_construct_signatures,
                ts_callable_fields: ctx.callable_fields,
                ts_callable_object_aliases: ctx.callable_object_aliases,
                ts_written_host_globals: ctx.written_host_globals,
                py_module_namespaces: state.py_module_namespaces,
                py_enum_members: state.py_enum_members,
            };
            Ok((ctx.krate, next_state, outcome))
        }
        SourceLang::Python | SourceLang::PythonDeclaration => {
            let (next_krate, py_module_namespaces, py_enum_members, outcome) =
                crate::python::lower_manifest_source(
                    krate,
                    state.py_module_namespaces,
                    state.py_enum_members,
                    &source.source,
                    file_id,
                    &file_string,
                    specialization.python.as_ref(),
                );
            let next_state = FrontendLoweringState {
                ts_export_aliases: state.ts_export_aliases,
                ts_module_exports: state.ts_module_exports,
                ts_object_namespaces: state.ts_object_namespaces,
                ts_object_consts: state.ts_object_consts,
                ts_object_value_collections: state.ts_object_value_collections,
                ts_const_collections: state.ts_const_collections,
                ts_enum_members: state.ts_enum_members,
                ts_overloads: state.ts_overloads,
                ts_function_rests: state.ts_function_rests,
                ts_date_returning_functions: state.ts_date_returning_functions,
                ts_type_alias_fields: state.ts_type_alias_fields,
                ts_interface_extends: state.ts_interface_extends,
                ts_interface_index_values: state.ts_interface_index_values,
                ts_class_index_values: state.ts_class_index_values,
                ts_interface_call_signatures: state.ts_interface_call_signatures,
                ts_interface_construct_signatures: state.ts_interface_construct_signatures,
                ts_callable_fields: state.ts_callable_fields,
                ts_callable_object_aliases: state.ts_callable_object_aliases,
                ts_written_host_globals: state.ts_written_host_globals,
                py_module_namespaces,
                py_enum_members,
            };
            Ok((next_krate, next_state, outcome))
        }
    }
}

/// Build per-file [`ManifestDiagnostic`]s from a frontend's categorized errors.
pub(crate) fn manifest_diagnostics(
    file: &str,
    errors: impl Iterator<Item = (DiagnosticCategory, &'static str, String)>,
) -> Vec<ManifestDiagnostic> {
    errors
        .map(|(category, code, message)| ManifestDiagnostic {
            file: file.to_owned(),
            category,
            code,
            message,
        })
        .collect()
}

/// Returns the generated Rust function name for a manifest module body.
fn manifest_module_name(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("main");
    if stem != "index" {
        return stem.to_owned();
    }
    path.parent()
        .and_then(Path::file_name)
        .and_then(|parent| parent.to_str())
        .map_or_else(|| stem.to_owned(), |parent| format!("{parent}_{stem}"))
}

/// Returns collision-free Rust function names for manifest module bodies.
///
/// Dependency sorting can place two files with the same stem in one HIR crate,
/// such as `src/main.py` importing `src/lib/main.ts`. Both frontends lower
/// module-level statements as a module body, and MIR lowering uses the module
/// name as the emitted Rust function name. This helper keeps the final
/// colliding module body under the plain name so an entry file named `main`
/// still emits the executable `fn main()`, while earlier collisions receive a
/// stable ordinal suffix.
fn manifest_module_names(sources: &[&ManifestSource]) -> Vec<String> {
    let bases = sources
        .iter()
        .map(|source| manifest_module_name(&source.path))
        .collect::<Vec<_>>();
    let mut totals = HashMap::<String, usize>::new();
    for base in &bases {
        let total = totals.entry(base.clone()).or_insert(0);
        *total = total.saturating_add(1);
    }
    let mut seen = HashMap::<String, usize>::new();
    bases
        .into_iter()
        .map(|base| {
            let total = totals.get(&base).copied().unwrap_or(1);
            if total == 1 {
                return base;
            }
            let ordinal = seen.entry(base.clone()).or_insert(0);
            *ordinal = ordinal.saturating_add(1);
            if *ordinal == total {
                base
            } else {
                format!("{base}_{ordinal}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    //! Unit tests for source-file glob matching and `[sources] exclude` filtering.

    use super::{is_excluded_source, path_matches_glob, retain_included_paths};
    use std::path::{Path, PathBuf};

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
}
