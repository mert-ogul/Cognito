use crate::app::state::SharedState;
use crate::models::AppHealth;
use crate::services::health_snapshot;
use serde_json::{json, Value};

#[tauri::command]
pub fn get_app_health(state: tauri::State<'_, SharedState>) -> Result<AppHealth, String> {
    health_snapshot::read_app_health(&state).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn ping_sidecar(state: tauri::State<'_, SharedState>) -> Result<Value, String> {
    state
        .sidecar
        .with_session(|s| s.request("ping", json!({})))
        .map_err(|e| e.to_string())
}
