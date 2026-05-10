//! Main entry point and orchestration logic for the smelt CLI tool.
//!
//! This module handles parsing arguments, loading configuration, and dispatching
//! to the appropriate frontend and codegen pipelines.

#![expect(
    clippy::type_complexity,
    reason = "CLI command helpers currently use boxed dynamic errors directly"
)]
#![expect(
    clippy::str_to_string,
    reason = "CLI string conversion style will be normalized separately"
)]
#![expect(
    clippy::or_fun_call,
    reason = "manifest default construction is not performance-sensitive"
)]
#![expect(
    clippy::missing_const_for_fn,
    reason = "configuration accessors use non-const Option helpers on current MSRV"
)]
#![expect(
    clippy::exhaustive_enums,
    reason = "CLI command and config enums are internal to the binary crate"
)]
#![expect(
    clippy::exhaustive_structs,
    reason = "CLI parser structs are internal clap data models"
)]
#![expect(
    clippy::use_debug,
    reason = "dump commands intentionally expose debug forms for development inspection"
)]

pub mod cli_parser;
pub mod config;
pub mod config_parser;
pub mod stubs;

use clap::Parser;
use cli_parser::{Args, Command};
use config::Config;
use smelt_hir::{FileId, ModuleId, format_compact};
use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

/// Source language inferred from a file path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceLang {
    /// TypeScript source file.
    TypeScript,
    /// Python source file.
    Python,
}

impl SourceLang {
    /// Infer source language from file extension.
    fn from_path(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        match Path::new(path).extension().and_then(|e| e.to_str()) {
            Some("ts") => Ok(Self::TypeScript),
            Some("py") => Ok(Self::Python),
            _ => Err(format!("unsupported source extension: {path}").into()),
        }
    }
}

/// Represents a lowered crate with its modules.
type LoweredCrate = (smelt_hir::Crate, Vec<(String, ModuleId)>);

/// A manifest source entry prepared for module graph ordering.
#[derive(Debug, Clone)]
struct ManifestSource {
    /// Original manifest entry resolved against the manifest directory.
    path: PathBuf,
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

/// Lower TypeScript source files to HIR.
fn lower_typescript_files(files: &[String]) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let mut ctx = smelt_frontend_ts::HirCtx::new();
    let mut modules = Vec::new();

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

/// Lower Python source files to HIR.
fn lower_python_files(files: &[String]) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let mut ctx = smelt_frontend_py::HirCtx::new();
    let mut modules = Vec::new();

    for (idx, file) in files.iter().enumerate() {
        let source = fs::read_to_string(file)?;
        let file_id = u32::try_from(idx).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("too many source files: {error}"),
            )
        })?;
        let module = smelt_frontend_py::to_hir_with_path(&source, FileId(file_id), file, &mut ctx)
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

/// Dispatch a single source file to the right frontend based on its extension.
fn lower_single_file(file: &str) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    match SourceLang::from_path(file)? {
        SourceLang::TypeScript => lower_typescript_files(&[file.to_string()]),
        SourceLang::Python => lower_python_files(&[file.to_string()]),
    }
}

/// Lower manifest entries in order, sharing one HIR crate across frontends.
fn lower_manifest_entries(
    config: &Config,
    manifest_path: &Path,
) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let sources = config
        .entries()
        .iter()
        .map(|path| resolve_manifest_path(manifest_dir, path))
        .map(read_manifest_source)
        .collect::<Result<Vec<_>, _>>()?;

    let files = order_manifest_sources(&sources)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
        .into_iter()
        .filter_map(|idx| sources.get(idx).map(|source| source.path.clone()))
        .collect::<Vec<_>>();

    if files.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "manifest has no source entries to lower",
        )
        .into());
    }

    lower_ordered_manifest_files(&files)
}

/// Lower already ordered manifest files into one shared HIR crate.
fn lower_ordered_manifest_files(
    files: &[PathBuf],
) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let mut krate = smelt_hir::Crate::new();
    let mut modules = Vec::new();

    for (idx, file) in files.iter().enumerate() {
        let (next_krate, module) = lower_manifest_file(krate, file, idx)?;
        krate = next_krate;
        if let Ok(module_idx) = usize::try_from(module.0)
            && let Some(module_value) = krate.modules.get_mut(module_idx)
        {
            module_value.name = manifest_module_name(file);
        }
        modules.push((file.display().to_string(), module));
    }

    Ok((krate, modules))
}

/// Lower one manifest file with the language-specific frontend.
fn lower_manifest_file(
    krate: smelt_hir::Crate,
    file: &Path,
    idx: usize,
) -> Result<(smelt_hir::Crate, ModuleId), Box<dyn std::error::Error>> {
    let file_string = file.display().to_string();
    let source = fs::read_to_string(file)?;
    match SourceLang::from_path(&file_string)? {
        SourceLang::TypeScript => {
            let mut ctx = smelt_frontend_ts::HirCtx { krate };
            let module = smelt_frontend_ts::to_hir_with_path(
                &source,
                FileId(u32::try_from(idx).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("too many source files: {error}"),
                    )
                })?),
                &file_string,
                &mut ctx,
            )
            .map_err(|errors| lowering_error(file, &errors))?;
            Ok((ctx.krate, module))
        }
        SourceLang::Python => {
            let mut ctx = smelt_frontend_py::HirCtx { krate };
            let module = smelt_frontend_py::to_hir_with_path(
                &source,
                FileId(u32::try_from(idx).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("too many source files: {error}"),
                    )
                })?),
                &file_string,
                &mut ctx,
            )
            .map_err(|errors| lowering_error(file, &errors))?;
            Ok((ctx.krate, module))
        }
    }
}

/// Format frontend lowering errors with the source path.
fn lowering_error(file: &Path, errors: &[impl std::fmt::Debug]) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{}:\n{errors:#?}", file.display()),
    )
}

/// Read a manifest entry and collect the imports needed for dependency sorting.
fn read_manifest_source(path: PathBuf) -> Result<ManifestSource, Box<dyn std::error::Error>> {
    let file_string = path.display().to_string();
    let lang = SourceLang::from_path(&file_string)?;
    let source = fs::read_to_string(&path)?;
    Ok(ManifestSource {
        path,
        lang,
        imports: scan_imports(&source, lang),
    })
}

/// Return manifest source indexes in dependency-first order.
fn order_manifest_sources(sources: &[ManifestSource]) -> Result<Vec<usize>, String> {
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

/// Build lookup keys that import specifiers can resolve against.
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

/// Add canonical and extensionless keys for one manifest path.
fn add_manifest_key(index: &mut HashMap<PathBuf, usize>, path: &Path, idx: usize) {
    index.insert(normalize_path_key(path), idx);
    if let Some(without_extension) = path.with_extension("").to_str() {
        index.insert(normalize_path_key(Path::new(without_extension)), idx);
    }
}

/// Visit a manifest source and all known local dependencies.
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

/// Resolve an import specifier to a known manifest source when it is local.
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

/// Build possible source paths for an import specifier.
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

/// Normalize a path key without requiring the path to already exist.
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

/// Scan TypeScript and Python source text for import module specifiers.
fn scan_imports(source: &str, lang: SourceLang) -> Vec<String> {
    match lang {
        SourceLang::TypeScript => scan_typescript_imports(source),
        SourceLang::Python => scan_python_imports(source),
    }
}

/// Scan TypeScript `import ... from "module"` and side-effect import specifiers.
fn scan_typescript_imports(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with("import ") {
                return None;
            }
            let specifier = trimmed
                .split_once(" from ")
                .map_or(trimmed.strip_prefix("import "), |(_, right)| Some(right))?;
            quoted_module_specifier(specifier)
        })
        .collect()
}

/// Scan Python `import module` and `from module import name` specifiers.
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

/// Extract the first quoted module specifier from an import tail.
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

/// Parse a Python file and dump the Ruff AST. Used for the M8 scaffold while
/// HIR lowering is still incomplete — lets the user verify parsing works
/// end-to-end via the CLI without needing a fully-populated HIR.
fn dump_python_ast(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(file)?;
    let module = smelt_frontend_py::parse_module(&source, FileId(0)).map_err(|errors| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}:\n{errors:#?}", Path::new(file).display()),
        )
    })?;
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{module:#?}")?;
    Ok(())
}

/// Resolve a path relative to the manifest directory, or return it if absolute.
fn resolve_manifest_path(manifest_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        manifest_dir.join(path)
    }
}

/// Return the generated Rust function name for a manifest module body.
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

/// Print the HIR in compact or debug format.
fn print_hir(krate: &smelt_hir::Crate, modules: &[(String, ModuleId)], debug: bool) {
    let mut stdout = io::stdout().lock();
    if debug {
        let _ignored = writeln!(stdout, "{krate:#?}");
    } else {
        let _ignored = write!(stdout, "{}", format_compact(krate, modules));
    }
}

/// Print the optimized MIR in compact format.
fn print_mir(krate: &smelt_hir::Crate) -> Result<(), Box<dyn std::error::Error>> {
    let mir = lower_to_optimized_mir(krate)?;
    let mut stdout = io::stdout().lock();
    write!(stdout, "{}", smelt_mir::format_compact(&mir))?;
    Ok(())
}

/// Lower HIR to MIR, optimize, and validate.
fn lower_to_optimized_mir(
    krate: &smelt_hir::Crate,
) -> Result<smelt_mir::Mir, Box<dyn std::error::Error>> {
    let mut mir = smelt_mir::lower_hir(krate).map_err(|errors| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("MIR lowering failed:\n{errors:#?}"),
        )
    })?;
    smelt_mir::opt::optimize(&mut mir);
    let validation_errors = smelt_mir::validate(&mir);
    if !validation_errors.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("MIR validation failed:\n{validation_errors:#?}"),
        )
        .into());
    }
    Ok(mir)
}

/// Build a Rust crate from manifest configuration.
fn build_rust_crate(
    config: &Config,
    manifest_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let (krate, modules) = lower_manifest_entries(config, manifest_path)?;
    stubs::emit_type_declarations(&krate, &modules)?;
    let mir = lower_to_optimized_mir(&krate)?;
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let output_dir = resolve_manifest_path(manifest_dir, config.output_target());
    let crate_name = config
        .output_crate_name()
        .unwrap_or_else(|| config.project_name())
        .replace('-', "_");
    smelt_codegen_rust::emit_crate(
        &mir,
        &output_dir,
        &smelt_codegen_rust::EmitOptions::new(crate_name),
    )?;

    if config.should_build_output() {
        let output = ProcessCommand::new("cargo")
            .arg("build")
            .current_dir(&output_dir)
            .output()?;
        if !output.status.success() {
            return Err(io::Error::other(format!(
                "generated crate failed to build\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
    }

    Ok(())
}

/// Check manifest entries and emit declaration stubs without generating Rust.
fn check_manifest(config: &Config, manifest_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let (krate, modules) = lower_manifest_entries(config, manifest_path)?;
    stubs::emit_type_declarations(&krate, &modules)?;
    Ok(())
}

/// Main CLI entry point.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if matches!(args.command, Command::DumpSchema) {
        let schema = schemars::schema_for!(Config);
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{}", serde_json::to_string_pretty(&schema)?)?;
        return Ok(());
    }

    let manifest_path_string = args.manifest_path.unwrap_or("Smelt.toml".to_string());
    let manifest_path = PathBuf::from(manifest_path_string);
    let config = config_parser::parse(
        manifest_path
            .to_str()
            .ok_or("manifest path contains invalid UTF-8")?,
    )?;
    match args.command {
        Command::Check => check_manifest(&config, &manifest_path)?,
        Command::Build { hir, hir_debug } => {
            if hir || hir_debug {
                let (krate, modules) = lower_manifest_entries(&config, &manifest_path)?;
                print_hir(&krate, &modules, hir_debug);
                return Ok(());
            }
            build_rust_crate(&config, &manifest_path)?;
        }
        Command::New { name, python } => {
            return Err(format!(
                "`smelt new` is not implemented yet: name={name}, python={python}"
            )
            .into());
        }
        Command::DumpHir { file, debug } => {
            let (krate, modules) = lower_single_file(&file)?;
            print_hir(&krate, &modules, debug);
        }
        Command::DumpMir { file } => {
            let (krate, _) = lower_single_file(&file)?;
            print_mir(&krate)?;
        }
        Command::DumpAst { file } => {
            // Currently Python-only; TS already roundtrips through HIR cleanly.
            match SourceLang::from_path(&file)? {
                SourceLang::Python => dump_python_ast(&file)?,
                SourceLang::TypeScript => {
                    return Err("--dump-ast is only supported for .py files; \
                                use `smelt dump-hir --debug` for TypeScript"
                        .into());
                }
            }
        }
        Command::Clean => return Err("`smelt clean` is not implemented yet".into()),
        Command::DumpSchema => return Ok(()),
    }
    Ok(())
}
