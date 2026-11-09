//! The labelled-example library, kept per class preset and per class:
//!
//! ```text
//! data/examples/<preset>/descriptions.json      {"class name": "hint", ...}
//! data/examples/<preset>/<class>/<image>.png    copied example image
//! data/examples/<preset>/<class>/<image>.json   its boxes, by class *name*
//! ```
//!
//! Boxes are stored by class name (not id) so examples survive class
//! reordering/remapping in the preset.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::classes;
use crate::yolo::BBox;

pub const CUSTOM_PRESET: &str = "custom";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NamedBox {
    pub class: String,
    pub x_center: f64,
    pub y_center: f64,
    pub w: f64,
    pub h: f64,
}

impl NamedBox {
    pub fn to_bbox(&self, class_id: i64) -> BBox {
        BBox::new(class_id, self.x_center, self.y_center, self.w, self.h)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ExampleMeta {
    #[serde(default)]
    source: String,
    #[serde(default)]
    boxes: Vec<NamedBox>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Example {
    pub image: PathBuf,
    pub boxes: Vec<NamedBox>,
    /// Original image path it was copied from.
    pub source: String,
}

/// Filesystem-safe folder name for a preset or class name.
pub fn safe_name(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' })
        .collect();
    let s = s.trim_matches('.').to_string();
    if s.is_empty() { "_".to_string() } else { s }
}

pub fn preset_dir(root: &Path, preset: &str) -> PathBuf {
    root.join(safe_name(preset))
}

pub fn class_dir(root: &Path, preset: &str, class_name: &str) -> PathBuf {
    preset_dir(root, preset).join(safe_name(class_name))
}

/// Copies `image_path` into the class's example folder together with
/// `boxes` (`(class name, box)` pairs). Returns the stored image path.
pub fn add_example(
    root: &Path,
    preset: &str,
    class_name: &str,
    image_path: &Path,
    boxes: &[(String, BBox)],
) -> std::io::Result<PathBuf> {
    let dir = class_dir(root, preset, class_name);
    std::fs::create_dir_all(&dir)?;
    let stem = image_path
        .file_stem()
        .map(|s| safe_name(&s.to_string_lossy()))
        .unwrap_or_else(|| "example".into());
    let ext = image_path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "png".into());

    // Same source already stored → overwrite its boxes instead of duplicating.
    let source = image_path.to_string_lossy().to_string();
    let existing = list_examples(root, preset, class_name)
        .into_iter()
        .find(|e| e.source == source);
    let target = match existing {
        Some(e) => e.image,
        None => {
            let mut n = 1;
            loop {
                let name = if n == 1 { format!("{stem}.{ext}") } else { format!("{stem}_{n}.{ext}") };
                let candidate = dir.join(name);
                if !candidate.exists() {
                    break candidate;
                }
                n += 1;
            }
        }
    };
    if !target.exists() {
        std::fs::copy(image_path, &target)?;
    }
    let meta = ExampleMeta {
        source,
        boxes: boxes
            .iter()
            .filter(|(_, b)| b.class_id >= 0)
            .map(|(name, b)| NamedBox {
                class: name.clone(),
                x_center: b.x_center,
                y_center: b.y_center,
                w: b.w,
                h: b.h,
            })
            .collect(),
    };
    let text = serde_json::to_string_pretty(&meta).map_err(std::io::Error::other)?;
    std::fs::write(target.with_extension("json"), text)?;
    Ok(target)
}

/// Examples for a class, natural-sorted by file name.
pub fn list_examples(root: &Path, preset: &str, class_name: &str) -> Vec<Example> {
    let dir = class_dir(root, preset, class_name);
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| classes::is_image_file(n))
                .collect()
        })
        .unwrap_or_default();
    names.sort_by(|a, b| classes::natural_sort_key(a).cmp(&classes::natural_sort_key(b)));
    names
        .into_iter()
        .map(|name| {
            let image = dir.join(&name);
            let meta: ExampleMeta = std::fs::read_to_string(image.with_extension("json"))
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
            Example { image, boxes: meta.boxes, source: meta.source }
        })
        .collect()
}

pub fn remove_example(image: &Path) -> std::io::Result<()> {
    std::fs::remove_file(image)?;
    match std::fs::remove_file(image.with_extension("json")) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

pub fn load_descriptions(root: &Path, preset: &str) -> BTreeMap<String, String> {
    std::fs::read_to_string(preset_dir(root, preset).join("descriptions.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

pub fn save_descriptions(
    root: &Path,
    preset: &str,
    descriptions: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    let dir = preset_dir(root, preset);
    std::fs::create_dir_all(&dir)?;
    let cleaned: BTreeMap<&String, &String> =
        descriptions.iter().filter(|(_, v)| !v.trim().is_empty()).collect();
    let text = serde_json::to_string_pretty(&cleaned).map_err(std::io::Error::other)?;
    std::fs::write(dir.join("descriptions.json"), text)
}

fn class_lines(text: &str) -> Vec<String> {
    text.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect()
}

/// Which `data/<preset>.txt` the active class file came from. Matches by
/// content first (so a manually copied preset is recognised), then falls
/// back to the remembered session value, then [`CUSTOM_PRESET`].
pub fn detect_active_preset(data_dir: &Path, classes_file: &Path, remembered: Option<&str>) -> String {
    let active = std::fs::read_to_string(classes_file).map(|t| class_lines(&t)).unwrap_or_default();
    if !active.is_empty() {
        let mut candidates: Vec<PathBuf> = std::fs::read_dir(data_dir)
            .map(|entries| entries.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        candidates.sort();
        for path in candidates {
            let is_txt = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("txt"));
            if !is_txt || path == classes_file || path.file_name() == classes_file.file_name() {
                continue;
            }
            if std::fs::read_to_string(&path).map(|t| class_lines(&t)).ok().as_ref() == Some(&active) {
                if let Some(stem) = path.file_stem() {
                    return stem.to_string_lossy().to_string();
                }
            }
        }
    }
    match remembered.map(str::trim) {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => CUSTOM_PRESET.to_string(),
    }
}

/// Classes whose name matches the image folder name (e.g. folder
/// `triple_kill` → class `triple_kill`), used to pre-select targets.
pub fn classes_matching_folder(folder: &str, classes: &[(i64, String)]) -> Vec<i64> {
    let key = |s: &str| -> String {
        s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
    };
    let folder_key = key(folder);
    if folder_key.is_empty() {
        return Vec::new();
    }
    classes
        .iter()
        .filter(|(_, n)| key(n) == folder_key)
        .map(|(id, _)| *id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "autolabel_examples_{tag}_{}_{}",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_png(path: &Path) {
        image::RgbImage::new(4, 4).save(path).unwrap();
    }

    #[test]
    fn add_list_dedupe_remove() {
        let root = temp_dir("lib");
        let src_dir = temp_dir("src");
        let img = src_dir.join("shot 1.png");
        write_png(&img);
        let boxes = vec![
            ("kill_banner".to_string(), BBox::new(0, 0.5, 0.5, 0.2, 0.1)),
            ("unlabeled".to_string(), BBox::new(-1, 0.1, 0.1, 0.1, 0.1)),
        ];
        let stored = add_example(&root, "wz", "kill_banner", &img, &boxes).unwrap();
        assert_eq!(stored.file_name().unwrap(), "shot_1.png");

        let list = list_examples(&root, "wz", "kill_banner");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].boxes.len(), 1, "unlabeled boxes are not stored");
        assert_eq!(list[0].boxes[0].class, "kill_banner");

        // Re-adding the same source updates instead of duplicating.
        add_example(&root, "wz", "kill_banner", &img, &boxes[..1]).unwrap();
        assert_eq!(list_examples(&root, "wz", "kill_banner").len(), 1);

        // A different source with the same file name gets a suffix.
        let other_dir = temp_dir("src2");
        let img2 = other_dir.join("shot 1.png");
        write_png(&img2);
        let stored2 = add_example(&root, "wz", "kill_banner", &img2, &boxes[..1]).unwrap();
        assert_eq!(stored2.file_name().unwrap(), "shot_1_2.png");
        assert_eq!(list_examples(&root, "wz", "kill_banner").len(), 2);
        assert!(list_examples(&root, "wz", "other").is_empty());

        remove_example(&stored).unwrap();
        assert!(!stored.with_extension("json").exists());
        assert_eq!(list_examples(&root, "wz", "kill_banner").len(), 1);

        for d in [root, src_dir, other_dir] {
            std::fs::remove_dir_all(d).ok();
        }
    }

    #[test]
    fn descriptions_round_trip() {
        let root = temp_dir("desc");
        let mut d = BTreeMap::new();
        d.insert("a".to_string(), "red banner at the top".to_string());
        d.insert("b".to_string(), "  ".to_string());
        save_descriptions(&root, "wz", &d).unwrap();
        let back = load_descriptions(&root, "wz");
        assert_eq!(back.len(), 1);
        assert_eq!(back["a"], "red banner at the top");
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn detects_preset_by_content() {
        let data = temp_dir("data");
        std::fs::write(data.join("wz.txt"), "a\nb\n").unwrap();
        std::fs::write(data.join("valo.txt"), "x\ny\n").unwrap();
        let active = data.join("predefined_classes.txt");
        std::fs::write(&active, "x\r\ny\r\n").unwrap();
        assert_eq!(detect_active_preset(&data, &active, Some("wz")), "valo");
        std::fs::write(&active, "edited\n").unwrap();
        assert_eq!(detect_active_preset(&data, &active, Some("wz")), "wz");
        assert_eq!(detect_active_preset(&data, &active, None), CUSTOM_PRESET);
        std::fs::remove_dir_all(data).ok();
    }

    #[test]
    fn folder_matching() {
        let classes = vec![(0, "triple_kill".to_string()), (1, "Released Banner".to_string())];
        assert_eq!(classes_matching_folder("triple_kill", &classes), vec![0]);
        assert_eq!(classes_matching_folder("released-banner", &classes), vec![1]);
        assert!(classes_matching_folder("misc", &classes).is_empty());
    }

    #[test]
    fn safe_names() {
        assert_eq!(safe_name("a/b:c"), "a_b_c");
        assert_eq!(safe_name(".."), "_");
        assert_eq!(safe_name("kill_banner"), "kill_banner");
    }
}
