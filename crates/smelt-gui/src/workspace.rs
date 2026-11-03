//! Root workspace view — the Smelt product showcase.
//!
//! Lays out a full-window workspace: a top product header, a left rail of
//! built-in examples, stacked TypeScript/Python source editors, a tabbed output
//! workspace (Rust / Diagnostics / Pipeline / Export), and a bottom proof rail.
//! State lives here; rendering of the output panels is delegated to
//! [`crate::output`] and [`crate::pipeline`].

use gpui::{
    App, AppContext, ClickEvent, ClipboardItem, Context, Entity, FontWeight, Hsla,
    InteractiveElement, IntoElement, ParentElement, Render, SharedString,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::compiler::{CompileReport, PY_PATH, TS_PATH, compile_report};
use crate::examples::{self, DEFAULT_EXAMPLE_ID, ShowcaseExample};
use crate::output::{self, OutputTab};
use crate::pipeline;
use crate::theme;

/// Root view of the application.
pub(super) struct SmeltWorkspace {
    /// Editable TypeScript source input.
    ts_input: Entity<InputState>,
    /// Editable Python source input.
    py_input: Entity<InputState>,
    /// Read-only generated Rust source.
    output_input: Entity<InputState>,
    /// Identifier of the currently loaded showcase example.
    active_example_id: &'static str,
    /// Currently visible output panel.
    active_output_tab: OutputTab,
    /// Most recent compilation report.
    report: CompileReport,
    /// Total successful compilations this session.
    compile_count: u32,
    /// Kept alive to receive input-change events.
    _subscriptions: Vec<Subscription>,
}

impl SmeltWorkspace {
    /// Creates the workspace with the default cross-language example loaded.
    pub(super) fn new(window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let ts_input = cx.new(|input_cx| {
            InputState::new(window, input_cx)
                .code_editor("typescript")
                .placeholder("TypeScript source…")
        });
        let py_input = cx.new(|input_cx| {
            InputState::new(window, input_cx)
                .code_editor("python")
                .placeholder("Python source…")
        });
        let output_input = cx.new(|input_cx| {
            InputState::new(window, input_cx)
                .code_editor("rust")
                .placeholder("Generated Rust will appear here…")
        });

        let sub_ts = cx.subscribe_in(&ts_input, window, |this, _entity, event, window, sub_cx| {
            if matches!(event, InputEvent::Change) {
                this.recompile(window, sub_cx);
                sub_cx.notify();
            }
        });
        let sub_py = cx.subscribe_in(&py_input, window, |this, _entity, event, window, sub_cx| {
            if matches!(event, InputEvent::Change) {
                this.recompile(window, sub_cx);
                sub_cx.notify();
            }
        });

        let mut workspace = Self {
            ts_input,
            py_input,
            output_input,
            active_example_id: DEFAULT_EXAMPLE_ID,
            active_output_tab: OutputTab::Rust,
            // Nothing compiled yet: the empty-input report runs no stage.
            report: compile_report("", ""),
            compile_count: 0,
            _subscriptions: vec![sub_ts, sub_py],
        };
        workspace.load_example(DEFAULT_EXAMPLE_ID, window, cx);
        workspace
    }

    /// Reads current source texts and recompiles.
    fn recompile(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let ts_text = self.ts_input.read(cx).value().to_string();
        let py_text = self.py_input.read(cx).value().to_string();
        self.apply_report(compile_report(&ts_text, &py_text), window, cx);
    }

    /// The single transition for a new compile result.
    ///
    /// Counts a successful compilation, re-clamps the visible tab to what the
    /// new report can show, and syncs the read-only output editor. Doing all
    /// three here (rather than syncing during render) keeps the view state a
    /// function of the report.
    fn apply_report(&mut self, report: CompileReport, window: &mut Window, cx: &mut Context<'_, Self>) {
        if report.is_ok() {
            self.compile_count = self.compile_count.saturating_add(1);
        }
        self.report = report;
        self.select_tab(self.active_output_tab);
        let target = self.report.rust_or_empty();
        if self.output_input.read(cx).value() != target {
            self.output_input.update(cx, |state, update_cx| {
                state.set_value(target, window, update_cx);
            });
        }
    }

    /// Shows `tab`, or the nearest available tab when it has nothing to show.
    fn select_tab(&mut self, tab: OutputTab) {
        self.active_output_tab = tab.clamp(&self.report);
    }

    /// Loads a showcase example into both editors and recompiles.
    fn load_example(&mut self, id: &'static str, window: &mut Window, cx: &mut Context<'_, Self>) {
        let example = examples::by_id(id);
        self.active_example_id = example.id;
        self.ts_input.update(cx, |state, update_cx| {
            state.set_value(example.ts_source, window, update_cx);
        });
        self.py_input.update(cx, |state, update_cx| {
            state.set_value(example.py_source, window, update_cx);
        });
        self.recompile(window, cx);
    }

    /// Restores the active example's source.
    fn reset_active(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let id = self.active_example_id;
        self.load_example(id, window, cx);
    }

    /// Counts lines in an input entity's current value.
    fn line_count(input: &Entity<InputState>, cx: &Context<'_, Self>) -> usize {
        let value = input.read(cx).value();
        if value.is_empty() {
            0
        } else {
            value.lines().count()
        }
    }

    /// Whether an input's current value differs from the given example source.
    fn is_dirty(input: &Entity<InputState>, source: &str, cx: &Context<'_, Self>) -> bool {
        **input.read(cx).value() != *source
    }
}

impl Render for SmeltWorkspace {
    /// Builds the workspace element tree for the current frame.
    #[expect(clippy::too_many_lines, reason = "single top-level GPUI element tree")]
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let example = examples::by_id(self.active_example_id);
        let ts_lines = Self::line_count(&self.ts_input, cx);
        let py_lines = Self::line_count(&self.py_input, cx);
        let rust_lines = self.report.rust_or_empty().lines().count();
        let ts_dirty = Self::is_dirty(&self.ts_input, example.ts_source, cx);
        let py_dirty = Self::is_dirty(&self.py_input, example.py_source, cx);
        let diagnostic_count = self.report.diagnostics().len();

        // ── Listeners ───────────────────────────────────────────────
        let on_compile = cx.listener(|this, _e: &ClickEvent, w, ctx| {
            this.recompile(w, ctx);
            ctx.notify();
        });
        let on_reset = cx.listener(|this, _e: &ClickEvent, w, ctx| {
            this.reset_active(w, ctx);
            ctx.notify();
        });
        let on_export_btn = cx.listener(|this, _e: &ClickEvent, _w, ctx| {
            this.select_tab(OutputTab::Export);
            ctx.notify();
        });
        let rust_for_header = self.report.rust_or_empty();
        let on_copy_header = cx.listener(move |_t, _e: &ClickEvent, _w, ctx| {
            ctx.write_to_clipboard(ClipboardItem::new_string(rust_for_header.clone()));
        });

        let on_tab_rust = cx.listener(|this, _e: &ClickEvent, _w, ctx| {
            this.select_tab(OutputTab::Rust);
            ctx.notify();
        });
        let on_tab_diag = cx.listener(|this, _e: &ClickEvent, _w, ctx| {
            this.select_tab(OutputTab::Diagnostics);
            ctx.notify();
        });
        let on_tab_pipe = cx.listener(|this, _e: &ClickEvent, _w, ctx| {
            this.select_tab(OutputTab::Pipeline);
            ctx.notify();
        });
        let on_tab_export = cx.listener(|this, _e: &ClickEvent, _w, ctx| {
            this.select_tab(OutputTab::Export);
            ctx.notify();
        });

        // ── Active output content ───────────────────────────────────
        let content = match self.active_output_tab {
            OutputTab::Rust => output::render_rust_tab(&self.output_input).into_any_element(),
            OutputTab::Diagnostics => {
                output::render_diagnostics_tab(self.report.diagnostics()).into_any_element()
            }
            OutputTab::Pipeline => {
                pipeline::render_pipeline(&self.report.stages()).into_any_element()
            }
            OutputTab::Export => {
                let rust_for_export = self.report.rust_or_empty();
                let diag_text = self
                    .report
                    .diagnostics()
                    .iter()
                    .map(|d| format!("[{}] {}", d.stage_label(), d.message))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                let on_copy_rust = cx.listener(move |_t, _e: &ClickEvent, _w, ctx| {
                    ctx.write_to_clipboard(ClipboardItem::new_string(rust_for_export.clone()));
                });
                let on_copy_diags = cx.listener(move |_t, _e: &ClickEvent, _w, ctx| {
                    ctx.write_to_clipboard(ClipboardItem::new_string(diag_text.clone()));
                });
                output::render_export_tab(
                    &self.report.rust_or_empty(),
                    diagnostic_count,
                    on_copy_rust,
                    on_copy_diags,
                )
                .into_any_element()
            }
        };

        // ── Example rail ────────────────────────────────────────────
        let mut rail = div()
            .w(px(244.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(12.0))
            .bg(theme::bg_panel())
            .border_r_1()
            .border_color(theme::border())
            .child(
                div()
                    .px(px(4.0))
                    .pb(px(4.0))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::text_faint())
                    .child("EXAMPLES"),
            );
        for entry in examples::EXAMPLES {
            let selected = entry.id == self.active_example_id;
            let id = entry.id;
            let on_select = cx.listener(move |this, _e: &ClickEvent, w, ctx| {
                this.load_example(id, w, ctx);
                ctx.notify();
            });
            rail = rail.child(example_card(entry, selected, on_select));
        }

        // ── Assemble ────────────────────────────────────────────────
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg_base())
            .text_color(theme::text_primary())
            .child(render_header(
                self.report.is_ok(),
                self.report.duration,
                rust_lines,
                on_compile,
                on_reset,
                on_copy_header,
                on_export_btn,
            ))
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .w_full()
                    .overflow_hidden()
                    .child(rail)
                    .child(render_source_column(
                        &self.ts_input,
                        &self.py_input,
                        ts_lines,
                        py_lines,
                        ts_dirty,
                        py_dirty,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .child(output::render_tab_bar(
                                self.active_output_tab,
                                diagnostic_count,
                                on_tab_rust,
                                on_tab_diag,
                                on_tab_pipe,
                                on_tab_export,
                            ))
                            .child(content),
                    ),
            )
            .child(render_status_bar(
                self.report.is_ok(),
                self.report.failing_stage(),
                self.compile_count,
                self.report.duration,
                ts_lines,
                py_lines,
                rust_lines,
                example.title,
            ))
    }
}

// ── Header ──────────────────────────────────────────────────────────

/// Top product header: brand, value statement, status pills, and actions.
#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "render helper passes header state and builds one element tree"
)]
fn render_header(
    ok: bool,
    duration: std::time::Duration,
    rust_lines: usize,
    on_compile: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_reset: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_copy: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    on_export: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (status_color, status_bg, status_label) = if ok {
        (theme::success(), theme::success_dim(), "Live compile")
    } else {
        (theme::error(), theme::error_dim(), "Error")
    };
    let timing = format!("{duration:.1?}");

    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(20.0))
        .py(px(12.0))
        .bg(theme::bg_elevated())
        .border_b_1()
        .border_color(theme::border())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(16.0))
                .child(
                    div()
                        .text_size(px(22.0))
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_color(theme::primary())
                        .child("smelt"),
                )
                .child(
                    div()
                        .text_size(px(13.0))
                        .text_color(theme::text_muted())
                        .child("Strict TypeScript + Python → one Rust program"),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(status_pill(
                    theme::python(),
                    theme::primary_dim(),
                    "Pre-alpha",
                ))
                .child(status_pill(status_color, status_bg, status_label))
                .child(
                    div()
                        .px(px(8.0))
                        .py(px(4.0))
                        .rounded(px(4.0))
                        .bg(theme::bg_base())
                        .font_family("monospace")
                        .text_size(px(11.0))
                        .text_color(theme::text_faint())
                        .child(timing),
                )
                .child(ghost_button("Reset", on_reset))
                .child(ghost_button("Copy Rust", on_copy))
                .child(ghost_button("Export", on_export))
                .child(primary_button(
                    &format!("Compile ({rust_lines}L)"),
                    on_compile,
                )),
        )
}

/// A small status pill with a colored dot and label.
fn status_pill(color: Hsla, bg: Hsla, label: &str) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(6.0))
        .px(px(10.0))
        .py(px(5.0))
        .rounded(px(6.0))
        .bg(bg)
        .child(div().size(px(6.0)).rounded(px(6.0)).bg(color))
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(color)
                .child(label.to_owned()),
        )
}

// ── Source editors ──────────────────────────────────────────────────

/// Middle column with stacked TypeScript and Python editors.
#[expect(
    clippy::too_many_arguments,
    reason = "render helper passes editor state"
)]
fn render_source_column(
    ts_input: &Entity<InputState>,
    py_input: &Entity<InputState>,
    ts_lines: usize,
    py_lines: usize,
    ts_dirty: bool,
    py_dirty: bool,
) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .min_w(px(0.0))
        .border_r_1()
        .border_color(theme::border())
        .overflow_hidden()
        .child(render_editor_panel(
            ts_input,
            "TS",
            TS_PATH,
            ts_lines,
            ts_dirty,
            theme::typescript(),
        ))
        .child(render_editor_panel(
            py_input,
            "PY",
            PY_PATH,
            py_lines,
            py_dirty,
            theme::python(),
        ))
}

/// A single editable source panel with a labelled header.
#[expect(
    clippy::too_many_arguments,
    reason = "render helper passes editor state"
)]
fn render_editor_panel(
    input: &Entity<InputState>,
    badge: &str,
    path: &str,
    line_count: usize,
    dirty: bool,
    accent: Hsla,
) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .min_h(px(0.0))
        .border_b_1()
        .border_color(theme::border_subtle())
        .child(panel_header(badge, path, line_count, dirty, accent))
        .child(
            div()
                .flex_1()
                .min_h(px(0.0))
                .bg(theme::bg_surface())
                .child(Input::new(input).appearance(false).bordered(false).h_full()),
        )
}

/// Panel header with language badge, file path, dirty marker, and line count.
fn panel_header(
    badge: &str,
    path: &str,
    line_count: usize,
    dirty: bool,
    accent: Hsla,
) -> impl IntoElement {
    let mut left = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .px(px(6.0))
                .py(px(1.0))
                .rounded(px(3.0))
                .bg(accent.opacity(0.15))
                .text_size(px(10.0))
                .font_weight(FontWeight::BOLD)
                .text_color(accent)
                .child(badge.to_owned()),
        )
        .child(
            div()
                .font_family("monospace")
                .text_size(px(11.0))
                .text_color(theme::text_muted())
                .child(path.to_owned()),
        );
    if dirty {
        left = left.child(
            div()
                .text_size(px(10.0))
                .text_color(theme::python())
                .child("• edited"),
        );
    }

    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(12.0))
        .py(px(6.0))
        .bg(theme::bg_elevated())
        .border_b_1()
        .border_color(theme::border_subtle())
        .child(left)
        .child(
            div()
                .font_family("monospace")
                .text_size(px(10.0))
                .text_color(theme::text_faint())
                .child(format!("{line_count}L")),
        )
}

// ── Example card ────────────────────────────────────────────────────

/// A selectable example card in the left rail.
fn example_card(
    example: &ShowcaseExample,
    selected: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let (bg, border_color) = if selected {
        (theme::bg_selected(), theme::primary())
    } else {
        (theme::bg_surface(), theme::border_subtle())
    };

    div()
        .id(SharedString::from(format!("example-{}", example.id)))
        .cursor_pointer()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .p(px(10.0))
        .rounded(px(6.0))
        .bg(bg)
        .border_1()
        .border_color(border_color)
        .hover(|style| style.border_color(theme::primary()))
        .child(
            div()
                .text_size(px(13.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_primary())
                .child(example.title),
        )
        .child(
            div()
                .text_size(px(11.0))
                .text_color(theme::text_muted())
                .child(example.description),
        )
        .child(
            div()
                .text_size(px(10.0))
                .font_family("monospace")
                .text_color(theme::text_faint())
                .child(example.tags.join(" · ")),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .pt(px(2.0))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(theme::ember())
                        .child(example.languages_label()),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(theme::text_faint())
                        .child(example.focus.label()),
                ),
        )
        .on_click(on_click)
}

// ── Status bar ──────────────────────────────────────────────────────

/// Bottom proof rail with compile stats, line counts, and active example.
#[expect(
    clippy::too_many_arguments,
    reason = "render helper passes proof state"
)]
fn render_status_bar(
    ok: bool,
    failing_stage: Option<&'static str>,
    compile_count: u32,
    duration: std::time::Duration,
    ts_lines: usize,
    py_lines: usize,
    rust_lines: usize,
    example_title: &str,
) -> impl IntoElement {
    let status_color = if ok { theme::success() } else { theme::error() };
    let outcome = match (ok, failing_stage) {
        (true, _) => "ok".to_owned(),
        (false, Some(stage)) => format!("failed at {stage}"),
        (false, None) => "failed".to_owned(),
    };

    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(16.0))
        .py(px(5.0))
        .bg(theme::bg_panel())
        .border_t_1()
        .border_color(theme::border())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .font_family("monospace")
                .text_size(px(10.0))
                .text_color(theme::text_faint())
                .child(div().size(px(6.0)).rounded(px(6.0)).bg(status_color))
                .child(format!("{compile_count} compiled"))
                .child(format!("{duration:.1?}"))
                .child(outcome),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .font_family("monospace")
                .text_size(px(10.0))
                .text_color(theme::text_faint())
                .child(format!("TS {ts_lines}L"))
                .child(format!("PY {py_lines}L"))
                .child(format!("Rust {rust_lines}L"))
                .child(example_title.to_owned()),
        )
}

// ── Buttons ─────────────────────────────────────────────────────────

/// Primary action button with the copper fill.
fn primary_button(
    label: &str,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("btn-{label}")))
        .cursor_pointer()
        .px(px(16.0))
        .py(px(6.0))
        .rounded(px(6.0))
        .bg(theme::primary())
        .text_color(theme::text_on_primary())
        .text_size(px(12.0))
        .font_weight(FontWeight::BOLD)
        .hover(|style| style.opacity(0.88))
        .active(|style| style.opacity(0.7))
        .child(label.to_owned())
        .on_click(handler)
}

/// Ghost button — text with a hover background.
fn ghost_button(
    label: &str,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("ghost-{label}")))
        .cursor_pointer()
        .px(px(12.0))
        .py(px(6.0))
        .rounded(px(6.0))
        .text_color(theme::text_muted())
        .text_size(px(12.0))
        .hover(|style| {
            style
                .bg(theme::bg_hover())
                .text_color(theme::text_primary())
        })
        .active(|style| style.opacity(0.7))
        .child(label.to_owned())
        .on_click(handler)
}
