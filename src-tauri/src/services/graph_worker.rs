//! Background graph extraction: one sidecar call per chunk.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::CognitoResult;
use crate::events::{GRAPH_EXTRACTION_PROGRESS, LIBRARY_CHANGED};
use crate::models::GraphExtractionProgress;
use serde_json::json;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

fn sp(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

pub(crate) fn sidecar_purge_graph(
    state: &SharedState,
    duck_path: &Path,
    document_id: &str,
) -> CognitoResult<()> {
    state.sidecar.with_session(|s| {
        s.request(
            "purge_graph_document",
            json!({ "duckPath": sp(duck_path), "documentId": document_id }),
        )?;
        Ok(())
    })
}

pub(crate) fn emit_graph(
    app: Option<&AppHandle>,
    job_id: Option<&str>,
    document_id: &str,
    phase: &str,
    detail: Option<&str>,
) {
    let Some(a) = app else {
        return;
    };
    let _ = a.emit(
        GRAPH_EXTRACTION_PROGRESS,
        &GraphExtractionProgress {
            job_id: job_id.map(String::from),
            document_id: document_id.to_string(),
            phase: phase.to_string(),
            detail: detail.map(String::from),
        },
    );
}

fn process_one_document(
    state: &SharedState,
    app: Option<&AppHandle>,
    document_id: &str,
) -> CognitoResult<()> {
    let sqlite_path = state.sqlite_path();
    let duck_path = state.duckdb_path();
    let settings = state.with_sqlite(|c| sqlite::load_settings(c))?;
    if !settings.graph_extraction_enabled {
        return Ok(());
    }
    state.with_sqlite(|c| sqlite::mark_document_graph_running(c, document_id))?;
    sidecar_purge_graph(state, &duck_path, document_id)?;
    let chunk_ids = state.with_sqlite(|c| sqlite::list_chunk_ids_for_document(c, document_id))?;
    let total = chunk_ids.len();
    for (i, chunk_id) in chunk_ids.iter().enumerate() {
        let still = state.with_sqlite(|c| sqlite::document_graph_still_running(c, document_id))?;
        if !still {
            return Ok(());
        }
        let enabled = state
            .with_sqlite(|c| sqlite::load_settings(c))?
            .graph_extraction_enabled;
        if !enabled {
            state.with_sqlite(|c| sqlite::mark_document_graph_pending(c, document_id))?;
            return Ok(());
        }
        let detail = format!("chunk {} of {}", i + 1, total);
        emit_graph(app, None, document_id, "extract", Some(&detail));
        let res = state.sidecar.with_session(|s| {
            s.request(
                "extract_graph_chunk",
                json!({
                    "sqlitePath": sp(&sqlite_path),
                    "duckPath": sp(&duck_path),
                    "documentId": document_id,
                    "chunkId": chunk_id,
                    "chatModel": settings.chat_model,
                }),
            )
        });
        match res {
            Ok(v) => {
                let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
                if !ok {
                    let err = v
                        .get("error")
                        .and_then(|x| x.as_str())
                        .unwrap_or("graph extraction failed");
                    state.with_sqlite(|c| sqlite::mark_document_graph_failed(c, document_id, err))?;
                    emit_graph(app, None, document_id, "failed", Some(err));
                    if let Some(a) = app {
                        let _ = a.emit(LIBRARY_CHANGED, &());
                    }
                    return Ok(());
                }
            }
            Err(e) => {
                let msg = e.to_string();
                state.with_sqlite(|c| sqlite::mark_document_graph_failed(c, document_id, &msg))?;
                emit_graph(app, None, document_id, "failed", Some(&msg));
                if let Some(a) = app {
                    let _ = a.emit(LIBRARY_CHANGED, &());
                }
                return Ok(());
            }
        }
    }
    state.with_sqlite(|c| sqlite::mark_document_graph_ok(c, document_id))?;
    emit_graph(app, None, document_id, "ok", None);
    if let Some(a) = app {
        let _ = a.emit(LIBRARY_CHANGED, &());
    }
    Ok(())
}

/// Processes every pending graph document; returns how many were processed.
pub fn process_pending(state: &SharedState, app: Option<&AppHandle>) -> usize {
    let mut n = 0usize;
    loop {
        let enabled = state
            .with_sqlite(|c| sqlite::load_settings(c))
            .map(|s| s.graph_extraction_enabled)
            .unwrap_or(true);
        if !enabled {
            break;
        }
        let next = state
            .with_sqlite(|c| sqlite::next_pending_graph_document(c))
            .ok()
            .flatten();
        let Some(doc_id) = next else {
            break;
        };
        let _ = process_one_document(state, app, &doc_id);
        n += 1;
    }
    n
}

pub fn spawn(state: SharedState, app: AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    {
        let mut g = state.graph_tx.lock().expect("graph_tx lock");
        *g = Some(tx);
    }
    let _ = state.with_sqlite(|c| sqlite::reset_running_graph_jobs(c));
    thread::spawn(move || worker_loop(state, app, rx));
}

fn worker_loop(state: SharedState, app: AppHandle, rx: Receiver<()>) {
    loop {
        match rx.recv_timeout(Duration::from_secs(30)) {
            Ok(()) | Err(RecvTimeoutError::Timeout) => {
                while process_pending(&state, Some(&app)) > 0 {}
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}
