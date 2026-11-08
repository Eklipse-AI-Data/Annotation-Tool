//! Core application state — a port of `AnnotationModel` from `src/model.py`,
//! including the undo/redo snapshot semantics and per-scope box presets that
//! are persisted inside session.json (shared with the Python tool).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::config::Config;
use super::paths;
use super::session::{self, RecentProject};
use crate::classes::{self, Class};
use crate::yolo::{self, BBox};

pub const MAX_HISTORY: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewSource {
    Clipboard,
    Preset,
}

#[derive(Debug)]
pub struct AppState {
    // Persisted-loaded state
    pub session: Map<String, Value>,
    pub config: Config,
    pub classes: Vec<Class>,

    // Project
    pub image_dir: String,
    pub output_dir: String,
    /// Currently visible (possibly filtered) image filenames, natural-sorted.
    pub image_list: Vec<String>,
    /// Unfiltered image list.
    pub full_image_list: Vec<String>,
    pub current_image_index: i64,
    /// Decoded size of the current image (w, h).
    pub current_image_size: Option<(u32, u32)>,

    // UI/view state
    pub zoom_factor: f64,
    pub auto_save: bool,
    pub show_labels: bool,
    pub show_right_sidebar: bool,
    pub show_left_sidebar: bool,
    pub show_filmstrip: bool,
    pub unannotated_filter_active: bool,
    pub current_theme_name: String,

    // Annotation state
    pub boxes: Vec<BBox>,
    pub selected_indices: BTreeSet<usize>,
    pub clipboard: Vec<BBox>,
    pub preview_boxes: Vec<BBox>,
    pub preview_visible: bool,
    pub preview_source: Option<PreviewSource>,
    pub preview_slot: Option<u8>,
    pub current_class_index: i64,

    // Filter state
    /// Indices into `classes` matching the current search text.
    pub filtered_classes: Vec<usize>,
    pub class_counts: HashMap<i64, usize>,

    // History
    pub history: Vec<Vec<BBox>>,
    pub redo_stack: Vec<Vec<BBox>>,

    // Box presets (slots 1-9) for the current scope
    pub presets: BTreeMap<u8, Vec<BBox>>,

    // Recent projects (cached view of session.json)
    pub recent_projects: Vec<RecentProject>,
}

impl AppState {
    pub fn new() -> Self {
        let session = session::load_session(&paths::session_file());
        let config = Config::load();
        let classes = classes::load_classes(&paths::classes_file());

        let image_dir = session
            .get("last_image_dir")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let output_dir = session
            .get("last_output_dir")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();

        let mut state = Self {
            current_theme_name: session
                .get("theme_name")
                .and_then(Value::as_str)
                .unwrap_or("Midnight Glass")
                .to_string(),
            zoom_factor: session.get("zoom_factor").and_then(Value::as_f64).unwrap_or(1.0),
            auto_save: session.get("auto_save").and_then(Value::as_bool).unwrap_or(true),
            show_labels: session.get("show_labels").and_then(Value::as_bool).unwrap_or(true),
            show_right_sidebar: session
                .get("show_right_sidebar")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            show_left_sidebar: session
                .get("show_left_sidebar")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            show_filmstrip: session.get("show_filmstrip").and_then(Value::as_bool).unwrap_or(true),
            image_dir,
            output_dir,
            config,
            classes,
            session,
            image_list: Vec::new(),
            full_image_list: Vec::new(),
            current_image_index: -1,
            current_image_size: None,
            unannotated_filter_active: false,
            boxes: Vec::new(),
            selected_indices: BTreeSet::new(),
            clipboard: Vec::new(),
            preview_boxes: Vec::new(),
            preview_visible: false,
            preview_source: None,
            preview_slot: None,
            current_class_index: -1,
            filtered_classes: Vec::new(),
            class_counts: HashMap::new(),
            history: Vec::new(),
            redo_stack: Vec::new(),
            presets: BTreeMap::new(),
            recent_projects: Vec::new(),
        };
        state.filtered_classes = (0..state.classes.len()).collect();
        state.recent_projects = session::parse_recent(&state.session);
        state.load_scoped_presets();
        state
    }

    // ------------------------------------------------------------------
    // Undo / redo — snapshot semantics identical to model.py
    // ------------------------------------------------------------------

    /// Push the current boxes onto the history stack (deduped against the
    /// top), trimming to 50 and clearing the redo stack.
    pub fn save_state(&mut self) {
        if self.history.last() != Some(&self.boxes) {
            self.history.push(self.boxes.clone());
            if self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }
            self.redo_stack.clear();
        }
    }

    pub fn undo(&mut self) -> bool {
        if self.history.is_empty() {
            return false;
        }
        self.redo_stack.push(self.boxes.clone());
        self.boxes = self.history.pop().unwrap();
        self.selected_indices.clear();
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.redo_stack.is_empty() {
            return false;
        }
        self.history.push(self.boxes.clone());
        self.boxes = self.redo_stack.pop().unwrap();
        self.selected_indices.clear();
        true
    }

    // ------------------------------------------------------------------
    // Preview overlay
    // ------------------------------------------------------------------

    pub fn set_preview(&mut self, boxes: &[BBox], source: PreviewSource, slot: Option<u8>) {
        self.preview_boxes = boxes.to_vec();
        self.preview_visible = !self.preview_boxes.is_empty();
        self.preview_source = if self.preview_visible { Some(source) } else { None };
        self.preview_slot = if self.preview_visible { slot } else { None };
    }

    pub fn clear_preview(&mut self) {
        self.preview_boxes.clear();
        self.preview_visible = false;
        self.preview_source = None;
        self.preview_slot = None;
    }

    // ------------------------------------------------------------------
    // Annotation file I/O
    // ------------------------------------------------------------------

    /// Path of the annotation file for `filename` inside the output dir.
    pub fn annotation_path_for(&self, filename: &str) -> Option<PathBuf> {
        if self.output_dir.is_empty() {
            return None;
        }
        let stem = Path::new(filename).file_stem()?.to_string_lossy().to_string();
        Some(Path::new(&self.output_dir).join(format!("{stem}.txt")))
    }

    /// Replaces `boxes` with the parsed annotations for `filename`.
    pub fn load_annotations(&mut self, filename: &str) {
        self.boxes.clear();
        self.selected_indices.clear();
        let Some(txt) = self.annotation_path_for(filename) else {
            return;
        };
        if txt.exists() {
            self.boxes = yolo::parse_yolo(&txt);
        }
    }

    /// Saves the current image's annotations with Python's exact save gating:
    /// only writes when there are valid boxes or the file already exists;
    /// unknown class ids and `-1` (unlabeled) boxes are never written.
    pub fn save_annotations(&mut self) {
        if self.current_image_index == -1 || self.output_dir.is_empty() || self.image_list.is_empty() {
            return;
        }
        let Some(filename) = self.image_list.get(self.current_image_index.max(0) as usize) else {
            return;
        };
        let Some(txt) = self.annotation_path_for(filename) else {
            return;
        };

        let valid_class_ids: HashSet<i64> = self.classes.iter().map(|c| c.id).collect();
        let valid_boxes: Vec<&BBox> = self
            .boxes
            .iter()
            .filter(|b| valid_class_ids.contains(&b.class_id) || b.class_id == -1)
            .collect();

        if !valid_boxes.is_empty() || txt.exists() {
            let final_boxes: Vec<BBox> =
                valid_boxes.into_iter().filter(|b| b.class_id != -1).copied().collect();
            if let Err(err) = yolo::save_yolo(&txt, &final_boxes) {
                eprintln!("Error saving YOLO file {}: {err}", txt.display());
            }
        }
    }

    /// (annotated, total) across the full image list.
    pub fn get_annotated_count(&self) -> (usize, usize) {
        if self.full_image_list.is_empty() {
            return (0, 0);
        }
        let annotated = session::get_annotated_images(
            &self.image_dir,
            &self.output_dir,
            &self.full_image_list,
        );
        (annotated.len(), self.full_image_list.len())
    }

    // ------------------------------------------------------------------
    // Session persistence
    // ------------------------------------------------------------------

    pub fn save_session(&mut self) {
        self.store_presets_for_current_scope();
        let mut state = Map::new();
        state.insert("last_image_dir".into(), Value::String(self.image_dir.clone()));
        state.insert("last_output_dir".into(), Value::String(self.output_dir.clone()));
        state.insert("last_image_index".into(), Value::from(self.current_image_index));
        state.insert("zoom_factor".into(), Value::from(self.zoom_factor));
        state.insert("show_labels".into(), Value::Bool(self.show_labels));
        state.insert("auto_save".into(), Value::Bool(self.auto_save));
        state.insert("show_right_sidebar".into(), Value::Bool(self.show_right_sidebar));
        state.insert("show_left_sidebar".into(), Value::Bool(self.show_left_sidebar));
        state.insert("show_filmstrip".into(), Value::Bool(self.show_filmstrip));
        state.insert("theme_name".into(), Value::String(self.current_theme_name.clone()));
        if let Some(Value::Object(presets)) = self.session.get("box_presets_by_scope") {
            state.insert(
                "box_presets_by_scope".into(),
                Value::Object(presets.clone()),
            );
        }
        session::save_session(&paths::session_file(), &state);
        // Refresh our in-memory copy so subsequent loads see the merged result.
        self.session = session::load_session(&paths::session_file());
    }

    // ------------------------------------------------------------------
    // Scoped box presets — scope key must stay byte-compatible with Python
    // (`abspath(imgdir)::abspath(outdir)::names||joined` with sentinels).
    // ------------------------------------------------------------------

    pub fn get_preset_scope_key(&self) -> String {
        let image_dir = if self.image_dir.is_empty() {
            "__no_image_dir__".to_string()
        } else {
            abspath(&self.image_dir)
        };
        let output_dir = if self.output_dir.is_empty() {
            "__no_output_dir__".to_string()
        } else {
            abspath(&self.output_dir)
        };
        let class_signature = if self.classes.is_empty() {
            "__no_classes__".to_string()
        } else {
            self.classes
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
                .join("||")
        };
        format!("{image_dir}::{output_dir}::{class_signature}")
    }

    pub fn load_scoped_presets(&mut self) {
        let scope_key = self.get_preset_scope_key();
        let empty = Map::new();
        let preset_store = self
            .session
            .get("box_presets_by_scope")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let mut presets = BTreeMap::new();
        if let Some(Value::Object(scoped)) = preset_store.get(&scope_key) {
            for (slot, boxes_value) in scoped {
                let Ok(slot_num) = slot.parse::<u8>() else {
                    continue;
                };
                if let Value::Array(arr) = boxes_value {
                    let mut boxes = Vec::with_capacity(arr.len());
                    for b in arr {
                        if let Some(bbox) = bbox_from_json(b) {
                            boxes.push(bbox);
                        }
                    }
                    presets.insert(slot_num, boxes);
                }
            }
        }
        self.presets = presets;
    }

    pub fn store_presets_for_current_scope(&mut self) {
        let scope_key = self.get_preset_scope_key();
        let store = self
            .session
            .entry("box_presets_by_scope".to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        let Some(store_map) = store.as_object_mut() else {
            return;
        };

        if !self.presets.is_empty() {
            let mut scoped = Map::new();
            for (slot, boxes) in &self.presets {
                if boxes.is_empty() {
                    continue;
                }
                scoped.insert(
                    slot.to_string(),
                    Value::Array(boxes.iter().map(bbox_to_json).collect()),
                );
            }
            store_map.insert(scope_key, Value::Object(scoped));
        } else {
            store_map.remove(&scope_key);
        }
    }

    pub fn refresh_preset_scope(&mut self) {
        self.load_scoped_presets();
        self.clear_preview();
    }

    // ------------------------------------------------------------------
    // Classes
    // ------------------------------------------------------------------

    pub fn current_class(&self) -> Option<&Class> {
        if self.current_class_index < 0 {
            return None;
        }
        self.classes.get(self.current_class_index as usize)
    }

    pub fn add_to_recent_projects(&mut self, name: Option<&str>) {
        if self.image_dir.is_empty() {
            return;
        }
        let name = name
            .map(|s| s.to_string())
            .unwrap_or_else(|| final_path_segment(&self.image_dir));
        session::add_recent_project(&name, &self.image_dir, &self.output_dir);
        self.recent_projects = session::get_recent_projects();
    }
}

/// `os.path.abspath` equivalent for Windows paths: makes the path absolute
/// against the CWD and normalizes separators / `.` / `..` lexically without
/// resolving symlinks (so scope keys match Python's output for the same
/// input strings).
pub fn abspath(input: &str) -> String {
    let p = Path::new(input);
    let absolute = if p.is_absolute() {
        p.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(p),
            Err(_) => p.to_path_buf(),
        }
    };

    let text = absolute.to_string_lossy().replace('/', "\\");
    let has_unc_prefix = text.starts_with("\\\\?\\") || text.starts_with("\\\\.");
    let (prefix, rest) = if has_unc_prefix {
        let idx = text[4..].find('\\').map(|i| i + 4).unwrap_or(text.len());
        (text[..idx].to_string(), text[idx..].to_string())
    } else if text.starts_with("\\\\") {
        // \\server\share — keep the first two components intact.
        let mut it = text[2..].splitn(2, '\\');
        let server = it.next().unwrap_or("");
        match it.next() {
            Some(share) => {
                let share = share.split('\\').next().unwrap_or("");
                (format!("\\\\{server}\\{share}"), text[2 + server.len() + 1 + share.len()..].to_string())
            }
            None => (text.clone(), String::new()),
        }
    } else if text.len() >= 2 && text.as_bytes()[1] == b':' {
        (text[..2].to_string(), text[2..].to_string())
    } else {
        (String::new(), text.clone())
    };

    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('\\') {
        match part {
            "" | "." => {}
            ".." => {
                // Do not pop past a drive root.
                if !parts.is_empty() && *parts.last().unwrap() != ".." || prefix.contains(':') && !parts.is_empty() {
                    parts.pop();
                } else if prefix.is_empty() {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }

    let joined = parts.join("\\");
    if prefix.is_empty() {
        format!("\\{joined}")
    } else if joined.is_empty() {
        prefix.trim_end_matches('\\').to_string()
    } else {
        format!("{}\\{}", prefix.trim_end_matches('\\'), joined)
    }
}

pub fn final_path_segment(path: &str) -> String {
    // Python: os.path.basename(path.rstrip(os.sep))
    let trimmed = path.trim_end_matches(['\\', '/']);
    Path::new(trimmed)
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default()
}

pub fn bbox_to_json(b: &BBox) -> Value {
    Value::Array(vec![
        Value::from(b.class_id),
        Value::from(b.x_center),
        Value::from(b.y_center),
        Value::from(b.w),
        Value::from(b.h),
    ])
}

pub fn bbox_from_json(v: &Value) -> Option<BBox> {
    // Accept both the Python box-dict form {class_id, x_center, ...} and our
    // compact array form.
    if let Value::Object(map) = v {
        return Some(BBox::new(
            map.get("class_id")?.as_i64()?,
            map.get("x_center")?.as_f64()?,
            map.get("y_center")?.as_f64()?,
            map.get("w")?.as_f64()?,
            map.get("h")?.as_f64()?,
        ));
    }
    if let Value::Array(arr) = v {
        if arr.len() == 5 {
            return Some(BBox::new(
                arr[0].as_i64()?,
                arr[1].as_f64()?,
                arr[2].as_f64()?,
                arr[3].as_f64()?,
                arr[4].as_f64()?,
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abspath_matches_python_shapes() {
        assert_eq!(abspath(r"D:\kerjaan\Images"), r"D:\kerjaan\Images");
        assert_eq!(abspath(r"D:/kerjaan/Images"), r"D:\kerjaan\Images");
        assert_eq!(abspath(r"D:\kerjaan\Images\.."), r"D:\kerjaan");
        assert_eq!(abspath(r"D:\kerjaan\.\Images\"), r"D:\kerjaan\Images");
    }

    #[test]
    fn scope_key_format() {
        let mut state = AppState {
            session: Map::new(),
            config: Config::load_from(Path::new("nope.json")),
            classes: vec![
                Class { id: 0, name: "a".into(), color_hex: "#000000".into(), color: (0, 0, 0), default_w: 100.0, default_h: 100.0 },
                Class { id: 1, name: "b".into(), color_hex: "#000000".into(), color: (0, 0, 0), default_w: 100.0, default_h: 100.0 },
            ],
            image_dir: r"D:\imgs".into(),
            output_dir: r"D:\out".into(),
            image_list: vec![],
            full_image_list: vec![],
            current_image_index: -1,
            current_image_size: None,
            zoom_factor: 1.0,
            auto_save: true,
            show_labels: true,
            show_right_sidebar: true,
            show_left_sidebar: true,
            show_filmstrip: true,
            unannotated_filter_active: false,
            current_theme_name: "Midnight Glass".into(),
            boxes: vec![],
            selected_indices: BTreeSet::new(),
            clipboard: vec![],
            preview_boxes: vec![],
            preview_visible: false,
            preview_source: None,
            preview_slot: None,
            current_class_index: -1,
            filtered_classes: vec![],
            class_counts: HashMap::new(),
            history: vec![],
            redo_stack: vec![],
            presets: BTreeMap::new(),
            recent_projects: vec![],
        };
        assert_eq!(state.get_preset_scope_key(), r"D:\imgs::D:\out::a||b");
        state.image_dir = String::new();
        state.output_dir = String::new();
        state.classes.clear();
        assert_eq!(
            state.get_preset_scope_key(),
            "__no_image_dir__::__no_output_dir__::__no_classes__"
        );
    }

    #[test]
    fn undo_redo_snapshot_semantics() {
        let mut state = AppState {
            session: Map::new(),
            config: Config::load_from(Path::new("nope.json")),
            classes: vec![],
            image_dir: String::new(),
            output_dir: String::new(),
            image_list: vec![],
            full_image_list: vec![],
            current_image_index: -1,
            current_image_size: None,
            zoom_factor: 1.0,
            auto_save: true,
            show_labels: true,
            show_right_sidebar: true,
            show_left_sidebar: true,
            show_filmstrip: true,
            unannotated_filter_active: false,
            current_theme_name: String::new(),
            boxes: vec![BBox::new(0, 0.5, 0.5, 0.1, 0.1)],
            selected_indices: BTreeSet::from([0]),
            clipboard: vec![],
            preview_boxes: vec![],
            preview_visible: false,
            preview_source: None,
            preview_slot: None,
            current_class_index: -1,
            filtered_classes: vec![],
            class_counts: HashMap::new(),
            history: vec![],
            redo_stack: vec![],
            presets: BTreeMap::new(),
            recent_projects: vec![],
        };

        // save_state is always called BEFORE a mutation (like the real flows):
        state.save_state(); // snapshot [b0]
        state.boxes.push(BBox::new(1, 0.2, 0.2, 0.1, 0.1)); // now [b0, b1]
        state.save_state(); // snapshot [b0, b1]
        state.boxes.push(BBox::new(2, 0.3, 0.3, 0.1, 0.1)); // now [b0, b1, b2]
        state.save_state(); // snapshot [b0, b1, b2] — top equals current…
        state.save_state(); // …so a deduped second save adds nothing.
        // NOTE: the trailing snapshot makes the FIRST undo a no-op (Python
        // behaves identically); the second undo restores the older state.
        assert_eq!(state.history.len(), 3);

        assert!(state.undo()); // no-op restore of identical state
        assert_eq!(state.boxes.len(), 3);
        assert!(state.undo());
        assert_eq!(state.boxes.len(), 2);
        assert!(state.selected_indices.is_empty());
        assert!(state.redo());
        assert_eq!(state.boxes.len(), 3);
        assert!(state.redo()); // replays the no-op snapshot
        assert!(!state.redo());
    }
}
