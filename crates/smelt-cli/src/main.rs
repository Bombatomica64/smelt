//! Main entry point and orchestration logic for the smelt CLI tool.
//!
//! This module handles parsing arguments, loading configuration, and dispatching
//! to the appropriate frontend and codegen pipelines.

#![expect(
    clippy::type_complexity,
    reason = "CLI command helpers currently use boxed dynamic errors directly"
)]
#![expect(
    clippy::unnecessary_wraps,
    reason = "command handlers share a Result-returning shape for dispatch"
)]
#![expect(
    clippy::cast_possible_truncation,
    reason = "CLI file IDs are small indexes from manifest or command-line entries"
)]
#![expect(
    clippy::str_to_string,
    reason = "CLI string conversion style will be normalized separately"
)]
#![expect(
    clippy::match_like_matches_macro,
    reason = "command matching is kept explicit for future variants"
)]
#![expect(
    clippy::or_fun_call,
    reason = "manifest default construction is not performance-sensitive"
)]
#![expect(
    clippy::missing_const_for_fn,
    reason = "configuration accessors use non-const Option helpers on current MSRV"
)]

mod cli_parser;
mod config;
mod config_parser;

use clap::Parser;
use cli_parser::{Args, Command};
use config::{Config, Pipeline};
use smelt_hir::{FileId, ModuleId, format_compact};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

/// Source language inferred from a file path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceLang {
    TypeScript,
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

/// Check and validate source files without emitting any output.
fn check(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let pipelines = config.pipelines();
    if pipelines.contains(&Pipeline::TypeScript) {
        return Err("project-wide TypeScript check is not implemented yet".into());
    }
    if pipelines.contains(&Pipeline::Python) {
        return Err("project-wide Python check is not implemented yet".into());
    }
    Ok(())
}

/// Lower TypeScript source files to HIR.
fn lower_typescript_files(files: &[String]) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let mut ctx = smelt_frontend_ts::HirCtx::new();
    let mut modules = Vec::new();

    for (idx, file) in files.iter().enumerate() {
        let source = fs::read_to_string(file)?;
        let module =
            smelt_frontend_ts::to_hir(&source, FileId(idx as u32), &mut ctx).map_err(|errors| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
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
        let module =
            smelt_frontend_py::to_hir(&source, FileId(idx as u32), &mut ctx).map_err(|errors| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
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

/// Parse a Python file and dump the Ruff AST. Used for the M8 scaffold while
/// HIR lowering is still incomplete — lets the user verify parsing works
/// end-to-end via the CLI without needing a fully-populated HIR.
fn dump_python_ast(file: &str) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(file)?;
    let module = smelt_frontend_py::parse_module(&source, FileId(0)).map_err(|errors| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("{}:\n{errors:#?}", Path::new(file).display()),
        )
    })?;
    println!("{module:#?}");
    Ok(())
}

/// Lower TypeScript entries from a manifest config to HIR.
fn lower_manifest_typescript(
    config: &Config,
    manifest_path: &Path,
) -> Result<LoweredCrate, Box<dyn std::error::Error>> {
    let manifest_dir = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    let files = config
        .entries()
        .iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "ts"))
        .map(|path| {
            resolve_manifest_path(manifest_dir, path)
                .display()
                .to_string()
        })
        .collect::<Vec<_>>();

    if files.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "manifest has no TypeScript source entries to lower to HIR",
        )
        .into());
    }

    lower_typescript_files(&files)
}

/// Resolve a path relative to the manifest directory, or return it if absolute.
fn resolve_manifest_path(manifest_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        manifest_dir.join(path)
    }
}

/// Print the HIR in compact or debug format.
fn print_hir(krate: &smelt_hir::Crate, modules: &[(String, ModuleId)], debug: bool) {
    if debug {
        println!("{krate:#?}");
    } else {
        print!("{}", format_compact(krate, modules));
    }
}

/// Print the optimized MIR in compact format.
fn print_mir(krate: &smelt_hir::Crate) -> Result<(), Box<dyn std::error::Error>> {
    let mir = lower_to_optimized_mir(krate)?;
    print!("{}", smelt_mir::format_compact(&mir));
    Ok(())
}

/// Lower HIR to MIR, optimize, and validate.
fn lower_to_optimized_mir(
    krate: &smelt_hir::Crate,
) -> Result<smelt_mir::Mir, Box<dyn std::error::Error>> {
    let mut mir = smelt_mir::lower_hir(krate).map_err(|errors| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("MIR lowering failed:\n{errors:#?}"),
        )
    })?;
    smelt_mir::opt::optimize(&mut mir);
    let validation_errors = smelt_mir::validate(&mir);
    if !validation_errors.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
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
    let (krate, _) = lower_manifest_typescript(config, manifest_path)?;
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
        &smelt_codegen_rust::EmitOptions { crate_name },
    )?;

    if config.should_build_output() {
        let output = ProcessCommand::new("cargo")
            .arg("build")
            .current_dir(&output_dir)
            .output()?;
        if !output.status.success() {
            return Err(std::io::Error::other(format!(
                "generated crate failed to build\nstdout:\n{}\nstderr:\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ))
            .into());
        }
    }

    Ok(())
}

/// Main CLI entry point.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if matches!(args.command, Command::DumpSchema) {
        let schema = schemars::schema_for!(Config);
        println!("{}", serde_json::to_string_pretty(&schema)?);
        return Ok(());
    }

    let manifest_path = args.manifest_path.unwrap_or("Smelt.toml".to_string());
    let manifest_path = PathBuf::from(manifest_path);
    let config = config_parser::parse(
        manifest_path
            .to_str()
            .ok_or("manifest path contains invalid UTF-8")?,
    )?;
    match args.command {
        Command::Check => check(&config)?,
        Command::Build { hir, hir_debug } => {
            if hir || hir_debug {
                let (krate, modules) = lower_manifest_typescript(&config, &manifest_path)?;
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
        Command::DumpSchema => unreachable!(),
    }
    Ok(())
}
