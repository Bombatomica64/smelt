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
#![allow(
    clippy::redundant_pub_crate,
    reason = "CLI submodules use crate-visible helpers even though the binary crate root keeps modules private"
)]
#![allow(
    clippy::too_many_lines,
    reason = "manifest lowering orchestration is intentionally kept in one helper during the active feature phase"
)]

pub mod cli_parser;
pub mod config;
pub mod config_parser;
mod lowering;
mod manifest;
mod pipeline;
pub mod stubs;

use std::{io, io::Write as _, path::PathBuf};

use clap::Parser;
use cli_parser::{Args, Command};
use config::Config;
use lowering::SourceLang;

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
        Command::Check => pipeline::check_manifest(&config, &manifest_path)?,
        Command::Build { hir, hir_debug } => {
            if hir || hir_debug {
                let (krate, modules) = lowering::lower_manifest_entries(&config, &manifest_path)?;
                pipeline::print_hir(&krate, &modules, hir_debug);
                return Ok(());
            }
            pipeline::build_rust_crate(&config, &manifest_path)?;
        }
        Command::New { name, python } => {
            return Err(format!(
                "`smelt new` is not implemented yet: name={name}, python={python}"
            )
            .into());
        }
        Command::DumpHir { file, debug } => {
            let (krate, modules) = lowering::lower_single_file(&file)?;
            pipeline::print_hir(&krate, &modules, debug);
        }
        Command::DumpMir { file } => {
            let (krate, _) = lowering::lower_single_file(&file)?;
            pipeline::print_mir(&krate)?;
        }
        Command::DumpAst { file } => match SourceLang::from_path(&file)? {
            SourceLang::Python => pipeline::dump_python_ast(&file)?,
            SourceLang::TypeScript => {
                return Err("--dump-ast is only supported for .py files; \
                                use `smelt dump-hir --debug` for TypeScript"
                    .into());
            }
        },
        Command::Clean => return Err("`smelt clean` is not implemented yet".into()),
        Command::DumpSchema => return Ok(()),
    }
    Ok(())
}
