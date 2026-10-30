//! Chat completions from a local Ollama `/api/chat` endpoint.

use crate::errors::CognitoError;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Context window for every chat-model request. Keep in sync with sidecar `CHAT_CONTEXT_TOKENS`.
pub const CHAT_CONTEXT_TOKENS: u64 = 8192;

/// Test hook: increments when `unload_model` runs.
pub static UNLOAD_CALLS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn ollama_base_url() -> String {
    let raw = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
    if raw.starts_with("http://") || raw.starts_with("https://") {
        raw
    } else {
        format!("http://{raw}")
    }
}

/// Non-streaming chat with a JSON `format` schema. Returns `message.content`.
pub async fn chat_completion_json(
    model: &str,
    messages: Vec<Value>,
    format_schema: Value,
) -> Result<String, String> {
    let client = reqwest::Client::new();
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
    let res = client
        .post(&url)
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

/// Ask Ollama to drop a model from memory (`keep_alive: 0`).
pub async fn unload_model(model: &str) -> Result<(), String> {
    UNLOAD_CALLS.fetch_add(1, Ordering::SeqCst);
    if std::env::var("COGNITO_MOCK_LLM").ok().as_deref() == Some("1") {
        return Ok(());
    }
    let client = reqwest::Client::new();
    let url = format!("{}/api/generate", ollama_base_url().trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "keep_alive": 0,
        "prompt": "",
        "stream": false,
    });
    let res = client
        .post(&url)
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
}
