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
use crate::check_program::{CrossLanguageStubs, ModuleGraph, checker_location};
use crate::manifest::ManifestSource;

/// Environment switch: `SMELT_TS7=0` lowers without checker facts (A/B runs).
const DISABLE_VAR: &str = "SMELT_TS7";

/// Code of every diagnostic this module reports; the message carries `TSnnnn`.
const CHECK_CODE: &str = "smelt::ts-check";

/// Code of a checker diagnostic that is Smelt's own resolution drift.
const DRIFT_CODE: &str = "smelt::checker-resolution-drift";

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
    /// The checker's verdict over `graph`, for the type gate: its errors and
    /// warnings when it ran, or why nothing vouches for the sources.
    pub(crate) fn language_check(&self, graph: &ModuleGraph<'_>) -> LanguageCheck {
        match &self.status {
            TsStatus::NoSources => LanguageCheck::Absent,
            TsStatus::Disabled(reason) => LanguageCheck::Unverified((*reason).to_owned()),
            TsStatus::Failed(failure) => LanguageCheck::Unverified(failure_message(failure)),
            TsStatus::Ran => LanguageCheck::Ran(self.diagnostics(graph)),
        }
    }

    /// The checker's errors and warnings over `graph`'s sources.
    fn diagnostics(&self, graph: &ModuleGraph<'_>) -> Vec<ManifestDiagnostic> {
        let located = graph
            .sources()
            .iter()
            .map(|source| (checker_location(source).display().to_string(), *source))
            .collect::<Vec<_>>();
        self.facts
            .iter()
            .flat_map(|facts| &facts.diagnostics)
            .filter_map(|diagnostic| checker_diagnostic(diagnostic, &located, graph))
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
    graph: &ModuleGraph<'_>,
) -> Option<ManifestDiagnostic> {
    if diagnostic.code == UNUSED_EXPECT_ERROR {
        return None;
    }
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
    // A diagnostic in a file the program only reads (a generated stub, a
    // module the manifest excludes, Node's declarations) is not about the
    // crate's sources; one with no file is about the whole program.
    if found.is_none() && diagnostic.file.is_some() {
        return None;
    }
    let (line, column) = found
        .zip(diagnostic.start)
        .and_then(|(source, start)| source_line_column(&source.source, start))
        .unzip();
    let drifted = found
        .zip(unresolved_specifier(diagnostic))
        .filter(|(source, specifier)| graph.resolved_specifier(&source.path, specifier))
        .map(|(_, specifier)| specifier);
    let (code, message) = drifted.map_or_else(
        || {
            let text = format!("TS{}: {}", diagnostic.code, diagnostic.message);
            // An error in a typed version is the typed version's to fix, as
            // a frontend rejection of it is.
            let message = found.and_then(|source| source.typed_version.as_deref()).map_or(text.clone(), |origin| {
                format!(
                    "{text} (in the typed version of '{name}' in {shown}; fix it, or remove it to compile '{name}' untyped)",
                    name = origin.name,
                    shown = origin.shown,
                )
            });
            (CHECK_CODE, message)
        },
        |specifier| {
            (
                DRIFT_CODE,
                format!(
                    "internal error: the TypeScript checker cannot resolve `{specifier}`, which Smelt resolved; \
                     the checker's program must hold every module Smelt resolves (a Smelt bug, not a type error)"
                ),
            )
        },
    );
    Some(ManifestDiagnostic {
        file: found.map_or_else(|| file.to_owned(), |source| source.path.display().to_string()),
        category: DiagnosticCategory::TypeCheck,
        code,
        message,
        line,
        column,
        severity: Some(severity),
    })
}

/// TS2578, "Unused '@ts-expect-error' directive": not a type error in the
/// program, so never one the gate counts.
///
/// A `@ts-expect-error` asserts an error under the PROJECT's compiler options
/// (remeda's `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`), and
/// Smelt checks every program under its own fixed profile, whose types are
/// the ones lowering reads. Under that profile the expected error may not
/// occur; the directive is then unused, which says nothing about whether the
/// program is well typed. An error the directive does suppress is still
/// suppressed, as the source asks.
const UNUSED_EXPECT_ERROR: i32 = 2578;

/// The specifier a "cannot find module" diagnostic names (TS2307, TS2792).
fn unresolved_specifier(diagnostic: &TsDiagnostic) -> Option<&str> {
    if !matches!(diagnostic.code, 2307_i32 | 2792_i32) {
        return None;
    }
    let (_, rest) = diagnostic.message.split_once('\'')?;
    rest.split_once('\'').map(|(specifier, _)| specifier)
}

/// Type-checks the TypeScript sources of `graph` as one program, the one
/// [`crate::check_program::typescript_program`] generates from it.
///
/// Facts are filed under each source's path exactly as lowered.
#[cfg(feature = "ts7")]
pub(crate) fn analyze_graph(graph: &ModuleGraph<'_>, stubs: &CrossLanguageStubs) -> TsCheck {
    if !graph.sources().iter().any(|source| source.lang.is_typescript()) {
        return TsCheck::default();
    }
    if std::env::var(DISABLE_VAR).is_ok_and(|value| value == "0") {
        return TsCheck { facts: None, status: TsStatus::Disabled("the TypeScript checker is off (`SMELT_TS7=0`)") };
    }
    let program = crate::check_program::typescript_program(graph, stubs);
    match crate::timing::measure("manifest.ts7_check", || {
        smelt_ts_types::analyze_program(program, smelt_ts_types::Spellings::Omitted)
    }) {
        Ok(facts) => TsCheck { facts: Some(facts), status: TsStatus::Ran },
        Err(error) => TsCheck { facts: None, status: TsStatus::Failed(error.to_string()) },
    }
}

/// Without the `ts7` feature there is no checker: lowering uses annotations
/// only, and nothing vouches for the sources.
#[cfg(not(feature = "ts7"))]
pub(crate) fn analyze_graph(graph: &ModuleGraph<'_>, _stubs: &CrossLanguageStubs) -> TsCheck {
    let _ = DISABLE_VAR;
    if !graph.sources().iter().any(|source| source.lang.is_typescript()) {
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
        let verdict = check.language_check(&ModuleGraph::new(&[]));
        assert!(
            matches!(&verdict, LanguageCheck::Unverified(reason) if reason.contains("thread panicked")),
            "{verdict:?}"
        );
    }

    /// A run without TypeScript sources has nothing to vouch for.
    #[test]
    fn no_sources_is_absent() {
        assert!(matches!(TsCheck::default().language_check(&ModuleGraph::new(&[])), LanguageCheck::Absent));
    }

    /// A disabled checker is not a pass: nothing vouches for the sources.
    #[test]
    fn a_disabled_checker_is_unverified() {
        let check = TsCheck { facts: None, status: TsStatus::Disabled("off") };
        assert!(matches!(check.language_check(&ModuleGraph::new(&[])), LanguageCheck::Unverified(_)));
    }
}
