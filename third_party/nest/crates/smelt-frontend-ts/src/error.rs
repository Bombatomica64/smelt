//! Diagnostic types for TypeScript frontend lowering.

use smelt_hir::Span;

/// Error type for Smelt TypeScript frontend.
///
/// Contains diagnostic information about parse or lowering errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmeltError {
    /// Error code identifying the error type.
    pub code: &'static str,
    /// Source location of the error.
    pub span: Span,
    /// Human-readable error message.
    pub message: String,
    /// Optional note with additional context.
    pub note: Option<String>,
}

impl SmeltError {
    /// Create an unsupported TypeScript feature error.
    pub(crate) fn unsupported(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::unsupported-ts",
            span,
            message: message.into(),
            note: None,
        }
    }

    /// Create a parse error.
    pub(crate) fn parse(span: Span, message: impl Into<String>) -> Self {
        Self {
            code: "smelt::parse-error",
            span,
            message: message.into(),
            note: None,
        }
    }
}
