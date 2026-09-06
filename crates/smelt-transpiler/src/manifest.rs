//! Manifest source graph helpers.
//!
//! This module collects import edges from manifest entries and computes
//! dependency-first ordering for multi-file lowering.

use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

use oxc_resolver::{ResolveOptions, Resolver};

use crate::lowering::SourceLang;

/// A manifest source entry prepared for module graph ordering.
#[derive(Debug, Clone)]
pub(crate) struct ManifestSource {
    /// Original manifest entry resolved against the manifest directory.
    pub(crate) path: PathBuf,
    /// Source text read while scanning imports.
    pub(crate) source: String,
    /// Source language inferred from the file extension.
    pub(crate) lang: SourceLang,
    /// Import specifiers found before lowering.
    imports: Vec<ManifestImport>,
    /// Resolved local dependency paths used for dependency-first ordering.
    pub(crate) dependencies: Vec<PathBuf>,
}

/// Import metadata found while scanning source text.
#[derive(Debug, Clone)]
pub(crate) struct ManifestImport {
    /// Module specifier as written in source.
    pub(crate) module: String,
    /// Named value imports, when the statement exposes them cheaply.
    pub(crate) names: Option<Vec<String>>,
    /// Python relative import level from `from .pkg import name` syntax.
    pub(crate) python_relative_level: Option<u32>,
}

/// Mutable state while visiting the manifest import graph.
struct ManifestGraphVisit<'a> {
    /// Manifest sources being ordered.
    sources: &'a [ManifestSource],
    /// Lookup from normalized path keys to source indexes.
    index: &'a HashMap<PathBuf, usize>,
    /// Dependency-first output order.
    ordered: Vec<usize>,
    /// Nodes currently on the DFS stack.
    ///
    /// Back edges are allowed because TypeScript and Python modules can form
    /// import cycles. The manifest sorter still emits each source once in a
    /// stable order; later lowering stages are responsible for reporting any
    /// cycle shape whose runtime symbols cannot be resolved yet.
    temporary: HashSet<usize>,
    /// Nodes already visited.
    permanent: HashSet<usize>,
}

/// Resolves a path relative to the manifest directory, or returns it if absolute.
pub(crate) fn resolve_manifest_path(manifest_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        manifest_dir.join(path)
    }
}

/// Reads a manifest entry and collects imports for dependency sorting.
pub(crate) fn read_manifest_source(
    path: PathBuf,
) -> Result<ManifestSource, Box<dyn std::error::Error>> {
    let file_string = path.display().to_string();
    let lang = SourceLang::from_path(&file_string)?;
    let source = fs::read_to_string(&path)?;
    let imports = scan_imports(&source, lang).map_err(|message| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to scan imports in {}: {message}", path.display()),
        )
    })?;
    Ok(ManifestSource {
        path,
        source,
        lang,
        imports,
        dependencies: Vec::new(),
    })
}

/// Expands root manifest entries with local imports discovered from each source.
pub(crate) fn dependency_closure(
    roots: Vec<ManifestSource>,
) -> Result<Vec<ManifestSource>, Box<dyn std::error::Error>> {
    let mut collector = DependencyCollector::default();
    for root in roots {
        collector.collect_source(root)?;
    }
    Ok(collector.sources)
}

/// Returns manifest source indexes in dependency-first order.
pub(crate) fn order_manifest_sources(sources: &[ManifestSource]) -> Result<Vec<usize>, String> {
    let index = manifest_source_index(sources);
    let mut visit = ManifestGraphVisit {
        sources,
        index: &index,
        ordered: Vec::new(),
        temporary: HashSet::new(),
        permanent: HashSet::new(),
    };

    for idx in 0..sources.len() {
        visit_manifest_source(idx, &mut visit)?;
    }

    Ok(visit.ordered)
}

/// Builds lookup keys that import specifiers can resolve against.
fn manifest_source_index(sources: &[ManifestSource]) -> HashMap<PathBuf, usize> {
    let mut index = HashMap::new();
    for (idx, source) in sources.iter().enumerate() {
        add_manifest_key(&mut index, &source.path, idx);
        if source
            .path
            .file_name()
            .is_some_and(|name| name == "__init__.py")
            && let Some(package_root) = source.path.parent()
        {
            add_manifest_key(&mut index, package_root, idx);
            if let Some(package_name) = package_root.file_name() {
                index.insert(PathBuf::from(package_name), idx);
            }
        }
        if let Some(stem) = source.path.file_stem() {
            index.insert(PathBuf::from(stem), idx);
        }
    }
    index
}

/// Stateful dependency collector for one manifest expansion.
struct DependencyCollector {
    /// Sources discovered so far in traversal order.
    sources: Vec<ManifestSource>,
    /// Normalized source paths that have already been collected.
    seen: HashSet<PathBuf>,
    /// TypeScript resolver reused across all import edges.
    ts_resolver: Resolver,
    /// Parsed TypeScript barrel export maps keyed by normalized path.
    barrel_exports: HashMap<PathBuf, HashMap<String, String>>,
    /// Bare package import targets discovered from workspace package metadata.
    workspace_packages: HashMap<String, Option<PathBuf>>,
}

impl Default for DependencyCollector {
    /// Creates an empty collector with shared resolver state.
    fn default() -> Self {
        Self {
            sources: Vec::new(),
            seen: HashSet::new(),
            ts_resolver: typescript_resolver(),
            barrel_exports: HashMap::new(),
            workspace_packages: HashMap::new(),
        }
    }
}

impl DependencyCollector {
    /// Adds one source and recursively adds local import targets that exist on disk.
    fn collect_source(
        &mut self,
        mut source: ManifestSource,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let key = normalize_path_key(&source.path);
        if !self.seen.insert(key) {
            return Ok(());
        }
        let imports = source.imports.clone();
        let source_path = source.path.clone();
        let lang = source.lang;
        let mut dependencies = Vec::new();
        for import in &imports {
            dependencies.extend(self.resolve_import_to_existing_sources(
                &source_path,
                lang,
                import,
            )?);
        }
        source.dependencies.clone_from(&dependencies);
        self.sources.push(source);
        for path in dependencies {
            if self.seen.contains(&normalize_path_key(&path)) {
                continue;
            }
            let dep = read_manifest_source(path)?;
            self.collect_source(dep)?;
        }
        Ok(())
    }

    /// Resolves a local import specifier to existing source files.
    fn resolve_import_to_existing_sources(
        &mut self,
        importer_path: &Path,
        importer_lang: SourceLang,
        import: &ManifestImport,
    ) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        if importer_lang.is_typescript() {
            return self.resolve_typescript_import_to_existing_sources(importer_path, import);
        }

        let base = python_import_base(importer_path, import);
        for candidate in manifest_import_candidates(&base) {
            if candidate.is_file() {
                return Ok(vec![candidate.canonicalize()?]);
            }
        }
        Ok(Vec::new())
    }

    /// Resolves a TypeScript import specifier with the shared resolver.
    fn resolve_typescript_import_to_existing_sources(
        &mut self,
        importer_path: &Path,
        import: &ManifestImport,
    ) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        let Some(candidate) =
            resolve_typescript_path(&self.ts_resolver, importer_path, &import.module)?
        else {
            return self.resolve_workspace_package_import(importer_path, import);
        };
        let canonical_candidate = candidate.canonicalize()?;
        if let Some(mut targets) = self.resolve_barrel_import_targets(&candidate, import) {
            prepend_unique_dependency(&mut targets, canonical_candidate);
            return Ok(targets);
        }
        Ok(vec![canonical_candidate])
    }

    /// Resolve a bare package import through workspace package metadata.
    fn resolve_workspace_package_import(
        &mut self,
        importer_path: &Path,
        import: &ManifestImport,
    ) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
        if import.module.starts_with('.') || import.module.starts_with('/') {
            return Ok(Vec::new());
        }
        let package_name = bare_package_name(&import.module);
        if !self.workspace_packages.contains_key(&package_name) {
            let root = workspace_search_root(importer_path);
            let target = find_workspace_package_entry(&root, &package_name)?;
            self.workspace_packages.insert(package_name.clone(), target);
        }
        let Some(entry) = self
            .workspace_packages
            .get(&package_name)
            .cloned()
            .flatten()
        else {
            return Ok(Vec::new());
        };
        let workspace_import = ManifestImport {
            module: entry.display().to_string(),
            names: import.names.clone(),
            python_relative_level: None,
        };
        let canonical_entry = entry.canonicalize()?;
        if let Some(mut targets) = self.resolve_barrel_import_targets(&entry, &workspace_import) {
            prepend_unique_dependency(&mut targets, canonical_entry);
            return Ok(targets);
        }
        Ok(vec![canonical_entry])
    }

    /// Resolves named imports from an `index.ts` barrel to their source files.
    ///
    /// A consuming application only needs modules that supply its named
    /// imports. Including the barrel itself causes its unrelated `export *`
    /// edges to expand the entire package, including runtime surfaces not used
    /// by the consumer.
    fn resolve_barrel_import_targets(
        &mut self,
        candidate: &Path,
        import: &ManifestImport,
    ) -> Option<Vec<PathBuf>> {
        if candidate.file_name().is_none_or(|name| name != "index.ts") {
            return None;
        }
        let names = import.names.as_ref()?;
        if names.is_empty() {
            return None;
        }
        let key = normalize_path_key(candidate);
        if !self.barrel_exports.contains_key(&key) {
            let source = fs::read_to_string(candidate).ok()?;
            self.barrel_exports
                .insert(key.clone(), scan_typescript_barrel_exports(&source));
        }
        let export_map = self.barrel_exports.get(&key)?;
        let mut targets = Vec::new();
        for name in names {
            let module = export_map.get(name)?;
            let target = resolve_typescript_path(&self.ts_resolver, candidate, module).ok()??;
            targets.push(target.canonicalize().ok()?);
        }
        Some(targets)
    }
}

/// Keeps a barrel module in the dependency graph before its selected targets.
///
/// Named barrel imports are resolved to concrete target modules to avoid pulling
/// in an entire package. The barrel still has to be lowered because renamed
/// re-exports, such as `export { enUS as defaultLocale }`, define the public
/// alias that importers reference.
fn prepend_unique_dependency(dependencies: &mut Vec<PathBuf>, dependency: PathBuf) {
    let dependency_key = normalize_path_key(&dependency);
    if dependencies
        .iter()
        .any(|existing| normalize_path_key(existing) == dependency_key)
    {
        return;
    }
    dependencies.insert(0, dependency);
}

/// Builds the filesystem base path for a Python import using AST relative levels.
fn python_import_base(importer_path: &Path, import: &ManifestImport) -> PathBuf {
    let importer_dir = importer_path.parent().unwrap_or_else(|| Path::new("."));
    let base_dir = import.python_relative_level.map_or_else(
        || importer_dir.to_path_buf(),
        |level| python_relative_base_dir(importer_dir, level),
    );
    if import.module.is_empty() {
        base_dir
    } else {
        base_dir.join(import.module.replace('.', "/"))
    }
}

/// Moves from an importer directory to the package root implied by leading dots.
fn python_relative_base_dir(importer_dir: &Path, level: u32) -> PathBuf {
    let mut base = importer_dir.to_path_buf();
    for _ in 1..level {
        if let Some(parent) = base.parent() {
            base = parent.to_path_buf();
        }
    }
    base
}

/// Builds the resolver options used for TypeScript manifest dependency discovery.
fn typescript_resolver() -> Resolver {
    Resolver::new(ResolveOptions {
        extensions: vec![
            ".ts".into(),
            ".mts".into(),
            ".cts".into(),
            ".d.ts".into(),
            ".py".into(),
            ".pyi".into(),
        ],
        main_files: vec!["index".into(), "__init__".into()],
        ..ResolveOptions::default()
    })
}

/// Resolves one TypeScript module specifier and returns supported Smelt sources.
fn resolve_typescript_path(
    resolver: &Resolver,
    importer_path: &Path,
    module: &str,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let importer = importer_path.canonicalize()?;
    if let Ok(resolution) = resolver.resolve_file(importer.clone(), module) {
        let path = resolution.into_path_buf();
        if SourceLang::from_path(&path.display().to_string()).is_ok() {
            return Ok(Some(path));
        }
    }
    if module.starts_with('.') || module.starts_with('/') {
        let base = if module.starts_with('/') {
            PathBuf::from(module)
        } else {
            importer
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join(module)
        };
        for candidate in manifest_import_candidates(&base) {
            if candidate.is_file()
                && SourceLang::from_path(&candidate.display().to_string()).is_ok()
            {
                return Ok(Some(candidate.canonicalize()?));
            }
        }
    }
    Ok(None)
}

/// Adds canonical and extensionless keys for one manifest path.
fn add_manifest_key(index: &mut HashMap<PathBuf, usize>, path: &Path, idx: usize) {
    index.insert(normalize_path_key(path), idx);
    if let Some(without_extension) = path.with_extension("").to_str() {
        index.insert(normalize_path_key(Path::new(without_extension)), idx);
    }
}

/// Visits a manifest source and all known local dependencies.
fn visit_manifest_source(idx: usize, visit: &mut ManifestGraphVisit<'_>) -> Result<(), String> {
    if visit.permanent.contains(&idx) {
        return Ok(());
    }
    if !visit.temporary.insert(idx) {
        return Ok(());
    }

    let Some(source) = visit.sources.get(idx) else {
        return Ok(());
    };
    for dependency in &source.dependencies {
        if let Some(dep) = visit.index.get(&normalize_path_key(dependency)).copied() {
            if dep != idx {
                visit_manifest_source(dep, visit)?;
            }
        }
    }

    visit.temporary.remove(&idx);
    visit.permanent.insert(idx);
    visit.ordered.push(idx);
    Ok(())
}

/// TypeScript inputs a JavaScript output extension can have come from.
///
/// Under `moduleResolution: NodeNext` an ESM import specifier names the file
/// that will exist *after* compilation, so a TypeScript project spells its own
/// modules `./app.js` even though the file on disk is `./app.ts`. `tsc` resolves
/// such a specifier by substituting the input extension; this table is that
/// substitution, keyed by the output extension the specifier carries. It is a
/// property of the module system, not of any particular package, so it applies
/// to every relative specifier Smelt collects.
const NODE_NEXT_INPUT_EXTENSIONS: &[(&str, &[&str])] = &[
    ("js", &["ts", "d.ts"]),
    ("mjs", &["mts", "d.mts"]),
    ("cjs", &["cts", "d.cts"]),
];

/// Return the TypeScript input extensions a specifier extension can resolve to.
fn node_next_input_extensions(extension: &str) -> &'static [&'static str] {
    NODE_NEXT_INPUT_EXTENSIONS
        .iter()
        .find(|(output, _)| *output == extension)
        .map_or(&[], |(_, inputs)| *inputs)
}

/// Builds possible source paths for an import specifier.
///
/// An extensionless specifier gets the usual per-language extension and
/// directory-index candidates. A specifier that already carries a JavaScript
/// output extension additionally gets its NodeNext input candidates (see
/// [`NODE_NEXT_INPUT_EXTENSIONS`]), so `./app.js` finds `./app.ts`.
fn manifest_import_candidates(base: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(base.to_path_buf());
    match base.extension().and_then(|extension| extension.to_str()) {
        None => {
            candidates.push(base.with_extension("ts"));
            candidates.push(base.with_extension("d.ts"));
            candidates.push(base.with_extension("py"));
            candidates.push(base.with_extension("pyi"));
            candidates.push(base.join("index.ts"));
            candidates.push(base.join("index.d.ts"));
            candidates.push(base.join("__init__.py"));
            candidates.push(base.join("__init__.pyi"));
        }
        Some(extension) => {
            for input in node_next_input_extensions(extension) {
                candidates.push(base.with_extension(input));
            }
        }
    }
    candidates
}

/// Return the package name part of a bare TypeScript import specifier.
fn bare_package_name(module: &str) -> String {
    let mut parts = module.split('/');
    let Some(first) = parts.next() else {
        return module.to_owned();
    };
    if first.starts_with('@')
        && let Some(second) = parts.next()
    {
        return format!("{first}/{second}");
    }
    first.to_owned()
}

/// Choose a bounded root for scanning workspace packages.
fn workspace_search_root(importer_path: &Path) -> PathBuf {
    let mut candidate = importer_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    for ancestor in importer_path.ancestors() {
        if ancestor.join("pnpm-workspace.yaml").exists()
            || ancestor.join("yarn.lock").exists()
            || ancestor.join("package.json").exists() && ancestor.join("packages").is_dir()
        {
            candidate = ancestor.to_path_buf();
        }
    }
    candidate
}

/// Find a workspace package entry file by its package.json `name`.
fn find_workspace_package_entry(
    root: &Path,
    package_name: &str,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        if dir
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| matches!(name, "node_modules" | "dist" | "target" | ".git"))
        {
            continue;
        }
        let package_json = dir.join("package.json");
        if package_json.exists()
            && let Some(entry) = workspace_package_entry(&package_json, package_name)?
        {
            return Ok(Some(entry));
        }
        let Ok(children) = fs::read_dir(&dir) else {
            continue;
        };
        for child in children.flatten() {
            let Ok(file_type) = child.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(child.path());
            }
        }
    }
    Ok(None)
}

/// Return a supported source entry for a matching workspace package.
fn workspace_package_entry(
    package_json: &Path,
    package_name: &str,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let value: serde_json::Value = serde_json::from_str(&fs::read_to_string(package_json)?)?;
    if value.get("name").and_then(serde_json::Value::as_str) != Some(package_name) {
        return Ok(None);
    }
    let Some(package_dir) = package_json.parent() else {
        return Ok(None);
    };
    for field in ["source", "types", "typings", "main"] {
        let Some(entry) = value.get(field).and_then(serde_json::Value::as_str) else {
            continue;
        };
        for candidate in manifest_import_candidates(&package_dir.join(entry)) {
            if candidate.exists() && SourceLang::from_path(&candidate.display().to_string()).is_ok()
            {
                return Ok(Some(candidate));
            }
        }
    }
    for source_entry in [
        package_dir.join("src/index.ts"),
        package_dir.join("index.ts"),
    ] {
        if source_entry.is_file() {
            return Ok(Some(source_entry));
        }
    }
    Ok(None)
}

/// Maps simple named exports in a TypeScript barrel file to their module specifiers.
fn scan_typescript_barrel_exports(source: &str) -> HashMap<String, String> {
    let mut exports = HashMap::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("export ") || !trimmed.contains(" from ") {
            continue;
        }
        let Some((left, right)) = trimmed.split_once(" from ") else {
            continue;
        };
        let Some(module) = quoted_module_specifier(right) else {
            continue;
        };
        if left.trim() == "export *" {
            if let Some(name) = barrel_export_name_from_module(&module) {
                exports.insert(name, module);
            }
            continue;
        }
        let export_clause = left.trim();
        let Some(export_specifiers_start) = export_clause
            .strip_prefix("export {")
            .or_else(|| export_clause.strip_prefix("export type {"))
        else {
            continue;
        };
        let Some(export_specifiers) = export_specifiers_start.trim().strip_suffix('}') else {
            continue;
        };
        for name in named_specifiers(export_specifiers, true) {
            exports.insert(name, module.clone());
        }
    }
    exports
}

/// Infers a conventional exported symbol from one-module `export *` entries.
///
/// Utility packages commonly expose `flat` with `export * from "./flat"` and
/// place that public function in `flat.ts`. Resolving an imported name equal
/// to the module basename lets a named consumer compile only the modules it
/// actually imports. Multi-export modules still fall back to barrel lowering
/// when their requested name cannot be inferred here.
fn barrel_export_name_from_module(module: &str) -> Option<String> {
    let trimmed = module.strip_prefix("./").unwrap_or(module);
    let mut parts = trimmed.split('/');
    let name = parts.next()?;
    match parts.next() {
        None => Path::new(name)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(ToOwned::to_owned),
        Some("index" | "index.ts") => Some(name.to_owned()),
        Some(_) => None,
    }
}

/// Normalizes a path key without requiring the path to already exist.
fn normalize_path_key(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::Normal(normal) => normalized.push(normal),
        }
    }
    normalized
}

/// Scans TypeScript and Python source text for import module specifiers.
fn scan_imports(source: &str, lang: SourceLang) -> Result<Vec<ManifestImport>, String> {
    match lang {
        SourceLang::TypeScript | SourceLang::TypeScriptDeclaration => {
            Ok(scan_typescript_imports(source))
        }
        SourceLang::Python | SourceLang::PythonDeclaration => crate::python::scan_imports(source),
    }
}

/// Scans TypeScript import and re-export module specifiers.
fn scan_typescript_imports(source: &str) -> Vec<ManifestImport> {
    typescript_import_statements(source)
        .into_iter()
        .filter_map(|line| {
            let trimmed = line.as_str();
            if trimmed.starts_with("import ") {
                let specifier = trimmed
                    .split_once(" from ")
                    .map_or(trimmed.strip_prefix("import "), |(_, right)| Some(right))?;
                let module = quoted_module_specifier(specifier)?;
                let names = trimmed
                    .split_once(" from ")
                    .and_then(|(left, _)| named_imports_from_import_clause(left));
                return Some(ManifestImport {
                    module,
                    names,
                    python_relative_level: None,
                });
            }
            if (trimmed.starts_with("export ") || trimmed.starts_with("export type "))
                && trimmed.contains(" from ")
            {
                let (_, right) = trimmed.split_once(" from ")?;
                return quoted_module_specifier(right).map(|module| ManifestImport {
                    module,
                    names: None,
                    python_relative_level: None,
                });
            }
            None
        })
        .collect()
}

/// Return complete TypeScript import/export declarations, including multiline forms.
fn typescript_import_statements(source: &str) -> Vec<String> {
    let mut statements = Vec::new();
    let mut current: Option<String> = None;
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(buffer) = &mut current {
            buffer.push(' ');
            buffer.push_str(trimmed);
            if trimmed.ends_with(';') {
                statements.push(buffer.trim_end_matches(';').trim().to_owned());
                current = None;
            }
            continue;
        }
        if trimmed.starts_with("import ")
            || ((trimmed.starts_with("export ") || trimmed.starts_with("export type "))
                && trimmed.contains(" from "))
        {
            if trimmed.ends_with(';') || trimmed.contains(" from ") {
                statements.push(trimmed.trim_end_matches(';').trim().to_owned());
            } else {
                current = Some(trimmed.to_owned());
            }
        }
    }
    if let Some(buffer) = current {
        statements.push(buffer.trim_end_matches(';').trim().to_owned());
    }
    statements
}

/// Extracts named imports from a simple TypeScript import clause.
fn named_imports_from_import_clause(input: &str) -> Option<Vec<String>> {
    let start = input.find('{')?;
    let end = input.rfind('}')?;
    let body_start = start.checked_add(1)?;
    let body = input.get(body_start..end)?;
    Some(named_specifiers(body, false))
}

/// Extracts value names from comma-separated import/export specifiers.
fn named_specifiers(input: &str, use_alias: bool) -> Vec<String> {
    input
        .split(',')
        .filter_map(|part| {
            let trimmed = part.trim();
            if trimmed.is_empty() {
                return None;
            }
            let without_type = trimmed.strip_prefix("type ").unwrap_or(trimmed).trim();
            let raw_name = without_type
                .split_once(" as ")
                .map_or(
                    without_type,
                    |(name, alias)| {
                        if use_alias { alias } else { name }
                    },
                );
            let specifier_name = raw_name.trim();
            (!specifier_name.is_empty()).then(|| specifier_name.to_owned())
        })
        .collect()
}

/// Extracts the first quoted module specifier from an import tail.
fn quoted_module_specifier(input: &str) -> Option<String> {
    let trimmed = input.trim().trim_end_matches(';').trim();
    let quote = trimmed.chars().next()?;
    if quote != '\'' && quote != '"' {
        return None;
    }
    let rest = &trimmed[quote.len_utf8()..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn scans_multiline_typescript_imports() {
        let source = r#"
            import { expect, test } from "vitest";
            import {
              ALL_TYPES_DATA_PROVIDER,
              TYPES_DATA_PROVIDER,
            } from "../test/typesDataProvider";
        "#;

        let imports = scan_typescript_imports(source);

        assert!(imports.iter().any(|import| {
            import.module == "../test/typesDataProvider"
                && import.names.as_ref().is_some_and(|names| {
                    names
                        == &vec![
                            "ALL_TYPES_DATA_PROVIDER".to_owned(),
                            "TYPES_DATA_PROVIDER".to_owned(),
                        ]
                })
        }));
    }

    #[test]
    fn orders_relative_typescript_dependencies_before_importers() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("smelt_manifest_test_{unique}"));
        let src_dir = root.join("packages/remeda/src");
        let test_dir = root.join("packages/remeda/test");
        fs::create_dir_all(&src_dir).expect("create src test dir");
        fs::create_dir_all(&test_dir).expect("create helper test dir");
        let importer = src_dir.join("isDefined.test.ts");
        let helper = test_dir.join("typesDataProvider.ts");
        fs::write(
            &importer,
            r#"
                import {
                  ALL_TYPES_DATA_PROVIDER,
                  TYPES_DATA_PROVIDER,
                } from "../test/typesDataProvider";
                export const value = ALL_TYPES_DATA_PROVIDER;
            "#,
        )
        .expect("write importer");
        fs::write(&helper, "export const ALL_TYPES_DATA_PROVIDER = [];\n").expect("write helper");

        let roots = vec![read_manifest_source(importer.clone()).expect("read importer")];
        let sources = dependency_closure(roots).expect("collect closure");
        let ordered = order_manifest_sources(&sources).expect("order sources");
        let ordered_paths = ordered
            .into_iter()
            .map(|index| sources[index].path.clone())
            .collect::<Vec<_>>();

        assert!(
            ordered_paths
                .iter()
                .position(|path| normalize_path_key(path) == normalize_path_key(&helper))
                < ordered_paths
                    .iter()
                    .position(|path| normalize_path_key(path) == normalize_path_key(&importer))
        );

        drop(fs::remove_dir_all(root));
    }

    /// NodeNext output extensions map to their TypeScript inputs.
    #[test]
    fn node_next_specifiers_map_to_typescript_inputs() {
        assert_eq!(node_next_input_extensions("js"), &["ts", "d.ts"]);
        assert_eq!(node_next_input_extensions("mjs"), &["mts", "d.mts"]);
        assert_eq!(node_next_input_extensions("cjs"), &["cts", "d.cts"]);
        assert!(node_next_input_extensions("ts").is_empty());
        assert!(node_next_input_extensions("json").is_empty());
    }

    /// A `./x.js` specifier proposes `./x.ts` as a candidate source.
    #[test]
    fn candidates_include_node_next_inputs() {
        let candidates = manifest_import_candidates(Path::new("/tmp/pkg/app.js"));
        assert!(candidates.contains(&PathBuf::from("/tmp/pkg/app.ts")));
        assert!(candidates.contains(&PathBuf::from("/tmp/pkg/app.d.ts")));
    }

    /// A NodeNext `./dep.js` import resolves to `dep.ts` in the dependency closure.
    #[test]
    fn collects_node_next_javascript_specifier_dependencies() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("smelt_manifest_nodenext_{unique}"));
        let src_dir = root.join("src");
        fs::create_dir_all(&src_dir).expect("create src test dir");
        let importer = src_dir.join("main.ts");
        let dependency = src_dir.join("app.ts");
        fs::write(
            &importer,
            "import { createApp } from './app.js';\nexport const app = createApp();\n",
        )
        .expect("write importer");
        fs::write(&dependency, "export const createApp = () => 1;\n").expect("write dependency");

        let roots = vec![read_manifest_source(importer.clone()).expect("read importer")];
        let sources = dependency_closure(roots).expect("collect closure");

        assert!(
            sources
                .iter()
                .any(|source| normalize_path_key(&source.path)
                    == normalize_path_key(&dependency)),
            "`./app.js` should resolve to app.ts, got {:?}",
            sources.iter().map(|source| source.path.clone()).collect::<Vec<_>>()
        );

        drop(fs::remove_dir_all(root));
    }
}
