//! Chat completions from a local Ollama `/api/chat` endpoint.

use crate::errors::CognitoError;
use futures_util::StreamExt;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

/// Context window for every chat-model request. Keep in sync with sidecar `CHAT_CONTEXT_TOKENS`.
pub const CHAT_CONTEXT_TOKENS: u64 = 8192;

pub const ANSWER_TIMEOUT: Duration = Duration::from_secs(240);
pub const CARD_TIMEOUT: Duration = Duration::from_secs(300);
pub const UNLOAD_TIMEOUT: Duration = Duration::from_secs(10);

/// Test hook: increments when `unload_model` runs.
pub static UNLOAD_CALLS: AtomicUsize = AtomicUsize::new(0);

static HTTP: OnceLock<reqwest::Client> = OnceLock::new();

fn http_client() -> &'static reqwest::Client {
    HTTP.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest client")
    })
}

pub(crate) fn ollama_base_url() -> String {
    let raw = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
    if raw.starts_with("http://") || raw.starts_with("https://") {
        raw
    } else {
        format!("http://{raw}")
    }
}

/// Parse a complete JSON string at the start of `s` (leading `"`). Returns value and bytes consumed.
pub fn parse_json_string_prefix(s: &str) -> Option<(String, usize)> {
    let bytes = s.as_bytes();
    if bytes.first() != Some(&b'"') {
        return None;
    }
    let mut i = 1usize;
    let mut escaped = false;
    while i < bytes.len() {
        let b = bytes[i];
        if escaped {
            escaped = false;
            i += 1;
            continue;
        }
        if b == b'\\' {
            escaped = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            let raw = &s[..=i];
            let parsed: String = serde_json::from_str(raw).ok()?;
            return Some((parsed, i + 1));
        }
        i += 1;
    }
    None
}

/// Complete `"sentence"` string values found so far in a partial JSON object buffer.
pub fn draft_sentences_from_partial(buf: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut search_from = 0usize;
    let key = "\"sentence\"";
    while let Some(rel) = buf[search_from..].find(key) {
        let key_at = search_from + rel;
        let after_key = key_at + key.len();
        let trimmed = buf[after_key..].trim_start();
        let colon_skip = buf[after_key..].len() - trimmed.len();
        if !trimmed.starts_with(':') {
            search_from = after_key;
            continue;
        }
        let after_colon = trimmed[1..].trim_start();
        let value_skip = trimmed[1..].len() - after_colon.len();
        match parse_json_string_prefix(after_colon) {
            Some((value, consumed)) => {
                out.push(value);
                search_from = after_key + colon_skip + 1 + value_skip + consumed;
            }
            None => break,
        }
    }
    out
}

async fn chat_completion_json_once(
    model: &str,
    messages: Vec<Value>,
    format_schema: Value,
    timeout: Duration,
) -> Result<String, String> {
    let url = format!("{}/api/chat", ollama_base_url().trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": false,
        "think": false,
        "format": format_schema,
        "options": {
            "temperature": 0,
            "num_ctx": CHAT_CONTEXT_TOKENS,
        },
    });
    let res = http_client()
        .post(&url)
        .timeout(timeout)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ollama request failed: {}. Is Ollama running?", e))?;
    let status = res.status();
    if !status.is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(format!(
            "Ollama returned {}: {}",
            status,
            t.chars().take(200).collect::<String>()
        ));
    }
    let v: Value = res
        .json()
        .await
        .map_err(|e| format!("Ollama response parse: {e}"))?;
    v.get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Ollama response missing message.content".to_string())
}

/// Card generation: abort when `interrupt` becomes true (checked every 250 ms).
pub async fn chat_completion_json_interruptible(
    model: &str,
    messages: Vec<Value>,
    format_schema: Value,
    interrupt: impl Fn() -> bool,
) -> Result<Result<String, String>, ()> {
    let request = chat_completion_json_once(model, messages, format_schema, CARD_TIMEOUT);
    tokio::pin!(request);
    loop {
        tokio::select! {
            res = &mut request => {
                return Ok(res);
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if interrupt() {
                    return Err(());
                }
            }
        }
    }
}

/// Streaming chat with a JSON `format` schema. Invokes `on_partial` with the accumulated content.
pub async fn chat_completion_json_streaming(
    model: &str,
    messages: Vec<Value>,
    format_schema: Value,
    mut on_partial: impl FnMut(&str),
) -> Result<String, String> {
    let url = format!("{}/api/chat", ollama_base_url().trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
        "think": false,
        "format": format_schema,
        "options": {
            "temperature": 0,
            "num_ctx": CHAT_CONTEXT_TOKENS,
        },
    });
    let res = http_client()
        .post(&url)
        .timeout(ANSWER_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ollama request failed: {}. Is Ollama running?", e))?;
    let status = res.status();
    if !status.is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(format!(
            "Ollama returned {}: {}",
            status,
            t.chars().take(200).collect::<String>()
        ));
    }
    let mut stream = res.bytes_stream();
    let mut line_buf = String::new();
    let mut acc = String::new();
    while let Some(item) = stream.next().await {
        let chunk = item.map_err(|e| format!("Ollama stream: {e}"))?;
        line_buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = line_buf.find('\n') {
            let line = line_buf[..pos].trim_end_matches('\r').to_string();
            line_buf.drain(..=pos);
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: Value =
                serde_json::from_str(line).map_err(|e| format!("Ollama stream JSON: {e}"))?;
            if let Some(c) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                acc.push_str(c);
                on_partial(&acc);
            }
            if v.get("done").and_then(|d| d.as_bool()).unwrap_or(false) {
                return Ok(acc);
            }
        }
    }
    if !acc.is_empty() {
        return Ok(acc);
    }
    Err("Ollama stream ended without content".into())
}

/// Ask Ollama to drop a model from memory (`keep_alive: 0`).
pub async fn unload_model(model: &str) -> Result<(), String> {
    UNLOAD_CALLS.fetch_add(1, Ordering::SeqCst);
    if std::env::var("COGNITO_MOCK_LLM").ok().as_deref() == Some("1") {
        return Ok(());
    }
    let url = format!("{}/api/generate", ollama_base_url().trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "keep_alive": 0,
        "prompt": "",
        "stream": false,
    });
    let res = http_client()
        .post(&url)
        .timeout(UNLOAD_TIMEOUT)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Ollama unload failed: {e}"))?;
    let status = res.status();
    if !status.is_success() {
        let t = res.text().await.unwrap_or_default();
        return Err(format!(
            "Ollama unload returned {}: {}",
            status,
            t.chars().take(200).collect::<String>()
        ));
    }
    Ok(())
}

pub fn unload_model_blocking(model: &str) {
    let _ = tauri::async_runtime::block_on(unload_model(model));
}

/// True when the error looks like Ollama is down rather than a model/content failure.
pub fn ollama_unreachable(err: &CognitoError) -> bool {
    let s = err.to_string().to_lowercase();
    s.contains("error sending request")
        || s.contains("connection refused")
        || s.contains("connect error")
        || s.contains("tcp connect")
        || s.contains("dns error")
        || s.contains("failed to connect")
        || (s.contains("timed out") && s.contains("ollama"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ollama_unreachable_detects_connection_errors() {
        assert!(ollama_unreachable(&CognitoError::Sidecar(
            "Ollama request failed: error sending request for url".into()
        )));
        assert!(ollama_unreachable(&CognitoError::Sidecar(
            "connection refused".into()
        )));
        assert!(!ollama_unreachable(&CognitoError::Sidecar(
            "JSON: expected value".into()
        )));
    }

    #[test]
    fn draft_scanner_handles_escaped_quotes() {
        let buf = r#"{"answerable":true,"answer":[{"quotes":[{"source":1,"text":"hello"}],"sentence":"She said \"hello\"."}"#;
        let draft = draft_sentences_from_partial(buf);
        assert_eq!(draft, vec![r#"She said "hello"."#]);
    }

    #[test]
    fn draft_scanner_handles_latex_backslashes() {
        let buf = r#"{"answer":[{"quotes":[{"source":1,"text":"E=mc2"}],"sentence":"Energy $E=mc^{2}$ holds."}]}"#;
        let draft = draft_sentences_from_partial(buf);
        assert_eq!(draft, vec!["Energy $E=mc^{2}$ holds."]);
    }

    #[test]
    fn draft_scanner_sentence_split_across_chunks() {
        let a = r#"{"answer":[{"quotes":[{"source":1,"text":"a"}],"sentence":"First half "#;
        assert!(draft_sentences_from_partial(a).is_empty());
        let b = r#"{"answer":[{"quotes":[{"source":1,"text":"a"}],"sentence":"First half continues here."},{"quotes":[{"source":2,"text":"b"}],"sentence":"Second."#;
        let draft = draft_sentences_from_partial(b);
        assert_eq!(draft, vec!["First half continues here."]);
        let c = r#"{"answer":[{"quotes":[{"source":1,"text":"a"}],"sentence":"First half continues here."},{"quotes":[{"source":2,"text":"b"}],"sentence":"Second done."}]}"#;
        assert_eq!(
            draft_sentences_from_partial(c),
            vec!["First half continues here.", "Second done."]
        );
    }
}
