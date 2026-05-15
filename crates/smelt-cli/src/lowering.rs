//! Frontend lowering helpers for single files and mixed-language manifests.
//!
//! This module owns source-language detection plus HIR lowering entrypoints
//! used by CLI commands and manifest-driven builds.

use std::{collections::HashMap, fs, io, path::Path};

use smelt_hir::{FileId, ModuleId};

use crate::manifest::{
    ManifestSource, dependency_closure, order_manifest_sources, read_manifest_source,
    resolve_manifest_path,
};

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
    /// TypeScript exported object constants used as namespace-like APIs.
    ts_object_namespaces: HashMap<String, HashMap<String, smelt_hir::ItemId>>,
    /// TypeScript exported object constants with static literal values.
    ts_object_consts: HashMap<String, smelt_frontend_ts::ObjectConst>,
    /// TypeScript overload signatures visible across manifest entries.
    ts_overloads: HashMap<String, Vec<smelt_frontend_ts::OverloadSignature>>,
    /// TypeScript structural type-alias fields visible across manifest entries.
    ts_type_alias_fields: HashMap<smelt_hir::Symbol, Vec<smelt_hir::Field>>,
    /// TypeScript interface heritage edges visible across manifest entries.
    ts_interface_extends: HashMap<smelt_hir::Symbol, Vec<smelt_frontend_ts::InterfaceHeritageRef>>,
    /// TypeScript callable intersection fields visible across manifest entries.
    ts_callable_fields: HashMap<smelt_hir::TypeId, Vec<smelt_hir::Field>>,
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

/// Lowers Python source files to HIR.
pub(crate) fn lower_python_files(
    files: &[String],
) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
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

/// Dispatches one source file to the matching frontend.
pub(crate) fn lower_single_file(file: &str) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    match SourceLang::from_path(file)? {
        SourceLang::TypeScript | SourceLang::TypeScriptDeclaration => {
            lower_typescript_files(&[file.to_owned()])
        }
        SourceLang::Python | SourceLang::PythonDeclaration => {
            lower_python_files(&[file.to_owned()])
        }
    }
}

/// Lowers manifest entries in dependency order using one shared HIR crate.
pub(crate) fn lower_manifest_entries(
    config: &crate::config::Config,
    manifest_path: &Path,
) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let root_sources = config
        .entries()
        .iter()
        .map(|path| resolve_manifest_path(manifest_dir, path))
        .map(read_manifest_source)
        .collect::<Result<Vec<_>, _>>()?;
    let sources = dependency_closure(root_sources)?;

    let ordered_sources = order_manifest_sources(&sources)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?
        .into_iter()
        .filter_map(|idx| sources.get(idx))
        .collect::<Vec<_>>();

    if ordered_sources.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "manifest has no source entries to lower",
        )
        .into());
    }

    lower_ordered_manifest_sources(&ordered_sources)
}

/// Lowers already ordered manifest files into one shared HIR crate.
fn lower_ordered_manifest_sources(
    sources: &[&ManifestSource],
) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let mut krate = smelt_hir::Crate::new();
    let mut state = FrontendLoweringState::default();
    let mut modules = Vec::new();

    for (idx, source) in sources.iter().enumerate() {
        let (next_krate, module, next_state) = lower_manifest_source(krate, state, source, idx)?;
        krate = next_krate;
        state = next_state;
        let file = &source.path;
        if let Ok(module_idx) = usize::try_from(module.0)
            && let Some(module_value) = krate.modules.get_mut(module_idx)
        {
            module_value.name = manifest_module_name(file);
        }
        modules.push((file.display().to_string(), module));
    }

    Ok((krate, modules))
}

/// Lowers one manifest file with the language-specific frontend.
fn lower_manifest_source(
    krate: smelt_hir::Crate,
    state: FrontendLoweringState,
    source: &ManifestSource,
    idx: usize,
) -> Result<(smelt_hir::Crate, ModuleId, FrontendLoweringState), Box<dyn std::error::Error>> {
    let file = &source.path;
    let file_string = file.display().to_string();
    match SourceLang::from_path(&file_string)? {
        SourceLang::TypeScript | SourceLang::TypeScriptDeclaration => {
            let mut ctx = smelt_frontend_ts::HirCtx {
                krate,
                export_aliases: state.ts_export_aliases,
                object_namespaces: state.ts_object_namespaces,
                object_consts: state.ts_object_consts,
                overloads: state.ts_overloads,
                type_alias_fields: state.ts_type_alias_fields,
                interface_extends: state.ts_interface_extends,
                callable_fields: state.ts_callable_fields,
            };
            let module = smelt_frontend_ts::to_hir_with_path(
                &source.source,
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
            Ok((
                ctx.krate,
                module,
                FrontendLoweringState {
                    ts_export_aliases: ctx.export_aliases,
                    ts_object_namespaces: ctx.object_namespaces,
                    ts_object_consts: ctx.object_consts,
                    ts_overloads: ctx.overloads,
                    ts_type_alias_fields: ctx.type_alias_fields,
                    ts_interface_extends: ctx.interface_extends,
                    ts_callable_fields: ctx.callable_fields,
                    py_module_namespaces: state.py_module_namespaces,
                    py_enum_members: state.py_enum_members,
                },
            ))
        }
        SourceLang::Python | SourceLang::PythonDeclaration => {
            let mut ctx = smelt_frontend_py::HirCtx {
                krate,
                module_namespaces: state.py_module_namespaces,
                enum_members: state.py_enum_members,
            };
            let module = smelt_frontend_py::to_hir_with_path(
                &source.source,
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
            Ok((
                ctx.krate,
                module,
                FrontendLoweringState {
                    ts_export_aliases: state.ts_export_aliases,
                    ts_object_namespaces: state.ts_object_namespaces,
                    ts_object_consts: state.ts_object_consts,
                    ts_overloads: state.ts_overloads,
                    ts_type_alias_fields: state.ts_type_alias_fields,
                    ts_interface_extends: state.ts_interface_extends,
                    ts_callable_fields: state.ts_callable_fields,
                    py_module_namespaces: ctx.module_namespaces,
                    py_enum_members: ctx.enum_members,
                },
            ))
        }
    }
}

/// Formats frontend lowering errors with the source path.
fn lowering_error(file: &Path, errors: &[impl std::fmt::Debug]) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{}:\n{errors:#?}", file.display()),
    )
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
