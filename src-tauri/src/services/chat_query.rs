//! Orchestrate sidecar retrieval + Ollama streaming for `send_chat_query`.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::CognitoResult;
use crate::events::{CHAT_FINAL, CHAT_STREAM};
use crate::models::{ChatFinalResult, ChatStreamEvent, CitationItem};
use crate::services::ollama_stream;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

fn sp(path: &std::path::Path) -> String {
    path.to_string_lossy().into_owned()
}

pub async fn run_chat_query(
    state: SharedState,
    app: AppHandle,
    query: String,
    message_id: String,
) -> CognitoResult<()> {
    let sqlite_path = state.sqlite_path();
    let duck_path = state.duckdb_path();
    let settings = state.with_sqlite(|c| sqlite::load_settings(c))?;
    let prep = match state.sidecar.with_session(|s| {
        s.request(
            "prepare_chat_retrieval",
            json!({
                "sqlite_path": sp(&sqlite_path),
                "duckdb_path": sp(&duck_path),
                "query": query,
                "embed_model": settings.embed_model,
            }),
        )
    }) {
        Ok(p) => p,
        Err(e) => {
            let _ = app.emit(
                CHAT_FINAL,
                &ChatFinalResult {
                    message_id: message_id.clone(),
                    citations: vec![],
                    retrieval_mode: "error".into(),
                    error: Some(e.to_string()),
                },
            );
            return Ok(());
        }
    };
    let messages = prep
        .get("messages")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();
    let retrieval_mode = prep
        .get("retrievalMode")
        .and_then(|x| x.as_str())
        .unwrap_or("vector_only")
        .to_string();
    let citations_raw = prep.get("citations").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    let citations: Vec<CitationItem> = citations_raw
        .into_iter()
        .filter_map(|v| serde_json::from_value(v).ok())
        .collect();
    let msgs: Vec<Value> = messages.into_iter().collect();
    let mid = message_id.clone();
    let stream_res = ollama_stream::stream_chat_completion(&settings.chat_model, msgs, |tok| {
        let _ = app.emit(
            CHAT_STREAM,
            &ChatStreamEvent {
                message_id: mid.clone(),
                token: tok.to_string(),
            },
        );
    })
    .await;
    if let Err(e) = stream_res {
        let _ = app.emit(
            CHAT_FINAL,
            &ChatFinalResult {
                message_id: message_id.clone(),
                citations: citations.clone(),
                retrieval_mode: retrieval_mode.clone(),
                error: Some(e),
            },
        );
        return Ok(());
    }
    let _ = app.emit(
        CHAT_FINAL,
        &ChatFinalResult {
            message_id,
            citations,
            retrieval_mode,
            error: None,
        },
    );
    Ok(())
}
