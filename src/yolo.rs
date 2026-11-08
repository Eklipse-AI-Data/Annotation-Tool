//! YOLO annotation format I/O and file manipulation — a faithful port of
//! `src/utils.py` (parse_yolo / save_yolo / update_annotation_file /
//! backup_annotations / normalize_box / denormalize_box) so both the Python
//! and Rust tools produce byte-identical annotation files.

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

/// A YOLO bounding box. All coordinates are normalized (0.0–1.0) floats and
/// `class_id == -1` means "unlabeled" (kept in memory, never written to disk).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub class_id: i64,
    pub x_center: f64,
    pub y_center: f64,
    pub w: f64,
    pub h: f64,
}

impl BBox {
    pub fn new(class_id: i64, x_center: f64, y_center: f64, w: f64, h: f64) -> Self {
        Self {
            class_id,
            x_center,
            y_center,
            w,
            h,
        }
    }

    /// Pixel corners `(x1, y1, x2, y2)` for this box on an image of the given
    /// size. Port of `denormalize_box`.
    pub fn denormalized(&self, img_width: f64, img_height: f64) -> (f64, f64, f64, f64) {
        let w = self.w * img_width;
        let h = self.h * img_height;
        let x_center = self.x_center * img_width;
        let y_center = self.y_center * img_height;
        (x_center - w / 2.0, y_center - h / 2.0, x_center + w / 2.0, y_center + h / 2.0)
    }

    /// Build a normalized box from pixel corners. Drag direction does not
    /// matter (`abs`/`min` normalization). Port of `normalize_box`.
    pub fn from_corners(
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
        img_width: f64,
        img_height: f64,
        class_id: i64,
    ) -> Self {
        let w = (x2 - x1).abs();
        let h = (y2 - y1).abs();
        let x_center = x1.min(x2) + w / 2.0;
        let y_center = y1.min(y2) + h / 2.0;
        Self::new(
            class_id,
            x_center / img_width,
            y_center / img_height,
            w / img_width,
            h / img_height,
        )
    }
}

/// Parses a YOLO `.txt` file. Lines with fewer than 5 whitespace-separated
/// tokens are silently skipped; extra tokens are ignored. A malformed value
/// aborts parsing but keeps the boxes parsed so far (Python parity).
pub fn parse_yolo(file_path: &Path) -> Vec<BBox> {
    let mut boxes = Vec::new();
    let Ok(text) = std::fs::read_to_string(file_path) else {
        return boxes;
    };
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }
        let (Ok(class_id), Ok(x_center), Ok(y_center), Ok(w), Ok(h)) = (
            parts[0].parse::<i64>(),
            parts[1].parse::<f64>(),
            parts[2].parse::<f64>(),
            parts[3].parse::<f64>(),
            parts[4].parse::<f64>(),
        ) else {
            // Python wraps the whole loop in try/except: one bad line stops
            // the parse while keeping everything read so far.
            break;
        };
        boxes.push(BBox::new(class_id, x_center, y_center, w, h));
    }
    boxes
}

/// Serializes boxes with Python's exact `f"{id} {x:.6f} {y:.6f} {w:.6f} {h:.6f}\n"`
/// formatting, truncating any existing file.
pub fn save_yolo(file_path: &Path, boxes: &[BBox]) -> io::Result<()> {
    
    let mut text = String::with_capacity(boxes.len() * 40);
    for b in boxes {
        text.push_str(&format!(
            "{} {:.6} {:.6} {:.6} {:.6}\n",
            b.class_id, b.x_center, b.y_center, b.w, b.h
        ));
    }
    std::fs::write(file_path, text)
}

/// Rewrites the class ids in an annotation file according to `mapping`.
/// `None` values drop the line; ids not in the mapping are kept unless
/// `remove_unmapped` is true. Lines with < 5 tokens are dropped entirely
/// (Python parity). Returns false on I/O failure.
pub fn update_annotation_file(
    file_path: &Path,
    class_mapping: &HashMap<i64, Option<i64>>,
    remove_unmapped: bool,
) -> bool {
    if !file_path.exists() {
        return true; // Nothing to update
    }
    let Ok(text) = std::fs::read_to_string(file_path) else {
        return false;
    };
    let mut updated = String::new();
    for line in text.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }
        let Ok(old_class_id) = parts[0].parse::<i64>() else {
            continue;
        };
        if let Some(mapped) = class_mapping.get(&old_class_id) {
            if let Some(new_id) = mapped {
                let id_str = new_id.to_string();
                let parts = [id_str.as_str()]
                    .into_iter()
                    .chain(parts[1..].iter().copied())
                    .collect::<Vec<&str>>();
                updated.push_str(&parts.join(" "));
                updated.push('\n');
            }
            // None => class removed, drop the line
        } else if !remove_unmapped {
            updated.push_str(&parts.join(" "));
            updated.push('\n');
        }
    }
    std::fs::write(file_path, updated).is_ok()
}

/// Creates a timestamped backup of every `.txt` file in `dir` inside
/// `annotations_backup_YYYYMMDD_HHMMSS/`. Returns `None` when there is
/// nothing to back up or on failure (Python parity).
pub fn backup_annotations(
    dir: &Path,
    mut progress: impl FnMut(usize, usize, &str) + 'static,
) -> Option<PathBuf> {
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let backup_dir = dir.join(format!("annotations_backup_{timestamp}"));

    let mut txt_files: Vec<String> = Vec::new();
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.to_lowercase().ends_with(".txt") {
            txt_files.push(name);
        }
    }
    if txt_files.is_empty() {
        return None;
    }

    if std::fs::create_dir_all(&backup_dir).is_err() {
        return None;
    }
    let total = txt_files.len();
    for (i, filename) in txt_files.iter().enumerate() {
        let _ = std::fs::copy(dir.join(filename), backup_dir.join(filename));
        progress(i + 1, total, filename);
    }
    Some(backup_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_str(b: BBox) -> String {
        format!(
            "{} {:.6} {:.6} {:.6} {:.6}\n",
            b.class_id, b.x_center, b.y_center, b.w, b.h
        )
    }

    #[test]
    fn formats_like_python() {
        // Matches Python f"{0} {0.5:.6f} ..." exactly.
        assert_eq!(box_str(BBox::new(0, 0.5, 0.25, 0.75, 0.125)), "0 0.500000 0.250000 0.750000 0.125000\n");
        // Python truncates trailing zeros the same way ({:.6f} keeps them) — verify keep.
        assert_eq!(box_str(BBox::new(3, 1.0, 0.0, 0.1234565, 2.0)), "3 1.000000 0.000000 0.123456 2.000000\n");
    }

    #[test]
    fn round_trip() {
        let dir = std::env::temp_dir().join("at_yolo_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rt.txt");
        // Values limited to what 6-decimal formatting preserves.
        let boxes = vec![
            BBox::new(0, 0.5, 0.5, 0.25, 0.25),
            BBox::new(12, 0.123457, 0.987654, 0.1, 0.2),
        ];
        save_yolo(&path, &boxes).unwrap();
        assert_eq!(parse_yolo(&path), boxes);
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "0 0.500000 0.500000 0.250000 0.250000\n12 0.123457 0.987654 0.100000 0.200000\n");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn skips_short_lines_and_stops_on_garbage() {
        let dir = std::env::temp_dir().join("at_yolo_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("skips.txt");
        std::fs::write(&path, "0 0.5 0.5 0.1 0.1\n\nbad line\n1 0.1 0.1 0.2 0.2 0.3 extra\nx y z\n2 0.9 0.9 0.05 0.05\n").unwrap();
        let parsed = parse_yolo(&path);
        // Short lines ("bad line", "x y z") are skipped, not errors — Python
        // behavior is identical; a bad VALUE would stop the parse.
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].class_id, 0);
        assert_eq!(parsed[1].class_id, 1);
        assert_eq!(parsed[2].class_id, 2);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn remap_drops_and_keeps() {
        let dir = std::env::temp_dir().join("at_yolo_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("remap.txt");
        std::fs::write(&path, "0 0.1 0.1 0.1 0.1\n5 0.2 0.2 0.1 0.1\n9 0.3 0.3 0.1 0.1\n").unwrap();
        let mut mapping = HashMap::new();
        mapping.insert(0i64, Some(7i64));
        mapping.insert(5i64, None); // removed class
        assert!(update_annotation_file(&path, &mapping, false));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "7 0.1 0.1 0.1 0.1\n9 0.3 0.3 0.1 0.1\n" // 5 dropped, 9 kept (not in mapping)
        );
        // remove_unmapped drops unknown ids too
        std::fs::write(&path, "0 0.1 0.1 0.1 0.1\n9 0.3 0.3 0.1 0.1\n").unwrap();
        assert!(update_annotation_file(&path, &mapping, true));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "7 0.1 0.1 0.1 0.1\n");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn normalize_is_direction_agnostic() {
        let b = BBox::from_corners(100.0, 200.0, 10.0, 20.0, 1000.0, 1000.0, 4);
        assert!((b.x_center - 0.055).abs() < 1e-12);
        assert!((b.w - 0.09).abs() < 1e-12);
        let (x1, y1, x2, y2) = b.denormalized(1000.0, 1000.0);
        assert!((x1 - 10.0).abs() < 1e-9 && (y2 - 200.0).abs() < 1e-9 && x1 < x2 && y1 < y2);
    }
}
