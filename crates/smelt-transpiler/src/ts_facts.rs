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
//! alone, which changes the types of the output. Without a checker (feature
//! off, or `SMELT_TS7=0`, the switch for comparing a run with and without
//! facts) or with one that failed, nothing vouches that the sources type-check,
//! so [`TsCheck::language_check`] reports them unverified and the type gate
//! (`crate::type_gate`) refuses to build them unless type errors are allowed.
//! The checker's own diagnostics are the TypeScript type errors of that gate.

use smelt_stdlib::DiagnosticCategory;
use smelt_ts_model::{TsDiagnostic, TsDiagnosticCategory, TsTypeFacts};

use crate::lowering::{ManifestDiagnostic, Severity, source_line_column};
use crate::type_gate::LanguageCheck;
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
    /// Whether the checker ran, and why not when it did not.
    status: TsStatus,
}

/// Whether the TypeScript checker ran over a run's sources.
#[derive(Debug, Default)]
enum TsStatus {
    /// The run has no TypeScript source to check.
    #[default]
    NoSources,
    /// There are sources, but this build or environment has no checker.
    Disabled(&'static str),
    /// The checker that should have run produced nothing.
    Failed(String),
    /// The checker ran; its report is in the facts.
    Ran,
}

impl TsCheck {
    /// The checker's verdict over `sources`, for the type gate: its errors and
    /// warnings when it ran, or why nothing vouches for the sources.
    pub(crate) fn language_check(&self, sources: &[&ManifestSource]) -> LanguageCheck {
        match &self.status {
            TsStatus::NoSources => LanguageCheck::Absent,
            TsStatus::Disabled(reason) => LanguageCheck::Unverified((*reason).to_owned()),
            TsStatus::Failed(failure) => LanguageCheck::Unverified(failure_message(failure)),
            TsStatus::Ran => LanguageCheck::Ran(self.diagnostics(sources)),
        }
    }

    /// The checker's errors and warnings over `sources`.
    fn diagnostics(&self, sources: &[&ManifestSource]) -> Vec<ManifestDiagnostic> {
        let located = sources
            .iter()
            .filter_map(|source| {
                let absolute = std::path::absolute(&source.path).ok()?;
                Some((absolute.display().to_string(), *source))
            })
            .collect::<Vec<_>>();
        self.facts
            .iter()
            .flat_map(|facts| &facts.diagnostics)
            .filter_map(|diagnostic| checker_diagnostic(diagnostic, &located))
            .collect()
    }
}

/// The sentence a checker failure is reported with.
fn failure_message(failure: &str) -> String {
    format!("the TypeScript checker failed, so types come from annotations alone: {failure}")
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
    if files.is_empty() {
        return TsCheck::default();
    }
    if std::env::var(DISABLE_VAR).is_ok_and(|value| value == "0") {
        return TsCheck { facts: None, status: TsStatus::Disabled("the TypeScript checker is off (`SMELT_TS7=0`)") };
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
        Ok(facts) => TsCheck { facts: Some(facts), status: TsStatus::Ran },
        Err(error) => TsCheck { facts: None, status: TsStatus::Failed(error.to_string()) },
    }
}

/// Without the `ts7` feature there is no checker: lowering uses annotations
/// only, and nothing vouches for the sources.
#[cfg(not(feature = "ts7"))]
fn analyze_files(files: Vec<(String, String)>) -> TsCheck {
    let _ = DISABLE_VAR;
    if files.is_empty() {
        return TsCheck::default();
    }
    TsCheck {
        facts: None,
        status: TsStatus::Disabled("this smelt was built without the TypeScript checker (feature `ts7`)"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A checker that failed vouches for nothing, and says why.
    #[test]
    fn a_checker_failure_is_unverified() {
        let check = TsCheck { facts: None, status: TsStatus::Failed("thread panicked".to_owned()) };
        let verdict = check.language_check(&[]);
        assert!(
            matches!(&verdict, LanguageCheck::Unverified(reason) if reason.contains("thread panicked")),
            "{verdict:?}"
        );
    }

    /// A run without TypeScript sources has nothing to vouch for.
    #[test]
    fn no_sources_is_absent() {
        assert!(matches!(TsCheck::default().language_check(&[]), LanguageCheck::Absent));
    }

    /// A disabled checker is not a pass: nothing vouches for the sources.
    #[test]
    fn a_disabled_checker_is_unverified() {
        let check = TsCheck { facts: None, status: TsStatus::Disabled("off") };
        assert!(matches!(check.language_check(&[]), LanguageCheck::Unverified(_)));
    }
}
