//! CLI argument parsing and command definitions.

use clap::{Parser, Subcommand};

/// Top-level CLI arguments for the smelt command.
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

    /// Command to execute.
    #[command(subcommand)]
    pub command: Command,
}

/// Available CLI subcommands.
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
    Check {
        /// Diagnostic output: `human` (default, fail-fast) or `json` (collect
        /// every module's categorized diagnostics in one recoverable pass).
        #[arg(long = "message-format", default_value = "human")]
        message_format: String,
    },
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

        /// Input source file path.
        file: String,
    },

    /// Print the MIR for a single source file (debug)
    DumpMir {
        /// Input source file path.
        file: String,
    },

    /// Print the raw parser AST for a single source file (Python only for now)
    DumpAst {
        /// Input source file path.
        file: String,
    },

    /// Run cargo check for a Rust crate and summarize diagnostics as Markdown
    RustDiagnostics {
        /// Path to the generated crate Cargo.toml.
        #[arg(long = "cargo-manifest")]
        cargo_manifest: String,

        /// Optional path to write the Markdown report instead of stdout.
        #[arg(long)]
        output: Option<String>,
    },

    /// Run generated Rust tests and write a compact Markdown investigation report
    RustTestReport {
        /// Path to the generated Rust crate Cargo.toml.
        #[arg(long = "cargo-manifest")]
        cargo_manifest: String,

        /// Optional Smelt.toml to build before running generated Rust tests.
        #[arg(long = "build-manifest")]
        build_manifest: Option<String>,

        /// Test filter to investigate; repeat for independent focused runs.
        #[arg(long)]
        focus: Vec<String>,

        /// Regression test filter to protect while investigating; repeat as needed.
        #[arg(long)]
        guard: Vec<String>,

        /// Run the complete generated Rust test suite after focused runs.
        #[arg(long)]
        full: bool,

        /// Previous Markdown report used to compute resolved and newly failing tests.
        #[arg(long = "baseline-report")]
        baseline_report: Option<String>,

        /// Include grouped `cargo check` diagnostics in the Markdown report.
        #[arg(long)]
        diagnostics: bool,

        /// Suppress generated Rust warnings while executing tests.
        #[arg(long)]
        suppress_warnings: bool,

        /// Optional path to write the Markdown report instead of stdout.
        #[arg(long)]
        output: Option<String>,
    },

    /// Probe how far a manifest transpiles and enumerate blockers by category
    Probe {
        /// Also run the generated `cargo test` suite when the crate transpiles.
        #[arg(long = "run-tests")]
        run_tests: bool,

        /// Report format: `md` (default) or `json`.
        #[arg(long, default_value = "md")]
        format: String,

        /// Optional path to write the report instead of stdout.
        #[arg(long)]
        output: Option<String>,
    },

    /// Remove the output target directory
    Clean,
    /// Print the JSON Schema for Smelt.toml
    DumpSchema,
}
