//! "Needs review" tracking for auto-labeled images: a small sidecar
//! `.autolabel.json` next to the images (not a .txt, so YOLO tooling, class
//! remaps and annotation backups ignore it).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = ".autolabel.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Labels were written by the model and haven't been reviewed yet.
    Pending,
    /// The model found nothing; no label file was written.
    Empty,
    /// The request failed.
    Error,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub status: Status,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub boxes: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Entry {
    pub fn new(status: Status, model: &str, boxes: usize, error: Option<String>) -> Self {
        Self {
            status,
            model: model.to_string(),
            timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            boxes,
            error,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReviewFile {
    dir: Option<PathBuf>,
    pub entries: BTreeMap<String, Entry>,
}

impl ReviewFile {
    /// Loads the sidecar for `dir` (empty when missing or unreadable).
    pub fn load(dir: &Path) -> Self {
        let entries = std::fs::read_to_string(dir.join(FILE_NAME))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        Self { dir: Some(dir.to_path_buf()), entries }
    }

    /// Writes the sidecar; deletes it when there is nothing left to track.
    pub fn save(&self) -> std::io::Result<()> {
        let Some(dir) = &self.dir else {
            return Ok(());
        };
        let path = dir.join(FILE_NAME);
        if self.entries.is_empty() {
            match std::fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e),
                _ => return Ok(()),
            }
        }
        let text = serde_json::to_string_pretty(&self.entries).map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }

    pub fn set(&mut self, filename: &str, entry: Entry) {
        self.entries.insert(filename.to_string(), entry);
    }

    pub fn status(&self, filename: &str) -> Option<Status> {
        self.entries.get(filename).map(|e| e.status)
    }

    pub fn is_pending(&self, filename: &str) -> bool {
        self.status(filename) == Some(Status::Pending)
    }

    /// Clears the flag for `filename`; true when something was removed.
    pub fn mark_reviewed(&mut self, filename: &str) -> bool {
        self.entries.remove(filename).is_some()
    }

    pub fn count(&self, status: Status) -> usize {
        self.entries.values().filter(|e| e.status == status).count()
    }

    /// Drops `Empty` / `Error` entries so those images are retried.
    pub fn clear_non_pending(&mut self) -> usize {
        let before = self.entries.len();
        self.entries.retain(|_, e| e.status == Status::Pending);
        before - self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "autolabel_review_{tag}_{}_{}",
            std::process::id(),
            chrono::Local::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trip_and_cleanup() {
        let dir = temp_dir("rt");
        let mut r = ReviewFile::load(&dir);
        assert!(r.entries.is_empty());
        r.set("a.png", Entry::new(Status::Pending, "m", 2, None));
        r.set("b.png", Entry::new(Status::Empty, "m", 0, None));
        r.set("c.png", Entry::new(Status::Error, "m", 0, Some("boom".into())));
        r.save().unwrap();

        let mut back = ReviewFile::load(&dir);
        assert_eq!(back.entries, r.entries);
        assert!(back.is_pending("a.png"));
        assert_eq!(back.count(Status::Pending), 1);
        assert!(!back.is_pending("c.png"));

        assert_eq!(back.clear_non_pending(), 2);
        assert!(back.mark_reviewed("a.png"));
        assert!(!back.mark_reviewed("a.png"));
        back.save().unwrap();
        assert!(!dir.join(FILE_NAME).exists(), "empty sidecar is removed");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn corrupt_file_loads_empty() {
        let dir = temp_dir("bad");
        std::fs::write(dir.join(FILE_NAME), "not json").unwrap();
        assert!(ReviewFile::load(&dir).entries.is_empty());
        std::fs::remove_dir_all(dir).ok();
    }
}
