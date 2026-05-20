//! Smelt GUI — a lightweight GPUI interface for transpiling TypeScript/Python to Rust.
//!
//! Lets users paste source code and see the generated Rust output instantly.

#![expect(
    clippy::expect_used,
    reason = "GPUI window creation is infallible in practice"
)]
#![allow(
    clippy::redundant_pub_crate,
    reason = "GUI submodules use pub(super) for cross-module access within the binary crate"
)]

mod compiler;
mod theme;
mod workspace;

use gpui::{
    AppContext, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size,
};
use gpui_component::Root;
use gpui_component::theme::{Theme, ThemeMode};
use workspace::SmeltWorkspace;

/// Application entry point — opens a single GPUI window.
fn main() {
    Application::new().run(|app: &mut gpui::App| {
        gpui_component::init(app);
        Theme::change(ThemeMode::Dark, None, app);

        let bounds = Bounds::centered(None, size(px(1400.0), px(800.0)), app);
        app.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Smelt".into()),
                    ..TitlebarOptions::default()
                }),
                ..WindowOptions::default()
            },
            |window, outer_cx| {
                let workspace_view = outer_cx.new(|cx| SmeltWorkspace::new(window, cx));
                outer_cx.new(|cx| Root::new(workspace_view, window, cx))
            },
        )
        .expect("failed to open window");
    });
}
