mod cli_parser;
mod config;
mod config_parser;

use clap::Parser;
use cli_parser::{Args, Command};
use config::{Config, Pipeline};
use smelt_frontend_ts::{HirCtx, to_hir};
use smelt_hir::{FileId, ModuleId, format_compact};
use std::{fs, path::Path};

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
) -> Result<(smelt_hir::Crate, Vec<(String, ModuleId)>), Box<dyn std::error::Error>> {
    let files = config
        .entries()
        .iter()
        .filter(|path| path.extension().is_some_and(|extension| extension == "ts"))
        .map(|path| path.display().to_string())
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

fn print_hir(krate: &smelt_hir::Crate, modules: &[(String, ModuleId)], debug: bool) {
    if debug {
        println!("{krate:#?}");
    } else {
        print!("{}", format_compact(krate, modules));
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if let Command::DumpSchema = args.command {
        let schema = schemars::schema_for!(config::Config);
        println!("{}", serde_json::to_string_pretty(&schema)?);
        return Ok(());
    }

    let manifest_path = args.manifest_path.unwrap_or("Smelt.toml".to_string());
    let config = config_parser::parse(&manifest_path)?;
    match args.command {
        Command::Check => check(&config)?,
        Command::Build { hir, hir_debug } => {
            if hir || hir_debug {
                let (krate, modules) = lower_manifest_typescript(&config)?;
                print_hir(&krate, &modules, hir_debug);
                return Ok(());
            }
            check(&config)?;
            todo!("codegen")
        }
        Command::New { name, python } => todo!("new {name} python={python}"),
        Command::DumpHir { file, debug } => {
            let (krate, modules) = lower_typescript_files(&[file])?;
            print_hir(&krate, &modules, debug);
        }
        Command::DumpMir { file } => todo!("dump-mir {file}"),
        Command::Clean => todo!("clean"),
        Command::DumpSchema => unreachable!(),
    }
    Ok(())
}
