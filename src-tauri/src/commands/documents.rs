use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::events::{IMPORT_PROGRESS, LIBRARY_CHANGED};
use crate::models::{DocumentPreview, DocumentRow, ImportProgress};
use crate::services::import_job;
use crate::services::index_rebuild;
use crate::services::ingest;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use super::run_blocking;

#[tauri::command]
pub async fn list_documents(
    state: tauri::State<'_, SharedState>,
) -> Result<Vec<DocumentRow>, String> {
    run_blocking(state, |st| st.with_sqlite(|c| sqlite::list_documents(c))).await
}

#[tauri::command]
pub async fn get_document_preview(
    state: tauri::State<'_, SharedState>,
    document_id: String,
    highlight_snippet: Option<String>,
) -> Result<DocumentPreview, String> {
    run_blocking(state, move |st| {
        let text = st.with_sqlite(|c| sqlite::document_preview_text(c, &document_id))?;
        Ok(DocumentPreview {
            document_id,
            text,
            highlight_snippet,
        })
    })
    .await
}

pub(crate) fn spawn_import_job(
    app: AppHandle,
    state: &SharedState,
    job_id: String,
    vault: PathBuf,
    paths: Vec<PathBuf>,
    embed: String,
    chat: String,
) -> Result<(), String> {
    if state.import_running.swap(true, Ordering::SeqCst) {
        return Err("An import is already running.".to_string());
    }
    let st: SharedState = state.clone();
    let app_c = app.clone();
    let jid = job_id.clone();
    std::thread::spawn(move || {
        let res = ingest::run_import_job(
            st.clone(),
            Some(app_c.clone()),
            jid.clone(),
            vault,
            paths,
            embed,
            chat,
        );
        if let Err(ref e) = res {
            let _ = st.with_sqlite(|c| import_job::complete_fail(c, &jid, &e.to_string()));
            let _ = app_c.emit(
                IMPORT_PROGRESS,
                &ImportProgress {
                    job_id: jid.clone(),
                    status: "failed".into(),
                    total_files: 0,
                    completed_files: 0,
                    failed_files: 0,
                    phase: "error".into(),
                    path: None,
                    error_message: Some(e.to_string()),
                },
            );
            let _ = app_c.emit(LIBRARY_CHANGED, &());
        }
        st.import_running.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[tauri::command]
pub async fn import_documents(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    folder: Option<String>,
) -> Result<String, String> {
    let job_id = Uuid::new_v4().to_string();
    let settings = state
        .with_sqlite(|c| sqlite::load_settings(c))
        .map_err(|e| e.to_string())?;
    let vault_str = settings
        .vault_path
        .clone()
        .ok_or_else(|| "Configure a vault folder first.".to_string())?;
    let vault = PathBuf::from(&vault_str);
    let import_root = if let Some(ref f) = folder {
        let fp = PathBuf::from(f);
        crate::vault::ensure_folder_in_vault(&vault, &fp).map_err(|e| e.to_string())?
    } else {
        vault.clone()
    };
    let active = state
        .with_sqlite(|c| sqlite::count_active_documents(c))
        .map_err(|e| e.to_string())?;
    let slots = crate::vault::slots_remaining(active);
    if slots == 0 {
        return Err("Maximum of 100 active documents reached.".to_string());
    }
    let paths = crate::vault::collect_import_paths_bounded(&import_root, slots)
        .map_err(|e| e.to_string())?;
    if paths.is_empty() {
        return Err("No PDF or Markdown files found.".to_string());
    }
    spawn_import_job(
        app,
        &*state,
        job_id.clone(),
        vault,
        paths,
        settings.embed_model.clone(),
        settings.chat_model.clone(),
    )?;
    Ok(job_id)
}

#[tauri::command]
pub async fn retry_failed_import(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
) -> Result<String, String> {
    let job_id = Uuid::new_v4().to_string();
    let settings = state
        .with_sqlite(|c| sqlite::load_settings(c))
        .map_err(|e| e.to_string())?;
    let vault_str = settings
        .vault_path
        .clone()
        .ok_or_else(|| "Configure a vault folder first.".to_string())?;
    let vault = PathBuf::from(&vault_str);
    let graph_n = state
        .with_sqlite(|c| sqlite::requeue_failed_graphs(c))
        .map_err(|e| e.to_string())?;
    let paths_str = state
        .with_sqlite(|c| sqlite::failed_document_paths(c, 100))
        .map_err(|e| e.to_string())?;
    if paths_str.is_empty() && graph_n == 0 {
        return Err("No failed documents to retry.".to_string());
    }
    if graph_n > 0 {
        state.notify_graph_worker();
    }
    if !paths_str.is_empty() {
        let paths: Vec<PathBuf> = paths_str.iter().map(PathBuf::from).collect();
        spawn_import_job(
            app,
            &*state,
            job_id.clone(),
            vault,
            paths,
            settings.embed_model.clone(),
            settings.chat_model.clone(),
        )?;
    }
    Ok(job_id)
}

#[tauri::command]
pub async fn rebuild_index(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    embed_model: Option<String>,
) -> Result<String, String> {
    if state.import_running.load(Ordering::SeqCst) {
        return Err("An import is already running.".to_string());
    }
    if let Some(ref model) = embed_model {
        state
            .with_sqlite(|c| sqlite::upsert_setting(c, "embed_model", model))
            .map_err(|e| e.to_string())?;
    }
    index_rebuild::reset_library(&*state).map_err(|e| e.to_string())?;
    let settings = state
        .with_sqlite(|c| sqlite::load_settings(c))
        .map_err(|e| e.to_string())?;
    let vault_str = settings
        .vault_path
        .clone()
        .ok_or_else(|| "Configure a vault folder first.".to_string())?;
    let vault = PathBuf::from(&vault_str);
    let slots = crate::vault::slots_remaining(0);
    let paths = crate::vault::collect_import_paths_bounded(&vault, slots)
        .map_err(|e| e.to_string())?;
    if paths.is_empty() {
        return Err("No PDF or Markdown files found.".to_string());
    }
    let job_id = Uuid::new_v4().to_string();
    spawn_import_job(
        app,
        &*state,
        job_id.clone(),
        vault,
        paths,
        settings.embed_model.clone(),
        settings.chat_model.clone(),
    )?;
    Ok(job_id)
}
