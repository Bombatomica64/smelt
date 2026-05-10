//! Diagnostics for recognized but unsupported standard-library forms.

use crate::RuleId;

/// Unsupported source shape for a known standard-library rule.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum UnsupportedForm {
    /// The call used an unsupported argument count.
    ArgumentCount,
    /// The call used keyword/named arguments that are not modeled yet.
    KeywordArguments,
    /// The call used unsupported argument types.
    ArgumentTypes,
    /// The call omitted required type context.
    MissingTypeContext,
}

/// Diagnostic metadata for a known standard-library API that cannot lower yet.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub struct StdlibDiagnostic {
    /// Rule whose API spelling was recognized.
    pub rule: RuleId,
    /// Unsupported source shape.
    pub form: UnsupportedForm,
    /// Stable user-facing diagnostic message.
    pub message: &'static str,
}

impl StdlibDiagnostic {
    /// Create a diagnostic for an unsupported known API form.
    #[must_use]
    pub const fn unsupported(
        rule: RuleId,
        form: UnsupportedForm,
        message: &'static str,
    ) -> Self {
        Self {
            rule,
            form,
            message,
        }
    }
}
