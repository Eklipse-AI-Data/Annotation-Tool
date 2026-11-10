//! config.json persistence + the Tkinter keybinding-syntax ⇄ GPUI keystroke
//! converter. The Python tool stores bindings like `<Control-c>` / `<d>` /
//! `<Down>`; GPUI keymaps use `ctrl-c` / `d` / `down`. We convert on load and
//! regenerate Tk syntax when the user captures a new key.

use serde_json::{Map, Value};

use super::paths;

/// The user-configurable actions (keys in config.json, in display order).
pub const BINDABLE_ACTIONS: &[(&str, &str)] = &[
    ("deselect", "Deselect Class"),
    ("next_image", "Next Image"),
    ("prev_image", "Previous Image"),
    ("cycle_class", "Cycle Class"),
    ("delete_box", "Delete Box"),
    ("copy", "Copy"),
    ("paste", "Paste"),
    ("duplicate", "Duplicate"),
    ("edit_class", "Edit Class"),
    ("undo", "Undo"),
    ("redo", "Redo"),
];

pub const DEFAULT_CROSSHAIR_DASH: (f32, f32) = (2.0, 2.0);

#[derive(Debug, Clone)]
pub struct Config {
    /// Raw merged config map (preserves unknown keys for save).
    pub raw: Map<String, Value>,
    /// Tk pattern per action id, e.g. "next_image" -> "<d>".
    pub bindings: std::collections::BTreeMap<String, String>,
    pub crosshair_color: String, // "#FFFFFF"
    pub crosshair_dash: (f32, f32),
}

impl Config {
    pub fn defaults() -> Map<String, Value> {
        let mut m = Map::new();
        let defaults: &[(&str, &str)] = &[
            ("deselect", "<Escape>"),
            ("next_image", "<Right>"),
            ("prev_image", "<Left>"),
            ("cycle_class", "<Down>"),
            ("delete_box", "<Delete>"),
            ("copy", "<Control-c>"),
            ("paste", "<Control-v>"),
            ("duplicate", "<Control-d>"),
            ("edit_class", "<Control-e>"),
            ("undo", "<Control-z>"),
            ("redo", "<Control-y>"),
        ];
        for (k, v) in defaults {
            m.insert((*k).into(), Value::String((*v).into()));
        }
        m.insert("crosshair_color".into(), Value::String("#FFFFFF".into()));
        m.insert(
            "crosshair_dash".into(),
            Value::Array(vec![Value::from(2), Value::from(2)]),
        );
        m
    }

    pub fn load() -> Self {
        Self::load_from(&paths::config_file())
    }

    pub fn load_from(path: &std::path::Path) -> Self {
        let mut raw = Self::defaults();
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(Value::Object(file_map)) = serde_json::from_str::<Value>(&text) {
                for (k, v) in file_map {
                    raw.insert(k, v);
                }
            }
        }
        Self::from_raw(raw)
    }

    fn from_raw(raw: Map<String, Value>) -> Self {
        let mut bindings = std::collections::BTreeMap::new();
        for (action, _) in BINDABLE_ACTIONS {
            if let Some(Value::String(pattern)) = raw.get(*action) {
                bindings.insert(action.to_string(), pattern.clone());
            }
        }
        let crosshair_color = raw
            .get("crosshair_color")
            .and_then(Value::as_str)
            .unwrap_or("#FFFFFF")
            .to_string();
        let crosshair_dash = match raw.get("crosshair_dash").and_then(Value::as_array) {
            Some(arr) if arr.len() >= 2 => (
                arr[0].as_f64().unwrap_or(2.0) as f32,
                arr[1].as_f64().unwrap_or(2.0) as f32,
            ),
            _ => DEFAULT_CROSSHAIR_DASH,
        };
        Self {
            raw,
            bindings,
            crosshair_color,
            crosshair_dash,
        }
    }

    pub fn set_binding(&mut self, action: &str, tk_pattern: &str) {
        self.bindings.insert(action.to_string(), tk_pattern.to_string());
        self.raw.insert(action.to_string(), Value::String(tk_pattern.to_string()));
    }

    pub fn save(&self) {
        let path = paths::config_file();
        // Merge over existing file contents to preserve unknown keys.
        let mut merged = Self::defaults();
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(Value::Object(file_map)) = serde_json::from_str::<Value>(&text) {
                for (k, v) in file_map {
                    merged.insert(k, v);
                }
            }
        }
        for (k, v) in &self.raw {
            merged.insert(k.clone(), v.clone());
        }
        std::fs::write(&path, super::session::to_json_pretty4(&Value::Object(merged))).ok();
    }

    /// GPUI keystroke string for an action, if convertible.
    pub fn gpui_binding(&self, action: &str) -> Option<String> {
        self.bindings.get(action).and_then(|p| tk_to_gpui(p))
    }

    /// Human-readable shortcut for an action ("Ctrl+C", "→"), falling back
    /// to the raw Tk pattern when it can't be converted.
    pub fn display_binding(&self, action: &str) -> String {
        match self.bindings.get(action) {
            Some(p) => tk_to_gpui(p)
                .map(|k| pretty_keystroke(&k))
                .unwrap_or_else(|| p.clone()),
            None => "—".to_string(),
        }
    }
}

/// Formats a GPUI keystroke string (`ctrl-shift-c`, `right`) for display
/// (`Ctrl+Shift+C`, `→`).
pub fn pretty_keystroke(keys: &str) -> String {
    // Split modifiers off the front; the key itself may be "-".
    let mut parts: Vec<&str> = Vec::new();
    let mut rest = keys;
    loop {
        let mut matched = false;
        for m in ["ctrl-", "alt-", "shift-", "cmd-"] {
            if let Some(r) = rest.strip_prefix(m) {
                if !r.is_empty() {
                    parts.push(&m[..m.len() - 1]);
                    rest = r;
                    matched = true;
                }
            }
        }
        if !matched {
            break;
        }
    }
    let mut out: Vec<String> = parts
        .iter()
        .map(|m| match *m {
            "ctrl" => "Ctrl".to_string(),
            "alt" => "Alt".to_string(),
            "shift" => "Shift".to_string(),
            _ => "Cmd".to_string(),
        })
        .collect();
    let key = match rest {
        "left" => "←".to_string(),
        "right" => "→".to_string(),
        "up" => "↑".to_string(),
        "down" => "↓".to_string(),
        "escape" => "Esc".to_string(),
        "enter" => "Enter".to_string(),
        "space" => "Space".to_string(),
        "delete" => "Delete".to_string(),
        "backspace" => "Backspace".to_string(),
        "tab" => "Tab".to_string(),
        "pageup" => "PgUp".to_string(),
        "pagedown" => "PgDn".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),
        k if k.starts_with('f') && k.len() > 1 && k[1..].chars().all(|c| c.is_ascii_digit()) => k.to_uppercase(),
        k if k.chars().count() == 1 => k.to_uppercase(),
        k => {
            let mut c = k.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        }
    };
    out.push(key);
    out.join("+")
}

// ---------------------------------------------------------------------------
// Tk ⇄ GPUI keystroke conversion
// ---------------------------------------------------------------------------

/// Tk symbolic keysyms that map to plain characters.
const TK_SYMBOLIC_CHARS: &[(&str, &str)] = &[
    ("exclam", "!"),
    ("at", "@"),
    ("numbersign", "#"),
    ("dollar", "$"),
    ("percent", "%"),
    ("asciicircum", "^"),
    ("ampersand", "&"),
    ("asterisk", "*"),
    ("parenleft", "("),
    ("parenright", ")"),
    ("minus", "-"),
    ("underscore", "_"),
    ("plus", "+"),
    ("equal", "="),
    ("question", "?"),
    ("slash", "/"),
    ("backslash", "\\"),
    ("period", "."),
    ("comma", ","),
    ("semicolon", ";"),
    ("colon", ":"),
    ("quotedbl", "\""),
    ("apostrophe", "'"),
    ("bracketleft", "["),
    ("bracketright", "]"),
    ("braceleft", "{"),
    ("braceright", "}"),
    ("grave", "`"),
    ("less", "<"),
    ("greater", ">"),
    ("bar", "|"),
];

/// Convert a Tk pattern like `<Control-c>`, `<d>`, `<Down>` or `<space>` to a
/// GPUI keystroke string like `ctrl-c`, `d`, `down`, `space`. Modifiers are
/// emitted in GPUI's canonical order: ctrl, alt, shift, cmd.
pub fn tk_to_gpui(pattern: &str) -> Option<String> {
    let inner = if let Some(stripped) = pattern.strip_prefix('<') {
        stripped.strip_suffix('>')?.to_string()
    } else {
        pattern.to_string()
    };
    if inner.is_empty() {
        return None;
    }

    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut cmd = false;
    // The final component is the key; all others must be modifiers.
    let parts: Vec<&str> = inner.split('-').collect();
    if parts.len() > 5 {
        return None;
    }
    for part in &parts[..parts.len() - 1] {
        match part.to_ascii_lowercase().as_str() {
            "control" | "ctrl" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            "command" | "cmd" | "meta" => cmd = true,
            "key" => {} // "<Key-x>" form
            _ => return None,
        }
    }
    let key = normalize_key(parts[parts.len() - 1])?;

    let mut out = String::new();
    if ctrl {
        out.push_str("ctrl-");
    }
    if alt {
        out.push_str("alt-");
    }
    if shift {
        out.push_str("shift-");
    }
    if cmd {
        out.push_str("cmd-");
    }
    out.push_str(&key);
    Some(out)
}

/// Maps a Tk keysym to GPUI's key name.
fn normalize_key(keysym: &str) -> Option<String> {
    let k = keysym.trim();
    if k.is_empty() {
        return None;
    }
    // Single characters pass through (letters lowercased, Python capture style).
    if k.chars().count() == 1 {
        let c = k.chars().next().unwrap();
        return Some(c.to_lowercase().to_string());
    }
    Some(
        match k.to_ascii_lowercase().as_str() {
            "escape" => "escape",
            "space" => "space",
            "return" | "enter" => "enter",
            "tab" => "tab",
            "backspace" => "backspace",
            "delete" => "delete",
            "insert" => "insert",
            "home" => "home",
            "end" => "end",
            "prior" | "pageup" => "pageup",
            "next" | "pagedown" => "pagedown",
            "left" => "left",
            "right" => "right",
            "up" => "up",
            "down" => "down",
            f if f.starts_with('f') && f.len() <= 3 && f[1..].parse::<u8>().is_ok() => {
                return Some(f.to_string())
            }
            other => {
                // Tk symbolic punctuation names.
                return TK_SYMBOLIC_CHARS
                    .iter()
                    .find(|(name, _)| *name == other)
                    .map(|(_, ch)| ch.to_string());
            }
        }
        .to_string(),
    )
}

/// Convert a GPUI keystroke (modifiers + key) back into a Tk pattern, using
/// Python's capture order `<Control-Alt-Shift-keysym>` with single-char
/// keysyms lowercased. `None` when the key has no Tk representation.
pub fn gpui_to_tk(ctrl: bool, alt: bool, shift: bool, cmd: bool, key: &str) -> Option<String> {
    let keysym = gpui_key_to_tk_keysym(key)?;
    let mut pattern = String::from("<");
    if ctrl {
        pattern.push_str("Control-");
    }
    if alt {
        pattern.push_str("Alt-");
    }
    if shift {
        pattern.push_str("Shift-");
    }
    if cmd {
        pattern.push_str("Command-");
    }
    pattern.push_str(&keysym);
    pattern.push('>');
    Some(pattern)
}

fn gpui_key_to_tk_keysym(key: &str) -> Option<String> {
    if key.chars().count() == 1 {
        return Some(key.to_lowercase());
    }
    Some(match key.to_ascii_lowercase().as_str() {
        "escape" => "Escape",
        "space" => "space",
        "enter" | "return" => "Return",
        "tab" => "Tab",
        "backspace" => "BackSpace",
        "delete" => "Delete",
        "insert" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" => "Prior",
        "pagedown" => "Next",
        "left" => "Left",
        "right" => "Right",
        "up" => "Up",
        "down" => "Down",
        "?" => "question",
        "!" => "exclam",
        "@" => "at",
        "#" => "numbersign",
        "$" => "dollar",
        "%" => "percent",
        "^" => "asciicircum",
        "&" => "ampersand",
        "*" => "asterisk",
        "(" => "parenleft",
        ")" => "parenright",
        "-" => "minus",
        "_" => "underscore",
        "+" => "plus",
        "=" => "equal",
        "/" => "slash",
        "\\" => "backslash",
        "." => "period",
        "," => "comma",
        ";" => "semicolon",
        ":" => "colon",
        "\"" => "quotedbl",
        "'" => "apostrophe",
        "[" => "bracketleft",
        "]" => "bracketright",
        "{" => "braceleft",
        "}" => "braceright",
        "`" => "grave",
        "<" => "less",
        ">" => "greater",
        "|" => "bar",
        f if f.starts_with('f') && f.len() <= 3 && f[1..].parse::<u8>().is_ok() => {
            return Some(f.to_ascii_uppercase())
        }
        _ => return None,
    }
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tk_to_gpui_basics() {
        assert_eq!(tk_to_gpui("<Escape>").as_deref(), Some("escape"));
        assert_eq!(tk_to_gpui("<d>").as_deref(), Some("d"));
        assert_eq!(tk_to_gpui("<space>").as_deref(), Some("space"));
        assert_eq!(tk_to_gpui("<Down>").as_deref(), Some("down"));
        assert_eq!(tk_to_gpui("<Right>").as_deref(), Some("right"));
        assert_eq!(tk_to_gpui("<Delete>").as_deref(), Some("delete"));
        assert_eq!(tk_to_gpui("<Control-c>").as_deref(), Some("ctrl-c"));
        assert_eq!(tk_to_gpui("<Control-z>").as_deref(), Some("ctrl-z"));
        assert_eq!(tk_to_gpui("<Control-Shift-1>").as_deref(), Some("ctrl-shift-1"));
        assert_eq!(
            tk_to_gpui("<Control-Alt-Shift-x>").as_deref(),
            Some("ctrl-alt-shift-x")
        );
        assert_eq!(tk_to_gpui("<q>").as_deref(), Some("q"));
        assert_eq!(tk_to_gpui("garbage-mods-x"), None);
    }

    #[test]
    fn gpui_to_tk_roundtrip() {
        assert_eq!(gpui_to_tk(false, false, false, false, "escape").as_deref(), Some("<Escape>"));
        assert_eq!(gpui_to_tk(false, false, false, false, "d").as_deref(), Some("<d>"));
        assert_eq!(gpui_to_tk(true, false, false, false, "c").as_deref(), Some("<Control-c>"));
        assert_eq!(gpui_to_tk(true, true, true, false, "x").as_deref(), Some("<Control-Alt-Shift-x>"));
        assert_eq!(gpui_to_tk(true, false, true, false, "1").as_deref(), Some("<Control-Shift-1>"));
    }

    #[test]
    fn config_merge_defaults() {
        let dir = std::env::temp_dir().join("at_config_test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"next_image":"<d>","crosshair_dash":[1,3],"weird_key":true}"#).unwrap();
        let config = Config::load_from(&path);
        assert_eq!(config.bindings.get("next_image").unwrap(), "<d>");
        assert_eq!(config.bindings.get("prev_image").unwrap(), "<Left>"); // default
        assert_eq!(config.gpui_binding("next_image").as_deref(), Some("d"));
        assert_eq!(config.crosshair_dash, (1.0, 3.0));
        assert!(config.raw.get("weird_key").is_some());
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn pretty_keystrokes() {
        assert_eq!(pretty_keystroke("ctrl-c"), "Ctrl+C");
        assert_eq!(pretty_keystroke("ctrl-shift-s"), "Ctrl+Shift+S");
        assert_eq!(pretty_keystroke("right"), "→");
        assert_eq!(pretty_keystroke("escape"), "Esc");
        assert_eq!(pretty_keystroke("ctrl--"), "Ctrl+-");
        assert_eq!(pretty_keystroke("delete"), "Delete");
    }
}
