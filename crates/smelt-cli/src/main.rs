mod cli_parser;
mod config;
mod config_parser;

use clap::Parser;
use cli_parser::{Args, Command};
use config::{Config, Pipeline};

fn check(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    for pipeline in config.pipelines() {
        match pipeline {
            Pipeline::TypeScript => todo!("oxclint"),
            Pipeline::Python => todo!("ty"),
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
    let config = config_parser::parse(&manifest_path)?;
    match args.command {
        Command::Check => check(&config)?,
        Command::Build => {
            check(&config)?;
            todo!("codegen")
        }
        Command::New { name, python } => todo!("new {name} python={python}"),
        Command::DumpHir { file } => todo!("dump-hir {file}"),
        Command::DumpMir { file } => todo!("dump-mir {file}"),
        Command::Clean => todo!("clean"),
        Command::DumpSchema => unreachable!(),
    }
    Ok(())
}
