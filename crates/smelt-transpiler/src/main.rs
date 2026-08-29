//! Main entry point and orchestration logic for the smelt CLI tool.
//!
//! This module handles parsing arguments, loading configuration, and dispatching
//! to the appropriate frontend and codegen pipelines.

#![expect(
    clippy::type_complexity,
    reason = "CLI command helpers currently use boxed dynamic errors directly"
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
mod diagnostics;
mod list_escape_report;
mod lowering;
mod manifest;
mod pipeline;
mod probe;
mod python;
mod specialization;
pub mod stubs;
mod test_report;
mod timing;
mod unknown_report;

use std::{io, io::Write as _, path::PathBuf};

use clap::Parser;
use cli_parser::{Args, Command};
use config::Config;
use lowering::SourceLang;

/// Result type used by CLI command handlers.
type CliResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Main CLI entry point.
fn main() -> CliResult<()> {
    let args = Args::parse();

    if matches!(args.command, Command::DumpSchema) {
        let schema = schemars::schema_for!(Config);
        let mut stdout = io::stdout().lock();
        writeln!(stdout, "{}", serde_json::to_string_pretty(&schema)?)?;
        return Ok(());
    }
    if let Command::RustDiagnostics {
        cargo_manifest,
        output,
    } = &args.command
    {
        let report = diagnostics::rust_diagnostics_markdown(&PathBuf::from(cargo_manifest))?;
        if let Some(output_path) = output {
            std::fs::write(output_path, report)?;
        } else {
            let mut stdout = io::stdout().lock();
            write!(stdout, "{report}")?;
        }
        return Ok(());
    }
    if let Command::RustTestReport {
        cargo_manifest,
        build_manifest,
        focus,
        guard,
        full,
        baseline_report,
        diagnostics: include_diagnostics,
        suppress_warnings,
        output,
    } = &args.command
    {
        if let Some(source_manifest) = build_manifest {
            let path = PathBuf::from(source_manifest);
            let config = config_parser::parse(
                path.to_str()
                    .ok_or("build manifest path contains invalid UTF-8")?,
            )?;
            pipeline::build_rust_crate(&config, &path)?;
        }
        let report = test_report::rust_test_report_markdown(&test_report::RustTestReportOptions {
            cargo_manifest: &PathBuf::from(cargo_manifest),
            focus,
            guard,
            full: *full,
            baseline_report: baseline_report.as_deref().map(PathBuf::from),
            include_diagnostics: *include_diagnostics,
            suppress_warnings: *suppress_warnings,
        })?;
        if let Some(output_path) = output {
            std::fs::write(output_path, report)?;
        } else {
            let mut stdout = io::stdout().lock();
            write!(stdout, "{report}")?;
        }
        return Ok(());
    }

    if let Command::ListEscapeReport {
        manifest,
        format,
        function,
        top,
        output,
    } = &args.command
    {
        let report_format = match format.as_str() {
            "json" => list_escape_report::ListEscapeReportFormat::Json,
            "md" | "markdown" => list_escape_report::ListEscapeReportFormat::Markdown,
            other => {
                return Err(format!("unknown --format `{other}`; use `md` or `json`").into());
            }
        };
        let report =
            list_escape_report::list_escape_report(&list_escape_report::ListEscapeReportOptions {
                manifest: &PathBuf::from(manifest),
                format: report_format,
                functions: function,
                top: *top,
            })?;
        if let Some(output_path) = output {
            std::fs::write(output_path, &report)?;
        } else {
            let mut stdout = io::stdout().lock();
            write!(stdout, "{report}")?;
        }
        return Ok(());
    }

    if let Command::SmeltUnknownReport {
        roots,
        format,
        baseline,
        fail_on_regression,
        output,
    } = &args.command
    {
        let report_format = match format.as_str() {
            "json" => unknown_report::UnknownReportFormat::Json,
            "md" | "markdown" => unknown_report::UnknownReportFormat::Markdown,
            other => {
                return Err(format!("unknown --format `{other}`; use `md` or `json`").into());
            }
        };
        let root_paths = roots.iter().map(PathBuf::from).collect::<Vec<_>>();
        let outcome = unknown_report::unknown_report(&unknown_report::UnknownReportOptions {
            roots: &root_paths,
            format: report_format,
            baseline: baseline.as_deref().map(PathBuf::from),
            fail_on_regression: *fail_on_regression,
        })?;
        if let Some(output_path) = output {
            std::fs::write(output_path, &outcome.rendered)?;
        } else {
            let mut stdout = io::stdout().lock();
            write!(stdout, "{}", outcome.rendered)?;
        }
        // With `--fail-on-regression`, a rise in avoidable erasure above the
        // baseline exits nonzero; the message names the delta and the files that
        // contribute the most so reviewers know where to look first.
        if let Some(regression) = outcome.regression {
            use std::fmt::Write as _;
            let mut message = format!(
                "avoidable erasure regressed: {} > {} (baseline), +{} occurrence(s)",
                regression.current, regression.baseline, regression.delta,
            );
            if !regression.top_files.is_empty() {
                message.push_str("\ntop offending files:");
                for (file, count) in &regression.top_files {
                    let _ = write!(message, "\n  {count:>6}  {file}");
                }
            }
            message.push_str(
                "\nReclassify a genuine boundary (with a code comment + regression test) \
                 and re-snapshot the baseline, or reduce the erasure. See the \
                 `SmeltUnknown enforcement` policy in AGENTS.md.",
            );
            // Print the message verbatim and exit nonzero. Returning `Err` would
            // route through the default `Termination` impl, which Debug-prints
            // the string with escaped newlines — unreadable for a CI gate.
            let mut stderr = io::stderr().lock();
            writeln!(stderr, "{message}")?;
            std::process::exit(1);
        }
        return Ok(());
    }

    let manifest_path_string = args
        .manifest_path
        .unwrap_or_else(|| "Smelt.toml".to_owned());
    let manifest_path = PathBuf::from(manifest_path_string);
    let config = config_parser::parse(
        manifest_path
            .to_str()
            .ok_or("manifest path contains invalid UTF-8")?,
    )?;
    match args.command {
        Command::Check { message_format } => match message_format.as_str() {
            "human" => pipeline::check_manifest(&config, &manifest_path)?,
            "json" => {
                let diagnostics =
                    lowering::collect_manifest_diagnostics(&config, &manifest_path)?;
                let mut stdout = io::stdout().lock();
                writeln!(stdout, "{}", serde_json::to_string_pretty(&diagnostics)?)?;
            }
            other => {
                return Err(
                    format!("unknown --message-format `{other}`; use `human` or `json`").into(),
                );
            }
        },
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
            SourceLang::Python | SourceLang::PythonDeclaration => pipeline::dump_python_ast(&file)?,
            SourceLang::TypeScript | SourceLang::TypeScriptDeclaration => {
                return Err("--dump-ast is only supported for .py files; \
                                use `smelt dump-hir --debug` for TypeScript"
                    .into());
            }
        },
        Command::Probe {
            run_tests,
            format,
            output,
        } => {
            let probe_format = match format.as_str() {
                "json" => probe::ProbeFormat::Json,
                "md" | "markdown" => probe::ProbeFormat::Markdown,
                other => {
                    return Err(format!("unknown --format `{other}`; use `md` or `json`").into());
                }
            };
            let report = probe::probe_report(&probe::ProbeOptions {
                config: &config,
                manifest_path: &manifest_path,
                run_tests,
                format: probe_format,
            })?;
            if let Some(output_path) = output {
                std::fs::write(output_path, report)?;
            } else {
                let mut stdout = io::stdout().lock();
                write!(stdout, "{report}")?;
            }
        }
        Command::RustDiagnostics { .. }
        | Command::RustTestReport { .. }
        | Command::SmeltUnknownReport { .. }
        | Command::ListEscapeReport { .. }
        | Command::DumpSchema => {
            return Ok(());
        }
        Command::Clean => return Err("`smelt clean` is not implemented yet".into()),
    }
    Ok(())
}
