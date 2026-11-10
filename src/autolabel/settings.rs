//! Auto Label settings, stored as one nested `"auto_label"` object inside
//! config.json (`Config.raw`), so the Python tool simply ignores it.

use serde_json::{Map, Value};

pub const CONFIG_KEY: &str = "auto_label";

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:8317/v1";

/// `reasoning_effort` choices as `(value, label)`; "" = don't send it (the
/// model's default). Codex models accept none/low/medium/high/xhigh/max;
/// an unsupported level comes back as an error naming the valid ones.
pub const REASONING_LEVELS: [(&str, &str); 7] = [
    ("", "Default"),
    ("none", "None"),
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("xhigh", "Extra high"),
    ("max", "Max"),
];

pub fn reasoning_label(value: &str) -> String {
    REASONING_LEVELS
        .iter()
        .find(|(v, _)| *v == value)
        .map(|(_, l)| l.to_string())
        .unwrap_or_else(|| value.to_string())
}

/// Whether a listed model can label images (drops image-generation,
/// embedding, audio and review-only models).
pub fn is_labeling_model(id: &str) -> bool {
    let id = id.to_ascii_lowercase();
    !["image", "embedding", "tts", "whisper", "transcribe", "dall-e", "auto-review", "moderation"]
        .iter()
        .any(|bad| id.contains(bad))
}

#[derive(Debug, Clone, PartialEq)]
pub struct AutoLabelSettings {
    /// OpenAI-compatible base URL (CLIProxyAPI: `http://127.0.0.1:8317/v1`).
    pub base_url: String,
    /// Bearer token; must match one of CLIProxyAPI's `api-keys`.
    pub api_key: String,
    /// Model id as listed by `GET /models`.
    pub model: String,
    /// Optional `reasoning_effort` ("low" / "medium" / "high"); empty = omit.
    pub reasoning_effort: String,
    /// Parallel requests during a batch run.
    pub concurrency: usize,
    /// Images are downscaled so their longest side is at most this (px).
    pub max_side: u32,
    /// Example images sent per target class.
    pub max_examples_per_class: usize,
    /// Per-request read timeout (seconds).
    pub timeout_secs: u64,
    /// Start CLIProxyAPI with the app and stop it on exit.
    pub proxy_managed: bool,
    /// Path to cli-proxy-api(.exe); empty = look in `<app home>/cliproxy/`, the app home, PATH.
    pub proxy_exe: String,
    /// Custom CLIProxyAPI config.yaml; empty = the app writes its own.
    pub proxy_config: String,
}

impl Default for AutoLabelSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: String::new(),
            model: String::new(),
            reasoning_effort: String::new(),
            concurrency: 3,
            max_side: 1280,
            max_examples_per_class: 3,
            timeout_secs: 180,
            proxy_managed: false,
            proxy_exe: String::new(),
            proxy_config: String::new(),
        }
    }
}

impl AutoLabelSettings {
    pub fn from_config(raw: &Map<String, Value>) -> Self {
        let mut s = Self::default();
        let Some(Value::Object(obj)) = raw.get(CONFIG_KEY) else {
            return s;
        };
        let get_str = |key: &str| obj.get(key).and_then(Value::as_str).map(|v| v.trim().to_string());
        let get_u64 = |key: &str| obj.get(key).and_then(Value::as_u64);
        if let Some(v) = get_str("base_url").filter(|v| !v.is_empty()) {
            s.base_url = v;
        }
        if let Some(v) = get_str("api_key") {
            s.api_key = v;
        }
        if let Some(v) = get_str("model") {
            s.model = v;
        }
        if let Some(v) = get_str("reasoning_effort") {
            s.reasoning_effort = v;
        }
        if let Some(v) = get_u64("concurrency") {
            s.concurrency = (v as usize).clamp(1, 16);
        }
        if let Some(v) = get_u64("max_side") {
            s.max_side = (v as u32).clamp(256, 4096);
        }
        if let Some(v) = get_u64("max_examples_per_class") {
            s.max_examples_per_class = (v as usize).clamp(0, 10);
        }
        if let Some(v) = get_u64("timeout_secs") {
            s.timeout_secs = v.clamp(10, 900);
        }
        if let Some(v) = obj.get("proxy_managed").and_then(Value::as_bool) {
            s.proxy_managed = v;
        }
        if let Some(v) = get_str("proxy_exe") {
            s.proxy_exe = v;
        }
        if let Some(v) = get_str("proxy_config") {
            s.proxy_config = v;
        }
        s
    }

    pub fn to_value(&self) -> Value {
        let mut m = Map::new();
        m.insert("base_url".into(), Value::String(self.base_url.clone()));
        m.insert("api_key".into(), Value::String(self.api_key.clone()));
        m.insert("model".into(), Value::String(self.model.clone()));
        m.insert("reasoning_effort".into(), Value::String(self.reasoning_effort.clone()));
        m.insert("concurrency".into(), Value::from(self.concurrency));
        m.insert("max_side".into(), Value::from(self.max_side));
        m.insert("max_examples_per_class".into(), Value::from(self.max_examples_per_class));
        m.insert("timeout_secs".into(), Value::from(self.timeout_secs));
        m.insert("proxy_managed".into(), Value::Bool(self.proxy_managed));
        m.insert("proxy_exe".into(), Value::String(self.proxy_exe.clone()));
        m.insert("proxy_config".into(), Value::String(self.proxy_config.clone()));
        Value::Object(m)
    }

    /// Writes these settings into a raw config map (caller saves the file).
    pub fn store(&self, raw: &mut Map<String, Value>) {
        raw.insert(CONFIG_KEY.into(), self.to_value());
    }

    /// `base_url` without a trailing slash.
    pub fn base(&self) -> &str {
        self.base_url.trim().trim_end_matches('/')
    }

    /// Why a request can't be made yet, if anything is missing.
    pub fn missing(&self) -> Option<&'static str> {
        if self.base().is_empty() {
            Some("Set the endpoint URL in Settings → Auto Label.")
        } else if self.model.trim().is_empty() {
            Some("Pick a model in Settings → Auto Label (use Test connection to list them).")
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_missing() {
        let s = AutoLabelSettings::from_config(&Map::new());
        assert_eq!(s, AutoLabelSettings::default());
        assert_eq!(s.base(), "http://127.0.0.1:8317/v1");
    }

    #[test]
    fn round_trip_and_clamp() {
        let mut s = AutoLabelSettings::default();
        s.model = "gpt-x".into();
        s.api_key = "k".into();
        s.base_url = "http://localhost:1/v1/".into();
        s.proxy_managed = true;
        s.proxy_exe = "C:/tools/cli-proxy-api.exe".into();
        let mut raw = Map::new();
        s.store(&mut raw);
        let back = AutoLabelSettings::from_config(&raw);
        assert_eq!(back, s);
        assert_eq!(back.base(), "http://localhost:1/v1");

        let mut obj = Map::new();
        obj.insert("concurrency".into(), Value::from(999));
        obj.insert("max_side".into(), Value::from(1));
        let mut raw = Map::new();
        raw.insert(CONFIG_KEY.into(), Value::Object(obj));
        let s = AutoLabelSettings::from_config(&raw);
        assert_eq!(s.concurrency, 16);
        assert_eq!(s.max_side, 256);
    }

    #[test]
    fn filters_non_labeling_models() {
        assert!(is_labeling_model("gpt-6.1-terra"));
        assert!(is_labeling_model("gpt-6-chat-thinking"));
        assert!(!is_labeling_model("gpt-image-2.6"));
        assert!(!is_labeling_model("codex-auto-review"));
        assert_eq!(reasoning_label("xhigh"), "Extra high");
        assert_eq!(reasoning_label(""), "Default");
        assert_eq!(reasoning_label("weird"), "weird");
    }

    #[test]
    fn missing_reports_model() {
        let s = AutoLabelSettings::default();
        assert!(s.missing().is_some());
        let s = AutoLabelSettings { model: "m".into(), ..Default::default() };
        assert!(s.missing().is_none());
    }
}
