//! Background work: entity extraction, then document cards, then link recompute.

use crate::app::state::SharedState;
use crate::db::sqlite;
use crate::errors::{CognitoError, CognitoResult};
use crate::events::{BACKGROUND_PROGRESS, LIBRARY_CHANGED};
use crate::models::BackgroundProgress;
use crate::services::document_cards;
use crate::services::ollama_stream;
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

pub(crate) fn emit_progress(
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
        BACKGROUND_PROGRESS,
        &BackgroundProgress {
            job_id: job_id.map(String::from),
            document_id: document_id.to_string(),
            phase: phase.to_string(),
            detail: detail.map(String::from),
        },
    );
}

fn background_model(state: &SharedState) -> String {
    state
        .with_sqlite(|c| sqlite::load_settings(c))
        .map(|s| s.background_model)
        .unwrap_or_else(|_| "qwen3.5:4b".into())
}

fn unload_background(state: &SharedState) {
    let model = background_model(state);
    ollama_stream::unload_model_blocking(&model);
}

enum WaitKind {
    Entities,
    Card,
}

fn wait_while_chat_in_flight(
    state: &SharedState,
    document_id: &str,
    kind: WaitKind,
) -> CognitoResult<bool> {
    let mut unloaded = false;
    while state.chat_in_flight.load(Ordering::SeqCst) > 0 {
        if !unloaded {
            unload_background(state);
            unloaded = true;
        }
        match kind {
            WaitKind::Card => {
                let still = state.with_sqlite(|c| sqlite::document_card_still_running(c, document_id))?;
                if !still {
                    return Ok(false);
                }
                let enabled = state
                    .with_sqlite(|c| sqlite::load_settings(c))?
                    .summarise_documents;
                if !enabled {
                    state.with_sqlite(|c| sqlite::mark_document_card_pending(c, document_id))?;
                    return Ok(false);
                }
            }
            WaitKind::Entities => {
                let still =
                    state.with_sqlite(|c| sqlite::document_entities_still_running(c, document_id))?;
                if !still {
                    return Ok(false);
                }
                let enabled = state
                    .with_sqlite(|c| sqlite::load_settings(c))?
                    .extract_entities;
                if !enabled {
                    state.with_sqlite(|c| sqlite::mark_document_entities_pending(c, document_id))?;
                    return Ok(false);
                }
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
    Ok(true)
}

enum DocOutcome {
    Finished,
    Pause,
}

fn process_one_entities(
    state: &SharedState,
    app: Option<&AppHandle>,
    document_id: &str,
) -> CognitoResult<DocOutcome> {
    let settings = state.with_sqlite(|c| sqlite::load_settings(c))?;
    if !settings.extract_entities {
        return Ok(DocOutcome::Finished);
    }
    state.with_sqlite(|c| {
        sqlite::mark_document_entities_running(c, document_id)?;
        sqlite::clear_document_entity_rows(c, document_id)?;
        Ok(())
    })?;
    emit_progress(app, None, document_id, "entities", None);
    if !wait_while_chat_in_flight(state, document_id, WaitKind::Entities)? {
        return Ok(DocOutcome::Finished);
    }
    let sqlite_path = state.sqlite_path();
    let app_data = sp(&state.data_dir);
    let labels: Vec<String> = settings
        .extra_entity_labels
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let res = state.sidecar.with_session(|s| {
        s.request(
            "extract_entities",
            json!({
                "sqlitePath": sp(&sqlite_path),
                "documentId": document_id,
                "app_data": app_data,
                "labels": labels,
            }),
        )
    });
    match res {
        Ok(v) => {
            let ok = v.get("ok").and_then(|x| x.as_bool()).unwrap_or(false);
            if !ok {
                let msg = v
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("entity extraction failed")
                    .to_string();
                state.with_sqlite(|c| sqlite::mark_document_entities_failed(c, document_id, &msg))?;
                emit_progress(app, None, document_id, "failed", Some(&msg));
            } else {
                state.with_sqlite(|c| sqlite::mark_document_entities_ok(c, document_id, None))?;
                emit_progress(app, None, document_id, "ok", None);
            }
            if let Some(a) = app {
                let _ = a.emit(LIBRARY_CHANGED, &());
            }
            Ok(DocOutcome::Finished)
        }
        Err(e) => {
            let msg = e.to_string();
            if ollama_stream::ollama_unreachable(&e) {
                state.with_sqlite(|c| sqlite::mark_document_entities_pending(c, document_id))?;
                emit_progress(app, None, document_id, "pending", Some(&msg));
                if let Some(a) = app {
                    let _ = a.emit(LIBRARY_CHANGED, &());
                }
                return Ok(DocOutcome::Pause);
            }
            state.with_sqlite(|c| sqlite::mark_document_entities_failed(c, document_id, &msg))?;
            emit_progress(app, None, document_id, "failed", Some(&msg));
            if let Some(a) = app {
                let _ = a.emit(LIBRARY_CHANGED, &());
            }
            Ok(DocOutcome::Finished)
        }
    }
}

fn process_one_card(
    state: &SharedState,
    app: Option<&AppHandle>,
    document_id: &str,
) -> CognitoResult<DocOutcome> {
    let settings = state.with_sqlite(|c| sqlite::load_settings(c))?;
    if !settings.summarise_documents {
        return Ok(DocOutcome::Finished);
    }
    state.with_sqlite(|c| sqlite::mark_document_card_running(c, document_id))?;
    emit_progress(app, None, document_id, "summarising", None);
    if !wait_while_chat_in_flight(state, document_id, WaitKind::Card)? {
        return Ok(DocOutcome::Finished);
    }
    match document_cards::build_and_store_card(state, document_id) {
        Ok(()) => {
            state.with_sqlite(|c| sqlite::mark_document_card_ok(c, document_id))?;
            emit_progress(app, None, document_id, "ok", None);
            if let Some(a) = app {
                let _ = a.emit(LIBRARY_CHANGED, &());
            }
            Ok(DocOutcome::Finished)
        }
        Err(e) => {
            let msg = e.to_string();
            if ollama_stream::ollama_unreachable(&e) {
                state.with_sqlite(|c| sqlite::mark_document_card_pending(c, document_id))?;
                emit_progress(app, None, document_id, "pending", Some(&msg));
                if let Some(a) = app {
                    let _ = a.emit(LIBRARY_CHANGED, &());
                }
                return Ok(DocOutcome::Pause);
            }
            state.with_sqlite(|c| sqlite::mark_document_card_failed(c, document_id, &msg))?;
            emit_progress(app, None, document_id, "failed", Some(&msg));
            if let Some(a) = app {
                let _ = a.emit(LIBRARY_CHANGED, &());
            }
            Ok(DocOutcome::Finished)
        }
    }
}

fn recompute_links(state: &SharedState) {
    let sqlite_path = state.sqlite_path();
    let _ = state.sidecar.with_session(|s| {
        s.request(
            "recompute_links",
            json!({ "sqlitePath": sp(&sqlite_path) }),
        )
    });
}

/// Processes pending entities, then cards, then recomputes links when idle.
pub fn process_pending(state: &SharedState, app: Option<&AppHandle>) -> usize {
    let mut n = 0usize;
    let mut pause = false;

    loop {
        let enabled = state
            .with_sqlite(|c| sqlite::load_settings(c))
            .map(|s| s.extract_entities)
            .unwrap_or(true);
        if !enabled {
            break;
        }
        let next = state
            .with_sqlite(|c| sqlite::next_pending_entities_document(c))
            .ok()
            .flatten();
        let Some(doc_id) = next else {
            break;
        };
        match process_one_entities(state, app, &doc_id) {
            Ok(DocOutcome::Finished) => {}
            Ok(DocOutcome::Pause) => {
                n += 1;
                pause = true;
                break;
            }
            Err(e) => {
                let msg = e.to_string();
                if ollama_stream::ollama_unreachable(&e) {
                    let _ =
                        state.with_sqlite(|c| sqlite::mark_document_entities_pending(c, &doc_id));
                    emit_progress(app, None, &doc_id, "pending", Some(&msg));
                    if let Some(a) = app {
                        let _ = a.emit(LIBRARY_CHANGED, &());
                    }
                    n += 1;
                    pause = true;
                    break;
                }
                let _ = state.with_sqlite(|c| sqlite::mark_document_entities_failed(c, &doc_id, &msg));
                emit_progress(app, None, &doc_id, "failed", Some(&msg));
                if let Some(a) = app {
                    let _ = a.emit(LIBRARY_CHANGED, &());
                }
            }
        }
        n += 1;
    }

    if !pause {
        loop {
            let summarise = state
                .with_sqlite(|c| sqlite::load_settings(c))
                .map(|s| s.summarise_documents)
                .unwrap_or(true);
            if !summarise {
                break;
            }
            let next = state
                .with_sqlite(|c| sqlite::next_pending_card_document(c))
                .ok()
                .flatten();
            let Some(doc_id) = next else {
                break;
            };
            match process_one_card(state, app, &doc_id) {
                Ok(DocOutcome::Finished) => {}
                Ok(DocOutcome::Pause) => {
                    n += 1;
                    pause = true;
                    break;
                }
                Err(e) => {
                    let msg = e.to_string();
                    let _ = state.with_sqlite(|c| sqlite::mark_document_card_failed(c, &doc_id, &msg));
                    emit_progress(app, None, &doc_id, "failed", Some(&msg));
                    if let Some(a) = app {
                        let _ = a.emit(LIBRARY_CHANGED, &());
                    }
                }
            }
            n += 1;
        }
    }

    if !pause {
        let busy = state
            .with_sqlite(|c| sqlite::pending_entities_or_cards_remain(c))
            .unwrap_or(true);
        if !busy {
            recompute_links(state);
        }
        unload_background(state);
    }
    n
}

pub fn spawn(state: SharedState, app: AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    {
        let mut g = state.background_tx.lock().expect("background_tx lock");
        *g = Some(tx);
    }
    let _ = state.with_sqlite(|c| {
        sqlite::reset_running_card_jobs(c)?;
        sqlite::reset_running_entities_jobs(c)?;
        Ok(())
    });
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
    use std::sync::atomic::Ordering as AtomicOrdering;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn cards_deleted_on_reindex() {
        let data = tempfile::tempdir().expect("data tempdir");
        let pool = SidecarPool::new(
            PathBuf::from("/nonexistent/cognito-sidecar/main.py"),
            PathBuf::from("/nonexistent/cognito-sidecar"),
            data.path().join("sidecar.log"),
        );
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        let doc_id = "doc-reindex-card";
        state
            .with_sqlite(|c| {
                sqlite::insert_document_row(
                    c,
                    doc_id,
                    "/tmp/x.md",
                    "X",
                    "markdown",
                    "hash1",
                    None,
                    "indexed",
                    None,
                    None,
                )?;
                sqlite::upsert_document_card(
                    c,
                    doc_id,
                    "note",
                    "X",
                    None,
                    "[]",
                    "[]",
                    "[]",
                    "[]",
                    "[]",
                    "[]",
                    "[]",
                    "qwen3.5:4b",
                )?;
                let n: i64 = c
                    .query_row(
                        "SELECT COUNT(*) FROM document_cards WHERE document_id = ?1",
                        [doc_id],
                        |r| r.get(0),
                    )
                    .map_err(|e| CognitoError::Database(e.to_string()))?;
                assert_eq!(n, 1);
                sqlite::delete_document_by_id(c, doc_id)?;
                let n2: i64 = c
                    .query_row(
                        "SELECT COUNT(*) FROM document_cards WHERE document_id = ?1",
                        [doc_id],
                        |r| r.get(0),
                    )
                    .map_err(|e| CognitoError::Database(e.to_string()))?;
                assert_eq!(n2, 0);
                Ok(())
            })
            .expect("cascade");
    }

    #[test]
    fn process_pending_entities_before_cards() {
        let _env = ENV_LOCK.lock().expect("env lock");
        std::env::set_var("COGNITO_MOCK_MODELS", "1");
        let data = tempfile::tempdir().expect("data tempdir");
        let sidecar_cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../sidecar");
        let main_py = sidecar_cwd.join("main.py");
        let pool = SidecarPool::new(main_py, sidecar_cwd, data.path().join("sidecar.log"));
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        state.sidecar.ensure().expect("sidecar");
        state
            .with_sqlite(|c| {
                sqlite::upsert_setting(c, "extract_entities", "true")?;
                sqlite::upsert_setting(c, "summarise_documents", "false")?;
                sqlite::insert_document_row(
                    c,
                    "doc-ent",
                    "/tmp/e.md",
                    "E",
                    "markdown",
                    "hash",
                    None,
                    "indexed",
                    None,
                    None,
                )?;
                sqlite::insert_chunk_row(
                    c,
                    "c-ent",
                    "doc-ent",
                    0,
                    None,
                    None,
                    "Ada Chen works at Meridian Labs in Cambridge.",
                    8,
                    "h",
                    "body",
                    "[]",
                    None,
                )?;
                c.execute(
                    "UPDATE documents SET entities_status = 'pending', card_status = 'pending' WHERE id = 'doc-ent'",
                    [],
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
                    "SELECT entities_status, entities_error FROM documents WHERE id = 'doc-ent'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .map_err(|e| CognitoError::Database(e.to_string()))
            })
            .expect("status");
        assert_eq!(status, "ok", "entities_error={err:?}");
        std::env::remove_var("COGNITO_MOCK_MODELS");
    }

    #[test]
    fn background_model_unloaded_when_chat_starts() {
        let _env = ENV_LOCK.lock().expect("env lock");
        std::env::set_var("COGNITO_MOCK_LLM", "1");
        ollama_stream::UNLOAD_CALLS.store(0, AtomicOrdering::SeqCst);
        let data = tempfile::tempdir().expect("data tempdir");
        let pool = SidecarPool::new(
            PathBuf::from("/nonexistent/cognito-sidecar/main.py"),
            PathBuf::from("/nonexistent/cognito-sidecar"),
            data.path().join("sidecar.log"),
        );
        let state: SharedState = Arc::new(AppState::new(data.path().to_path_buf(), pool));
        state
            .with_sqlite(|c| {
                sqlite::upsert_setting(c, "summarise_documents", "true")?;
                sqlite::upsert_setting(c, "extract_entities", "false")?;
                sqlite::insert_document_row(
                    c,
                    "doc-unload",
                    "/tmp/u.md",
                    "U",
                    "markdown",
                    "hash",
                    None,
                    "indexed",
                    None,
                    None,
                )?;
                c.execute(
                    "UPDATE documents SET card_status = 'running' WHERE id = 'doc-unload'",
                    [],
                )
                .map_err(|e| CognitoError::Database(e.to_string()))?;
                Ok(())
            })
            .expect("seed");
        state.chat_in_flight.fetch_add(1, AtomicOrdering::SeqCst);
        let before = ollama_stream::UNLOAD_CALLS.load(AtomicOrdering::SeqCst);
        let handle = {
            let state = state.clone();
            thread::spawn(move || wait_while_chat_in_flight(&state, "doc-unload", WaitKind::Card))
        };
        thread::sleep(Duration::from_millis(600));
        let mid = ollama_stream::UNLOAD_CALLS.load(AtomicOrdering::SeqCst);
        assert!(mid > before, "expected unload while chat in flight");
        state.chat_in_flight.fetch_sub(1, AtomicOrdering::SeqCst);
        let ok = handle.join().expect("join").expect("wait");
        assert!(ok);
        std::env::remove_var("COGNITO_MOCK_LLM");
    }
}
