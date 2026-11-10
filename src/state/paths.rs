//! App-home resolution. The Python tool anchors `session.json`, `config.json`
//! and `data/` to its repository root; this port resolves its home in this
//! order:
//!   1. `ANNOTATION_TOOL_HOME` environment variable
//!   2. `--home <dir>` command line flag
//!   3. the executable's directory (release) or the crate root (debug builds)
//!
//! Point `ANNOTATION_TOOL_HOME` at the Python checkout (e.g.
//! `D:\kerjaan\Annotation-Tool`) to share session/config/classes live between
//! both tools.

use std::path::PathBuf;

pub fn app_home() -> PathBuf {
    if let Ok(dir) = std::env::var("ANNOTATION_TOOL_HOME") {
        if !dir.trim().is_empty() {
            return PathBuf::from(dir.trim_end_matches(['\\', '/']));
        }
    }
    let args: Vec<String> = std::env::args().collect();
    if let Some(pos) = args.iter().position(|a| a == "--home") {
        if let Some(dir) = args.get(pos + 1) {
            if !dir.trim().is_empty() {
                return PathBuf::from(dir.trim_end_matches(['\\', '/']));
            }
        }
    }
    if let Some(home) = home_flag_value(&args) {
        return home;
    }
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    } else {
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

fn home_flag_value(args: &[String]) -> Option<PathBuf> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if let Some(dir) = arg.strip_prefix("--home=") {
            return Some(PathBuf::from(dir));
        }
        if arg == "--home" {
            return iter.next().map(PathBuf::from);
        }
    }
    None
}

pub fn session_file() -> PathBuf {
    app_home().join("session.json")
}

pub fn config_file() -> PathBuf {
    app_home().join("config.json")
}

pub fn data_dir() -> PathBuf {
    app_home().join("data")
}

pub fn classes_file() -> PathBuf {
    data_dir().join("predefined_classes.txt")
}
