//! TypeScript diagnostics for the analysed program.
//!
//! These are the diagnostics `tsc --noEmit` (TypeScript 7) reports for the
//! same sources and options: config/program diagnostics, syntax errors, and
//! semantic (type) errors. The frontend uses them to reject invalid
//! TypeScript in-process; Smelt's own "supported subset" checks stay separate.

#![expect(
    clippy::redundant_pub_crate,
    reason = "diagnostic collection is called from the crate root"
)]

use tsrs_ast::Diagnostic;
use tsrs_compiler::{Context, Program};
use tsrs_core::P;
use tsrs_diagnostics::Category;

/// Severity of a [`TsDiagnostic`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub enum TsDiagnosticCategory {
    /// A type or syntax error; the program is not valid TypeScript.
    Error,
    /// A warning.
    Warning,
    /// A suggestion.
    Suggestion,
    /// An informational message.
    Message,
}

/// One TypeScript diagnostic.
#[derive(Clone, PartialEq, Eq, Debug)]
#[non_exhaustive]
pub struct TsDiagnostic {
    /// Program path of the file, or `None` for global diagnostics.
    pub file: Option<String>,
    /// Byte offset where the diagnostic starts.
    pub start: Option<u32>,
    /// Byte offset where it ends.
    pub end: Option<u32>,
    /// TypeScript error code (`2322` for TS2322).
    pub code: i32,
    /// Severity.
    pub category: TsDiagnosticCategory,
    /// Message text with its elaboration chain flattened, as `tsc` prints it.
    pub message: String,
}

/// Collects program, syntactic and semantic diagnostics, in that order.
pub(crate) fn collect(program: &'static Program, ctx: &Context) -> Vec<TsDiagnostic> {
    let mut raw = program.get_program_diagnostics();
    raw.extend(program.get_syntactic_diagnostics(ctx, None));
    raw.extend(program.get_semantic_diagnostics(ctx, None));
    raw.extend(program.get_global_diagnostics(ctx));
    raw.into_iter().map(convert).collect()
}

/// Copies one tsrs diagnostic into owned form.
fn convert(diagnostic: P<Diagnostic>) -> TsDiagnostic {
    TsDiagnostic {
        file: diagnostic.file().map(|file| file.file_name().to_owned()),
        start: diagnostic.pos(),
        end: diagnostic.end(),
        code: diagnostic.code(),
        category: match diagnostic.category() {
            Category::Error => TsDiagnosticCategory::Error,
            Category::Warning => TsDiagnosticCategory::Warning,
            Category::Suggestion => TsDiagnosticCategory::Suggestion,
            Category::Message => TsDiagnosticCategory::Message,
        },
        message: tsrs_compiler::diagnosticwriter::flatten_diagnostic_message(diagnostic, "\n"),
    }
}
