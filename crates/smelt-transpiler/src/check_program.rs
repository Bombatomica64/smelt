//! The type checkers' programs, derived from the module graph Smelt lowers.
//!
//! Smelt resolves imports itself: `[packages]` sources, workspace packages, a
//! package's typed version laid over `node_modules`, and imports from
//! TypeScript into Python and back. The checkers resolve modules their own
//! way, and a module Smelt reaches that they cannot is a false "cannot find
//! module" error that refuses a valid project. So nothing here computes a
//! resolution of its own: [`ModuleGraph`] is built from the very
//! [`ManifestSource`]s lowering consumes, with the edges the dependency
//! closure resolved ([`ManifestSource::resolved_imports`]), and each checker's
//! program is generated from it:
//!
//! - every TypeScript source, at a location the checker reads as TypeScript
//!   (a typed version's `.js` path is served as `.ts`, with the laid text);
//! - every bare specifier Smelt resolved, mapped to the file it resolved to;
//! - each Python module TypeScript imports, as a `.d.ts` generated from its
//!   lowered HIR, and each TypeScript module Python imports, as a `.pyi`
//!   ([`CrossLanguageStubs`]), served in memory where the import looks;
//! - in a test-tier module, the vitest names lowering treats as ambient
//!   globals (`smelt_frontend_ts::ambient_test_globals`), as an import from
//!   the checker's declaration of the test API Smelt models.
//!
//! A checker that still cannot resolve a specifier Smelt resolved is a bug in
//! this module, not in the user's program: [`ModuleGraph::resolved_specifier`]
//! lets the diagnostics turn it into an internal error.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::manifest::ManifestSource;
use crate::source_files::SourceLang;

/// The resolved module graph of one run, over its sources.
pub(crate) struct ModuleGraph<'run> {
    /// Every source, in dependency order.
    sources: &'run [&'run ManifestSource],
    /// Source index by canonical path.
    by_path: HashMap<PathBuf, usize>,
}

impl<'run> ModuleGraph<'run> {
    /// The graph of `sources`.
    pub(crate) fn new(sources: &'run [&'run ManifestSource]) -> Self {
        let by_path = sources
            .iter()
            .enumerate()
            .map(|(index, source)| (canonical(&source.path), index))
            .collect();
        Self { sources, by_path }
    }

    /// Every source.
    pub(crate) const fn sources(&self) -> &'run [&'run ManifestSource] {
        self.sources
    }

    /// The source a resolved edge's target is.
    fn source_at(&self, target: &Path) -> Option<&'run ManifestSource> {
        self.by_path.get(&canonical(target)).and_then(|index| self.sources.get(*index)).copied()
    }

    /// Whether `importer` (a source path as the checker reports it) has an
    /// import of `specifier` that Smelt resolved.
    pub(crate) fn resolved_specifier(&self, importer: &Path, specifier: &str) -> bool {
        self.source_at(importer).is_some_and(|source| {
            source
                .resolved_imports
                .iter()
                .chain(&source.excluded_targets)
                .any(|edge| edge.specifier == specifier)
        })
    }

    /// The modules of `provider` language that a source of the other language
    /// imports: the providers whose stubs one checker needs.
    pub(crate) fn cross_language_providers(&self, provider: Language) -> Vec<&'run ManifestSource> {
        let mut providers: Vec<&ManifestSource> = Vec::new();
        for importer in self.sources.iter().filter(|source| Language::of(source.lang) == provider.other()) {
            for edge in &importer.resolved_imports {
                if let Some(target) = self.source_at(&edge.target)
                    && Language::of(target.lang) == provider
                    && !providers.iter().any(|known| std::ptr::eq(*known, target))
                {
                    providers.push(target);
                }
            }
        }
        providers
    }

    /// The sources `roots` need lowered first: their dependency closure, in
    /// the run's (dependency) order.
    pub(crate) fn closure_of(&self, roots: &[&ManifestSource]) -> Vec<&'run ManifestSource> {
        let mut wanted = vec![false; self.sources.len()];
        let mut pending = roots.iter().map(|root| canonical(&root.path)).collect::<Vec<_>>();
        while let Some(path) = pending.pop() {
            let Some(&index) = self.by_path.get(&path) else { continue };
            let Some(seen) = wanted.get_mut(index) else { continue };
            if *seen {
                continue;
            }
            *seen = true;
            if let Some(source) = self.sources.get(index) {
                pending.extend(source.dependencies.iter().map(|dependency| canonical(dependency)));
            }
        }
        self.sources
            .iter()
            .zip(wanted)
            .filter_map(|(source, keep)| keep.then_some(*source))
            .collect()
    }
}

/// The two source languages of the graph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Language {
    /// TypeScript (and its declaration files).
    TypeScript,
    /// Python (and its stubs).
    Python,
}

impl Language {
    /// The language of a source.
    pub(crate) const fn of(lang: SourceLang) -> Self {
        match lang {
            SourceLang::TypeScript | SourceLang::TypeScriptDeclaration => Self::TypeScript,
            SourceLang::Python | SourceLang::PythonDeclaration => Self::Python,
        }
    }

    /// The other language.
    const fn other(self) -> Self {
        match self {
            Self::TypeScript => Self::Python,
            Self::Python => Self::TypeScript,
        }
    }
}

/// Declarations of the modules one checker cannot read: by provider path, the
/// TypeScript declaration of a Python module (for the TypeScript checker) or
/// the Python stub of a TypeScript module (for ty).
#[derive(Debug, Default)]
pub(crate) struct CrossLanguageStubs {
    /// Provider path → `.d.ts` text.
    typescript: BTreeMap<PathBuf, String>,
    /// Provider path → `.pyi` text.
    python: BTreeMap<PathBuf, String>,
}

impl CrossLanguageStubs {
    /// Record the `.d.ts` of the Python module at `provider`.
    pub(crate) fn add_typescript(&mut self, provider: &Path, text: String) {
        self.typescript.insert(canonical(provider), text);
    }

    /// Record the `.pyi` of the TypeScript module at `provider`.
    pub(crate) fn add_python(&mut self, provider: &Path, text: String) {
        self.python.insert(canonical(provider), text);
    }

    /// The `.pyi` stubs, each at the path a Python import of its provider looks.
    pub(crate) fn python_declarations(&self) -> Vec<(PathBuf, &str)> {
        self.python.iter().map(|(provider, text)| (stub_path(provider, "pyi", "__init__"), text.as_str())).collect()
    }
}

/// Where a stub of `provider` goes: beside it with `extension`, or, for a
/// package's `__init__.py` / `index.ts`, as the package's `package_entry`.
fn stub_path(provider: &Path, extension: &str, package_entry: &str) -> PathBuf {
    let is_entry = provider
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == "__init__" || stem == "index");
    if is_entry && let Some(parent) = provider.parent() {
        return parent.join(format!("{package_entry}.{extension}"));
    }
    provider.with_extension(extension)
}

/// Where the TypeScript checker reads `source`: its absolute path, with a
/// JavaScript extension replaced by its TypeScript one when the text is
/// TypeScript (a typed version laid at an installed `.js` path).
pub(crate) fn checker_location(source: &ManifestSource) -> PathBuf {
    let absolute = std::path::absolute(&source.path).unwrap_or_else(|_| source.path.clone());
    if source.lang != SourceLang::TypeScript {
        return absolute;
    }
    let swapped = match absolute.extension().and_then(|extension| extension.to_str()) {
        Some("js") => "ts",
        Some("mjs") => "mts",
        Some("cjs") => "cts",
        Some("jsx") => "tsx",
        _ => return absolute,
    };
    absolute.with_extension(swapped)
}

/// `path` canonicalized when it exists, made absolute otherwise.
fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
}

/// Whether `specifier` is bare (a package name), not relative or absolute.
fn is_bare(specifier: &str) -> bool {
    !(specifier.starts_with('.') || specifier.starts_with('/'))
}

/// The TypeScript checker's program for `graph`, with `stubs` for the Python
/// modules TypeScript imports.
#[cfg(feature = "ts7")]
pub(crate) fn typescript_program(graph: &ModuleGraph<'_>, stubs: &CrossLanguageStubs) -> smelt_ts_types::CheckProgram {
    let typescript = graph
        .sources()
        .iter()
        .filter(|source| Language::of(source.lang) == Language::TypeScript)
        .collect::<Vec<_>>();
    let sources = typescript
        .iter()
        .map(|source| {
            let mut text = source.source.clone();
            let globals = smelt_frontend_ts::ambient_test_globals(&source.source, &source.path.display().to_string());
            if !globals.is_empty() {
                // Appended, so every span of the lowered text is unchanged.
                let _ = write!(text, "\n;import {{ {} }} from \"vitest\";\n", globals.join(", "));
            }
            smelt_ts_types::SourceInput::new(source.path.display().to_string(), text)
                .located_at(checker_location(source).display().to_string())
        })
        .collect();
    let mut declarations = stubs
        .typescript
        .iter()
        .map(|(provider, text)| {
            let location = stub_path(provider, "d.ts", "index");
            smelt_ts_types::SourceInput::new(location.display().to_string(), text.clone())
        })
        .collect::<Vec<_>>();
    // A module `[sources] exclude` keeps out of the crate is still what its
    // import means: the checker reads it (its own diagnostics are not the
    // crate's) and resolves the import to it.
    let mut excluded = BTreeSet::new();
    for edge in typescript.iter().flat_map(|importer| &importer.excluded_targets) {
        let target = canonical(&edge.target);
        let is_typescript = SourceLang::from_path(&target.display().to_string()).is_ok_and(SourceLang::is_typescript);
        if is_typescript
            && !excluded.contains(&target)
            && let Ok(text) = std::fs::read_to_string(&target)
        {
            declarations.push(smelt_ts_types::SourceInput::new(target.display().to_string(), text));
            excluded.insert(target);
        }
    }
    let modeled = smelt_frontend_ts::modeled_module_declarations();
    let mut untyped = BTreeSet::new();
    for importer in &typescript {
        for specifier in &importer.unresolved_packages {
            let declared = format!("declare module {specifier:?}");
            if !smelt_ts_types::declares_module(specifier) && !modeled.contains(&declared) {
                untyped.insert(specifier.as_str());
            }
        }
    }
    // A package Smelt resolved to nothing (a dev dependency not installed)
    // is what lowering reads as a dynamic value; the checker gets the same
    // view, an untyped module, rather than a "cannot find module" error.
    let mut untyped_declarations = String::new();
    for specifier in &untyped {
        let _ = writeln!(untyped_declarations, "declare module {specifier:?};");
    }
    declarations.push(smelt_ts_types::SourceInput::new(
        HOST_MODULES_PATH,
        format!("{}{modeled}{untyped_declarations}", host_module_declarations()),
    ));
    let mut module_paths = BTreeMap::new();
    for importer in &typescript {
        for edge in importer.excluded_targets.iter().filter(|edge| is_bare(&edge.specifier)) {
            module_paths.entry(edge.specifier.clone()).or_insert_with(|| canonical(&edge.target).display().to_string());
        }
        for edge in importer.resolved_imports.iter().filter(|edge| is_bare(&edge.specifier)) {
            let Some(target) = graph.source_at(&edge.target) else { continue };
            let location = match Language::of(target.lang) {
                Language::TypeScript => checker_location(target),
                Language::Python => stub_path(&canonical(&target.path), "d.ts", "index"),
            };
            module_paths.entry(edge.specifier.clone()).or_insert_with(|| location.display().to_string());
        }
    }
    smelt_ts_types::CheckProgram::of_sources(sources)
        .with_declarations(declarations)
        .with_module_paths(module_paths)
}

/// Program path of the declarations of the npm packages Smelt models natively.
#[cfg(feature = "ts7")]
const HOST_MODULES_PATH: &str = "__smelt_host_modules.d.ts";

/// Ambient declarations of the npm packages Smelt models natively
/// (`smelt_stdlib::HOST_MODULES`), generated from that registry so the checker
/// sees exactly the specifiers and export names lowering resolves.
///
/// A module with a `node:` specifier is Node's own, which the vendored
/// `@types/node` types precisely; only the others (`@hono/node-server`,
/// `@date-fns/tz`) are declared here. Their exports are `any`: Smelt models a
/// name, not a TypeScript signature, and an export it only declares is a named
/// lowering blocker when used, not a type error.
#[cfg(feature = "ts7")]
fn host_module_declarations() -> String {
    let mut text = String::new();
    for module in smelt_stdlib::HOST_MODULES {
        if module.specifiers.iter().any(|specifier| specifier.starts_with("node:")) {
            continue;
        }
        let mut body = String::new();
        for export in module.exports {
            if matches!(export.kind, smelt_stdlib::HostExportKind::Value | smelt_stdlib::HostExportKind::ValueAndType) {
                let _ = writeln!(body, "  export const {}: any;", export.name);
            }
            if matches!(export.kind, smelt_stdlib::HostExportKind::Type | smelt_stdlib::HostExportKind::ValueAndType) {
                let _ = writeln!(body, "  export type {} = any;", export.name);
            }
        }
        for specifier in module.specifiers {
            let _ = writeln!(text, "declare module {specifier:?} {{\n{body}}}");
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A package entry's stub is the package's entry; any other module's sits
    /// beside it.
    #[test]
    fn stubs_go_where_the_import_looks() {
        assert_eq!(stub_path(Path::new("/p/src/math.py"), "d.ts", "index"), Path::new("/p/src/math.d.ts"));
        assert_eq!(stub_path(Path::new("/p/src/pkg/__init__.py"), "d.ts", "index"), Path::new("/p/src/pkg/index.d.ts"));
        assert_eq!(stub_path(Path::new("/p/src/lib/index.ts"), "pyi", "__init__"), Path::new("/p/src/lib/__init__.pyi"));
        assert_eq!(stub_path(Path::new("/p/src/util.ts"), "pyi", "__init__"), Path::new("/p/src/util.pyi"));
    }

    /// Every modeled npm package is declared with every export it models.
    #[cfg(feature = "ts7")]
    #[test]
    fn modeled_npm_packages_are_declared() {
        let text = host_module_declarations();
        assert!(text.contains("declare module \"@hono/node-server\" {\n  export const serve: any;"), "{text}");
        assert!(!text.contains("\"node:"), "Node's own modules come from @types/node: {text}");
    }

    /// Bare specifiers are package names; relative and absolute ones are not.
    #[test]
    fn bare_specifiers_are_package_names() {
        assert!(is_bare("geom") && is_bare("@scope/pkg/sub") && is_bare("node:events"));
        assert!(!is_bare("./a") && !is_bare("../a") && !is_bare("/abs"));
    }
}
