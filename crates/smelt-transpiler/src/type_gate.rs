//! The type gate: Smelt emits code only for sources its checkers accepted.
//!
//! The frontends lower a program on the assumption that it type-checks: a
//! literal takes its type hint on trust, a missing required field is
//! "impossible", two implementations of one function never meet. Ill-typed
//! input breaks those assumptions silently. So every
//! lowering hands its result back wrapped in [`Checked`], together with what
//! the TypeScript checker and ty said about the sources, and the only way to
//! the lowered value is a decision about that report:
//!
//! - [`Checked::into_inner`] with a [`TypeErrorPolicy`]: `Deny` (the default of
//!   `smelt build`) fails on any checker error and on a language no checker
//!   vouched for; `Allow` (`--allow-type-errors`, or `[build] type-errors =
//!   "allow"` in the manifest) prints them as warnings and goes on.
//! - [`Checked::into_measured`] for a report that measures the lowering itself
//!   and shows no diagnostics.
//!
//! The report also decides how far the frontends may trust each source
//! ([`CheckReport::typescript_trust`]): a source the checker did not accept is
//! lowered [`SourceTrust::Unchecked`], and the shortcuts that rely on a checked
//! program refuse it instead of miscompiling. So allowing type errors never
//! turns into silently wrong Rust; at worst it turns into a lowering error.

use std::fmt;
use std::io::Write as _;

use schemars::JsonSchema;
use serde::Deserialize;
use smelt_frontend_ts::SourceTrust;
use smelt_stdlib::DiagnosticCategory;

use crate::lowering::{ManifestDiagnostic, Severity};

/// What a command does with checker errors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TypeErrorPolicy {
    /// A checker error, or a language no checker could vouch for, fails the
    /// command before anything is written.
    #[default]
    Deny,
    /// Checker errors are printed as warnings and lowering goes on, with the
    /// frontends' checked-program shortcuts turned into errors.
    Allow,
}

impl TypeErrorPolicy {
    /// The policy of one run: `allow_flag` (`--allow-type-errors`) wins over the
    /// manifest's `[build] type-errors`.
    #[must_use]
    pub const fn resolve(manifest: Self, allow_flag: bool) -> Self {
        if allow_flag { Self::Allow } else { manifest }
    }
}

/// What one language's checker made of a run's sources.
#[derive(Debug, Clone)]
pub(crate) enum LanguageCheck {
    /// The run has no source in this language.
    Absent,
    /// The checker ran; these are its errors and warnings.
    Ran(Vec<ManifestDiagnostic>),
    /// There are sources, but no checker vouched for them; the reason.
    Unverified(String),
}

impl LanguageCheck {
    /// How many errors the checker reported.
    fn errors(&self) -> usize {
        match self {
            Self::Ran(diagnostics) => diagnostics.iter().filter(|diagnostic| is_error(diagnostic)).count(),
            Self::Absent | Self::Unverified(_) => 0,
        }
    }

}

/// Whether `diagnostic` is a checker error.
fn is_error(diagnostic: &ManifestDiagnostic) -> bool {
    diagnostic.category == DiagnosticCategory::TypeCheck && diagnostic.severity == Some(Severity::Error)
}

/// The checkers' verdict over every source of one run.
#[derive(Debug, Clone)]
pub(crate) struct CheckReport {
    /// The TypeScript checker's verdict.
    typescript: LanguageCheck,
    /// ty's verdict over the Python sources.
    python: LanguageCheck,
}

impl CheckReport {
    /// A report from each language's verdict.
    pub(crate) const fn new(typescript: LanguageCheck, python: LanguageCheck) -> Self {
        Self { typescript, python }
    }

    /// How far the TypeScript frontend may trust the source it lowers under
    /// `file` (the path string the checker's diagnostics carry).
    ///
    /// Trust is per file, so a spec file the checker rejects (a test runner's
    /// globals without their type package, say) does not take it from the
    /// library sources the checker accepted. A source is trusted when a
    /// checker ran over the language, reported no error in the file and no
    /// error it could not place in a file.
    pub(crate) fn typescript_trust(&self, file: &str) -> SourceTrust {
        let LanguageCheck::Ran(diagnostics) = &self.typescript else {
            return match self.typescript {
                LanguageCheck::Absent => SourceTrust::Checked,
                LanguageCheck::Ran(_) | LanguageCheck::Unverified(_) => SourceTrust::Unchecked,
            };
        };
        let rejected = diagnostics
            .iter()
            .any(|diagnostic| is_error(diagnostic) && (diagnostic.file.is_empty() || diagnostic.file == file));
        if rejected { SourceTrust::Unchecked } else { SourceTrust::Checked }
    }

    /// Every checker diagnostic, then one warning per language no checker
    /// vouched for.
    pub(crate) fn diagnostics(&self) -> Vec<ManifestDiagnostic> {
        let mut diagnostics = Vec::new();
        for check in [&self.typescript, &self.python] {
            match check {
                LanguageCheck::Ran(reported) => diagnostics.extend(reported.iter().cloned()),
                LanguageCheck::Unverified(reason) => diagnostics.push(unverified_diagnostic(reason)),
                LanguageCheck::Absent => {}
            }
        }
        diagnostics
    }

    /// Why the run cannot pass the gate under `Deny`, or `None` when it can.
    fn refusal(&self) -> Option<TypeGateError> {
        let errors = self.typescript.errors().saturating_add(self.python.errors());
        let unverified = [&self.typescript, &self.python]
            .into_iter()
            .filter_map(|check| match check {
                LanguageCheck::Unverified(reason) => Some(reason.clone()),
                LanguageCheck::Absent | LanguageCheck::Ran(_) => None,
            })
            .collect::<Vec<_>>();
        (errors > 0 || !unverified.is_empty()).then_some(TypeGateError { errors, unverified })
    }
}

/// The warning that stands for a language no checker vouched for.
fn unverified_diagnostic(reason: &str) -> ManifestDiagnostic {
    ManifestDiagnostic {
        file: String::new(),
        category: DiagnosticCategory::TypeCheck,
        code: "smelt::unverified-types",
        message: format!("the sources were not type-checked: {reason}"),
        line: None,
        column: None,
        severity: Some(Severity::Warning),
    }
}

/// A lowering's result, held until its caller decides what the checkers'
/// report means for it; see the module docs.
#[derive(Debug)]
#[must_use = "a lowering's type errors are decided by `into_inner` or `into_measured`"]
pub(crate) struct Checked<T> {
    /// The lowered value.
    value: T,
    /// What the checkers said about the sources it was lowered from.
    report: CheckReport,
}

impl<T> Checked<T> {
    /// Pair a lowered value with the report over its sources.
    pub(crate) const fn new(value: T, report: CheckReport) -> Self {
        Self { value, report }
    }

    /// The lowered value under `policy`.
    ///
    /// Prints every checker diagnostic to stderr, as an `error` when `Deny`
    /// makes it fail the run and as a `warning` otherwise.
    ///
    /// # Errors
    ///
    /// Under [`TypeErrorPolicy::Deny`], when a checker reported an error or a
    /// language with sources had no checker.
    pub(crate) fn into_inner(self, policy: TypeErrorPolicy) -> Result<T, TypeGateError> {
        print_diagnostics(&self.report.diagnostics(), policy);
        self.verdict(policy)?;
        Ok(self.value)
    }

    /// What `policy` makes of the report, without unwrapping: for a command
    /// that shows the diagnostics itself (`check --message-format json`) and
    /// decides its exit status after.
    ///
    /// # Errors
    ///
    /// As [`Self::into_inner`] would fail.
    pub(crate) fn verdict(&self, policy: TypeErrorPolicy) -> Result<(), TypeGateError> {
        match (policy, self.report.refusal()) {
            (TypeErrorPolicy::Deny, Some(refusal)) => Err(refusal),
            (TypeErrorPolicy::Deny | TypeErrorPolicy::Allow, _) => Ok(()),
        }
    }

    /// The lowered value, for a report that measures lowering itself (typedness,
    /// list escapes, checker parity, HIR/MIR dumps).
    ///
    /// Such a command emits no program, and a source the checker did not accept
    /// was already lowered with its checked-program shortcuts refused, so its
    /// measurement is of honest output. The checkers' diagnostics are not this
    /// command's to show and are dropped.
    pub(crate) fn into_measured(self) -> T {
        self.value
    }
}

/// Why the type gate refused a run.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct TypeGateError {
    /// How many checker errors were reported.
    errors: usize,
    /// Each language no checker vouched for, by reason.
    unverified: Vec<String>,
}

impl fmt::Display for TypeGateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut causes = Vec::new();
        if self.errors > 0 {
            causes.push(format!("the type checkers reported {} error(s)", self.errors));
        }
        causes.extend(self.unverified.iter().map(|reason| format!("the sources were not type-checked ({reason})")));
        write!(
            formatter,
            "{}; Smelt lowers only programs that type-check. Fix the errors, or pass \
             `--allow-type-errors` (or set `[build] type-errors = \"allow\"`) to lower it anyway",
            causes.join("; ")
        )
    }
}

/// The message itself: `main` prints a failing command's error with `Debug`.
impl fmt::Debug for TypeGateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl std::error::Error for TypeGateError {}

/// Prints checker diagnostics to stderr, as
/// `<label>[type-check]: <file>:<line>:<col>: <message>`. A checker error is
/// labelled `error` only when `policy` makes it fail the run.
fn print_diagnostics(diagnostics: &[ManifestDiagnostic], policy: TypeErrorPolicy) {
    let mut stderr = std::io::stderr().lock();
    for diagnostic in diagnostics {
        let label = if policy == TypeErrorPolicy::Deny
            && (is_error(diagnostic) || diagnostic.code == "smelt::unverified-types")
        {
            "error"
        } else {
            "warning"
        };
        let position = diagnostic
            .line
            .zip(diagnostic.column)
            .map_or_else(String::new, |(line, column)| format!(":{line}:{column}"));
        let location = if diagnostic.file.is_empty() { String::new() } else { format!("{}{position}: ", diagnostic.file) };
        let _ignored = writeln!(stderr, "{label}[type-check]: {location}{}", diagnostic.message);
    }
}

#[cfg(test)]
#[expect(clippy::expect_used, reason = "a test fails fast on what it set up")]
mod tests {
    use super::*;

    /// A checker error with no location.
    fn error() -> ManifestDiagnostic {
        ManifestDiagnostic {
            file: "a.ts".to_owned(),
            category: DiagnosticCategory::TypeCheck,
            code: "smelt::ts-check",
            message: "TS2322: nope".to_owned(),
            line: Some(1),
            column: Some(1),
            severity: Some(Severity::Error),
        }
    }

    /// A clean report passes either policy; an error passes only `Allow`.
    #[test]
    fn deny_refuses_errors_and_allow_does_not() {
        let clean = || Checked::new(1_u8, CheckReport::new(LanguageCheck::Ran(Vec::new()), LanguageCheck::Absent));
        assert_eq!(clean().into_inner(TypeErrorPolicy::Deny), Ok(1_u8));
        assert_eq!(clean().into_inner(TypeErrorPolicy::Allow), Ok(1_u8));
        let dirty = || Checked::new(1_u8, CheckReport::new(LanguageCheck::Ran(vec![error()]), LanguageCheck::Absent));
        let refusal = dirty().into_inner(TypeErrorPolicy::Deny).expect_err("an error is refused");
        assert_eq!(refusal.errors, 1_usize);
        assert!(refusal.to_string().contains("--allow-type-errors"), "{refusal}");
        assert_eq!(dirty().into_inner(TypeErrorPolicy::Allow), Ok(1_u8));
    }

    /// Sources no checker vouched for are refused under `Deny`, like an error.
    #[test]
    fn unverified_sources_are_refused() {
        let report = CheckReport::new(LanguageCheck::Absent, LanguageCheck::Unverified("no ty".to_owned()));
        assert_eq!(report.typescript_trust("a.ts"), SourceTrust::Checked);
        let refusal = Checked::new((), report).into_inner(TypeErrorPolicy::Deny).expect_err("unverified");
        assert_eq!(refusal.unverified, ["no ty".to_owned()]);
    }

    /// The TypeScript frontend trusts a source only when the checker accepted it.
    #[test]
    fn trust_follows_the_typescript_verdict_per_file() {
        let trust = |typescript, file| CheckReport::new(typescript, LanguageCheck::Absent).typescript_trust(file);
        assert_eq!(trust(LanguageCheck::Ran(Vec::new()), "a.ts"), SourceTrust::Checked);
        assert_eq!(trust(LanguageCheck::Ran(vec![error()]), "a.ts"), SourceTrust::Unchecked);
        assert_eq!(trust(LanguageCheck::Ran(vec![error()]), "b.ts"), SourceTrust::Checked);
        let unplaced = ManifestDiagnostic { file: String::new(), ..error() };
        assert_eq!(trust(LanguageCheck::Ran(vec![unplaced]), "b.ts"), SourceTrust::Unchecked);
        assert_eq!(trust(LanguageCheck::Unverified("off".to_owned()), "a.ts"), SourceTrust::Unchecked);
    }

    /// The manifest decides unless the flag allows.
    #[test]
    fn the_flag_overrides_the_manifest() {
        assert_eq!(TypeErrorPolicy::resolve(TypeErrorPolicy::Deny, true), TypeErrorPolicy::Allow);
        assert_eq!(TypeErrorPolicy::resolve(TypeErrorPolicy::Deny, false), TypeErrorPolicy::Deny);
        assert_eq!(TypeErrorPolicy::resolve(TypeErrorPolicy::Allow, false), TypeErrorPolicy::Allow);
    }
}
