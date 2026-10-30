use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::events::LIBRARY_CHANGED;
use crate::services::library;
use crate::vault::ensure_vault_path;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

use super::documents::{release_import, spawn_import_thread, try_lock_import};

#[tauri::command]
pub async fn select_vault_folder(app: AppHandle) -> Result<Option<String>, String> {
    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        handle.dialog().file().blocking_pick_folder()
    })
    .await
    .map_err(|e| e.to_string())?;
    let Some(file) = picked else {
        return Ok(None);
    };
    let path = file
        .into_path()
        .map_err(|e| format!("path: {:?}", e))?
        .to_string_lossy()
        .to_string();
    ensure_vault_path(&path).map_err(|e| e.to_string())?;
    Ok(Some(path))
}

#[tauri::command]
pub async fn set_vault(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    path: String,
) -> Result<Option<String>, String> {
    try_lock_import(&*state)?;
    let paths = match library::switch_vault(&*state, &path) {
        Ok(p) => p,
        Err(e) => {
            release_import(&*state);
            return Err(e.to_string());
        }
    };
    state.notify_watcher(PathBuf::from(&path));
    let _ = app.emit(LIBRARY_CHANGED, &());
    if paths.is_empty() {
        release_import(&*state);
        return Ok(None);
    }
    let settings = match state.with_sqlite(|c| sqlite::load_settings(c)) {
        Ok(s) => s,
        Err(e) => {
            release_import(&*state);
            return Err(e.to_string());
        }
    };
    let job_id = Uuid::new_v4().to_string();
    spawn_import_thread(
        app,
        &*state,
        job_id.clone(),
        PathBuf::from(&path),
        paths,
        settings.embed_model,
        settings.chat_model,
    );
    Ok(Some(job_id))
}

#[tauri::command]
pub async fn clear_library(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
) -> Result<(), String> {
    try_lock_import(&*state)?;
    if let Err(e) = library::forget(&*state) {
        release_import(&*state);
        return Err(e.to_string());
    }
    state.stop_watcher();
    release_import(&*state);
    let _ = app.emit(LIBRARY_CHANGED, &());
    Ok(())
}
