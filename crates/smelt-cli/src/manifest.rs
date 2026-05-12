//! Manifest source graph helpers.
//!
//! This module collects import edges from manifest entries and computes
//! dependency-first ordering for multi-file lowering.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use crate::lowering::SourceLang;

/// A manifest source entry prepared for module graph ordering.
#[derive(Debug, Clone)]
pub(crate) struct ManifestSource {
    /// Original manifest entry resolved against the manifest directory.
    pub(crate) path: PathBuf,
    /// Source language inferred from the file extension.
    lang: SourceLang,
    /// Import specifiers found before lowering.
    imports: Vec<String>,
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
    Ok(ManifestSource {
        path,
        lang,
        imports: scan_imports(&source, lang),
    })
}

/// Expands root manifest entries with local imports discovered from each source.
pub(crate) fn dependency_closure(
    roots: Vec<ManifestSource>,
) -> Result<Vec<ManifestSource>, Box<dyn std::error::Error>> {
    let mut sources = Vec::new();
    let mut seen = HashSet::new();
    for root in roots {
        collect_manifest_source(root, &mut sources, &mut seen)?;
    }
    Ok(sources)
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

/// Adds one source and recursively adds local import targets that exist on disk.
fn collect_manifest_source(
    source: ManifestSource,
    sources: &mut Vec<ManifestSource>,
    seen: &mut HashSet<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let key = normalize_path_key(&source.path);
    if !seen.insert(key) {
        return Ok(());
    }
    let imports = source.imports.clone();
    let source_path = source.path.clone();
    let lang = source.lang;
    sources.push(source);
    for import in imports {
        if let Some(path) = resolve_import_to_existing_source(&source_path, lang, &import)? {
            let dep = read_manifest_source(path)?;
            collect_manifest_source(dep, sources, seen)?;
        }
    }
    Ok(())
}

/// Resolves a local import specifier to an existing source file.
fn resolve_import_to_existing_source(
    importer_path: &Path,
    importer_lang: SourceLang,
    import: &str,
) -> Result<Option<PathBuf>, Box<dyn std::error::Error>> {
    let base = match importer_lang {
        SourceLang::TypeScript if import.starts_with('.') => importer_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(import),
        SourceLang::Python if import.starts_with('.') => importer_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(import.trim_start_matches('.').replace('.', "/")),
        SourceLang::Python => PathBuf::from(import.replace('.', "/")),
        SourceLang::TypeScript => return Ok(None),
    };
    for candidate in manifest_import_candidates(&base) {
        if candidate.is_file() {
            return Ok(Some(candidate.canonicalize()?));
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
        return Err(format!(
            "cyclic manifest import involving {}",
            visit.sources.get(idx).map_or_else(
                || "<unknown>".to_owned(),
                |source| source.path.display().to_string()
            )
        ));
    }

    let Some(source) = visit.sources.get(idx) else {
        return Ok(());
    };
    for import in &source.imports {
        if let Some(dep) = resolve_import_to_manifest_source(source, import, visit.index)
            && dep != idx
        {
            visit_manifest_source(dep, visit)?;
        }
    }

    visit.temporary.remove(&idx);
    visit.permanent.insert(idx);
    visit.ordered.push(idx);
    Ok(())
}

/// Resolves an import specifier to a known manifest source when it is local.
fn resolve_import_to_manifest_source(
    importer: &ManifestSource,
    import: &str,
    index: &HashMap<PathBuf, usize>,
) -> Option<usize> {
    let base = match importer.lang {
        SourceLang::TypeScript if import.starts_with('.') => importer
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(import),
        SourceLang::Python if import.starts_with('.') => importer
            .path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(import.trim_start_matches('.').replace('.', "/")),
        SourceLang::Python => PathBuf::from(import.replace('.', "/")),
        SourceLang::TypeScript => PathBuf::from(import),
    };

    manifest_import_candidates(&base)
        .into_iter()
        .find_map(|candidate| index.get(&normalize_path_key(&candidate)).copied())
}

/// Builds possible source paths for an import specifier.
fn manifest_import_candidates(base: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    candidates.push(base.to_path_buf());
    if base.extension().is_none() {
        candidates.push(base.with_extension("ts"));
        candidates.push(base.with_extension("py"));
        candidates.push(base.join("index.ts"));
        candidates.push(base.join("__init__.py"));
    }
    candidates
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
fn scan_imports(source: &str, lang: SourceLang) -> Vec<String> {
    match lang {
        SourceLang::TypeScript => scan_typescript_imports(source),
        SourceLang::Python => scan_python_imports(source),
    }
}

/// Scans TypeScript import and re-export module specifiers.
fn scan_typescript_imports(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("import type ") || trimmed.starts_with("export type ") {
                return None;
            }
            if trimmed.starts_with("import ") {
                let specifier = trimmed
                    .split_once(" from ")
                    .map_or(trimmed.strip_prefix("import "), |(_, right)| Some(right))?;
                return quoted_module_specifier(specifier);
            }
            if trimmed.starts_with("export ") && trimmed.contains(" from ") {
                let (_, right) = trimmed.split_once(" from ")?;
                return quoted_module_specifier(right);
            }
            None
        })
        .collect()
}

/// Scans Python `import module` and `from module import name` specifiers.
fn scan_python_imports(source: &str) -> Vec<String> {
    let mut imports = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("from ") {
            if let Some((module, _)) = rest.split_once(" import ") {
                imports.push(module.trim().to_owned());
            }
        } else if let Some(rest) = trimmed.strip_prefix("import ") {
            let first_module = rest
                .split(',')
                .next()
                .and_then(|part| part.split_whitespace().next());
            if let Some(module_name) = first_module {
                imports.push(module_name.to_owned());
            }
        }
    }
    imports
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
