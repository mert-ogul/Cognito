use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::vault::ensure_vault_path;
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
pub async fn select_vault_folder(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
) -> Result<Option<String>, String> {
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
    state
        .with_sqlite(|c| sqlite::upsert_setting(c, "vault_path", &path))
        .map_err(|e| e.to_string())?;
    state.notify_watcher(std::path::PathBuf::from(&path));
    Ok(Some(path))
}
