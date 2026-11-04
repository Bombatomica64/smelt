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

use crate::{TsDiagnostic, TsDiagnosticCategory};

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
        message: flattened_message(diagnostic),
    }
}

/// The diagnostic's message with its chained details, one per line and
/// indented by depth, exactly as `tsc` flattens a message chain.
///
/// tsrs writes the flattened message as bytes; the text it writes comes from
/// Rust strings, so the lossy conversion never replaces anything in practice
/// and only avoids a panic path.
fn flattened_message(diagnostic: P<Diagnostic>) -> String {
    let mut output = Vec::new();
    tsrs_compiler::diagnosticwriter::write_flattened_diagnostic_message(
        &mut output,
        diagnostic,
        "\n",
    );
    String::from_utf8_lossy(&output).into_owned()
}
