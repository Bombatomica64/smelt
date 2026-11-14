//! The Markdown rendering of a report.

use super::{Category, UnknownReport};
use crate::report::{escape_cell, push_fmt};

/// Render the report as Markdown, optionally with a baseline delta section.
pub(super) fn render_markdown(report: &UnknownReport, baseline: Option<&UnknownReport>) -> String {
    let mut out = String::new();
    push_fmt(&mut out, format_args!("# SmeltUnknown usage report\n"));
    out.push('\n');
    push_fmt(
        &mut out,
        format_args!("- Files scanned: {}\n", report.files_scanned),
    );
    push_fmt(
        &mut out,
        format_args!("- Total `SmeltUnknown` occurrences: {}\n", report.total),
    );
    push_fmt(
        &mut out,
        format_args!(
            "- Avoidable erasure: **{}** (the number this metric is designed to drive down)\n",
            report.category_total(Category::AvoidableErasure)
        ),
    );
    out.push('\n');

    push_fmt(&mut out, format_args!("## Occurrences by category\n"));
    out.push('\n');
    push_fmt(&mut out, format_args!("| Category | Occurrences |\n"));
    push_fmt(&mut out, format_args!("| --- | ---: |\n"));
    for category in Category::all() {
        push_fmt(
            &mut out,
            format_args!(
                "| {} | {} |\n",
                category.label(),
                report.category_total(category)
            ),
        );
    }
    out.push('\n');

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
    push_fmt(out, format_args!("## Change vs baseline\n"));
    out.push('\n');
    push_fmt(
        out,
        format_args!("| Category | Baseline | Current | Delta |\n"),
    );
    push_fmt(out, format_args!("| --- | ---: | ---: | ---: |\n"));
    for category in Category::all() {
        let before = baseline.category_total(category);
        let after = report.category_total(category);
        push_fmt(
            out,
            format_args!(
                "| {} | {} | {} | {} |\n",
                category.label(),
                before,
                after,
                format_delta(before, after)
            ),
        );
    }
    out.push('\n');
    let avoidable_delta = report
        .category_total(Category::AvoidableErasure)
        .checked_sub(baseline.category_total(Category::AvoidableErasure));
    match avoidable_delta {
        Some(0) | None => {
            push_fmt(
                out,
                format_args!("Avoidable erasure did not increase versus the baseline.\n"),
            );
        }
        Some(increase) => {
            push_fmt(
                out,
                format_args!(
                    "> **Regression to review:** avoidable erasure rose by {increase}. \
                    Confirm each new `SmeltUnknown` marks a genuine dynamic boundary \
                    (see the `SmeltUnknown enforcement` policy in `AGENTS.md`).\n"
                ),
            );
        }
    }
    out.push('\n');
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
    push_fmt(out, format_args!("## Occurrence shapes\n"));
    out.push('\n');
    if report.shapes.is_empty() {
        push_fmt(
            out,
            format_args!("No `SmeltUnknown` occurrences were found.\n"),
        );
        return;
    }
    push_fmt(
        out,
        format_args!("| Occurrences | Files | Category | Shape | Example |\n"),
    );
    push_fmt(out, format_args!("| ---: | ---: | --- | --- | --- |\n"));
    for shape in &report.shapes {
        push_fmt(
            out,
            format_args!(
                "| {} | {} | {} | `{}` | `{}` |\n",
                shape.occurrences,
                shape.files,
                shape.category.label(),
                escape_cell(&shape.shape),
                escape_cell(&shape.example)
            ),
        );
    }
    out.push('\n');
}
