//! The workspace root view: toolbar, sidebars, canvas area, filmstrip,
//! status bar, modal dialogs, toasts and the help overlay. Action handlers
//! and background flows live in `workspace_actions.rs`.

use std::time::Instant;

use gpui::{
    div, prelude::*, px, uniform_list, App, AppContext, Context, Entity,
    FocusHandle, Focusable, Hsla, IntoElement, MouseButton, MouseMoveEvent, SharedString,
    Styled, UniformListScrollHandle, Window,
};

use crate::state::model::AppState;
use crate::validation::ValidationIssue;

use super::canvas::{app_state, app_state_mut, install_app_state, Canvas, CanvasChanged};
use super::dialogs::SettingsView;
use super::entry::Entry;
use super::filmstrip::{Filmstrip, FilmstripClick, STRIP_H};
use super::theme::ThemeState;
use super::widgets::{self, BtnVariant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeftTab {
    Project,
    Classes,
    AutoLabel,
}

pub struct Toast {
    pub id: usize,
    pub text: SharedString,
    #[allow(dead_code)]
    pub born: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetPending {
    pub slot: u8,
    pub key: String,
    pub shown: bool,
}

pub enum Modal {
    Settings(Entity<SettingsView>),
    Confirm {
        title: SharedString,
        message: String,
        on_yes: Box<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>,
        on_no: Option<Box<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>>,
        /// Custom (no, yes) button labels; also adds a separate Cancel
        /// button that dismisses without running either action.
        labels: Option<(SharedString, SharedString)>,
    },
    Alert {
        title: SharedString,
        message: String,
    },
    Progress {
        title: SharedString,
        label: SharedString,
        current: usize,
        total: usize,
        /// When set, the modal shows a Cancel button that raises this flag.
        cancel: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    },
    EditClass,
}

pub struct Workspace {
    pub canvas: Entity<Canvas>,
    pub filmstrip: Entity<Filmstrip>,
    pub focus: FocusHandle,
    pub class_search: Entity<Entry>,
    pub class_list_scroll: UniformListScrollHandle,
    pub file_list_scroll: UniformListScrollHandle,
    pub box_list_scroll: UniformListScrollHandle,

    pub left_tab: LeftTab,
    pub collapse_dirs: bool,
    pub collapse_counts: bool,
    pub collapse_files: bool,
    pub collapse_classes: bool,
    pub collapse_tools: bool,
    pub status: SharedString,
    pub toasts: Vec<Toast>,
    pub next_toast_id: usize,
    pub help_visible: bool,
    pub validation_visible: bool,
    pub validation_issues: Vec<ValidationIssue>,

    pub modal: Option<Modal>,
    pub task_running: bool,
    pub startup_alerts: Vec<(String, String)>,

    // Popover/dropdown state
    pub recents_open: bool,
    /// When a popover was last closed by an outside press; the press that
    /// closed it must not immediately re-open it via its toggle button.
    pub popover_closed_at: Option<Instant>,
    pub selected_recent: Option<usize>,
    pub filter_open: bool,
    pub filter_class_id: Option<i64>,

    // Preset hold gesture
    pub preset_pending: Option<PresetPending>,
    pub preset_lock: bool,

    // Background scan guard (mirrors `_filter_scan_running`)
    pub filter_scan_running: bool,

    // Box list drag selection
    pub box_list_anchor: Option<usize>,
    pub box_list_dragging: bool,

    pub did_focus: bool,
    pub last_title: String,

    /// Auto Label tab state.
    pub autolabel: super::autolabel_ui::AutoLabelPanel,
}

impl Workspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let state = AppState::new();
        let canvas = cx.new(|cx| Canvas::new(cx));
        let filmstrip = cx.new(|cx| Filmstrip::new(cx));

        let class_search = cx.new(|cx| {
            let mut entry = Entry::new("Search classes…", cx);
            entry.set_on_change(Box::new(|_text, _window, _cx| {
                // Live filtering is applied by a workspace subscription below.
            }));
            entry
        });

        install_app_state(cx, state);

        // Refresh class filtering whenever the search text changes.
        cx.subscribe(&class_search, |ws: &mut Workspace, _entry, _event: &super::entry::EntryChanged, cx| {
            ws.apply_class_search(cx);
        })
        .detach();

        // Canvas interactions (draw/move/resize/zoom) can change app state.
        cx.subscribe(&canvas, |ws: &mut Workspace, _canvas, _event: &CanvasChanged, cx| {
            ws.refresh_after_canvas_change(cx);
        })
        .detach();

        cx.subscribe(&filmstrip, |ws: &mut Workspace, _strip, click: &FilmstripClick, cx| {
            if ws.modal.is_none() {
                let ix = click.0;
                ws.guard_unsaved(move |ws, cx| ws.load_image(ix, cx), cx);
            }
        })
        .detach();

        Self {
            canvas,
            filmstrip,
            focus: cx.focus_handle(),
            class_search,
            class_list_scroll: UniformListScrollHandle::new(),
            file_list_scroll: UniformListScrollHandle::new(),
            box_list_scroll: UniformListScrollHandle::new(),
            left_tab: LeftTab::Project,
            collapse_dirs: true,
            collapse_counts: true,
            collapse_files: true,
            collapse_classes: true,
            collapse_tools: true,
            status: "Ready".into(),
            toasts: Vec::new(),
            next_toast_id: 0,
            help_visible: false,
            validation_visible: false,
            validation_issues: Vec::new(),
            modal: None,
            task_running: false,
            startup_alerts: Vec::new(),
            recents_open: false,
            popover_closed_at: None,
            selected_recent: None,
            filter_open: false,
            filter_class_id: None,
            preset_pending: None,
            preset_lock: false,
            filter_scan_running: false,
            box_list_anchor: None,
            box_list_dragging: false,
            did_focus: false,
            last_title: String::new(),
            autolabel: super::autolabel_ui::AutoLabelPanel::new(),
        }
    }

    // ------------------------------------------------------------------
    // Small helpers used across render + actions
    // ------------------------------------------------------------------

    pub fn toast(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let toast = Toast {
            id: self.next_toast_id,
            text: text.into(),
            born: Instant::now(),
        };
        self.next_toast_id += 1;
        self.toasts.push(toast);
        let id = self.next_toast_id - 1;
        cx.notify();
        let ws = cx.entity();
        cx.spawn(async move |_this: gpui::WeakEntity<Workspace>, cx: &mut gpui::AsyncApp| {
            smol::Timer::after(std::time::Duration::from_secs(4)).await;
            let _ = cx.update(|cx| {
                ws.update(cx, |ws, cx| {
                    ws.toasts.retain(|t| t.id != id);
                    cx.notify();
                });
            });
        })
        .detach();
    }

    pub(crate) fn popover_just_closed(&self) -> bool {
        self.popover_closed_at
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(250))
    }

    pub fn set_status(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.status = text.into();
        cx.notify();
    }

    pub fn alert(&mut self, title: &str, message: impl Into<String>) {
        self.modal = Some(Modal::Alert {
            title: title.to_string().into(),
            message: message.into(),
        });
    }

    pub fn confirm(
        &mut self,
        title: &str,
        message: impl Into<String>,
        on_yes: Box<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>,
    ) {
        self.modal = Some(Modal::Confirm {
            title: title.to_string().into(),
            message: message.into(),
            on_yes,
            on_no: None,
            labels: None,
        });
    }

    /// True when a text entry has focus — configurable shortcuts must not
    /// fire (Python parity). Escape is exempt (handled by caller).
    pub fn entry_focused(&self, window: &Window, cx: &App) -> bool {
        let entry_focus = self.class_search.read(cx).focus_handle(cx);
        entry_focus.is_focused(window) || self.autolabel.any_entry_focused(window, cx)
    }

    pub fn blocked(&self, window: &Window, cx: &App) -> bool {
        self.modal.is_some() || self.entry_focused(window, cx) || self.task_running
    }

    pub fn refresh_after_canvas_change(&mut self, cx: &mut Context<Self>) {
        self.refresh_validation(cx);
        cx.notify();
    }

    pub fn refresh_validation(&mut self, cx: &mut Context<Self>) {
        let issues = crate::validation::validate_annotations(
            &app_state(cx).boxes,
            &app_state(cx).classes,
        );
        self.validation_issues = issues;
    }

    pub fn toolbar_indicator(&self) -> SharedString {
        let (errors, warnings, infos) = crate::validation::summarize(&self.validation_issues);
        if self.validation_issues.is_empty() {
            "✓".into()
        } else if errors > 0 {
            format!("⚠ {errors} errors").into()
        } else if warnings > 0 {
            format!("⚠ {warnings} warnings").into()
        } else {
            format!("ℹ {infos} info").into()
        }
    }

    pub fn indicator_color(&self, theme: &crate::ui::theme::Theme) -> Hsla {
        let (errors, warnings, _) = crate::validation::summarize(&self.validation_issues);
        if self.validation_issues.is_empty() {
            theme.rgb(theme.success)
        } else if errors > 0 {
            theme.rgb(theme.error)
        } else if warnings > 0 {
            theme.rgb(theme.warning)
        } else {
            theme.rgb(theme.info)
        }
    }

    pub fn update_title(&mut self, window: &mut Window, cx: &Context<Self>) {
        let state = app_state(cx);
        let filename = state
            .image_list
            .get(state.current_image_index.max(0) as usize)
            .filter(|_| state.current_image_index != -1)
            .cloned();
        let dirty = if state.is_dirty() { "• " } else { "" };
        let title = match filename {
            Some(name) => format!("{dirty}Annotation Tool - {name}"),
            None => "Annotation Tool".to_string(),
        };
        if title != self.last_title {
            self.last_title = title.clone();
            window.set_window_title(&title);
        }
    }

    // ------------------------------------------------------------------
    // Render
    // ------------------------------------------------------------------

    fn render_toolbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let state = app_state(cx);
        let show_left = state.show_left_sidebar;
        let show_filmstrip = state.show_filmstrip;
        let show_right = state.show_right_sidebar;
        let dirty = state.is_dirty();
        let index = state.current_image_index;
        let total = state.image_list.len();
        let filename: SharedString = state
            .image_list
            .get(index.max(0) as usize)
            .filter(|_| index != -1)
            .cloned()
            .unwrap_or_default()
            .into();

        let save_btn = div()
            .relative()
            .child(widgets::icon_button("tb-save", "save", false, theme).on_click(cx.listener(
                |ws: &mut Workspace, _, window, cx| {
                    ws.save_annotations_action(&crate::actions::SaveAnnotations, window, cx)
                },
            )))
            .when(dirty, |el| {
                el.child(
                    div()
                        .absolute()
                        .top(px(4.))
                        .right(px(4.))
                        .size(px(8.))
                        .rounded(px(4.))
                        .bg(theme.rgb(theme.accent))
                        .border_1()
                        .border_color(theme.rgb(theme.bg_chrome)),
                )
            });

        let left = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(widgets::icon_button("tb-project", "panel-left", show_left, theme).on_click(
                cx.listener(move |_ws: &mut Workspace, _, _, cx| {
                    let show = app_state(cx).show_left_sidebar;
                    app_state_mut(cx).show_left_sidebar = !show;
                    cx.notify();
                }),
            ))
            .child(widgets::toolbar_divider(theme))
            .child(save_btn)
            .child(widgets::icon_button("tb-undo", "undo", false, theme).on_click(cx.listener(
                |ws: &mut Workspace, _, window, cx| ws.undo_impl(window, cx),
            )))
            .child(widgets::icon_button("tb-redo", "redo", false, theme).on_click(cx.listener(
                |ws: &mut Workspace, _, window, cx| ws.redo_impl(window, cx),
            )))
            .child(widgets::toolbar_divider(theme))
            .child(
                widgets::icon_button(
                    "tb-autolabel",
                    "sparkles",
                    show_left && self.left_tab == LeftTab::AutoLabel,
                    theme,
                )
                .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| ws.autolabel_open_tab(cx))),
            )
            .child(widgets::icon_button("tb-validate", "check-circle", self.validation_visible, theme).on_click(
                cx.listener(|ws: &mut Workspace, _, window, cx| ws.show_validation(window, cx)),
            ))
            .child(
                div()
                    .id("tb-indicator")
                    .ml(px(2.))
                    .px(px(8.))
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .rounded(px(6.))
                    .bg(theme.rgba(theme.fg_muted, 0.1))
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.rgb(theme.fg_muted))
                    .child(div().size(px(6.)).rounded(px(3.)).bg(self.indicator_color(theme)))
                    .child(self.toolbar_indicator()),
            );

        let center = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(2.))
            .child(widgets::icon_button("tb-previous", "chevron-left", false, theme).on_click(
                cx.listener(|ws: &mut Workspace, _, window, cx| {
                    ws.prev_image_action(&crate::actions::PrevImage, window, cx)
                }),
            ))
            .child(
                div()
                    .min_w(px(72.))
                    .flex()
                    .justify_center()
                    .text_size(px(12.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme.rgb(theme.fg_text))
                    .child(format!("{} / {}", (index + 1).max(0), total)),
            )
            .child(widgets::icon_button("tb-next", "chevron-right", false, theme).on_click(
                cx.listener(|ws: &mut Workspace, _, window, cx| {
                    ws.next_image_action(&crate::actions::NextImage, window, cx)
                }),
            ))
            .child(
                div()
                    .ml(px(8.))
                    .max_w(px(320.))
                    .truncate()
                    .text_size(px(11.5))
                    .text_color(theme.rgb(theme.fg_muted))
                    .child(filename),
            );

        let right = div()
            .flex_1()
            .min_w_0()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(2.))
            .child(
                widgets::button("tb-fit", "Fit image", BtnVariant::Default, theme).on_click(
                    cx.listener(|ws: &mut Workspace, _, window, cx| {
                        ws.zoom_reset_action(&crate::actions::ZoomReset, window, cx)
                    }),
                ),
            )
            .child(widgets::icon_button("tb-keys", "keyboard", self.help_visible, theme).on_click(
                cx.listener(|ws: &mut Workspace, _, _, cx| {
                    ws.help_visible = !ws.help_visible;
                    cx.notify();
                }),
            ))
            .child(widgets::toolbar_divider(theme))
            .child(widgets::icon_button("tb-filmstrip", "film", show_filmstrip, theme).on_click(
                cx.listener(|_ws: &mut Workspace, _, _, cx| {
                    let show = app_state(cx).show_filmstrip;
                    app_state_mut(cx).show_filmstrip = !show;
                    cx.notify();
                }),
            ))
            .child(widgets::icon_button("tb-boxes", "list", show_right, theme).on_click(
                cx.listener(move |_ws: &mut Workspace, _, _, cx| {
                    let show = app_state(cx).show_right_sidebar;
                    app_state_mut(cx).show_right_sidebar = !show;
                    cx.notify();
                }),
            ));

        div()
            .id("toolbar")
            .h(px(44.))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(8.))
            .bg(theme.rgb(theme.bg_chrome))
            .border_b_1()
            .border_color(theme.rgb(theme.divider))
            .child(left)
            .child(center)
            .child(right)
            .into_any_element()
    }

    fn render_statusbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let coords = self.canvas.read(cx).coords_text(app_state(cx));
        let state = app_state(cx);
        let has_image = state.current_image_index != -1;
        let dirty = state.is_dirty();
        let auto = state.auto_save;
        let ai_pending = self.autolabel_current_pending(cx);

        let save_state = div()
            .flex_none()
            .flex()
            .items_center()
            .gap(px(6.))
            .when(has_image && dirty, |el| {
                el.text_color(theme.rgb(theme.warning))
                    .child(div().size(px(6.)).rounded(px(3.)).bg(theme.rgb(theme.warning)))
                    .child(if auto { "Unsaved changes" } else { "Unsaved changes · Ctrl+S to save" })
            })
            .when(has_image && !dirty, |el| {
                el.child(super::icons::icon("check", 11.0, theme.rgb(theme.success)))
                    .child("Saved")
            })
            .when(!has_image, |el| el.child("No image"));

        div()
            .id("statusbar")
            .h(px(26.))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .bg(theme.rgb(theme.bg_chrome))
            .border_t_1()
            .border_color(theme.rgb(theme.divider))
            .text_size(px(11.))
            .text_color(theme.rgb(theme.fg_muted))
            .child(div().flex_1().min_w_0().truncate().child(self.status.clone()))
            .when(ai_pending, |el| {
                el.child(widgets::v_divider(theme)).child(
                    div()
                        .flex_none()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_color(theme.rgb(theme.info))
                        .child(super::icons::icon("sparkles", 11.0, theme.rgb(theme.info)))
                        .child("AI labels · review, then Ctrl+S to accept"),
                )
            })
            .child(widgets::v_divider(theme))
            .child(save_state)
            .child(widgets::v_divider(theme))
            .child(div().flex_none().child(if auto { "Auto-save on" } else { "Auto-save off" }))
            .when(!coords.is_empty(), |el| {
                el.child(widgets::v_divider(theme)).child(div().flex_none().child(coords))
            })
            .into_any_element()
    }

    fn render_left_sidebar(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);

        let tabs = div()
            .h(px(34.))
            .flex_none()
            .w_full()
            .flex()
            .bg(theme.rgb(theme.bg_chrome))
            .border_b_1()
            .border_color(theme.rgb(theme.divider))
            .child(
                widgets::strip_tab("tab-project", "Project", self.left_tab == LeftTab::Project, theme)
                    .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                        ws.left_tab = LeftTab::Project;
                        cx.notify();
                    })),
            )
            .child(
                widgets::strip_tab("tab-classes", "Classes", self.left_tab == LeftTab::Classes, theme)
                    .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                        ws.left_tab = LeftTab::Classes;
                        cx.notify();
                    })),
            )
            .child(
                widgets::strip_tab("tab-autolabel", "Auto Label", self.left_tab == LeftTab::AutoLabel, theme)
                    .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| ws.autolabel_open_tab(cx))),
            );

        let body: gpui::AnyElement = match self.left_tab {
            LeftTab::Project => self.render_project_tab(cx),
            LeftTab::Classes => self.render_classes_tab(cx),
            LeftTab::AutoLabel => self.render_autolabel_tab(cx),
        };

        div()
            .id("left-sidebar")
            .w(px(292.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.rgb(theme.bg_pane))
            .border_r_1()
            .border_color(theme.rgb(theme.divider))
            .child(tabs)
            .child(body)
            .into_any_element()
    }

    fn render_project_tab(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let state = app_state(cx);
        let has_folder = !state.image_dir.is_empty();
        let has_output = !state.output_dir.is_empty();
        let dir_path = state.image_dir.clone();
        let dir_name = std::path::Path::new(&dir_path)
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir_path.clone());
        let visible_files = state.image_list.len();
        let total_files = state.full_image_list.len();
        let filter_active = self.filter_class_id.is_some() || state.unannotated_filter_active
            || visible_files != total_files;
        let loading = state.class_counts_loading;
        let count_error = state.class_counts_error.clone();
        let counts_ready = has_folder && has_output && !loading && count_error.is_none();
        let total_boxes: usize = state.class_counts.values().sum();
        let used_classes = state.classes.iter()
            .filter(|class| state.class_counts.get(&class.id).copied().unwrap_or(0) > 0)
            .count();
        let available_classes = state.classes.len();
        let class_rows: Vec<_> = state.classes.iter().map(|class| {
            let id = class.id;
            let count = state.class_counts.get(&id).copied().unwrap_or(0);
            div()
                .id(("folder-class", id as usize))
                .h(px(26.))
                .w_full()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(6.))
                .rounded(px(5.))
                .text_size(px(12.))
                .when(self.filter_class_id == Some(id), |el| el.bg(theme.selection_bg()))
                .when(counts_ready, |el| {
                    el.cursor_pointer()
                        .hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.1)))
                })
                .when(counts_ready && count == 0, |el| el.text_color(theme.rgb(theme.fg_muted)))
                .child(widgets::color_swatch(class.color, px(10.), theme))
                .child(div().flex_1().min_w_0().truncate()
                    .child(format!("{}: {}", id, class.name)))
                .child(div().w(px(48.)).flex_none().text_right()
                    .text_color(theme.rgb(theme.fg_muted))
                    .child(if counts_ready { count.to_string() } else { "...".to_string() }))
                .on_click(cx.listener(move |ws: &mut Workspace, _, _window, cx| {
                    let state = app_state(cx);
                    if !state.class_counts_loading && state.class_counts_error.is_none()
                        && !state.output_dir.is_empty() && !ws.filter_scan_running && !ws.task_running {
                        ws.filter_class_id = Some(id);
                        ws.filter_open = false;
                        ws.apply_image_filter(cx);
                    }
                }))
                .into_any_element()
        }).collect();

        // --- Folder -------------------------------------------------------
        let folder_header = widgets::pane_header_collapsible(
            "col-dirs", "Folder", self.collapse_dirs, None, theme,
        ).on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
            ws.collapse_dirs = !ws.collapse_dirs;
            cx.notify();
        }));
        let folder_content = div()
            .px(px(12.)).py(px(10.)).flex().flex_col().gap(px(6.))
            .flex_none()
            .when(has_folder, |el| el.child(
                div().flex().flex_col().gap(px(2.)).pb(px(4.))
                    .child(div().text_size(px(13.)).font_weight(gpui::FontWeight::SEMIBOLD)
                        .truncate().child(dir_name))
                    .child(div().text_size(px(11.)).line_height(px(16.))
                        .text_color(theme.rgb(theme.fg_muted)).child(dir_path.clone())),
            ))
            .when(!has_folder, |el| el.child(widgets::caption(
                "Open a folder to browse images and review saved boxes.", theme,
            )))
            .child(widgets::button_with_icon(
                "btn-open-images", "Open image folder", Some("folder-open"), BtnVariant::Accent, theme,
            ).w_full().on_click(cx.listener(|ws: &mut Workspace, _, _window, cx| {
                ws.open_images_dialog(cx)
            })))
            .child(self.render_recents_dropdown(cx));

        // --- Saved boxes --------------------------------------------------
        let summary_status = if loading {
            "Counting saved boxes...".to_string()
        } else if !has_folder {
            "Choose an image folder to see class counts.".to_string()
        } else if let Some(error) = count_error.as_ref() {
            format!("Could not count saved boxes: {error}")
        } else {
            format!("{total_boxes} boxes · {used_classes}/{available_classes} classes used")
        };
        let counts_header = widgets::pane_header_collapsible(
            "col-counts",
            "Saved boxes",
            self.collapse_counts,
            Some(
                widgets::header_link("btn-refresh-counts", "Refresh", theme)
                    .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                        cx.stop_propagation();
                        if !app_state(cx).class_counts_loading && !ws.task_running {
                            ws.refresh_class_counts(cx);
                        }
                    }))
                    .into_any_element(),
            ),
            theme,
        ).on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
            ws.collapse_counts = !ws.collapse_counts;
            cx.notify();
        }));
        let counts_content = div()
            .px(px(12.)).py(px(10.)).flex().flex_col().gap(px(6.))
            .flex_none()
            .child(div().text_size(px(11.)).line_height(px(16.))
                .text_color(theme.rgb(if count_error.is_some() { theme.error } else { theme.fg_muted }))
                .child(summary_status))
            .when(available_classes > 0, |el| el
                .child(div().flex().justify_between().px(px(6.)).text_size(px(10.5))
                    .text_color(theme.rgb(theme.fg_muted))
                    .child("Class · click to filter").child("Boxes"))
                .child(div().id("folder-class-counts").w_full()
                    .h(px((available_classes as f32 * 26.).min(182.))).flex_none()
                    .overflow_y_scroll().flex().flex_col().children(class_rows)))
            .when(available_classes == 0, |el| el.child(widgets::caption(
                "Add classes in Settings to see the class list.", theme,
            )))
            .when(counts_ready && total_boxes == 0, |el| el.child(widgets::caption(
                "No saved boxes in this folder. Unsaved edits are not counted.", theme,
            )));

        // --- Files --------------------------------------------------------
        let files_header = widgets::pane_header_collapsible(
            "col-files",
            "Files",
            self.collapse_files,
            Some(div().child(format!("{visible_files} / {total_files}")).into_any_element()),
            theme,
        ).on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
            ws.collapse_files = !ws.collapse_files;
            cx.notify();
        }));
        let files_content = div()
            .flex_1().min_h_0().flex().flex_col()
            .child(div()
                .px(px(12.)).pt(px(10.)).pb(px(6.)).flex().flex_col().gap(px(6.)).flex_none()
                .child(div().flex().gap(px(6.)).items_center()
                    .child(div().flex_1().min_w_0().child(self.render_filter_dropdown(cx)))
                    .child(widgets::icon_button("btn-filter", "filter", self.filter_class_id.is_some(), theme)
                        .on_click(cx.listener(|ws: &mut Workspace, _, _window, cx| {
                            ws.apply_image_filter(cx)
                        }))))
                .child(div().flex().items_center().justify_between()
                    .child(widgets::checkbox(
                        "chk-unannotated", "Unannotated only", app_state(cx).unannotated_filter_active, theme,
                    ).on_click(cx.listener(|ws: &mut Workspace, _, _window, cx| {
                        ws.toggle_unannotated_filter(cx)
                    })))
                    .when(filter_active, |el| el.child(
                        widgets::header_link("btn-clear-filter", "Clear filters", theme)
                            .on_click(cx.listener(|ws: &mut Workspace, _, _window, cx| {
                                if !ws.filter_scan_running && !ws.task_running {
                                    ws.clear_image_filter(cx);
                                }
                            })),
                    )))
                .when(self.filter_scan_running, |el| el.child(widgets::caption("Filtering files...", theme)))
                .when(visible_files == 0, |el| el.child(widgets::caption(
                    if total_files > 0 { "No files match. Clear filters to show all images." }
                    else if has_folder { "No supported images in this folder. Open another image folder." }
                    else { "Your images will appear here after you open a folder." }, theme,
                ))))
            .when(visible_files > 0, |el| el.child(
                div().flex_1().min_h_0().w_full().px(px(6.)).pt(px(4.)).pb(px(6.)).flex().flex_col()
                    .border_t_1().border_color(theme.rgb(theme.divider))
                    .overflow_hidden()
                    .child(self.render_file_list(cx)),
            ));

        div().id("project-tab").flex_1().min_h_0().flex().flex_col()
            .child(folder_header)
            .when(self.collapse_dirs, |el| el.child(folder_content))
            .child(widgets::h_divider(theme))
            .child(counts_header)
            .when(self.collapse_counts, |el| el.child(counts_content))
            .child(widgets::h_divider(theme))
            .child(files_header)
            .when(self.collapse_files, |el| el.child(files_content))
            .into_any_element()
    }

    fn render_recents_dropdown(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let recents = app_state(cx).recent_projects.clone();
        let current_dir = crate::state::session::normalize_dir(&app_state(cx).image_dir);

        if recents.is_empty() {
            return div()
                .id("recents-dd")
                .child(widgets::caption("No recent projects yet", theme))
                .into_any_element();
        }

        let chevron = if self.recents_open { "chevron-up" } else { "chevron-down" };
        let dd_arrow = super::icons::icon(chevron, 13.0, theme.rgb(theme.fg_muted));

        let mut dd = div()
            .id("recents-dd")
            .w_full()
            .child(
                div()
                    .id("recents-btn")
                    .w_full()
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(8.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(theme.rgb(theme.divider))
                    .bg(theme.rgb(theme.entry_bg))
                    .text_size(px(12.))
                    .text_color(theme.rgb(theme.fg_text))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.12)))
                    .child(div().flex().items_center().gap(px(6.))
                        .child(super::icons::icon("history", 13.0, theme.rgb(theme.fg_muted)))
                        .child("Recent projects"))
                    .child(dd_arrow)
                    .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                        if !ws.popover_just_closed() {
                            ws.recents_open = !ws.recents_open;
                            ws.filter_open = false;
                        }
                        cx.notify();
                    })),
            );

        if self.recents_open {
            let mut rows = Vec::new();
            for (i, p) in recents.iter().enumerate() {
                let is_current = crate::state::session::normalize_dir(&p.image_dir) == current_dir;
                rows.push(
                    div()
                        .id(("recent", i))
                        .flex_none()
                        .px(px(8.))
                        .py(px(5.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.12)))
                        .child(div().w(px(14.)).flex_none().when(is_current, |el| {
                            el.child(super::icons::icon("check", 13.0, theme.rgb(theme.accent)))
                        }))
                        .child(
                            div().flex_1().min_w_0().flex().flex_col()
                                .child(div().text_size(px(12.)).truncate()
                                    .font_weight(if is_current { gpui::FontWeight::SEMIBOLD } else { gpui::FontWeight::NORMAL })
                                    .child(p.name.clone()))
                                .child(div().text_size(px(10.5)).truncate()
                                    .text_color(theme.rgb(theme.fg_muted))
                                    .child(p.image_dir.clone())),
                        )
                        .on_click(cx.listener(move |ws: &mut Workspace, _, _window, cx| {
                            ws.selected_recent = Some(i);
                            ws.recents_open = false;
                            ws.load_recent_project(cx);
                        }))
                        .into_any_element(),
                );
            }
            dd = dd.child(popover("recents-popover", px(268.), px(280.), theme, rows,
                cx.listener(|ws: &mut Workspace, _, _, cx| {
                    ws.recents_open = false;
                    ws.popover_closed_at = Some(Instant::now());
                    cx.notify();
                })));
        }

        dd.into_any_element()
    }

    fn render_filter_dropdown(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let classes = app_state(cx).classes.clone();
        let selected = self.filter_class_id.and_then(|id| classes.iter().find(|c| c.id == id));
        let label: SharedString = match selected {
            Some(c) => format!("{}: {}", c.id, c.name).into(),
            None => "Filter by class…".into(),
        };
        let swatch = selected.map(|c| c.color);

        let chevron = if self.filter_open { "chevron-up" } else { "chevron-down" };
        let dd_arrow = super::icons::icon(chevron, 13.0, theme.rgb(theme.fg_muted));

        let mut dd = div().id("filter-dd").w_full().child(
            div()
                .id("filter-btn")
                .w_full()
                .h(px(30.))
                .flex()
                .items_center()
                .justify_between()
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(theme.rgb(theme.divider))
                .bg(theme.rgb(theme.entry_bg))
                .text_size(px(12.))
                .text_color(if selected.is_some() { theme.rgb(theme.fg_text) } else { theme.rgb(theme.fg_muted) })
                .cursor_pointer()
                .hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.12)))
                .child(div().flex_1().min_w_0().flex().items_center().gap(px(8.))
                    .when_some(swatch, |el, color| el.child(widgets::color_swatch(color, px(10.), theme)))
                    .child(div().min_w_0().truncate().child(label)))
                .child(dd_arrow)
                .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                    if !ws.popover_just_closed() {
                        ws.filter_open = !ws.filter_open;
                        ws.recents_open = false;
                    }
                    cx.notify();
                })),
        );

        if self.filter_open {
            let mut rows = Vec::new();
            for c in &classes {
                let id = c.id;
                let is_selected = self.filter_class_id == Some(id);
                rows.push(
                    div()
                        .id(("filter-class", id as usize))
                        .flex_none()
                        .px(px(8.))
                        .h(px(28.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(6.))
                        .text_size(px(12.))
                        .cursor_pointer()
                        .when(is_selected, |el| el.bg(theme.selection_bg()))
                        .hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.12)))
                        .child(widgets::color_swatch(c.color, px(10.0), theme))
                        .child(div().min_w_0().truncate().child(format!("{}: {}", c.id, c.name)))
                        .on_click(cx.listener(move |ws: &mut Workspace, _, _, cx| {
                            ws.filter_class_id = Some(id);
                            ws.filter_open = false;
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
            dd = dd.child(popover("filter-popover", px(232.), px(300.), theme, rows,
                cx.listener(|ws: &mut Workspace, _, _, cx| {
                    ws.filter_open = false;
                    ws.popover_closed_at = Some(Instant::now());
                    cx.notify();
                })));
        }

        dd.into_any_element()
    }

    fn render_file_list(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let count = app_state(cx).image_list.len();

        let on_click = std::rc::Rc::new(cx.listener({
            move |ws: &mut Workspace, ix: &usize, _window, cx| {
                if ws.modal.is_none() {
                    let ix = *ix;
                    ws.guard_unsaved(move |ws, cx| ws.load_image(ix, cx), cx);
                }
            }
        }));

        uniform_list("file-list", count, move |range, _window, cx| {
            let state = app_state(cx);
            let mut rows = Vec::with_capacity(range.len());
            for ix in range {
                let Some(filename) = state.image_list.get(ix) else { continue };
                let is_current = state.current_image_index == ix as i64;
                let ai_status = super::autolabel_ui::review_status(cx, filename);
                rows.push(
                    div()
                        .id(("file-row", ix))
                        .px(px(8.))
                        .h(px(26.))
                        .w_full()
                        .flex()
                        .items_center()
                        .rounded(px(5.))
                        .text_size(px(12.))
                        .cursor_pointer()
                        .when(is_current, |el| {
                            el.bg(theme.selection_bg())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme.rgb(theme.fg_highlight))
                        })
                        .when(!is_current, |el| el.hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.1))))
                        .child(div().flex_1().min_w_0().truncate().child(filename.clone()))
                        .when_some(ai_status, |el, status| {
                            use crate::autolabel::review::Status;
                            let (text, color) = match status {
                                Status::Pending => ("AI", theme.rgb(theme.info)),
                                Status::Empty => ("AI ∅", theme.rgb(theme.fg_muted)),
                                Status::Error => ("AI !", theme.rgb(theme.error)),
                            };
                            el.child(
                                div()
                                    .flex_none()
                                    .ml(px(6.))
                                    .text_size(px(10.))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(color)
                                    .child(text),
                            )
                        })
                        .on_click({
                            let on_click = std::rc::Rc::clone(&on_click);
                            move |_, window, cx| on_click(&ix, window, cx)
                        })
                        .into_any_element(),
                );
            }
            rows
        })
        .track_scroll(self.file_list_scroll.clone())
        .w_full()
        .h_full()
        .flex_1()
        .min_h_0()
        .into_any_element()
    }

    fn render_classes_tab(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let show_labels = app_state(cx).show_labels;
        let auto_save = app_state(cx).auto_save;
        let class_count = app_state(cx).classes.len();

        let classes_header = widgets::pane_header_collapsible(
            "col-classes",
            "Classes",
            self.collapse_classes,
            Some(div().child(class_count.to_string()).into_any_element()),
            theme,
        ).on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
            ws.collapse_classes = !ws.collapse_classes;
            cx.notify();
        }));

        let classes_content = div()
            .flex_1()
            .min_h_0()
            .px(px(10.))
            .py(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().flex_none().child(self.class_search.clone()))
            .child(div().flex_1().min_h_0().flex().flex_col().overflow_hidden().child(self.render_class_list(cx)));

        let tools_header = widgets::pane_header_collapsible(
            "col-tools",
            "Tools",
            self.collapse_tools,
            None,
            theme,
        ).on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
            ws.collapse_tools = !ws.collapse_tools;
            cx.notify();
        }));

        let quiet_button = |id: &'static str,
                            label: &'static str,
                            icon_name: &'static str,
                            theme: &crate::ui::theme::Theme|
         -> gpui::Stateful<gpui::Div> {
            let fg = theme.rgb(theme.fg_text);
            let icon_fg = theme.rgb(theme.fg_muted);
            let hover = theme.rgba(theme.fg_muted, 0.1);
            div()
                .id(id)
                .w_full()
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(8.))
                .rounded(px(6.))
                .text_size(px(12.))
                .text_color(fg)
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .child(super::icons::icon(icon_name, 14.0, icon_fg))
                .child(label)
        };

        let tools_content = div()
            .flex_none()
            .px(px(6.))
            .py(px(8.))
            .flex()
            .flex_col()
            .gap(px(1.))
            .child(
                quiet_button("btn-copy", "Copy Boxes", "copy", theme).on_click(cx.listener(
                    |ws: &mut Workspace, _, window, cx| ws.copy_boxes_impl(window, cx),
                )),
            )
            .child(
                quiet_button("btn-paste", "Paste Boxes", "clipboard", theme).on_click(cx.listener(
                    |ws: &mut Workspace, _, window, cx| ws.paste_boxes_impl(window, cx),
                )),
            )
            .child(
                quiet_button("btn-preview-clip", "Preview Clipboard", "eye", theme).on_click(
                    cx.listener(|ws: &mut Workspace, _, window, cx| {
                        ws.toggle_clipboard_preview(window, cx)
                    }),
                ),
            )
            .child(
                quiet_button("btn-duplicate", "Duplicate Boxes", "layers", theme).on_click(
                    cx.listener(|ws: &mut Workspace, _, window, cx| {
                        ws.duplicate_boxes_impl(window, cx)
                    }),
                ),
            )
            .child(div().mx(px(8.)).my(px(6.)).child(widgets::h_divider(theme)))
            .child(
                div().px(px(8.)).flex().flex_col()
                    .child(
                        widgets::checkbox("chk-autosave", "Auto Save", auto_save, theme).on_click(
                            cx.listener(|_ws: &mut Workspace, _, _, cx| {
                                let v = app_state(cx).auto_save;
                                app_state_mut(cx).auto_save = !v;
                                cx.notify();
                            }),
                        ),
                    )
                    .child(
                        widgets::checkbox("chk-labels", "Show Labels", show_labels, theme).on_click(
                            cx.listener(|_ws: &mut Workspace, _, _, cx| {
                                let v = app_state(cx).show_labels;
                                app_state_mut(cx).show_labels = !v;
                                cx.notify();
                            }),
                        ),
                    ),
            );

        div()
            .id("classes-tab")
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(classes_header)
            .when(self.collapse_classes, |el| el.child(classes_content))
            .child(widgets::h_divider(theme))
            .child(tools_header)
            .when(self.collapse_tools, |el| el.child(tools_content))
            .when(!self.collapse_classes, |el| el.child(div().flex_1()))
            .child(widgets::h_divider(theme))
            .child(
                div()
                    .flex_none()
                    .p(px(10.))
                    .bg(theme.rgb(theme.bg_chrome))
                    .child(
                        widgets::button_with_icon(
                            "btn-settings",
                            "Settings",
                            Some("sliders"),
                            BtnVariant::Default,
                            theme,
                        )
                        .w_full()
                        .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                            ws.open_settings(window, cx)
                        })),
                    ),
            )
            .into_any_element()
    }

    fn render_class_list(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let state = app_state(cx);
        let filtered: Vec<(usize, i64, String, (u8, u8, u8), usize)> = state
            .filtered_classes
            .iter()
            .filter_map(|&ix| {
                state.classes.get(ix).map(|c| {
                    (ix, c.id, c.name.clone(), c.color, state.class_counts.get(&c.id).copied().unwrap_or(0))
                })
            })
            .collect();
        let current_class = state.current_class_index;
        let count = filtered.len();
        let counts_ready = !state.class_counts_loading && state.class_counts_error.is_none()
            && !state.image_dir.is_empty() && !state.output_dir.is_empty();
        if count == 0 {
            return div().p(px(8.)).child(widgets::caption(
                if state.classes.is_empty() { "Add classes in Settings to start labeling." }
                else { "No classes match. Clear the search to show all classes." }, theme,
            )).into_any_element();
        }

        let on_click = std::rc::Rc::new(cx.listener(move |_ws: &mut Workspace, ix: &usize, _window, cx| {
            if let Some(c) = app_state(cx).classes.get(*ix) {
                app_state_mut(cx).current_class_index = c.id;
                cx.notify();
            }
        }));

        uniform_list("class-list", count, move |range, _window, _cx| {
            let mut rows = Vec::new();
            for rix in range {
                let (ix, id, name, color, n) = &filtered[rix];
                let ix = *ix;
                let is_current = current_class == *id;
                let label: SharedString = format!("{}: {}", id, name).into();
                rows.push(
                    div()
                        .id(("class-row", rix))
                        .px(px(8.))
                        .h(px(28.))
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(6.))
                        .text_size(px(12.))
                        .cursor_pointer()
                        .when(is_current, |el| {
                            el.bg(theme.selection_bg())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme.rgb(theme.fg_highlight))
                        })
                        .when(!is_current, |el| el.hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.1))))
                        .child(widgets::color_swatch(*color, px(11.0), theme))
                        .child(div().flex_1().min_w_0().truncate().child(label))
                        .child(div().w(px(48.)).flex_none().text_right()
                            .text_color(theme.rgb(theme.fg_muted))
                            .child(if counts_ready { n.to_string() } else { "...".to_string() }))
                        .on_click({
                            let on_click = std::rc::Rc::clone(&on_click);
                            move |_, window, cx| on_click(&ix, window, cx)
                        })
                        .into_any_element(),
                );
            }
            rows
        })
        .track_scroll(self.class_list_scroll.clone())
        .w_full()
        .h_full()
        .flex_1()
        .min_h_0()
        .into_any_element()
    }

    fn render_right_sidebar(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let state = app_state(cx);
        let boxes: Vec<(usize, String, Option<(u8, u8, u8)>)> = state
            .boxes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let class = state.classes.iter().find(|c| c.id == b.class_id);
                (
                    i,
                    class.map(|c| c.name.clone()).unwrap_or_else(|| "Unknown".to_string()),
                    class.map(|c| c.color),
                )
            })
            .collect();
        let selected = state.selected_indices.clone();
        let count = boxes.len();
        let has_image = state.current_image_index != -1;
        let header_trailing = if selected.is_empty() {
            count.to_string()
        } else {
            format!("{} selected · {count}", selected.len())
        };

        let on_row_down = std::rc::Rc::new(cx.listener(
            move |ws: &mut Workspace, (ix, event): &(usize, gpui::MouseDownEvent), _window, cx| {
                let state = app_state_mut(cx);
                let ctrl = event.modifiers.control || event.modifiers.alt;
                if ctrl {
                    if state.selected_indices.contains(ix) {
                        state.selected_indices.remove(ix);
                    } else {
                        state.selected_indices.insert(*ix);
                    }
                    ws.box_list_anchor = Some(*ix);
                } else {
                    state.selected_indices.clear();
                    state.selected_indices.insert(*ix);
                    ws.box_list_anchor = Some(*ix);
                }
                ws.box_list_dragging = true;
                cx.notify();
            },
        ));
        let on_row_move = std::rc::Rc::new(cx.listener(move |ws: &mut Workspace, ix: &usize, _window, cx| {
            if ws.box_list_dragging {
                if let Some(anchor) = ws.box_list_anchor {
                    let state = app_state_mut(cx);
                    let (lo, hi) = if anchor < *ix { (anchor, *ix) } else { (*ix, anchor) };
                    state.selected_indices.clear();
                    for i in lo..=hi {
                        state.selected_indices.insert(i);
                    }
                    cx.notify();
                }
            }
        }));

        let list = uniform_list("box-list", count, move |range, _window, _cx| {
            let mut rows = Vec::new();
            for ix in range {
                let (i, name, color) = &boxes[ix];
                let i = *i;
                let name = name.clone();
                let is_selected = selected.contains(&i);
                rows.push(
                    div()
                        .id(("box-row", ix))
                        .px(px(8.))
                        .h(px(28.))
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .rounded(px(6.))
                        .text_size(px(12.))
                        .cursor_pointer()
                        .when(is_selected, |el| {
                            el.bg(theme.selection_bg())
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(theme.rgb(theme.fg_highlight))
                        })
                        .when(!is_selected, |el| el.hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.1))))
                        .child(match color {
                            Some(c) => widgets::color_swatch(*c, px(11.), theme).into_any_element(),
                            None => div().size(px(11.)).flex_none().rounded(px(3.5)).border_1()
                                .border_color(theme.rgb(theme.fg_muted)).into_any_element(),
                        })
                        .child(div().w(px(22.)).flex_none().text_color(theme.rgb(theme.fg_muted)).child(i.to_string()))
                        .child(div().flex_1().min_w_0().truncate().child(name))
                        .on_mouse_down(
                            MouseButton::Left,
                            {
                                let on_row_down = std::rc::Rc::clone(&on_row_down);
                                move |event: &gpui::MouseDownEvent, window, cx| {
                                    on_row_down(&(i, event.clone()), window, cx)
                                }
                            },
                        )
                        .on_mouse_move({
                            let on_row_move = std::rc::Rc::clone(&on_row_move);
                            move |_, window, cx| on_row_move(&i, window, cx)
                        })
                        .into_any_element(),
                );
            }
            rows
        })
        .track_scroll(self.box_list_scroll.clone())
        .w_full()
        .h_full()
        .flex_1()
        .min_h_0();


        div()
            .id("right-sidebar")
            .w(px(256.))
            .flex_none()
            .h_full()
            .flex()
            .flex_col()
            .bg(theme.rgb(theme.bg_pane))
            .border_l_1()
            .border_color(theme.rgb(theme.divider))
            .child(div().h(px(34.)).flex_none().flex().items_center().px(px(12.))
                .bg(theme.rgb(theme.bg_chrome))
                .border_b_1()
                .border_color(theme.rgb(theme.divider))
                .text_size(px(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.rgb(theme.fg_highlight))
                .child("Annotations"))
            .child(widgets::pane_header(
                "hdr-boxes",
                "Boxes",
                Some(div().child(header_trailing).into_any_element()),
                theme,
            ))
            .when(count == 0, |el| {
                el.child(div().p(px(12.)).child(widgets::caption(
                    if has_image { "No boxes on this image yet. Pick a class, then drag on the canvas." }
                    else { "Open an image to see its boxes." },
                    theme,
                )))
            })
            .when(count > 0, |el| {
                el.child(div().flex_1().min_h_0().p(px(6.)).flex().flex_col().overflow_hidden().child(list))
            })
            .into_any_element()
    }

    fn render_help_overlay(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let cfg = &app_state(cx).config;
        let b = |id: &str| cfg.display_binding(id);
        // Left/Right always navigate in addition to the configured keys.
        let nav = |id: &str, fixed: &str| {
            let k = cfg.display_binding(id);
            if k == fixed { k } else { format!("{k} / {fixed}") }
        };
        let sections: Vec<(&str, Vec<(String, &str)>)> = vec![
            (
                "NAVIGATION",
                vec![
                    (nav("prev_image", "←"), "Previous Image"),
                    (nav("next_image", "→"), "Next Image"),
                    (b("cycle_class"), "Cycle Class"),
                    (b("deselect"), "Deselect Class / Close"),
                ],
            ),
            (
                "EDITING",
                vec![
                    ("Click + Drag".into(), "Draw Box"),
                    ("Click".into(), "Stamp Template Box"),
                    ("Shift + Click".into(), "Add to Selection"),
                    (b("delete_box"), "Delete Selected"),
                    ("Ctrl+A".into(), "Select All Boxes"),
                    (b("copy"), "Copy Boxes"),
                    (b("paste"), "Paste Boxes"),
                    (b("duplicate"), "Duplicate Boxes"),
                    (b("edit_class"), "Edit Box Class"),
                ],
            ),
            (
                "FILE & HISTORY",
                vec![
                    ("Ctrl+S".into(), "Save Annotations"),
                    (b("undo"), "Undo"),
                    (b("redo"), "Redo"),
                    ("Ctrl+Shift+O".into(), "Open Image Folder"),
                ],
            ),
            (
                "VIEW",
                vec![
                    ("Mouse Wheel".into(), "Zoom In/Out"),
                    ("Ctrl + / -".into(), "Zoom In/Out"),
                    ("Ctrl+0".into(), "Fit to Window"),
                    ("Right Click + Drag".into(), "Pan"),
                    ("U".into(), "Toggle Unannotated Filter"),
                    ("?".into(), "Toggle This Overlay"),
                ],
            ),
            (
                "PRESETS",
                vec![
                    ("Tap Ctrl+1-9".into(), "Apply Box Preset"),
                    ("Hold Ctrl+1-9".into(), "Preview Box Preset"),
                    ("Ctrl+Shift+1-9".into(), "Save Box Preset"),
                ],
            ),
        ];

        let mut columns: Vec<gpui::AnyElement> = Vec::new();
        let split = 2.min(sections.len());
        for col_sections in [&sections[..split], &sections[split..]] {
            let mut col = div().flex().flex_col().gap(px(18.));
            for (name, rows) in col_sections.iter() {
                let mut section = div().flex().flex_col().gap(px(6.));
                section = section.child(
                    div()
                        .pb(px(2.))
                        .text_size(px(10.5))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(theme.rgb(theme.fg_muted))
                        .child(*name),
                );
                for (key, desc) in rows.iter() {
                    section = section.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .w(px(128.))
                                    .flex_none()
                                    .flex()
                                    .child(widgets::kbd(key.clone(), theme)),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.rgb(theme.fg_text))
                                    .child(*desc),
                            ),
                    );
                }
                col = col.child(section);
            }
            columns.push(col.flex_1().into_any_element());
        }

        widgets::panel(theme)
            .id("help-overlay")
            .w(px(700.))
            .p(px(28.))
            .flex_col()
            .gap(px(22.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(super::icons::icon(
                        "keyboard",
                        18.0,
                        theme.rgb(theme.fg_muted),
                    ))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.rgb(theme.fg_highlight))
                            .child("Keyboard Shortcuts"),
                    ),
            )
            .child(div().flex().gap(px(32.)).children(columns))
            .child(
                div().flex().items_center().gap(px(8.)).child(widgets::kbd("?", theme)).child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.rgb(theme.fg_muted))
                        .child("or click outside to close · configurable keys follow Settings → Keys"),
                ),
            )
            .into_any_element()
    }

    fn render_modal(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let Some(modal) = &self.modal else {
            return div().into_any_element();
        };

        let backdrop_click = cx.listener(|ws: &mut Workspace, _, _, cx| {
            // Clicking the backdrop dismisses alerts/confirm (not progress).
            if !ws.task_running {
                ws.modal = None;
                cx.notify();
            }
        });

        let panel: gpui::AnyElement = match modal {
            Modal::Settings(view) => view.clone().into_any_element(),
            Modal::Confirm { title, message, on_no, labels, .. } => {
                let title = title.clone();
                let message = message.clone();
                // Yes/No prompts (with an on_no action) vs. plain confirmations.
                let three_way = labels.is_some();
                let (no_label, yes_label): (SharedString, SharedString) = match labels {
                    Some((no, yes)) => (no.clone(), yes.clone()),
                    None if on_no.is_some() => ("No".into(), "Yes".into()),
                    None => ("Cancel".into(), "Confirm".into()),
                };
                type Action = Box<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>;
                let run = cx.listener(move |ws: &mut Workspace, _, window, cx| {
                    let on_yes: Option<Action> = match ws.modal.take() {
                        Some(Modal::Confirm { on_yes, .. }) => Some(on_yes),
                        _ => None,
                    };
                    ws.modal = None;
                    if let Some(action) = on_yes {
                        action(ws, window, cx);
                    }
                    cx.notify();
                });
                let cancel = cx.listener(move |ws: &mut Workspace, _: &gpui::ClickEvent, window, cx| {
                    let on_no: Option<Action> = match ws.modal.take() {
                        Some(Modal::Confirm { on_no, .. }) => on_no,
                        _ => None,
                    };
                    ws.modal = None;
                    if let Some(action) = on_no {
                        action(ws, window, cx);
                    }
                    cx.notify();
                });
                let _ = cancel;
                widgets::panel(theme)
                    .id("modal-confirm")
                    .w(px(440.))
                    .p(px(24.))
                    .flex_col()
                    .gap(px(16.))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.rgb(theme.fg_highlight))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .line_height(px(19.0))
                            .text_color(theme.rgb(theme.fg_text))
                            .child(message),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .mt(px(4.))
                            .when(three_way, |el| {
                                el.child(
                                    widgets::button("confirm-cancel", "Cancel", BtnVariant::Default, theme)
                                        .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                                            ws.modal = None;
                                            cx.notify();
                                        })),
                                )
                                .child(div().flex_1())
                            })
                            .child(
                                widgets::button("confirm-no", no_label, if three_way { BtnVariant::Danger } else { BtnVariant::Default }, theme)
                                    .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                                        let on_no = match ws.modal.take() {
                                            Some(Modal::Confirm { on_no, .. }) => on_no,
                                            _ => None,
                                        };
                                        ws.modal = None;
                                        if let Some(action) = on_no {
                                            action(ws, window, cx);
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(
                                widgets::button("confirm-yes", yes_label, BtnVariant::Accent, theme)
                                    .on_click(run),
                            ),
                    )
                    .into_any_element()
            }
            Modal::Alert { title, message } => widgets::panel(theme)
                .id("modal-alert")
                .w(px(440.))
                .p(px(24.))
                .flex_col()
                .gap(px(16.))
                .child(
                    div()
                        .text_size(px(14.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(theme.rgb(theme.fg_highlight))
                        .child(title.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.5))
                        .line_height(px(19.0))
                        .text_color(theme.rgb(theme.fg_text))
                        .child(message.clone()),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
                        .mt(px(4.))
                        .child(
                            widgets::button("alert-ok", "OK", BtnVariant::Accent, theme).on_click(
                                cx.listener(|ws: &mut Workspace, _, _, cx| {
                                    ws.modal = None;
                                    ws.show_next_startup_alert(cx);
                                    cx.notify();
                                }),
                            ),
                        ),
                )
                .into_any_element(),
            Modal::Progress { title, label, current, total, cancel } => {
                let pct = if *total > 0 { *current as f32 / *total as f32 } else { 0.0 };
                let cancel = cancel.clone();
                let cancelling = cancel
                    .as_ref()
                    .is_some_and(|c| c.load(std::sync::atomic::Ordering::Relaxed));
                widgets::panel(theme)
                    .id("modal-progress")
                    .w(px(440.))
                    .p(px(24.))
                    .flex_col()
                    .gap(px(14.))
                    .child(
                        div()
                            .text_size(px(14.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.rgb(theme.fg_highlight))
                            .child(title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.rgb(theme.fg_muted))
                            .child(label.clone()),
                    )
                    .child(widgets::progress_bar("modal-progress-bar", pct, theme))
                    .when_some(cancel, |el, flag| {
                        el.child(
                            div().flex().justify_end().child(
                                widgets::button(
                                    "progress-cancel",
                                    if cancelling { "Stopping…" } else { "Cancel" },
                                    BtnVariant::Default,
                                    theme,
                                )
                                .on_click(cx.listener(move |_ws: &mut Workspace, _, _, cx| {
                                    flag.store(true, std::sync::atomic::Ordering::Relaxed);
                                    cx.notify();
                                })),
                            ),
                        )
                    })
                    .into_any_element()
            }
            Modal::EditClass => {
                let classes: Vec<(i64, String)> = app_state(cx)
                    .classes
                    .iter()
                    .map(|c| (c.id, c.name.clone()))
                    .collect();
                let mut rows = Vec::new();
                for (id, name) in classes {
                    rows.push(
                        div()
                            .id(("edit-class-row", id as usize))
                            .px(px(10.))
                            .h(px(30.))
                            .mx(px(4.))
                            .flex()
                            .items_center()
                            .rounded(px(6.))
                            .text_size(px(12.5))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.1)))
                            .child(name)
                            .on_click(cx.listener(move |ws: &mut Workspace, _, _, cx| {
                                ws.apply_edit_class(id, cx);
                            }))
                            .into_any_element(),
                    );
                }
                widgets::panel(theme)
                    .id("modal-edit-class")
                    .w(px(320.))
                    .h(px(440.))
                    .p(px(16.))
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .px(px(4.))
                            .text_size(px(10.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme.rgb(theme.fg_muted))
                            .child("SELECT CLASS"),
                    )
                    .child(
                        div()
                            .id("edit-class-list")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .rounded(px(8.))
                            .bg(theme.rgb(theme.list_bg))
                            .border_1()
                            .border_color(theme.rgba(theme.border, 0.8))
                            .flex_col()
                            .children(rows),
                    )
                    .into_any_element()
            }
        };

        div()
            .id("modal-backdrop")
            .absolute()
            .size_full()
            .top_0()
            .left_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::black().opacity(0.45))
            // Block hover/scroll/clicks from reaching the canvas and panes
            // underneath (otherwise the wheel zooms the image behind dialogs).
            .occlude()
            .on_mouse_down(MouseButton::Left, backdrop_click)
            .child(
                // Swallow presses inside the panel so they don't bubble to the
                // backdrop and dismiss the modal before buttons get on_click.
                div()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(panel),
            )
            .into_any_element()
    }

    fn render_validation_panel(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let (errors, warnings, infos) = crate::validation::summarize(&self.validation_issues);
        let mut rows = Vec::new();
        for (i, issue) in self.validation_issues.iter().enumerate() {
            let (icon_name, color) = match issue.severity {
                crate::validation::Severity::Error => ("error", theme.rgb(theme.error)),
                crate::validation::Severity::Warning => ("warning", theme.rgb(theme.warning)),
                crate::validation::Severity::Info => ("info", theme.rgb(theme.info)),
            };
            let box_index = issue.box_index;
            rows.push(
                div()
                    .id(("validation-row", i))
                    .px(px(8.))
                    .h(px(30.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .mx(px(4.))
                    .rounded(px(6.))
                    .text_size(px(12.))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.1)))
                    .child(div().flex_none().child(super::icons::icon(icon_name, 14.0, color)))
                    .child(issue.message.clone())
                    .on_click(cx.listener(move |ws: &mut Workspace, _, _, cx| {
                        let state = app_state_mut(cx);
                        state.selected_indices.clear();
                        state.selected_indices.insert(box_index);
                        ws.validation_visible = true;
                        cx.notify();
                    }))
                    .into_any_element(),
            );
        }

        widgets::panel(theme)
            .id("validation-panel")
            .absolute()
            .top(px(12.))
            .right(px(12.))
            .w(px(380.))
            .max_h(px(440.))
            .p(px(16.))
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.rgb(theme.fg_highlight))
                            .child("Validation"),
                    )
                    .child(
                        div()
                            .id("validation-close-x")
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.rgba(theme.fg_muted, 0.12)))
                            .child(super::icons::icon("x", 13.0, theme.rgb(theme.fg_muted)))
                            .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                                ws.validation_visible = false;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .text_color(theme.rgb(theme.fg_muted))
                    .child(format!(
                        "{errors} errors · {warnings} warnings · {infos} info"
                    )),
            )
            .child(
                div()
                    .id("validation-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex_col()
                    .children(rows),
            )
            .into_any_element()
    }

    fn render_toasts(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let toasts: Vec<(usize, SharedString)> = self
            .toasts
            .iter()
            .map(|t| (t.id, t.text.clone()))
            .collect();
        div()
            .absolute()
            .bottom(px(16.))
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .items_center()
            .gap(px(6.))
            .children(toasts.into_iter().map(|(id, text)| {
                div()
                    .id(("toast", id))
                    .px(px(12.))
                    .py(px(7.))
                    .rounded(px(8.))
                    .bg(theme.rgba(theme.bg_elevated, 0.95))
                    .border_1()
                    .border_color(theme.rgba(theme.border, 0.9))
                    .shadow_md()
                    .text_size(px(12.))
                    .text_color(theme.rgb(theme.fg_text))
                    .child(text)
            }))
            .into_any_element()
    }
}

// ---------------------------------------------------------------------------
// Root Render
// ---------------------------------------------------------------------------

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.did_focus {
            self.did_focus = true;
            window.focus(&self.focus);
            // Startup: restore session state once, on first layout.
            self.init_from_session(window, cx);
        }
        self.update_title(window, cx);

        let theme = ThemeState::current(cx);
        let show_left = app_state(cx).show_left_sidebar;
        let show_right = app_state(cx).show_right_sidebar;
        let show_filmstrip = app_state(cx).show_filmstrip;

        let root: gpui::Stateful<gpui::Div> = div()
            .id("workspace-root")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.rgb(theme.bg_pane))
            .text_size(px(12.5))
            .text_color(theme.rgb(theme.fg_text))
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::next_image_action))
            .on_action(cx.listener(Self::prev_image_action))
            .on_action(cx.listener(Self::cycle_class_action))
            .on_action(cx.listener(Self::deselect_action))
            .on_action(cx.listener(Self::delete_box_action))
            .on_action(cx.listener(Self::copy_boxes_action))
            .on_action(cx.listener(Self::paste_boxes_action))
            .on_action(cx.listener(Self::duplicate_boxes_action))
            .on_action(cx.listener(Self::edit_box_class))
            .on_action(cx.listener(Self::undo_action))
            .on_action(cx.listener(Self::redo_action))
            .on_action(cx.listener(Self::select_all_boxes))
            .on_action(cx.listener(Self::zoom_in_action))
            .on_action(cx.listener(Self::zoom_out_action))
            .on_action(cx.listener(Self::zoom_reset_action))
            .on_action(cx.listener(Self::toggle_help_overlay))
            .on_action(cx.listener(Self::toggle_unannotated_action))
            .on_action(cx.listener(Self::open_images_action))
            .on_action(cx.listener(Self::save_annotations_action))
            .on_action(cx.listener(Self::auto_label_current_action))
            .on_key_down(cx.listener(Self::on_root_key_down))
            .on_key_up(cx.listener(Self::on_root_key_up))
            .on_modifiers_changed(cx.listener(Self::on_root_modifiers_changed))
            .on_mouse_up(MouseButton::Left, cx.listener(|ws: &mut Workspace, _, _, cx| {
                // End box-list drag selection wherever the mouse lands.
                if ws.box_list_dragging {
                    ws.box_list_dragging = false;
                    ws.box_list_anchor = None;
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(
                |ws: &mut Workspace, event: &MouseMoveEvent, _window, cx| {
                    if ws.box_list_dragging && event.pressed_button.is_none() {
                        ws.box_list_dragging = false;
                        cx.notify();
                    }
                },
            ));

        root.child(self.render_toolbar(cx))
            .child(
                div()
                    .id("main-row")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .when(show_left, |el| el.child(self.render_left_sidebar(cx)))
                    .child(
                        div()
                            .id("canvas-column")
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .id("canvas-area")
                                    .relative()
                                    .flex_1()
                                    .min_h_0()
                                    .bg(theme.rgb(theme.bg_canvas))
                                    .child(self.canvas.clone())
                                    .child(self.render_toasts(cx))
                                    .when(self.validation_visible, |el| {
                                        el.child(self.render_validation_panel(cx))
                                    })
                                    .when(self.help_visible, |el| {
                                        el.child(
                                            div()
                                                .id("help-backdrop")
                                                .absolute()
                                                .inset_0()
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .bg(gpui::black().opacity(0.35))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|ws: &mut Workspace, _, _, cx| {
                                                        ws.help_visible = false;
                                                        cx.notify();
                                                    }),
                                                )
                                                .child(self.render_help_overlay(cx)),
                                        )
                                    }),
                            )
                            .when(show_filmstrip, |el| {
                                el.child(
                                    div()
                                        .flex_none()
                                        .w_full()
                                        .h(px(STRIP_H))
                                        .child(self.filmstrip.clone()),
                                )
                            }),
                    )
                    .when(show_right, |el| el.child(self.render_right_sidebar(cx))),
            )
            .child(self.render_statusbar(cx))
            .when(self.modal.is_some(), |el| el.child(self.render_modal(cx)))
            .into_any_element()
    }
}

use super::widgets::popover;

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
