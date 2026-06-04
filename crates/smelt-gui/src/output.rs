//! Output workspace — the tabbed right panel showing generated Rust,
//! diagnostics, pipeline proof, and an export preview.
//!
//! Rendering helpers are plain functions that take already-prepared data and
//! click listeners, so [`crate::workspace`] stays the single owner of state.

use gpui::{
    App, ClickEvent, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::input::{Input, InputState};

use crate::compiler::DiagnosticMessage;
use crate::theme;

/// Which output panel is currently visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OutputTab {
    /// Generated Rust source.
    Rust,
    /// Compiler diagnostics.
    Diagnostics,
    /// Per-stage pipeline trace.
    Pipeline,
    /// Export preview and copy actions.
    Export,
}

impl OutputTab {
    /// Tab label.
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Diagnostics => "Diagnostics",
            Self::Pipeline => "Pipeline",
            Self::Export => "Export",
        }
    }
}

/// Renders the output tab bar. Each tab gets its own click listener.
#[expect(
    clippy::too_many_arguments,
    reason = "one click listener per output tab"
)]
pub(super) fn render_tab_bar(
    active: OutputTab,
    diagnostic_count: usize,
    on_rust: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_diagnostics: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_pipeline: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_export: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let diag_suffix = if diagnostic_count > 0 {
        format!(" ({diagnostic_count})")
    } else {
        String::new()
    };

    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(2.0))
        .px(px(8.0))
        .bg(theme::bg_elevated())
        .border_b_1()
        .border_color(theme::border())
        .child(tab_button(
            OutputTab::Rust.label().to_owned(),
            active == OutputTab::Rust,
            on_rust,
        ))
        .child(tab_button(
            format!("{}{diag_suffix}", OutputTab::Diagnostics.label()),
            active == OutputTab::Diagnostics,
            on_diagnostics,
        ))
        .child(tab_button(
            OutputTab::Pipeline.label().to_owned(),
            active == OutputTab::Pipeline,
            on_pipeline,
        ))
        .child(tab_button(
            OutputTab::Export.label().to_owned(),
            active == OutputTab::Export,
            on_export,
        ))
}

/// A single clickable output tab.
fn tab_button(
    label: String,
    active: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (text_color, border_color) = if active {
        (theme::text_primary(), theme::primary())
    } else {
        (theme::text_muted(), theme::border_subtle())
    };

    div()
        .id(SharedString::from(format!("tab-{label}")))
        .cursor_pointer()
        .px(px(14.0))
        .py(px(9.0))
        .border_b_1()
        .border_color(border_color)
        .text_size(px(12.0))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(text_color)
        .hover(|style| style.text_color(theme::text_primary()))
        .child(label)
        .on_click(on_click)
}

/// Renders the Rust tab — the read-only generated-source editor.
pub(super) fn render_rust_tab(output_input: &Entity<InputState>) -> impl IntoElement {
    div().flex_1().bg(theme::bg_surface()).child(
        Input::new(output_input)
            .appearance(false)
            .bordered(false)
            .disabled(true)
            .h_full(),
    )
}

/// Renders the Diagnostics tab — a readable, grouped list of messages.
pub(super) fn render_diagnostics_tab(diagnostics: &[DiagnosticMessage]) -> impl IntoElement {
    let mut column = div()
        .flex_1()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .p(px(16.0))
        .overflow_hidden();

    if diagnostics.is_empty() {
        return column.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(div().size(px(8.0)).rounded(px(8.0)).bg(theme::success()))
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(theme::text_muted())
                        .child("No diagnostics — the program compiled cleanly."),
                ),
        );
    }

    for diagnostic in diagnostics {
        column = column.child(render_diagnostic_card(diagnostic));
    }
    column
}

/// Renders one diagnostic as a card with a stage label and message body.
fn render_diagnostic_card(diagnostic: &DiagnosticMessage) -> impl IntoElement {
    let mut body = div()
        .flex()
        .flex_col()
        .gap(px(1.0))
        .font_family("monospace");
    for line in diagnostic.message.lines() {
        body = body.child(
            div()
                .text_size(px(12.0))
                .text_color(theme::text_primary())
                .child(line.to_owned()),
        );
    }

    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .p(px(12.0))
        .rounded(px(6.0))
        .bg(theme::error_dim())
        .border_l_1()
        .border_color(theme::error())
        .child(
            div()
                .text_size(px(10.0))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::error())
                .child(diagnostic.stage.to_uppercase()),
        )
        .child(body)
}

/// Renders the Export tab — virtual crate layout, copy actions, and a preview.
#[expect(clippy::too_many_lines, reason = "single export-panel element tree")]
pub(super) fn render_export_tab(
    rust_source: &str,
    diagnostic_count: usize,
    on_copy_rust: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_copy_diagnostics: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let preview: String = rust_source.lines().take(40).collect::<Vec<_>>().join("\n");
    let has_more = rust_source.lines().count() > 40;

    let mut preview_body = div().flex().flex_col().font_family("monospace");
    for line in preview.lines() {
        preview_body = preview_body.child(
            div()
                .text_size(px(12.0))
                .text_color(theme::text_muted())
                .child(line.to_owned()),
        );
    }
    if has_more {
        preview_body = preview_body.child(
            div()
                .text_size(px(11.0))
                .text_color(theme::text_faint())
                .child("…"),
        );
    }

    div()
        .flex_1()
        .flex()
        .flex_col()
        .gap(px(14.0))
        .p(px(16.0))
        .overflow_hidden()
        .child(section_title("Generated crate layout"))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .p(px(12.0))
                .rounded(px(6.0))
                .bg(theme::bg_surface())
                .font_family("monospace")
                .text_size(px(12.0))
                .text_color(theme::text_muted())
                .child(div().text_color(theme::ember()).child("smelt-demo/"))
                .child(div().child("├─ Cargo.toml"))
                .child(div().child("└─ src/"))
                .child(div().child("   └─ main.rs")),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(8.0))
                .child(copy_button("Copy Rust", on_copy_rust))
                .child(copy_button(
                    format!("Copy diagnostics ({diagnostic_count})"),
                    on_copy_diagnostics,
                )),
        )
        .child(section_title("main.rs preview"))
        .child(
            div()
                .flex_1()
                .p(px(12.0))
                .rounded(px(6.0))
                .bg(theme::bg_surface())
                .overflow_hidden()
                .child(preview_body),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(theme::text_faint())
                .child("Pre-alpha: export shows a preview only — the GUI does not run Cargo."),
        )
}

/// A small section heading used inside the export tab.
fn section_title(text: &'static str) -> impl IntoElement {
    div()
        .text_size(px(11.0))
        .font_weight(FontWeight::BOLD)
        .text_color(theme::text_faint())
        .child(text)
}

/// A secondary "copy" action button.
fn copy_button(
    label: impl Into<String>,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let label = label.into();
    div()
        .id(SharedString::from(format!("copy-{label}")))
        .cursor_pointer()
        .px(px(12.0))
        .py(px(6.0))
        .rounded(px(6.0))
        .bg(theme::bg_elevated())
        .border_1()
        .border_color(theme::border())
        .text_size(px(12.0))
        .text_color(theme::text_primary())
        .hover(|style| style.bg(theme::bg_hover()))
        .active(|style| style.opacity(0.7))
        .child(label)
        .on_click(on_click)
}
