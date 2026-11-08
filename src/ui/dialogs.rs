//! The Settings dialog (6 tabs): Keybindings (with live capture), Class
//! Management (add/remove/reorder + remap), Batch Operations, Game Presets,
//! Box Presets and Themes. Mirrors the Python settings dialog flow by flow.

use gpui::{div, prelude::*, px, App, AppContext, Context, Entity, Focusable, InteractiveElement,
    IntoElement, SharedString, Styled, Window};

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
}

impl SettingsTab {
    fn title(self) -> &'static str {
        match self {
            SettingsTab::Keybindings => "Keybindings",
            SettingsTab::ClassManagement => "Class Management",
            SettingsTab::BatchOperations => "Batch Operations",
            SettingsTab::GamePresets => "Game Presets",
            SettingsTab::BoxPresets => "Box Presets",
            SettingsTab::Themes => "Themes",
        }
    }
}

const TABS: [SettingsTab; 6] = [
    SettingsTab::Keybindings,
    SettingsTab::ClassManagement,
    SettingsTab::BatchOperations,
    SettingsTab::GamePresets,
    SettingsTab::BoxPresets,
    SettingsTab::Themes,
];

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
    focus: gpui::FocusHandle,
}

impl SettingsView {
    pub fn new(workspace: Entity<Workspace>, cx: &mut Context<Self>) -> Self {
        let temp_classes: Vec<String> =
            app_state(cx).classes.iter().map(|c| c.name.clone()).collect();
        let mut preset_files: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(paths::data_dir()) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.to_lowercase().ends_with(".txt") && name != "predefined_classes.txt" {
                    preset_files.push(name);
                }
            }
        }
        preset_files.sort();
        let new_class = cx.new(|cx| Entry::new("New class name…", cx));
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
            focus: cx.focus_handle(),
        }
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
        let config = app_state(cx).config.clone();
        config.save();
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
        let _ = window;

        let tab_bar = widgets::segmented_control(theme)
            .mx(px(20.))
            .mt(px(20.))
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
        };

        widgets::panel(theme)
            .id("settings")
            .w(px(680.))
            .h(px(580.))
            .flex_col()
            .on_key_down(cx.listener(Self::on_capture_key))
            .child(tab_bar)
            .child(
                div()
                    .id("settings-content")
                    .px(px(20.))
                    .py(px(16.))
                    .mt(px(14.))
                    .mx(px(20.))
                    .mb(px(16.))
                    .flex_1()
                    .min_h_0()
                    .rounded(px(10.))
                    .bg(theme.rgba(theme.fg_muted, 0.05))
                    .overflow_y_scroll()
                    .child(content),
            )
            .child(
                div()
                    .px(px(20.))
                    .pb(px(18.))
                    .flex()
                    .justify_end()
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
            let binding = app_state(cx)
                .config
                .bindings
                .get(*action_id)
                .cloned()
                .unwrap_or_else(|| "—".to_string());
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
            .flex_col()
            .gap_1()
            .child(widgets::caption("Click a key to rebind it. Press Escape to cancel capture.", theme))
            .children(rows)
            .into_any_element()
    }

    fn render_class_management(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let mut list = div().flex_col().gap_0p5().rounded(px(8.)).p(px(4.)).bg(theme.rgba(theme.fg_muted, 0.06));
        for (i, name) in self.temp_classes.iter().enumerate() {
            let selected = self.temp_selected == Some(i);
            list = list.child(
                div()
                    .id(("cm-row", i))
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

        let make_list = || {
            div().flex_col().rounded(px(8.)).p(px(4.)).bg(theme.rgba(theme.fg_muted, 0.06))
        };
        let mut old_list = make_list();
        let mut new_list = make_list();
        for (id, name) in &classes {
            let id = *id;
            old_list = old_list.child(
                div()
                    .id(("batch-old", id as usize))
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
        let mut list = div().flex_col().rounded(px(8.)).p(px(4.)).bg(theme.rgba(theme.fg_muted, 0.06));
        for (i, name) in self.preset_files.iter().enumerate() {
            let selected = self.preset_selected == Some(i);
            list = list.child(
                div()
                    .id(("gp-row", i))
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
                                    ws.load_game_preset(&name, cx);
                                });
                                let _ = window;
                            }),
                        ),
                    )
                    .child(
                        widgets::button("gp-refresh", "Refresh", BtnVariant::Default, theme).on_click(
                            cx.listener(|view: &mut SettingsView, _, _, cx| {
                                view.preset_files.clear();
                                if let Ok(entries) = std::fs::read_dir(paths::data_dir()) {
                                    for entry in entries.flatten() {
                                        let name = entry.file_name().to_string_lossy().to_string();
                                        if name.to_lowercase().ends_with(".txt")
                                            && name != "predefined_classes.txt"
                                        {
                                            view.preset_files.push(name);
                                        }
                                    }
                                }
                                view.preset_files.sort();
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
}
