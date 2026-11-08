//! Command dispatch for the `smelt` CLI.
//!
//! Parses arguments, loads configuration and routes each subcommand to the
//! frontend, codegen and report pipelines.

use std::{io, io::Write as _, path::{Path, PathBuf}};

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
use crate::type_gate::TypeErrorPolicy;
use crate::scaffold;
use crate::clean;

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
            allow_type_errors,
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
                let type_errors = TypeErrorPolicy::resolve(config.type_errors(), allow_type_errors);
                pipeline::build_rust_crate(&config, &path, type_errors)?;
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
            retire_program,
        } => {
            let input_paths = paths.iter().map(PathBuf::from).collect::<Vec<_>>();
            let outcome =
                typedness_report::typedness_report(&typedness_report::TypednessReportOptions {
                    paths: &input_paths,
                    defines: &define.values,
                    format,
                    baseline: baseline.as_deref().map(PathBuf::from),
                    update_baseline,
                    retired: &retire_program,
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
        Command::Check { message_format, define, erasure, deny_type_errors } => {
            let mut config = load_config(&manifest_path)?;
            config.extend_defines(define.values);
            let settings = erasure::ErasureSettings::new(config.erasure(), &erasure)?;
            let type_errors = if deny_type_errors { TypeErrorPolicy::Deny } else { TypeErrorPolicy::Allow };
            match message_format {
                MessageFormat::Human => {
                    pipeline::check_manifest(&config, &manifest_path, &settings, type_errors)?;
                }
                MessageFormat::Json => {
                    // The report goes to stdout whole, type errors included, and
                    // the gate's verdict decides the exit status after it.
                    let checked = ManifestLowering::for_manifest(&config, &manifest_path)?.check()?;
                    let verdict = checked.verdict(type_errors);
                    let diagnostics = checked.into_measured().diagnostics(Some(&settings))?;
                    let mut stdout = io::stdout().lock();
                    writeln!(stdout, "{}", serde_json::to_string_pretty(&diagnostics)?)?;
                    verdict?;
                }
            }
        }
        Command::Build { hir, define, erasure, allow_type_errors } => {
            let mut config = load_config(&manifest_path)?;
            config.extend_defines(define.values);
            let type_errors = TypeErrorPolicy::resolve(config.type_errors(), allow_type_errors);
            if let Some(mode) = hir.mode() {
                let lowered = ManifestLowering::for_manifest(&config, &manifest_path)?
                    .check()?
                    .into_inner(type_errors)?
                    .lower()?;
                pipeline::print_hir(&lowered.krate, &lowered.modules, mode);
            } else {
                let settings = erasure::ErasureSettings::new(config.erasure(), &erasure)?;
                pipeline::build_rust_crate_with_erasure(&config, &manifest_path, &settings, type_errors)?;
            }
        }
        Command::New { name, python } => {
            let language =
                if python { scaffold::EntryLanguage::Python } else { scaffold::EntryLanguage::TypeScript };
            let root = scaffold::new_project(Path::new("."), &name, language)?;
            writeln!(io::stdout().lock(), "Created `{}`. Run `smelt build` there.", root.display())?;
        }
        // The dumps show what lowering makes of a file, type errors or not;
        // they emit no program.
        Command::DumpHir { file, debug } => {
            let lowered = ManifestLowering::for_file(&file)?.check()?.into_measured().lower()?;
            let mode = if debug { HirMode::Debug } else { HirMode::Compact };
            pipeline::print_hir(&lowered.krate, &lowered.modules, mode);
        }
        Command::DumpMir { file } => {
            let lowered = ManifestLowering::for_file(&file)?.check()?.into_measured().lower()?;
            pipeline::print_mir(&lowered.krate)?;
        }
        Command::DumpAst { file } => match SourceLang::from_path(&file)? {
            SourceLang::Python | SourceLang::PythonDeclaration => pipeline::dump_python_ast(&file)?,
            SourceLang::TypeScript | SourceLang::TypeScriptDeclaration => {
                return Err("--dump-ast is only supported for .py files; \
                                use `smelt dump-hir --debug` for TypeScript"
                    .into());
            }
        },
        Command::Probe { run_tests, define, allow_type_errors, report: ReportArgs { format, output } } => {
            let mut config = load_config(&manifest_path)?;
            config.extend_defines(define.values);
            let report = probe::probe_report(&probe::ProbeOptions {
                config: &config,
                manifest_path: &manifest_path,
                run_tests,
                format,
                type_errors: TypeErrorPolicy::resolve(config.type_errors(), allow_type_errors),
            })?;
            write_report(output.as_deref(), &report)?;
        }
        Command::Clean { force } => {
            let config = load_config(&manifest_path)?;
            let message = match clean::clean(&config, &manifest_path, force, confirm_removal)? {
                clean::Cleaned::Nothing(target) => format!("Nothing to clean: `{}` does not exist.", target.display()),
                clean::Cleaned::Removed(target) => format!("Removed `{}`.", target.display()),
            };
            writeln!(io::stdout().lock(), "{message}")?;
        }
    }
    Ok(())
}

/// Asks on the terminal before `smelt clean` removes files smelt did not
/// write. Without a terminal there is nobody to ask, so the answer is no and
/// the error tells the user about `--force`.
fn confirm_removal(foreign: &[PathBuf]) -> io::Result<bool> {
    use std::io::{BufRead as _, IsTerminal as _};
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "These files were not written by smelt:")?;
    for path in foreign.iter().take(10) {
        writeln!(stderr, "  {}", path.display())?;
    }
    if foreign.len() > 10 {
        writeln!(stderr, "  ... and {} more", foreign.len().saturating_sub(10))?;
    }
    if !io::stdin().is_terminal() {
        writeln!(stderr, "Not a terminal: pass --force to remove them.")?;
        return Ok(false);
    }
    write!(stderr, "Remove them too? [y/N] ")?;
    stderr.flush()?;
    let mut answer = String::new();
    io::stdin().lock().read_line(&mut answer)?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes"))
}

/// Parse the `Smelt.toml` at `path`.
fn load_config(path: &Path) -> CliResult<Config> {
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
