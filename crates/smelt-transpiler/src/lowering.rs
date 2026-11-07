//! The one lowering driver, for manifests and for single files.
//!
//! [`ManifestLowering`] resolves what a run lowers: the dependency closure, read
//! once, in dependency order, with its module names and file ids. `build`,
//! `check`, `probe`, `dump-hir`, `dump-mir` and every report go through it, so a
//! program lowers the same way whichever command asked. A single file is a
//! project of one entry: it has no manifest, so no excludes, packages, typed
//! versions or specialization, and its target is named `main` as a project's
//! `src/main.ts` is.

use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;
use smelt_hir::{FileId, ModuleId};
use smelt_stdlib::DiagnosticCategory;

use crate::manifest::{
    ManifestSource, dependency_closure, order_manifest_sources, read_manifest_source,
};
use crate::config::Config;
use crate::sound::SoundSettings;
use crate::source_files::{
    SourceLang, discover_source_files, discover_test_paths, resolve_manifest_path,
    retain_included_paths,
};
use crate::specialization::PreparedSpecialization;
use crate::timing;
use crate::ts_facts::TsCheck;

mod driver;

use driver::{ManifestSeed, ModuleInputs, PredeclaredLowering, RecoverableLowering, SeededLowering};

/// Represents a lowered crate with its modules.
pub(crate) type LoweredCrate = (smelt_hir::Crate, Vec<(String, ModuleId)>);

/// One lowering run, resolved and ready; see the module docs.
pub(crate) struct ManifestLowering {
    /// Every source of the run, in dependency order.
    sources: Vec<ManifestSource>,
    /// Frontend file id of each source.
    file_ids: Vec<FileId>,
    /// Rust-facing module body name of each source.
    module_names: Vec<String>,
    /// Materialized specialization data.
    specialization: PreparedSpecialization,
    /// Project sources the dependency closure did not reach.
    outside_crate: HashSet<String>,
    /// Directory paths in erasure records are relative to.
    project_dir: PathBuf,
    /// The source a single-file command prints, by position in `sources`.
    focus: Option<usize>,
}

impl ManifestLowering {
    /// Resolves the entries of the manifest at `manifest_path`.
    pub(crate) fn for_manifest(
        config: &Config,
        manifest_path: &Path,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
        let root_sources = timing::measure("manifest.read_roots", || {
            manifest_root_paths(config, manifest_dir)?
                .into_iter()
                .map(read_manifest_source)
                .collect::<Result<Vec<_>, _>>()
        })?;
        let sources = timing::measure("manifest.dependency_closure", || {
            let packages = crate::package_sources::resolve_packages(config, manifest_dir)?;
            dependency_closure(
                root_sources,
                config.source_excludes(),
                manifest_dir,
                &packages,
                Some(sound_settings(config, manifest_dir)),
            )
        })?;
        // Ordering cannot fail (import cycles are cut deliberately).
        let Ok(sources) = timing::measure("manifest.order_sources", || {
            Ok::<_, std::convert::Infallible>(in_dependency_order(sources))
        });
        if sources.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "manifest has no source entries to lower",
            )
            .into());
        }
        let ordered = sources.iter().collect::<Vec<_>>();
        let specialization = timing::measure("manifest.specialize", || {
            crate::specialization::prepare(config, manifest_path, &ordered)
        })?;
        let outside_crate = sources_outside_crate(config, manifest_dir, &ordered);
        let module_names = manifest_module_names(&ordered, |source| manifest_module_name(&source.path));
        let file_ids = file_ids(sources.len())?;
        Ok(Self {
            sources,
            file_ids,
            module_names,
            specialization,
            outside_crate,
            project_dir: manifest_dir.to_path_buf(),
            focus: None,
        })
    }

    /// Resolves `file` and its local imports.
    ///
    /// Only the target module is reported as lowered, so printed HIR stays on the
    /// requested file, while the crate carries every dependency it references.
    pub(crate) fn for_file(file: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let target = PathBuf::from(file);
        let root = read_manifest_source(target.clone())?;
        let sources = in_dependency_order(dependency_closure(vec![root], &[], Path::new("."), &[], None)?);
        let focus = sources.iter().position(|source| source.path == target);
        let ordered = sources.iter().collect::<Vec<_>>();
        let module_names = manifest_module_names(&ordered, |source| {
            if source.path == target {
                ENTRY_MODULE_NAME.to_owned()
            } else {
                manifest_module_name(&source.path)
            }
        });
        let file_ids = file_ids(sources.len())?;
        Ok(Self {
            sources,
            file_ids,
            module_names,
            specialization: PreparedSpecialization::none(),
            outside_crate: HashSet::new(),
            project_dir: PathBuf::from("."),
            focus,
        })
    }

    /// Runs what every lowering does first, in the order it must happen: the
    /// checker over all sources, then the seed scan, then the predeclaration
    /// pass, so no module body lowers before the crate-wide facts exist.
    fn prepare(&self) -> Result<Prepared<'_>, Box<dyn std::error::Error>> {
        let ordered = self.sources.iter().collect::<Vec<_>>();
        let ts_check = crate::ts_facts::analyze_sources(&ordered);
        ts_check.report_failure();
        let seed = ManifestSeed::scan(&ordered, &self.module_names, self.outside_crate.clone());
        let lowering = SeededLowering::new(seed).predeclare(&ordered, &self.file_ids)?;
        Ok(Prepared { ordered, ts_check, lowering })
    }

    /// The whole-crate inputs of this run's module lowerings.
    fn module_inputs<'run>(&'run self, ts_check: &'run TsCheck) -> ModuleInputs<'run> {
        ModuleInputs { specialization: &self.specialization, ts_facts: ts_check.facts.as_ref() }
    }

    /// Lowers every source into one crate; the first file that fails stops it.
    pub(crate) fn lower(self) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
        self.lower_checked().map(|(lowered, _)| lowered)
    }

    /// [`Self::lower`], also answering what the TypeScript checker reported.
    pub(crate) fn lower_checked(
        self,
    ) -> Result<(LoweredCrate, Vec<ManifestDiagnostic>), Box<dyn std::error::Error>> {
        timing::measure("manifest.frontend_lower", || {
            let Prepared { ordered, ts_check, mut lowering } = self.prepare()?;
            let inputs = self.module_inputs(&ts_check);
            let mut modules = Vec::new();
            for ((source, file_id), name) in ordered.iter().zip(&self.file_ids).zip(&self.module_names) {
                let module = lowering
                    .lower_source(source, *file_id, inputs)
                    .map_err(|diagnostics| manifest_source_error(source, &diagnostics))?;
                if let Ok(module_idx) = usize::try_from(module.0)
                    && let Some(module_value) = lowering.krate_mut().modules.get_mut(module_idx)
                {
                    module_value.name.clone_from(name);
                }
                modules.push((source.path.display().to_string(), module));
            }
            let modules = match self.focus {
                Some(index) => modules.into_iter().skip(index).take(1).collect(),
                None => modules,
            };
            Ok(((lowering.into_crate(), modules), ts_check.diagnostics(&ordered)))
        })
    }

    /// Lowers every source, recovering from per-file failures so one pass
    /// enumerates the blockers of the whole crate.
    ///
    /// A file that fails does not stop the run: imports into already-lowered
    /// modules still resolve. A frontend panic is reported as an `internal`
    /// diagnostic and stops the pass, since the shared crate cannot be recovered.
    ///
    /// With `erasure` settings (`smelt check`), every frontend erasure they mark
    /// `warn`/`deny` is appended as an `erasure` diagnostic carrying its severity,
    /// and the TypeScript checker's report as `type-check` diagnostics; `None`
    /// (the probe) reports lowering blockers only.
    pub(crate) fn diagnostics(
        self,
        erasure: Option<&crate::erasure::ErasureSettings>,
    ) -> Result<Vec<ManifestDiagnostic>, Box<dyn std::error::Error>> {
        let _quiet = QuietPanics::install();
        let Prepared { ordered, ts_check, lowering: predeclared } = self.prepare()?;
        let inputs = self.module_inputs(&ts_check);
        let mut lowering = RecoverableLowering::new(predeclared);
        let mut diagnostics = Vec::new();
        for (source, file_id) in ordered.iter().zip(&self.file_ids) {
            match lowering.lower_source(source, *file_id, inputs) {
                Some(Ok(_module)) => {}
                Some(Err(file_diagnostics)) => diagnostics.extend(file_diagnostics),
                None => {
                    diagnostics.push(ManifestDiagnostic {
                        file: source.path.display().to_string(),
                        category: DiagnosticCategory::Internal,
                        code: "smelt::frontend-panic",
                        message: "frontend panicked while lowering this file; \
                                  remaining modules were not scanned"
                            .to_owned(),
                        line: None,
                        column: None,
                        severity: None,
                    });
                    break;
                }
            }
        }
        // A poisoned crate has no erasure records worth reading.
        if let (Some(settings), Some(krate)) = (erasure, lowering.krate()) {
            diagnostics.extend(erasure_diagnostics(krate, &self.project_dir, settings));
        }
        if erasure.is_some() {
            diagnostics.extend(ts_check.diagnostics(&ordered));
        }
        Ok(diagnostics)
    }
}

/// A run ready to lower its first module; see [`ManifestLowering::prepare`].
struct Prepared<'run> {
    /// Every source of the run, in dependency order.
    ordered: Vec<&'run ManifestSource>,
    /// What the checker made of the sources.
    ts_check: TsCheck,
    /// The predeclared lowering context.
    lowering: PredeclaredLowering,
}

/// The name of the module a program enters through.
const ENTRY_MODULE_NAME: &str = "main";

/// `sources` reordered dependency-first (see [`order_manifest_sources`]).
fn in_dependency_order(sources: Vec<ManifestSource>) -> Vec<ManifestSource> {
    let order = order_manifest_sources(&sources);
    let mut slots = sources.into_iter().map(Some).collect::<Vec<_>>();
    order
        .into_iter()
        .filter_map(|index| slots.get_mut(index).and_then(Option::take))
        .collect()
}

/// One frontend file id per source, or an error when they do not fit.
fn file_ids(count: usize) -> Result<Vec<FileId>, io::Error> {
    (0..count)
        .map(|index| {
            u32::try_from(index).map(FileId).map_err(|error| {
                io::Error::new(io::ErrorKind::InvalidInput, format!("too many source files: {error}"))
            })
        })
        .collect()
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
    /// 1-based line of the diagnostic's span start, when the frontend
    /// reported a span inside this file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) line: Option<u32>,
    /// 1-based column (in UTF-8 bytes) of the span start; see `line`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) column: Option<u32>,
    /// `warning` or `error` for an erasure diagnostic; omitted for lowering
    /// diagnostics, which are always errors (the field is additive so existing
    /// consumers of the JSON keep working).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) severity: Option<Severity>,
}

/// Severity of an erasure or type-check diagnostic, serialized as `warning` /
/// `error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Severity {
    /// The erasure reason is set to `warn`, or the checker warned.
    Warning,
    /// The erasure reason is set to `deny`, or the checker found an error.
    Error,
}

thread_local! {
    /// How many [`QuietPanics`] guards are alive on this thread.
    static QUIET_PANIC_DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Installs, once per process, the panic hook that consults [`QUIET_PANIC_DEPTH`].
static QUIET_PANIC_HOOK: std::sync::Once = std::sync::Once::new();

/// RAII guard that silences panic messages on the current thread while alive.
///
/// Used by recoverable/probing lowering where individual-file panics are caught
/// and reported rather than aborting the process.
///
/// Swapping the process-global hook per guard is unsound under threads (one
/// thread's restore clobbers another's silence) and under nesting dropped out
/// of order. Instead a single hook is installed once for the whole process: it
/// wraps whatever hook was active then and forwards to it unless the panicking
/// thread holds at least one guard. Guards only bump a thread-local depth, so
/// they nest freely and never affect other threads. The guard is `!Send`
/// because the depth it decrements on drop belongs to the creating thread.
pub(crate) struct QuietPanics {
    /// Ties the guard to the thread whose depth it incremented.
    _not_send: std::marker::PhantomData<*const ()>,
}

impl QuietPanics {
    /// Silence panic messages on this thread until the guard drops.
    pub(crate) fn install() -> Self {
        QUIET_PANIC_HOOK.call_once(|| {
            let previous = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                if QUIET_PANIC_DEPTH.with(std::cell::Cell::get) == 0 {
                    previous(info);
                }
            }));
        });
        QUIET_PANIC_DEPTH.with(|depth| depth.set(depth.get().saturating_add(1)));
        Self { _not_send: std::marker::PhantomData }
    }
}

impl Drop for QuietPanics {
    /// Leave this guard's quiet scope on the current thread.
    fn drop(&mut self) {
        QUIET_PANIC_DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

/// Whether panic messages are currently silenced on this thread.
#[cfg(test)]
fn panics_are_quiet() -> bool {
    QUIET_PANIC_DEPTH.with(std::cell::Cell::get) > 0
}

/// The typed-version lookup inputs for a manifest build: the manifest
/// directory is the root `.sound/` and the project's `package.json` are found
/// from, and the manifest's (plus the command line's) defines decide `when`.
fn sound_settings(config: &crate::config::Config, manifest_dir: &Path) -> SoundSettings {
    let root = if manifest_dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        manifest_dir
    };
    SoundSettings {
        root: root.canonicalize().unwrap_or_else(|_| root.to_path_buf()),
        defines: config.defines(),
    }
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

/// The crate-level host-global override set, scanned from every TypeScript source.
///
/// The scan runs once before per-file lowering so a `globalThis.<Name> =` write
/// in any module (typically a spec file) activates the dynamic override
/// machinery — presence guards, `new` dispatch, reads — in the module that
/// defines the predicate, even though that module lowers first.
fn scan_written_host_globals(sources: &[&ManifestSource]) -> std::collections::HashSet<String> {
    sources
        .iter()
        .filter(|source| source.lang.is_typescript())
        .flat_map(|source| {
            smelt_frontend_ts::scan_written_host_globals(
                &source.source,
                &source.path.display().to_string(),
            )
        })
        .collect()
}

/// Crate-unique module identity for each source path.
///
/// Pairs each source with the collision-free module name
/// [`manifest_module_names`] already computes for module BODIES, so a
/// module-private item's qualified Rust name depends on the module's place in
/// the crate rather than on the absolute path the compiler was handed (H55).
fn manifest_module_identities(sources: &[&ManifestSource], names: &[String]) -> HashMap<String, String> {
    names
        .iter()
        .zip(sources.iter())
        .map(|(name, source)| (source.path.display().to_string(), name.clone()))
        .collect()
}

/// Rust-facing class names for class names that more than one module declares.
///
/// Class identity in HIR is the class's name symbol, so two modules exporting a
/// class of the same name interned ONE symbol for two different classes and
/// every method of the loser reported "unknown class method". Hono's two `Node`
/// classes (`router/trie-router/node.ts` and `router/reg-exp-router/node.ts`)
/// are the case that found it.
///
/// The scheme is the one [`manifest_module_names`] already uses for module
/// bodies, for the same reason and with the same shape: a name declared by
/// exactly one module is absent from the result and keeps its bare spelling, so
/// every existing golden stays byte-identical, and among several declarations
/// the LAST in dependency order keeps the bare name while earlier ones take a
/// stable ordinal suffix (`Node_1`, `Node_2`, ...).
///
/// The result maps EVERY module that declares an ambiguous name, including the
/// one whose rendering is the bare spelling (which maps the name to itself).
/// The map is therefore also the answer to "does this module declare this
/// ambiguous name", which is what makes the frontend bind the name in the
/// module's own scope rather than resolve it through the crate-wide by-name
/// item map — see the frontend's `class_declaration`.
///
/// Only the Rust rendering changes; the frontend records the source spelling as
/// the symbol's original name, because `instanceof` and `__smelt_class` read
/// that and JavaScript answers `Node` for both classes.
fn manifest_type_renames(
    sources: &[&ManifestSource],
) -> HashMap<String, HashMap<String, String>> {
    let declared = sources
        .iter()
        .map(|source| {
            let path = source.path.display().to_string();
            if source.lang.is_typescript() {
                smelt_frontend_ts::scan_declared_type_names(&source.source, &path)
            } else {
                Vec::new()
            }
        })
        .collect::<Vec<_>>();
    let mut totals = HashMap::<&str, usize>::new();
    for name in declared.iter().flatten() {
        let total = totals.entry(name.as_str()).or_insert(0);
        *total = total.saturating_add(1);
    }
    let mut seen = HashMap::<&str, usize>::new();
    let mut renames = HashMap::<String, HashMap<String, String>>::new();
    for (source, names) in sources.iter().zip(&declared) {
        for name in names {
            let total = totals.get(name.as_str()).copied().unwrap_or(1);
            if total == 1 {
                continue;
            }
            let ordinal = seen.entry(name.as_str()).or_insert(0);
            *ordinal = ordinal.saturating_add(1);
            // The module that keeps the BARE spelling gets an entry too,
            // mapping the name to itself. It is not a rename — the Rust name is
            // unchanged — but it is what tells the frontend "this module
            // declares this ambiguous name", so the frontend binds the name in
            // the module's own scope instead of resolving it through the
            // crate-wide by-name item map, whose entry for an ambiguous
            // spelling is whichever module registered last. Without it, the
            // winner's own `Node<T>` annotation resolved to the LOSER's class
            // (Hono's trie router read the reg-exp router's `#children`).
            let rendered = if *ordinal == total {
                name.clone()
            } else {
                format!("{name}_{ordinal}")
            };
            renames
                .entry(source.path.display().to_string())
                .or_default()
                .insert(name.clone(), rendered);
        }
    }
    if std::env::var_os("SMELT_DEBUG_TYPE_RENAMES").is_some() {
        for (path, map) in &renames {
            for (name, rendered) in map {
                eprintln!("[type-rename] {path}: {name} -> {rendered}");
            }
        }
    }
    renames
}

/// Canonical paths of project sources the dependency closure did not reach.
///
/// The manifest's source roots (with `[sources] exclude` already applied by
/// [`discover_source_files`]) minus the sources being lowered. A module in this
/// set is one this project owns and the crate does not have, which is the one
/// case where a type reference to a name imported from it can say so instead of
/// erasing to a nominal class (`HirCtx::project_sources_outside_crate`).
///
/// Paths are canonicalized so they match the module keys the frontend builds
/// for an import specifier (`resolved_module_export_keys`). A path that cannot
/// be canonicalized is skipped rather than compared in a second spelling: a key
/// that never matches would only make the check silently inert.
fn sources_outside_crate(
    config: &crate::config::Config,
    manifest_dir: &Path,
    lowered: &[&ManifestSource],
) -> std::collections::HashSet<String> {
    let Ok(project_sources) = discover_source_files(config, manifest_dir) else {
        return std::collections::HashSet::new();
    };
    let in_crate = lowered
        .iter()
        .filter_map(|source| fs::canonicalize(&source.path).ok())
        .collect::<std::collections::HashSet<_>>();
    project_sources
        .into_iter()
        .filter_map(|path| fs::canonicalize(path).ok())
        .filter(|path| !in_crate.contains(path))
        .map(|path| path.display().to_string())
        .collect()
}

/// The build error for a manifest source the frontend rejected.
///
/// A project source reports its path and diagnostics as it always has. A file
/// laid by a package's typed version (see [`crate::sound`]) is reported as that
/// typed version's error, adapting Bun's `TS100021` ("The typed version of 'X'
/// in .sound/X has type errors (N), the first at F. Fix it, or remove it to
/// compile 'X' untyped."): in Bun a type error in a typed version stops the
/// build, and Smelt's frontend rejecting the laid text is the same event —
/// the text claims types Smelt cannot lower. Smelt's diagnostics carry no
/// line and column, so the error names the file and the first diagnostic's
/// message where Bun names a position, and keeps the full diagnostic list
/// below it. Removing the typed version is a real way out: the package is then
/// resolved as installed.
fn manifest_source_error(source: &ManifestSource, diagnostics: &[ManifestDiagnostic]) -> io::Error {
    let path = source.path.display();
    let Some(origin) = &source.typed_version else {
        return io::Error::new(io::ErrorKind::InvalidData, format!("{path}:\n{diagnostics:#?}"));
    };
    let first = diagnostics
        .first()
        .map_or_else(String::new, |diagnostic| format!(": {}", diagnostic.message));
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "error: the typed version of '{name}' in {shown} is rejected by Smelt's frontend ({count} diagnostic{plural}), the first at {path}{first}. Fix it, or remove it to compile '{name}' untyped.\n{diagnostics:#?}",
            name = origin.name,
            shown = origin.shown,
            count = diagnostics.len(),
            plural = if diagnostics.len() == 1 { "" } else { "s" },
        ),
    )
}

/// The frontend erasures `settings` marks `warn`/`deny`, as diagnostics.
fn erasure_diagnostics(
    krate: &smelt_hir::Crate,
    manifest_dir: &Path,
    settings: &crate::erasure::ErasureSettings,
) -> Vec<ManifestDiagnostic> {
    crate::erasure::resolve(krate, &[], manifest_dir, settings)
        .into_iter()
        .filter_map(|record| {
            let severity = match record.severity.as_str() {
                "warn" => Severity::Warning,
                "deny" => Severity::Error,
                _ => return None,
            };
            let reason = smelt_hir::ErasureReason::named(&record.reason)?;
            Some(ManifestDiagnostic {
                file: record.file.clone(),
                category: DiagnosticCategory::Erasure,
                code: reason.info().code,
                message: crate::erasure::message(&record),
                line: Some(record.line),
                column: Some(record.column),
                severity: Some(severity),
            })
        })
        .collect()
}

/// Build per-file [`ManifestDiagnostic`]s from a frontend's categorized errors.
///
/// Each error carries the byte offset of its span start when the span lies in
/// `file` (`None` otherwise); it is resolved against `source` to the 1-based
/// line/column a reader can jump to.
pub(crate) fn manifest_diagnostics(
    file: &str,
    source: &str,
    errors: impl Iterator<Item = (DiagnosticCategory, &'static str, String, Option<u32>)>,
) -> Vec<ManifestDiagnostic> {
    errors
        .map(|(category, code, message, offset)| {
            let (line, column) = offset
                .and_then(|offset| source_line_column(source, offset))
                .map_or((None, None), |(line, column)| (Some(line), Some(column)));
            ManifestDiagnostic {
                file: file.to_owned(),
                category,
                code,
                message,
                line,
                column,
                severity: None,
            }
        })
        .collect()
}

/// Resolve a byte offset in `source` to a 1-based `(line, column)` pair.
///
/// The column counts UTF-8 bytes from the line start. Returns `None` for an
/// offset past the end of `source` (a span from synthesized code).
pub(crate) fn source_line_column(source: &str, offset: u32) -> Option<(u32, u32)> {
    smelt_hir::line_col(source, offset)
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
fn manifest_module_names(
    sources: &[&ManifestSource],
    base_name: impl Fn(&ManifestSource) -> String,
) -> Vec<String> {
    let bases = sources.iter().map(|source| base_name(source)).collect::<Vec<_>>();
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
    //! Unit tests for panic silencing.

    use super::{QuietPanics, panics_are_quiet};

    #[test]
    fn quiet_panics_nest_and_stay_on_their_thread() {
        assert!(!panics_are_quiet());
        let outer = QuietPanics::install();
        {
            let _inner = QuietPanics::install();
            assert!(panics_are_quiet());
            // Another thread is never silenced by this thread's guards.
            let other = std::thread::spawn(panics_are_quiet).join().expect("thread ran");
            assert!(!other);
        }
        // Dropping the inner guard keeps the outer one in force.
        assert!(panics_are_quiet());
        drop(outer);
        assert!(!panics_are_quiet());
    }

    #[test]
    fn quiet_panics_survive_out_of_order_drops() {
        let first = QuietPanics::install();
        let second = QuietPanics::install();
        drop(first);
        assert!(panics_are_quiet());
        drop(second);
        assert!(!panics_are_quiet());
    }

    /// Severity keeps the JSON spelling `check --message-format json` emits.
    #[test]
    fn severity_serializes_as_before() {
        assert_eq!(
            serde_json::to_string(&super::Severity::Warning).expect("serializes"),
            r#""warning""#
        );
        assert_eq!(
            serde_json::to_string(&super::Severity::Error).expect("serializes"),
            r#""error""#
        );
    }
}
