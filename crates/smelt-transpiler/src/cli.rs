//! Command dispatch for the `smelt` CLI.
//!
//! Parses arguments, loads configuration and routes each subcommand to the
//! frontend, codegen and report pipelines.

use std::{io, io::Write as _, path::PathBuf};

use clap::Parser as _;

use crate::{
    CliResult,
    cli_parser::{Args, Command, GateArgs, HirMode, MessageFormat, ReportArgs},
    config::Config,
    config_parser, diagnostics, erasure, list_escape_report, lowering,
    pipeline, probe,
    report::{self, ReportFormat, write_report},
    source_files::SourceLang,
    test_report, typedness_report, unknown_report,
};
#[cfg(feature = "ts7")]
use crate::ts7_parity_report;

/// CLI entry point: one dispatch over every subcommand.
///
/// # Errors
///
/// Returns the failure of whichever subcommand ran.
pub fn run() -> CliResult<()> {
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
        Command::ListEscapeReport { manifest, report: ReportArgs { format, output }, function, top } => {
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
            gate: GateArgs { format, baseline, fail_on_regression, output },
            update_baseline,
        } => {
            let input_paths = paths.iter().map(PathBuf::from).collect::<Vec<_>>();
            let outcome =
                typedness_report::typedness_report(&typedness_report::TypednessReportOptions {
                    paths: &input_paths,
                    format,
                    baseline: baseline.as_deref().map(PathBuf::from),
                    update_baseline,
                })?;
            report::finish_gated(&outcome, output.as_deref(), fail_on_regression)?;
        }
        Command::Ts7ParityReport { inputs, report: ReportArgs { format, output } } => {
            run_ts7_parity_report(&inputs, format, output.as_deref())?;
        }
        Command::SmeltUnknownReport {
            roots,
            gate: GateArgs { format, baseline, fail_on_regression, output },
            erasure: erasure_sidecars,
        } => {
            let root_paths = roots.iter().map(PathBuf::from).collect::<Vec<_>>();
            let sidecar_paths = erasure_sidecars.iter().map(PathBuf::from).collect::<Vec<_>>();
            let outcome = unknown_report::unknown_report(&unknown_report::UnknownReportOptions {
                roots: &root_paths,
                format,
                baseline: baseline.as_deref().map(PathBuf::from),
                erasure_sidecars: &sidecar_paths,
            })?;
            report::finish_gated(&outcome, output.as_deref(), fail_on_regression)?;
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
        Command::Probe { run_tests, report: ReportArgs { format, output } } => {
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
    config_parser::parse_path(path)
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
