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
mod erasure;
mod list_escape_report;
mod lowering;
mod manifest;
mod package_sources;
mod pipeline;
mod probe;
mod python;
mod sound;
mod sound_overlay;
mod specialization;
pub mod stubs;
mod test_report;
mod timing;
mod ts_facts;
#[cfg(feature = "ts7")]
mod ts7_parity;
#[cfg(feature = "ts7")]
mod ts7_parity_report;
mod typedness_report;
mod unknown_report;

use std::{io, io::Write as _, path::PathBuf};

use clap::Parser;
use cli_parser::{Args, Command, HirMode, MessageFormat, ReportFormat};
use config::Config;
use lowering::SourceLang;

/// Result type used by CLI command handlers.
type CliResult<T> = Result<T, Box<dyn std::error::Error>>;

/// Main CLI entry point: one dispatch over every subcommand.
fn main() -> CliResult<()> {
    let args = Args::parse();
    let manifest_path = PathBuf::from(args.manifest_path.unwrap_or_else(|| "Smelt.toml".to_owned()));
    match args.command {
        Command::DumpSchema => {
            let schema = schemars::schema_for!(Config);
            let mut stdout = io::stdout().lock();
            writeln!(stdout, "{}", serde_json::to_string_pretty(&schema)?)?;
        }
        Command::RustDiagnostics { cargo_manifest, output } => {
            let report = diagnostics::rust_diagnostics_markdown(&PathBuf::from(cargo_manifest))?;
            write_report(output.as_deref(), &report)?;
        }
        Command::RustTestReport {
            cargo_manifest,
            build_manifest,
            focus,
            guard,
            full,
            baseline_report,
            diagnostics: include_diagnostics,
            suppress_warnings,
            output,
        } => {
            if let Some(source_manifest) = build_manifest {
                let path = PathBuf::from(source_manifest);
                let config = load_config(&path)?;
                pipeline::build_rust_crate(&config, &path)?;
            }
            let report =
                test_report::rust_test_report_markdown(&test_report::RustTestReportOptions {
                    cargo_manifest: &PathBuf::from(cargo_manifest),
                    focus: &focus,
                    guard: &guard,
                    full,
                    baseline_report: baseline_report.as_deref().map(PathBuf::from),
                    include_diagnostics,
                    suppress_warnings,
                })?;
            write_report(output.as_deref(), &report)?;
        }
        Command::ListEscapeReport { manifest, format, function, top, output } => {
            let report = list_escape_report::list_escape_report(
                &list_escape_report::ListEscapeReportOptions {
                    manifest: &PathBuf::from(manifest),
                    format,
                    functions: &function,
                    top,
                },
            )?;
            write_report(output.as_deref(), &report)?;
        }
        Command::TypednessReport {
            paths,
            format,
            baseline,
            fail_on_regression,
            update_baseline,
            output,
        } => {
            let input_paths = paths.iter().map(PathBuf::from).collect::<Vec<_>>();
            let outcome =
                typedness_report::typedness_report(&typedness_report::TypednessReportOptions {
                    paths: &input_paths,
                    format,
                    baseline: baseline.as_deref().map(PathBuf::from),
                    update_baseline,
                })?;
            write_report(output.as_deref(), &outcome.rendered)?;
            if fail_on_regression {
                return gate(typedness_regression(&outcome));
            }
        }
        Command::Ts7ParityReport { inputs, format, output } => {
            run_ts7_parity_report(&inputs, format, output.as_deref())?;
        }
        Command::SmeltUnknownReport {
            roots,
            format,
            baseline,
            fail_on_regression,
            erasure: erasure_sidecars,
            output,
        } => {
            let root_paths = roots.iter().map(PathBuf::from).collect::<Vec<_>>();
            let sidecar_paths = erasure_sidecars.iter().map(PathBuf::from).collect::<Vec<_>>();
            let outcome = unknown_report::unknown_report(&unknown_report::UnknownReportOptions {
                roots: &root_paths,
                format,
                baseline: baseline.as_deref().map(PathBuf::from),
                fail_on_regression,
                erasure_sidecars: &sidecar_paths,
            })?;
            write_report(output.as_deref(), &outcome.rendered)?;
            // `outcome.regression` is only set under `--fail-on-regression`.
            return gate(outcome.regression.as_ref().map(unknown_regression_message));
        }
        Command::Check { message_format, erasure } => {
            let config = load_config(&manifest_path)?;
            let settings = erasure::ErasureSettings::new(config.erasure(), &erasure)?;
            match message_format {
                MessageFormat::Human => {
                    pipeline::check_manifest(&config, &manifest_path, &settings)?;
                }
                MessageFormat::Json => {
                    let diagnostics = lowering::collect_manifest_diagnostics(
                        &config,
                        &manifest_path,
                        Some(&settings),
                    )?;
                    let mut stdout = io::stdout().lock();
                    writeln!(stdout, "{}", serde_json::to_string_pretty(&diagnostics)?)?;
                }
            }
        }
        Command::Build { hir, define, erasure } => {
            let mut config = load_config(&manifest_path)?;
            config.extend_defines(define);
            if let Some(mode) = hir.mode() {
                let (krate, modules) = lowering::lower_manifest_entries(&config, &manifest_path)?;
                pipeline::print_hir(&krate, &modules, mode);
            } else {
                let settings = erasure::ErasureSettings::new(config.erasure(), &erasure)?;
                pipeline::build_rust_crate_with_erasure(&config, &manifest_path, &settings)?;
            }
        }
        Command::New { name, python } => {
            return Err(format!(
                "`smelt new` is not implemented yet: name={name}, python={python}"
            )
            .into());
        }
        Command::DumpHir { file, debug } => {
            let (krate, modules) = lowering::lower_single_file(&file)?;
            let mode = if debug { HirMode::Debug } else { HirMode::Compact };
            pipeline::print_hir(&krate, &modules, mode);
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
        Command::Probe { run_tests, format, output } => {
            let config = load_config(&manifest_path)?;
            let report = probe::probe_report(&probe::ProbeOptions {
                config: &config,
                manifest_path: &manifest_path,
                run_tests,
                format,
            })?;
            write_report(output.as_deref(), &report)?;
        }
        Command::Clean => return Err("`smelt clean` is not implemented yet".into()),
    }
    Ok(())
}

/// Parse the `Smelt.toml` at `path`.
fn load_config(path: &std::path::Path) -> CliResult<Config> {
    config_parser::parse(path.to_str().ok_or("manifest path contains invalid UTF-8")?)
}

/// Write a rendered report to `output`, or to stdout when none is given.
fn write_report(output: Option<&str>, report: &str) -> CliResult<()> {
    if let Some(output_path) = output {
        std::fs::write(output_path, report)?;
    } else {
        write!(io::stdout().lock(), "{report}")?;
    }
    Ok(())
}

/// Finish a gated report: print a failing gate's message verbatim and exit 1.
///
/// Returning `Err` instead would route through the default `Termination`
/// impl, which Debug-prints the string with escaped newlines — unreadable for
/// a CI gate.
#[expect(clippy::exit, reason = "a failing CI gate exits 1 with its message as written")]
fn gate(failure: Option<String>) -> CliResult<()> {
    if let Some(message) = failure {
        writeln!(io::stderr().lock(), "{message}")?;
        std::process::exit(1);
    }
    Ok(())
}

/// The typedness gate's failure message, when any program became less typed.
fn typedness_regression(outcome: &typedness_report::TypednessOutcome) -> Option<String> {
    use std::fmt::Write as _;
    if outcome.worsened.is_empty() {
        return None;
    }
    let mut message = format!(
        "typedness regressed: {} program(s)/kind(s) became less typed than the baseline:",
        outcome.worsened.len()
    );
    for worsening in &outcome.worsened {
        let _ = write!(message, "\n  {}: {}", worsening.program, worsening.detail);
    }
    message.push_str(
        "\nMake the program at least as typed again, or (only for a documented, \
         justified boundary) re-snapshot with --update-baseline. See the \
         `Typedness ratchet` policy in AGENTS.md.",
    );
    Some(message)
}

/// The `SmeltUnknown` gate's failure message: the delta and the files that
/// contribute the most, so reviewers know where to look first.
fn unknown_regression_message(regression: &unknown_report::Regression) -> String {
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
    message
}

/// Runs `smelt ts7-parity-report`.
#[cfg(feature = "ts7")]
fn run_ts7_parity_report(inputs: &[String], format: ReportFormat, output: Option<&str>) -> CliResult<()> {
    let input_paths: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
    let rendered = ts7_parity_report::ts7_parity_report(&ts7_parity_report::ParityReportOptions {
        inputs: &input_paths,
        format,
    })?;
    write_report(output, &rendered)
}

/// `smelt ts7-parity-report` needs the embedded TypeScript 7 checker.
#[cfg(not(feature = "ts7"))]
fn run_ts7_parity_report(
    _inputs: &[String],
    _format: ReportFormat,
    _output: Option<&str>,
) -> CliResult<()> {
    Err("`smelt ts7-parity-report` needs the `ts7` feature (the embedded TypeScript 7 checker)".into())
}
