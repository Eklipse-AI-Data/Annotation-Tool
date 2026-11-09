//! The Settings dialog (6 tabs): Keybindings (with live capture), Class
//! Management (add/remove/reorder + remap), Batch Operations, Game Presets,
//! Box Presets and Themes. Mirrors the Python settings dialog flow by flow.

use gpui::{div, prelude::*, px, App, AppContext, Context, Entity, Focusable, InteractiveElement,
    IntoElement, SharedString, Styled, Window};

use crate::autolabel::proxy::{self, ProxyState};
use crate::autolabel::settings::{AutoLabelSettings, DEFAULT_BASE_URL};
use crate::state::config::{gpui_to_tk, BINDABLE_ACTIONS};
use crate::state::paths;

use super::canvas::{app_state, app_state_mut};
use super::entry::Entry;
use super::theme::{ThemeState, THEMES};
use super::widgets::{self, BtnVariant};
use super::workspace::Workspace;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    Keybindings,
    ClassManagement,
    BatchOperations,
    GamePresets,
    BoxPresets,
    Themes,
    AutoLabel,
}

impl SettingsTab {
    fn title(self) -> &'static str {
        match self {
            SettingsTab::Keybindings => "Keys",
            SettingsTab::ClassManagement => "Classes",
            SettingsTab::BatchOperations => "Batch",
            SettingsTab::GamePresets => "Games",
            SettingsTab::BoxPresets => "Box presets",
            SettingsTab::Themes => "Theme",
            SettingsTab::AutoLabel => "Auto Label",
        }
    }
}

const TABS: [SettingsTab; 7] = [
    SettingsTab::Keybindings,
    SettingsTab::ClassManagement,
    SettingsTab::BatchOperations,
    SettingsTab::GamePresets,
    SettingsTab::BoxPresets,
    SettingsTab::Themes,
    SettingsTab::AutoLabel,
];

/// Text fields of the Auto Label settings tab.
struct AutoLabelFields {
    base_url: Entity<Entry>,
    api_key: Entity<Entry>,
    model: String,
    reasoning: String,
    model_open: bool,
    reasoning_open: bool,
    popover_closed_at: Option<std::time::Instant>,
    concurrency: Entity<Entry>,
    max_side: Entity<Entry>,
    max_examples: Entity<Entry>,
    proxy_managed: bool,
    proxy_exe: Entity<Entry>,
    proxy_config: Entity<Entry>,
    /// `(is_error, text)` from the last proxy button.
    proxy_msg: Option<(bool, String)>,
}

impl AutoLabelFields {
    fn new(s: &AutoLabelSettings, cx: &mut Context<SettingsView>) -> Self {
        let field = |placeholder: &str, value: String, cx: &mut Context<SettingsView>| {
            cx.new(|cx| {
                let mut e = Entry::new(placeholder, cx);
                e.content = value.into();
                e
            })
        };
        Self {
            base_url: field(DEFAULT_BASE_URL, s.base_url.clone(), cx),
            api_key: field("Client key (made up automatically when the app runs the proxy)", s.api_key.clone(), cx),
            model: s.model.clone(),
            reasoning: s.reasoning_effort.clone(),
            model_open: false,
            reasoning_open: false,
            popover_closed_at: None,
            concurrency: field("3", s.concurrency.to_string(), cx),
            max_side: field("1280", s.max_side.to_string(), cx),
            max_examples: field("3", s.max_examples_per_class.to_string(), cx),
            proxy_managed: s.proxy_managed,
            proxy_exe: field("Auto-detect: cliproxy\\cli-proxy-api.exe in the app folder, or PATH", s.proxy_exe.clone(), cx),
            proxy_config: field("Empty = the app writes its own config", s.proxy_config.clone(), cx),
            proxy_msg: None,
        }
    }

    /// Settings as currently typed (unparseable numbers keep `base` values).
    fn read(&self, base: &AutoLabelSettings, cx: &App) -> AutoLabelSettings {
        let text = |e: &Entity<Entry>| e.read(cx).text().trim().to_string();
        let mut s = base.clone();
        s.base_url = text(&self.base_url);
        if s.base_url.is_empty() {
            s.base_url = DEFAULT_BASE_URL.to_string();
        }
        s.api_key = text(&self.api_key);
        s.model = self.model.trim().to_string();
        s.reasoning_effort = self.reasoning.clone();
        if let Ok(v) = text(&self.concurrency).parse::<usize>() {
            s.concurrency = v.clamp(1, 16);
        }
        if let Ok(v) = text(&self.max_side).parse::<u32>() {
            s.max_side = v.clamp(256, 4096);
        }
        if let Ok(v) = text(&self.max_examples).parse::<usize>() {
            s.max_examples_per_class = v.clamp(0, 10);
        }
        s.proxy_managed = self.proxy_managed;
        s.proxy_exe = text(&self.proxy_exe).trim_matches('"').to_string();
        s.proxy_config = text(&self.proxy_config).trim_matches('"').to_string();
        s
    }
}

/// Game preset files in `data/` (every `.txt` except the active class list).
fn scan_game_presets() -> Vec<String> {
    let mut files: Vec<String> = std::fs::read_dir(paths::data_dir())
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.to_lowercase().ends_with(".txt") && n != "predefined_classes.txt")
        .collect();
    files.sort();
    files
}

pub struct SettingsView {
    workspace: Entity<Workspace>,
    tab: SettingsTab,
    capture_target: Option<&'static str>,
    temp_classes: Vec<String>,
    temp_selected: Option<usize>,
    new_class: Entity<Entry>,
    batch_old: Option<i64>,
    batch_new: Option<i64>,
    preset_files: Vec<String>,
    preset_selected: Option<usize>,
    box_slot_selected: Option<u8>,
    auto_label: AutoLabelFields,
    focus: gpui::FocusHandle,
}

impl SettingsView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let temp_classes: Vec<String> =
            app_state(cx).classes.iter().map(|c| c.name.clone()).collect();
        let preset_files = scan_game_presets();
        let new_class = cx.new(|cx| Entry::new("New class name…", cx));
        let al_settings = AutoLabelSettings::from_config(&app_state(cx).config.raw);
        let auto_label = AutoLabelFields::new(&al_settings, cx);
        Self {
            workspace,
            tab: SettingsTab::Keybindings,
            capture_target: None,
            temp_classes,
            temp_selected: None,
            new_class,
            batch_old: None,
            batch_new: None,
            preset_files,
            preset_selected: None,
            box_slot_selected: None,
            auto_label,
            focus: cx.focus_handle(),
        }
    }

    pub fn select_tab(&mut self, tab: SettingsTab, cx: &mut Context<Self>) {
        self.tab = tab;
        self.capture_target = None;
        cx.notify();
    }

    fn auto_label_settings(&self, cx: &App) -> AutoLabelSettings {
        let base = AutoLabelSettings::from_config(&app_state(cx).config.raw);
        self.auto_label.read(&base, cx)
    }

    /// Reloads the model list from the endpoint as currently typed.
    fn test_auto_label_connection(&mut self, cx: &mut Context<Self>) {
        let settings = self.auto_label_settings(cx);
        super::autolabel_ui::refresh_models(settings, cx);
        cx.notify();
    }

    fn popover_just_closed(&self) -> bool {
        self.auto_label
            .popover_closed_at
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(250))
    }

    /// Typed settings, with a client key made up if the app writes the proxy
    /// config and none is set (shown in the API key field).
    fn auto_label_settings_with_key(&mut self, cx: &mut Context<Self>) -> AutoLabelSettings {
        let mut s = self.auto_label_settings(cx);
        if s.proxy_config.is_empty() && s.api_key.is_empty() {
            s.api_key = proxy::generate_key();
            let key = s.api_key.clone();
            self.auto_label.api_key.update(cx, |e, cx| e.set_text(&key, cx));
        }
        s
    }

    fn proxy_start(&mut self, cx: &mut Context<Self>) {
        let s = self.auto_label_settings_with_key(cx);
        self.auto_label.proxy_msg = None;
        proxy::stop();
        super::autolabel_ui::proxy_start_with(s, cx);
        cx.notify();
    }

    fn proxy_stop(&mut self, cx: &mut Context<Self>) {
        proxy::stop();
        self.auto_label.proxy_msg = None;
        cx.notify();
    }

    fn proxy_sign_in(&mut self, cx: &mut Context<Self>) {
        let s = self.auto_label_settings_with_key(cx);
        self.auto_label.proxy_msg = Some(match super::autolabel_ui::proxy_codex_login(&s, cx) {
            Ok(()) => (false, "Sign-in window opened. Finish the ChatGPT login in your browser; the proxy restarts when that window closes.".into()),
            Err(e) => (true, e),
        });
        cx.notify();
    }

    fn proxy_browse_exe(&mut self, cx: &mut Context<Self>) {
        let entry = self.auto_label.proxy_exe.clone();
        cx.spawn(async move |_this: gpui::WeakEntity<SettingsView>, cx: &mut gpui::AsyncApp| {
            let picked = rfd::AsyncFileDialog::new()
                .set_title("Select cli-proxy-api.exe")
                .add_filter("Program", &["exe"])
                .pick_file()
                .await;
            if let Some(path) = picked.and_then(|f| f.path().to_str().map(|s| s.to_string())) {
                let _ = cx.update(|cx| entry.update(cx, |e, cx| e.set_text(&path, cx)));
            }
        })
        .detach();
    }

    fn on_capture_key(&mut self, event: &gpui::KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.capture_target else {
            return;
        };
        let key = &event.keystroke.key;
        let m = &event.keystroke.modifiers;
        // Ignore bare modifier presses (Python parity).
        if matches!(
            key.to_ascii_lowercase().as_str(),
            "control" | "shift" | "alt" | "meta" | "cmd" | "secondary" | "capslock"
        ) {
            return;
        }
        if key == "escape" {
            self.capture_target = None;
            cx.notify();
            return;
        }
        if let Some(pattern) = gpui_to_tk(m.control, m.alt, m.shift, m.secondary(), key) {
            app_state_mut(cx).config.set_binding(target, &pattern);
        }
        self.capture_target = None;
        cx.notify();
    }

    fn save_and_close(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let al = self.auto_label_settings(cx);
        al.store(&mut app_state_mut(cx).config.raw);
        let config = app_state(cx).config.clone();
        config.save();
        // Start/stop/restart CLIProxyAPI to match (no-op if nothing changed).
        super::autolabel_ui::proxy_apply(cx);
        let ws = self.workspace.clone();
        cx.spawn(async move |_this: gpui::WeakEntity<SettingsView>, cx: &mut gpui::AsyncApp| {
            let _ = cx.update(|cx| {
                let cfg = app_state(cx).config.clone();
                crate::actions::apply_keymap(cx, &cfg);
                ws.update(cx, |ws, cx| {
                    ws.modal = None;
                    ws.alert("Success", "Settings saved!");
                    cx.notify();
                });
            });
        })
        .detach();
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = ThemeState::current(cx);

        let header = div()
            .flex_none()
            .h(px(48.))
            .px(px(20.))
            .flex()
            .items_center()
            .justify_between()
            .border_b_1()
            .border_color(theme.rgb(theme.divider))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(super::icons::icon("sliders", 15.0, theme.rgb(theme.fg_muted)))
                    .child(
                        div()
                            .text_size(px(14.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.rgb(theme.fg_highlight))
                            .child("Settings"),
                    ),
            )
            .child(
                div()
                    .id("settings-close-x")
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.14)))
                    .child(super::icons::icon("x", 14.0, theme.rgb(theme.fg_muted)))
                    .on_click(cx.listener(|view: &mut SettingsView, _, _, cx| {
                        view.capture_target = None;
                        view.workspace.update(cx, |ws, cx| {
                            ws.modal = None;
                            cx.notify();
                        });
                    })),
            );

        let tab_bar = widgets::segmented_control(theme)
            .flex_none()
            .mx(px(20.))
            .mt(px(14.))
            .child(
                div().flex_1().flex().gap(px(2.)).children(TABS.map(|tab| {
                    widgets::tab(
                        ("settings-tab", tab as usize),
                        tab.title(),
                        self.tab == tab,
                        theme,
                    )
                    .on_click({
                        cx.listener(move |view: &mut SettingsView, _, _, cx| {
                            view.tab = tab;
                            view.capture_target = None;
                            cx.notify();
                        })
                    })
                })),
            );

        let content = match self.tab {
            SettingsTab::Keybindings => self.render_keybindings(cx),
            SettingsTab::ClassManagement => self.render_class_management(cx),
            SettingsTab::BatchOperations => self.render_batch(cx),
            SettingsTab::GamePresets => self.render_game_presets(cx),
            SettingsTab::BoxPresets => self.render_box_presets(cx),
            SettingsTab::Themes => self.render_themes(cx),
            SettingsTab::AutoLabel => self.render_auto_label(cx),
        };

        widgets::panel(theme)
            .id("settings")
            .w(px(700.))
            .h(px(600.))
            .max_h(window.viewport_size().height - px(40.))
            .flex_col()
            .overflow_hidden()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_capture_key))
            .child(header)
            .child(tab_bar)
            .child(
                div()
                    .id("settings-content")
                    .px(px(20.))
                    .py(px(16.))
                    .mt(px(12.))
                    .mx(px(20.))
                    .mb(px(14.))
                    .flex_1()
                    .min_h_0()
                    .rounded(px(10.))
                    .bg(theme.rgba(theme.fg_muted, 0.05))
                    .overflow_y_scroll()
                    .child(content),
            )
            .child(
                div()
                    .flex_none()
                    .px(px(20.))
                    .py(px(12.))
                    .flex()
                    .justify_end()
                    .border_t_1()
                    .border_color(theme.rgb(theme.divider))
                    .child(
                        widgets::button(
                            "settings-save",
                            "Save & Close",
                            BtnVariant::Accent,
                            theme,
                        )
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.save_and_close(window, cx);
                        })),
                    ),
            )
            .into_any_element()
    }
}

impl SettingsView {
    fn render_keybindings(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let mut rows = Vec::new();
        for (action_id, label) in BINDABLE_ACTIONS {
            let binding = app_state(cx).config.display_binding(action_id);
            let capturing = self.capture_target == Some(*action_id);
            let action_id: &'static str = action_id;
            rows.push(
                div()
                    .id(("kb-row", BINDABLE_ACTIONS.iter().position(|(a, _)| *a == action_id).unwrap_or(0)))
                    .flex()
                    .items_center()
                    .justify_between()
                    .py_1()
                    .child(widgets::body_text(*label, theme))
                    .child(
                        widgets::button(
                            ("kb-btn", BINDABLE_ACTIONS.iter().position(|(a, _)| *a == action_id).unwrap_or(0)),
                            if capturing { "Press any key…".to_string() } else { binding },
                            if capturing { BtnVariant::Accent } else { BtnVariant::Default },
                            theme,
                        )
                        .on_click({
                            cx.listener(move |view: &mut SettingsView, _, _, cx| {
                                view.capture_target = Some(action_id);
                                cx.notify();
                            })
                        }),
                    )
                    .into_any_element(),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(widgets::caption("Click a key to rebind it. Press Escape to cancel capture.", theme))
            .children(rows)
            .into_any_element()
    }

    fn render_class_management(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let mut list = div().id("cm-list").max_h(px(260.)).overflow_y_scroll().flex().flex_col().gap_0p5().rounded(px(8.)).p(px(4.)).bg(theme.rgba(theme.fg_muted, 0.06));
        for (i, name) in self.temp_classes.iter().enumerate() {
            let selected = self.temp_selected == Some(i);
            list = list.child(
                div()
                    .id(("cm-row", i))
                    .flex_none()
                    .px_2()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .text_size(px(12.0))
                    .cursor_pointer()
                    .when(selected, |el| el.bg(theme.selection_bg()))
                    .when(!selected, |el| el.hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.1))))
                    .child(name.clone())
                    .on_click({
                        cx.listener(move |view: &mut SettingsView, _, _, cx| {
                            view.temp_selected = Some(i);
                            cx.notify();
                        })
                    }),
            );
        }

        let _temp = self.temp_classes.clone();
        let ws = self.workspace.clone();

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Manage Classes"),
            )
            .child(widgets::caption(
                "Add, remove, and reorder classes. On apply, all annotation files are remapped by class name (with a backup).",
                theme,
            ))
            .child(self.new_class.clone())
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        widgets::button("cm-add", "Add Class", BtnVariant::Default, theme).on_click(
                            cx.listener(|view: &mut SettingsView, _, _, cx| {
                                let name = view.new_class.read(cx).text().trim().to_string();
                                if !name.is_empty() && !view.temp_classes.contains(&name) {
                                    view.temp_classes.push(name.clone());
                                    view.new_class.update(cx, |entry, cx| entry.set_text("", cx));
                                }
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        widgets::button("cm-remove", "Remove Selected", BtnVariant::Default, theme)
                            .on_click(cx.listener(|view: &mut SettingsView, _, _, cx| {
                                if let Some(ix) = view.temp_selected {
                                    if ix < view.temp_classes.len() {
                                        view.temp_classes.remove(ix);
                                        view.temp_selected = None;
                                    }
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        widgets::button("cm-up", "Move Up", BtnVariant::Default, theme).on_click(
                            cx.listener(|view: &mut SettingsView, _, _, cx| {
                                if let Some(ix) = view.temp_selected && ix > 0 {
                                    view.temp_classes.swap(ix, ix - 1);
                                    view.temp_selected = Some(ix - 1);
                                    cx.notify();
                                }
                            }),
                        ),
                    )
                    .child(
                        widgets::button("cm-down", "Move Down", BtnVariant::Default, theme).on_click(
                            cx.listener(|view: &mut SettingsView, _, _, cx| {
                                if let Some(ix) = view.temp_selected
                                    && ix + 1 < view.temp_classes.len()
                                {
                                    view.temp_classes.swap(ix, ix + 1);
                                    view.temp_selected = Some(ix + 1);
                                    cx.notify();
                                }
                            }),
                        ),
                    ),
            )
            .child(list)
            .child(
                widgets::button("cm-apply", "Apply Changes", BtnVariant::Accent, theme).on_click(
                    cx.listener(move |view: &mut SettingsView, _, window, cx| {
                        let temp = view.temp_classes.clone();
                        let ws = ws.clone();
                        view.tab = SettingsTab::ClassManagement;
                        ws.update(cx, |ws, cx| {
                            ws.apply_class_changes(temp, cx);
                            let _ = window;
                        });
                        cx.notify();
                    }),
                ),
            )
            .into_any_element()
    }

    fn render_batch(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let classes: Vec<(i64, String)> =
            app_state(cx).classes.iter().map(|c| (c.id, c.name.clone())).collect();

        let make_list = |id: &'static str| {
            div().id(id).max_h(px(300.)).overflow_y_scroll().flex().flex_col().rounded(px(8.)).p(px(4.)).bg(theme.rgba(theme.fg_muted, 0.06))
        };
        let mut old_list = make_list("batch-old-list");
        let mut new_list = make_list("batch-new-list");
        for (id, name) in &classes {
            let id = *id;
            old_list = old_list.child(
                div()
                    .id(("batch-old", id as usize))
                    .flex_none()
                    .px_2()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .text_size(px(11.5))
                    .cursor_pointer()
                    .when(self.batch_old == Some(id), |el| {
                        el.bg(theme.selection_bg())
                    })
                    .when(self.batch_old != Some(id), |el| {
                        el.hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.1)))
                    })
                    .child(format!("{id}: {name}"))
                    .on_click(cx.listener(move |view: &mut SettingsView, _, _, cx| {
                        view.batch_old = Some(id);
                        cx.notify();
                    })),
            );
            new_list = new_list.child(
                div()
                    .id(("batch-new", id as usize))
                    .flex_none()
                    .px_2()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .text_size(px(11.5))
                    .cursor_pointer()
                    .when(self.batch_new == Some(id), |el| {
                        el.bg(theme.selection_bg())
                    })
                    .when(self.batch_new != Some(id), |el| {
                        el.hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.1)))
                    })
                    .child(format!("{id}: {name}"))
                    .on_click(cx.listener(move |view: &mut SettingsView, _, _, cx| {
                        view.batch_new = Some(id);
                        cx.notify();
                    })),
            );
        }

        let _old_sel = self.batch_old;
        let _new_sel = self.batch_new;

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Batch Replace Class IDs"),
            )
            .child(widgets::caption(
                "Replace every instance of one class ID with another across all annotation files. A timestamped backup is created first.",
                theme,
            ))
            .child(
                div()
                    .flex()
                    .gap_3()
                    .child(div().flex_1().flex_col().gap_1().child(widgets::caption("Old ID", theme)).child(old_list))
                    .child(div().flex_1().flex_col().gap_1().child(widgets::caption("New ID", theme)).child(new_list)),
            )
            .child(
                widgets::button("batch-execute", "Execute Batch Replace", BtnVariant::Accent, theme)
                    .on_click(cx.listener(move |view: &mut SettingsView, _, window, cx| {
                        let (Some(old), Some(new)) = (view.batch_old, view.batch_new) else {
                            view.alert_local(cx, "Warning", "Please select both old and new classes.");
                            cx.notify();
                            let _ = window;
                            return;
                        };
                        let ws = view.workspace.clone();
                        ws.update(cx, |ws, cx| {
                            ws.modal = None;
                            ws.execute_batch_replace(old, new, cx);
                        });
                    })),
            )
            .into_any_element()
    }

    fn alert_local(&mut self, cx: &mut Context<Self>, title: &str, message: &str) {
        // Defer onto the workspace's modal system (closes settings first).
        self.workspace.update(cx, |ws, cx| {
            ws.alert(title, message);
            cx.notify();
        });
    }

    fn render_game_presets(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let mut list = div().id("gp-list").max_h(px(320.)).overflow_y_scroll().flex().flex_col().rounded(px(8.)).p(px(4.)).bg(theme.rgba(theme.fg_muted, 0.06));
        for (i, name) in self.preset_files.iter().enumerate() {
            let selected = self.preset_selected == Some(i);
            list = list.child(
                div()
                    .id(("gp-row", i))
                    .flex_none()
                    .px_2()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .text_size(px(11.5))
                    .cursor_pointer()
                    .when(selected, |el| el.bg(theme.selection_bg()))
                    .when(!selected, |el| el.hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.1))))
                    .child(name.clone())
                    .on_click(cx.listener(move |view: &mut SettingsView, _, _, cx| {
                        view.preset_selected = Some(i);
                        cx.notify();
                    })),
            );
        }
        let files = self.preset_files.clone();
        let _selected = self.preset_selected;

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Switch Game Classes"),
            )
            .child(list)
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        widgets::button("gp-load", "Load Selected", BtnVariant::Accent, theme).on_click(
                            cx.listener(move |view: &mut SettingsView, _, window, cx| {
                                let Some(ix) = view.preset_selected else { return; };
                                let Some(name) = files.get(ix).cloned() else { return; };
                                let ws = view.workspace.clone();
                                ws.update(cx, |ws, cx| {
                                    ws.modal = None;
                                    ws.guard_unsaved(move |ws, cx| ws.load_game_preset(&name, cx), cx);
                                });
                                let _ = window;
                            }),
                        ),
                    )
                    .child(
                        widgets::button("gp-refresh", "Refresh", BtnVariant::Default, theme).on_click(
                            cx.listener(|view: &mut SettingsView, _, _, cx| {
                                view.preset_files = scan_game_presets();
                                view.preset_selected = None;
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .into_any_element()
    }

    fn render_box_presets(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let presets = app_state(cx).presets.clone();
        let mut list = div().flex_col().rounded(px(8.)).p(px(4.)).bg(theme.rgba(theme.fg_muted, 0.06));
        for slot in 1u8..=9 {
            let n = presets.get(&slot).map(|v| v.len()).unwrap_or(0);
            let label: SharedString = if n > 0 {
                format!("  Slot {slot}: {n} box(es)").into()
            } else {
                format!("  Slot {slot}: Empty").into()
            };
            let selected = self.box_slot_selected == Some(slot);
            list = list.child(
                div()
                    .id(("bp-row", slot as usize))
                    .px_2()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .rounded_sm()
                    .text_size(px(11.5))
                    .cursor_pointer()
                    .when(selected, |el| el.bg(theme.selection_bg()))
                    .when(!selected, |el| el.hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.1))))
                    .child(label)
                    .on_click(cx.listener(move |view: &mut SettingsView, _, _, cx| {
                        view.box_slot_selected = Some(slot);
                        cx.notify();
                    })),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .child("Manage Bounding Box Presets"),
            )
            .child(widgets::caption(
                "View and manage bounding box slots (1-9).\nPresets are remembered per project and class set across app restarts.",
                theme,
            ))
            .child(list)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .child(
                        widgets::button("bp-preview", "Preview Selected Slot", BtnVariant::Default, theme)
                            .on_click(cx.listener(|view: &mut SettingsView, _, window, cx| {
                                let Some(slot) = view.box_slot_selected else {
                                    view.alert_local(cx, "Warning", "Please select a slot in the list first.");
                                    let _ = window;
                                    cx.notify();
                                    return;
                                };
                                let ws = view.workspace.clone();
                                ws.update(cx, |ws, cx| {
                                    ws.preview_preset(slot, true, cx);
                                });
                            })),
                    )
                    .child(
                        widgets::button(
                            "bp-save",
                            "Save Selected/All to Selected Slot",
                            BtnVariant::Accent,
                            theme,
                        )
                        .on_click(cx.listener(|view: &mut SettingsView, _, window, cx| {
                            let Some(slot) = view.box_slot_selected else {
                                view.alert_local(cx, "Warning", "Please select a slot in the list first.");
                                let _ = window;
                                cx.notify();
                                return;
                            };
                            let ws = view.workspace.clone();
                            ws.update(cx, |ws, cx| {
                                ws.save_preset(slot, cx);
                            });
                        })),
                    )
                    .child(
                        widgets::button("bp-clear", "Clear All Presets", BtnVariant::Danger, theme)
                            .on_click(cx.listener(|view: &mut SettingsView, _, window, cx| {
                                let ws = view.workspace.clone();
                                ws.update(cx, |ws, cx| {
                                    ws.modal = None;
                                    ws.clear_all_presets(cx);
                                });
                                let _ = window;
                            })),
                    ),
            )
            .into_any_element()
    }

    fn render_themes(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let current_name = app_state(cx).current_theme_name.clone();

        // Preview chips showing each theme's surface + accent.
        let mut cards = Vec::new();
        for t in &THEMES {
            let name = t.name;
            let is_current = name == current_name;
            cards.push(
                div()
                    .id(("theme-card", THEMES.iter().position(|t| t.name == name).unwrap_or(0)))
                    .flex_1()
                    .p(px(14.))
                    .flex_col()
                    .gap(px(12.))
                    .rounded(px(10.))
                    .bg(theme.rgb(theme.list_bg))
                    .border_1()
                    .border_color(if is_current {
                        theme.rgb(theme.accent)
                    } else {
                        theme.rgba(theme.border, 0.9)
                    })
                    .cursor_pointer()
                    .hover(|s| s.border_color(theme.rgb(theme.border)))
                    .child(
                        div()
                            .h(px(56.))
                            .rounded(px(6.))
                            .bg(gpui::rgb(t.bg_main))
                            .border_1()
                            .border_color(gpui::rgba(0x00000040))
                            .child(
                                div()
                                    .flex()
                                    .h(px(18.))
                                    .mt(px(8.))
                                    .px(px(6.))
                                    .gap(px(4.))
                                    .child(div().size(px(10.)).rounded(px(3.)).bg(gpui::rgb(t.bg_hover)))
                                    .child(div().size(px(10.)).rounded(px(3.)).bg(gpui::rgb(t.accent)))
                                    .child(div().size(px(10.)).rounded(px(3.)).bg(gpui::rgb(t.fg_muted))),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(if is_current {
                                        gpui::FontWeight::SEMIBOLD
                                    } else {
                                        gpui::FontWeight::MEDIUM
                                    })
                                    .text_color(theme.rgb(theme.fg_text))
                                    .child(name),
                            )
                            .when(is_current, |el| {
                                el.child(super::icons::icon(
                                    "check",
                                    13.0,
                                    theme.rgb(theme.accent),
                                ))
                            }),
                    )
                    .on_click(cx.listener(move |_view: &mut SettingsView, _, _, cx| {
                        // Live re-theme (applied immediately).
                        ThemeState::set(cx, name);
                        app_state_mut(cx).current_theme_name = name.to_string();
                        app_state_mut(cx).save_session();
                        cx.notify();
                    })),
            );
        }

        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .text_size(px(13.5))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.rgb(theme.fg_highlight))
                    .child("Appearance"),
            )
            .child(widgets::caption(
                "Select a theme to apply it immediately.",
                theme,
            ))
            .child(div().flex().gap(px(10.)).children(cards))
            .into_any_element()
    }

    fn render_auto_label(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let row = |label: &'static str, entry: &Entity<Entry>, hint: &'static str| {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(div().w(px(150.)).flex_none().child(widgets::body_text(label, theme)))
                .child(div().flex_1().min_w_0().child(entry.clone()))
                .when(!hint.is_empty(), |el| {
                    el.child(div().w(px(80.)).flex_none().child(widgets::caption(hint, theme)))
                })
        };
        let label_cell = |label: &'static str| div().w(px(150.)).flex_none().child(widgets::body_text(label, theme));

        // Model dropdown (+ connection test / reload).
        let model = self.auto_label.model.clone();
        let mut model_dd = div().id("al-set-model-dd").flex_1().min_w_0().child(
            widgets::dropdown_button(
                "al-set-model-btn",
                if model.is_empty() { "Select a model…".to_string() } else { model.clone() },
                model.is_empty(),
                self.auto_label.model_open,
                theme,
            )
            .on_click(cx.listener(|view: &mut SettingsView, _, _, cx| {
                if !view.popover_just_closed() {
                    view.auto_label.model_open = !view.auto_label.model_open;
                    view.auto_label.reasoning_open = false;
                    let empty = super::autolabel_ui::model_list(cx).is_none_or(|l| !l.loaded && !l.loading);
                    if view.auto_label.model_open && empty {
                        view.test_auto_label_connection(cx);
                    }
                }
                cx.notify();
            })),
        );
        if self.auto_label.model_open {
            let rows = super::autolabel_ui::model_rows(
                "al-set-model",
                &model,
                theme,
                cx,
                |view: &mut SettingsView, m, cx| {
                    view.auto_label.model = m;
                    view.auto_label.model_open = false;
                    cx.notify();
                },
                |view: &mut SettingsView, cx| view.test_auto_label_connection(cx),
            );
            model_dd = model_dd.child(widgets::popover("al-set-model-pop", px(320.), px(320.), theme, rows,
                cx.listener(|view: &mut SettingsView, _, _, cx| {
                    view.auto_label.model_open = false;
                    view.auto_label.popover_closed_at = Some(std::time::Instant::now());
                    cx.notify();
                })));
        }

        // Reasoning effort dropdown.
        let reasoning = self.auto_label.reasoning.clone();
        let mut reasoning_dd = div().id("al-set-reasoning-dd").flex_1().min_w_0().child(
            widgets::dropdown_button(
                "al-set-reasoning-btn",
                crate::autolabel::settings::reasoning_label(&reasoning),
                reasoning.is_empty(),
                self.auto_label.reasoning_open,
                theme,
            )
            .on_click(cx.listener(|view: &mut SettingsView, _, _, cx| {
                if !view.popover_just_closed() {
                    view.auto_label.reasoning_open = !view.auto_label.reasoning_open;
                    view.auto_label.model_open = false;
                }
                cx.notify();
            })),
        );
        if self.auto_label.reasoning_open {
            let rows = super::autolabel_ui::reasoning_rows("al-set-reasoning", &reasoning, theme, cx,
                |view: &mut SettingsView, level, cx| {
                    view.auto_label.reasoning = level;
                    view.auto_label.reasoning_open = false;
                    cx.notify();
                });
            reasoning_dd = reasoning_dd.child(widgets::popover("al-set-reasoning-pop", px(240.), px(300.), theme, rows,
                cx.listener(|view: &mut SettingsView, _, _, cx| {
                    view.auto_label.reasoning_open = false;
                    view.auto_label.popover_closed_at = Some(std::time::Instant::now());
                    cx.notify();
                })));
        }

        let (loading, status): (bool, Option<(bool, String)>) = match super::autolabel_ui::model_list(cx) {
            Some(l) if l.loading => (true, Some((false, "Loading models…".to_string()))),
            Some(l) => match &l.error {
                Some(e) => (false, Some((true, e.clone()))),
                None if l.loaded => (false, Some((false, format!("Connected: {} model(s) can label images.", l.models.len())))),
                None => (false, None),
            },
            None => (false, None),
        };
        let testing = loading;
        let f = &self.auto_label;

        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(px(13.5))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.rgb(theme.fg_highlight))
                    .child("Auto Label endpoint"),
            )
            .child(widgets::caption(
                "Any OpenAI-compatible endpoint. For a Codex subscription use CLIProxyAPI at http://127.0.0.1:8317/v1; the app can start and stop it for you (see CLIProxyAPI below).",
                theme,
            ))
            .child(row("Endpoint URL", &f.base_url, ""))
            .child(row("API key", &f.api_key, ""))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(label_cell("Model"))
                    .child(model_dd),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .child(label_cell("Reasoning effort"))
                    .child(reasoning_dd),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        widgets::button(
                            "al-test-conn",
                            if testing { "Testing…" } else { "Test connection" },
                            BtnVariant::Default,
                            theme,
                        )
                        .on_click(cx.listener(|view: &mut SettingsView, _, _, cx| view.test_auto_label_connection(cx))),
                    )
                    .when_some(status, |el, (is_err, text)| {
                        el.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .text_size(px(11.5))
                                .line_height(px(16.))
                                .text_color(if is_err { theme.rgb(theme.error) } else { theme.rgb(theme.fg_muted) })
                                .child(text),
                        )
                    }),
            )
            .child(div().mt(px(6.)).child(widgets::h_divider(theme)))
            .child(self.render_proxy_section(cx))
            .child(div().mt(px(6.)).child(widgets::h_divider(theme)))
            .child(row("Parallel requests", &f.concurrency, "1–16"))
            .child(row("Max image side (px)", &f.max_side, "256–4096"))
            .child(row("Examples per class", &f.max_examples, "0–10"))
            .child(widgets::caption(
                "Changes apply after Save & Close. Larger images and more examples improve accuracy but use more of your usage limits per image.",
                theme,
            ))
            .into_any_element()
    }

    fn render_proxy_section(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let f = &self.auto_label;
        let managed = f.proxy_managed;
        let typed = f.read(&AutoLabelSettings::default(), cx);
        let exe = proxy::find_exe(&typed.proxy_exe);
        let exe_line: (bool, String) = match &exe {
            Some(p) => (false, format!("Using {}", p.display())),
            None if typed.proxy_exe.is_empty() => (
                true,
                format!("cli-proxy-api.exe not found. Put it in {} or browse to it.", proxy::proxy_dir().display()),
            ),
            None => (true, format!("{} does not exist.", typed.proxy_exe)),
        };
        let accounts = proxy::codex_accounts();
        let account_line = if accounts.is_empty() {
            "No Codex account signed in yet. Use Sign in to Codex.".to_string()
        } else {
            format!("Codex account: {}", accounts.join(", "))
        };
        let state = proxy::state();
        let (state_err, state_text) = super::autolabel_ui::proxy_status_line(&state);
        let running = matches!(state, ProxyState::Running { .. } | ProxyState::Starting);
        let signing_in = proxy::login_running();
        let msg = f.proxy_msg.clone();
        let line = |is_err: bool, text: String| {
            div()
                .text_size(px(11.5))
                .line_height(px(16.))
                .text_color(if is_err { theme.rgb(theme.error) } else { theme.rgb(theme.fg_muted) })
                .child(text)
        };
        let row = |label: &'static str, entry: &Entity<Entry>| {
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(div().w(px(150.)).flex_none().child(widgets::body_text(label, theme)))
                .child(div().flex_1().min_w_0().child(entry.clone()))
        };

        div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(px(13.5))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.rgb(theme.fg_highlight))
                    .child("CLIProxyAPI"),
            )
            .child(
                widgets::checkbox(
                    "al-proxy-managed",
                    "Start CLIProxyAPI when the app opens and stop it when the app closes",
                    managed,
                    theme,
                )
                .on_click(cx.listener(|view: &mut SettingsView, _, _, cx| {
                    view.auto_label.proxy_managed = !view.auto_label.proxy_managed;
                    cx.notify();
                })),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(row("cli-proxy-api.exe", &f.proxy_exe)))
                    .child(
                        widgets::button("al-proxy-browse", "Browse…", BtnVariant::Default, theme)
                            .on_click(cx.listener(|view: &mut SettingsView, _, _, cx| view.proxy_browse_exe(cx))),
                    ),
            )
            .child(line(exe_line.0, exe_line.1))
            .child(row("Proxy config", &f.proxy_config))
            .child(line(false, account_line))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        widgets::button(
                            "al-proxy-signin",
                            if signing_in { "Signing in…" } else { "Sign in to Codex…" },
                            BtnVariant::Default,
                            theme,
                        )
                        .on_click(cx.listener(|view: &mut SettingsView, _, _, cx| view.proxy_sign_in(cx))),
                    )
                    .child(
                        widgets::button("al-proxy-start", if running { "Restart" } else { "Start" }, BtnVariant::Default, theme)
                            .on_click(cx.listener(|view: &mut SettingsView, _, _, cx| view.proxy_start(cx))),
                    )
                    .when(running, |el| {
                        el.child(
                            widgets::button("al-proxy-stop", "Stop", BtnVariant::Default, theme)
                                .on_click(cx.listener(|view: &mut SettingsView, _, _, cx| view.proxy_stop(cx))),
                        )
                    })
                    .when(proxy::log_path().is_file(), |el| {
                        el.child(widgets::header_link("al-proxy-log", "Open log", theme).on_click(|_, _, _| {
                            let _ = std::process::Command::new("notepad.exe").arg(proxy::log_path()).spawn();
                        }))
                    }),
            )
            .child(line(state_err, state_text))
            .when_some(msg, |el, (is_err, text)| el.child(line(is_err, text)))
            .child(widgets::caption(
                "Unless you set your own config file, the app writes the proxy config itself: 127.0.0.1, the port from the endpoint URL, and the API key above. Sign in once; the login is kept in %USERPROFILE%\\.cli-proxy-api.",
                theme,
            ))
            .into_any_element()
    }
}
