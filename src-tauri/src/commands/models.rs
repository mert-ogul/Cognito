use crate::app::state::SharedState;
use crate::events::MODELS_PROGRESS;
use crate::models::{ModelsProgressEvent, ModelsStatus};
use serde_json::json;
use tauri::{AppHandle, Emitter};

fn app_data_string(state: &SharedState) -> String {
    state.data_dir.to_string_lossy().into_owned()
}

fn parse_models_status(v: serde_json::Value) -> Result<ModelsStatus, String> {
    serde_json::from_value(v).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn prepare_models(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
) -> Result<ModelsStatus, String> {
    let st = state.inner().clone();
    let app_c = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let app_data = app_data_string(&st);
        let _ = app_c.emit(
            MODELS_PROGRESS,
            &ModelsProgressEvent {
                line: "Downloading checking models…".into(),
            },
        );
        let thorough = st
            .with_sqlite(|c| crate::db::sqlite::load_settings(c))
            .map(|s| s.thorough_reranking)
            .unwrap_or(false);
        st.sidecar
            .with_session(|s| {
                s.request(
                    "download_models",
                    json!({
                        "app_data": app_data,
                        "thorough_reranking": thorough,
                    }),
                )
            })
            .map_err(|e| e.to_string())?;
        let _ = app_c.emit(
            MODELS_PROGRESS,
            &ModelsProgressEvent {
                line: "Ready".into(),
            },
        );
        let _ = st.sidecar.with_session(|s| {
            s.request(
                "warm_checking_models",
                json!({
                    "app_data": app_data_string(&st),
                    "thorough_reranking": thorough,
                }),
            )
        });
        let status = st
            .sidecar
            .with_session(|s| {
                s.request(
                    "models_status",
                    json!({
                        "app_data": app_data_string(&st),
                        "thorough_reranking": thorough,
                    }),
                )
            })
            .map_err(|e| e.to_string())?;
        parse_models_status(status)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_models_status(
    state: tauri::State<'_, SharedState>,
) -> Result<ModelsStatus, String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let thorough = st
            .with_sqlite(|c| crate::db::sqlite::load_settings(c))
            .map(|s| s.thorough_reranking)
            .unwrap_or(false);
        let status = st
            .sidecar
            .with_session(|s| {
                s.request(
                    "models_status",
                    json!({
                        "app_data": app_data_string(&st),
                        "thorough_reranking": thorough,
                    }),
                )
            })
            .map_err(|e| e.to_string())?;
        parse_models_status(status)
    })
    .await
    .map_err(|e| e.to_string())?
}
