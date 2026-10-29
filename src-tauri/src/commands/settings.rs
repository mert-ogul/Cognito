use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::models::SettingsPayload;
use crate::services::model_warm;
use std::path::PathBuf;
use tauri::AppHandle;

use super::run_blocking;

#[tauri::command]
pub async fn get_settings(
    state: tauri::State<'_, SharedState>,
) -> Result<SettingsPayload, String> {
    run_blocking(state, |st| st.with_sqlite(|c| sqlite::load_settings(c))).await
}

#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    payload: SettingsPayload,
) -> Result<(), String> {
    let st = (*state).clone();
    let old_chat = st
        .with_sqlite(|c| sqlite::load_settings(c))
        .map(|s| s.chat_model)
        .unwrap_or_default();
    let chat_changed = payload.chat_model != old_chat;
    run_blocking(state, move |st| {
        st.with_sqlite(|c| {
            if let Some(ref v) = payload.vault_path {
                sqlite::upsert_setting(c, "vault_path", v)?;
            }
            sqlite::upsert_setting(c, "chat_model", &payload.chat_model)?;
            sqlite::upsert_setting(c, "embed_model", &payload.embed_model)?;
            let tel = if payload.telemetry_opt_in {
                "true"
            } else {
                "false"
            };
            sqlite::upsert_setting(c, "telemetry_opt_in", tel)?;
            let crash = if payload.crash_reporting_opt_in {
                "true"
            } else {
                "false"
            };
            sqlite::upsert_setting(c, "crash_reporting_opt_in", crash)?;
            Ok(())
        })?;
        if let Some(ref v) = payload.vault_path {
            st.notify_watcher(PathBuf::from(v));
        }
        Ok(())
    })
    .await?;
    if chat_changed {
        st.clear_chat_warm();
        model_warm::spawn_warm_chat_model(st, app);
    }
    Ok(())
}
