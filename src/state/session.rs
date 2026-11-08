//! session.json persistence with Python-compatible semantics:
//! defaults merged over the file on load, and saves merge over the existing
//! file content so unknown/stale keys are never dropped. Files are written
//! with 4-space indentation, byte-compatible with `json.dump(indent=4)` for
//! ASCII content.

use serde_json::{Map, Value};

use super::paths;

pub fn default_session() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("last_image_dir".into(), Value::String(String::new()));
    m.insert("last_output_dir".into(), Value::String(String::new()));
    m.insert("last_image_index".into(), Value::from(0));
    m.insert("zoom_factor".into(), Value::from(1.0));
    m.insert("show_labels".into(), Value::Bool(true));
    m.insert("auto_save".into(), Value::Bool(true));
    m.insert("show_right_sidebar".into(), Value::Bool(true));
    m.insert("show_left_sidebar".into(), Value::Bool(true));
    m.insert("show_filmstrip".into(), Value::Bool(true));
    m.insert("theme_name".into(), Value::String("Midnight Glass".into()));
    m.insert("recent_projects".into(), Value::Array(Vec::new()));
    m.insert("box_presets_by_scope".into(), Value::Object(Map::new()));
    m
}

/// Load session state: defaults overlaid with the file's contents. Missing or
/// corrupt file yields the defaults (Python parity).
pub fn load_session(path: &std::path::Path) -> Map<String, Value> {
    let mut merged = default_session();
    let Ok(text) = std::fs::read_to_string(path) else {
        return merged;
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(file_map)) => {
            for (k, v) in file_map {
                merged.insert(k, v);
            }
        }
        _ => {}
    }
    merged
}

/// Save `state` merged over the existing file contents (never drops keys).
pub fn save_session(path: &std::path::Path, state: &Map<String, Value>) {
    let mut merged = load_session(path);
    for (k, v) in state {
        merged.insert(k.clone(), v.clone());
    }
    let _ = std::fs::write(path, to_json_pretty4(&Value::Object(merged)));
}

/// JSON pretty printer with 4-space indent, matching Python's
/// `json.dump(..., indent=4)`.
pub fn to_json_pretty4(value: &Value) -> String {
    use serde::ser::Serialize;
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(
        &mut buf,
        serde_json::ser::PrettyFormatter::with_indent(b"    "),
    );
    if value.serialize(&mut ser).is_err() {
        return value.to_string();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

// ---------------------------------------------------------------------------
// Recent projects (MRU of 10, deduped by image_dir) — Python parity. These
// operate directly on the file like the Python code does.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub struct RecentProject {
    pub name: String,
    pub image_dir: String,
    pub output_dir: String,
}

pub fn get_recent_projects() -> Vec<RecentProject> {
    let session = load_session(&paths::session_file());
    parse_recent(&session)
}

pub fn parse_recent(session: &Map<String, Value>) -> Vec<RecentProject> {
    let mut out = Vec::new();
    if let Some(Value::Array(entries)) = session.get("recent_projects") {
        for entry in entries {
            if let Value::Object(map) = entry {
                let name = map.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                let image_dir =
                    map.get("image_dir").and_then(Value::as_str).unwrap_or("").to_string();
                let output_dir =
                    map.get("output_dir").and_then(Value::as_str).unwrap_or("").to_string();
                out.push(RecentProject { name, image_dir, output_dir });
            }
        }
    }
    out
}

/// Inserts at the front, dedupes by image_dir, trims to 10, and round-trips
/// the file on disk (exactly like `add_recent_project`, which takes the
/// session path as a parameter in Python too).
pub fn add_recent_project_at(path: &std::path::Path, name: &str, image_dir: &str, output_dir: &str) {
    let mut session = load_session(path);
    let mut recent = parse_recent(&session);
    recent.retain(|p| p.image_dir != image_dir);
    recent.insert(
        0,
        RecentProject {
            name: name.to_string(),
            image_dir: image_dir.to_string(),
            output_dir: output_dir.to_string(),
        },
    );
    recent.truncate(10);
    session.insert("recent_projects".into(), recent_into_value(&recent));
    std::fs::write(path, to_json_pretty4(&Value::Object(session))).ok();
}

/// App-home variant used by the app itself.
pub fn add_recent_project(name: &str, image_dir: &str, output_dir: &str) {
    add_recent_project_at(&paths::session_file(), name, image_dir, output_dir);
}

pub fn recent_into_value(recent: &[RecentProject]) -> Value {
    Value::Array(
        recent
            .iter()
            .map(|p| {
                Value::Object({
                    let mut m = Map::new();
                    m.insert("name".into(), Value::String(p.name.clone()));
                    m.insert("image_dir".into(), Value::String(p.image_dir.clone()));
                    m.insert("output_dir".into(), Value::String(p.output_dir.clone()));
                    m
                })
            })
            .collect(),
    )
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_preserves_unknown_keys() {
        let dir = std::env::temp_dir().join("at_session_test_merge");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        std::fs::write(
            &path,
            r#"{"theme_name":"Ruby","custom_future_key":[1,2],"zoom_factor":2.5}"#,
        )
        .unwrap();

        let mut session = load_session(&path);
        assert_eq!(session.get("theme_name").unwrap(), "Ruby");
        assert_eq!(session.get("zoom_factor").unwrap(), &Value::from(2.5));
        assert_eq!(session.get("auto_save").unwrap(), &Value::Bool(true)); // default kept

        session.insert("theme_name".into(), Value::String("Amber".into()));
        save_session(&path, &session);

        let reloaded = load_session(&path);
        assert_eq!(reloaded.get("theme_name").unwrap(), "Amber");
        assert_eq!(reloaded.get("custom_future_key").unwrap(), &Value::Array(vec![1.into(), 2.into()]));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn pretty4_indent_matches_python() {
        let mut m = Map::new();
        m.insert("a".into(), Value::from(1));
        m.insert("b".into(), Value::Array(vec![Value::from(2), Value::Bool(true)]));
        let text = to_json_pretty4(&Value::Object(m));
        assert_eq!(text.replace("\r\n", "\n"), "{\n    \"a\": 1,\n    \"b\": [\n        2,\n        true\n    ]\n}");
    }

    #[test]
    fn recent_dedup_and_trim() {
        let dir = std::env::temp_dir().join("at_session_test_recent");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("session.json");
        let _ = std::fs::remove_file(&path);
        for i in 0..12 {
            add_recent_project_at(&path, &format!("p{i}"), &format!("C:\\img{i}"), &format!("C:\\out{i}"));
        }
        // Re-add the first; it should move to front without duplicating.
        add_recent_project_at(&path, "p0", "C:\\img0", "C:\\out0b");
        let recent = {
            let s = load_session(&path);
            parse_recent(&s)
        };
        assert_eq!(recent.len(), 10);
        assert_eq!(recent[0].image_dir, "C:\\img0");
        assert_eq!(recent[0].output_dir, "C:\\out0b");
        assert_eq!(recent[1].image_dir, "C:\\img11");
        std::fs::remove_file(&path).ok();
    }
}
