//! The workspace root view: toolbar, sidebars, canvas area, filmstrip,
//! status bar, modal dialogs, toasts and the help overlay. Action handlers
//! and background flows live in `workspace_actions.rs`.

use std::time::Instant;

use gpui::{
    div, prelude::*, px, uniform_list, App, AppContext, Context, Entity, FocusHandle,
    Focusable, Hsla, IntoElement, MouseButton, MouseMoveEvent, SharedString, Styled,
    UniformListScrollHandle, Window,
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
                ws.load_image(click.0, cx);
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
        });
    }

    pub fn confirm_with_cancel(
        &mut self,
        title: &str,
        message: impl Into<String>,
        on_yes: Box<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>,
        on_no: Box<dyn Fn(&mut Workspace, &mut Window, &mut Context<Workspace>)>,
    ) {
        self.modal = Some(Modal::Confirm {
            title: title.to_string().into(),
            message: message.into(),
            on_yes,
            on_no: Some(on_no),
        });
    }

    /// True when a text entry has focus — configurable shortcuts must not
    /// fire (Python parity). Escape is exempt (handled by caller).
    pub fn entry_focused(&self, window: &Window, cx: &App) -> bool {
        let entry_focus = self.class_search.read(cx).focus_handle(cx);
        entry_focus.is_focused(window)
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
        let filename = app_state(cx)
            .current_image_index
            .checked_sub(0)
            .and_then(|i| app_state(cx).image_list.get(i.max(0) as usize).cloned());
        let title = match filename {
            Some(name) => format!("Annotation Tool - {name}"),
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
        let _ = show_filmstrip;

        div()
            .id("toolbar")
            .h(px(42.))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .gap_1()
            .px_2()
            .bg(theme.rgb(theme.bg_elevated))
            .border_b_1()
            .border_color(theme.rgba(theme.border, 0.7))
            .child(widgets::icon_button("tb-project", "🗂", "Project", theme).on_click({
                cx.listener(move |_ws: &mut Workspace, _, _, cx| {
                    let show = app_state(cx).show_left_sidebar;
                    app_state_mut(cx).show_left_sidebar = !show;
                    cx.notify();
                })
            }).when(!show_left, |el| el))
            .child(widgets::icon_button("tb-undo", "↶", "Undo", theme).on_click(cx.listener(
                |ws: &mut Workspace, _, window, cx| ws.undo_impl(window, cx),
            )))
            .child(widgets::icon_button("tb-redo", "↷", "Redo", theme).on_click(cx.listener(
                |ws: &mut Workspace, _, window, cx| ws.redo_impl(window, cx),
            )))
            .child(widgets::icon_button("tb-validate", "✓", "Validate", theme).on_click(
                cx.listener(|ws: &mut Workspace, _, window, cx| ws.show_validation(window, cx)),
            ))
            .child(
                div()
                    .id("tb-indicator")
                    .px_2()
                    .h(px(26.))
                    .flex()
                    .items_center()
                    .rounded_md()
                    .text_size(px(11.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(self.indicator_color(theme))
                    .child(self.toolbar_indicator()),
            )
            .child(div().flex_1())
            .child(widgets::icon_button("tb-keys", "⌨", "Keys", theme).on_click(cx.listener(
                |ws: &mut Workspace, _, _, cx| {
                    ws.help_visible = !ws.help_visible;
                    cx.notify();
                }
            )))
            .child(widgets::icon_button("tb-filmstrip", "🎞", "Filmstrip", theme).on_click(
                cx.listener(|_ws: &mut Workspace, _, _, cx| {
                    let show = app_state(cx).show_filmstrip;
                    app_state_mut(cx).show_filmstrip = !show;
                    cx.notify();
                }),
            ))
            .child(widgets::icon_button("tb-boxes", "📜", "Box List", theme).on_click(
                cx.listener(move |ws: &mut Workspace, _, _, cx| {
                    let _ = ws;
                    let show = app_state(cx).show_right_sidebar;
                    app_state_mut(cx).show_right_sidebar = !show;
                    cx.notify();
                }),
            ))
            .when(!show_right, |el| el)
            .into_any_element()
    }

    fn render_statusbar(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let coords = self.canvas.read(cx).coords_text(app_state(cx));
        div()
            .id("statusbar")
            .h(px(24.))
            .flex_none()
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .bg(theme.rgb(theme.bg_elevated))
            .border_t_1()
            .border_color(theme.rgba(theme.border, 0.7))
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.rgb(theme.fg_muted))
                    .child(self.status.clone()),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.rgb(theme.fg_muted))
                    .child(coords),
            )
            .into_any_element()
    }

    fn render_left_sidebar(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let state = app_state(cx);
        let (_annotated, _total) = state.get_annotated_count();

        let tabs = div()
            .flex()
            .gap_1()
            .px_2()
            .pt_2()
            .child(widgets::tab(
                "tab-project",
                "Project",
                self.left_tab == LeftTab::Project,
                theme,
            ).on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                ws.left_tab = LeftTab::Project;
                cx.notify();
            })))
            .child(widgets::tab(
                "tab-classes",
                "Classes",
                self.left_tab == LeftTab::Classes,
                theme,
            ).on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                ws.left_tab = LeftTab::Classes;
                cx.notify();
            })));

        let body: gpui::AnyElement = match self.left_tab {
            LeftTab::Project => self.render_project_tab(cx),
            LeftTab::Classes => self.render_classes_tab(cx),
        };

        div()
            .id("left-sidebar")
            .w(px(280.))
            .flex_none()
            .h_full()
            .flex_col()
            .bg(theme.rgb(theme.bg_sidebar))
            .border_r_1()
            .border_color(theme.rgba(theme.border, 0.7))
            .child(tabs)
            .child(body)
            .into_any_element()
    }

    fn render_project_tab(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let (dir_label, pct, progress_text) = {
            let state = app_state(cx);
            let dir_label: SharedString = if state.image_dir.is_empty() {
                "No directory selected".into()
            } else {
                state.image_dir.clone().into()
            };
            let (annotated, total) = state.get_annotated_count();
            let pct = if total > 0 { annotated as f32 / total as f32 } else { 0.0 };
            (
                dir_label,
                pct,
                format!("{annotated}/{total} ({}%)", (pct * 100.0) as i32).into(),
            )
        };

        // Directory section
        let directory = div().child(widgets::collapsible_header(
            "col-dirs",
            "DIRECTORY",
            self.collapse_dirs,
            theme,
        )
        .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
            ws.collapse_dirs = !ws.collapse_dirs;
            cx.notify();
        })));
        let directory_content = div()
            .px_2()
            .pb_2()
            .flex_col()
            .gap_1p5()
            .child(
                widgets::button("btn-open-images", "Open Images", BtnVariant::Accent, theme)
                    .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                        ws.open_images_dialog(window, cx)
                    })),
            )
            .child(
                widgets::button("btn-set-output", "Set Output Dir", BtnVariant::Default, theme)
                    .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                        ws.set_output_dir_dialog(window, cx)
                    })),
            )
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.rgb(theme.fg_muted))
                    .line_height(px(15.0))
                    .child(dir_label),
            )
            .child(widgets::caption("Recent Projects:", theme))
            .child(self.render_recents_dropdown(cx));

        // Files section
        let files_header = div().child(widgets::collapsible_header(
            "col-files",
            "FILES",
            self.collapse_files,
            theme,
        ).on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
            ws.collapse_files = !ws.collapse_files;
            cx.notify();
        })));
        let file_rows = self.render_file_list(cx);

        let files_content = div()
            .px_2()
            .pb_2()
            .flex_col()
            .gap_1p5()
            .child(self.render_filter_dropdown(cx))
            .child(
                div()
                    .flex()
                    .gap_1p5()
                    .child(
                        widgets::button("btn-filter", "Filter", BtnVariant::Default, theme)
                            .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                                ws.apply_image_filter(window, cx)
                            })),
                    )
                    .child(
                        widgets::button("btn-clear-filter", "Clear", BtnVariant::Default, theme)
                            .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                                ws.clear_image_filter(window, cx)
                            })),
                    ),
            )
            .child(
                widgets::checkbox(
                    "chk-unannotated",
                    "Show Unannotated Only (U)",
                    app_state(cx).unannotated_filter_active,
                    theme,
                )
                .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                    ws.toggle_unannotated_filter(window, cx)
                })),
            )
            .child(div().h(px(300.)).child(file_rows));

        // Progress pinned at the bottom
        let progress = div()
            .px_2()
            .pb_2()
            .flex_none()
            .flex_col()
            .gap_1()
            .child(widgets::section_label("PROGRESS", theme))
            .child(widgets::progress_bar(
                "progress-bar",
                pct,
                Some(progress_text),
                theme,
            ));

        div()
            .id("project-tab-scroll")
            .flex_1()
            .min_h_0()
            .flex_col()
            .gap_1()
            .overflow_y_scroll()
            .child(directory)
            .when(self.collapse_dirs, |el| el.child(directory_content))
            .child(files_header)
            .when(self.collapse_files, |el| el.child(files_content))
            .child(div().flex_1())
            .child(progress)
            .into_any_element()
    }

    fn render_recents_dropdown(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let recents = app_state(cx).recent_projects.clone();
        let selected_label: SharedString = match self.selected_recent {
            Some(i) if i < recents.len() => recents[i].name.clone().into(),
            _ => {
                if recents.is_empty() {
                    "No recent projects".into()
                } else {
                    "Select project…".into()
                }
            }
        };

        let mut dd = div()
            .id("recents-dd")
            .relative()
            .w_full()
            .child(
                widgets::button("recents-btn", selected_label, BtnVariant::Default, theme)
                    .on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
                        ws.recents_open = !ws.recents_open;
                        cx.notify();
                    })),
            );

        if self.recents_open {
            let mut rows = Vec::new();
            for (i, p) in recents.iter().enumerate() {
                rows.push(
                    div()
                        .id(("recent", i))
                        .px_2()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .rounded_sm()
                        .text_size(px(11.5))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.rgba(theme.list_hover, 0.9)))
                        .child(p.name.clone())
                        .on_click({
                            cx.listener(move |ws: &mut Workspace, _, _, cx| {
                                ws.selected_recent = Some(i);
                                ws.recents_open = false;
                                cx.notify();
                            })
                        })
                        .into_any_element(),
                );
            }
            dd = dd.child(
                div()
                    .id("recents-popover")
                    .absolute()
                    .top(px(32.))
                    .left_0()
                    .right_0()
                    .rounded_md()
                    .bg(theme.rgb(theme.bg_elevated))
                    .border_1()
                    .border_color(theme.rgba(theme.border, 1.0))
                    .shadow_md()
                    .py_1()
                    .max_h(px(180.))
                    .overflow_y_scroll()
                    .on_mouse_down_out(cx.listener(|ws: &mut Workspace, _, _, cx| {
                        ws.recents_open = false;
                        cx.notify();
                    }))
                    .children(rows),
            );
        }

        dd.child(
            widgets::button(
                "btn-load-recent",
                "Load Selected Project",
                BtnVariant::Default,
                theme,
            )
            .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                ws.load_recent_project(window, cx)
            })),
        )
        .into_any_element()
    }

    fn render_filter_dropdown(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let classes = app_state(cx).classes.clone();
        let label: SharedString = match self.filter_class_id {
            Some(id) => match classes.iter().find(|c| c.id == id) {
                Some(c) => format!("{}: {}", c.id, c.name).into(),
                None => "Filter by class…".into(),
            },
            None => "Filter by class…".into(),
        };

        let mut dd = div().id("filter-dd").relative().w_full().child(
            widgets::button("filter-btn", label, BtnVariant::Default, theme).on_click(
                cx.listener(|ws: &mut Workspace, _, _, cx| {
                    ws.filter_open = !ws.filter_open;
                    cx.notify();
                }),
            ),
        );

        if self.filter_open {
            let mut rows = Vec::new();
            for c in &classes {
                let id = c.id;
                rows.push(
                    div()
                        .id(("filter-class", id as usize))
                        .px_2()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .gap_2()
                        .rounded_sm()
                        .text_size(px(11.5))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.rgba(theme.list_hover, 0.9)))
                        .child(widgets::color_swatch(c.color, px(10.0), theme))
                        .child(format!("{}: {}", c.id, c.name))
                        .on_click(cx.listener(move |ws: &mut Workspace, _, _, cx| {
                            ws.filter_class_id = Some(id);
                            ws.filter_open = false;
                            cx.notify();
                        }))
                        .into_any_element(),
                );
            }
            dd = dd.child(
                div()
                    .id("filter-popover")
                    .absolute()
                    .top(px(32.))
                    .left_0()
                    .right_0()
                    .rounded_md()
                    .bg(theme.rgb(theme.bg_elevated))
                    .border_1()
                    .border_color(theme.rgba(theme.border, 1.0))
                    .shadow_md()
                    .py_1()
                    .max_h(px(220.))
                    .overflow_y_scroll()
                    .on_mouse_down_out(cx.listener(|ws: &mut Workspace, _, _, cx| {
                        ws.filter_open = false;
                        cx.notify();
                    }))
                    .children(rows),
            );
        }

        dd.into_any_element()
    }

    fn render_file_list(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let filenames: Vec<String> = app_state(cx).image_list.clone();
        let current = app_state(cx).current_image_index;
        let count = filenames.len();

        let on_click = std::rc::Rc::new(cx.listener({
            move |ws: &mut Workspace, ix: &usize, _window, cx| {
                if ws.modal.is_none() {
                    ws.load_image(*ix, cx);
                }
            }
        }));

        uniform_list("file-list", count, move |range, _window, _cx| {
            let mut rows = Vec::new();
            for ix in range {
                let is_current = current == ix as i64;
                rows.push(
                    div()
                        .id(("file-row", ix))
                        .pl_2()
                        .pr_2()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .rounded_sm()
                        .text_size(px(11.5))
                        .cursor_pointer()
                        .when(is_current, |el| {
                            el.bg(theme.rgba(theme.selection, 0.35))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(theme.rgb(theme.fg_highlight))
                        })
                        .when(!is_current, |el| el.hover(|s| s.bg(theme.rgba(theme.list_hover, 0.9))))
                        .child(filenames[ix].clone())
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
        .flex_1()
        .min_h_0()
        .rounded_md()
        .bg(theme.rgb(theme.list_bg))
        .border_1()
        .border_color(theme.rgba(theme.border, 0.8))
        .into_any_element()
    }

    fn render_classes_tab(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let state = app_state(cx);
        let show_labels = state.show_labels;
        let auto_save = state.auto_save;
        let _ = (show_labels, auto_save);

        let classes_header = div().child(widgets::collapsible_header(
            "col-classes",
            "CLASS LIST",
            self.collapse_classes,
            theme,
        ).on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
            ws.collapse_classes = !ws.collapse_classes;
            cx.notify();
        })));

        let classes_content = div()
            .px_2()
            .pb_2()
            .flex_col()
            .gap_1p5()
            .child(self.class_search.clone())
            .child(self.render_class_list(cx));

        let tools_header = div().child(widgets::collapsible_header(
            "col-tools",
            "TOOLS",
            self.collapse_tools,
            theme,
        ).on_click(cx.listener(|ws: &mut Workspace, _, _, cx| {
            ws.collapse_tools = !ws.collapse_tools;
            cx.notify();
        })));

        let tools_content = div()
            .px_2()
            .pb_2()
            .flex_col()
            .gap_1p5()
            .child(
                widgets::button("btn-copy", "Copy Boxes (Ctrl+C)", BtnVariant::Default, theme)
                    .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                        ws.copy_boxes_impl(window, cx)
                    })),
            )
            .child(
                widgets::button("btn-paste", "Paste Boxes (Ctrl+V)", BtnVariant::Default, theme)
                    .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                        ws.paste_boxes_impl(window, cx)
                    })),
            )
            .child(
                widgets::button("btn-preview-clip", "Preview Clipboard", BtnVariant::Default, theme)
                    .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                        ws.toggle_clipboard_preview(window, cx)
                    })),
            )
            .child(
                widgets::button("btn-duplicate", "Duplicate Boxes (Ctrl+D)", BtnVariant::Default, theme)
                    .on_click(cx.listener(|ws: &mut Workspace, _, window, cx| {
                        ws.duplicate_boxes_impl(window, cx)
                    })),
            )
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
            );

        div()
            .flex_1()
            .min_h_0()
            .flex_col()
            .gap_1()
            .child(classes_header)
            .when(self.collapse_classes, |el| el.child(classes_content))
            .child(tools_header)
            .when(self.collapse_tools, |el| el.child(tools_content))
            .child(div().flex_1())
            .child(
                div()
                    .px_2()
                    .pb_3()
                    .child(
                        widgets::button("btn-settings", "Settings", BtnVariant::Default, theme)
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

        let on_click = std::rc::Rc::new(cx.listener(move |ws: &mut Workspace, ix: &usize, _window, cx| {
            if let Some(c) = app_state(cx).classes.get(*ix) {
                app_state_mut(cx).current_class_index = c.id;
                cx.notify();
                let _ = ws;
            }
        }));

        uniform_list("class-list", count, move |range, _window, _cx| {
            let mut rows = Vec::new();
            for rix in range {
                let (ix, _id, name, color, n) = &filtered[rix];
                let ix = *ix;
                let is_current = current_class == ix as i64;
                let label: SharedString = if *n > 0 {
                    format!("  {}: {}   ({})", _id, name, n).into()
                } else {
                    format!("  {}: {}", _id, name).into()
                };
                rows.push(
                    div()
                        .id(("class-row", rix))
                        .pl_1()
                        .pr_2()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .gap_1()
                        .rounded_sm()
                        .text_size(px(11.5))
                        .cursor_pointer()
                        .when(is_current, |el| {
                            el.bg(theme.rgba(theme.selection, 0.35))
                                .text_color(theme.rgb(theme.fg_highlight))
                        })
                        .when(!is_current, |el| el.hover(|s| s.bg(theme.rgba(theme.list_hover, 0.9))))
                        .child(widgets::color_swatch(*color, px(12.0), theme))
                        .child(label)
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
        .h(px(320.))
        .rounded_md()
        .bg(theme.rgb(theme.list_bg))
        .border_1()
        .border_color(theme.rgba(theme.border, 0.8))
        .into_any_element()
    }

    fn render_right_sidebar(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let state = app_state(cx);
        let boxes: Vec<(usize, String)> = state
            .boxes
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let name = state
                    .classes
                    .iter()
                    .find(|c| c.id == b.class_id)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| "Unknown".to_string());
                (i, name)
            })
            .collect();
        let selected = state.selected_indices.clone();
        let count = boxes.len();

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
                let (i, name) = &boxes[ix];
                let i = *i;
                let name = name.clone();
                let is_selected = selected.contains(&i);
                rows.push(
                    div()
                        .id(("box-row", ix))
                        .pl_2()
                        .pr_2()
                        .h(px(24.))
                        .flex()
                        .items_center()
                        .rounded_sm()
                        .text_size(px(11.5))
                        .cursor_pointer()
                        .when(is_selected, |el| {
                            el.bg(theme.rgb(theme.selection))
                                .text_color(theme.rgb(theme.fg_highlight))
                        })
                        .when(!is_selected, |el| el.hover(|s| s.bg(theme.rgba(theme.list_hover, 0.9))))
                        .child(format!("{}: {}", i, name))
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
        .flex_1()
        .min_h_0()
        .rounded_md()
        .bg(theme.rgb(theme.list_bg))
        .border_1()
        .border_color(theme.rgba(theme.border, 0.8));

        div()
            .id("right-sidebar")
            .w(px(250.))
            .flex_none()
            .h_full()
            .flex_col()
            .gap_1()
            .p_2()
            .bg(theme.rgb(theme.bg_sidebar))
            .border_l_1()
            .border_color(theme.rgba(theme.border, 0.7))
            .child(widgets::section_label("BOX LABELS", theme))
            .child(list)
            .into_any_element()
    }

    fn render_help_overlay(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let sections: &[(&str, &[(&str, &str)])] = &[
            (
                "NAVIGATION",
                &[
                    ("A / ←", "Previous Image"),
                    ("D / →", "Next Image"),
                    ("↓", "Cycle Class"),
                    ("Escape", "Deselect Class"),
                ],
            ),
            (
                "EDITING",
                &[
                    ("Click + Drag", "Draw Box"),
                    ("Click", "Stamp Template Box"),
                    ("Q / Delete", "Delete Selected"),
                    ("Ctrl+A", "Select All Boxes"),
                    ("Ctrl+C", "Copy Boxes"),
                    ("Space / Ctrl+V", "Paste Boxes"),
                    ("Ctrl+D", "Duplicate Boxes"),
                    ("Ctrl+E", "Edit Box Class"),
                ],
            ),
            ("HISTORY", &[("Ctrl+Z", "Undo"), ("Ctrl+Y", "Redo")]),
            (
                "VIEW",
                &[
                    ("Mouse Wheel", "Zoom In/Out"),
                    ("Ctrl + / -", "Zoom In/Out"),
                    ("Ctrl+0", "Fit to Window"),
                    ("Right Click + Drag", "Pan"),
                    ("U", "Toggle Unannotated Filter"),
                    ("?", "Toggle This Overlay"),
                ],
            ),
            (
                "PRESETS",
                &[
                    ("Tap Ctrl+1-9", "Apply Box Preset"),
                    ("Hold Ctrl+1-9", "Preview Box Preset"),
                    ("Ctrl+Shift+1-9", "Save Box Preset"),
                ],
            ),
        ];

        let mut columns: Vec<gpui::AnyElement> = Vec::new();
        for col_sections in [&sections[0..3.min(sections.len())], &sections[3.min(sections.len())..]] {
            let mut col = div().flex_col().gap_2();
            for (name, rows) in col_sections.iter() {
                let mut section = div().flex_col().gap_1();
                section = section.child(
                    div()
                        .text_size(px(11.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(theme.rgb(theme.button_highlight))
                        .child(*name),
                );
                for (key, desc) in rows.iter() {
                    section = section.child(
                        div()
                            .flex()
                            .gap_2()
                            .child(
                                div()
                                    .w(px(120.))
                                    .text_size(px(11.0))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme.rgb(theme.fg_highlight))
                                    .child(*key),
                            )
                            .child(
                                div()
                                    .text_size(px(11.0))
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
            .w(px(640.))
            .p_5()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .text_size(px(15.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.rgb(theme.fg_highlight))
                    .child("⌨  KEYBOARD SHORTCUTS"),
            )
            .child(div().flex().gap_6().children(columns))
            .child(widgets::caption(
                "Press ? or click 'Keys' button to close",
                theme,
            ))
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
            Modal::Confirm { title, message, .. } => {
                let title = title.clone();
                let message = message.clone();
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
                    .w(px(420.))
                    .p_4()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.rgb(theme.fg_highlight))
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(12.0))
                            .line_height(px(17.0))
                            .text_color(theme.rgb(theme.fg_text))
                            .child(message),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap_2()
                            .child(
                                widgets::button("confirm-no", "Cancel", BtnVariant::Default, theme)
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
                                widgets::button("confirm-yes", "Yes", BtnVariant::Accent, theme)
                                    .on_click(run),
                            ),
                    )
                    .into_any_element()
            }
            Modal::Alert { title, message } => widgets::panel(theme)
                .id("modal-alert")
                .w(px(420.))
                .p_4()
                .flex_col()
                .gap_3()
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(theme.rgb(theme.fg_highlight))
                        .child(title.clone()),
                )
                .child(
                    div()
                        .text_size(px(12.0))
                        .line_height(px(17.0))
                        .text_color(theme.rgb(theme.fg_text))
                        .child(message.clone()),
                )
                .child(
                    div()
                        .flex()
                        .justify_end()
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
            Modal::Progress { title, label, current, total } => {
                let pct = if *total > 0 { *current as f32 / *total as f32 } else { 0.0 };
                widgets::panel(theme)
                    .id("modal-progress")
                    .w(px(420.))
                    .p_4()
                    .flex_col()
                    .gap_3()
                    .child(
                        div()
                            .text_size(px(13.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.rgb(theme.fg_highlight))
                            .child(title.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .text_color(theme.rgb(theme.fg_muted))
                            .child(label.clone()),
                    )
                    .child(widgets::progress_bar(
                        "modal-progress-bar",
                        pct,
                        Some(format!("{}%", (pct * 100.0) as i32).into()),
                        theme,
                    ))
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
                            .pl_2()
                            .pr_2()
                            .h(px(28.))
                            .flex()
                            .items_center()
                            .rounded_sm()
                            .text_size(px(12.0))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.rgba(theme.list_hover, 0.9)))
                            .child(name)
                            .on_click(cx.listener(move |ws: &mut Workspace, _, _, cx| {
                                ws.apply_edit_class(id, cx);
                            }))
                            .into_any_element(),
                    );
                }
                widgets::panel(theme)
                    .id("modal-edit-class")
                    .w(px(300.))
                    .h(px(420.))
                    .p_3()
                    .flex_col()
                    .gap_2()
                    .child(
                        div()
                            .text_size(px(12.0))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme.rgb(theme.fg_highlight))
                            .child("SELECT CLASS"),
                    )
                    .child(
                        div()
                            .id("edit-class-list")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .rounded_md()
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
            .bg(gpui::black().opacity(0.5))
            .on_mouse_down(MouseButton::Left, backdrop_click)
            .child(panel)
            .into_any_element()
    }

    fn render_validation_panel(&mut self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let theme = ThemeState::current(cx);
        let (errors, warnings, infos) = crate::validation::summarize(&self.validation_issues);
        let mut rows = Vec::new();
        for (i, issue) in self.validation_issues.iter().enumerate() {
            let icon = match issue.severity {
                crate::validation::Severity::Error => "❌",
                crate::validation::Severity::Warning => "⚠",
                crate::validation::Severity::Info => "ℹ",
            };
            let box_index = issue.box_index;
            rows.push(
                div()
                    .id(("validation-row", i))
                    .pl_2()
                    .pr_2()
                    .h(px(24.))
                    .flex()
                    .items_center()
                    .gap_2()
                    .rounded_sm()
                    .text_size(px(11.5))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.rgba(theme.list_hover, 0.9)))
                    .child(
                        div()
                            .flex_none()
                            .text_color(if issue.severity == crate::validation::Severity::Error {
                                theme.rgb(theme.error)
                            } else if issue.severity == crate::validation::Severity::Warning {
                                theme.rgb(theme.warning)
                            } else {
                                theme.rgb(theme.info)
                            })
                            .child(icon),
                    )
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
            .top(px(52.))
            .right(px(262.))
            .w(px(360.))
            .max_h(px(420.))
            .p_3()
            .flex_col()
            .gap_2()
            .shadow_lg()
            .child(
                div()
                    .text_size(px(13.0))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme.rgb(theme.fg_highlight))
                    .child("Validation Results"),
            )
            .child(
                div()
                    .text_size(px(11.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(theme.rgb(theme.button_highlight))
                    .child(format!(
                        "Found: {errors} errors, {warnings} warnings, {infos}"
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
            .child(
                div().flex().justify_end().child(
                    widgets::button("validation-close", "Close", BtnVariant::Default, theme).on_click(
                        cx.listener(|ws: &mut Workspace, _, _, cx| {
                            ws.validation_visible = false;
                            cx.notify();
                        }),
                    ),
                ),
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
            .bottom(px(34.0 + STRIP_H))
            .left_0()
            .right_0()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .children(toasts.into_iter().map(|(id, text)| {
                div()
                    .id(("toast", id))
                    .px_3()
                    .py_1p5()
                    .rounded_lg()
                    .bg(theme.rgba(theme.bg_elevated, 0.92))
                    .border_1()
                    .border_color(theme.rgba(theme.border, 0.9))
                    .shadow_md()
                    .text_size(px(11.5))
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
            .text_size(px(12.5))
            .text_color(theme.rgb(theme.fg_text))
            .font_family(".SystemUIText") // system UI font (Segoe UI on Windows)
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
            .on_action(cx.listener(Self::set_output_dir_action))
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
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .flex_col()
                            .child(
                                div()
                                    .h_full()
                                    .when(show_filmstrip, |el| el.pb(px(66.)))
                                    .child(self.canvas.clone()),
                            )
                            .when(show_filmstrip, |el| {
                                el.child(
                                    div()
                                        .absolute()
                                        .bottom_0()
                                        .left_0()
                                        .w_full()
                                        .child(self.filmstrip.clone()),
                                )
                            })
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
                    .when(show_right, |el| el.child(self.render_right_sidebar(cx))),
            )
            .child(self.render_statusbar(cx))
            .when(self.modal.is_some(), |el| el.child(self.render_modal(cx)))
            .into_any_element()
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
