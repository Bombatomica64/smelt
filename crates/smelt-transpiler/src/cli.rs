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
    config_parser, diagnostics, erasure, list_escape_report,
    pipeline, probe,
    report::{self, ReportFormat, write_report},
    source_files::SourceLang,
    test_report, typedness_report, unknown_report,
};
#[cfg(feature = "ts7")]
use crate::ts7_parity_report;
use crate::lowering::ManifestLowering;

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
            report: ReportArgs { format, output },
            json_output,
        } => {
            if let Some(source_manifest) = build_manifest {
                let path = PathBuf::from(source_manifest);
                let config = load_config(&path)?;
                pipeline::build_rust_crate(&config, &path)?;
            }
            let report = test_report::rust_test_report(&test_report::RustTestReportOptions {
                cargo_manifest: &PathBuf::from(cargo_manifest),
                focus: &focus,
                guard: &guard,
                full,
                baseline_report: baseline_report.as_deref().map(PathBuf::from),
                include_diagnostics,
                suppress_warnings,
            })?;
            let json = serde_json::to_string_pretty(&report)?;
            if let Some(path) = json_output {
                write_report(Some(&path), &json)?;
            }
            let rendered = match format {
                ReportFormat::Markdown => test_report::render_markdown(&report),
                ReportFormat::Json => json,
            };
            write_report(output.as_deref(), &rendered)?;
        }
        Command::ListEscapeReport { manifest, define, report: ReportArgs { format, output }, function, top } => {
            let report = list_escape_report::list_escape_report(
                &list_escape_report::ListEscapeReportOptions {
                    manifest: &PathBuf::from(manifest),
                    defines: &define.values,
                    format,
                    functions: &function,
                    top,
                },
            )?;
            write_report(output.as_deref(), &report)?;
        }
        Command::TypednessReport {
            paths,
            define,
            gate: GateArgs { format, baseline, fail_on_regression, update_baseline, output },
        } => {
            let input_paths = paths.iter().map(PathBuf::from).collect::<Vec<_>>();
            let outcome =
                typedness_report::typedness_report(&typedness_report::TypednessReportOptions {
                    paths: &input_paths,
                    defines: &define.values,
                    format,
                    baseline: baseline.as_deref().map(PathBuf::from),
                    update_baseline,
                })?;
            report::finish_gated(&outcome, output.as_deref(), fail_on_regression)?;
        }
        Command::Ts7ParityReport { inputs, define, report: ReportArgs { format, output } } => {
            run_ts7_parity_report(&inputs, &define.values, format, output.as_deref())?;
        }
        Command::SmeltUnknownReport {
            roots,
            gate: GateArgs { format, baseline, fail_on_regression, update_baseline, output },
            erasure: erasure_sidecars,
        } => {
            let root_paths = roots.iter().map(PathBuf::from).collect::<Vec<_>>();
            let sidecar_paths = erasure_sidecars.iter().map(PathBuf::from).collect::<Vec<_>>();
            let outcome = unknown_report::unknown_report(&unknown_report::UnknownReportOptions {
                roots: &root_paths,
                format,
                baseline: baseline.as_deref().map(PathBuf::from),
                update_baseline,
                erasure_sidecars: &sidecar_paths,
            })?;
            report::finish_gated(&outcome, output.as_deref(), fail_on_regression)?;
        }
        Command::Check { message_format, define, erasure } => {
            let mut config = load_config(&manifest_path)?;
            config.extend_defines(define.values);
            let settings = erasure::ErasureSettings::new(config.erasure(), &erasure)?;
            match message_format {
                MessageFormat::Human => {
                    pipeline::check_manifest(&config, &manifest_path, &settings)?;
                }
                MessageFormat::Json => {
                    let diagnostics = ManifestLowering::for_manifest(&config, &manifest_path)?
                        .diagnostics(Some(&settings))?;
                    let mut stdout = io::stdout().lock();
                    writeln!(stdout, "{}", serde_json::to_string_pretty(&diagnostics)?)?;
                }
            }
        }
        Command::Build { hir, define, erasure } => {
            let mut config = load_config(&manifest_path)?;
            config.extend_defines(define.values);
            if let Some(mode) = hir.mode() {
                let (krate, modules) = ManifestLowering::for_manifest(&config, &manifest_path)?.lower()?;
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
            let (krate, modules) = ManifestLowering::for_file(&file)?.lower()?;
            let mode = if debug { HirMode::Debug } else { HirMode::Compact };
            pipeline::print_hir(&krate, &modules, mode);
        }
        Command::DumpMir { file } => {
            let (krate, _) = ManifestLowering::for_file(&file)?.lower()?;
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
        Command::Probe { run_tests, define, report: ReportArgs { format, output } } => {
            let mut config = load_config(&manifest_path)?;
            config.extend_defines(define.values);
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
fn run_ts7_parity_report(
    inputs: &[String],
    defines: &[(String, String)],
    format: ReportFormat,
    output: Option<&str>,
) -> CliResult<()> {
    let input_paths: Vec<PathBuf> = inputs.iter().map(PathBuf::from).collect();
    let rendered = ts7_parity_report::ts7_parity_report(&ts7_parity_report::ParityReportOptions {
        inputs: &input_paths,
        defines,
        format,
    })?;
    write_report(output, &rendered)
}

/// `smelt ts7-parity-report` needs the embedded TypeScript 7 checker.
#[cfg(not(feature = "ts7"))]
fn run_ts7_parity_report(
    _inputs: &[String],
    _defines: &[(String, String)],
    _format: ReportFormat,
    _output: Option<&str>,
) -> CliResult<()> {
    Err("`smelt ts7-parity-report` needs the `ts7` feature (the embedded TypeScript 7 checker)".into())
}
