mod cli_parser;
mod config;
mod config_parser;

use clap::Parser;
use cli_parser::{Args, Command};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    if let Command::DumpSchema = args.command {
        let schema = schemars::schema_for!(config::Config);
        println!("{}", serde_json::to_string_pretty(&schema)?);
        return Ok(());
    }

    let manifest_path = args.manifest_path.unwrap_or("Smelt.toml".to_string());
    let config = config_parser::parse(&manifest_path)?;
    println!("config {:#?}", config);
    match args.command {
        Command::Build => todo!("build"),
        Command::Check => todo!("check"),
        Command::New { name, python } => todo!("new {name} python={python}"),
        Command::DumpHir { file } => todo!("dump-hir {file}"),
        Command::DumpMir { file } => todo!("dump-mir {file}"),
        Command::Clean => todo!("clean"),
        Command::DumpSchema => unreachable!(),
    }
}
