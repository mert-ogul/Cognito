//! Background graph extraction: Ollama structured extract in Rust, DuckDB store via sidecar.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::CognitoResult;
use crate::events::{GRAPH_EXTRACTION_PROGRESS, LIBRARY_CHANGED};
use crate::models::GraphExtractionProgress;
use crate::services::graph_extract;
use serde_json::json;
use std::path::Path;
use std::sync::atomic::Ordering;
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

fn wait_while_chat_in_flight(
    state: &SharedState,
    document_id: &str,
) -> CognitoResult<bool> {
    // Returns Ok(false) when the document should stop (status left running or disabled).
    while state.chat_in_flight.load(Ordering::SeqCst) > 0 {
        let still = state.with_sqlite(|c| sqlite::document_graph_still_running(c, document_id))?;
        if !still {
            return Ok(false);
        }
        let enabled = state
            .with_sqlite(|c| sqlite::load_settings(c))?
            .graph_extraction_enabled;
        if !enabled {
            state.with_sqlite(|c| sqlite::mark_document_graph_pending(c, document_id))?;
            return Ok(false);
        }
        thread::sleep(Duration::from_millis(500));
    }
    Ok(true)
}

fn process_one_document(
    state: &SharedState,
    app: Option<&AppHandle>,
    document_id: &str,
) -> CognitoResult<()> {
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
        if !wait_while_chat_in_flight(state, document_id)? {
            return Ok(());
        }
        let detail = format!("chunk {} of {}", i + 1, total);
        emit_graph(app, None, document_id, "extract", Some(&detail));
        let text = state
            .with_sqlite(|c| sqlite::chunk_text_by_id(c, chunk_id))?
            .unwrap_or_default();
        let graph = match tauri::async_runtime::block_on(graph_extract::extract_chunk(
            &settings.chat_model,
            &text,
        )) {
            Ok(g) => g,
            Err(e) => {
                let msg = e.to_string();
                state.with_sqlite(|c| sqlite::mark_document_graph_failed(c, document_id, &msg))?;
                emit_graph(app, None, document_id, "failed", Some(&msg));
                if let Some(a) = app {
                    let _ = a.emit(LIBRARY_CHANGED, &());
                }
                return Ok(());
            }
        };
        let res = state.sidecar.with_session(|s| {
            s.request(
                "store_graph_chunk",
                json!({
                    "duckPath": sp(&duck_path),
                    "documentId": document_id,
                    "chunkId": chunk_id,
                    "entities": graph.entities,
                    "relations": graph.relations,
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
                        .unwrap_or("graph store failed");
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
        let still_after =
            state.with_sqlite(|c| sqlite::document_graph_still_running(c, document_id))?;
        if !still_after {
            sidecar_purge_graph(state, &duck_path, document_id)?;
            return Ok(());
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
        if let Err(e) = process_one_document(state, app, &doc_id) {
            let msg = e.to_string();
            let _ = state.with_sqlite(|c| sqlite::mark_document_graph_failed(c, &doc_id, &msg));
            emit_graph(app, None, &doc_id, "failed", Some(&msg));
            if let Some(a) = app {
                let _ = a.emit(LIBRARY_CHANGED, &());
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::state::AppState;
    use crate::sidecar::SidecarPool;
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn process_pending_marks_failed_when_purge_errors() {
        let data = tempfile::tempdir().expect("data tempdir");
        let pool = SidecarPool::new(
            PathBuf::from("/nonexistent/cognito-sidecar/main.py"),
            PathBuf::from("/nonexistent/cognito-sidecar"),
            data.path().join("sidecar.log"),
        );
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        let doc_id = "doc-purge-fail";
        state
            .with_sqlite(|c| {
                sqlite::insert_document_row(
                    c,
                    doc_id,
                    "/tmp/note.md",
                    "note",
                    "markdown",
                    "hash",
                    None,
                    "indexed",
                    None,
                )?;
                c.execute(
                    "UPDATE documents SET graph_status = 'pending' WHERE id = ?1",
                    [doc_id],
                )
                .map_err(|e| crate::errors::CognitoError::Database(e.to_string()))?;
                Ok(())
            })
            .expect("seed document");

        let n = process_pending(&state, None);
        assert_eq!(n, 1);

        let (status, err): (String, Option<String>) = state
            .with_sqlite(|c| {
                c.query_row(
                    "SELECT graph_status, graph_error FROM documents WHERE id = ?1",
                    [doc_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| crate::errors::CognitoError::Database(e.to_string()))
            })
            .expect("read status");
        assert_eq!(status, "failed");
        assert!(err.as_ref().map(|e| !e.is_empty()).unwrap_or(false));
    }
}
