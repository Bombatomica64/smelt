mod cli_parser;
mod config;
mod config_parser;

use clap::Parser;
use cli_parser::{Args, Command};
use config::{Config, Pipeline};
use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::{FileId, ModuleId, format_compact};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command as ProcessCommand,
};

fn check(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    for pipeline in config.pipelines() {
        match pipeline {
            Pipeline::TypeScript => todo!("oxclint"),
            Pipeline::Python => todo!("ty"),
        }
    }
    Ok(())
}

fn lower_typescript_files(
    files: &[String],
) -> Result<(smelt_hir::Crate, Vec<(String, ModuleId)>), Box<dyn std::error::Error>> {
    let mut ctx = HirCtx::new();
    let mut modules = Vec::new();

    for (idx, file) in files.iter().enumerate() {
        let source = fs::read_to_string(file)?;
        let module = to_hir(&source, FileId(idx as u32), &mut ctx).map_err(|errors| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("{}:\n{errors:#?}", Path::new(file).display()),
            )
        })?;
        modules.push((file.clone(), module));
    }

    Ok((ctx.krate, modules))
}

fn lower_manifest_typescript(
    config: &Config,
    manifest_path: &Path,
) -> Result<(smelt_hir::Crate, Vec<(String, ModuleId)>), Box<dyn std::error::Error>> {
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

fn resolve_manifest_path(manifest_dir: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        manifest_dir.join(path)
    }
}

fn print_hir(krate: &smelt_hir::Crate, modules: &[(String, ModuleId)], debug: bool) {
    if debug {
        println!("{krate:#?}");
    } else {
        print!("{}", format_compact(krate, modules));
    }
}

fn print_mir(krate: &smelt_hir::Crate) -> Result<(), Box<dyn std::error::Error>> {
    let mir = lower_to_optimized_mir(krate)?;
    print!("{}", smelt_mir::format_compact(&mir));
    Ok(())
}

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if let Command::DumpSchema = args.command {
        let schema = schemars::schema_for!(config::Config);
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
        Command::New { name, python } => todo!("new {name} python={python}"),
        Command::DumpHir { file, debug } => {
            let (krate, modules) = lower_typescript_files(&[file])?;
            print_hir(&krate, &modules, debug);
        }
        Command::DumpMir { file } => {
            let (krate, _) = lower_typescript_files(&[file])?;
            print_mir(&krate)?;
        }
        Command::Clean => todo!("clean"),
        Command::DumpSchema => unreachable!(),
    }
    Ok(())
}
