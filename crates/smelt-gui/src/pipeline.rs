//! Pipeline proof view — renders the per-stage pass/fail/skip trace.
//!
//! Gives a compact, honest picture of how far a compilation progressed through
//! the shared Smelt pipeline, marking the failing stage when one exists.

use gpui::{FontWeight, IntoElement, ParentElement, Styled, div, px};

use crate::compiler::{PipelineStageReport, StageStatus};
use crate::theme;

/// Renders the ordered list of pipeline stages with status indicators.
pub(super) fn render_pipeline(stages: &[PipelineStageReport]) -> impl IntoElement {
    let mut column = div().flex().flex_col().gap(px(2.0)).p(px(16.0));

    for stage in stages {
        column = column.child(render_stage_row(stage));
    }

    column.child(
        div()
            .pt(px(12.0))
            .text_size(px(11.0))
            .text_color(theme::text_faint())
            .child("TS → HIR → MIR → optimize → validate → Rust"),
    )
}

/// Renders a single stage row: status dot, name, and status text.
fn render_stage_row(stage: &PipelineStageReport) -> impl IntoElement {
    let (color, status_text) = match stage.status {
        StageStatus::Passed => (theme::success(), "passed"),
        StageStatus::Failed => (theme::error(), "failed"),
        StageStatus::Skipped => (theme::neutral(), "skipped"),
    };

    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(10.0))
        .py(px(7.0))
        .rounded(px(6.0))
        .bg(theme::bg_surface())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(div().size(px(8.0)).rounded(px(8.0)).bg(color))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::text_primary())
                        .child(stage.name),
                ),
        )
        .child(
            div()
                .text_size(px(11.0))
                .font_family("monospace")
                .text_color(color)
                .child(status_text),
        )
}
