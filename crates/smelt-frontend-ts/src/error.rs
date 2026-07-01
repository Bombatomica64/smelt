//! Diagnostic types for TypeScript frontend lowering.

use smelt_hir::Span;
use smelt_stdlib::{DiagnosticCategory, is_javascript_global_builtin};

/// Error type for Smelt TypeScript frontend.
///
/// Contains diagnostic information about parse or lowering errors. The `code`
/// is a stable per-site identifier; the `category` is the coarse machine bucket
/// (see [`DiagnosticCategory`]) decided where the error is raised, so tooling
/// can group failures without parsing `message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmeltError {
    /// Error code identifying the error type.
    pub code: &'static str,
    /// Coarse classification used by reports and tooling.
    pub category: DiagnosticCategory,
    /// Source location of the error.
    pub span: Span,
    /// Human-readable error message.
    pub message: String,
    /// Optional note with additional context.
    pub note: Option<String>,
}

impl SmeltError {
    /// Create a diagnostic for decorated source lowered without its manifest.
    pub(crate) fn specialization_required(span: Span, definition: &str) -> Self {
        Self {
            code: "smelt::specialization-required",
            category: DiagnosticCategory::UnsupportedLowering,
            span,
            message: format!(
                "definition-time metaprogramming for '{definition}' requires host-runtime specialization"
            ),
            note: Some(
                "Run the specialization pipeline before lowering this TypeScript module."
                    .to_owned(),
            ),
        }
    }

    /// Create a diagnostic for manifest behavior that cannot map to source.
    pub(crate) fn native_specialization_adapter_required(
        span: Span,
        adapter: &str,
        detail: &str,
    ) -> Self {
        Self {
            code: "smelt::native-specialization-adapter-required",
            category: DiagnosticCategory::UnsupportedLowering,
            span,
            message: format!("native specialization adapter '{adapter}' is required: {detail}"),
            note: Some(
                "Install an exact versioned adapter or replace the opaque definition-time behavior with source-defined code."
                    .to_owned(),
            ),
        }
    }

    /// Create an unsupported TypeScript feature error (an unimplemented
    /// lowering). This is the default for constructs Smelt cannot yet emit.
    pub(crate) fn unsupported(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::unsupported-ts",
            category: DiagnosticCategory::UnsupportedLowering,
            span,
            message: message.into(),
            note: None,
        }
    }

    /// Create a parse error.
    pub(crate) fn parse(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::parse-error",
            category: DiagnosticCategory::Parse,
            span,
            message: message.into(),
            note: None,
        }
    }

    /// Create a diagnostic for a reference to a recognized-but-unimplemented
    /// JS/TS builtin (categorized [`DiagnosticCategory::MissingStdlib`]).
    pub(crate) fn missing_stdlib(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::unsupported-ts",
            category: DiagnosticCategory::MissingStdlib,
            span,
            message: message.into(),
            note: None,
        }
    }

    /// Create a diagnostic for a name that resolves to nothing known
    /// (categorized [`DiagnosticCategory::UnresolvedReference`]).
    pub(crate) fn unresolved(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::unsupported-ts",
            category: DiagnosticCategory::UnresolvedReference,
            span,
            message: message.into(),
            note: None,
        }
    }

    /// Create a diagnostic for an unresolved name, classified by whether the
    /// name is a recognized builtin (missing stdlib) or unknown (unresolved).
    pub(crate) fn for_unresolved_name(span: Span, name: &str, message: impl Into<String>) -> Self {
        if is_javascript_global_builtin(name) {
            Self::missing_stdlib(span, message)
        } else {
            Self::unresolved(span, message)
        }
    }
}
