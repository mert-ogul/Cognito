use crate::app::state::SharedState;
use crate::models::AppHealth;
use crate::services::health_snapshot;
use crate::services::model_warm;
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub async fn get_app_health(state: tauri::State<'_, SharedState>) -> Result<AppHealth, String> {
    let st = (*state).clone();
    health_snapshot::read_app_health(st)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn warm_chat_model(
    state: tauri::State<'_, SharedState>,
) -> Result<(), String> {
    if state.chat_warm_ready().is_none() {
        return Ok(());
    }
    state.clear_chat_warm();
    model_warm::spawn_warm_chat_model((*state).clone());
    Ok(())
}

#[tauri::command]
pub async fn open_logs_folder(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
) -> Result<(), String> {
    let logs = state.data_dir.join("logs");
    std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(logs.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(())
}
