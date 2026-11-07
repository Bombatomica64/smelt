//! The Markdown rendering of a report.

use std::fmt::Write as _;

use super::{Category, UnknownReport};
use crate::report::escape_cell;

/// Render the report as Markdown, optionally with a baseline delta section.
pub(super) fn render_markdown(report: &UnknownReport, baseline: Option<&UnknownReport>) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "# SmeltUnknown usage report");
    let _ = writeln!(out);
    let _ = writeln!(out, "- Files scanned: {}", report.files_scanned);
    let _ = writeln!(out, "- Total `SmeltUnknown` occurrences: {}", report.total);
    let _ = writeln!(
        out,
        "- Avoidable erasure: **{}** (the number this metric is designed to drive down)",
        report.category_total(Category::AvoidableErasure)
    );
    let _ = writeln!(out);

    let _ = writeln!(out, "## Occurrences by category");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Category | Occurrences |");
    let _ = writeln!(out, "| --- | ---: |");
    for category in Category::all() {
        let _ = writeln!(
            out,
            "| {} | {} |",
            category.label(),
            report.category_total(category)
        );
    }
    let _ = writeln!(out);

    if let Some(baseline_report) = baseline {
        render_baseline_delta(&mut out, report, baseline_report);
    }

    if let Some(breakdown) = &report.erasure {
        crate::erasure::render_breakdown_markdown(&mut out, breakdown);
    }

    render_shape_table(&mut out, report);
    out
}

/// Render the advisory baseline-delta section.
///
/// Deltas are informational: a positive avoidable-erasure delta is flagged as a
/// regression to review. The default run is advisory and exits successfully;
/// only `--fail-on-regression` turns an avoidable-erasure rise into a nonzero exit, so intentional
/// boundary additions (which raise the legitimate-boundary count) never block a
/// build or CI step.
fn render_baseline_delta(out: &mut String, report: &UnknownReport, baseline: &UnknownReport) {
    let _ = writeln!(out, "## Change vs baseline");
    let _ = writeln!(out);
    let _ = writeln!(out, "| Category | Baseline | Current | Delta |");
    let _ = writeln!(out, "| --- | ---: | ---: | ---: |");
    for category in Category::all() {
        let before = baseline.category_total(category);
        let after = report.category_total(category);
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} |",
            category.label(),
            before,
            after,
            format_delta(before, after),
        );
    }
    let _ = writeln!(out);
    let avoidable_delta = report
        .category_total(Category::AvoidableErasure)
        .checked_sub(baseline.category_total(Category::AvoidableErasure));
    match avoidable_delta {
        Some(0) | None => {
            let _ = writeln!(
                out,
                "Avoidable erasure did not increase versus the baseline.",
            );
        }
        Some(increase) => {
            let _ = writeln!(
                out,
                "> **Regression to review:** avoidable erasure rose by {increase}. \
                 Confirm each new `SmeltUnknown` marks a genuine dynamic boundary \
                 (see the `SmeltUnknown enforcement` policy in `AGENTS.md`).",
            );
        }
    }
    let _ = writeln!(out);
}

/// Format a signed delta between two counts for a Markdown table cell.
fn format_delta(before: usize, after: usize) -> String {
    if after >= before {
        format!("+{}", after.saturating_sub(before))
    } else {
        format!("-{}", before.saturating_sub(after))
    }
}

/// Render the per-shape breakdown table.
fn render_shape_table(out: &mut String, report: &UnknownReport) {
    let _ = writeln!(out, "## Occurrence shapes");
    let _ = writeln!(out);
    if report.shapes.is_empty() {
        let _ = writeln!(out, "No `SmeltUnknown` occurrences were found.");
        return;
    }
    let _ = writeln!(out, "| Occurrences | Files | Category | Shape | Example |");
    let _ = writeln!(out, "| ---: | ---: | --- | --- | --- |");
    for shape in &report.shapes {
        let _ = writeln!(
            out,
            "| {} | {} | {} | `{}` | `{}` |",
            shape.occurrences,
            shape.files,
            shape.category.label(),
            escape_cell(&shape.shape),
            escape_cell(&shape.example),
        );
    }
    let _ = writeln!(out);
}
