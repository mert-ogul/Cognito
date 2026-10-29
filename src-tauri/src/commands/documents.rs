use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::events::{IMPORT_PROGRESS, LIBRARY_CHANGED};
use crate::models::{DocumentPreview, DocumentRow, ImportProgress};
use crate::services::import_job;
use crate::services::ingest;
use crate::services::library;
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
    chunk_id: Option<String>,
) -> Result<DocumentPreview, String> {
    run_blocking(state, move |st| {
        if let Some(ref cid) = chunk_id {
            let win = st.with_sqlite(|c| sqlite::chunk_window(c, &document_id, cid))?;
            if let Some(w) = win {
                return Ok(DocumentPreview {
                    document_id,
                    text: Some(w.text),
                    highlight_snippet: Some(w.highlight),
                    page_start: w.page_start,
                    page_end: w.page_end,
                });
            }
        }
        let text = st.with_sqlite(|c| sqlite::document_preview_text(c, &document_id))?;
        Ok(DocumentPreview {
            document_id,
            text,
            highlight_snippet: None,
            page_start: None,
            page_end: None,
        })
    })
    .await
}

#[tauri::command]
pub async fn open_document(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    document_id: String,
) -> Result<(), String> {
    let path = state
        .with_sqlite(|c| sqlite::document_path_by_id(c, &document_id))
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Document not found.".to_string())?;
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub(crate) fn try_lock_import(state: &SharedState) -> Result<(), String> {
    if state.import_running.swap(true, Ordering::SeqCst) {
        return Err("An import is already running.".to_string());
    }
    Ok(())
}

pub(crate) fn release_import(state: &SharedState) {
    state.import_running.store(false, Ordering::SeqCst);
}

/// Assumes the import lock is held; releases it when the thread ends.
pub(crate) fn spawn_import_thread(
    app: AppHandle,
    state: &SharedState,
    job_id: String,
    vault: PathBuf,
    paths: Vec<PathBuf>,
    embed: String,
    chat: String,
) {
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
        st.index_format_updating.store(false, Ordering::SeqCst);
        st.import_running.store(false, Ordering::SeqCst);
    });
}

/// If documents exist and `index_format_version` is behind, rebuild like Settings → Rebuild index.
pub(crate) fn maybe_rebuild_stale_index_format(app: AppHandle, state: SharedState) {
    std::thread::spawn(move || {
        let _ = state.sidecar.ensure();
        let need = match state.with_sqlite(|c| {
            let n = sqlite::count_active_documents(c)?;
            if n == 0 {
                return Ok(false);
            }
            let stored = sqlite::get_setting(c, "index_format_version")?
                .and_then(|s| s.parse::<i32>().ok())
                .unwrap_or(0);
            Ok(stored < sqlite::INDEX_FORMAT_VERSION)
        }) {
            Ok(v) => v,
            Err(_) => return,
        };
        if !need {
            return;
        }
        if try_lock_import(&state).is_err() {
            return;
        }
        state.index_format_updating.store(true, Ordering::SeqCst);
        let (vault, paths) = match library::plan_rebuild(&state) {
            Ok(v) => v,
            Err(_) => {
                state.index_format_updating.store(false, Ordering::SeqCst);
                release_import(&state);
                return;
            }
        };
        if library::reset(&state).is_err() {
            state.index_format_updating.store(false, Ordering::SeqCst);
            release_import(&state);
            return;
        }
        let settings = match state.with_sqlite(|c| sqlite::load_settings(c)) {
            Ok(s) => s,
            Err(_) => {
                state.index_format_updating.store(false, Ordering::SeqCst);
                release_import(&state);
                return;
            }
        };
        let job_id = Uuid::new_v4().to_string();
        spawn_import_thread(
            app,
            &state,
            job_id,
            vault,
            paths,
            settings.embed_model.clone(),
            settings.chat_model.clone(),
        );
    });
}

#[tauri::command]
pub async fn import_documents(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    folder: Option<String>,
) -> Result<String, String> {
    try_lock_import(&*state)?;
    let job_id = Uuid::new_v4().to_string();
    let settings = match state.with_sqlite(|c| sqlite::load_settings(c)) {
        Ok(s) => s,
        Err(e) => {
            release_import(&*state);
            return Err(e.to_string());
        }
    };
    let vault_str = match settings.vault_path.clone() {
        Some(v) => v,
        None => {
            release_import(&*state);
            return Err("Configure a vault folder first.".to_string());
        }
    };
    let vault = PathBuf::from(&vault_str);
    let import_root = if let Some(ref f) = folder {
        let fp = PathBuf::from(f);
        match crate::vault::ensure_folder_in_vault(&vault, &fp) {
            Ok(p) => p,
            Err(e) => {
                release_import(&*state);
                return Err(e.to_string());
            }
        }
    } else {
        vault.clone()
    };
    let active = match state.with_sqlite(|c| sqlite::count_active_documents(c)) {
        Ok(n) => n,
        Err(e) => {
            release_import(&*state);
            return Err(e.to_string());
        }
    };
    let slots = crate::vault::slots_remaining(active);
    if slots == 0 {
        release_import(&*state);
        return Err("Maximum of 100 active documents reached.".to_string());
    }
    let paths = match crate::vault::collect_import_paths_bounded(&import_root, slots) {
        Ok(p) => p,
        Err(e) => {
            release_import(&*state);
            return Err(e.to_string());
        }
    };
    if paths.is_empty() {
        release_import(&*state);
        return Err("No PDF or Markdown files found.".to_string());
    }
    spawn_import_thread(
        app,
        &*state,
        job_id.clone(),
        vault,
        paths,
        settings.embed_model.clone(),
        settings.chat_model.clone(),
    );
    Ok(job_id)
}

#[tauri::command]
pub async fn retry_failed_import(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
) -> Result<String, String> {
    try_lock_import(&*state)?;
    let job_id = Uuid::new_v4().to_string();
    let settings = match state.with_sqlite(|c| sqlite::load_settings(c)) {
        Ok(s) => s,
        Err(e) => {
            release_import(&*state);
            return Err(e.to_string());
        }
    };
    let vault_str = match settings.vault_path.clone() {
        Some(v) => v,
        None => {
            release_import(&*state);
            return Err("Configure a vault folder first.".to_string());
        }
    };
    let vault = PathBuf::from(&vault_str);
    let graph_n = match state.with_sqlite(|c| sqlite::requeue_failed_graphs(c)) {
        Ok(n) => n,
        Err(e) => {
            release_import(&*state);
            return Err(e.to_string());
        }
    };
    let paths_str = match state.with_sqlite(|c| sqlite::failed_document_paths(c, 100)) {
        Ok(p) => p,
        Err(e) => {
            release_import(&*state);
            return Err(e.to_string());
        }
    };
    if paths_str.is_empty() && graph_n == 0 {
        release_import(&*state);
        return Err("No failed documents to retry.".to_string());
    }
    if graph_n > 0 {
        state.notify_graph_worker();
    }
    if !paths_str.is_empty() {
        let paths: Vec<PathBuf> = paths_str.iter().map(PathBuf::from).collect();
        spawn_import_thread(
            app,
            &*state,
            job_id.clone(),
            vault,
            paths,
            settings.embed_model.clone(),
            settings.chat_model.clone(),
        );
    } else {
        release_import(&*state);
    }
    Ok(job_id)
}

#[tauri::command]
pub async fn rebuild_index(
    app: AppHandle,
    state: tauri::State<'_, SharedState>,
    embed_model: Option<String>,
) -> Result<String, String> {
    try_lock_import(&*state)?;
    let (vault, paths) = match library::plan_rebuild(&*state) {
        Ok(v) => v,
        Err(e) => {
            release_import(&*state);
            return Err(e.to_string());
        }
    };
    if let Some(ref model) = embed_model {
        if let Err(e) = state.with_sqlite(|c| sqlite::upsert_setting(c, "embed_model", model)) {
            release_import(&*state);
            return Err(e.to_string());
        }
    }
    if let Err(e) = library::reset(&*state) {
        release_import(&*state);
        return Err(e.to_string());
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
        vault,
        paths,
        settings.embed_model.clone(),
        settings.chat_model.clone(),
    );
    Ok(job_id)
}
