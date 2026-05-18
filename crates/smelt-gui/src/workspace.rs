//! Root workspace view — header, dual source editors, output panel.

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
    /// Compilation output (Rust or error text).
    output: String,
    /// Whether last compilation succeeded.
    output_ok: bool,
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
                .placeholder("Generated Rust...")
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

        let (output, output_ok) = run_compile(SAMPLE_TS, SAMPLE_PY);
        output_input.update(cx, |state, update_cx| {
            state.set_value(output.clone(), window, update_cx);
        });

        Self {
            ts_input,
            py_input,
            output_input,
            output,
            output_ok,
            _subscriptions: vec![sub_ts, sub_py],
        }
    }

    /// Read current source texts and recompile.
    fn recompile(&mut self, cx: &Context<'_, Self>) {
        let ts_text = self.ts_input.read(cx).value().to_string();
        let py_text = self.py_input.read(cx).value().to_string();
        let (output, ok) = run_compile(&ts_text, &py_text);
        self.output = output;
        self.output_ok = ok;
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
    fn sync_output_input(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.output_input.read(cx).value() == self.output {
            return;
        }
        self.output_input.update(cx, |state, update_cx| {
            state.set_value(self.output.clone(), window, update_cx);
        });
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

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::bg_primary())
            .text_color(theme::text_primary())
            .font_family("monospace")
            .child(render_header(on_compile, on_reset, self.output_ok))
            .child(render_body(
                &self.ts_input,
                &self.py_input,
                &self.output_input,
                &self.output,
                self.output_ok,
            ))
    }
}

/// Top bar with logo and compile button.
fn render_header(
    on_compile: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
    on_reset: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
    output_ok: bool,
) -> impl IntoElement {
    let status_color = if output_ok {
        theme::success()
    } else {
        theme::error()
    };
    let status_text = if output_ok {
        "Ready"
    } else {
        "Needs attention"
    };

    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(20.0))
        .py(px(12.0))
        .bg(theme::bg_header())
        .border_b_1()
        .border_color(theme::border())
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .text_size(px(18.0))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::accent())
                        .child("smelt"),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(theme::text_muted())
                        .child("Live TypeScript + Python to Rust"),
                ),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.0))
                        .px(px(10.0))
                        .py(px(4.0))
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(theme::border())
                        .child(div().size(px(7.0)).rounded(px(7.0)).bg(status_color))
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(theme::text_muted())
                                .child(status_text),
                        ),
                )
                .child(secondary_button("Reset sample", on_reset))
                .child(action_button("Compile", on_compile)),
        )
}

/// Three-panel body: TS left, Python center, Rust right.
fn render_body(
    ts_input: &Entity<InputState>,
    py_input: &Entity<InputState>,
    output_input: &Entity<InputState>,
    output: &str,
    output_ok: bool,
) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_row()
        .w_full()
        .overflow_hidden()
        .child(render_editor_panel(
            ts_input,
            TS_PATH,
            "exports functions",
            theme::typescript(),
        ))
        .child(render_editor_panel(
            py_input,
            PY_PATH,
            "imports from lib",
            theme::python(),
        ))
        .child(render_output_panel(output_input, output, output_ok))
}

/// Editable source code panel.
fn render_editor_panel(
    input: &Entity<InputState>,
    label: &str,
    detail: &str,
    label_color: Hsla,
) -> impl IntoElement {
    div()
        .flex_1()
        .flex()
        .flex_col()
        .border_r_1()
        .border_color(theme::border())
        .child(panel_label(label, detail, label_color))
        .child(
            div()
                .flex_1()
                .bg(theme::bg_surface())
                .child(Input::new(input).appearance(false).bordered(false).h_full()),
        )
}

/// Right panel — generated Rust or error output.
fn render_output_panel(
    output_input: &Entity<InputState>,
    output: &str,
    output_ok: bool,
) -> impl IntoElement {
    let label_color = if output_ok {
        theme::rust()
    } else {
        theme::error()
    };
    let label_text = if output_ok {
        "Generated Rust"
    } else {
        "Errors"
    };
    let line_count = output.lines().count();
    let detail = if output_ok {
        format!("{line_count} lines")
    } else {
        "compiler diagnostics".to_owned()
    };

    div()
        .flex_1()
        .flex()
        .flex_col()
        .min_w(px(0.0))
        .child(panel_label(label_text, &detail, label_color))
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

/// Panel header label.
fn panel_label(text: &str, detail: &str, color: Hsla) -> impl IntoElement {
    div()
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .px(px(16.0))
        .py(px(6.0))
        .bg(theme::bg_header())
        .border_b_1()
        .border_color(theme::border())
        .child(
            div()
                .text_size(px(11.0))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(color)
                .child(text.to_owned()),
        )
        .child(
            div()
                .text_size(px(10.0))
                .text_color(theme::text_muted())
                .child(detail.to_owned()),
        )
}

/// Compile action button.
fn action_button(
    label: &str,
    handler: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(label.to_owned()))
        .cursor_pointer()
        .px(px(16.0))
        .py(px(4.0))
        .rounded(px(4.0))
        .bg(theme::success())
        .text_color(theme::bg_primary())
        .text_size(px(12.0))
        .font_weight(FontWeight::BOLD)
        .child(label.to_owned())
        .on_click(handler)
}

/// Lower-emphasis header button for non-primary actions.
fn secondary_button(
    label: &str,
    handler: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    div()
        .id(SharedString::from(label.to_owned()))
        .cursor_pointer()
        .px(px(12.0))
        .py(px(4.0))
        .rounded(px(4.0))
        .border_1()
        .border_color(theme::border())
        .text_color(theme::text_primary())
        .text_size(px(12.0))
        .child(label.to_owned())
        .on_click(handler)
}
