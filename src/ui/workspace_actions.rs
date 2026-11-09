//! All workspace action handlers and background flows — the port of
//! `controller.py`: navigation, editing, filters, presets, batch operations,
//! class remapping, and the modal progress-task runner.

use std::path::{Path, PathBuf};
use std::sync::mpsc as std_mpsc;

use gpui::{prelude::*, Context, ModifiersChangedEvent, Window};
use rfd::AsyncFileDialog;

use crate::actions::*;
use crate::classes;
use crate::state::model::PreviewSource;
use crate::state::paths;
use crate::yolo::{self, BBox};

use super::canvas::{app_state, app_state_mut};
use super::dialogs::SettingsView;
use super::workspace::{Modal, Workspace};

const PRESET_HOLD_MS: u64 = 250;

/// Result payload for modal background tasks.
pub enum TaskValue {
    ClassRemap {
        backup_dir: Option<String>,
        updated: usize,
    },
    BatchReplace {
        backup_dir: Option<String>,
        updated: usize,
    },
}

enum TaskMsg {
    Progress(usize, usize, String),
    Done(Result<TaskValue, String>),
}

impl Workspace {
    // ------------------------------------------------------------------
    // Action registration
    // ------------------------------------------------------------------

    pub fn copy_boxes_action(&mut self, _: &CopyBoxes, window: &mut Window, cx: &mut Context<Self>) {
        self.copy_boxes_impl(window, cx);
    }

    pub fn paste_boxes_action(&mut self, _: &PasteBoxes, window: &mut Window, cx: &mut Context<Self>) {
        self.paste_boxes_impl(window, cx);
    }

    pub fn duplicate_boxes_action(&mut self, _: &DuplicateBoxes, window: &mut Window, cx: &mut Context<Self>) {
        self.duplicate_boxes_impl(window, cx);
    }

    pub fn undo_action(&mut self, _: &UndoAction, window: &mut Window, cx: &mut Context<Self>) {
        self.undo_impl(window, cx);
    }

    pub fn redo_action(&mut self, _: &RedoAction, window: &mut Window, cx: &mut Context<Self>) {
        self.redo_impl(window, cx);
    }

    pub fn open_images_action(&mut self, _: &OpenImages, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_images_dialog(cx);
    }

    // ------------------------------------------------------------------
    // Startup
    // ------------------------------------------------------------------

    pub fn init_from_session(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        // Directory existence checks (Python: startup warning dialogs).
        let image_dir = app_state(cx).image_dir.clone();
        if !image_dir.is_empty() && !Path::new(&image_dir).is_dir() {
            self.startup_alerts.push((
                "Session Warning".into(),
                format!(
                    "The last image directory no longer exists:\n{image_dir}\n\nPlease select a new directory."
                ),
            ));
            app_state_mut(cx).image_dir = String::new();
            app_state_mut(cx).output_dir = String::new();
            app_state_mut(cx).current_image_index = -1;
        }
        if !app_state(cx).image_dir.is_empty() {
            let saved_index = app_state(cx)
                .session
                .get("last_image_index")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            self.load_images(cx);
            let len = app_state(cx).image_list.len();
            if len > 0 {
                let index = if (0..len as i64).contains(&saved_index) {
                    saved_index as usize
                } else {
                    0
                };
                self.load_image(index, cx);
            }
            self.refresh_class_counts(cx);
        }
        self.show_next_startup_alert(cx);
        self.start_autosave(cx);
    }

    pub fn show_next_startup_alert(&mut self, cx: &mut Context<Self>) {
        if self.modal.is_none() {
            if let Some((title, message)) = self.startup_alerts.pop() {
                self.alert(&title, message);
                cx.notify();
            }
        }
    }

    fn start_autosave(&mut self, cx: &mut Context<Self>) {
        let ws = cx.entity();
        cx.spawn(async move |_this: gpui::WeakEntity<Workspace>, cx: &mut gpui::AsyncApp| {
            loop {
                smol::Timer::after(std::time::Duration::from_secs(30)).await;
                let Ok(continue_running) = cx.update(|cx| {
                    ws.update(cx, |_ws, cx| {
                        if app_state(cx).auto_save && app_state(cx).current_image_index != -1 {
                            app_state_mut(cx).save_annotations();
                        }
                        app_state_mut(cx).save_session();
                        cx.notify();
                    });
                    true
                }) else {
                    return;
                };
                if !continue_running {
                    return;
                }
            }
        })
        .detach();
    }

    // ------------------------------------------------------------------
    // Raw key handling: preset slots, modal escape
    // ------------------------------------------------------------------

    pub fn on_root_key_down(
        &mut self,
        event: &gpui::KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = &event.keystroke.key;
        let modifiers = &event.keystroke.modifiers;

        // Escape: close modal/popover first; otherwise deselect class.
        if key == "escape" {
            if self.task_running {
                return;
            }
            if let Some(Modal::Settings(_)) = self.modal {
                self.modal = None;
                cx.notify();
                return;
            }
            if self.recents_open || self.filter_open {
                self.recents_open = false;
                self.filter_open = false;
                cx.notify();
                return;
            }
            if matches!(self.modal, Some(Modal::EditClass) | Some(Modal::Alert { .. }) | Some(Modal::Confirm { .. })) {
                self.modal = None;
                self.show_next_startup_alert(cx);
                cx.notify();
                return;
            }
            if self.validation_visible {
                self.validation_visible = false;
                cx.notify();
                return;
            }
            // Escape is exempt from the entry-focus guard (Python parity).
            if !self.task_running {
                app_state_mut(cx).current_class_index = -1;
                app_state_mut(cx).clear_preview();
                cx.notify();
            }
            return;
        }

        if self.task_running {
            return;
        }

        // Preset slots: Ctrl+1..9 tap/hold, Ctrl+Shift+1..9 save.
        let slot_from_key = |key: &str| -> Option<u8> {
            if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() {
                let d = key.as_bytes()[0] - b'0';
                if (1..=9).contains(&d) {
                    return Some(d);
                }
            }
            None
        };
        let is_numpad = false; // 0.2.2 Keystroke has no keypad flag

        if modifiers.control && !modifiers.shift && !modifiers.alt {
            if let Some(slot) = slot_from_key(key) {
                if self.entry_focused(window, cx) || self.preset_lock {
                    return;
                }
                let key_name = if is_numpad { format!("numpad{slot}") } else { format!("digit{slot}") };
                // Skip when the same hold is already pending.
                if let Some(p) = &self.preset_pending {
                    if p.slot == slot && p.key == key_name {
                        return;
                    }
                }
                self.preset_pending = Some(super::workspace::PresetPending {
                    slot,
                    key: key_name,
                    shown: false,
                });
                let ws = cx.entity();
                cx.spawn(async move |_this: gpui::WeakEntity<Workspace>, cx: &mut gpui::AsyncApp| {
                    smol::Timer::after(std::time::Duration::from_millis(PRESET_HOLD_MS)).await;
                    let _ = cx.update(|cx| {
                        ws.update(cx, |ws, cx| {
                            if let Some(p) = &mut ws.preset_pending {
                                if !p.shown {
                                    p.shown = true;
                                    ws.preset_lock = true;
                                    let slot = p.slot;
                                    ws.preview_preset(slot, false, cx);
                                }
                            }
                            cx.notify();
                        });
                    });
                })
                .detach();
            }
        } else if modifiers.control && modifiers.shift && !modifiers.alt {
            if let Some(slot) = slot_from_key(key) {
                if !self.entry_focused(window, cx) {
                    self.save_preset(slot, cx);
                }
            }
        }
    }

    pub fn on_root_key_up(
        &mut self,
        event: &gpui::KeyUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = &event.keystroke.key;
        let slot_from_key = |key: &str| -> Option<u8> {
            if key.len() == 1 && key.as_bytes()[0].is_ascii_digit() {
                let d = key.as_bytes()[0] - b'0';
                if (1..=9).contains(&d) {
                    return Some(d);
                }
            }
            None
        };
        if let Some(slot) = slot_from_key(key) {
            let key_name = if false {
                format!("numpad{slot}")
            } else {
                format!("digit{slot}")
            };
            if let Some(p) = self.preset_pending.take() {
                if p.slot == slot && p.key == key_name {
                    if !p.shown {
                        // Tap → apply
                        self.apply_preset(slot, cx);
                    }
                    // If preview was shown, do nothing (release of ctrl hides it).
                } else {
                    self.preset_pending = Some(p);
                }
            }
        }
        let _ = window;
    }

    pub fn on_root_modifiers_changed(
        &mut self,
        event: &ModifiersChangedEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !event.modifiers.control {
            self.preset_pending = None;
            if self.preset_lock {
                self.preset_lock = false;
                let state = app_state(cx);
                if state.preview_visible {
                    let slot = state.preview_slot;
                    app_state_mut(cx).clear_preview();
                    if let Some(slot) = slot {
                        self.set_status(format!("Preset {slot} preview hidden"), cx);
                    }
                }
                cx.notify();
            }
        }
    }

    // ------------------------------------------------------------------
    // Directory / project flows
    // ------------------------------------------------------------------

    pub fn open_images_dialog(&mut self, cx: &mut Context<Self>) {
        if self.task_running || matches!(self.modal, Some(Modal::Settings(_))) {
            return;
        }
        self.guard_unsaved(|ws, cx| ws.open_images_picker(cx), cx);
    }

    fn open_images_picker(&mut self, cx: &mut Context<Self>) {
        let ws = cx.entity();
        cx.spawn(async move |_this: gpui::WeakEntity<Workspace>, cx: &mut gpui::AsyncApp| {
            let Some(dir) = AsyncFileDialog::new()
                .set_title("Select Image Directory")
                .pick_folder()
                .await
                .and_then(|f| f.path().to_str().map(|s| s.to_string()))
            else {
                return;
            };
            let _ = cx.update(|cx| {
                ws.update(cx, |ws, cx| {
                    // Autosave the current image into its folder first.
                    let (idx, auto) = {
                        let s = app_state(cx);
                        (s.current_image_index, s.auto_save)
                    };
                    if idx != -1 && auto {
                        app_state_mut(cx).save_annotations();
                    }
                    ws.finish_open_dir(dir, cx);
                });
            });
        })
        .detach();
    }

    pub fn finish_open_dir(&mut self, image_dir: String, cx: &mut Context<Self>) {
        if let Some(Modal::Progress { .. }) = self.modal {
            self.modal = None;
        }
        self.task_running = false;
        {
            let state = app_state_mut(cx);
            state.current_image_index = -1;
            state.boxes.clear();
            state.selected_indices.clear();
            state.image_dir = image_dir;
            // Labels are always read/written alongside the images.
            state.output_dir = state.image_dir.clone();
            state.refresh_preset_scope();
        }
        crate::images::ImageStore::clear_all(cx);
        self.filmstrip.update(cx, |strip, _cx| strip.clear());
        self.load_images(cx);
        if !app_state(cx).image_list.is_empty() {
            self.load_image(0, cx);
        }
        {
            let state = app_state_mut(cx);
            state.add_to_recent_projects(None);
            state.save_session();
        }
        self.refresh_class_counts(cx);
        self.sync_filmstrip_current(cx);
        cx.notify();
    }

    pub fn load_recent_project(&mut self, cx: &mut Context<Self>) {
        self.guard_unsaved(|ws, cx| ws.load_recent_project_now(cx), cx);
    }

    fn load_recent_project_now(&mut self, cx: &mut Context<Self>) {
        let Some(idx) = self.selected_recent else {
            self.alert("Warning", "Please select a valid project.");
            cx.notify();
            return;
        };
        let Some(project) = app_state(cx).recent_projects.get(idx).cloned() else {
            self.alert("Warning", "Please select a valid project.");
            cx.notify();
            return;
        };
        let image_dir = project.image_dir.clone();
        if image_dir.is_empty() || !Path::new(&image_dir).is_dir() {
            self.alert("Error", format!("Image directory no longer exists:\n{image_dir}"));
            cx.notify();
            return;
        }

        {
            let state = app_state_mut(cx);
            if state.auto_save && state.current_image_index != -1 {
                state.save_annotations();
            }
            state.current_image_index = -1;
            state.boxes.clear();
            state.selected_indices.clear();
            state.image_dir = image_dir.clone();
            state.output_dir = image_dir.clone();
            state.refresh_preset_scope();
            state.save_session();
        }
        crate::images::ImageStore::clear_all(cx);
        self.filmstrip.update(cx, |strip, _cx| strip.clear());
        self.load_images(cx);
        if !app_state(cx).image_list.is_empty() {
            self.load_image(0, cx);
        }
        self.refresh_class_counts(cx);
        let name = if project.name.is_empty() { "Unknown".to_string() } else { project.name.clone() };
        self.set_status(format!("Loaded project: {name}"), cx);
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Image list + navigation
    // ------------------------------------------------------------------

    pub fn load_images(&mut self, cx: &mut Context<Self>) {
        self.filter_class_id = None;
        self.filter_scan_running = false;
        let state = app_state_mut(cx);
        state.filter_scan_generation = state.filter_scan_generation.wrapping_add(1);
        let dir = Path::new(&state.image_dir).to_path_buf();
        let mut names: Vec<String> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if classes::is_image_file(&name) {
                    names.push(name);
                }
            }
        }
        names.sort_by(|a, b| classes::natural_sort_key(a).cmp(&classes::natural_sort_key(b)));
        state.full_image_list = names;
        state.image_list = state.full_image_list.clone();
        state.unannotated_filter_active = false;
    }

    pub fn load_image(&mut self, index: usize, cx: &mut Context<Self>) {
        let len = app_state(cx).image_list.len();
        if len == 0 || index >= len {
            return;
        }
        {
            let state = app_state_mut(cx);
            if state.auto_save && state.current_image_index != -1 {
                state.save_annotations();
            }
            state.clear_preview();
            state.current_image_index = index as i64;
            let filename = state.image_list[index].clone();
            state.load_annotations(&filename);
            state.current_image_size = None;
        }
        self.canvas.update(cx, |canvas, cx| {
            canvas.current = None;
            let path = PathBuf::from(&app_state(cx).image_dir).join(&app_state(cx).image_list[index]);
            canvas.request_image(path, cx);
        });
        self.refresh_validation(cx);
        self.sync_filmstrip_current(cx);
        self.preload_neighbors(index, cx);
        cx.notify();
    }

    fn sync_filmstrip_current(&mut self, cx: &mut Context<Self>) {
        let (index, count) = {
            let s = app_state(cx);
            (s.current_image_index.max(0) as usize, s.image_list.len())
        };
        self.filmstrip.update(cx, |strip, cx| {
            strip.set_current(index, count);
            cx.notify();
        });
        if count > 0 {
            self.file_list_scroll.scroll_to_item(index, gpui::ScrollStrategy::Center);
        }
    }

    // ------------------------------------------------------------------
    // Manual save + unsaved-changes guard
    // ------------------------------------------------------------------

    /// Writes the current image's boxes now. Returns false (and tells the
    /// user why) when nothing could be saved.
    pub fn save_now(&mut self, cx: &mut Context<Self>) -> bool {
        let (index, has_output) = {
            let s = app_state(cx);
            (s.current_image_index, !s.output_dir.is_empty())
        };
        if index == -1 {
            return false;
        }
        if !has_output {
            self.alert("Warning", "Open an image folder before saving.");
            cx.notify();
            return false;
        }
        app_state_mut(cx).save_annotations();
        if app_state(cx).is_dirty() {
            let msg = app_state(cx)
                .class_counts_error
                .clone()
                .unwrap_or_else(|| "Could not save annotations.".to_string());
            self.alert("Save failed", msg);
            cx.notify();
            return false;
        }
        cx.notify();
        true
    }

    pub fn save_annotations_action(&mut self, _: &SaveAnnotations, _window: &mut Window, cx: &mut Context<Self>) {
        if self.modal.is_some() || self.task_running || app_state(cx).current_image_index == -1 {
            return;
        }
        if self.save_now(cx) {
            self.set_status("Saved", cx);
            self.toast("Saved", cx);
        }
    }

    /// Window-close hook. Saves pending edits when Auto Save is on;
    /// otherwise asks Save / Discard / Cancel and quits from the answer.
    pub fn should_close(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.task_running {
            return false;
        }
        let (dirty, auto) = {
            let s = app_state(cx);
            (s.is_dirty(), s.auto_save)
        };
        if dirty && !auto {
            self.modal = None;
            self.guard_unsaved(
                |_ws, cx| {
                    app_state_mut(cx).save_session();
                    cx.quit();
                },
                cx,
            );
            return false;
        }
        if dirty {
            app_state_mut(cx).save_annotations();
        }
        app_state_mut(cx).save_session();
        true
    }

    /// Runs `then` right away unless the current image has unsaved edits and
    /// Auto Save is off; in that case asks Save / Discard / Cancel first.
    /// (With Auto Save on, the follow-up action saves on its own.)
    pub fn guard_unsaved(
        &mut self,
        then: impl Fn(&mut Workspace, &mut Context<Workspace>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let (dirty, auto, name) = {
            let s = app_state(cx);
            let name = s
                .image_list
                .get(s.current_image_index.max(0) as usize)
                .cloned()
                .unwrap_or_default();
            (s.is_dirty(), s.auto_save, name)
        };
        if !dirty || auto || self.modal.is_some() {
            then(self, cx);
            return;
        }
        let then = std::rc::Rc::new(then);
        let then_no = std::rc::Rc::clone(&then);
        self.modal = Some(Modal::Confirm {
            title: "Unsaved changes".into(),
            message: format!(
                "\"{name}\" has unsaved box changes and Auto Save is off.\n\nSave them before continuing?"
            ),
            on_yes: Box::new(move |ws, _window, cx| {
                if ws.save_now(cx) {
                    then(ws, cx);
                }
            }),
            on_no: Some(Box::new(move |ws, _window, cx| {
                {
                    let s = app_state_mut(cx);
                    s.boxes = s.saved_boxes.clone();
                    s.selected_indices.clear();
                }
                then_no(ws, cx);
            })),
            labels: Some(("Discard".into(), "Save".into())),
        });
        cx.notify();
    }

    fn preload_neighbors(&mut self, index: usize, cx: &mut Context<Self>) {
        let (dir, list) = {
            let s = app_state(cx);
            (s.image_dir.clone(), s.image_list.clone())
        };
        for offset in [1i64, 2, 3, -1, -2] {
            let target = index as i64 + offset;
            if target >= 0 && (target as usize) < list.len() {
                crate::images::preload_full(cx, PathBuf::from(&dir).join(&list[target as usize]));
            }
        }
    }

    fn save_current_before_list_change(&mut self, cx: &mut Context<Self>) {
        let state = app_state_mut(cx);
        if state.auto_save && state.current_image_index != -1 {
            state.save_annotations();
        }
        state.current_image_index = -1;
        state.boxes.clear();
        state.selected_indices.clear();
        state.current_image_size = None;
        self.canvas.update(cx, |canvas, cx| {
            canvas.current = None;
            cx.notify();
        });
    }

    pub fn next_image_action(&mut self, _: &NextImage, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        let index = app_state(cx).current_image_index;
        let len = app_state(cx).image_list.len() as i64;
        if index + 1 < len {
            self.guard_unsaved(move |ws, cx| ws.load_image((index + 1) as usize, cx), cx);
        }
    }

    pub fn prev_image_action(&mut self, _: &PrevImage, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        let index = app_state(cx).current_image_index;
        if index - 1 >= 0 {
            self.guard_unsaved(move |ws, cx| ws.load_image((index - 1) as usize, cx), cx);
        }
    }

    pub fn cycle_class_action(&mut self, _: &CycleClass, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        let state = app_state_mut(cx);
        if state.classes.is_empty() {
            return;
        }
        let current = state.current_class_index;
        let next = if current == -1 { 0 } else { (current + 1) % state.classes.len() as i64 };
        state.current_class_index = next;
        cx.notify();
    }

    pub fn deselect_action(&mut self, _: &DeselectClass, _window: &mut Window, cx: &mut Context<Self>) {
        // Handled by the raw escape path too; the action binding may use a
        // non-escape key, so apply deselect semantics here as well.
        if self.task_running {
            return;
        }
        app_state_mut(cx).current_class_index = -1;
        app_state_mut(cx).clear_preview();
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Editing actions
    // ------------------------------------------------------------------

    pub fn delete_box_action(&mut self, _: &DeleteBox, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        let state = app_state_mut(cx);
        if state.selected_indices.is_empty() {
            return;
        }
        state.save_state();
        let selected: Vec<usize> = state.selected_indices.iter().copied().collect();
        for ix in selected.iter().rev() {
            if *ix < state.boxes.len() {
                state.boxes.remove(*ix);
            }
        }
        state.selected_indices.clear();
        cx.notify();
        self.refresh_validation(cx);
    }

    pub fn copy_boxes_impl(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        let state = app_state_mut(cx);
        let source: Vec<BBox> = if state.selected_indices.is_empty() {
            state.boxes.clone()
        } else {
            state
                .selected_indices
                .iter()
                .filter_map(|i| state.boxes.get(*i).copied())
                .collect()
        };
        state.clipboard = source.clone();
        let preview_was_clipboard = state.preview_source == Some(PreviewSource::Clipboard);
        if preview_was_clipboard && state.preview_visible {
            let clip = state.clipboard.clone();
            state.set_preview(&clip, PreviewSource::Clipboard, None);
        }
        let n = source.len();
        self.toast(format!("Copied {n} boxes."), cx);
    }

    pub fn paste_boxes_impl(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        let n = app_state(cx).clipboard.len();
        if n == 0 {
            return;
        }
        let state = app_state_mut(cx);
        state.save_state();
        let copies = state.clipboard.clone();
        state.boxes.extend(copies);
        state.clear_preview();
        self.set_status(format!("Pasted {n} box(es)"), cx);
        cx.notify();
        self.refresh_validation(cx);
    }

    pub fn duplicate_boxes_impl(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        let state = app_state_mut(cx);
        let source: Vec<usize> = if state.selected_indices.is_empty() {
            (0..state.boxes.len()).collect()
        } else {
            state.selected_indices.iter().copied().collect()
        };
        if source.is_empty() {
            return;
        }
        state.save_state();
        let mut new_indices = Vec::new();
        for ix in &source {
            if let Some(b) = state.boxes.get(*ix) {
                let mut copy = *b;
                copy.x_center = (b.x_center + 0.02).min(1.0 - b.w / 2.0);
                copy.y_center = (b.y_center + 0.02).min(1.0 - b.h / 2.0);
                state.boxes.push(copy);
                new_indices.push(state.boxes.len() - 1);
            }
        }
        state.selected_indices.clear();
        state.selected_indices.extend(new_indices);
        let n = source.len();
        self.set_status(format!("Duplicated {n} box(es)"), cx);
        cx.notify();
        self.refresh_validation(cx);
    }

    pub fn edit_box_class(&mut self, _: &EditBoxClass, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        if app_state(cx).selected_indices.is_empty() {
            return;
        }
        self.modal = Some(Modal::EditClass);
        cx.notify();
    }

    pub fn apply_edit_class(&mut self, class_id: i64, cx: &mut Context<Self>) {
        self.modal = None;
        let state = app_state_mut(cx);
        state.save_state();
        let indices: Vec<usize> = state.selected_indices.iter().copied().collect();
        for ix in indices {
            if let Some(b) = state.boxes.get_mut(ix) {
                b.class_id = class_id;
            }
        }
        cx.notify();
        self.refresh_validation(cx);
    }

    pub fn select_all_boxes(&mut self, _: &SelectAllBoxes, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        let state = app_state_mut(cx);
        state.selected_indices = (0..state.boxes.len()).collect();
        cx.notify();
    }

    pub fn undo_impl(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        if app_state_mut(cx).undo() {
            self.set_status("Action undone", cx);
            self.refresh_validation(cx);
            cx.notify();
        }
    }

    pub fn redo_impl(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        if app_state_mut(cx).redo() {
            self.set_status("Action redone", cx);
            self.refresh_validation(cx);
            cx.notify();
        }
    }

    // ------------------------------------------------------------------
    // Zoom / view actions
    // ------------------------------------------------------------------

    pub fn zoom_in_action(&mut self, _: &ZoomIn, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        self.canvas.update(cx, |canvas, cx| canvas.zoom_center(1.1, cx));
    }

    pub fn zoom_out_action(&mut self, _: &ZoomOut, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        self.canvas.update(cx, |canvas, cx| canvas.zoom_center(1.0 / 1.1, cx));
    }

    pub fn zoom_reset_action(&mut self, _: &ZoomReset, window: &mut Window, cx: &mut Context<Self>) {
        if self.blocked(window, cx) {
            return;
        }
        self.canvas.update(cx, |canvas, cx| canvas.fit(cx));
    }

    pub fn toggle_help_overlay(&mut self, _: &ToggleHelpOverlay, _window: &mut Window, cx: &mut Context<Self>) {
        self.help_visible = !self.help_visible;
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Filters + class search + counts
    // ------------------------------------------------------------------

    pub fn toggle_unannotated_action(
        &mut self,
        _: &ToggleUnannotatedFilter,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.blocked(window, cx) {
            return;
        }
        self.toggle_unannotated_filter(cx);
    }

    pub fn toggle_unannotated_filter(&mut self, cx: &mut Context<Self>) {
        if self.task_running || self.filter_scan_running {
            return;
        }
        if app_state(cx).unannotated_filter_active {
            self.clear_image_filter(cx);
            return;
        }
        self.guard_unsaved(|ws, cx| ws.start_image_filter_scan(None, cx), cx);
    }

    pub fn apply_image_filter(&mut self, cx: &mut Context<Self>) {
        if self.task_running || self.filter_scan_running {
            return;
        }
        if let Some(class_id) = self.filter_class_id {
            self.guard_unsaved(move |ws, cx| ws.start_image_filter_scan(Some(class_id), cx), cx);
        }
    }

    fn start_image_filter_scan(&mut self, class_id: Option<i64>, cx: &mut Context<Self>) {
        if app_state(cx).output_dir.is_empty() {
            self.alert("Warning", "Open an image folder first.");
            cx.notify();
            return;
        }
        // Flush the current label before the worker reads it.
        self.save_current_before_list_change(cx);
        let (image_dir, output_dir, images, generation) = {
            let state = app_state_mut(cx);
            state.filter_scan_generation = state.filter_scan_generation.wrapping_add(1);
            (state.image_dir.clone(), state.output_dir.clone(), state.full_image_list.clone(), state.filter_scan_generation)
        };
        self.filter_scan_running = true;
        cx.notify();
        let ws = cx.entity();
        let scan_output = output_dir.clone();
        let scan_images = images.clone();
        cx.background_spawn(async move {
            crate::state::model::scan_class_counts(&scan_output, &scan_images).map(|counts| {
                scan_images.into_iter().filter(|name| {
                    let mut label = Path::new(name).file_stem().unwrap_or_default().to_os_string();
                    label.push(".txt");
                    let per_label = counts.get(&Path::new(&scan_output).join(label));
                    match class_id {
                        Some(id) => per_label.is_some_and(|counts| counts.contains_key(&id)),
                        None => per_label.is_none_or(|counts| counts.is_empty()),
                    }
                }).collect::<Vec<_>>()
            })
        })
        .detach_and_attach_cb(cx, move |result, cx| {
            ws.update(cx, |ws, cx| {
                let state = app_state(cx);
                if state.filter_scan_generation != generation || state.image_dir != image_dir
                    || state.output_dir != output_dir || state.full_image_list != images {
                    return;
                }
                ws.filter_scan_running = false;
                match result {
                    Ok(matched) => {
                        ws.save_current_before_list_change(cx);
                        let state = app_state_mut(cx);
                        state.image_list = matched;
                        state.unannotated_filter_active = class_id.is_none();
                        ws.filter_class_id = class_id;
                    }
                    Err(error) => ws.alert("Filter error", error),
                }
                if !app_state(cx).image_list.is_empty() {
                    ws.load_image(0, cx);
                }
                ws.sync_filmstrip_current(cx);
                cx.notify();
            });
        });
    }

    pub fn clear_image_filter(&mut self, cx: &mut Context<Self>) {
        if self.task_running {
            return;
        }
        self.guard_unsaved(|ws, cx| ws.clear_image_filter_now(cx), cx);
    }

    fn clear_image_filter_now(&mut self, cx: &mut Context<Self>) {
        self.save_current_before_list_change(cx);
        {
            let state = app_state_mut(cx);
            state.filter_scan_generation = state.filter_scan_generation.wrapping_add(1);
            state.image_list = state.full_image_list.clone();
            state.unannotated_filter_active = false;
        }
        self.filter_scan_running = false;
        self.filter_class_id = None;
        if !app_state(cx).image_list.is_empty() {
            self.load_image(0, cx);
        }
        self.sync_filmstrip_current(cx);
        cx.notify();
    }

    pub fn apply_class_search(&mut self, cx: &mut Context<Self>) {
        let query = self.class_search.read(cx).text().to_lowercase();
        let state = app_state_mut(cx);
        state.filtered_classes = state
            .classes
            .iter()
            .enumerate()
            .filter(|(_, c)| c.name.to_lowercase().contains(&query))
            .map(|(ix, _)| ix)
            .collect();
        cx.notify();
    }

    pub fn refresh_class_counts(&mut self, cx: &mut Context<Self>) {
        let (image_dir, output_dir, images, generation) = {
            let state = app_state_mut(cx);
            let generation = state.begin_class_counts_scan();
            (state.image_dir.clone(), state.output_dir.clone(), state.full_image_list.clone(), generation)
        };
        cx.notify();
        if !app_state(cx).class_counts_loading {
            return;
        }
        let ws = cx.entity();
        let scan_output = output_dir.clone();
        let scan_images = images.clone();
        cx.background_spawn(async move {
            crate::state::model::scan_class_counts(&scan_output, &scan_images)
        })
        .detach_and_attach_cb(cx, move |counts, cx| {
            ws.update(cx, |_ws, cx| {
                let state = app_state_mut(cx);
                if state.image_dir != image_dir || state.output_dir != output_dir || state.full_image_list != images {
                    return;
                }
                state.finish_class_counts_scan(generation, counts);
                cx.notify();
            });
        });
    }

    // ------------------------------------------------------------------
    // Clipboard / box presets
    // ------------------------------------------------------------------

    pub fn toggle_clipboard_preview(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let state = app_state(cx);
        if state.current_image_index == -1 {
            self.set_status("Load an image to preview the clipboard", cx);
            return;
        }
        if state.preview_visible && state.preview_source == Some(PreviewSource::Clipboard) {
            app_state_mut(cx).clear_preview();
            self.set_status("Clipboard preview hidden", cx);
            cx.notify();
            return;
        }
        let clip = state.clipboard.clone();
        let n = clip.len();
        app_state_mut(cx).set_preview(&clip, PreviewSource::Clipboard, None);
        self.set_status(format!("Previewing clipboard: {n} box(es)"), cx);
        cx.notify();
    }

    pub fn preview_preset(&mut self, slot: u8, toggle: bool, cx: &mut Context<Self>) {
        let state = app_state(cx);
        if state.current_image_index == -1 {
            self.set_status("Load an image to preview a preset", cx);
            return;
        }
        if toggle && state.preview_visible && state.preview_slot == Some(slot) {
            app_state_mut(cx).clear_preview();
            self.set_status(format!("Preset {slot} preview hidden"), cx);
            cx.notify();
            return;
        }
        let Some(boxes) = state.presets.get(&slot).cloned() else {
            self.set_status(format!("Preset {slot} is empty"), cx);
            return;
        };
        let n = boxes.len();
        app_state_mut(cx).set_preview(&boxes, PreviewSource::Preset, Some(slot));
        self.set_status(format!("Previewing Preset {slot}: {n} box(es)"), cx);
        cx.notify();
    }

    pub fn apply_preset(&mut self, slot: u8, cx: &mut Context<Self>) {
        let Some(boxes) = app_state(cx).presets.get(&slot).cloned() else {
            self.set_status(
                format!("Preset {slot} is empty. Use Ctrl+Shift+{slot} to save current boxes."),
                cx,
            );
            return;
        };
        let n = boxes.len();
        {
            let state = app_state_mut(cx);
            state.save_state();
            state.boxes.extend(boxes);
            state.clear_preview();
        }
        self.set_status(format!("Applied Preset {slot} ({n} boxes)"), cx);
        self.refresh_validation(cx);
        cx.notify();
    }

    pub fn save_preset(&mut self, slot: u8, cx: &mut Context<Self>) {
        let (selected, all_empty) = {
            let state = app_state(cx);
            (
                state
                    .selected_indices
                    .iter()
                    .filter_map(|i| state.boxes.get(*i).copied())
                    .collect::<Vec<_>>(),
                state.boxes.is_empty(),
            )
        };
        if all_empty {
            self.set_status(format!("Preset {slot} remains empty (no boxes to save)"), cx);
            return;
        }
        let (boxes, source) = if !selected.is_empty() {
            (selected, "selected")
        } else {
            (app_state(cx).boxes.clone(), "all")
        };
        let n = boxes.len();
        {
            let state = app_state_mut(cx);
            state.presets.insert(slot, boxes.clone());
            if state.preview_visible && state.preview_slot == Some(slot) {
                state.set_preview(&boxes, PreviewSource::Preset, Some(slot));
            }
            state.save_session();
        }
        self.set_status(format!("Stored {n} {source} boxes in Preset {slot}"), cx);
        cx.notify();
    }

    pub fn clear_all_presets(&mut self, cx: &mut Context<Self>) {
        self.confirm(
            "Confirm",
            "Clear all stored box presets for the current project and class set?",
            Box::new(move |ws, _window, cx| {
                let state = app_state_mut(cx);
                state.presets.clear();
                state.clear_preview();
                state.save_session();
                ws.set_status("All box presets cleared for this project/class set", cx);
                cx.notify();
            }),
        );
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Validation
    // ------------------------------------------------------------------

    pub fn show_validation(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.refresh_validation(cx);
        if self.validation_issues.is_empty() {
            self.alert("Validation", "✓ All annotations are valid!");
        } else {
            self.validation_visible = true;
        }
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Settings dialog
    // ------------------------------------------------------------------

    pub fn open_settings(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if self.task_running {
            return;
        }
        let ws_handle = cx.entity();
        let settings = cx.new(|cx| SettingsView::new(ws_handle, cx));
        self.modal = Some(Modal::Settings(settings));
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Game presets
    // ------------------------------------------------------------------

    pub fn load_game_preset(&mut self, preset_file: &str, cx: &mut Context<Self>) {
        let data_dir = paths::data_dir();
        let source = data_dir.join(preset_file);
        let target = paths::classes_file();
        self.confirm(
            "Confirm",
            format!("Overwrite current classes with {preset_file}?"),
            Box::new(move |ws, _window, cx| {
                if std::fs::copy(&source, &target).is_ok() {
                    {
                        let state = app_state_mut(cx);
                        state.classes = classes::load_classes(&paths::classes_file());
                        state.filtered_classes = (0..state.classes.len()).collect();
                        if state.current_class_index >= state.classes.len() as i64 {
                            state.current_class_index = if state.classes.is_empty() {
                                -1
                            } else {
                                state.classes.len() as i64 - 1
                            };
                        }
                        state.refresh_preset_scope();
                    }
                    let index = app_state(cx).current_image_index;
                    if index != -1 {
                        self_load_image(ws, index as usize, cx);
                    }
                    ws.alert("Success", "Preset loaded!");
                }
                cx.notify();
            }),
        );
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Class editor apply (with remap + backup)
    // ------------------------------------------------------------------

    pub fn apply_class_changes(&mut self, new_classes: Vec<String>, cx: &mut Context<Self>) {
        let old_classes: Vec<String> = app_state(cx).classes.iter().map(|c| c.name.clone()).collect();
        if new_classes.is_empty() || old_classes == new_classes {
            return;
        }
        let removed: Vec<String> = old_classes
            .iter()
            .filter(|n| !new_classes.contains(n))
            .cloned()
            .collect();
        let mut confirm_msg =
            "Apply class changes and update all annotation files?\n\n".to_string();
        if !removed.is_empty() {
            confirm_msg += &format!(
                "Removed classes (their annotations will be DELETED):\n{}\n\n",
                removed.join(", ")
            );
        }
        confirm_msg += "A backup of all annotation files will be created first.";

        let new_classes = std::rc::Rc::new(new_classes);
        let old_classes = std::rc::Rc::new(old_classes);
        self.confirm(
            "Confirm",
            confirm_msg,
            Box::new(move |ws, _window, cx| {
                let output_dir = app_state(cx).output_dir.clone();
                if output_dir.is_empty() {
                    ws.finish_class_changes(new_classes.to_vec(), None, 0, cx);
                    return;
                }
                let mapping = classes::create_class_mapping(&old_classes, &new_classes);
                ws.run_class_remap_task(new_classes.to_vec(), mapping, cx);
            }),
        );
        cx.notify();
    }

    fn run_class_remap_task(
        &mut self,
        new_classes: Vec<String>,
        mapping: std::collections::HashMap<i64, Option<i64>>,
        cx: &mut Context<Self>,
    ) {
        if self.task_running {
            return;
        }
        self.task_running = true;
        self.modal = Some(Modal::Progress {
            title: "Updating Classes".into(),
            label: "Starting…".into(),
            current: 0,
            total: 0,
        });
        cx.notify();

        let output_dir = PathBuf::from(&app_state(cx).output_dir);
        let (tx, rx) = std_mpsc::channel::<TaskMsg>();
        let progress_tx = tx.clone();
        std::thread::spawn(move || {
            let mut txt_files: Vec<PathBuf> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(&output_dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("txt")) {
                        txt_files.push(p);
                    }
                }
            }
            if txt_files.is_empty() {
                let _ = tx.send(TaskMsg::Done(Ok(TaskValue::ClassRemap {
                    backup_dir: None,
                    updated: 0,
                })));
                return;
            }
            let backup_tx = progress_tx.clone();
            let backup = yolo::backup_annotations(&output_dir, move |c, t, _f| {
                let _ = backup_tx.send(TaskMsg::Progress(c, t, format!("Backing up... {c}/{t}")));
            });
            let Some(backup) = backup else {
                let _ = tx.send(TaskMsg::Done(Err(
                    "Backup failed — class update aborted. No files were changed.".to_string(),
                )));
                return;
            };
            let mut updated = 0;
            for (i, path) in txt_files.iter().enumerate() {
                if yolo::update_annotation_file(path, &mapping, true) {
                    updated += 1;
                }
                let _ = progress_tx.send(TaskMsg::Progress(
                    i + 1,
                    txt_files.len(),
                    format!("Updating... {}/{}", i + 1, txt_files.len()),
                ));
            }
            let _ = tx.send(TaskMsg::Done(Ok(TaskValue::ClassRemap {
                backup_dir: backup.file_name().map(|n| n.to_string_lossy().to_string()),
                updated,
            })));
        });

        let new_classes_for_done = new_classes.clone();
        self.pump_task_messages(
            rx,
            cx,
            Box::new(move |ws, result, cx| match result {
                Ok(TaskValue::ClassRemap { backup_dir, updated }) => {
                    ws.finish_class_changes(new_classes_for_done, backup_dir, updated, cx);
                }
                Err(msg) => {
                    ws.alert("Error", msg);
                    ws.task_running = false;
                    cx.notify();
                }
                _ => {}
            }),
        );
    }

    fn finish_class_changes(
        &mut self,
        new_classes: Vec<String>,
        backup_dir: Option<String>,
        updated: usize,
        cx: &mut Context<Self>,
    ) {
        if let Some(Modal::Progress { .. }) = self.modal {
            self.modal = None;
        }
        self.task_running = false;
        {
            let state = app_state_mut(cx);
            classes::save_classes(&paths::classes_file(), &new_classes).ok();
            state.classes = classes::load_classes(&paths::classes_file());
            state.filtered_classes = (0..state.classes.len()).collect();
            if state.current_class_index >= state.classes.len() as i64 {
                state.current_class_index = if state.classes.is_empty() {
                    -1
                } else {
                    state.classes.len() as i64 - 1
                };
            }
            state.refresh_preset_scope();
        }
        // Suppress the autosave that load_image would otherwise perform
        // (in-memory boxes still carry pre-remap ids) — Python parity.
        let index = app_state(cx).current_image_index;
        if index != -1 {
            app_state_mut(cx).current_image_index = -1;
            self.load_image(index as usize, cx);
        }
        self.refresh_class_counts(cx);
        let mut msg = format!("Classes updated! {updated} annotation file(s) rewritten.");
        if let Some(backup) = &backup_dir {
            msg += &format!("\nBackup: {backup}");
        }
        self.alert("Success", msg);
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Batch replace
    // ------------------------------------------------------------------

    pub fn execute_batch_replace(&mut self, old_id: i64, new_id: i64, cx: &mut Context<Self>) {
        if old_id == new_id {
            self.alert("Warning", "Old and new class IDs are the same.");
            cx.notify();
            return;
        }
        let output_dir = app_state(cx).output_dir.clone();
        if output_dir.is_empty() {
            self.alert("Error", "Open an image folder first.");
            cx.notify();
            return;
        }
        let mut txt_files: Vec<PathBuf> = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&output_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("txt")) {
                    txt_files.push(p);
                }
            }
        }
        if txt_files.is_empty() {
            self.alert("Info", "No annotation files found in the image folder.");
            cx.notify();
            return;
        }
        let old_name = app_state(cx)
            .classes
            .iter()
            .find(|c| c.id == old_id)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let new_name = app_state(cx)
            .classes
            .iter()
            .find(|c| c.id == new_id)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        let file_count = txt_files.len();
        let txt_files = std::rc::Rc::new(txt_files);
        let dir_display = output_dir.clone();
        self.confirm(
            "Confirm Batch Replace",
            format!(
                "Replace all ID {old_id} ({old_name}) with {new_id} ({new_name})?\n\nDirectory: {dir_display}\nFiles to process: {file_count}\nA backup will be created before making changes."
            ),
            Box::new(move |ws, _window, cx| {
                if ws.task_running {
                    return;
                }
                let txt_files = txt_files.to_vec();
                ws.task_running = true;
                ws.modal = Some(Modal::Progress {
                    title: "Batch Replacing".into(),
                    label: "Starting…".into(),
                    current: 0,
                    total: 0,
                });
                cx.notify();

                let (tx, rx) = std_mpsc::channel::<TaskMsg>();
                let progress_tx = tx.clone();
                let mut mapping = std::collections::HashMap::new();
                mapping.insert(old_id, Some(new_id));
                let output_dir = PathBuf::from(&dir_display);
                std::thread::spawn(move || {
                    let backup_tx = progress_tx.clone();
                    let backup = yolo::backup_annotations(&output_dir, move |c, t, _f| {
                        let _ = backup_tx.send(TaskMsg::Progress(c, t, format!("Backing up... {c}/{t}")));
                    });
                    let Some(backup) = backup else {
                        let _ = tx.send(TaskMsg::Done(Err(
                            "Backup failed — batch replace aborted. No files were changed.".to_string(),
                        )));
                        return;
                    };
                    let mut updated = 0;
                    for (i, path) in txt_files.iter().enumerate() {
                        if yolo::update_annotation_file(path, &mapping, false) {
                            updated += 1;
                        }
                        let _ = progress_tx.send(TaskMsg::Progress(
                            i + 1,
                            txt_files.len(),
                            format!("Updating... {}/{}", i + 1, txt_files.len()),
                        ));
                    }
                    let _ = tx.send(TaskMsg::Done(Ok(TaskValue::BatchReplace {
                        backup_dir: backup.file_name().map(|n| n.to_string_lossy().to_string()),
                        updated,
                    })));
                });

                ws.pump_task_messages(rx, cx, Box::new(move |ws, result, cx| {
                    match result {
                        Ok(TaskValue::BatchReplace { backup_dir, updated }) => {
                            if let Some(Modal::Progress { .. }) = ws.modal {
                                ws.modal = None;
                            }
                            ws.task_running = false;
                            // In-memory remap so autosave can't clobber the change.
                            let state = app_state_mut(cx);
                            let old_id_v = old_id;
                            let new_id_v = new_id;
                            for b in &mut state.boxes {
                                if b.class_id == old_id_v {
                                    b.class_id = new_id_v;
                                }
                            }
                            ws.refresh_validation(cx);
                            ws.refresh_class_counts(cx);
                            let backup = backup_dir.unwrap_or_default();
                            ws.alert(
                                "Success",
                                format!("Batch replace complete! Updated {updated} files.\nBackup: {backup}"),
                            );
                            cx.notify();
                        }
                        Err(msg) => {
                            ws.alert("Error", msg);
                            ws.task_running = false;
                            cx.notify();
                        }
                        _ => {}
                    }
                }));
            }),
        );
        cx.notify();
    }

    // ------------------------------------------------------------------
    // Progress task pump (std mpsc → UI)
    // ------------------------------------------------------------------

    fn pump_task_messages(
        &mut self,
        rx: std_mpsc::Receiver<TaskMsg>,
        cx: &mut Context<Self>,
        on_done: Box<dyn FnOnce(&mut Workspace, Result<TaskValue, String>, &mut Context<Self>)>,
    ) {
        let ws = cx.entity();
        cx.spawn(async move |_this: gpui::WeakEntity<Workspace>, cx: &mut gpui::AsyncApp| {
            let mut on_done = Some(on_done);
            loop {
                match rx.try_recv() {
                    Ok(TaskMsg::Progress(current, total, text)) => {
                        let _ = cx.update(|cx| {
                            ws.update(cx, |ws, cx| {
                                if let Some(Modal::Progress { label, current: c, total: t, .. }) =
                                    &mut ws.modal
                                {
                                    *label = text.into();
                                    *c = current;
                                    *t = total;
                                }
                                cx.notify();
                            });
                        });
                    }
                    Ok(TaskMsg::Done(result)) => {
                        let _ = cx.update(|cx| {
                            ws.update(cx, |ws, cx| {
                                if let Some(action) = on_done.take() {
                                    action(ws, result, cx);
                                }
                                cx.notify();
                            });
                        });
                        return;
                    }
                    Err(std_mpsc::TryRecvError::Empty) => {
                        smol::Timer::after(std::time::Duration::from_millis(50)).await;
                    }
                    Err(std_mpsc::TryRecvError::Disconnected) => {
                        let _ = cx.update(|cx| {
                            ws.update(cx, |ws, cx| {
                                ws.task_running = false;
                                if let Some(Modal::Progress { .. }) = ws.modal {
                                    ws.modal = None;
                                }
                                cx.notify();
                            });
                        });
                        return;
                    }
                }
            }
        })
        .detach();
    }
}

// Helper so the game-preset confirm closure can call load_image on `ws`.
fn self_load_image(ws: &mut Workspace, index: usize, cx: &mut Context<Workspace>) {
    ws.load_image(index, cx);
}

// ---------------------------------------------------------------------------
// Small extension: attach a completion callback to a background task result.
// ---------------------------------------------------------------------------

trait DetachAndAttach<T>: Sized {
    fn detach_and_attach_cb(
        self,
        cx: &mut gpui::App,
        on_done: impl FnOnce(T, &mut gpui::App) + 'static,
    );
}

impl<T: Send + 'static> DetachAndAttach<T> for gpui::Task<T> {
    fn detach_and_attach_cb(
        self,
        cx: &mut gpui::App,
        on_done: impl FnOnce(T, &mut gpui::App) + 'static,
    ) {
        cx.spawn(async move |cx: &mut gpui::AsyncApp| {
            let value = self.await;
            let _ = cx.update(|cx| on_done(value, cx));
        })
        .detach();
    }
}
