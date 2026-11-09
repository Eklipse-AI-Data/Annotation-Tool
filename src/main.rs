//! Annotation Tool (Rust/GPUI) — a port of the Tkinter Annotation-Tool.
//! Data formats (YOLO .txt, session.json, config.json, class presets) are
//! byte-compatible with the Python original.
//!
//! App home (session.json / config.json / data/) resolves from the
//! `ANNOTATION_TOOL_HOME` env var, a `--home <dir>` flag, or the executable
//! directory (release) / crate directory (debug). Point it at the Python
//! checkout to share state between both tools.

mod actions;
mod classes;
mod images;
mod state;
mod ui;
mod validation;
mod yolo;

use gpui::{prelude::*, App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, px, size};
use ui::theme::ThemeState;
use ui::workspace::Workspace;

fn main() {
    Application::new()
        .with_assets(ui::icons::IconAssets)
        .run(|cx: &mut App| {
        ThemeState::init(cx, "Dark");
        images::ImageStore::init(cx);

        let config = state::config::Config::load();
        actions::apply_keymap(cx, &config);

        let bounds = Bounds::centered(None, size(px(1400.), px(800.)), cx);
        let window = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Annotation Tool".into()),
                    ..Default::default()
                }),
                focus: true,
                ..Default::default()
            },
            |window, cx| {
                let workspace = cx.new(|cx| Workspace::new(cx)); // installs AppState
                // Closing with unsaved edits: autosave, or ask first.
                let ws = workspace.clone();
                window.on_window_should_close(cx, move |window, cx| {
                    ws.update(cx, |ws, cx| ws.should_close(window, cx))
                });
                workspace
            },
        );

        // Live-apply the persisted theme (AppState is installed by now).
        let persisted_theme = ui::canvas::app_state(cx).current_theme_name.clone();
        ThemeState::set(cx, &persisted_theme);
        if let Ok(handle) = window {
            let _ = handle.update(cx, |_view, _window, _cx| {});
        }

        cx.activate(true);
    });
}
