//! Stream chat completions from a local Ollama `/api/chat` endpoint (NDJSON lines).

use futures_util::StreamExt;
use serde_json::Value;

pub(crate) fn ollama_base_url() -> String {
    let raw = std::env::var("OLLAMA_HOST").unwrap_or_else(|_| "http://127.0.0.1:11434".to_string());
    if raw.starts_with("http://") || raw.starts_with("https://") {
        raw
    } else {
        format!("http://{raw}")
    }
}

pub async fn stream_chat_completion(
    model: &str,
    messages: Vec<Value>,
    mut on_token: impl FnMut(&str),
) -> Result<(), String> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/chat", ollama_base_url().trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": messages,
        "stream": true,
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
    let mut stream = res.bytes_stream();
    let mut buf = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| e.to_string())?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(pos) = buf.find('\n') {
            let line: String = buf[..pos].trim().to_string();
            buf = buf[pos + 1..].to_string();
            if line.is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(&line).map_err(|e| format!("Ollama stream parse: {e}"))?;
            if v.get("done").and_then(|x| x.as_bool()).unwrap_or(false) {
                break;
            }
            if let Some(content) = v
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
            {
                if !content.is_empty() {
                    on_token(content);
                }
            }
        }
    }
    Ok(())
}
