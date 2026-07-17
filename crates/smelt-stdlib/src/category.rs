//! Diagnostic categories shared across the TypeScript and Python frontends.
//!
//! The category is decided at the point a diagnostic is raised, where the
//! frontend knows *why* lowering failed — a missing builtin, an unlowered
//! construct, a typed-subset violation, and so on. Tooling (the library
//! probes, a future LSP) consumes the category instead of re-deriving it by
//! pattern-matching on human-readable message text.

use serde::{Deserialize, Serialize};

/// Coarse, machine-stable classification of a Smelt frontend diagnostic.
///
/// This is orthogonal to the per-site `code`/`message`: many distinct codes map
/// to the same category. Reports group by category to answer "is this blocked
/// on missing stdlib coverage or on an unimplemented lowering?".
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticCategory {
    /// Source text could not be parsed.
    Parse,
    /// A known JS/Python builtin or stdlib API that Smelt recognizes but has
    /// not implemented lowering or runtime support for yet.
    MissingStdlib,
    /// A language construct Smelt does not lower into Rust yet.
    UnsupportedLowering,
    /// Source violates Smelt's strict typed-subset requirements, such as a
    /// missing required type annotation. The fix is in the source, not Smelt.
    TypeConstraint,
    /// A name could not be resolved to any declaration or known builtin.
    UnresolvedReference,
    /// Internal invariant violation or a not-yet-classified diagnostic.
    Internal,
}

impl DiagnosticCategory {
    /// Stable kebab-case identifier, e.g. for JSON output or report grouping.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::MissingStdlib => "missing-stdlib",
            Self::UnsupportedLowering => "unsupported-lowering",
            Self::TypeConstraint => "type-constraint",
            Self::UnresolvedReference => "unresolved-reference",
            Self::Internal => "internal",
        }
    }

    /// Short human-readable label for report tables.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Parse => "parse error",
            Self::MissingStdlib => "missing stdlib",
            Self::UnsupportedLowering => "non-working Rust (unlowered)",
            Self::TypeConstraint => "typed-subset violation",
            Self::UnresolvedReference => "unresolved reference",
            Self::Internal => "internal",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every category round-trips through its stable string identifier.
    #[test]
    fn as_str_is_stable_kebab_case() {
        assert_eq!(DiagnosticCategory::MissingStdlib.as_str(), "missing-stdlib");
        assert_eq!(
            DiagnosticCategory::UnsupportedLowering.as_str(),
            "unsupported-lowering"
        );
        assert_eq!(DiagnosticCategory::Parse.as_str(), "parse");
    }

    /// Serde uses the same kebab-case spelling as `as_str`.
    #[test]
    fn serde_matches_as_str() {
        let json = serde_json::to_string(&DiagnosticCategory::TypeConstraint)
            .expect("category serializes to JSON");
        assert_eq!(json, "\"type-constraint\"");
        let back: DiagnosticCategory =
            serde_json::from_str(&json).expect("category deserializes from JSON");
        assert_eq!(back, DiagnosticCategory::TypeConstraint);
    }
}
