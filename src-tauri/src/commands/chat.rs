use crate::app::state::SharedState;
use crate::events::CHAT_FINAL;
use crate::models::{ChatFinalResult, CitationItem};
use crate::services::chat_query;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

#[tauri::command]
pub async fn send_chat_query(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    query: String,
) -> Result<String, String> {
    let message_id = Uuid::new_v4().to_string();
    let st = state.inner().clone();
    let app_c = app.clone();
    let mid = message_id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = chat_query::run_chat_query(st, app_c.clone(), query, mid.clone()).await {
            let _ = app_c.emit(
                CHAT_FINAL,
                &ChatFinalResult {
                    message_id: mid,
                    citations: Vec::<CitationItem>::new(),
                    retrieval_mode: "error".into(),
                    error: Some(e.to_string()),
                },
            );
        }
    });
    Ok(message_id)
}
