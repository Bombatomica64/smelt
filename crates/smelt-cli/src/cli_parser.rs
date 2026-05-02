use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "smelt",
    version,
    about = "Smelt your TypeScript and Python into Rust"
)]
pub struct Args {
    /// Path to Smelt.toml (defaults to ./)
    #[arg(long, global = true)]
    pub manifest_path: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Read manifest, transpile, and emit a Rust crate
    Build {
        /// Print compact HIR after frontend lowering and stop
        #[arg(long)]
        hir: bool,

        /// Print full debug HIR after frontend lowering and stop
        #[arg(long = "hir-debug")]
        hir_debug: bool,
    },
    /// Type-check and validate without emitting any output
    Check,
    /// Scaffold a new smelt project
    New {
        /// Project name
        name: String,

        /// Use Python as the entry language instead of TypeScript
        #[arg(long)]
        python: bool,
    },

    /// Print compact HIR for a single source file
    DumpHir {
        /// Print the full debug representation instead of compact HIR
        #[arg(long)]
        debug: bool,

        file: String,
    },

    /// Print the MIR for a single source file (debug)
    DumpMir { file: String },

    /// Print the raw parser AST for a single source file (Python only for now)
    DumpAst { file: String },

    /// Remove the output target directory
    Clean,
    /// Print the JSON Schema for Smelt.toml
    DumpSchema,
}
