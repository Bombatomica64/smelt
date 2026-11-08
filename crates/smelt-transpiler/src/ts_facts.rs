//! TypeScript 7 checker facts for a manifest's TypeScript sources.
//!
//! Before the frontend lowers a crate, the transpiler type-checks every
//! TypeScript source of the dependency closure as one program with the
//! embedded TypeScript 7 checker (`smelt-ts-types`, behind the `ts7`
//! feature) and hands the resulting span-keyed facts to each module's
//! lowering through `FrontendOptions::ts_facts`. Each source is analysed
//! under exactly the path string it is lowered under, so the frontend looks
//! its own spans up with its own `path`.
//!
//! The facts are input to lowering: without them it proceeds on annotations
//! alone, which changes the types of the output. That is expected when there is
//! no checker (feature off, or `SMELT_TS7=0`, the switch for comparing a run with
//! and without facts). A checker that failed is not expected, so [`TsCheck`]
//! carries the failure and every command reports it. The checker's own
//! diagnostics are what `smelt check` reports as type errors.

use std::io::Write as _;

use smelt_stdlib::DiagnosticCategory;
use smelt_ts_model::{TsDiagnostic, TsDiagnosticCategory, TsTypeFacts};

use crate::lowering::{ManifestDiagnostic, Severity, source_line_column};
use crate::manifest::ManifestSource;
use crate::source_files::SourceLang;

/// Environment switch: `SMELT_TS7=0` lowers without checker facts (A/B runs).
const DISABLE_VAR: &str = "SMELT_TS7";

/// Code of every diagnostic this module reports; the message carries `TSnnnn`.
const CHECK_CODE: &str = "smelt::ts-check";

/// What the checker made of a run's TypeScript sources.
#[derive(Debug, Default)]
pub(crate) struct TsCheck {
    /// The facts lowering reads, when the checker ran.
    pub(crate) facts: Option<TsTypeFacts>,
    /// Why the checker that should have run produced nothing.
    failure: Option<String>,
}

impl TsCheck {
    /// Prints the failure, if any, to stderr: output built past it is typed
    /// differently, and whoever reads that output has to be told.
    pub(crate) fn report_failure(&self) {
        if let Some(failure) = &self.failure {
            let _ignored = writeln!(std::io::stderr().lock(), "warning[ts-check]: {}", failure_message(failure));
        }
    }

    /// The checker's errors and warnings over `sources`, and its failure.
    pub(crate) fn diagnostics(&self, sources: &[&ManifestSource]) -> Vec<ManifestDiagnostic> {
        let located = sources
            .iter()
            .filter_map(|source| {
                let absolute = std::path::absolute(&source.path).ok()?;
                Some((absolute.display().to_string(), *source))
            })
            .collect::<Vec<_>>();
        let reported = self
            .facts
            .iter()
            .flat_map(|facts| &facts.diagnostics)
            .filter_map(|diagnostic| checker_diagnostic(diagnostic, &located));
        reported.chain(self.failure.iter().map(|failure| failure_diagnostic(failure))).collect()
    }
}

/// Prints checker diagnostics to stderr, as
/// `warning[ts-check]: <file>:<line>:<col>: TSnnnn: <message>`. A checker error is
/// labelled `error` only when `deny_errors` makes it fail the run.
pub(crate) fn print_diagnostics(diagnostics: &[ManifestDiagnostic], deny_errors: bool) {
    let mut stderr = std::io::stderr().lock();
    for diagnostic in diagnostics {
        let label = if deny_errors && diagnostic.severity == Some(Severity::Error) { "error" } else { "warning" };
        let position = diagnostic
            .line
            .zip(diagnostic.column)
            .map_or_else(String::new, |(line, column)| format!(":{line}:{column}"));
        let _ignored = writeln!(
            stderr,
            "{label}[ts-check]: {}{position}: {}",
            diagnostic.file, diagnostic.message
        );
    }
}

/// `--deny-type-errors`: an error naming how many checker errors `diagnostics` holds.
pub(crate) fn fail_on_type_errors(diagnostics: &[ManifestDiagnostic]) -> Result<(), String> {
    let errors = diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.category == DiagnosticCategory::TypeCheck && diagnostic.severity == Some(Severity::Error)
        })
        .count();
    if errors == 0 {
        return Ok(());
    }
    Err(format!("the TypeScript checker reported {errors} error(s)"))
}

/// The sentence a checker failure is reported with.
fn failure_message(failure: &str) -> String {
    format!("the TypeScript checker failed, so types come from annotations alone: {failure}")
}

/// A checker failure as a warning with no location.
fn failure_diagnostic(failure: &str) -> ManifestDiagnostic {
    ManifestDiagnostic {
        file: String::new(),
        category: DiagnosticCategory::TypeCheck,
        code: CHECK_CODE,
        message: failure_message(failure),
        line: None,
        column: None,
        severity: Some(Severity::Warning),
    }
}

/// One checker diagnostic, located in its source; a suggestion or a message
/// says nothing wrong and is dropped.
///
/// The checker's file names are program paths, which end in the source's
/// absolute path without its leading `/`.
fn checker_diagnostic(
    diagnostic: &TsDiagnostic,
    located: &[(String, &ManifestSource)],
) -> Option<ManifestDiagnostic> {
    let severity = if diagnostic.category == TsDiagnosticCategory::Error {
        Severity::Error
    } else if diagnostic.category == TsDiagnosticCategory::Warning {
        Severity::Warning
    } else {
        return None;
    };
    let file = diagnostic.file.as_deref().unwrap_or_default();
    let found = located
        .iter()
        .find(|(path, _)| file.ends_with(path.trim_start_matches('/')))
        .map(|(_, source)| *source);
    let (line, column) = found
        .zip(diagnostic.start)
        .and_then(|(source, start)| source_line_column(&source.source, start))
        .unzip();
    Some(ManifestDiagnostic {
        file: found.map_or_else(|| file.to_owned(), |source| source.path.display().to_string()),
        category: DiagnosticCategory::TypeCheck,
        code: CHECK_CODE,
        message: format!("TS{}: {}", diagnostic.code, diagnostic.message),
        line,
        column,
        severity: Some(severity),
    })
}

/// Type-checks the TypeScript sources among `sources`.
pub(crate) fn analyze_sources(sources: &[&ManifestSource]) -> TsCheck {
    analyze_files(
        sources
            .iter()
            .filter(|source| matches!(source.lang, SourceLang::TypeScript | SourceLang::TypeScriptDeclaration))
            .map(|source| (source.path.display().to_string(), source.source.clone()))
            .collect(),
    )
}

/// Type-checks `(path, text)` TypeScript files as one program.
///
/// Facts are filed under each `path` exactly as given (the string the
/// frontend lowers the file under), while the file is placed in the program
/// at its absolute location so relative imports resolve as on disk.
#[cfg(feature = "ts7")]
fn analyze_files(files: Vec<(String, String)>) -> TsCheck {
    if files.is_empty() || std::env::var(DISABLE_VAR).is_ok_and(|value| value == "0") {
        return TsCheck::default();
    }
    let inputs: Vec<smelt_ts_types::SourceInput> = files
        .into_iter()
        .map(|(path, text)| {
            let located = std::path::absolute(&path).map(|location| location.display().to_string());
            let input = smelt_ts_types::SourceInput::new(path, text);
            match located {
                Ok(location) => input.located_at(location),
                Err(_) => input,
            }
        })
        .collect();
    match crate::timing::measure("manifest.ts7_check", || smelt_ts_types::analyze_with(inputs, smelt_ts_types::Spellings::Omitted)) {
        Ok(facts) => TsCheck { facts: Some(facts), failure: None },
        Err(error) => TsCheck { facts: None, failure: Some(error.to_string()) },
    }
}

/// Without the `ts7` feature there is no checker: lowering uses annotations only.
#[cfg(not(feature = "ts7"))]
fn analyze_files(_files: Vec<(String, String)>) -> TsCheck {
    let _ = DISABLE_VAR;
    TsCheck::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A checker that failed is a warning with its cause, in every report.
    #[test]
    fn a_checker_failure_is_a_diagnostic() {
        let check = TsCheck { facts: None, failure: Some("thread panicked".to_owned()) };
        let diagnostics = check.diagnostics(&[]);
        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        for diagnostic in &diagnostics {
            assert_eq!(diagnostic.severity, Some(Severity::Warning));
            assert!(diagnostic.message.contains("thread panicked"), "{diagnostic:?}");
        }
        assert_eq!(fail_on_type_errors(&diagnostics), Ok(()));
    }

    /// No checker, by feature or by `SMELT_TS7=0`, is not a failure.
    #[test]
    fn no_checker_reports_nothing() {
        assert!(TsCheck::default().diagnostics(&[]).is_empty());
    }
}
