//! CLI argument parsing and command definitions.

use clap::{Parser, Subcommand};

/// Top-level CLI arguments for the smelt command.
#[derive(Parser, Debug)]
#[command(
    name = "smelt",
    version,
    about = "Smelt your TypeScript and Python into Rust",
    long_about = "Smelt transpiles TypeScript and Python into a Rust crate.\n\n\
                  Beyond `build` and `check`, the tooling subcommands drive the \
                  porting campaign: `probe` reports how far a project transpiles \
                  and groups the remaining blockers; `rust-diagnostics` and \
                  `rust-test-report` summarize a generated crate's `cargo \
                  check`/`cargo test` output as stable Markdown for hand-off."
)]
pub struct Args {
    /// Path to Smelt.toml (defaults to `./Smelt.toml`).
    #[arg(long, global = true, value_name = "PATH")]
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
    #[command(long_about = "Run `cargo check --message-format=json` for a \
        generated crate and render a Markdown report. Diagnostics are grouped by \
        rustc code and first message line and sorted by count (desc), then \
        level, then code, so reports diff cleanly between runs and the biggest \
        warning/error classes appear first.")]
    RustDiagnostics {
        /// Path to the generated crate `Cargo.toml`.
        #[arg(long = "cargo-manifest", value_name = "PATH")]
        cargo_manifest: String,

        /// Write the Markdown report to this file instead of stdout.
        #[arg(long, value_name = "PATH")]
        output: Option<String>,
    },

    /// Run generated Rust tests and write a compact Markdown investigation report
    #[command(long_about = "Run focused, guarded, and/or full generated `cargo \
        test` runs and render a compact Markdown report. Use `--focus` to drill \
        into a failing family, `--guard` to prove a prior fix still passes, and \
        `--full` for the whole-suite inventory and largest failing groups. \
        Requires at least one of `--focus`, `--guard`, or `--full`.")]
    RustTestReport {
        /// Path to the generated Rust crate `Cargo.toml`.
        #[arg(long = "cargo-manifest", value_name = "PATH")]
        cargo_manifest: String,

        /// Smelt.toml to transpile/build before running the generated tests.
        #[arg(long = "build-manifest", value_name = "PATH")]
        build_manifest: Option<String>,

        /// Test filter to investigate; repeat for independent focused runs.
        #[arg(long, value_name = "FILTER")]
        focus: Vec<String>,

        /// Regression test filter to protect while investigating; repeat as needed.
        #[arg(long, value_name = "FILTER")]
        guard: Vec<String>,

        /// Run the complete generated Rust test suite after focused runs.
        #[arg(long)]
        full: bool,

        /// Previous Markdown report used to compute resolved and newly failing tests.
        #[arg(long = "baseline-report", value_name = "PATH")]
        baseline_report: Option<String>,

        /// Include grouped `cargo check` diagnostics in the Markdown report.
        #[arg(long)]
        diagnostics: bool,

        /// Suppress generated Rust warnings (RUSTFLAGS=-Awarnings) while testing.
        #[arg(long)]
        suppress_warnings: bool,

        /// Write the Markdown report to this file instead of stdout.
        #[arg(long, value_name = "PATH")]
        output: Option<String>,
    },

    /// Probe how far a manifest transpiles and enumerate blockers by category
    #[command(long_about = "Attempt a whole-crate build, then lower every source \
        file in isolation and group the remaining blockers. On success, optionally \
        run the generated test suite (`--run-tests`). Blocker classes are sorted \
        by occurrences (desc), then affected files, then class name; long `oxc` \
        AST dumps in messages are elided to a short class key in the table, with \
        the full text kept in a collapsible detail section.")]
    Probe {
        /// Also run the generated `cargo test` suite when the crate transpiles.
        #[arg(long = "run-tests")]
        run_tests: bool,

        /// Report format: `md` (default) or `json`.
        #[arg(long, default_value = "md", value_name = "FORMAT")]
        format: String,

        /// Write the report to this file instead of stdout.
        #[arg(long, value_name = "PATH")]
        output: Option<String>,
    },

    /// Remove the output target directory
    Clean,
    /// Print the JSON Schema for Smelt.toml
    DumpSchema,
}
