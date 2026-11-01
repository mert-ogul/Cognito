use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::events::LIBRARY_CHANGED;
use crate::services::library;
use crate::vault::ensure_vault_path;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};
use tauri_plugin_dialog::DialogExt;
use uuid::Uuid;

use super::documents::{spawn_import_thread, try_lock_import};

#[tauri::command]
pub async fn select_vault_folder(app: AppHandle) -> Result<Option<String>, String> {
    let handle = app.clone();
    let picked =
        tauri::async_runtime::spawn_blocking(move || handle.dialog().file().blocking_pick_folder())
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
    let guard = try_lock_import(&state)?;
    let paths = library::switch_vault(&state, &path).map_err(|e| e.to_string())?;
    state.notify_watcher(PathBuf::from(&path));
    let _ = app.emit(LIBRARY_CHANGED, &());
    if paths.is_empty() {
        return Ok(None);
    }
    let settings = state
        .with_sqlite(sqlite::load_settings)
        .map_err(|e| e.to_string())?;
    let job_id = Uuid::new_v4().to_string();
    spawn_import_thread(
        app,
        guard,
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
    let _guard = try_lock_import(&state)?;
    library::forget(&state).map_err(|e| e.to_string())?;
    state.stop_watcher();
    let _ = app.emit(LIBRARY_CHANGED, &());
    Ok(())
}
