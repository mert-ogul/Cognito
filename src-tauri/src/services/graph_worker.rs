//! Background graph extraction: Ollama structured extract in Rust, DuckDB store via sidecar.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::{CognitoError, CognitoResult};
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

enum DocOutcome {
    Finished,
    /// Ollama unreachable — leave pending and stop the worker loop.
    Pause,
}

fn process_one_document(
    state: &SharedState,
    app: Option<&AppHandle>,
    document_id: &str,
) -> CognitoResult<DocOutcome> {
    let duck_path = state.duckdb_path();
    let settings = state.with_sqlite(|c| sqlite::load_settings(c))?;
    if !settings.graph_extraction_enabled {
        return Ok(DocOutcome::Finished);
    }
    state.with_sqlite(|c| sqlite::mark_document_graph_running(c, document_id))?;
    sidecar_purge_graph(state, &duck_path, document_id)?;
    let chunks = state.with_sqlite(|c| sqlite::list_chunks_for_graph(c, document_id))?;
    let body_chunks: Vec<_> = chunks
        .iter()
        .filter(|c| c.role != "references")
        .collect();
    let total = body_chunks.len();
    let mut skipped = 0usize;
    let mut stored = 0usize;

    for (i, chunk) in body_chunks.iter().enumerate() {
        let still = state.with_sqlite(|c| sqlite::document_graph_still_running(c, document_id))?;
        if !still {
            return Ok(DocOutcome::Finished);
        }
        let enabled = state
            .with_sqlite(|c| sqlite::load_settings(c))?
            .graph_extraction_enabled;
        if !enabled {
            state.with_sqlite(|c| sqlite::mark_document_graph_pending(c, document_id))?;
            return Ok(DocOutcome::Finished);
        }
        if !wait_while_chat_in_flight(state, document_id)? {
            return Ok(DocOutcome::Finished);
        }
        let detail = format!("chunk {} of {}", i + 1, total);
        emit_graph(app, None, document_id, "extract", Some(&detail));
        let graph = match tauri::async_runtime::block_on(graph_extract::extract_chunk(
            &settings.chat_model,
            &chunk.text,
        )) {
            Ok(g) => g,
            Err(e) => {
                if graph_extract::ollama_unreachable(&e) {
                    state.with_sqlite(|c| sqlite::mark_document_graph_pending(c, document_id))?;
                    emit_graph(app, None, document_id, "pending", Some(&e.to_string()));
                    if let Some(a) = app {
                        let _ = a.emit(LIBRARY_CHANGED, &());
                    }
                    return Ok(DocOutcome::Pause);
                }
                skipped += 1;
                continue;
            }
        };
        let res = state.sidecar.with_session(|s| {
            s.request(
                "store_graph_chunk",
                json!({
                    "duckPath": sp(&duck_path),
                    "documentId": document_id,
                    "chunkId": chunk.id,
                    "entities": graph.entities,
                    "relations": graph.relations,
                }),
            )
        });
        match res {
            Ok(v) => {
                let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
                if !ok {
                    skipped += 1;
                    continue;
                }
                stored += 1;
            }
            Err(_) => {
                skipped += 1;
                continue;
            }
        }
        let still_after =
            state.with_sqlite(|c| sqlite::document_graph_still_running(c, document_id))?;
        if !still_after {
            sidecar_purge_graph(state, &duck_path, document_id)?;
            return Ok(DocOutcome::Finished);
        }
    }

    if total > 0 && stored == 0 && skipped == total {
        let msg = format!("{skipped} of {total} passages were skipped");
        state.with_sqlite(|c| sqlite::mark_document_graph_failed(c, document_id, &msg))?;
        emit_graph(app, None, document_id, "failed", Some(&msg));
        if let Some(a) = app {
            let _ = a.emit(LIBRARY_CHANGED, &());
        }
        return Ok(DocOutcome::Finished);
    }

    let title = state
        .with_sqlite(|c| sqlite::document_title_by_id(c, document_id))?
        .unwrap_or_else(|| document_id.to_string());
    let first_chunk_id = chunks
        .first()
        .map(|c| c.id.clone())
        .unwrap_or_default();
    if !first_chunk_id.is_empty() {
        let _ = state.sidecar.with_session(|s| {
            s.request(
                "link_document",
                json!({
                    "duckPath": sp(&duck_path),
                    "documentId": document_id,
                    "title": title,
                    "firstChunkId": first_chunk_id,
                }),
            )
        });
    }

    let note = if skipped > 0 {
        Some(format!("{skipped} of {total} passages were skipped"))
    } else {
        None
    };
    state.with_sqlite(|c| sqlite::mark_document_graph_ok(c, document_id, note.as_deref()))?;
    emit_graph(app, None, document_id, "ok", note.as_deref());
    if let Some(a) = app {
        let _ = a.emit(LIBRARY_CHANGED, &());
    }
    Ok(DocOutcome::Finished)
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
        match process_one_document(state, app, &doc_id) {
            Ok(DocOutcome::Finished) => {}
            Ok(DocOutcome::Pause) => {
                n += 1;
                break;
            }
            Err(e) => {
                let msg = e.to_string();
                if graph_extract::ollama_unreachable(&e) {
                    let _ = state.with_sqlite(|c| sqlite::mark_document_graph_pending(c, &doc_id));
                    emit_graph(app, None, &doc_id, "pending", Some(&msg));
                    if let Some(a) = app {
                        let _ = a.emit(LIBRARY_CHANGED, &());
                    }
                    n += 1;
                    break;
                }
                let _ = state.with_sqlite(|c| sqlite::mark_document_graph_failed(c, &doc_id, &msg));
                emit_graph(app, None, &doc_id, "failed", Some(&msg));
                if let Some(a) = app {
                    let _ = a.emit(LIBRARY_CHANGED, &());
                }
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
    use std::sync::{Arc, Mutex};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
                .map_err(|e| CognitoError::Database(e.to_string()))?;
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
                .map_err(|e| CognitoError::Database(e.to_string()))
            })
            .expect("read status");
        assert_eq!(status, "failed");
        assert!(err.as_ref().map(|e| !e.is_empty()).unwrap_or(false));
    }

    #[test]
    fn one_failed_chunk_does_not_fail_document() {
        let _env = ENV_LOCK.lock().expect("env lock");
        std::env::remove_var("OLLAMA_HOST");
        std::env::set_var("COGNITO_MOCK_EXTRACT", "1");
        let data = tempfile::tempdir().expect("data tempdir");
        let sidecar_cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar");
        let main_py = sidecar_cwd.join("main.py");
        let log_path = data.path().join("sidecar.log");
        let pool = SidecarPool::new(main_py, sidecar_cwd, log_path);
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        state.sidecar.ensure().expect("sidecar");
        let doc_id = "doc-skip-one";
        state
            .with_sqlite(|c| {
                sqlite::insert_document_row(
                    c,
                    doc_id,
                    "/tmp/paper.md",
                    "Paper Title",
                    "markdown",
                    "hash",
                    None,
                    "indexed",
                    None,
                )?;
                sqlite::insert_chunk_row(
                    c, "c-ok", doc_id, 0, None, None, "Ada works at Meridian.", 4, "h0", "body",
                )?;
                sqlite::insert_chunk_row(
                    c,
                    "c-refs",
                    doc_id,
                    1,
                    None,
                    None,
                    "1. Someone, Journal 1, 1999",
                    4,
                    "h1",
                    "references",
                )?;
                c.execute(
                    "UPDATE documents SET graph_status = 'pending' WHERE id = ?1",
                    [doc_id],
                )
                .map_err(|e| CognitoError::Database(e.to_string()))?;
                Ok(())
            })
            .expect("seed");

        let n = process_pending(&state, None);
        assert_eq!(n, 1);
        let (status, err): (String, Option<String>) = state
            .with_sqlite(|c| {
                c.query_row(
                    "SELECT graph_status, graph_error FROM documents WHERE id = ?1",
                    [doc_id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| CognitoError::Database(e.to_string()))
            })
            .expect("status");
        assert_eq!(status, "ok");
        assert!(err.is_none());
        std::env::remove_var("COGNITO_MOCK_EXTRACT");
    }

    #[test]
    fn unreachable_ollama_leaves_document_pending() {
        let _env = ENV_LOCK.lock().expect("env lock");
        let data = tempfile::tempdir().expect("data tempdir");
        // Point Ollama at a closed port so extract fails as unreachable.
        std::env::set_var("OLLAMA_HOST", "127.0.0.1:9");
        std::env::remove_var("COGNITO_MOCK_EXTRACT");
        let sidecar_cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar");
        let main_py = sidecar_cwd.join("main.py");
        let pool = SidecarPool::new(main_py, sidecar_cwd, data.path().join("sidecar.log"));
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        state.sidecar.ensure().expect("sidecar");
        let doc_id = "doc-ollama-down";
        state
            .with_sqlite(|c| {
                sqlite::insert_document_row(
                    c,
                    doc_id,
                    "/tmp/a.md",
                    "A",
                    "markdown",
                    "hash",
                    None,
                    "indexed",
                    None,
                )?;
                sqlite::insert_chunk_row(
                    c, "c1", doc_id, 0, None, None, "Graph attention networks.", 3, "h", "body",
                )?;
                c.execute(
                    "UPDATE documents SET graph_status = 'pending' WHERE id = ?1",
                    [doc_id],
                )
                .map_err(|e| CognitoError::Database(e.to_string()))?;
                Ok(())
            })
            .expect("seed");

        let n = process_pending(&state, None);
        assert_eq!(n, 1);
        let status: String = state
            .with_sqlite(|c| {
                c.query_row(
                    "SELECT graph_status FROM documents WHERE id = ?1",
                    [doc_id],
                    |r| r.get(0),
                )
                .map_err(|e| CognitoError::Database(e.to_string()))
            })
            .expect("status");
        assert_eq!(status, "pending");
        std::env::remove_var("OLLAMA_HOST");
    }
}
