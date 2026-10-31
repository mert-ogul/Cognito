use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::models::{ChatFinalResult, ChatTurn};
use crate::services::chat_query;
use std::sync::atomic::Ordering;
use tauri::AppHandle;
use uuid::Uuid;

struct ChatInFlightGuard(SharedState);

impl Drop for ChatInFlightGuard {
    fn drop(&mut self) {
        self.0.chat_in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

fn require_conversation(state: &SharedState, conversation_id: &str) -> Result<(), String> {
    if conversation_id.trim().is_empty() {
        return Err("conversation_id must not be empty".into());
    }
    let exists = state
        .with_sqlite(|c| sqlite::conversations::conversation_exists(c, conversation_id))
        .map_err(|e| e.to_string())?;
    if !exists {
        return Err("Unknown conversation.".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn send_chat_query(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    query: String,
    message_id: String,
    conversation_id: String,
    history: Option<Vec<ChatTurn>>,
    scope_ids: Option<Vec<String>>,
) -> Result<String, String> {
    if message_id.trim().is_empty() {
        return Err("message_id must not be empty".into());
    }
    require_conversation(state.inner(), &conversation_id)?;
    let user_id = Uuid::new_v4().to_string();
    state
        .with_sqlite(|c| {
            sqlite::conversations::append_turn(
                c,
                &conversation_id,
                &user_id,
                &query,
                &message_id,
            )
        })
        .map_err(|e| e.to_string())?;
    let st = state.inner().clone();
    st.chat_in_flight.fetch_add(1, Ordering::SeqCst);
    let app_c = app.clone();
    let mid = message_id.clone();
    let cid = conversation_id.clone();
    let hist = history.unwrap_or_default();
    let scopes = scope_ids.unwrap_or_default();
    tauri::async_runtime::spawn(async move {
        let _guard = ChatInFlightGuard(st.clone());
        if let Err(e) = chat_query::run_chat_query(
            st.clone(),
            app_c.clone(),
            query,
            mid.clone(),
            cid.clone(),
            hist,
            scopes,
        )
        .await
        {
            chat_query::emit_final(
                &st,
                &app_c,
                &ChatFinalResult::failure(mid, cid, e.to_string()),
            );
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
    conversation_id: String,
) -> Result<String, String> {
    if message_id.trim().is_empty() {
        return Err("message_id must not be empty".into());
    }
    if entity_id_a.trim().is_empty() || entity_id_b.trim().is_empty() {
        return Err("entity ids must not be empty".into());
    }
    require_conversation(state.inner(), &conversation_id)?;
    let (name_a, name_b) = state
        .with_sqlite(|c| sqlite::entity_names(c, &entity_id_a, &entity_id_b))
        .map_err(|e| e.to_string())?;
    let query = format!("How are {name_a} and {name_b} related?");
    let user_id = Uuid::new_v4().to_string();
    state
        .with_sqlite(|c| {
            sqlite::conversations::append_turn(
                c,
                &conversation_id,
                &user_id,
                &query,
                &message_id,
            )
        })
        .map_err(|e| e.to_string())?;
    let st = state.inner().clone();
    st.chat_in_flight.fetch_add(1, Ordering::SeqCst);
    let app_c = app.clone();
    let mid = message_id.clone();
    let cid = conversation_id.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = ChatInFlightGuard(st.clone());
        if let Err(e) = chat_query::run_related_query(
            st.clone(),
            app_c.clone(),
            entity_id_a,
            entity_id_b,
            mid.clone(),
            cid.clone(),
        )
        .await
        {
            chat_query::emit_final(
                &st,
                &app_c,
                &ChatFinalResult::failure(mid, cid, e.to_string()),
            );
        }
    });
    Ok(message_id)
}

#[tauri::command]
pub async fn list_conversations(
    state: tauri::State<'_, SharedState>,
) -> Result<Vec<sqlite::conversations::ConversationListItem>, String> {
    super::run_blocking(state, |st| {
        st.with_sqlite(|c| sqlite::conversations::list_conversations(c))
    })
    .await
}

#[tauri::command]
pub async fn get_conversation(
    state: tauri::State<'_, SharedState>,
    id: String,
) -> Result<Option<sqlite::conversations::ConversationDetail>, String> {
    super::run_blocking(state, move |st| {
        st.with_sqlite(|c| sqlite::conversations::get_conversation(c, &id))
    })
    .await
}

#[tauri::command]
pub async fn create_conversation(
    state: tauri::State<'_, SharedState>,
    title: String,
) -> Result<sqlite::conversations::ConversationListItem, String> {
    super::run_blocking(state, move |st| {
        st.with_sqlite(|c| sqlite::conversations::create_conversation(c, &title))
    })
    .await
}

#[tauri::command]
pub async fn delete_conversation(
    state: tauri::State<'_, SharedState>,
    id: String,
) -> Result<(), String> {
    super::run_blocking(state, move |st| {
        st.with_sqlite(|c| sqlite::conversations::delete_conversation(c, &id))
    })
    .await
}
