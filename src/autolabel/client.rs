//! Minimal blocking client for an OpenAI-compatible endpoint (CLIProxyAPI).
//! Runs on worker threads only — never call it from the UI thread.

use std::time::Duration;

use serde_json::Value;

use super::settings::AutoLabelSettings;

const MAX_ATTEMPTS: u32 = 3;

pub struct Client {
    agent: ureq::Agent,
    base: String,
    api_key: String,
}

impl Client {
    pub fn new(settings: &AutoLabelSettings) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(5))
            .timeout_read(Duration::from_secs(settings.timeout_secs))
            .timeout_write(Duration::from_secs(60))
            .build();
        Self {
            agent,
            base: settings.base().to_string(),
            api_key: settings.api_key.trim().to_string(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base, path.trim_start_matches('/'))
    }

    fn authorize(&self, req: ureq::Request) -> ureq::Request {
        if self.api_key.is_empty() {
            req
        } else {
            req.set("Authorization", &format!("Bearer {}", self.api_key))
        }
    }

    /// `GET /models` → sorted model ids.
    pub fn list_models(&self) -> Result<Vec<String>, String> {
        let req = self.authorize(self.agent.get(&self.url("models")));
        let resp = req.call().map_err(|e| self.describe(e))?;
        let json: Value = resp
            .into_json()
            .map_err(|e| format!("Invalid /models response: {e}"))?;
        let mut ids: Vec<String> = json
            .get("data")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// `POST /chat/completions` → the assistant message text. Retries
    /// transient failures (connection errors, 429, 5xx) with backoff.
    pub fn chat(&self, body: &Value) -> Result<String, String> {
        let mut last_err = String::new();
        for attempt in 0..MAX_ATTEMPTS {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs(2u64.pow(attempt)));
            }
            let req = self.authorize(self.agent.post(&self.url("chat/completions")));
            match req.send_json(body.clone()) {
                Ok(resp) => {
                    let json: Value = resp
                        .into_json()
                        .map_err(|e| format!("Invalid completion response: {e}"))?;
                    return extract_message_text(&json);
                }
                Err(err) => {
                    let retry = match &err {
                        ureq::Error::Status(code, _) => *code == 429 || *code >= 500,
                        ureq::Error::Transport(_) => true,
                    };
                    last_err = self.describe(err);
                    if !retry {
                        break;
                    }
                }
            }
        }
        Err(last_err)
    }

    fn describe(&self, err: ureq::Error) -> String {
        match err {
            ureq::Error::Status(code, resp) => {
                let body = resp.into_string().unwrap_or_default();
                let body = truncate(body.trim(), 300);
                match code {
                    401 | 403 => format!(
                        "Unauthorized ({code}). Check the API key — it must match one of the api-keys in CLIProxyAPI's config.yaml. {body}"
                    ),
                    404 => format!("Not found (404) at {}. Is the endpoint URL right (it usually ends in /v1)? {body}", self.base),
                    _ => format!("HTTP {code}: {body}"),
                }
            }
            ureq::Error::Transport(t) => format!(
                "Could not reach {} — is CLIProxyAPI running? ({t})",
                self.base
            ),
        }
    }
}

/// Pulls the assistant text out of a chat-completions response. Content may
/// be a plain string or an array of `{type:"text", text}` parts.
pub fn extract_message_text(json: &Value) -> Result<String, String> {
    if let Some(err) = json.get("error") {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| err.to_string());
        return Err(format!("Endpoint error: {msg}"));
    }
    let message = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .ok_or_else(|| format!("No choices in response: {}", truncate(&json.to_string(), 200)))?;
    match message.get("content") {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Array(parts)) => Ok(parts
            .iter()
            .filter_map(|p| p.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("")),
        _ => {
            if let Some(refusal) = message.get("refusal").and_then(Value::as_str) {
                Err(format!("Model refused: {refusal}"))
            } else {
                Err("Empty model reply".to_string())
            }
        }
    }
}

pub fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max_chars).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    /// Tiny one-shot-per-connection HTTP stub: replies with `(status, body)`
    /// responses in order and records each request's path + headers + body.
    fn stub(responses: Vec<(u16, String)>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_thread = Arc::clone(&seen);
        std::thread::spawn(move || {
            for (status, body) in responses {
                let Ok((stream, _)) = listener.accept() else { return };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                let mut content_len = 0usize;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        content_len = v.trim().parse().unwrap_or(0);
                    }
                    head.push_str(&line);
                }
                let mut req_body = vec![0u8; content_len];
                reader.read_exact(&mut req_body).ok();
                head.push_str(&String::from_utf8_lossy(&req_body));
                seen_thread.lock().unwrap().push(head);
                let mut stream = stream;
                let reason = if status == 200 { "OK" } else { "ERR" };
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (format!("http://{addr}/v1"), seen)
    }

    fn settings(base: &str) -> AutoLabelSettings {
        AutoLabelSettings {
            base_url: base.to_string(),
            api_key: "secret".into(),
            model: "m".into(),
            timeout_secs: 10,
            ..Default::default()
        }
    }

    #[test]
    fn lists_models_with_bearer() {
        let (base, seen) = stub(vec![(200, r#"{"data":[{"id":"b"},{"id":"a"}]}"#.into())]);
        let models = Client::new(&settings(&base)).list_models().unwrap();
        assert_eq!(models, vec!["a", "b"]);
        let req = seen.lock().unwrap()[0].clone();
        assert!(req.starts_with("GET /v1/models"));
        assert!(req.to_ascii_lowercase().contains("authorization: bearer secret"));
    }

    #[test]
    fn chat_returns_content() {
        let (base, seen) = stub(vec![(
            200,
            r#"{"choices":[{"message":{"role":"assistant","content":"{\"boxes\":[]}"}}]}"#.into(),
        )]);
        let text = Client::new(&settings(&base))
            .chat(&serde_json::json!({"model":"m","messages":[]}))
            .unwrap();
        assert_eq!(text, r#"{"boxes":[]}"#);
        assert!(seen.lock().unwrap()[0].starts_with("POST /v1/chat/completions"));
    }

    #[test]
    fn unauthorized_is_not_retried() {
        let (base, seen) = stub(vec![(401, r#"{"error":"bad key"}"#.into())]);
        let err = Client::new(&settings(&base))
            .chat(&serde_json::json!({}))
            .unwrap_err();
        assert!(err.contains("Unauthorized"), "{err}");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn server_error_is_retried() {
        let ok = r#"{"choices":[{"message":{"content":"hi"}}]}"#.to_string();
        let (base, seen) = stub(vec![(500, "{}".into()), (200, ok)]);
        let text = Client::new(&settings(&base)).chat(&serde_json::json!({})).unwrap();
        assert_eq!(text, "hi");
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[test]
    fn unreachable_mentions_proxy() {
        // Bind then drop to get a port nothing listens on.
        let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let s = settings(&format!("http://127.0.0.1:{port}/v1"));
        let err = Client::new(&s).list_models().unwrap_err();
        assert!(err.contains("CLIProxyAPI"), "{err}");
    }

    #[test]
    fn extracts_array_content_and_errors() {
        let j = serde_json::json!({"choices":[{"message":{"content":[{"type":"text","text":"a"},{"type":"text","text":"b"}]}}]});
        assert_eq!(extract_message_text(&j).unwrap(), "ab");
        let j = serde_json::json!({"error":{"message":"nope"}});
        assert!(extract_message_text(&j).unwrap_err().contains("nope"));
    }
}
