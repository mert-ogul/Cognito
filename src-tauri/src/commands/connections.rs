//! Connections / entities read APIs and rebuild.

use crate::app::state::SharedState;
use crate::db::sqlite;

use super::run_blocking;

#[tauri::command]
pub async fn rebuild_connections(
    state: tauri::State<'_, SharedState>,
) -> Result<(), String> {
    let st = (*state).clone();
    run_blocking(state, move |st| {
        st.with_sqlite(|c| sqlite::requeue_all_entities(c))?;
        Ok(())
    })
    .await?;
    st.notify_background_worker();
    Ok(())
}

#[tauri::command]
pub async fn get_entities_index(
    state: tauri::State<'_, SharedState>,
) -> Result<Vec<serde_json::Value>, String> {
    run_blocking(state, |st| {
        st.with_sqlite(|c| sqlite::list_entities_index(c))
    })
    .await
}

#[tauri::command]
pub async fn get_entity_detail(
    state: tauri::State<'_, SharedState>,
    entity_id: String,
) -> Result<Option<serde_json::Value>, String> {
    run_blocking(state, move |st| {
        st.with_sqlite(|c| sqlite::entity_detail(c, &entity_id))
    })
    .await
}

#[tauri::command]
pub async fn get_related_documents(
    state: tauri::State<'_, SharedState>,
    document_id: String,
) -> Result<Vec<serde_json::Value>, String> {
    run_blocking(state, move |st| {
        st.with_sqlite(|c| sqlite::list_related_documents(c, &document_id))
    })
    .await
}

#[tauri::command]
pub async fn get_connections_timeline(
    state: tauri::State<'_, SharedState>,
) -> Result<serde_json::Value, String> {
    run_blocking(state, |st| {
        st.with_sqlite(|c| sqlite::connections_timeline(c))
    })
    .await
}
