//! Cognito desktop shell: Tauri commands, local databases, Python sidecar bridge.

mod app;
mod commands;
mod db;
mod errors;
mod events;
mod models;
mod services;
mod sidecar;
mod sidecar_paths;
pub mod vault;

use app::state::{AppState, SharedState};
use db::sqlite;
use services::background_worker;
use services::file_watcher;
use services::model_warm;
use sidecar::SidecarPool;
use sidecar_paths::resolve_sidecar_paths;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
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
            let pool = SidecarPool::new(main_py, cwd, log_path.clone(), data_dir.clone());
            let state: SharedState = Arc::new(AppState::new(data_dir.clone(), pool));
            state
                .with_sqlite(|_| Ok(()))
                .map_err(|e| e.to_string())?;
            for name in ["graph.duckdb", "graph.duckdb-wal", "graph.duckdb-shm"] {
                let p = data_dir.join(name);
                if p.exists() {
                    let _ = std::fs::remove_file(&p);
                }
            }
            if let Err(e) = state.sidecar.ensure() {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&log_path)
                {
                    let _ = writeln!(f, "sidecar startup failed: {e}");
                    let _ = f.flush();
                }
            }
            let (reload_tx, reload_rx) = mpsc::channel::<Option<PathBuf>>();
            {
                let mut wg = state
                    .watcher_tx
                    .lock()
                    .map_err(|e| format!("watcher lock: {}", e))?;
                *wg = Some(reload_tx.clone());
            }
            file_watcher::spawn_background(state.clone(), app.handle().clone(), reload_rx);
            if let Ok(s) = state.with_sqlite(|c| sqlite::load_settings(c)) {
                if let Some(vp) = s.vault_path {
                    let _ = reload_tx.send(Some(PathBuf::from(vp)));
                }
            }
            background_worker::spawn(state.clone(), app.handle().clone());
            commands::maybe_rebuild_stale_index_format(app.handle().clone(), state.clone());
            app.manage(state.clone());
            model_warm::spawn_warm_chat_model(state.clone());
            model_warm::spawn_warm_checking_models(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::select_vault_folder,
            commands::set_vault,
            commands::clear_library,
            commands::get_app_health,
            commands::warm_chat_model,
            commands::open_logs_folder,
            commands::list_documents,
            commands::get_document_preview,
            commands::render_page_region,
            commands::render_document_page,
            commands::get_document_card,
            commands::get_suggested_questions,
            commands::open_document,
            commands::send_chat_query,
            commands::send_related_query,
            commands::prepare_models,
            commands::get_models_status,
            commands::import_documents,
            commands::retry_failed_import,
            commands::rebuild_index,
            commands::get_settings,
            commands::update_settings,
            commands::rebuild_connections,
            commands::get_entities_index,
            commands::get_entity_detail,
            commands::get_related_documents,
            commands::get_connections_timeline,
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
