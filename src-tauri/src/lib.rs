//! Cognito desktop shell: Tauri commands, local databases, Python sidecar bridge.

mod app;
mod commands;
mod db;
mod errors;
#[allow(dead_code)]
mod events;
mod models;
mod services;
mod sidecar;
mod sidecar_paths;
pub mod vault;

use app::state::{AppState, SharedState};
use db::sqlite;
use services::file_watcher;
use services::model_warm;
use sidecar::SidecarPool;
use sidecar_paths::resolve_sidecar_paths;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let resolver = app.path();
            let data_dir = resolver
                .app_data_dir()
                .map_err(|e| format!("app_data_dir: {}", e))?;
            std::fs::create_dir_all(&data_dir).map_err(|e| e.to_string())?;
            let (main_py, cwd) = resolve_sidecar_paths(app.handle()).map_err(|e| e.to_string())?;
            let logs_dir = data_dir.join("logs");
            std::fs::create_dir_all(&logs_dir).map_err(|e| e.to_string())?;
            let log_path = logs_dir.join("sidecar.log");
            let pool = SidecarPool::new(main_py, cwd, log_path);
            let state: SharedState = Arc::new(AppState::new(data_dir, pool));
            state
                .with_sqlite(|_| Ok(()))
                .map_err(|e| e.to_string())?;
            state.sidecar.ensure().map_err(|e| e.to_string())?;
            {
                let st = state.clone();
                std::thread::spawn(move || {
                    let duck = st.duckdb_path();
                    let ok = st
                        .sidecar
                        .with_session(|s| {
                            s.request(
                                "init_graph_store",
                                json!({ "duckdb_path": duck.to_string_lossy() }),
                            )
                        })
                        .is_ok();
                    st.graph_store_ready.store(ok, Ordering::SeqCst);
                });
            }
            let (reload_tx, reload_rx) = mpsc::channel::<PathBuf>();
            {
                let mut wg = state
                    .watcher_tx
                    .lock()
                    .map_err(|e| format!("watcher lock: {}", e))?;
                *wg = Some(reload_tx.clone());
            }
            file_watcher::spawn_background(state.clone(), reload_rx);
            if let Ok(s) = state.with_sqlite(|c| sqlite::load_settings(c)) {
                if let Some(vp) = s.vault_path {
                    let _ = reload_tx.send(PathBuf::from(vp));
                }
            }
            app.manage(state.clone());
            model_warm::spawn_warm_chat_model(state, app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::select_vault_folder,
            commands::get_app_health,
            commands::ping_sidecar,
            commands::warm_chat_model,
            commands::list_documents,
            commands::get_document_preview,
            commands::send_chat_query,
            commands::import_documents,
            commands::retry_failed_import,
            commands::get_settings,
            commands::update_settings,
            commands::search_graph,
            commands::get_graph_snapshot,
            commands::get_graph_node_detail,
        ]);
    let app = builder
        .build(tauri::generate_context!())
        .expect("error while building cognito");
    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = app_handle.try_state::<SharedState>() {
                state.sidecar.shutdown();
            }
        }
    });
}
