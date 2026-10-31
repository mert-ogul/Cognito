use crate::app::state::SharedState;
use crate::events::CHAT_FINAL;
use crate::models::{ChatFinalResult, ChatTurn};
use crate::services::chat_query;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter};

struct ChatInFlightGuard(SharedState);

impl Drop for ChatInFlightGuard {
    fn drop(&mut self) {
        self.0.chat_in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

#[tauri::command]
pub async fn send_chat_query(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    query: String,
    message_id: String,
    history: Option<Vec<ChatTurn>>,
    scope_ids: Option<Vec<String>>,
) -> Result<String, String> {
    if message_id.trim().is_empty() {
        return Err("message_id must not be empty".into());
    }
    let st = state.inner().clone();
    st.chat_in_flight.fetch_add(1, Ordering::SeqCst);
    let app_c = app.clone();
    let mid = message_id.clone();
    let hist = history.unwrap_or_default();
    let scopes = scope_ids.unwrap_or_default();
    tauri::async_runtime::spawn(async move {
        let _guard = ChatInFlightGuard(st.clone());
        if let Err(e) = chat_query::run_chat_query(
            st,
            app_c.clone(),
            query,
            mid.clone(),
            hist,
            scopes,
        )
        .await
        {
            let _ = app_c.emit(CHAT_FINAL, &ChatFinalResult::failure(mid, e.to_string()));
        }
    });
    Ok(message_id)
}

#[tauri::command]
pub async fn send_related_query(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    entity_id_a: String,
    entity_id_b: String,
    message_id: String,
) -> Result<String, String> {
    if message_id.trim().is_empty() {
        return Err("message_id must not be empty".into());
    }
    if entity_id_a.trim().is_empty() || entity_id_b.trim().is_empty() {
        return Err("entity ids must not be empty".into());
    }
    let st = state.inner().clone();
    st.chat_in_flight.fetch_add(1, Ordering::SeqCst);
    let app_c = app.clone();
    let mid = message_id.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = ChatInFlightGuard(st.clone());
        if let Err(e) = chat_query::run_related_query(
            st,
            app_c.clone(),
            entity_id_a,
            entity_id_b,
            mid.clone(),
        )
        .await
        {
            let _ = app_c.emit(CHAT_FINAL, &ChatFinalResult::failure(mid, e.to_string()));
        }
    });
    Ok(message_id)
}
