//! Root workspace view — header, dual source editors, output panel, status bar.

use std::time::{Duration, Instant};

use gpui::{
    AppContext, ClickEvent, Context, Entity, FontWeight, Hsla, InteractiveElement, IntoElement,
    ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window,
    div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};

use crate::compiler::{CompileResult, PY_PATH, TS_PATH, compile};
use crate::theme;

/// Sample TypeScript snippet — exports a function the Python side can import.
const SAMPLE_TS: &str = "\
export function add(a: number, b: number): number {
    return a + b;
}
";

/// Sample Python snippet — imports from the TS module by its file stem (`lib`).
const SAMPLE_PY: &str = "\
from lib import add

result: float = add(2.0, 3.0)
print(result)
";

/// Root view of the application.
pub(super) struct SmeltWorkspace {
    /// Editable TypeScript source input.
    ts_input: Entity<InputState>,
    /// Editable Python source input.
    py_input: Entity<InputState>,
    /// Selectable generated Rust or diagnostic output.
    output_input: Entity<InputState>,
    /// Last compilation output text.
    output: String,
    /// Whether last compilation succeeded.
    output_ok: bool,
    /// How long the last compilation took.
    compile_duration: Duration,
    /// Total number of successful compilations this session.
    compile_count: u32,
    /// Kept alive to receive input events.
    _subscriptions: Vec<Subscription>,
}

impl SmeltWorkspace {
    /// Creates the workspace with sample code pre-filled.
    pub(super) fn new(window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let ts_input = cx.new(|input_cx| {
            InputState::new(window, input_cx)
                .code_editor("typescript")
                .placeholder("TypeScript source...")
        });

        let py_input = cx.new(|input_cx| {
            InputState::new(window, input_cx)
                .code_editor("python")
                .placeholder("Python source...")
        });

        let output_input = cx.new(|input_cx| {
            InputState::new(window, input_cx)
                .code_editor("rust")
                .placeholder("Generated Rust will appear here...")
        });

        let sub_ts = cx.subscribe(&ts_input, |this, _entity, event, sub_cx| {
            if matches!(event, InputEvent::Change) {
                this.recompile(sub_cx);
                sub_cx.notify();
            }
        });

        let sub_py = cx.subscribe(&py_input, |this, _entity, event, sub_cx| {
            if matches!(event, InputEvent::Change) {
                this.recompile(sub_cx);
                sub_cx.notify();
            }
        });

        ts_input.update(cx, |state, update_cx| {
            state.set_value(SAMPLE_TS, window, update_cx);
        });
        py_input.update(cx, |state, update_cx| {
            state.set_value(SAMPLE_PY, window, update_cx);
        });

        let start = Instant::now();
        let (output, output_ok) = run_compile(SAMPLE_TS, SAMPLE_PY);
        let compile_duration = start.elapsed();

        output_input.update(cx, |state, update_cx| {
            state.set_value(output.clone(), window, update_cx);
        });

        Self {
            ts_input,
            py_input,
            output_input,
            output,
            output_ok,
            compile_duration,
            compile_count: u32::from(output_ok),
            _subscriptions: vec![sub_ts, sub_py],
        }
    }

    /// Read current source texts and recompile.
    fn recompile(&mut self, cx: &Context<'_, Self>) {
        let ts_text = self.ts_input.read(cx).value().to_string();
        let py_text = self.py_input.read(cx).value().to_string();
        let start = Instant::now();
        let (output, ok) = run_compile(&ts_text, &py_text);
        self.compile_duration = start.elapsed();
        self.output = output;
        self.output_ok = ok;
        if ok {
            self.compile_count = self.compile_count.saturating_add(1);
        }
    }

    /// Restore the built-in cross-language sample and compile it.
    fn reset_sample(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.ts_input.update(cx, |state, update_cx| {
            state.set_value(SAMPLE_TS, window, update_cx);
        });
        self.py_input.update(cx, |state, update_cx| {
            state.set_value(SAMPLE_PY, window, update_cx);
        });
        self.recompile(cx);
        self.sync_output_input(window, cx);
    }

    /// Keep the selectable output editor in sync with the generated text.
    fn sync_output_input(&self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.output_input.read(cx).value() == self.output {
            return;
        }
        self.output_input.update(cx, |state, update_cx| {
            state.set_value(self.output.clone(), window, update_cx);
        });
    }

    /// Count lines in an input entity.
    fn line_count(input: &Entity<InputState>, cx: &Context<'_, Self>) -> usize {
        let val = input.read(cx).value();
        if val.is_empty() {
            0
        } else {
            val.lines().count()
        }
    }
}

/// Run the compiler and return `(output_text, success)`.
fn run_compile(ts_source: &str, py_source: &str) -> (String, bool) {
    match compile(ts_source, py_source) {
        CompileResult::Ok(rust) => (rust, true),
        CompileResult::Err(msg) => (msg, false),
    }
}

impl Render for SmeltWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        self.sync_output_input(window, cx);

        let on_compile = cx.listener(|this, _event: &ClickEvent, _win, ctx| {
            this.recompile(ctx);
            ctx.notify();
        });
        let on_reset = cx.listener(|this, _event: &ClickEvent, win, ctx| {
            this.reset_sample(win, ctx);
            ctx.notify();
        });

        let ts_lines = Self::line_count(&self.ts_input, cx);
        let py_lines = Self::line_count(&self.py_input, cx);
        let out_lines = self.output.lines().count();

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg_primary())
            .text_color(theme::text_primary())
            .font_family("monospace")
            .child(render_header(
                on_compile,
                on_reset,
                self.output_ok,
                self.compile_duration,
            ))
            .child(render_body(
                &self.ts_input,
                &self.py_input,
                &self.output_input,
                self.output_ok,
                ts_lines,
                py_lines,
                out_lines,
            ))
            .child(render_status_bar(
                self.output_ok,
                self.compile_count,
                self.compile_duration,
                ts_lines,
                py_lines,
                out_lines,
            ))
    }
}

// ── Header ──────────────────────────────────────────────────────────

/// Top bar with branding, status pill, and action buttons.
#[expect(clippy::too_many_lines, reason = "single GPUI element tree")]
fn render_header(
    on_compile: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
    on_reset: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
    output_ok: bool,
    duration: Duration,
) -> impl IntoElement {
    let (status_color, status_bg, status_text) = if output_ok {
        (theme::success(), theme::success_dim(), "Ready")
    } else {
        (theme::error(), theme::error_dim(), "Error")
    };

    let timing = format!("{duration:.0?}");

    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(20.0))
        .py(px(10.0))
        .bg(theme::bg_header())
        .border_b_1()
        .border_color(theme::border())
        // Left: branding
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(14.0))
                .child(
                    div()
                        .text_size(px(20.0))
                        .font_weight(FontWeight::EXTRA_BOLD)
                        .text_color(theme::accent())
                        .child("smelt"),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(theme::text_faint())
                        .child("TS + Python → Rust"),
                ),
        )
        // Right: status + buttons
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                // Status pill
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(10.0))
                        .py(px(4.0))
                        .rounded(px(6.0))
                        .bg(status_bg)
                        .child(div().size(px(6.0)).rounded(px(6.0)).bg(status_color))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(status_color)
                                .child(status_text),
                        ),
                )
                // Timing badge
                .child(
                    div()
                        .px(px(8.0))
                        .py(px(3.0))
                        .rounded(px(4.0))
                        .bg(theme::bg_primary())
                        .text_size(px(10.0))
                        .text_color(theme::text_faint())
                        .child(timing),
                )
                .child(ghost_button("Reset", on_reset))
                .child(primary_button("Compile", on_compile)),
        )
}

// ── Body (three-panel editor) ───────────────────────────────────────

/// Three-panel body: TS left, Python center, Rust right (wider).
#[expect(
    clippy::too_many_arguments,
    reason = "GPUI render helper passes panel state"
)]
fn render_body(
    ts_input: &Entity<InputState>,
    py_input: &Entity<InputState>,
    output_input: &Entity<InputState>,
    output_ok: bool,
    ts_lines: usize,
    py_lines: usize,
    out_lines: usize,
) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_row()
        .w_full()
        .overflow_hidden()
        .child(render_editor_panel(
            ts_input,
            "TS",
            TS_PATH,
            ts_lines,
            theme::typescript(),
        ))
        .child(render_editor_panel(
            py_input,
            "PY",
            PY_PATH,
            py_lines,
            theme::python(),
        ))
        .child(render_output_panel(output_input, output_ok, out_lines))
}

/// Editable source code panel with language badge and line count.
fn render_editor_panel(
    input: &Entity<InputState>,
    lang_badge: &str,
    path: &str,
    line_count: usize,
    accent: Hsla,
) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme::border_subtle())
        .child(panel_header(lang_badge, path, line_count, accent))
        .child(
            div()
                .flex_1()
                .bg(theme::bg_surface())
                .child(Input::new(input).appearance(false).bordered(false).h_full()),
        )
}

/// Right panel — generated Rust output or error diagnostics.
fn render_output_panel(
    output_input: &Entity<InputState>,
    output_ok: bool,
    line_count: usize,
) -> impl IntoElement {
    let (accent, badge, path) = if output_ok {
        (theme::rust(), "RS", "output.rs")
    } else {
        (theme::error(), "!!", "errors")
    };

    div()
        .flex()
        .flex_col()
        .min_w(px(0.0))
        // Output panel gets ~40% width
        .flex_basis(px(0.0))
        .flex_grow()
        .flex_shrink()
        .child(panel_header(badge, path, line_count, accent))
        .child(
            div()
                .id("output-scroll")
                .flex_1()
                .bg(theme::bg_surface())
                .child(
                    Input::new(output_input)
                        .appearance(false)
                        .bordered(false)
                        .disabled(true)
                        .h_full(),
                ),
        )
}

/// Panel header with language badge, file path, and line count.
fn panel_header(badge: &str, path: &str, line_count: usize, accent: Hsla) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(12.0))
        .py(px(5.0))
        .bg(theme::bg_header())
        .border_b_1()
        .border_color(theme::border_subtle())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                // Language badge
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
                // File path
                .child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::text_muted())
                        .child(path.to_owned()),
                ),
        )
        // Line count
        .child(
            div()
                .text_size(px(10.0))
                .text_color(theme::text_faint())
                .child(format!("{line_count}L")),
        )
}

// ── Status bar ──────────────────────────────────────────────────────

/// Bottom status bar with compilation stats.
#[expect(
    clippy::too_many_arguments,
    reason = "GPUI render helper passes panel state"
)]
fn render_status_bar(
    output_ok: bool,
    compile_count: u32,
    duration: Duration,
    ts_lines: usize,
    py_lines: usize,
    out_lines: usize,
) -> impl IntoElement {
    let status_color = if output_ok {
        theme::success()
    } else {
        theme::error()
    };

    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(16.0))
        .py(px(3.0))
        .bg(theme::bg_status_bar())
        .border_t_1()
        .border_color(theme::border_subtle())
        // Left: status dot + compile stats
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(div().size(px(5.0)).rounded(px(5.0)).bg(status_color))
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(theme::text_faint())
                        .child(format!("{compile_count} compiled")),
                )
                .child(
                    div()
                        .text_size(px(10.0))
                        .text_color(theme::text_faint())
                        .child(format!("{duration:.1?}")),
                ),
        )
        // Right: line totals
        .child(
            div()
                .text_size(px(10.0))
                .text_color(theme::text_faint())
                .child(format!(
                    "{} source → {} output",
                    ts_lines.saturating_add(py_lines),
                    out_lines
                )),
        )
}

// ── Buttons ─────────────────────────────────────────────────────────

/// Primary action button with accent background and hover state.
fn primary_button(
    label: &str,
    handler: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("btn-{label}")))
        .cursor_pointer()
        .px(px(16.0))
        .py(px(5.0))
        .rounded(px(6.0))
        .bg(theme::accent())
        .text_color(theme::bg_primary())
        .text_size(px(12.0))
        .font_weight(FontWeight::BOLD)
        .hover(|style| style.opacity(0.85))
        .active(|style| style.opacity(0.7))
        .child(label.to_owned())
        .on_click(handler)
}

/// Ghost button — text only with hover background.
fn ghost_button(
    label: &str,
    handler: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(format!("ghost-{label}")))
        .cursor_pointer()
        .px(px(12.0))
        .py(px(5.0))
        .rounded(px(6.0))
        .text_color(theme::text_muted())
        .text_size(px(12.0))
        .hover(|style| {
            style
                .bg(theme::bg_hover())
                .text_color(theme::text_primary())
        })
        .active(|style| style.bg(theme::bg_button()))
        .child(label.to_owned())
        .on_click(handler)
}
