use crate::app::state::SharedState;
use crate::models::AppHealth;
use crate::services::health_snapshot;
use serde_json::{json, Value};

use super::run_blocking;

#[tauri::command]
pub async fn get_app_health(state: tauri::State<'_, SharedState>) -> Result<AppHealth, String> {
    run_blocking(state, |st| health_snapshot::read_app_health(&st)).await
}

#[tauri::command]
pub async fn ping_sidecar(state: tauri::State<'_, SharedState>) -> Result<Value, String> {
    run_blocking(state, |st| {
        st.sidecar.with_session(|s| s.request("ping", json!({})))
    })
    .await
}
