use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::models::SettingsPayload;
use crate::services::model_warm;
use std::path::PathBuf;

use super::run_blocking;

#[tauri::command]
pub async fn get_settings(
    state: tauri::State<'_, SharedState>,
) -> Result<SettingsPayload, String> {
    run_blocking(state, |st| st.with_sqlite(|c| sqlite::load_settings(c))).await
}

#[tauri::command]
pub async fn update_settings(
    state: tauri::State<'_, SharedState>,
    payload: SettingsPayload,
) -> Result<(), String> {
    let st = (*state).clone();
    let prev = st
        .with_sqlite(|c| sqlite::load_settings(c))
        .map_err(|e| e.to_string())?;
    let index_embed = st
        .with_sqlite(|c| sqlite::get_setting(c, "index_embed_model"))
        .map_err(|e| e.to_string())?;
    let active = st
        .with_sqlite(|c| sqlite::count_active_documents(c))
        .map_err(|e| e.to_string())?;
    if let Some(ref indexed) = index_embed {
        if payload.embed_model != *indexed && active > 0 {
            return Err("Changing the embedding model requires rebuilding the index.".into());
        }
    }
    let chat_changed = payload.chat_model != prev.chat_model;
    let wake_background = (payload.extract_entities && !prev.extract_entities)
        || (payload.summarise_documents && !prev.summarise_documents);
    run_blocking(state, move |st| {
        st.with_sqlite(|c| {
            if let Some(ref v) = payload.vault_path {
                sqlite::upsert_setting(c, "vault_path", v)?;
            }
            sqlite::upsert_setting(c, "chat_model", &payload.chat_model)?;
            sqlite::upsert_setting(c, "embed_model", &payload.embed_model)?;
            sqlite::upsert_setting(c, "background_model", &payload.background_model)?;
            let summarise = if payload.summarise_documents {
                "true"
            } else {
                "false"
            };
            sqlite::upsert_setting(c, "summarise_documents", summarise)?;
            let extract = if payload.extract_entities {
                "true"
            } else {
                "false"
            };
            sqlite::upsert_setting(c, "extract_entities", extract)?;
            sqlite::upsert_setting(c, "extra_entity_labels", &payload.extra_entity_labels)?;
            Ok(())
        })?;
        if let Some(ref v) = payload.vault_path {
            st.notify_watcher(PathBuf::from(v));
        }
        Ok(())
    })
    .await?;
    if wake_background {
        st.notify_background_worker();
    }
    if chat_changed {
        st.clear_chat_warm();
        model_warm::spawn_warm_chat_model(st);
    }
    Ok(())
}
